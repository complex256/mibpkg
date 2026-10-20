//! The `[sdk]` package (`mibsdk`): the install guards, the move of an old `mibsdk-dev` setup's
//! state, the all-or-undo deploy and the guarded uninstall. A port of `mibsdk-dev`'s
//! `cmd_install` / `cmd_uninstall` (`check_owned_classes`, `check_hmicar`, `migrate_state`,
//! `migrate_ledger`, `check_ledger`, `restore_bundles_mode`), writing through `put`.
use super::{bundles, ledger, stamp, zipnames};
use super::{installed_sdk, put, put_file, put_with, reboot_flag, FileOps, StdOps};
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

/// The stock bundle list on record: mibpkg's backup, else a complete `mibsdk-dev` backup that
/// the install will adopt; None on a unit nothing has touched yet.
fn stock_source(layout: &Layout) -> Option<PathBuf> {
    if backups::has_backup(layout, BUNDLES) {
        return Some(backups::backup_path(layout, BUNDLES));
    }
    let st = stock_dir();
    layout.p(&format!("{st}/tracer.was")).exists().then(|| layout.p(&format!("{st}/bundles_dsitracer_enabled.properties")))
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
    match stock_source(l) {
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
    stock: Vec<u8>,
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
    let marker_tracer = || {
        if backups::has_backup(l, MARKER) {
            !backups::is_tombstone(l, MARKER)
        } else {
            l.p(MARKER).exists()
        }
    };
    let (stock, adopt, tracer) = if backups::has_backup(l, BUNDLES) {
        let bk = backups::backup_path(l, BUNDLES);
        (fs::read(&bk).with_context(|| format!("the stock backup {} is unreadable", bk.display()))?, None, marker_tracer())
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
        (stock, Some(Adopt { tracer_was, mode }), tracer_was)
    } else {
        let live = fs::read(l.p(BUNDLES)).with_context(|| format!("cannot read {BUNDLES}; nothing changed"))?;
        if live.windows(6).any(|w| w == b"mibsdk") {
            bail!("{BUNDLES} already references mibsdk but no stock backup exists; restore the stock file by hand; nothing changed");
        }
        (live, None, marker_tracer())
    };
    // check_hmicar before generate (its HMICar= reading is the stricter one).
    bundles::check_hmicar(&stock, "the stock bundles file", &[&hmicar_lines(&spec).0])?;
    let generated = bundles::generate(&stock, &spec, tracer).map_err(|e| anyhow!("{e:#}; nothing changed"))?;
    let props = bundles::sdk_properties(&spec, tracer);
    Ok(Plan { dir: dir.to_path_buf(), adopt, stock, bundles: generated, props, spec })
}

/// Record the stock bundle list and marker: adopt `mibsdk-dev`'s backup, or back up the live files.
fn back_up(ctx: &Ctx, plan: &Plan, notes: &mut Vec<String>) -> Result<()> {
    let l = &ctx.layout;
    let bk = backups::backup_path(l, BUNDLES);
    if let Some(a) = &plan.adopt {
        let st = stock_dir();
        crate::fsutil::copy_atomic(&l.p(&format!("{st}/bundles_dsitracer_enabled.properties")), &bk, a.mode)
            .context("adopting the mibsdk-dev stock backup")?;
        if a.tracer_was {
            backups::backup_once(l, MARKER)?;
        } else {
            backups::record_absent(l, MARKER)?;
        }
    } else {
        backups::backup_once(l, BUNDLES)?;
        backups::backup_once(l, MARKER)?;
    }
    if fs::read(&bk).context("reading back the stock backup")? != plan.stock {
        bail!("the backup of {BUNDLES} does not match the stock file; aborting");
    }
    if plan.adopt.is_some() {
        let st = stock_dir();
        fs::remove_dir_all(l.p(&st)).with_context(|| format!("removing {st}"))?;
        notes.push(format!("adopted the mibsdk-dev stock backup from {st}"));
    }
    Ok(())
}

/// The writes, in order; `written` collects the jars this run put into JARS.
fn apply(ctx: &Ctx, plan: &Plan, written: &mut Vec<String>, notes: &mut Vec<String>) -> Result<()> {
    let l = &ctx.layout;
    back_up(ctx, plan, notes)?;
    for d in [format!("{BASE}/plugins"), PERSIST.to_string()] {
        fs::create_dir_all(l.p(&d)).with_context(|| format!("cannot create {d}"))?;
    }
    notes.extend(migrate_state(ctx)?);
    for j in &plan.spec.jars {
        written.push(j.clone());
        put_file(ctx, &format!("{JARS}/{j}"), &plan.dir.join(j), 0o644)?;
    }
    put(ctx, &format!("{BASE}/sdk.properties"), &plan.props, 0o644)?;
    put_file(ctx, PERMISSIONS, &plan.dir.join(&plan.spec.permissions), 0o644)?;
    // Only if absent: lsd.sh reads the on-disk bundle list only with the marker, and touching an
    // existing one would change the stock file.
    if !l.p(MARKER).exists() {
        put(ctx, MARKER, b"", 0o644)?;
    }
    // Last: the bundle list activates everything, generated from the backup with its mode.
    let mode = mode_of(&backups::backup_path(l, BUNDLES))?;
    put(ctx, BUNDLES, &plan.bundles, mode)?;
    crate::fsutil::atomic_write(&reboot_flag(l), b"").context("writing the reboot flag")?;
    Ok(())
}

/// Install the SDK version unpacked in `ver_dir`: verify its stamp, record the stock files,
/// migrate old state, then write the jars, sdk.properties, permissions.json, the marker and, last,
/// the bundle list. All or undo: on failure `prev_dir` (the version being replaced) is deployed
/// again, or on a fresh install the bundle list and marker are restored and the jars written are
/// deleted; the original error is returned. The caller holds /mnt/app read-write.
pub fn deploy(ctx: &Ctx, m: &Manifest, ver_dir: &Path, prev_dir: Option<&Path>) -> Result<()> {
    let mut notes = Vec::new();
    let plan = plan(ctx, m, ver_dir, &mut notes)?; // nothing written yet
    let mut written = Vec::new();
    let r = apply(ctx, &plan, &mut written, &mut notes);
    for n in &notes {
        (ctx.progress)(n);
    }
    let Err(e) = r else { return Ok(()) };
    let undo = match prev_dir {
        Some(prev) => redeploy(ctx, prev, &written),
        None => undo_fresh(ctx, &written),
    };
    match undo {
        Ok(()) => Err(e),
        Err(u) => Err(anyhow!("{e:#}; undoing it failed too ({u:#}); install again or remove mibsdk")),
    }
}

fn redeploy(ctx: &Ctx, prev: &Path, written: &[String]) -> Result<()> {
    let m = Manifest::parse(&fs::read_to_string(prev.join("manifest.toml"))?)?;
    let mut notes = Vec::new();
    let plan = plan(ctx, &m, prev, &mut notes)?;
    apply(ctx, &plan, &mut Vec::new(), &mut notes)?;
    for j in written.iter().filter(|j| !plan.spec.jars.contains(j)) {
        remove_if_present(&ctx.layout.p(&format!("{JARS}/{j}")))?;
    }
    Ok(())
}

fn undo_fresh(ctx: &Ctx, written: &[String]) -> Result<()> {
    if backups::has_backup(&ctx.layout, BUNDLES) {
        restore_bundles(ctx)?;
    }
    restore_marker(ctx)?;
    for j in written {
        remove_if_present(&ctx.layout.p(&format!("{JARS}/{j}")))?;
    }
    Ok(())
}

/// The stock bundle list back, bytes and mode, then the backup forgotten.
fn restore_bundles(ctx: &Ctx) -> Result<()> {
    let l = &ctx.layout;
    let bk = backups::backup_path(l, BUNDLES);
    let bytes = fs::read(&bk).with_context(|| format!("reading the stock backup of {BUNDLES}"))?;
    put(ctx, BUNDLES, &bytes, mode_of(&bk)?)?;
    backups::forget(l, BUNDLES)
}

/// The marker as it was: removed if it was absent; an existing one is left alone (touching it
/// would change the stock file); a missing one put back.
fn restore_marker(ctx: &Ctx) -> Result<()> {
    let l = &ctx.layout;
    if backups::is_tombstone(l, MARKER) {
        remove_if_present(&l.p(MARKER)).with_context(|| format!("cannot remove {MARKER}"))?;
    } else if backups::has_backup(l, MARKER) && !l.p(MARKER).exists() {
        let bk = backups::backup_path(l, MARKER);
        put(ctx, MARKER, &fs::read(&bk)?, mode_of(&bk)?)?;
    }
    backups::forget(l, MARKER)
}

fn remove_tree(l: &Layout, abs: &str) -> Result<()> {
    match fs::remove_dir_all(l.p(abs)) {
        Err(e) if e.kind() != io::ErrorKind::NotFound => Err(e).with_context(|| format!("cannot remove {abs}")),
        _ => Ok(()),
    }
}

/// Uninstall the SDK runtime: refuse while plugin packages, pushed plugins (unless `--force`) or
/// overrides of vehicle state (unless `--force`, spec I2) remain; then restore the stock bundle
/// list (first, so nothing references the jars once they are gone) and marker, and remove the
/// jars and the SDK's own files. Without `purge` the grants, plugins/ and PERSIST stay. Returns
/// any warnings (one per line) and, last, a note. The caller holds /mnt/app read-write.
pub fn undeploy(ctx: &Ctx, purge: bool) -> Result<String> {
    let l = &ctx.layout;
    let (_, spec, _) = installed_sdk(l).context("cannot read the installed mibsdk package; restore the stock bundle list by hand")?;
    let plugins: Vec<String> = plugin_packages(l)?.into_iter().map(|(id, _)| id).collect();
    if !plugins.is_empty() {
        bail!("remove these plugins first: {}", plugins.join(", "));
    }
    let pushed: Vec<String> = match names(l, &format!("{BASE}/plugins")) {
        Ok(v) => v.into_iter().filter_map(|n| n.strip_suffix(".jar").map(String::from)).collect(),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Vec::new(),
        Err(e) => bail!("cannot list {BASE}/plugins ({e}); refusing to uninstall"),
    };
    let mut warnings = Vec::new();
    if !pushed.is_empty() {
        if !ctx.force {
            bail!(
                "these plugins were pushed by mibsdk-dev: {}; remove them first (mibpkg sdk remove <id>) or add --force",
                pushed.join(", ")
            );
        }
        warnings.push(format!("WARNING: --force: uninstalling while these pushed plugins remain: {}", pushed.join(", ")));
    }
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
    if !backups::has_backup(l, BUNDLES) || backups::is_tombstone(l, BUNDLES) {
        bail!("no stock backup of {BUNDLES}; refusing to guess. Restore it by hand");
    }

    restore_bundles(ctx)?;
    restore_marker(ctx)?;
    for j in &spec.jars {
        let abs = format!("{JARS}/{j}");
        remove_if_present(&l.p(&abs)).with_context(|| format!("cannot remove {abs}"))?;
    }
    for abs in [format!("{BASE}/sdk.properties"), PERMISSIONS.to_string()] {
        remove_if_present(&l.p(&abs)).with_context(|| format!("cannot remove {abs}"))?;
    }
    let note = if purge {
        for abs in [BASE, PERSIST, RAM] {
            remove_tree(l, abs)?;
        }
        format!("removed {BASE}, {PERSIST} and {RAM}")
    } else {
        // Only when empty: grants.properties and plugins/ stay.
        let _ = fs::remove_dir(l.p(BASE));
        format!("kept plugin data in {PERSIST} (remove it with: mibpkg remove mibsdk --purge)")
    };
    crate::fsutil::atomic_write(&reboot_flag(l), b"").context("writing the reboot flag")?;
    warnings.push(note);
    Ok(warnings.join("\n"))
}
