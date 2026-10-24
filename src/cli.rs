//! Command line: host tooling (build, index) and on-unit commands.
use crate::archive;
use crate::config;
use crate::control;
use crate::fsutil;
use crate::ctx::Ctx;
use crate::gem;
use crate::gem_enable;
use crate::gempatch;
use crate::swdl;
use crate::hook;
use crate::index::{build_index, write_index, Collection};
use crate::install;
use crate::lock::Lock;
use crate::manifest::Manifest;
use crate::media;
use crate::mibsdk::plugin_pkg::NeedsApproval;
use crate::paths::Layout;
use crate::state::{Installed, State};
use crate::status::{self, now_secs};
use crate::supervisor::{self, Timing};
use crate::sys::RealSys;
use anyhow::{bail, Context, Result};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

const USAGE: &str = "usage:
  mibpkg build <src-dir> <out-dir>
  mibpkg index <collection-dir> --id ID --name NAME [--flat]
  mibpkg release <pkg-dir|file.mpk>... --repo OWNER/REPO --tag TAG [--id ID --name NAME [--new-id]] [--keep N] [--out DIR] [--asset FILE]... [--publish]
  mibpkg verify <github:owner/repo|index.json|release-dir> [--latest] [--unit MHI2Q]
  mibpkg new <name> [--dir DIR]
  mibpkg [--root DIR] [--no-supervisor] <command>
    supervise | status | list | install <pkg> [collection] | remove <pkg> [--purge|--purge-data] [--force]
    install-file <pkg> <upload-id> [collection|-]
    (install, install-file, rollback and gem install/install-file/rollback take --approve=<perm,...> for a plugin's permissions)
    sdk status | sdk grant <id> <perm,...> | sdk push <id> | sdk remove <id> [--purge|--purge-data]
    sdk safe-mode on|off | sdk boot-state set-wrap <n> | sdk data-clear <id>
    rollback <pkg> [--approve=<perm,...>] | restart <pkg> | rescan | update-all | gem <action> [args..]
    config-resolve <pkg> keep|card [config]
    (install, rescan and update-all take --config=keep|card to settle config conflicts)
    online-check | sntp [host:port] | web
    gem install-file <pkg> <id> <collection|-> | gem config-set <pkg> <n> <id>
    gem source-add <source> | gem source-remove <source> | gem web-settings <id> | gem web-reset | gem web-restart
    gem sdk grant <id> <perm,...>
    bootstrap | enable-gem | disable-gem | gem-patch <apply|status|revert> [--from DIR] | uninstall-self\n  mibpkg swdl-card <out-dir> --payload DIR [--seed DIR] --version V [--signed-header FILE] [--no-mib]\n  mibpkg gem-patch build --stock JAR --patched JAR [--out DIR]";

pub fn run(args: &[String]) -> Result<i32> {
    let mut root = PathBuf::from("/");
    let mut no_supervisor = false;
    let mut rest: Vec<String> = Vec::new();
    let mut it = args.iter();
    // Global flags only before the command word: after it, "--root" is just an argument (a
    // package name from GEM or the web must never be able to redirect mibpkg to another root).
    while let Some(a) = it.next() {
        match a.as_str() {
            "--root" if rest.is_empty() => root = PathBuf::from(it.next().context("--root needs a directory")?),
            "--no-supervisor" if rest.is_empty() => no_supervisor = true,
            _ => rest.push(a.clone()),
        }
    }
    let cmd = match rest.first() {
        Some(c) => c.clone(),
        None => {
            crate::warn!("{USAGE}");
            return Ok(2);
        }
    };
    // Like dpkg's --force-confold/confnew: settles a config changed both on the card and on the car.
    let mut config_choice = None;
    if matches!(cmd.as_str(), "install" | "rescan" | "update-all") {
        if let Some(i) = rest.iter().position(|a| a.starts_with("--config=")) {
            config_choice = Some(config::Choice::parse(&rest.remove(i)["--config=".len()..])?);
        }
    }
    // --approve/--force/--purge(-data) of the verbs that install or remove packages and plugins.
    let flags = match flag_scope(&rest) {
        true => take_action_flags(&mut rest),
        false => Ok(ActionFlags::default()),
    };
    let args = &rest[1..];
    match cmd.as_str() {
        "build" => return host_build(args),
        "index" => return host_index(args),
        "release" => return host_release(args),
        "verify" => return host_verify(args),
        "new" => return host_new(args),
        // gem-patch build is a host command; apply/status/revert are on-unit actions below.
        "gem-patch" if args.first().map(|s| s.as_str()) == Some("build") => return host_gem_patch_build(&args[1..]),
        "swdl-card" => return host_swdl_card(args),
        "help" | "--help" | "-h" => {
            crate::say!("{USAGE}");
            return Ok(0);
        }
        _ => {}
    }

    let layout = Layout::load(&root)?;
    let sys = RealSys { mounts: root == Path::new("/") };
    let mut ctx = Ctx::new(layout, &sys);
    if root == Path::new("/") {
        // Online installs download through the unit's libcurl; online-check refreshes the
        // clock offset file this reads.
        ctx.fetcher = Box::new(crate::fetch::CurlFetcher::for_unit(&ctx.layout, crate::clock::read_offset(&ctx.layout)));
    }
    ctx.config_choice = config_choice;
    // A malformed flag fails the action; the gem arm records it as FAILED (gem never fails).
    let (flags, flags_err) = match flags {
        Ok(f) => (f, None),
        Err(e) => (ActionFlags::default(), Some(e)),
    };
    if cmd != "gem" {
        if let Some(e) = flags_err {
            return Err(e);
        }
    }
    if cmd == "update-all" && flags.approved.is_some() {
        // One list for every package would approve on the user's behalf for the ones it covers.
        bail!("update-all never approves permissions; approve each plugin update with: mibpkg install <pkg> --approve=<perm,...>");
    }
    ctx.approved = flags.approved.clone();
    ctx.force = flags.force;
    let purge = flags.purge;
    let ctx = ctx;
    // The shims append every action's output to /tmp/mibpkg.log (RAM): keep it bounded.
    let _ = fsutil::cap_file(&ctx.layout.run("log"), supervisor::LOG_CAP);
    let after = |ctx: &Ctx| {
        if let Err(e) = gem::regenerate(ctx) {
            crate::warn!("warning: updating GEM screens: {e:#}");
        }
        if !no_supervisor {
            if let Err(e) = control::ensure_supervisor(&ctx.layout, ctx.sys) {
                crate::warn!("warning: starting the supervisor: {e:#}");
            }
        }
    };
    match cmd.as_str() {
        "supervise" => {
            supervisor::run(&ctx.layout, &sys, Timing::default())?;
            Ok(0)
        }
        "gem-screens" => {
            // Redraw the GEM page (run by the web server when its addresses change). Skipped
            // quietly while an action holds the lock: that action redraws the page when it ends.
            // Exit 1 when busy, so the web server tries again later.
            let Ok(_lock) = Lock::acquire(&ctx.layout, ctx.sys) else { return Ok(1) };
            if let Err(e) = gem::regenerate(&ctx) {
                crate::warn!("warning: updating GEM screens: {e:#}");
            }
            Ok(0)
        }
        "web" => {
            crate::web::run(ctx.layout.clone())?;
            Ok(0)
        }
        "status" => {
            print_status(&ctx);
            Ok(0)
        }
        "sntp" => {
            // Diagnostic: always queries, ignoring the cached offset.
            let server = args.first().map(|s| s.as_str()).unwrap_or(crate::clock::SERVERS[0]);
            crate::say!("{}", crate::clock::sntp_offset(server, Duration::from_secs(5))?);
            Ok(0)
        }
        "online-check" => {
            // Network first, without the lock, so GEM actions stay usable meanwhile. A busy lock or
            // a failed check exits 1: the supervisor then retries within minutes, not hours.
            let fetched = online_fetch(&ctx);
            let _lock = match Lock::acquire(&ctx.layout, ctx.sys) {
                Ok(l) => l,
                Err(_) => {
                    crate::say!("online: busy");
                    return Ok(1);
                }
            };
            let (text, ok) = online_store(&ctx, fetched);
            publish_online(&ctx, &text);
            after(&ctx);
            crate::say!("{text}");
            Ok(if ok { 0 } else { 1 })
        }
        "list" => {
            print_list(&ctx)?;
            Ok(0)
        }
        "install" | "install-file" | "remove" | "rollback" | "restart" | "rescan" | "update-all" | "config-resolve" => {
            // One lock across the action, the screens and the result: a second action changes nothing.
            let _lock = Lock::acquire(&ctx.layout, ctx.sys)?;
            let r = action(&ctx, &cmd, args, purge);
            after(&ctx);
            match r {
                Ok(m) => {
                    crate::say!("{m}");
                    Ok(0)
                }
                Err(e) => match e.downcast_ref::<NeedsApproval>() {
                    Some(n) => Ok(print_needs_approval(n)),
                    None => Err(e),
                },
            }
        }
        "sdk" => sdk(&ctx, args, purge, &after),
        "gem" => {
            let verb = args.first().context("gem needs an action")?.clone();
            let _lock = match Lock::acquire(&ctx.layout, ctx.sys) {
                Ok(l) => l,
                Err(e) => {
                    // Double tap, or GEM and the web racing: nothing was done. The running action
                    // refreshes the screens when it finishes. BUSY (not FAILED) so the web UI can
                    // say "try again" instead of reporting a failure.
                    crate::say!("BUSY: {e:#}");
                    return Ok(0);
                }
            };
            let key = match verb.as_str() {
                // The mibpkg update shows on GEM's "mibpkg update" row.
                "self-update" => install::SELF.to_string(),
                "install" | "remove" | "rollback" | "restart" | "install-file" | "config-set" | "config-resolve" => {
                    args.get(1).filter(|n| crate::manifest::valid_name(n)).cloned().unwrap_or_else(|| "_".into())
                }
                "sdk" => args.get(2).filter(|n| crate::manifest::valid_name(n)).cloned().unwrap_or_else(|| "_".into()),
                _ => "_".into(),
            };
            let r = match flags_err {
                Some(e) => Err(e),
                // The web's grant/revoke: only `sdk grant`, under this lock like every gem action.
                None if verb == "sdk" => match &args[1..] {
                    [g, rest @ ..] if g == "grant" && rest.len() == 2 => need_sdk(&ctx).and_then(|()| sdk_action(&ctx, "grant", rest, purge)),
                    _ => Err(anyhow::anyhow!("gem sdk takes only: grant <id> <perm,...>")),
                },
                None => action(&ctx, &verb, &args[1..], purge),
            };
            let text = match r {
                Ok(m) => format!("OK: {m}"),
                Err(e) => format!("FAILED: {e:#}"),
            };
            // Uninstall removed the state, screens and (with --purge) every mibpkg file; recording
            // a result or running after() would recreate them, so skip both for it.
            if verb != "uninstall-self" {
                if let Err(e) = record_result(&ctx, &key, &text) {
                    crate::warn!("warning: recording result: {e:#}");
                }
                after(&ctx);
            }
            crate::say!("{text}");
            Ok(0)
        }
        "bootstrap" => {
            let _lock = Lock::acquire(&ctx.layout, ctx.sys)?;
            {
                let _rw = ctx.mounts.rw("/mnt/app")?;
                for d in [ctx.layout.boot_d(), ctx.layout.state_dir(), ctx.layout.gem_dir(), ctx.layout.pkgs_dir()] {
                    fs::create_dir_all(&d).with_context(|| format!("creating {}", d.display()))?;
                }
                register_self(&ctx)?;
            }
            let changed = hook::install_hook(&ctx)?;
            after(&ctx);
            crate::say!("{}", if changed { "boot hook installed" } else { "boot hook already present" });
            Ok(0)
        }
        "gem-patch" => {
            let _lock = Lock::acquire(&ctx.layout, ctx.sys)?;
            let sub = arg(args, 0, "apply|status|revert")?;
            let from = match args.iter().position(|a| a == "--from") {
                Some(i) => Some(PathBuf::from(args.get(i + 1).context("--from needs a directory")?)),
                None => None,
            };
            let _rw = ctx.mounts.rw("/mnt/app")?;
            let msg = match sub {
                "apply" => gempatch::apply(&ctx.layout, &from.context("gem-patch apply needs --from <dir>")?)?,
                "status" => format!("{:?}", gempatch::status(&ctx.layout, &from.context("gem-patch status needs --from <dir>")?)?),
                "revert" => gempatch::revert(&ctx.layout)?,
                other => bail!("gem-patch: unknown subcommand {other:?}"),
            };
            crate::say!("{msg}");
            Ok(0)
        }
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
        "uninstall-self" => {
            let _lock = Lock::acquire(&ctx.layout, ctx.sys)?;
            let purge = args.iter().any(|a| a == "--purge");
            let force = args.iter().any(|a| a == "--force");
            let msg = uninstall_self(&ctx, purge, force)?;
            crate::say!("{msg}");
            Ok(0)
        }
        other => bail!("unknown command {other:?}\n{USAGE}"),
    }
}

/// Record the running mibpkg as installed (once), so GEM offers updates, not "Install".
/// Remove mibpkg from boot and GEM, stopping the supervisor and closing the hotspot port. With
/// `purge`, also restore every stock file it (or its packages) patched and delete all mibpkg files;
/// `force` lets a purge after a firmware update leave the live bundle list and marker as they are.
/// The caller holds the action lock (the top-level verb acquires it; GEM already holds it).
fn uninstall_self(ctx: &Ctx, purge: bool, force: bool) -> Result<String> {
    // Without mibpkg nothing could restore the stock bundle list or the plugins' overrides.
    let mut held: Vec<String> =
        State::load(&ctx.layout)?.packages.keys().filter(|n| n.as_str() == crate::mibsdk::SDK_PKG).cloned().collect();
    held.extend(crate::mibsdk::sdk_pkg::plugin_packages(&ctx.layout)?.into_iter().map(|(id, _)| id));
    if !held.is_empty() {
        bail!(
            "remove the SDK packages first ({}): mibpkg remove <name>, plugins before mibsdk; without mibpkg nothing can restore the stock bundle list",
            held.join(", ")
        );
    }
    if let Some(why) = crate::mibsdk::sdk_pkg::unrecorded_sdk(&ctx.layout, purge) {
        bail!("{why}");
    }
    // After a firmware update the SDK stock records would put the old firmware's list back.
    let firmware_note = if purge { crate::mibsdk::sdk_pkg::purge_firmware_check(&ctx.layout, force)? } else { None };
    control::request_stop(&ctx.layout)?;
    let end = Instant::now() + Duration::from_secs(15);
    while control::supervisor_pid(&ctx.layout, ctx.sys).is_some() && Instant::now() < end {
        std::thread::sleep(Duration::from_millis(200));
    }
    hook::uninstall_hook(ctx)?;
    // Close the web interface's hotspot port again (nothing to do if it was never opened).
    if let Err(e) = crate::firewall::apply(ctx, None) {
        crate::warn!("warning: hotspot firewall: {e:#}");
    }
    {
        let _rw = ctx.mounts.rw("/mnt/app")?;
        gem::remove_screens(&ctx.layout)?;
        if purge {
            // Stock-file backups live on /mnt/app (e.g. the GEM jar) AND /mnt/system (startup.sh),
            // both read-only by default; restore_all writes to all of them, so mount system rw too.
            let _rw_sys = ctx.mounts.rw("/mnt/system")?;
            // A stale SDK stock record would put an older firmware's bundle list back.
            crate::mibsdk::sdk_pkg::drop_stale_records(ctx)?;
            if firmware_note.is_some() {
                crate::mibsdk::sdk_pkg::forget_backups(ctx)?;
            }
            crate::backups::restore_all(&ctx.layout)?;
            match fs::remove_dir_all(ctx.layout.home()) {
                Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(e.into()),
                _ => {}
            }
        }
    }
    let _ = fs::remove_file(ctx.layout.run("stop"));
    Ok(if purge {
        let mut msg = String::from("mibpkg removed from boot and GEM; stock files restored and all mibpkg files deleted");
        if let Some(note) = firmware_note {
            msg = format!("{note}\n{msg}");
        }
        msg
    } else {
        format!(
            "mibpkg removed from boot and GEM; installed files remain in {} (delete by hand if wanted)",
            ctx.layout.unit_path(&ctx.layout.home())
        )
    })
}

fn register_self(ctx: &Ctx) -> Result<()> {
    let mut st = State::load(&ctx.layout)?;
    if st.packages.contains_key(install::SELF) {
        return Ok(());
    }
    let collection = media::loaded(&media::scan(&ctx.layout))
        .into_iter()
        .find(|c| c.latest(install::SELF).is_some())
        .map(|c| c.id().to_string())
        .unwrap_or_else(|| "complex256-mibpkg".into());
    st.packages.insert(
        install::SELF.to_string(),
        Installed {
            collection,
            version: env!("CARGO_PKG_VERSION").to_string(),
            previous: None,
            services: Vec::new(),
            uses_scripts: false,
        },
    );
    st.save(&ctx.layout)
}

fn arg<'a>(args: &'a [String], i: usize, what: &str) -> Result<&'a str> {
    args.get(i).map(|s| s.as_str()).with_context(|| format!("missing {what}"))
}

