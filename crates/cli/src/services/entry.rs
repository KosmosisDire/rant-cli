//! What starts the service process of a rant home at boot, which takes admin once, or at login.
//! Each entry names the rant and home that wrote it, and a rant changes only its own.

use std::path::Path;

pub use os::{boot_place, present, remove, write};

use super::When;
use crate::failure::Result;

/// A path as a service definition reads it, with the separators of this OS.
fn native(p: &Path) -> String {
    p.to_string_lossy().into_owned()
}

/// What an entry runs: `rant __services --boot HOME` or `--login HOME`.
pub fn argv(when: When, rant: &Path, home: &Path) -> Vec<String> {
    vec![native(rant), "__services".into(), format!("--{}", when.word()), native(home)]
}

fn mine(when: When, home: &Path) -> Result<Vec<String>> {
    Ok(argv(when, &crate::home::program()?, home))
}

/// A word as systemd reads it inside ExecStart: quoted, with its own escapes and % and $ doubled.
#[cfg(any(all(unix, not(target_os = "macos")), test))]
fn systemd_word(s: &str) -> String {
    let mut out = String::from("\"");
    for c in s.chars() {
        if c == '\\' || c == '"' {
            out.push('\\');
        }
        if c == '%' || c == '$' {
            out.push(c);
        }
        out.push(c);
    }
    out + "\""
}

#[cfg(any(all(unix, not(target_os = "macos")), test))]
fn exec_line(argv: &[String]) -> String {
    argv.iter().map(|a| systemd_word(a)).collect::<Vec<_>>().join(" ")
}

/// The command only starts what is enabled and a service process, then ends. What it started
/// keeps running when the unit stops.
#[cfg(any(all(unix, not(target_os = "macos")), test))]
fn systemd_user_unit(argv: &[String]) -> String {
    let exec = exec_line(argv);
    format!(
        r#"[Unit]
Description=Rant service process

[Service]
Type=oneshot
RemainAfterExit=yes
ExecStart={exec}
KillMode=process

[Install]
WantedBy=default.target
"#
    )
}

#[cfg(any(all(unix, not(target_os = "macos")), test))]
fn systemd_system_unit(argv: &[String], user: &str) -> String {
    let exec = exec_line(argv);
    format!(
        r#"[Unit]
Description=Rant service process of {user}
Wants=network-online.target
After=network-online.target

[Service]
Type=oneshot
RemainAfterExit=yes
User={user}
ExecStart={exec}
KillMode=process

[Install]
WantedBy=multi-user.target
"#
    )
}

#[cfg(any(windows, target_os = "macos", test))]
fn xml_text(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

/// It ends when it has nothing to watch, so launchd neither keeps it alive nor takes its nodes.
#[cfg(any(target_os = "macos", test))]
fn launchd_plist(argv: &[String], label: &str, user: &str, log: &Path) -> String {
    let s = |v: &str| format!("<string>{}</string>", xml_text(v));
    let args: String = argv.iter().map(|a| format!("    {}\n", s(a))).collect();
    let user = if user.is_empty() { String::new() } else { format!("  <key>UserName</key>\n  {}\n", s(user)) };
    let (label, log) = (s(label), s(&native(log)));
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key>
  {label}
{user}  <key>ProgramArguments</key>
  <array>
{args}  </array>
  <key>RunAtLoad</key>
  <true/>
  <key>AbandonProcessGroup</key>
  <true/>
  <key>StandardOutPath</key>
  {log}
  <key>StandardErrorPath</key>
  {log}
</dict>
</plist>
"#
    )
}

