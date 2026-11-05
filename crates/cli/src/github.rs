//! Releases and files on GitHub, over HTTPS with the root certificates built in, so nothing on
//! the system is needed. GITHUB_TOKEN, when set, lifts the API's rate limit.

use std::path::Path;
use std::sync::OnceLock;
use std::time::Duration;

use rant_config::paths;
use serde_json::Value;

use crate::failure::{Result, failed};
use crate::files;

/// One file a release carries.
#[derive(Debug, Clone, PartialEq)]
pub struct Asset {
    pub name: String,
    pub url: String,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Release {
    pub tag: String,
    /// The tag without a v in front of its first digit.
    pub version: String,
    pub assets: Vec<Asset>,
}

impl Release {
    pub fn asset(&self, name: &str) -> Option<&Asset> {
        self.assets.iter().find(|a| a.name == name)
    }
}

/// The platform a release names its programs for, as this rant was built.
pub fn platform() -> &'static str {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("windows", "aarch64") => "win-arm64",
        ("windows", _) => "win-x64",
        ("macos", "aarch64") => "osx-arm64",
        ("macos", _) => "osx-x64",
        (_, "aarch64") => "linux-arm64",
        _ => "linux-x64",
    }
}

/// A version as a tag writes it: its numbers, then the stage of a pre-release, compared in
/// this order.
#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
struct Version {
    /// Trailing zeros dropped, so 1.2 is 1.2.0.
    numbers: Vec<u64>,
    /// dev, alpha, beta, rc, the release, then post.
    stage: u8,
    /// The number after the stage, as in beta.2.
    n: u64,
    rest: String,
}

fn parse_version(v: &str) -> Version {
    let v = v.strip_prefix(['v', 'V']).unwrap_or(v);
    // build metadata orders nothing
    let v = v.split('+').next().unwrap_or_default();
    let mut numbers = Vec::new();
    let mut at = 0;
    let b = v.as_bytes();
    while at < b.len() && b[at].is_ascii_digit() {
        let end = at + b[at..].iter().take_while(|c| c.is_ascii_digit()).count();
        numbers.push(v[at..end].parse().unwrap_or(u64::MAX));
        at = end;
        if at + 1 < b.len() && b[at] == b'.' && b[at + 1].is_ascii_digit() {
            at += 1;
        } else {
            break;
        }
    }
    while numbers.last() == Some(&0) {
        numbers.pop();
    }
    // -beta.1, -rc.1 and PEP 440's b1 all read as a word and a number
    let tag: String = v[at..].chars().filter(|c| !"-._".contains(*c)).flat_map(char::to_lowercase).collect();
    if tag.is_empty() {
        return Version { numbers, stage: 4, n: 0, rest: String::new() };
    }
    let letters = tag.chars().take_while(char::is_ascii_alphabetic).count();
    let stage = match &tag[..letters] {
        "dev" => 0,
        "b" | "beta" => 2,
        "c" | "rc" | "pre" | "preview" => 3,
        "post" => 5,
        _ => 1,
    };
    let digits = tag[letters..].chars().take_while(char::is_ascii_digit).count();
    let n = tag[letters..letters + digits].parse().unwrap_or(0);
    Version { numbers, stage, n, rest: tag[letters + digits..].to_string() }
}

/// Whether version is newer than than, pre-releases before their release.
pub fn newer(version: &str, than: &str) -> bool {
    parse_version(version) > parse_version(than)
}

fn version_of(tag: &str) -> String {
    let b = tag.as_bytes();
    let v = b.len() > 1 && (b[0] == b'v' || b[0] == b'V') && b[1].is_ascii_digit();
    if v { tag[1..].to_string() } else { tag.to_string() }
}

pub fn parse_release(body: &[u8]) -> Result<Release> {
    let j: Value = serde_json::from_slice(body).unwrap_or(Value::Null);
    let Some(tag) = j["tag_name"].as_str() else { return Err(failed("cannot read the release from GitHub")) };
    let assets = j["assets"].as_array().into_iter().flatten();
    let assets = assets.filter_map(|a| Some(Asset { name: a["name"].as_str()?.to_string(), url: a["browser_download_url"].as_str()?.to_string() }));
    Ok(Release { tag: tag.to_string(), version: version_of(tag), assets: assets.collect() })
}

