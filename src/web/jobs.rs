//! Jobs: one `mibpkg gem <verb>` child at a time, with its output kept in memory so the browser
//! can follow it live (GET /api/jobs/<id>?since=N).
use serde_json::{json, Value};
use std::collections::VecDeque;
use std::io::{self, Write};
use std::path::PathBuf;
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
/// array, never through a shell). Lines also go to the mibpkg log, like gem.sh does.
///
/// Output comes through pipes. (Following a file in /tmp was tried: QNX 6.5's /tmp is shared
/// memory, and a reader there lost the first bytes of the output, seen on the unit 2026-10-05.)
/// Bytes are read raw and decoded lossily, so odd output cannot stop the reader; the job ends
/// when the child exits, after a short grace for the rest of its output, so a daemon that a
/// package script leaves holding the pipe cannot keep the job running.
pub struct ProcessRunner {
    pub exe: PathBuf,
    pub root: PathBuf,
    pub log: Option<PathBuf>,
}

/// How often the runner looks for new output and for the child's exit.
const FOLLOW_EVERY: Duration = Duration::from_millis(100);
/// After the child exits, how long to wait for output still in the pipes.
const EXIT_GRACE: Duration = Duration::from_secs(1);

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

/// Read a pipe until EOF, sending each chunk as (stream, bytes).
fn pump(mut r: impl io::Read + Send + 'static, stream: usize, tx: std::sync::mpsc::Sender<(usize, Vec<u8>)>) {
    std::thread::spawn(move || {
        let mut chunk = [0u8; 4096];
        loop {
            match r.read(&mut chunk) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    if tx.send((stream, chunk[..n].to_vec())).is_err() {
                        break;
                    }
                }
            }
        }
    });
}

impl Runner for ProcessRunner {
    fn start(&self, argv: Vec<String>, job: Arc<Job>) -> io::Result<()> {
        let mut child = Command::new(&self.exe)
            .arg("--root")
            .arg(&self.root)
            .arg("gem")
            .args(&argv)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;
        let (tx, rx) = std::sync::mpsc::channel();
        pump(child.stdout.take().unwrap(), 0, tx.clone());
        pump(child.stderr.take().unwrap(), 1, tx);
        let log = self.log.clone();
        let root = self.root.clone();
        std::thread::spawn(move || {
            // One buffer per stream, so a partial stdout line never mixes with stderr.
            let mut bufs = [Vec::new(), Vec::new()];
            let mut exited_at: Option<Instant> = None;
            loop {
                match rx.recv_timeout(FOLLOW_EVERY) {
                    Ok((stream, bytes)) => {
                        bufs[stream].extend_from_slice(&bytes);
                        push_lines(&mut bufs[stream], &job, log.as_ref(), false);
                    }
                    // Both pipes closed: all output is in.
                    Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
                    Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
                }
                if exited_at.is_none() && matches!(child.try_wait(), Ok(Some(_)) | Err(_)) {
                    exited_at = Some(Instant::now());
                }
                // Exited, but something it started still holds the pipes: stop waiting.
                if exited_at.map_or(false, |t| t.elapsed() >= EXIT_GRACE) {
                    break;
                }
            }
            for b in bufs.iter_mut() {
                push_lines(b, &job, log.as_ref(), true);
            }
            let _ = child.wait();
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
