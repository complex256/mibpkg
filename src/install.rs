//! Install / update / remove / rollback. The only commit point of an install is
//! rewriting pkgs/<name>/current; everything before it is undone on failure.
use crate::archive::{sha256_file, Package};
use crate::bootd;
use crate::config;
use crate::control;
use crate::ctx::Ctx;
use crate::fsutil::{atomic_write, copy_atomic};
use crate::index::{cmp_versions, Collection};
use crate::legacy;
use crate::manifest::{Kind, Manifest};
use crate::mibsdk::{icons, plugin_pkg, sdk_pkg};
use crate::paths::Layout;
use crate::state::{Installed, State};
use anyhow::{bail, Context, Result};
use std::cmp::Ordering;
use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::time::Duration;

pub const SELF: &str = "mibpkg";
const SCRIPT_TIMEOUT: Duration = Duration::from_secs(120);

fn script_timeout(m: &Manifest) -> Duration {
    m.scripts
        .timeout
        .as_deref()
        .map(|t| Duration::from_secs(crate::conditions::parse_timeout(t).expect("validated")))
        .unwrap_or(SCRIPT_TIMEOUT)
}
/// Appended to the result of an SDK install, update or rollback: the HMI loads it at boot.
const SDK_START: &str = "; reboot the unit (or restart the HMI) to start it";
const SDK_REMOVED: &str = "; reboot the unit (or restart the HMI) to finish removing it";

pub fn current_version(layout: &Layout, pkg: &str) -> Option<String> {
    fs::read_to_string(layout.pkg_dir(pkg).join("current"))
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

pub fn current_dir(layout: &Layout, pkg: &str) -> Option<PathBuf> {
    current_version(layout, pkg).map(|v| layout.pkg_dir(pkg).join(v)).filter(|d| d.is_dir())
}

pub fn current_manifest(layout: &Layout, pkg: &str) -> Result<Manifest> {
    let d = current_dir(layout, pkg).with_context(|| format!("{pkg} is not installed"))?;
    Manifest::parse(&fs::read_to_string(d.join("manifest.toml"))?)
}

/// The installed package that carries plugin `id`, if any. A plugin-only package is its own
/// owner; a combined product package (for example `tailscale` carrying `tailscale-status`) is
/// found by its manifest.
pub fn package_owning_plugin(layout: &Layout, id: &str) -> Option<String> {
    let st = State::load(layout).ok()?;
    st.packages.keys().find_map(|n| {
        current_manifest(layout, n)
            .ok()
            .filter(|m| m.plugin_id() == Some(id))
            .map(|_| n.clone())
    })
}

/// Remove `<version>.new` dirs left by an interrupted install. Caller holds /mnt/app rw.
pub fn cleanup_stale(layout: &Layout) -> Result<()> {
    let dir = layout.pkgs_dir();
    if !dir.is_dir() {
        return Ok(());
    }
    for p in fs::read_dir(&dir)? {
        let p = p?.path();
        if !p.is_dir() {
            continue;
        }
        for v in fs::read_dir(&p)? {
            let v = v?.path();
            if v.is_dir() && v.extension().map_or(false, |e| e == "new") {
                fs::remove_dir_all(&v)?;
            }
        }
    }
    Ok(())
}

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

pub fn pick_collection<'c>(
    state: &State,
    colls: &'c [Collection],
    name: &str,
    id: Option<&str>,
    unit: &crate::unit::UnitInfo,
) -> Result<&'c Collection> {
    // Several origins (card, online) may carry the same collection id: take the one offering the
    // newest version of `name` that fits this unit; on a tie the card wins (no download, works
    // offline).
    let best_of = |want: &str| -> Option<&'c Collection> {
        colls
            .iter()
            .filter(|c| c.id() == want)
            .max_by(|a, b| {
                let va = a.latest_compatible(name, unit).map(|e| e.version.clone()).unwrap_or_default();
                let vb = b.latest_compatible(name, unit).map(|e| e.version.clone()).unwrap_or_default();
                cmp_versions(&va, &vb).then(a.base_url.is_none().cmp(&b.base_url.is_none()))
            })
    };
    let want = id.map(String::from).or_else(|| state.packages.get(name).map(|i| i.collection.clone()));
    if want.as_deref() == Some(crate::index::RESERVED_COLLECTION) {
        bail!("{name} was installed from an uploaded file; remove it first to install it from a collection");
    }
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
    let mut ids: Vec<&str> = colls.iter().filter(|c| c.latest(name).is_some()).map(|c| c.id()).collect();
    ids.sort();
    ids.dedup();
    match ids.len() {
        0 => bail!("{name} is not in any collection on the card or online"),
        1 => Ok(best_of(ids[0]).unwrap()),
        _ => bail!("{name} is in several collections ({}); choose one", ids.join(", ")),
    }
}


