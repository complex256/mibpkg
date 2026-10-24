//! One merged view per package name: what is installed and what every card and online collection
//! offers. The web interface lists, searches and acts on this.
use crate::config::missing_required;
use crate::index::{cmp_versions, Collection};
use crate::install::current_manifest;
use crate::mibsdk::catalog::PermInfo;
use crate::paths::Layout;
use crate::state::State;
use crate::unit::UnitInfo;
use serde::Serialize;
use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::SystemTime;

#[derive(Debug, Clone, Serialize)]
pub struct Avail {
    pub collection: String,
    pub collection_name: String,
    /// "card" or "online".
    pub origin: &'static str,
    pub version: String,
    pub size: u64,
    pub description: String,
    pub compatible: bool,
    pub uses_scripts: bool,
    pub sha256: String,
    /// "plugin" or "sdk"; None for a plain package.
    pub kind: Option<String>,
    /// A plugin's declared permissions, in plain words.
    pub permissions: Vec<Perm>,
}

/// One declared permission as the web shows it. One entry per id, so an "optional" flag can be
/// added later without changing the shape.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Perm {
    pub id: String,
    /// The installed SDK's plain-words text, or a note that the SDK does not know it.
    pub text: String,
    /// Shown as a warning (`platform.unrestricted`).
    pub loud: bool,
    /// Installed plugins only: whether `grants.properties` grants it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub granted: Option<bool>,
}

/// `ids` described by the installed SDK's catalog (`None`: no readable catalog), with `granted`
/// filled in when a grant set is given.
pub fn permissions(cat: Option<&[PermInfo]>, ids: &[String], grant: Option<&BTreeSet<String>>) -> Vec<Perm> {
    ids.iter()
        .map(|id| {
            let (text, loud) = match cat.and_then(|c| c.iter().find(|p| &p.id == id)) {
                Some(p) => (p.text.clone(), p.loud),
                None => (format!("{id}: not known to the installed SDK"), false),
            };
            Perm { id: id.clone(), text, loud, granted: grant.map(|g| g.contains(id)) }
        })
        .collect()
}

