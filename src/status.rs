//! /tmp/mibpkg.status: what the supervisor is doing, read by the CLI and GEM.
use crate::fsutil::atomic_write;
use crate::paths::Layout;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SvcState {
    Starting,
    Running,
    Backoff,
    Failed,
    Stopped,
    NeedsConfig,
    Invalid,
    /// Start conditions not met yet.
    Waiting,
}

impl fmt::Display for SvcState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            SvcState::Starting => "starting",
            SvcState::Running => "running",
            SvcState::Backoff => "backoff",
            SvcState::Failed => "failed",
            SvcState::Stopped => "stopped",
            SvcState::NeedsConfig => "needs-config",
            SvcState::Invalid => "invalid",
            SvcState::Waiting => "waiting",
        };
        f.write_str(s)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SvcStatus {
    pub state: SvcState,
    pub pid: Option<u32>,
    pub restarts: u32,
    pub last_exit: Option<String>,
    pub error: Option<String>,
    /// Unmet start conditions while waiting.
    #[serde(default)]
    pub waiting_for: Vec<String>,
    #[serde(default)]
    pub waiting_secs: Option<u64>,
    /// e.g. "started without tailnet after 5m".
    #[serde(default)]
    pub note: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Status {
    pub updated: u64,
    pub pid: u32,
    #[serde(default)]
    pub disabled: bool,
    #[serde(default)]
    pub services: BTreeMap<String, SvcStatus>,
    /// boot.d files that could not be used: file -> error.
    #[serde(default)]
    pub invalid: BTreeMap<String, String>,
}

pub fn now_secs() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

pub fn read(layout: &Layout) -> Option<Status> {
    let t = std::fs::read_to_string(layout.run("status")).ok()?;
    serde_json::from_str(&t).ok()
}

pub fn write(layout: &Layout, s: &Status) -> Result<()> {
    atomic_write(&layout.run("status"), serde_json::to_string(s)?.as_bytes())?;
    Ok(())
}