fn script_env(layout: &Layout, name: &str, dir: &Path, version: &str) -> Vec<(String, String)> {
    vec![
        ("MIBPKG_PKG_DIR".into(), dir.display().to_string()),
        ("MIBPKG_CONFIG_DIR".into(), layout.config_dir(name).display().to_string()),
        ("MIBPKG_VERSION".into(), version.to_string()),
    ]
}

fn prune_versions(pkg_dir: &Path, keep: &str, keep_prev: Option<&str>) -> Result<()> {
    for e in fs::read_dir(pkg_dir)? {
        let p = e?.path();
        if !p.is_dir() {
            continue;
        }
        let n = p.file_name().unwrap().to_string_lossy().to_string();
        if n != keep && Some(n.as_str()) != keep_prev {
            fs::remove_dir_all(&p)?;
        }
    }
    Ok(())
}

fn self_update(layout: &Layout, ver_dir: &Path) -> Result<()> {
    for f in ["mibpkg", "launch.sh", "gem.sh"] {
        copy_atomic(&ver_dir.join("bin").join(f), &layout.bin_dir().join(f), 0o755)
            .with_context(|| format!("installing new {f}"))?;
    }
    let ca = ver_dir.join("bin/ca.pem");
    if ca.is_file() {
        copy_atomic(&ca, &layout.bin_dir().join("ca.pem"), 0o644).context("installing new ca.pem")?;
    }
    control::request_selfupdate(layout)
}

pub fn install(ctx: &Ctx, media: &[PathBuf], coll: &Collection, name: &str) -> Result<String> {
    let layout = &ctx.layout;
    let coll_id = coll.id().to_string();
    let newest = coll.latest(name).with_context(|| format!("{name} is not in collection {coll_id}"))?;
    let entry = coll
        .latest_compatible(name, &ctx.unit)
        .with_context(|| format!("{name} {} is not compatible with this unit: {}", newest.version, ctx.unit.describe()))?
        .clone();
    let state = State::load(layout)?;
    let previous = state.packages.get(name).cloned();
    if let Some(p) = &previous {
        let recorded_present = crate::catalog::collections(layout).iter().any(|c| c.id() == p.collection);
        if p.collection != coll_id && recorded_present {
            bail!("{name} is installed from collection {}; remove it before installing it from {coll_id}", p.collection);
        }
        match cmp_versions(&entry.version, &p.version) {
            Ordering::Equal => {
                // Retry a hand-install migration that failed after an earlier commit.
                return match legacy::migrate(ctx, name)? {
                    Some(note) => {
                        control::request_reload(layout)?;
                        Ok(format!("{name} {} is already installed; {note}", p.version))
                    }
                    None => Ok(format!("{name} {} is already installed", p.version)),
                };
            }
            Ordering::Less => bail!(
                "the card has {name} {}, older than the installed {} (use Roll back instead)",
                entry.version,
                p.version
            ),
            Ordering::Greater => {}
        }
    }
    (ctx.progress)(&format!("checking {name} {}", entry.version));
    let archive = match coll.location(&entry) {
        crate::index::Location::Local(p) => p,
        crate::index::Location::Remote(url) => {
            (ctx.progress)(&format!("downloading {name} {}", entry.version));
            let bytes = ctx.fetcher.get(&url, crate::fetch::MAX_PACKAGE)?;
            let p = layout.run(&format!("dl.{name}-{}.mpk", entry.version));
            atomic_write(&p, &bytes)?;
            p
        }
    };
    // Downloads live in /tmp (RAM) only for this install, success or not.
    struct RemoveDownload(Option<PathBuf>);
    impl Drop for RemoveDownload {
        fn drop(&mut self) {
            if let Some(p) = &self.0 {
                let _ = fs::remove_file(p);
            }
        }
    }
    let _download = RemoveDownload(if coll.base_url.is_some() { Some(archive.clone()) } else { None });
    let (sha, size) = sha256_file(&archive).with_context(|| format!("reading {}", archive.display()))?;
    if size != entry.size || sha != entry.sha256 {
        bail!("{} is corrupt (size or sha256 does not match the index); copy it to the card again", archive.display());
    }
    let pkg = Package::read(&archive).with_context(|| format!("{} is corrupt", archive.display()))?;
    if pkg.manifest.package.name != name || pkg.manifest.package.version != entry.version {
        bail!(
            "{} holds {} {}, but the index says {name} {}",
            archive.display(),
            pkg.manifest.package.name,
            pkg.manifest.package.version,
            entry.version
        );
    }
    install_package(ctx, media, &pkg, &coll_id, &coll.index.collection.name)
}

