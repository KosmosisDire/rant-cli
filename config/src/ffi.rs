//! The C ABI. Every handle owns the strings and arrays its view points into, so a view
//! stays valid until its handle is freed. cbindgen writes rant_config.h from this file.

use std::any::Any;
use std::collections::BTreeMap;
use std::ffi::{c_char, CStr, CString};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Mutex;

use crate::build_plan;
use crate::database;
use crate::diag::Diag;
use crate::group::{GroupDef, Groups};
use crate::manifest::ManifestKind;
use crate::model::{Model, NodeType, Package};
use crate::package;
use crate::param::{self, Param};
use crate::paths;
use crate::plan::{self, Plan};
use crate::refs;
use crate::scan::NodeKind;
use crate::service;
use crate::vsix;
use crate::zip;
use crate::workspace::{self, WorkspaceConfig};

/// rant_config_open flag: also find every package and scan it for node types.
pub const RANT_CONFIG_PACKAGES: u32 = 1;

/// One located error. line and column are 1 based, 0 means the whole file. file is empty
/// for an error with no place in a file.
#[repr(C)]
pub struct RantConfigDiagnostic {
    pub file: *const c_char,
    pub line: u32,
    pub column: u32,
    pub message: *const c_char,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub enum RantConfigNodeKind {
    Native,
    Python,
    CSharp,
    Declared,
    Rust,
}

/// A runnable node type. path is NULL for a node declared in package.rant.
#[repr(C)]
pub struct RantConfigNodeType {
    pub package: *const c_char,
    pub name: *const c_char,
    pub kind: RantConfigNodeKind,
    pub path: *const c_char,
    pub run: *const *const c_char,
    pub run_count: usize,
    pub cwd: *const c_char,
    /// The file it is written in when the manifest says, as for a Rust program, else NULL.
    pub source: *const c_char,
}

#[repr(C)]
pub struct RantConfigPackage {
    pub name: *const c_char,
    /// What its package.rant says it is for, NULL when it says nothing.
    pub description: *const c_char,
    pub dir: *const c_char,
    /// What makes it a package: "cmake", "python", "csharp", "rust" for its manifests,
    /// "package.rant" for its own settings.
    pub kinds: *const *const c_char,
    pub kind_count: usize,
    /// Its build files, then its package.rant if it has one.
    pub files: *const *const c_char,
    pub file_count: usize,
    pub nodes: *const RantConfigNodeType,
    pub node_count: usize,
    /// Put in rant's home by `rant install`, already built.
    pub installed: bool,
}

/// A group file and the name it goes by. short_name is the shortest name that finds it from
/// start_dir, the bare name where that is enough.
#[repr(C)]
pub struct RantConfigGroupFile {
    pub name: *const c_char,
    pub short_name: *const c_char,
    pub file: *const c_char,
}

/// A service block, checked against its group. full is NULL and words empty when problem
/// says what is wrong with it.
#[repr(C)]
pub struct RantConfigService {
    /// The group as the block names it.
    pub group: *const c_char,
    pub full: *const c_char,
    /// Its params as key=value words, as `rant run` takes them in the folder of its file.
    pub words: *const *const c_char,
    pub word_count: usize,
    pub problem: *const RantConfigDiagnostic,
}

/// What rant_config_open found. With no workspace around the directory, rant's home stands
/// in when one was set, and root is NULL when none was. data holds the state and caches.
/// packages, groups and services are filled only when RANT_CONFIG_PACKAGES was asked for.
#[repr(C)]
pub struct RantConfigWorkspaceView {
    pub root: *const c_char,
    pub logs: *const c_char,
    pub data: *const c_char,
    /// root is rant's home, which holds only the installed packages.
    pub home: bool,
    /// The workspace's domain, -1 when it sets none.
    pub domain: i32,
    pub packages: *const RantConfigPackage,
    pub package_count: usize,
    /// Python nodes outside every package.
    pub loose: *const RantConfigNodeType,
    pub loose_count: usize,
    pub groups: *const RantConfigGroupFile,
    pub group_count: usize,
    /// The file that lists its services, which may not be there yet: its workspace.rant, or
    /// services.rant in rant's home. NULL with root.
    pub services_file: *const c_char,
    pub services: *const RantConfigService,
    pub service_count: usize,
    pub diagnostics: *const RantConfigDiagnostic,
    pub diagnostic_count: usize,
}

/// An opened workspace and everything its view points into.
pub struct RantConfigWorkspace {
    view: RantConfigWorkspaceView,
    #[allow(dead_code)]
    store: Store,
}

/// A name and a value: an environment entry, or a resolved group param.
#[repr(C)]
pub struct RantConfigPair {
    pub name: *const c_char,
    pub value: *const c_char,
}

/// One node instance of a plan: its mesh name, the node type it runs and how.
#[repr(C)]
pub struct RantConfigInstance {
    pub name: *const c_char,
    pub type_ref: *const c_char,
    pub kind: RantConfigNodeKind,
    pub argv: *const *const c_char,
    pub argc: usize,
    pub env: *const RantConfigPair,
    pub env_count: usize,
    pub cwd: *const c_char,
    /// The group whose own node block makes it, NULL for a node run alone.
    pub group: *const c_char,
    /// It joins the mesh once started. Only a declared program may not.
    pub expect_mesh: bool,
    /// "never", "on-failure" or "always".
    pub restart: *const c_char,
}

/// A resolved plan in start order, every path absolute. Empty when diagnostics are not.
/// group and params name a group root, with every param resolved, defaults included. For a
/// single node group is NULL.
#[repr(C)]
pub struct RantConfigPlanView {
    pub root: *const c_char,
    pub logs: *const c_char,
    pub data: *const c_char,
    pub group: *const c_char,
    pub params: *const RantConfigPair,
    pub param_count: usize,
    pub instances: *const RantConfigInstance,
    pub instance_count: usize,
    pub diagnostics: *const RantConfigDiagnostic,
    pub diagnostic_count: usize,
}

/// A resolved plan and everything its view points into.
pub struct RantConfigPlan {
    view: RantConfigPlanView,
    #[allow(dead_code)]
    store: Store,
}

/// Owns every C string and array a view points into. A CString or boxed slice never moves
/// its contents, so pointers stay valid as the store grows.
#[derive(Default)]
pub(crate) struct Store {
    strings: Vec<CString>,
    arrays: Vec<Box<dyn Any>>,
}

impl Store {
    pub fn str(&mut self, s: &str) -> *const c_char {
        let c = CString::new(s.replace('\0', "")).unwrap_or_default();
        let p = c.as_ptr();
        self.strings.push(c);
        p
    }

