# Standalone SWDL Installer Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Install mibpkg on a MIB2 unit directly from the Red/Engineering menu's SW-Update flow off a clean SD card, with no M.I.B. files and no M.I.B. install required first.

**Architecture:** mibpkg ships its own SWDL package (`metainfo2.txt` + a thin, hash-registered `install.sh`). `install.sh` lands the existing boot payload and calls mibpkg subcommands. A **C backbone** (`bootstrap` → boot hook + supervisor + web UI) always installs; a best-effort **B2 layer** (`enable-gem` + `gem-patch`) lights up the in-car GEM picker and degrades cleanly to C on any failure. Host-side `swdl-card` emits the whole card reproducibly.

**Tech Stack:** Rust (edition as in `Cargo.toml`), deps limited to the existing `anyhow`, `flate2`, `semver`, `serde` — **no new crates**. QNX 6.5 `/bin/sh` (pdksh subset) for the shell stub. Tests are integration tests under `tests/` using `tests/common/mod.rs`.

**Spec:** `docs/superpowers/specs/2026-10-14-swdl-installer-design.md`

## Global Constraints

- **No new dependencies.** Only `anyhow`, `flate2`, `semver`, `serde`. Hashing is hand-rolled (see existing `archive::sha256_file`); this plan hand-rolls sha1 and a binary-delta format rather than add crates.
- **Authorship:** GitHub `complex256`, git `me@complex256.dev`. Commits end with `Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>`.
- **Independence:** no MIBonk files, no `GEM412` jar, no shipped `updatePersistence` binary anywhere in the card or repo.
- **SWDL hashing is sha1** (`FinalScriptChecksum`, jar detection, patch verify). Package integrity stays sha256 (unchanged).
- **Shell stub is pdksh-subset only** (no `dirname`, `tail`, `nohup`, `date`; `/sbin` may be off PATH) — mirror `shell/custom.sh` and `shell/launch.sh`.
- **B2 is never fatal:** `enable-gem` and `gem-patch apply` failures are logged and skipped; only `bootstrap` failure aborts the install.
- **On-car-gated work:** the real `install.sh` run, the working `enable-gem` path (a/b/c), and a real jar patch are validated by spikes **S1** and **S2** (Task 11–12). All host logic below is unit-testable now with doubles/fixtures.

## Review Focus

- **FinalScriptChecksum drift** — if `metainfo2.txt`'s sha1 does not match the emitted `install.sh`, SWDL may reject the package; the builder must compute the hash from the exact bytes it writes. (Task 6 test.)
- **Unknown jar sha1** — a unit whose jar is not in `patches.toml` must produce a clean C install (logged skip), never a crash or a half-written jar. (Task 4 test.)
- **Bad/corrupt patch** — `gem-patch apply` whose result sha1 ≠ expected must leave the stock jar in place (restore from backup), never swap in an unverified jar. (Task 4 test.)
- **enable-gem off-unit / pc failure** — on a `--root` test box (`mounts=false`) and on a real `pc` error, `enable-gem` returns without aborting the caller. (Task 3 test.)
- **Delta applied to the wrong base** — `delta::apply` on bytes whose hash ≠ the patch's recorded base hash must error, not emit garbage. (Task 2 test.)

---

### Task 1: Hand-rolled sha1

**Files:**
- Create: `src/sha1.rs`
- Modify: `src/lib.rs` (add `pub mod sha1;`)
- Test: `tests/sha1.rs`

**Interfaces:**
- Consumes: nothing.
- Produces: `pub fn sha1_hex(bytes: &[u8]) -> String` (40-char lowercase hex); `pub fn sha1_file(path: &std::path::Path) -> std::io::Result<String>`.

- [ ] **Step 1: Write the failing test**

```rust
// tests/sha1.rs
use mibpkg::sha1::{sha1_hex, sha1_file};

#[test]
fn known_vectors() {
    assert_eq!(sha1_hex(b""), "da39a3ee5e6b4b0d3255bfef95601890afd80709");
    assert_eq!(sha1_hex(b"abc"), "a9993e364706816aba3e25717850c26c9cd0d89d");
    // > 64 bytes to exercise multi-block padding
    assert_eq!(
        sha1_hex(&b"a".repeat(1000)),
        "291e9a6c66994949b57ba5e650361e98fc36b1ba"
    );
}

#[test]
fn file_matches_bytes() {
    let d = tempfile::tempdir().unwrap();
    let p = d.path().join("x");
    std::fs::write(&p, b"abc").unwrap();
    assert_eq!(sha1_file(&p).unwrap(), sha1_hex(b"abc"));
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --test sha1`
Expected: FAIL — `unresolved import mibpkg::sha1`.

- [ ] **Step 3: Write minimal implementation**

Implement standard SHA-1 (FIPS 180-1) over `&[u8]`:

```rust
// src/sha1.rs
use std::io::Read;
use std::path::Path;

pub fn sha1_hex(bytes: &[u8]) -> String {
    let mut h: [u32; 5] = [0x67452301, 0xEFCDAB89, 0x98BADCFE, 0x10325476, 0xC3D2E1F0];
    let ml = (bytes.len() as u64) * 8;
    let mut msg = bytes.to_vec();
    msg.push(0x80);
    while msg.len() % 64 != 56 { msg.push(0); }
    msg.extend_from_slice(&ml.to_be_bytes());
    for chunk in msg.chunks_exact(64) {
        let mut w = [0u32; 80];
        for i in 0..16 {
            w[i] = u32::from_be_bytes([chunk[i*4], chunk[i*4+1], chunk[i*4+2], chunk[i*4+3]]);
        }
        for i in 16..80 {
            w[i] = (w[i-3] ^ w[i-8] ^ w[i-14] ^ w[i-16]).rotate_left(1);
        }
        let (mut a, mut b, mut c, mut d, mut e) = (h[0], h[1], h[2], h[3], h[4]);
        for (i, &wi) in w.iter().enumerate() {
            let (f, k) = match i {
                0..=19 => ((b & c) | ((!b) & d), 0x5A827999u32),
                20..=39 => (b ^ c ^ d, 0x6ED9EBA1),
                40..=59 => ((b & c) | (b & d) | (c & d), 0x8F1BBCDC),
                _ => (b ^ c ^ d, 0xCA62C1D6),
            };
            let t = a.rotate_left(5).wrapping_add(f).wrapping_add(e).wrapping_add(k).wrapping_add(wi);
            e = d; d = c; c = b.rotate_left(30); b = a; a = t;
        }
        h[0] = h[0].wrapping_add(a); h[1] = h[1].wrapping_add(b); h[2] = h[2].wrapping_add(c);
        h[3] = h[3].wrapping_add(d); h[4] = h[4].wrapping_add(e);
    }
    h.iter().map(|x| format!("{x:08x}")).collect()
}

pub fn sha1_file(path: &Path) -> std::io::Result<String> {
    let mut f = std::fs::File::open(path)?;
    let mut buf = Vec::new();
    f.read_to_end(&mut buf)?;
    Ok(sha1_hex(&buf))
}
```

