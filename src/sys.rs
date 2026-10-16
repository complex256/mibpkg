//! Every side effect on the unit that is not plain file I/O.
use anyhow::{bail, Context, Result};
use std::cell::{Cell, RefCell};
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

pub trait Sys {
    /// `mount -uw|-ur <mount>` (sync first when going read-only).
    fn remount(&self, mount: &str, rw: bool) -> Result<()>;
    /// `ksh -n <script>` succeeded.
    fn shell_syntax_ok(&self, script: &Path) -> bool;
    /// `/bin/sh <script> <args>` with a timeout; non-zero exit is an error.
    fn run_script(&self, script: &Path, args: &[&str], env: &[(String, String)], timeout: Duration) -> Result<()>;
    fn process_alive(&self, pid: u32) -> bool;
    /// SIGTERM via `kill <pid>`.
    fn terminate(&self, pid: u32);
    /// Free bytes on the filesystem holding `path`, if `df` can tell.
    fn free_bytes(&self, path: &Path) -> Option<u64>;
    /// Store a string in the unit's persistence (read live by GEM `per` rows).
    fn persist_write(&self, ns: u32, key: u32, value: &str) -> Result<()>;
    /// Store a single byte in the unit's persistence (`pc b:ns:key`). The GEM dev-mode flag is a
    /// byte; writing it as a string hangs pc on a type mismatch (found on-car 2026-10-16).
    fn persist_write_byte(&self, ns: u32, key: u32, value: u8) -> Result<()>;
}

/// Run `cmd args` with a deadline, stdin closed: (exited successfully, stdout+stderr).
pub fn run_limited(cmd: &Path, args: &[String], env: &[(String, String)], timeout: Duration) -> std::result::Result<(bool, String), String> {
    let name = cmd.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    let mut c = Command::new(cmd);
    c.args(args).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null());
    for (k, v) in env {
        c.env(k, v);
    }
    let mut child = c.spawn().map_err(|e| format!("{name}: {e}"))?;
    // Read on a thread so a full pipe can't stall the child while we wait on it.
    let mut out = child.stdout.take().unwrap();
    let reader = std::thread::spawn(move || {
        let mut s = String::new();
        let _ = std::io::Read::read_to_string(&mut out, &mut s);
        s
    });
    let end = Instant::now() + timeout;
    loop {
        match child.try_wait() {
            Ok(Some(st)) => return Ok((st.success(), reader.join().unwrap_or_default())),
            Ok(None) if Instant::now() < end => std::thread::sleep(Duration::from_millis(20)),
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!("{name}: timed out after {}s", timeout.as_secs().max(1)));
            }
            Err(e) => return Err(format!("{name}: {e}")),
        }
    }
}

/// Whether `pid` is running, from `<proc>/<pid>` (QNX and Linux list each process there), or None
/// when this system has no usable /proc (no `<proc>/self`). Starting `kill -0` through a shell for
/// every check was slow on the unit, and the web server checks on every status request.
pub fn alive_in_proc(proc_root: &Path, pid: u32) -> Option<bool> {
    if !proc_root.join("self").exists() {
        return None;
    }
    Some(proc_root.join(pid.to_string()).exists())
}

/// The unit's persistence client; aborts (and writes a core dump) without IPL_CONFIG_DIR.
pub const PC: &str = "/eso/bin/apps/pc";

pub struct RealSys {
    pub mounts: bool,
}

impl RealSys {
    /// `pc <ty>:<ns>:<key> <value>` with IPL_CONFIG_DIR set. `ty` is 's' (string) or 'b' (byte).
    fn pc_write(&self, ty: char, ns: u32, key: u32, value: &str) -> Result<()> {
        if !self.mounts {
            return Ok(()); // not on the unit (tests / --root): there is no persistence service
        }
        let env = [("IPL_CONFIG_DIR".to_string(), "/etc/eso/production".to_string())];
        let args = [format!("{ty}:{ns}:{key}"), value.to_string()];
        let (ok, out) = run_limited(Path::new(PC), &args, &env, Duration::from_secs(5)).map_err(anyhow::Error::msg)?;
        // pc prints "[PersistenceClient::...] error: ..." on failure.
        if !ok || out.contains("error") {
            bail!("pc {ty}:{ns}:{key}: {}", out.trim());
        }
        Ok(())
    }
}

