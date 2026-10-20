//! `OverrideLedger`'s file (v2): one `<key>=<pluginId>|<scope>|<prior>|<active>|<A|R>` line per
//! override of vehicle state (`A` held, `R` restoring); a 4-field line is from before the state
//! existed and counts as held. No comments. Read from PERSIST and from the old location under
//! BASE; `merge`/`contains_all` port `mibsdk-dev`'s `LEDGER_PY` for moving the old one over.
use super::{LEDGER, OLD_LEDGER};
use crate::paths::Layout;
use anyhow::{Context, Result};
use std::collections::HashMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LedgerEntry {
    pub key: String,
    /// Empty for a line that does not parse (it still counts as an override).
    pub plugin: String,
    /// `'A'` held or `'R'` restoring.
    pub state: char,
    /// The line, trimmed.
    pub line: String,
    pub source: PathBuf,
}

/// The ledger is written by Java as ISO-8859-1.
fn latin1(bytes: &[u8]) -> String {
    bytes.iter().map(|&b| b as char).collect()
}

fn parse(line: &str, source: &Path) -> LedgerEntry {
    let (key, value) = line.split_once('=').unwrap_or((line, ""));
    let f: Vec<&str> = value.trim().split('|').collect();
    let state = match (f.len(), f.get(4).copied()) {
        (4, _) | (5, Some("A")) => Some('A'),
        (5, Some("R")) => Some('R'),
        _ => None,
    };
    let (plugin, state) = match state {
        Some(s) if !f[0].is_empty() => (f[0].to_string(), s),
        _ => (String::new(), 'A'),
    };
    LedgerEntry { key: key.trim().to_string(), plugin, state, line: line.trim().to_string(), source: source.to_path_buf() }
}

/// Every entry of `PERSIST/ledger.properties`, then of the old `BASE/ledger.properties`. Blank
/// lines are skipped; any other line is an entry, parsed or not, so a refusal never misses one. A
/// missing file is empty; an unreadable one is an error (fail closed).
pub fn read(layout: &Layout) -> Result<Vec<LedgerEntry>> {
    let mut out = Vec::new();
    for abs in [LEDGER, OLD_LEDGER] {
        let path = layout.p(abs);
        let bytes = match fs::read(&path) {
            Ok(b) => b,
            Err(e) if e.kind() == io::ErrorKind::NotFound => continue,
            Err(e) => return Err(e).with_context(|| format!("cannot read {abs}")),
        };
        for line in latin1(&bytes).lines().filter(|l| !l.trim().is_empty()) {
            out.push(parse(line, &path));
        }
    }
    Ok(out)
}

/// The entries of plugin `id`: the lines containing `=<id>|` (so `bike` never matches
/// `bike-rack`).
pub fn of<'a>(entries: &'a [LedgerEntry], id: &str) -> Vec<&'a LedgerEntry> {
    let needle = format!("={id}|");
    entries.iter().filter(|e| e.line.contains(&needle)).collect()
}

/// `(key, line)` per non-blank line, the key being the trimmed text before the first `=`.
fn entries(text: &str) -> Vec<(&str, &str)> {
    text.lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| (l.split_once('=').map_or(l, |(k, _)| k).trim(), l))
        .collect()
}

/// The old ledger merged into the current one, never overwriting: the current text
/// (newline-terminated) plus each old line whose key it lacks (trimmed, once per key); and the old
/// lines (trimmed) whose key the current ledger holds with another line, which are not merged.
pub fn merge(old: &str, cur: &str) -> (String, Vec<String>) {
    let mut have: HashMap<&str, &str> = entries(cur).into_iter().collect();
    let old = entries(old);
    let conflicts =
        old.iter().filter(|(k, l)| have.get(k).is_some_and(|c| c.trim() != l.trim())).map(|(_, l)| l.trim().to_string()).collect();
    let mut text = cur.to_string();
    if !text.is_empty() && !text.ends_with('\n') {
        text.push('\n');
    }
    for (k, l) in old {
        if !have.contains_key(k) {
            text.push_str(l.trim());
            text.push('\n');
            have.insert(k, l);
        }
    }
    (text, conflicts)
}

/// Every old entry is in `cur` with the same (trimmed) line.
pub fn contains_all(old: &str, cur: &str) -> bool {
    let have: HashMap<&str, &str> = entries(cur).into_iter().collect();
    entries(old).iter().all(|(k, l)| have.get(k).is_some_and(|c| c.trim() == l.trim()))
}
