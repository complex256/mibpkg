# Online sources + publisher tooling (M3 + M4, unsigned) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Cars discover and enable sources from a community list and from an SD card's
`sources.toml`; packages can move between collections; publishers release, verify and scaffold
packages with `mibpkg release`, `mibpkg verify`, `mibpkg new`.

**Architecture:** A new `src/sourcelist.rs` parses the list and `sources.toml` and computes the
*available* sources; `online.rs` fetches/caches the list during the online check (next to the
indexes) and enforces one owner per collection id; `install.rs` gains `update_sources` (the
"package moved" rule) used by `pick_collection`, `updates_available` and the catalog. A new
`src/publish.rs` holds `release` and `verify` over the existing `Fetcher` trait plus a new
`Publisher` trait (real impl shells out to `gh`); `src/scaffold.rs` writes `mibpkg new` packages.

**Tech Stack:** Rust 2021, std-only on the unit (QNX 6.5 armv7), serde/serde_json/toml/semver/
sha2/flate2 (existing deps only, no new crates), libcurl via dlopen (unit and host), `gh` CLI on
the host, vanilla ES modules in `web/js`.

**Spec:** `docs/superpowers/specs/2026-10-11-online-sources-publishing-design.md` (amends
`docs/superpowers/specs/2026-09-30-online-updates-design.md`).

## Global Constraints

- No signatures. `key` fields in the list / `sources.toml` are parsed and ignored.
- No new crate dependencies (the unit build is std-only; `libc`, `ring`, `rustls` do not build).
- QNX: `/mnt/app` is read-only except inside `ctx.mounts.rw("/mnt/app")`; `/tmp` files are RAM
  (`layout.run(name)` = `/tmp/mibpkg.<name>`), no subdirectories in `/tmp`, rename may fail with
  EXDEV (use `fsutil::atomic_write`).
- Flash writes only when content changes (state files under `<home>/state`).
- Untrusted input caps: list/index ≤ `fetch::MAX_INDEX` (1 MB); source name ≤ 60 chars;
  description ≤ 200 chars; one bad entry never hides the rest.
- `settings.toml` is hand-owned; mibpkg never rewrites it. New key: `online_list` (default
  `github:complex256/mibpkg-packages`; `""` = list off).
- The list lives in `complex256/mibpkg-packages`' latest release as `sources.json`; its source of
  truth is `packaging/sources.json` in this repo.
- Release entries are absolute URLs `https://github.com/<owner>/<repo>/releases/download/<tag>/<file>`.
- `release --keep` default 3. `release --publish` waits up to 60 s for `latest/download/index.json`.
- GEM gets nothing new (it is the recovery page). The web UI is the only sources screen.
- Commits: author `complex256 <me@complex256.dev>`; never reference the owner's personal names.
- Outward actions (creating/updating GitHub releases, pushing) only with the owner's explicit
  go-ahead at that moment.
- mibpkg version for this work: 0.6.0.

## Plan rulings (where the plan settles what the spec left open or could not do as written)

- Module name `src/sourcelist.rs` (not `src/sources.rs`): `tests/sources.rs` already covers the
  effective-sources code in `online.rs`; a distinct name avoids confusion.
- Relative entries from the 0.3.x-era index are made absolute with the previous release's tag from
  `gh release view` (the `Publisher` trait), not from a 302 `Location` header: the `Fetcher` trait
  returns bodies only, and `gh` is already required to publish.
- Per-source fetch problems (including collection-id conflicts) and the list error are written to
  `/tmp/mibpkg.online-errors.json` (RAM) by the online check and shown by `/api/sources` as
  `error` per source and `list.error`; the spec's `conflict` field is this `error` field.
- A list fetch failure is reported (`Report.list_error`, summary text) but does not count as a
  failed check (it would otherwise make the supervisor retry every few minutes for a list problem).
- The list cache records `fetched_at`; it is rewritten when fetched (at most once per 24 h), which
  is the only flash write the list adds.

## Review Focus

1. A list or `sources.toml` with one malformed entry (missing `source`, `http://` URL, 500-char
   name, wrong types) → that entry is skipped with a reason; every other entry still shows.
