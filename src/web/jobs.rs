//! Jobs: one `mibpkg gem <verb>` child at a time, with its output kept in memory so the browser
//! can follow it live (GET /api/jobs/<id>?since=N).
use serde_json::{json, Value};
use std::collections::VecDeque;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Output kept per job; older lines are dropped first, so the result line always survives.
pub const MAX_OUTPUT: usize = 64 * 1024;
/// Finished jobs stay readable this long.
pub const KEEP_DONE: Duration = Duration::from_secs(10 * 60);

#[derive(Default)]
struct JobState {
    lines: VecDeque<String>,
    /// Lines dropped from the front (line numbers stay stable for `since`).
    dropped: usize,
    bytes: usize,
    done: bool,
    finished_at: Option<Instant>,
}

pub struct Job {
    pub id: String,
    /// What the user asked for, e.g. "Install carmqtt".
    pub label: String,
    /// The package it acts on, if any.
    pub package: Option<String>,
    state: Mutex<JobState>,
}

/// The last `OK: …`, `FAILED: …` or `BUSY: …` line: how `mibpkg gem` reports its outcome.
fn result_line(lines: &VecDeque<String>) -> Option<String> {
    lines.iter().rev().find(|l| l.starts_with("OK:") || l.starts_with("FAILED:") || l.starts_with("BUSY:")).cloned()
}

impl Job {
    pub fn new(label: String, package: Option<String>) -> Job {
        Job { id: super::settings::random_id(), label, package, state: Mutex::new(JobState::default()) }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, JobState> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn push(&self, line: &str) {
        let mut s = self.lock();
        s.bytes += line.len() + 1;
        s.lines.push_back(line.to_string());
        while s.bytes > MAX_OUTPUT && s.lines.len() > 1 {
            let old = s.lines.pop_front().unwrap();
            s.bytes -= old.len() + 1;
            s.dropped += 1;
        }
    }

    /// The child has exited (or could not run).
    pub fn finish(&self) {
        let mut s = self.lock();
        s.done = true;
        s.finished_at = Some(Instant::now());
    }

    pub fn done(&self) -> bool {
        self.lock().done
    }

    fn finished_before(&self, t: Instant) -> bool {
        self.lock().finished_at.map_or(false, |f| f + KEEP_DONE < t)
    }

    /// Lines from number `since` on, plus the outcome once finished. `result` is null when the
    /// child ended without reporting one (e.g. killed by a mibpkg self-update restart).
    pub fn json(&self, since: usize) -> Value {
        let s = self.lock();
        let from = since.max(s.dropped) - s.dropped;
        let lines: Vec<&String> = s.lines.iter().skip(from).collect();
        let result = if s.done { result_line(&s.lines) } else { None };
        let ok = result.as_deref().map_or(false, |r| r.starts_with("OK:"));
        // Another action (GEM) took the lock first: nothing happened, try again later.
        let busy = result.as_deref().map_or(false, |r| r.starts_with("BUSY:"));
        json!({
            "id": self.id,
            "label": self.label,
            "package": self.package,
            "lines": lines,
            "next": s.dropped + s.lines.len(),
            "truncated": s.dropped > 0,
            "done": s.done,
            "ok": ok,
            "busy": busy,
            "result": result,
        })
    }
}

/// Starts a job's process and feeds its output into the job, finishing it at exit.
pub trait Runner: Send + Sync {
    fn start(&self, argv: Vec<String>, job: Arc<Job>) -> io::Result<()>;
}

impl<T: Runner + ?Sized> Runner for Arc<T> {
    fn start(&self, argv: Vec<String>, job: Arc<Job>) -> io::Result<()> {
        (**self).start(argv, job)
    }
}

/// The real runner: `<exe> --root <root> gem <argv…>`, started directly (execve with an argv
/// array, never through a shell).
///
/// The child writes to a file in /tmp, not a pipe: a package script that leaves a daemon holding
/// the output, bytes that are not UTF-8, or this web process restarting mid-action (a mibpkg
/// self-update) can then never stall or kill the child. A thread follows the file into the job
/// and finishes it when the child exits. Lines also go to the mibpkg log, like gem.sh does.
pub struct ProcessRunner {
    pub exe: PathBuf,
    pub root: PathBuf,
    pub log: Option<PathBuf>,
}

/// How often the output file is read while the child runs.
const FOLLOW_EVERY: Duration = Duration::from_millis(100);

/// Push the complete lines in `buf` to the job (and the log), keeping a partial last line.
fn push_lines(buf: &mut Vec<u8>, job: &Job, log: Option<&PathBuf>, flush_partial: bool) {
    while let Some(i) = buf.iter().position(|b| *b == b'\n') {
        let line: Vec<u8> = buf.drain(..=i).collect();
        emit(&line[..line.len() - 1], job, log);
    }
    if flush_partial && !buf.is_empty() {
        let rest = std::mem::take(buf);
        emit(&rest, job, log);
    }
}

fn emit(bytes: &[u8], job: &Job, log: Option<&PathBuf>) {
    let line = String::from_utf8_lossy(bytes);
    let line = line.trim_end_matches('\r');
    job.push(line);
    if let Some(p) = log {
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(p) {
            let _ = writeln!(f, "{line}");
        }
    }
}

/// The bytes of `path` from byte `offset` on, and the offset after them. The file is opened
/// fresh and read from an absolute position every time: on QNX 6.5's /tmp (shared memory) a
/// follow-the-file reader that relied on the descriptor's own position re-read the output from
/// byte 6 on each poll and lost the first 6 bytes (seen on the unit, 2026-10-05).
pub fn read_new(path: &Path, offset: u64) -> (Vec<u8>, u64) {
    use std::io::{Read, Seek, SeekFrom};
    let mut out = Vec::new();
    let Ok(mut f) = std::fs::File::open(path) else { return (out, offset) };
    if f.seek(SeekFrom::Start(offset)).is_err() {
        return (out, offset);
    }
    let mut chunk = [0u8; 8192];
    loop {
        match f.read(&mut chunk) {
            Ok(0) | Err(_) => break,
            Ok(n) => out.extend_from_slice(&chunk[..n]),
        }
    }
    let next = offset + out.len() as u64;
    (out, next)
}

impl Runner for ProcessRunner {
    fn start(&self, argv: Vec<String>, job: Arc<Job>) -> io::Result<()> {
        let out_path = self.root.join("tmp").join(format!("mibpkg.job.{}", job.id));
        // Opened twice, not try_clone: that needs F_DUPFD_CLOEXEC, which QNX 6.5 rejects.
        let open = || std::fs::OpenOptions::new().create(true).write(true).append(true).open(&out_path);
        let mut child = Command::new(&self.exe)
            .arg("--root")
            .arg(&self.root)
            .arg("gem")
            .args(&argv)
            .stdin(Stdio::null())
            .stdout(open()?)
            .stderr(open()?)
            .spawn()?;
        let log = self.log.clone();
        let root = self.root.clone();
        std::thread::spawn(move || {
            let mut buf = Vec::new();
            let mut offset = 0u64;
            loop {
                let exited = matches!(child.try_wait(), Ok(Some(_)) | Err(_));
                // Read whatever was written since last time (after the exit, that is everything).
                let (new, next) = read_new(&out_path, offset);
                offset = next;
                buf.extend_from_slice(&new);
                push_lines(&mut buf, &job, log.as_ref(), exited);
                if exited {
                    break;
                }
                std::thread::sleep(FOLLOW_EVERY);
            }
            let _ = std::fs::remove_file(&out_path);
            // Text handed to the verb (config contents, settings) is normally deleted by the verb;
            // if it never ran (busy, bad arguments), delete it here: /tmp is RAM.
            for id in argv.iter().filter(|a| super::settings::is_id(a)) {
                let _ = std::fs::remove_file(root.join("tmp").join(format!("mibpkg.text.{id}")));
            }
            job.finish();
        });
        Ok(())
    }
}

/// Test double: records argv; finishes each job with "OK: recorded" unless holding.
#[derive(Default)]
pub struct RecordingRunner {
    seen: Mutex<Vec<Vec<String>>>,
    hold: bool,
}

impl RecordingRunner {
    /// Jobs never finish on their own (to test "busy").
    pub fn holding() -> RecordingRunner {
        RecordingRunner { seen: Mutex::new(Vec::new()), hold: true }
    }

