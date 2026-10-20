//! `[plugin]` packages: the install checks and permission approval, the deploy to
//! `BASE/plugins/<id>.jar`, the hot-load through a reload marker, and the one removal path for
//! packages and `mibsdk-dev push`ed ids. A port of `mibsdk-dev`'s `cmd_push`, `cmd_grant` and
//! `cmd_remove`, writing through `put`.
use super::grants::{self, Approval};
use super::sdk_pkg::on_app;
use super::{ack, catalog, ledger, stamp, toggles};
use super::{installed_sdk, put, put_file, sdk_running, BASE, PERMISSIONS, PERSIST};
use crate::ctx::Ctx;
use crate::manifest::Manifest;
use crate::state::State;
use anyhow::{anyhow, bail, Context, Result};
use std::collections::BTreeSet;
use std::fmt;
use std::fs;
use std::io;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const JAR_MODE: u32 = 0o644;
/// `mibsdk-dev remove`'s default wait for the ledger to drain.
const REMOVE_WAIT_DEFAULT: Duration = Duration::from_secs(30);
const REMOVE_WAIT_ENV: &str = "MIBPKG_SDK_REMOVE_WAIT_S";
const LEDGER_POLL: Duration = Duration::from_secs(2);

/// A plugin install or update that adds permissions the action's approval list does not cover.
/// Nothing was changed; callers find it with `downcast_ref::<NeedsApproval>()`.
#[derive(Debug)]
pub struct NeedsApproval {
    pub package: String,
    pub version: String,
    /// The permissions to approve, in declared order.
    pub ask: Vec<String>,
    /// One line per `ask` entry, in plain words (`catalog::describe`).
    pub lines: Vec<String>,
}

impl fmt::Display for NeedsApproval {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "approve these permissions to install {} {}: {}", self.package, self.version, self.lines.join("; "))
    }
}

impl std::error::Error for NeedsApproval {}

fn plugins_dir() -> String {
    format!("{BASE}/plugins")
}

fn jar_abs(id: &str) -> String {
    format!("{BASE}/plugins/{id}.jar")
}

fn reload_abs(id: &str) -> String {
    format!("{BASE}/plugins/{id}.reload")
}

fn data_abs(id: &str) -> String {
    format!("{PERSIST}/data/{id}.properties")
}

/// Whether `id` may be removed: as `mibsdk-dev`'s `check_id` (`[A-Za-z0-9][A-Za-z0-9._-]*`), so
/// a pushed id the runtime accepts can always be taken off.
fn check_id(id: &str) -> Result<()> {
    let b = id.as_bytes();
    if b.is_empty() || !b[0].is_ascii_alphanumeric() || !b.iter().all(|c| c.is_ascii_alphanumeric() || b"._-".contains(c)) {
        bail!("invalid plugin id '{id}'");
    }
    Ok(())
}

