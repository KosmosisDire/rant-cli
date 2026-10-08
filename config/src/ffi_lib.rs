//! The C ABI for adding and updating the Rant library in packages, and for the GitHub
//! releases and downloads that `rant lib` and `rant explore` need. Same rules as `ffi`:
//! every handle owns what its view points into.

use std::ffi::c_char;
use std::path::{Path, PathBuf};

use crate::ffi::{arg_path, arg_str, Store};
use crate::lib_use::{self, How, Kind};
use crate::model::{python, venv_python};
use crate::release;
use crate::workspace;

#[repr(C)]
#[derive(Clone, Copy)]
pub enum RantConfigLibKind {
    CMake,
    Python,
    CSharp,
}

/// One place a package takes Rant from.
#[repr(C)]
pub struct RantConfigLibUse {
    pub kind: RantConfigLibKind,
    /// "CPM", "FetchContent", "find_package", "pyproject", "venv", "PackageReference" or
    /// "ProjectReference".
    pub how: *const c_char,
    pub dir: *const c_char,
    /// The build file, or for a venv its folder, which may not exist yet.
    pub file: *const c_char,
    /// NULL when no version is named, or the venv lacks Rant.
    pub version: *const c_char,
    /// For a venv: whether it exists, its own python, and the interpreter that makes it.
    pub venv_exists: bool,
    pub venv_python: *const c_char,
    pub python: *const *const c_char,
    pub python_count: usize,
}

#[repr(C)]
pub struct RantConfigLibUsesView {
    pub uses: *const RantConfigLibUse,
    pub use_count: usize,
}

pub struct RantConfigLibUses {
    view: RantConfigLibUsesView,
    #[allow(dead_code)]
    store: Store,
}

/// What an action came to: error is NULL on success, and note may carry a line for the user.
#[repr(C)]
pub struct RantConfigOutcomeView {
    pub error: *const c_char,
    pub note: *const c_char,
}

pub struct RantConfigOutcome {
    view: RantConfigOutcomeView,
    #[allow(dead_code)]
    store: Store,
}

#[repr(C)]
pub struct RantConfigAsset {
    pub name: *const c_char,
    pub url: *const c_char,
}

/// A release, or error when it could not be found. version is the tag without its `v`.
#[repr(C)]
pub struct RantConfigReleaseView {
    pub tag: *const c_char,
    pub version: *const c_char,
    pub assets: *const RantConfigAsset,
    pub asset_count: usize,
    pub error: *const c_char,
}

pub struct RantConfigRelease {
    view: RantConfigReleaseView,
    #[allow(dead_code)]
    store: Store,
}

fn kind(k: Kind) -> RantConfigLibKind {
    match k {
        Kind::CMake => RantConfigLibKind::CMake,
        Kind::Python => RantConfigLibKind::Python,
        Kind::CSharp => RantConfigLibKind::CSharp,
    }
}

/// Where a Python folder with no venv gets one: beside its pyproject, else at the workspace
/// root, else in the folder.
fn new_venv(dir: &Path, root: Option<&Path>) -> PathBuf {
    if dir.join("pyproject.toml").is_file() {
        return dir.join(".venv");
    }
    root.unwrap_or(dir).join(".venv")
}

fn outcome(result: Result<Option<String>, String>) -> *mut RantConfigOutcome {
    let mut store = Store::default();
    let view = match result {
        Ok(note) => RantConfigOutcomeView {
            error: std::ptr::null(),
            note: note.map(|n| store.str(&n)).unwrap_or(std::ptr::null()),
        },
        Err(e) => RantConfigOutcomeView { error: store.str(&e), note: std::ptr::null() },
    };
    Box::into_raw(Box::new(RantConfigOutcome { view, store }))
}

/// The uses of Rant in the folder at path, where any Python file counts, or with recursive
/// in every folder under it, where only Python that imports rant does. Venvs are looked for
/// up to the enclosing workspace root, if there is one.
///
/// # Safety
/// `path` must be NULL or a NUL terminated UTF-8 string.
#[no_mangle]
pub unsafe extern "C" fn rant_config_lib_uses(path: *const c_char, recursive: bool) -> *mut RantConfigLibUses {
    let dir = arg_path(path);
    let root = workspace::open(&dir).ok().flatten().map(|c| c.root);
    let mut uses = if recursive { lib_use::under(&dir, root.as_deref()) } else { lib_use::in_folder(&dir, root.as_deref(), true) };
    for u in uses.iter_mut().filter(|u| u.how == How::Venv && u.file.as_os_str().is_empty()) {
        u.file = new_venv(&u.dir, root.as_deref());
    }
    let mut store = Store::default();
    let views: Vec<RantConfigLibUse> = uses
        .iter()
        .map(|u| {
            let venv = u.how == How::Venv;
            let exists = venv && u.file.join("pyvenv.cfg").is_file();
            let interpreter = if venv && !exists { python(None, &u.dir, root.as_deref().unwrap_or(&u.dir)) } else { Vec::new() };
            let (python, python_count) = store.strs(&interpreter);
            RantConfigLibUse {
                kind: kind(u.kind),
                how: store.str(u.how.name()),
                dir: store.path(&u.dir),
                file: store.path(&u.file),
                version: u.version.as_deref().map(|v| store.str(v)).unwrap_or(std::ptr::null()),
                venv_exists: exists,
                venv_python: if venv { store.path(&venv_python(&u.file)) } else { std::ptr::null() },
                python,
                python_count,
            }
        })
        .collect();
    let (ptr, use_count) = store.array(views);
    Box::into_raw(Box::new(RantConfigLibUses { view: RantConfigLibUsesView { uses: ptr, use_count }, store }))
}