/// Runs as the user without a stored password, never stopped for time or battery, at normal
/// priority, and anyone signed in may read it, so rant checks it without admin.
#[cfg(any(windows, test))]
fn task_xml(argv: &[String], user_sid: &str) -> String {
    let command = xml_text(&argv[0]);
    let args = xml_text(&rant_process::quote::windows_command_line(&argv[1..]));
    format!(
        r#"<?xml version="1.0" encoding="UTF-16"?>
<Task version="1.2" xmlns="http://schemas.microsoft.com/windows/2004/02/mit/task">
  <RegistrationInfo>
    <Description>Starts what rant enable named at boot</Description>
    <SecurityDescriptor>D:(A;;FA;;;BA)(A;;FA;;;SY)(A;;FR;;;AU)</SecurityDescriptor>
  </RegistrationInfo>
  <Triggers>
    <BootTrigger>
      <Enabled>true</Enabled>
    </BootTrigger>
  </Triggers>
  <Principals>
    <Principal id="Author">
      <UserId>{user_sid}</UserId>
      <LogonType>S4U</LogonType>
      <RunLevel>LeastPrivilege</RunLevel>
    </Principal>
  </Principals>
  <Settings>
    <MultipleInstancesPolicy>IgnoreNew</MultipleInstancesPolicy>
    <DisallowStartIfOnBatteries>false</DisallowStartIfOnBatteries>
    <StopIfGoingOnBatteries>false</StopIfGoingOnBatteries>
    <ExecutionTimeLimit>PT0S</ExecutionTimeLimit>
    <Priority>5</Priority>
  </Settings>
  <Actions Context="Author">
    <Exec>
      <Command>{command}</Command>
      <Arguments>{args}</Arguments>
    </Exec>
  </Actions>
</Task>
"#
    )
}

/// A scheduled task running as the user at boot, and a Run value at login.
#[cfg(windows)]
mod os {
    use std::path::{Path, PathBuf};

    use rant_process::Command;
    use rant_process::quote::windows_command_line;
    use windows_sys::Win32::System::Registry::REG_SZ;

    use super::{When, mine, native, task_xml};
    use crate::failure::{Result, failed};
    use crate::registry;

    const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
    const RUN_NAME: &str = "Rant";

    fn user_name() -> String {
        use windows_sys::Win32::System::WindowsProgramming::GetUserNameW;
        let mut buf = [0u16; 257];
        let mut n = buf.len() as u32;
        // SAFETY: n says how much buf holds, and on success counts the NUL written after the name.
        if unsafe { GetUserNameW(buf.as_mut_ptr(), &mut n) } == 0 {
            return "user".into();
        }
        String::from_utf16_lossy(&buf[..n.saturating_sub(1) as usize])
    }

    fn task_name() -> String {
        format!("Rant {}", user_name())
    }

    pub fn boot_place() -> String {
        format!("the scheduled task {}", task_name())
    }

    fn schtasks(args: &[&str]) -> Command {
        let program: PathBuf = rant_process::system_dir().join("schtasks.exe");
        Command::new(std::iter::once(native(&program)).chain(args.iter().map(|a| a.to_string())), std::env::temp_dir())
    }

    /// XML as schtasks prints it, its escapes undone.
    fn unescaped(s: &str) -> String {
        [("&quot;", "\""), ("&apos;", "'"), ("&lt;", "<"), ("&gt;", ">"), ("&amp;", "&")].iter().fold(s.to_string(), |s, (from, to)| s.replace(from, to))
    }

    /// The entry exists and names this rant and this home.
    pub fn present(when: When, home: &Path) -> bool {
        let Ok(a) = mine(when, home) else { return false };
        if when == When::Login {
            return registry::read(RUN_KEY, RUN_NAME).is_some_and(|(v, _)| v == windows_command_line(&a));
        }
        let Some(xml) = rant_process::output(&schtasks(&["/query", "/tn", &task_name(), "/xml", "ONE"])) else { return false };
        let text = unescaped(&xml);
        text.contains(&format!("<Command>{}</Command>", a[0])) && text.contains(&format!("<Arguments>{}</Arguments>", windows_command_line(&a[1..])))
    }

    /// Writes the entry for this rant and home, over one another wrote. The boot entry asks for
    /// admin.
    pub fn write(when: When, home: &Path) -> Result<()> {
        if when == When::Login {
            if !registry::write(RUN_KEY, RUN_NAME, &windows_command_line(&mine(when, home)?), REG_SZ) {
                return Err(failed("cannot write the Run key of your user, so nothing can start at login"));
            }
            return Ok(());
        }
        if present(when, home) {
            return Ok(());
        }
        let sid = rant_process::user_sid().ok_or_else(|| failed("cannot read who you are from Windows"))?;
        // schtasks reads a task as UTF-16
        let text = task_xml(&mine(when, home)?, &sid);
        let bytes: Vec<u8> = [0xFF, 0xFE].into_iter().chain(text.encode_utf16().flat_map(u16::to_le_bytes)).collect();
        let file = std::env::temp_dir().join(format!("rant-boot-{}.xml", std::process::id()));
        std::fs::write(&file, bytes).map_err(|e| failed(format!("cannot write {}: {e}", native(&file))))?;
        let code = rant_process::run_elevated(&schtasks(&["/create", "/tn", &task_name(), "/xml", &native(&file), "/f"]));
        let _ = std::fs::remove_file(&file);
        if code? != 0 || !present(when, home) {
            return Err(failed(format!("Windows did not take {}, so nothing starts at boot", boot_place())));
        }
        Ok(())
    }