    pub fn path(&mut self, p: &Path) -> *const c_char {
        self.str(&crate::eval::slash(p))
    }

    pub fn opt_path(&mut self, p: Option<&Path>) -> *const c_char {
        p.map(|p| self.path(p)).unwrap_or(std::ptr::null())
    }

    pub fn array<T: 'static>(&mut self, v: Vec<T>) -> (*const T, usize) {
        let b: Box<[T]> = v.into_boxed_slice();
        let out = (b.as_ptr(), b.len());
        self.arrays.push(Box::new(b));
        out
    }

    pub fn strs(&mut self, v: &[String]) -> (*const *const c_char, usize) {
        let ptrs: Vec<*const c_char> = v.iter().map(|s| self.str(s)).collect();
        self.array(ptrs)
    }

    pub fn diags(&mut self, diags: &[Diag]) -> (*const RantConfigDiagnostic, usize) {
        let v: Vec<RantConfigDiagnostic> = diags
            .iter()
            .map(|d| RantConfigDiagnostic {
                file: if d.file.as_os_str().is_empty() { self.str("") } else { self.path(&d.file) },
                line: d.line,
                column: d.column,
                message: self.str(&d.message),
            })
            .collect();
        self.array(v)
    }

    pub fn params(&mut self, params: &[Param]) -> (*const RantConfigParam, usize) {
        let v: Vec<RantConfigParam> = params
            .iter()
            .map(|p| {
                let opts: Vec<String> = p.options.iter().map(param::text).collect();
                let (options, option_count) = self.strs(&opts);
                RantConfigParam {
                    name: self.str(&p.name),
                    type_name: self.str(p.ty.name()),
                    default_value: p.default.as_ref().map(|d| self.str(&param::text(d))).unwrap_or(std::ptr::null()),
                    options,
                    option_count,
                    description: p.description.as_deref().map(|d| self.str(d)).unwrap_or(std::ptr::null()),
                }
            })
            .collect();
        self.array(v)
    }

    pub fn node_type(&mut self, n: &NodeType) -> RantConfigNodeType {
        let (run, run_count) = self.strs(&n.run);
        RantConfigNodeType {
            package: self.str(&n.package),
            name: self.str(&n.name),
            kind: kind(n.kind),
            path: self.opt_path(n.path.as_deref()),
            run,
            run_count,
            cwd: self.path(&n.cwd),
            source: self.opt_path(n.source.as_deref()),
        }
    }

    fn package(&mut self, p: &Package) -> RantConfigPackage {
        let nodes: Vec<RantConfigNodeType> = p.nodes.iter().map(|n| self.node_type(n)).collect();
        let (nodes, node_count) = self.array(nodes);
        let mut kinds: Vec<String> = p
            .manifests
            .iter()
            .map(|m| match m.kind {
                ManifestKind::CMake => "cmake",
                ManifestKind::Python => "python",
                ManifestKind::CSharp => "csharp",
                ManifestKind::Rust => "rust",
            })
            .map(str::to_string)
            .collect();
        kinds.dedup();
        if p.block.is_some() {
            kinds.push(crate::package::PACKAGE_FILE.into());
        }
        let (kinds, kind_count) = self.strs(&kinds);
        let mut files: Vec<String> = p.manifests.iter().map(|m| crate::eval::slash(&m.path)).collect();
        if p.block.is_some() {
            files.push(crate::eval::slash(&p.dir.join(crate::package::PACKAGE_FILE)));
        }
        let (files, file_count) = self.strs(&files);
        RantConfigPackage {
            name: self.str(&p.name),
            description: p.block.as_ref().and_then(|b| b.description.as_deref()).map(|d| self.str(d)).unwrap_or(std::ptr::null()),
            dir: self.path(&p.dir),
            kinds,
            kind_count,
            files,
            file_count,
            nodes,
            node_count,
            installed: p.installed,
        }
    }

    fn instance(&mut self, i: &plan::Instance) -> RantConfigInstance {
        let (argv, argc) = self.strs(&i.argv);
        let env: Vec<RantConfigPair> =
            i.env.iter().map(|(k, v)| RantConfigPair { name: self.str(k), value: self.str(v) }).collect();
        let (env, env_count) = self.array(env);
        RantConfigInstance {
            name: self.str(&i.name),
            type_ref: self.str(&i.type_ref()),
            kind: kind(i.node.kind),
            argv,
            argc,
            env,
            env_count,
            cwd: self.path(&i.cwd),
            group: i.group.as_deref().map(|g| self.str(g)).unwrap_or(std::ptr::null()),
            expect_mesh: i.node.expect_mesh,
            restart: self.str(i.restart.as_str()),
        }
    }

    /// The root, logs and data paths of a view, all NULL outside a workspace.
    fn roots(&mut self, config: Option<&WorkspaceConfig>) -> [*const c_char; 3] {
        match config {
            Some(c) => [self.path(&c.root), self.path(&c.logs), self.path(&c.data)],
            None => [std::ptr::null(); 3],
        }
    }
}

fn kind(k: NodeKind) -> RantConfigNodeKind {
    match k {
        NodeKind::Native => RantConfigNodeKind::Native,
        NodeKind::Python => RantConfigNodeKind::Python,
        NodeKind::CSharp => RantConfigNodeKind::CSharp,
        NodeKind::Declared => RantConfigNodeKind::Declared,
        NodeKind::Rust => RantConfigNodeKind::Rust,
    }
}

/// # Safety
/// `s` must be NULL or a NUL terminated string.
pub(crate) unsafe fn arg_str(s: *const c_char) -> String {
    if s.is_null() {
        return String::new();
    }
    CStr::from_ptr(s).to_string_lossy().into_owned()
}

/// # Safety
/// `s` must be NULL or a NUL terminated string.
pub(crate) unsafe fn arg_path(s: *const c_char) -> PathBuf {
    if s.is_null() {
        return PathBuf::from(".");
    }
    PathBuf::from(arg_str(s))
}