    pub fn seen(&self) -> Vec<Vec<String>> {
        self.seen.lock().unwrap().clone()
    }
}

impl Runner for RecordingRunner {
    fn start(&self, argv: Vec<String>, job: Arc<Job>) -> io::Result<()> {
        self.seen.lock().unwrap().push(argv);
        if !self.hold {
            job.push("OK: recorded");
            job.finish();
        }
        Ok(())
    }
}

impl std::fmt::Debug for Job {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Job({} {})", self.id, self.label)
    }
}

#[derive(Debug)]
pub enum StartError {
    /// Another job is running (it is returned so the browser can follow it).
    Busy(Option<Arc<Job>>),
    Spawn(io::Error),
}

#[derive(Default)]
struct JobsInner {
    running: Option<Arc<Job>>,
    done: Vec<Arc<Job>>,
}

/// The job table: at most one running job, finished ones kept for `KEEP_DONE`.
#[derive(Default)]
pub struct Jobs {
    inner: Mutex<JobsInner>,
}

impl Jobs {
    fn lock(&self) -> std::sync::MutexGuard<'_, JobsInner> {
        let mut g = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        if g.running.as_ref().map_or(false, |j| j.done()) {
            let j = g.running.take().unwrap();
            g.done.push(j);
        }
        g
    }

    pub fn start(&self, runner: &dyn Runner, label: String, package: Option<String>, argv: Vec<String>) -> Result<Arc<Job>, StartError> {
        let mut g = self.lock();
        if let Some(j) = &g.running {
            return Err(StartError::Busy(Some(j.clone())));
        }
        let job = Arc::new(Job::new(label, package));
        runner.start(argv, job.clone()).map_err(StartError::Spawn)?;
        g.running = Some(job.clone());
        Ok(job)
    }

    pub fn get(&self, id: &str) -> Option<Arc<Job>> {
        let g = self.lock();
        g.running.iter().chain(g.done.iter()).find(|j| j.id == id).cloned()
    }

    pub fn running(&self) -> Option<Arc<Job>> {
        self.lock().running.clone()
    }

    pub fn sweep(&self) {
        self.sweep_at(Instant::now())
    }

    /// Forget jobs finished more than `KEEP_DONE` before `now`.
    pub fn sweep_at(&self, now: Instant) {
        self.lock().done.retain(|j| !j.finished_before(now));
    }
}

/// Delete /tmp/mibpkg.text.* and /tmp/mibpkg.job.* left by an earlier web process (it may have
/// been restarted mid-action). Run once at start, before any job of this process exists.
pub fn sweep_leftovers(layout: &crate::paths::Layout) {
    if let Ok(rd) = std::fs::read_dir(layout.p("/tmp")) {
        for e in rd.filter_map(|e| e.ok()) {
            let name = e.file_name().to_string_lossy().to_string();
            if name.starts_with("mibpkg.text.") || name.starts_with("mibpkg.job.") {
                let _ = std::fs::remove_file(e.path());
            }
        }
    }
}