/// The supervisor's id for the built-in web server service.
pub const WEB_SERVICE: &str = "mibpkg.web";

fn id_arg<'a>(args: &'a [String], i: usize) -> Result<&'a str> {
    let id = arg(args, i, "id")?;
    if !crate::web::settings::is_id(id) {
        bail!("invalid id {id:?}");
    }
    Ok(id)
}

fn pkg_arg<'a>(args: &'a [String], i: usize) -> Result<&'a str> {
    let name = arg(args, i, "package name")?;
    if !crate::manifest::valid_name(name) {
        bail!("invalid package name {name:?}");
    }
    Ok(name)
}

/// Caller holds the action lock. `purge`: `--purge`/`--purge-data` was given.
fn action(ctx: &Ctx, verb: &str, args: &[String], purge: bool) -> Result<String> {
    let media_roots = media::roots(&ctx.layout);
    let mut colls: Vec<Collection> = media::loaded(&media::scan(&ctx.layout));
    colls.extend(crate::online::collections(&ctx.layout));
    let state = State::load(&ctx.layout)?;
    match verb {
        "install" => {
            let name = pkg_arg(args, 0)?;
            let coll = install::pick_collection(&state, &colls, name, args.get(1).map(|s| s.as_str()), &ctx.unit)?;
            install::install(ctx, &media_roots, coll, name)
        }
        "remove" => install::remove(ctx, pkg_arg(args, 0)?, purge),
        "rollback" => install::rollback(ctx, pkg_arg(args, 0)?),
        "restart" => {
            let name = pkg_arg(args, 0)?;
            let inst = state.packages.get(name).with_context(|| format!("{name} is not installed"))?;
            for id in &inst.services {
                control::request_restart(&ctx.layout, id)?;
            }
            Ok(format!("restart requested for {name}"))
        }
        "rescan" => {
            let _rw = ctx.mounts.rw("/mnt/app")?;
            install::cleanup_stale(&ctx.layout)?;
            let mut msgs = Vec::new();
            for name in state.packages.keys() {
                if let Ok(m) = install::current_manifest(&ctx.layout, name) {
                    let r = config::import(&ctx.layout, &media_roots, &m, ctx.config_choice)?;
                    let text = r.summary(name);
                    if !text.is_empty() {
                        msgs.push(format!("{name}: {text}"));
                    }
                }
            }
            control::request_reload(&ctx.layout)?;
            Ok(if msgs.is_empty() { "rescanned; no new config files".into() } else { msgs.join("; ") })
        }
        "update-all" => Ok(install::update_all(ctx, &media_roots, &colls)?.join("; ")),
        "install-file" => {
            let name = pkg_arg(args, 0)?;
            let id = id_arg(args, 1)?;
            let coll = args.get(2).map(String::as_str).unwrap_or("-");
            if coll != "-" && !crate::manifest::valid_name(coll) {
                bail!("invalid collection {coll:?}");
            }
            let path = ctx.layout.run(&format!("upload.{id}.mpk"));
            // The upload is in RAM (/tmp): gone after one attempt, whatever the outcome.
            let r = install::install_file(ctx, &media_roots, &colls, &path, name, Some(coll));
            let _ = fs::remove_file(&path);
            r
        }
        "config-set" => {
            let name = pkg_arg(args, 0)?;
            let n: usize = arg(args, 1, "config number")?.parse().context("invalid config number")?;
            let id = id_arg(args, 2)?;
            let text = ctx.layout.run(&format!("text.{id}"));
            let r = (|| -> Result<String> {
                let m = install::current_manifest(&ctx.layout, name)?;
                let c = m.configs.get(n).with_context(|| format!("{name} has no config number {n}"))?;
                {
                    let _rw = ctx.mounts.rw("/mnt/app")?;
                    config::import_file(&ctx.layout, name, &c.path, &text)?;
                }
                let inst = state.packages.get(name).with_context(|| format!("{name} is not installed"))?;
                for svc in &inst.services {
                    control::request_restart(&ctx.layout, svc)?;
                }
                Ok(format!("saved {} for {name}; restarting its services", c.path))
            })();
            let _ = fs::remove_file(&text);
            r
        }
        "config-resolve" => {
            let name = pkg_arg(args, 0)?;
            let choice = config::Choice::parse(arg(args, 1, "keep or card")?)?;
            let m = install::current_manifest(&ctx.layout, name)?;
            let paths = match args.get(2) {
                Some(which) => vec![config::find(&m, which)?.path.clone()],
                None => config::conflicts(&ctx.layout, &m),
            };
            if paths.is_empty() {
                bail!("{name} has no config conflict");
            }
            let mut changed = false;
            {
                let _rw = ctx.mounts.rw("/mnt/app")?;
                for p in &paths {
                    changed |= config::resolve(&ctx.layout, name, p, choice)?;
                }
            }
            let what = paths.join(", ");
            if !changed {
                return Ok(format!("kept the current {what} for {name}"));
            }
            for svc in &state.packages.get(name).with_context(|| format!("{name} is not installed"))?.services {
                control::request_restart(&ctx.layout, svc)?;
            }
            Ok(format!("using the card's {what} for {name}; restarting its services"))
        }
        "source-add" => {
            let source = arg(args, 0, "source")?;
            let msg = {
                let _rw = ctx.mounts.rw("/mnt/app")?;
                crate::online::add_source(&ctx.layout, source)?
            };
            let t = online_check(ctx);
            publish_online(ctx, &t);
            Ok(format!("{msg}; {t}"))
        }
        "source-remove" => {
            let _rw = ctx.mounts.rw("/mnt/app")?;
            crate::online::remove_source(&ctx.layout, arg(args, 0, "source")?)
        }
        "web-settings" => {
            let id = id_arg(args, 0)?;
            let text = ctx.layout.run(&format!("text.{id}"));
            let r = (|| -> Result<String> {
                let s: crate::web::settings::WebSettings = serde_json::from_str(&fs::read_to_string(&text)?).context("reading the new web settings")?;
                s.validate()?;
                let _rw = ctx.mounts.rw("/mnt/app")?;
                crate::web::settings::save(&ctx.layout, &s)?;
                // "enabled" may have changed: the supervisor re-reads its built-in services.
                control::request_reload(&ctx.layout)?;
                Ok("web settings saved".into())
            })();
            let _ = fs::remove_file(&text);
            r
        }
        "web-reset" => {
            {
                let _rw = ctx.mounts.rw("/mnt/app")?;
                match fs::remove_file(crate::web::settings::path(&ctx.layout)) {
                    Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(e.into()),
                    _ => {}
                }
            }
            // Reload starts it if web.toml had disabled it; restart applies the defaults if running.
            control::request_reload(&ctx.layout)?;
            control::request_restart(&ctx.layout, WEB_SERVICE)?;
            Ok("web settings reset to the defaults; the web server restarts".into())
        }
        "web-firewall" => {
            // Run by the web server when pf.conf does not match its port and listen choice.
            let port = match arg(args, 0, "port or off")? {
                "off" => None,
                p => Some(p.parse::<u16>().ok().filter(|p| *p > 0).with_context(|| format!("invalid port {p:?}"))?),
            };
            crate::firewall::apply(ctx, port)
        }
        "self-update" => {
            // GEM's "Update mibpkg": refresh the online indexes first (the cache can be hours old
            // and GEM has no separate check), then install the newest mibpkg, card or online.
            let checked = online_check(ctx);
            publish_online(ctx, &checked);
            let mut fresh: Vec<Collection> = media::loaded(&media::scan(&ctx.layout));
            fresh.extend(crate::online::collections(&ctx.layout));
            let coll = install::pick_collection(&state, &fresh, install::SELF, None, &ctx.unit).with_context(|| checked.clone())?;
            let msg = install::install(ctx, &media_roots, coll, install::SELF).with_context(|| checked.clone())?;
            Ok(format!("{checked}; {msg}"))
        }
        "web-clear-pin" => {
            // GEM's "Remove PIN" for a forgotten PIN: whoever can open GEM may use the car.
            let (mut s, _) = crate::web::settings::load(&ctx.layout);
            if s.pin.is_none() {
                return Ok("no PIN was set".into());
            }
            s.pin = None;
            {
                let _rw = ctx.mounts.rw("/mnt/app")?;
                crate::web::settings::save(&ctx.layout, &s)?;
            }
            control::request_restart(&ctx.layout, WEB_SERVICE)?;
            Ok("PIN removed; the web server restarts without it".into())
        }
        "web-restart" => {
            control::request_restart(&ctx.layout, WEB_SERVICE)?;
            Ok("web server restart requested".into())
        }
        "check" => {
            let t = online_check(ctx);
            publish_online(ctx, &t);
            Ok(t)
        }
        "uninstall-self" => {
            // GEM's "Remove mibpkg" buttons. The caller (the gem arm) holds the lock and skips
            // the usual after() so the screens/supervisor are not recreated right after removal.
            let purge = args.iter().any(|a| a == "--purge");
            uninstall_self(ctx, purge, args.iter().any(|a| a == "--force"))
        }
        other => bail!("unknown action {other:?}"),
    }
}