/// The workspace enclosing a directory, its model when asked for, and every error met.
struct Loaded {
    start: PathBuf,
    config: Option<WorkspaceConfig>,
    model: Option<Model>,
    diags: Vec<Diag>,
}

/// rant's home, set once before any other call. Installed packages live in its `packages`
/// folder, and it stands in for a workspace outside every one.
static HOME: Mutex<Option<PathBuf>> = Mutex::new(None);

fn home() -> Option<PathBuf> {
    HOME.lock().ok().and_then(|h| h.clone())
}

/// Sets rant's home, NULL or empty for none.
///
/// # Safety
/// `dir` must be NULL or a NUL terminated UTF-8 string.
#[no_mangle]
pub unsafe extern "C" fn rant_config_set_home(dir: *const c_char) {
    let dir = (!dir.is_null()).then(|| arg_path(dir)).filter(|d| !d.as_os_str().is_empty());
    if let Ok(mut h) = HOME.lock() {
        *h = dir;
    }
}

fn load(start: &Path, packages: bool) -> Loaded {
    let mut diags = Vec::new();
    let home = home();
    let config = match workspace::open(start) {
        Ok(Some(c)) => Some(c),
        Ok(None) => match home.as_deref().map(workspace::home).transpose() {
            Ok(c) => c,
            Err(d) => {
                diags.push(d);
                None
            }
        },
        Err(d) => {
            diags.push(d);
            None
        }
    };
    let model = match (&config, packages) {
        (Some(c), true) => {
            let (m, d) = Model::load(c.clone(), home.as_deref());
            diags.extend(d);
            Some(m)
        }
        _ => None,
    };
    Loaded { start: start.to_path_buf(), config, model, diags }
}

fn workspace_handle(loaded: &Loaded, extra: Vec<Diag>) -> *mut RantConfigWorkspace {
    let mut store = Store::default();
    let packages: Vec<RantConfigPackage> =
        loaded.model.iter().flat_map(|m| m.packages.iter()).map(|p| store.package(p)).collect();
    let (packages, package_count) = store.array(packages);
    let loose: Vec<RantConfigNodeType> = loaded.model.iter().flat_map(|m| m.loose.iter()).map(|n| store.node_type(n)).collect();
    let (loose, loose_count) = store.array(loose);
    let mut groups: Vec<RantConfigGroupFile> = Vec::new();
    let mut services: Vec<RantConfigService> = Vec::new();
    if let Some(m) = &loaded.model {
        let here = m.package_holding(&paths::normalize(&loaded.start)).map(|p| p.name.clone());
        let lookup = Groups::new(m);
        for g in &m.groups {
            let short = lookup.short_name(g, here.as_deref());
            groups.push(RantConfigGroupFile { name: store.str(&g.name), short_name: store.str(&short), file: store.path(&g.path) });
        }
        for s in &m.config.services {
            use std::ptr::null;
            let mut view = RantConfigService { group: store.str(&s.group), full: null(), words: null(), word_count: 0, problem: null() };
            match s.check(m, &lookup) {
                Ok(c) => {
                    view.full = store.str(&c.full);
                    (view.words, view.word_count) = store.strs(&c.words);
                }
                Err(d) => view.problem = store.diags(&[d]).0,
            }
            services.push(view);
        }
    }
    let (groups, group_count) = store.array(groups);
    let (services, service_count) = store.array(services);
    let services_file = loaded.config.as_ref().map_or(std::ptr::null(), |c| store.path(&c.services_file));
    let diags: Vec<Diag> = loaded.diags.iter().cloned().chain(extra).collect();
    let (diagnostics, diagnostic_count) = store.diags(&diags);
    let [root, logs, data] = store.roots(loaded.config.as_ref());
    let view = RantConfigWorkspaceView {
        root,
        logs,
        data,
        home: loaded.config.as_ref().is_some_and(|c| c.home),
        domain: loaded.config.as_ref().and_then(|c| c.placement.domain).map_or(-1, i32::from),
        packages,
        package_count,
        loose,
        loose_count,
        groups,
        group_count,
        services_file,
        services,
        service_count,
        diagnostics,
        diagnostic_count,
    };
    Box::into_raw(Box::new(RantConfigWorkspace { view, store }))
}

/// A plan, and for a group root the group's name and its resolved params.
struct Planned {
    plan: Plan,
    group: Option<(String, BTreeMap<String, hcl::Value>)>,
}

fn plan_handle(loaded: &Loaded, planned: Result<Planned, Diag>) -> *mut RantConfigPlan {
    let mut store = Store::default();
    let mut diags = loaded.diags.clone();
    let planned = match planned {
        Ok(p) if diags.is_empty() => p,
        Ok(_) => Planned { plan: Plan::default(), group: None },
        Err(d) => {
            diags.push(d);
            Planned { plan: Plan::default(), group: None }
        }
    };
    let instances: Vec<RantConfigInstance> = planned.plan.instances.iter().map(|i| store.instance(i)).collect();
    let (instances, instance_count) = store.array(instances);
    let (group, params) = match &planned.group {
        Some((name, values)) => {
            let pairs: Vec<RantConfigPair> =
                values.iter().map(|(k, v)| RantConfigPair { name: store.str(k), value: store.str(&param::text(v)) }).collect();
            (store.str(name), pairs)
        }
        None => (std::ptr::null(), Vec::new()),
    };
    let (params, param_count) = store.array(params);
    let (diagnostics, diagnostic_count) = store.diags(&diags);
    let [root, logs, data] = store.roots(loaded.config.as_ref());
    let view =
        RantConfigPlanView { root, logs, data, group, params, param_count, instances, instance_count, diagnostics, diagnostic_count };
    Box::into_raw(Box::new(RantConfigPlan { view, store }))
}

fn no_workspace() -> Diag {
    Diag::plain("no workspace here, `rant init` makes one")
}

/// Finds the workspace enclosing start_dir (NULL = the current directory) and loads what
/// flags ask for. Never NULL: failures are in the view's diagnostics.
///
/// # Safety
/// `start_dir` must be NULL or a NUL terminated UTF-8 string.
#[no_mangle]
pub unsafe extern "C" fn rant_config_open(start_dir: *const c_char, flags: u32) -> *mut RantConfigWorkspace {
    workspace_handle(&load(&arg_path(start_dir), flags & RANT_CONFIG_PACKAGES != 0), Vec::new())
}

