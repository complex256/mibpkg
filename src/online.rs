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

/// Where a source in the effective list comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum SourceOrigin {
    /// mibpkg's built-in default (settings.toml did not change the list).
    Default,
    /// Listed by hand in settings.toml `online_sources`.
    Settings,
    /// Added on the unit (web interface), kept in state/sources.json.
    Added,
}

#[derive(Debug, Clone, Serialize)]
pub struct EffectiveSource {
    pub source: String,
    pub origin: SourceOrigin,
    pub enabled: bool,
}

/// Sources changed on the unit. settings.toml is hand-owned and never rewritten, so additions and
/// removals of its sources are recorded here instead.
#[derive(Debug, Default, Serialize, Deserialize)]
struct SourceEdits {
    #[serde(default)]
    added: Vec<String>,
    #[serde(default)]
    disabled: Vec<String>,
}

pub fn state_path(layout: &Layout) -> PathBuf {
    layout.state_dir().join("sources.json")
}

fn load_edits(layout: &Layout) -> SourceEdits {
    fs::read_to_string(state_path(layout)).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
}

fn save_edits(layout: &Layout, e: &SourceEdits) -> Result<()> {
    let mut text = serde_json::to_string_pretty(e)?;
    text.push('\n');
    atomic_write(&state_path(layout), text.as_bytes())?;
    Ok(())
}

/// settings.toml sources, then the ones added on the unit; removed ones stay listed as disabled.
pub fn effective_sources(layout: &Layout) -> Vec<EffectiveSource> {
    let edits = load_edits(layout);
    let mut out: Vec<EffectiveSource> = Vec::new();
    for s in &layout.settings.online_sources {
        let origin = if s == DEFAULT_SOURCE { SourceOrigin::Default } else { SourceOrigin::Settings };
        out.push(EffectiveSource { source: s.clone(), origin, enabled: !edits.disabled.contains(s) });
    }
    for s in &edits.added {
        if !out.iter().any(|e| &e.source == s) {
            out.push(EffectiveSource { source: s.clone(), origin: SourceOrigin::Added, enabled: true });
        }
    }
    out
}

/// Add a source (or re-enable a removed settings.toml one). Caller holds /mnt/app rw.
pub fn add_source(layout: &Layout, source: &str) -> Result<String> {
    index_url(source)?;
    let mut edits = load_edits(layout);
    if edits.disabled.iter().any(|s| s == source) {
        edits.disabled.retain(|s| s != source);
        save_edits(layout, &edits)?;
        return Ok(format!("re-enabled {source}"));
    }
    if effective_sources(layout).iter().any(|e| e.source == source) {
        bail!("{source} is already a source");
    }
    edits.added.push(source.to_string());
    save_edits(layout, &edits)?;
    Ok(format!("added {source}"))
}

/// Remove an added source, or disable one from settings.toml, and drop its cached index.
/// Caller holds /mnt/app rw.
pub fn remove_source(layout: &Layout, source: &str) -> Result<String> {
    let known = effective_sources(layout).into_iter().find(|e| e.source == source).with_context(|| format!("{source} is not a source"))?;
    let mut edits = load_edits(layout);
    if known.origin == SourceOrigin::Added {
        edits.added.retain(|s| s != source);
    } else if !edits.disabled.iter().any(|s| s == source) {
        edits.disabled.push(source.to_string());
    }
    save_edits(layout, &edits)?;
    // Cached indexes are named by collection id; the cache records which source it came from.
    if let Ok(rd) = fs::read_dir(layout.state_dir().join("online")) {
        for p in rd.filter_map(|e| e.ok().map(|e| e.path())) {
            let from_source = fs::read_to_string(&p).ok().and_then(|t| serde_json::from_str::<Cached>(&t).ok()).map_or(false, |c| c.source == source);
            if from_source {
                fs::remove_file(&p)?;
            }
        }
    }
    Ok(format!("removed {source}"))
}

#[derive(Debug, Default)]
pub struct Report {
    pub checked: usize,
    pub errors: Vec<String>,
    pub updates: Vec<(String, String)>,
    /// The community list could not be fetched (the check itself still counts as done).
    pub list_error: Option<String>,
}

impl Report {
    pub fn summary(&self, now_secs: u64) -> String {
        let when = if now_secs < 1_577_836_800 { format!("checked at {now_secs} s") } else { "checked".to_string() };
        let mut s = format!("{when}, {} update{}", self.updates.len(), if self.updates.len() == 1 { "" } else { "s" });
        if !self.errors.is_empty() {
            s.push_str(&format!("; {}", self.errors.join("; ")));
        }
        if let Some(e) = &self.list_error {
            s.push_str(&format!("; list: {e}"));
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

/// What the network part of a check got: each enabled source's index, and the list when due.
pub struct Fetched {
    pub indexes: Vec<(String, Result<Cached>)>,
    pub list: Option<Result<crate::sourcelist::ListCache>>,
}

/// Problems from the last check, for the Sources screen (RAM: /tmp/mibpkg.online-errors.json).
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct OnlineErrors {
    pub sources: std::collections::BTreeMap<String, String>,
    pub list: Option<String>,
}

pub fn load_errors(layout: &Layout) -> OnlineErrors {
    fs::read_to_string(layout.run("online-errors.json")).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
}

/// Every enabled source's index and the list when due, fetched and validated. Touches nothing
/// on the unit, so it runs without the action lock (network I/O can take minutes).
pub fn fetch_all(ctx: &Ctx, fetcher: &dyn Fetcher) -> Fetched {
    let indexes = effective_sources(&ctx.layout)
        .into_iter()
        .filter(|e| e.enabled)
        .map(|e| {
            let got = fetch_one(fetcher, &e.source);
            (e.source, got)
        })
        .collect();
    let list = crate::sourcelist::fetch_list(&ctx.layout, fetcher, crate::sourcelist::real_now(&ctx.layout));
    Fetched { indexes, list }
}

/// Caches what `fetch_all` got (failed sources keep their previous cache), records problems for
/// the Sources screen and reports updates. Run under the action lock.
pub fn store(ctx: &Ctx, fetched: Fetched) -> Report {
    let mut r = Report::default();
    let mut errs = OnlineErrors::default();
    for (source, got) in fetched.indexes {
        match got.and_then(|c| store_one(ctx, &c)) {
            Ok(()) => r.checked += 1,
            Err(e) => {
                errs.sources.insert(source.clone(), format!("{e:#}"));
                r.errors.push(format!("{source}: {e:#}"));
            }
        }
    }
    match fetched.list {
        Some(Ok(list)) => {
            let stored = ctx.mounts.rw("/mnt/app").and_then(|_rw| crate::sourcelist::store_cache(&ctx.layout, &list));
            if let Err(e) = stored {
                r.list_error = Some(format!("{e:#}"));
            }
        }
        Some(Err(e)) => r.list_error = Some(format!("{e:#}")),
        None => {}
    }
    errs.list = r.list_error.clone();
    if let Ok(text) = serde_json::to_vec(&errs) {
        let _ = atomic_write(&ctx.layout.run("online-errors.json"), &text);
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