/// Install or update to an already verified package: everything after "which file" is known.
/// Records `coll_id` as where future updates come from.
pub fn install_package(ctx: &Ctx, media: &[PathBuf], pkg: &Package, coll_id: &str, coll_name: &str) -> Result<String> {
    let layout = &ctx.layout;
    let name = pkg.manifest.package.name.as_str();
    let version = pkg.manifest.package.version.clone();
    let plugin_id = pkg.manifest.plugin_id().map(str::to_string);
    let mut state = State::load(layout)?;
    let previous = state.packages.get(name).cloned();
    // `current` already names this version but the state file doesn't: power was lost after
    // the commit point. Keep the active files and finish the bookkeeping (nothing to extract,
    // so no space check: the disk is at its fullest exactly now).
    let repair = current_version(layout, name).as_deref() == Some(version.as_str())
        && layout.pkg_dir(name).join(&version).join("manifest.toml").is_file();
    match ctx.sys.free_bytes(&layout.p("/mnt/app")) {
        _ if repair => {}
        Some(free) => {
            let need = pkg.unpacked_size() * 2 + 1024 * 1024;
            if free < need {
                bail!("not enough space on /mnt/app: {free} bytes free, {need} needed");
            }
        }
        None => (ctx.progress)("free space unknown (could not read df); continuing"),
    }

    let kind = pkg.manifest.kind();
    if kind == Kind::Sdk && !repair {
        // The guards run before /mnt/app goes read-write: a refusal changes nothing at all.
        sdk_pkg::check(ctx, &pkg.manifest, current_dir(layout, name).as_deref(), icons::Source::Package(pkg))?;
    }
    // A plugin's checks and approval, also before anything is written (NeedsApproval included).
    let approval = if kind == Kind::Plugin && !repair {
        let old = previous.as_ref().and_then(|_| current_manifest(layout, name).ok());
        Some(plugin_pkg::check(ctx, &pkg.manifest, old.as_ref(), icons::Source::Package(pkg))?)
    } else {
        None
    };

    let rw = ctx.mounts.rw("/mnt/app")?;
    cleanup_stale(layout)?;
    let pkg_dir = layout.pkg_dir(name);
    let ver_dir = pkg_dir.join(&version);
    let prev_dir = previous.as_ref().map(|p| pkg_dir.join(&p.version)).filter(|d| d.is_dir());
    let new_dir = pkg_dir.join(format!("{}.new", version));
    if !repair && ver_dir.exists() {
        // Newer than current, so never the active version: a leftover or the version a
        // rollback moved away from. Re-extract it fresh.
        fs::remove_dir_all(&ver_dir)?;
    }
    let mut cfg = config::Report::default();
    let mut sdk_notes = Vec::new();
    let mut plugin_notes = Vec::new();
    // Dropped before `rw` (declared after it): an error return undoes the plugin's deploy.
    let mut plugin_deployed = None;
    let staged = if repair {
        Ok(())
    } else {
        (ctx.progress)("unpacking");
        (|| -> Result<()> {
        pkg.extract_to(&new_dir)?;
        fs::rename(&new_dir, &ver_dir)?;
        if kind == Kind::Sdk {
            // From the staged copy just extracted: the jars verified are the jars written.
            sdk_notes = sdk_pkg::deploy(ctx, &pkg.manifest, &ver_dir, prev_dir.as_deref())?;
        }
        cfg = config::import(layout, media, &pkg.manifest, ctx.config_choice)?;
        if let Some(s) = &pkg.manifest.scripts.post_install {
            (ctx.progress)("running post_install");
            ctx.sys
                .run_script(&ver_dir.join(s), &[], &script_env(layout, name, &ver_dir, &version), script_timeout(&pkg.manifest))
                .context("post_install script failed")?;
        }
        if let Some(a) = &approval {
            // Last, so little can fail after it; until `current` is written, an error puts the jar
            // and grants line back. As for the SDK: verified and installed from the staged copy.
            plugin_deployed = Some(plugin_pkg::deploy(ctx, &pkg.manifest, &ver_dir, a)?);
        }
        Ok(())
        })()
    };
    if let Err(e) = staged {
        let _ = fs::remove_dir_all(&new_dir);
        let _ = fs::remove_dir_all(&ver_dir);
        return Err(e);
    }

    (ctx.progress)(&format!("switching {name} to {}", version));
    if repair && kind == Kind::Plugin {
        // `current` already names this version: make the unit run it too (an earlier run may have
        // failed before, or put another jar back); never grants more than is granted now.
        plugin_notes = plugin_pkg::redeploy(ctx, &pkg.manifest, &ver_dir)?;
    }
    atomic_write(&pkg_dir.join("current"), version.as_bytes())?; // commit point
    if let Some(d) = plugin_deployed.take() {
        // Committed: from here on, the jar and grants line belong to the version `current` names.
        plugin_notes = d.commit();
    }
    bootd::write_for_package(layout, name, &bootd::entries(&pkg.manifest))?;
    let old_version = previous.as_ref().map(|p| p.version.clone());
    // Only a version whose files are still here can be rolled back to (mibpkg itself,
    // registered by bootstrap, has none for its first version).
    let kept = old_version.clone().filter(|v| pkg_dir.join(v).is_dir());
    prune_versions(&pkg_dir, &version, kept.as_deref())?;
    state.packages.insert(
        name.to_string(),
        Installed {
            collection: coll_id.to_string(),
            version: version.clone(),
            previous: kept,
            services: pkg.manifest.services.iter().map(|s| format!("{name}.{}", s.name)).collect(),
            uses_scripts: pkg.manifest.uses_scripts(),
        },
    );
    state.collections.insert(coll_id.to_string(), coll_name.to_string());
    state.save(layout)?;

    let mut msg = match &old_version {
        _ if repair => format!("repaired {name} {} (an earlier install was interrupted)", version),
        Some(v) => format!("updated {name} {v} -> {}", version),
        None => format!("installed {name} {}", version),
    };
    if let Some(p) = previous.as_ref().filter(|p| p.collection != coll_id) {
        // The package moved: say so, it now comes from another publisher's collection.
        msg.push_str(&format!("; now from collection {coll_id} (was {})", p.collection));
    }
    let migrated = legacy::migrate(ctx, name);
    if migrated.is_err() {
        // The new version is committed: start it even if moving the old install aside failed.
        control::request_reload(layout)?;
    }
    if let Some(note) = migrated.context("migrating the old install failed; install again to retry")? {
        msg.push_str("; ");
        msg.push_str(&note);
    }
    if name == SELF {
        self_update(layout, &ver_dir)?;
        msg.push_str("; mibpkg restarts itself");
    }
    let cfg = cfg.summary(name);
    if !cfg.is_empty() {
        msg.push_str("; ");
        msg.push_str(&cfg);
    }
    let missing = config::missing_required(layout, &pkg.manifest);
    if !missing.is_empty() {
        msg.push_str(&format!(
            "; needs config: {} (put it on the SD card in mibpkg/config/{name}/, then Rescan)",
            missing.join(", ")
        ));
    }
    if kind == Kind::Sdk {
        msg.push_str(SDK_START);
        for n in &sdk_notes {
            msg.push('\n');
            msg.push_str(n);
        }
    }
    (ctx.progress)("starting services");
    if let Err(e) = control::request_reload(layout) {
        // Committed: the install stands; only the supervisor's pick-up of it is late.
        msg.push_str(&format!(
            "\nwarning: could not ask the mibpkg supervisor to reload ({e:#}); it picks {name} up when it next restarts"
        ));
    }
    if kind == Kind::Plugin {
        // No /mnt/app read-write guard is held while the runtime is waited for.
        drop(rw);
        let id = plugin_id.as_deref().context("plugin package without a plugin id")?;
        msg.push_str(&plugin_tail(ctx, id, &plugin_notes));
    }
    Ok(msg)
}

