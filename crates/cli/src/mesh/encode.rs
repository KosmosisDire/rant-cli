//! Messages built from command line values, field by field through the paths the schema
//! resolves, on top of the schema's default message.

use rant::{Field, FieldType, MapWriter, MessageBuilder, Schema};
use serde_json::Value;

use super::text::Assignments;
use super::values::is_number;
use crate::failure::{Result, failed};

/// The message of schema the assignments make, in order. An unknown field or a value its
/// field cannot hold fails. With no schema the one value goes as text.
pub fn encode(values: &Assignments, schema: Option<&Schema>) -> Result<Vec<u8>> {
    let Some(schema) = schema else {
        return match values.as_slice() {
            [(path, v)] if path.is_empty() => Ok(text_of(v).into_bytes()),
            _ => Err(failed("the entity has no type, so it takes one value, sent as text")),
        };
    };
    let mut e = Encoder { schema, fields: schema.fields(), b: MessageBuilder::new(schema) };
    // a struct takes an object key by key, a bare root the value itself
    let is_struct = e.fields.first().is_some_and(|f| !f.name.is_empty());
    for (path, v) in values {
        match v {
            Value::Object(o) if path.is_empty() && is_struct => o.iter().try_for_each(|(k, v)| e.set(k, v))?,
            v => e.set(path, v)?,
        }
    }
    Ok(e.b.bytes().to_vec())
}

fn text_of(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        v => v.to_string(),
    }
}

fn shown(path: &str) -> String {
    if path.is_empty() { "the value".to_string() } else { format!("field `{path}`") }
}

fn join(path: &str, key: &str) -> String {
    if path.is_empty() { key.to_string() } else { format!("{path}.{key}") }
}

fn wrong(path: &str, v: &Value, want: &str) -> crate::failure::Failure {
    failed(format!("{} takes {want}, not {}", shown(path), text_of(v)))
}

struct Encoder<'a> {
    schema: &'a Schema,
    fields: Vec<Field>,
    b: MessageBuilder,
}

impl Encoder<'_> {
    fn set(&mut self, path: &str, v: &Value) -> Result<()> {
        self.grow_arrays(path)?;
        let Some(at) = self.schema.field_index(path) else { return Err(self.unknown(path)) };
        let f = self.fields[at as usize].clone();
        let refused = |_| wrong(path, v, "a value that fits it");
        match f.kind {
            FieldType::Struct => {
                let Value::Object(o) = v else { return Err(wrong(path, v, "an object of its fields")) };
                o.iter().try_for_each(|(k, e)| self.set(&join(path, k), e))?;
            }
            FieldType::Array | FieldType::VArray => {
                let Value::Array(items) = v else { return Err(wrong(path, v, "a list")) };
                let e = self.fields[at as usize + 1].clone();
                if f.kind == FieldType::Array && items.len() > f.count as usize {
                    return Err(failed(format!("{} holds at most {} items", shown(path), f.count)));
                }
                if is_number(e.kind) && e.size != 0 {
                    let mut raw = Vec::with_capacity(items.len() * e.size as usize);
                    for (i, item) in items.iter().enumerate() {
                        let bytes = scalar_bytes(item, e.kind).ok_or_else(|| wrong(&format!("{path}[{i}]"), item, kind_word(e.kind)))?;
                        raw.extend_from_slice(&bytes[..e.size as usize]);
                    }
                    self.b.set_array(path, &raw).map_err(refused)?;
                } else {
                    // any other element by its own path: an object, text, an option or another list
                    if f.kind == FieldType::VArray {
                        self.b.set_array_count(path, items.len() as u32).map_err(refused)?;
                    }
                    for (i, item) in items.iter().enumerate() {
                        self.set(&format!("{path}[{i}]"), item)?;
                    }
                }
            }
            FieldType::String | FieldType::VString => {
                self.b.set_string(path, &text_of(v)).map_err(refused)?;
            }
            FieldType::Enum => {
                let variants = self.schema.enum_variants(at);
                let name = match v {
                    Value::Number(n) => n.as_i64().and_then(|n| variants.iter().find(|o| o.value == n)).map(|o| o.name.clone()),
                    v => Some(text_of(v)),
                };
                let options = || variants.iter().map(|o| o.name.as_str()).collect::<Vec<_>>().join(", ");
                let set = name.is_some_and(|n| self.b.set_enum(path, &n).is_ok());
                if !set {
                    return Err(wrong(path, v, &format!("one of {}", options())));
                }
            }
            FieldType::Map => {
                let Value::Object(_) = v else { return Err(wrong(path, v, "an object")) };
                self.b.set_map(path, &map_body(v)?).map_err(refused)?;
            }
            FieldType::Bool => {
                let Value::Bool(b) = v else { return Err(wrong(path, v, "true or false")) };
                self.b.set_bool(path, *b).map_err(refused)?;
            }
            FieldType::F32 | FieldType::F64 => {
                let Some(n) = v.as_f64() else { return Err(wrong(path, v, "a number")) };
                self.b.set_f64(path, n).map_err(refused)?;
            }
            FieldType::U8 | FieldType::U16 | FieldType::U32 | FieldType::U64 => {
                let Some(n) = v.as_u64() else {
                    let want = if v.is_i64() { "a whole number from 0 up" } else { "a whole number" };
                    return Err(wrong(path, v, want));
                };
                self.b.set_uint(path, n).map_err(refused)?;
            }
            _ => {
                let Some(n) = v.as_i64() else { return Err(wrong(path, v, "a whole number")) };
                self.b.set_int(path, n).map_err(refused)?;
            }
        }
        Ok(())
    }

    /// The field names one level under the path's parent, for the error about a name it does
    /// not have.
    fn unknown(&self, path: &str) -> crate::failure::Failure {
        let parent = path.rfind('.').map(|dot| &path[..dot]);
        let (from, depth) = match parent.and_then(|p| self.schema.field_index(p)) {
            Some(p) => (p as usize + 1, self.fields[p as usize].depth + 1),
            None => (0, 0),
        };
        let known: Vec<&str> =
            self.fields[from.min(self.fields.len())..].iter().take_while(|f| f.depth >= depth).filter(|f| f.depth == depth && !f.name.is_empty()).map(|f| f.name.as_str()).collect();
        let list = if known.is_empty() { String::new() } else { format!(", the fields are {}", known.join(", ")) };
        failed(format!("there is no field `{path}`{list}"))
    }

    /// pts[2].x on a variable array of two grows it to three first.
    fn grow_arrays(&mut self, path: &str) -> Result<()> {
        for (open, _) in path.match_indices('[') {
            let prefix = &path[..open];
            let Some(close) = path[open..].find(']') else { continue };
            let Ok(index) = path[open + 1..open + close].parse::<u32>() else { continue };
            let Some(at) = self.schema.field_index(prefix) else { continue };
            let now = self.schema.view(self.b.bytes()).get_array_count(prefix);
            if self.fields[at as usize].kind == FieldType::VArray && now <= index {
                self.b.set_array_count(prefix, index + 1).map_err(|_| failed(format!("{} cannot hold {} items", shown(prefix), index + 1)))?;
            }
        }
        Ok(())
    }
}