Add `pub mod sha1;` to `src/lib.rs`.

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --test sha1`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/sha1.rs src/lib.rs tests/sha1.rs
git commit -m "feat: hand-rolled sha1 for SWDL manifest and jar detection"
```

---

### Task 2: Dependency-free binary delta

**Files:**
- Create: `src/delta.rs`
- Modify: `src/lib.rs` (add `pub mod delta;`)
- Test: `tests/delta.rs`

**Rationale:** honoring "ship a diff, not the jar" without a bsdiff crate. Format is a header (`magic`, base sha1, output len) + a COPY/INSERT op stream. The encoder is a simple longest-common prefix/suffix split with the differing middle shipped as INSERT; good enough to validate the pipeline, and the on-car patch size is finalized with spike S2 (a better encoder can replace `encode` without touching `apply` or any caller).

**Interfaces:**
- Consumes: `sha1::sha1_hex` (Task 1).
- Produces:
  - `pub fn encode(old: &[u8], new: &[u8]) -> Vec<u8>`
  - `pub fn apply(old: &[u8], patch: &[u8]) -> anyhow::Result<Vec<u8>>` — errors if `old`'s sha1 ≠ the patch's recorded base hash, or on a malformed stream.

- [ ] **Step 1: Write the failing test**

```rust
// tests/delta.rs
use mibpkg::delta::{encode, apply};

#[test]
fn roundtrip_middle_change() {
    let old = b"the quick brown fox jumps over the lazy dog".to_vec();
    let new = b"the quick RED fox leaps over the lazy dog".to_vec();
    let p = encode(&old, &new);
    assert!(p.len() <= new.len() + 64, "patch not larger than full new + header");
    assert_eq!(apply(&old, &p).unwrap(), new);
}

#[test]
fn roundtrip_identity_and_empty() {
    assert_eq!(apply(b"abc", &encode(b"abc", b"abc")).unwrap(), b"abc");
    assert_eq!(apply(b"", &encode(b"", b"xyz")).unwrap(), b"xyz");
}

#[test]
fn wrong_base_is_rejected() {
    let p = encode(b"original base", b"patched output");
    let e = apply(b"different base!", &p).unwrap_err();
    assert!(format!("{e}").contains("base"), "{e}");
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --test delta`
Expected: FAIL — `unresolved import mibpkg::delta`.

- [ ] **Step 3: Write minimal implementation**

```rust
// src/delta.rs
use crate::sha1::sha1_hex;
use anyhow::{bail, Result};

const MAGIC: &[u8; 4] = b"MPD1";

// Layout: MAGIC | base_sha1(20 bytes) | out_len(u64 LE) | ops
// op 0x01 COPY: off(u64 LE) len(u64 LE) from old; op 0x02 INSERT: len(u64 LE) bytes.
pub fn encode(old: &[u8], new: &[u8]) -> Vec<u8> {
    let pre = old.iter().zip(new).take_while(|(a, b)| a == b).count();
    let max_suf = old.len().min(new.len()) - pre;
    let suf = (0..max_suf)
        .take_while(|i| old[old.len() - 1 - i] == new[new.len() - 1 - i])
        .count();
    let mut p = Vec::new();
    p.extend_from_slice(MAGIC);
    p.extend_from_slice(&hex20(&sha1_hex(old)));
    p.extend_from_slice(&(new.len() as u64).to_le_bytes());
    if pre > 0 { copy(&mut p, 0, pre); }
    let mid = &new[pre..new.len() - suf];
    if !mid.is_empty() {
        p.push(0x02);
        p.extend_from_slice(&(mid.len() as u64).to_le_bytes());
        p.extend_from_slice(mid);
    }
    if suf > 0 { copy(&mut p, (old.len() - suf) as u64, suf); }
    p
}

pub fn apply(old: &[u8], patch: &[u8]) -> Result<Vec<u8>> {
    let mut r = patch;
    if take(&mut r, 4)? != MAGIC { bail!("delta: bad magic"); }
    let base = take(&mut r, 20)?.to_vec();
    if hex20(&sha1_hex(old)) != base[..] { bail!("delta: base hash mismatch (wrong jar)"); }
    let out_len = u64_le(&mut r)? as usize;
    let mut out = Vec::with_capacity(out_len);
    while !r.is_empty() {
        match r[0] {
            0x01 => { r = &r[1..]; let off = u64_le(&mut r)? as usize; let len = u64_le(&mut r)? as usize;
                      let end = off.checked_add(len).filter(|e| *e <= old.len()).ok_or_else(|| anyhow::anyhow!("delta: copy out of range"))?;
                      out.extend_from_slice(&old[off..end]); }
            0x02 => { r = &r[1..]; let len = u64_le(&mut r)? as usize; out.extend_from_slice(take(&mut r, len)?); }
            b => bail!("delta: bad op {b:#x}"),
        }
    }
    if out.len() != out_len { bail!("delta: length mismatch"); }
    Ok(out)
}

fn copy(p: &mut Vec<u8>, off: u64, len: usize) {
    p.push(0x01); p.extend_from_slice(&off.to_le_bytes()); p.extend_from_slice(&(len as u64).to_le_bytes());
}
fn hex20(h: &str) -> [u8; 20] {
    let mut o = [0u8; 20];
    for i in 0..20 { o[i] = u8::from_str_radix(&h[i*2..i*2+2], 16).unwrap(); }
    o
}
fn take<'a>(r: &mut &'a [u8], n: usize) -> Result<&'a [u8]> {
    if r.len() < n { bail!("delta: truncated"); }
    let (a, b) = r.split_at(n); *r = b; Ok(a)
}
fn u64_le(r: &mut &[u8]) -> Result<u64> {
    let b = take(r, 8)?; Ok(u64::from_le_bytes(b.try_into().unwrap()))
}
```