fn agent() -> &'static ureq::Agent {
    static AGENT: OnceLock<ureq::Agent> = OnceLock::new();
    AGENT.get_or_init(|| {
        let config = ureq::Agent::config_builder().timeout_global(Some(Duration::from_secs(120))).http_status_as_error(false).user_agent("rant-cli").build();
        config.into()
    })
}

fn host(url: &str) -> &str {
    let rest = url.split_once("://").map_or(url, |(_, r)| r);
    rest.split('/').next().unwrap_or(rest)
}

/// What a failed request means, in a reader's words.
fn unreached(url: &str, e: ureq::Error) -> crate::failure::Failure {
    let host = host(url);
    match e {
        ureq::Error::HostNotFound => failed(format!("cannot reach {host}, its name did not resolve")),
        ureq::Error::Timeout(_) => failed(format!("cannot reach {host} in time")),
        ureq::Error::Io(_) | ureq::Error::ConnectionFailed => failed(format!("cannot reach {host}")),
        e => failed(format!("cannot fetch {url}: {e}")),
    }
}

/// A GET's status and body, read whole. api adds the API's headers and the token.
fn get(url: &str, api: bool) -> Result<(u16, Vec<u8>)> {
    let mut req = agent().get(url);
    if api {
        req = req.header("Accept", "application/vnd.github+json");
        if let Ok(token) = std::env::var("GITHUB_TOKEN")
            && !token.is_empty()
        {
            req = req.header("Authorization", format!("Bearer {token}"));
        }
    }
    let mut resp = req.call().map_err(|e| unreached(url, e))?;
    let status = resp.status().as_u16();
    let body = resp.body_mut().with_config().limit(64 << 20).read_to_vec().map_err(|e| unreached(url, e))?;
    Ok((status, body))
}

/// Releases of repos by the API's path of each, None for one there is none of.
fn releases_at(wanted: &[(&str, String)]) -> Result<Vec<Option<Release>>> {
    let mut out = Vec::new();
    for (repo, path) in wanted {
        let url = format!("https://api.github.com/repos/{repo}/releases/{path}");
        let (status, body) = get(&url, true)?;
        out.push(match status {
            200 => Some(parse_release(&body)?),
            404 => None,
            403 | 429 => return Err(failed("GitHub refused, its rate limit was probably reached. Wait an hour or set GITHUB_TOKEN")),
            s => return Err(failed(format!("GitHub answered {s} for {url}"))),
        });
    }
    Ok(out)
}

/// The latest release of each repo, None for one with no release.
pub fn latest(repos: &[String]) -> Result<Vec<Option<Release>>> {
    let wanted: Vec<(&str, String)> = repos.iter().map(|r| (r.as_str(), "latest".to_string())).collect();
    releases_at(&wanted)
}

/// A repo's release of version, with or without its v, or its latest when version is empty.
pub fn release(repo: &str, version: &str) -> Result<Release> {
    if version.is_empty() {
        return releases_at(&[(repo, "latest".into())])?.remove(0).ok_or_else(|| failed(format!("{repo} has no release yet")));
    }
    let bare = version_of(version);
    for tag in [format!("v{bare}"), bare.clone()] {
        if let Some(r) = releases_at(&[(repo, format!("tags/{tag}"))])?.remove(0) {
            return Ok(r);
        }
    }
    Err(failed(format!("{repo} has no release {bare}")))
}

/// Each url's text, None for one the server has not.
pub fn fetch_all(urls: &[String]) -> Result<Vec<Option<String>>> {
    let mut out = Vec::new();
    for url in urls {
        let (status, body) = get(url, false)?;
        out.push(match status {
            200 => Some(String::from_utf8_lossy(&body).into_owned()),
            404 => None,
            s => return Err(failed(format!("{} answered {s} for {url}", host(url)))),
        });
    }
    Ok(out)
}