/// The deploy notes and the hot-load's note of a plugin, each on its own line.
fn plugin_tail(ctx: &Ctx, id: &str, notes: &[String]) -> String {
    let mut s: String = notes.iter().map(|n| format!("\n{n}")).collect();
    s.push('\n');
    s.push_str(&plugin_pkg::activate(ctx, id));
    s
}

/// Install or upgrade from a .mpk file the user supplied (a web upload), not from a collection.
/// An upgrade keeps the package's recorded collection; a fresh install records `chosen`: a loaded
/// collection id, or "-" for none (recorded as `uploaded`, which never offers updates).
pub fn install_file(
    ctx: &Ctx,
    media: &[PathBuf],
    colls: &[Collection],
    path: &Path,
    name: &str,
    chosen: Option<&str>,
) -> Result<String> {
    let pkg = Package::read(path).with_context(|| format!("{} is not a valid package", path.display()))?;
    let m = &pkg.manifest.package;
    if m.name != name {
        bail!("the file holds {} {}, not {name}", m.name, m.version);
    }
    if !pkg.manifest.compat.matches(&ctx.unit) {
        bail!("{name} {} is not compatible with this unit: {}", m.version, ctx.unit.describe());
    }
    let state = State::load(&ctx.layout)?;
    let (coll_id, coll_name) = match state.packages.get(name) {
        Some(p) => {
            match cmp_versions(&m.version, &p.version) {
                Ordering::Equal => return Ok(format!("{name} {} is already installed", p.version)),
                Ordering::Less => {
                    bail!("the file has {name} {}, older than the installed {} (use Roll back instead)", m.version, p.version)
                }
                Ordering::Greater => {}
            }
            let coll_name = state.collections.get(&p.collection).cloned().unwrap_or_else(|| p.collection.clone());
            (p.collection.clone(), coll_name)
        }
        None => match chosen {
            None => bail!("choose a collection for {name} (or \"-\" for none)"),
            Some("-") => (crate::index::RESERVED_COLLECTION.to_string(), "Uploaded file".to_string()),
            Some(id) => {
                let c = colls.iter().find(|c| c.id() == id).with_context(|| format!("collection {id} is not on any card or online"))?;
                (id.to_string(), c.index.collection.name.clone())
            }
        },
    };
    (ctx.progress)(&format!("checking {name} {} (uploaded file)", m.version));
    install_package(ctx, media, &pkg, &coll_id, &coll_name)
}

