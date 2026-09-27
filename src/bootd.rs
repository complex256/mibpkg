//! boot.d/<pkg>.<service>.toml: one file per service. Placeholders (@pkg,
//! @config) stay unresolved; the supervisor resolves @pkg via pkgs/<pkg>/current.
use crate::fsutil::atomic_write;
use crate::manifest::{Manifest, Restart, KNOWN_PHASES};
use crate::paths::Layout;
use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BootEntry {
    pub package: String,
    pub name: String,
    pub exec: String,
    #[serde(default)]
    pub args: Vec<String>,
    pub phase: String,
    #[serde(default)]
    pub restart: Restart,
    /// Files that must exist before the service starts (required configs).
    #[serde(default)]
    pub requires: Vec<String>,
    /// Start conditions (unresolved; see conditions.rs).
    #[serde(default)]
    pub after: Vec<String>,
    #[serde(default)]
    pub after_timeout_secs: Option<u64>,
}

impl BootEntry {
    pub fn id(&self) -> String {
        format!("{}.{}", self.package, self.name)
    }
}

pub fn entries(m: &Manifest) -> Vec<BootEntry> {
    let requires: Vec<String> =
        m.configs.iter().filter(|c| c.required).map(|c| format!("@config/{}", c.path)).collect();
    m.services
        .iter()
        .map(|s| BootEntry {
            package: m.package.name.clone(),
            name: s.name.clone(),
            exec: s.exec.clone(),
            args: s.args.clone(),
            phase: s.phase.clone(),
            restart: s.restart,
            requires: requires.clone(),
            after: s.after.clone(),
            // Manifest::parse already validated the timeout.
            after_timeout_secs: s.after_timeout.as_deref().map(|t| crate::conditions::parse_timeout(t).expect("validated")),
        })
        .collect()
}

pub fn remove_for_package(layout: &Layout, pkg: &str) -> Result<()> {
    let dir = layout.boot_d();
    if !dir.is_dir() {
        return Ok(());
    }
    let prefix = format!("{pkg}.");
    for e in fs::read_dir(&dir)? {
        let e = e?;
        let n = e.file_name().to_string_lossy().to_string();
        if n.starts_with(&prefix) && n.ends_with(".toml") {
            fs::remove_file(e.path())?;
        }
    }
    Ok(())
}

/// Write the package's entries, then remove its entries that are no longer listed
/// (in that order, so a power cut never leaves the package without its boot entries).
pub fn write_for_package(layout: &Layout, pkg: &str, entries: &[BootEntry]) -> Result<()> {
    let mut keep = Vec::new();
    for e in entries {
        let name = format!("{}.toml", e.id());
        let p = layout.boot_d().join(&name);
        atomic_write(&p, toml::to_string(e)?.as_bytes()).with_context(|| format!("writing {}", p.display()))?;
        keep.push(name);
    }
    let prefix = format!("{pkg}.");
    for e in fs::read_dir(layout.boot_d())? {
        let e = e?;
        let n = e.file_name().to_string_lossy().to_string();
        if n.starts_with(&prefix) && n.ends_with(".toml") && !keep.contains(&n) {
            fs::remove_file(e.path())?;
        }
    }
    Ok(())
}

pub fn load_all(layout: &Layout) -> Vec<(String, Result<BootEntry>)> {
    let mut names: Vec<String> = match fs::read_dir(layout.boot_d()) {
        Ok(rd) => rd
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().to_string())
            .filter(|n| n.ends_with(".toml"))
            .collect(),
        Err(_) => Vec::new(),
    };
    names.sort();
    names
        .into_iter()
        .map(|n| {
            let r = fs::read_to_string(layout.boot_d().join(&n))
                .map_err(anyhow::Error::from)
                .and_then(|t| toml::from_str::<BootEntry>(&t).map_err(anyhow::Error::from))
                .and_then(|e| {
                    if KNOWN_PHASES.contains(&e.phase.as_str()) {
                        Ok(e)
                    } else {
                        Err(anyhow!("boot phase {:?} requires a newer mibpkg", e.phase))
                    }
                });
            (n, r)
        })
        .collect()
}