pub fn fetch(url: &str) -> Result<Option<String>> {
    Ok(fetch_all(&[url.to_string()])?.remove(0))
}

/// A file of a repo at a ref, as GitHub serves it raw.
pub fn raw_url(repo: &str, git_ref: &str, file: &str) -> String {
    format!("https://raw.githubusercontent.com/{repo}/{git_ref}/{file}")
}

/// Downloads url into dest through a .part file beside it, made runnable for a program. A dest
/// a running program holds is moved aside to .old first, as Windows allows.
pub fn download(url: &str, dest: &Path, program: bool) -> Result<()> {
    if let Some(dir) = dest.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let part = dest.with_file_name(format!("{}.part", dest.file_name().unwrap_or_default().to_string_lossy()));
    let fetched = (|| {
        let mut resp = agent().get(url).call().map_err(|e| unreached(url, e))?;
        if !resp.status().is_success() {
            return Err(failed(format!("downloading {url} failed, the server answered with an error")));
        }
        let mut file = std::fs::File::create(&part).map_err(|e| failed(format!("cannot write {}: {e}", paths::slash(&part))))?;
        let mut body = resp.body_mut().as_reader();
        std::io::copy(&mut body, &mut file).map_err(|e| failed(format!("downloading {url} failed: {e}")))?;
        Ok(())
    })();
    if let Err(e) = fetched {
        let _ = std::fs::remove_file(&part);
        return Err(e);
    }
    if program {
        make_runnable(&part);
    }
    if files::rename(&part, dest).is_ok() {
        return Ok(());
    }
    let old = dest.with_file_name(format!("{}.old", dest.file_name().unwrap_or_default().to_string_lossy()));
    let _ = std::fs::remove_file(&old);
    let _ = std::fs::rename(dest, &old);
    files::rename(&part, dest).map_err(|e| {
        let _ = std::fs::remove_file(&part);
        failed(format!("cannot replace {}: {e}", paths::slash(dest)))
    })
}

#[cfg(unix)]
fn make_runnable(p: &Path) {
    use std::os::unix::fs::PermissionsExt;
    if let Ok(m) = std::fs::metadata(p) {
        let _ = std::fs::set_permissions(p, std::fs::Permissions::from_mode(m.permissions().mode() | 0o111));
    }
}

#[cfg(not(unix))]
fn make_runnable(_: &Path) {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_order_as_releases_do() {
        assert!(newer("0.0.10", "0.0.9"));
        assert!(newer("v1.2.1", "1.2"));
        assert!(!newer("1.2.0", "v1.2"), "1.2 is 1.2.0");
        assert!(newer("1.0.0", "1.0.0-rc.2"));
        assert!(newer("1.0.0-rc.2", "1.0.0-beta.9"));
        assert!(newer("1.0.0b2", "1.0.0b1"));
        assert!(newer("1.0.0.post1", "1.0.0"));
        assert!(newer("1.0.0-alpha", "1.0.0.dev3"));
        assert!(!newer("1.0.0+build.5", "1.0.0"), "build metadata orders nothing");
    }

    #[test]
    fn a_release_reads_its_tag_and_files() {
        let body = br#"{"tag_name": "v0.0.20", "assets": [{"name": "rant-0.0.20-win-x64.exe", "browser_download_url": "https://x/a"}, {"name": 3}]}"#;
        let r = parse_release(body).unwrap();
        assert_eq!((r.tag.as_str(), r.version.as_str()), ("v0.0.20", "0.0.20"));
        assert_eq!(r.asset("rant-0.0.20-win-x64.exe").map(|a| a.url.as_str()), Some("https://x/a"));
        assert_eq!(r.assets.len(), 1);
        assert!(parse_release(b"{}").is_err());
        assert_eq!(version_of("crate-v1"), "crate-v1");
    }

    #[test]
    fn a_host_is_named_from_its_url() {
        assert_eq!(host("https://api.github.com/repos/a/b"), "api.github.com");
        assert_eq!(raw_url("a/b", "HEAD", "package.rant"), "https://raw.githubusercontent.com/a/b/HEAD/package.rant");
    }
}