/// Writes a new workspace.rant into dir and opens it. Refused when dir already has one.
///
/// # Safety
/// `dir` must be NULL or a NUL terminated UTF-8 string.
#[no_mangle]
pub unsafe extern "C" fn rant_config_init(dir: *const c_char) -> *mut RantConfigWorkspace {
    match workspace::init(&arg_path(dir)) {
        Ok(root) => workspace_handle(&load(&root, false), Vec::new()),
        Err(d) => workspace_handle(&Loaded { start: PathBuf::new(), config: None, model: None, diags: Vec::new() }, vec![d]),
    }
}

/// # Safety
/// `ws` must be a live handle from rant_config_open or rant_config_init.
#[no_mangle]
pub unsafe extern "C" fn rant_config_view(ws: *const RantConfigWorkspace) -> *const RantConfigWorkspaceView {
    &(*ws).view
}

/// # Safety
/// `ws` must be NULL or a live handle, and is invalid afterwards.
#[no_mangle]
pub unsafe extern "C" fn rant_config_free(ws: *mut RantConfigWorkspace) {
    if !ws.is_null() {
        drop(Box::from_raw(ws));
    }
}

/// The plan for one node of a node type, given in any reference form and resolved from
/// start_dir. Never NULL: failures are in the view's diagnostics.
///
/// # Safety
/// Both arguments must be NULL or NUL terminated UTF-8 strings.
#[no_mangle]
pub unsafe extern "C" fn rant_config_plan_node(start_dir: *const c_char, node_type: *const c_char) -> *mut RantConfigPlan {
    let start = arg_path(start_dir);
    let loaded = load(&start, true);
    let planned = match &loaded.model {
        Some(m) => plan::single(m, &arg_str(node_type), &start).map(|plan| Planned { plan, group: None }),
        None => Err(no_workspace()),
    };
    plan_handle(&loaded, planned)
}

/// Whether a name is a group or a node type: kind is 1 for a group, which wins over a node of
/// its name, 2 for a node and 0 for neither, with a diagnostic that says so.
#[repr(C)]
pub struct RantConfigNamedView {
    pub kind: u32,
    pub diagnostics: *const RantConfigDiagnostic,
    pub diagnostic_count: usize,
}

pub struct RantConfigNamed {
    view: RantConfigNamedView,
    #[allow(dead_code)]
    store: Store,
}

/// What a name means from start_dir. Never NULL.
///
/// # Safety
/// Both arguments must be NULL or NUL terminated UTF-8 strings.
#[no_mangle]
pub unsafe extern "C" fn rant_config_named(start_dir: *const c_char, name: *const c_char) -> *mut RantConfigNamed {
    let start = arg_path(start_dir);
    let loaded = load(&start, true);
    let mut diags = loaded.diags.clone();
    let mut kind = 0;
    match &loaded.model {
        Some(m) => match plan::named(m, &arg_str(name), &start) {
            plan::Named::Group => kind = 1,
            plan::Named::Node => kind = 2,
            plan::Named::Neither(e) => diags.push(Diag::plain(e)),
        },
        None => diags.push(no_workspace()),
    }
    let mut store = Store::default();
    let (diagnostics, diagnostic_count) = store.diags(&diags);
    Box::into_raw(Box::new(RantConfigNamed { view: RantConfigNamedView { kind, diagnostics, diagnostic_count }, store }))
}

/// # Safety
/// `named` must be a live handle from rant_config_named.
#[no_mangle]
pub unsafe extern "C" fn rant_config_named_view(named: *const RantConfigNamed) -> *const RantConfigNamedView {
    &(*named).view
}

/// # Safety
/// `named` must be NULL or a live handle, and is invalid afterwards.
#[no_mangle]
pub unsafe extern "C" fn rant_config_named_free(named: *mut RantConfigNamed) {
    if !named.is_null() {
        drop(Box::from_raw(named));
    }
}

/// The group a reference names from start_dir, read and with its interface worked out.
fn group_def(groups: &Groups, model: &Model, start: &Path, reference: &str) -> Result<(Rc<GroupDef>, Vec<Param>), Diag> {
    let package = model.package_holding(&paths::normalize(start)).map(|p| p.name.clone());
    let file = groups.find(reference, package.as_deref()).map_err(Diag::plain)?;
    let def = groups.load(file)?;
    let iface = groups.interface(&def)?;
    Ok((def, iface))
}

/// The plan for a group started with `key=value` params, each read as its param's type.
fn plan_group(model: &Model, start: &Path, reference: &str, raw: &[String]) -> Result<Planned, Diag> {
    let groups = Groups::new(model);
    let (def, iface) = group_def(&groups, model, start, reference)?;
    let given = param::from_words(&iface, &def.file.name, raw, start)?;
    let (plan, values) = groups.plan(&def, given.into_iter().collect())?;
    Ok(Planned { plan, group: Some((def.file.name.clone(), values)) })
}

/// The plan for a group, given by reference from start_dir, with params as an array of
/// "key=value" strings. Never NULL: failures are in the view's diagnostics.
///
/// # Safety
/// start_dir and group must be NULL or NUL terminated UTF-8 strings, and params must point
/// at param_count such strings.
#[no_mangle]
pub unsafe extern "C" fn rant_config_plan_group(
    start_dir: *const c_char,
    group: *const c_char,
    params: *const *const c_char,
    param_count: usize,
) -> *mut RantConfigPlan {
    let start = arg_path(start_dir);
    let raw: Vec<String> = (0..param_count).map(|i| arg_str(*params.add(i))).collect();
    let loaded = load(&start, true);
    let planned = match &loaded.model {
        Some(m) => plan_group(m, &start, &arg_str(group), &raw),
        None => Err(no_workspace()),
    };
    plan_handle(&loaded, planned)
}

/// What an edit of files did. changed is false when it had nothing to do.
#[repr(C)]
pub struct RantConfigEditView {
    pub changed: bool,
    pub diagnostics: *const RantConfigDiagnostic,
    pub diagnostic_count: usize,
}

pub struct RantConfigEdit {
    view: RantConfigEditView,
    #[allow(dead_code)]
    store: Store,
}

