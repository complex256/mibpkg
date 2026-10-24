//! The `[sdk]` package (`mibsdk`): the install guards, the move of an old `mibsdk-dev` setup's
//! state, the all-or-undo deploy and the guarded uninstall. A port of `mibsdk-dev`'s
//! `cmd_install` / `cmd_uninstall` (`check_owned_classes`, `check_hmicar`, `migrate_state`,
//! `migrate_ledger`, `check_ledger`, `restore_bundles_mode`), writing through `put`.
use super::{bundles, ledger, stamp, zipnames};
use super::{installed_sdk, put, put_file, put_with, reboot_flag, FileOps, StdOps, SDK_PKG};
use super::{BASE, BUNDLES, HMI_ROOT, JARS, LEDGER, MARKER, OLD_LEDGER, PERMISSIONS, PERSIST, RAM, TRACER_JAR};
use crate::backups;
use crate::ctx::Ctx;
use crate::install;
use crate::manifest::{Manifest, SdkSpec};
use crate::paths::Layout;
use crate::state::State;
use anyhow::{anyhow, bail, Context, Result};
use std::ffi::OsString;
use std::fs;
use std::io;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

/// Where `mibsdk-dev` kept its stock backup (`tracer.was` is written last: present = complete).
fn stock_dir() -> String {
    format!("{BASE}/stock")
}

/// What `mibsdk-dev` assumed for a backup without `bundles.mode`: every unit seen has 0777.
const STOCK_BUNDLES_MODE: u32 = 0o777;
const LOST: &str = "(nothing was lost; run the install again)";

/// Installed `[plugin]` packages and their plugin api, by id.
pub fn plugin_packages(layout: &Layout) -> Result<Vec<(String, u32)>> {
    let st = State::load(layout)?;
    Ok(st
        .packages
        .keys()
        .filter_map(|n| install::current_manifest(layout, n).ok().and_then(|m| m.plugin).map(|p| (n.clone(), p.api)))
        .collect())
}

/// Sorted names in the on-unit directory `abs`.
fn names(layout: &Layout, abs: &str) -> io::Result<Vec<String>> {
    let mut v: Vec<String> =
        fs::read_dir(layout.p(abs))?.map(|e| e.map(|e| e.file_name().to_string_lossy().into_owned())).collect::<io::Result<_>>()?;
    v.sort();
    Ok(v)
}

/// The first `*.carplay-stock` name in the tree under `dir` (as `ls -R` lists it: symlinked
/// directories are not followed). A directory that cannot be read is an error.
fn find_carplay_stock(dir: &Path) -> std::result::Result<Option<PathBuf>, (PathBuf, io::Error)> {
    let rd = fs::read_dir(dir).map_err(|e| (dir.to_path_buf(), e))?;
    let mut entries: Vec<fs::DirEntry> = rd.collect::<io::Result<_>>().map_err(|e| (dir.to_path_buf(), e))?;
    entries.sort_by_key(|e| e.file_name());
    for e in entries {
        let path = e.path();
        if e.file_name().to_string_lossy().ends_with(".carplay-stock") {
            return Ok(Some(path));
        }
        if e.file_type().map_err(|err| (path.clone(), err))?.is_dir() {
            if let Some(hit) = find_carplay_stock(&path)? {
                return Ok(Some(hit));
            }
        }
    }
    Ok(None)
}

fn hmicar_lines(sdk: &SdkSpec) -> (String, String) {
    (format!("HMICar={}", sdk.hmicar_stock), format!("HMICar={}", sdk.hmicar))
}

/// The live bundle list references the SDK: it names mibsdk or (with the spec) the SDK bundle,
/// or its `HMICar=` is not the stock line.
fn references_sdk(live: &[u8], spec: Option<&SdkSpec>) -> bool {
    let has = |needle: &[u8]| live.windows(needle.len()).any(|w| w == needle);
    has(b"mibsdk")
        || spec.is_some_and(|spec| {
            has(spec.bundle.as_bytes()) || bundles::check_hmicar(live, "", &[&hmicar_lines(spec).0]).is_err()
        })
}

/// Any SDK file is on the unit: a `mibsdk-*.jar` in JARS or BASE/sdk.properties. A deploy writes
/// them before the marker and the bundle list, an undo removes them only after the marker, and an
/// uninstall removes them: their presence means an install is half done. A JARS that cannot be
/// listed counts as present.
fn sdk_files_present(l: &Layout) -> bool {
    l.p(&format!("{BASE}/sdk.properties")).exists()
        || names(l, JARS).map_or(true, |v| v.iter().any(|n| n.starts_with("mibsdk-") && n.ends_with(".jar")))
}

/// Bundle-list / marker records left by an earlier install (a forget that failed) are stale when
/// no mibsdk package is recorded, no SDK file is on the unit and the live list does not reference
/// the SDK: the live files are stock, perhaps of a newer firmware, and must be backed up again.
/// While the live list is generated (an undo that failed) or SDK files remain (a half-done
/// install, whose marker tombstone is still needed), the records are the stock state and are kept.
pub fn stale_records(l: &Layout, spec: Option<&SdkSpec>) -> Result<bool> {
    if !backups::has_backup(l, BUNDLES) && !backups::has_backup(l, MARKER) {
        return Ok(false);
    }
    if State::load(l)?.packages.contains_key(SDK_PKG) || sdk_files_present(l) {
        return Ok(false);
    }
    Ok(fs::read(l.p(BUNDLES)).is_ok_and(|live| !references_sdk(&live, spec)))
}

/// Forget stale records (`stale_records`, without a spec) so that restoring every backup
/// (`uninstall-self --purge`) never puts an older list over the live one. The caller holds
/// /mnt/app read-write.
pub fn drop_stale_records(ctx: &Ctx) -> Result<bool> {
    if !stale_records(&ctx.layout, None)? {
        return Ok(false);
    }
    forget_on_app(ctx, BUNDLES)?;
    forget_on_app(ctx, MARKER)?;
    Ok(true)
}

