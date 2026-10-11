//! Discovering online sources: the community list (sources.json, an asset of the list repo's
//! latest release) and an SD card's mibpkg/sources.toml. Listed sources are only *available*:
//! nothing is fetched from one until it is enabled (online::add_source).
use crate::paths::Layout;
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;

pub const LIST_FORMAT: u64 = 1;
const MAX_NAME: usize = 60;
const MAX_DESC: usize = 200;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ListEntry {
    pub name: String,
    #[serde(default)]
    pub description: String,
    pub source: String,
}

/// Valid entries, and one reason per skipped entry.
#[derive(Debug, Default)]
pub struct Parsed {
    pub entries: Vec<ListEntry>,
    pub skipped: Vec<String>,
}

fn check(e: &ListEntry) -> Result<()> {
    let n = e.name.trim().chars().count();
    if n == 0 || n > MAX_NAME {
        bail!("name must be 1-{MAX_NAME} characters");
    }
    if e.description.chars().count() > MAX_DESC {
        bail!("description must be at most {MAX_DESC} characters");
    }
    crate::online::index_url(&e.source)?;
    Ok(())
}

/// One entry from loosely typed fields: a missing or non-text field reads as "" and fails `check`.
fn entry(get: impl Fn(&str) -> Option<String>) -> ListEntry {
    ListEntry {
        name: get("name").unwrap_or_default(),
        description: get("description").unwrap_or_default(),
        source: get("source").unwrap_or_default(),
    }
}

fn push(p: &mut Parsed, i: usize, e: ListEntry) {
    match check(&e) {
        Ok(()) => p.entries.push(e),
        Err(err) => p.skipped.push(format!("entry {}: {err:#}", i + 1)),
    }
}

/// sources.json. Unknown fields (including the reserved `key`) are ignored.
pub fn parse_list(bytes: &[u8]) -> Result<Parsed> {
    let v: serde_json::Value = serde_json::from_slice(bytes).context("sources.json is not valid JSON")?;
    let format = v.get("format").and_then(|f| f.as_u64()).context("sources.json has no format")?;
    if format != LIST_FORMAT {
        bail!("sources.json format {format} is not supported by this mibpkg");
    }
    let list = v.get("sources").and_then(|s| s.as_array()).context("sources.json has no sources list")?;
    let mut p = Parsed::default();
    for (i, e) in list.iter().enumerate() {
        push(&mut p, i, entry(|k| e.get(k).and_then(|x| x.as_str()).map(String::from)));
    }
    Ok(p)
}

/// A card's sources.toml: `[[source]]` tables with name, description, source (key ignored).
pub fn parse_card(text: &str) -> Result<Parsed> {
    let t: toml::Table = toml::from_str(text).context("sources.toml is not valid TOML")?;
    let mut p = Parsed::default();
    if let Some(list) = t.get("source").and_then(|s| s.as_array()) {
        for (i, e) in list.iter().enumerate() {
            push(&mut p, i, entry(|k| e.get(k).and_then(|x| x.as_str()).map(String::from)));
        }
    }
    Ok(p)
}

/// Every inserted card's mibpkg/sources.toml (unreadable files are reported, not fatal).
pub fn card_sources(layout: &Layout) -> Parsed {
    let mut all = Parsed::default();
    for root in crate::media::roots(layout) {
        let f = root.join("mibpkg/sources.toml");
        let Ok(text) = fs::read_to_string(&f) else { continue };
        match parse_card(&text) {
            Ok(p) => {
                all.entries.extend(p.entries);
                all.skipped.extend(p.skipped.into_iter().map(|s| format!("{}: {s}", f.display())));
            }
            Err(e) => all.skipped.push(format!("{}: {e:#}", f.display())),
        }
    }
    all
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Available {
    pub name: String,
    pub description: String,
    pub source: String,
    /// "list" or "card".
    pub origin: &'static str,
}

/// Sources that could be enabled: card entries first (a card can label a source the list also
/// has), then list entries; without sources already in the effective list (enabled or not).
pub fn available(layout: &Layout, list: &[ListEntry]) -> Vec<Available> {
    let known: Vec<String> = crate::online::effective_sources(layout).into_iter().map(|e| e.source).collect();
    let card = card_sources(layout).entries;
    let mut out: Vec<Available> = Vec::new();
    let tagged = card.into_iter().map(|e| (e, "card")).chain(list.iter().cloned().map(|e| (e, "list")));
    for (e, origin) in tagged {
        if known.contains(&e.source) || out.iter().any(|a| a.source == e.source) {
            continue;
        }
        out.push(Available { name: e.name, description: e.description, source: e.source, origin });
    }
    out
}