/// The install checks, before anything is written (nothing here writes): the SDK is installed,
/// supports the plugin's api and this unit's HMI, and knows every declared permission. Then the
/// approval: what the plugin adds over `old` (the installed package's manifest, or None: every
/// declared permission) must be covered by `ctx.approved`, else `NeedsApproval`. A permission the
/// user revoked stays revoked; a jar pushed for the same id is no refusal (`old` is None for it).
pub fn check(ctx: &Ctx, m: &Manifest, old: Option<&Manifest>) -> Result<Approval> {
    let plugin = m.plugin.as_ref().context("not a [plugin] package")?;
    let id = m.package.name.as_str();
    let l = &ctx.layout;
    let Some((_, sdk, _)) = installed_sdk(l) else {
        bail!("{id} is a plugin for the mibsdk runtime: install the mibsdk package first");
    };
    if !(sdk.min_api <= plugin.api && plugin.api <= sdk.api) {
        bail!("{id} needs plugin api {}; the installed SDK supports {}..{}", plugin.api, sdk.min_api, sdk.api);
    }
    let hmi = crate::unit::hmi_version(l);
    if !hmi.as_ref().is_some_and(|v| sdk.hmi.contains(v)) {
        bail!("the SDK does not support this firmware ({}), so no plugin would load", hmi.as_deref().unwrap_or("unknown"));
    }
    // Without a readable catalog every permission is unknown: the check fails closed.
    let cat = catalog::installed(l);
    let known = |p: &String| matches!(&cat, Ok(Some(c)) if c.iter().any(|i| &i.id == p));
    let unknown: Vec<&str> = plugin.permissions.iter().filter(|p| !known(p)).map(String::as_str).collect();
    if !unknown.is_empty() {
        let why = match &cat {
            Ok(Some(_)) => String::new(),
            Ok(None) => format!(" ({PERMISSIONS} is missing)"),
            Err(e) => format!(" ({e:#})"),
        };
        bail!("{id} asks for permissions the installed SDK does not know: {}{why}", unknown.join(", "));
    }
    let current = grants::read(l)?.remove(id).unwrap_or_default();
    let declared_old = old.and_then(|o| o.plugin.as_ref()).map(|p| p.permissions.as_slice());
    let approval = grants::plan(&plugin.permissions, declared_old, &current);
    let approved = ctx.approved.as_deref().unwrap_or(&[]);
    if !approval.ask.iter().all(|p| approved.contains(p)) {
        let cat = cat.ok().flatten();
        return Err(NeedsApproval {
            package: id.to_string(),
            version: m.package.version.clone(),
            lines: catalog::describe(cat.as_deref(), &approval.ask),
            ask: approval.ask,
        }
        .into());
    }
    Ok(approval)
}

/// Create BASE/plugins if it is missing. The caller holds /mnt/app read-write.
fn ensure_plugins_dir(ctx: &Ctx) -> Result<()> {
    let dir = ctx.layout.p(&plugins_dir());
    if dir.is_dir() {
        return Ok(());
    }
    on_app(ctx, &format!("cannot create {}", plugins_dir()), &mut || fs::create_dir_all(&dir))
}

fn remove_if_present(p: &Path) -> io::Result<()> {
    match fs::remove_file(p) {
        Err(e) if e.kind() != io::ErrorKind::NotFound => Err(e),
        _ => Ok(()),
    }
}

/// Install the plugin's jar from `ver_dir` (the staged copy, verified against its stamp there)
/// to `BASE/plugins/<id>.jar` and set its grants line to `approval.grant`. If the grants line
/// cannot be written, the jar that was there before is put back. Returns the notes: the
/// unrestricted warning, a takeover of a pushed jar, and a plugin left blocked by missing grants.
///
/// Caller contract: holds the action lock and `ctx.mounts.rw("/mnt/app")`.
pub fn deploy(ctx: &Ctx, m: &Manifest, ver_dir: &Path, approval: &Approval) -> Result<Vec<String>> {
    let plugin = m.plugin.as_ref().context("not a [plugin] package")?;
    let id = m.package.name.as_str();
    let l = &ctx.layout;
    let jar = ver_dir.join(&plugin.jar);
    let verified = stamp::verify(&ver_dir.join(format!("{}.stamp", plugin.jar)), stamp::StampKind::Plugin, Some(id), &[&jar])
        .map_err(|e| anyhow!(stamp::plugin_refusal(id, &format!("{e:#}"))))?;
    let target = jar_abs(id);
    let prior = match fs::read(l.p(&target)) {
        Ok(b) => Some(b),
        Err(e) if e.kind() == io::ErrorKind::NotFound => None,
        Err(e) => return Err(e).with_context(|| format!("reading {target}")),
    };
    let takeover = prior.is_some() && !State::load(l)?.packages.contains_key(id);
    ensure_plugins_dir(ctx)?;
    put_file(ctx, &target, &jar, JAR_MODE)?;
    if let Err(e) = grants::set(ctx, id, &approval.grant) {
        let undo = match &prior {
            Some(b) => put(ctx, &target, b, JAR_MODE),
            None => {
                let p = l.p(&target);
                on_app(ctx, &format!("cannot remove {target}"), &mut || remove_if_present(&p))
            }
        };
        return Err(match undo {
            Ok(()) => e.context(format!("{id}.jar was put back as it was")),
            Err(u) => e.context(format!("putting {target} back failed too ({u:#})")),
        });
    }
    let mut notes = Vec::new();
    notes.extend(verified.warning(id));
    if takeover {
        notes.push(format!("replaced the pushed {id}.jar"));
    }
    let missing = grants::missing(&plugin.permissions, &approval.grant);
    if !missing.is_empty() {
        notes.push(format!(
            "{id} stays blocked until granted: {}; grant with: mibpkg sdk grant {id} {}",
            missing.join(", "),
            // Every declared permission (once each): the runtime runs the plugin only with all.
            grants::missing(&plugin.permissions, &BTreeSet::new()).join(",")
        ));
    }
    Ok(notes)
}

