//! The hotspot firewall rule mibpkg manages in /mnt/system/etc/pf.conf (the stock pf blocks every
//! port on the car's Wi-Fi except a short list, so the web interface needs its own pass rule).
mod common;
use mibpkg::ctx::Ctx;
use mibpkg::firewall::{self, edit, wanted, MARK};
use mibpkg::sys::RecordingSys;
use mibpkg::web::settings::WebSettings;
use std::fs;

/// The car's pf.conf (MHI2Q, with the M.I.B. "SSH Access" block), captured 2026-10-05.
const STOCK: &str = include_str!("fixtures/pf.conf.unit");
const BLOCK_LINE: &str = "block in quick on $wlan_if from any to ($wlan_if)";

#[test]
fn the_rule_goes_just_before_the_hotspot_block_and_nothing_else_changes() {
    let out = edit(STOCK, Some(80)).unwrap();
    let lines: Vec<&str> = out.lines().collect();
    let at = lines.iter().position(|l| *l == BLOCK_LINE).unwrap();
    assert_eq!(lines[at - 2], MARK);
    assert_eq!(lines[at - 1], "pass in quick on $wlan_if proto tcp from ($wlan_if:network) to ($wlan_if) port 80 keep state allow-opts");
    // Removing it gives back the stock file exactly.
    assert_eq!(edit(&out, None).unwrap(), STOCK);
    assert_eq!(edit(STOCK, None).unwrap(), STOCK, "nothing to remove");
}

#[test]
fn editing_again_is_stable_and_a_new_port_replaces_the_old_one() {
    let once = edit(STOCK, Some(80)).unwrap();
    assert_eq!(edit(&once, Some(80)).unwrap(), once);
    let moved = edit(&once, Some(8081)).unwrap();
    assert_eq!(moved.matches(MARK).count(), 1);
    assert!(moved.contains("port 8081 keep state") && !moved.contains("port 80 keep state"));
}

#[test]
fn a_file_without_the_hotspot_block_is_left_alone() {
    let e = edit("pass all\n", Some(80)).unwrap_err();
    assert!(format!("{e:#}").contains("not changing it"), "{e:#}");
}

#[test]
fn the_rule_is_wanted_only_when_the_hotspot_is_listened_on() {
    let mut s = WebSettings::default();
    assert_eq!(wanted(&s, 80), Some(80));
    assert_eq!(wanted(&s, 8080), Some(8080), "the port actually bound");
    s.listen = vec!["tailscale".into()];
    assert_eq!(wanted(&s, 80), None);
    s.listen = vec!["any".into()];
    assert_eq!(wanted(&s, 80), Some(80));
    s.enabled = false;
    assert_eq!(wanted(&s, 80), None);
}

#[test]
fn apply_writes_pf_conf_under_a_system_remount_and_backs_it_up_once() {
    let f = common::fixture();
    let pf = f.layout.p("/mnt/system/etc/pf.conf");
    fs::write(&pf, STOCK).unwrap();
    let sys = RecordingSys::default();
    let ctx = Ctx::new(f.layout.clone(), &sys);
    assert_eq!(firewall::apply(&ctx, Some(80)).unwrap(), "hotspot firewall: port 80 open for the web interface");
    assert!(fs::read_to_string(&pf).unwrap().contains(MARK));
    assert_eq!(fs::read_to_string(firewall::backup_path(&f.layout)).unwrap(), STOCK);
    assert_eq!(*sys.calls.borrow(), vec!["rw /mnt/system", "ro /mnt/system"]);
    assert!(firewall::needs_change(&f.layout, Some(8080)));
    assert!(!firewall::needs_change(&f.layout, Some(80)));
    // Unchanged: no remount at all.
    assert_eq!(firewall::apply(&ctx, Some(80)).unwrap(), "hotspot firewall: already set");
    assert_eq!(sys.calls.borrow().len(), 2);
    firewall::apply(&ctx, None).unwrap();
    assert_eq!(fs::read_to_string(&pf).unwrap(), STOCK);
    assert_eq!(fs::read_to_string(firewall::backup_path(&f.layout)).unwrap(), STOCK, "the backup is the first original");
}

#[test]
fn the_verb_runs_under_the_lock_and_checks_its_argument() {
    let f = common::fixture();
    fs::write(f.layout.p("/mnt/system/etc/pf.conf"), STOCK).unwrap();
    let run = |arg: &str| {
        let args: Vec<String> = ["--root", f.root().to_str().unwrap(), "--no-supervisor", "gem", "web-firewall", arg].iter().map(|s| s.to_string()).collect();
        mibpkg::cli::run(&args).unwrap();
        mibpkg::state::State::load(&f.layout).unwrap().results["_"].clone()
    };
    assert!(run("80").starts_with("OK: hotspot firewall: port 80 open"));
    assert!(run("off").starts_with("OK: hotspot firewall: closed"));
    assert!(run("$(reboot)").starts_with("FAILED"));
    assert!(run("0").starts_with("FAILED"));
}

#[test]
fn uninstall_self_closes_the_hotspot_port() {
    let f = common::fixture();
    let pf = f.layout.p("/mnt/system/etc/pf.conf");
    fs::write(&pf, edit(STOCK, Some(80)).unwrap()).unwrap();
    let args: Vec<String> = ["--root", f.root().to_str().unwrap(), "--no-supervisor", "uninstall-self"].iter().map(|s| s.to_string()).collect();
    mibpkg::cli::run(&args).unwrap();
    assert_eq!(fs::read_to_string(&pf).unwrap(), STOCK);
}
