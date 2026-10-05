//! One merged view per package name: what is installed and what every card and online collection
//! offers. The web interface lists, searches and acts on this.
use crate::config::missing_required;
use crate::index::{cmp_versions, Collection};
use crate::install::current_manifest;
use crate::paths::Layout;
use crate::state::State;
use crate::unit::UnitInfo;
use serde::Serialize;
use std::cmp::Ordering;
use std::collections::BTreeMap;

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
}

#[derive(Debug, Clone, Serialize)]
pub struct PkgView {
    pub name: String,
    pub description: String,
    pub installed: Option<Inst>,
    pub available: Vec<Avail>,
    /// Newer compatible version in the package's own collection.
    pub update: Option<String>,
}

/// Card collections (first copy of each id) then cached online ones, with their origin.
pub fn sources(layout: &Layout) -> Vec<(Collection, &'static str)> {
    let mut out: Vec<(Collection, &'static str)> =
        crate::media::loaded(&crate::media::scan(layout)).into_iter().map(|c| (c, "card")).collect();
    out.extend(crate::online::collections(layout).into_iter().map(|c| (c, "online")));
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
            });
        }
    }

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
                }
            });
            let update = installed.as_ref().and_then(|i| {
                available
                    .iter()
                    .filter(|a| a.collection == i.collection && a.compatible)
                    .map(|a| a.version.clone())
                    .max_by(|a, b| cmp_versions(a, b))
                    .filter(|v| cmp_versions(v, &i.version) == Ordering::Greater)
            });
            let description = available
                .iter()
                .map(|a| a.description.clone())
                .find(|d| !d.is_empty())
                .or_else(|| manifest.as_ref().map(|m| m.package.description.clone()))
                .unwrap_or_default();
            PkgView { name, description, installed, available, update }
        })
        .collect()
}
