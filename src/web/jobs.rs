//! Jobs: one `mibpkg gem <verb>` child at a time, with its output kept in memory so the browser
//! can follow it live (GET /api/jobs/<id>?since=N).
use serde_json::{json, Value};
use std::collections::VecDeque;
use std::io::{self, BufRead, BufReader, Write};
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

/// The last `OK: …` or `FAILED: …` line: how `mibpkg gem` reports its outcome.
fn result_line(lines: &VecDeque<String>) -> Option<String> {
    lines.iter().rev().find(|l| l.starts_with("OK:") || l.starts_with("FAILED:")).cloned()
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
        json!({
            "id": self.id,
            "label": self.label,
            "package": self.package,
            "lines": lines,
            "next": s.dropped + s.lines.len(),
            "truncated": s.dropped > 0,
            "done": s.done,
            "ok": ok,
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
/// array, never through a shell), output also appended to the mibpkg log like gem.sh does.
pub struct ProcessRunner {
    pub exe: PathBuf,
    pub root: PathBuf,
    pub log: Option<PathBuf>,
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
        let log = Arc::new(Mutex::new(self.log.clone()));
        let pump = |r: Box<dyn io::Read + Send>, job: Arc<Job>, log: Arc<Mutex<Option<PathBuf>>>| {
            std::thread::spawn(move || {
                for line in BufReader::new(r).lines() {
                    let Ok(line) = line else { break };
                    job.push(&line);
                    if let Some(p) = log.lock().unwrap_or_else(|e| e.into_inner()).as_ref() {
                        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(p) {
                            let _ = writeln!(f, "{line}");
                        }
                    }
                }
            })
        };
        let out = pump(Box::new(child.stdout.take().unwrap()), job.clone(), log.clone());
        let err = pump(Box::new(child.stderr.take().unwrap()), job.clone(), log);
        std::thread::spawn(move || {
            let _ = out.join();
            let _ = err.join();
            let _ = child.wait();
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
