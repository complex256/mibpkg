use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

fn shell(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("shell").join(name)
}

fn have_ksh() -> bool {
    Command::new("ksh").arg("-c").arg("true").status().map(|s| s.success()).unwrap_or(false)
}

#[test]
fn all_scripts_pass_ksh_n() {
    if !have_ksh() {
        eprintln!("skipping: no ksh");
        return;
    }
    for s in ["launch.sh", "gem.sh", "custom.sh", "command.sh"] {
        let ok = Command::new("ksh").arg("-n").arg(shell(s)).status().unwrap().success();
        assert!(ok, "{s} fails ksh -n");
    }
}

/// A fake mibpkg that records its arguments.
fn fake_home(t: &Path) -> (PathBuf, PathBuf) {
    let home = t.join("home");
    let run = t.join("run");
    fs::create_dir_all(home.join("bin")).unwrap();
    fs::create_dir_all(&run).unwrap();
    let fake = home.join("bin/mibpkg");
    fs::write(&fake, format!("#!/bin/sh\necho \"$@\" > {}/args\n", run.display())).unwrap();
    fs::set_permissions(&fake, fs::Permissions::from_mode(0o755)).unwrap();
    (home, run)
}

#[test]
fn launch_copies_to_run_and_execs_supervise() {
    let t = tempfile::tempdir().unwrap();
    let (home, run) = fake_home(t.path());
    let sd = t.path().join("sd");
    fs::create_dir_all(&sd).unwrap();
    let st = Command::new("/bin/sh")
        .arg(shell("launch.sh"))
        .env("MIBPKG_HOME", &home)
        .env("MIBPKG_RUN", &run)
        .env("MIBPKG_MEDIA", &sd)
        .env("MIBPKG_ARGS", "--root /x")
        .status()
        .unwrap();
    assert!(st.success());
    assert!(run.join("mibpkg").is_file());
    assert_eq!(fs::read_to_string(run.join("args")).unwrap().trim(), "--root /x supervise");
}

#[test]
fn launch_honours_disable() {
    let t = tempfile::tempdir().unwrap();
    let (home, run) = fake_home(t.path());
    let sd = t.path().join("sd");
    fs::create_dir_all(sd.join("mibpkg")).unwrap();
    fs::write(sd.join("mibpkg/DISABLE"), "").unwrap();
    Command::new("/bin/sh")
        .arg(shell("launch.sh"))
        .env("MIBPKG_HOME", &home)
        .env("MIBPKG_RUN", &run)
        .env("MIBPKG_MEDIA", &sd)
        .status()
        .unwrap();
    assert!(!run.join("args").exists(), "nothing started");
    assert!(fs::read_to_string(run.join("mibpkg.log")).unwrap().contains("DISABLE"));
}

#[test]
fn launch_does_not_wait_forever_without_media() {
    let t = tempfile::tempdir().unwrap();
    let (home, run) = fake_home(t.path());
    let st = Command::new("/bin/sh")
        .arg(shell("launch.sh"))
        .env("MIBPKG_HOME", &home)
        .env("MIBPKG_RUN", &run)
        .env("MIBPKG_MEDIA", t.path().join("no-card"))
        .env("MIBPKG_WAIT", "1")
        .status()
        .unwrap();
    assert!(st.success());
    assert!(run.join("args").exists());
}

#[test]
fn gem_forwards_action() {
    let t = tempfile::tempdir().unwrap();
    let (home, run) = fake_home(t.path());
    Command::new("/bin/sh")
        .arg(shell("gem.sh"))
        .args(["install", "carmqtt", "complex256"])
        .env("MIBPKG_HOME", &home)
        .env("MIBPKG_RUN", &run)
        .status()
        .unwrap();
    assert_eq!(fs::read_to_string(run.join("args")).unwrap().trim(), "gem install carmqtt complex256");
}
