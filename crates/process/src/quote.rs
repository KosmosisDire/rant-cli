//! Windows command lines: one string each program splits again, an executable by the MSVC
//! rules, a batch file through cmd.exe, which also expands %VARS% and reads & and |.

/// argv as one command line for CreateProcess, split back by the MSVC C runtime rules.
pub fn windows_command_line(argv: &[String]) -> String {
    let mut cmd = String::new();
    for (i, arg) in argv.iter().enumerate() {
        if i > 0 {
            cmd.push(' ');
        }
        let quote = arg.is_empty() || arg.contains([' ', '\t']);
        append(&mut cmd, arg, quote, Rules::Msvc);
    }
    cmd
}

/// The cmd.exe command line that runs a batch script with args as given, never expanded or
/// run (CVE-2024-24576). Refused for a line break in an argument or a quote in the name.
pub fn batch_command_line(script: &str, args: &[String]) -> Result<String, String> {
    if script.contains('"') || script.ends_with('\\') {
        return Err("a batch file name cannot hold a quote or end in a backslash".into());
    }
    // /e:ON keeps the %cd% trick working, /v:OFF leaves ! alone, /d skips AutoRun
    let mut cmd = format!("cmd.exe /e:ON /v:OFF /d /c \"\"{script}\"");
    for arg in args {
        if arg.contains(['\r', '\n', '\0']) {
            return Err("a batch file argument cannot hold a line break".into());
        }
        cmd.push(' ');
        // any ASCII mark but a few known safe ones forces quotes
        let quote = arg.is_empty() || arg.ends_with('\\') || arg.chars().any(|c| c.is_ascii() && !c.is_ascii_alphanumeric() && !"#$*+-./:?@\\_".contains(c));
        append(&mut cmd, arg, quote, Rules::Cmd);
    }
    Ok(cmd + "\"")
}

/// Who splits the command line again.
#[derive(Clone, Copy, PartialEq)]
enum Rules {
    Msvc,
    Cmd,
}

/// One argument, its backslashes doubled before a quote and before the closing quote. A
/// quote inside is escaped for MSVC and doubled for cmd.exe.
fn append(cmd: &mut String, arg: &str, quote: bool, rules: Rules) {
    if quote {
        cmd.push('"');
    }
    let mut backslashes = 0;
    for c in arg.chars() {
        match c {
            '\\' => backslashes += 1,
            '"' => {
                cmd.push_str(&"\\".repeat(backslashes));
                cmd.push(if rules == Rules::Msvc { '\\' } else { '"' });
                backslashes = 0;
            }
            // cmd.exe expands no %VAR% with %%cd:~,% in front of each %: an empty substring of cd
            '%' if rules == Rules::Cmd => {
                cmd.push_str("%%cd:~,");
                backslashes = 0;
            }
            _ => backslashes = 0,
        }
        cmd.push(c);
    }
    if quote {
        cmd.push_str(&"\\".repeat(backslashes));
        cmd.push('"');
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(argv: &[&str]) -> String {
        windows_command_line(&argv.iter().map(|s| s.to_string()).collect::<Vec<_>>())
    }

    fn batch(script: &str, args: &[&str]) -> Result<String, String> {
        batch_command_line(script, &args.iter().map(|s| s.to_string()).collect::<Vec<_>>())
    }

    #[test]
    fn plain_words_stay_bare_and_spaces_or_empty_words_are_quoted() {
        assert_eq!(line(&["C:/bin/a.exe", "x", "--y=1"]), "C:/bin/a.exe x --y=1");
        assert_eq!(line(&["a", "b c", ""]), "a \"b c\" \"\"");
    }

    #[test]
    fn quotes_and_backslashes_follow_the_msvc_rules() {
        assert_eq!(line(&["a", "say \"hi\""]), "a \"say \\\"hi\\\"\"");
        assert_eq!(line(&["a", "C:\\dir with space\\"]), "a \"C:\\dir with space\\\\\"");
        assert_eq!(line(&["a", "x\\\\\"y"]), "a x\\\\\\\\\\\"y");
        assert_eq!(line(&["a", "C:\\plain\\"]), "a C:\\plain\\");
    }

    #[test]
    fn batch_arguments_never_reach_cmd_as_code() {
        let cmd = batch("C:\\t\\run.cmd", &["&calc", "%PATH%", "a\"b", "ok"]).unwrap();
        assert_eq!(cmd, "cmd.exe /e:ON /v:OFF /d /c \"\"C:\\t\\run.cmd\" \"&calc\" \"%%cd:~,%PATH%%cd:~,%\" \"a\"\"b\" ok\"");
        assert_eq!(batch("r.bat", &["dir\\"]).unwrap(), "cmd.exe /e:ON /v:OFF /d /c \"\"r.bat\" \"dir\\\\\"\"");
    }

    #[test]
    fn what_cmd_cannot_carry_is_refused() {
        assert!(batch("C:\\t\\run.cmd", &["a\nb"]).is_err());
        assert!(batch("C:\\t\\run.cmd", &["a\rb"]).is_err());
        assert!(batch("C:\\t\\bad\".cmd", &[]).is_err());
    }
}
