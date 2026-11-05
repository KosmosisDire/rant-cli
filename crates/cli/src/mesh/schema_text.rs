//! Types as a person reads them: one field per line, a user type defined once below the type
//! that uses it, the standard types by name.

use std::collections::{BTreeMap, BTreeSet};
use std::ffi::{CStr, c_char};

use rant::{Entity, Field, FieldType, Schema};

use crate::output::{Style, Text};

// SAFETY: Rant's C library, which the rant crate links, defines it as declared here.
unsafe extern "C" {
    fn rant_std_name(t: u32) -> *const c_char;
}

/// The names of every standard type.
pub fn standard_types() -> Vec<String> {
    // SAFETY: the C gives a static NUL terminated name for each type and null past the last.
    (1..).map_while(|t| unsafe { rant_std_name(t).as_ref().map(|p| CStr::from_ptr(p).to_string_lossy().into_owned()) }).collect()
}

/// A type as text, painted as t paints. A standard type is its name alone unless whole spells
/// it out too. "untyped" without a schema.
pub fn schema_text(s: Option<&Schema>, t: &Text, whole: bool) -> String {
    let Some(s) = s else { return t.paint(Style::Faint, "untyped") };
    let mut p = Printer { fields: s.fields(), schema: s, t, standard: standard_types().into_iter().collect(), defs: Vec::new(), defined: BTreeSet::new() };
    let name = s.name();
    let root = match p.fields.first() {
        None => vec![t.paint(Style::Faint, "{}")],
        Some(_) if !name.is_empty() && p.standard.contains(&name) && !whole => vec![t.paint(Style::Accent, &name)],
        // a bare root, such as f64 or u8[]
        Some(f) if f.name.is_empty() => p.type_lines(0),
        Some(_) => {
            let head = if name.is_empty() { String::new() } else { format!("{} ", t.paint(Style::Accent, &name)) };
            p.defined.insert(name);
            let mut lines = vec![format!("{head}{}", t.paint(Style::Faint, "{"))];
            lines.extend(p.members(&mut 0, 0));
            lines.push(t.paint(Style::Faint, "}"));
            lines
        }
    };
    std::iter::once(root.join("\n")).chain(p.defs).collect::<Vec<_>>().join("\n\n")
}

/// A named type the mesh uses, with the entities that use it.
pub struct TypeUse {
    pub schema: Schema,
    pub entities: Vec<String>,
}

/// Every named type of the entities' messages, requests, responses and progress, by name.
pub fn types_in_use(entities: &[Entity]) -> BTreeMap<String, TypeUse> {
    let mut out: BTreeMap<String, TypeUse> = BTreeMap::new();
    for e in entities {
        for s in [&e.schema, &e.response_schema, &e.progress_schema].into_iter().flatten() {
            let name = s.name();
            if name.is_empty() {
                continue;
            }
            let u = out.entry(name).or_insert_with(|| TypeUse { schema: s.clone(), entities: Vec::new() });
            if !u.entities.contains(&e.name) {
                u.entities.push(e.name.clone());
            }
        }
    }
    out
}

fn scalar_name(kind: FieldType) -> &'static str {
    use FieldType::*;
    match kind {
        U8 => "u8",
        U16 => "u16",
        U32 => "u32",
        U64 => "u64",
        I8 => "i8",
        I16 => "i16",
        I32 => "i32",
        I64 => "i64",
        F32 => "f32",
        F64 => "f64",
        Bool => "bool",
        _ => "?",
    }
}

struct Printer<'a> {
    fields: Vec<Field>,
    schema: &'a Schema,
    t: &'a Text<'a>,
    standard: BTreeSet<String>,
    /// The definitions of the user types used, each before the types it uses.
    defs: Vec<String>,
    defined: BTreeSet<String>,
}

