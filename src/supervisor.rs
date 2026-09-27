//! The service supervisor: starts boot.d services from /tmp copies, restarts
//! them with backoff, gives up on crash loops, keeps capped logs in RAM.
use std::collections::VecDeque;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::time::{Duration, Instant};

#[derive(Debug, Clone)]
pub struct Timing {
    pub backoff_start: Duration,
    pub backoff_max: Duration,
    pub fail_window: Duration,
    pub fail_count: usize,
    pub poll: Duration,
    pub stop_grace: Duration,
    pub config_recheck: Duration,
    /// A service running this long gets its backoff reset.
    pub stable_after: Duration,
}

impl Default for Timing {
    fn default() -> Self {
        Timing {
            backoff_start: Duration::from_secs(1),
            backoff_max: Duration::from_secs(300),
            fail_window: Duration::from_secs(600),
            fail_count: 5,
            poll: Duration::from_secs(1),
            stop_grace: Duration::from_secs(5),
            config_recheck: Duration::from_secs(10),
            stable_after: Duration::from_secs(60),
        }
    }
}

pub fn next_backoff(t: &Timing, prev: Option<Duration>) -> Duration {
    match prev {
        None => t.backoff_start,
        Some(p) => (p * 2).min(t.backoff_max),
    }
}

#[derive(Debug, Default)]
pub struct CrashWindow {
    exits: VecDeque<Instant>,
}

impl CrashWindow {
    /// Record an exit; true when `fail_count` exits fall within `fail_window`.
    pub fn record(&mut self, now: Instant, t: &Timing) -> bool {
        self.exits.push_back(now);
        while let Some(first) = self.exits.front() {
            if now.duration_since(*first) > t.fail_window {
                self.exits.pop_front();
            } else {
                break;
            }
        }
        self.exits.len() >= t.fail_count
    }
}

/// Append-only log that rotates to `<path>.1` once it would exceed `cap` bytes.
pub struct CappedLog {
    path: PathBuf,
    cap: u64,
    file: Option<File>,
    size: u64,
}

impl CappedLog {
    pub fn open(path: PathBuf, cap: u64) -> CappedLog {
        let size = fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
        let file = OpenOptions::new().create(true).append(true).open(&path).ok();
        CappedLog { path, cap, file, size }
    }

    pub fn write(&mut self, data: &[u8]) {
        if self.size + data.len() as u64 > self.cap && self.size > 0 {
            self.file = None;
            let mut rotated = self.path.as_os_str().to_owned();
            rotated.push(".1");
            let _ = crate::fsutil::rename(&self.path, &PathBuf::from(rotated));
            self.file = OpenOptions::new().create(true).write(true).truncate(true).open(&self.path).ok();
            self.size = 0;
        }
        if let Some(f) = self.file.as_mut() {
            if f.write_all(data).is_ok() {
                self.size += data.len() as u64;
            }
        }
    }
}

use crate::bootd::{self, BootEntry};
use crate::conditions::{self, fmt_secs, RealView, SystemView, TickCache};
use crate::control;
use crate::fsutil::{atomic_write, copy_atomic};
use crate::install::{current_dir, current_version};
use crate::manifest::{resolve, Restart};
use crate::media;
use crate::paths::Layout;
use crate::status::{self, now_secs, Status, SvcState, SvcStatus};
use crate::sys::Sys;
use anyhow::{bail, Result};
use std::collections::BTreeMap;
use std::io::Read;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};

/// Size cap for service logs and /tmp/mibpkg.log.
pub const LOG_CAP: u64 = 256 * 1024;

struct Service {
    entry: BootEntry,
    child: Option<Child>,
    state: SvcState,
    starts: u32,
    backoff: Option<Duration>,
    next_start: Option<Instant>,
    started_at: Option<Instant>,
    window: CrashWindow,
    last_exit: Option<String>,
    error: Option<String>,
    /// Package version the running child was started from.
    version: Option<String>,
    /// When the current wait for start conditions began.
    wait_since: Option<Instant>,
    waiting_for: Vec<String>,
    /// Timeout note, e.g. "started without tailnet after 5m"; only for a real start.
    note: Option<String>,
    /// The wait's timeout has fired ("tailnet after 5m"): do not wait again until the next
    /// real start, even if the start is held up by a missing config.
    timed_out: Option<String>,
}

