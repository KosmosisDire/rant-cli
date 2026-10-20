use std::collections::BTreeMap;
use std::path::Path;

use hcl::eval::{Context, Evaluate};
use hcl::Value;
use hcl_edit::expr::{Expression, Traversal, TraversalOperator};
use hcl_edit::structure::Attribute;
use hcl_edit::visit::{visit_expr, Visit};
use hcl_edit::Span;

use crate::diag::Diag;
use crate::source::{Loc, Source};

/// What an expression may reference: `param.*` in group files, and `path.*` everywhere.
pub struct Scope<'a> {
    pub params: Option<&'a BTreeMap<String, Value>>,
    pub file_dir: &'a Path,
    pub package_dir: Option<&'a Path>,
    pub workspace: &'a Path,
}

const PATH_NAMES: [&str; 3] = ["file", "package", "workspace"];

/// The names a release file name reads, known once a release is picked.
const RELEASE_NAMES: [&str; 2] = ["version", "platform"];

/// The release a file name is evaluated for: version is its tag without the v, platform
/// names a machine, such as win-x64.
pub struct Target<'a> {
    pub version: &'a str,
    pub platform: &'a str,
}

/// The name of a file a package's releases carry, such as `archive`. Its names are checked
/// when read, and it is evaluated once a release is picked.
#[derive(Debug, Clone)]
pub struct FileName {
    key: String,
    expr: hcl::Expression,
    at: Loc,
}

impl FileName {
    pub fn read(src: &Source, attr: &Attribute) -> Result<FileName, Diag> {
        let mut check = RefCheck { src, names: Names::Release, err: None };
        check.visit_expr(&attr.value);
        if let Some(err) = check.err {
            return Err(err);
        }
        Ok(FileName { key: attr.key.to_string(), expr: attr.value.clone().into(), at: Loc::of(src, &attr.value) })
    }

    /// The file's name in one release.
    pub fn of(&self, target: &Target) -> Result<String, Diag> {
        let mut ctx = Context::new();
        ctx.declare_var("version", target.version);
        ctx.declare_var("platform", target.platform);
        let key = &self.key;
        match self.expr.evaluate(&ctx) {
            Ok(Value::String(s)) if s.is_empty() || s.contains(['/', '\\']) => {
                Err(self.at.diag(format!("`{key}` must name a file of the release, found \"{s}\"")))
            }
            Ok(Value::String(s)) => Ok(s),
            Ok(v) => Err(self.at.diag(format!("`{key}` must be a string, found {}", type_name(&v)))),
            Err(e) => Err(self.at.diag(e.kind().to_string())),
        }
    }
}

impl Scope<'_> {
    fn context(&self) -> Context<'static> {
        let mut ctx = Context::new();
        if let Some(params) = self.params {
            let obj: hcl::Map<String, Value> = params.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
            ctx.declare_var("param", Value::Object(obj));
        }
        let mut path = hcl::Map::new();
        path.insert("file".to_string(), Value::String(slash(self.file_dir)));
        path.insert("workspace".to_string(), Value::String(slash(self.workspace)));
        if let Some(p) = self.package_dir {
            path.insert("package".to_string(), Value::String(slash(p)));
        }
        ctx.declare_var("path", Value::Object(path));
        ctx
    }
}

/// Forward slashes on every OS, so a templated path reads the same in every plan.
pub fn slash(p: &Path) -> String {
    p.to_string_lossy().replace('\\', "/")
}

/// Evaluates an attribute's value. References are checked first so an error points at the
/// exact `param.x` or `path.x` rather than the whole expression.
pub fn value(src: &Source, attr: &Attribute, scope: &Scope) -> Result<Value, Diag> {
    expr(src, &attr.value, scope)
}

pub fn expr(src: &Source, e: &Expression, scope: &Scope) -> Result<Value, Diag> {
    let mut check = RefCheck { src, names: Names::Config(scope), err: None };
    check.visit_expr(e);
    if let Some(err) = check.err {
        return Err(err);
    }
    let converted: hcl::Expression = e.clone().into();
    converted.evaluate(&scope.context()).map_err(|err| src.diag(e.span(), err.kind().to_string()))
}