/// (name, newer version) for installed packages whose collection offers something newer.
pub fn updates_available(state: &State, colls: &[Collection], unit: &crate::unit::UnitInfo) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for (name, inst) in &state.packages {
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
        if let Some(e) = best {
            if cmp_versions(&e.version, &inst.version) == Ordering::Greater {
                out.push((name.clone(), e.version.clone()));
            }
        }
    }
    out
}

pub fn update_all(ctx: &Ctx, media: &[PathBuf], colls: &[Collection]) -> Result<Vec<String>> {
    let state = State::load(&ctx.layout)?;
    let mut todo = updates_available(&state, colls, &ctx.unit);
    if todo.is_empty() {
        return Ok(vec!["everything is up to date".into()]);
    }
    // SDK first: a plugin build may require the newer SDK, and the plugin check reads the
    // package that is installed rather than the runtime that is still running until the reboot.
    todo.sort_by_key(|(name, _)| if name == crate::mibsdk::SDK_PKG { 0 } else { 1 });
    let mut out = Vec::new();
    for (name, _) in todo {
        let coll = pick_collection(&state, colls, &name, None, &ctx.unit)?;
        out.push(match install(ctx, media, coll, &name) {
            Ok(m) => m,
            // Never approved on the user's behalf: the update waits for an explicit approval.
            Err(e) => match e.downcast_ref::<plugin_pkg::NeedsApproval>() {
                Some(n) => format!("{name}: needs permission approval ({})", n.ask.join(", ")),
                None => format!("{name}: FAILED {e:#}"),
            },
        });
    }
    Ok(out)
}

