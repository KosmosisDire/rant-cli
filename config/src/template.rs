//! Templates for `rant new`: a folder of files rendered with minijinja, file names too, and
//! a template.hcl that describes it, includes other templates and declares its params in
//! the same `param` blocks groups use. The built in ones are compiled in.

use std::collections::BTreeMap;
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use hcl::Value;

use crate::diag::Diag;
use crate::eval::{self, Scope};
use crate::param::{self, Param};
use crate::source::{no_labels, Fields, Source};

const MANIFEST: &str = "template.hcl";

macro_rules! builtin {
    ($name:literal: $($file:literal),*) => {
        ($name, &[$(($file, include_str!(concat!("../templates/", $name, "/", $file)))),*])
    };
}

/// Every built in template and its files, by name.
const BUILTIN: &[(&str, &[(&str, &str)])] = &[
    builtin!("package-cpp": "template.hcl", "CMakeLists.txt"),
    builtin!("node-cpp": "template.hcl", "{{name}}.cpp"),
    builtin!("package-python": "template.hcl", "pyproject.toml"),
    builtin!("node-python": "template.hcl", "{{name}}.py"),
    builtin!("package-csharp": "template.hcl", "{{name}}.csproj", "Program.cs"),
    builtin!("group": "template.hcl", "{{name}}.hcl"),
];

/// Where a template comes from: compiled in by name, or a folder on disk.
#[derive(Debug, Clone)]
pub enum Origin {
    Builtin(String),
    Dir(PathBuf),
}

pub struct Template {
    pub description: Option<String>,
    pub params: Vec<Param>,
    next: Option<String>,
    include: Vec<String>,
    /// Paths relative to the template, as written, with their bytes.
    files: Vec<(String, Vec<u8>)>,
}

/// What a template made: the files written and the next steps it suggests.
#[derive(Debug)]
pub struct Made {
    pub files: Vec<PathBuf>,
    pub next: Option<String>,
}

fn builtin_files(name: &str) -> Option<Vec<(String, Vec<u8>)>> {
    BUILTIN.iter().find(|(n, _)| *n == name).map(|(_, files)| files.iter().map(|(p, t)| (p.to_string(), t.as_bytes().to_vec())).collect())
}

fn dir_files(dir: &Path) -> Result<Vec<(String, Vec<u8>)>, Diag> {
    let mut out = Vec::new();
    for e in ignore::WalkBuilder::new(dir).hidden(false).require_git(false).build().flatten() {
        if !e.file_type().is_some_and(|t| t.is_file()) || e.path().components().any(|c| c.as_os_str() == ".git") {
            continue;
        }
        let rel = crate::paths::relative(e.path(), dir);
        let bytes = std::fs::read(e.path()).map_err(|err| Diag::file(e.path(), format!("cannot read: {err}")))?;
        out.push((rel, bytes));
    }
    out.sort();
    Ok(out)
}

pub fn builtin_names() -> Vec<&'static str> {
    BUILTIN.iter().map(|(n, _)| *n).collect()
}

/// Reads a template and its template.hcl.
pub fn load(origin: &Origin) -> Result<Template, Diag> {
    let (files, manifest_path) = match origin {
        Origin::Builtin(name) => (
            builtin_files(name).ok_or_else(|| Diag::plain(format!("no built in template `{name}`, there are {}", builtin_names().join(", "))))?,
            PathBuf::from(format!("{name}/{MANIFEST}")),
        ),
        Origin::Dir(dir) => {
            if !dir.join(MANIFEST).is_file() {
                return Err(Diag::plain(format!("{} has no {MANIFEST}, so it is not a template", dir.display())));
            }
            (dir_files(dir)?, dir.join(MANIFEST))
        }
    };
    let text = files.iter().find(|(p, _)| p == MANIFEST).map(|(_, b)| String::from_utf8_lossy(b).into_owned()).unwrap_or_default();
    let src = Source::parse(&manifest_path, text)?;
    let blocks = src.top_blocks()?;
    let block = match blocks.as_slice() {
        [b] if b.ident.as_str() == "template" => *b,
        _ => return Err(Diag::file(&manifest_path, "a template.hcl holds exactly one `template` block")),
    };
    no_labels(&src, block)?;
    let fields = Fields::of(&src, block, &["description", "include", "next"], &["param"])?;
    let empty = BTreeMap::new();
    let dir = src.dir().to_path_buf();
    let scope = Scope { params: Some(&empty), file_dir: &dir, package_dir: None, workspace: &dir };
    let params = param::parse_all(&src, &fields, &scope)?;
    if let Some(p) = params.iter().find(|p| p.name == "name") {
        return Err(p.at.diag("`name` is the name given to `rant new`, a param cannot take it"));
    }
    Ok(Template {
        description: fields.attr("description").map(|a| eval::string(&src, a, &scope)).transpose()?,
        params,
        next: fields.attr("next").map(|a| eval::string(&src, a, &scope)).transpose()?,
        include: fields.attr("include").map(|a| eval::string_list(&src, a, &scope)).transpose()?.unwrap_or_default(),
        files: files.into_iter().filter(|(p, _)| p != MANIFEST).collect(),
    })
}