pub fn string(src: &Source, attr: &Attribute, scope: &Scope) -> Result<String, Diag> {
    match value(src, attr, scope)? {
        Value::String(s) => Ok(s),
        v => Err(src.diag_at(&attr.value, format!("`{}` must be a string, found {}", attr.key.as_str(), type_name(&v)))),
    }
}

pub fn string_list(src: &Source, attr: &Attribute, scope: &Scope) -> Result<Vec<String>, Diag> {
    let key = attr.key.as_str();
    match value(src, attr, scope)? {
        Value::Array(items) => items
            .into_iter()
            .map(|v| match v {
                Value::String(s) => Ok(s),
                v => Err(src.diag_at(&attr.value, format!("`{key}` must hold strings, found {}", type_name(&v)))),
            })
            .collect(),
        v => Err(src.diag_at(&attr.value, format!("`{key}` must be a list of strings, found {}", type_name(&v)))),
    }
}

/// A string or a list of strings, the two forms `run`, `args` and `build` take.
pub enum StringOrList {
    One(String),
    List(Vec<String>),
}

pub fn string_or_list(src: &Source, attr: &Attribute, scope: &Scope) -> Result<StringOrList, Diag> {
    match value(src, attr, scope)? {
        Value::String(s) => Ok(StringOrList::One(s)),
        Value::Array(_) => string_list(src, attr, scope).map(StringOrList::List),
        v => Err(src.diag_at(
            &attr.value,
            format!("`{}` must be a string or a list of strings, found {}", attr.key.as_str(), type_name(&v)),
        )),
    }
}

/// A bare identifier such as `string` in `type = string`, read and never evaluated.
pub fn identifier<'a>(src: &Source, attr: &'a Attribute) -> Result<&'a str, Diag> {
    match &attr.value {
        Expression::Variable(v) => Ok(v.as_str()),
        e => Err(src.diag_at(e, format!("`{}` takes a bare name such as `string`", attr.key.as_str()))),
    }
}

pub fn type_name(v: &Value) -> &'static str {
    match v {
        Value::Null => "null",
        Value::Bool(_) => "a bool",
        Value::Number(n) if n.is_f64() => "a float",
        Value::Number(_) => "an int",
        Value::String(_) => "a string",
        Value::Array(_) => "a list",
        Value::Object(_) => "an object",
    }
}

/// What an expression may name: a config file's scope, or only the release names.
#[derive(Clone, Copy)]
enum Names<'a> {
    Config(&'a Scope<'a>),
    Release,
}

struct RefCheck<'a> {
    src: &'a Source,
    names: Names<'a>,
    err: Option<Diag>,
}

impl RefCheck<'_> {
    fn fail(&mut self, span: Option<std::ops::Range<usize>>, message: String) {
        if self.err.is_none() {
            self.err = Some(self.src.diag(span, message));
        }
    }

    fn check_root(&mut self, root: &str, attr: Option<&str>, span: Option<std::ops::Range<usize>>) {
        let scope = match self.names {
            Names::Config(scope) => scope,
            Names::Release => {
                if !RELEASE_NAMES.contains(&root) {
                    self.fail(span, format!("unknown name `{root}`, a release file name reads only version and platform"));
                }
                return;
            }
        };
        match (root, attr) {
            ("param", None) | ("path", None) => self.fail(span, format!("`{root}` needs a name, such as `{root}.x`")),
            ("param", Some(name)) => match scope.params {
                None => self.fail(span, "`param` exists only in group files".to_string()),
                Some(p) if !p.contains_key(name) => self.fail(span, format!("param `{name}` is not declared")),
                Some(_) => {}
            },
            ("path", Some(name)) => {
                if !PATH_NAMES.contains(&name) {
                    self.fail(span, format!("unknown name `path.{name}`, expected path.file, path.package or path.workspace"));
                } else if name == "package" && scope.package_dir.is_none() {
                    self.fail(span, "`path.package` used outside a package".to_string());
                }
            }
            (r, _) if RELEASE_NAMES.contains(&r) => self.fail(span, format!("`{r}` is known only in `archive` and `vsix`")),
            _ => {}
        }
    }
}

