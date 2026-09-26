//! Command line: host tooling (build, index) and on-unit commands.
use crate::archive;
use crate::config;
use crate::control;
use crate::ctx::Ctx;
use crate::gem;
use crate::hook;
use crate::index::{build_index, write_index, Collection};
use crate::install;
use crate::lock::Lock;
use crate::manifest::Manifest;
use crate::media;
use crate::paths::Layout;
use crate::state::State;
use crate::status::{self, now_secs};
use crate::supervisor::{self, Timing};
use crate::sys::RealSys;
use anyhow::{bail, Context, Result};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

const USAGE: &str = "usage:
  mibpkg build <src-dir> <out-dir>
  mibpkg index <collection-dir> --id ID --name NAME
  mibpkg [--root DIR] [--no-supervisor] <command>
    supervise | status | list | install <pkg> [collection] | remove <pkg> [--purge]
    rollback <pkg> | restart <pkg> | rescan | update-all | gem <action> [args..]
    bootstrap | uninstall-self";

pub fn run(args: &[String]) -> Result<i32> {
    let mut root = PathBuf::from("/");
    let mut no_supervisor = false;
    let mut rest: Vec<String> = Vec::new();
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--root" => root = PathBuf::from(it.next().context("--root needs a directory")?),
            "--no-supervisor" => no_supervisor = true,
            _ => rest.push(a.clone()),
        }
    }
    let cmd = match rest.first() {
        Some(c) => c.clone(),
        None => {
            eprintln!("{USAGE}");
            return Ok(2);
        }
    };
    let args = &rest[1..];
    match cmd.as_str() {
        "build" => return host_build(args),
        "index" => return host_index(args),
        "help" | "--help" | "-h" => {
            println!("{USAGE}");
            return Ok(0);
        }
        _ => {}
    }

    let layout = Layout::load(&root)?;
    let sys = RealSys { mounts: root == Path::new("/") };
    let ctx = Ctx::new(layout, &sys);
    let after = |ctx: &Ctx| {
        if let Err(e) = gem::regenerate(ctx) {
            eprintln!("warning: updating GEM screens: {e:#}");
        }
        if !no_supervisor {
            if let Err(e) = control::ensure_supervisor(&ctx.layout, ctx.sys) {
                eprintln!("warning: starting the supervisor: {e:#}");
            }
        }
    };
    match cmd.as_str() {
        "supervise" => {
            supervisor::run(&ctx.layout, &sys, Timing::default())?;
            Ok(0)
        }
        "status" => {
            print_status(&ctx);
            Ok(0)
        }
        "list" => {
            print_list(&ctx)?;
            Ok(0)
        }
        "install" | "remove" | "rollback" | "restart" | "rescan" | "update-all" => {
            let r = action(&ctx, &cmd, args);
            after(&ctx);
            println!("{}", r?);
            Ok(0)
        }
        "gem" => {
            let verb = args.first().context("gem needs an action")?.clone();
            let key = match verb.as_str() {
                "install" | "remove" | "rollback" | "restart" => args.get(1).cloned().unwrap_or_else(|| "_".into()),
                _ => "_".into(),
            };
            let text = match action(&ctx, &verb, &args[1..]) {
                Ok(m) => format!("OK: {m}"),
                Err(e) => format!("FAILED: {e:#}"),
            };
            if let Err(e) = record_result(&ctx, &key, &text) {
                eprintln!("warning: recording result: {e:#}");
            }
            after(&ctx);
            println!("{text}");
            Ok(0)
        }
        "bootstrap" => {
            let _lock = Lock::acquire(&ctx.layout, ctx.sys)?;
            {
                let _rw = ctx.mounts.rw("/mnt/app")?;
                for d in [ctx.layout.boot_d(), ctx.layout.state_dir(), ctx.layout.gem_dir(), ctx.layout.pkgs_dir()] {
                    fs::create_dir_all(&d).with_context(|| format!("creating {}", d.display()))?;
                }
            }
            let changed = hook::install_hook(&ctx)?;
            after(&ctx);
            println!("{}", if changed { "boot hook installed" } else { "boot hook already present" });
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
            {
                let _rw = ctx.mounts.rw("/mnt/app")?;
                gem::remove_screens(&ctx.layout)?;
            }
            let _ = fs::remove_file(ctx.layout.run("stop"));
            println!(
                "mibpkg removed from boot and GEM; installed files remain in {} (delete by hand if wanted)",
                ctx.layout.unit_path(&ctx.layout.home())
            );
            Ok(0)
        }
        other => bail!("unknown command {other:?}\n{USAGE}"),
    }
}