/// `--approve=`, `--force`, `--purge`/`--purge-data` of an install or removal.
#[derive(Default)]
struct ActionFlags {
    /// None: no `--approve` given (ask instead of installing); `--approve=` approves nothing.
    approved: Option<Vec<String>>,
    force: bool,
    purge: bool,
}

/// The command words that take `ActionFlags`.
fn flag_scope(rest: &[String]) -> bool {
    let w = |i: usize| rest.get(i).map(String::as_str);
    match w(0) {
        Some("install" | "install-file" | "update-all" | "remove" | "rollback") => true,
        Some("gem") => matches!(w(1), Some("install" | "install-file" | "remove" | "rollback")),
        Some("sdk") => w(1) == Some("remove"),
        _ => false,
    }
}

/// Take the action flags out of `rest` (anywhere after the command word).
fn take_action_flags(rest: &mut Vec<String>) -> Result<ActionFlags> {
    let mut f = ActionFlags::default();
    let mut err = None;
    let mut first = true;
    rest.retain(|a| {
        if std::mem::take(&mut first) {
            return true; // the command word
        }
        match a.as_str() {
            "--force" => f.force = true,
            "--purge" | "--purge-data" => f.purge = true,
            _ => match a.strip_prefix("--approve=") {
                Some(list) => {
                    let mut v = f.approved.take().unwrap_or_default();
                    for p in list.split(',').filter(|p| !p.is_empty()) {
                        if !crate::mibsdk::catalog::is_permission(p) {
                            err.get_or_insert_with(|| anyhow::anyhow!("invalid permission {p:?} in --approve (1 or more of [A-Za-z0-9._-])"));
                        }
                        v.push(p.to_string());
                    }
                    f.approved = Some(v);
                }
                None => return true,
            },
        }
        false
    });
    err.map_or(Ok(f), Err)
}