/// The words of a name, split at anything not a letter or digit and where a lower case
/// letter meets an upper case one: `lidar-driver`, `lidar_driver` and `LidarDriver` alike.
fn words(s: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut cur = String::new();
    let mut prev_lower = false;
    for c in s.chars() {
        if !c.is_alphanumeric() {
            if !cur.is_empty() {
                out.push(std::mem::take(&mut cur));
            }
            prev_lower = false;
            continue;
        }
        if c.is_uppercase() && prev_lower && !cur.is_empty() {
            out.push(std::mem::take(&mut cur));
        }
        prev_lower = c.is_lowercase() || c.is_ascii_digit();
        cur.extend(c.to_lowercase());
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

fn capital(w: &str) -> String {
    let mut c = w.chars();
    c.next().map(|f| f.to_uppercase().chain(c).collect()).unwrap_or_default()
}

const CASES: [&str; 4] = ["snake", "kebab", "pascal", "camel"];

/// A name in one of CASES.
fn case(s: &str, which: &str) -> Option<String> {
    let w = words(s);
    Some(match which {
        "snake" => w.join("_"),
        "kebab" => w.join("-"),
        "pascal" => w.iter().map(|w| capital(w)).collect(),
        "camel" => w.iter().enumerate().map(|(i, w)| if i == 0 { w.clone() } else { capital(w) }).collect(),
        _ => return None,
    })
}

/// The name given to `rant new`. It renders as given, and `name.snake` and the other cases
/// work in file names too, where the `|` of a filter cannot go on Windows.
#[derive(Debug)]
struct Name(String);

impl minijinja::value::Object for Name {
    fn repr(self: &Arc<Self>) -> minijinja::value::ObjectRepr {
        minijinja::value::ObjectRepr::Plain
    }

    fn get_value(self: &Arc<Self>, key: &minijinja::Value) -> Option<minijinja::Value> {
        case(&self.0, key.as_str()?).map(minijinja::Value::from)
    }

    fn render(self: &Arc<Self>, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

fn environment() -> minijinja::Environment<'static> {
    let mut env = minijinja::Environment::new();
    env.set_undefined_behavior(minijinja::UndefinedBehavior::Strict);
    env.set_keep_trailing_newline(true);
    for which in CASES {
        env.add_filter(which, move |v: minijinja::Value| case(&v.to_string(), which).unwrap_or_default());
    }
    env
}

fn render(env: &minijinja::Environment, text: &str, values: &minijinja::Value, what: &str) -> Result<String, Diag> {
    env.render_str(text, values).map_err(|e| Diag::plain(format!("{what}: {e}")))
}

/// Every file a template and its includes make, rendered, relative to the destination.
fn rendered(t: &Template, env: &minijinja::Environment, values: &minijinja::Value, depth: usize) -> Result<Vec<(String, Vec<u8>)>, Diag> {
    if depth > 8 {
        return Err(Diag::plain("templates include each other in a loop"));
    }
    let mut out = Vec::new();
    for (path, bytes) in &t.files {
        let name = render(env, path, values, path)?;
        let body = match std::str::from_utf8(bytes) {
            Ok(text) => render(env, text, values, path)?.into_bytes(),
            Err(_) => bytes.clone(),    // not text, so copied as it is
        };
        out.push((name, body));
    }
    for inc in &t.include {
        let included = load(&Origin::Builtin(inc.clone()))?;
        out.extend(rendered(&included, env, values, depth + 1)?);
    }
    Ok(out)
}

/// Makes name from a template in dest with `key=value` params. Refuses before writing
/// anything when a file it would write exists.
pub fn make(origin: &Origin, dest: &Path, name: &str, raw: &[String]) -> Result<Made, Diag> {
    let t = load(origin)?;
    let mut given = BTreeMap::new();
    for kv in raw {
        let (k, v) = kv.split_once('=').ok_or_else(|| Diag::plain(format!("`{kv}` is not key=value")))?;
        let p = t.params.iter().find(|p| p.name == k).ok_or_else(|| Diag::plain(format!("the template has no param `{k}`")))?;
        given.insert(k.to_string(), param::parse_value(p, v).map_err(Diag::plain)?);
    }
    let bound: BTreeMap<String, Value> = param::bind(&t.params, given, &|m| Diag::plain(m), "")?;
    let mut ctx: BTreeMap<String, minijinja::Value> = bound.iter().map(|(k, v)| (k.clone(), minijinja::Value::from_serialize(v))).collect();
    ctx.insert("name".into(), minijinja::Value::from_object(Name(name.to_string())));
    let values = minijinja::Value::from(ctx);
    let env = environment();

    let files = rendered(&t, &env, &values, 0)?;
    for (rel, _) in &files {
        let p = dest.join(rel);
        if p.exists() {
            return Err(Diag::plain(format!("{} exists already, nothing was written", p.display())));
        }
    }
    let mut written = Vec::new();
    for (rel, body) in files {
        let p = dest.join(&rel);
        if let Some(dir) = p.parent() {
            std::fs::create_dir_all(dir).map_err(|e| Diag::file(dir, format!("cannot create: {e}")))?;
        }
        std::fs::write(&p, body).map_err(|e| Diag::file(&p, format!("cannot write: {e}")))?;
        written.push(p);
    }
    let next = t.next.as_deref().map(|n| render(&env, n, &values, "next")).transpose()?;
    Ok(Made { files: written, next })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_split_into_words_for_every_case() {
        assert_eq!(words("lidar-driver"), ["lidar", "driver"]);
        assert_eq!(words("LidarDriver2"), ["lidar", "driver2"]);
        assert_eq!(words("lidar_driver"), ["lidar", "driver"]);
        let env = environment();
        let v = minijinja::Value::from(BTreeMap::from([("name", minijinja::Value::from_object(Name("lidar-driver".into())))]));
        let text = "{{ name }} {{ name | pascal }} {{ name.snake }} {{ name.camel }} {{ 'Big Box' | kebab }}";
        assert_eq!(env.render_str(text, &v).unwrap(), "lidar-driver LidarDriver lidar_driver lidarDriver big-box");
    }

    #[test]
    fn every_builtin_loads() {
        for name in builtin_names() {
            load(&Origin::Builtin(name.into())).unwrap_or_else(|e| panic!("{name}: {e}"));
        }
    }

    #[test]
    fn a_package_renders_with_its_node_and_never_overwrites() {
        let t = crate::testdir::TestDir::new();
        let made = make(&Origin::Builtin("package-cpp".into()), &t.path("cam"), "cam", &[]).unwrap();
        let names: Vec<String> = made.files.iter().map(|p| p.file_name().unwrap().to_string_lossy().into_owned()).collect();
        assert_eq!(names, ["CMakeLists.txt", "cam.cpp"]);
        let cmake = std::fs::read_to_string(t.path("cam/CMakeLists.txt")).unwrap();
        assert!(cmake.contains("project(cam CXX)\n") && cmake.contains("add_executable(cam cam.cpp)\n"), "{cmake}");
        assert!(made.next.is_none(), "an included template's next is its own");
        assert!(make(&Origin::Builtin("package-cpp".into()), &t.path("cam"), "cam", &[]).unwrap_err().message.contains("exists already"));
    }

    #[test]
    fn a_folder_template_takes_params() {
        let t = crate::testdir::TestDir::new();
        t.write("tpl/template.hcl", "template {\n  param \"port\" {\n    type = int\n  }\n  next = \"port {{ port }}\"\n}\n");
        t.write("tpl/{{name}}.txt", "{{ name.snake }} on {{ port }}\n");
        let origin = Origin::Dir(t.path("tpl"));
        assert!(make(&origin, &t.path("out"), "Web App", &[]).unwrap_err().message.contains("required param `port`"));
        let made = make(&origin, &t.path("out"), "Web App", &["port=8080".into()]).unwrap();
        assert_eq!(std::fs::read_to_string(t.path("out/Web App.txt")).unwrap(), "web_app on 8080\n");
        assert_eq!(made.next.as_deref(), Some("port 8080"));
        assert!(make(&origin, &t.path("out2"), "x", &["port=eighty".into()]).unwrap_err().message.contains("takes an int"));
    }
}
