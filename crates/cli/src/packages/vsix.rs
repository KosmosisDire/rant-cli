//! VS Code extensions: the file a release carries, and the editor and profile it installs into
//! through the editor's command line, `code` or another such as `cursor`.

use std::path::{Path, PathBuf};

use rant_config::paths;
use rant_process::Command;
use serde_json::Value;

use super::zip;
use crate::context::Context;
use crate::failure::{Result, failed};
use crate::home::{env_path, home_dir};
use crate::prompt;

/// What an extension's extension/package.json names it.
#[derive(Debug, Clone, PartialEq)]
pub struct Extension {
    /// publisher.name in lowercase, as the editor's command line names it.
    pub id: String,
    pub version: String,
}

pub fn read(path: &Path) -> Result<Extension> {
    let bytes = std::fs::read(path).map_err(|e| failed(format!("cannot read {}: {e}", paths::slash(path))))?;
    let not = || failed(format!("{} is not a VS Code extension", paths::slash(path)));
    let manifest = zip::entry(&bytes, "extension/package.json").ok_or_else(not)?;
    let json: Value = serde_json::from_slice(&manifest).map_err(|_| not())?;
    let field = |key: &str| json[key].as_str().filter(|s| !s.is_empty());
    match (field("publisher"), field("name"), field("version")) {
        (Some(publisher), Some(name), Some(version)) => Ok(Extension { id: format!("{publisher}.{name}").to_lowercase(), version: version.to_string() }),
        _ => Err(not()),
    }
}

/// The editor and profile an extension goes into. An empty profile is the default.
#[derive(Debug, Clone, PartialEq)]
pub struct Target {
    pub editor: PathBuf,
    pub profile: String,
}

/// The editor and profile a user named, each None for the default.
#[derive(Debug, Clone, Default)]
pub struct Wanted {
    pub editor: Option<String>,
    pub profile: Option<String>,
}

impl Wanted {
    pub fn any(&self) -> bool {
        self.editor.is_some() || self.profile.is_some()
    }
}

pub fn shown(t: &Target) -> String {
    let profile = if t.profile.is_empty() { String::new() } else { format!(" in the profile {}", t.profile) };
    format!("{}{profile}", paths::slash(&t.editor))
}

/// The version `--list-extensions --show-versions` gives the extension, empty when it lists none.
pub fn version_in(listing: &str, id: &str) -> String {
    let prefix = format!("{}@", id.to_lowercase());
    let found = listing.lines().map(str::trim_end).find(|l| l.len() >= prefix.len() && l[..prefix.len()].to_lowercase() == prefix);
    found.map(|l| l[prefix.len()..].to_string()).unwrap_or_default()
}

/// A profile the editor keeps: its id, the folder name under profiles, and whether it shares the
/// default's extensions. The default comes first, with an empty id.
#[derive(Debug, Clone, PartialEq)]
pub struct Profile {
    pub id: String,
    pub name: String,
    pub shared: bool,
}

/// The profiles a storage.json lists.
pub fn profiles_in(storage: &str) -> Vec<Profile> {
    let mut out = vec![Profile { id: String::new(), name: "Default".into(), shared: false }];
    let j: Value = serde_json::from_str(storage).unwrap_or(Value::Null);
    for p in j["userDataProfiles"].as_array().into_iter().flatten() {
        let (Some(id), Some(name)) = (p["location"].as_str().filter(|s| !s.is_empty()), p["name"].as_str().filter(|s| !s.is_empty())) else { continue };
        out.push(Profile { id: id.into(), name: name.into(), shared: p["useDefaultFlags"]["extensions"] == Value::Bool(true) });
    }
    out
}

/// The local path of a file:// URI, None for any other scheme.
fn uri_path(uri: &str) -> Option<PathBuf> {
    let rest = uri.strip_prefix("file://")?;
    let b = rest.as_bytes();
    let mut s = Vec::new();
    let mut i = 0;
    while i < b.len() {
        let hex = |c: u8| (c as char).to_digit(16);
        match (b[i], b.get(i + 1).copied().and_then(hex), b.get(i + 2).copied().and_then(hex)) {
            (b'%', Some(h), Some(l)) => {
                s.push((h * 16 + l) as u8);
                i += 3;
            }
            (c, _, _) => {
                s.push(c);
                i += 1;
            }
        }
    }
    let mut s = String::from_utf8(s).ok().filter(|s| !s.is_empty())?;
    if !s.starts_with('/') {
        // file://server/share is a UNC path
        s = format!("//{s}");
    } else if cfg!(windows) && s.as_bytes().get(2) == Some(&b':') {
        // file:///c:/x is c:/x
        s.remove(0);
    }
    Some(PathBuf::from(s))
}

