//! APFS zero-copy clonefile.
//!
//! On APFS, `clonefile()` creates a copy-on-write clone that shares
//! physical blocks with the original. It takes nanoseconds and uses
//! zero additional disk space. Only when the original is later modified
//! does APFS allocate new blocks — and only for the changed portions.
//!
//! This inverts the cost model of file versioning:
//!   Traditional: pay full I/O cost upfront for every version
//!   APFS clone:  pay nothing upfront, pay only for actual diffs later
//!
//! When versioning is free, complexity disappears.

use std::ffi::CString;
use std::fs;
use std::io;
use std::path::Path;

extern "C" {
    fn clonefile(src: *const libc::c_char, dst: *const libc::c_char, flags: u32) -> libc::c_int;
}

/// Clone a file using APFS clonefile().
/// Falls back to regular copy on non-APFS or if clonefile fails.
///
/// Returns Ok(true) if cloned, Ok(false) if fell back to copy.
pub fn clone_or_copy(src: &Path, dst: &Path) -> io::Result<bool> {
    // Ensure parent directory exists
    if let Some(parent) = dst.parent() {
        fs::create_dir_all(parent)?;
    }

    // Try clonefile first
    let c_src = CString::new(src.as_os_str().as_encoded_bytes())
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e))?;
    let c_dst = CString::new(dst.as_os_str().as_encoded_bytes())
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e))?;

    // CLONE_NOFOLLOW = 0x0001 — don't follow symlinks
    let ret = unsafe { clonefile(c_src.as_ptr(), c_dst.as_ptr(), 0x0001) };

    if ret == 0 {
        return Ok(true); // cloned — instant, zero-copy
    }

    // EEXIST = destination already exists (duplicate event for same second)
    let errno = io::Error::last_os_error();
    if errno.raw_os_error() == Some(libc::EEXIST) {
        return Ok(true); // already versioned this second — skip
    }

    // clonefile failed for another reason (non-APFS, etc.) — fall back to copy
    fs::copy(src, dst)?;
    Ok(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn clone_creates_identical_copy() {
        let tmp = TempDir::new().unwrap();
        let src = tmp.path().join("original.txt");
        let dst = tmp.path().join("clone.txt");

        fs::write(&src, "hello world").unwrap();
        let result = clone_or_copy(&src, &dst).unwrap();

        assert!(result); // should be true (cloned on APFS)
        assert_eq!(fs::read_to_string(&dst).unwrap(), "hello world");
    }

    #[test]
    fn clone_creates_parent_dirs() {
        let tmp = TempDir::new().unwrap();
        let src = tmp.path().join("src.txt");
        let dst = tmp.path().join("deep").join("nested").join("dir").join("dst.txt");

        fs::write(&src, "nested test").unwrap();
        clone_or_copy(&src, &dst).unwrap();

        assert!(dst.exists());
        assert_eq!(fs::read_to_string(&dst).unwrap(), "nested test");
    }

    #[test]
    fn clone_handles_eexist() {
        let tmp = TempDir::new().unwrap();
        let src = tmp.path().join("src.txt");
        let dst = tmp.path().join("dst.txt");

        fs::write(&src, "data").unwrap();
        clone_or_copy(&src, &dst).unwrap(); // first clone
        let result = clone_or_copy(&src, &dst).unwrap(); // second — EEXIST

        assert!(result); // should return Ok(true), not error
    }

    #[test]
    fn clone_binary_file() {
        let tmp = TempDir::new().unwrap();
        let src = tmp.path().join("binary.bin");
        let dst = tmp.path().join("binary_clone.bin");

        let data: Vec<u8> = (0..256).map(|i| i as u8).collect();
        fs::write(&src, &data).unwrap();
        clone_or_copy(&src, &dst).unwrap();

        assert_eq!(fs::read(&dst).unwrap(), data);
    }

    #[test]
    fn clone_empty_file() {
        let tmp = TempDir::new().unwrap();
        let src = tmp.path().join("empty.txt");
        let dst = tmp.path().join("empty_clone.txt");

        fs::write(&src, "").unwrap();
        clone_or_copy(&src, &dst).unwrap();

        assert_eq!(fs::read_to_string(&dst).unwrap(), "");
    }

    #[test]
    fn clone_nonexistent_source_errors() {
        let tmp = TempDir::new().unwrap();
        let src = tmp.path().join("nope.txt");
        let dst = tmp.path().join("dst.txt");

        let result = clone_or_copy(&src, &dst);
        assert!(result.is_err());
    }
}
