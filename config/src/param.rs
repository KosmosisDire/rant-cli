//! Params, as group files declare them: a name, a type, options, a default
//! and a description. Values from a file or the command line are checked against them.

use std::collections::BTreeMap;
use std::path::Path;

use hcl::Value;
use hcl_edit::structure::Attribute;

use crate::diag::Diag;
use crate::eval::{self, Scope};
use crate::paths;
use crate::source::{one_label, Fields, Loc, Source};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParamType {
    String,
    Int,
    Float,
    Bool,
    /// A file or folder, a relative one counted from where it is given.
    Path,
}

impl ParamType {
    pub fn name(self) -> &'static str {
        match self {
            ParamType::String => "string",
            ParamType::Int => "int",
            ParamType::Float => "float",
            ParamType::Bool => "bool",
            ParamType::Path => "path",
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
        "path" => Ok(ParamType::Path),
        other => Err(src.diag_at(&attr.value, format!("unknown type `{other}`, expected string, int, float, bool or path"))),
    }
}

/// A value checked against a param's type, an int widened for a float, and against its
/// options. The error says what was wrong without a place, the caller adds one.
pub fn checked(p: &Param, v: Value) -> Result<Value, String> {
    let v = match (p.ty, v) {
        (ParamType::String | ParamType::Path, v @ Value::String(_)) => v,
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

/// A path param's value made absolute, a relative one counted from `base`: the folder rant
/// runs in for a command line value, the file's folder for one a file writes. Others pass.
pub fn placed(p: &Param, v: Value, base: &Path) -> Value {
    match (p.ty, v) {
        (ParamType::Path, Value::String(s)) => Value::String(eval::slash(&paths::normalize(&base.join(s)))),
        (_, v) => v,
    }
}

/// A scalar as plain text, the form argv, env and root keys take.
pub fn text(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Null => String::new(),
        other => other.to_string(),
    }
}

/// A command line `key=value` value read as the param's type, a path counted from `base`.
pub fn parse_value(p: &Param, raw: &str, base: &Path) -> Result<Value, String> {
    let v = match p.ty {
        ParamType::String => Value::from(raw),
        ParamType::Path => placed(p, Value::from(raw), base),
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

/// Command line `key=value` words read as the params of a group, a path counted from `base`,
/// in the order given.
pub fn from_words(iface: &[Param], group: &str, raw: &[String], base: &Path) -> Result<Vec<(String, Value)>, Diag> {
    let mut out: Vec<(String, Value)> = Vec::new();
    for kv in raw {
        let Some((k, v)) = kv.split_once('=') else {
            return Err(Diag::plain(format!("params are key=value, not `{kv}`")));
        };
        let Some(p) = iface.iter().find(|p| p.name == k) else {
            return Err(Diag::plain(format!("group `{group}` has no param `{k}`, see `rant run {group} --help`")));
        };
        let v = parse_value(p, v, base).map_err(Diag::plain)?;
        if out.iter().any(|(n, _)| n == k) {
            return Err(Diag::plain(format!("param `{k}` is given twice")));
        }
        out.push((k.to_string(), v));
    }
    Ok(out)
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
            if p.ty == ParamType::Path {
                return Err(src.diag_at(&a.value, "a path param takes no options"));
            }
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
            let v = checked(&p, eval::value(src, a, scope)?).map_err(|e| src.diag_at(&a.value, format!("default: {e}")))?;
            p.default = Some(placed(&p, v, src.dir()));
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
                None => return Err(at(format!("param `{}` needs a value{hint}", p.name))),
            },
        };
        out.insert(p.name.clone(), v);
    }
    Ok(out)
}
