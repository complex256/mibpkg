//! The permission catalog of the installed SDK (`permissions.json`, the constants of the API's
//! `Permissions`), used to describe what a plugin asks for and to refuse permissions the SDK does
//! not know.
use super::PERMISSIONS;
use crate::paths::Layout;
use anyhow::{bail, Context, Result};
use serde::Deserialize;
use std::fs;

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct PermInfo {
    pub id: String,
    pub group: String,
    pub text: String,
    pub loud: bool,
}

/// `api`/`min_api` are present too; the SDK package's manifest is what mibpkg checks them against.
#[derive(Deserialize)]
struct File {
    format: u32,
    permissions: Vec<PermInfo>,
}

/// A permission id: 1 or more of `[A-Za-z0-9._-]`.
pub fn is_permission(s: &str) -> bool {
    !s.is_empty() && s.bytes().all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
}

/// Parses `{"format":1,"api":N,"min_api":M,"permissions":[{"id","group","text","loud"},…]}`.
pub fn parse(json: &str) -> Result<Vec<PermInfo>> {
    let f: File = serde_json::from_str(json).context("parsing permissions.json")?;
    if f.format != 1 {
        bail!("permissions.json format {} is not supported (expected 1)", f.format);
    }
    for p in &f.permissions {
        if !is_permission(&p.id) {
            bail!("permissions.json: permission {:?} must be 1 or more of [A-Za-z0-9._-]", p.id);
        }
    }
    Ok(f.permissions)
}

/// The installed SDK's catalog; `None` when the file is missing or unparsable.
pub fn installed(layout: &Layout) -> Option<Vec<PermInfo>> {
    parse(&fs::read_to_string(layout.p(PERMISSIONS)).ok()?).ok()
}

/// One line per id: `"<text> (<id>)"`, prefixed `"WARNING: "` when loud, or
/// `"<id> (unknown to the installed SDK)"`.
pub fn describe(cat: Option<&[PermInfo]>, ids: &[String]) -> Vec<String> {
    ids.iter()
        .map(|id| match cat.and_then(|c| c.iter().find(|p| &p.id == id)) {
            Some(p) if p.loud => format!("WARNING: {} ({id})", p.text),
            Some(p) => format!("{} ({id})", p.text),
            None => format!("{id} (unknown to the installed SDK)"),
        })
        .collect()
}
