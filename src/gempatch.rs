//! B2 jar patch: make an old stock GEM jar render mibpkg's .esd by applying a binary
//! diff against it. Non-fatal by contract — no jar or no matching patch is a clean skip,
//! so a failed B2 step degrades the install to C (web UI only), never a failure. A bad
//! patch (result sha1 mismatch) leaves the stock jar untouched.
use crate::backups;
use crate::delta;
use crate::fsutil;
use crate::paths::Layout;
use crate::sha1::{sha1_file, sha1_hex};
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

/// Candidate stock jars, unit-absolute, in detection order (first that exists wins).
const JAR_CANDIDATES: &[&str] = &[
    "/mnt/app/eso/hmi/lsd/jars/AppDevelopment.jar",
    "/mnt/app/eso/hmi/lsd/jars/GEM.jar",
    "/mnt/app/eso/hmi/lsd/jars/lsd.jar",
];

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct PatchEntry {
    pub stock_sha1: String,
    pub file: String,
    pub result_sha1: String,
}

#[derive(Debug, Default, Deserialize, Serialize)]
pub struct Patches {
    #[serde(default, rename = "patch")]
    entries: Vec<PatchEntry>,
}

impl Patches {
    pub fn load(dir: &Path) -> Result<Patches> {
        let text = fs::read_to_string(dir.join("patches.toml")).context("reading patches.toml")?;
        toml::from_str(&text).context("parsing patches.toml")
    }
    pub fn get(&self, stock_sha1: &str) -> Option<&PatchEntry> {
        self.entries.iter().find(|e| e.stock_sha1 == stock_sha1)
    }
    /// Insert or replace the entry for a stock jar, so a rebuild never leaves a stale one.
    pub fn upsert(&mut self, entry: PatchEntry) {
        match self.entries.iter_mut().find(|e| e.stock_sha1 == entry.stock_sha1) {
            Some(e) => *e = entry,
            None => self.entries.push(entry),
        }
    }
    pub fn to_toml(&self) -> String {
        toml::to_string(self).unwrap_or_default()
    }
}

#[derive(Debug)]
pub enum Status {
    NoJar,
    Unpatched { sha1: String, available: bool },
    Patched { sha1: String },
}

/// The unit-absolute path of the stock jar on this unit, and its on-disk path under the root.
fn detect(layout: &Layout) -> Option<(&'static str, PathBuf)> {
    JAR_CANDIDATES.iter().find_map(|abs| {
        let p = layout.p(abs);
        p.exists().then_some((*abs, p))
    })
}

pub fn jar_path(layout: &Layout) -> Option<PathBuf> {
    detect(layout).map(|(_, p)| p)
}

pub fn status(layout: &Layout, patch_dir: &Path) -> Result<Status> {
    let Some((_, jar)) = detect(layout) else {
        return Ok(Status::NoJar);
    };
    let cur = sha1_file(&jar)?;
    let pats = Patches::load(patch_dir).unwrap_or_default();
    if pats.entries.iter().any(|e| e.result_sha1 == cur) {
        return Ok(Status::Patched { sha1: cur });
    }
    let available = pats.get(&cur).is_some();
    Ok(Status::Unpatched { sha1: cur, available })
}

pub fn apply(layout: &Layout, patch_dir: &Path) -> Result<String> {
    let Some((abs, jar)) = detect(layout) else {
        return Ok("no GEM jar found; skipped".into());
    };
    let stock_sha1 = sha1_file(&jar)?;
    let pats = Patches::load(patch_dir)?;
    let Some(entry) = pats.get(&stock_sha1) else {
        return Ok(format!("no patch for jar {stock_sha1}; skipped"));
    };
    let old = fs::read(&jar)?;
    let patch = fs::read(patch_dir.join(&entry.file)).with_context(|| format!("reading {}", entry.file))?;
    let new = delta::apply(&old, &patch)?;
    if sha1_hex(&new) != entry.result_sha1 {
        bail!("gem-patch: result sha1 mismatch; stock jar left in place");
    }
    let mode = fs::metadata(&jar)?.permissions().mode() & 0o7777;
    // Refresh a stale backup: a firmware update can change the stock jar after an earlier
    // backup, and backup_once never overwrites, so revert would otherwise restore the wrong jar.
    let bpath = backups::backup_path(layout, abs);
    if bpath.exists() && sha1_file(&bpath).ok().as_deref() != Some(stock_sha1.as_str()) {
        let _ = fs::remove_file(&bpath);
    }
    backups::backup_once(layout, abs)?; // once-only stock backup for revert
    fsutil::atomic_write(&jar, &new)?; // fsync + atomic rename, so a power cut can't truncate the jar
    fsutil::set_mode(&jar, mode)?; // keep the stock jar's permissions
    Ok(format!("patched GEM jar {stock_sha1} -> {}", entry.result_sha1))
}

pub fn revert(layout: &Layout) -> Result<String> {
    let (abs, _) = detect(layout).context("no GEM jar to revert")?;
    backups::restore(layout, abs)?;
    Ok(format!("restored stock GEM jar {abs}"))
}