/// # Safety
/// `uses` must be a live handle from rant_config_lib_uses.
#[no_mangle]
pub unsafe extern "C" fn rant_config_lib_uses_view(uses: *const RantConfigLibUses) -> *const RantConfigLibUsesView {
    &(*uses).view
}

/// # Safety
/// `uses` must be NULL or a live handle, and is invalid afterwards.
#[no_mangle]
pub unsafe extern "C" fn rant_config_lib_uses_free(uses: *mut RantConfigLibUses) {
    if !uses.is_null() {
        drop(Box::from_raw(uses));
    }
}

/// Moves the Rant version a CMakeLists, pyproject or C# project names. A venv is not
/// edited: pip installs into it.
///
/// # Safety
/// `file` and `version` must be NUL terminated UTF-8 strings.
#[no_mangle]
pub unsafe extern "C" fn rant_config_lib_set(file: *const c_char, version: *const c_char) -> *mut RantConfigOutcome {
    outcome(lib_use::set(&arg_path(file), &arg_str(version)).map(|_| None))
}

/// Adds Rant at version to a CMakeLists or a C# project. The note, when there is one, is a
/// line still to add by hand.
///
/// # Safety
/// `file` and `version` must be NUL terminated UTF-8 strings.
#[no_mangle]
pub unsafe extern "C" fn rant_config_lib_add(lib_kind: RantConfigLibKind, file: *const c_char, version: *const c_char) -> *mut RantConfigOutcome {
    let k = match lib_kind {
        RantConfigLibKind::CMake => Kind::CMake,
        RantConfigLibKind::Python => Kind::Python,
        RantConfigLibKind::CSharp => Kind::CSharp,
    };
    outcome(lib_use::add(k, &arg_path(file), &arg_str(version)))
}

/// The release of `owner/name` tagged tag, or the latest when tag is NULL. Never NULL:
/// a failure is in the view's error.
///
/// # Safety
/// `repo` must be a NUL terminated UTF-8 string, `tag` NULL or one.
#[no_mangle]
pub unsafe extern "C" fn rant_config_release(repo: *const c_char, tag: *const c_char) -> *mut RantConfigRelease {
    let tag = (!tag.is_null()).then(|| arg_str(tag));
    let mut store = Store::default();
    let view = match release::release(&arg_str(repo), tag.as_deref()) {
        Ok(r) => {
            let assets: Vec<RantConfigAsset> =
                r.assets.iter().map(|a| RantConfigAsset { name: store.str(&a.name), url: store.str(&a.url) }).collect();
            let (assets, asset_count) = store.array(assets);
            RantConfigReleaseView { tag: store.str(&r.tag), version: store.str(r.version()), assets, asset_count, error: std::ptr::null() }
        }
        Err(e) => RantConfigReleaseView {
            tag: std::ptr::null(),
            version: std::ptr::null(),
            assets: std::ptr::null(),
            asset_count: 0,
            error: store.str(&e),
        },
    };
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

/// Downloads url to dest, which is whole or untouched afterwards.
///
/// # Safety
/// Both arguments must be NUL terminated UTF-8 strings.
#[no_mangle]
pub unsafe extern "C" fn rant_config_download(url: *const c_char, dest: *const c_char) -> *mut RantConfigOutcome {
    outcome(release::download(&arg_str(url), &arg_path(dest)).map(|_| None))
}

/// # Safety
/// `o` must be a live handle from a function returning an outcome.
#[no_mangle]
pub unsafe extern "C" fn rant_config_outcome_view(o: *const RantConfigOutcome) -> *const RantConfigOutcomeView {
    &(*o).view
}

/// # Safety
/// `o` must be NULL or a live handle, and is invalid afterwards.
#[no_mangle]
pub unsafe extern "C" fn rant_config_outcome_free(o: *mut RantConfigOutcome) {
    if !o.is_null() {
        drop(Box::from_raw(o));
    }
}