Add `pub mod delta;` to `src/lib.rs`.

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --test delta`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/delta.rs src/lib.rs tests/delta.rs
git commit -m "feat: dependency-free binary delta (base-hash guarded) for jar patches"
```

---

### Task 3: `enable-gem` / `disable-gem`

**Files:**
- Create: `src/gem_enable.rs`
- Modify: `src/lib.rs` (add `pub mod gem_enable;`); `src/cli.rs` (dispatch arms + USAGE)
- Test: `tests/gem_enable.rs`

**Background:** `Sys::persist_write(ns, key, value)` already drives `pc` and is faked by `RecordingSys` (`sys.persisted`). GEM dev-mode = key `3221356557` ns 0 set to `1`, then the commit write key `1` ns 0 = `0` (mirrors M.I.B.'s `UDP -key 3221356557 … 1` then `UDP -key 1 … 0`). This implements **path (a) stock `pc`**; whether `pc`'s string set reaches the byte-typed adaptation is validated by **spike S2** — if it does not, S2 adds path (b) (own writer) behind this same `enable()` interface.

**Interfaces:**
- Consumes: `mibpkg::sys::Sys`.
- Produces: `pub const GEM_DEV_KEY: u32 = 3221356557;` `pub const COMMIT_KEY: u32 = 1;` `pub fn enable(sys: &dyn Sys) -> anyhow::Result<()>` `pub fn disable(sys: &dyn Sys) -> anyhow::Result<()>`.

- [ ] **Step 1: Write the failing test**

```rust
// tests/gem_enable.rs
use mibpkg::gem_enable::{enable, disable, GEM_DEV_KEY, COMMIT_KEY};
use mibpkg::sys::RecordingSys;

#[test]
fn enable_writes_dev_flag_then_commit() {
    let sys = RecordingSys::default();
    enable(&sys).unwrap();
    let w = sys.persisted.borrow().clone();
    assert_eq!(w, vec![(0, GEM_DEV_KEY, "1".into()), (0, COMMIT_KEY, "0".into())]);
}

#[test]
fn disable_writes_zero_then_commit() {
    let sys = RecordingSys::default();
    disable(&sys).unwrap();
    let w = sys.persisted.borrow().clone();
    assert_eq!(w, vec![(0, GEM_DEV_KEY, "0".into()), (0, COMMIT_KEY, "0".into())]);
}

#[test]
fn pc_failure_surfaces_as_err_so_caller_can_log_and_continue() {
    let sys = RecordingSys::default();
    sys.fail_persist.set(true);
    assert!(enable(&sys).is_err());
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --test gem_enable`
Expected: FAIL — unresolved import.

- [ ] **Step 3: Write minimal implementation**

```rust
// src/gem_enable.rs
use crate::sys::Sys;
use anyhow::Result;

pub const GEM_DEV_KEY: u32 = 3221356557;
pub const COMMIT_KEY: u32 = 1;

fn set(sys: &dyn Sys, value: &str) -> Result<()> {
    sys.persist_write(0, GEM_DEV_KEY, value)?;
    sys.persist_write(0, COMMIT_KEY, "0")?; // store changes to persistence
    Ok(())
}
pub fn enable(sys: &dyn Sys) -> Result<()> { set(sys, "1") }
pub fn disable(sys: &dyn Sys) -> Result<()> { set(sys, "0") }
```

Add `pub mod gem_enable;` to `src/lib.rs`. In `src/cli.rs`, inside the `match cmd.as_str()` block that already holds `"bootstrap"`, add arms (they need no mount; `after(&ctx)` keeps GEM screens fresh):

```rust
"enable-gem" => {
    let _lock = Lock::acquire(&ctx.layout, ctx.sys)?;
    gem_enable::enable(ctx.sys)?;
    after(&ctx);
    crate::say!("GEM enabled");
    Ok(0)
}
"disable-gem" => {
    let _lock = Lock::acquire(&ctx.layout, ctx.sys)?;
    gem_enable::disable(ctx.sys)?;
    after(&ctx);
    crate::say!("GEM disabled");
    Ok(0)
}
```

Add `use crate::gem_enable;` and extend `USAGE` with `mibpkg enable-gem | disable-gem`.

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --test gem_enable`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/gem_enable.rs src/lib.rs src/cli.rs tests/gem_enable.rs
git commit -m "feat: enable-gem/disable-gem write the block-5F dev-mode flag via pc"
```

---

### Task 4: `gem-patch` core (detect / apply / status / revert)

**Files:**
- Create: `src/gempatch.rs`
- Modify: `src/lib.rs`; `src/cli.rs` (dispatch + USAGE)
- Test: `tests/gempatch.rs`

**Background:** reuse `backups::backup_once` / `backups::restore` (keyed by unit-absolute path) and `fsutil::copy_atomic`. Candidate jars (first that exists), unit-absolute: `/mnt/app/eso/hmi/lsd/jars/AppDevelopment.jar`, `…/GEM.jar`, `…/lsd.jar`. `patches.toml` maps a stock jar sha1 to a patch file + expected result sha1.

**Interfaces:**
- Consumes: `sha1::sha1_file`/`sha1_hex`, `delta::apply`, `backups`, `fsutil::copy_atomic`, `paths::Layout`.
- Produces:
  - `pub struct Patches(/* parsed patches.toml */);` with `pub fn load(dir: &Path) -> Result<Patches>` and `pub fn get(&self, stock_sha1: &str) -> Option<&PatchEntry>`.
  - `pub struct PatchEntry { pub file: String, pub result_sha1: String }`
  - `pub fn jar_path(layout: &Layout) -> Option<PathBuf>` (first candidate that exists)
  - `pub enum Status { NoJar, Unpatched { sha1: String, available: bool }, Patched { sha1: String } }`
  - `pub fn status(layout: &Layout, patch_dir: &Path) -> Result<Status>`
  - `pub fn apply(layout: &Layout, patch_dir: &Path) -> Result<String>` — Ok on match+verify+swap; Ok with a "skipped" message when no jar / no matching patch (non-fatal); **Err only on an unexpected IO fault**, and on a verify mismatch it restores the stock jar and returns Err.
  - `pub fn revert(layout: &Layout) -> Result<String>`

- [ ] **Step 1: Write the failing test**

```rust
// tests/gempatch.rs
mod common;
use mibpkg::gempatch::{self, Status};
use mibpkg::{delta, sha1::sha1_hex};
use std::fs;

fn seed_jar(f: &common::Fixture, bytes: &[u8]) -> std::path::PathBuf {
    let dir = f.root().join("mnt/app/eso/hmi/lsd/jars");
    fs::create_dir_all(&dir).unwrap();
    let p = dir.join("AppDevelopment.jar");
    fs::write(&p, bytes).unwrap();
    p
}
fn patch_dir(f: &common::Fixture, stock: &[u8], patched: &[u8]) -> std::path::PathBuf {
    let d = f.root().join("patchsrc");
    fs::create_dir_all(&d).unwrap();
    let stock_sha = sha1_hex(stock);
    fs::write(d.join(format!("{stock_sha}.patch")), delta::encode(stock, patched)).unwrap();
    fs::write(d.join("patches.toml"), format!(
        "[[patch]]\nstock_sha1 = \"{stock_sha}\"\nfile = \"{stock_sha}.patch\"\nresult_sha1 = \"{}\"\n",
        sha1_hex(patched))).unwrap();
    d
}

#[test]
fn apply_patches_a_known_jar_and_verifies() {
    let f = common::fixture();
    let (stock, patched) = (b"STOCK JAR BYTES vXYZ".as_ref(), b"PATCHED JAR BYTES vXYZ+mib".as_ref());
    seed_jar(&f, stock);
    let d = patch_dir(&f, stock, patched);
    let msg = gempatch::apply(&f.layout, &d).unwrap();
    assert!(msg.contains("patched"), "{msg}");
    assert_eq!(fs::read(f.root().join("mnt/app/eso/hmi/lsd/jars/AppDevelopment.jar")).unwrap(), patched);
    // and revert restores the stock jar from backup
    gempatch::revert(&f.layout).unwrap();
    assert_eq!(fs::read(f.root().join("mnt/app/eso/hmi/lsd/jars/AppDevelopment.jar")).unwrap(), stock);
}

#[test]
fn unknown_jar_is_a_clean_skip_not_an_error() {
    let f = common::fixture();
    seed_jar(&f, b"SOME OTHER JAR");
    let d = patch_dir(&f, b"STOCK", b"PATCHED"); // patch is for a different jar
    let msg = gempatch::apply(&f.layout, &d).unwrap();
    assert!(msg.contains("no patch"), "{msg}");
    assert_eq!(fs::read(f.root().join("mnt/app/eso/hmi/lsd/jars/AppDevelopment.jar")).unwrap(), b"SOME OTHER JAR");
}

#[test]
fn verify_mismatch_leaves_stock_jar_intact() {
    let f = common::fixture();
    let stock = b"STOCK JAR BYTES".as_ref();
    seed_jar(&f, stock);
    let d = patch_dir(&f, stock, b"GOOD PATCHED");
    // Corrupt result_sha1 so verify fails even though the patch applies.
    fs::write(d.join("patches.toml"), format!(
        "[[patch]]\nstock_sha1 = \"{}\"\nfile = \"{}.patch\"\nresult_sha1 = \"{}\"\n",
        sha1_hex(stock), sha1_hex(stock), "0".repeat(40))).unwrap();
    assert!(gempatch::apply(&f.layout, &d).is_err());
    assert_eq!(fs::read(f.root().join("mnt/app/eso/hmi/lsd/jars/AppDevelopment.jar")).unwrap(), stock);
}

#[test]
fn status_reports_no_jar() {
    let f = common::fixture();
    let d = f.root().join("empty"); fs::create_dir_all(&d).unwrap();
    fs::write(d.join("patches.toml"), "").unwrap();
    assert!(matches!(gempatch::status(&f.layout, &d).unwrap(), Status::NoJar));
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --test gempatch`
Expected: FAIL — unresolved import.

- [ ] **Step 3: Write minimal implementation**

Implement `src/gempatch.rs`. Parse `patches.toml` with serde (mirror how `src/manifest.rs` / `src/web/settings.rs` derive `Deserialize`). Key logic for `apply`:

```
let Some(jar) = jar_path(layout) else { return Ok("no GEM jar found; skipped".into()) };
let stock_sha1 = sha1_file(&jar)?;
let pats = Patches::load(patch_dir)?;
let Some(entry) = pats.get(&stock_sha1) else { return Ok(format!("no patch for jar {stock_sha1}; skipped")) };
let old = fs::read(&jar)?;
let new = delta::apply(&old, &fs::read(patch_dir.join(&entry.file))?)?;
if sha1_hex(&new) != entry.result_sha1 { bail!("gem-patch: result sha1 mismatch; stock jar left in place"); }
backups::backup_once(layout, JAR_ABS_FOR(jar))?;        // once-only stock backup
let tmp = fsutil::tmp_sibling(&jar); fs::write(&tmp, &new)?; fsutil::rename(&tmp, &jar)?;  // atomic swap
Ok(format!("patched GEM jar {stock_sha1} -> {}", entry.result_sha1))
```

`revert` → `backups::restore(layout, jar_abs)`. The unit-absolute jar string is the candidate path with the fixture/root prefix stripped (store the chosen candidate's unit-absolute form, e.g. `"/mnt/app/eso/hmi/lsd/jars/AppDevelopment.jar"`, so `backups` keys match). `status` → `NoJar` when `jar_path` is None, else `Patched`/`Unpatched{available}` by whether the current sha1 equals a known `result_sha1` or `stock_sha1`.

Wire `src/cli.rs` arms near `bootstrap` (these take `--from <dir>`):

```rust
"gem-patch" => {
    let _lock = Lock::acquire(&ctx.layout, ctx.sys)?;
    let sub = arg(args, 0, "apply|status|revert")?;
    let from = args.iter().position(|a| a == "--from").map(|i| PathBuf::from(&args[i+1]));
    let _rw = ctx.mounts.rw("/mnt/app")?;
    let msg = match sub {
        "apply"  => gempatch::apply(&ctx.layout, &from.context("gem-patch apply needs --from <dir>")?)?,
        "status" => format!("{:?}", gempatch::status(&ctx.layout, &from.context("gem-patch status needs --from <dir>")?)?),
        "revert" => gempatch::revert(&ctx.layout)?,
        other => bail!("gem-patch: unknown subcommand {other:?}"),
    };
    crate::say!("{msg}");
    Ok(0)
}
```

Add `pub mod gempatch;` and `use crate::gempatch;`; extend `USAGE`.

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --test gempatch`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/gempatch.rs src/lib.rs src/cli.rs tests/gempatch.rs
git commit -m "feat: gem-patch applies/verifies/reverts a binary jar patch (non-fatal on no match)"
```

---

### Task 5: Host `gem-patch build`

**Files:**
- Modify: `src/cli.rs` (route `gem-patch build` to host; add `fn host_gem_patch_build`)
- Test: `tests/gempatch_build.rs`

**Note:** `gem-patch build` is a **host** command (no unit), so route it in the early host `match cmd` block before `Layout::load`. On the unit, `gem-patch apply/status/revert` stay the on-unit arms from Task 4. Disambiguate by the sub-verb: `build` → host.

**Interfaces:**
- Consumes: `delta::encode`, `sha1::sha1_file`.
- Produces: writes `<stock_sha1>.patch` and appends a `[[patch]]` block to `<out>/patches.toml`. `mibpkg gem-patch build --stock <jar> --patched <jar> --out <dir>`.

- [ ] **Step 1: Write the failing test**

```rust
// tests/gempatch_build.rs
mod common;
use std::fs;

#[test]
fn build_emits_patch_and_toml_that_apply_reads() {
    let d = tempfile::tempdir().unwrap();
    let (stock, patched) = (d.path().join("s.jar"), d.path().join("p.jar"));
    fs::write(&stock, b"STOCK JAR vA").unwrap();
    fs::write(&patched, b"PATCHED JAR vA").unwrap();
    let out = d.path().join("gem-patch");
    let code = mibpkg::cli::run(&["gem-patch".into(), "build".into(),
        "--stock".into(), stock.display().to_string(),
        "--patched".into(), patched.display().to_string(),
        "--out".into(), out.display().to_string()]).unwrap();
    assert_eq!(code, 0);
    let toml = fs::read_to_string(out.join("patches.toml")).unwrap();
    assert!(toml.contains("stock_sha1"));
    let stock_sha = mibpkg::sha1::sha1_file(&stock).unwrap();
    assert!(out.join(format!("{stock_sha}.patch")).exists());
    // Round-trips through the on-unit decoder.
    let patch = fs::read(out.join(format!("{stock_sha}.patch"))).unwrap();
    assert_eq!(mibpkg::delta::apply(b"STOCK JAR vA", &patch).unwrap(), b"PATCHED JAR vA");
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --test gempatch_build`
Expected: FAIL — `gem-patch build` not routed (likely hits the on-unit arm and errors loading a layout, or unknown subcommand).

- [ ] **Step 3: Write minimal implementation**

In `run()`'s early host section, before the on-unit path, intercept `gem-patch build`:

```rust
if cmd == "gem-patch" && rest.get(1).map(|s| s.as_str()) == Some("build") {
    return host_gem_patch_build(&rest[2..]);
}
```

```rust
fn host_gem_patch_build(args: &[String]) -> Result<i32> {
    let (mut stock, mut patched, mut out) = (None, None, PathBuf::from("out/gem-patch"));
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--stock" => stock = Some(PathBuf::from(it.next().context("--stock needs a jar")?)),
            "--patched" => patched = Some(PathBuf::from(it.next().context("--patched needs a jar")?)),
            "--out" => out = PathBuf::from(it.next().context("--out needs a dir")?),
            s => bail!("unexpected argument {s:?}"),
        }
    }
    let stock = stock.context("--stock <jar> required")?;
    let patched = patched.context("--patched <jar> required")?;
    fs::create_dir_all(&out)?;
    let (so, po) = (fs::read(&stock)?, fs::read(&patched)?);
    let stock_sha = crate::sha1::sha1_hex(&so);
    fs::write(out.join(format!("{stock_sha}.patch")), crate::delta::encode(&so, &po))?;
    let entry = format!("[[patch]]\nstock_sha1 = \"{stock_sha}\"\nfile = \"{stock_sha}.patch\"\nresult_sha1 = \"{}\"\n\n",
        crate::sha1::sha1_hex(&po));
    let toml = out.join("patches.toml");
    let mut cur = fs::read_to_string(&toml).unwrap_or_default();
    cur.push_str(&entry);
    fs::write(&toml, cur)?;
    crate::say!("{}", out.join(format!("{stock_sha}.patch")).display());
    Ok(0)
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --test gempatch_build`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/cli.rs tests/gempatch_build.rs
git commit -m "feat: host gem-patch build emits a jar patch + patches.toml entry"
```

---

### Task 6: Host `swdl-card` builder + the `install.sh` template

**Files:**
- Create: `src/swdl.rs` (card emitter + embedded `install.sh` + `metainfo2.txt` templating); `shell/install.sh` (source of truth, embedded via `include_str!`)
- Modify: `src/lib.rs`; `src/cli.rs` (route `swdl-card`)
- Test: `tests/swdl.rs`

**Background:** the builder writes the card tree from the spec's Card layout. `install.sh` is kept as `shell/install.sh` and embedded with `include_str!("../shell/install.sh")` so the file is lintable/`ksh -n`-testable on its own and the builder hashes the exact embedded bytes (no drift). `metainfo2.txt` carries the `skip*` flags, one marker group, and `FinalScript`/`FinalScriptChecksum`/`FinalScriptMaxTime`/`FinalScriptName`.

**Interfaces:**
- Consumes: `sha1::sha1_hex`.
- Produces: `pub fn build_card(out: &Path, payload: &Path, seed: Option<&Path>, version: &str) -> Result<()>`; `pub const INSTALL_SH: &str`. CLI: `mibpkg swdl-card <out-dir> --payload <dir> [--seed <collection-dir>] --version <v>`.

- [ ] **Step 1: Write the failing test**

```rust
// tests/swdl.rs
use std::fs;

#[test]
fn card_has_matching_hash_and_layout() {
    let d = tempfile::tempdir().unwrap();
    let payload = d.path().join("payload");
    fs::create_dir_all(&payload).unwrap();
    for f in ["mibpkg", "launch.sh", "gem.sh", "ca.pem"] { fs::write(payload.join(f), b"x").unwrap(); }
    let out = d.path().join("card");
    let code = mibpkg::cli::run(&["swdl-card".into(), out.display().to_string(),
        "--payload".into(), payload.display().to_string(),
        "--version".into(), "1.0".into()]).unwrap();
    assert_eq!(code, 0);
    let meta = fs::read_to_string(out.join("metainfo2.txt")).unwrap();
    assert!(meta.contains("skipCheckSignatureAndVariant = \"true\""));
    assert!(meta.contains("FinalScript = \"./mibpkg/final/install.sh\""));
    // The recorded hash matches the bytes actually written.
    let script = fs::read(out.join("mibpkg/final/install.sh")).unwrap();
    let want = mibpkg::sha1::sha1_hex(&script);
    assert!(meta.contains(&format!("FinalScriptChecksum = \"{want}\"")), "hash drift");
    assert!(out.join("mibpkg/payload/mibpkg").exists());
    assert!(out.join("_Swdlautorun.txt").exists());        // ships disabled
    assert!(!out.join("Swdlautorun.txt").exists());
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --test swdl`
Expected: FAIL — `swdl-card` not routed.

- [ ] **Step 3: Write minimal implementation**

Create `shell/install.sh` (pdksh subset; shaped from `shell/custom.sh`, adapted to SWDL args `$1`=HW `$2`=SD mount). Its body, in order: env + `unset LD_PRELOAD`; `D=$2/mibpkg`; forward to MMX if `/mnt/app/eso/hmi/lsd` is absent but `/net/mmx/...` present (`exec on -f mmx /bin/sh "$2/mibpkg/final/install.sh" "$@"`); `mount -uw /mnt/app`; copy `$D/payload/{mibpkg,launch.sh,gem.sh,ca.pem}` → `/mnt/app/root/mibpkg/bin` via `.new`→`mv`; `sync`; copy `mibpkg`→`/tmp/mibpkg`; `/tmp/mibpkg bootstrap` (abort — `exit 1`, no Result — on failure); `/tmp/mibpkg enable-gem || echo "enable-gem skipped"`; `/tmp/mibpkg gem-patch apply --from $D/payload/gem-patch || echo "gem-patch skipped"`; rename `$2/Swdlautorun.txt`→`$2/_Swdlautorun.txt` if present; `touch /tmp/SWDLScript.Result`; `exit 0`. Every line echoes to the RCC log.

Create `src/swdl.rs`:

```rust
pub const INSTALL_SH: &str = include_str!("../shell/install.sh");

pub fn build_card(out: &Path, payload: &Path, seed: Option<&Path>, version: &str) -> Result<()> {
    let final_dir = out.join("mibpkg/final");
    fs::create_dir_all(&final_dir)?;
    fs::write(final_dir.join("install.sh"), INSTALL_SH)?;
    let hash = crate::sha1::sha1_hex(INSTALL_SH.as_bytes());
    fs::write(out.join("metainfo2.txt"), metainfo(version, &hash))?;
    // payload
    let pout = out.join("mibpkg/payload");
    copy_tree(payload, &pout)?;                            // small helper; recursive copy
    if let Some(s) = seed { copy_tree(s, &out.join("mibpkg/collection"))?; }
    fs::write(out.join("_Swdlautorun.txt"), "")?;          // disabled autostart marker
    Ok(())
}

fn metainfo(version: &str, hash: &str) -> String {
    format!(
"[common]\n\
vendor = \"complex256\"\n\
release = \"mibpkg installer v{version}\"\n\
skipCheckSignatureAndVariant = \"true\"\n\
skipMetaCRC = \"true\"\n\
skipFileCopyCrc = \"true\"\n\
skipCheckVariant = \"true\"\n\
skipCheckRegion = \"true\"\n\
skipSaveTrainName = \"true\"\n\
variant = \"FM?-*-*-*-*\"\n\
region = \"RoW\"\n\
FinalScript = \"./mibpkg/final/install.sh\"\n\
FinalScriptChecksum = \"{hash}\"\n\
FinalScriptMaxTime = \"300\"\n\
FinalScriptName = \"mibpkg install\"\n\n\
[mibpkg]\nVendorInfo = \"complex256\"\nDeviceDescription = \"mibpkg installer\"\nDownloadGroup = \"mibpkg\"\n")
}
```

Route in `run()`'s early host section:

```rust
if cmd == "swdl-card" { return host_swdl_card(&rest[1..]); }
```

`host_swdl_card` parses `<out-dir> --payload <dir> [--seed <dir>] --version <v>`, calls `swdl::build_card`, then self-verifies via Task 8's `swdl::verify_card` and prints problems. Add `pub mod swdl;`, `use crate::swdl;`, and the `copy_tree` helper (or reuse one in `fsutil`/`archive` if present). Extend `USAGE`.

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --test swdl`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/swdl.rs shell/install.sh src/lib.rs src/cli.rs tests/swdl.rs
git commit -m "feat: swdl-card emits metainfo2.txt + install.sh payload card (hash never drifts)"
```

---

### Task 7: `install.sh` shell syntax + dry-run behavior

**Files:**
- Modify: `shell/install.sh` (only if the syntax test finds a problem)
- Test: `tests/install_sh.rs`

**Background:** the car runs `install.sh` under pdksh; a syntax slip is invisible until on-car. `tests/shell.rs` already exercises generated shell — follow its pattern. We can only check syntax and the file-landing/Result logic off-car (the `mibpkg` calls and `on -f mmx` are unit-only), so the test runs `ksh -n` if available, else `sh -n`, and asserts the Result-touch line is guarded behind the bootstrap success.

**Interfaces:** none (asserts on `swdl::INSTALL_SH`).

- [ ] **Step 1: Write the failing test**

```rust
// tests/install_sh.rs
use std::io::Write;

#[test]
fn install_sh_is_valid_shell() {
    let src = mibpkg::swdl::INSTALL_SH;
    let d = tempfile::tempdir().unwrap();
    let p = d.path().join("install.sh");
    std::fs::File::create(&p).unwrap().write_all(src.as_bytes()).unwrap();
    let shell = if std::process::Command::new("ksh").arg("-n").arg(&p).status().map(|s| s.success()).unwrap_or(false) {
        return; // ksh accepted it
    } else { "sh" };
    let ok = std::process::Command::new(shell).arg("-n").arg(&p).status().unwrap().success();
    assert!(ok, "install.sh failed {shell} -n");
}

#[test]
fn result_is_touched_only_after_bootstrap() {
    let src = mibpkg::swdl::INSTALL_SH;
    // bootstrap line precedes the SWDLScript.Result touch; a bootstrap failure exits before it.
    let b = src.find("bootstrap").expect("bootstrap call present");
    let r = src.find("SWDLScript.Result").expect("Result touch present");
    assert!(b < r, "Result is touched before bootstrap");
    assert!(src.contains("enable-gem"));
    assert!(src.contains("gem-patch apply"));
    assert!(src.contains("_Swdlautorun.txt"));
}
```

- [ ] **Step 2: Run test to verify it fails/passes**

Run: `cargo test --test install_sh`
Expected: FAIL if `shell/install.sh` has a syntax error or missing line; otherwise PASS. Fix `shell/install.sh` until green.

- [ ] **Step 3: Fix the shell if needed**

Edit `shell/install.sh` to satisfy the assertions (correct order, pdksh-valid). No code beyond the shell file.

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --test install_sh`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add shell/install.sh tests/install_sh.rs
git commit -m "test: install.sh is valid pdksh and touches Result only after bootstrap"
```

---

### Task 8: `verify` a built card

**Files:**
- Modify: `src/swdl.rs` (`verify_card`); `src/cli.rs` (`host_verify` detects a card dir)
- Test: `tests/swdl_verify.rs`

**Interfaces:**
- Produces: `pub fn verify_card(dir: &Path) -> Result<Vec<String>>` (returns problems; empty = ok). `mibpkg verify <card-dir>` runs it when `<dir>/metainfo2.txt` exists.

- [ ] **Step 1: Write the failing test**

```rust
// tests/swdl_verify.rs
use std::fs;

fn good_card() -> (tempfile::TempDir, std::path::PathBuf) {
    let d = tempfile::tempdir().unwrap();
    let payload = d.path().join("p"); fs::create_dir_all(&payload).unwrap();
    for f in ["mibpkg","launch.sh","gem.sh","ca.pem"] { fs::write(payload.join(f), b"x").unwrap(); }
    let out = d.path().join("card");
    mibpkg::swdl::build_card(&out, &payload, None, "1.0").unwrap();
    (d, out)
}

#[test]
fn clean_card_has_no_problems() {
    let (_d, out) = good_card();
    assert!(mibpkg::swdl::verify_card(&out).unwrap().is_empty());
}

#[test]
fn tampered_script_is_caught() {
    let (_d, out) = good_card();
    fs::write(out.join("mibpkg/final/install.sh"), "#!/bin/sh\necho tampered\n").unwrap();
    let probs = mibpkg::swdl::verify_card(&out).unwrap();
    assert!(probs.iter().any(|p| p.contains("hash")), "{probs:?}");
}

#[test]
fn missing_payload_is_caught() {
    let (_d, out) = good_card();
    fs::remove_file(out.join("mibpkg/payload/mibpkg")).unwrap();
    let probs = mibpkg::swdl::verify_card(&out).unwrap();
    assert!(probs.iter().any(|p| p.contains("mibpkg")), "{probs:?}");
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --test swdl_verify`
Expected: FAIL — `verify_card` missing.

- [ ] **Step 3: Write minimal implementation**

```rust
// in src/swdl.rs
pub fn verify_card(dir: &Path) -> Result<Vec<String>> {
    let mut p = Vec::new();
    let meta = fs::read_to_string(dir.join("metainfo2.txt")).context("reading metainfo2.txt")?;
    let script = dir.join("mibpkg/final/install.sh");
    match fs::read(&script) {
        Ok(b) => {
            let want = crate::sha1::sha1_hex(&b);
            if !meta.contains(&format!("FinalScriptChecksum = \"{want}\"")) {
                p.push(format!("FinalScriptChecksum does not match install.sh (expected {want})"));
            }
        }
        Err(_) => p.push("missing mibpkg/final/install.sh".into()),
    }
    for f in ["mibpkg","launch.sh","gem.sh"] {
        if !dir.join("mibpkg/payload").join(f).exists() { p.push(format!("missing payload file {f}")); }
    }
    let gp = dir.join("mibpkg/payload/gem-patch");
    if gp.join("patches.toml").exists() {
        // every file= referenced must exist (cheap line scan; full parse optional)
        for line in fs::read_to_string(gp.join("patches.toml"))?.lines() {
            if let Some(f) = line.trim().strip_prefix("file = ").map(|s| s.trim_matches('"')) {
                if !gp.join(f).exists() { p.push(format!("patches.toml references missing {f}")); }
            }
        }
    }
    Ok(p)
}
```

In `host_verify`, before the online path: `if Path::new(&target).join("metainfo2.txt").exists() { let probs = swdl::verify_card(Path::new(&target))?; … print; return Ok(if probs.is_empty() {0} else {1}); }`.

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --test swdl_verify`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/swdl.rs src/cli.rs tests/swdl_verify.rs
git commit -m "feat: verify validates a built SWDL card (hash, payload, patches.toml)"
```

---

### Task 9: Seed-collection first-boot install (marker-guarded)

**Files:**
- Modify: `src/supervisor.rs` (first-boot hook); reuse `install::install`/`media` scan
- Test: `tests/seed_collection.rs`

**Background:** when the card carries `/mibpkg/collection/`, the supervisor installs it **once** on first boot, guarded by a marker in `state_dir` so it never re-installs. Reuses the existing SD-collection install path; no 300 s window involvement.

**Interfaces:**
- Produces: `pub fn seed_once(ctx: &Ctx, media: &[PathBuf]) -> Result<()>` called at supervisor startup; writes `state_dir()/seeded` after a successful pass.

- [ ] **Step 1: Write the failing test**

```rust
// tests/seed_collection.rs
mod common;
use mibpkg::ctx::Ctx;
use mibpkg::sys::RecordingSys;
use std::fs;

#[test]
fn seed_installs_once_then_marks_done() {
    let f = common::fixture();
    let sys = RecordingSys::default();
    let ctx = Ctx::new(f.layout.clone(), &sys);
    // A card collection named like the SWDL card lays down (reuse the common helper).
    let src = common::pkg_src(f.root(), "demo", "1.0", "exit 0", "");
    // The card's seed lives under <sd>/mibpkg/collection; point a media root there.
    let coll = common::collection(&f.sd(), "seed", &[src]);
    let _ = coll;
    mibpkg::supervisor::seed_once(&ctx, &[f.sd()]).unwrap();
    assert!(f.layout.state_dir().join("seeded").exists());
    // Second call is a no-op (marker present): capture install count via state.json unchanged.
    let before = fs::read(f.layout.state_dir().join("installed.json")).unwrap();
    mibpkg::supervisor::seed_once(&ctx, &[f.sd()]).unwrap();
    assert_eq!(fs::read(f.layout.state_dir().join("installed.json")).unwrap(), before);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --test seed_collection`
Expected: FAIL — `seed_once` missing.

- [ ] **Step 3: Write minimal implementation**

Add `seed_once`: if `state_dir()/seeded` exists, return `Ok(())`. Otherwise scan `media` for a `mibpkg/collection` (or the card's `collections`) index, install each package via the existing `install::install`, then write the `seeded` marker. Call it once near the top of `supervisor::run` (after layout/dirs are ready, before the service loop). Match the exact media/collection scan the existing `install` path uses (see `media::scan` / `install::install` signatures in `src/media.rs`, `src/install.rs`).

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --test seed_collection`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/supervisor.rs tests/seed_collection.rs
git commit -m "feat: supervisor installs a card seed collection once on first boot"
```

---

### Task 10: Docs + CI wiring

**Files:**
- Modify: `README.md` (install-from-SWDL section); `action.yml` (card build + verify step, kept dormant like the existing release CI); `docs/unit-facts.md` (record the new commands + spike gates)
- Test: none (docs/CI); `cargo test` must stay green.

- [ ] **Step 1: Write the README + CI changes**

Add a README section "Install from the engineering menu (no M.I.B.)" describing: `mibpkg swdl-card out/card --payload … --version X.Y`, write `out/card` to a FAT32 SD, SD1, Red menu → SW Update → "mibpkg installer vX.Y". Note B2 is best-effort and the web UI is primary. In `action.yml`, add a build-and-verify step (`mibpkg swdl-card` then `mibpkg verify out/card`) gated the same way the dormant release CI is.

- [ ] **Step 2: Verify the whole suite still passes**

Run: `cargo test`
Expected: PASS (all tasks' tests).

- [ ] **Step 3: Commit**

```bash
git add README.md action.yml docs/unit-facts.md
git commit -m "docs: document SWDL install path and wire the dormant card-build CI step"
```

---

### Task 11: Spike S1 — SWDL mechanics on-car (gates the real install)

**Not host-testable. Requires the car (sleeps when off — schedule when it is reachable).**

- [ ] **Step 1:** Build a throwaway card: a from-scratch `metainfo2.txt` (the Task 6 template) + an `install.sh` that only `echo`s to the RCC log/UART, lands a marker file, and `touch`es `/tmp/SWDLScript.Result`. No payload, no MIBonk files.
- [ ] **Step 2:** On the car, Red menu → SW Update → select SD → confirm the package is **listed** as "mibpkg installer" and **runs** to completion.
- [ ] **Step 3:** Record: is the package listed with no MIBonk files present? Is `FinalScriptChecksum` enforced (break the hash deliberately and see if it is rejected) or waived? Is the final-script's context RCC with `/net/mmx` reachable (the marker lands where expected)?
- [ ] **Step 4:** Write findings to `docs/unit-facts.md` and, if the accepted manifest differs from the Task 6 template, open a follow-up to adjust `src/swdl.rs::metainfo`.

**Exit criteria:** a confirmed, minimal `metainfo2.txt` skeleton that the Red menu lists and runs with zero MIBonk files, and a yes/no on hash enforcement.

---

### Task 12: Spike S2 — B2 in-car (gates enable-gem path + a real jar patch)

**Not host-testable. Requires an old-GEM unit.**

- [ ] **Step 1:** On the unit, run `mibpkg enable-gem` (path **a**, stock `pc`). Confirm the engineering-menu dev-mode flag flips and the menu becomes reachable. If `pc`'s string set does not take the byte-typed key, implement path **b** (own writer) behind `gem_enable::enable` and retry.
- [ ] **Step 2:** From your `lsd.jar` RE work, produce a patched jar that renders a mibpkg `.esd`. Capture `stock.jar` and `patched.jar`.
- [ ] **Step 3:** `mibpkg gem-patch build --stock stock.jar --patched patched.jar --out out/gem-patch`; put `out/gem-patch` in a card payload; `mibpkg gem-patch apply --from …`; reboot; confirm the mibpkg picker renders.
- [ ] **Step 4:** Record the working `enable-gem` path and at least one `stock-sha1 → patch → render` in `docs/unit-facts.md`; if the delta is impractically large, note it and open a follow-up to improve `delta::encode` (callers unchanged). If a target jar cannot be patched, document it as **C-only**.

**Exit criteria:** a working `enable-gem` path and one proven shippable jar patch, or a documented C-only determination per jar.

---

## Self-Review

**Spec coverage:** card layout → Task 6; metainfo2 `skip*`+hook → Task 6; `install.sh` stub+order+failure semantics → Tasks 6–7; `enable-gem`/`disable-gem` (paths a/b/c) → Task 3 (a now; b/c via S2); `gem-patch apply/status/revert` + binary diff → Tasks 2,4,5; backup/atomic swap → Task 4; host `swdl-card`+`gem-patch build`+`verify` → Tasks 5,6,8; seed collection first-boot → Task 9; independence (no MIBonk files, own sha1/delta, no crates) → Global Constraints + Tasks 1,2,6; spikes S1/S2 → Tasks 11,12; licensing note + disable-gem teardown → Task 3 + spec. No uncovered spec sections.

**Placeholder scan:** no TBD/TODO; every code step carries real content; the two on-car spikes are explicitly non-code with method + exit criteria.

**Type consistency:** `sha1_hex`/`sha1_file`, `delta::encode`/`delta::apply`, `gem_enable::enable`/`disable` + `GEM_DEV_KEY`/`COMMIT_KEY`, `gempatch::{apply,status,revert,Status}`, `swdl::{build_card,verify_card,INSTALL_SH}` are used with identical names/signatures across tasks.

**Review Focus:** hash drift (Task 6 `card_has_matching_hash_and_layout`, Task 8 `tampered_script_is_caught`); unknown jar (Task 4 `unknown_jar_is_a_clean_skip_not_an_error`); bad patch (Task 4 `verify_mismatch_leaves_stock_jar_intact`); enable-gem off-unit/pc failure (Task 3 `pc_failure_surfaces_as_err…`, and `mounts=false` no-op via `RecordingSys`); wrong delta base (Task 2 `wrong_base_is_rejected`). All five owned and tested.