/// Why removing mibpkg would strand an SDK no package records, or None. A complete `mibsdk-dev`
/// backup means its install is active; a live list that references the SDK stays so unless
/// `purge` restores mibpkg's own record of the stock list. The message says how to remove it.
pub fn unrecorded_sdk(l: &Layout, purge: bool) -> Option<String> {
    const TAIL: &str = "without mibpkg nothing can restore the stock bundle list";
    if l.p(&format!("{}/tracer.was", stock_dir())).exists() {
        return Some(format!(
            "an SDK installed by mibsdk-dev is on this unit ({}/tracer.was exists); {TAIL}. Remove it first: \
             mibpkg install mibsdk (it takes the mibsdk-dev install over), then mibpkg remove mibsdk; or uninstall it with mibsdk-dev",
            stock_dir()
        ));
    }
    if !fs::read(l.p(BUNDLES)).is_ok_and(|live| references_sdk(&live, None)) {
        return None;
    }
    let recorded = backups::has_backup(l, BUNDLES) && !backups::is_tombstone(l, BUNDLES);
    if recorded && purge {
        return None; // restore_all puts the recorded stock list back
    }
    let how = if recorded {
        "run: mibpkg uninstall-self --purge (it restores the recorded stock bundle list)"
    } else {
        "put the stock bundle list back by hand (or uninstall the SDK with mibsdk-dev)"
    };
    Some(format!("{BUNDLES} references the SDK but no installed package does; {TAIL}. To remove it, {how}"))
}

/// The stock bundle list on record: mibpkg's backup (unless stale), else a complete `mibsdk-dev`
/// backup that the install will adopt; None on a unit nothing has touched yet.
fn stock_source(layout: &Layout, spec: &SdkSpec) -> Result<Option<PathBuf>> {
    if backups::has_backup(layout, BUNDLES) && !stale_records(layout, Some(spec))? {
        return Ok(Some(backups::backup_path(layout, BUNDLES)));
    }
    let st = stock_dir();
    Ok(layout.p(&format!("{st}/tracer.was")).exists().then(|| layout.p(&format!("{st}/bundles_dsitracer_enabled.properties"))))
}

/// The install guards, before anything is written (nothing here writes). `pkg_dir` is the
/// installed SDK version's directory on an update or rollback (None on a fresh install); only then
/// are the installed plugin packages checked against the new api range.
pub fn check(ctx: &Ctx, m: &Manifest, pkg_dir: Option<&Path>) -> Result<()> {
    let sdk = m.sdk.as_ref().context("not an [sdk] package")?;
    let l = &ctx.layout;

    let hmi = crate::unit::hmi_version(l);
    if !hmi.as_ref().is_some_and(|v| sdk.hmi.contains(v)) {
        bail!(
            "unsupported HMI '{}' (supported: {}); nothing changed",
            hmi.as_deref().unwrap_or("unknown"),
            sdk.hmi.join(", ")
        );
    }

    let jars = names(l, JARS).map_err(|e| anyhow!("cannot list {JARS} ({e}); nothing changed"))?;
    if jars.iter().any(|n| n.to_ascii_lowercase().contains("carplay")) {
        bail!("carplay-rgi is installed (a *carplay* jar is in {JARS}); the SDK does not support it (spec §13); nothing changed");
    }
    match find_carplay_stock(&l.p(HMI_ROOT)) {
        Ok(None) => {}
        Ok(Some(_)) => bail!(
            "carplay-rgi is installed (*.carplay-stock files exist); the SDK does not support it (spec §13); nothing changed"
        ),
        Err((p, e)) => {
            bail!("could not verify the absence of carplay-rgi ({}: {e}); nothing changed", l.unit_path(&p))
        }
    }

    if let Some(j) = jars.iter().find(|n| n.starts_with("mibsdk-spike-") && n.ends_with(".jar")) {
        bail!("spike jar {j} is installed; run spikes/hmi-ui/run.sh uninstall first; nothing changed");
    }

    check_owned_classes(l, sdk, &jars)?;

    // The bundle list must be stock (or, on an update, ours) before any backup exists.
    let (stock_line, sdk_line) = hmicar_lines(sdk);
    let live = fs::read(l.p(BUNDLES)).map_err(|e| anyhow!("cannot read {BUNDLES} ({e}); nothing changed"))?;
    match stock_source(l, sdk)? {
        Some(src) => {
            bundles::check_hmicar(&live, &format!("the installed {BUNDLES}"), &[&stock_line, &sdk_line])?;
            let stock =
                fs::read(&src).map_err(|e| anyhow!("the stock backup {} is unreadable ({e}); nothing changed", l.unit_path(&src)))?;
            bundles::check_hmicar(&stock, "the stock bundles file", &[&stock_line])?;
        }
        None => bundles::check_hmicar(&live, "the stock bundles file", &[&stock_line])?,
    }

    if !l.p(TRACER_JAR).exists() {
        bail!("{TRACER_JAR} is missing; the SDK chains DSITracer's configurator and needs it; nothing changed");
    }

    if pkg_dir.is_some() {
        for (id, api) in plugin_packages(l)? {
            if api < sdk.min_api || api > sdk.api {
                bail!("{id} needs plugin api {api}; this SDK supports {}..{}; nothing changed", sdk.min_api, sdk.api);
            }
        }
    }
    Ok(())
}

