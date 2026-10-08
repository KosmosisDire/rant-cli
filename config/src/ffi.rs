//! The C ABI. Every handle owns the strings and arrays its view points into, so a view
//! stays valid until its handle is freed. cbindgen writes rant_config.h from this file.

use std::any::Any;
use std::collections::BTreeMap;
use std::ffi::{c_char, CStr, CString};
use std::path::{Path, PathBuf};
use std::rc::Rc;

use crate::build_plan;
use crate::diag::Diag;
use crate::group::{self, GroupDef, Groups, Param};
use crate::model::{Model, NodeType, Package};
use crate::paths;
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

/// A group file and the name it goes by.
#[repr(C)]
pub struct RantConfigGroupFile {
    pub name: *const c_char,
    pub file: *const c_char,
}

/// What rant_config_open found. root is NULL when no workspace encloses the directory.
/// data is the workspace's .rant directory. packages and groups are filled only when
/// RANT_CONFIG_PACKAGES was asked for.
#[repr(C)]
pub struct RantConfigWorkspaceView {
    pub root: *const c_char,
    pub logs: *const c_char,
    pub data: *const c_char,
    pub packages: *const RantConfigPackage,
    pub package_count: usize,
    /// Python nodes outside every package.
    pub loose: *const RantConfigNodeType,
    pub loose_count: usize,
    pub groups: *const RantConfigGroupFile,
    pub group_count: usize,
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
    let loose: Vec<RantConfigNodeType> = loaded.model.iter().flat_map(|m| m.loose.iter()).map(|n| store.node_type(n)).collect();
    let (loose, loose_count) = store.array(loose);
    let groups: Vec<RantConfigGroupFile> = loaded
        .model
        .iter()
        .flat_map(|m| m.groups.iter())
        .map(|g| RantConfigGroupFile { name: store.str(&g.name), file: store.path(&g.path) })
        .collect();
    let (groups, group_count) = store.array(groups);
    let diags: Vec<Diag> = loaded.diags.iter().cloned().chain(extra).collect();
    let (diagnostics, diagnostic_count) = store.diags(&diags);
    let [root, logs, data] = store.roots(loaded.config.as_ref());
    let view = RantConfigWorkspaceView {
        root,
        logs,
        data,
        packages,
        package_count,
        loose,
        loose_count,
        groups,
        group_count,
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
                values.iter().map(|(k, v)| RantConfigPair { name: store.str(k), value: store.str(&group::text(v)) }).collect();
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
    let planned = match &loaded.model {
        Some(m) => plan::single(m, &arg_str(node_type), &start).map(|plan| Planned { plan, group: None }),
        None => Err(no_workspace()),
    };
    plan_handle(&loaded, planned)
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
    let mut given = BTreeMap::new();
    for kv in raw {
        let Some((k, v)) = kv.split_once('=') else {
            return Err(Diag::plain(format!("params are key=value, not `{kv}`")));
        };
        let Some(p) = iface.iter().find(|p| p.name == k) else {
            return Err(Diag::plain(format!(
                "group `{}` has no param `{k}`, see `rant start group {} --help`",
                def.file.name, def.file.name
            )));
        };
        if given.insert(k.to_string(), group::parse_value(p, v).map_err(Diag::plain)?).is_some() {
            return Err(Diag::plain(format!("param `{k}` is given twice")));
        }
    }
    let (plan, values) = groups.plan(&def, given)?;
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

/// One param of a group's interface. default_value is NULL for a required param.
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
        diagnostics: std::ptr::null(),
        diagnostic_count: 0,
    };
    let described = match &loaded.model {
        Some(m) => {
            let groups = Groups::new(m);
            group_def(&groups, m, &start, &arg_str(group))
        }
        None => Err(no_workspace()),
    };
    match described {
        Ok((def, iface)) if diags.is_empty() => {
            view.name = store.str(&def.file.name);
            view.file = store.path(&def.file.path);
            view.description = def.description.as_deref().map(|d| store.str(d)).unwrap_or(std::ptr::null());
            let params: Vec<RantConfigParam> = iface
                .iter()
                .map(|p| {
                    let opts: Vec<String> = p.options.iter().map(group::text).collect();
                    let (options, option_count) = store.strs(&opts);
                    RantConfigParam {
                        name: store.str(&p.name),
                        type_name: store.str(p.ty.name()),
                        default_value: p.default.as_ref().map(|d| store.str(&group::text(d))).unwrap_or(std::ptr::null()),
                        options,
                        option_count,
                        description: p.description.as_deref().map(|d| store.str(d)).unwrap_or(std::ptr::null()),
                    }
                })
                .collect();
            (view.params, view.param_count) = store.array(params);
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

/// One package's build: configure, when not empty, runs first and only after the user
/// agrees, then the commands in order, all in dir.
#[repr(C)]
pub struct RantConfigBuildStep {
    pub package: *const c_char,
    pub dir: *const c_char,
    pub configure: RantConfigCommand,
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
            RantConfigBuildStep { package: store.str(&st.package), dir: store.path(&st.dir), configure, commands, command_count }
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