impl Service {
    fn new(entry: BootEntry) -> Service {
        Service {
            entry,
            child: None,
            state: SvcState::Starting,
            starts: 0,
            backoff: None,
            next_start: None,
            started_at: None,
            window: CrashWindow::default(),
            last_exit: None,
            error: None,
            version: None,
            wait_since: None,
            waiting_for: Vec::new(),
            note: None,
            timed_out: None,
        }
    }
}

pub struct Supervisor<'a> {
    layout: &'a Layout,
    sys: &'a dyn Sys,
    timing: Timing,
    services: BTreeMap<String, Service>,
    invalid: BTreeMap<String, String>,
    view: Box<dyn SystemView>,
}

fn pump(mut r: impl Read + Send + 'static, log: Arc<Mutex<CappedLog>>) {
    std::thread::spawn(move || {
        let mut buf = [0u8; 4096];
        loop {
            match r.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    if let Ok(mut l) = log.lock() {
                        l.write(&buf[..n]);
                    }
                }
            }
        }
    });
}

impl<'a> Supervisor<'a> {
    pub fn new(layout: &'a Layout, sys: &'a dyn Sys, timing: Timing) -> Supervisor<'a> {
        Supervisor { layout, sys, timing, services: BTreeMap::new(), invalid: BTreeMap::new(), view: Box::new(RealView::default()) }
    }

    /// Replace how start conditions read the system (tests).
    pub fn with_view(mut self, view: Box<dyn SystemView>) -> Self {
        self.view = view;
        self
    }

    /// Re-read boot.d: start new services, stop removed ones, restart changed
    /// ones and ones whose package `current` version moved.
    pub fn reload(&mut self) {
        self.invalid.clear();
        let mut seen = Vec::new();
        for (file, r) in bootd::load_all(self.layout) {
            let e = match r {
                Ok(e) => e,
                Err(err) => {
                    self.invalid.insert(file, format!("{err:#}"));
                    continue;
                }
            };
            let id = e.id();
            seen.push(id.clone());
            let cur = current_version(self.layout, &e.package);
            // Replace on a changed entry, a moved `current` (even if not running, so an
            // update revives a failed service), or an invalid one (retry after install).
            let keep = match self.services.get(&id) {
                Some(s) => s.entry == e && s.state != SvcState::Invalid && (s.version.is_none() || s.version == cur),
                None => false,
            };
            if !keep {
                if let Some(mut old) = self.services.remove(&id) {
                    self.stop(&mut old);
                }
                self.services.insert(id, Service::new(e));
            }
        }
        let gone: Vec<String> = self.services.keys().filter(|k| !seen.contains(k)).cloned().collect();
        for id in gone {
            if let Some(mut s) = self.services.remove(&id) {
                self.stop(&mut s);
            }
            // The /tmp copy and logs are RAM; nothing reads them once the service is gone.
            for f in [id.clone(), format!("{id}.log"), format!("{id}.log.1")] {
                let _ = fs::remove_file(self.layout.run(&f));
            }
        }
    }

    pub fn restart(&mut self, id: &str) {
        if let Some(mut s) = self.services.remove(id) {
            self.stop(&mut s);
            let fresh = Service::new(s.entry.clone());
            self.services.insert(id.to_string(), fresh);
        }
    }

    pub fn stop_all(&mut self) {
        let ids: Vec<String> = self.services.keys().cloned().collect();
        for id in ids {
            let mut s = self.services.remove(&id).unwrap();
            self.stop(&mut s);
            self.services.insert(id, s);
        }
    }

    fn stop(&self, s: &mut Service) {
        if let Some(mut child) = s.child.take() {
            self.sys.terminate(child.id());
            let end = Instant::now() + self.timing.stop_grace;
            loop {
                match child.try_wait() {
                    Ok(Some(_)) => break,
                    Ok(None) if Instant::now() < end => std::thread::sleep(Duration::from_millis(20)),
                    _ => {
                        let _ = child.kill();
                        let _ = child.wait();
                        break;
                    }
                }
            }
        }
        s.state = SvcState::Stopped;
    }

    pub fn tick(&mut self, now: Instant) {
        let ids: Vec<String> = self.services.keys().cloned().collect();
        let cache = TickCache::new(self.view.as_ref());
        for id in ids {
            let mut s = self.services.remove(&id).unwrap();
            self.tick_one(&mut s, now, &cache);
            self.services.insert(id, s);
        }
    }

    /// Gate a start on the service's `after` conditions. Returns true to start now.
    /// A start with every condition met clears an earlier timeout note.
    fn conditions_allow(&self, s: &mut Service, now: Instant, cache: &TickCache) -> bool {
        let unmet = conditions::unmet(&s.entry.after, &self.layout.settings, cache);
        if unmet.is_empty() {
            s.wait_since = None;
            s.waiting_for.clear();
            s.timed_out = None;
            s.note = None;
            return true;
        }
        let limit = s.entry.after_timeout_secs;
        if s.timed_out.is_some() {
            // Timeout already spent on this wait (the start was held up by something else).
            s.timed_out = Some(format!("{} after {}", unmet.join(", "), fmt_secs(limit.unwrap_or(0))));
            s.waiting_for.clear();
            return true;
        }
        let since = *s.wait_since.get_or_insert(now);
        if let Some(limit) = limit {
            if now.duration_since(since) >= Duration::from_secs(limit) {
                s.timed_out = Some(format!("{} after {}", unmet.join(", "), fmt_secs(limit)));
                s.wait_since = None;
                s.waiting_for.clear();
                return true;
            }
        }
        s.state = SvcState::Waiting;
        s.waiting_for = unmet;
        s.note = None;
        false
    }

    fn tick_one(&self, s: &mut Service, now: Instant, cache: &TickCache) {
        let t = &self.timing;
        if let Some(child) = s.child.as_mut() {
            match child.try_wait() {
                Ok(Some(st)) => {
                    s.child = None;
                    s.last_exit = Some(st.to_string());
                    let restart = match s.entry.restart {
                        Restart::Always => true,
                        Restart::OnFailure => !st.success(),
                        Restart::Never => false,
                    };
                    if !restart {
                        s.state = SvcState::Stopped;
                    } else if s.window.record(now, t) {
                        s.state = SvcState::Failed;
                    } else {
                        let b = next_backoff(t, s.backoff);
                        s.backoff = Some(b);
                        s.next_start = Some(now + b);
                        s.state = SvcState::Backoff;
                    }
                }
                Ok(None) => {
                    if s.started_at.map_or(false, |at| now.duration_since(at) >= t.stable_after) {
                        s.backoff = None;
                    }
                }
                Err(e) => s.error = Some(e.to_string()),
            }
        }
        let due = s.next_start.map_or(true, |at| now >= at);
        if s.child.is_none()
            && matches!(s.state, SvcState::Starting | SvcState::Backoff | SvcState::NeedsConfig | SvcState::Waiting)
            && due
        {
            if !self.conditions_allow(s, now, cache) {
                return;
            }
            self.start(s, now);
        }
    }

    fn start(&self, s: &mut Service, now: Instant) {
        let e = &s.entry;
        let pkg_dir = match current_dir(self.layout, &e.package) {
            Some(d) => d,
            None => {
                s.state = SvcState::Invalid;
                s.error = Some(format!("package {} is not installed", e.package));
                return;
            }
        };
        s.version = current_version(self.layout, &e.package);
        let pkg_s = pkg_dir.display().to_string();
        let cfg_s = self.layout.config_dir(&e.package).display().to_string();
        let res = |x: &str| resolve(x, &pkg_s, &cfg_s);
        if let Some(missing) = e.requires.iter().map(|r| res(r)).find(|p| !Path::new(p).is_file()) {
            s.state = SvcState::NeedsConfig;
            s.error = Some(format!("missing {missing}"));
            s.next_start = Some(now + self.timing.config_recheck);
            return;
        }
        let id = e.id();
        let bin = self.layout.run(&id);
        if let Err(err) = copy_atomic(Path::new(&res(&e.exec)), &bin, 0o755) {
            s.state = SvcState::Invalid;
            s.error = Some(format!("copying {} to /tmp: {err}", e.exec));
            return;
        }
        let log = Arc::new(Mutex::new(CappedLog::open(self.layout.run(&format!("{id}.log")), LOG_CAP)));
        let spawned = Command::new(&bin)
            .args(e.args.iter().map(|a| res(a)))
            .current_dir(&pkg_dir)
            .env("MIBPKG_PKG_DIR", &pkg_s)
            .env("MIBPKG_CONFIG_DIR", &cfg_s)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn();
        match spawned {
            Ok(mut child) => {
                if let Some(o) = child.stdout.take() {
                    pump(o, log.clone());
                }
                if let Some(er) = child.stderr.take() {
                    pump(er, log);
                }
                s.child = Some(child);
                if let Some(t) = s.timed_out.take() {
                    s.note = Some(format!("started without {t}"));
                }
                s.state = SvcState::Running;
                s.starts += 1;
                s.started_at = Some(now);
                s.error = None;
            }
            Err(err) => {
                s.error = Some(format!("starting {id}: {err}"));
                if s.window.record(now, &self.timing) {
                    s.state = SvcState::Failed;
                } else {
                    let b = next_backoff(&self.timing, s.backoff);
                    s.backoff = Some(b);
                    s.next_start = Some(now + b);
                    s.state = SvcState::Backoff;
                }
            }
        }
    }

    pub fn status(&self) -> Status {
        let services = self
            .services
            .iter()
            .map(|(id, s)| {
                (
                    id.clone(),
                    SvcStatus {
                        state: s.state,
                        pid: s.child.as_ref().map(|c| c.id()),
                        restarts: s.starts.saturating_sub(1),
                        last_exit: s.last_exit.clone(),
                        error: s.error.clone(),
                        waiting_for: s.waiting_for.clone(),
                        waiting_secs: s
                            .wait_since
                            .filter(|_| s.state == SvcState::Waiting)
                            .map(|w| Instant::now().duration_since(w).as_secs()),
                        note: s.note.clone(),
                    },
                )
            })
            .collect();
        Status { updated: now_secs(), pid: std::process::id(), disabled: false, services, invalid: self.invalid.clone() }
    }
}

