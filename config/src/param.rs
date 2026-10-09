//! Params, as group files declare them: a name, a type, options, a default
//! and a description. Values from a file or the command line are checked against them.

use std::collections::BTreeMap;

use hcl::Value;
use hcl_edit::structure::Attribute;

use crate::diag::Diag;
use crate::eval::{self, Scope};
use crate::source::{one_label, Fields, Loc, Source};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParamType {
    String,
    Int,
    Float,
    Bool,
}

impl ParamType {
    pub fn name(self) -> &'static str {
        match self {
            ParamType::String => "string",
            ParamType::Int => "int",
            ParamType::Float => "float",
            ParamType::Bool => "bool",
        }
    }
}

#[derive(Debug, Clone)]
pub struct Param {
    pub name: String,
    pub ty: ParamType,
    pub options: Vec<Value>,
    pub default: Option<Value>,
    pub description: Option<String>,
    /// For an exposed param, the include it reaches: the group and the include's index.
    pub(crate) from: Option<(String, usize)>,
    pub(crate) at: Loc,
}

fn param_type(src: &Source, attr: &Attribute) -> Result<ParamType, Diag> {
    match eval::identifier(src, attr)? {
        "string" => Ok(ParamType::String),
        "int" => Ok(ParamType::Int),
        "float" => Ok(ParamType::Float),
        "bool" => Ok(ParamType::Bool),
        other => Err(src.diag_at(&attr.value, format!("unknown type `{other}`, expected string, int, float or bool"))),
    }
}

/// A value checked against a param's type, an int widened for a float, and against its
/// options. The error says what was wrong without a place, the caller adds one.
pub fn checked(p: &Param, v: Value) -> Result<Value, String> {
    let v = match (p.ty, v) {
        (ParamType::String, v @ Value::String(_)) => v,
        (ParamType::Bool, v @ Value::Bool(_)) => v,
        (ParamType::Int, Value::Number(n)) if n.as_i64().is_some() => Value::Number(n),
        (ParamType::Float, Value::Number(n)) => Value::from(n.as_f64().unwrap_or(0.0)),
        (ty, v) => return Err(format!("param `{}` is {}, not {}", p.name, ty.name(), eval::type_name(&v))),
    };
    if !p.options.is_empty() && !p.options.contains(&v) {
        let opts: Vec<String> = p.options.iter().map(text).collect();
        return Err(format!("param `{}` must be one of {}, not {}", p.name, opts.join(", "), text(&v)));
    }
    Ok(v)
}

/// A scalar as plain text, the form argv, env and root keys take.
pub fn text(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Null => String::new(),
        other => other.to_string(),
    }
}

/// A command line `key=value` value read as the param's type.
pub fn parse_value(p: &Param, raw: &str) -> Result<Value, String> {
    let v = match p.ty {
        ParamType::String => Value::from(raw),
        ParamType::Int => Value::from(raw.parse::<i64>().map_err(|_| format!("param `{}` takes an int, not `{raw}`", p.name))?),
        ParamType::Float => Value::from(raw.parse::<f64>().map_err(|_| format!("param `{}` takes a float, not `{raw}`", p.name))?),
        ParamType::Bool => match raw {
            "true" => Value::Bool(true),
            "false" => Value::Bool(false),
            _ => return Err(format!("param `{}` takes true or false, not `{raw}`", p.name)),
        },
    };
    checked(p, v)
}

/// The `param` blocks of a group or template, each checked as it is read.
pub fn parse_all(src: &Source, fields: &Fields, scope: &Scope) -> Result<Vec<Param>, Diag> {
    let mut params: Vec<Param> = Vec::new();
    for b in fields.blocks("param") {
        let name = one_label(src, b)?.to_string();
        if params.iter().any(|p| p.name == name) {
            return Err(src.diag_at(b, format!("param `{name}` is declared twice")));
        }
        let pf = Fields::of(src, b, &["type", "options", "default", "description"], &[])?;
        let ty = pf.attr("type").map(|a| param_type(src, a)).transpose()?.unwrap_or(ParamType::String);
        let mut p = Param { name, ty, options: Vec::new(), default: None, description: None, from: None, at: Loc::of(src, b) };
        if let Some(a) = pf.attr("options") {
            match eval::value(src, a, scope)? {
                Value::Array(opts) => {
                    for o in opts {
                        let o = checked(&Param { options: Vec::new(), ..p.clone() }, o).map_err(|e| src.diag_at(&a.value, e))?;
                        p.options.push(o);
                    }
                }
                v => return Err(src.diag_at(&a.value, format!("`options` must be a list, found {}", eval::type_name(&v)))),
            }
        }
        if let Some(a) = pf.attr("default") {
            p.default = Some(checked(&p, eval::value(src, a, scope)?).map_err(|e| src.diag_at(&a.value, format!("default: {e}")))?);
        }
        if let Some(a) = pf.attr("description") {
            p.description = Some(eval::string(src, a, scope)?);
        }
        params.push(p);
    }
    Ok(params)
}

/// Every param's value: the one given, checked, else its default. A required param left
/// unset is an error, made by at with hint after it.
pub fn bind(iface: &[Param], given: BTreeMap<String, Value>, at: &dyn Fn(String) -> Diag, hint: &str) -> Result<BTreeMap<String, Value>, Diag> {
    let mut out = BTreeMap::new();
    for p in iface {
        let v = match given.get(&p.name) {
            Some(v) => checked(p, v.clone()).map_err(|e| at(e))?,
            None => match &p.default {
                Some(d) => d.clone(),
                None => return Err(at(format!("required param `{}` is not set{hint}", p.name))),
            },
        };
        out.insert(p.name.clone(), v);
    }
    Ok(out)
}
