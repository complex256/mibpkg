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
        f.sync_all()?;
    }
    fs::rename(&tmp, path)
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
    fs::rename(&tmp, dst)
}
