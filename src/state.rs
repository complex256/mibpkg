//! state/installed.json: what is installed, from where, and the last result per package.
use crate::fsutil::atomic_write;
use crate::paths::Layout;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::io::ErrorKind;
use std::path::PathBuf;

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct State {
    #[serde(default)]
    pub packages: BTreeMap<String, Installed>,
    /// Collections used so far: id -> display name.
    #[serde(default)]
    pub collections: BTreeMap<String, String>,
    /// Last action result per package ("_" = global), shown in GEM.
    #[serde(default)]
    pub results: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Installed {
    pub collection: String,
    pub version: String,
    #[serde(default)]
    pub previous: Option<String>,
    /// Service ids "<pkg>.<service>".
    #[serde(default)]
    pub services: Vec<String>,
    #[serde(default)]
    pub uses_scripts: bool,
}

impl State {
    pub fn path(layout: &Layout) -> PathBuf {
        layout.state_dir().join("installed.json")
    }

    pub fn load(layout: &Layout) -> Result<State> {
        let p = Self::path(layout);
        match std::fs::read_to_string(&p) {
            Ok(t) => serde_json::from_str(&t).with_context(|| format!("parsing {}", p.display())),
            Err(e) if e.kind() == ErrorKind::NotFound => Ok(State::default()),
            Err(e) => Err(e).with_context(|| format!("reading {}", p.display())),
        }
    }

    pub fn save(&self, layout: &Layout) -> Result<()> {
        let mut t = serde_json::to_string_pretty(self)?;
        t.push('\n');
        atomic_write(&Self::path(layout), t.as_bytes())?;
        Ok(())
    }
}
