//! The end to end harness: each test drives rant in a scratch folder, home and Rant domain of its
//! own, never the real home or the lab mesh. RANT_CLI names another rant to drive.
#![allow(dead_code)]

use std::fs;
use std::io::Read;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant, SystemTime};

use regex::{Regex, RegexBuilder};

/// What the package database names once read, so showing and adding a library reads no database.
pub const RANT_LIBRARY: &str = r#"{"name": "rant", "repo": "KosmosisDire/Rant", "library": {"cmake": "rant::rant_host", "python": "rant-middleware", "csharp": "Rant", "rust": "rant-middleware"}}"#;

pub struct Test {
    /// The working directory of the commands, emptied at the start.
    pub scratch: String,
    /// A second workspace beside it, for a node another workspace started.
    pub other: String,
    pub home: String,
    pub rant_home: String,
    pub domain: u16,
    env: Vec<(String, Option<String>)>,
}

pub struct Output {
    pub out: String,
    pub err: String,
    pub code: i32,
}

/// One rant run on the test's domain, ended by `ok`, `fails` or `any`.
pub struct Run<'a> {
    test: &'a Test,
    args: Vec<String>,
    dir: String,
    env: Vec<(String, String)>,
    domain: Option<u16>,
}

impl Test {
    pub fn new(name: &str, domain: u16) -> Test {
        let scratch = slashed(&std::env::temp_dir().join("rant-cli-e2e").join(name));
        let home = format!("{scratch}/home");
        let mut env = toolchain_homes();
        env.push(("HOME".into(), Some(home.clone())));
        env.push(("XDG_CONFIG_HOME".into(), Some(format!("{home}/.config"))));
        env.push(("RANT_HOME".into(), Some(format!("{home}/.rant"))));
        let test = Test { other: format!("{scratch}-other"), rant_home: format!("{home}/.rant"), home, scratch, domain, env };
        test.stop_leftovers();
        remove(&test.scratch);
        remove(&test.other);
        mkdir(&test.scratch);
        test.cache_database(&[]);
        write(&test.empty_stdin(), "");
        test
    }