/// The profiles the editor opens a folder holding dir in, by id, empty for the default.
pub fn folder_profiles(storage: &str, dir: &Path) -> Vec<String> {
    let j: Value = serde_json::from_str(storage).unwrap_or(Value::Null);
    let mut out: Vec<String> = Vec::new();
    for (uri, id) in j["profileAssociations"]["workspaces"].as_object().into_iter().flatten() {
        let (Some(id), Some(folder)) = (id.as_str(), uri_path(uri)) else { continue };
        if !paths::within(dir, &folder) {
            continue;
        }
        let p = if id == "__default__profile__" { String::new() } else { id.to_string() };
        if !out.contains(&p) {
            out.push(p);
        }
    }
    out
}

/// Where the editor keeps its user data folder, as the editor itself picks it.
fn app_data() -> Option<PathBuf> {
    if cfg!(windows) {
        env_path("APPDATA").or_else(|| env_path("USERPROFILE").map(|h| h.join("AppData").join("Roaming")))
    } else if cfg!(target_os = "macos") {
        home_dir().map(|h| h.join("Library").join("Application Support"))
    } else {
        env_path("XDG_CONFIG_HOME").or_else(|| home_dir().map(|h| h.join(".config")))
    }
}

/// The editor's storage.json, which lists its profiles, in the user data folder its product.json
/// names or a portable install's own. None without a product.json, as for a remote `code`.
fn storage_of(editor: &Path) -> Option<PathBuf> {
    let root = std::fs::canonicalize(editor).ok()?.parent()?.parent()?.to_path_buf();
    // app/ on macOS, resources/app/ on Linux, <version>/resources/app/ on Windows
    let mut places = vec![root.join("product.json"), root.join("resources/app/product.json")];
    places.extend(std::fs::read_dir(&root).into_iter().flatten().flatten().map(|e| e.path().join("resources/app/product.json")));
    let product: Value = places.iter().find(|p| p.is_file()).and_then(|p| serde_json::from_slice(&std::fs::read(p).ok()?).ok())?;
    let name = product["nameShort"].as_str().filter(|s| !s.is_empty())?;
    let portable = env_path("VSCODE_PORTABLE").unwrap_or_else(|| {
        if cfg!(target_os = "macos") {
            let own = product["portable"].as_str().map_or_else(|| format!("{}-portable-data", product["applicationName"].as_str().unwrap_or_default()), str::to_string);
            root.ancestors().nth(4).unwrap_or(&root).join(own)
        } else {
            root.join("data")
        }
    });
    let data = if product.get("target").is_none() && portable.is_dir() {
        portable.join("user-data")
    } else if let Some(own) = env_path("VSCODE_APPDATA") {
        own.join(name)
    } else {
        app_data()?.join(name)
    };
    Some(data.join("User").join("globalStorage").join("storage.json"))
}

/// The editor's command line with these arguments, acting on the target's profile.
fn command(t: &Target, args: &[&str]) -> Command {
    let mut argv: Vec<String> = std::iter::once(t.editor.to_string_lossy().into_owned()).chain(args.iter().map(|a| a.to_string())).collect();
    if !t.profile.is_empty() {
        argv.extend(["--profile".to_string(), t.profile.clone()]);
    }
    Command::new(argv, std::env::temp_dir())
}

pub fn installed_version(t: &Target, id: &str) -> String {
    rant_process::output(&command(t, &["--list-extensions", "--show-versions"])).map(|l| version_in(&l, id)).unwrap_or_default()
}

pub fn install(t: &Target, file: &Path) -> Result<Extension> {
    let v = read(file)?;
    let file_text = file.to_string_lossy();
    if rant_process::run(&command(t, &["--install-extension", &file_text, "--force"])) != Ok(0) {
        return Err(failed(format!("{} could not install {}", paths::slash(&t.editor), paths::slash(file))));
    }
    Ok(v)
}