/// The sole-owner rule (spec §13): no jar in JARS but the SDK's own (`mibsdk-*`) may define a
/// class under an `owned` prefix. Fails closed on anything it cannot read.
fn check_owned_classes(l: &Layout, sdk: &SdkSpec, jars: &[String]) -> Result<()> {
    for name in jars {
        if !name.ends_with(".jar") || name.starts_with("mibsdk-") {
            continue;
        }
        let abs = format!("{JARS}/{name}");
        let path = l.p(&abs);
        let md = fs::symlink_metadata(&path).map_err(|e| anyhow!("cannot read {abs} ({e}); nothing changed"))?;
        if md.file_type().is_symlink() {
            bail!("{abs} is a symlink; the ownership check cannot follow it; nothing changed");
        }
        if !md.is_file() {
            continue;
        }
        if name.contains(char::is_whitespace) {
            bail!("{abs} has a space in its name; the ownership check cannot handle it; nothing changed");
        }
        let entries = zipnames::list(&path).map_err(|_| anyhow!("cannot list {abs} (not a zip?); nothing changed"))?;
        for prefix in &sdk.owned {
            if let Some(e) = entries.iter().find(|e| e.starts_with(prefix.as_str()) && e.ends_with(".class")) {
                bail!("{name} defines {e}, which the SDK owns; nothing changed");
            }
        }
    }
    Ok(())
}

/// Move the state an install from before the move to PERSIST left under BASE: plugin data file by
/// file (never over a file already in PERSIST), the old ledger merged into PERSIST's, and the
/// rest dropped. Every copy lands under a temp name and is checked, and an old file goes only once
/// its content is safe in PERSIST, so a failed run loses nothing and the next one picks up.
/// Returns notes for the user. The caller holds /mnt/app read-write.
pub fn migrate_state(ctx: &Ctx) -> Result<Vec<String>> {
    migrate_state_with(ctx, &StdOps)
}

#[doc(hidden)]
pub fn migrate_state_with(ctx: &Ctx, ops: &dyn FileOps) -> Result<Vec<String>> {
    let l = &ctx.layout;
    migrate_data(l, ops)
        .map_err(|e| anyhow!("cannot move the old plugin data from {BASE}/data to {PERSIST}/data ({e:#}) {LOST}"))?;
    let notes = migrate_ledger(ctx, ops).map_err(|e| anyhow!("{e:#} {LOST}"))?;
    for name in ["cache", "boot.state", "boot.state.tmp"] {
        let p = l.p(&format!("{BASE}/{name}"));
        let r = match fs::symlink_metadata(&p) {
            Ok(md) if md.is_dir() => fs::remove_dir_all(&p),
            Ok(_) => fs::remove_file(&p),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e),
        };
        r.map_err(|e| anyhow!("cannot remove the old runtime state {BASE}/{name} ({e}) {LOST}"))?;
    }
    Ok(notes)
}

fn remove_if_present(p: &Path) -> io::Result<()> {
    match fs::remove_file(p) {
        Err(e) if e.kind() != io::ErrorKind::NotFound => Err(e),
        _ => Ok(()),
    }
}

fn migrate_data(l: &Layout, ops: &dyn FileOps) -> Result<()> {
    let old = l.p(&format!("{BASE}/data"));
    let new = l.p(&format!("{PERSIST}/data"));
    if old.is_dir() {
        fs::create_dir_all(&new)?;
        let mut files: Vec<OsString> = fs::read_dir(&old)?.map(|e| e.map(|e| e.file_name())).collect::<io::Result<_>>()?;
        files.sort();
        for n in files {
            let src = old.join(&n);
            let dst = new.join(&n);
            if !src.is_file() || n.to_string_lossy().ends_with(".tmp") || dst.exists() {
                continue;
            }
            let mut t = n.clone();
            t.push(".migrate.tmp");
            let tmp = new.join(t);
            let r = (|| -> io::Result<()> {
                remove_if_present(&tmp)?;
                let bytes = ops.read(&src)?;
                let mode = fs::metadata(&src)?.permissions().mode() & 0o777;
                ops.write(&tmp, &bytes, mode)?;
                ops.rename(&tmp, &dst)?;
                // A rename on QNX can "succeed" without moving anything: judge it by its result.
                if tmp.exists() || !dst.is_file() || ops.read(&dst)? != bytes {
                    return Err(io::Error::other("it did not arrive intact"));
                }
                Ok(())
            })();
            if let Err(e) = r {
                let _ = remove_if_present(&tmp);
                return Err(anyhow!("{}: {e}", n.to_string_lossy()));
            }
        }
        fs::remove_dir_all(&old)?;
    }
    if new.is_dir() {
        for e in fs::read_dir(&new)? {
            let e = e?;
            if e.file_name().to_string_lossy().ends_with(".migrate.tmp") {
                remove_if_present(&e.path())?;
            }
        }
    }
    Ok(())
}

fn migrate_ledger(ctx: &Ctx, ops: &dyn FileOps) -> Result<Vec<String>> {
    let l = &ctx.layout;
    let old_tmp = format!("{OLD_LEDGER}.tmp");
    let old_abs = if l.p(OLD_LEDGER).exists() {
        OLD_LEDGER
    } else if l.p(&old_tmp).exists() {
        // The old runtime died between delete and rename: the temp file is the ledger.
        old_tmp.as_str()
    } else {
        return Ok(Vec::new());
    };
    let old_bytes = fs::read(l.p(old_abs)).with_context(|| format!("cannot read {old_abs}; nothing migrated"))?;
    let cur_bytes = match fs::read(l.p(LEDGER)) {
        Ok(b) => Some(b),
        Err(e) if e.kind() == io::ErrorKind::NotFound => None,
        Err(e) => return Err(e).with_context(|| format!("cannot read {LEDGER}; nothing migrated")),
    };
    let old = ledger::decode(&old_bytes);
    let cur = ledger::decode(cur_bytes.as_deref().unwrap_or_default());
    let (new, conflicts) = ledger::merge(&old, &cur);
    if cur_bytes.is_none() || new != cur {
        fs::create_dir_all(l.p(PERSIST)).with_context(|| format!("cannot create {PERSIST}"))?;
        put_with(ctx, LEDGER, &ledger::encode(&new)?, 0o644, ops).with_context(|| format!("kept {old_abs}"))?;
    }
    if !conflicts.is_empty() {
        let mut notes = vec![format!("WARNING: kept {old_abs}: these overrides conflict with {LEDGER} and were not merged:")];
        notes.extend(conflicts.iter().map(|c| format!("  {c}")));
        return Ok(notes);
    }
    let check = ops.read(&l.p(LEDGER)).with_context(|| format!("cannot read back {LEDGER}; kept {old_abs}"))?;
    if !ledger::contains_all(&old, &ledger::decode(&check)) {
        bail!("{LEDGER} lacks entries of {old_abs} after the merge; kept {old_abs}");
    }
    for abs in [OLD_LEDGER, old_tmp.as_str()] {
        remove_if_present(&l.p(abs)).with_context(|| format!("cannot remove {abs}"))?;
    }
    Ok(Vec::new())
}

