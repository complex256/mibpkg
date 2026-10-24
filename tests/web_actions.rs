//! Typed actions (no client-supplied commands) and the job table that runs them.
use mibpkg::web::actions::{parse_action, validate, Known};
use mibpkg::web::jobs::{Job, Jobs, ProcessRunner, RecordingRunner, Runner, StartError};
use std::collections::BTreeSet;
use std::os::unix::fs::PermissionsExt;
use std::sync::Arc;
use std::time::{Duration, Instant};

fn known() -> Known {
    let set = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<BTreeSet<_>>();
    Known { installed: set(&["carmqtt", "mibpkg"]), available: set(&["carmqtt", "extra"]), collections: set(&["complex256", "community"]) }
}

const HOSTILE: &[&str] = &[";reboot", "$(id)", "`id`", "a\nb", "--root", "-x", "../../etc", "ü", " carmqtt", "CARMQTT", "carmqtt;", ""];

fn argv(json: &str) -> Result<Vec<String>, String> {
    let a = parse_action(json.as_bytes())?;
    validate(&a, &known()).map(|v| v.argv)
}

#[test]
fn valid_actions_build_fixed_argv() {
    assert_eq!(argv(r#"{"action":"restart","package":"carmqtt"}"#).unwrap(), ["restart", "carmqtt"]);
    assert_eq!(argv(r#"{"action":"install","package":"extra","collection":"community"}"#).unwrap(), ["install", "extra", "community"]);
    assert_eq!(argv(r#"{"action":"install","package":"extra"}"#).unwrap(), ["install", "extra"]);
    assert_eq!(argv(r#"{"action":"remove","package":"carmqtt","purge":true}"#).unwrap(), ["remove", "carmqtt", "--purge"]);
    assert_eq!(argv(r#"{"action":"rollback","package":"carmqtt"}"#).unwrap(), ["rollback", "carmqtt"]);
    assert_eq!(argv(r#"{"action":"update-all"}"#).unwrap(), ["update-all"]);
    assert_eq!(argv(r#"{"action":"rescan"}"#).unwrap(), ["rescan"]);
    assert_eq!(argv(r#"{"action":"check"}"#).unwrap(), ["check"]);
    assert_eq!(argv(r#"{"action":"config-resolve","package":"carmqtt","config":0,"choice":"keep"}"#).unwrap(), ["config-resolve", "carmqtt", "keep", "0"]);
    assert_eq!(argv(r#"{"action":"config-resolve","package":"carmqtt","config":2,"choice":"card"}"#).unwrap(), ["config-resolve", "carmqtt", "card", "2"]);
    assert_eq!(argv(r#"{"action":"uninstall-self"}"#).unwrap(), ["uninstall-self"]);
    assert_eq!(argv(r#"{"action":"uninstall-self","purge":true}"#).unwrap(), ["uninstall-self", "--purge"]);
}

#[test]
fn every_hostile_string_in_every_field_is_rejected() {
    let long = "x".repeat(5000);
    let mut bad: Vec<&str> = HOSTILE.to_vec();
    bad.push(&long);
    for s in bad {
        let s = serde_json::to_string(s).unwrap();
        for json in [
            format!(r#"{{"action":"restart","package":{s}}}"#),
            format!(r#"{{"action":"rollback","package":{s}}}"#),
            format!(r#"{{"action":"remove","package":{s}}}"#),
            format!(r#"{{"action":"install","package":{s}}}"#),
            format!(r#"{{"action":"install","package":"extra","collection":{s}}}"#),
            format!(r#"{{"action":"config-resolve","package":{s},"config":0,"choice":"keep"}}"#),
            format!(r#"{{"action":"config-resolve","package":"carmqtt","config":0,"choice":{s}}}"#),
        ] {
            assert!(argv(&json).is_err(), "{json}");
        }
    }
}

#[test]
fn unknown_actions_extra_fields_and_wrong_targets_are_rejected() {
    for json in [
        r#"{"action":"exec","command":"reboot"}"#,
        r#"{"action":"restart","package":"carmqtt","args":["x"]}"#,
        r#"{"action":"rescan","x":1}"#,
        r#"{"action":"config-resolve","package":"carmqtt","config":-1,"choice":"keep"}"#,
        r#"{"action":"config-resolve","package":"carmqtt","config":"../x","choice":"keep"}"#,
        r#"{"action":"config-resolve","package":"extra","config":0,"choice":"keep"}"#,
        r#"{"package":"carmqtt"}"#,
        r#"not json"#,
        r#"{"action":"restart","package":"extra"}"#,
        r#"{"action":"install","package":"nothing-here"}"#,
        r#"{"action":"install","package":"extra","collection":"-"}"#,
        r#"{"action":"install","package":"extra","collection":"elsewhere"}"#,
    ] {
        assert!(argv(json).is_err(), "{json}");
    }
}

#[test]
fn a_second_job_while_one_runs_is_busy() {
    let jobs = Jobs::default();
    let runner = RecordingRunner::holding();
    let first = jobs.start(&runner, "Restart carmqtt".into(), None, vec!["restart".into(), "carmqtt".into()]).unwrap();
    match jobs.start(&runner, "Check".into(), None, vec!["check".into()]) {
        Err(StartError::Busy(Some(j))) => assert_eq!(j.id, first.id),
        _ => panic!("expected busy"),
    }
    assert_eq!(runner.seen(), vec![vec!["restart".to_string(), "carmqtt".to_string()]]);
    first.finish();
    assert!(jobs.start(&runner, "Check".into(), None, vec!["check".into()]).is_ok());
}

#[test]
fn job_lines_page_and_the_result_is_the_last_ok_or_failed_line() {
    let job = Job::new("Install x".into(), Some("x".into()));
    job.push("checking x 1.0.0");
    job.push("OK: installed x 1.0.0");
    let v = job.json(0);
    assert_eq!(v["lines"].as_array().unwrap().len(), 2);
    assert_eq!(v["next"], 2);
    assert_eq!(v["done"], false);
    assert_eq!(job.json(1)["lines"][0], "OK: installed x 1.0.0");
    job.finish();
    let v = job.json(2);
    assert_eq!(v["done"], true);
    assert_eq!(v["ok"], true);
    assert_eq!(v["result"], "OK: installed x 1.0.0");
    assert!(v["lines"].as_array().unwrap().is_empty());
}

#[test]
fn a_job_without_a_result_line_has_no_result() {
    // A self-update restarts everything, killing the child mid-way.
    let job = Job::new("Update mibpkg".into(), Some("mibpkg".into()));
    job.push("downloading mibpkg 0.4.1");
    job.finish();
    let v = job.json(0);
    assert_eq!(v["done"], true);
    assert!(v["result"].is_null());
    assert_eq!(v["ok"], false);
}

#[test]
fn output_is_capped_but_keeps_the_newest_lines() {
    let job = Job::new("Noisy".into(), None);
    for i in 0..5000 {
        job.push(&format!("line {i} {}", "x".repeat(40)));
    }
    job.push("FAILED: it broke");
    job.finish();
    let v = job.json(0);
    let lines = v["lines"].as_array().unwrap();
    let total: usize = lines.iter().map(|l| l.as_str().unwrap().len() + 1).sum();
    assert!(total <= 64 * 1024, "{total}");
    assert_eq!(lines.last().unwrap(), "FAILED: it broke");
    assert_eq!(v["truncated"], true);
    assert_eq!(v["next"], 5001);
}

#[test]
fn finished_jobs_expire_after_ten_minutes() {
    let jobs = Jobs::default();
    let runner = RecordingRunner::default();
    let j = jobs.start(&runner, "Check".into(), None, vec!["check".into()]).unwrap();
    assert!(jobs.get(&j.id).is_some());
    jobs.sweep_at(Instant::now() + Duration::from_secs(9 * 60));
    assert!(jobs.get(&j.id).is_some());
    jobs.sweep_at(Instant::now() + Duration::from_secs(11 * 60));
    assert!(jobs.get(&j.id).is_none());
}

#[test]
fn the_process_runner_passes_argv_without_a_shell() {
    let t = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(t.path().join("tmp")).unwrap();
    let exe = t.path().join("fake-mibpkg");
    // Prints each argument on its own line, then a result line.
    std::fs::write(&exe, "#!/bin/sh\nfor a in \"$@\"; do echo \"arg:$a\"; done\necho 'OK: done'\n").unwrap();
    std::fs::set_permissions(&exe, std::fs::Permissions::from_mode(0o755)).unwrap();
    let runner = ProcessRunner { exe, root: t.path().to_path_buf(), log: Some(t.path().join("log")) };
    let job = Arc::new(Job::new("Test".into(), None));
    runner.start(vec!["restart".into(), "$(echo pwned)".into()], job.clone()).unwrap();
    let end = Instant::now() + Duration::from_secs(5);
    while !job.json(0)["done"].as_bool().unwrap() && Instant::now() < end {
        std::thread::sleep(Duration::from_millis(20));
    }
    let v = job.json(0);
    let lines: Vec<&str> = v["lines"].as_array().unwrap().iter().map(|l| l.as_str().unwrap()).collect();
    assert_eq!(lines, ["arg:--root", &format!("arg:{}", t.path().display()), "arg:gem", "arg:restart", "arg:$(echo pwned)", "OK: done"]);
    assert_eq!(v["ok"], true);
    assert!(std::fs::read_to_string(t.path().join("log")).unwrap().contains("OK: done"));
}

/// Run `script` as the fake mibpkg through the real runner; return the finished job (or the
/// unfinished one after `wait`).
fn run_script(script: &str, wait: Duration) -> (serde_json::Value, tempfile::TempDir) {
    let t = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(t.path().join("tmp")).unwrap();
    let exe = t.path().join("fake-mibpkg");
    std::fs::write(&exe, script).unwrap();
    std::fs::set_permissions(&exe, std::fs::Permissions::from_mode(0o755)).unwrap();
    let runner = ProcessRunner { exe, root: t.path().to_path_buf(), log: None };
    let job = Arc::new(Job::new("Test".into(), None));
    runner.start(vec!["x".into()], job.clone()).unwrap();
    let end = Instant::now() + wait;
    while !job.done() && Instant::now() < end {
        std::thread::sleep(Duration::from_millis(20));
    }
    (job.json(0), t)
}

#[test]
fn output_that_is_not_utf8_is_kept_and_does_not_stop_the_job() {
    let (v, _t) = run_script("#!/bin/sh\nprintf 'bad \\377\\376 bytes\\n'\necho 'after'\necho 'OK: done'\n", Duration::from_secs(5));
    assert_eq!(v["done"], true);
    let lines: Vec<&str> = v["lines"].as_array().unwrap().iter().map(|l| l.as_str().unwrap()).collect();
    assert_eq!(lines.len(), 3, "{lines:?}");
    assert!(lines[0].starts_with("bad "));
    assert_eq!(v["result"], "OK: done");
}

#[test]
fn a_background_process_holding_the_output_does_not_keep_the_job_running() {
    // A package script that starts a daemon which inherits stdout.
    let (v, _t) = run_script("#!/bin/sh\nsleep 30 &\necho 'OK: started'\n", Duration::from_secs(3));
    assert_eq!(v["done"], true, "{v}");
    assert_eq!(v["result"], "OK: started");
}

#[test]
fn a_busy_result_is_reported_as_busy_not_failed() {
    let job = Job::new("Restart x".into(), Some("x".into()));
    job.push("BUSY: another mibpkg action is running (pid 123)");
    job.finish();
    let v = job.json(0);
    assert_eq!(v["busy"], true);
    assert_eq!(v["ok"], false);
    assert_eq!(v["result"], "BUSY: another mibpkg action is running (pid 123)");
}

#[test]
fn gem_prints_busy_when_another_action_holds_the_lock() {
    let t = tempfile::tempdir().unwrap();
    for d in ["mnt/app/root", "tmp"] {
        std::fs::create_dir_all(t.path().join(d)).unwrap();
    }
    // This test process is alive, so the lock counts as held.
    std::fs::write(t.path().join("tmp/mibpkg.lock"), std::process::id().to_string()).unwrap();
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_mibpkg"))
        .args(["--root", t.path().to_str().unwrap(), "--no-supervisor", "gem", "rescan"])
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.starts_with("BUSY: "), "{text}");
}

#[test]
fn leftover_text_and_job_files_are_removed_at_start() {
    let t = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(t.path().join("tmp")).unwrap();
    let layout = mibpkg::paths::Layout::load(t.path()).unwrap();
    for f in ["text.0123456789abcdef0123456789abcdef", "job.0123456789abcdef0123456789abcdef", "log"] {
        std::fs::write(layout.run(f), "x").unwrap();
    }
    mibpkg::web::jobs::sweep_leftovers(&layout);
    assert!(!layout.run("text.0123456789abcdef0123456789abcdef").exists());
    assert!(!layout.run("job.0123456789abcdef0123456789abcdef").exists());
    assert!(layout.run("log").exists(), "other mibpkg files are left alone");
}

#[test]
fn a_text_file_the_verb_never_read_is_deleted_when_the_job_ends() {
    let id = "0123456789abcdef0123456789abcdef";
    let t = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(t.path().join("tmp")).unwrap();
    let text = t.path().join(format!("tmp/mibpkg.text.{id}"));
    std::fs::write(&text, "broker").unwrap();
    let exe = t.path().join("fake-mibpkg");
    std::fs::write(&exe, "#!/bin/sh\necho 'BUSY: another mibpkg action is running'\n").unwrap();
    std::fs::set_permissions(&exe, std::fs::Permissions::from_mode(0o755)).unwrap();
    let runner = ProcessRunner { exe, root: t.path().to_path_buf(), log: None };
    let job = Arc::new(Job::new("Save".into(), None));
    runner.start(vec!["config-set".into(), "demo".into(), "0".into(), id.into()], job.clone()).unwrap();
    let end = Instant::now() + Duration::from_secs(5);
    while !job.done() && Instant::now() < end {
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(!text.exists());
}

#[test]
fn gem_survives_its_output_pipe_closing() {
    // A mibpkg self-update restarts the web server mid-action, closing the pipe the child writes
    // to; the child must finish its work, not abort on the broken pipe.
    let t = tempfile::tempdir().unwrap();
    for d in ["mnt/app/root", "tmp"] {
        std::fs::create_dir_all(t.path().join(d)).unwrap();
    }
    let mut child = std::process::Command::new(env!("CARGO_BIN_EXE_mibpkg"))
        .args(["--root", t.path().to_str().unwrap(), "--no-supervisor", "gem", "rescan"])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    drop(child.stdout.take());
    drop(child.stderr.take());
    let status = child.wait().unwrap();
    assert!(status.success(), "{status:?}");
    let st = mibpkg::state::State::load(&mibpkg::paths::Layout::load(t.path()).unwrap()).unwrap();
    assert!(st.results["_"].starts_with("OK: rescanned"), "the action ran: {:?}", st.results);
}

#[test]
fn install_action_with_approve_builds_argv_with_flag() {
    assert_eq!(
        argv(r#"{"action":"install","package":"extra","approve":["vehicle.read","network"]}"#).unwrap(),
        ["install", "extra", "--approve=vehicle.read,network"]
    );
    assert_eq!(
        argv(r#"{"action":"install","package":"carmqtt","collection":"community","approve":["platform.unrestricted"]}"#).unwrap(),
        ["install", "carmqtt", "community", "--approve=platform.unrestricted"]
    );
    // An empty list adds no flag: the install then asks instead of approving nothing.
    assert_eq!(argv(r#"{"action":"install","package":"extra","approve":[]}"#).unwrap(), ["install", "extra"]);
}

#[test]
fn install_action_rejects_bad_permission_strings() {
    let long = "x".repeat(200);
    for bad in ["a b", "--x", "-x", "", "a,b", "a=b", "ü", "$(id)", "a\nb", long.as_str()] {
        let s = serde_json::to_string(bad).unwrap();
        let json = format!(r#"{{"action":"install","package":"extra","approve":["network",{s}]}}"#);
        assert!(argv(&json).is_err(), "{json}");
    }
    assert!(argv(r#"{"action":"install","package":"extra","approve":"network"}"#).is_err(), "a list, not a string");
}

#[test]
fn grant_action_builds_sdk_grant_argv() {
    assert_eq!(
        argv(r#"{"action":"grant","id":"carmqtt","permissions":["vehicle.read","network"]}"#).unwrap(),
        ["sdk", "grant", "carmqtt", "vehicle.read,network"]
    );
    // Nothing checked revokes every grant (the plugin stops running).
    assert_eq!(argv(r#"{"action":"grant","id":"carmqtt","permissions":[]}"#).unwrap(), ["sdk", "grant", "carmqtt", ""]);
    let v = validate(&parse_action(br#"{"action":"grant","id":"carmqtt","permissions":[]}"#).unwrap(), &known()).unwrap();
    assert_eq!(v.package.as_deref(), Some("carmqtt"));
    assert!(v.label.contains("carmqtt"), "{}", v.label);
}

#[test]
fn grant_action_rejects_bad_id_and_permission() {
    let long = "x".repeat(5000);
    let mut bad: Vec<&str> = HOSTILE.to_vec();
    bad.push(&long);
    bad.push("extra"); // offered, not installed
    for s in bad {
        let s = serde_json::to_string(s).unwrap();
        let json = format!(r#"{{"action":"grant","id":{s},"permissions":["network"]}}"#);
        assert!(argv(&json).is_err(), "{json}");
    }
    for perm in ["a b", "--x", "", "a,b", "ü"] {
        let s = serde_json::to_string(perm).unwrap();
        let json = format!(r#"{{"action":"grant","id":"carmqtt","permissions":[{s}]}}"#);
        assert!(argv(&json).is_err(), "{json}");
    }
    for json in [r#"{"action":"grant","id":"carmqtt"}"#, r#"{"action":"grant","id":"carmqtt","permissions":[],"x":1}"#] {
        assert!(argv(json).is_err(), "{json}");
    }
}
