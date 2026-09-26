mod common;
use mibpkg::ctx::Ctx;
use mibpkg::index::Collection;
use mibpkg::install;
use mibpkg::sys::RecordingSys;
use std::fs;

const OLD_HOOK: &str = "    ## carmqtt autostart. Best effort only: detached, no stdio.\n    ( sleep 45; [ -x /mnt/app/root/carmqtt/carmqtt.sh ] && /mnt/app/root/carmqtt/carmqtt.sh start ) </dev/null >/dev/null 2>&1 &\n";

#[test]
fn installing_carmqtt_migrates_the_hand_install() {
    let f = common::fixture();
    let old = f.root().join("mnt/app/root/carmqtt");
    fs::create_dir_all(&old).unwrap();
    fs::write(old.join("carmqtt.toml"), "broker = \"x\"\n").unwrap();
    let stopped = f.root().join("stopped");
    fs::write(old.join("carmqtt.sh"), format!("[ \"$1\" = stop ] && : > {}\n", stopped.display())).unwrap();
    let startup = common::STARTUP.replace("    echo after\n", &format!("{OLD_HOOK}    echo after\n"));
    fs::write(f.layout.startup_sh(), &startup).unwrap();

    let src = common::pkg_src(f.root(), "carmqtt", "1.0.0", "sleep 30", "[[config]]\npath = \"carmqtt.toml\"\nrequired = true\n");
    let c = Collection::load(&common::collection(&f.sd(), "complex256", &[src])).unwrap();
    let sys = RecordingSys::default();
    let ctx = Ctx::new(f.layout.clone(), &sys);
    let msg = install::install(&ctx, &[f.sd()], &c, "carmqtt").unwrap();

    assert!(msg.contains("migrated"), "{msg}");
    assert!(!msg.contains("needs config"), "legacy config satisfies the requirement: {msg}");
    assert!(stopped.exists(), "old instance stopped");
    assert_eq!(fs::read_to_string(f.layout.config_dir("carmqtt").join("carmqtt.toml")).unwrap(), "broker = \"x\"\n");
    assert_eq!(fs::read_to_string(f.layout.startup_sh()).unwrap(), common::STARTUP);
    assert!(!old.exists());
    assert!(f.root().join("mnt/app/root/carmqtt.pre-mibpkg/carmqtt.toml").exists());
}

#[test]
fn card_config_wins_over_legacy_config() {
    let f = common::fixture();
    let old = f.root().join("mnt/app/root/carmqtt");
    fs::create_dir_all(&old).unwrap();
    fs::write(old.join("carmqtt.toml"), "old").unwrap();
    fs::write(old.join("carmqtt.sh"), "exit 0\n").unwrap();
    let card = mibpkg::config::card_path(&f.sd(), "carmqtt", "carmqtt.toml");
    fs::create_dir_all(card.parent().unwrap()).unwrap();
    fs::write(&card, "card").unwrap();
    let src = common::pkg_src(f.root(), "carmqtt", "1.0.0", "sleep 30", "[[config]]\npath = \"carmqtt.toml\"\n");
    let c = Collection::load(&common::collection(&f.sd(), "complex256", &[src])).unwrap();
    let sys = RecordingSys::default();
    let ctx = Ctx::new(f.layout.clone(), &sys);
    install::install(&ctx, &[f.sd()], &c, "carmqtt").unwrap();
    assert_eq!(fs::read_to_string(f.layout.config_dir("carmqtt").join("carmqtt.toml")).unwrap(), "card");
}