pub fn remove(ctx: &Ctx, name: &str, purge: bool) -> Result<String> {
    if name == SELF {
        bail!("mibpkg cannot remove itself this way; use `mibpkg uninstall-self`");
    }
    let layout = &ctx.layout;
    let mut state = State::load(layout)?;
    if !state.packages.contains_key(name) {
        bail!("{name} is not installed");
    }
    let manifest = current_manifest(layout, name);
    let is_plugin = manifest.as_ref().is_ok_and(|m| m.kind() == Kind::Plugin);
    // Unreadable manifest: if a plugin of that id is on the unit, say how to take it off.
    let plugin_left = manifest.is_err() && plugin_pkg::on_unit(layout, name);
    let rw = ctx.mounts.rw("/mnt/app")?;
    cleanup_stale(layout)?;
    // The SDK first puts the stock bundle list back (or refuses, changing nothing). A mibsdk whose
    // manifest cannot be read is refused there too, never removed as plain files.
    let sdk_note = if name == crate::mibsdk::SDK_PKG || current_manifest(layout, name).is_ok_and(|m| m.kind() == Kind::Sdk) {
        Some(sdk_pkg::undeploy(ctx, purge)?)
    } else {
        None
    };
    if let (Ok(m), Some(dir)) = (current_manifest(layout, name), current_dir(layout, name)) {
        if let Some(s) = &m.scripts.pre_remove {
            ctx.sys
                .run_script(&dir.join(s), &[], &script_env(layout, name, &dir, &m.package.version), script_timeout(&m))
                .context("pre_remove script failed; package left installed")?;
        }
    }
    (ctx.progress)(&format!("stopping {name} and removing its files"));
    bootd::remove_for_package(layout, name)?;
    control::request_reload(layout)?;
    match fs::remove_dir_all(layout.pkg_dir(name)) {
        Err(e) if e.kind() != ErrorKind::NotFound => return Err(e.into()),
        _ => {}
    }
    if purge {
        for d in [layout.config_dir(name), config::base_dir(layout, name)] {
            match fs::remove_dir_all(d) {
                Err(e) if e.kind() != ErrorKind::NotFound => return Err(e.into()),
                _ => {}
            }
        }
    }
    state.packages.remove(name);
    state.save(layout)?;
    if is_plugin {
        // The plugin itself: jar, grants line, switch and data, waiting for the runtime with no
        // /mnt/app read-write guard held. A combined product package removes the plugin by its
        // own id, not by the product name.
        let id = manifest.as_ref().ok().and_then(|m| m.plugin_id()).unwrap_or(name).to_string();
        drop(rw);
        return plugin_pkg::remove_package(ctx, &id, purge).with_context(|| {
            format!("the {name} package was removed, but taking the plugin {id} off the unit did not finish; run: mibpkg sdk remove {id}")
        });
    }
    let mut msg = format!("removed {name}{}", if purge { " and its config" } else { "" });
    if plugin_left {
        msg.push_str(&format!(
            "\nthe manifest of {name} could not be read, but its plugin is still on the unit; to take it off, run: mibpkg sdk remove {name}"
        ));
    }
    if sdk_note.is_some() {
        // Only now: until the package is recorded as removed, a rerun needs the stock backups.
        if let Err(e) = sdk_pkg::forget_backups(ctx) {
            msg.push_str(&format!(" (warning: could not drop the stock backups: {e:#})"));
        }
    }
    Ok(match sdk_note {
        // Warnings (one per line) come first, the note last.
        Some(note) => match note.rsplit_once('\n') {
            Some((warnings, last)) => format!("{warnings}\n{msg}; {last}{SDK_REMOVED}"),
            None => format!("{msg}; {note}{SDK_REMOVED}"),
        },
        None => msg,
    })
}