impl Printer<'_> {
    fn paint(&self, s: Style, text: &str) -> String {
        self.t.paint(s, text)
    }

    /// The direct members of a struct, from j on at depth, names aligned. Leaves j past
    /// everything nested under them.
    fn members(&mut self, j: &mut usize, depth: u16) -> Vec<String> {
        let start = *j;
        *j += self.fields[start..].iter().take_while(|f| f.depth >= depth).count();
        let direct: Vec<usize> = (start..*j).filter(|&i| self.fields[i].depth == depth).collect();
        let widest = direct.iter().map(|&i| self.fields[i].name.chars().count()).max().unwrap_or(0);
        let mut lines = Vec::new();
        for i in direct {
            let name = self.fields[i].name.clone();
            let mut ty = self.type_lines(i).into_iter();
            let pad = " ".repeat(widest - name.chars().count() + 1);
            lines.push(format!("  {name}{}{pad}{}", self.paint(Style::Faint, ":"), ty.next().unwrap_or_default()));
            lines.extend(ty);
        }
        lines
    }

    /// A named type other than a struct: a standard one stays a name, a user one is defined
    /// once below as Name = type.
    fn named(&mut self, name: &str, ty: &[String]) -> String {
        if !self.standard.contains(name) && self.defined.insert(name.to_string()) {
            let def = format!("{} {} {}", self.paint(Style::Accent, name), self.paint(Style::Faint, "="), ty.join("\n"));
            self.defs.push(def);
        }
        self.paint(Style::Accent, name)
    }

    fn enum_text(&self, i: usize) -> String {
        let f = &self.fields[i];
        let mut s = format!("{}{}{}{}", self.paint(Style::Amber, "enum"), self.paint(Style::Faint, "<"), self.paint(Style::Amber, scalar_name(f.backing)), self.paint(Style::Faint, "> {"));
        for (k, o) in self.schema.enum_variants(i as u16).iter().enumerate() {
            s += &if k == 0 { " ".to_string() } else { format!("{} ", self.paint(Style::Faint, ",")) };
            s += &self.paint(Style::GreenHi, &o.name);
            if o.value != k as i64 {
                s += &format!(" {} {}", self.paint(Style::Faint, "="), o.value);
            }
        }
        format!("{s} {}", self.paint(Style::Faint, "}"))
    }

    /// The struct a field or an array element opens, by its name and defined once below.
    fn struct_type(&mut self, i: usize) -> Vec<String> {
        let name = self.fields[i].type_name.clone().unwrap_or_default();
        if self.standard.contains(&name) || !self.defined.insert(name.clone()) {
            return vec![self.paint(Style::Accent, &name)];
        }
        // the definition goes before the types it uses
        let slot = self.defs.len();
        self.defs.push(String::new());
        let mut lines = vec![format!("{} {}", self.paint(Style::Accent, &name), self.paint(Style::Faint, "{"))];
        let depth = self.fields[i].depth + 1;
        lines.extend(self.members(&mut (i + 1), depth));
        lines.push(self.paint(Style::Faint, "}"));
        self.defs[slot] = lines.join("\n");
        vec![self.paint(Style::Accent, &name)]
    }

    fn type_lines(&mut self, i: usize) -> Vec<String> {
        let f = self.fields[i].clone();
        let alias = |p: &mut Self, base: String| match &f.type_name {
            Some(n) => p.named(n, &[base]),
            None => base,
        };
        let faint = |p: &Self, s: &str| p.paint(Style::Faint, s);
        match f.kind {
            FieldType::Struct => self.struct_type(i),
            FieldType::String => {
                let base = format!("{}{}{}{}", self.paint(Style::Amber, "string"), faint(self, "<"), f.str_cap, faint(self, ">"));
                vec![alias(self, base)]
            }
            FieldType::VString => vec![alias(self, self.paint(Style::Amber, "string"))],
            FieldType::Map => vec![alias(self, self.paint(Style::Amber, "map"))],
            FieldType::Enum => vec![alias(self, self.enum_text(i))],
            FieldType::Array | FieldType::VArray => {
                let count = if f.kind == FieldType::Array { f.count.to_string() } else { String::new() };
                let suffix = format!("{}{count}{}", faint(self, "["), faint(self, "]"));
                // the element is the next row, any type, a list of lists too
                let mut elem = self.type_lines(i + 1);
                if let Some(last) = elem.last_mut() {
                    last.push_str(&suffix);
                }
                match &f.type_name {
                    Some(n) => vec![self.named(n, &elem)],
                    None => elem,
                }
            }
            kind => vec![alias(self, self.paint(Style::Amber, scalar_name(kind)))],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mesh::test_node;

    fn text(schema: &str) -> String {
        schema_text(Some(&test_node().schema(schema).unwrap()), &Text::plain(), false)
    }

    #[test]
    fn a_standard_type_is_its_name() {
        assert_eq!(text("Pose"), "Pose");
        assert_eq!(text("f64"), "f64");
        assert_eq!(text("u8[]"), "u8[]");
        let pose = schema_text(Some(&test_node().schema("Pose").unwrap()), &Text::plain(), true);
        assert!(pose.starts_with("Pose {\n  position:    Double3\n"), "{pose}");
        assert!(standard_types().contains(&"Pose".to_string()) && standard_types().len() > 50);
    }

    #[test]
    fn a_user_type_prints_one_field_per_line_then_the_types_it_uses() {
        let t = text("Pt { x: f32, y: f32 } Stage = enum<u8> { Idle, Run = 5 } Route { name: string, pts: Pt[], at: Pose, mode: Stage, raw: u8[4], tags: string<8>[] }");
        let want = "Route {\n  name: string\n  pts:  Pt[]\n  at:   Pose\n  mode: Stage\n  raw:  u8[4]\n  tags: string<8>[]\n}\n\nPt {\n  x: f32\n  y: f32\n}\n\nStage = enum<u8> { Idle, Run = 5 }";
        assert_eq!(t, want);
    }

    #[test]
    fn no_schema_is_untyped() {
        assert_eq!(schema_text(None, &Text::plain(), false), "untyped");
    }
}