    /// Removes the entry when it names this rant and home. The boot entry asks for admin.
    pub fn remove(when: When, home: &Path) -> Result<()> {
        if !present(when, home) {
            return Ok(());
        }
        if when == When::Login {
            registry::delete(RUN_KEY, RUN_NAME);
            return Ok(());
        }
        rant_process::run_elevated(&schtasks(&["/delete", "/tn", &task_name(), "/f"]))?;
        Ok(())
    }
}

/// A system unit at boot and a user unit at login on Linux, a launch daemon and a launch agent
/// on macOS, each running as the user.
#[cfg(unix)]
mod os {
    use std::path::{Path, PathBuf};

    use rant_process::Command;

    use super::{When, mine, native};
    use crate::failure::{Result, failed};
    use crate::home::{env_path, home_dir};

    const MAC: bool = cfg!(target_os = "macos");

    fn user_name() -> Result<String> {
        if let Some(v) = ["USER", "LOGNAME"].iter().find_map(|n| std::env::var(n).ok().filter(|v| !v.is_empty())) {
            return Ok(v);
        }
        let id = rant_process::output(&Command::new(["id", "-un"], std::env::temp_dir()));
        id.map(|o| o.trim_end().to_string()).filter(|o| !o.is_empty()).ok_or_else(|| failed("cannot tell who you are, set USER"))
    }

    /// The launch daemon's label, or the system unit.
    fn boot_name() -> Result<String> {
        let user = user_name()?;
        Ok(if MAC { format!("dev.rantlib.rant.{user}") } else { format!("rant-{user}.service") })
    }

    fn user_home() -> Result<PathBuf> {
        home_dir().ok_or_else(|| failed("you have no home folder, set HOME"))
    }

    fn file(when: When) -> Result<PathBuf> {
        Ok(match when {
            When::Boot if MAC => Path::new("/Library/LaunchDaemons").join(format!("{}.plist", boot_name()?)),
            When::Boot => Path::new("/etc/systemd/system").join(boot_name()?),
            When::Login if MAC => user_home()?.join("Library/LaunchAgents/dev.rantlib.rant.plist"),
            When::Login => env_path("XDG_CONFIG_HOME").map_or_else(|| user_home().map(|h| h.join(".config")), Ok)?.join("systemd/user/rant.service"),
        })
    }

    #[cfg(target_os = "macos")]
    fn text(when: When, home: &Path) -> Result<String> {
        let (label, user) = match when {
            When::Boot => (boot_name()?, user_name()?),
            When::Login => ("dev.rantlib.rant".to_string(), String::new()),
        };
        Ok(super::launchd_plist(&mine(when, home)?, &label, &user, &crate::services::log_file(home)))
    }

    #[cfg(not(target_os = "macos"))]
    fn text(when: When, home: &Path) -> Result<String> {
        let argv = mine(when, home)?;
        Ok(match when {
            When::Boot => super::systemd_system_unit(&argv, &user_name()?),
            When::Login => super::systemd_user_unit(&argv),
        })
    }

    pub fn boot_place() -> String {
        let place = if MAC { file(When::Boot).map(|f| native(&f)) } else { boot_name() };
        format!("the {} {}", if MAC { "launch daemon" } else { "system unit" }, place.unwrap_or_default())
    }

    /// The entry exists and names this rant and this home.
    pub fn present(when: When, home: &Path) -> bool {
        match (file(when), text(when, home)) {
            (Ok(f), Ok(t)) => std::fs::read_to_string(f).is_ok_and(|had| had == t),
            _ => false,
        }
    }

    fn agent() -> String {
        // SAFETY: no arguments.
        format!("gui/{}/dev.rantlib.rant", unsafe { libc::getuid() })
    }