/// Exit code 3: what to approve, one line per permission, then how.
fn print_needs_approval(n: &NeedsApproval) -> i32 {
    crate::warn!("mibpkg: {} {} needs these permissions approved:", n.package, n.version);
    for l in &n.lines {
        crate::say!("{l}");
    }
    crate::say!("approve with: --approve={}", n.ask.join(","));
    3
}

const SDK_USAGE: &str = "usage:
  mibpkg sdk status
  mibpkg sdk grant <id> <perm,...>        (an empty list revokes all)
  mibpkg sdk push <id>                    (/tmp/mibpkg.push.<id>.jar and its .stamp, from mibsdk-dev push)
  mibpkg sdk remove <id> [--purge|--purge-data] [--force]
                                          (--purge deletes the plugin's data once the SDK confirms the
                                          unload; --force is accepted as for mibpkg remove and changes
                                          nothing for a plugin)
  mibpkg sdk safe-mode on|off
  mibpkg sdk boot-state set-wrap <n>
  mibpkg sdk data-clear <id>";

/// `boot-state set-wrap`'s count: 1 to 6 digits.
fn wrap_count(n: &str) -> bool {
    (1..=6).contains(&n.len()) && n.bytes().all(|b| b.is_ascii_digit())
}

/// `mibpkg sdk <verb>`. Every verb but `status` runs under the action lock (each writes through
/// `mibsdk::put`) and needs the SDK installed; `status` only reads, so it runs while an action
/// holds the lock and exits 0 whatever it finds.
fn sdk(ctx: &Ctx, args: &[String], purge: bool, after: &dyn Fn(&Ctx)) -> Result<i32> {
    let verb = args.first().map(String::as_str).unwrap_or("");
    let rest = args.get(1..).unwrap_or(&[]);
    let w = |i: usize| rest.get(i).map(String::as_str);
    // Exact arity: a surplus or missing argument is a usage error, never ignored.
    let well_formed = match verb {
        "status" => rest.is_empty(),
        "grant" => rest.len() == 2,
        "push" | "remove" | "data-clear" => rest.len() == 1,
        "safe-mode" => rest.len() == 1 && matches!(w(0), Some("on" | "off")),
        "boot-state" => rest.len() == 2 && w(0) == Some("set-wrap") && w(1).is_some_and(wrap_count),
        _ => false,
    };
    if !well_formed {
        crate::warn!("{SDK_USAGE}");
        return Ok(2);
    }
    if verb == "status" {
        sdk_status(ctx);
        return Ok(0);
    }
    // The pushed files are read once and deleted, whatever happens next (the lock included).
    let snapshot = if verb == "push" { Some(push_snapshot(ctx, rest)?) } else { None };
    let _lock = Lock::acquire(&ctx.layout, ctx.sys)?;
    // remove and data-clear also clean up what an SDK removal left behind, so they need no SDK.
    let needs_sdk = !matches!(verb, "remove" | "data-clear");
    let r = (if needs_sdk { need_sdk(ctx) } else { Ok(()) }).and_then(|()| match snapshot {
        Some((id, jar, stamp)) => crate::mibsdk::plugin_pkg::push(ctx, &id, &jar, stamp.as_deref()),
        None => sdk_action(ctx, verb, rest, purge),
    });
    after(ctx);
    crate::say!("{}", r?);
    Ok(0)
}