#[derive(Debug, Clone, Serialize)]
pub struct Svc {
    pub id: String,
    /// "running", "waiting", "failed", … or "unknown" when the supervisor has no status.
    pub state: String,
    /// One line, as `mibpkg status` prints it.
    pub text: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ConfigInfo {
    pub index: usize,
    pub path: String,
    pub required: bool,
    pub exists: bool,
    /// The card's copy changed too and waits for a choice (config-resolve).
    pub conflict: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct Inst {
    pub version: String,
    pub collection: String,
    pub previous: Option<String>,
    pub services: Vec<Svc>,
    pub uses_scripts: bool,
    pub missing_config: Vec<String>,
    pub last_result: Option<String>,
    pub configs: Vec<ConfigInfo>,
    /// "plugin" or "sdk"; None for a plain package (or an unreadable manifest).
    pub kind: Option<String>,
    /// A plugin's declared permissions, each with `granted`.
    pub permissions: Vec<Perm>,
    /// Declared permissions not granted: non-empty means the runtime blocks the plugin.
    pub blocked: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct PkgView {
    pub name: String,
    pub description: String,
    pub installed: Option<Inst>,
    pub available: Vec<Avail>,
    /// Newer compatible version in the package's own collection.
    pub update: Option<String>,
    /// The collection `update` comes from (the recorded one, or where the package moved).
    pub update_from: Option<String>,
    /// The recorded collection is gone and several offer something newer: the user picks one.
    pub moved_choices: Vec<MovedChoice>,
}

#[derive(Debug, Clone, Serialize)]
pub struct MovedChoice {
    pub collection: String,
    pub version: String,
}

/// Parsed indexes, keyed by file, valid while the file's size and modification time are unchanged.
/// The web server builds the catalog for every page; indexes on the SD card and the online caches
/// rarely change, and reading plus parsing them each time made the pages slow on the unit.
type ParseCache = Mutex<HashMap<PathBuf, (u64, Option<SystemTime>, Option<Collection>)>>;
static PARSED: OnceLock<ParseCache> = OnceLock::new();

/// `parse(file)` once per version of `file` (None when the file is unreadable or invalid).
fn cached(file: &Path, parse: impl FnOnce() -> Option<Collection>) -> Option<Collection> {
    let meta = std::fs::metadata(file).ok()?;
    let key = (meta.len(), meta.modified().ok());
    let cache = PARSED.get_or_init(|| Mutex::new(HashMap::new()));
    if let Some((len, mtime, c)) = cache.lock().unwrap_or_else(|e| e.into_inner()).get(file) {
        if (*len, *mtime) == key {
            return c.clone();
        }
    }
    let c = parse();
    cache.lock().unwrap_or_else(|e| e.into_inner()).insert(file.to_path_buf(), (key.0, key.1, c.clone()));
    c
}

/// Card collections (first copy of each id) then cached online ones, with their origin.
pub fn sources(layout: &Layout) -> Vec<(Collection, &'static str)> {
    let mut out: Vec<(Collection, &'static str)> = Vec::new();
    // Card: <media root>/mibpkg/collections/<dir>/index.json, as media::scan finds them.
    for root in crate::media::roots(layout) {
        let mut dirs: Vec<PathBuf> = match std::fs::read_dir(root.join("mibpkg/collections")) {
            Ok(rd) => rd.filter_map(|e| e.ok()).map(|e| e.path()).filter(|p| p.is_dir()).collect(),
            Err(_) => continue,
        };
        dirs.sort();
        for d in dirs {
            if let Some(c) = cached(&d.join("index.json"), || Collection::load(&d).ok()) {
                if !out.iter().any(|(o, _)| o.id() == c.id()) {
                    out.push((c, "card"));
                }
            }
        }
    }
    // Online: state/online/<id>.json, as online::collections reads them.
    if let Ok(rd) = std::fs::read_dir(layout.state_dir().join("online")) {
        let mut paths: Vec<PathBuf> = rd.filter_map(|e| e.ok().map(|e| e.path())).filter(|p| p.extension().map_or(false, |x| x == "json")).collect();
        paths.sort();
        for p in paths {
            let parse = || {
                let cached: crate::online::Cached = serde_json::from_str(&std::fs::read_to_string(&p).ok()?).ok()?;
                Collection::from_online(cached.index, &cached.base_url).ok()
            };
            if let Some(c) = cached(&p, parse) {
                out.push((c, "online"));
            }
        }
    }
    out
}

/// Every collection, card and online (what install and update choose from).
pub fn collections(layout: &Layout) -> Vec<Collection> {
    sources(layout).into_iter().map(|(c, _)| c).collect()
}

pub fn build(layout: &Layout, unit: &UnitInfo) -> Vec<PkgView> {
    let state = State::load(layout).unwrap_or_default();
    let status = crate::status::read(layout);
    let srcs = sources(layout);
    // A missing or unreadable catalog describes every permission as unknown.
    let cat = crate::mibsdk::catalog::installed(layout).ok().flatten();
    let cat = cat.as_deref();
    let grants = crate::mibsdk::grants::read(layout).unwrap_or_default();

    // name -> what each collection offers (its newest version that fits, else its newest).
    let mut offered: BTreeMap<String, Vec<Avail>> = BTreeMap::new();
    for (c, origin) in &srcs {
        let mut names: Vec<&str> = c.index.packages.iter().map(|e| e.name.as_str()).collect();
        names.sort();
        names.dedup();
        for n in names {
            let e = c.latest_compatible(n, unit).or_else(|| c.latest(n)).expect("name is in this index");
            offered.entry(n.to_string()).or_default().push(Avail {
                collection: c.id().to_string(),
                collection_name: c.index.collection.name.clone(),
                origin,
                version: e.version.clone(),
                size: e.size,
                description: e.description.clone(),
                compatible: e.compat.matches(unit),
                uses_scripts: e.uses_scripts,
                sha256: e.sha256.clone(),
                kind: e.kind.clone(),
                permissions: permissions(cat, &e.permissions, None),
            });
        }
    }

    let colls: Vec<Collection> = srcs.iter().map(|(c, _)| c.clone()).collect();
    let mut names: Vec<String> = offered.keys().cloned().collect();
    names.extend(state.packages.keys().cloned());
    names.sort();
    names.dedup();

    names
        .into_iter()
        .map(|name| {
            let available = offered.remove(&name).unwrap_or_default();
            let manifest = current_manifest(layout, &name).ok();
            let installed = state.packages.get(&name).map(|i| {
                let declared = manifest.as_ref().and_then(|m| m.plugin.as_ref()).map(|p| p.permissions.clone()).unwrap_or_default();
                let grant = grants.get(&name).cloned().unwrap_or_default();
                let services = i
                    .services
                    .iter()
                    .map(|id| match status.as_ref().and_then(|s| s.services.get(id)) {
                        Some(s) => Svc { id: id.clone(), state: s.state.to_string(), text: crate::status::describe(s) },
                        None => Svc { id: id.clone(), state: "unknown".into(), text: "no status from the supervisor".into() },
                    })
                    .collect();
                let configs = manifest
                    .as_ref()
                    .map(|m| {
                        m.configs
                            .iter()
                            .enumerate()
                            .map(|(index, c)| ConfigInfo {
                                index,
                                path: c.path.clone(),
                                required: c.required,
                                exists: layout.config_dir(&name).join(&c.path).is_file(),
                                conflict: crate::config::pending_path(layout, &name, &c.path).is_file(),
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                Inst {
                    version: i.version.clone(),
                    collection: i.collection.clone(),
                    previous: i.previous.clone(),
                    services,
                    uses_scripts: i.uses_scripts,
                    missing_config: manifest.as_ref().map(|m| missing_required(layout, m)).unwrap_or_default(),
                    last_result: state.results.get(&name).cloned(),
                    configs,
                    kind: manifest.as_ref().and_then(|m| m.kind().name()),
                    permissions: permissions(cat, &declared, Some(&grant)),
                    blocked: crate::mibsdk::grants::missing(&declared, &grant),
                }
            });
            let newer_in = |id: &str, inst: &Inst| {
                available
                    .iter()
                    .filter(|a| a.collection == id && a.compatible)
                    .map(|a| a.version.clone())
                    .max_by(|a, b| cmp_versions(a, b))
                    .filter(|v| cmp_versions(v, &inst.version) == Ordering::Greater)
            };
            let ids = installed.as_ref().map(|i| crate::install::update_sources(&i.collection, &name, &colls)).unwrap_or_default();
            let (update, update_from, moved_choices) = match (installed.as_ref(), ids.len()) {
                (Some(i), 1) => {
                    let v = newer_in(&ids[0], i);
                    let from = v.as_ref().map(|_| ids[0].clone());
                    (v, from, Vec::new())
                }
                (Some(i), n) if n > 1 => {
                    let choices = ids.iter().filter_map(|id| newer_in(id, i).map(|version| MovedChoice { collection: id.clone(), version })).collect();
                    (None, None, choices)
                }
                _ => (None, None, Vec::new()),
            };
            let description = available
                .iter()
                .map(|a| a.description.clone())
                .find(|d| !d.is_empty())
                .or_else(|| manifest.as_ref().map(|m| m.package.description.clone()))
                .unwrap_or_default();
            PkgView { name, description, installed, available, update, update_from, moved_choices }
        })
        .collect()
}
