//! Questions on stderr, answered on stdin.

use std::io::{IsTerminal, Write};

/// A yes or no question. -y answers yes at once, and so does a stdin that is no terminal,
/// with the question still printed so a log shows it.
pub fn confirm(question: &str, yes: bool) -> bool {
    let _ = std::io::stdout().flush();
    let mut err = std::io::stderr();
    if yes || !std::io::stdin().is_terminal() {
        let why = if yes { "" } else { " (stdin is not a terminal)" };
        let _ = writeln!(err, "{question} [y/N] y{why}");
        return true;
    }
    let _ = write!(err, "{question} [y/N] ");
    let _ = err.flush();
    let mut answer = String::new();
    if std::io::stdin().read_line(&mut answer).is_ok_and(|n| n > 0) {
        return matches!(answer.trim(), "y" | "Y" | "yes");
    }
    // no answer came, so the next line starts clean
    let _ = writeln!(err);
    false
}