/// What `sdk grant` may grant `id`: a package's grants are a subset of what its `[plugin]`
/// declares (any other package is refused); a pushed jar has no manifest, so the installed SDK's
/// catalog decides (failing closed when it is missing or unreadable). Revoking all is always fine.
fn grantable(ctx: &Ctx, id: &str, perms: &std::collections::BTreeSet<String>) -> Result<()> {
    let l = &ctx.layout;
    if State::load(l)?.packages.contains_key(id) {
        let m = install::current_manifest(l, id).with_context(|| format!("reading the manifest of the {id} package"))?;
        let Some(plugin) = m.plugin.as_ref() else {
            bail!("{id} is not a plugin package; only plugins have permissions");
        };
        if let Some(p) = perms.iter().find(|p| !plugin.permissions.contains(p)) {
            bail!("{id} does not declare {p}; it can only be granted: {}", plugin.permissions.join(", "));
        }
        return Ok(());
    }
    if perms.is_empty() {
        return Ok(());
    }
    let cat = crate::mibsdk::catalog::installed(l)?.with_context(|| format!("{} is missing", crate::mibsdk::PERMISSIONS))?;
    if let Some(p) = perms.iter().find(|p| !cat.iter().any(|c| &c.id == *p)) {
        bail!("the installed SDK does not know the permission {p}");
    }
    Ok(())
}

fn need_sdk(ctx: &Ctx) -> Result<()> {
    if crate::mibsdk::installed_sdk(&ctx.layout).is_none() {
        bail!("mibsdk is not installed (run: mibpkg install mibsdk, or mibsdk-dev install)");
    }
    Ok(())
}

/// `sdk push <id>`: the id and one read of `/tmp/mibpkg.push.<id>.jar` and its `.stamp` (None:
/// no stamp); both files are deleted before this returns.
fn push_snapshot(ctx: &Ctx, rest: &[String]) -> Result<(String, Vec<u8>, Option<Vec<u8>>)> {
    let id = arg(rest, 0, "plugin id")?;
    crate::mibsdk::plugin_pkg::check_id(id)?;
    let (jar_p, stamp_p) = (ctx.layout.run(&format!("push.{id}.jar")), ctx.layout.run(&format!("push.{id}.jar.stamp")));
    let (jar, stamp) = (fs::read(&jar_p), fs::read(&stamp_p).ok());
    let _ = fs::remove_file(&jar_p);
    let _ = fs::remove_file(&stamp_p);
    let jar = jar.with_context(|| format!("reading {} (upload it first: mibsdk-dev push <plugin.jar>)", ctx.layout.unit_path(&jar_p)))?;
    Ok((id.to_string(), jar, stamp))
}