/// A complete `mibsdk-dev` backup being taken over as mibpkg's own.
struct Adopt {
    tracer_was: bool,
    mode: u32,
}

/// Everything a deploy writes, worked out before the first write.
struct Plan {
    spec: SdkSpec,
    dir: PathBuf,
    adopt: Option<Adopt>,
    /// The bundle-list / marker records are stale (`stale_records`): forget them first.
    drop_stale: bool,
    stock: Vec<u8>,
    /// The stock list's mode (the backup's, the recorded one, or the live file's).
    stock_mode: u32,
    bundles: Vec<u8>,
    props: Vec<u8>,
}

fn mode_of(p: &Path) -> Result<u32> {
    Ok(fs::metadata(p).with_context(|| format!("reading {}", p.display()))?.permissions().mode() & 0o777)
}

/// `$(cat f)`: the content without its trailing newlines.
fn read_word(l: &Layout, abs: &str) -> Result<String> {
    let s = fs::read(l.p(abs)).with_context(|| format!("cannot read {abs}; nothing changed"))?;
    Ok(String::from_utf8_lossy(&s).trim_end_matches('\n').to_string())
}

/// Verify the stamp, decide where the stock list comes from and render every file. No writes.
fn plan(ctx: &Ctx, m: &Manifest, dir: &Path, notes: &mut Vec<String>) -> Result<Plan> {
    let l = &ctx.layout;
    let spec = m.sdk.clone().context("not an [sdk] package")?;
    let jars: Vec<PathBuf> = spec.jars.iter().map(|j| dir.join(j)).collect();
    let refs: Vec<&Path> = jars.iter().map(PathBuf::as_path).collect();
    stamp::verify(&dir.join(&spec.stamp), stamp::StampKind::Unit, None, &refs)
        .map_err(|e| anyhow!(stamp::sdk_refusal(&format!("{e:#}"))))?;

    let st = stock_dir();
    let drop_stale = stale_records(l, Some(&spec))?;
    if drop_stale {
        notes.push(format!("dropped a stale stock record of an earlier install ({BUNDLES} is stock); backed up the live files again"));
    }
    let marker_tracer = || {
        if backups::has_backup(l, MARKER) && !drop_stale {
            !backups::is_tombstone(l, MARKER)
        } else {
            l.p(MARKER).exists()
        }
    };
    let (stock, stock_mode, adopt, tracer) = if backups::has_backup(l, BUNDLES) && !drop_stale {
        let bk = backups::backup_path(l, BUNDLES);
        let stock = fs::read(&bk).with_context(|| format!("the stock backup {} is unreadable", bk.display()))?;
        (stock, mode_of(&bk)?, None, marker_tracer())
    } else if l.p(&format!("{st}/tracer.was")).exists() {
        let was = read_word(l, &format!("{st}/tracer.was"))?;
        let tracer_was = match was.as_str() {
            "true" => true,
            "false" => false,
            _ => bail!("stock/tracer.was is corrupt ('{was}'); nothing changed"),
        };
        let mode_abs = format!("{st}/bundles.mode");
        let mode = if l.p(&mode_abs).exists() {
            let m = read_word(l, &mode_abs)?;
            if m.len() != 3 || !m.bytes().all(|b| (b'0'..=b'7').contains(&b)) {
                bail!("stock/bundles.mode is corrupt ('{m}'); nothing changed");
            }
            u32::from_str_radix(&m, 8)?
        } else {
            notes.push(format!(
                "no recorded mode for {BUNDLES} (backup from an older install); using the stock mode {STOCK_BUNDLES_MODE:o}"
            ));
            STOCK_BUNDLES_MODE
        };
        let src = format!("{st}/bundles_dsitracer_enabled.properties");
        let stock = fs::read(l.p(&src)).with_context(|| format!("the stock backup {src} is unreadable; nothing changed"))?;
        (stock, mode, Some(Adopt { tracer_was, mode }), tracer_was)
    } else {
        let live = fs::read(l.p(BUNDLES)).with_context(|| format!("cannot read {BUNDLES}; nothing changed"))?;
        if live.windows(6).any(|w| w == b"mibsdk") {
            bail!("{BUNDLES} already references mibsdk but no stock backup exists; restore the stock file by hand; nothing changed");
        }
        (live, mode_of(&l.p(BUNDLES))?, None, marker_tracer())
    };
    // check_hmicar before generate (its HMICar= reading is the stricter one).
    bundles::check_hmicar(&stock, "the stock bundles file", &[&hmicar_lines(&spec).0])?;
    let generated = bundles::generate(&stock, &spec, tracer).map_err(|e| anyhow!("{e:#}; nothing changed"))?;
    let props = bundles::sdk_properties(&spec, tracer);
    Ok(Plan { dir: dir.to_path_buf(), adopt, drop_stale, stock, stock_mode, bundles: generated, props, spec })
}

/// Run `f`, a write or removal under /mnt/app, judged by its result: re-assert read-write first
/// (another process may have remounted it read-only), and on failure re-assert and retry once, as
/// `put` does. The caller holds /mnt/app read-write.
#[doc(hidden)]
pub fn on_app<E: Into<anyhow::Error>>(ctx: &Ctx, what: &str, f: &mut dyn FnMut() -> std::result::Result<(), E>) -> Result<()> {
    ctx.mounts.reassert("/mnt/app").with_context(|| what.to_string())?;
    let first: anyhow::Error = match f() {
        Ok(()) => return Ok(()),
        Err(e) => e.into(),
    };
    ctx.mounts
        .reassert("/mnt/app")
        .with_context(|| format!("{what} failed ({first:#}); re-asserting read-write failed"))?;
    f().map_err(|e| anyhow!("{what}: {:#} (and before the retry: {first:#})", e.into()))
}