fn quiet_status(cmd: &mut Command) -> bool {
    cmd.stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

impl Sys for RealSys {
    fn remount(&self, mount: &str, rw: bool) -> Result<()> {
        if !self.mounts {
            return Ok(());
        }
        let flag = if rw { "-uw" } else { "-ur" };
        if !rw {
            let _ = Command::new("sync").status();
        }
        let st = Command::new("mount").arg(flag).arg(mount).status().context("running mount")?;
        if !st.success() {
            bail!("mount {flag} {mount} failed");
        }
        Ok(())
    }

    fn shell_syntax_ok(&self, script: &Path) -> bool {
        quiet_status(Command::new("ksh").arg("-n").arg(script))
    }

    fn run_script(&self, script: &Path, args: &[&str], env: &[(String, String)], timeout: Duration) -> Result<()> {
        let mut c = Command::new("/bin/sh");
        c.arg(script).args(args).stdin(Stdio::null());
        for (k, v) in env {
            c.env(k, v);
        }
        let mut child = c.spawn().with_context(|| format!("starting {}", script.display()))?;
        let start = Instant::now();
        loop {
            if let Some(st) = child.try_wait()? {
                if st.success() {
                    return Ok(());
                }
                bail!("{} exited with {st}", script.display());
            }
            if start.elapsed() > timeout {
                let _ = child.kill();
                let _ = child.wait();
                bail!("{} timed out after {}s", script.display(), timeout.as_secs());
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    fn process_alive(&self, pid: u32) -> bool {
        match alive_in_proc(Path::new("/proc"), pid) {
            Some(alive) => alive,
            None => quiet_status(Command::new("/bin/sh").arg("-c").arg(format!("kill -0 {pid}"))),
        }
    }

    fn terminate(&self, pid: u32) {
        quiet_status(Command::new("/bin/sh").arg("-c").arg(format!("kill {pid}")));
    }

    fn persist_write(&self, ns: u32, key: u32, value: &str) -> Result<()> {
        self.pc_write('s', ns, key, &value.to_string())
    }

    fn persist_write_byte(&self, ns: u32, key: u32, value: u8) -> Result<()> {
        self.pc_write('b', ns, key, &value.to_string())
    }

    fn free_bytes(&self, path: &Path) -> Option<u64> {
        let out = Command::new("df").arg("-k").arg(path).stderr(Stdio::null()).output().ok()?;
        let text = String::from_utf8_lossy(&out.stdout);
        let last = text.lines().last()?;
        let avail_kb: u64 = last.split_whitespace().nth(3)?.parse().ok()?;
        Some(avail_kb * 1024)
    }
}

/// Test double: records remounts, can fail them, fakes `ksh -n` and `df`;
/// processes and scripts are real (host /bin/sh).
#[derive(Default)]
pub struct RecordingSys {
    pub calls: RefCell<Vec<String>>,
    pub fail_remount_rw: Cell<bool>,
    pub syntax_bad: Cell<bool>,
    pub free: Cell<Option<u64>>,
    pub persisted: RefCell<Vec<(u32, u32, String)>>,
    pub persisted_bytes: RefCell<Vec<(u32, u32, u8)>>,
    pub fail_persist: Cell<bool>,
}

const HOST: RealSys = RealSys { mounts: false };

impl Sys for RecordingSys {
    fn remount(&self, mount: &str, rw: bool) -> Result<()> {
        self.calls.borrow_mut().push(format!("{} {mount}", if rw { "rw" } else { "ro" }));
        if rw && self.fail_remount_rw.get() {
            bail!("mount -uw {mount} failed");
        }
        Ok(())
    }
    fn shell_syntax_ok(&self, _script: &Path) -> bool {
        !self.syntax_bad.get()
    }
    fn run_script(&self, script: &Path, args: &[&str], env: &[(String, String)], timeout: Duration) -> Result<()> {
        HOST.run_script(script, args, env, timeout)
    }
    fn process_alive(&self, pid: u32) -> bool {
        HOST.process_alive(pid)
    }
    fn terminate(&self, pid: u32) {
        HOST.terminate(pid)
    }
    fn free_bytes(&self, _path: &Path) -> Option<u64> {
        self.free.get()
    }
    fn persist_write(&self, ns: u32, key: u32, value: &str) -> Result<()> {
        self.persisted.borrow_mut().push((ns, key, value.to_string()));
        if self.fail_persist.get() {
            bail!("pc s:{ns}:{key}: error: result == TIMEOUT");
        }
        Ok(())
    }
    fn persist_write_byte(&self, ns: u32, key: u32, value: u8) -> Result<()> {
        self.persisted_bytes.borrow_mut().push((ns, key, value));
        if self.fail_persist.get() {
            bail!("pc b:{ns}:{key}: error: result == TIMEOUT");
        }
        Ok(())
    }
}
