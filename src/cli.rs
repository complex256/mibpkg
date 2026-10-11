//! Command line: host tooling (build, index) and on-unit commands.
use crate::archive;
use crate::config;
use crate::control;
use crate::fsutil;
use crate::ctx::Ctx;
use crate::gem;
use crate::hook;
use crate::index::{build_index, write_index, Collection};
use crate::install;
use crate::lock::Lock;
use crate::manifest::Manifest;
use crate::media;
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
  mibpkg [--root DIR] [--no-supervisor] <command>
    supervise | status | list | install <pkg> [collection] | remove <pkg> [--purge]
    rollback <pkg> | restart <pkg> | rescan | update-all | gem <action> [args..]
    config-resolve <pkg> keep|card [config]
    (install, rescan and update-all take --config=keep|card to settle config conflicts)
    online-check | sntp [host:port] | web
    gem install-file <pkg> <id> <collection|-> | gem config-set <pkg> <n> <id>
    gem source-add <source> | gem source-remove <source> | gem web-settings <id> | gem web-reset | gem web-restart
    bootstrap | uninstall-self";

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
    let args = &rest[1..];
    match cmd.as_str() {
        "build" => return host_build(args),
        "index" => return host_index(args),
        "release" => return host_release(args),
        "verify" => return host_verify(args),
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
        "install" | "remove" | "rollback" | "restart" | "rescan" | "update-all" | "config-resolve" => {
            // One lock across the action, the screens and the result: a second action changes nothing.
            let _lock = Lock::acquire(&ctx.layout, ctx.sys)?;
            let r = action(&ctx, &cmd, args);
            after(&ctx);
            crate::say!("{}", r?);
            Ok(0)
        }
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
                _ => "_".into(),
            };
            let text = match action(&ctx, &verb, &args[1..]) {
                Ok(m) => format!("OK: {m}"),
                Err(e) => format!("FAILED: {e:#}"),
            };
            if let Err(e) = record_result(&ctx, &key, &text) {
                crate::warn!("warning: recording result: {e:#}");
            }
            after(&ctx);
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
        "uninstall-self" => {
            let _lock = Lock::acquire(&ctx.layout, ctx.sys)?;
            control::request_stop(&ctx.layout)?;
            let end = Instant::now() + Duration::from_secs(15);
            while control::supervisor_pid(&ctx.layout, ctx.sys).is_some() && Instant::now() < end {
                std::thread::sleep(Duration::from_millis(200));
            }
            hook::uninstall_hook(&ctx)?;
            // Close the web interface's hotspot port again (nothing to do if it was never opened).
            if let Err(e) = crate::firewall::apply(&ctx, None) {
                crate::warn!("warning: hotspot firewall: {e:#}");
            }
            {
                let _rw = ctx.mounts.rw("/mnt/app")?;
                gem::remove_screens(&ctx.layout)?;
            }
            let _ = fs::remove_file(ctx.layout.run("stop"));
            crate::say!(
                "mibpkg removed from boot and GEM; installed files remain in {} (delete by hand if wanted)",
                ctx.layout.unit_path(&ctx.layout.home())
            );
            Ok(0)
        }
        other => bail!("unknown command {other:?}\n{USAGE}"),
    }
}

/// Record the running mibpkg as installed (once), so GEM offers updates, not "Install".
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

/// Caller holds the action lock.
fn action(ctx: &Ctx, verb: &str, args: &[String]) -> Result<String> {
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
        "remove" => install::remove(ctx, pkg_arg(args, 0)?, args.iter().any(|a| a == "--purge")),
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
            let coll = arg(args, 2, "collection")?;
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
        other => bail!("unknown action {other:?}"),
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
