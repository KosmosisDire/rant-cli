//! Data as YAML for a person to read: keys aligned, floats to two decimals, colored in the
//! explorer's palette on a terminal. JSON and CSV keep the full values.

use serde_json::Value;

use super::{Style, paint};

/// Block style over as many lines as it takes, with no newline at the end.
pub fn block(v: &Value, color: bool) -> String {
    let y = Yaml { color };
    if (one_line(v) && !v.is_object()) || is_empty(v) {
        return y.inline(v);
    }
    let mut out = String::new();
    y.block_into(&mut out, v, 0, 0);
    out
}

struct Yaml {
    color: bool,
}

impl Yaml {
    fn paint(&self, s: Style, text: &str) -> String {
        paint(self.color, s, text)
    }

    fn block_into(&self, out: &mut String, v: &Value, indent: usize, key_width: usize) {
        let pad = " ".repeat(indent);
        if let Value::Object(map) = v {
            let widest = map.keys().map(|k| text_of(k, false).chars().count()).fold(key_width, usize::max);
            for (k, e) in map {
                if !out.is_empty() {
                    out.push('\n');
                }
                if one_line(e) {
                    *out += &format!("{pad}{} {}", self.key(k, widest), self.inline(e));
                } else {
                    *out += &format!("{pad}{}\n", self.key(k, 0));
                    let mut inner = String::new();
                    self.block_into(&mut inner, e, indent + 2, 0);
                    *out += &inner;
                }
            }
            return;
        }
        // a list holding objects or lists, one item per dash
        for e in v.as_array().into_iter().flatten() {
            if !out.is_empty() {
                out.push('\n');
            }
            let item = if e.is_object() && !is_empty(e) {
                let mut item = String::new();
                self.block_into(&mut item, e, indent + 2, 0);
                // the first key sits on the dash line
                item.split_off(indent + 2)
            } else {
                self.inline(e)
            };
            *out += &format!("{pad}{} {item}", self.paint(Style::Faint, "-"));
        }
    }

    fn key(&self, k: &str, width: usize) -> String {
        let text = text_of(k, false);
        let pad = " ".repeat(width.saturating_sub(text.chars().count()));
        format!("{}{}{pad}", self.paint(Style::Dim, &text), self.paint(Style::Faint, ":"))
    }

    /// A value that fits on one line: a scalar, an empty container or a list of scalars.
    fn inline(&self, v: &Value) -> String {
        match v {
            Value::Array(items) if items.is_empty() => self.paint(Style::Faint, "[]"),
            Value::Object(map) if map.is_empty() => self.paint(Style::Faint, "{}"),
            Value::Array(items) => {
                let sep = format!("{} ", self.paint(Style::Faint, ","));
                let inner: Vec<String> = items.iter().map(|e| self.scalar(e, true)).collect();
                format!("{}{}{}", self.paint(Style::Faint, "["), inner.join(&sep), self.paint(Style::Faint, "]"))
            }
            v => self.scalar(v, false),
        }
    }

    fn scalar(&self, v: &Value, in_flow: bool) -> String {
        match v {
            Value::Null => self.paint(Style::Faint, "null"),
            Value::Bool(b) => self.paint(Style::Amber, if *b { "true" } else { "false" }),
            Value::String(s) => self.paint(Style::GreenHi, &text_of(s, in_flow)),
            Value::Number(n) => number_text(n),
            v => serde_json::to_string(v).unwrap_or_default(),
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
    fn colors_follow_the_palette() {
        assert_eq!(block(&json!({"up": true}), true), "\x1b[38;2;154;154;154mup\x1b[0m\x1b[38;2;100;100;100m:\x1b[0m \x1b[38;2;207;154;58mtrue\x1b[0m");
    }
}