fn edit_handle(loaded: &Loaded, edit: impl FnOnce(&Model) -> Result<bool, Diag>) -> *mut RantConfigEdit {
    let mut store = Store::default();
    let mut diags = loaded.diags.clone();
    let mut changed = false;
    match &loaded.model {
        Some(m) if diags.is_empty() => match edit(m) {
            Ok(c) => changed = c,
            Err(d) => diags.push(d),
        },
        Some(_) => {}
        None => diags.push(no_workspace()),
    }
    let (diagnostics, diagnostic_count) = store.diags(&diags);
    Box::into_raw(Box::new(RantConfigEdit { view: RantConfigEditView { changed, diagnostics, diagnostic_count }, store }))
}

/// Adds a service block for a group, by reference from start_dir with "key=value" params, to
/// the file that lists the services there. Unchanged when one with the same params is there.
/// Never NULL: failures are in the view's diagnostics.
///
/// # Safety
/// start_dir and group must be NULL or NUL terminated UTF-8 strings, and params must point
/// at param_count such strings.
#[no_mangle]
pub unsafe extern "C" fn rant_config_add_service(
    start_dir: *const c_char,
    group: *const c_char,
    params: *const *const c_char,
    param_count: usize,
) -> *mut RantConfigEdit {
    let start = arg_path(start_dir);
    let raw: Vec<String> = (0..param_count).map(|i| arg_str(*params.add(i))).collect();
    let reference = arg_str(group);
    edit_handle(&load(&start, true), |m| {
        let groups = Groups::new(m);
        let (def, iface) = group_def(&groups, m, &start, &reference)?;
        let given = param::from_words(&iface, &def.file.name, &raw, &start)?;
        service::add(m, &groups, &def, given)
    })
}

/// Removes the service blocks at these places among those the view of start_dir lists.
/// Never NULL: failures are in the view's diagnostics.
///
/// # Safety
/// start_dir must be NULL or a NUL terminated UTF-8 string, and indexes must point at count
/// values.
#[no_mangle]
pub unsafe extern "C" fn rant_config_remove_services(start_dir: *const c_char, indexes: *const usize, count: usize) -> *mut RantConfigEdit {
    let picked: Vec<usize> = (0..count).map(|i| *indexes.add(i)).collect();
    edit_handle(&load(&arg_path(start_dir), true), |m| service::remove(&m.config.services_file, &picked).map(|()| true))
}

/// Unpacks every file of the zip at path into dest. Never NULL: failures are in the view's
/// diagnostics.
///
/// # Safety
/// Both arguments must be NULL or NUL terminated UTF-8 strings.
#[no_mangle]
pub unsafe extern "C" fn rant_config_unzip(path: *const c_char, dest: *const c_char) -> *mut RantConfigEdit {
    let mut store = Store::default();
    let (changed, diags) = match zip::extract(&arg_path(path), &arg_path(dest)) {
        Ok(()) => (true, vec![]),
        Err(d) => (false, vec![d]),
    };
    let (diagnostics, diagnostic_count) = store.diags(&diags);
    Box::into_raw(Box::new(RantConfigEdit { view: RantConfigEditView { changed, diagnostics, diagnostic_count }, store }))
}

/// # Safety
/// `e` must be a live handle from an edit.
#[no_mangle]
pub unsafe extern "C" fn rant_config_edit_view(e: *const RantConfigEdit) -> *const RantConfigEditView {
    &(*e).view
}

/// # Safety
/// `e` must be NULL or a live handle, and is invalid afterwards.
#[no_mangle]
pub unsafe extern "C" fn rant_config_edit_free(e: *mut RantConfigEdit) {
    if !e.is_null() {
        drop(Box::from_raw(e));
    }
}

/// One param of a group or template. default_value is NULL for a required param.
#[repr(C)]
pub struct RantConfigParam {
    pub name: *const c_char,
    pub type_name: *const c_char,
    pub default_value: *const c_char,
    pub options: *const *const c_char,
    pub option_count: usize,
    pub description: *const c_char,
}

/// A group and the params it takes, own and exposed, for help and completion.
#[repr(C)]
pub struct RantConfigGroupView {
    pub name: *const c_char,
    pub file: *const c_char,
    pub description: *const c_char,
    pub params: *const RantConfigParam,
    pub param_count: usize,
    /// The groups it includes, by full name.
    pub includes: *const *const c_char,
    pub include_count: usize,
    pub diagnostics: *const RantConfigDiagnostic,
    pub diagnostic_count: usize,
}

/// A described group and everything its view points into.
pub struct RantConfigGroup {
    view: RantConfigGroupView,
    #[allow(dead_code)]
    store: Store,
}

/// A group by reference from start_dir, with its params. Never NULL: failures are in the
/// view's diagnostics.
///
/// # Safety
/// Both arguments must be NULL or NUL terminated UTF-8 strings.
#[no_mangle]
pub unsafe extern "C" fn rant_config_group(start_dir: *const c_char, group: *const c_char) -> *mut RantConfigGroup {
    let start = arg_path(start_dir);
    let loaded = load(&start, true);
    let mut store = Store::default();
    let mut diags = loaded.diags.clone();
    let mut view = RantConfigGroupView {
        name: std::ptr::null(),
        file: std::ptr::null(),
        description: std::ptr::null(),
        params: std::ptr::null(),
        param_count: 0,
        includes: std::ptr::null(),
        include_count: 0,
        diagnostics: std::ptr::null(),
        diagnostic_count: 0,
    };
    let described = match &loaded.model {
        Some(m) => {
            let groups = Groups::new(m);
            group_def(&groups, m, &start, &arg_str(group)).and_then(|(def, iface)| {
                let includes = groups.includes(&def)?;
                Ok((def, iface, includes))
            })
        }
        None => Err(no_workspace()),
    };
    match described {
        Ok((def, iface, includes)) if diags.is_empty() => {
            view.name = store.str(&def.file.name);
            view.file = store.path(&def.file.path);
            view.description = def.description.as_deref().map(|d| store.str(d)).unwrap_or(std::ptr::null());
            (view.params, view.param_count) = store.params(&iface);
            (view.includes, view.include_count) = store.strs(&includes);
        }
        Ok(_) => {}
        Err(d) => diags.push(d),
    }
    (view.diagnostics, view.diagnostic_count) = store.diags(&diags);
    Box::into_raw(Box::new(RantConfigGroup { view, store }))
}

