//! GitHub releases and downloads over HTTPS. The TLS roots are built in, so the static
//! binary needs nothing from the system. GITHUB_TOKEN, when set, lifts the API rate limit.

use std::path::Path;
use std::time::Duration;

/// One release of a repository and the files attached to it.
#[derive(Debug, Clone, PartialEq)]
pub struct Release {
    pub tag: String,
    pub assets: Vec<Asset>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Asset {
    pub name: String,
    pub url: String,
}

impl Release {
    /// The version the tag names, without its `v`.
    pub fn version(&self) -> &str {
        self.tag.strip_prefix('v').unwrap_or(&self.tag)
    }
}

fn agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(120)))
        .http_status_as_error(false)
        .user_agent("rant-cli")
        .build()
        .into()
}

fn get(url: &str, api: bool) -> Result<ureq::http::Response<ureq::Body>, String> {
    let mut req = agent().get(url);
    if api {
        req = req.header("Accept", "application/vnd.github+json");
        if let Ok(token) = std::env::var("GITHUB_TOKEN") {
            if !token.is_empty() {
                req = req.header("Authorization", &format!("Bearer {token}"));
            }
        }
    }
    req.call().map_err(|e| format!("cannot reach {}: {e}", host(url)))
}

fn host(url: &str) -> &str {
    url.split("://").nth(1).and_then(|r| r.split('/').next()).unwrap_or(url)
}

/// A release of `owner/name`: the latest when tag is None. A version without its `v` finds
/// the `v` tag too.
pub fn release(repo: &str, tag: Option<&str>) -> Result<Release, String> {
    let tag = tag.map(|t| if t.starts_with('v') { t.to_string() } else { format!("v{t}") });
    let url = match &tag {
        Some(t) => format!("https://api.github.com/repos/{repo}/releases/tags/{t}"),
        None => format!("https://api.github.com/repos/{repo}/releases/latest"),
    };
    let mut rsp = get(&url, true)?;
    let status = rsp.status().as_u16();
    let body = rsp.body_mut().read_to_string().map_err(|e| format!("reading {url}: {e}"))?;
    match status {
        200 => parse(&body).ok_or_else(|| format!("{repo} answered a release that cannot be read")),
        404 => Err(match tag {
            Some(t) => format!("{repo} has no release {t}"),
            None => format!("{repo} has no release yet"),
        }),
        403 | 429 => Err("GitHub refused, its rate limit was probably reached. Wait an hour or set GITHUB_TOKEN".into()),
        s => Err(format!("GitHub answered {s} for {url}")),
    }
}

fn parse(body: &str) -> Option<Release> {
    let j: serde_json::Value = serde_json::from_str(body).ok()?;
    let tag = j.get("tag_name")?.as_str()?.to_string();
    let assets = j
        .get("assets")?
        .as_array()?
        .iter()
        .filter_map(|a| {
            Some(Asset { name: a.get("name")?.as_str()?.to_string(), url: a.get("browser_download_url")?.as_str()?.to_string() })
        })
        .collect();
    Some(Release { tag, assets })
}

/// Downloads url to dest through a temporary file beside it, so dest is whole or untouched.
pub fn download(url: &str, dest: &Path) -> Result<(), String> {
    let mut rsp = get(url, false)?;
    let status = rsp.status().as_u16();
    if status != 200 {
        return Err(format!("downloading {url} failed with {status}"));
    }
    if let Some(dir) = dest.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
    }
    let mut tmp = dest.as_os_str().to_owned();
    tmp.push(".part");
    let tmp = std::path::PathBuf::from(tmp);
    let written = (|| -> std::io::Result<()> {
        let mut out = std::fs::File::create(&tmp)?;
        let mut reader = rsp.body_mut().with_config().limit(1 << 30).reader();
        std::io::copy(&mut reader, &mut out)?;
        out.sync_all()
    })();
    if let Err(e) = written.and_then(|_| replace(&tmp, dest)) {
        let _ = std::fs::remove_file(&tmp);
        return Err(format!("downloading {url}: {e}"));
    }
    Ok(())
}

/// Moves tmp over dest. A running program on Windows cannot be replaced but can be renamed,
/// so the old file steps aside first.
fn replace(tmp: &Path, dest: &Path) -> std::io::Result<()> {
    if std::fs::rename(tmp, dest).is_ok() {
        return Ok(());
    }
    let mut old = dest.as_os_str().to_owned();
    old.push(".old");
    let old = std::path::PathBuf::from(old);
    let _ = std::fs::remove_file(&old);
    std::fs::rename(dest, &old)?;
    std::fs::rename(tmp, dest)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_release_reads_its_tag_and_assets() {
        let r = parse(r#"{"tag_name":"v0.0.17","assets":[{"name":"rant.h","browser_download_url":"https://x/rant.h"}]}"#).unwrap();
        assert_eq!(r.version(), "0.0.17");
        assert_eq!(r.assets, vec![Asset { name: "rant.h".into(), url: "https://x/rant.h".into() }]);
        assert!(parse("{}").is_none());
    }
}
