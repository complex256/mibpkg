//! User-owned config files: supplied on the card at mibpkg/config/<pkg>/<path>,
//! kept internally in state/config/<pkg>/ (never on the removable card).
//!
//! Two writers change a config: a file on the card (imported on install/update/rescan) and the
//! web interface (`config-set`, which writes the internal copy only). Like dpkg's conffiles, the
//! card import is a three-way decision against the card bytes last imported, kept in
//! state/card-config/<pkg>/<path>:
//!   card unchanged since the last import  -> keep the internal copy (it may hold web edits);
//!   card changed, internal copy untouched -> import the card copy;
//!   both changed                          -> conflict: keep the internal copy, save the card's
//!                                            next to it as <path>.card until the user chooses.
use crate::fsutil::copy_atomic;
use crate::manifest::{ConfigEntry, Manifest};
use crate::paths::Layout;
use anyhow::{bail, Result};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

pub fn card_path(media_root: &Path, pkg: &str, rel: &str) -> PathBuf {
    media_root.join("mibpkg/config").join(pkg).join(rel)
}

/// The card bytes last imported (or found equal to the internal copy): the common base.
pub fn base_path(layout: &Layout, pkg: &str, rel: &str) -> PathBuf {
    base_dir(layout, pkg).join(rel)
}

pub fn base_dir(layout: &Layout, pkg: &str) -> PathBuf {
    layout.state_dir().join("card-config").join(pkg)
}

/// The card's copy of a config in conflict, waiting for `resolve`.
pub fn pending_path(layout: &Layout, pkg: &str, rel: &str) -> PathBuf {
    let mut p = layout.config_dir(pkg).join(rel).into_os_string();
    p.push(".card");
    PathBuf::from(p)
}

/// Which copy wins a conflict: the internal one (`keep`) or the card's (`card`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Choice {
    Keep,
    Card,
}

impl Choice {
    pub fn parse(s: &str) -> Result<Choice> {
        match s {
            "keep" => Ok(Choice::Keep),
            "card" => Ok(Choice::Card),
            _ => bail!("invalid choice {s:?} (keep or card)"),
        }
    }
}

/// What an import did, by config path.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Report {
    pub imported: Vec<String>,
    /// Conflicts settled by a `--config=keep` choice.
    pub kept: Vec<String>,
    pub conflicts: Vec<String>,
}

impl Report {
    /// One line for an action's result, empty when nothing happened.
    pub fn summary(&self, pkg: &str) -> String {
        let mut parts = Vec::new();
        if !self.imported.is_empty() {
            parts.push(format!("imported {}", self.imported.join(", ")));
        }
        if !self.kept.is_empty() {
            parts.push(format!("kept the current {} (card copy ignored)", self.kept.join(", ")));
        }
        if !self.conflicts.is_empty() {
            parts.push(conflict_note(pkg, &self.conflicts));
        }
        parts.join("; ")
    }
}

pub fn conflict_note(pkg: &str, paths: &[String]) -> String {
    format!(
        "config conflict: {} changed both on the card and on the car; kept the car's copy, the card's is saved as {} - choose in the web interface or run: mibpkg config-resolve {pkg} keep|card",
        paths.join(", "),
        paths.iter().map(|p| format!("{p}.card")).collect::<Vec<_>>().join(", ")
    )
}

fn read_opt(p: &Path) -> Result<Option<Vec<u8>>> {
    match fs::read(p) {
        Ok(b) => Ok(Some(b)),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.into()),
    }
}