/// # Safety
/// `group` must be a live handle from rant_config_group.
#[no_mangle]
pub unsafe extern "C" fn rant_config_group_view(group: *const RantConfigGroup) -> *const RantConfigGroupView {
    &(*group).view
}

/// # Safety
/// `group` must be NULL or a live handle, and is invalid afterwards.
#[no_mangle]
pub unsafe extern "C" fn rant_config_group_free(group: *mut RantConfigGroup) {
    if !group.is_null() {
        drop(Box::from_raw(group));
    }
}

/// # Safety
/// `plan` must be a live handle from a rant_config_plan_* call.
#[no_mangle]
pub unsafe extern "C" fn rant_config_plan_view(plan: *const RantConfigPlan) -> *const RantConfigPlanView {
    &(*plan).view
}

/// # Safety
/// `plan` must be NULL or a live handle, and is invalid afterwards.
#[no_mangle]
pub unsafe extern "C" fn rant_config_plan_free(plan: *mut RantConfigPlan) {
    if !plan.is_null() {
        drop(Box::from_raw(plan));
    }
}

/// One command as argv. argc 0 means no command.
#[repr(C)]
pub struct RantConfigCommand {
    pub argv: *const *const c_char,
    pub argc: usize,
}

/// One package's build: the CMake configure when it needs one, then the commands in order,
/// all in dir.
#[repr(C)]
pub struct RantConfigBuildStep {
    pub package: *const c_char,
    pub dir: *const c_char,
    /// The CMake configure command, argc 0 for a package without CMake.
    pub configure: RantConfigCommand,
    /// The CMake build folder is not configured yet.
    pub unconfigured: bool,
    /// The CMake build folder, NULL without CMake.
    pub build_dir: *const c_char,
    /// Where the CMake cache was made when that is not build_dir, else NULL.
    pub moved_from: *const c_char,
    /// What a clean build removes first.
    pub clean: *const *const c_char,
    pub clean_count: usize,
    pub commands: *const RantConfigCommand,
    pub command_count: usize,
}

/// `from` depends on `to`, found where `source` says.
#[repr(C)]
pub struct RantConfigEdge {
    pub from: *const c_char,
    pub to: *const c_char,
    pub source: *const c_char,
}

/// A build plan in build order, and the dependencies between the packages it holds.
#[repr(C)]
pub struct RantConfigBuildView {
    pub steps: *const RantConfigBuildStep,
    pub step_count: usize,
    pub edges: *const RantConfigEdge,
    pub edge_count: usize,
    pub diagnostics: *const RantConfigDiagnostic,
    pub diagnostic_count: usize,
}

/// A build plan and everything its view points into.
pub struct RantConfigBuild {
    view: RantConfigBuildView,
    #[allow(dead_code)]
    store: Store,
}

impl Store {
    fn command(&mut self, argv: &[String]) -> RantConfigCommand {
        let (argv, argc) = self.strs(argv);
        RantConfigCommand { argv, argc }
    }
}

/// The build plan for the named packages and all they depend on, every package when
/// count is 0. Never NULL: failures are in the view's diagnostics.
///
/// # Safety
/// start_dir must be NULL or a NUL terminated UTF-8 string, and packages must point at
/// count such strings.
#[no_mangle]
pub unsafe extern "C" fn rant_config_build(
    start_dir: *const c_char,
    packages: *const *const c_char,
    count: usize,
) -> *mut RantConfigBuild {
    let start = arg_path(start_dir);
    let wanted: Vec<String> = (0..count).map(|i| arg_str(*packages.add(i))).collect();
    let loaded = load(&start, true);
    let mut store = Store::default();
    let mut diags = loaded.diags.clone();
    let planned = match &loaded.model {
        Some(m) if diags.is_empty() => build_plan::plan(m, &wanted).map_err(|d| diags.push(d)).ok(),
        Some(_) => None,
        None => {
            diags.push(no_workspace());
            None
        }
    };
    let plan = planned.unwrap_or_default();
    let steps: Vec<RantConfigBuildStep> = plan
        .steps
        .iter()
        .map(|st| {
            let configure = store.command(st.configure.as_deref().unwrap_or(&[]));
            let commands: Vec<RantConfigCommand> = st.commands.iter().map(|c| store.command(c)).collect();
            let (commands, command_count) = store.array(commands);
            let clean: Vec<String> = st.clean.iter().map(|p| crate::eval::slash(p)).collect();
            let (clean, clean_count) = store.strs(&clean);
            RantConfigBuildStep {
                package: store.str(&st.package),
                dir: store.path(&st.dir),
                configure,
                unconfigured: st.unconfigured,
                build_dir: store.opt_path(st.build_dir.as_deref()),
                moved_from: store.opt_path(st.moved_from.as_deref()),
                clean,
                clean_count,
                commands,
                command_count,
            }
        })
        .collect();
    let (steps, step_count) = store.array(steps);
    let edges: Vec<RantConfigEdge> = plan
        .edges
        .iter()
        .map(|e| RantConfigEdge { from: store.str(&e.from), to: store.str(&e.to), source: store.str(&e.source) })
        .collect();
    let (edges, edge_count) = store.array(edges);
    let (diagnostics, diagnostic_count) = store.diags(&diags);
    let view = RantConfigBuildView { steps, step_count, edges, edge_count, diagnostics, diagnostic_count };
    Box::into_raw(Box::new(RantConfigBuild { view, store }))
}

/// # Safety
/// `build` must be a live handle from rant_config_build.
#[no_mangle]
pub unsafe extern "C" fn rant_config_build_view(build: *const RantConfigBuild) -> *const RantConfigBuildView {
    &(*build).view
}

/// # Safety
/// `build` must be NULL or a live handle, and is invalid afterwards.
#[no_mangle]
pub unsafe extern "C" fn rant_config_build_free(build: *mut RantConfigBuild) {
    if !build.is_null() {
        drop(Box::from_raw(build));
    }
}

