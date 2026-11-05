//! Names as Rant spells them, by the rule of its C library, so a group made here is named as
//! Rant names a type.

use std::ffi::{CString, c_char};

// SAFETY: Rant's C library, which the rant crate links, defines it as declared here.
unsafe extern "C" {
    fn rant_type_name(name: *const c_char, out: *mut c_char, cap: usize) -> usize;
}

/// PascalCase as Rant spells a type: `lidar_driver` is `LidarDriver`. A name with no word in
/// it stays as it is.
pub fn pascal(name: &str) -> String {
    let Ok(c) = CString::new(name) else { return name.to_string() };
    let mut out = [0u8; 256];
    // SAFETY: c is NUL terminated and out holds the cap bytes passed.
    let n = unsafe { rant_type_name(c.as_ptr(), out.as_mut_ptr().cast(), out.len()) };
    if n == 0 { name.to_string() } else { String::from_utf8_lossy(&out[..n]).into_owned() }
}

#[cfg(test)]
mod tests {
    use super::pascal;

    #[test]
    fn names_take_rants_type_spelling() {
        assert_eq!(pascal("nav"), "Nav");
        assert_eq!(pascal("lidar_driver"), "LidarDriver");
        assert_eq!(pascal("HTTPServer"), "HttpServer");
        assert_eq!(pascal("__"), "__");
    }
}