pub fn remove(t: &Target, id: &str) -> Result<()> {
    if rant_process::run(&command(t, &["--uninstall-extension", id])) != Ok(0) {
        return Err(failed(format!("{} could not remove the extension {id}", paths::slash(&t.editor))));
    }
    Ok(())
}

/// The name to install into for a profile, empty for one that holds the default's extensions.
fn install_name(p: &Profile) -> String {
    if p.id.is_empty() || p.shared { String::new() } else { p.name.clone() }
}

/// The profile to install into: the one named, the one this VS Code terminal's folder opens in,
/// the only one, or the one the user picks.
fn profile_of(ctx: &Context, wanted: &Wanted, editor: &Path) -> Result<String> {
    if let Some(p) = &wanted.profile {
        return Ok(p.clone());
    }
    let storage = storage_of(editor).and_then(|f| std::fs::read_to_string(f).ok()).unwrap_or_default();
    let all = profiles_in(&storage);
    // a terminal of a VS Code window, or of an editor built from it
    let in_editor = std::env::var("TERM_PROGRAM").is_ok_and(|v| v == "vscode");
    let ids = if in_editor { folder_profiles(&storage, ctx.cwd()?) } else { Vec::new() };
    if let [id] = ids.as_slice()
        && let Some(p) = all.iter().find(|p| &p.id == id)
    {
        return Ok(install_name(p));
    }
    let own: Vec<&Profile> = all.iter().filter(|p| !p.shared).collect();
    if own.len() == 1 {
        return Ok(String::new());
    }
    ctx.out.note(&format!("{} has the profiles:", paths::slash(editor)));
    let width = own.len().to_string().len();
    for (i, p) in own.iter().enumerate() {
        ctx.out.note(&format!("  {:>width$}  {}", i + 1, p.name));
    }
    loop {
        let Some(answer) = prompt::ask("Install into which profile? Give its number or name:") else {
            return Err(failed("no profile picked, pass --profile NAME with one of the names above"));
        };
        if let Some(p) = own.iter().enumerate().find(|(i, p)| answer == (i + 1).to_string() || answer == p.name).map(|(_, p)| p) {
            return Ok(install_name(p));
        }
        ctx.out.note(&format!("no profile `{answer}`"));
    }
}

fn editor_of(ctx: &Context, wanted: &Wanted) -> Result<PathBuf> {
    let cwd = ctx.cwd()?;
    if let Some(given) = &wanted.editor {
        return rant_process::find_program(given, cwd).ok_or_else(|| failed(format!("no editor `{given}` found, give the program the editor installs extensions with")));
    }
    rant_process::find_program("code", cwd).ok_or_else(|| {
        failed("VS Code's `code` command is not on PATH. In VS Code run `Shell Command: Install 'code' command in PATH`, or pass --editor for another editor")
    })
}

/// Whether the editor named, else `code`, is on PATH.
pub fn editor_found(ctx: &Context, wanted: &Wanted) -> bool {
    ctx.cwd().is_ok_and(|cwd| rant_process::find_program(wanted.editor.as_deref().unwrap_or("code"), cwd).is_some())
}