/// The Python a folder runs with, by the interpreter rule node types follow.
#[repr(C)]
pub struct RantConfigPythonView {
    /// The nearest venv up to the workspace root, NULL when there is none.
    pub venv: *const c_char,
    /// The venv's own python, NULL without a venv.
    pub venv_python: *const c_char,
    /// What a node there runs with: the venv's python, else the system's.
    pub interpreter: *const *const c_char,
    pub interpreter_count: usize,
    /// One of the folder's own Python files imports rant.
    pub imports_rant: bool,
    /// The enclosing workspace's root, NULL outside one.
    pub root: *const c_char,
}

pub struct RantConfigPython {
    view: RantConfigPythonView,
    #[allow(dead_code)]
    store: Store,
}

/// # Safety
/// `dir` must be NULL or a NUL terminated UTF-8 string.
#[no_mangle]
pub unsafe extern "C" fn rant_config_python(dir: *const c_char) -> *mut RantConfigPython {
    let dir = arg_path(dir);
    let root = workspace::open(&dir).ok().flatten().map(|c| c.root);
    let top = root.clone().unwrap_or_else(|| dir.ancestors().last().unwrap_or(&dir).to_path_buf());
    let venv = crate::model::nearest_venv(&dir, Some(&top));
    let imports_rant = std::fs::read_dir(&dir).into_iter().flatten().flatten().any(|e| {
        let p = e.path();
        p.extension().is_some_and(|x| x == "py") && std::fs::read_to_string(&p).is_ok_and(|t| crate::scan::imports_rant(&t))
    });
    let mut store = Store::default();
    let (interpreter, interpreter_count) = store.strs(&crate::model::python(None, &dir, &top));
    let view = RantConfigPythonView {
        venv: venv.as_deref().map(|v| store.path(v)).unwrap_or(std::ptr::null()),
        venv_python: venv.as_deref().map(|v| store.path(&crate::model::venv_python(v))).unwrap_or(std::ptr::null()),
        interpreter,
        interpreter_count,
        imports_rant,
        root: root.as_deref().map(|r| store.path(r)).unwrap_or(std::ptr::null()),
    };
    Box::into_raw(Box::new(RantConfigPython { view, store }))
}

/// # Safety
/// `p` must be a live handle from rant_config_python.
#[no_mangle]
pub unsafe extern "C" fn rant_config_python_view(p: *const RantConfigPython) -> *const RantConfigPythonView {
    &(*p).view
}

/// # Safety
/// `p` must be NULL or a live handle, and is invalid afterwards.
#[no_mangle]
pub unsafe extern "C" fn rant_config_python_free(p: *mut RantConfigPython) {
    if !p.is_null() {
        drop(Box::from_raw(p));
    }
}

/// The folders under a folder and the venvs among them, as `rant lib` walks them.
#[repr(C)]
pub struct RantConfigFoldersView {
    pub folders: *const *const c_char,
    pub folder_count: usize,
    pub venvs: *const *const c_char,
    pub venv_count: usize,
}

pub struct RantConfigFolders {
    view: RantConfigFoldersView,
    #[allow(dead_code)]
    store: Store,
}

/// Every folder under start, start included, and every venv met on the way, by the
/// enclosing workspace's `ignore` globs when there is one.
///
/// # Safety
/// `start` must be NULL or a NUL terminated UTF-8 string.
#[no_mangle]
pub unsafe extern "C" fn rant_config_folders(start: *const c_char) -> *mut RantConfigFolders {
    let start = arg_path(start);
    let config = workspace::open(&start).ok().flatten();
    let (dirs, venvs) = crate::discover::folders(&start, config.as_ref());
    let mut store = Store::default();
    let dirs: Vec<String> = dirs.iter().map(|d| crate::eval::slash(d)).collect();
    let venvs: Vec<String> = venvs.iter().map(|v| crate::eval::slash(v)).collect();
    let (folders, folder_count) = store.strs(&dirs);
    let (venvs, venv_count) = store.strs(&venvs);
    Box::into_raw(Box::new(RantConfigFolders { view: RantConfigFoldersView { folders, folder_count, venvs, venv_count }, store }))
}

/// # Safety
/// `f` must be a live handle from rant_config_folders.
#[no_mangle]
pub unsafe extern "C" fn rant_config_folders_view(f: *const RantConfigFolders) -> *const RantConfigFoldersView {
    &(*f).view
}

/// # Safety
/// `f` must be NULL or a live handle, and is invalid afterwards.
#[no_mangle]
pub unsafe extern "C" fn rant_config_folders_free(f: *mut RantConfigFolders) {
    if !f.is_null() {
        drop(Box::from_raw(f));
    }
}

/// A package.rant as one release carries it, read before that release is installed.
#[repr(C)]
pub struct RantConfigReleaseView {
    /// Each NULL when the file says nothing.
    pub name: *const c_char,
    pub description: *const c_char,
    /// Whether it is a library, and then its name for each language, NULL for one it leaves out.
    pub library: bool,
    pub cmake: *const c_char,
    pub python: *const c_char,
    pub csharp: *const c_char,
    pub rust: *const c_char,
    pub diagnostics: *const RantConfigDiagnostic,
    pub diagnostic_count: usize,
}

pub struct RantConfigRelease {
    view: RantConfigReleaseView,
    #[allow(dead_code)]
    store: Store,
}

/// Reads package.rant text as if it were in dir, where the release installs.
///
/// # Safety
/// Every argument must be NULL or a NUL terminated UTF-8 string.
#[no_mangle]
pub unsafe extern "C" fn rant_config_release(dir: *const c_char, text: *const c_char) -> *mut RantConfigRelease {
    let dir = arg_path(dir);
    let mut store = Store::default();
    let null = std::ptr::null();
    let mut view = RantConfigReleaseView {
        name: null,
        description: null,
        library: false,
        cmake: null,
        python: null,
        csharp: null,
        rust: null,
        diagnostics: std::ptr::null(),
        diagnostic_count: 0,
    };
    match package::from_text(&dir, arg_str(text), &dir) {
        Ok(p) => {
            let mut opt = |s: Option<String>| s.map(|s| store.str(&s)).unwrap_or(null);
            view.name = opt(p.name);
            view.description = opt(p.description);
            if let Some(l) = p.library {
                view.library = true;
                (view.cmake, view.python, view.csharp, view.rust) = (opt(l.cmake), opt(l.python), opt(l.csharp), opt(l.rust));
            }
        }
        Err(d) => (view.diagnostics, view.diagnostic_count) = store.diags(&[d]),
    }
    Box::into_raw(Box::new(RantConfigRelease { view, store }))
}