    /// `rant --domain D <args>` in the scratch folder with an empty stdin that is no terminal.
    pub fn rant(&self, args: &[&str]) -> Run<'_> {
        Run { test: self, args: args.iter().map(|a| a.to_string()).collect(), dir: self.scratch.clone(), env: Vec::new(), domain: None }
    }

    /// Runs rant while a test node with `node_args` runs beside it for a few seconds. Both start
    /// together, so rant meets the node the way it meets any node that just came up.
    pub fn beside_node(&self, node_args: &[&str], args: &[&str]) -> Output {
        let shown = args.join(" ");
        let mut node = self.command(&test_node());
        node.args(["--domain", &self.domain.to_string(), "--life", "4"]).args(node_args);
        node.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
        let mut node = node.spawn().expect("the test node starts");
        let mut cli = self.command(&rant_cli());
        cli.arg("--domain").arg(self.domain.to_string()).args(args).current_dir(&self.scratch);
        cli.stdin(node.stdout.take().unwrap());
        let node_err = drain(node.stderr.take().unwrap());
        let mut run = capture(cli, Duration::from_secs(60), &shown);
        wait(&mut node, Duration::from_secs(60), "the test node");
        run.err += &received(node_err, "the test node");
        let run = run.with_plain_line_ends();
        if run.code != 0 {
            panic!("rant {shown} exited {}\nstdout:\n{}\nstderr:\n{}", run.code, run.out, run.err);
        }
        run
    }

    /// Runs rant beside a test node while others call it: once rant had two seconds to start, each
    /// of `callers` runs in turn, rant's words, or a second test node's after the word "node".
    pub fn while_calling(&self, node_args: &[&str], args: &[&str], callers: &[&[&str]]) -> Output {
        let shown = args.join(" ");
        let domain = self.domain.to_string();
        let mut node = self.command(&test_node());
        node.args(["--domain", &domain, "--life", "12"]).args(node_args);
        node.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::piped());
        let mut node = node.spawn().expect("the test node starts");
        let node_err = drain(node.stderr.take().unwrap());
        let mut cli = self.command(&rant_cli());
        cli.arg("--domain").arg(&domain).args(args).current_dir(&self.scratch);
        cli.stdin(fs::File::open(self.empty_stdin()).expect("the empty stdin opens")).stdout(Stdio::piped()).stderr(Stdio::piped());
        let mut child = cli.spawn().unwrap_or_else(|e| panic!("cannot start rant {shown}: {e}"));
        let (out, err) = (drain(child.stdout.take().unwrap()), drain(child.stderr.take().unwrap()));
        sleep(2.0);
        for words in callers {
            match words.split_first() {
                Some((&"node", rest)) => {
                    let mut caller = self.command(&test_node());
                    caller.args(["--domain", &domain, "--life", "1"]).args(rest).stdin(Stdio::null()).stdout(Stdio::null());
                    let mut caller = caller.spawn().expect("the calling test node starts");
                    wait(&mut caller, Duration::from_secs(30), "the calling test node");
                }
                _ => _ = self.rant(words).ok(),
            }
        }
        let code = wait(&mut child, Duration::from_secs(30), &shown);
        let _ = node.kill();
        wait(&mut node, Duration::from_secs(10), "the test node");
        let run = Output { out: received(out, &shown), err: received(err, &shown) + &received(node_err, "the test node"), code }.with_plain_line_ends();
        if run.code != 0 {
            panic!("rant {shown} exited {}\nstdout:\n{}\nstderr:\n{}", run.code, run.out, run.err);
        }
        run
    }

    /// The candidates `rant __complete` offers for the words and the word at the cursor.
    pub fn complete(&self, words: &[&str], cur: &str) -> String {
        let mut cli = self.command(&rant_cli());
        cli.arg("__complete").arg(format!("--cur={cur}")).arg("--domain").arg(self.domain.to_string());
        cli.args(words).current_dir(&self.scratch).stdin(Stdio::null());
        let run = capture(cli, Duration::from_secs(30), "__complete");
        if run.code != 0 {
            panic!("__complete exited {}\n{}", run.code, run.err);
        }
        run.out
    }

    /// Runs rant as a boot or login entry does: no domain, nothing read back, any exit code.
    /// It works outside the scratch folder, which a process it leaves running would hold.
    pub fn rant_unwatched(&self, args: &[&str]) {
        let mut cli = self.command(&rant_cli());
        cli.args(args).current_dir(std::env::temp_dir()).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
        let mut child = cli.spawn().expect("rant starts");
        wait(&mut child, Duration::from_secs(60), &args.join(" "));
    }

    /// Sets a variable for every command the test runs from here on.
    pub fn set_env(&mut self, key: &str, value: &str) {
        self.env.push((key.into(), Some(value.into())));
    }

    pub fn unset_env(&mut self, key: &str) {
        self.env.push((key.into(), None));
    }

    /// A path inside the scratch folder.
    pub fn path(&self, relative: &str) -> String {
        format!("{}/{relative}", self.scratch)
    }

    /// Copies tests/fixtures/<name> into the scratch folder, without the state a run left.
    pub fn use_fixture(&self, name: &str) {
        let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures").join(name);
        copy_dir(&fixture, Path::new(&self.scratch), &|p| p.file_name().is_some_and(|n| n == ".rant"));
    }

    /// Copies the test node into the scratch folder as <dir>/<name>, with .exe on Windows.
    pub fn install_test_node(&self, dir: &str, name: &str) {
        mkdir(&format!("{}/{dir}", self.scratch));
        copy_file(&test_node(), &format!("{}/{dir}/{name}{}", self.scratch, std::env::consts::EXE_SUFFIX));
    }

    /// Writes the cache of the package database as `rant` keeps it once read.
    pub fn cache_database(&self, entries: &[&str]) {
        write(&format!("{}/cache/sources/KosmosisDire+rant-packages.json", self.rant_home), &format!("[{}]\n", entries.join(", ")));
    }

    /// Waits up to 30 s for the service process's log to match, and returns the log.
    pub fn wait_for_service_log(&self, regex: &str) -> String {
        let log = format!("{}/logs/services.log", self.rant_home);
        let re = pattern(regex);
        let mut text = String::new();
        for _ in 0..=60 {
            text = fs::read_to_string(&log).unwrap_or_default();
            if re.is_match(&text) {
                return text;
            }
            sleep(0.5);
        }
        panic!("the service process's log never matched {regex}:\n{text}");
    }

    fn command(&self, program: &str) -> Command {
        let mut cmd = Command::new(program);
        for (key, value) in &self.env {
            match value {
                Some(v) => cmd.env(key, v),
                None => cmd.env_remove(key),
            };
        }
        cmd
    }

    fn empty_stdin(&self) -> String {
        format!("{}/.empty-stdin", self.scratch)
    }

    /// Stops what a failed run left in its scratch workspaces and home, since a running
    /// program cannot be replaced, and waits for a service process left behind to end.
    fn stop_leftovers(&self) {
        let mut states = Vec::new();
        find_states(Path::new(&self.scratch), &mut states);
        let other_state = Path::new(&self.other).join(".rant/state");
        if other_state.is_file() {
            states.push(other_state);
        }
        for state in states {
            let workspace = state.parent().and_then(Path::parent).unwrap();
            let Ok(text) = fs::read_to_string(&state) else { continue };
            let Ok(json) = serde_json::from_str::<serde_json::Value>(&text) else { continue };
            for instance in json["instances"].as_array().into_iter().flatten() {
                let Some(name) = instance["name"].as_str() else { continue };
                let mut cli = self.command(&rant_cli());
                cli.args(["stop", name, "-y"]).current_dir(workspace);
                cli.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
                if let Ok(mut child) = cli.spawn() {
                    wait(&mut child, Duration::from_secs(30), "the leftover stop");
                }
            }
        }
        let log = format!("{}/logs/services.log", self.rant_home);
        for _ in 0..=40 {
            match fs::read_to_string(&log) {
                Ok(text) if !text.ends_with("nothing to watch\n") => sleep(0.5),
                _ => break,
            }
        }
    }
}