fn kind_word(kind: FieldType) -> &'static str {
    match kind {
        FieldType::Bool => "true or false",
        FieldType::F32 | FieldType::F64 => "a number",
        _ => "a whole number",
    }
}

/// One scalar of kind, little endian as on the wire, eight bytes of which the kind takes its size.
fn scalar_bytes(v: &Value, kind: FieldType) -> Option<[u8; 8]> {
    let bits = match kind {
        FieldType::F32 => (v.as_f64()? as f32).to_bits() as u64,
        FieldType::F64 => v.as_f64()?.to_bits(),
        FieldType::Bool => v.as_bool()? as u64,
        _ if v.is_u64() => v.as_u64()?,
        _ => v.as_i64()? as u64,
    };
    Some(bits.to_le_bytes())
}

/// A `map` field's body: a self describing tree, so any JSON object fits.
fn map_body(v: &Value) -> Result<Vec<u8>> {
    let mut cap = 256;
    while cap <= 64 << 20 {
        let mut w = MapWriter::with_capacity(cap);
        if let Value::Object(o) = v {
            o.iter().for_each(|(k, e)| put(&mut w, k, e));
        }
        if let Ok(body) = w.finish() {
            return Ok(body.to_vec());
        }
        cap *= 4;
    }
    Err(failed("the value is too big"))
}