/// # Safety
/// `r` must be a live handle from rant_config_release.
#[no_mangle]
pub unsafe extern "C" fn rant_config_release_view(r: *const RantConfigRelease) -> *const RantConfigReleaseView {
    &(*r).view
}

/// # Safety
/// `r` must be NULL or a live handle, and is invalid afterwards.
#[no_mangle]
pub unsafe extern "C" fn rant_config_release_free(r: *mut RantConfigRelease) {
    if !r.is_null() {
        drop(Box::from_raw(r));
    }
}

#[repr(C)]
pub struct RantConfigDatabaseView {
    /// GitHub repos as owner/name, each releasing one package.
    pub repos: *const *const c_char,
    pub repo_count: usize,
    /// Local build folders and release archives, only in sources.rant.
    pub paths: *const *const c_char,
    pub path_count: usize,
    /// The databases sources.rant adds, as owner/name, in the order to read them.
    pub databases: *const *const c_char,
    pub database_count: usize,
    pub diagnostics: *const RantConfigDiagnostic,
    pub diagnostic_count: usize,
}

pub struct RantConfigDatabase {
    view: RantConfigDatabaseView,
    store: Store,
}

/// Reads a database's text, sources.rant when local. file names it in errors, and a relative
/// path in sources.rant is from its folder.
///
/// # Safety
/// file and text must be NULL or NUL terminated UTF-8 strings.
#[no_mangle]
pub unsafe extern "C" fn rant_config_database(file: *const c_char, text: *const c_char, local: bool) -> *mut RantConfigDatabase {
    use std::ptr::null;
    let mut store = Store::default();
    let mut view = RantConfigDatabaseView {
        repos: null(),
        repo_count: 0,
        paths: null(),
        path_count: 0,
        databases: null(),
        database_count: 0,
        diagnostics: null(),
        diagnostic_count: 0,
    };
    match database::parse(&arg_path(file), arg_str(text), local) {
        Ok(db) => {
            (view.repos, view.repo_count) = store.strs(&db.repos);
            let paths: Vec<String> = db.paths.iter().map(|p| crate::eval::slash(p)).collect();
            (view.paths, view.path_count) = store.strs(&paths);
            (view.databases, view.database_count) = store.strs(&db.databases);
        }
        Err(d) => (view.diagnostics, view.diagnostic_count) = store.diags(&[d]),
    }
    Box::into_raw(Box::new(RantConfigDatabase { view, store }))
}

/// # Safety
/// `d` must be a live handle from rant_config_database.
#[no_mangle]
pub unsafe extern "C" fn rant_config_database_view(d: *const RantConfigDatabase) -> *const RantConfigDatabaseView {
    &(*d).view
}

/// # Safety
/// `d` must be NULL or a live handle, and is invalid afterwards.
#[no_mangle]
pub unsafe extern "C" fn rant_config_database_free(d: *mut RantConfigDatabase) {
    if !d.is_null() {
        drop(Box::from_raw(d));
    }
}

/// Whether a name can name a package: lowercase letters, digits and _, starting with a letter.
///
/// # Safety
/// name must be NULL or a NUL terminated UTF-8 string.
#[no_mangle]
pub unsafe extern "C" fn rant_config_is_package_name(name: *const c_char) -> bool {
    database::is_name(&arg_str(name))
}

/// Writes the GitHub repo a word names, owner/name itself or a link into the repo, as owner/name
/// into out, NUL terminated. False when it names none or out holds less than cap bytes.
///
/// # Safety
/// word must be NULL or a NUL terminated UTF-8 string, and out must hold cap bytes.
#[no_mangle]
pub unsafe extern "C" fn rant_config_repo_of(word: *const c_char, out: *mut c_char, cap: usize) -> bool {
    match database::repo_of(&arg_str(word)) {
        Some(repo) if repo.len() < cap => {
            std::ptr::copy_nonoverlapping(repo.as_ptr() as *const c_char, out, repo.len());
            *out.add(repo.len()) = 0;
            true
        }
        _ => false,
    }
}

/// Whether two names are one, matching without case or underscores.
///
/// # Safety
/// Both arguments must be NULL or NUL terminated UTF-8 strings.
#[no_mangle]
pub unsafe extern "C" fn rant_config_same_name(a: *const c_char, b: *const c_char) -> bool {
    refs::same_name(&arg_str(a), &arg_str(b))
}

/// The extension a .vsix file holds.
#[repr(C)]
pub struct RantConfigVsixView {
    /// publisher.name in lowercase, NULL when the file is no extension.
    pub id: *const c_char,
    pub version: *const c_char,
    pub diagnostics: *const RantConfigDiagnostic,
    pub diagnostic_count: usize,
}

pub struct RantConfigVsix {
    view: RantConfigVsixView,
    #[allow(dead_code)]
    store: Store,
}

/// Reads the extension's id and version from a .vsix file.
///
/// # Safety
/// path must be NULL or a NUL terminated UTF-8 string.
#[no_mangle]
pub unsafe extern "C" fn rant_config_vsix(path: *const c_char) -> *mut RantConfigVsix {
    let mut store = Store::default();
    let mut view = RantConfigVsixView { id: std::ptr::null(), version: std::ptr::null(), diagnostics: std::ptr::null(), diagnostic_count: 0 };
    match vsix::read(&arg_path(path)) {
        Ok(e) => {
            view.id = store.str(&e.id);
            view.version = store.str(&e.version);
        }
        Err(d) => (view.diagnostics, view.diagnostic_count) = store.diags(&[d]),
    }
    Box::into_raw(Box::new(RantConfigVsix { view, store }))
}

/// # Safety
/// `v` must be a live handle from rant_config_vsix.
#[no_mangle]
pub unsafe extern "C" fn rant_config_vsix_view(v: *const RantConfigVsix) -> *const RantConfigVsixView {
    &(*v).view
}

/// # Safety
/// `v` must be NULL or a live handle, and is invalid afterwards.
#[no_mangle]
pub unsafe extern "C" fn rant_config_vsix_free(v: *mut RantConfigVsix) {
    if !v.is_null() {
        drop(Box::from_raw(v));
    }
}
