//! Splits a command string into argv with one POSIX style rule on every OS. Nothing is
//! expanded: no variables, globs or tildes, since no shell ever runs it.

/// Single quotes keep everything literally. Double quotes keep everything except a
/// backslash before `"`, `\`, `$` or a backtick. Outside quotes a backslash keeps the next
/// character. Unquoted whitespace separates words.
pub fn split(s: &str) -> Result<Vec<String>, String> {
    let mut out = Vec::new();
    let mut word = String::new();
    let mut in_word = false;
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            c if c.is_whitespace() => {
                if in_word {
                    out.push(std::mem::take(&mut word));
                    in_word = false;
                }
            }
            '\'' => {
                in_word = true;
                loop {
                    match chars.next() {
                        Some('\'') => break,
                        Some(c) => word.push(c),
                        None => return Err("unterminated single quote".into()),
                    }
                }
            }
            '"' => {
                in_word = true;
                loop {
                    match chars.next() {
                        Some('"') => break,
                        Some('\\') => match chars.peek() {
                            Some(&n) if matches!(n, '"' | '\\' | '$' | '`') => {
                                word.push(n);
                                chars.next();
                            }
                            _ => word.push('\\'),
                        },
                        Some(c) => word.push(c),
                        None => return Err("unterminated double quote".into()),
                    }
                }
            }
            '\\' => {
                in_word = true;
                match chars.next() {
                    Some(n) => word.push(n),
                    None => return Err("trailing backslash".into()),
                }
            }
            c => {
                in_word = true;
                word.push(c);
            }
        }
    }
    if in_word {
        out.push(word);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::split;

    #[test]
    fn plain_words() {
        assert_eq!(split("  npm  run build ").unwrap(), ["npm", "run", "build"]);
    }

    #[test]
    fn quotes() {
        assert_eq!(split(r#"a 'b c' "d \"e\" \n" f"#).unwrap(), ["a", "b c", r#"d "e" \n"#, "f"]);
        assert_eq!(split(r#"--name='' x"#).unwrap(), ["--name=", "x"]);
    }

    #[test]
    fn backslashes_and_windows_paths() {
        assert_eq!(split(r"a\ b").unwrap(), ["a b"]);
        assert_eq!(split(r#""C:\tools\x.exe" -v"#).unwrap(), [r"C:\tools\x.exe", "-v"]);
    }

    #[test]
    fn nothing_expands() {
        assert_eq!(split("echo $HOME ~ *.txt").unwrap(), ["echo", "$HOME", "~", "*.txt"]);
    }

    #[test]
    fn errors() {
        assert!(split("'open").is_err());
        assert!(split("\"open").is_err());
        assert!(split("end\\").is_err());
    }
}