fn put(w: &mut MapWriter, key: &str, v: &Value) {
    match v {
        Value::Bool(b) => _ = w.put_bool(key, *b),
        Value::Number(n) if n.is_u64() => _ = w.put_uint(key, n.as_u64().unwrap_or_default()),
        Value::Number(n) if n.is_i64() => _ = w.put_int(key, n.as_i64().unwrap_or_default()),
        Value::Number(n) => _ = w.put_f64(key, n.as_f64().unwrap_or_default()),
        Value::String(s) => _ = w.put_string(key, s),
        Value::Object(o) => {
            w.open_map(key);
            o.iter().for_each(|(k, e)| put(w, k, e));
            w.close();
        }
        Value::Array(items) => {
            w.open_array(key);
            items.iter().for_each(|e| put(w, "", e));
            w.close();
        }
        Value::Null => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mesh::{test_node, to_json};
    use serde_json::json;

    fn schema(text: &str) -> Schema {
        test_node().schema(text).unwrap()
    }

    fn round_trip(s: &Schema, a: Assignments) -> Value {
        to_json(&encode(&a, Some(s)).unwrap(), Some(s))
    }

    #[test]
    fn every_kind_round_trips() {
        let s = schema(
            "Pt { x: f32, y: f32 } Mode = enum<u8> { Idle, Run } Sample { seq: u32, delta: i16, value: f64, name: string, label: string<8>, \
             tags: string<16>[], nums: i16[], fixed: f64[3], pts: Pt[], mode: Mode, on: bool, extra: map, grid: u8[][] }",
        );
        let v = json!({"seq": 7, "delta": -2, "value": 1.5, "name": "long name", "label": "short", "tags": ["a", "b"], "nums": [1, -2, 3],
            "fixed": [1.0, 2.0, 3.0], "pts": [{"x": 1.0, "y": 2.0}, {"x": 3.0, "y": 4.0}], "mode": "Run", "on": true,
            "extra": {"k": 1, "s": "t", "nested": {"b": false}, "list": [1, -2]}, "grid": [[1, 2], [3]]});
        assert_eq!(round_trip(&s, vec![(String::new(), v.clone())]), v);
    }

    #[test]
    fn assignments_reach_nested_and_indexed_fields() {
        let s = schema("Pt { x: f32, y: f32 } Trail { name: string, pts: Pt[], at: Pt }");
        let a = vec![("pts[1].y".into(), json!(5)), ("at.x".into(), json!(2)), ("name".into(), json!("p"))];
        assert_eq!(round_trip(&s, a), json!({"name": "p", "pts": [{"x": 0.0, "y": 0.0}, {"x": 0.0, "y": 5.0}], "at": {"x": 2.0, "y": 0.0}}));
    }

    #[test]
    fn a_bare_root_takes_the_value_itself() {
        assert_eq!(round_trip(&schema("f64"), vec![(String::new(), json!(2.5))]), json!(2.5));
        assert_eq!(round_trip(&schema("string"), vec![(String::new(), json!("hi there"))]), json!("hi there"));
        assert_eq!(round_trip(&schema("u32[]"), vec![(String::new(), json!([1, 2]))]), json!([1, 2]));
        assert_eq!(round_trip(&schema("Pose"), vec![("position.x".into(), json!(1))])["position"]["x"], json!(1.0));
    }

    #[test]
    fn an_enum_takes_its_name_or_number() {
        let s = schema("Gear = enum<u8> { Idle, Run = 4 } Shift { mode: Gear }");
        assert_eq!(round_trip(&s, vec![("mode".into(), json!(4))]), json!({"mode": "Run"}));
        assert_eq!(round_trip(&s, vec![("mode".into(), json!("idle"))]), json!({"mode": "Idle"}));
    }

    #[test]
    fn mistakes_say_what_was_wanted() {
        let s = schema("Mode = enum<u8> { Idle, Run } Shape { side: u8, mode: Mode, label: string<4>, at: Pose, fixed: f64[2] }");
        let message = |path: &str, v: Value| encode(&vec![(path.to_string(), v)], Some(&s)).unwrap_err().messages()[0].clone();
        assert_eq!(message("colour", json!(1)), "there is no field `colour`, the fields are side, mode, label, at, fixed");
        assert_eq!(message("at.nope", json!(1)), "there is no field `at.nope`, the fields are position, orientation");
        assert_eq!(message("side", json!(-1)), "field `side` takes a whole number from 0 up, not -1");
        assert_eq!(message("side", json!(1.5)), "field `side` takes a whole number, not 1.5");
        assert_eq!(message("mode", json!("Fly")), "field `mode` takes one of Idle, Run, not Fly");
        assert_eq!(message("label", json!("too long")), "field `label` takes a value that fits it, not too long");
        assert_eq!(message("fixed", json!([1, 2, 3])), "field `fixed` holds at most 2 items");
        assert_eq!(message("fixed", json!([1, "x"])), "field `fixed[1]` takes a number, not x");
    }

    #[test]
    fn no_schema_sends_text_and_shows_hex() {
        assert_eq!(encode(&vec![(String::new(), json!("raw words"))], None).unwrap(), b"raw words");
        assert!(encode(&vec![("x".into(), json!(1))], None).is_err());
        assert_eq!(to_json(&[0x0a, 0xff], None), json!("0x0aff"));
    }
}
