//! The small hook each shell loads to hand tab completion to `rant __complete`, and the lines
//! setup and the install scripts add outside rant's home.

use std::path::{Path, PathBuf};

use rant_process::Command;

use crate::failure::{Result, failed};
use crate::home::{env_path, home_dir};

pub struct Shell {
    pub name: &'static str,
    /// The script the shell loads.
    pub hook: &'static str,
}

/// bash splits words at = and :, so the part of a candidate up to the last of them is cut,
/// since bash only replaces what follows.
const BASH: &str = r#"# rant tab completion for bash, written by `rant setup`
_rant_complete() {
    local line="${COMP_LINE:0:COMP_POINT}" word keep
    local -a words reply
    read -ra words <<< "$line"
    [[ $line == *[[:space:]] ]] && words+=("")
    word="${words[${#words[@]}-1]}"
    keep="${word%"${word##*[=:]}"}"
    local IFS=$'\n'
    reply=($(command rant __complete "--cur=$word" "${words[@]:1:${#words[@]}-2}" 2>/dev/null))
    COMPREPLY=("${reply[@]#"$keep"}")
    if [[ ${#reply[@]} -eq 1 && ${reply[0]} == *[/=] ]] && type compopt &>/dev/null; then
        compopt -o nospace
    fi
}
complete -F _rant_complete rant
"#;

/// zsh filters candidates by case unless -U takes them as rant matched them.
const ZSH: &str = r#"# rant tab completion for zsh, written by `rant setup`
_rant() {
    local -a reply
    local c
    reply=("${(@f)$(command rant __complete "--cur=${words[CURRENT]}" "${(@)words[2,CURRENT-1]}" 2>/dev/null)}")
    for c in "${reply[@]}"; do
        [[ -z $c ]] && continue
        if [[ $c == *[/=] ]]; then compadd -U -S '' -- "$c"; else compadd -U -- "$c"; fi
    done
}
(( $+functions[compdef] )) || { autoload -Uz compinit && compinit }
compdef _rant rant
"#;

/// fish adds no space after a candidate ending in / or = by itself.
const FISH: &str = r#"# rant tab completion for fish, written by `rant setup`
function __rant_complete
    set -l before (commandline -opc)
    set -l cur (commandline -ct)
    command rant __complete "--cur=$cur" $before[2..-1] 2>/dev/null
end
complete -c rant -f -a '(__rant_complete)'
"#;

const POWERSHELL: &str = r#"# rant tab completion for PowerShell, written by `rant setup`
Register-ArgumentCompleter -Native -CommandName rant, rant.exe -ScriptBlock {
    param($wordToComplete, $commandAst, $cursorPosition)
    $words = @($commandAst.CommandElements | Select-Object -Skip 1 |
        Where-Object { $_.Extent.EndOffset -le $cursorPosition } | ForEach-Object { $_.Extent.Text })
    if ($wordToComplete -ne '' -and $words.Count -gt 0) { $words = @($words | Select-Object -SkipLast 1) }
    & rant __complete "--cur=$wordToComplete" @words 2>$null | ForEach-Object {
        [System.Management.Automation.CompletionResult]::new($_, $_, 'ParameterValue', $_)
    }
}
"#;

pub const SHELLS: [Shell; 5] = [
    Shell { name: "bash", hook: BASH },
    Shell { name: "zsh", hook: ZSH },
    Shell { name: "fish", hook: FISH },
    Shell { name: "powershell", hook: POWERSHELL },
    Shell { name: "pwsh", hook: POWERSHELL },
];

pub fn find(name: &str) -> Option<&'static Shell> {
    SHELLS.iter().find(|s| s.name == name)
}

/// The shells found on this machine: PowerShell on Windows, else bash, zsh and fish on PATH.
pub fn detected() -> Vec<&'static Shell> {
    let names: &[&str] = if cfg!(windows) { &["powershell", "pwsh"] } else { &["bash", "zsh", "fish"] };
    names.iter().filter(|n| rant_process::find_program(n, &std::env::temp_dir()).is_some()).filter_map(|n| find(n)).collect()
}

fn user_home() -> Result<PathBuf> {
    home_dir().ok_or_else(|| failed("no home folder known, set HOME"))
}

fn config_dir() -> Result<PathBuf> {
    env_path("XDG_CONFIG_HOME").map_or_else(|| user_home().map(|h| h.join(".config")), Ok)
}

fn zshrc() -> Result<PathBuf> {
    env_path("ZDOTDIR").map_or_else(user_home, Ok).map(|d| d.join(".zshrc"))
}

fn write_file(file: &Path, text: &str) -> Result<()> {
    if let Some(dir) = file.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    std::fs::write(file, text).map_err(|e| failed(format!("cannot write {}: {e}", file.display())))
}

/// Adds line to a shell's startup file unless it is there already.
fn add_line(rc: &Path, line: &str) -> Result<()> {
    let mut text = std::fs::read_to_string(rc).unwrap_or_default();
    if text.contains(line) {
        return Ok(());
    }
    if !text.is_empty() && !text.ends_with('\n') {
        text.push('\n');
    }
    write_file(rc, &format!("{text}{line}\n"))
}

/// A path in single quotes for a POSIX shell.
fn sh_quoted(p: &Path) -> String {
    format!("'{}'", rant_config::paths::slash(p).replace('\'', r"'\''"))
}

/// A string in single quotes for PowerShell.
fn ps_quoted(s: &str) -> String {
    format!("'{}'", s.replace('\'', "''"))
}

fn hook_file(rant_home: &Path, name: &str) -> PathBuf {
    rant_home.join("completions").join(name)
}

/// The line a POSIX shell's startup file loads a hook with.
fn posix_line(hook: &Path) -> String {
    format!("[ -f {q} ] && . {q}", q = sh_quoted(hook))
}

/// The line a PowerShell profile loads the hook with.
fn powershell_line(rant_home: &Path) -> String {
    let path = ps_quoted(&hook_file(rant_home, "rant.ps1").to_string_lossy());
    format!("if (Test-Path {path}) {{ . {path} }}")
}

/// Base64 of UTF-16LE, as PowerShell's -EncodedCommand takes a script that no quoting can break.
fn encoded_command(script: &str) -> String {
    const B64: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let bytes: Vec<u8> = script.encode_utf16().flat_map(u16::to_le_bytes).collect();
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let n = chunk.iter().enumerate().fold(0u32, |n, (i, &b)| n | (b as u32) << (16 - 8 * i));
        for i in 0..4 {
            out.push(if i <= chunk.len() { B64[(n >> (18 - 6 * i) & 63) as usize] as char } else { '=' });
        }
    }
    out
}

/// PowerShell run alone, with its own module path, since Windows PowerShell started from pwsh
/// otherwise loads pwsh's modules and fails on Get-ExecutionPolicy.
fn powershell(exe: &str, args: &[&str]) -> Command {
    let base = rant_process::environment(&Command::default()).into_iter().filter(|e| !e.to_ascii_lowercase().starts_with("psmodulepath=")).collect();
    let argv = [exe, "-NoProfile", "-NoLogo", "-NonInteractive"].iter().chain(args).map(|a| a.to_string()).collect();
    Command { argv, cwd: std::env::temp_dir(), base: Some(base), ..Command::default() }
}

/// PowerShell edits its own profile, since only it knows where that is and whether it may run
/// scripts.
fn install_powershell(exe: &str, rant_home: &Path) -> Result<String> {
    let line = powershell_line(rant_home);
    let script = format!(
        "$ProgressPreference = 'SilentlyContinue'\n\
         $line = {}\n\
         $policy = Get-ExecutionPolicy\n\
         if ($policy -eq 'Restricted' -or $policy -eq 'AllSigned') {{ exit 3 }}\n\
         New-Item -ItemType Directory -Force (Split-Path $PROFILE) | Out-Null\n\
         if (-not ((Test-Path $PROFILE) -and (Select-String -Path $PROFILE -SimpleMatch $line -Quiet))) {{\n\
         \x20   Add-Content -Path $PROFILE -Value $line -Encoding utf8\n\
         }}\n\
         exit 0\n",
        ps_quoted(&line)
    );
    if rant_process::find_program(exe, &std::env::temp_dir()).is_none() {
        return Err(failed(format!("{exe} is not on PATH")));
    }
    match rant_process::run(&powershell(exe, &["-EncodedCommand", &encoded_command(&script)]))? {
        0 => Ok("its profile".into()),
        3 => Err(failed(format!(
            "it may not run scripts here, allow that with `Set-ExecutionPolicy -Scope CurrentUser RemoteSigned` in {exe} and run `rant setup {exe}` again"
        ))),
        code => Err(failed(format!("could not edit the {exe} profile (exit {code})"))),
    }
}

/// Writes the hook under rant's home and makes the shell load it, once however often it runs.
/// Says where it loads from: the startup file, or its profile.
pub fn install(shell: &Shell, rant_home: &Path) -> Result<String> {
    match shell.name {
        "fish" => {
            let file = config_dir()?.join("fish").join("completions").join("rant.fish");
            write_file(&file, shell.hook)?;
            Ok(file.display().to_string())
        }
        "powershell" | "pwsh" => {
            write_file(&hook_file(rant_home, "rant.ps1"), shell.hook)?;
            install_powershell(shell.name, rant_home)
        }
        name => {
            let hook = hook_file(rant_home, &format!("rant.{name}"));
            write_file(&hook, shell.hook)?;
            let rc = if name == "bash" { user_home()?.join(".bashrc") } else { zshrc()? };
            add_line(&rc, &posix_line(&hook))?;
            Ok(rc.display().to_string())
        }
    }
}

/// One thing setup or an install script added outside rant's home: where it is, what part of
/// it when not all, and how to take it out again.
pub struct Added {
    pub path: PathBuf,
    pub part: String,
    pub remove: Box<dyn Fn() -> Result<()>>,
}

/// The lines of a file that drop matches gone, each with a blank line right before it, which
/// the install script writes. None when nothing matched.
fn without_lines(text: &str, drop: &dyn Fn(&str) -> bool) -> Option<String> {
    let mut kept: Vec<&str> = Vec::new();
    let mut any = false;
    for l in text.split_inclusive('\n') {
        if !drop(l.trim_end_matches(['\n', '\r'])) {
            kept.push(l);
            continue;
        }
        any = true;
        if kept.last().is_some_and(|k| k.trim_end_matches(['\n', '\r']).is_empty()) {
            kept.pop();
        }
    }
    any.then(|| kept.concat())
}

/// A file whose matching lines go when the remover runs.
fn lines_in(out: &mut Vec<Added>, file: PathBuf, drop: impl Fn(&str) -> bool + 'static) {
    let Ok(text) = std::fs::read_to_string(&file) else { return };
    if without_lines(&text, &drop).is_none() {
        return;
    }
    let target = file.clone();
    let remove = move || {
        let text = std::fs::read_to_string(&target).unwrap_or_default();
        match without_lines(&text, &drop) {
            Some(rest) => write_file(&target, &rest),
            None => Ok(()),
        }
    };
    out.push(Added { path: file, part: "its rant lines".into(), remove: Box::new(remove) });
}

/// A file that is wholly rant's, when what it holds shows that.
fn file_of_rant(out: &mut Vec<Added>, file: PathBuf, mark: &str) {
    if !std::fs::read_to_string(&file).is_ok_and(|t| t.contains(mark)) {
        return;
    }
    let target = file.clone();
    let remove = move || std::fs::remove_file(&target).map_err(|e| failed(format!("cannot remove {}: {e}", target.display())));
    out.push(Added { path: file, part: String::new(), remove: Box::new(remove) });
}

/// What is there now of the lines and files that load rant into shells and put it on PATH.
pub fn added(rant_home: &Path) -> Vec<Added> {
    let mut out = Vec::new();
    let Ok(home) = user_home() else { return out };
    // the install script marks its PATH line, setup writes its hook line exactly so
    let posix = |hook: &str| {
        let line = posix_line(&hook_file(rant_home, hook));
        move |l: &str| l == line || (l.starts_with("export PATH=") && l.ends_with("# rant-cli"))
    };
    lines_in(&mut out, home.join(".bashrc"), posix("rant.bash"));
    if let Ok(rc) = zshrc() {
        lines_in(&mut out, rc, posix("rant.zsh"));
    }
    lines_in(&mut out, home.join(".profile"), posix("rant.bash"));
    if let Ok(config) = config_dir() {
        file_of_rant(&mut out, config.join("fish").join("conf.d").join("rant.fish"), "# rant-cli");
        file_of_rant(&mut out, config.join("fish").join("completions").join("rant.fish"), "rant __complete");
    }
    if cfg!(windows) {
        // only PowerShell knows where its profile is, and it is asked as setup asks it
        for exe in ["powershell", "pwsh"] {
            if rant_process::find_program(exe, &std::env::temp_dir()).is_none() {
                continue;
            }
            let Some(profile) = rant_process::output(&powershell(exe, &["-Command", "$PROFILE"])) else { continue };
            let profile = profile.trim_end();
            if !profile.is_empty() {
                let line = powershell_line(rant_home);
                lines_in(&mut out, PathBuf::from(profile), move |l| l == line);
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_script_encodes_as_powershell_reads_it() {
        assert_eq!(encoded_command("exit 0"), "ZQB4AGkAdAAgADAA");
        assert_eq!(encoded_command("ab"), "YQBiAA==");
    }

    #[test]
    fn rant_lines_go_with_the_blank_line_before_them() {
        let line = "[ -f '/h/.rant/completions/rant.bash' ] && . '/h/.rant/completions/rant.bash'";
        let text = format!("alias ll='ls -l'\n\n{line}\n\nexport PATH=\"/h/.rant/bin:$PATH\"    # rant-cli\n");
        let drop = |l: &str| l == line || (l.starts_with("export PATH=") && l.ends_with("# rant-cli"));
        assert_eq!(without_lines(&text, &drop).as_deref(), Some("alias ll='ls -l'\n"));
        assert_eq!(without_lines("alias ll='ls -l'\n", &drop), None);
    }

    #[test]
    fn a_posix_hook_line_quotes_its_path() {
        assert_eq!(posix_line(Path::new("/h/it's/rant.bash")), r"[ -f '/h/it'\''s/rant.bash' ] && . '/h/it'\''s/rant.bash'");
    }
}
