//! Messages as JSON, walked through their schema at run time, so rant reads any type without
//! compiling it in.

use rant::{Field, FieldType, FieldView, MapValue, Schema};
use serde_json::{Map, Value};

/// A message as JSON: an object for a struct, the bare value for a bare root. Bytes with no
/// schema become a "0x..." hex string.
pub fn to_json(data: &[u8], schema: Option<&Schema>) -> Value {
    let Some(schema) = schema else { return Value::String(hex(data)) };
    let fields = schema.fields();
    let Some(first) = fields.first() else { return Value::Null };
    let walk = Walk { fields: &fields, view: schema.view(data) };
    let mut i = 0;
    if first.name.is_empty() { walk.field(&mut i, "") } else { walk.members(&mut i, 0, "") }
}

fn hex(data: &[u8]) -> String {
    data.iter().fold(String::from("0x"), |s, b| s + &format!("{b:02x}"))
}

/// Walks the flat depth first field table, reading each field by its path, `pts[2].x`.
struct Walk<'a> {
    fields: &'a [Field],
    view: FieldView<'a>,
}

impl Walk<'_> {
    /// The struct members from i on at depth, stopping at the first shallower field.
    fn members(&self, i: &mut usize, depth: u16, path: &str) -> Value {
        let mut obj = Map::new();
        while let Some(f) = self.fields.get(*i).filter(|f| f.depth >= depth) {
            let child = if path.is_empty() { f.name.clone() } else { format!("{path}.{}", f.name) };
            obj.insert(f.name.clone(), self.field(i, &child));
        }
        Value::Object(obj)
    }

    /// The value of field i, leaving i past it and everything nested under it. An array's
    /// element is its next row: numbers come packed, the rest one by one by index.
    fn field(&self, i: &mut usize, path: &str) -> Value {
        let f = &self.fields[*i];
        *i += 1;
        match f.kind {
            FieldType::Struct => return self.members(i, f.depth + 1, path),
            FieldType::Array | FieldType::VArray => {}
            _ => return self.leaf(f, path),
        }
        let elem = *i;
        let end = elem + self.fields[elem..].iter().take_while(|e| e.depth > f.depth).count();
        let out = match self.fields.get(elem) {
            Some(e) if packed(e) => self.view.get_array(path).chunks_exact(e.size as usize).map(|b| scalar(b, e.kind)).collect(),
            Some(_) => (0..self.view.get_array_count(path)).map(|k| self.field(&mut elem.clone(), &format!("{path}[{k}]"))).collect(),
            None => Vec::new(),
        };
        *i = end;
        Value::Array(out)
    }

    fn leaf(&self, f: &Field, path: &str) -> Value {
        let v = &self.view;
        match f.kind {
            FieldType::String | FieldType::VString => Value::String(v.get_string(path).into_owned()),
            FieldType::Map => map(v.get_map(path).iter()),
            FieldType::Enum => v.get_enum_name(path).map_or_else(|| Value::from(v.get_int(path)), Value::from),
            FieldType::Bool => Value::Bool(v.get_bool(path)),
            FieldType::F32 | FieldType::F64 => float(v.get_f64(path)),
            FieldType::I8 | FieldType::I16 | FieldType::I32 | FieldType::I64 => Value::from(v.get_int(path)),
            FieldType::U8 | FieldType::U16 | FieldType::U32 | FieldType::U64 => Value::from(v.get_uint(path)),
            _ => Value::Null,
        }
    }
}

/// An element read straight from the array's packed bytes: a fixed number.
fn packed(e: &Field) -> bool {
    e.size != 0 && is_number(e.kind)
}

pub fn is_number(k: FieldType) -> bool {
    use FieldType::*;
    matches!(k, U8 | U16 | U32 | U64 | I8 | I16 | I32 | I64 | F32 | F64 | Bool)
}

/// One fixed scalar, little endian as on the wire.
fn scalar(b: &[u8], kind: FieldType) -> Value {
    let u = b.iter().rev().fold(0u64, |u, &x| (u << 8) | x as u64);
    match kind {
        FieldType::I8 => Value::from(u as u8 as i8),
        FieldType::I16 => Value::from(u as u16 as i16),
        FieldType::I32 => Value::from(u as u32 as i32),
        FieldType::I64 => Value::from(u as i64),
        FieldType::F32 => float(f32::from_bits(u as u32) as f64),
        FieldType::F64 => float(f64::from_bits(u)),
        FieldType::Bool => Value::Bool(u != 0),
        _ => Value::from(u),
    }
}

/// A float, null for one JSON cannot hold such as NaN.
fn float(f: f64) -> Value {
    serde_json::Number::from_f64(f).map_or(Value::Null, Value::Number)
}

/// A map body in its own order, each value carrying its kind.
fn map<'a>(entries: impl Iterator<Item = (&'a str, MapValue<'a>)>) -> Value {
    Value::Object(entries.map(|(k, v)| (k.to_string(), tagged(&v))).collect())
}

fn tagged(v: &MapValue) -> Value {
    use FieldType::*;
    match v.kind() {
        U8 | U16 | U32 | U64 => Value::from(v.as_uint()),
        I8 | I16 | I32 | I64 => Value::from(v.as_int()),
        F32 | F64 => float(v.as_f64()),
        Bool => Value::Bool(v.as_bool()),
        String | VString => Value::String(v.as_str().into_owned()),
        Map => map(v.as_map().iter()),
        Array | VArray => Value::Array((0..v.array_len()).filter_map(|i| v.array_at(i)).map(|e| tagged(&e)).collect()),
        _ => Value::Null,
    }
}
