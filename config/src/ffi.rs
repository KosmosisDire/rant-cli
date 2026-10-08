//! The C ABI. Every handle owns the strings and arrays its view points into, so a view
//! stays valid until its handle is freed. cbindgen writes rant_config.h from this file.

use std::any::Any;
use std::ffi::{c_char, CStr, CString};
use std::path::{Path, PathBuf};

use crate::diag::Diag;
use crate::model::{Model, NodeType, Package};
use crate::plan::{self, Plan};
use crate::scan::NodeKind;
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
}

/// A runnable node type. path is NULL for a node declared in a package block.
#[repr(C)]
pub struct RantConfigNodeType {
    pub package: *const c_char,
    pub name: *const c_char,
    pub kind: RantConfigNodeKind,
    pub path: *const c_char,
    pub run: *const *const c_char,
    pub run_count: usize,
    pub cwd: *const c_char,
}

#[repr(C)]
pub struct RantConfigPackage {
    pub name: *const c_char,
    pub dir: *const c_char,
    pub nodes: *const RantConfigNodeType,
    pub node_count: usize,
}

/// What rant_config_open found. root is NULL when no workspace encloses the directory.
/// data is the workspace's .rant directory. packages is filled only when
/// RANT_CONFIG_PACKAGES was asked for.
#[repr(C)]
pub struct RantConfigWorkspaceView {
    pub root: *const c_char,
    pub logs: *const c_char,
    pub data: *const c_char,
    pub packages: *const RantConfigPackage,
    pub package_count: usize,
    pub diagnostics: *const RantConfigDiagnostic,
    pub diagnostic_count: usize,
}

/// An opened workspace and everything its view points into.
pub struct RantConfigWorkspace {
    view: RantConfigWorkspaceView,
    #[allow(dead_code)]
    store: Store,
}

/// One environment entry a node gets on top of the inherited environment.
#[repr(C)]
pub struct RantConfigEnvVar {
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
    pub env: *const RantConfigEnvVar,
    pub env_count: usize,
    pub cwd: *const c_char,
}

/// A resolved plan in start order, every path absolute. Empty when diagnostics are not.
#[repr(C)]
pub struct RantConfigPlanView {
    pub root: *const c_char,
    pub logs: *const c_char,
    pub data: *const c_char,
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
        }
    }

    fn package(&mut self, p: &Package) -> RantConfigPackage {
        let nodes: Vec<RantConfigNodeType> = p.nodes.iter().map(|n| self.node_type(n)).collect();
        let (nodes, node_count) = self.array(nodes);
        RantConfigPackage { name: self.str(&p.name), dir: self.path(&p.dir), nodes, node_count }
    }

    fn instance(&mut self, i: &plan::Instance) -> RantConfigInstance {
        let (argv, argc) = self.strs(&i.argv);
        let env: Vec<RantConfigEnvVar> =
            i.env.iter().map(|(k, v)| RantConfigEnvVar { name: self.str(k), value: self.str(v) }).collect();
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
        }
    }

    /// The root, logs and data paths of a view, all NULL outside a workspace.
    fn roots(&mut self, config: Option<&WorkspaceConfig>) -> [*const c_char; 3] {
        match config {
            Some(c) => [self.path(&c.root), self.path(&c.logs), self.path(&workspace::data_dir(&c.root))],
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
    }
}

/// # Safety
/// `s` must be NULL or a NUL terminated string.
unsafe fn arg_str(s: *const c_char) -> String {
    if s.is_null() {
        return String::new();
    }
    CStr::from_ptr(s).to_string_lossy().into_owned()
}

/// # Safety
/// `s` must be NULL or a NUL terminated string.
unsafe fn arg_path(s: *const c_char) -> PathBuf {
    if s.is_null() {
        return PathBuf::from(".");
    }
    PathBuf::from(arg_str(s))
}

/// The workspace enclosing a directory, its model when asked for, and every error met.
struct Loaded {
    config: Option<WorkspaceConfig>,
    model: Option<Model>,
    diags: Vec<Diag>,
}

fn load(start: &Path, packages: bool) -> Loaded {
    let mut diags = Vec::new();
    let config = workspace::open(start).unwrap_or_else(|d| {
        diags.push(d);
        None
    });
    let model = match (&config, packages) {
        (Some(c), true) => {
            let (m, d) = Model::load(c.clone());
            diags.extend(d);
            Some(m)
        }
        _ => None,
    };
    Loaded { config, model, diags }
}

fn workspace_handle(loaded: &Loaded, extra: Vec<Diag>) -> *mut RantConfigWorkspace {
    let mut store = Store::default();
    let packages: Vec<RantConfigPackage> =
        loaded.model.iter().flat_map(|m| m.packages.iter()).map(|p| store.package(p)).collect();
    let (packages, package_count) = store.array(packages);
    let diags: Vec<Diag> = loaded.diags.iter().cloned().chain(extra).collect();
    let (diagnostics, diagnostic_count) = store.diags(&diags);
    let [root, logs, data] = store.roots(loaded.config.as_ref());
    let view = RantConfigWorkspaceView { root, logs, data, packages, package_count, diagnostics, diagnostic_count };
    Box::into_raw(Box::new(RantConfigWorkspace { view, store }))
}

fn plan_handle(loaded: &Loaded, plan: Result<Plan, Diag>) -> *mut RantConfigPlan {
    let mut store = Store::default();
    let mut diags = loaded.diags.clone();
    let plan = match plan {
        Ok(p) if diags.is_empty() => p,
        Ok(_) => Plan::default(),
        Err(d) => {
            diags.push(d);
            Plan::default()
        }
    };
    let instances: Vec<RantConfigInstance> = plan.instances.iter().map(|i| store.instance(i)).collect();
    let (instances, instance_count) = store.array(instances);
    let (diagnostics, diagnostic_count) = store.diags(&diags);
    let [root, logs, data] = store.roots(loaded.config.as_ref());
    let view = RantConfigPlanView { root, logs, data, instances, instance_count, diagnostics, diagnostic_count };
    Box::into_raw(Box::new(RantConfigPlan { view, store }))
}

fn no_workspace() -> Diag {
    Diag::plain("no rant.hcl with a workspace block here or above, run `rant init` to make one")
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

/// Writes a new rant.hcl into dir and opens it. Refused when dir already has one.
///
/// # Safety
/// `dir` must be NULL or a NUL terminated UTF-8 string.
#[no_mangle]
pub unsafe extern "C" fn rant_config_init(dir: *const c_char) -> *mut RantConfigWorkspace {
    match workspace::init(&arg_path(dir)) {
        Ok(root) => workspace_handle(&load(&root, false), Vec::new()),
        Err(d) => workspace_handle(&Loaded { config: None, model: None, diags: Vec::new() }, vec![d]),
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
    let plan = match &loaded.model {
        Some(m) => plan::single(m, &arg_str(node_type), &start),
        None => Err(no_workspace()),
    };
    plan_handle(&loaded, plan)
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