/// Forget the backup record of `abs` (the records live under /mnt/app).
fn forget_on_app(ctx: &Ctx, abs: &str) -> Result<()> {
    on_app(ctx, &format!("cannot forget the backup of {abs}"), &mut || backups::forget(&ctx.layout, abs))
}

/// Remove the on-unit file `abs` under /mnt/app if it is there.
fn remove_on_app(ctx: &Ctx, abs: &str) -> Result<()> {
    let p = ctx.layout.p(abs);
    on_app(ctx, &format!("cannot remove {abs}"), &mut || remove_if_present(&p))
}

/// What a deploy has done so far, for the undo of a fresh install.
#[derive(Default)]
struct Done {
    /// The bundle-list / marker backups did not exist before this run.
    created_bundles_backup: bool,
    created_marker_backup: bool,
    /// The bundle-list backup was read back equal to the stock bytes of the plan.
    backup_verified: bool,
    /// The marker or the bundle list may have been written.
    activating: bool,
    /// Anything (a record dropped, a backup written) may have changed.
    changed: bool,
    /// The bundle list may have been written (set just before its write).
    bundles_written: bool,
    /// Jars this run put into JARS.
    jars: Vec<String>,
    /// The SDK's own files (sdk.properties, permissions.json) this run may have written.
    files: Vec<String>,
}

/// Record the stock bundle list and marker: adopt `mibsdk-dev`'s backup, or back up the live
/// files. The backup is read back (through `ops`) and must equal the plan's stock bytes.
fn back_up(ctx: &Ctx, plan: &Plan, ops: &dyn FileOps, done: &mut Done, notes: &mut Vec<String>) -> Result<()> {
    let l = &ctx.layout;
    let bk = backups::backup_path(l, BUNDLES);
    let st = stock_dir();
    if plan.drop_stale {
        forget_on_app(ctx, BUNDLES)?;
        done.changed = true;
        forget_on_app(ctx, MARKER)?;
    }
    done.changed = true;
    let (had_bundles, had_marker) = (backups::has_backup(l, BUNDLES), backups::has_backup(l, MARKER));
    let r = (|| -> Result<()> {
        if let Some(a) = &plan.adopt {
            let src = l.p(&format!("{st}/bundles_dsitracer_enabled.properties"));
            on_app(ctx, "adopting the mibsdk-dev stock backup", &mut || crate::fsutil::copy_atomic(&src, &bk, a.mode))?;
            if a.tracer_was {
                on_app(ctx, "backing up the marker", &mut || backups::backup_once(l, MARKER).map(|_| ()))?;
            } else {
                on_app(ctx, "recording the marker as absent", &mut || backups::record_absent(l, MARKER))?;
            }
        } else {
            on_app(ctx, "backing up the bundle list", &mut || backups::backup_once(l, BUNDLES).map(|_| ()))?;
            on_app(ctx, "backing up the marker", &mut || backups::backup_once(l, MARKER).map(|_| ()))?;
        }
        Ok(())
    })();
    done.created_bundles_backup = !had_bundles && backups::has_backup(l, BUNDLES);
    done.created_marker_backup = !had_marker && backups::has_backup(l, MARKER);
    r?;
    if ops.read(&bk).context("reading back the stock backup")? != plan.stock {
        bail!("the backup of {BUNDLES} does not match the stock file; aborting");
    }
    done.backup_verified = true;
    if plan.adopt.is_some() {
        let dir = l.p(&st);
        on_app(ctx, &format!("removing {st}"), &mut || fs::remove_dir_all(&dir))?;
        notes.push(format!("adopted the mibsdk-dev stock backup from {st}"));
    }
    Ok(())
}

/// The writes, in order, recording progress in `done`.
fn apply(ctx: &Ctx, plan: &Plan, ops: &dyn FileOps, done: &mut Done, notes: &mut Vec<String>) -> Result<()> {
    let l = &ctx.layout;
    back_up(ctx, plan, ops, done, notes)?;
    for d in [format!("{BASE}/plugins"), PERSIST.to_string()] {
        let p = l.p(&d);
        on_app(ctx, &format!("cannot create {d}"), &mut || fs::create_dir_all(&p))?;
    }
    notes.extend(migrate_state(ctx)?);
    track_jars(l, &plan.spec.jars)?;
    for j in &plan.spec.jars {
        done.jars.push(j.clone());
        put_file(ctx, &format!("{JARS}/{j}"), &plan.dir.join(j), 0o644)?;
    }
    let props = format!("{BASE}/sdk.properties");
    done.files.push(props.clone());
    put(ctx, &props, &plan.props, 0o644)?;
    done.files.push(PERMISSIONS.to_string());
    put_file(ctx, PERMISSIONS, &plan.dir.join(&plan.spec.permissions), 0o644)?;
    done.activating = true;
    // Only if absent: lsd.sh reads the on-disk bundle list only with the marker, and touching an
    // existing one would change the stock file.
    if !l.p(MARKER).exists() {
        put(ctx, MARKER, b"", 0o644)?;
    }
    // Last: the bundle list activates everything, generated from the backup with its mode.
    let mode = mode_of(&backups::backup_path(l, BUNDLES))?;
    done.bundles_written = true;
    put(ctx, BUNDLES, &plan.bundles, mode)?;
    crate::fsutil::atomic_write(&reboot_flag(l), b"").context("writing the reboot flag")?;
    Ok(())
}