2. Unit clock still 1970 and no offset yet → the list is fetched on every check (never "fresh
   forever"); a cache `fetched_at` in the future (clock jumped back) → refetch.
3. A source that is removed and re-added after another source took its collection id → stays
   refused until the other source is removed; removing the owner frees the id on the next check.
4. A package whose recorded collection disappeared while a *same-named* package exists in two
   other collections → no automatic update (count excludes it), explicit choice works, and the old
   guard still refuses a cross-collection install while the recorded collection is present.
5. `mibpkg release` re-run after a failed `gh release create` → reuses/clears its own `out` dir and
   publishes the same files; a reused version with different contents is refused before anything
   is uploaded.

Each line has a test in the owning task (Tasks 1, 2, 3, 4, 6).

---

### Task 1: Source list and sources.toml parsing, available sources

**Files:**
- Create: `src/sourcelist.rs`
- Modify: `src/lib.rs` (add `pub mod sourcelist;` after `pub mod online;`)
- Test: `tests/sourcelist.rs`

**Interfaces:**
- Consumes: `online::index_url(&str) -> Result<String>`, `online::effective_sources(&Layout) -> Vec<EffectiveSource>`, `media::roots(&Layout) -> Vec<PathBuf>`.
- Produces:
  - `pub struct ListEntry { pub name: String, pub description: String, pub source: String }` (Serialize, Deserialize, Clone, Debug, PartialEq)
  - `pub struct Parsed { pub entries: Vec<ListEntry>, pub skipped: Vec<String> }` (Default, Debug)
  - `pub fn parse_list(bytes: &[u8]) -> Result<Parsed>`
  - `pub fn parse_card(text: &str) -> Result<Parsed>`
  - `pub fn card_sources(layout: &Layout) -> Parsed`
  - `pub struct Available { pub name: String, pub description: String, pub source: String, pub origin: &'static str }` (Serialize, Debug, Clone, PartialEq)
  - `pub fn available(layout: &Layout, list: &[ListEntry]) -> Vec<Available>`

- [ ] **Step 1: Write the failing tests**

`tests/sourcelist.rs`:

```rust
//! The community source list (sources.json) and an SD card's sources.toml.
mod common;
use mibpkg::online;
use mibpkg::sourcelist::{self, ListEntry};
use std::fs;

const LIST: &str = r#"{ "format": 1, "sources": [
  { "name": "mibpkg", "description": "The package manager", "source": "github:complex256/mibpkg-packages" },
  { "name": "alice", "source": "github:alice/mib-tools", "key": "ed25519:ignored", "extra": 5 },
  { "name": "no source" },
  { "name": "plain http", "source": "http://example.com/index.json" },
  { "name": "XXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXX", "source": "github:a/b" },
  { "name": 7, "source": "github:c/d" }
] }"#;

#[test]
fn list_keeps_good_entries_and_explains_bad_ones() {
    let p = sourcelist::parse_list(LIST.as_bytes()).unwrap();
    assert_eq!(p.entries.len(), 2, "{:?}", p.skipped);
    assert_eq!(p.entries[1], ListEntry { name: "alice".into(), description: String::new(), source: "github:alice/mib-tools".into() });
    assert_eq!(p.skipped.len(), 4);
    assert!(p.skipped[0].starts_with("entry 3:"), "{:?}", p.skipped);
    assert!(p.skipped.iter().any(|s| s.contains("1-60")), "{:?}", p.skipped);
}

#[test]
fn list_with_unknown_format_or_garbage_is_an_error() {
    assert!(sourcelist::parse_list(br#"{"format":2,"sources":[]}"#).unwrap_err().to_string().contains("format 2"));
    assert!(sourcelist::parse_list(b"<html>").is_err());
    assert!(sourcelist::parse_list(br#"{"format":1}"#).is_err());
}

#[test]
fn card_sources_toml_is_parsed_per_entry() {
    let p = sourcelist::parse_card(
        "[[source]]\nname = \"alice's tools\"\ndescription = \"dashcam\"\nsource = \"github:alice/mib-tools\"\nkey = \"ed25519:x\"\n\n[[source]]\nname = \"bad\"\nsource = \"ftp://x\"\n",
    )
    .unwrap();
    assert_eq!(p.entries.len(), 1);
    assert_eq!(p.entries[0].description, "dashcam");
    assert_eq!(p.skipped.len(), 1);
    assert!(sourcelist::parse_card("not toml [").is_err());
}

#[test]
fn card_sources_are_read_from_every_card() {
    let f = common::fixture();
    fs::create_dir_all(f.sd().join("mibpkg")).unwrap();
    fs::write(f.sd().join("mibpkg/sources.toml"), "[[source]]\nname = \"alice\"\nsource = \"github:alice/mib-tools\"\n").unwrap();
    let p = sourcelist::card_sources(&f.layout);
    assert_eq!(p.entries.len(), 1);
    fs::write(f.sd().join("mibpkg/sources.toml"), "garbage [").unwrap();
    let p = sourcelist::card_sources(&f.layout);
    assert!(p.entries.is_empty());
    assert!(p.skipped[0].contains("sources.toml"), "{:?}", p.skipped);
}

#[test]
fn available_excludes_enabled_sources_and_prefers_card_labels() {
    let f = common::fixture();
    fs::create_dir_all(f.sd().join("mibpkg")).unwrap();
    fs::write(f.sd().join("mibpkg/sources.toml"), "[[source]]\nname = \"Alice (card)\"\nsource = \"github:alice/mib-tools\"\n\n[[source]]\nname = \"Bob\"\nsource = \"github:bob/x\"\n").unwrap();
    let list = sourcelist::parse_list(LIST.as_bytes()).unwrap().entries;
    let a = sourcelist::available(&f.layout, &list);
    // The default source is already enabled: not available.
    assert!(a.iter().all(|x| x.source != online::DEFAULT_SOURCE), "{a:?}");
    let alice = a.iter().find(|x| x.source == "github:alice/mib-tools").unwrap();
    assert_eq!((alice.name.as_str(), alice.origin), ("Alice (card)", "card"));
    assert!(a.iter().any(|x| x.source == "github:bob/x" && x.origin == "card"));
    online::add_source(&f.layout, "github:bob/x").unwrap();
    assert!(sourcelist::available(&f.layout, &list).iter().all(|x| x.source != "github:bob/x"));
}
```

- [ ] **Step 2: Run the tests to see them fail**

Run: `cargo test --test sourcelist`
Expected: compile error, "could not find `sourcelist` in `mibpkg`".

- [ ] **Step 3: Implement `src/sourcelist.rs`**

```rust
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
```

Add `pub mod sourcelist;` to `src/lib.rs` right after `pub mod online;`.

- [ ] **Step 4: Run the tests**

Run: `cargo test --test sourcelist`
Expected: 5 passed.

- [ ] **Step 5: Commit**

```bash
git add src/sourcelist.rs src/lib.rs tests/sourcelist.rs
git commit -m "feat: parse the community source list and a card's sources.toml; available sources"
```

---

### Task 2: Fetch and cache the list during the online check

**Files:**
- Modify: `src/paths.rs` (Settings: `online_list`), `src/sourcelist.rs` (list URL, cache, due rule), `src/online.rs` (`Fetched`, `fetch_all`, `store`, `Report.list_error`, errors file), `src/cli.rs` (`type Fetched`)
- Test: `tests/sourcelist.rs`, `tests/online.rs` (existing tests must still pass)

**Interfaces:**
- Consumes: Task 1's `parse_list`, `ListEntry`; `clock::read_offset(&Layout) -> Option<i64>`; `fetch::{Fetcher, MAX_INDEX}`.
- Produces:
  - `Settings.online_list: String`; `pub const DEFAULT_LIST: &str = "github:complex256/mibpkg-packages";` in `sourcelist`
  - `pub fn list_url(spec: &str) -> Result<Option<String>>`
  - `pub struct ListCache { pub fetched_at: u64, pub url: String, pub entries: Vec<ListEntry>, pub skipped: Vec<String> }` (Serialize, Deserialize, Clone, Debug, PartialEq)
  - `pub fn cache_path(layout: &Layout) -> PathBuf` (`state/sources-list.json`), `pub fn load_cache(layout: &Layout) -> Option<ListCache>`
  - `pub fn real_now(layout: &Layout) -> Option<u64>`
  - `pub fn fetch_list(layout: &Layout, fetcher: &dyn Fetcher, now: Option<u64>) -> Option<Result<ListCache>>`
  - `online::Fetched { pub indexes: Vec<(String, Result<Cached>)>, pub list: Option<Result<sourcelist::ListCache>> }`
  - `online::fetch_all(ctx, fetcher) -> Fetched`; `online::store(ctx, Fetched) -> Report`
  - `Report.list_error: Option<String>`
  - `pub struct OnlineErrors { pub sources: BTreeMap<String, String>, pub list: Option<String> }` (Serialize, Deserialize, Default) + `pub fn load_errors(layout) -> OnlineErrors` in `online`; written to `layout.run("online-errors.json")`.

- [ ] **Step 1: Write the failing tests** (append to `tests/sourcelist.rs`)

```rust
use mibpkg::ctx::Ctx;
use mibpkg::fetch::MemoryFetcher;
use mibpkg::sys::RecordingSys;

const LIST_URL: &str = "https://github.com/complex256/mibpkg-packages/releases/latest/download/sources.json";

#[test]
fn list_urls() {
    assert_eq!(sourcelist::list_url("github:o/r").unwrap().unwrap(), "https://github.com/o/r/releases/latest/download/sources.json");
    assert_eq!(sourcelist::list_url("https://x.org/s/sources.json").unwrap().unwrap(), "https://x.org/s/sources.json");
    assert_eq!(sourcelist::list_url("").unwrap(), None);
    assert!(sourcelist::list_url("http://x.org/sources.json").is_err());
}

fn list_fetcher() -> MemoryFetcher {
    let m = MemoryFetcher::default();
    m.files.borrow_mut().insert(LIST_URL.into(), LIST.as_bytes().to_vec());
    m
}

#[test]
fn the_check_fetches_and_caches_the_list() {
    let f = common::fixture();
    let sys = RecordingSys::default();
    let ctx = Ctx::new(f.layout.clone(), &sys);
    let m = list_fetcher();
    let r = online::check(&ctx, &m);
    assert_eq!(r.list_error, None);
    let c = sourcelist::load_cache(&f.layout).unwrap();
    assert_eq!(c.entries.len(), 2);
    assert_eq!(c.url, LIST_URL);
    // Fresh (fetched just now with a real host clock): not fetched again.
    let before = m.calls.borrow().iter().filter(|u| u.as_str() == LIST_URL).count();
    online::check(&ctx, &m);
    assert_eq!(m.calls.borrow().iter().filter(|u| u.as_str() == LIST_URL).count(), before);
}

#[test]
fn a_stale_or_future_cache_is_refetched() {
    let f = common::fixture();
    let now = sourcelist::real_now(&f.layout).expect("host clock is real");
    let m = list_fetcher();
    for fetched_at in [now - 25 * 3600, now + 3600] {
        let c = sourcelist::ListCache { fetched_at, url: LIST_URL.into(), entries: vec![], skipped: vec![] };
        fs::create_dir_all(f.layout.state_dir()).unwrap();
        fs::write(sourcelist::cache_path(&f.layout), serde_json::to_string(&c).unwrap()).unwrap();
        assert!(sourcelist::fetch_list(&f.layout, &m, Some(now)).is_some(), "fetched_at {fetched_at}");
    }
    // Unknown real time (unit at boot, no offset yet): always fetched.
    let c = sourcelist::ListCache { fetched_at: now, url: LIST_URL.into(), entries: vec![], skipped: vec![] };
    fs::write(sourcelist::cache_path(&f.layout), serde_json::to_string(&c).unwrap()).unwrap();
    assert!(sourcelist::fetch_list(&f.layout, &m, None).is_some());
    assert!(sourcelist::fetch_list(&f.layout, &m, Some(now)).is_none(), "fresh cache is used");
}

#[test]
fn a_failed_list_fetch_keeps_the_cache_and_does_not_fail_the_check() {
    let f = common::fixture();
    let sys = RecordingSys::default();
    let ctx = Ctx::new(f.layout.clone(), &sys);
    online::check(&ctx, &list_fetcher());
    let old = sourcelist::load_cache(&f.layout).unwrap();
    // Force a refetch against a server that no longer has the list.
    let mut stale = old.clone();
    stale.fetched_at = 0;
    fs::write(sourcelist::cache_path(&f.layout), serde_json::to_string(&stale).unwrap()).unwrap();
    let r = online::check(&ctx, &MemoryFetcher::default());
    assert!(r.list_error.as_deref().unwrap().contains("404"), "{:?}", r.list_error);
    assert_eq!(sourcelist::load_cache(&f.layout).unwrap().entries, old.entries);
    assert!(r.summary(100).contains("list:"), "{}", r.summary(100));
    assert!(online::load_errors(&f.layout).list.unwrap().contains("404"));
}

#[test]
fn an_empty_online_list_setting_turns_the_list_off() {
    let f = common::fixture();
    fs::create_dir_all(f.layout.home()).unwrap();
    fs::write(f.layout.home().join("settings.toml"), "online_list = \"\"\n").unwrap();
    let layout = mibpkg::paths::Layout::load(f.root()).unwrap();
    let m = list_fetcher();
    assert!(sourcelist::fetch_list(&layout, &m, None).is_none());
}
```

- [ ] **Step 2: Run to see them fail**

Run: `cargo test --test sourcelist`
Expected: compile errors (`list_url`, `ListCache`, `list_error`, … not found).

- [ ] **Step 3: Implement**

`src/paths.rs` — add to `Settings` (after `online_sources`) and to `Default`:

```rust
    /// The community source list (sources.json): github:<owner>/<repo> or an https URL; "" = off.
    pub online_list: String,
```
```rust
            online_list: crate::sourcelist::DEFAULT_LIST.to_string(),
```

`src/sourcelist.rs` — append:

```rust
use crate::fetch::{Fetcher, MAX_INDEX};
use std::path::PathBuf;

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
    crate::fsutil::atomic_write(&cache_path(layout), text.as_bytes())
}
```

(Move these `use` lines to the top of the file with the others.)

`src/online.rs` — replace the `Report` struct, `fetch_all` and `store`:

```rust
#[derive(Debug, Default)]
pub struct Report {
    pub checked: usize,
    pub errors: Vec<String>,
    pub updates: Vec<(String, String)>,
    /// The community list could not be fetched (the check itself still counts as done).
    pub list_error: Option<String>,
}
```

In `Report::summary`, after the errors are appended:

```rust
        if let Some(e) = &self.list_error {
            s.push_str(&format!("; list: {e}"));
        }
```

```rust
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
```

`src/cli.rs`: change `type Fetched = Vec<(String, Result<crate::online::Cached>)>;` to
`type Fetched = crate::online::Fetched;`.

- [ ] **Step 4: Run the whole suite**

Run: `cargo test 2>&1 | grep -E "test result|FAILED|panicked" | sort | uniq -c`
Expected: all `ok`. If a test in `tests/online.rs` counts `MemoryFetcher.calls` or asserts an exact
summary string, the extra list request/`list:` note is the cause: give that test's fixture
`online_list = ""` in `settings.toml` (as in `an_empty_online_list_setting_turns_the_list_off`)
rather than changing production behaviour.

- [ ] **Step 5: Commit**

```bash
git add src/paths.rs src/sourcelist.rs src/online.rs src/cli.rs tests/sourcelist.rs tests/online.rs
git commit -m "feat: the online check fetches the community source list (24 h cache, errors kept in RAM)"
```

---

### Task 3: One enabled source per collection id

**Files:**
- Modify: `src/online.rs` (`store_one`)
- Test: `tests/online.rs`

**Interfaces:**
- Consumes: `Cached { source, base_url, index }`, `cache_path`, `effective_sources`, Task 2's `OnlineErrors`.
- Produces: `store_one` refuses `collection <id> is already provided by <owner>`.

- [ ] **Step 1: Write the failing test** (append to `tests/online.rs`; it uses the file's existing `published` helper, `URL` and `BASE` constants)

```rust
#[test]
fn a_second_source_cannot_take_an_enabled_sources_collection_id() {
    let f = common::fixture();
    let sys = RecordingSys::default();
    let ctx = Ctx::new(f.layout.clone(), &sys);
    let (m, _) = published(&f, &["1.0.0"]);
    // The same index (collection id "complex256") also served by a second source.
    let index = m.files.borrow().get(URL).unwrap().clone();
    m.files.borrow_mut().insert("https://github.com/mallory/copy/releases/latest/download/index.json".into(), index);
    online::check(&ctx, &m);
    online::add_source(&f.layout, "github:mallory/copy").unwrap();
    let r = online::check(&ctx, &m);
    assert_eq!(r.errors.len(), 1, "{:?}", r.errors);
    assert!(r.errors[0].contains("already provided by"), "{:?}", r.errors);
    assert!(online::load_errors(&f.layout).sources["github:mallory/copy"].contains("already provided by"));
    // Removing the owner frees the id for the other source.
    online::remove_source(&f.layout, mibpkg::online::DEFAULT_SOURCE).unwrap();
    let r = online::check(&ctx, &m);
    assert!(r.errors.is_empty(), "{:?}", r.errors);
    let cached: online::Cached = serde_json::from_str(&fs::read_to_string(online::cache_path(&f.layout, "complex256")).unwrap()).unwrap();
    assert_eq!(cached.source, "github:mallory/copy");
}
```

(`published` builds its index from the fixture's default source URL `URL`; check the file's
constants at the top of `tests/online.rs` and adjust the URL literal if `URL` points elsewhere.)

- [ ] **Step 2: Run it**

Run: `cargo test --test online a_second_source`
Expected: FAIL — `r.errors` is empty (the second source silently overwrote the cache).

- [ ] **Step 3: Implement** — at the top of `store_one`, after `let path = …`:

```rust
    // One enabled source per collection id: without signatures this is what stops one source
    // from replacing another's packages. The cache records which source owns the id.
    let owner = fs::read_to_string(&path).ok().and_then(|t| serde_json::from_str::<Cached>(&t).ok()).map(|c| c.source);
    if let Some(owner) = owner {
        let owner_enabled = effective_sources(&ctx.layout).iter().any(|e| e.enabled && e.source == owner);
        if owner != cached.source && owner_enabled {
            bail!("collection {} is already provided by {owner}", cached.index.collection.id);
        }
    }
```

(`store_one` currently computes `path` after serializing; move the `let path = cache_path(…)`
line above this block.)

- [ ] **Step 4: Run the suite**

Run: `cargo test 2>&1 | grep -E "test result|FAILED" | sort | uniq -c`
Expected: all ok.

- [ ] **Step 5: Commit**

```bash
git add src/online.rs tests/online.rs
git commit -m "feat: only one enabled source may provide a collection id"
```

---

### Task 4: Package moved

**Files:**
- Modify: `src/install.rs` (`update_sources`, `pick_collection`, `updates_available`, guard in `install`), `src/catalog.rs` (`PkgView.update_from`, `PkgView.moved_choices`), `web/js/views/package.js` (Update button(s))
- Test: `tests/install.rs`, `tests/web_api.rs`

**Interfaces:**
- Consumes: `Collection`, `State`, `index::RESERVED_COLLECTION`, `catalog::collections(&Layout) -> Vec<Collection>`.
- Produces:
  - `pub fn update_sources(recorded: &str, name: &str, colls: &[Collection]) -> Vec<String>` in `install`
  - `PkgView.update_from: Option<String>`, `PkgView.moved_choices: Vec<MovedChoice>`; `pub struct MovedChoice { pub collection: String, pub version: String }` (Serialize, Debug, Clone)

- [ ] **Step 1: Write the failing tests** (append to `tests/install.rs`; reuse its existing imports and `common` helpers — `fixture`, `pkg_src`, `collection`, `RecordingSys`, `Ctx`, `State`, `install`, `media`)

```rust
fn card_colls(f: &common::Fixture) -> Vec<mibpkg::index::Collection> {
    mibpkg::media::loaded(&mibpkg::media::scan(&f.layout))
}

/// demo 1.0.0 installed from collection "old"; then the card only has `ids` offering 1.1.0.
fn moved_setup(ids: &[&str]) -> (common::Fixture, RecordingSys) {
    let f = common::fixture();
    let sys = RecordingSys::default();
    let v1 = common::pkg_src(f.root(), "demo", "1.0.0", "sleep 30", "");
    let old = common::collection(&f.sd(), "old", &[v1]);
    {
        let ctx = Ctx::new(f.layout.clone(), &sys);
        let colls = card_colls(&f);
        let c = install::pick_collection(&State::default(), &colls, "demo", Some("old"), &ctx.unit).unwrap();
        install::install(&ctx, &mibpkg::media::roots(&f.layout), c, "demo").unwrap();
    }
    std::fs::remove_dir_all(old).unwrap();
    let v2 = common::pkg_src(f.root(), "demo", "1.1.0", "sleep 30", "");
    for id in ids {
        common::collection(&f.sd(), id, &[v2.clone()]);
    }
    (f, sys)
}

#[test]
fn a_package_whose_collection_is_gone_updates_from_the_one_that_has_it() {
    let (f, sys) = moved_setup(&["new"]);
    let ctx = Ctx::new(f.layout.clone(), &sys);
    let state = State::load(&f.layout).unwrap();
    let colls = card_colls(&f);
    assert_eq!(install::update_sources("old", "demo", &colls), vec!["new".to_string()]);
    assert_eq!(install::updates_available(&state, &colls, &ctx.unit), vec![("demo".into(), "1.1.0".into())]);
    let c = install::pick_collection(&state, &colls, "demo", None, &ctx.unit).unwrap();
    assert_eq!(c.id(), "new");
    install::install(&ctx, &mibpkg::media::roots(&f.layout), c, "demo").unwrap();
    assert_eq!(State::load(&f.layout).unwrap().packages["demo"].collection, "new");
}

#[test]
fn several_new_homes_need_a_choice() {
    let (f, sys) = moved_setup(&["new-a", "new-b"]);
    let ctx = Ctx::new(f.layout.clone(), &sys);
    let state = State::load(&f.layout).unwrap();
    let colls = card_colls(&f);
    assert!(install::updates_available(&state, &colls, &ctx.unit).is_empty());
    let e = install::pick_collection(&state, &colls, "demo", None, &ctx.unit).unwrap_err().to_string();
    assert!(e.contains("new-a") && e.contains("new-b"), "{e}");
    let c = install::pick_collection(&state, &colls, "demo", Some("new-b"), &ctx.unit).unwrap();
    install::install(&ctx, &mibpkg::media::roots(&f.layout), c, "demo").unwrap();
    assert_eq!(State::load(&f.layout).unwrap().packages["demo"].collection, "new-b");
}

#[test]
fn the_cross_collection_guard_holds_while_the_recorded_collection_is_present() {
    let (f, sys) = moved_setup(&["new"]);
    // Put "old" back (still offering 1.0.0): it is present again, so "new" is refused.
    let v1 = common::pkg_src(f.root(), "demo", "1.0.0", "sleep 30", "");
    common::collection(&f.sd(), "old", &[v1]);
    let ctx = Ctx::new(f.layout.clone(), &sys);
    let state = State::load(&f.layout).unwrap();
    let colls = card_colls(&f);
    assert_eq!(install::update_sources("old", "demo", &colls), vec!["old".to_string()]);
    let c = install::pick_collection(&state, &colls, "demo", Some("new"), &ctx.unit).unwrap();
    let e = install::install(&ctx, &mibpkg::media::roots(&f.layout), c, "demo").unwrap_err().to_string();
    assert!(e.contains("remove it before installing it from new"), "{e}");
}
```

Append to `tests/web_api.rs` (uses its `setup()`/`body()` helpers; `setup` returns a fixture with
`demo` available on the card — read `setup` first and build the moved state the same way as
`moved_setup` above inside the test if `setup` does not fit):

```rust
#[test]
fn moved_packages_name_where_their_update_comes_from() {
    let f = common::fixture();
    let sys = mibpkg::sys::RecordingSys::default();
    let v1 = common::pkg_src(f.root(), "demo", "1.0.0", "sleep 30", "");
    let old = common::collection(&f.sd(), "old", &[v1]);
    {
        let ctx = Ctx::new(f.layout.clone(), &sys);
        let colls = mibpkg::media::loaded(&mibpkg::media::scan(&f.layout));
        install::install(&ctx, &mibpkg::media::roots(&f.layout), &colls[0], "demo").unwrap();
    }
    fs::remove_dir_all(old).unwrap();
    let v2 = common::pkg_src(f.root(), "demo", "1.1.0", "sleep 30", "");
    common::collection(&f.sd(), "new", &[v2]);
    let app = App::new(f.layout.clone(), Box::new(|| None));
    let (_, v) = body(api::packages(&app));
    let demo = v.as_array().unwrap().iter().find(|p| p["name"] == "demo").unwrap();
    assert_eq!(demo["update"], "1.1.0");
    assert_eq!(demo["update_from"], "new");
}
```

(If `api::packages` has a different name, use the handler that serves `GET /api/packages` in
`src/web/routes.rs`.)

- [ ] **Step 2: Run them**

Run: `cargo test --test install moved; cargo test --test install several; cargo test --test install guard_holds; cargo test --test web_api moved`
Expected: compile error (`update_sources` missing) — after adding a stub returning `vec![]`, the
tests fail on their assertions.

- [ ] **Step 3: Implement**

`src/install.rs` — add above `pick_collection`:

```rust
/// Collections an installed package takes updates from: its recorded collection while any card
/// or online source has it; otherwise (the package moved) every present collection offering the
/// name. Packages installed from an uploaded file never take updates.
pub fn update_sources(recorded: &str, name: &str, colls: &[Collection]) -> Vec<String> {
    if recorded == crate::index::RESERVED_COLLECTION {
        return Vec::new();
    }
    if colls.iter().any(|c| c.id() == recorded) {
        return vec![recorded.to_string()];
    }
    let mut ids: Vec<String> = colls.iter().filter(|c| c.latest(name).is_some()).map(|c| c.id().to_string()).collect();
    ids.sort();
    ids.dedup();
    ids
}
```

In `pick_collection`, replace

```rust
    if let Some(want) = want {
        return best_of(&want).with_context(|| format!("collection {want} is not on any card or online"));
    }
```

with

```rust
    if let Some(want) = want {
        if id.is_none() {
            // No explicit choice: the recorded collection, or where the package moved to.
            let ids = update_sources(&want, name, colls);
            return match ids.len() {
                0 => bail!("collection {want} is not on any card or online"),
                1 => best_of(&ids[0]).with_context(|| format!("collection {} is not on any card or online", ids[0])),
                _ => bail!("{name}: collection {want} is gone and several collections offer it ({}); choose one", ids.join(", ")),
            };
        }
        return best_of(&want).with_context(|| format!("collection {want} is not on any card or online"));
    }
```

In `updates_available`, replace the `let best = colls … .filter(|c| c.id() == inst.collection)`
selection with:

```rust
        let ids = update_sources(&inst.collection, name, colls);
        if ids.len() != 1 {
            continue; // nothing to update from, or a choice only the user can make
        }
        // Card and online copies of the same collection: the newest compatible version counts.
        let best = colls
            .iter()
            .filter(|c| c.id() == ids[0])
            .filter_map(|c| c.latest_compatible(name, unit))
            .max_by(|a, b| cmp_versions(&a.version, &b.version));
```

In `install`, change the guard

```rust
        if p.collection != coll_id {
```

to

```rust
        let recorded_present = crate::catalog::collections(layout).iter().any(|c| c.id() == p.collection);
        if p.collection != coll_id && recorded_present {
```

`src/catalog.rs` — add:

```rust
#[derive(Debug, Clone, Serialize)]
pub struct MovedChoice {
    pub collection: String,
    pub version: String,
}
```

Add to `PkgView` (after `update`):

```rust
    /// The collection `update` comes from (the recorded one, or where the package moved).
    pub update_from: Option<String>,
    /// The recorded collection is gone and several offer something newer: the user picks one.
    pub moved_choices: Vec<MovedChoice>,
```

In `build`, compute `let colls = collections(layout);` once before the `names.into_iter()` map, then
replace the `let update = …;` block with:

```rust
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
```

and construct `PkgView { name, description, installed, available, update, update_from, moved_choices }`.

`web/js/views/package.js` — in the main-button block (near line 96), change the update button's
collection from `inst.collection` to `p.update_from`, and add the moved-choices case right after it:

```js
  if (inst && p.update) {
    main = h('button', { class: 'pill primary', type: 'button', onclick: () => act({ action: 'install', package: p.name, collection: p.update_from }) }, `Update to ${p.update}`);
  } else if (inst && p.moved_choices && p.moved_choices.length) {
    // The package's collection is gone and several offer it: one button per collection.
    main = h('div', { style: 'display:flex;flex-direction:column;gap:6px' },
      p.moved_choices.map((c) => h('button', { class: 'pill', type: 'button', onclick: () => act({ action: 'install', package: p.name, collection: c.collection }) }, `Update to ${c.version} from ${c.collection}`)));
  }
```

(Keep whatever `else` branches follow the existing `if (inst && p.update)` block; only the first
branch changes and the new `else if` is inserted.)

- [ ] **Step 4: Run the suite**

Run: `cargo test 2>&1 | grep -E "test result|FAILED" | sort | uniq -c`
Expected: all ok.

- [ ] **Step 5: Commit**

```bash
git add src/install.rs src/catalog.rs web/js/views/package.js tests/install.rs tests/web_api.rs
git commit -m "feat: a package whose collection is gone updates from the collection that now has it"
```

---

### Task 5: Sources screen — available sources, errors, list status

**Files:**
- Modify: `src/web/api.rs` (`sources`), `web/js/views/sources.js`
- Test: `tests/web_api.rs` (update `sources_and_settings_never_leak_the_pin`, add one test)

**Interfaces:**
- Consumes: `sourcelist::{available, load_cache, list_url}`, `online::{effective_sources, load_errors}`.
- Produces: `GET /api/sources` →
  `{ "sources": [{source, origin, enabled, collection, collection_name, packages, error}],
     "available": [{name, description, source, origin}],
     "list": {url, fetched_at, error, skipped} }`

- [ ] **Step 1: Write the failing tests** (`tests/web_api.rs`)

In `sources_and_settings_never_leak_the_pin`, change the last assertion to
`assert_eq!(v["sources"][0]["source"], mibpkg::online::DEFAULT_SOURCE);`. Then add:

```rust
#[test]
fn sources_lists_what_could_be_enabled() {
    let (f, _) = setup();
    fs::create_dir_all(f.sd().join("mibpkg")).unwrap();
    fs::write(f.sd().join("mibpkg/sources.toml"), "[[source]]\nname = \"Alice\"\nsource = \"github:alice/mib-tools\"\n").unwrap();
    let cache = mibpkg::sourcelist::ListCache {
        fetched_at: 1_800_000_000,
        url: "https://github.com/complex256/mibpkg-packages/releases/latest/download/sources.json".into(),
        entries: vec![mibpkg::sourcelist::ListEntry { name: "Bob".into(), description: "tools".into(), source: "github:bob/x".into() }],
        skipped: vec!["entry 2: name must be 1-60 characters".into()],
    };
    mibpkg::sourcelist::store_cache(&f.layout, &cache).unwrap();
    fs::write(f.layout.run("online-errors.json"), r#"{"sources":{"github:complex256/mibpkg-packages":"HTTP 404"},"list":null}"#).unwrap();
    let app = App::new(f.layout.clone(), Box::new(|| None));
    let (_, v) = body(api::sources(&app));
    let avail = v["available"].as_array().unwrap();
    assert_eq!(avail.len(), 2, "{v}");
    assert_eq!(avail[0]["origin"], "card");
    assert_eq!(avail[1]["source"], "github:bob/x");
    assert_eq!(v["list"]["fetched_at"], 1_800_000_000u64);
    assert_eq!(v["list"]["skipped"][0], "entry 2: name must be 1-60 characters");
    assert_eq!(v["sources"][0]["error"], "HTTP 404");
}
```

- [ ] **Step 2: Run them**

Run: `cargo test --test web_api sources`
Expected: FAIL — `v["sources"]` is null (the endpoint returns an array).

- [ ] **Step 3: Implement**

`src/web/api.rs` `sources` — keep the existing per-source mapping, add `"error"`, and wrap:

```rust
pub fn sources(app: &App) -> Response {
    let layout = &app.layout;
    let cached = crate::online::collections(layout);
    let caches: Vec<crate::online::Cached> = fs::read_dir(layout.state_dir().join("online"))
        .map(|rd| rd.filter_map(|e| e.ok()).filter_map(|e| fs::read_to_string(e.path()).ok()).filter_map(|t| serde_json::from_str(&t).ok()).collect())
        .unwrap_or_default();
    let errors = crate::online::load_errors(layout);
    let sources: Vec<_> = crate::online::effective_sources(layout)
        .into_iter()
        .map(|s| {
            let c = caches.iter().find(|c| c.source == s.source);
            let packages = c.and_then(|c| cached.iter().find(|x| x.id() == c.index.collection.id)).map(|x| x.index.packages.len());
            json!({
                "source": s.source,
                "origin": s.origin,
                "enabled": s.enabled,
                "collection": c.map(|c| c.index.collection.id.clone()),
                "collection_name": c.map(|c| c.index.collection.name.clone()),
                "packages": packages,
                "error": errors.sources.get(&s.source),
            })
        })
        .collect();
    let list = crate::sourcelist::load_cache(layout);
    let available = crate::sourcelist::available(layout, list.as_ref().map(|l| l.entries.as_slice()).unwrap_or(&[]));
    let url = crate::sourcelist::list_url(&layout.settings.online_list).ok().flatten();
    Response::json(
        200,
        &json!({
            "sources": sources,
            "available": available,
            "list": {
                "url": url,
                "fetched_at": list.as_ref().map(|l| l.fetched_at),
                "error": errors.list,
                "skipped": list.as_ref().map(|l| l.skipped.clone()).unwrap_or_default(),
            },
        }),
    )
}
```

`web/js/views/sources.js` — replace the file:

```js
/*
  Sources: where the car looks for packages online.
  Enabled: sources the car checks (built in, from settings.toml, or added here).
  Available: sources from the community list and from the SD card's
  mibpkg/sources.toml — nothing is fetched from one until you enable it.
  A source is github:<owner>/<repo> (that repo's latest release) or an
  https://…/index.json address.
*/
import { api } from '../api.js';
import { backLink, h, pageHead, replace, toast } from '../dom.js';
import { jobView, redrawSoon } from '../jobs.js';

const ORIGIN = { default: 'built in', settings: 'settings.toml', added: 'added here', list: 'community list', card: 'SD card' };

export async function render() {
  const { sources, available, list } = await api('GET', '/api/sources');
  const jobArea = h('div', {});

  async function change(method, source) {
    try {
      const reply = await api(method, '/api/sources', { source });
      replace(jobArea, jobView(reply.job, { onDone: () => redrawSoon(1500) }));
    } catch (e) {
      toast(e.message, { error: true });
    }
  }

  const button = (label, onclick) => h('button', { class: 'pill', type: 'button', onclick }, label);
  const head = (title, btn) => h('div', { style: 'display:flex;justify-content:space-between;gap:8px;align-items:center' }, h('strong', { style: 'overflow-wrap:anywhere' }, title), btn);

  const enabled = sources.map((s) =>
    h(
      'li',
      { style: 'padding:12px 14px' },
      head(s.source, s.enabled ? button('Remove', () => change('DELETE', s.source)) : button('Turn on', () => change('POST', s.source))),
      h('div', { class: 'muted' }, [ORIGIN[s.origin] || s.origin, s.enabled ? null : 'off', s.collection ? `${s.collection_name || s.collection} · ${s.packages} packages` : 'not fetched yet'].filter(Boolean).join(' · ')),
      s.error ? h('div', { class: 'error' }, s.error) : null,
    ),
  );

  const offered = available.map((a) =>
    h(
      'li',
      { style: 'padding:12px 14px' },
      head(a.name, button('Enable', () => change('POST', a.source))),
      a.description ? h('div', {}, a.description) : null,
      h('div', { class: 'muted', style: 'overflow-wrap:anywhere' }, `${a.source} · ${ORIGIN[a.origin] || a.origin}`),
    ),
  );

  const listNote = list.url === null
    ? 'The community list is turned off (online_list in settings.toml).'
    : list.error
      ? `Community list: ${list.error}`
      : list.fetched_at
        ? `Community list checked ${new Date(list.fetched_at * 1000).toLocaleString()}.`
        : 'The community list is fetched with the next online check.';

  const input = h('input', { type: 'text', placeholder: 'github:owner/repo', autocapitalize: 'off', autocomplete: 'off', spellcheck: 'false' });
  const add = h('button', { class: 'button', type: 'button', onclick: () => input.value.trim() && change('POST', input.value.trim()) }, 'Add source');

  return h(
    'div',
    {},
    backLink('#/more', 'More'),
    pageHead('Sources'),
    h('h2', {}, 'Enabled'),
    h('ul', { class: 'menu' }, enabled),
    h('h2', {}, 'Available'),
    offered.length ? h('ul', { class: 'menu' }, offered) : h('p', { class: 'muted' }, 'Nothing new from the community list or the SD card.'),
    h('p', { class: 'muted' }, listNote),
    list.skipped.length ? h('details', {}, h('summary', {}, `${list.skipped.length} list entries skipped`), h('ul', {}, list.skipped.map((s) => h('li', {}, s)))) : null,
    h('label', { class: 'field', style: 'margin-top:20px' }, h('span', {}, 'Add a source (github:owner/repo or an https://…/index.json address)'), input),
    add,
    jobArea,
    h('p', { class: 'muted' }, 'Enabling or adding a source checks it online right away; the car needs internet for that.'),
  );
}
```

(If `dom.js`'s `h` does not skip `null` children, filter them: check `h` in `web/js/dom.js` first.
If there is no `.error` class in `web/style.css`, add `.error { color: var(--danger, #c0392b); }`
next to `.muted`.)

- [ ] **Step 4: Run the suite and the front-end tests**

Run: `cargo test 2>&1 | grep -E "test result|FAILED" | sort | uniq -c; node --test web/tests/`
Expected: all ok.

- [ ] **Step 5: Look at it in a browser**

Run the web server against the web fixture the web-interface work uses (see
`docs/superpowers/specs/2026-10-05-web-interface-design.md` Testing, or `scripts/` for a
fixture/dev-server script), open `#/sources` at phone width, and confirm the Enabled and Available
sections, the list note, and that Enable starts a job. Note what you saw in the commit message body.

- [ ] **Step 6: Commit**

```bash
git add src/web/api.rs web/js/views/sources.js web/style.css tests/web_api.rs
git commit -m "feat(web): Sources screen shows available sources from the list and the SD card, and fetch errors"
```

---

### Task 6: `mibpkg release`

**Files:**
- Create: `src/publish.rs`
- Modify: `src/lib.rs` (`pub mod publish;`), `src/fetch.rs` (`CurlFetcher::for_host`), `src/cli.rs` (`release` command, USAGE)
- Test: `tests/publish.rs`

**Interfaces:**
- Consumes: `archive::{pack, sha256_file, Package}`, `index::{Index, IndexEntry, CollectionMeta, Collection, cmp_versions, FORMAT}`, `online::index_url`, `fetch::{Fetcher, MAX_INDEX}`.
- Produces:
  - `pub trait Publisher { fn latest_tag(&self, repo: &str) -> Result<Option<String>>; fn create(&self, repo: &str, tag: &str, notes: &str, files: &[PathBuf]) -> Result<()>; }`
  - `pub struct Gh;` (real `Publisher`)
  - `pub struct ReleaseOpts { pub inputs: Vec<PathBuf>, pub repo: String, pub tag: String, pub id: Option<String>, pub name: Option<String>, pub new_id: bool, pub keep: usize, pub out: PathBuf, pub assets: Vec<PathBuf>, pub publish: bool }`
  - `pub struct ReleaseReport { pub index: Index, pub added: Vec<String>, pub dropped: Vec<String>, pub kept: usize, pub files: Vec<PathBuf>, pub summary: String }`
  - `pub fn release(opts: &ReleaseOpts, fetcher: &dyn Fetcher, publisher: &dyn Publisher) -> Result<ReleaseReport>`
  - `pub fn wait_live(fetcher: &dyn Fetcher, url: &str, want: &[u8], timeout: Duration, every: Duration) -> bool`
  - `pub fn download_url(repo: &str, tag: &str, file: &str) -> String`
  - `CurlFetcher::for_host() -> CurlFetcher`

- [ ] **Step 1: Write the failing tests** — `tests/publish.rs`:

```rust
//! Host publisher tooling: mibpkg release.
mod common;
use mibpkg::fetch::MemoryFetcher;
use mibpkg::index::Index;
use mibpkg::publish::{self, Publisher, ReleaseOpts};
use std::cell::RefCell;
use std::fs;
use std::path::PathBuf;

const REPO: &str = "alice/tools";
const LATEST: &str = "https://github.com/alice/tools/releases/latest/download/index.json";

#[derive(Default)]
struct FakeGh {
    tag: Option<String>,
    created: RefCell<Vec<(String, Vec<String>)>>,
}
impl Publisher for FakeGh {
    fn latest_tag(&self, _repo: &str) -> anyhow::Result<Option<String>> {
        Ok(self.tag.clone())
    }
    fn create(&self, _repo: &str, tag: &str, _notes: &str, files: &[PathBuf]) -> anyhow::Result<()> {
        let mut names: Vec<String> = files.iter().map(|p| p.file_name().unwrap().to_string_lossy().into()).collect();
        names.sort();
        self.created.borrow_mut().push((tag.into(), names));
        Ok(())
    }
}

fn opts(f: &common::Fixture, inputs: Vec<PathBuf>, tag: &str) -> ReleaseOpts {
    ReleaseOpts {
        inputs,
        repo: REPO.into(),
        tag: tag.into(),
        id: Some("alice-tools".into()),
        name: Some("Alice's tools".into()),
        new_id: false,
        keep: 3,
        out: f.root().join(format!("out-{tag}")),
        assets: vec![],
        publish: false,
    }
}

/// Serve a release's index as the repo's latest.
fn serve(m: &MemoryFetcher, index: &Index) {
    m.files.borrow_mut().insert(LATEST.into(), serde_json::to_vec(index).unwrap());
}

#[test]
fn first_release_builds_and_links_absolute_urls() {
    let f = common::fixture();
    let src = common::pkg_src(f.root(), "demo", "1.0.0", "sleep 30", "");
    let gh = FakeGh::default();
    let r = publish::release(&opts(&f, vec![src], "v1"), &MemoryFetcher::default(), &gh).unwrap();
    assert_eq!(r.index.collection.id, "alice-tools");
    assert_eq!(r.index.packages.len(), 1);
    assert_eq!(r.index.packages[0].file, "https://github.com/alice/tools/releases/download/v1/demo-1.0.0.mpk");
    assert!(f.root().join("out-v1/index.json").is_file());
    assert!(f.root().join("out-v1/demo-1.0.0.mpk").is_file());
    assert!(gh.created.borrow().is_empty(), "no --publish, nothing uploaded");
}

#[test]
fn later_releases_carry_old_versions_and_keep_the_newest_n() {
    let f = common::fixture();
    let m = MemoryFetcher::default();
    let gh = FakeGh::default();
    for (i, v) in ["1.0.0", "1.1.0", "1.2.0", "1.3.0"].iter().enumerate() {
        let src = common::pkg_src(f.root(), "demo", v, "sleep 30", "");
        let mut o = opts(&f, vec![src], &format!("v{i}"));
        o.publish = true;
        let r = publish::release(&o, &m, &gh).unwrap();
        serve(&m, &r.index);
    }
    let last = gh.created.borrow().last().unwrap().clone();
    assert_eq!(last.0, "v3");
    assert_eq!(last.1, vec!["demo-1.3.0.mpk".to_string(), "index.json".into()], "only new files are uploaded");
    let r = publish::release(&opts(&f, vec![], "v4"), &m, &gh).unwrap();
    let versions: Vec<&str> = r.index.packages.iter().map(|e| e.version.as_str()).collect();
    assert_eq!(versions, ["1.1.0", "1.2.0", "1.3.0"], "keep 3 newest");
    assert!(r.index.packages[0].file.ends_with("/v1/demo-1.1.0.mpk"));
}

#[test]
fn a_reused_version_with_new_contents_is_refused_before_uploading() {
    let f = common::fixture();
    let m = MemoryFetcher::default();
    let gh = FakeGh::default();
    let src = common::pkg_src(f.root(), "demo", "1.0.0", "sleep 30", "");
    serve(&m, &publish::release(&opts(&f, vec![src], "v1"), &m, &gh).unwrap().index);
    let changed = common::pkg_src(&f.root().join("b"), "demo", "1.0.0", "sleep 99", "");
    let mut o = opts(&f, vec![changed], "v2");
    o.publish = true;
    let e = publish::release(&o, &m, &gh).unwrap_err().to_string();
    assert!(e.contains("1.0.0") && e.contains("different contents"), "{e}");
    assert!(gh.created.borrow().is_empty());
    // The identical file again is fine (kept once, not re-uploaded). Reuse the built file: packing
    // the same directory twice need not give identical bytes.
    let same = f.root().join("out-v1/demo-1.0.0.mpk");
    let r = publish::release(&opts(&f, vec![same], "v3"), &m, &gh).unwrap();
    assert_eq!(r.index.packages.len(), 1);
    assert!(r.index.packages[0].file.ends_with("/v1/demo-1.0.0.mpk"));
}

#[test]
fn the_collection_id_cannot_change_by_accident() {
    let f = common::fixture();
    let m = MemoryFetcher::default();
    let gh = FakeGh::default();
    let src = common::pkg_src(f.root(), "demo", "1.0.0", "sleep 30", "");
    serve(&m, &publish::release(&opts(&f, vec![src], "v1"), &m, &gh).unwrap().index);
    let mut o = opts(&f, vec![], "v2");
    o.id = Some("other".into());
    assert!(publish::release(&o, &m, &gh).unwrap_err().to_string().contains("--new-id"));
    o.new_id = true;
    assert_eq!(publish::release(&o, &m, &gh).unwrap().index.collection.id, "other");
    // Without --id a later release keeps the published id; a first release needs one.
    let mut o = opts(&f, vec![], "v3");
    o.id = None;
    assert_eq!(publish::release(&o, &m, &gh).unwrap().index.collection.id, "alice-tools");
    let mut o = opts(&f, vec![], "v1");
    o.id = None;
    assert!(publish::release(&o, &MemoryFetcher::default(), &gh).unwrap_err().to_string().contains("--id"));
}

#[test]
fn relative_entries_from_older_indexes_become_absolute() {
    let f = common::fixture();
    let m = MemoryFetcher::default();
    let gh = FakeGh { tag: Some("v2".into()), ..Default::default() };
    let src = common::pkg_src(f.root(), "demo", "1.0.0", "sleep 30", "");
    let mut idx = publish::release(&opts(&f, vec![src], "v2"), &m, &gh).unwrap().index;
    idx.packages[0].file = "demo-1.0.0.mpk".into(); // a 0.3.x-era flat index
    serve(&m, &idx);
    let r = publish::release(&opts(&f, vec![], "v3"), &m, &gh).unwrap();
    assert_eq!(r.index.packages[0].file, "https://github.com/alice/tools/releases/download/v2/demo-1.0.0.mpk");
}

#[test]
fn a_rerun_reuses_its_own_out_dir_and_assets_ride_along() {
    let f = common::fixture();
    let gh = FakeGh::default();
    let src = common::pkg_src(f.root(), "demo", "1.0.0", "sleep 30", "");
    let asset = f.root().join("sources.json");
    fs::write(&asset, "{}").unwrap();
    let mut o = opts(&f, vec![src], "v1");
    o.assets = vec![asset];
    publish::release(&o, &MemoryFetcher::default(), &gh).unwrap();
    let r = publish::release(&o, &MemoryFetcher::default(), &gh).unwrap();
    let mut names: Vec<String> = r.files.iter().map(|p| p.file_name().unwrap().to_string_lossy().into()).collect();
    names.sort();
    assert_eq!(names, ["demo-1.0.0.mpk", "index.json", "sources.json"]);
    // An out dir holding anything else is never wiped.
    fs::write(o.out.join("notes.txt"), "mine").unwrap();
    assert!(publish::release(&o, &MemoryFetcher::default(), &gh).unwrap_err().to_string().contains("not a release output"));
}

#[test]
fn wait_live_sees_the_new_index() {
    let m = MemoryFetcher::default();
    m.files.borrow_mut().insert(LATEST.into(), b"new".to_vec());
    let d = std::time::Duration::from_millis(1);
    assert!(publish::wait_live(&m, LATEST, b"new", d, d));
    assert!(!publish::wait_live(&m, LATEST, b"other", d, d));
}
```

- [ ] **Step 2: Run them**

Run: `cargo test --test publish`
Expected: compile error, "could not find `publish` in `mibpkg`".

- [ ] **Step 3: Implement `src/publish.rs`**

```rust
//! Host publisher tooling: `mibpkg release` builds packages into a GitHub release whose
//! index.json carries older versions forward (every entry an absolute download URL), and
//! `mibpkg verify` checks a published or local collection the way a car would.
use crate::archive::{self, sha256_file, Package};
use crate::fetch::{Fetcher, MAX_INDEX};
use crate::index::{cmp_versions, Collection, CollectionMeta, Index, IndexEntry, FORMAT};
use anyhow::{bail, Context, Result};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

/// Creates GitHub releases (real: the `gh` CLI; tests: a fake).
pub trait Publisher {
    /// Tag of the repo's latest release, None when it has none.
    fn latest_tag(&self, repo: &str) -> Result<Option<String>>;
    fn create(&self, repo: &str, tag: &str, notes: &str, files: &[PathBuf]) -> Result<()>;
}

pub struct Gh;

impl Publisher for Gh {
    fn latest_tag(&self, repo: &str) -> Result<Option<String>> {
        let out = Command::new("gh")
            .args(["release", "view", "--repo", repo, "--json", "tagName", "-q", ".tagName"])
            .output()
            .context("running gh (install the GitHub CLI and run `gh auth login`, or set GH_TOKEN)")?;
        if out.status.success() {
            let t = String::from_utf8_lossy(&out.stdout).trim().to_string();
            return Ok((!t.is_empty()).then_some(t));
        }
        let err = String::from_utf8_lossy(&out.stderr);
        if err.contains("release not found") {
            return Ok(None);
        }
        bail!("gh release view: {}", err.trim())
    }

    fn create(&self, repo: &str, tag: &str, notes: &str, files: &[PathBuf]) -> Result<()> {
        let st = Command::new("gh")
            .args(["release", "create", tag, "--repo", repo, "--title", tag, "--notes", notes])
            .args(files)
            .status()
            .context("running gh (install the GitHub CLI and run `gh auth login`, or set GH_TOKEN)")?;
        if !st.success() {
            bail!("gh release create failed; the files are still in the out directory, run again to retry");
        }
        Ok(())
    }
}

pub struct ReleaseOpts {
    pub inputs: Vec<PathBuf>,
    pub repo: String,
    pub tag: String,
    pub id: Option<String>,
    pub name: Option<String>,
    pub new_id: bool,
    pub keep: usize,
    pub out: PathBuf,
    pub assets: Vec<PathBuf>,
    pub publish: bool,
}

pub struct ReleaseReport {
    pub index: Index,
    /// "name version" of each package new in this release.
    pub added: Vec<String>,
    /// "name version" dropped by --keep.
    pub dropped: Vec<String>,
    /// Entries carried over from the previous release.
    pub kept: usize,
    /// Everything in the out directory (what --publish uploads).
    pub files: Vec<PathBuf>,
    pub summary: String,
}

pub fn download_url(repo: &str, tag: &str, file: &str) -> String {
    format!("https://github.com/{repo}/releases/download/{tag}/{file}")
}

fn latest_index_url(repo: &str) -> Result<String> {
    crate::online::index_url(&format!("github:{repo}"))
}

/// The out directory, emptied — but only if it holds nothing except what a release writes
/// (.mpk files, index.json, the given assets), so a mistyped --out never wipes anything else.
fn fresh_out(out: &Path, assets: &[PathBuf]) -> Result<()> {
    if out.exists() {
        let asset_names: Vec<_> = assets.iter().filter_map(|a| a.file_name().map(|n| n.to_os_string())).collect();
        for e in fs::read_dir(out)? {
            let e = e?;
            let n = e.file_name();
            let ours = e.file_type()?.is_file()
                && (n == "index.json" || n.to_string_lossy().ends_with(".mpk") || asset_names.contains(&n));
            if !ours {
                bail!("{} is not a release output directory (it contains {}); choose another --out", out.display(), n.to_string_lossy());
            }
        }
        fs::remove_dir_all(out)?;
    }
    fs::create_dir_all(out)?;
    Ok(())
}

/// The previous release's index; None for a first release (HTTP 404).
fn previous(fetcher: &dyn Fetcher, repo: &str) -> Result<Option<Index>> {
    let url = latest_index_url(repo)?;
    match fetcher.get(&url, MAX_INDEX) {
        Ok(b) => Ok(Some(serde_json::from_slice(&b).with_context(|| format!("{url}: not a valid index.json"))?)),
        Err(e) if format!("{e:#}").contains("HTTP 404") => Ok(None),
        Err(e) => Err(e).context("fetching the previous release's index"),
    }
}

/// Build or copy each input into `out` as <name>-<version>.mpk.
fn stage(inputs: &[PathBuf], out: &Path) -> Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    for input in inputs {
        let tmp = out.join(".staging.mpk");
        let m = if input.is_dir() {
            archive::pack(input, &tmp)?
        } else {
            fs::copy(input, &tmp).with_context(|| format!("reading {}", input.display()))?;
            Package::read(&tmp).with_context(|| format!("{} is not a valid package", input.display()))?.manifest
        };
        let dest = out.join(format!("{}-{}.mpk", m.package.name, m.package.version));
        if files.contains(&dest) {
            bail!("{} {} is given twice", m.package.name, m.package.version);
        }
        fs::rename(&tmp, &dest)?;
        files.push(dest);
    }
    Ok(files)
}

pub fn release(o: &ReleaseOpts, fetcher: &dyn Fetcher, publisher: &dyn Publisher) -> Result<ReleaseReport> {
    latest_index_url(&o.repo)?; // validates owner/repo
    if o.tag.is_empty() || !o.tag.bytes().all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b)) {
        bail!("tag {:?}: use letters, digits, '.', '_' and '-'", o.tag);
    }
    let prev = previous(fetcher, &o.repo)?;
    let id = match (&o.id, &prev) {
        (Some(id), Some(p)) if *id != p.collection.id && !o.new_id => bail!(
            "the published collection id is {:?}; changing it to {id:?} makes every car treat it as a different collection (pass --new-id if you mean it)",
            p.collection.id
        ),
        (Some(id), _) => id.clone(),
        (None, Some(p)) => p.collection.id.clone(),
        (None, None) => bail!("first release of {}: pass --id <collection-id> (and --name)", o.repo),
    };
    let name = o.name.clone().or_else(|| prev.as_ref().map(|p| p.collection.name.clone())).unwrap_or_else(|| id.clone());

    fresh_out(&o.out, &o.assets)?;
    let staged = stage(&o.inputs, &o.out)?;

    // Previous entries, with 0.3.x-era relative files made absolute against the previous release.
    let mut entries: Vec<IndexEntry> = Vec::new();
    let mut prev_tag: Option<String> = None;
    for mut e in prev.map(|p| p.packages).unwrap_or_default() {
        if !e.file.starts_with("https://") {
            if prev_tag.is_none() {
                prev_tag = Some(publisher.latest_tag(&o.repo)?.context("the previous index has relative package paths but the repo has no latest release tag")?);
            }
            e.file = download_url(&o.repo, prev_tag.as_deref().unwrap(), &e.file);
        }
        entries.push(e);
    }
    let kept_before = entries.len();

    let mut added = Vec::new();
    for f in &staged {
        let pkg = Package::read(f)?;
        let m = &pkg.manifest;
        let (sha256, size) = sha256_file(f)?;
        if let Some(old) = entries.iter().find(|e| e.name == m.package.name && e.version == m.package.version) {
            if old.sha256 != sha256 {
                bail!("{} {} is already published with different contents; bump the version", m.package.name, m.package.version);
            }
            fs::remove_file(f)?; // identical: keep the published file, upload nothing
            continue;
        }
        let fname = f.file_name().unwrap().to_string_lossy().to_string();
        entries.push(IndexEntry {
            name: m.package.name.clone(),
            version: m.package.version.clone(),
            file: download_url(&o.repo, &o.tag, &fname),
            sha256,
            size,
            description: m.package.description.clone(),
            compat: m.compat.clone(),
            uses_scripts: m.uses_scripts(),
        });
        added.push(format!("{} {}", m.package.name, m.package.version));
    }

    // Keep the newest `keep` versions of each name.
    let mut by_name: BTreeMap<String, Vec<IndexEntry>> = BTreeMap::new();
    for e in entries {
        by_name.entry(e.name.clone()).or_default().push(e);
    }
    let mut packages = Vec::new();
    let mut dropped = Vec::new();
    for (_, mut v) in by_name {
        v.sort_by(|a, b| cmp_versions(&b.version, &a.version));
        for (i, e) in v.into_iter().enumerate() {
            if i < o.keep.max(1) {
                packages.push(e);
            } else {
                dropped.push(format!("{} {}", e.name, e.version));
            }
        }
    }
    packages.sort_by(|a, b| a.name.cmp(&b.name).then(cmp_versions(&a.version, &b.version)));
    let index = Index { format: FORMAT, collection: CollectionMeta { id, name }, packages, signatures: vec![] };
    Collection::from_online(index.clone(), "https://github.com/").context("the new index would be refused by cars")?;

    let mut text = serde_json::to_string_pretty(&index)?;
    text.push('\n');
    fs::write(o.out.join("index.json"), text)?;
    for a in &o.assets {
        let n = a.file_name().context("asset without a file name")?;
        if n == "index.json" || o.out.join(n).exists() {
            bail!("asset {} would replace a release file", a.display());
        }
        fs::copy(a, o.out.join(n)).with_context(|| format!("copying {}", a.display()))?;
    }
    let mut files: Vec<PathBuf> = fs::read_dir(&o.out)?.filter_map(|e| e.ok().map(|e| e.path())).collect();
    files.sort();

    let kept = index.packages.len() - added.len();
    let summary = format!(
        "{}: added {}; kept {} older; dropped {}",
        o.tag,
        if added.is_empty() { "nothing".to_string() } else { added.join(", ") },
        kept.min(kept_before),
        if dropped.is_empty() { "nothing".to_string() } else { dropped.join(", ") }
    );
    if o.publish {
        publisher.create(&o.repo, &o.tag, &summary, &files)?;
    }
    Ok(ReleaseReport { index, added, dropped, kept, files, summary })
}

/// Poll `url` until it serves `want` (GitHub's latest-release redirect lags ~20 s).
pub fn wait_live(fetcher: &dyn Fetcher, url: &str, want: &[u8], timeout: Duration, every: Duration) -> bool {
    let start = Instant::now();
    loop {
        if fetcher.get(url, MAX_INDEX).map_or(false, |b| b == want) {
            return true;
        }
        if start.elapsed() >= timeout {
            return false;
        }
        std::thread::sleep(every);
    }
}
```

`src/fetch.rs` — add to `impl CurlFetcher`:

```rust
    /// The publisher's computer: system libcurl and its CA store, real clock.
    pub fn for_host() -> CurlFetcher {
        let lib = if cfg!(target_os = "macos") { "/usr/lib/libcurl.4.dylib" } else { "libcurl.so.4" };
        CurlFetcher {
            lib: lib.into(),
            preload: vec![],
            ca_bundle: None,
            clock_offset: None,
            timeout_secs: 30,
            allow_http: false,
            clock_file: None,
            time_servers: vec![],
        }
    }
```

`src/cli.rs` — in the host-command match (next to `"build"` / `"index"`) add
`"release" => return host_release(args),`, and add:

```rust
fn host_release(args: &[String]) -> Result<i32> {
    let mut o = crate::publish::ReleaseOpts {
        inputs: vec![],
        repo: String::new(),
        tag: String::new(),
        id: None,
        name: None,
        new_id: false,
        keep: 3,
        out: PathBuf::new(),
        assets: vec![],
        publish: false,
    };
    let mut it = args.iter();
    while let Some(a) = it.next() {
        let mut val = |what: &str| it.next().cloned().with_context(|| format!("{a} needs {what}"));
        match a.as_str() {
            "--repo" => o.repo = val("owner/repo")?,
            "--tag" => o.tag = val("a tag")?,
            "--id" => o.id = Some(val("a collection id")?),
            "--name" => o.name = Some(val("a name")?),
            "--new-id" => o.new_id = true,
            "--keep" => o.keep = val("a number")?.parse().context("--keep needs a number")?,
            "--out" => o.out = PathBuf::from(val("a directory")?),
            "--asset" => o.assets.push(PathBuf::from(val("a file")?)),
            "--publish" => o.publish = true,
            s if s.starts_with("--") => bail!("unexpected option {s}"),
            s => o.inputs.push(PathBuf::from(s)),
        }
    }
    if o.repo.is_empty() || o.tag.is_empty() {
        bail!("release needs --repo <owner/repo> and --tag <tag>");
    }
    if o.out.as_os_str().is_empty() {
        o.out = PathBuf::from(format!("out/release-{}", o.tag));
    }
    let fetcher = crate::fetch::CurlFetcher::for_host();
    let r = crate::publish::release(&o, &fetcher, &crate::publish::Gh)?;
    crate::say!("{}", r.summary);
    crate::say!("files in {}", o.out.display());
    if o.publish {
        let url = crate::online::index_url(&format!("github:{}", o.repo))?;
        let want = fs::read(o.out.join("index.json"))?;
        if crate::publish::wait_live(&fetcher, &url, &want, Duration::from_secs(60), Duration::from_secs(5)) {
            crate::say!("published: cars with github:{} see it on their next check", o.repo);
        } else {
            crate::say!("published, but GitHub still serves the old index after 60 s; cars see the new one within minutes");
        }
    }
    Ok(0)
}
```

Add to `USAGE` (after the `index` line):
`  mibpkg release <pkg-dir|file.mpk>... --repo OWNER/REPO --tag TAG [--id ID --name NAME [--new-id]] [--keep N] [--out DIR] [--asset FILE]... [--publish]`

Add `pub mod publish;` to `src/lib.rs` after `pub mod catalog;`.

- [ ] **Step 4: Run the tests**

Run: `cargo test --test publish && cargo test 2>&1 | grep -E "test result|FAILED" | sort | uniq -c`
Expected: 7 passed in `publish`; whole suite ok.

- [ ] **Step 5: Cross-build check** (the module must not break the unit build)

Run: `scripts/build.sh > /tmp/claude-build.log 2>&1; echo $?; grep -E "^error" /tmp/claude-build.log | head`
Expected: `0`, no errors (`std::process::Command` is available on QNX; `gh` is only run on the host).

- [ ] **Step 6: Commit**

```bash
git add src/publish.rs src/lib.rs src/fetch.rs src/cli.rs tests/publish.rs
git commit -m "feat: mibpkg release builds a GitHub release that carries older versions forward"
```

---

### Task 7: `mibpkg verify`

**Files:**
- Modify: `src/publish.rs` (`verify`), `src/cli.rs` (`verify` command, USAGE)
- Test: `tests/publish.rs`

**Interfaces:**
- Consumes: Task 6's `publish` module, `Collection::{from_online, location, latest}`, `archive::Package::read`, `fetch::MAX_PACKAGE`, `unit::UnitInfo`.
- Produces:
  - `pub struct VerifyReport { pub checked: usize, pub problems: Vec<String>, pub notes: Vec<String> }`
  - `pub fn verify(target: &str, latest_only: bool, unit: Option<&crate::unit::UnitInfo>, fetcher: &dyn Fetcher) -> Result<VerifyReport>` (Err only when no index can be read)

- [ ] **Step 1: Write the failing tests** (append to `tests/publish.rs`)

```rust
/// A published release served by a MemoryFetcher: index + package bytes at their URLs.
fn published_release(f: &common::Fixture, versions: &[&str]) -> (MemoryFetcher, Index) {
    let m = MemoryFetcher::default();
    let gh = FakeGh::default();
    let mut index = None;
    for (i, v) in versions.iter().enumerate() {
        let src = common::pkg_src(f.root(), "demo", v, "sleep 30", "");
        let tag = format!("v{i}");
        let o = opts(f, vec![src], &tag);
        let r = publish::release(&o, &m, &gh).unwrap();
        for p in &r.files {
            let n = p.file_name().unwrap().to_string_lossy().to_string();
            if n.ends_with(".mpk") {
                m.files.borrow_mut().insert(publish::download_url(REPO, &tag, &n), fs::read(p).unwrap());
            }
        }
        serve(&m, &r.index);
        index = Some(r.index);
    }
    (m, index.unwrap())
}

#[test]
fn verify_passes_a_good_source() {
    let f = common::fixture();
    let (m, _) = published_release(&f, &["1.0.0", "1.1.0"]);
    let r = publish::verify("github:alice/tools", false, None, &m).unwrap();
    assert!(r.problems.is_empty(), "{:?}", r.problems);
    assert_eq!(r.checked, 2);
    assert_eq!(publish::verify("github:alice/tools", true, None, &m).unwrap().checked, 1);
}

#[test]
fn verify_reports_every_problem() {
    let f = common::fixture();
    let (m, mut idx) = published_release(&f, &["1.0.0", "1.1.0"]);
    idx.packages[0].sha256 = "0".repeat(64); // 1.0.0: wrong hash
    idx.packages[1].version = "1.2.0".into(); // 1.1.0's file claims 1.2.0
    serve(&m, &idx);
    let r = publish::verify("github:alice/tools", false, None, &m).unwrap();
    assert_eq!(r.problems.len(), 2, "{:?}", r.problems);
    assert!(r.problems.iter().any(|p| p.contains("sha256")), "{:?}", r.problems);
    assert!(r.problems.iter().any(|p| p.contains("holds demo 1.1.0")), "{:?}", r.problems);
}

#[test]
fn verify_reports_unreachable_files_and_bad_indexes() {
    let f = common::fixture();
    let (m, idx) = published_release(&f, &["1.0.0"]);
    m.files.borrow_mut().retain(|k, _| k.ends_with("index.json"));
    let r = publish::verify("github:alice/tools", false, None, &m).unwrap();
    assert!(r.problems[0].contains("404"), "{:?}", r.problems);
    let mut bad = idx.clone();
    bad.packages[0].file = "http://insecure/demo-1.0.0.mpk".into();
    serve(&m, &bad);
    assert!(publish::verify("github:alice/tools", false, None, &m).unwrap().problems[0].contains("https"));
    assert!(publish::verify("github:nobody/none", false, None, &m).is_err());
}

#[test]
fn verify_checks_a_local_release_dir_before_publishing() {
    let f = common::fixture();
    let src = common::pkg_src(f.root(), "demo", "1.0.0", "sleep 30", "");
    let o = opts(&f, vec![src], "v1");
    publish::release(&o, &MemoryFetcher::default(), &FakeGh::default()).unwrap();
    let r = publish::verify(o.out.to_str().unwrap(), false, None, &MemoryFetcher::default()).unwrap();
    assert!(r.problems.is_empty(), "{:?}", r.problems);
    assert_eq!(r.checked, 1);
}

#[test]
fn verify_notes_entries_another_unit_cannot_use() {
    let f = common::fixture();
    let src = common::pkg_src(f.root(), "demo", "1.0.0", "sleep 30", "[compat]\nunits = [\"MHI2\"]\n");
    let o = opts(&f, vec![src], "v1");
    publish::release(&o, &MemoryFetcher::default(), &FakeGh::default()).unwrap();
    let unit = mibpkg::unit::UnitInfo { unit: Some("MHI2Q".into()), firmware: None };
    let r = publish::verify(o.out.to_str().unwrap(), false, Some(&unit), &MemoryFetcher::default()).unwrap();
    assert!(r.problems.is_empty());
    assert!(r.notes[0].contains("not for MHI2Q"), "{:?}", r.notes);
}
```

(`pkg_src`'s `extra` is appended after the `[[service]]` table; a `[compat]` table there is valid
TOML.)

- [ ] **Step 2: Run them**

Run: `cargo test --test publish verify`
Expected: compile error (`verify` not found).

- [ ] **Step 3: Implement** — append to `src/publish.rs`:

```rust
#[derive(Debug, Default)]
pub struct VerifyReport {
    pub checked: usize,
    pub problems: Vec<String>,
    pub notes: Vec<String>,
}

/// Check a source (github:… / https://…index.json), a local index.json, or a release output dir
/// the way a car would. Err only when no index can be read at all.
pub fn verify(target: &str, latest_only: bool, unit: Option<&crate::unit::UnitInfo>, fetcher: &dyn Fetcher) -> Result<VerifyReport> {
    let path = Path::new(target);
    let (bytes, base, local) = if path.exists() {
        let file = if path.is_dir() { path.join("index.json") } else { path.to_path_buf() };
        let dir = file.parent().map(Path::to_path_buf).unwrap_or_default();
        (fs::read(&file).with_context(|| format!("reading {}", file.display()))?, "https://local.invalid/".to_string(), Some(dir))
    } else {
        let url = crate::online::index_url(target)?;
        (fetcher.get(&url, MAX_INDEX)?, crate::online::base_of(&url), None)
    };
    let mut r = VerifyReport::default();
    let index: Index = serde_json::from_slice(&bytes).context("not a valid index.json")?;
    let coll = match Collection::from_online(index, &base) {
        Ok(c) => c,
        Err(e) => {
            r.problems.push(format!("index: {e:#}"));
            return Ok(r);
        }
    };
    let mut entries: Vec<&IndexEntry> = coll.index.packages.iter().collect();
    if latest_only {
        entries.retain(|e| coll.latest(&e.name).map_or(false, |l| l.version == e.version));
    }
    let tmp = std::env::temp_dir().join(format!("mibpkg-verify-{}.mpk", std::process::id()));
    for e in entries {
        r.checked += 1;
        let what = format!("{} {}", e.name, e.version);
        if let Some(u) = unit {
            if !e.compat.matches(u) {
                r.notes.push(format!("{what}: not for {}", u.unit.as_deref().unwrap_or("this unit")));
            }
        }
        let basename = e.file.rsplit('/').next().unwrap_or(&e.file);
        let got = match &local {
            Some(dir) if dir.join(basename).is_file() => fs::read(dir.join(basename)).map_err(anyhow::Error::from),
            _ => match coll.location(e) {
                crate::index::Location::Remote(url) => fetcher.get(&url, crate::fetch::MAX_PACKAGE),
                crate::index::Location::Local(p) => fs::read(&p).map_err(anyhow::Error::from),
            },
        };
        let bytes = match got {
            Ok(b) => b,
            Err(err) => {
                r.problems.push(format!("{what}: {err:#}"));
                continue;
            }
        };
        if let Err(err) = fs::write(&tmp, &bytes) {
            r.problems.push(format!("{what}: writing a temp file: {err}"));
            continue;
        }
        let (sha, size) = match sha256_file(&tmp) {
            Ok(x) => x,
            Err(err) => {
                r.problems.push(format!("{what}: {err:#}"));
                continue;
            }
        };
        if size != e.size || sha != e.sha256 {
            r.problems.push(format!("{what}: size/sha256 do not match the index (got {size} bytes, sha256 {sha})"));
            continue;
        }
        match Package::read(&tmp) {
            Err(err) => r.problems.push(format!("{what}: not a valid package: {err:#}")),
            Ok(p) => {
                let m = &p.manifest;
                if m.package.name != e.name || m.package.version != e.version {
                    r.problems.push(format!("{what}: the file holds {} {}", m.package.name, m.package.version));
                } else if m.uses_scripts() != e.uses_scripts {
                    r.problems.push(format!("{what}: uses_scripts is {} in the index but {} in the package", e.uses_scripts, m.uses_scripts()));
                }
            }
        }
    }
    let _ = fs::remove_file(&tmp);
    Ok(r)
}
```

Note on the test `verify_reports_every_problem`: the entry whose `version` was rewritten to
`1.2.0` still points at the `demo-1.1.0.mpk` URL with the correct hash, so it passes the hash
check and fails with "the file holds demo 1.1.0".

`src/cli.rs` — host command `"verify" => return host_verify(args),`:

```rust
fn host_verify(args: &[String]) -> Result<i32> {
    let mut target = None;
    let mut latest = false;
    let mut unit = None;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--latest" => latest = true,
            "--unit" => unit = Some(crate::unit::UnitInfo { unit: Some(it.next().context("--unit needs a unit, e.g. MHI2Q")?.to_uppercase()), firmware: None }),
            s if s.starts_with("--") => bail!("unexpected option {s}"),
            s => target = Some(s.to_string()),
        }
    }
    let target = target.context("verify needs a source (github:owner/repo), an index.json, or a release directory")?;
    let r = crate::publish::verify(&target, latest, unit.as_ref(), &crate::fetch::CurlFetcher::for_host())?;
    for n in &r.notes {
        crate::say!("note: {n}");
    }
    for p in &r.problems {
        crate::say!("problem: {p}");
    }
    crate::say!("{} package(s) checked, {} problem(s)", r.checked, r.problems.len());
    Ok(if r.problems.is_empty() { 0 } else { 1 })
}
```

USAGE line: `  mibpkg verify <github:owner/repo|index.json|release-dir> [--latest] [--unit MHI2Q]`

- [ ] **Step 4: Run the tests**

Run: `cargo test --test publish && cargo test 2>&1 | grep -E "test result|FAILED" | sort | uniq -c`
Expected: all ok.

- [ ] **Step 5: Real GitHub, read-only** (no outward action)

Run: `cargo build --release && target/release/mibpkg verify github:complex256/mibpkg-packages`
Expected: `1 package(s) checked, 0 problem(s)` (v2 holds mibpkg 0.3.1 with a relative file name,
which resolves against the latest-download base).

- [ ] **Step 6: Commit**

```bash
git add src/publish.rs src/cli.rs tests/publish.rs
git commit -m "feat: mibpkg verify checks a source or release directory the way a car would"
```

---

### Task 8: `mibpkg new` and the files switched on when public

**Files:**
- Create: `src/scaffold.rs`, `action.yml`, `.github/workflows/host-release.yml`, `packaging/list-ci/verify-sources.yml`
- Modify: `src/lib.rs` (`pub mod scaffold;`), `src/cli.rs` (`new` command, USAGE)
- Test: `tests/publish.rs`

**Interfaces:**
- Consumes: `manifest::{valid_name, Manifest}`, `archive::pack`.
- Produces: `pub fn new_package(dir: &Path, name: &str) -> Result<Vec<PathBuf>>` (files written).

- [ ] **Step 1: Write the failing test** (append to `tests/publish.rs`)

```rust
#[test]
fn new_scaffolds_a_package_that_builds() {
    let f = common::fixture();
    let dir = f.root().join("mytool");
    let files = mibpkg::scaffold::new_package(&dir, "mytool").unwrap();
    assert!(files.iter().any(|p| p.ends_with(".github/workflows/mibpkg-release.yml")));
    fs::write(dir.join("bin/mytool"), "#!/bin/sh\nsleep 60\n").unwrap();
    let m = mibpkg::archive::pack(&dir, &f.root().join("mytool.mpk")).unwrap();
    assert_eq!((m.package.name.as_str(), m.package.version.as_str()), ("mytool", "0.1.0"));
    assert!(mibpkg::scaffold::new_package(&dir, "mytool").unwrap_err().to_string().contains("not empty"));
    assert!(mibpkg::scaffold::new_package(&f.root().join("x"), "Bad_Name").is_err());
}
```

(If `archive::pack` refuses a manifest whose `[[file]] src` is missing, the test writes
`bin/mytool` before packing — as above. Check `archive::pack` for other required files.)

- [ ] **Step 2: Run it**

Run: `cargo test --test publish new_scaffolds`
Expected: compile error (`scaffold` not found).

- [ ] **Step 3: Implement `src/scaffold.rs`**

```rust
//! `mibpkg new <name>`: a package directory a developer can build, release and verify.
use crate::manifest::{valid_name, Manifest};
use anyhow::{bail, Result};
use std::fs;
use std::path::{Path, PathBuf};

fn manifest(name: &str) -> String {
    format!(
        r#"# mibpkg package manifest. Reference: the mibpkg README ("Package format").
[package]
name = "{name}"
version = "0.1.0"            # bump for every release (semver); a version is never reused
description = "What {name} does, in one line"

# Units it runs on; leave out to allow every unit.
# [compat]
# units = ["MHI2Q"]

# Files copied into the package directory on the car (@pkg).
[[file]]
src = "bin/{name}"
mode = "755"

# A long-running program the supervisor starts at boot and restarts if it exits.
[[service]]
name = "{name}"
exec = "@pkg/bin/{name}"
# args  = ["--config", "@config/{name}.toml"]
# after = ["internet"]       # wait for: internet, tailnet, hmi-ready, file:/path, iface:<name>, route:<dest>
"#
    )
}

fn workflow() -> &'static str {
    r#"# Publishes a release when you push a tag like v1.2.0:
#   git tag v1.2.0 && git push --tags
# Cars that have this repo as a source (github:<owner>/<repo>) offer the update.
name: mibpkg release
on:
  push:
    tags: ['v*']
permissions:
  contents: write
jobs:
  release:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      # Put your build here: the package directory must contain bin/<name> before releasing.
      - uses: complex256/mibpkg@v1
        with:
          packages: .
"#
}

fn readme(name: &str) -> String {
    format!(
        "# {name}\n\nA [mibpkg](https://github.com/complex256/mibpkg) package for MIB2 High head units.\n\n\
         ## Release\n\n```sh\nmibpkg release . --repo <owner>/{name} --tag v0.1.0 --id <your-collection-id> --name \"<your name>'s packages\" --publish\n```\n\n\
         Later releases need only `--repo` and `--tag` (or push a `v*` tag: see `.github/workflows/mibpkg-release.yml`).\n\n\
         ## Check what cars will see\n\n```sh\nmibpkg verify github:<owner>/{name}\n```\n\n\
         Cars add the source `github:<owner>/{name}` in the web interface (More → Sources).\n"
    )
}