impl Run<'_> {
    /// Runs in another folder than the scratch one.
    pub fn cwd(mut self, dir: &str) -> Self {
        self.dir = dir.into();
        self
    }

    /// Sets a variable for this run only.
    pub fn env(mut self, key: &str, value: &str) -> Self {
        self.env.push((key.into(), value.into()));
        self
    }

    /// Gives --domain d in place of the test's own, and the test nodes it starts take theirs
    /// from Rant, as a node that leaves the default does.
    pub fn on_domain(mut self, d: u16) -> Self {
        self.domain = Some(d);
        self
    }

    /// Expects exit code 0.
    pub fn ok(self) -> Output {
        let shown = self.args.join(" ");
        let run = self.go();
        if run.code != 0 {
            panic!("rant {shown} exited {}\nstdout:\n{}\nstderr:\n{}", run.code, run.out, run.err);
        }
        run
    }

    /// Expects a nonzero exit code.
    pub fn fails(self) -> Output {
        let shown = self.args.join(" ");
        let run = self.go();
        if run.code == 0 {
            panic!("rant {shown} should have failed\nstdout:\n{}\nstderr:\n{}", run.out, run.err);
        }
        run
    }

    /// Takes any exit code.
    pub fn any(self) -> Output {
        self.go()
    }

    fn go(self) -> Output {
        let t = self.test;
        let mut cli = t.command(&rant_cli());
        cli.env("TEST_NODE_DOMAIN", self.domain.map_or_else(|| t.domain.to_string(), |_| String::new()));
        cli.envs(self.env);
        cli.arg("--domain").arg(self.domain.unwrap_or(t.domain).to_string()).args(&self.args).current_dir(&self.dir);
        cli.stdin(fs::File::open(t.empty_stdin()).expect("the empty stdin opens"));
        capture(cli, Duration::from_secs(300), &self.args.join(" ")).with_plain_line_ends()
    }
}

impl Output {
    /// Windows line ends made plain.
    fn with_plain_line_ends(self) -> Output {
        Output { out: self.out.replace("\r\n", "\n"), err: self.err.replace("\r\n", "\n"), code: self.code }
    }
}

/// The rant under test: RANT_CLI, else the one this package builds.
pub fn rant_cli() -> String {
    std::env::var("RANT_CLI").unwrap_or_else(|_| env!("CARGO_BIN_EXE_rant").into())
}