/// Install the SDK version unpacked in `ver_dir`: verify its stamp, record the stock files,
/// migrate old state, then write the jars, sdk.properties, permissions.json, the marker and the
/// bundle list; last, remove the SDK jars an earlier version deployed that this one no longer
/// lists. Returns notes for the user. All or undo: on failure `prev_dir` (the version
/// being replaced) is deployed again, or a fresh install is undone (the stock bundle list and
/// marker back, the jars written deleted); the error says which. The caller holds /mnt/app
/// read-write.
pub fn deploy(ctx: &Ctx, m: &Manifest, ver_dir: &Path, prev_dir: Option<&Path>) -> Result<Vec<String>> {
    deploy_with(ctx, m, ver_dir, prev_dir, &StdOps)
}

/// `deploy`, reading the backup back through `ops` (tests stand in for a bad read-back).
#[doc(hidden)]
pub fn deploy_with(ctx: &Ctx, m: &Manifest, ver_dir: &Path, prev_dir: Option<&Path>, ops: &dyn FileOps) -> Result<Vec<String>> {
    let mut notes = Vec::new();
    let plan = plan(ctx, m, ver_dir, &mut notes)?; // nothing written yet
    let mut done = Done::default();
    // Last, so a failure redeploys the replaced version with all its jars.
    let r = apply(ctx, &plan, ops, &mut done, &mut notes).and_then(|()| drop_unlisted_jars(ctx, &plan.spec));
    let Err(e) = r else { return Ok(notes) };
    let undo = match prev_dir {
        Some(prev) => redeploy(ctx, prev, &done.jars),
        None => undo_fresh(ctx, &plan, &done),
    };
    Err(match undo {
        Ok(said) => anyhow!("{e:#}; {said}"),
        Err(u) if prev_dir.is_some() => anyhow!("{e:#}; deploying the previous version again failed too ({u:#}); install again or remove mibsdk"),
        Err(u) => anyhow!("{e:#}; undoing it failed too ({u:#}); the stock backup is kept: run the install again"),
    })
}

fn redeploy(ctx: &Ctx, prev: &Path, written: &[String]) -> Result<String> {
    let m = Manifest::parse(&fs::read_to_string(prev.join("manifest.toml"))?)?;
    let mut notes = Vec::new();
    let plan = plan(ctx, &m, prev, &mut notes)?;
    apply(ctx, &plan, &StdOps, &mut Done::default(), &mut notes)?;
    for j in written.iter().filter(|j| !plan.spec.jars.contains(j)) {
        remove_on_app(ctx, &format!("{JARS}/{j}"))?;
    }
    Ok(format!("mibsdk {} was deployed again", m.package.version))
}

/// Where mibpkg records every jar name an `[sdk]` version it deployed listed, one per line (in
/// mibpkg's own package directory, which an uninstall removes).
fn deployed_jars_file(l: &Layout) -> PathBuf {
    l.pkg_dir(SDK_PKG).join("deployed-jars")
}

/// The SDK jar names mibpkg deployed: only `mibsdk-*.jar` names (as `[sdk] jars` must be), so a
/// damaged record never names another jar.
fn deployed_jars(l: &Layout) -> Result<Vec<String>> {
    let text = match fs::read_to_string(deployed_jars_file(l)) {
        Ok(t) => t,
        Err(e) if e.kind() == io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(e).context("reading the record of the deployed SDK jars"),
    };
    Ok(text
        .lines()
        .filter(|n| n.starts_with("mibsdk-") && n.ends_with(".jar") && !n.contains('/'))
        .map(String::from)
        .collect())
}

/// Add `jars` to the record of the deployed SDK jars.
fn track_jars(l: &Layout, jars: &[String]) -> Result<()> {
    let mut all = deployed_jars(l)?;
    all.extend(jars.iter().cloned());
    all.sort();
    all.dedup();
    fs::create_dir_all(l.pkg_dir(SDK_PKG))?;
    crate::fsutil::atomic_write(&deployed_jars_file(l), (all.join("\n") + "\n").as_bytes())
        .context("recording the deployed SDK jars")
}

/// Remove the SDK jars an earlier version deployed that `spec` no longer lists.
fn drop_unlisted_jars(ctx: &Ctx, spec: &SdkSpec) -> Result<()> {
    for j in deployed_jars(&ctx.layout)?.iter().filter(|j| !spec.jars.contains(j)) {
        remove_on_app(ctx, &format!("{JARS}/{j}"))?;
    }
    Ok(())
}

/// Undo a fresh install. The live bundle list is written only with the plan's (verified) stock
/// bytes, and only if this run may have changed it or adopted a backup of a generated one; a
/// backup is forgotten only once the live list is stock.
fn undo_fresh(ctx: &Ctx, plan: &Plan, done: &Done) -> Result<String> {
    let l = &ctx.layout;
    if !done.backup_verified {
        // Nothing but backups was written: drop the ones this run made (a bad copy included).
        if done.created_bundles_backup {
            forget_on_app(ctx, BUNDLES)?;
        }
        if done.created_marker_backup {
            forget_on_app(ctx, MARKER)?;
        }
        return Ok(if done.changed { "the install was undone" } else { "nothing changed" }.into());
    }
    let live_stock = if done.bundles_written || plan.adopt.is_some() {
        put(ctx, BUNDLES, &plan.stock, plan.stock_mode)?;
        true
    } else {
        // Untouched: stock if this run backed it up, unknown if the backup is an older one.
        done.created_bundles_backup
    };
    if done.activating || plan.adopt.is_some() || done.created_marker_backup {
        restore_marker(ctx)?;
    }
    // sdk.properties too: an SDK file left behind would keep the records from going stale.
    for abs in done.jars.iter().map(|j| format!("{JARS}/{j}")).chain(done.files.iter().cloned()) {
        // A directory in the file's place was never written (put cannot replace one): not ours.
        if !l.p(&abs).is_dir() {
            remove_on_app(ctx, &abs)?;
        }
    }
    if live_stock {
        forget_backups(ctx)?;
    }
    Ok(if plan.adopt.is_some() {
        "the unit was returned to the stock bundle list (the earlier mibsdk-dev install is no longer active); install again to retry".into()
    } else {
        "the install was undone".into()
    })
}

