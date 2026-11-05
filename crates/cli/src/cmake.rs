//! Edits of a CMakeLists.txt that keep the rest as written, through the calls in it, found
//! past comments, quoted arguments and bracket arguments.

use std::ops::Range;
use std::path::Path;

use rant_config::paths;

use crate::failure::{Result, failed};

/// Adds a node's target after the last target of a CMakeLists.txt. Refused when it has a
/// target of that name.
pub fn add_node(file: &Path, name: &str, source: &str) -> Result<()> {
    let text = std::fs::read_to_string(file).map_err(|e| failed(format!("cannot read {}: {e}", paths::slash(file))))?;
    let changed = with_node(&text, name, source)?;
    std::fs::write(file, changed).map_err(|e| failed(format!("cannot write {}: {e}", paths::slash(file))))
}

fn with_node(text: &str, name: &str, source: &str) -> Result<String> {
    let mut after = None;
    for c in calls(text) {
        if !c.named(&["add_executable", "add_library", "target_link_libraries"]) {
            continue;
        }
        if !c.named(&["target_link_libraries"]) && first_arg(&text[c.args.clone()]) == name {
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

/// One command call: its name, where its arguments are, and the offset past its `)`.
struct Call {
    name: String,
    args: Range<usize>,
    end: usize,
}

impl Call {
    fn named(&self, names: &[&str]) -> bool {
        names.iter().any(|n| n.eq_ignore_ascii_case(&self.name))
    }
}

fn calls(text: &str) -> Vec<Call> {
    let t = text.as_bytes();
    let mut calls = Vec::new();
    let mut i = 0;
    while i < t.len() {
        let c = t[i];
        if c == b'#' {
            i = match bracket_open(t, i + 1) {
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
        while i < t.len() && (t[i].is_ascii_alphanumeric() || t[i] == b'_') {
            i += 1;
        }
        let mut j = i;
        while j < t.len() && (t[j] == b' ' || t[j] == b'\t') {
            j += 1;
        }
        if j >= t.len() || t[j] != b'(' {
            continue;
        }
        let open = j + 1;
        let mut k = open;
        let mut depth = 1;
        while k < t.len() && depth > 0 {
            match t[k] {
                b'(' => depth += 1,
                b')' => depth -= 1,
                b'"' => {
                    k += 1;
                    while k < t.len() && t[k] != b'"' {
                        k += if t[k] == b'\\' { 2 } else { 1 };
                    }
                }
                b'#' => {
                    k = match bracket_open(t, k + 1) {
                        Some(b) => skip_bracket(t, k + 1, b),
                        None => line_end(t, k),
                    } - 1;
                }
                b'[' => {
                    if let Some(b) = bracket_open(t, k) {
                        k = skip_bracket(t, k, b) - 1;
                    }
                }
                _ => {}
            }
            k += 1;
        }
        let end = k.min(t.len());
        calls.push(Call { name: text[start..i].to_string(), args: open..end.saturating_sub(1).max(open), end });
        i = end;
    }
    calls
}

/// A bracket opening such as `[[` or `[==[` at i: its length and its count of `=`.
fn bracket_open(t: &[u8], i: usize) -> Option<(usize, usize)> {
    if t.get(i) != Some(&b'[') {
        return None;
    }
    let eqs = t[i + 1..].iter().take_while(|c| **c == b'=').count();
    (t.get(i + 1 + eqs) == Some(&b'[')).then_some((eqs + 2, eqs))
}

/// The offset past a bracket argument or comment opened at i.
fn skip_bracket(t: &[u8], i: usize, (len, eqs): (usize, usize)) -> usize {
    let close = [b"]".as_slice(), &vec![b'='; eqs], b"]"].concat();
    t[i + len..].windows(close.len()).position(|w| w == close).map_or(t.len(), |k| i + len + k + close.len())
}

fn line_end(t: &[u8], i: usize) -> usize {
    t[i..].iter().position(|c| *c == b'\n').map_or(t.len(), |k| i + k)
}

/// The first word of a call's arguments, without its quotes.
fn first_arg(args: &str) -> &str {
    let a = args.trim_start_matches([' ', '\t', '\r', '\n']);
    let a = &a[..a.find([' ', '\t', '\r', '\n', ')']).unwrap_or(a.len())];
    a.strip_prefix('"').and_then(|s| s.strip_suffix('"')).unwrap_or(a)
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
        let found: Vec<(String, &str)> = calls(text).into_iter().map(|c| (c.name, &text[c.args])).collect();
        assert_eq!(
            found,
            [("set".to_string(), "A \"add_executable(y)\""), ("set".to_string(), "B [=[ ) ]=]"), ("add_executable".to_string(), " \"real\" r.cpp")]
        );
        assert_eq!(first_arg(" \"real\" r.cpp"), "real");
    }
}
