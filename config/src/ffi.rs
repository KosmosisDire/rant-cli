//! The C ABI. Every handle owns the strings and arrays its view points into, so a view
//! stays valid until its handle is freed. cbindgen writes rant_config.h from this file.

use std::any::Any;
use std::ffi::{c_char, CStr, CString};
use std::path::{Path, PathBuf};

use crate::diag::Diag;
use crate::model::{Model, NodeType, Package};
use crate::scan::NodeKind;
use crate::workspace;

/// rant_config_open flag: also find every package and scan it for node types.
pub const RANT_CONFIG_PACKAGES: u32 = 1;

/// One located error. line and column are 1 based, 0 means the whole file.
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
            .map(|d| RantConfigDiagnostic { file: self.path(&d.file), line: d.line, column: d.column, message: self.str(&d.message) })
            .collect();
        self.array(v)
    }

    pub fn node_type(&mut self, n: &NodeType) -> RantConfigNodeType {
        let (run, run_count) = self.strs(&n.run);
        RantConfigNodeType {
            package: self.str(&n.package),
            name: self.str(&n.name),
            kind: match n.kind {
                NodeKind::Native => RantConfigNodeKind::Native,
                NodeKind::Python => RantConfigNodeKind::Python,
                NodeKind::CSharp => RantConfigNodeKind::CSharp,
                NodeKind::Declared => RantConfigNodeKind::Declared,
            },
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
}

/// # Safety
/// `s` must be NULL or a NUL terminated string.
unsafe fn arg_path(s: *const c_char) -> PathBuf {
    if s.is_null() {
        return PathBuf::from(".");
    }
    PathBuf::from(CStr::from_ptr(s).to_string_lossy().into_owned())
}

fn open(start: &Path, flags: u32) -> *mut RantConfigWorkspace {
    let mut store = Store::default();
    let mut diags = Vec::new();
    let config = match workspace::open(start) {
        Ok(c) => c,
        Err(d) => {
            diags.push(d);
            None
        }
    };
    let mut model = None;
    if let (Some(c), true) = (&config, flags & RANT_CONFIG_PACKAGES != 0) {
        let (m, d) = Model::load(c.clone());
        diags.extend(d);
        model = Some(m);
    }
    let packages: Vec<RantConfigPackage> = model.iter().flat_map(|m| m.packages.iter()).map(|p| store.package(p)).collect();
    let (packages, package_count) = store.array(packages);
    let (diagnostics, diagnostic_count) = store.diags(&diags);
    let view = RantConfigWorkspaceView {
        root: config.as_ref().map(|c| store.path(&c.root)).unwrap_or(std::ptr::null()),
        logs: config.as_ref().map(|c| store.path(&c.logs)).unwrap_or(std::ptr::null()),
        data: config.as_ref().map(|c| store.path(&workspace::data_dir(&c.root))).unwrap_or(std::ptr::null()),
        packages,
        package_count,
        diagnostics,
        diagnostic_count,
    };
    Box::into_raw(Box::new(RantConfigWorkspace { view, store }))
}

/// Finds the workspace enclosing start_dir (NULL = the current directory) and loads what
/// flags ask for. Never NULL: failures are in the view's diagnostics.
///
/// # Safety
/// `start_dir` must be NULL or a NUL terminated UTF-8 string.
#[no_mangle]
pub unsafe extern "C" fn rant_config_open(start_dir: *const c_char, flags: u32) -> *mut RantConfigWorkspace {
    open(&arg_path(start_dir), flags)
}

/// Writes a new rant.hcl into dir and opens it. Refused when dir already has one.
///
/// # Safety
/// `dir` must be NULL or a NUL terminated UTF-8 string.
#[no_mangle]
pub unsafe extern "C" fn rant_config_init(dir: *const c_char) -> *mut RantConfigWorkspace {
    match workspace::init(&arg_path(dir)) {
        Ok(root) => open(&root, 0),
        Err(d) => {
            let mut store = Store::default();
            let (diagnostics, diagnostic_count) = store.diags(&[d]);
            let (packages, package_count) = store.array(Vec::<RantConfigPackage>::new());
            let view = RantConfigWorkspaceView {
                root: std::ptr::null(),
                logs: std::ptr::null(),
                data: std::ptr::null(),
                packages,
                package_count,
                diagnostics,
                diagnostic_count,
            };
            Box::into_raw(Box::new(RantConfigWorkspace { view, store }))
        }
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
