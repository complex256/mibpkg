//! Crash-safe file writes: write a sibling, fsync, rename over the target.
use std::fs;
use std::io::{self, Write};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

pub fn tmp_sibling(path: &Path) -> PathBuf {
    let mut s = path.as_os_str().to_owned();
    s.push(".tmp");
    PathBuf::from(s)
}

pub fn atomic_write(path: &Path, data: &[u8]) -> io::Result<()> {
    if let Some(d) = path.parent() {
        fs::create_dir_all(d)?;
    }
    let tmp = tmp_sibling(path);
    {
        let mut f = fs::File::create(&tmp)?;
        f.write_all(data)?;
        sync_result(f.sync_all())?;
    }
    rename(&tmp, path)
}

/// rename(2), falling back to copy + remove where the filesystem cannot rename:
/// QNX 6.5 /tmp (/dev/shmem) returns EXDEV even within one directory.
pub fn rename(from: &Path, to: &Path) -> io::Result<()> {
    match fs::rename(from, to) {
        Err(e) if is_cross_device(&e) => replace_by_copy(from, to),
        other => other,
    }
}

pub fn is_cross_device(e: &io::Error) -> bool {
    e.kind() == io::ErrorKind::CrossesDevices
}

/// Replace `to` with a copy of `from` (mode included), then remove `from`.
/// Not atomic; only used where rename is impossible (RAM files in /tmp).
pub fn replace_by_copy(from: &Path, to: &Path) -> io::Result<()> {
    match fs::remove_file(to) {
        Err(e) if e.kind() != io::ErrorKind::NotFound => return Err(e),
        _ => {}
    }
    fs::copy(from, to)?;
    fs::remove_file(from)
}

/// fsync, where the filesystem has it: QNX 6.5 /tmp (/dev/shmem) returns ENOSYS,
/// and a RAM file has nothing to flush anyway.
pub fn sync_result(r: io::Result<()>) -> io::Result<()> {
    match r {
        Err(e) if e.kind() == io::ErrorKind::Unsupported => Ok(()),
        other => other,
    }
}

pub fn set_mode(path: &Path, mode: u32) -> io::Result<()> {
    fs::set_permissions(path, fs::Permissions::from_mode(mode))
}

pub fn copy_atomic(src: &Path, dst: &Path, mode: u32) -> io::Result<()> {
    if let Some(d) = dst.parent() {
        fs::create_dir_all(d)?;
    }
    let tmp = tmp_sibling(dst);
    fs::copy(src, &tmp)?;
    set_mode(&tmp, mode)?;
    rename(&tmp, dst)
}
