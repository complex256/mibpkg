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
use crate::manifest::Manifest;
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

pub fn pick_collection<'c>(
    state: &State,
    colls: &'c [Collection],
    name: &str,
    id: Option<&str>,
) -> Result<&'c Collection> {
    let want = id.map(String::from).or_else(|| state.packages.get(name).map(|i| i.collection.clone()));
    if let Some(want) = want {
        return colls
            .iter()
            .find(|c| c.id() == want)
            .with_context(|| format!("collection {want} is not on any card"));
    }
    let having: Vec<&Collection> = colls.iter().filter(|c| c.latest(name).is_some()).collect();
    match having.len() {
        0 => bail!("{name} is not in any collection on the card"),
        1 => Ok(having[0]),
        _ => bail!(
            "{name} is in several collections ({}); choose one",
            having.iter().map(|c| c.id()).collect::<Vec<_>>().join(", ")
        ),
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
    let mut state = State::load(layout)?;
    let previous = state.packages.get(name).cloned();
    if let Some(p) = &previous {
        if p.collection != coll_id {
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
    let archive = coll.archive_path(&entry);
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
    // `current` already names this version but the state file doesn't: power was lost after
    // the commit point. Keep the active files and finish the bookkeeping (nothing to extract,
    // so no space check: the disk is at its fullest exactly now).
    let repair = current_version(layout, name).as_deref() == Some(entry.version.as_str())
        && layout.pkg_dir(name).join(&entry.version).join("manifest.toml").is_file();
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

    let _rw = ctx.mounts.rw("/mnt/app")?;
    cleanup_stale(layout)?;
    let pkg_dir = layout.pkg_dir(name);
    let ver_dir = pkg_dir.join(&entry.version);
    let new_dir = pkg_dir.join(format!("{}.new", entry.version));
    if !repair && ver_dir.exists() {
        // Newer than current, so never the active version: a leftover or the version a
        // rollback moved away from. Re-extract it fresh.
        fs::remove_dir_all(&ver_dir)?;
    }
    let staged = if repair {
        Ok(())
    } else {
        (ctx.progress)("unpacking");
        (|| -> Result<()> {
        pkg.extract_to(&new_dir)?;
        fs::rename(&new_dir, &ver_dir)?;
        config::import(layout, media, &pkg.manifest)?;
        if let Some(s) = &pkg.manifest.scripts.post_install {
            (ctx.progress)("running post_install");
            ctx.sys
                .run_script(&ver_dir.join(s), &[], &script_env(layout, name, &ver_dir, &entry.version), SCRIPT_TIMEOUT)
                .context("post_install script failed")?;
        }
        Ok(())
        })()
    };
    if let Err(e) = staged {
        let _ = fs::remove_dir_all(&new_dir);
        let _ = fs::remove_dir_all(&ver_dir);
        return Err(e);
    }

    (ctx.progress)(&format!("switching {name} to {}", entry.version));
    atomic_write(&pkg_dir.join("current"), entry.version.as_bytes())?; // commit point
    bootd::write_for_package(layout, name, &bootd::entries(&pkg.manifest))?;
    let old_version = previous.as_ref().map(|p| p.version.clone());
    // Only a version whose files are still here can be rolled back to (mibpkg itself,
    // registered by bootstrap, has none for its first version).
    let kept = old_version.clone().filter(|v| pkg_dir.join(v).is_dir());
    prune_versions(&pkg_dir, &entry.version, kept.as_deref())?;
    state.packages.insert(
        name.to_string(),
        Installed {
            collection: coll_id.clone(),
            version: entry.version.clone(),
            previous: kept,
            services: pkg.manifest.services.iter().map(|s| format!("{name}.{}", s.name)).collect(),
            uses_scripts: pkg.manifest.uses_scripts(),
        },
    );
    state.collections.insert(coll_id, coll.index.collection.name.clone());
    state.save(layout)?;

    let mut msg = match &old_version {
        _ if repair => format!("repaired {name} {} (an earlier install was interrupted)", entry.version),
        Some(v) => format!("updated {name} {v} -> {}", entry.version),
        None => format!("installed {name} {}", entry.version),
    };
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
    let missing = config::missing_required(layout, &pkg.manifest);
    if !missing.is_empty() {
        msg.push_str(&format!(
            "; needs config: {} (put it on the SD card in mibpkg/config/{name}/, then Rescan)",
            missing.join(", ")
        ));
    }
    (ctx.progress)("starting services");
    control::request_reload(layout)?;
    Ok(msg)
}

/// (name, newer version) for installed packages whose collection offers something newer.
pub fn updates_available(state: &State, colls: &[Collection], unit: &crate::unit::UnitInfo) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for (name, inst) in &state.packages {
        if let Some(c) = colls.iter().find(|c| c.id() == inst.collection) {
            if let Some(e) = c.latest_compatible(name, unit) {
                if cmp_versions(&e.version, &inst.version) == Ordering::Greater {
                    out.push((name.clone(), e.version.clone()));
                }
            }
        }
    }
    out
}

pub fn update_all(ctx: &Ctx, media: &[PathBuf], colls: &[Collection]) -> Result<Vec<String>> {
    let state = State::load(&ctx.layout)?;
    let todo = updates_available(&state, colls, &ctx.unit);
    if todo.is_empty() {
        return Ok(vec!["everything is up to date".into()]);
    }
    let mut out = Vec::new();
    for (name, _) in todo {
        let coll = pick_collection(&state, colls, &name, None)?;
        out.push(match install(ctx, media, coll, &name) {
            Ok(m) => m,
            Err(e) => format!("{name}: FAILED {e:#}"),
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
    let _rw = ctx.mounts.rw("/mnt/app")?;
    cleanup_stale(layout)?;
    if let (Ok(m), Some(dir)) = (current_manifest(layout, name), current_dir(layout, name)) {
        if let Some(s) = &m.scripts.pre_remove {
            ctx.sys
                .run_script(&dir.join(s), &[], &script_env(layout, name, &dir, &m.package.version), SCRIPT_TIMEOUT)
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
        match fs::remove_dir_all(layout.config_dir(name)) {
            Err(e) if e.kind() != ErrorKind::NotFound => return Err(e.into()),
            _ => {}
        }
    }
    state.packages.remove(name);
    state.save(layout)?;
    Ok(format!("removed {name}{}", if purge { " and its config" } else { "" }))
}

pub fn rollback(ctx: &Ctx, name: &str) -> Result<String> {
    let layout = &ctx.layout;
    let mut state = State::load(layout)?;
    let inst = state.packages.get(name).cloned().with_context(|| format!("{name} is not installed"))?;
    let prev = inst.previous.clone().with_context(|| format!("no earlier version of {name} is kept"))?;
    let prev_dir = layout.pkg_dir(name).join(&prev);
    let m = Manifest::parse(
        &fs::read_to_string(prev_dir.join("manifest.toml"))
            .with_context(|| format!("{name} {prev} is no longer on the unit"))?,
    )?;
    let _rw = ctx.mounts.rw("/mnt/app")?;
    cleanup_stale(layout)?;
    (ctx.progress)(&format!("switching {name} back to {prev}"));
    atomic_write(&layout.pkg_dir(name).join("current"), prev.as_bytes())?;
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
    Ok(format!("rolled back {name} {} -> {prev}", inst.version))
}