/// Drop the bundle-list and marker backups: after an uninstall has finished (the caller saved
/// the state), or an undo left the stock files live.
/// Every record is attempted (each with the F8 re-assert and retry); the first error is returned.
pub fn forget_backups(ctx: &Ctx) -> Result<()> {
    let mut first = None;
    for abs in [BUNDLES, MARKER] {
        if let Err(e) = forget_on_app(ctx, abs) {
            first.get_or_insert(e);
        }
    }
    first.map_or(Ok(()), Err)
}

/// The stock bundle list back from the backup, bytes and mode. The backup is kept.
fn restore_bundles(ctx: &Ctx) -> Result<()> {
    let bk = backups::backup_path(&ctx.layout, BUNDLES);
    let bytes = fs::read(&bk).with_context(|| format!("reading the stock backup of {BUNDLES}"))?;
    put(ctx, BUNDLES, &bytes, mode_of(&bk)?)
}

/// The marker as it was: removed if it was absent; an existing one is left alone (touching it
/// would change the stock file); a missing one put back. The backup is kept.
fn restore_marker(ctx: &Ctx) -> Result<()> {
    let l = &ctx.layout;
    if backups::is_tombstone(l, MARKER) {
        remove_on_app(ctx, MARKER)?;
    } else if backups::has_backup(l, MARKER) && !l.p(MARKER).exists() {
        let bk = backups::backup_path(l, MARKER);
        put(ctx, MARKER, &fs::read(&bk)?, mode_of(&bk)?)?;
    }
    Ok(())
}

fn remove_tree(l: &Layout, abs: &str) -> std::io::Result<()> {
    match fs::remove_dir_all(l.p(abs)) {
        Err(e) if e.kind() != io::ErrorKind::NotFound => Err(e),
        _ => Ok(()),
    }
}

/// Uninstall the SDK runtime: refuse while plugin packages, pushed plugins (unless `--force`) or
/// overrides of vehicle state (unless `--force`, spec I2) remain; then restore the stock bundle
/// list (first, so nothing references the jars once they are gone) and marker, and remove the
/// jars and the SDK's own files. After a firmware update (the live list no longer the generated
/// one, or an HMI the SDK does not support) the live list and marker are left alone and a note
/// says so: the records hold the old firmware's files. A live list that still references the
/// SDK on an unsupported HMI is refused (nothing changed) unless `--force`. Without `purge` the grants, plugins/ and
/// PERSIST stay. Returns any warnings (one per line) and, last, a note. The backups are kept: the
/// caller forgets them (`forget_backups`) once the package is recorded as removed, so a run that
/// fails anywhere can be run again. The caller holds /mnt/app read-write.
pub fn undeploy(ctx: &Ctx, purge: bool) -> Result<String> {
    let l = &ctx.layout;
    let plugins: Vec<String> = plugin_packages(l)?.into_iter().map(|(id, _)| id).collect();
    if !plugins.is_empty() {
        bail!("remove these plugins first: {}", plugins.join(", "));
    }
    let pushed: Vec<String> = match names(l, &format!("{BASE}/plugins")) {
        Ok(v) => v.into_iter().filter_map(|n| n.strip_suffix(".jar").map(String::from)).collect(),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Vec::new(),
        Err(e) => bail!("cannot list {BASE}/plugins ({e}); refusing to uninstall"),
    };
    if !pushed.is_empty() && !ctx.force {
        bail!(
            "these plugins were pushed by mibsdk-dev: {}; remove them first (mibpkg sdk remove <id>) or add --force",
            pushed.join(", ")
        );
    }
    let mut warnings = Vec::new();
    let entries = ledger::read(l).context("refusing to uninstall")?;
    if !entries.is_empty() {
        let lines: Vec<String> = entries.iter().map(|e| format!("  {}", e.line)).collect();
        if !ctx.force {
            bail!(
                "refusing to uninstall: these overrides of vehicle state are still in force (key=plugin|scope|prior|active|A or R):\n\
                 {}\n\
                 let the SDK put the prior values back first: mibpkg sdk safe-mode on, then restart the HMI\n\
                 (safe mode loads no plugin and restores every override in the ledger; check /tmp/mibsdk.log), then run the removal again.\n\
                 after a later reinstall, leave safe mode with: mibpkg sdk safe-mode off\n\
                 to uninstall anyway and leave those settings overridden, add --force",
                lines.join("\n")
            );
        }
        warnings.push("WARNING: --force: uninstalling while these overrides of vehicle state are still in force:".into());
        warnings.extend(lines);
        warnings.push("WARNING: nothing will restore their prior values; the car keeps the overridden settings until you change them back by hand (format: key=plugin|scope|prior|active|A or R).".into());
    }
    if !pushed.is_empty() {
        warnings.push(format!("WARNING: --force: uninstalling while these pushed plugins remain: {}", pushed.join(", ")));
    }
    if !backups::has_backup(l, BUNDLES) || backups::is_tombstone(l, BUNDLES) {
        bail!("no stock backup of {BUNDLES}; refusing to guess. Restore it by hand");
    }
    let Some((_, spec, _)) = installed_sdk(l) else {
        // A run interrupted after the package files went: finish it if the stock list is live.
        let bk = fs::read(backups::backup_path(l, BUNDLES)).context("reading the stock backup")?;
        let live = fs::read(l.p(BUNDLES)).ok();
        if live.as_deref() == Some(bk.as_slice()) {
            restore_marker(ctx)?;
        } else if live.is_some_and(|b| !references_sdk(&b, None)) && !sdk_files_present(l) {
            // An earlier run after a firmware update: it left the firmware's list alone.
            warnings.push(firmware_changed(crate::unit::hmi_version(l).as_deref(), false));
        } else {
            bail!("cannot read the installed mibsdk package and {BUNDLES} is not stock; restore it by hand");
        }
        warnings.push("the SDK files were already removed by an earlier run".into());
        warnings.push(remove_sdk_dirs(ctx, purge)?);
        return Ok(warnings.join("\n"));
    };

    // The records are this firmware's stock only while the live list is still the one the install
    // generated and the HMI one the SDK supports; after a firmware update replaced lsd/ they would
    // put the old firmware's list (and marker) onto the new one.
    let hmi = crate::unit::hmi_version(l);
    let refs = fs::read(l.p(BUNDLES)).is_ok_and(|live| references_sdk(&live, Some(&spec)));
    let supported = hmi.as_ref().is_some_and(|v| spec.hmi.contains(v));
    if refs && !supported && !ctx.force {
        bail!("{}", firmware_changed_refusal(hmi.as_deref(), Some(&spec)));
    }
    if refs && supported {
        restore_bundles(ctx)?;
        restore_marker(ctx)?;
    } else {
        warnings.push(firmware_changed(hmi.as_deref(), refs));
    }
    // Every SDK jar an installed version listed, not only the current version's.
    let mut jars = deployed_jars(l)?;
    jars.extend(spec.jars.iter().cloned());
    jars.sort();
    jars.dedup();
    for j in &jars {
        remove_on_app(ctx, &format!("{JARS}/{j}"))?;
    }
    for abs in [format!("{BASE}/sdk.properties"), PERMISSIONS.to_string()] {
        remove_on_app(ctx, &abs)?;
    }
    warnings.push(remove_sdk_dirs(ctx, purge)?);
    Ok(warnings.join("\n"))
}