/// Write a new package skeleton into `dir` (created; must be empty if it exists).
pub fn new_package(dir: &Path, name: &str) -> Result<Vec<PathBuf>> {
    if !valid_name(name) {
        bail!("package name {name:?} must be 1-40 of [a-z0-9-]");
    }
    if dir.exists() && fs::read_dir(dir)?.next().is_some() {
        bail!("{} is not empty", dir.display());
    }
    let m = manifest(name);
    Manifest::parse(&m)?; // the template must always be valid
    let files: Vec<(PathBuf, String)> = vec![
        (dir.join("manifest.toml"), m),
        (dir.join("bin/.keep"), String::new()),
        (dir.join("README.md"), readme(name)),
        (dir.join(".github/workflows/mibpkg-release.yml"), workflow().to_string()),
    ];
    for (p, text) in &files {
        fs::create_dir_all(p.parent().unwrap())?;
        fs::write(p, text)?;
    }
    Ok(files.into_iter().map(|(p, _)| p).collect())
}
```

`src/cli.rs` host command `"new" => return host_new(args),`:

```rust
fn host_new(args: &[String]) -> Result<i32> {
    let name = arg(args, 0, "package name")?;
    let mut dir = PathBuf::from(name);
    let mut it = args[1..].iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--dir" => dir = PathBuf::from(it.next().context("--dir needs a directory")?),
            other => bail!("unexpected argument {other:?}"),
        }
    }
    for f in crate::scaffold::new_package(&dir, name)? {
        crate::say!("{}", f.display());
    }
    crate::say!("next: put your program at {}/bin/{name}, then `mibpkg release` (see README.md)", dir.display());
    Ok(0)
}
```

USAGE line: `  mibpkg new <name> [--dir DIR]`. Add `pub mod scaffold;` to `src/lib.rs`.

`action.yml` (repo root; inactive until the repo is public):

```yaml
# Publishes a mibpkg release from a package repository. Used by the workflow `mibpkg new` writes.
# Requires the mibpkg repository to be public (it downloads the Linux host binary from there).
name: mibpkg release
description: Build mibpkg packages and publish them as a GitHub release cars can update from
inputs:
  packages:
    description: Package directories or .mpk files, space separated
    default: .
  tag:
    description: Release tag (default the pushed tag)
    default: ${{ github.ref_name }}
  args:
    description: Extra arguments for mibpkg release (e.g. --id, --name, --keep)
    default: ''
