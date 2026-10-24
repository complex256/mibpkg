//! `[plugin]` packages: the install checks and permission approval, the deploy to
//! `BASE/plugins/<id>.jar`, the hot-load through a reload marker, and the one removal path for
//! packages and `mibsdk-dev push`ed ids. A port of `mibsdk-dev`'s `cmd_push`, `cmd_grant` and
//! `cmd_remove`, writing through `put`.
use super::grants::{self, Approval};
use super::sdk_pkg::on_app;
use super::{ack, catalog, ledger, stamp, toggles};
use super::{installed_sdk, put, put_file, BASE, LOG, PERMISSIONS, PERSIST};
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

/// Whether `id` may be pushed, removed or have its data cleared: as `mibsdk-dev`'s `check_id`
/// (`[A-Za-z0-9][A-Za-z0-9._-]*`), so a pushed id the runtime accepts can always be taken off.
pub fn check_id(id: &str) -> Result<()> {
    let b = id.as_bytes();
    if b.is_empty() || !b[0].is_ascii_alphanumeric() || !b.iter().all(|c| c.is_ascii_alphanumeric() || b"._-".contains(c)) {
        bail!("invalid plugin id '{id}'");
    }
    Ok(())
}

/// Plugin `id`'s jar or grants line is on the unit.
pub fn on_unit(layout: &crate::paths::Layout, id: &str) -> bool {
    check_id(id).is_ok() && (layout.p(&jar_abs(id)).exists() || grants::has_line(layout, id).unwrap_or(true))
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

/// What a deploy replaced: the jar and the grants line before it (None: there was none).
struct Prior {
    jar: Option<Vec<u8>>,
    grants: Option<BTreeSet<String>>,
}

/// Put the jar and the grants line of `id` back as `prior` holds them. The caller holds
/// /mnt/app read-write.
fn restore(ctx: &Ctx, id: &str, prior: &Prior) -> Result<()> {
    let target = jar_abs(id);
    match &prior.jar {
        Some(b) => put(ctx, &target, b, JAR_MODE)?,
        None => {
            let p = ctx.layout.p(&target);
            on_app(ctx, &format!("cannot remove {target}"), &mut || remove_if_present(&p))?;
        }
    }
    match &prior.grants {
        Some(g) => grants::set(ctx, id, g),
        None => grants::drop_line(ctx, id).map(|_| ()),
    }
}

/// A plugin deployed by an install or rollback whose `current` is not written yet. Dropped without
/// `commit`, it puts the jar and the grants line back as they were (an error after the deploy
/// must not leave the unit on a version the package state does not record). Keep it alive while
/// /mnt/app is held read-write.
pub struct Deployed<'c, 'a> {
    ctx: &'c Ctx<'a>,
    id: String,
    prior: Option<Prior>,
    notes: Vec<String>,
}

impl Deployed<'_, '_> {
    /// `current` now names the deployed version: keep the deploy; its notes.
    pub fn commit(mut self) -> Vec<String> {
        self.prior = None;
        std::mem::take(&mut self.notes)
    }
}

impl Drop for Deployed<'_, '_> {
    fn drop(&mut self) {
        let Some(prior) = self.prior.take() else { return };
        match restore(self.ctx, &self.id, &prior) {
            Ok(()) => (self.ctx.progress)(&format!("put {}.jar and its grants line back as they were", self.id)),
            Err(e) => crate::warn!("warning: could not put {}.jar and its grants line back ({e:#}); run: mibpkg sdk remove {} or install it again", self.id, self.id),
        }
    }
}

/// Install the plugin's jar from `ver_dir` (the staged copy, verified against its stamp there)
/// to `BASE/plugins/<id>.jar` and set its grants line to `approval.grant`. If the grants line
/// cannot be written, the jar and line are put back at once; afterwards, until `commit`, by
/// dropping the result. Its notes: the unrestricted warning, a takeover of a pushed jar, and a
/// plugin left blocked by missing grants.
///
/// Caller contract: holds the action lock and `ctx.mounts.rw("/mnt/app")`.
pub fn deploy<'c, 'a>(ctx: &'c Ctx<'a>, m: &Manifest, ver_dir: &Path, approval: &Approval) -> Result<Deployed<'c, 'a>> {
    let plugin = m.plugin.as_ref().context("not a [plugin] package")?;
    let id = m.package.name.as_str();
    let l = &ctx.layout;
    let jar = ver_dir.join(&plugin.jar);
    let verified = stamp::verify(&ver_dir.join(format!("{}.stamp", plugin.jar)), stamp::StampKind::Plugin, Some(id), &[&jar])
        .map_err(|e| anyhow!(stamp::plugin_refusal(id, &format!("{e:#}"))))?;
    let target = jar_abs(id);
    let prior = Prior {
        jar: match fs::read(l.p(&target)) {
            Ok(b) => Some(b),
            Err(e) if e.kind() == io::ErrorKind::NotFound => None,
            Err(e) => return Err(e).with_context(|| format!("reading {target}")),
        },
        grants: grants::read(l)?.remove(id),
    };
    let takeover = prior.jar.is_some() && !State::load(l)?.packages.contains_key(id);
    ensure_plugins_dir(ctx)?;
    put_file(ctx, &target, &jar, JAR_MODE)?;
    if let Err(e) = grants::set(ctx, id, &approval.grant) {
        return Err(match restore(ctx, id, &prior) {
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
    Ok(Deployed { ctx, id: id.to_string(), prior: Some(prior), notes })
}

/// The repair of an install that committed (`current` names the version in `ver_dir`) but did not
/// finish: deploy that version's jar again (stamp-verified, idempotent), with the grants line
/// limited to what it declares and is granted now (a repair never asks and never adds). Its
/// notes. Caller contract as `deploy`.
pub fn redeploy(ctx: &Ctx, m: &Manifest, ver_dir: &Path) -> Result<Vec<String>> {
    let plugin = m.plugin.as_ref().context("not a [plugin] package")?;
    let current = grants::read(&ctx.layout)?.remove(&m.package.name).unwrap_or_default();
    let approval = grants::plan(&plugin.permissions, Some(&plugin.permissions), &current);
    Ok(deploy(ctx, m, ver_dir, &approval)?.commit())
}

static TOKEN_SEQ: AtomicU64 = AtomicU64::new(0);

/// A reload marker's content: new on every request (the runtime reloads once per new content and
/// never deletes the marker). Printable ASCII without spaces, under 64 chars, then a line break, so
/// the token the runtime logs for it (`ack::token_of`) is exactly the text before the break.
fn token() -> String {
    let secs = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    format!("{secs}-{}-{}\n", std::process::id(), TOKEN_SEQ.fetch_add(1, Ordering::Relaxed))
}

/// Put a fresh token into `plugins/<id>.reload`; the token. The caller holds /mnt/app read-write.
fn put_marker(ctx: &Ctx, id: &str) -> Result<String> {
    ensure_plugins_dir(ctx)?;
    let t = token();
    put(ctx, &reload_abs(id), t.as_bytes(), JAR_MODE)?;
    Ok(t)
}

fn seconds(d: Duration) -> String {
    if d.subsec_nanos() == 0 {
        format!("{} s", d.as_secs())
    } else {
        format!("{} s", d.as_secs_f64())
    }
}

/// The SDK runtime started this boot (its log exists), reboot pending or not: it may be running a
/// plugin and acts on its reload marker.
fn runtime_up(l: &crate::paths::Layout) -> bool {
    l.p(LOG).exists()
}

/// `activate_with` the usual wait (`ack::ACK_TIMEOUT`).
pub fn activate(ctx: &Ctx, id: &str) -> String {
    activate_with(ctx, id, ack::ACK_TIMEOUT)
}

/// Ask the running SDK to (re)load `id` and wait up to `timeout` for its answer; returns the note
/// for the result. /mnt/app is held read-write only to write the marker, never while waiting, so
/// call this after dropping any guard of your own.
pub fn activate_with(ctx: &Ctx, id: &str, timeout: Duration) -> String {
    // The runtime started this boot (as in `remove`): it runs the plugin now even while a reboot
    // is pending, so a revoke or an update must reach it.
    if !runtime_up(&ctx.layout) {
        return "the SDK is not running; it loads at the next boot".into();
    }
    let base = ack::baseline(&ctx.layout);
    let requested = (|| -> Result<String> {
        let _rw = ctx.mounts.rw("/mnt/app")?;
        put_marker(ctx, id)
    })();
    let token = match requested {
        Ok(t) => t,
        Err(e) => return format!("could not request a reload ({e:#}); it loads at the next HMI start"),
    };
    await_load(ctx, id, &base, &token, timeout)
}

/// The note for the runtime's answer to the reload request `token`.
fn await_load(ctx: &Ctx, id: &str, base: &ack::Baseline, token: &str, timeout: Duration) -> String {
    match ack::wait(&ctx.layout, id, base, Some(&ack::token_of(token.as_bytes())), timeout) {
        Some((state, reason)) => format!("loaded: {state} {reason}").trim_end().to_string(),
        None => format!("reload requested; the SDK did not confirm within {} (it loads at the next HMI start)", seconds(timeout)),
    }
}

/// `mibsdk-dev push` on the unit: install `jar` as `plugins/<id>.jar` once `stamp` vouches for it
/// (both are one snapshot of the uploaded files, so the bytes checked are the bytes written), keep
/// the grants line as it is, and request a reload with a fresh token (always: the runtime reloads
/// once per new marker content). An id an installed package owns is refused. The unrestricted
/// warning goes to stderr at once.
///
/// Caller contract: holds the action lock and no /mnt/app read-write guard; the SDK is installed.
pub fn push(ctx: &Ctx, id: &str, jar: &[u8], stamp: Option<&[u8]>) -> Result<String> {
    check_id(id)?;
    let l = &ctx.layout;
    if State::load(l)?.packages.contains_key(id) {
        bail!("{id} is installed as a package; remove it first or install a newer version");
    }
    let name = format!("{id}.jar");
    let verified = stamp::verify_bytes(stamp, stamp::StampKind::Plugin, Some(id), &[(&name, jar)]).map_err(|e| {
        anyhow!("{name} has no valid check stamp ({e:#}); only jars built by the mibsdk.plugin convention can be pushed: ./gradlew :<project>:mibPluginJar (M7)")
    })?;
    if let Some(w) = verified.warning(id) {
        crate::warn!("{w}");
    }
    let running = runtime_up(l);
    let base = if running { ack::baseline(l) } else { ack::Baseline::unknown() };
    let token = {
        let _rw = ctx.mounts.rw("/mnt/app")?;
        ensure_plugins_dir(ctx)?;
        put(ctx, &jar_abs(id), jar, JAR_MODE)?;
        put_marker(ctx, id)?
    };
    let note = if running { await_load(ctx, id, &base, &token, ack::ACK_TIMEOUT) } else { "the SDK is not running; it loads at the next boot".into() };
    Ok(format!("pushed {id}; reload requested\n{note}"))
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
    // `up`: the runtime started this boot (its log exists), so it may be running the plugin now,
    // whatever the package state says (the reboot after `remove mibsdk`, an old mibsdk-dev install
    // never recorded as a package, lost state) and even while a reboot is pending. `live`: a
    // runtime can still act on the plugin, now or at the next start. Only with neither (no
    // package, no runtime log) is nothing written for a runtime and nothing waited for.
    let up = runtime_up(l);
    let live = up || installed_sdk(l).is_some();
    let ledger_wait = if live { ledger_wait } else { Duration::ZERO };
    let base = if up { ack::baseline(l) } else { ack::Baseline::unknown() };
    let (off_dropped, token) = {
        let _rw = ctx.mounts.rw("/mnt/app")?;
        let p = l.p(&jar);
        on_app(ctx, &format!("cannot remove {jar}"), &mut || remove_if_present(&p))?;
        if fs::symlink_metadata(&p).is_ok() {
            bail!("{jar} is still there after removal");
        }
        grants::drop_line(ctx, id)?;
        // Before the marker: the reload must see the plugin switched on again.
        let off_dropped = toggles::drop(ctx, id)?;
        let token = if live {
            Some(put_marker(ctx, id)?)
        } else {
            // A marker an earlier partial run left would make a later SDK reload a gone plugin.
            let m = l.p(&marker);
            on_app(ctx, &format!("cannot remove {marker}"), &mut || remove_if_present(&m))?;
            None
        };
        (off_dropped, token)
    };
    let mut out = vec![format!(
        "removed {id}: plugins/{id}.jar and its grants line{}; {}",
        if off_dropped { format!(" and its {id}.off switch") } else { String::new() },
        if live { "reload requested" } else { "the SDK is neither installed nor running, so no reload was requested" }
    )];
    let acked = match (&token, up) {
        (Some(t), true) => ack::wait(l, id, &base, Some(&ack::token_of(t.as_bytes())), ack_timeout).is_some(),
        _ => false,
    };

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
            out.push(if live {
                "they are restored when the SDK next starts (the plugin is no longer installed); check /tmp/mibsdk.log".into()
            } else {
                "nothing restores them while the SDK is not installed; install it again (mibpkg install mibsdk) and its runtime restores them".into()
            });
            break;
        }
        thread::sleep(LEDGER_POLL.min(ledger_wait - waited));
    }

    if purge {
        // A runtime up this boot (reboot pending or not) may write the data back until it confirms
        // the unload; with none up, nothing can.
        if acked || !up {
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