pub fn test_node() -> String {
    env!("CARGO_BIN_EXE_test_node").into()
}

/// The GUI test program, which exists on Windows only.
pub fn test_window() -> Option<String> {
    cfg!(windows).then(|| env!("CARGO_BIN_EXE_test_window").into())
}

/// The absolute path of a program on PATH, with forward slashes.
pub fn which(program: &str) -> Option<String> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|dir| dir.join(format!("{program}{}", std::env::consts::EXE_SUFFIX)))
        .find(|p| p.is_file())
        .map(|p| slashed(&p))
}

pub fn cmake() -> String {
    which("cmake").expect("cmake is on PATH")
}

pub fn expect_match(text: &str, regex: &str) {
    if !pattern(regex).is_match(text) {
        panic!("expected a match for: {regex}\nin:\n{text}");
    }
}

pub fn expect_no_match(text: &str, regex: &str) {
    if pattern(regex).is_match(text) {
        panic!("expected no match for: {regex}\nin:\n{text}");
    }
}

/// The first group of the first match, or None.
pub fn capture_group(text: &str, regex: &str) -> Option<String> {
    pattern(regex).captures(text).and_then(|c| c.get(1)).map(|m| m.as_str().to_string())
}

pub fn count_matches(text: &str, regex: &str) -> usize {
    pattern(regex).find_iter(text).count()
}

/// A regex in which . matches a line end too.
pub fn pattern(regex: &str) -> Regex {
    RegexBuilder::new(regex).dot_matches_new_line(true).build().unwrap_or_else(|e| panic!("bad regex {regex}: {e}"))
}

/// Writes a file, making the folders it needs.
pub fn write(path: &str, text: &str) {
    if let Some(parent) = Path::new(path).parent() {
        mkdir(&slashed(parent));
    }
    fs::write(path, text).unwrap_or_else(|e| panic!("cannot write {path}: {e}"));
}

pub fn append(path: &str, text: &str) {
    let mut all = fs::read_to_string(path).unwrap_or_default();
    all += text;
    write(path, &all);
}

pub fn read(path: &str) -> String {
    fs::read_to_string(path).unwrap_or_else(|e| panic!("cannot read {path}: {e}"))
}

pub fn mkdir(path: &str) {
    fs::create_dir_all(path).unwrap_or_else(|e| panic!("cannot make {path}: {e}"));
}

pub fn exists(path: &str) -> bool {
    Path::new(path).exists()
}

/// Removes a file or a folder with all in it, never following a link out of it.
pub fn remove(path: &str) {
    let result = match fs::symlink_metadata(path) {
        Ok(m) if m.is_dir() => fs::remove_dir_all(path),
        Ok(_) => fs::remove_file(path).or_else(|_| fs::remove_dir(path)),
        Err(_) => return,
    };
    result.unwrap_or_else(|e| panic!("cannot remove {path}: {e}"));
}

/// The names in a folder, none if it is missing.
pub fn list(dir: &str) -> Vec<String> {
    fs::read_dir(dir)
        .map(|entries| entries.filter_map(Result::ok).map(|e| e.file_name().to_string_lossy().into_owned()).collect())
        .unwrap_or_default()
}

/// Copies a folder into another folder under its own name, as CMake's file(COPY) does.
pub fn copy_into(dir: &str, destination: &str) {
    let name = Path::new(dir).file_name().unwrap();
    copy_dir(Path::new(dir), &Path::new(destination).join(name), &|_| false);
}

pub fn copy_file(from: &str, to: &str) {
    fs::copy(from, to).unwrap_or_else(|e| panic!("cannot copy {from} to {to}: {e}"));
}

/// Lets the owner run a script, which Windows needs no mark for.
pub fn make_executable(path: &str) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700)).unwrap_or_else(|e| panic!("cannot mark {path}: {e}"));
    }
    let _ = path;
}

/// Marks a file changed now.
pub fn touch(path: &str) {
    let file = fs::File::options().write(true).open(path).unwrap_or_else(|e| panic!("cannot open {path}: {e}"));
    file.set_modified(SystemTime::now()).unwrap_or_else(|e| panic!("cannot touch {path}: {e}"));
}

