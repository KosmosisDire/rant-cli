//! The C ABI for the template.hcl of a `rant new` template. Same rules as `ffi`: every
//! handle owns what its view points into.

use std::ffi::c_char;

use crate::ffi::{arg_path, arg_str, RantConfigDiagnostic, RantConfigParam, Store};
use crate::template;

/// What a template.hcl declares. values is every param's value as a JSON object when the
/// call bound params, else NULL. Nothing but diagnostics is set when they are not empty.
#[repr(C)]
pub struct RantConfigTemplateView {
    pub description: *const c_char,
    pub includes: *const *const c_char,
    pub include_count: usize,
    pub next: *const c_char,
    pub params: *const RantConfigParam,
    pub param_count: usize,
    pub values: *const c_char,
    pub diagnostics: *const RantConfigDiagnostic,
    pub diagnostic_count: usize,
}

pub struct RantConfigTemplate {
    view: RantConfigTemplateView,
    #[allow(dead_code)]
    store: Store,
}

/// Reads a template.hcl from its text, path naming it in errors, and with bind checks the
/// `key=value` params given against it. Never NULL: failures are in the view's diagnostics.
///
/// # Safety
/// `text` and `path` must be NUL terminated UTF-8 strings, and `params` an array of count
/// of them.
#[no_mangle]
pub unsafe extern "C" fn rant_config_template(
    text: *const c_char,
    path: *const c_char,
    params: *const *const c_char,
    count: usize,
    bind: bool,
) -> *mut RantConfigTemplate {
    let raw: Vec<String> = (0..count).map(|i| arg_str(*params.add(i))).collect();
    let mut store = Store::default();
    let mut view = RantConfigTemplateView {
        description: std::ptr::null(),
        includes: std::ptr::null(),
        include_count: 0,
        next: std::ptr::null(),
        params: std::ptr::null(),
        param_count: 0,
        values: std::ptr::null(),
        diagnostics: std::ptr::null(),
        diagnostic_count: 0,
    };
    let read = template::read(&arg_str(text), &arg_path(path))
        .and_then(|m| if bind { template::bind(&m, &raw).map(|v| (m, Some(v))) } else { Ok((m, None)) });
    match read {
        Ok((m, values)) => {
            view.description = m.description.as_deref().map(|d| store.str(d)).unwrap_or(std::ptr::null());
            (view.includes, view.include_count) = store.strs(&m.include);
            view.next = m.next.as_deref().map(|n| store.str(n)).unwrap_or(std::ptr::null());
            (view.params, view.param_count) = store.params(&m.params);
            view.values = values.as_deref().map(|v| store.str(v)).unwrap_or(std::ptr::null());
        }
        Err(d) => (view.diagnostics, view.diagnostic_count) = store.diags(&[d]),
    }
    Box::into_raw(Box::new(RantConfigTemplate { view, store }))
}

/// # Safety
/// `t` must be a live handle from rant_config_template.
#[no_mangle]
pub unsafe extern "C" fn rant_config_template_view(t: *const RantConfigTemplate) -> *const RantConfigTemplateView {
    &(*t).view
}

/// # Safety
/// `t` must be NULL or a live handle, and is invalid afterwards.
#[no_mangle]
pub unsafe extern "C" fn rant_config_template_free(t: *mut RantConfigTemplate) {
    if !t.is_null() {
        drop(Box::from_raw(t));
    }
}
