//! Online collections: sources -> index.json over HTTPS, cached per collection id in
//! state/online/<id>.json (rewritten only when changed), loaded next to SD collections.
use crate::ctx::Ctx;
use crate::fetch::{Fetcher, MAX_INDEX};
use crate::fsutil::atomic_write;
use crate::index::{Collection, Index};
use crate::install::updates_available;
use crate::paths::Layout;
use crate::state::State;
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

pub const DEFAULT_SOURCE: &str = "github:complex256/mibpkg-packages";

pub fn index_url(source: &str) -> Result<String> {
    if let Some(repo) = source.strip_prefix("github:") {
        let parts: Vec<&str> = repo.split('/').collect();
        let ok = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b));
        if parts.len() != 2 || !parts.iter().all(|p| ok(p)) {
            bail!("source {source:?}: expected github:<owner>/<repo>");
        }
        return Ok(format!("https://github.com/{}/{}/releases/latest/download/index.json", parts[0], parts[1]));
    }
    if source.starts_with("https://") && source.ends_with(".json") {
        return Ok(source.to_string());
    }
    bail!("source {source:?}: expected github:<owner>/<repo> or an https://…/index.json URL")
}

/// The directory of an index URL (package paths resolve against it).
pub fn base_of(url: &str) -> String {
    match url.rfind('/') {
        Some(i) => url[..=i].to_string(),
        None => url.to_string(),
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Cached {
    pub source: String,
    pub base_url: String,
    pub index: Index,
}

pub fn cache_path(layout: &Layout, id: &str) -> PathBuf {
    layout.state_dir().join("online").join(format!("{id}.json"))
}

/// Cached online collections (unreadable cache files are skipped).
pub fn collections(layout: &Layout) -> Vec<Collection> {
    let mut out = Vec::new();
    if let Ok(rd) = fs::read_dir(layout.state_dir().join("online")) {
        let mut paths: Vec<PathBuf> = rd.filter_map(|e| e.ok().map(|e| e.path())).filter(|p| p.extension().map_or(false, |x| x == "json")).collect();
        paths.sort();
        for p in paths {
            if let Some(c) = fs::read_to_string(&p).ok().and_then(|t| serde_json::from_str::<Cached>(&t).ok()).and_then(|c| Collection::from_online(c.index, &c.base_url).ok()) {
                out.push(c);
            }
        }
    }
    out
}

#[derive(Debug, Default)]
pub struct Report {
    pub checked: usize,
    pub errors: Vec<String>,
    pub updates: Vec<(String, String)>,
}

impl Report {
    pub fn summary(&self, now_secs: u64) -> String {
        let when = if now_secs < 1_577_836_800 { format!("checked at {now_secs} s") } else { "checked".to_string() };
        let mut s = format!("{when}, {} update{}", self.updates.len(), if self.updates.len() == 1 { "" } else { "s" });
        if !self.errors.is_empty() {
            s.push_str(&format!("; {}", self.errors.join("; ")));
        }
        s
    }
}

fn fetch_one(fetcher: &dyn Fetcher, source: &str) -> Result<Cached> {
    let url = index_url(source)?;
    let bytes = fetcher.get(&url, MAX_INDEX)?;
    let index: Index = serde_json::from_slice(&bytes).with_context(|| format!("{url}: not a valid index.json"))?;
    let base = base_of(&url);
    let coll = Collection::from_online(index, &base).with_context(|| url.clone())?;
    Ok(Cached { source: source.to_string(), base_url: base, index: coll.index })
}

fn store_one(ctx: &Ctx, cached: &Cached) -> Result<()> {
    let mut text = serde_json::to_string_pretty(cached)?;
    text.push('\n');
    let path = cache_path(&ctx.layout, &cached.index.collection.id);
    if fs::read_to_string(&path).ok().as_deref() != Some(text.as_str()) {
        let _rw = ctx.mounts.rw("/mnt/app")?;
        atomic_write(&path, text.as_bytes())?;
    }
    Ok(())
}

/// Every configured source's index, fetched and validated. Touches nothing on the unit, so it
/// runs without the action lock (network I/O can take minutes).
pub fn fetch_all(ctx: &Ctx, fetcher: &dyn Fetcher) -> Vec<(String, Result<Cached>)> {
    ctx.layout.settings.online_sources.iter().map(|s| (s.clone(), fetch_one(fetcher, s))).collect()
}

/// Caches what `fetch_all` got (failed sources keep their previous cache) and reports updates.
/// Run under the action lock.
pub fn store(ctx: &Ctx, fetched: Vec<(String, Result<Cached>)>) -> Report {
    let mut r = Report::default();
    for (source, got) in fetched {
        match got.and_then(|c| store_one(ctx, &c)) {
            Ok(()) => r.checked += 1,
            Err(e) => r.errors.push(format!("{source}: {e:#}")),
        }
    }
    if let Ok(state) = State::load(&ctx.layout) {
        let mut colls = crate::media::loaded(&crate::media::scan(&ctx.layout));
        colls.extend(collections(&ctx.layout));
        r.updates = updates_available(&state, &colls, &ctx.unit);
    }
    r
}

/// Fetch and store in one go (on-demand checks that already hold the lock).
pub fn check(ctx: &Ctx, fetcher: &dyn Fetcher) -> Report {
    store(ctx, fetch_all(ctx, fetcher))
}