runs:
  using: composite
  steps:
    - shell: bash
      env:
        GH_TOKEN: ${{ github.token }}
      run: |
        set -euo pipefail
        gh release download --repo complex256/mibpkg --pattern 'mibpkg-host-linux-x86_64' --output /tmp/mibpkg
        chmod +x /tmp/mibpkg
        /tmp/mibpkg release ${{ inputs.packages }} --repo "${{ github.repository }}" --tag "${{ inputs.tag }}" ${{ inputs.args }} --publish
```

`.github/workflows/host-release.yml` (builds the Linux host binary for a mibpkg release; only
useful once public):

```yaml
# Attaches the Linux x86_64 host mibpkg (release/verify/new/build/index) to each mibpkg release.
name: host binary
on:
  release:
    types: [published]
permissions:
  contents: write
jobs:
  linux:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - run: cargo build --release
      - run: cp target/release/mibpkg mibpkg-host-linux-x86_64
      - env:
          GH_TOKEN: ${{ github.token }}
        run: gh release upload "${{ github.event.release.tag_name }}" mibpkg-host-linux-x86_64 --clobber
```

`packaging/list-ci/verify-sources.yml` (to be copied into `complex256/mibpkg-packages` as
`.github/workflows/verify-sources.yml` when mibpkg is public):

```yaml
# On a pull request that changes sources.json: run `mibpkg verify --latest` on every entry.
name: verify sources
on:
  pull_request:
    paths: ['sources.json']