fn remove_opt(p: &Path) -> Result<()> {
    match fs::remove_file(p) {
        Err(e) if e.kind() != io::ErrorKind::NotFound => Err(e.into()),
        _ => Ok(()),
    }
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

#[derive(Debug, PartialEq, Eq)]
enum Outcome {
    Unchanged,
    Imported,
    Kept,
    Conflict,
}

/// Bring one card file in, deciding against the last imported card bytes (see the module doc).
fn import_card(layout: &Layout, pkg: &str, rel: &str, src: &Path, choice: Option<Choice>) -> Result<Outcome> {
    let card = fs::read(src)?;
    let store = read_opt(&layout.config_dir(pkg).join(rel))?;
    let base_p = base_path(layout, pkg, rel);
    let base = read_opt(&base_p)?;
    let settled = |out| -> Result<Outcome> {
        copy_atomic(src, &base_p, 0o600)?;
        remove_opt(&pending_path(layout, pkg, rel))?;
        Ok(out)
    };
    if store.is_some() && base.as_deref() == Some(card.as_slice()) {
        // Not changed on the card since it was last imported: the car's copy stands.
        remove_opt(&pending_path(layout, pkg, rel))?;
        return Ok(Outcome::Unchanged);
    }
    if store.as_deref() == Some(card.as_slice()) {
        return settled(Outcome::Unchanged);
    }
    // No base yet (installed by an older mibpkg) with a different car copy: it may hold web
    // edits, so that is a conflict too rather than a guess.
    let untouched = store.is_none() || (base.is_some() && store == base);
    if untouched || choice == Some(Choice::Card) {
        import_file(layout, pkg, rel, src)?;
        return settled(Outcome::Imported);
    }
    if choice == Some(Choice::Keep) {
        return settled(Outcome::Kept);
    }
    copy_atomic(src, &pending_path(layout, pkg, rel), 0o600)?;
    Ok(Outcome::Conflict)
}

/// Import every config the manifest declares that is present on a card. `choice` settles
/// conflicts; without one they are left for `resolve`.
pub fn import(layout: &Layout, media: &[PathBuf], m: &Manifest, choice: Option<Choice>) -> Result<Report> {
    let pkg = &m.package.name;
    let mut r = Report::default();
    for c in &m.configs {
        if let Some(src) = media.iter().map(|r| card_path(r, pkg, &c.path)).find(|p| p.is_file()) {
            let list = match import_card(layout, pkg, &c.path, &src, choice)? {
                Outcome::Unchanged => continue,
                Outcome::Imported => &mut r.imported,
                Outcome::Kept => &mut r.kept,
                Outcome::Conflict => &mut r.conflicts,
            };
            list.push(c.path.clone());
        }
    }
    Ok(r)
}

/// Settle a conflict. Returns true if the car's copy changed (its services need a restart).
pub fn resolve(layout: &Layout, pkg: &str, rel: &str, choice: Choice) -> Result<bool> {
    let pending = pending_path(layout, pkg, rel);
    if !pending.is_file() {
        bail!("{rel} of {pkg} has no config conflict");
    }
    let changed = choice == Choice::Card && import_file(layout, pkg, rel, &pending)?;
    copy_atomic(&pending, &base_path(layout, pkg, rel), 0o600)?;
    fs::remove_file(&pending)?;
    Ok(changed)
}

/// Configs whose card copy waits for a choice.
pub fn conflicts(layout: &Layout, m: &Manifest) -> Vec<String> {
    m.configs
        .iter()
        .filter(|c| pending_path(layout, &m.package.name, &c.path).is_file())
        .map(|c| c.path.clone())
        .collect()
}

/// A manifest config named by its path or its number.
pub fn find<'m>(m: &'m Manifest, which: &str) -> Result<&'m ConfigEntry> {
    m.configs
        .iter()
        .find(|c| c.path == which)
        .or_else(|| which.parse::<usize>().ok().and_then(|n| m.configs.get(n)))
        .ok_or_else(|| anyhow::anyhow!("{} has no config {which:?}", m.package.name))
}

pub fn missing_required(layout: &Layout, m: &Manifest) -> Vec<String> {
    m.configs
        .iter()
        .filter(|c| c.required && !layout.config_dir(&m.package.name).join(&c.path).is_file())
        .map(|c| c.path.clone())
        .collect()
}