/// Why an uninstall refuses to restore the stock records: the HMI is not one `spec` (the installed
/// mibsdk; None: unreadable) supports, so the records are an older firmware's.
fn firmware_changed_refusal(hmi: Option<&str>, spec: Option<&SdkSpec>) -> String {
    format!(
        "refusing to uninstall: the firmware changed since mibsdk was installed (HMI {}; this SDK supports {}), \
         but {BUNDLES} still references the SDK.\n\
         the stock backup is from the older firmware: restoring it, or removing the SDK jars this list still loads, \
         could stop the HMI booting.\n\
         put this firmware's stock bundle list back first (reinstall the firmware, or install a mibsdk that supports \
         this HMI), then run the removal again.\n\
         to remove the SDK files anyway and leave the bundle list as it is, add --force; nothing changed",
        hmi.unwrap_or("unknown"),
        spec.map_or("unknown (the installed mibsdk package cannot be read)".into(), |s| s.hmi.join(", "))
    )
}

/// Before `uninstall-self --purge` writes anything: may restoring every backup put the recorded
/// (non-stale) bundle list and marker back? Not when the HMI is not one the last installed mibsdk
/// supports, or that spec cannot be read: the records may be an older firmware's (the I1 rule of
/// `undeploy`). Ok(None): restore as usual. Refused without `force`; with it Ok(Some(note)): the
/// caller forgets the records (`forget_backups`) instead of restoring them, leaving the live files.
pub fn purge_firmware_check(l: &Layout, force: bool) -> Result<Option<String>> {
    if !backups::has_backup(l, BUNDLES) && !backups::has_backup(l, MARKER) || stale_records(l, None)? {
        return Ok(None); // nothing to restore, or drop_stale_records forgets it
    }
    let hmi = crate::unit::hmi_version(l);
    let spec = installed_sdk(l).map(|(_, spec, _)| spec);
    if spec.as_ref().is_some_and(|s| hmi.as_ref().is_some_and(|v| s.hmi.contains(v))) {
        return Ok(None);
    }
    if !force {
        bail!("{}", firmware_changed_refusal(hmi.as_deref(), spec.as_ref()));
    }
    // The SDK files stay on the unit (purge removes only mibpkg's), so the list may keep loading them.
    let mut note = format!("WARNING: --force: {}", firmware_changed(hmi.as_deref(), false));
    if fs::read(l.p(BUNDLES)).is_ok_and(|live| references_sdk(&live, spec.as_ref())) {
        note.push_str(&format!("; {BUNDLES} still references the SDK, and nothing is left to restore it: put this firmware's stock bundle list back by hand"));
    }
    Ok(Some(note))
}

/// The note of an uninstall that left the live bundle list and marker alone (the firmware changed
/// since the install); `refs`: the live list still references the SDK.
fn firmware_changed(hmi: Option<&str>, refs: bool) -> String {
    let mut s = format!(
        "the firmware changed since mibsdk was installed (HMI {}): left the live bundle list and DSITracer marker as they are and dropped the old firmware's stock records",
        hmi.unwrap_or("unknown")
    );
    if refs {
        s = format!(
            "WARNING: {s}; but {BUNDLES} still references the SDK, whose files are now removed: put this firmware's stock bundle list back by hand"
        );
    }
    s
}

/// With `purge` remove BASE, PERSIST and RAM; else remove BASE only if empty (grants.properties
/// and plugins/ stay). Then the reboot flag. Returns the note.
fn remove_sdk_dirs(ctx: &Ctx, purge: bool) -> Result<String> {
    let l = &ctx.layout;
    let note = if purge {
        on_app(ctx, &format!("cannot remove {BASE}"), &mut || remove_tree(l, BASE))?;
        for abs in [PERSIST, RAM] {
            remove_tree(l, abs).with_context(|| format!("cannot remove {abs}"))?;
        }
        format!("removed {BASE}, {PERSIST} and {RAM}")
    } else {
        let base = l.p(BASE);
        match fs::remove_dir(&base) {
            Err(e) if !matches!(e.kind(), io::ErrorKind::DirectoryNotEmpty | io::ErrorKind::NotFound) => {
                // Best effort, so a read-only remount by someone else gets one re-assert and retry.
                let _ = on_app(ctx, "", &mut || fs::remove_dir(&base));
            }
            _ => {}
        }
        format!("kept plugin data in {PERSIST} (remove it with: mibpkg remove mibsdk --purge)")
    };
    crate::fsutil::atomic_write(&reboot_flag(l), b"").context("writing the reboot flag")?;
    Ok(note)
}