jobs:
  verify:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - env:
          GH_TOKEN: ${{ github.token }}
        run: |
          set -euo pipefail
          gh release download --repo complex256/mibpkg --pattern 'mibpkg-host-linux-x86_64' --output /tmp/mibpkg
          chmod +x /tmp/mibpkg
          for s in $(jq -r '.sources[].source' sources.json); do
            echo "== $s"; /tmp/mibpkg verify "$s" --latest
          done
```

- [ ] **Step 4: Run the tests**

Run: `cargo test --test publish && cargo test 2>&1 | grep -E "test result|FAILED" | sort | uniq -c`
Expected: all ok.

- [ ] **Step 5: Commit**

```bash
git add src/scaffold.rs src/lib.rs src/cli.rs tests/publish.rs action.yml .github/workflows/host-release.yml packaging/list-ci/verify-sources.yml
git commit -m "feat: mibpkg new scaffolds a package; release action, host binary and list CI (inactive until public)"
```

---

### Task 9: Official publishing through `release`, the list file, version 0.6.0

**Files:**
- Create: `packaging/sources.json`
- Modify: `scripts/publish-official.sh`, `Cargo.toml` (version `0.6.0`), `docs/ideas.md`, `tests/` only if a version string is asserted
- Test: `tests/publish.rs` (the committed list parses)

- [ ] **Step 1: Write the failing test** (append to `tests/publish.rs`)

```rust
#[test]
fn the_committed_source_list_is_valid() {
    let text = fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/packaging/sources.json")).unwrap();
    let p = mibpkg::sourcelist::parse_list(&text).unwrap();
    assert!(p.skipped.is_empty(), "{:?}", p.skipped);
    assert!(p.entries.iter().any(|e| e.source == mibpkg::online::DEFAULT_SOURCE));
}
```

- [ ] **Step 2: Run it**

Run: `cargo test --test publish the_committed_source_list`
Expected: FAIL (file missing).

- [ ] **Step 3: Implement**

`packaging/sources.json`:

```json
{
  "format": 1,
  "sources": [
    {
      "name": "mibpkg",
      "description": "The package manager itself (official collection)",
      "source": "github:complex256/mibpkg-packages"
    }
  ]
}
```

`scripts/publish-official.sh`:

```bash
#!/usr/bin/env bash
# Publish the official collection (complex256-mibpkg) and the community source list as a release
# of complex256/mibpkg-packages. Older versions stay listed (mibpkg release carries them forward).
#   scripts/publish-official.sh <tag> <mpk>...
set -euo pipefail
ROOT=$(cd "$(dirname "$0")/.." && pwd)
TAG=$1; shift
cargo build --release --manifest-path "$ROOT/Cargo.toml"
"$ROOT/target/release/mibpkg" release "$@" \
    --repo complex256/mibpkg-packages --tag "$TAG" \
    --id complex256-mibpkg --name "mibpkg" \
    --asset "$ROOT/packaging/sources.json" \
    --out "$ROOT/out/release-$TAG" --publish
