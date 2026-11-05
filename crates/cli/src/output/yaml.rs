//! Data as YAML for a person to read: keys aligned, floats to two decimals, colored in the
//! explorer's palette on a terminal. JSON and CSV keep the full values.

use std::collections::HashMap;

use serde_json::Value;

use super::{Style, paint};

/// Block style over as many lines as it takes, with no newline at the end.
pub fn block(v: &Value, color: bool) -> String {
    Yaml::new(color, false).block(v)
}

/// A YAML writer. A stable one gives each number a sign slot and its field's widest width yet,
/// so values redrawn in place or a line per message never shift.
pub struct Yaml {
    color: bool,
    stable: bool,
    widths: HashMap<String, usize>,
}

impl Yaml {
    pub fn new(color: bool, stable: bool) -> Yaml {
        Yaml { color, stable, widths: HashMap::new() }
    }

    /// Block style over as many lines as it takes, with no newline at the end.
    pub fn block(&mut self, v: &Value) -> String {
        if (one_line(v) && !v.is_object()) || is_empty(v) {
            return self.inline(v, "", false);
        }
        let mut out = String::new();
        self.block_into(&mut out, v, 0, "", 0);
        out
    }

    /// Flow style on one line: {x: 1.00, tags: [a, b]}.
    pub fn flow(&mut self, v: &Value) -> String {
        self.flow_at(v, "")
    }

    /// One mapping entry in block style, its key padded to key_width so entries written one by
    /// one line their values up.
    pub fn entry(&mut self, key: &str, v: &Value, key_width: usize) -> String {
        let mut out = String::new();
        let one = Value::Object(std::iter::once((key.to_string(), v.clone())).collect());
        self.block_into(&mut out, &one, 0, "", key_width);
        out
    }

    /// A key as an entry writes it, padded to width, colon included.
    pub fn key(&self, k: &str, width: usize) -> String {
        let text = text_of(k, false);
        let pad = " ".repeat(width.saturating_sub(text.chars().count()));
        format!("{}{}{pad}", self.paint(Style::Dim, &text), self.paint(Style::Faint, ":"))
    }

    fn paint(&self, s: Style, text: &str) -> String {
        paint(self.color, s, text)
    }

    fn block_into(&mut self, out: &mut String, v: &Value, indent: usize, path: &str, key_width: usize) {
        let pad = " ".repeat(indent);
        if let Value::Object(map) = v {
            let widest = map.keys().map(|k| text_of(k, false).chars().count()).fold(key_width, usize::max);
            for (k, e) in map {
                let child = if path.is_empty() { k.clone() } else { format!("{path}.{k}") };
                if !out.is_empty() {
                    out.push('\n');
                }
                if one_line(e) {
                    let value = self.inline(e, &child, false);
                    *out += &format!("{pad}{} {value}", self.key(k, widest));
                } else {
                    *out += &format!("{pad}{}\n", self.key(k, 0));
                    let mut inner = String::new();
                    self.block_into(&mut inner, e, indent + 2, &child, 0);
                    *out += &inner;
                }
            }
            return;
        }
        // a list holding objects or lists, one item per dash
        let child = format!("{path}[]");
        for e in v.as_array().into_iter().flatten() {
            if !out.is_empty() {
                out.push('\n');
            }
            let item = if e.is_object() && !is_empty(e) {
                let mut item = String::new();
                self.block_into(&mut item, e, indent + 2, &child, 0);
                // the first key sits on the dash line
                item.split_off(indent + 2)
            } else {
                self.inline(e, &child, false)
            };
            *out += &format!("{pad}{} {item}", self.paint(Style::Faint, "-"));
        }
    }

    fn flow_at(&mut self, v: &Value, path: &str) -> String {
        let Value::Object(map) = v else { return self.inline(v, path, true) };
        let mut parts = Vec::new();
        for (k, e) in map {
            let child = if path.is_empty() { k.clone() } else { format!("{path}.{k}") };
            let value = self.inline(e, &child, true);
            parts.push(format!("{}{} {value}", self.paint(Style::Dim, &text_of(k, true)), self.paint(Style::Faint, ":")));
        }
        let sep = format!("{} ", self.paint(Style::Faint, ","));
        format!("{}{}{}", self.paint(Style::Faint, "{"), parts.join(&sep), self.paint(Style::Faint, "}"))
    }

    /// A value on one line: a scalar, an empty container, or a list or object in flow style.
    fn inline(&mut self, v: &Value, path: &str, in_flow: bool) -> String {
        match v {
            Value::Array(items) if items.is_empty() => self.paint(Style::Faint, "[]"),
            Value::Object(map) if map.is_empty() => self.paint(Style::Faint, "{}"),
            Value::Array(items) => {
                let child = format!("{path}[]");
                let inner: Vec<String> = items.iter().map(|e| self.inline(e, &child, true)).collect();
                let sep = format!("{} ", self.paint(Style::Faint, ","));
                format!("{}{}{}", self.paint(Style::Faint, "["), inner.join(&sep), self.paint(Style::Faint, "]"))
            }
            Value::Object(_) => self.flow_at(v, path),
            v => self.scalar(v, path, in_flow),
        }
    }

    fn scalar(&mut self, v: &Value, path: &str, in_flow: bool) -> String {
        match v {
            Value::Null => self.paint(Style::Faint, "null"),
            Value::Bool(b) => self.paint(Style::Amber, if *b { "true" } else { "false" }),
            Value::String(s) => self.paint(Style::GreenHi, &text_of(s, in_flow)),
            Value::Number(n) if self.stable => {
                // the slot a minus sign takes
                let t = number_text(n);
                let t = if t.starts_with('-') { t } else { format!(" {t}") };
                let w = self.widths.entry(path.to_string()).or_default();
                *w = (*w).max(t.len());
                format!("{t:>w$}", w = *w)
            }
            Value::Number(n) => number_text(n),
            v => v.to_string(),
        }
    }
}