/// Kill service pids recorded by a supervisor that died without stopping them.
/// ... and wait (up to `grace`) for them to exit, so a new copy never overlaps the old one.
fn reap_orphans(layout: &Layout, sys: &dyn Sys, grace: Duration) {
    let mut pids = Vec::new();
    if let Some(prev) = status::read(layout) {
        for s in prev.services.values() {
            if let Some(pid) = s.pid {
                if sys.process_alive(pid) {
                    sys.terminate(pid);
                    pids.push(pid);
                }
            }
        }
    }
    let end = Instant::now() + grace;
    while pids.iter().any(|p| sys.process_alive(*p)) && Instant::now() < end {
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// The supervisor main loop (`mibpkg supervise`).
pub fn run(layout: &Layout, sys: &dyn Sys, timing: Timing) -> Result<()> {
    if let Some(pid) = control::supervisor_pid(layout, sys) {
        if pid != std::process::id() {
            bail!("a supervisor is already running (pid {pid})");
        }
    }
    atomic_write(&layout.run("pid"), std::process::id().to_string().as_bytes())?;
    // Stop/self-update requests left before this supervisor existed are stale.
    let _ = fs::remove_file(layout.run("stop"));
    let _ = fs::remove_file(layout.run("selfupdate"));
    reap_orphans(layout, sys, timing.stop_grace);
    if media::disabled(layout) {
        println!("mibpkg: DISABLE marker on the card; starting nothing");
        status::write(layout, &Status { updated: now_secs(), pid: std::process::id(), disabled: true, ..Default::default() })?;
        let _ = fs::remove_file(layout.run("pid"));
        return Ok(());
    }
    let mut sup = Supervisor::new(layout, sys, timing.clone());
    sup.reload();
    loop {
        if control::take(&layout.run("stop")) {
            sup.stop_all();
            break;
        }
        if control::take(&layout.run("selfupdate")) {
            sup.stop_all();
            let _ = status::write(layout, &sup.status());
            let _ = fs::remove_file(layout.run("pid"));
            Command::new("/bin/sh")
                .arg(layout.bin_dir().join("launch.sh"))
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()?;
            return Ok(());
        }
        if control::take(&layout.run("reload")) {
            sup.reload();
        }
        for id in control::take_restarts(layout) {
            sup.restart(&id);
        }
        sup.tick(Instant::now());
        let _ = crate::fsutil::cap_file(&layout.run("log"), LOG_CAP);
        if let Err(e) = status::write(layout, &sup.status()) {
            eprintln!("mibpkg: writing status: {e:#}");
        }
        std::thread::sleep(timing.poll);
    }
    let _ = status::write(layout, &sup.status());
    let _ = fs::remove_file(layout.run("pid"));
    Ok(())
}
