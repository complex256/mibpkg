//! Discovering online sources: the community list (sources.json, an asset of the list repo's
//! latest release) and an SD card's mibpkg/sources.toml. Listed sources are only *available*:
//! nothing is fetched from one until it is enabled (online::add_source).
use crate::paths::Layout;
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use crate::fetch::{Fetcher, MAX_INDEX};
use std::fs;
use std::path::PathBuf;

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

pub const DEFAULT_LIST: &str = "github:complex256/mibpkg-packages";
const FRESH_SECS: u64 = 24 * 3600;
const Y2020: u64 = 1_577_836_800;

/// Where the list is fetched from; None when the list is turned off ("").
pub fn list_url(spec: &str) -> Result<Option<String>> {
    if spec.is_empty() {
        return Ok(None);
    }
    if spec.starts_with("github:") {
        let idx = crate::online::index_url(spec)?;
        return Ok(Some(format!("{}sources.json", crate::online::base_of(&idx))));
    }
    if spec.starts_with("https://") && spec.ends_with(".json") {
        return Ok(Some(spec.to_string()));
    }
    bail!("online_list {spec:?}: expected github:<owner>/<repo>, an https://…/sources.json URL, or \"\"")
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ListCache {
    /// Real (network-corrected) seconds when fetched.
    pub fetched_at: u64,
    pub url: String,
    pub entries: Vec<ListEntry>,
    #[serde(default)]
    pub skipped: Vec<String>,
}

pub fn cache_path(layout: &Layout) -> PathBuf {
    layout.state_dir().join("sources-list.json")
}

pub fn load_cache(layout: &Layout) -> Option<ListCache> {
    serde_json::from_str(&fs::read_to_string(cache_path(layout)).ok()?).ok()
}

/// Real time: the system clock plus the SNTP offset; None while it is obviously unset (the unit
/// boots at 1970 and has no offset until the first check).
pub fn real_now(layout: &Layout) -> Option<u64> {
    let sys = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).ok()?.as_secs() as i64;
    let now = sys + crate::clock::read_offset(layout).unwrap_or(0);
    (now >= Y2020 as i64).then_some(now as u64)
}

fn due(cache: Option<&ListCache>, url: &str, now: Option<u64>) -> bool {
    match (cache, now) {
        (Some(c), Some(now)) => c.url != url || c.fetched_at > now || now - c.fetched_at >= FRESH_SECS,
        _ => true,
    }
}

/// The list when it is due (missing, from another URL, older than 24 h, or real time unknown);
/// None when the list is off or the cache is fresh. Touches nothing on the unit.
pub fn fetch_list(layout: &Layout, fetcher: &dyn Fetcher, now: Option<u64>) -> Option<Result<ListCache>> {
    let url = match list_url(&layout.settings.online_list) {
        Ok(Some(u)) => u,
        Ok(None) => return None,
        Err(e) => return Some(Err(e)),
    };
    if !due(load_cache(layout).as_ref(), &url, now) {
        return None;
    }
    Some((|| {
        let bytes = fetcher.get(&url, MAX_INDEX)?;
        let p = parse_list(&bytes).with_context(|| url.clone())?;
        Ok(ListCache { fetched_at: now.unwrap_or(0), url: url.clone(), entries: p.entries, skipped: p.skipped })
    })())
}

/// Caller holds /mnt/app rw (or knows nothing changed).
pub fn store_cache(layout: &Layout, c: &ListCache) -> Result<()> {
    let mut text = serde_json::to_string_pretty(c)?;
    text.push('\n');
    fs::create_dir_all(layout.state_dir())?;
    crate::fsutil::atomic_write(&cache_path(layout), text.as_bytes())?;
    Ok(())
}