fn is_empty(v: &Value) -> bool {
    match v {
        Value::Array(a) => a.is_empty(),
        Value::Object(o) => o.is_empty(),
        _ => false,
    }
}

/// A value that fits after its key on one line: a scalar, an empty container, or a list of
/// scalars, which reads best as [a, b].
fn one_line(v: &Value) -> bool {
    match v {
        Value::Object(o) => o.is_empty(),
        Value::Array(a) => !a.iter().any(|e| e.is_object() || e.is_array()),
        _ => true,
    }
}

/// A number as shown: floats to two decimals, a rounded zero without its minus sign.
fn number_text(n: &serde_json::Number) -> String {
    match n.as_f64().filter(|_| n.is_f64()) {
        Some(d) => {
            let s = format!("{d:.2}");
            if s == "-0.00" { "0.00".to_string() } else { s }
        }
        None => n.to_string(),
    }
}

fn text_of(s: &str, in_flow: bool) -> String {
    if needs_quotes(s, in_flow) { quoted(s) } else { s.to_string() }
}

/// Text that would read back as something else, or break the structure, unquoted.
fn needs_quotes(s: &str, in_flow: bool) -> bool {
    let (Some(first), Some(last)) = (s.chars().next(), s.chars().last()) else { return true };
    let lower = s.to_lowercase();
    let hex = lower.strip_prefix("0x").is_some_and(|h| !h.is_empty() && h.chars().all(|c| c.is_ascii_hexdigit()));
    s.parse::<f64>().is_ok()
        || hex
        || ["true", "false", "null", "~", "yes", "no", "on", "off", ".inf", ".nan"].contains(&lower.as_str())
        || " -?:,[]{}#&*!|>'\"%@`".contains(first)
        || last == ' '
        || last == ':'
        || s.contains(": ")
        || s.contains(" #")
        || s.chars().any(|c| (c as u32) < 0x20 || c == '\x7f' || (in_flow && ",[]{}".contains(c)))
}

fn quoted(s: &str) -> String {
    let mut out = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => out += "\\\"",
            '\\' => out += "\\\\",
            '\n' => out += "\\n",
            '\t' => out += "\\t",
            '\r' => out += "\\r",
            c if (c as u32) < 0x20 || c == '\x7f' => out += &format!("\\x{:02x}", c as u32),
            c => out.push(c),
        }
    }
    out + "\""
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn plain(v: Value) -> String {
        block(&v, false)
    }

    #[test]
    fn a_block_aligns_keys_and_nests() {
        let v = json!({"seq": 3, "pose": {"x": 1.5, "yaw": -0.25}, "tags": ["a", "b"], "pts": [{"x": 1, "y": 2}], "none": []});
        assert_eq!(plain(v), "seq:  3\npose:\n  x:   1.50\n  yaw: -0.25\ntags: [a, b]\npts:\n  - x: 1\n    y: 2\nnone: []");
    }

    #[test]
    fn floats_take_two_decimals_and_no_negative_zero() {
        assert_eq!(plain(json!(2.0)), "2.00");
        assert_eq!(plain(json!(-0.004)), "0.00");
        assert_eq!(plain(json!(1234.5678)), "1234.57");
        assert_eq!(plain(json!(7)), "7");
    }

    #[test]
    fn quotes_only_what_would_misread() {
        assert_eq!(plain(json!("hello world")), "hello world");
        assert_eq!(plain(json!("true")), "\"true\"");
        assert_eq!(plain(json!("")), "\"\"");
        assert_eq!(plain(json!("12")), "\"12\"");
        assert_eq!(plain(json!("0x1f")), "\"0x1f\"");
        assert_eq!(plain(json!("a: b")), "\"a: b\"");
        assert_eq!(plain(json!("-x")), "\"-x\"");
        assert_eq!(plain(json!("two\nlines")), "\"two\\nlines\"");
        assert_eq!(plain(json!(["a,b"])), "[\"a,b\"]", "a list in a block is flow");
    }

    #[test]
    fn a_stable_writer_holds_each_number_in_place() {
        let mut y = Yaml::new(false, true);
        assert_eq!(y.flow(&json!({"from": "a", "hz": 9.5, "value": {"seq": 7, "pts": [1, -2]}})), "{from: a, hz:  9.50, value: {seq:  7, pts: [ 1, -2]}}");
        assert_eq!(y.flow(&json!({"from": "a", "hz": 10.25, "value": {"seq": 8, "pts": [3]}})), "{from: a, hz:  10.25, value: {seq:  8, pts: [ 3]}}");
        assert_eq!(y.flow(&json!({"from": "a", "hz": 1.0, "value": {"seq": 9, "pts": []}})), "{from: a, hz:   1.00, value: {seq:  9, pts: []}}");
        assert_eq!(y.block(&json!({"x": -1.0})), "x: -1.00");
        assert_eq!(y.block(&json!({"x": 1.0})), "x:  1.00");
        assert_eq!(Yaml::new(false, false).entry("pid", &json!(12), 5), "pid:   12");
    }

    #[test]
    fn colors_follow_the_palette() {
        assert_eq!(block(&json!({"up": true}), true), "\x1b[38;2;154;154;154mup\x1b[0m\x1b[38;2;100;100;100m:\x1b[0m \x1b[38;2;207;154;58mtrue\x1b[0m");
    }
}