static TOKEN_SEQ: AtomicU64 = AtomicU64::new(0);

/// A reload marker's content: new on every request (the runtime reloads once per new content and
/// never deletes the marker).
fn token() -> String {
    let secs = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    format!("{secs}-{}-{}\n", std::process::id(), TOKEN_SEQ.fetch_add(1, Ordering::Relaxed))
}

/// Put a fresh token into `plugins/<id>.reload`. The caller holds /mnt/app read-write.
fn put_marker(ctx: &Ctx, id: &str) -> Result<()> {
    ensure_plugins_dir(ctx)?;
    put(ctx, &reload_abs(id), token().as_bytes(), JAR_MODE)
}

fn seconds(d: Duration) -> String {
    if d.subsec_nanos() == 0 {
        format!("{} s", d.as_secs())
    } else {
        format!("{} s", d.as_secs_f64())
    }
}

/// `activate_with` the usual wait (`ack::ACK_TIMEOUT`).
pub fn activate(ctx: &Ctx, id: &str) -> String {
    activate_with(ctx, id, ack::ACK_TIMEOUT)
}

/// Ask the running SDK to (re)load `id` and wait up to `timeout` for its answer; returns the note
/// for the result. /mnt/app is held read-write only to write the marker, never while waiting, so
/// call this after dropping any guard of your own.
pub fn activate_with(ctx: &Ctx, id: &str, timeout: Duration) -> String {
    if !sdk_running(&ctx.layout) {
        return "the SDK is not running; it loads at the next boot".into();
    }
    let before = ack::count(&ctx.layout, id);
    let requested = (|| -> Result<()> {
        let _rw = ctx.mounts.rw("/mnt/app")?;
        put_marker(ctx, id)
    })();
    if let Err(e) = requested {
        return format!("could not request a reload ({e:#}); it loads at the next HMI start");
    }
    match ack::wait(&ctx.layout, id, before, timeout) {
        Some((state, reason)) => format!("loaded: {state} {reason}").trim_end().to_string(),
        None => format!("reload requested; the SDK did not confirm within {} (it loads at the next HMI start)", seconds(timeout)),
    }
}

/// `MIBPKG_SDK_REMOVE_WAIT_S` as `mibsdk-dev` reads `MIBSDK_REMOVE_WAIT_S`: digits only, else 30.
#[doc(hidden)]
pub fn parse_remove_wait(v: Option<&str>) -> Duration {
    match v {
        Some(s) if !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()) => {
            s.parse().map(Duration::from_secs).unwrap_or(REMOVE_WAIT_DEFAULT)
        }
        _ => REMOVE_WAIT_DEFAULT,
    }
}

/// How long a removal waits for the ledger to drain.
pub fn remove_wait() -> Duration {
    parse_remove_wait(std::env::var(REMOVE_WAIT_ENV).ok().as_deref())
}

/// Take plugin `id` off the unit: the one removal path for plugin packages (`install::remove`
/// calls it after its own bookkeeping) and for pushed ids (`mibpkg sdk remove`). A port of
/// `mibsdk-dev remove`; a re-run after a partial failure finishes it (M2). Plugin data is kept
/// unless `purge`, and then deleted only once the runtime has confirmed the unload (or is not
/// running).
///
/// Caller contract: holds the action lock and no /mnt/app read-write guard.
pub fn remove_id(ctx: &Ctx, id: &str, purge: bool) -> Result<String> {
    remove_id_with(ctx, id, purge, ack::ACK_TIMEOUT, remove_wait())
}

