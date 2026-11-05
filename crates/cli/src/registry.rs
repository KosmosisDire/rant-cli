//! Text values under HKEY_CURRENT_USER, the user's own part of the Windows registry.

use std::ptr::null_mut;

use windows_sys::Win32::System::Registry::{
    HKEY_CURRENT_USER, REG_VALUE_TYPE, RRF_NOEXPAND, RRF_RT_REG_EXPAND_SZ, RRF_RT_REG_SZ, RegDeleteKeyValueW, RegGetValueW,
    RegSetKeyValueW,
};

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}

/// A value as written, any %VARS% left as they are, with its type.
pub fn read(key: &str, name: &str) -> Option<(String, REG_VALUE_TYPE)> {
    let (key, name) = (wide(key), wide(name));
    let flags = RRF_RT_REG_SZ | RRF_RT_REG_EXPAND_SZ | RRF_NOEXPAND;
    let (mut ty, mut size) = (0, 0u32);
    // SAFETY: the first call only asks the size, the second fills a buffer of that size.
    unsafe {
        if RegGetValueW(HKEY_CURRENT_USER, key.as_ptr(), name.as_ptr(), flags, &mut ty, null_mut(), &mut size) != 0 {
            return None;
        }
        let mut buf = vec![0u16; (size as usize).div_ceil(2)];
        if RegGetValueW(HKEY_CURRENT_USER, key.as_ptr(), name.as_ptr(), flags, &mut ty, buf.as_mut_ptr().cast(), &mut size) != 0 {
            return None;
        }
        let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
        Some((String::from_utf16_lossy(&buf[..len]), ty))
    }
}

/// Writes a value of type ty, REG_SZ or REG_EXPAND_SZ. False when Windows refused.
pub fn write(key: &str, name: &str, value: &str, ty: REG_VALUE_TYPE) -> bool {
    let data = wide(value);
    // SAFETY: the data is NUL ended and its size counts the NUL.
    unsafe { RegSetKeyValueW(HKEY_CURRENT_USER, wide(key).as_ptr(), wide(name).as_ptr(), ty, data.as_ptr().cast(), (data.len() * 2) as u32) == 0 }
}

pub fn delete(key: &str, name: &str) {
    // SAFETY: both strings are NUL ended.
    unsafe { RegDeleteKeyValueW(HKEY_CURRENT_USER, wide(key).as_ptr(), wide(name).as_ptr()) };
}
