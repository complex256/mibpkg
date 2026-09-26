//! User-owned config files: supplied on the card at mibpkg/config/<pkg>/<path>,
//! kept internally in state/config/<pkg>/ (never on the removable card).
use crate::fsutil::copy_atomic;
use crate::manifest::Manifest;
use crate::paths::Layout;
use anyhow::Result;
use std::fs;
use std::path::{Path, PathBuf};

pub fn card_path(media_root: &Path, pkg: &str, rel: &str) -> PathBuf {
    media_root.join("mibpkg/config").join(pkg).join(rel)
}

/// Copy `src` into the package's config dir if its content differs. Returns true if copied.
pub fn import_file(layout: &Layout, pkg: &str, rel: &str, src: &Path) -> Result<bool> {
    let dst = layout.config_dir(pkg).join(rel);
    let new = fs::read(src)?;
    if fs::read(&dst).ok().as_deref() == Some(new.as_slice()) {
        return Ok(false);
    }
    if dst.exists() {
        let mut prev = dst.as_os_str().to_owned();
        prev.push(".prev");
        fs::copy(&dst, PathBuf::from(prev))?;
    }
    copy_atomic(src, &dst, 0o600)?;
    Ok(true)
}

/// Import every config the manifest declares that is present on a card. Returns imported paths.
pub fn import(layout: &Layout, media: &[PathBuf], m: &Manifest) -> Result<Vec<String>> {
    let pkg = &m.package.name;
    let mut done = Vec::new();
    for c in &m.configs {
        if let Some(src) = media.iter().map(|r| card_path(r, pkg, &c.path)).find(|p| p.is_file()) {
            if import_file(layout, pkg, &c.path, &src)? {
                done.push(c.path.clone());
            }
        }
    }
    Ok(done)
}

pub fn missing_required(layout: &Layout, m: &Manifest) -> Vec<String> {
    m.configs
        .iter()
        .filter(|c| c.required && !layout.config_dir(&m.package.name).join(&c.path).is_file())
        .map(|c| c.path.clone())
        .collect()
}
