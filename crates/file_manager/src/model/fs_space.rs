//! Free and total space of the file system behind a directory.
//!
//! Read with `statvfs`, the same call a file manager's capacity bar uses: the
//! panel shows how full the drive that holds the current folder is.

use std::path::Path;

/// Total and available bytes of the file system that holds `path`.
///
/// `None` when the numbers cannot be trusted: `statvfs` failed, or the folder
/// does not exist locally (a worktree on a remote server would otherwise show
/// the free space of the local disk).
#[cfg(unix)]
pub(crate) fn space_for(path: &Path) -> Option<(u64, u64)> {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;

    if !path.is_dir() {
        return None;
    }

    let c_path = CString::new(path.as_os_str().as_bytes()).ok()?;
    // SAFETY: `statvfs` only writes into the struct passed to it, and `c_path`
    // is a valid NUL-terminated string for the duration of the call.
    let mut stats = std::mem::MaybeUninit::<libc::statvfs>::uninit();
    if unsafe { libc::statvfs(c_path.as_ptr(), stats.as_mut_ptr()) } != 0 {
        return None;
    }
    let stats = unsafe { stats.assume_init() };

    // `f_frsize` is the fragment size; some file systems report 0 and expect the
    // block size instead.
    let fragment = if stats.f_frsize > 0 {
        stats.f_frsize as u64
    } else {
        stats.f_bsize as u64
    };
    let total = (stats.f_blocks as u64).saturating_mul(fragment);
    // `f_bavail` is what an unprivileged process may use, which is what a file
    // manager should show.
    let available = (stats.f_bavail as u64).saturating_mul(fragment);

    (total > 0).then_some((total, available))
}

#[cfg(not(unix))]
pub(crate) fn space_for(_path: &Path) -> Option<(u64, u64)> {
    None
}

#[cfg(all(test, unix))]
mod tests {
    use super::space_for;

    #[test]
    fn the_root_file_system_reports_space() {
        let (total, available) = space_for(std::path::Path::new("/")).expect("statvfs on /");
        assert!(total > 0, "total: {total}");
        assert!(available <= total, "available {available} <= total {total}");
    }

    #[test]
    fn a_missing_directory_reports_nothing() {
        assert_eq!(
            space_for(std::path::Path::new("/definitely/not/here")),
            None
        );
    }
}