fn remove_if_present(p: &Path) -> std::io::Result<()> {
    match fs::remove_file(p) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e),
        _ => Ok(()),
    }
}

/// The `sdk` verbs that change something; the caller holds the action lock and checked the SDK.
fn sdk_action(ctx: &Ctx, verb: &str, rest: &[String], purge: bool) -> Result<String> {
    use crate::mibsdk::{plugin_pkg, BASE, BOOT_STATE_RESET, PERSIST};
    let l = &ctx.layout;
    let boot_state = format!("{PERSIST}/boot.state");
    match verb {
        "grant" => {
            // Any id the runtime accepts (a pushed plugin's id need not be a package name).
            let id = arg(rest, 0, "plugin id")?;
            plugin_pkg::check_id(id)?;
            let list = arg(rest, 1, "permission list (empty to revoke all)")?;
            if !list.bytes().all(|b| b.is_ascii_alphanumeric() || b"._,-".contains(&b)) {
                bail!("invalid permission list {list:?}");
            }
            let perms: std::collections::BTreeSet<String> = list.split(',').filter(|p| !p.is_empty()).map(String::from).collect();
            grantable(ctx, id, &perms)?;
            {
                let _rw = ctx.mounts.rw("/mnt/app")?;
                crate::mibsdk::grants::set(ctx, id, &perms)?;
            }
            let shown = if perms.is_empty() { "<none>".to_string() } else { perms.iter().cloned().collect::<Vec<_>>().join(",") };
            Ok(format!("granted {id}: {shown}\n{}", plugin_pkg::activate(ctx, id)))
        }
        "remove" => {
            let id = arg(rest, 0, "plugin id")?;
            plugin_pkg::check_id(id)?;
            if id == crate::mibsdk::SDK_PKG {
                bail!("mibsdk is the SDK itself; remove it with: mibpkg remove mibsdk");
            }
            if !State::load(l)?.packages.contains_key(id) {
                return plugin_pkg::remove_id(ctx, id, purge);
            }
            match install::current_manifest(l, id) {
                Ok(m) if m.kind() != crate::manifest::Kind::Plugin => {
                    bail!("{id} is not a plugin package; remove it with: mibpkg remove {id}")
                }
                Ok(_) => {}
                // Unknown kind: never removed as a plugin on a guess.
                Err(e) => bail!(
                    "the manifest of the {id} package cannot be read ({e:#}); remove the package first, then the plugin: mibpkg remove {id}, then: mibpkg sdk remove {id}"
                ),
            }
            install::remove(ctx, id, purge)
        }
        "safe-mode" if rest[0] == "on" => {
            let _rw = ctx.mounts.rw("/mnt/app")?;
            crate::mibsdk::put(ctx, &format!("{BASE}/SAFE_MODE"), b"", 0o644)?;
            Ok("safe mode marker set; it takes effect on the next HMI restart".into())
        }
        "safe-mode" => {
            {
                let _rw = ctx.mounts.rw("/mnt/app")?;
                let marker = l.p(&format!("{BASE}/SAFE_MODE"));
                crate::mibsdk::sdk_pkg::on_app(ctx, "cannot remove SAFE_MODE", &mut || remove_if_present(&marker))?;
            }
            // PERSIST is writable without a remount.
            fs::create_dir_all(l.p(PERSIST)).with_context(|| format!("creating {PERSIST}"))?;
            crate::mibsdk::put(ctx, &boot_state, BOOT_STATE_RESET.as_bytes(), 0o644)?;
            Ok("safe mode off: SAFE_MODE removed and boot.state reset (unhealthyBoots=0, wrapBoots=0: plugins load and\n\
                DSI services are wrapped again).\n\
                It takes effect on the next HMI restart; recommended: restart the HMI (mibsdk-dev restart).\n\
                Note: if the runtime is currently running in normal mode it may rewrite boot.state from cached state;\n      \
                restart promptly (or run this with the HMI stopped) so the reset sticks."
                .into())
        }
        "boot-state" => {
            let n = &rest[1];
            let old = match fs::read(l.p(&boot_state)) {
                Ok(b) => b,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => BOOT_STATE_RESET.as_bytes().to_vec(),
                Err(e) => return Err(e).with_context(|| format!("reading {boot_state}")),
            };
            fs::create_dir_all(l.p(PERSIST)).with_context(|| format!("creating {PERSIST}"))?;
            crate::mibsdk::put(ctx, &boot_state, &set_wrap(&old, n), 0o644)?;
            Ok(format!("boot.state: wrapBoots={n} (other keys kept); it is read on the next HMI start"))
        }
        "data-clear" => {
            let id = arg(rest, 0, "plugin id")?;
            plugin_pkg::check_id(id)?;
            let data = format!("{PERSIST}/data/{id}.properties");
            let tmp = format!("{data}.tmp");
            if !l.p(&data).exists() && !l.p(&tmp).exists() {
                return Ok(format!("no data for {id} in {PERSIST}/data; nothing to clear"));
            }
            for abs in [&data, &tmp] {
                remove_if_present(&l.p(abs)).with_context(|| format!("cannot remove {abs}"))?;
            }
            Ok(format!(
                "cleared the data of {id} ({data})\nRestart the HMI now: a running {id} may write its data back, and the storage cap frees the space only from the next runtime start."
            ))
        }
        other => bail!("unknown sdk verb {other:?}"),
    }
}

/// `boot.state` with every `wrapBoots=` line set to `n` (appended when there is none), CRs
/// stripped, every other line kept (as `mibsdk-dev boot-state set-wrap`).
fn set_wrap(old: &[u8], n: &str) -> Vec<u8> {
    let mut out = Vec::new();
    let mut found = false;
    let mut lines: Vec<&[u8]> = old.split(|&b| b == b'\n').collect();
    if lines.last().is_some_and(|l| l.is_empty()) {
        lines.pop();
    }
    for line in lines {
        let line = line.strip_suffix(b"\r").unwrap_or(line);
        if line.starts_with(b"wrapBoots=") {
            out.extend_from_slice(format!("wrapBoots={n}").as_bytes());
            found = true;
        } else {
            out.extend_from_slice(line);
        }
        out.push(b'\n');
    }
    if !found {
        out.extend_from_slice(format!("wrapBoots={n}\n").as_bytes());
    }
    out
}

/// ISO-8859-1 text of a runtime file (every byte is the code point of the same value).
fn latin1(b: &[u8]) -> String {
    b.iter().map(|&c| c as char).collect()
}

