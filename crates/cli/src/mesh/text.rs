//! Values typed on the command line, as JSON that forgives what a shell makes awkward, so
//! `{x: 1, frame: map}`, `[a, b]` and 'single quotes' all read.

use serde_json::{Map, Value};

use crate::failure::{Result, failed, usage};

/// What to write: the value each field path gets. A whole value has the path "".
pub type Assignments = Vec<(String, Value)>;

/// One value as typed. A failure says where the text stopped making sense.
pub fn parse_text(text: &str) -> Result<Value> {
    let t = text.trim();
    if t.starts_with(['{', '[', '"', '\'']) {
        return Parser { s: t, i: 0 }.whole();
    }
    // anything else is one word or plain text
    Ok(word_value(t))
}

/// Command line words as assignments: either one value, or `path=value` words such as
/// pose.position.x=1 or corners[2].y=3.
pub fn parse_words(words: &[String]) -> Result<Assignments> {
    if words.is_empty() {
        return Err(usage("give a value, such as 1.5, {x: 1, y: 2} or x=1 y=2"));
    }
    let n = words.iter().filter(|w| assignment(w)).count();
    if n == words.len() {
        let each = |w: &String| {
            let (path, text) = w.split_once('=').unwrap_or_default();
            Ok((path.to_string(), parse_text(text)?))
        };
        return words.iter().map(each).collect();
    }
    if n > 0 {
        return Err(usage("give either one value or only field=value words, not both"));
    }
    Ok(vec![(String::new(), parse_text(&words.join(" "))?)])
}

/// A `path=value` word: names joined by dots, each maybe indexed as `[2]`.
fn assignment(w: &str) -> bool {
    let b = w.as_bytes();
    let name = |i: &mut usize| {
        let start = *i;
        while *i < b.len() && (b[*i] == b'_' || b[*i].is_ascii_alphanumeric()) {
            *i += 1;
        }
        *i > start && !b[start].is_ascii_digit()
    };
    let mut i = 0;
    if !name(&mut i) {
        return false;
    }
    loop {
        match b.get(i) {
            Some(b'=') => return true,
            Some(b'.') => {
                i += 1;
                if !name(&mut i) {
                    return false;
                }
            }
            Some(b'[') => {
                let start = i + 1;
                i = start;
                while i < b.len() && b[i].is_ascii_digit() {
                    i += 1;
                }
                if i == start || b.get(i) != Some(&b']') {
                    return false;
                }
                i += 1;
            }
            _ => return false,
        }
    }
}

/// A bare word: true, false, null, a number, or else text. A whole number too big for 64
/// bits stays text.
fn word_value(w: &str) -> Value {
    match w {
        "true" => return Value::Bool(true),
        "false" => return Value::Bool(false),
        "null" => return Value::Null,
        _ => {}
    }
    let digits = w.strip_prefix(['-', '+']).unwrap_or(w);
    if !digits.is_empty() && digits.bytes().all(|c| c.is_ascii_digit()) {
        let n = if w.starts_with('-') { w.parse::<i64>().ok().map(Value::from) } else { digits.parse::<u64>().ok().map(Value::from) };
        return n.unwrap_or_else(|| Value::String(w.to_string()));
    }
    if real(digits)
        && let Some(n) = w.parse::<f64>().ok().and_then(serde_json::Number::from_f64)
    {
        return Value::Number(n);
    }
    Value::String(w.to_string())
}

/// Digits with a point somewhere and an exponent maybe: `1.`, `.5`, `2.5e-3`, `1e9`.
fn real(s: &str) -> bool {
    let (mantissa, exponent) = match s.find(['e', 'E']) {
        Some(at) => (&s[..at], Some(&s[at + 1..])),
        None => (s, None),
    };
    let (whole, fraction) = mantissa.split_once('.').unwrap_or((mantissa, ""));
    let digits = |t: &str| t.bytes().all(|c| c.is_ascii_digit());
    let mantissa_ok = digits(whole) && digits(fraction) && !(whole.is_empty() && fraction.is_empty());
    let exponent_ok = exponent.is_none_or(|e| {
        let e = e.strip_prefix(['-', '+']).unwrap_or(e);
        !e.is_empty() && digits(e)
    });
    mantissa_ok && exponent_ok
}

struct Parser<'a> {
    s: &'a str,
    i: usize,
}