/// Packs `files` of `dir` with cmake -E tar, `flags` such as czf, `format` such as zip.
pub fn tar(dir: &str, flags: &str, archive: &str, files: &[&str], format: Option<&str>) {
    let mut cmd = Command::new(cmake());
    cmd.args(["-E", "tar", flags, archive]);
    if let Some(format) = format {
        cmd.arg(format!("--format={format}"));
    }
    let status = cmd.args(files).current_dir(dir).status().expect("cmake starts");
    assert!(status.success(), "cannot pack {archive}");
}

pub fn sleep(seconds: f64) {
    thread::sleep(Duration::from_secs_f64(seconds));
}

pub fn slashed(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

/// Toolchains keep their own homes when HOME moves, which on Windows they find without it.
fn toolchain_homes() -> Vec<(String, Option<String>)> {
    if !cfg!(unix) {
        return Vec::new();
    }
    let home = std::env::var("HOME").unwrap_or_default();
    [("CARGO_HOME", ".cargo"), ("RUSTUP_HOME", ".rustup"), ("NUGET_PACKAGES", ".nuget/packages"), ("DOTNET_CLI_HOME", "."), ("PYTHONUSERBASE", ".local")]
        .into_iter()
        .filter(|(key, _)| std::env::var_os(key).is_none())
        .map(|(key, dir)| (key.to_string(), Some(format!("{home}/{dir}"))))
        .collect()
}

fn find_states(dir: &Path, states: &mut Vec<std::path::PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else { return };
    for entry in entries.filter_map(Result::ok) {
        let path = entry.path();
        let Ok(kind) = entry.file_type() else { continue };
        if kind.is_symlink() || !kind.is_dir() {
            continue;
        }
        if path.file_name().is_some_and(|n| n == ".rant") && path.join("state").is_file() {
            states.push(path.join("state"));
        }
        find_states(&path, states);
    }
}

fn copy_dir(from: &Path, to: &Path, skip: &dyn Fn(&Path) -> bool) {
    fs::create_dir_all(to).unwrap_or_else(|e| panic!("cannot make {}: {e}", to.display()));
    for entry in fs::read_dir(from).unwrap_or_else(|e| panic!("cannot read {}: {e}", from.display())) {
        let path = entry.unwrap().path();
        if skip(&path) {
            continue;
        }
        let target = to.join(path.file_name().unwrap());
        if path.is_dir() {
            copy_dir(&path, &target, skip);
        } else {
            fs::copy(&path, &target).unwrap_or_else(|e| panic!("cannot copy {}: {e}", path.display()));
        }
    }
}

/// Runs a command to its end and reads what it printed.
fn capture(mut cmd: Command, timeout: Duration, shown: &str) -> Output {
    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = cmd.spawn().unwrap_or_else(|e| panic!("cannot start rant {shown}: {e}"));
    let out = drain(child.stdout.take().unwrap());
    let err = drain(child.stderr.take().unwrap());
    let code = wait(&mut child, timeout, shown);
    Output { out: received(out, shown), err: received(err, shown), code }
}

fn drain(mut pipe: impl Read + Send + 'static) -> mpsc::Receiver<String> {
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let mut bytes = Vec::new();
        let _ = pipe.read_to_end(&mut bytes);
        let _ = tx.send(String::from_utf8_lossy(&bytes).into_owned());
    });
    rx
}

/// What a pipe carried once its writers are gone. A program left holding it open is a bug.
fn received(rx: mpsc::Receiver<String>, shown: &str) -> String {
    rx.recv_timeout(Duration::from_secs(10))
        .unwrap_or_else(|_| panic!("rant {shown} ended, but a program it started holds its output open"))
}

fn wait(child: &mut Child, timeout: Duration, shown: &str) -> i32 {
    let end = Instant::now() + timeout;
    loop {
        if let Some(status) = child.try_wait().expect("the child can be waited on") {
            return status.code().unwrap_or(-1);
        }
        if Instant::now() > end {
            let _ = child.kill();
            panic!("{shown} ran longer than {} s", timeout.as_secs());
        }
        thread::sleep(Duration::from_millis(20));
    }
}