/// `sdk status`: the live `HMICar=` line first, the SDK and its plugins, the ledger, then the
/// runtime's STATUS lines. Never fails: what cannot be read is said on stderr.
fn sdk_status(ctx: &Ctx) {
    use crate::mibsdk::{grants, installed_sdk, ledger, reboot_flag, sdk_running, toggles, BASE, BUNDLES, LOG};
    let l = &ctx.layout;
    match fs::read(l.p(BUNDLES)) {
        Ok(b) => {
            let t = latin1(&b);
            let line = t.split('\n').map(|x| x.strip_suffix('\r').unwrap_or(x)).find(|x| x.starts_with("HMICar="));
            crate::say!("{}", line.unwrap_or("HMICar=<missing>"));
        }
        Err(e) => {
            crate::warn!("warning: cannot read {BUNDLES} ({e})");
            crate::say!("HMICar=<missing>");
        }
    }
    let sdk = installed_sdk(l);
    let hmi = crate::unit::hmi_version(l);
    let hmi_name = hmi.clone().unwrap_or_else(|| "unknown".into());
    let mut unsupported = false;
    match &sdk {
        Some((version, spec, _)) => {
            unsupported = !hmi.as_ref().is_some_and(|v| spec.hmi.contains(v));
            crate::say!("sdk: {version}");
            crate::say!("api: {}..{}", spec.min_api, spec.api);
            crate::say!("hmi: {hmi_name} ({})", if unsupported { "UNSUPPORTED" } else { "supported" });
        }
        None => {
            crate::say!("sdk: not installed");
            crate::say!("api: -");
            crate::say!("hmi: {hmi_name}");
        }
    }
    let yes = |b: bool| if b { "yes" } else { "no" };
    crate::say!("running: {}", yes(sdk_running(l)));
    crate::say!("reboot needed: {}", yes(reboot_flag(l).exists()));

    // Packages (version, declared permissions) and pushed jars (no package), by id.
    let mut plugins: std::collections::BTreeMap<String, (String, Option<Vec<String>>)> = Default::default();
    if let Ok(st) = State::load(l) {
        for (name, inst) in &st.packages {
            if let Some(pl) = install::current_manifest(l, name).ok().and_then(|m| m.plugin) {
                plugins.insert(name.clone(), (inst.version.clone(), Some(pl.permissions)));
            }
        }
    }
    if let Ok(rd) = fs::read_dir(l.p(&format!("{BASE}/plugins"))) {
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            if let Some(id) = name.strip_suffix(".jar") {
                plugins.entry(id.to_string()).or_insert_with(|| ("pushed".into(), None));
            }
        }
    }
    let granted = grants::read(l).unwrap_or_else(|e| {
        crate::warn!("warning: {e:#}");
        Default::default()
    });
    let off = toggles::off_ids(l);
    for (id, (version, declared)) in &plugins {
        let g = granted.get(id).cloned().unwrap_or_default();
        let mut line = format!("plugin {id} {version} grants={}", g.iter().cloned().collect::<Vec<_>>().join(","));
        if off.contains(id) {
            line.push_str(" off");
        }
        let missing = declared.as_deref().map(|d| grants::missing(d, &g)).unwrap_or_default();
        if !missing.is_empty() {
            line.push_str(&format!(" blocked: {}", missing.join(",")));
        }
        crate::say!("{line}");
    }

    match ledger::read(l) {
        Ok(entries) => {
            crate::say!("ledger: {} override(s)", entries.len());
            for e in &entries {
                crate::say!("{}", e.line);
            }
            if !entries.is_empty() {
                // Spec 6.1.
                crate::say!("WARNING: vehicle overrides are in force; disable their plugins before a firmware update (a firmware without a matching adapter cannot restore them)");
                if unsupported {
                    crate::say!("WARNING: the SDK is off on this firmware and cannot restore these overrides");
                }
            }
        }
        Err(e) => crate::say!("ledger: unreadable ({e:#})"),
    }

    match fs::read(l.p(LOG)) {
        Ok(b) => {
            let t = latin1(&b);
            let status: Vec<&str> = t.lines().filter(|x| x.contains(" STATUS ")).collect();
            if status.is_empty() {
                crate::warn!("no STATUS lines in {LOG} yet");
            }
            for s in status {
                crate::say!("{s}");
            }
        }
        Err(_) => crate::warn!("{LOG} not found (the SDK has not started this boot)"),
    }
}

/// Fetch the online indexes (configured sources) and report; never fails the caller.
type Fetched = crate::online::Fetched;

/// Fetches every online source; needs no lock. Err is the finished status line.
fn online_fetch(ctx: &Ctx) -> std::result::Result<Fetched, String> {
    let on_unit = ctx.layout.root == Path::new("/");
    let offset = if on_unit {
        match crate::clock::ensure_offset(&ctx.layout, crate::clock::SERVERS, Duration::from_secs(5)) {
            Ok(o) => Some(o),
            Err(e) => return Err(format!("online: no clock yet ({e:#})")),
        }
    } else {
        None
    };
    let fetcher: Box<dyn crate::fetch::Fetcher> = if on_unit {
        Box::new(crate::fetch::CurlFetcher::for_unit(&ctx.layout, offset))
    } else {
        Box::new(crate::fetch::Unavailable)
    };
    Ok(crate::online::fetch_all(ctx, fetcher.as_ref()))
}

/// Stores what was fetched (under the lock): the status line and whether everything worked.
fn online_store(ctx: &Ctx, fetched: std::result::Result<Fetched, String>) -> (String, bool) {
    match fetched {
        Err(text) => (text, false),
        Ok(f) => {
            let report = crate::online::store(ctx, f);
            (format!("online: {}", report.summary(now_secs())), report.errors.is_empty())
        }
    }
}

/// The online status line, for the web UI (/tmp/mibpkg.online, RAM). GEM has no online row since
/// it became the single recovery page, so nothing goes to persistence (flash).
fn publish_online(ctx: &Ctx, text: &str) {
    let _ = crate::fsutil::atomic_write(&ctx.layout.run("online"), text.as_bytes());
}

fn online_check(ctx: &Ctx) -> String {
    online_store(ctx, online_fetch(ctx)).0
}

fn record_result(ctx: &Ctx, key: &str, text: &str) -> Result<()> {
    let _rw = ctx.mounts.rw("/mnt/app")?;
    let mut st = State::load(&ctx.layout)?;
    let line = format!("{text}{}", result_stamp(now_secs()));
    st.results.insert(key.to_string(), line.clone());
    st.save(&ctx.layout)?;
    // The live "last result" row reads this.
    crate::persist::publish_result(ctx.sys, &ctx.layout, key, &line);
    Ok(())
}

