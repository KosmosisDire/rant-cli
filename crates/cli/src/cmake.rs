//! A CMakeLists.txt as text: the calls in it, found past comments, quoted arguments and bracket
//! arguments, and edits that keep the rest as written.

use std::path::Path;

use crate::failure::{Result, failed};
use crate::files;

/// One command call: its name, where its arguments are, and the offset past its `)`.
pub struct Call {
    pub name: String,
    pub args_begin: usize,
    pub args_end: usize,
    pub end: usize,
}

impl Call {
    pub fn args<'t>(&self, text: &'t str) -> &'t str {
        &text[self.args_begin..self.args_end]
    }

    pub fn named(&self, names: &[&str]) -> bool {
        names.iter().any(|n| self.name.eq_ignore_ascii_case(n))
    }
}

/// Every command call. An unclosed one runs to the end of the text.
pub fn calls(t: &str) -> Vec<Call> {
    let b = t.as_bytes();
    let mut calls = Vec::new();
    let mut i = 0;
    while i < b.len() {
        let c = b[i];
        if c == b'#' {
            i = match bracket_open(b, i + 1) {
                Some(open) => skip_bracket(t, i + 1, open),
                None => line_end(t, i),
            };
            continue;
        }
        if !(c.is_ascii_alphabetic() || c == b'_') {
            i += 1;
            continue;
        }
        let start = i;
        while i < b.len() && (b[i].is_ascii_alphanumeric() || b[i] == b'_') {
            i += 1;
        }
        let mut j = i;
        while j < b.len() && (b[j] == b' ' || b[j] == b'\t') {
            j += 1;
        }
        if b.get(j) != Some(&b'(') {
            continue;
        }
        let open = j + 1;
        let (mut k, mut depth) = (open, 1);
        while k < b.len() && depth > 0 {
            match b[k] {
                b'(' => depth += 1,
                b')' => depth -= 1,
                b'"' => {
                    k += 1;
                    while k < b.len() && b[k] != b'"' {
                        k += if b[k] == b'\\' { 2 } else { 1 };
                    }
                }
                b'#' => {
                    k = match bracket_open(b, k + 1) {
                        Some(o) => skip_bracket(t, k + 1, o),
                        None => line_end(t, k),
                    } - 1
                }
                b'[' => {
                    if let Some(o) = bracket_open(b, k) {
                        k = skip_bracket(t, k, o) - 1;
                    }
                }
                _ => {}
            }
            k += 1;
        }
        let end = k.min(b.len());
        let args_end = if depth == 0 { end - 1 } else { b.len() };
        calls.push(Call { name: t[start..i].to_string(), args_begin: open, args_end: args_end.max(open), end });
        i = end;
    }
    calls
}

/// The length of a bracket opening such as `[[` or `[==[` at i, and its count of `=`.
fn bracket_open(t: &[u8], i: usize) -> Option<(usize, usize)> {
    if t.get(i) != Some(&b'[') {
        return None;
    }
    let eqs = t[i + 1..].iter().take_while(|&&c| c == b'=').count();
    (t.get(i + 1 + eqs) == Some(&b'[')).then_some((eqs + 2, eqs))
}

/// The offset past a bracket argument or comment opened at i.
fn skip_bracket(t: &str, i: usize, (len, eqs): (usize, usize)) -> usize {
    let close = format!("]{}]", "=".repeat(eqs));
    t[i + len..].find(&close).map_or(t.len(), |k| i + len + k + close.len())
}

/// The offset of the line break that ends the line at from, or the end of the text.
pub fn line_end(t: &str, from: usize) -> usize {
    t[from..].find('\n').map_or(t.len(), |k| from + k)
}

/// The first word of a call's arguments, without its quotes.
pub fn first_arg(args: &str) -> &str {
    let a = args.trim_start();
    let a = &a[..a.find([' ', '\t', '\r', '\n', ')']).unwrap_or(a.len())];
    a.strip_prefix('"').and_then(|x| x.strip_suffix('"')).unwrap_or(a)
}

