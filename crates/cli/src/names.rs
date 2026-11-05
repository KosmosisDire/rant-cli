//! Names as Rant spells them, by the rule of its C library, so a name made or checked here
//! is the one Rant uses: types and groups PascalCase, nodes and fields snake_case.

use std::ffi::{CString, c_char};

// SAFETY: Rant's C library, which the rant crate links, defines them as declared here.
unsafe extern "C" {
    fn rant_type_name(name: *const c_char, out: *mut c_char, cap: usize) -> usize;
    fn rant_field_name(name: *const c_char, out: *mut c_char, cap: usize) -> usize;
}

/// PascalCase as Rant spells a type: `lidar_driver` is `LidarDriver`.
pub fn pascal(name: &str) -> String {
    spelled(name, rant_type_name)
}

/// snake_case as Rant spells a field: `ArmController` is `arm_controller`.
pub fn snake(name: &str) -> String {
    spelled(name, rant_field_name)
}

/// A name with no word in it stays as it is.
fn spelled(name: &str, rule: unsafe extern "C" fn(*const c_char, *mut c_char, usize) -> usize) -> String {
    let Ok(c) = CString::new(name) else { return name.to_string() };
    let mut out = [0u8; 256];
    // SAFETY: c is NUL terminated and out holds the cap bytes passed.
    let n = unsafe { rule(c.as_ptr(), out.as_mut_ptr().cast(), out.len()) };
    if n == 0 { name.to_string() } else { String::from_utf8_lossy(&out[..n]).into_owned() }
}

/// A name filter: a glob when the pattern holds * or ?, otherwise a substring. An empty
/// pattern matches everything.
pub fn matches(pattern: &str, name: &str) -> bool {
    if pattern.contains(['*', '?']) { glob(pattern.as_bytes(), name.as_bytes()) } else { name.contains(pattern) }
}

/// A name chooser: a glob when the pattern holds * or ?, otherwise the whole name.
pub fn selects(pattern: &str, name: &str) -> bool {
    glob(pattern.as_bytes(), name.as_bytes())
}

fn glob(p: &[u8], t: &[u8]) -> bool {
    let (mut pi, mut ti, mut star, mut mark) = (0, 0, None, 0);
    while ti < t.len() {
        if pi < p.len() && (p[pi] == b'?' || p[pi] == t[ti]) {
            (pi, ti) = (pi + 1, ti + 1);
        } else if pi < p.len() && p[pi] == b'*' {
            (star, mark) = (Some(pi), ti);
            pi += 1;
        } else if let Some(s) = star {
            mark += 1;
            (pi, ti) = (s + 1, mark);
        } else {
            return false;
        }
    }
    p[pi..].iter().all(|&c| c == b'*')
}

/// A note for each name against the convention: a group such as nav for Nav, a node such as
/// ArmController for arm_controller. A package or prefix before a `/` is left alone.
pub fn renames(groups: &[String], nodes: &[String]) -> Vec<String> {
    let check = |kind: &str, name: &String, want_of: fn(&str) -> String| {
        let last = name.rsplit('/').next().unwrap_or(name);
        let want = want_of(last);
        (want != last).then(|| format!("rename {kind} `{name}` to `{want}`"))
    };
    let groups = groups.iter().filter_map(|g| check("group", g, pascal));
    groups.chain(nodes.iter().filter_map(|n| check("node", n, snake))).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_take_rants_spelling() {
        assert_eq!(pascal("nav"), "Nav");
        assert_eq!(pascal("lidar_driver"), "LidarDriver");
        assert_eq!(pascal("HTTPServer"), "HttpServer");
        assert_eq!(pascal("__"), "__");
        assert_eq!(snake("ArmController"), "arm_controller");
    }

    #[test]
    fn a_pattern_is_a_substring_or_a_glob() {
        assert!(matches("", "talker") && matches("alk", "talker") && !matches("x", "talker"));
        assert!(matches("t*r", "talker") && matches("ta?ker", "talker") && matches("*", "") && !matches("t*x", "talker"));
        assert!(matches("*cam*", "left/cam_driver") && !matches("cam", "Cam"));
        assert!(selects("arm/pose", "arm/pose") && !selects("arm", "arm/pose") && selects("arm/*", "arm/pose") && selects("*", "x"));
    }

    #[test]
    fn a_name_against_the_convention_gets_a_note() {
        let notes = renames(&["nav".into(), "drivers/Cameras".into()], &["drivers/ArmController".into(), "lidar".into()]);
        assert_eq!(notes, ["rename group `nav` to `Nav`", "rename node `drivers/ArmController` to `arm_controller`"]);
    }
}