impl Parser<'_> {
    fn whole(mut self) -> Result<Value> {
        let v = self.value()?;
        self.space();
        if self.i < self.s.len() {
            return Err(self.fail("unexpected text after the value"));
        }
        Ok(v)
    }

    fn fail(&self, what: &str) -> crate::failure::Failure {
        let at = self.s[..self.i.min(self.s.len())].chars().count() + 1;
        failed(format!("cannot read the value `{}`: {what} at character {at}", self.s))
    }

    fn peek(&self) -> Option<u8> {
        self.s.as_bytes().get(self.i).copied()
    }

    fn space(&mut self) {
        while self.peek().is_some_and(|c| c.is_ascii_whitespace()) {
            self.i += 1;
        }
    }

    fn at(&mut self, c: u8) -> bool {
        self.space();
        self.peek() == Some(c)
    }

    fn value(&mut self) -> Result<Value> {
        self.space();
        match self.peek() {
            None => Err(self.fail("a value is missing")),
            Some(b'{') => self.object(),
            Some(b'[') => self.array(),
            Some(b'"' | b'\'') => self.quoted().map(Value::String),
            Some(_) => self.word().map(|w| word_value(&w)),
        }
    }

    fn word(&mut self) -> Result<String> {
        let start = self.i;
        while self.peek().is_some_and(|c| !c.is_ascii_whitespace() && !b",:{}[]".contains(&c)) {
            self.i += 1;
        }
        if self.i == start {
            return Err(self.fail("expected a value"));
        }
        Ok(self.s[start..self.i].to_string())
    }

    /// A string in single or double quotes, the double ones taking \n, \t, \r and \x for x.
    fn quoted(&mut self) -> Result<String> {
        let q = self.s[self.i..].chars().next().unwrap_or('"');
        self.i += 1;
        let mut out = String::new();
        let mut chars = self.s[self.i..].char_indices();
        while let Some((at, c)) = chars.next() {
            if c == q {
                self.i += at + 1;
                return Ok(out);
            }
            if c == '\\' && q == '"' {
                match chars.next() {
                    Some((_, 'n')) => out.push('\n'),
                    Some((_, 't')) => out.push('\t'),
                    Some((_, 'r')) => out.push('\r'),
                    Some((_, e)) => out.push(e),
                    None => break,
                }
            } else {
                out.push(c);
            }
        }
        self.i = self.s.len();
        Err(self.fail("a quote is not closed"))
    }

    fn object(&mut self) -> Result<Value> {
        self.i += 1;
        let mut o = Map::new();
        while !self.at(b'}') {
            let key = match self.peek() {
                None => return Err(self.fail("expected }")),
                Some(b'"' | b'\'') => self.quoted()?,
                Some(_) => self.word()?,
            };
            if !self.at(b':') {
                return Err(self.fail(&format!("expected : after `{key}`")));
            }
            self.i += 1;
            o.insert(key, self.value()?);
            if self.at(b',') {
                self.i += 1;
            } else if !self.at(b'}') {
                return Err(self.fail("expected , or }"));
            }
        }
        self.i += 1;
        Ok(Value::Object(o))
    }

    fn array(&mut self) -> Result<Value> {
        self.i += 1;
        let mut a = Vec::new();
        while !self.at(b']') {
            if self.peek().is_none() {
                return Err(self.fail("expected ]"));
            }
            a.push(self.value()?);
            if self.at(b',') {
                self.i += 1;
            } else if !self.at(b']') {
                return Err(self.fail("expected , or ]"));
            }
        }
        self.i += 1;
        Ok(Value::Array(a))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn parsed(t: &str) -> Value {
        parse_text(t).unwrap()
    }

    fn words(w: &[&str]) -> Result<Assignments> {
        parse_words(&w.iter().map(|s| s.to_string()).collect::<Vec<_>>())
    }

    #[test]
    fn a_word_reads_as_what_it_looks_like() {
        assert_eq!(parsed("true"), json!(true));
        assert_eq!(parsed("null"), json!(null));
        assert_eq!(parsed("42"), json!(42));
        assert_eq!(parsed("-7"), json!(-7));
        assert_eq!(parsed("+7"), json!(7));
        assert_eq!(parsed("1.5"), json!(1.5));
        assert_eq!(parsed(".5"), json!(0.5));
        assert_eq!(parsed("2e3"), json!(2000.0));
        assert_eq!(parsed("hello world"), json!("hello world"));
        assert_eq!(parsed("inf"), json!("inf"));
        assert_eq!(parsed("99999999999999999999999"), json!("99999999999999999999999"));
        assert_eq!(parsed("  map "), json!("map"));
    }

    #[test]
    fn structures_need_no_quotes() {
        assert_eq!(parsed("{x: 1, frame: map}"), json!({"x": 1, "frame": "map"}));
        assert_eq!(parsed("[a, b,]"), json!(["a", "b"]));
        assert_eq!(parsed("{'a b': \"x\\ty\", n: [1, {k: -2.5}]}"), json!({"a b": "x\ty", "n": [1, {"k": -2.5}]}));
    }

    #[test]
    fn a_mistake_says_where() {
        let e = |t: &str| parse_text(t).unwrap_err().messages()[0].clone();
        assert_eq!(e("{seq: 1"), "cannot read the value `{seq: 1`: expected , or } at character 8");
        assert_eq!(e("{seq 1}"), "cannot read the value `{seq 1}`: expected : after `seq` at character 6");
        assert_eq!(e("\"open"), "cannot read the value `\"open`: a quote is not closed at character 6");
        assert_eq!(e("[1] 2"), "cannot read the value `[1] 2`: unexpected text after the value at character 5");
    }

    #[test]
    fn words_are_one_value_or_assignments() {
        assert_eq!(words(&["{x:", "1}"]).unwrap(), vec![(String::new(), json!({"x": 1}))]);
        assert_eq!(words(&["hello", "there"]).unwrap(), vec![(String::new(), json!("hello there"))]);
        let a = words(&["pose.position.x=1", "corners[2].y=3", "name=a=b"]).unwrap();
        assert_eq!(a, vec![("pose.position.x".into(), json!(1)), ("corners[2].y".into(), json!(3)), ("name".into(), json!("a=b"))]);
        assert!(words(&["x=1", "2"]).is_err());
        assert!(words(&[]).is_err());
        assert_eq!(words(&["2x=1"]).unwrap(), vec![(String::new(), json!("2x=1"))], "a path starts with a letter");
    }
}