/// The words of a call's arguments: quoted ones whole and unquoted, bracket arguments left out.
pub fn words(args: &str) -> Vec<&str> {
    let b = args.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        if b[i].is_ascii_whitespace() {
            i += 1;
        } else if b[i] == b'"' {
            let mut k = i + 1;
            while k < b.len() && b[k] != b'"' {
                k += if b[k] == b'\\' { 2 } else { 1 };
            }
            out.push(&args[i + 1..k.min(b.len())]);
            i = k + 1;
        } else if let Some(open) = bracket_open(b, i) {
            i = skip_bracket(args, i, open);
        } else {
            let k = i + b[i..].iter().take_while(|c| !c.is_ascii_whitespace()).count();
            out.push(&args[i..k]);
            i = k;
        }
    }
    out
}

/// Adds a node's target after the last target of a CMakeLists.txt. Refused when it has a
/// target of that name.
pub fn add_node(file: &Path, name: &str, source: &str) -> Result<()> {
    files::edit(file, |text| with_node(text, name, source))
}

fn with_node(text: &str, name: &str, source: &str) -> Result<String> {
    let mut after = None;
    for c in calls(text) {
        if !c.named(&["add_executable", "add_library", "target_link_libraries"]) {
            continue;
        }
        if !c.named(&["target_link_libraries"]) && first_arg(c.args(text)) == name {
            return Err(failed(format!("CMakeLists.txt has a target `{name}` already, nothing was written")));
        }
        after = Some(c.end);
    }
    let lines = format!("add_executable({name} {source})\ntarget_link_libraries({name} PRIVATE rant::rant_host)\n");
    let mut out = text.to_string();
    if !out.is_empty() && !out.ends_with('\n') {
        out.push('\n');
    }
    let Some(after) = after else {
        return Ok(if out.is_empty() { lines } else { out + "\n" + &lines });
    };
    let at = out[after..].find('\n').map_or(out.len(), |k| after + k + 1);
    out.insert_str(at, &lines);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_node_joins_after_the_last_target_and_never_twice() {
        let text = "project(a)\nadd_executable(a a.cpp)\ntarget_link_libraries(a PRIVATE rant::rant_host)\n\ninstall(TARGETS a)\n";
        assert_eq!(
            with_node(text, "lidar", "nodes/lidar.cpp").unwrap(),
            "project(a)\nadd_executable(a a.cpp)\ntarget_link_libraries(a PRIVATE rant::rant_host)\n\
             add_executable(lidar nodes/lidar.cpp)\ntarget_link_libraries(lidar PRIVATE rant::rant_host)\n\ninstall(TARGETS a)\n"
        );
        assert_eq!(with_node("project(b)", "b", "b.cpp").unwrap(), "project(b)\n\nadd_executable(b b.cpp)\ntarget_link_libraries(b PRIVATE rant::rant_host)\n");
        assert!(with_node(text, "a", "a2.cpp").unwrap_err().messages()[0].contains("has a target `a` already"));
    }

    #[test]
    fn calls_are_found_past_comments_quotes_and_brackets() {
        let text = "# add_executable(x)\nset(A \"add_executable(y)\")\n#[[ add_library(z) ]]\nset(B [=[ ) ]=])\nadd_executable( \"real\" r.cpp)";
        let found: Vec<(String, &str)> = calls(text).into_iter().map(|c| (c.name.clone(), c.args(text))).collect();
        assert_eq!(
            found,
            [("set".to_string(), "A \"add_executable(y)\""), ("set".to_string(), "B [=[ ) ]=]"), ("add_executable".to_string(), " \"real\" r.cpp")]
        );
        assert_eq!(first_arg(" \"real\" r.cpp"), "real");
        assert_eq!(words(" \"my app\" [[skip]] main.cpp"), ["my app", "main.cpp"]);
    }
}