/// The editor and profile to install into, which may ask which profile.
pub fn target(ctx: &Context, wanted: &Wanted) -> Result<Target> {
    let editor = editor_of(ctx, wanted)?;
    let profile = profile_of(ctx, wanted, &editor)?;
    Ok(Target { editor, profile })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::packages::zip::zip_of;
    use crate::scratch::Scratch;

    const ID: &str = "kosmosisdire.vscode-rant";

    fn root() -> (&'static str, PathBuf) {
        if cfg!(windows) { ("file:///c%3A/Main/RANT", PathBuf::from("C:\\Main\\RANT")) } else { ("file:///home/me/RANT", PathBuf::from("/home/me/RANT")) }
    }

    /// A storage.json as VS Code writes it: the built in Agents profile shares the default's extensions.
    fn storage() -> String {
        let (uri, _) = root();
        format!(
            r#"{{"userDataProfiles": [
                {{ "location": "builtin/agents", "name": "Agents", "useDefaultFlags": {{ "extensions": true, "settings": true }} }},
                {{ "location": "-500e13d6", "name": "Mixed" }},
                {{ "location": "-3b410830", "name": "Rust", "icon": "robot" }}],
              "profileAssociations": {{ "workspaces": {{
                "{uri}": "-500e13d6", "{uri}/cli-rant": "-3b410830", "{uri}%20Old": "__default__profile__",
                "vscode-remote://ssh-remote%2Bhost/home/me/RANT": "-3b410830" }} }} }}"#
        )
    }

    #[test]
    fn reads_the_id_and_version_an_extension_names() {
        let t = Scratch::new("vsix-reads");
        let manifest = "{ \"name\": \"vscode-rant\", \"publisher\": \"KosmosisDire\", \"version\": \"0.0.9\" }";
        for deflate in [true, false] {
            let file = t.write("ext.vsix", zip_of(&[("extension.vsixmanifest", "<x/>", deflate), ("extension/package.json", manifest, deflate)]));
            assert_eq!(read(&file).unwrap(), Extension { id: ID.into(), version: "0.0.9".into() });
        }
        let cases = [("text.vsix", b"not a zip".to_vec()), ("other.vsix", zip_of(&[("readme.md", "hi", true)])), ("unnamed.vsix", zip_of(&[("extension/package.json", "{ \"name\": \"x\", \"version\": \"1\" }", true)]))];
        for (name, bytes) in cases {
            let file = t.write(name, bytes);
            assert!(read(&file).unwrap_err().messages()[0].contains("is not a VS Code extension"), "{name}");
        }
        assert!(read(&t.path("missing.vsix")).unwrap_err().messages()[0].contains("cannot read"));
    }

    #[test]
    fn a_listing_gives_the_extension_version() {
        assert_eq!(version_in("ms-python.python@2026.1.0\nkosmosisdire.vscode-rant@0.2.1\n", ID), "0.2.1");
        assert_eq!(version_in("KosmosisDire.vscode-rant@1.0.0\r\n", ID), "1.0.0");
        assert_eq!(version_in("kosmosisdire.vscode-rant@1.0.0\n", "KosmosisDire.vscode-rant"), "1.0.0");
        assert_eq!(version_in("", ID), "");
        assert_eq!(version_in("kosmosisdire.vscode-rant-extra@1.0.0\nother.vscode-rant@1.0.0\n", ID), "");
    }

    #[test]
    fn profiles_start_with_the_default_and_mark_the_shared() {
        let p = profiles_in(&storage());
        assert_eq!(p.iter().map(|p| (p.id.as_str(), p.name.as_str(), p.shared)).collect::<Vec<_>>(), [("", "Default", false), ("builtin/agents", "Agents", true), ("-500e13d6", "Mixed", false), ("-3b410830", "Rust", false)]);
        assert_eq!(profiles_in("").len(), 1);
        assert_eq!(profiles_in("{ \"userDataProfiles\": 3 }").len(), 1);
        assert_eq!(profiles_in("{ \"userDataProfiles\": [ 1, { \"name\": \"Lost\" } ] }").len(), 1);
    }

    #[test]
    fn a_folder_gives_each_profile_it_opens_in() {
        let (_, root) = root();
        let s = storage();
        assert_eq!(folder_profiles(&s, &root), ["-500e13d6"]);
        assert_eq!(folder_profiles(&s, &root.join("src").join("app")), ["-500e13d6"]);
        assert_eq!(folder_profiles(&s, &root.join("cli-rant").join("src")), ["-500e13d6", "-3b410830"]);
        assert_eq!(folder_profiles(&s, &PathBuf::from(format!("{} Old", root.display()))), [""]);
        assert!(folder_profiles(&s, &PathBuf::from(format!("{}X", root.display()))).is_empty());
        assert!(folder_profiles(&s, root.parent().unwrap()).is_empty());
        assert!(folder_profiles("", &root).is_empty());
        assert!(folder_profiles("{ \"profileAssociations\": [] }", &root).is_empty());
        if cfg!(windows) {
            assert_eq!(folder_profiles(&s, Path::new("c:/main/rant")), ["-500e13d6"]);
        }
    }
}