"$ROOT/target/release/mibpkg" verify github:complex256/mibpkg-packages --latest
```

`Cargo.toml`: `version = "0.6.0"`.

`docs/ideas.md`: change the M3/M4 line `- [designing 2026-10-11, unsigned] M3 sources + M4 publisher tooling` to
`- [built 2026-10-xx, mibpkg 0.6.0; verify on the car] M3 sources + M4 publisher tooling` (use the
actual date), and add under "Before going public":
`- Switch on: action.yml, .github/workflows/host-release.yml, packaging/list-ci/verify-sources.yml (copy into mibpkg-packages).`

- [ ] **Step 4: Run the suite and the cross-build**

Run: `cargo test 2>&1 | grep -E "test result|FAILED" | sort | uniq -c; scripts/build.sh >/tmp/claude-build.log 2>&1; echo build=$?`
Expected: all ok, `build=0`.

- [ ] **Step 5: Commit**

```bash
git add packaging/sources.json scripts/publish-official.sh Cargo.toml Cargo.lock docs/ideas.md tests/publish.rs
git commit -m "chore: official releases go through mibpkg release and carry the source list; mibpkg 0.6.0"
```

---

### Task 10: Publish and verify on the car (needs the owner)

**Files:**
- Modify: `docs/unit-facts.md`, `docs/ideas.md` (results)

This task has outward actions and needs the car. Ask the owner before Step 2; do Steps 3-5 only
with the owner at the car.

- [ ] **Step 1: Build the 0.6.0 package**

Run: `scripts/build.sh && scripts/bundle.sh`
Expected: `out/sd/mibpkg/collections/complex256-mibpkg/packages/mibpkg-0.6.0.mpk`.

- [ ] **Step 2: Publish release v3 to `complex256/mibpkg-packages` (ASK FIRST)**

Ask: "Publish release v3 of complex256/mibpkg-packages with mibpkg 0.6.0 and sources.json?"
On yes, run: `scripts/publish-official.sh v3 out/sd/mibpkg/collections/complex256-mibpkg/packages/mibpkg-0.6.0.mpk`
Expected: summary `v3: added mibpkg 0.6.0; kept 1 older; dropped nothing`, "published: …", and
the final `verify` prints `0 problem(s)`. The v3 index lists 0.3.1 with an absolute `/download/v2/`
URL and 0.6.0 with `/download/v3/`.

- [ ] **Step 3: Update the car over the internet**

With the owner at the car: web UI → Updates → Check now → Update mibpkg to 0.6.0. Confirm over
ssh (`ssh audi '/tmp/mibpkg list'`): `installed mibpkg 0.6.0 [complex256-mibpkg]`.

- [ ] **Step 4: List and card sources on the car**

- Web UI → More → Sources: the Available section is empty (the list's only entry is already
  enabled) and the list note shows when it was checked.
- Put a `mibpkg/sources.toml` on the SD card (on the Mac; the car mounts the card read-only) with
  one entry `github:complex256/mibpkg-packages-test` named "card test"; after Rescan the Available
  section shows it with the SD card badge. Do not enable it (the repo does not exist): confirm only
  that it appears. Remove the file afterwards.

- [ ] **Step 5: Package moved on the car**

Ask the owner first: this leaves carmqtt on the car recorded under `complex256-carmqtt` with a
test version number (same code). On yes:
- Build a carmqtt package with version `0.1.3` from `../mib2q-carmqtt` main in a worktree in the
  scratchpad (`git -C ../mib2q-carmqtt worktree add <scratch>/carmqtt main`; never switch that
  repo's checked-out branch), changing only the version in its packaging manifest there; build it
  as its README/packaging script says. Never publish it online.
- On the Mac: back up the card's `mibpkg/collections/complex256/` to the scratchpad and remove it
  from the card; create `mibpkg/collections/complex256-carmqtt/` with that package
  (`mibpkg index <dir> --id complex256-carmqtt --name "carmqtt"`).
- At the car: the carmqtt package page offers "Update to 0.1.3"; tap it. Then
  `ssh audi '/tmp/mibpkg list'` shows `installed carmqtt 0.1.3 [complex256-carmqtt]` and the
  service is running.
- On the Mac: restore `mibpkg/collections/complex256/` to the card.

- [ ] **Step 6: Record and commit**

Add the results to `docs/unit-facts.md` (what was seen, mibpkg version, date) and mark the
M3/M4 line in `docs/ideas.md` as verified on the car.

```bash
git add docs/unit-facts.md docs/ideas.md
git commit -m "docs: online sources and publisher tooling verified on the unit"
```
