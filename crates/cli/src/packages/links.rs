//! Links and junctions. A package from a local build folder is a link to it, so a new build
//! counts at once, and removing a link must never reach into the folder.

use std::path::Path;

use rant_config::paths;

use crate::failure::{Result, failed};

/// Whether p is a symbolic link or, on Windows, a junction.
pub fn is_link(p: &Path) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
        std::fs::symlink_metadata(p).is_ok_and(|m| m.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0)
    }
    #[cfg(not(windows))]
    {
        std::fs::symlink_metadata(p).is_ok_and(|m| m.file_type().is_symlink())
    }
}

/// Removes the link itself, never what it points at.
pub fn unlink(p: &Path) -> Result<()> {
    std::fs::remove_dir(p).or_else(|_| std::fs::remove_file(p)).map_err(|_| failed(format!("cannot remove the link {}", paths::slash(p))))
}

/// A junction, which unlike a symbolic link needs no privilege. Its reparse data holds the
/// target twice: the NT path it resolves and the path shown.
#[cfg(windows)]
pub fn make_link(link: &Path, target: &Path) -> Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use std::os::windows::fs::OpenOptionsExt;
    use std::os::windows::io::AsRawHandle;

    use windows_sys::Win32::Storage::FileSystem::{FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT};
    use windows_sys::Win32::System::IO::DeviceIoControl;
    use windows_sys::Win32::System::Ioctl::FSCTL_SET_REPARSE_POINT;
    use windows_sys::Win32::System::SystemServices::IO_REPARSE_TAG_MOUNT_POINT;

    std::fs::create_dir(link).map_err(|e| failed(format!("cannot create {}: {e}", paths::slash(link))))?;
    let shown: Vec<u16> = target.as_os_str().encode_wide().collect();
    let nt: Vec<u16> = "\\??\\".encode_utf16().chain(shown.iter().copied()).collect();
    let (nt_bytes, shown_bytes) = (nt.len() * 2, shown.len() * 2);
    let data = 8 + nt_bytes + 2 + shown_bytes + 2;
    let mut buf: Vec<u8> = Vec::with_capacity(8 + data);
    buf.extend(IO_REPARSE_TAG_MOUNT_POINT.to_le_bytes());
    // the data's length, a reserved word, then where each name sits and its length
    for v in [data, 0, 0, nt_bytes, nt_bytes + 2, shown_bytes] {
        buf.extend((v as u16).to_le_bytes());
    }
    for c in nt.iter().chain(&[0]).chain(&shown).chain(&[0]) {
        buf.extend(c.to_le_bytes());
    }
    let opened = std::fs::OpenOptions::new().write(true).custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT).open(link);
    let mut got = 0u32;
    let set = opened.is_ok_and(|f| {
        // SAFETY: the handle is open for the call, and the buffer outlives it
        unsafe { DeviceIoControl(f.as_raw_handle(), FSCTL_SET_REPARSE_POINT, buf.as_ptr().cast(), buf.len() as u32, std::ptr::null_mut(), 0, &mut got, std::ptr::null_mut()) != 0 }
    });
    if !set {
        let _ = std::fs::remove_dir(link);
        return Err(failed(format!("cannot link {} to {}", paths::slash(link), paths::slash(target))));
    }
    Ok(())
}

#[cfg(not(windows))]
pub fn make_link(link: &Path, target: &Path) -> Result<()> {
    std::os::unix::fs::symlink(target, link).map_err(|e| failed(format!("cannot link {} to {}: {e}", paths::slash(link), paths::slash(target))))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scratch::Scratch;

    #[test]
    fn a_link_reaches_its_folder_and_goes_alone() {
        let t = Scratch::new("links");
        t.write("build/package.rant", "name = \"tool\"\n");
        let target = std::path::absolute(t.path("build")).unwrap();
        std::fs::create_dir_all(t.path("packages")).unwrap();
        make_link(&t.path("packages/tool"), &target).unwrap();
        assert!(is_link(&t.path("packages/tool")) && !is_link(&target));
        assert_eq!(std::fs::read_to_string(t.path("packages/tool/package.rant")).unwrap(), "name = \"tool\"\n");
        unlink(&t.path("packages/tool")).unwrap();
        assert!(!t.path("packages/tool").exists());
        assert!(t.path("build/package.rant").is_file(), "unlinking reached into the folder");
    }
}