fn print_status(ctx: &Ctx) {
    // "port N" then "<class> <url>" lines, written by the running web server.
    match fs::read_to_string(ctx.layout.run("web.url")) {
        Ok(t) => {
            let urls: Vec<&str> = t.lines().skip(1).filter(|l| !l.starts_with("iface ")).collect();
            crate::say!("web: {}", if urls.is_empty() { t.lines().next().unwrap_or("running").to_string() } else { urls.join(", ") });
        }
        Err(_) => crate::say!("web: not running"),
    }
    match status::read(&ctx.layout) {
        None => crate::say!("supervisor: no status (not running since boot?)"),
        Some(s) => {
            let alive = control::supervisor_pid(&ctx.layout, ctx.sys).is_some();
            crate::say!(
                "supervisor {}pid {} ({}){}",
                s.version.as_deref().map(|v| format!("{v} ")).unwrap_or_default(),
                s.pid,
                if alive { "running" } else { "gone" },
                if s.disabled { ", DISABLED" } else { "" }
            );
            for (id, v) in &s.services {
                crate::say!("  {id}: {}", status::describe(v));
            }
            for (f, e) in &s.invalid {
                crate::say!("  boot.d/{f}: {e}");
            }
        }
    }
}

fn print_list(ctx: &Ctx) -> Result<()> {
    crate::say!("unit: {}", ctx.unit.describe());
    let st = State::load(&ctx.layout)?;
    for (n, i) in &st.packages {
        crate::say!(
            "installed {n} {} [{}]{}",
            i.version,
            i.collection,
            i.previous.as_ref().map(|p| format!(" (previous {p})")).unwrap_or_default()
        );
    }
    for (dir, c) in media::scan(&ctx.layout) {
        match c {
            Ok(c) => {
                for e in &c.index.packages {
                    crate::say!("card [{}] {} {}", c.id(), e.name, e.version);
                }
            }
            Err(e) => crate::say!("card {}: {e:#}", dir.display()),
        }
    }
    Ok(())
}

fn host_build(args: &[String]) -> Result<i32> {
    let src = Path::new(arg(args, 0, "source dir")?);
    let out_dir = Path::new(arg(args, 1, "output dir")?);
    let m = Manifest::parse(&fs::read_to_string(src.join("manifest.toml")).context("reading manifest.toml")?)?;
    fs::create_dir_all(out_dir)?;
    let out = out_dir.join(format!("{}-{}.mpk", m.package.name, m.package.version));
    archive::pack(src, &out)?;
    crate::say!("{}", out.display());
    Ok(0)
}

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
    let target = target.context("verify needs a source (github:owner/repo), an index.json, a release directory, or an SWDL card directory")?;
    // An SWDL card directory (has metainfo2.txt) is checked by the card validator, not the collection one.
    if Path::new(&target).join("metainfo2.txt").is_file() {
        let probs = swdl::verify_card(Path::new(&target))?;
        for p in &probs {
            crate::say!("problem: {p}");
        }
        crate::say!("SWDL card checked, {} problem(s)", probs.len());
        return Ok(if probs.is_empty() { 0 } else { 1 });
    }
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

fn host_swdl_card(args: &[String]) -> Result<i32> {
    let out = PathBuf::from(arg(args, 0, "output dir")?);
    let (mut payload, mut seed, mut version, mut header) = (None, None, None, None);
    let mut with_mib = true; // one card installs via the Red menu and M.I.B. by default
    let mut it = args[1..].iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--payload" => payload = Some(PathBuf::from(it.next().context("--payload needs a dir")?)),
            "--seed" => seed = Some(PathBuf::from(it.next().context("--seed needs a dir")?)),
            "--version" => version = Some(it.next().context("--version needs a value")?.clone()),
            "--signed-header" => header = Some(PathBuf::from(it.next().context("--signed-header needs a file")?)),
            "--no-mib" => with_mib = false,
            s => bail!("unexpected argument {s:?}"),
        }
    }
    let payload = payload.context("--payload <dir> required")?;
    let version = version.context("--version <v> required")?;
    let header_text = match &header {
        Some(p) => Some(fs::read_to_string(p).with_context(|| format!("reading {}", p.display()))?),
        None => None,
    };
    swdl::build_card(&out, &payload, seed.as_deref(), &version, header_text.as_deref(), with_mib)?;
    let probs = swdl::verify_card(&out)?;
    for p in &probs {
        crate::say!("problem: {p}");
    }
    crate::say!("{}", out.display());
    if !probs.is_empty() {
        bail!("swdl-card produced {} problem(s); see above", probs.len());
    }
    Ok(0)
}

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
    // Upsert by stock_sha1: rebuilding the same stock jar (e.g. a revised patched jar) must
    // replace its entry, not leave a stale result_sha1 first that would make apply bail.
    let toml = out.join("patches.toml");
    let mut pats: crate::gempatch::Patches =
        toml::from_str(&fs::read_to_string(&toml).unwrap_or_default()).context("parsing patches.toml")?;
    pats.upsert(crate::gempatch::PatchEntry {
        stock_sha1: stock_sha.clone(),
        file: format!("{stock_sha}.patch"),
        result_sha1: crate::sha1::sha1_hex(&po),
    });
    fs::write(&toml, pats.to_toml())?;
    crate::say!("{}", out.join(format!("{stock_sha}.patch")).display());
    Ok(0)
}

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

fn host_index(args: &[String]) -> Result<i32> {
    let dir = Path::new(arg(args, 0, "collection dir")?);
    let mut id = None;
    let mut name = None;
    let mut flat = false;
    let mut it = args[1..].iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--id" => id = it.next().cloned(),
            "--name" => name = it.next().cloned(),
            "--flat" => flat = true,
            other => bail!("unexpected argument {other:?}"),
        }
    }
    let id = id.context("--id is required")?;
    let name = name.unwrap_or_else(|| id.clone());
    let idx = if flat { crate::index::build_index_flat(dir, &id, &name)? } else { build_index(dir, &id, &name)? };
    write_index(dir, &idx)?;
    crate::say!("{} packages in {}", idx.packages.len(), dir.join("index.json").display());
    Ok(0)
}

/// " (YYYY-MM-DD HH:MM UTC)" for a result line, or "" while the unit clock is still unset
/// at boot (it starts at 1970 until time sync).
pub fn result_stamp(secs: u64) -> String {
    const Y2020: u64 = 1_577_836_800;
    if secs < Y2020 {
        String::new()
    } else {
        format!(" ({})", utc_stamp(secs))
    }
}

/// "YYYY-MM-DD HH:MM UTC" without a date crate (the unit has no `date` either).
pub fn utc_stamp(secs: u64) -> String {
    let days = (secs / 86_400) as i64;
    let rem = secs % 86_400;
    let (y, m, d) = civil_from_days(days);
    format!("{y:04}-{m:02}-{d:02} {:02}:{:02} UTC", rem / 3600, (rem % 3600) / 60)
}

fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = (if z >= 0 { z } else { z - 146_096 }) / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = (if mp < 10 { mp + 3 } else { mp - 9 }) as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}