    /// A service manager command. A failure is the user's to read, so its output stays on screen.
    fn manage(argv: &[&str], must: bool) -> Result<()> {
        if rant_process::run(&Command::new(argv.iter().copied(), std::env::temp_dir()))? != 0 && must {
            return Err(failed(format!("`{}` failed, see what it said above", argv.join(" "))));
        }
        Ok(())
    }

    /// The boot entry goes in place as root in one sudo, so the password is asked once.
    fn write_boot(home: &Path) -> Result<()> {
        let tmp = std::env::temp_dir().join(format!("rant-boot-{}", std::process::id()));
        std::fs::write(&tmp, text(When::Boot, home)?).map_err(|e| failed(format!("cannot write {}: {e}", native(&tmp))))?;
        let script = if MAC { r#"install -m 644 "$1" "$2""# } else { r#"install -m 644 "$1" "$2" && systemctl daemon-reload && systemctl enable "$3""# };
        let (target, name) = (native(&file(When::Boot)?), boot_name()?);
        let code = rant_process::run_elevated(&Command::new(["sh", "-c", script, "rant", &native(&tmp), &target, &name], std::env::temp_dir()));
        let _ = std::fs::remove_file(&tmp);
        if code? != 0 || !present(When::Boot, home) {
            return Err(failed(format!("cannot add {}, so nothing starts at boot", boot_place())));
        }
        Ok(())
    }

    /// Writes the entry for this rant and home, over one another wrote. The boot entry asks for
    /// admin.
    pub fn write(when: When, home: &Path) -> Result<()> {
        if present(when, home) {
            return Ok(());
        }
        if when == When::Boot {
            return write_boot(home);
        }
        let f = file(when)?;
        if let Some(dir) = f.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let _ = std::fs::write(&f, text(when, home)?);
        if !present(when, home) {
            return Err(failed(format!("cannot write {}, so nothing can start at login", native(&f))));
        }
        if MAC {
            manage(&["launchctl", "bootout", &agent()], false)?;
            // SAFETY: no arguments.
            manage(&["launchctl", "bootstrap", &format!("gui/{}", unsafe { libc::getuid() }), &native(&f)], true)
        } else {
            manage(&["systemctl", "--user", "daemon-reload"], true)?;
            manage(&["systemctl", "--user", "enable", "rant.service"], true)
        }
    }

    /// Removes the entry when it names this rant and home. The boot entry asks for admin.
    pub fn remove(when: When, home: &Path) -> Result<()> {
        if !present(when, home) {
            return Ok(());
        }
        if when == When::Boot {
            let (name, target) = (boot_name()?, native(&file(when)?));
            let script = if MAC { r#"launchctl bootout system/"$1"; rm -f "$2""# } else { r#"systemctl disable "$1"; rm -f "$2"; systemctl daemon-reload"# };
            rant_process::run_elevated(&Command::new(["sh", "-c", script, "rant", &name, &target], std::env::temp_dir()))?;
            return Ok(());
        }
        if MAC {
            manage(&["launchctl", "bootout", &agent()], false)?;
        } else {
            manage(&["systemctl", "--user", "disable", "rant.service"], false)?;
        }
        let _ = std::fs::remove_file(file(when)?);
        if !MAC {
            manage(&["systemctl", "--user", "daemon-reload"], false)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_windows_login_entry_names_the_rant_and_its_home() {
        let a = argv(When::Login, Path::new(r"C:\Users\a b\.rant\bin\rant.exe"), Path::new(r"C:\Users\a b\.rant"));
        assert_eq!(rant_process::quote::windows_command_line(&a), r#""C:\Users\a b\.rant\bin\rant.exe" __services --login "C:\Users\a b\.rant""#);
    }

    #[test]
    fn the_windows_boot_task_runs_as_the_user_at_boot_without_a_time_limit() {
        let xml = task_xml(&argv(When::Boot, Path::new(r"C:\r&d\rant.exe"), Path::new(r"C:\Users\a b\.rant")), "S-1-5-21-1-2-3-1001");
        for part in [
            "<BootTrigger>",
            "<UserId>S-1-5-21-1-2-3-1001</UserId>",
            "<LogonType>S4U</LogonType>",
            "<ExecutionTimeLimit>PT0S</ExecutionTimeLimit>",
            "<StopIfGoingOnBatteries>false</StopIfGoingOnBatteries>",
            r"<Command>C:\r&amp;d\rant.exe</Command>",
            r#"<Arguments>__services --boot "C:\Users\a b\.rant"</Arguments>"#,
            "(A;;FR;;;AU)",
        ] {
            assert!(xml.contains(part), "{part} in {xml}");
        }
    }

    #[test]
    fn the_systemd_boot_unit_runs_as_the_user_after_the_network() {
        let unit = systemd_system_unit(&argv(When::Boot, Path::new("/home/a/.rant/bin/rant"), Path::new("/home/a/.rant")), "a");
        let want = "[Unit]\nDescription=Rant service process of a\nWants=network-online.target\nAfter=network-online.target\n\n\
                    [Service]\nType=oneshot\nRemainAfterExit=yes\nUser=a\n\
                    ExecStart=\"/home/a/.rant/bin/rant\" \"__services\" \"--boot\" \"/home/a/.rant\"\nKillMode=process\n\n\
                    [Install]\nWantedBy=multi-user.target\n";
        assert_eq!(unit, want);
    }

    #[test]
    fn the_launchd_boot_daemon_runs_as_the_user() {
        let plist = launchd_plist(&argv(When::Boot, Path::new("/Users/a/rant"), Path::new("/Users/a/.rant")), "dev.rantlib.rant.a", "a", Path::new("/l.log"));
        assert!(plist.contains("<key>UserName</key>\n  <string>a</string>\n"), "{plist}");
        assert!(plist.contains("<string>--boot</string>"), "{plist}");
    }

    #[test]
    fn the_systemd_login_unit_leaves_what_it_started() {
        let unit = systemd_user_unit(&argv(When::Login, Path::new("/home/a/.rant/bin/rant"), Path::new("/home/a/.rant")));
        let want = "[Unit]\nDescription=Rant service process\n\n[Service]\nType=oneshot\nRemainAfterExit=yes\n\
                    ExecStart=\"/home/a/.rant/bin/rant\" \"__services\" \"--login\" \"/home/a/.rant\"\nKillMode=process\n\n\
                    [Install]\nWantedBy=default.target\n";
        assert_eq!(unit, want);
    }

    #[test]
    fn the_systemd_unit_escapes_what_systemd_expands() {
        let unit = systemd_user_unit(&argv(When::Login, Path::new("/home/a b/50%/rant"), Path::new("/home/$x/.rant")));
        assert!(unit.contains("ExecStart=\"/home/a b/50%%/rant\" \"__services\" \"--login\" \"/home/$$x/.rant\"\n"), "{unit}");
    }

    #[test]
    fn the_launchd_agent_starts_once_and_leaves_the_nodes() {
        let a = argv(When::Login, Path::new("/Users/a/.rant/bin/rant"), Path::new("/Users/a/.rant"));
        let want = r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key>
  <string>dev.rantlib.rant</string>
  <key>ProgramArguments</key>
  <array>
    <string>/Users/a/.rant/bin/rant</string>
    <string>__services</string>
    <string>--login</string>
    <string>/Users/a/.rant</string>
  </array>
  <key>RunAtLoad</key>
  <true/>
  <key>AbandonProcessGroup</key>
  <true/>
  <key>StandardOutPath</key>
  <string>/Users/a/.rant/logs/services.log</string>
  <key>StandardErrorPath</key>
  <string>/Users/a/.rant/logs/services.log</string>
</dict>
</plist>
"#;
        assert_eq!(launchd_plist(&a, "dev.rantlib.rant", "", Path::new("/Users/a/.rant/logs/services.log")), want);
    }

    #[test]
    fn the_launchd_agent_escapes_xml() {
        let plist = launchd_plist(&argv(When::Login, Path::new("/Users/a&b/rant"), Path::new("/h")), "dev.rantlib.rant", "", Path::new("/l.log"));
        assert!(plist.contains("<string>/Users/a&amp;b/rant</string>"), "{plist}");
    }

    /// Only reads: the tests run with another home, so no real entry names them.
    #[test]
    fn no_real_entry_names_a_test() {
        let home = std::env::temp_dir().join("rant-unit-entry-home");
        assert!(!present(When::Login, &home));
        assert!(!present(When::Boot, &home));
    }
}
