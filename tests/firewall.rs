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
    assert_eq!(firewall::apply(&ctx, Some(80)).unwrap(), "hotspot firewall: port 80 opens after the next restart of the head unit");
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
    assert!(run("80").starts_with("OK: hotspot firewall: port 80 opens"));
    assert!(run("off").starts_with("OK: hotspot firewall: closed after"));
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

#[test]
fn apply_only_checks_with_pfctl_and_never_reloads_the_live_firewall() {
    // Reloading pf.conf at runtime resets the anchors the car's connectivity manager fills in
    // (mmi_data, dns_data, wlan_data back to "block all"): the car lost its internet (2026-10-05).
    let f = common::fixture();
    fs::write(f.layout.p("/mnt/system/etc/pf.conf"), STOCK).unwrap();
    let sys = RecordingSys::default();
    let ctx = Ctx::new(f.layout.clone(), &sys);
    let mut calls: Vec<Vec<String>> = Vec::new();
    let msg = firewall::apply_with(&ctx, Some(80), &mut |args: &[&str]| {
        calls.push(args.iter().map(|s| s.to_string()).collect());
        Ok(())
    })
    .unwrap();
    assert_eq!(calls.len(), 1, "{calls:?}");
    assert_eq!(calls[0][0], "-nf", "only a syntax check");
    assert!(msg.contains("after the next restart"), "{msg}");
}

/// The variant the car loads once its data connection is up on ecm0 (captured 2026-10-05).
const ECM0: &str = include_str!("fixtures/pf.ecm0.conf.unit");

#[test]
fn every_pf_variant_the_car_may_load_gets_the_rule() {
    // The connectivity manager swaps in pf.<uplink>.conf (pf.ecm0.conf when SIM data is up), so a
    // rule only in pf.conf vanished from the live firewall (seen on the unit, 2026-10-05).
    let f = common::fixture();
    let etc = f.layout.p("/mnt/system/etc");
    fs::write(etc.join("pf.conf"), STOCK).unwrap();
    fs::write(etc.join("pf.ecm0.conf"), ECM0).unwrap();
    let sys = RecordingSys::default();
    let ctx = Ctx::new(f.layout.clone(), &sys);
    firewall::apply_with(&ctx, Some(80), &mut |_: &[&str]| Ok(())).unwrap();
    for name in ["pf.conf", "pf.ecm0.conf"] {
        let text = fs::read_to_string(etc.join(name)).unwrap();
        assert_eq!(text.matches(MARK).count(), 1, "{name}");
        assert!(etc.join(format!("{name}.mibpkg-orig")).is_file(), "{name} backed up");
    }
    assert!(!etc.join("pf.ppp0.conf").exists(), "variants that are not there are not created");
    assert!(!firewall::needs_change(&f.layout, Some(80)));
    fs::write(etc.join("pf.ecm0.conf"), ECM0).unwrap();
    assert!(firewall::needs_change(&f.layout, Some(80)), "any variant without the rule needs a change");
    firewall::apply_with(&ctx, None, &mut |_: &[&str]| Ok(())).unwrap();
    assert_eq!(fs::read_to_string(etc.join("pf.conf")).unwrap(), STOCK);
    assert_eq!(fs::read_to_string(etc.join("pf.ecm0.conf")).unwrap(), ECM0);
}

#[test]
fn a_variant_whose_stock_file_also_fails_the_check_is_still_edited() {
    // pf.ppp0.conf shapes traffic on ppp0; without that uplink `pfctl -nf` fails on the stock file
    // too ("SIOCGIFMTU: No such device or address", seen on the unit 2026-10-06), so the check
    // cannot judge the edit and must not block it.
    let f = common::fixture();
    let etc = f.layout.p("/mnt/system/etc");
    fs::write(etc.join("pf.conf"), STOCK).unwrap();
    // A marker stands for "needs the ppp0 uplink to check".
    fs::write(etc.join("pf.ppp0.conf"), format!("# NEEDS_PPP0\n{ECM0}")).unwrap();
    let sys = RecordingSys::default();
    let ctx = Ctx::new(f.layout.clone(), &sys);
    let ppp0 = |args: &[&str]| -> anyhow::Result<()> {
        // Fails for the stock ppp0 file and for the check copy made from it.
        let text = fs::read_to_string(f.layout.p(args[1])).unwrap_or_default();
        if text.contains("NEEDS_PPP0") { anyhow::bail!("SIOCGIFMTU: No such device or address") } else { Ok(()) }
    };
    firewall::apply_with(&ctx, Some(80), &mut |a: &[&str]| ppp0(a)).unwrap();
    assert_eq!(fs::read_to_string(etc.join("pf.ppp0.conf")).unwrap().matches(MARK).count(), 1);

    // But when only the edited file fails, nothing changes and the reason is reported.
    let g = common::fixture();
    let etc = g.layout.p("/mnt/system/etc");
    fs::write(etc.join("pf.conf"), STOCK).unwrap();
    let ctx = Ctx::new(g.layout.clone(), &sys);
    let e = firewall::apply_with(&ctx, Some(80), &mut |a: &[&str]| {
        let text = fs::read_to_string(g.layout.p(a[1])).unwrap_or_default();
        if text.contains(MARK) { anyhow::bail!("syntax error on line 139") } else { Ok(()) }
    })
    .unwrap_err();
    assert!(format!("{e:#}").contains("syntax error on line 139"), "{e:#}");
    assert!(!fs::read_to_string(etc.join("pf.conf")).unwrap().contains(MARK));
}
