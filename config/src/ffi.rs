//! The C ABI. Every handle owns the strings and arrays its view points into, so a view
//! stays valid until its handle is freed. cbindgen writes rant_config.h from this file.

use std::ffi::{c_char, CStr, CString};
use std::path::{Path, PathBuf};

use crate::diag::Diag;
use crate::workspace::{self, WorkspaceConfig};

/// One located error. line and column are 1 based, 0 means the whole file.
#[repr(C)]
pub struct RantConfigDiagnostic {
    pub file: *const c_char,
    pub line: u32,
    pub column: u32,
    pub message: *const c_char,
}

/// What rant_config_open found. root is NULL when no workspace encloses the directory.
#[repr(C)]
pub struct RantConfigWorkspaceView {
    pub root: *const c_char,
    pub logs: *const c_char,
    pub diagnostics: *const RantConfigDiagnostic,
    pub diagnostic_count: usize,
}

/// An opened workspace and everything its view points into.
pub struct RantConfigWorkspace {
    view: RantConfigWorkspaceView,
    #[allow(dead_code)]
    store: Store,
}

/// Owned C strings and arrays. A CString's bytes never move, so pointers stay valid as
/// the store grows.
#[derive(Default)]
pub(crate) struct Store {
    strings: Vec<CString>,
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

    pub fn diags(&mut self, diags: &[Diag]) -> Vec<RantConfigDiagnostic> {
        diags
            .iter()
            .map(|d| RantConfigDiagnostic {
                file: self.path(&d.file),
                line: d.line,
                column: d.column,
                message: self.str(&d.message),
            })
            .collect()
    }
}

/// Leaks a vector as a pointer and length for a view. Freed by `free_slice`.
fn leak<T>(v: Vec<T>) -> (*const T, usize) {
    let b = v.into_boxed_slice();
    let len = b.len();
    (Box::into_raw(b) as *const T, len)
}

/// # Safety
/// `ptr` and `len` must come from one call to `leak`.
unsafe fn free_slice<T>(ptr: *const T, len: usize) {
    if !ptr.is_null() {
        drop(Box::from_raw(std::ptr::slice_from_raw_parts_mut(ptr as *mut T, len)));
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

fn workspace_handle(ws: Option<WorkspaceConfig>, diags: Vec<Diag>) -> *mut RantConfigWorkspace {
    let mut store = Store::default();
    let root = ws.as_ref().map(|w| store.path(&w.root)).unwrap_or(std::ptr::null());
    let logs = ws.as_ref().map(|w| store.path(&w.logs)).unwrap_or(std::ptr::null());
    let (diagnostics, diagnostic_count) = leak(store.diags(&diags));
    let view = RantConfigWorkspaceView { root, logs, diagnostics, diagnostic_count };
    Box::into_raw(Box::new(RantConfigWorkspace { view, store }))
}

impl Drop for RantConfigWorkspace {
    fn drop(&mut self) {
        unsafe { free_slice(self.view.diagnostics, self.view.diagnostic_count) };
    }
}

/// Finds the workspace enclosing start_dir (NULL = the current directory). Never NULL:
/// a failure is in the view's diagnostics.
///
/// # Safety
/// `start_dir` must be NULL or a NUL terminated UTF-8 string.
#[no_mangle]
pub unsafe extern "C" fn rant_config_open(start_dir: *const c_char) -> *mut RantConfigWorkspace {
    match workspace::open(&arg_path(start_dir)) {
        Ok(ws) => workspace_handle(ws, Vec::new()),
        Err(d) => workspace_handle(None, vec![d]),
    }
}

/// Writes a new rant.hcl into dir and opens it. Refused when dir already has one.
///
/// # Safety
/// `dir` must be NULL or a NUL terminated UTF-8 string.
#[no_mangle]
pub unsafe extern "C" fn rant_config_init(dir: *const c_char) -> *mut RantConfigWorkspace {
    match workspace::init(&arg_path(dir)) {
        Ok(root) => rant_config_open_path(&root),
        Err(d) => workspace_handle(None, vec![d]),
    }
}

fn rant_config_open_path(dir: &Path) -> *mut RantConfigWorkspace {
    match workspace::open(dir) {
        Ok(ws) => workspace_handle(ws, Vec::new()),
        Err(d) => workspace_handle(None, vec![d]),
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