impl Visit for RefCheck<'_> {
    fn visit_traversal(&mut self, node: &Traversal) {
        if let Expression::Variable(root) = &node.expr {
            let attr = match node.operators.first().map(|o| o.value()) {
                Some(TraversalOperator::GetAttr(ident)) => Some(ident.as_str()),
                _ => None,
            };
            self.check_root(root.as_str(), attr, node.span());
        } else {
            self.visit_expr(&node.expr);
        }
        for op in &node.operators {
            if let TraversalOperator::Index(e) = op.value() {
                self.visit_expr(e);
            }
        }
    }

    fn visit_expr(&mut self, node: &Expression) {
        match node {
            Expression::Variable(v) => self.check_root(v.as_str(), None, node.span()),
            _ => visit_expr(self, node),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::source::Fields;

    fn eval_attr(text: &str, params: Option<&BTreeMap<String, Value>>, package: Option<&Path>) -> Result<Value, Diag> {
        let src = Source::parse(Path::new("/ws/g.hcl"), text.to_string()).unwrap();
        let fields = Fields::of_body(&src, &src.body, "test", &["v"], &[]).unwrap();
        let scope = Scope { params, file_dir: Path::new("/ws/sub"), package_dir: package, workspace: Path::new("/ws") };
        value(&src, fields.attr("v").unwrap(), &scope)
    }

    #[test]
    fn templates_see_params_and_paths() {
        let mut p = BTreeMap::new();
        p.insert("side".to_string(), Value::from("left"));
        let v = eval_attr("\n  v = \"${path.workspace}/cam_${param.side}\"\n", Some(&p), None).unwrap();
        assert_eq!(v, Value::from("/ws/cam_left"));
    }

    #[test]
    fn undeclared_param_points_at_the_reference() {
        let p = BTreeMap::new();
        let err = eval_attr("\n  v = [\"--x\", param.nope]\n", Some(&p), None).err().unwrap();
        assert_eq!((err.line, err.column), (2, 15));
        assert!(err.message.contains("param `nope` is not declared"));
    }

    #[test]
    fn path_package_outside_a_package() {
        let err = eval_attr("\n  v = path.package\n", None, None).err().unwrap();
        assert!(err.message.contains("outside a package"));
        assert!(eval_attr("\n  v = path.package\n", None, Some(Path::new("/ws/p"))).is_ok());
    }

    #[test]
    fn unknown_path_name() {
        let err = eval_attr("\n  v = path.root\n", None, None).err().unwrap();
        assert!(err.message.contains("path.root"));
    }

    #[test]
    fn param_outside_group_files() {
        let err = eval_attr("\n  v = param.x\n", None, None).err().unwrap();
        assert!(err.message.contains("only in group files"));
    }

    fn file_name(text: &str) -> Result<FileName, Diag> {
        let src = Source::parse(Path::new("/ws/package.rant"), text.to_string()).unwrap();
        let fields = Fields::of_body(&src, &src.body, "test", &["archive"], &[]).unwrap();
        FileName::read(&src, fields.attr("archive").unwrap())
    }

    #[test]
    fn a_file_name_is_read_for_one_release() {
        let f = file_name("archive = \"rant-x-${version}-${platform}.zip\"\n").unwrap();
        assert_eq!(f.of(&Target { version: "1.2.0", platform: "win-x64" }).unwrap(), "rant-x-1.2.0-win-x64.zip");
    }

    #[test]
    fn a_file_name_reads_only_version_and_platform() {
        let err = file_name("archive = \"x-${path.package}\"\n").err().unwrap();
        assert!(err.message.contains("only version and platform"), "{}", err.message);
        assert_eq!((err.line, err.column), (1, 16));
        assert!(file_name("archive = \"x-${verison}\"\n").err().unwrap().message.contains("`verison`"));
        let err = eval_attr("\n  v = \"${version}\"\n", None, None).err().unwrap();
        assert!(err.message.contains("only in `archive` and `vsix`"), "{}", err.message);
    }

    #[test]
    fn a_file_name_names_one_file() {
        let t = Target { version: "1", platform: "p" };
        let err = file_name("\narchive = \"bin/${version}\"\n").unwrap().of(&t).err().unwrap();
        assert_eq!(err.line, 2);
        assert!(err.message.contains("must name a file of the release"), "{}", err.message);
        assert!(file_name("archive = [version]\n").unwrap().of(&t).err().unwrap().message.contains("must be a string"));
    }
}