pub fn rollback(ctx: &Ctx, name: &str) -> Result<String> {
    let layout = &ctx.layout;
    let mut state = State::load(layout)?;
    let inst = state.packages.get(name).cloned().with_context(|| format!("{name} is not installed"))?;
    let prev = inst.previous.clone().with_context(|| format!("no earlier version of {name} is kept"))?;
    let current = current_dir(layout, name).with_context(|| format!("{name} has no active version"))?;
    if !crate::requires::read_dir(&current)?.rollback_supported() {
        bail!("{name} {} does not support rollback; remove it, then install the older package", inst.version);
    }
    let prev_dir = layout.pkg_dir(name).join(&prev);
    let m = Manifest::parse(
        &fs::read_to_string(prev_dir.join("manifest.toml"))
            .with_context(|| format!("{name} {prev} is no longer on the unit"))?,
    )?;
    let kind = m.kind();
    let mut sdk_notes = Vec::new();
    let mut plugin_notes = Vec::new();
    if kind == Kind::Sdk {
        sdk_pkg::check(ctx, &m, current_dir(layout, name).as_deref(), icons::Source::Dir(&prev_dir))?;
    }
    // Grants never grow on a rollback: what the current version declared and is granted, of what
    // the previous one declares; a permission only the previous one declares needs an approval.
    let approval = if kind == Kind::Plugin {
        Some(plugin_pkg::check(ctx, &m, current_manifest(layout, name).ok().as_ref(), icons::Source::Dir(&prev_dir))?)
    } else {
        None
    };
    let rw = ctx.mounts.rw("/mnt/app")?;
    cleanup_stale(layout)?;
    if kind == Kind::Sdk {
        // On failure this deploys the current version again; `current` is not switched.
        sdk_notes = sdk_pkg::deploy(ctx, &m, &prev_dir, current_dir(layout, name).as_deref())?;
    }
    // Until `current` is switched, an error puts the current jar and grants line back.
    let plugin_deployed = match &approval {
        Some(a) => Some(plugin_pkg::deploy(ctx, &m, &prev_dir, a)?),
        None => None,
    };
    (ctx.progress)(&format!("switching {name} back to {prev}"));
    atomic_write(&layout.pkg_dir(name).join("current"), prev.as_bytes())?;
    if let Some(d) = plugin_deployed {
        // Committed (`current` names the previous version now); a re-run finishes the rest.
        plugin_notes = d.commit();
    }
    bootd::write_for_package(layout, name, &bootd::entries(&m))?;
    if name == SELF {
        self_update(layout, &prev_dir)?;
    }
    let entry = state.packages.get_mut(name).unwrap();
    entry.version = prev.clone();
    entry.previous = Some(inst.version.clone());
    entry.services = m.services.iter().map(|s| format!("{name}.{}", s.name)).collect();
    entry.uses_scripts = m.uses_scripts();
    state.save(layout)?;
    control::request_reload(layout)?;
    let suffix = if kind == Kind::Sdk { SDK_START } else { "" };
    let notes: String = sdk_notes.iter().map(|n| format!("\n{n}")).collect();
    let mut msg = format!("rolled back {name} {} -> {prev}{suffix}{notes}", inst.version);
    if kind == Kind::Plugin {
        drop(rw);
        let id = m.plugin_id().context("plugin package without a plugin id")?;
        msg.push_str(&plugin_tail(ctx, id, &plugin_notes));
    }
    Ok(msg)
}