/// `remove_id` with explicit waits for the runtime's answer and for the ledger.
#[doc(hidden)]
pub fn remove_id_with(ctx: &Ctx, id: &str, purge: bool, ack_timeout: Duration, ledger_wait: Duration) -> Result<String> {
    remove(ctx, id, purge, ack_timeout, ledger_wait, false)
}

/// `remove_id` for an installed package: it is removed even when nothing of it is left on the unit.
pub(crate) fn remove_package(ctx: &Ctx, id: &str, purge: bool) -> Result<String> {
    remove(ctx, id, purge, ack::ACK_TIMEOUT, remove_wait(), true)
}

fn remove(ctx: &Ctx, id: &str, purge: bool, ack_timeout: Duration, ledger_wait: Duration, known: bool) -> Result<String> {
    check_id(id)?;
    let l = &ctx.layout;
    let (jar, data, marker) = (jar_abs(id), data_abs(id), reload_abs(id));
    // A reload marker or ledger lines count too: a run that failed after removing the jar and the
    // grants line leaves only those, and its re-run must still request the reload.
    let entries = ledger::read(l)?;
    let present = known
        || l.p(&jar).exists()
        || l.p(&data).exists()
        || grants::has_line(l, id)?
        || toggles::has_line(l, id)
        || l.p(&marker).exists()
        || !ledger::of(&entries, id).is_empty();
    if !present {
        bail!("{id} is not installed");
    }
    let running = sdk_running(l);
    let before = ack::count(l, id);
    let off_dropped = {
        let _rw = ctx.mounts.rw("/mnt/app")?;
        let p = l.p(&jar);
        on_app(ctx, &format!("cannot remove {jar}"), &mut || remove_if_present(&p))?;
        if fs::symlink_metadata(&p).is_ok() {
            bail!("{jar} is still there after removal");
        }
        grants::drop_line(ctx, id)?;
        // Before the marker: the reload must see the plugin switched on again.
        let off_dropped = toggles::drop(ctx, id)?;
        put_marker(ctx, id)?;
        off_dropped
    };
    let mut out = vec![format!(
        "removed {id}: plugins/{id}.jar and its grants line{}; reload requested",
        if off_dropped { format!(" and its {id}.off switch") } else { String::new() }
    )];
    let acked = running && ack::wait(l, id, before, ack_timeout).is_some();

    let start = Instant::now();
    let mut first = true;
    loop {
        let left: Vec<String> = match ledger::read(l) {
            Ok(e) => ledger::of(&e, id).iter().map(|e| e.line.clone()).collect(),
            Err(e) => {
                out.push(format!("WARNING: cannot read the ledger to check the overrides of {id} ({e:#}); check /tmp/mibsdk.log"));
                break;
            }
        };
        if left.is_empty() {
            out.push(if first { format!("{id} has no overrides in the ledger") } else { format!("overrides of {id} restored") });
            break;
        }
        first = false;
        let waited = start.elapsed();
        if waited >= ledger_wait {
            out.push(format!("WARNING: these overrides of {id} are not restored yet:"));
            out.extend(left);
            out.push("they are restored when the SDK next starts (the plugin is no longer installed); check /tmp/mibsdk.log".into());
            break;
        }
        thread::sleep(LEDGER_POLL.min(ledger_wait - waited));
    }

    if purge {
        if acked || !running {
            for abs in [data.clone(), format!("{data}.tmp")] {
                remove_if_present(&l.p(&abs)).with_context(|| format!("cannot remove {abs}"))?;
            }
            out.push(format!("cleared the data of {id} ({data})"));
            out.push(format!(
                "Restart the HMI now: a running {id} may write its data back, and the storage cap frees the space only from the next runtime start."
            ));
        } else {
            out.push(format!("data kept: the SDK did not confirm the unload; run `mibpkg sdk data-clear {id}` after a restart"));
        }
    }
    Ok(out.join("\n"))
}