fn arg<'a>(args: &'a [String], i: usize, what: &str) -> Result<&'a str> {
    args.get(i).map(|s| s.as_str()).with_context(|| format!("missing {what}"))
}

fn action(ctx: &Ctx, verb: &str, args: &[String]) -> Result<String> {
    let _lock = Lock::acquire(&ctx.layout, ctx.sys)?;
    let media_roots = media::roots(&ctx.layout);
    let colls: Vec<Collection> = media::loaded(&media::scan(&ctx.layout));
    let state = State::load(&ctx.layout)?;
    match verb {
        "install" => {
            let name = arg(args, 0, "package name")?;
            let coll = install::pick_collection(&state, &colls, name, args.get(1).map(|s| s.as_str()))?;
            install::install(ctx, &media_roots, coll, name)
        }
        "remove" => install::remove(ctx, arg(args, 0, "package name")?, args.iter().any(|a| a == "--purge")),
        "rollback" => install::rollback(ctx, arg(args, 0, "package name")?),
        "restart" => {
            let name = arg(args, 0, "package name")?;
            let inst = state.packages.get(name).with_context(|| format!("{name} is not installed"))?;
            for id in &inst.services {
                control::request_restart(&ctx.layout, id)?;
            }
            Ok(format!("restart requested for {name}"))
        }
        "rescan" => {
            let _rw = ctx.mounts.rw("/mnt/app")?;
            let mut msgs = Vec::new();
            for name in state.packages.keys() {
                if let Ok(m) = install::current_manifest(&ctx.layout, name) {
                    let got = config::import(&ctx.layout, &media_roots, &m)?;
                    if !got.is_empty() {
                        msgs.push(format!("{name}: imported {}", got.join(", ")));
                    }
                }
            }
            control::request_reload(&ctx.layout)?;
            Ok(if msgs.is_empty() { "rescanned; no new config files".into() } else { msgs.join("; ") })
        }
        "update-all" => Ok(install::update_all(ctx, &media_roots, &colls)?.join("; ")),
        other => bail!("unknown action {other:?}"),
    }
}

fn record_result(ctx: &Ctx, key: &str, text: &str) -> Result<()> {
    let _rw = ctx.mounts.rw("/mnt/app")?;
    let mut st = State::load(&ctx.layout)?;
    st.results.insert(key.to_string(), format!("{text} ({})", utc_stamp(now_secs())));
    st.save(&ctx.layout)
}

fn print_status(ctx: &Ctx) {
    match status::read(&ctx.layout) {
        None => println!("supervisor: no status (not running since boot?)"),
        Some(s) => {
            let alive = control::supervisor_pid(&ctx.layout, ctx.sys).is_some();
            println!(
                "supervisor pid {} ({}){}",
                s.pid,
                if alive { "running" } else { "gone" },
                if s.disabled { ", DISABLED" } else { "" }
            );
            for (id, v) in &s.services {
                println!(
                    "  {id}: {}{} restarts {}{}",
                    v.state,
                    v.pid.map(|p| format!(" pid {p}")).unwrap_or_default(),
                    v.restarts,
                    v.error.as_ref().map(|e| format!(" ({e})")).unwrap_or_default()
                );
            }
            for (f, e) in &s.invalid {
                println!("  boot.d/{f}: {e}");
            }
        }
    }
}

fn print_list(ctx: &Ctx) -> Result<()> {
    println!("unit: {}", ctx.unit.describe());
    let st = State::load(&ctx.layout)?;
    for (n, i) in &st.packages {
        println!(
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
                    println!("card [{}] {} {}", c.id(), e.name, e.version);
                }
            }
            Err(e) => println!("card {}: {e:#}", dir.display()),
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
    println!("{}", out.display());
    Ok(0)
}

fn host_index(args: &[String]) -> Result<i32> {
    let dir = Path::new(arg(args, 0, "collection dir")?);
    let mut id = None;
    let mut name = None;
    let mut it = args[1..].iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--id" => id = it.next().cloned(),
            "--name" => name = it.next().cloned(),
            other => bail!("unexpected argument {other:?}"),
        }
    }
    let id = id.context("--id is required")?;
    let name = name.unwrap_or_else(|| id.clone());
    let idx = build_index(dir, &id, &name)?;
    write_index(dir, &idx)?;
    println!("{} packages in {}", idx.packages.len(), dir.join("index.json").display());
    Ok(0)
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
