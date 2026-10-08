//! The generated GEM (green engineering menu) page: one static .esd file in
//! /eso/hmi/engdefs whose buttons run one-line scripts in <home>/gem/. Since the web interface
//! (M2), GEM is only the way back in: where the web UI is, and buttons that work when it does not.
use crate::ctx::Ctx;
use crate::fsutil::{atomic_write, set_mode};
use crate::install::SELF;
use crate::media;
use crate::paths::{Layout, HOME};
use crate::state::State;
use crate::unit::UnitInfo;
use anyhow::Result;
use std::fs;
use std::path::PathBuf;

pub enum Item {
    Text(String),
    Button { label: String, action: Vec<String> },
    /// A row GEM re-reads from persistence every second while the screen is open.
    Live { label: String, key: u32 },
}

pub struct Screen {
    pub name: String,
    pub parent: String,
    pub items: Vec<Item>,
}

pub struct Model<'a> {
    pub layout: &'a Layout,
    pub unit: &'a UnitInfo,
    /// DISABLE marker on a card: no services start (the web server neither).
    pub disabled: bool,
    /// A global result exists (restart / reset web): show its live row.
    pub has_result: bool,
    /// A result for mibpkg itself exists (Update mibpkg): show its live row.
    pub has_update_result: bool,
    /// The kept earlier mibpkg version: a self-update that broke the web UI is undone from here.
    pub rollback_to: Option<String>,
}


/// "." cannot appear in package or collection names, so it can't make two actions collide.
pub fn script_name(action: &[String]) -> String {
    format!("{}.sh", action.join("."))
}

pub fn script_body(action: &[String]) -> String {
    format!("#!/bin/sh\nexec /bin/sh {HOME}/bin/gem.sh {}\n", action.join(" "))
}

fn clean(s: &str) -> String {
    s.chars()
        .map(|c| if c == '"' { '\'' } else if c.is_ascii() && !c.is_ascii_control() { c } else { '?' })
        .collect()
}

pub fn render(s: &Screen, gem_dir_unit: &str) -> String {
    let mut o = format!("screen {} {}\n", s.name, s.parent);
    for it in &s.items {
        o.push('\n');
        match it {
            Item::Text(t) => o.push_str(&format!(
                "keyValue\n\tvalue String sys 0x00000000 0\n\t\tlabel \"{}\"\n\t\tpoll 0\n",
                clean(t)
            )),
            Item::Live { label, key } => o.push_str(&format!(
                "keyValue\n\tvalue String per {} 0x{:08x}\n\t\tlabel \"{}\"\n\t\tpoll 1000\n",
                crate::persist::NS,
                key,
                clean(label)
            )),
            Item::Button { label, action } => o.push_str(&format!(
                "script\n\tvalue sys 1 0x100 \"{}/{}\"\n\tlabel \"{}\"\n",
                gem_dir_unit,
                script_name(action),
                clean(label)
            )),
        }
    }
    o
}

fn text(s: impl Into<String>) -> Item {
    Item::Text(s.into())
}

fn button(label: impl Into<String>, action: &[&str]) -> Item {
    Item::Button { label: label.into(), action: action.iter().map(|s| s.to_string()).collect() }
}

/// The one GEM page (web interface M2): where the web interface is, how it is set up, and the
/// buttons that still work when it does not. Packages, logs and sources live in the web UI.
pub fn screens(m: &Model) -> Vec<Screen> {
    let (web, warning) = crate::web::settings::load(m.layout);
    let mut items = vec![
        text(format!("mibpkg {} - unit {}", env!("CARGO_PKG_VERSION"), m.unit.describe())),
        Item::Live { label: "supervisor".into(), key: crate::persist::KEY_SUPERVISOR },
    ];
    if m.disabled {
        items.push(text("DISABLE marker on the card: no services are started (the web server neither)"));
    }
    items.push(Item::Live { label: "web server".into(), key: crate::persist::service_key(crate::cli::WEB_SERVICE) });
    if !web.enabled {
        items.push(text("web server disabled in web.toml (Reset web settings turns it back on)"));
    }
    // Written by the running server: "port N", "<class> <url>" lines, "iface <name> <addr>" lines.
    let file = fs::read_to_string(m.layout.run("web.url")).unwrap_or_default();
    let lines: Vec<&str> = file.lines().skip(1).collect();
    let ifaces: Vec<&str> = lines.iter().filter_map(|l| l.strip_prefix("iface ")).collect();
    let urls: Vec<String> = lines.iter().filter(|l| !l.starts_with("iface ")).map(|l| l.replacen(' ', ": ", 1)).collect();
    if urls.is_empty() && ifaces.is_empty() {
        items.push(text("address: not known yet (join the car's Wi-Fi, then open this screen again)"));
    } else if urls.is_empty() {
        items.push(text("address: no hotspot or Tailscale address recognised (see interfaces below)"));
    }
    for u in urls {
        items.push(text(u));
    }
    if !ifaces.is_empty() {
        items.push(text(format!("interfaces up: {}", ifaces.join(", "))));
    }
    items.push(text("Packages, logs and settings: open the address above in a phone browser"));
    items.push(text(format!("listen: {}", web.listen.join(", "))));
    items.push(text(format!("PIN: {}", if web.pin.is_some() { "set" } else { "not set" })));
    if let Some(w) = warning {
        items.push(text(w));
    }
    items.push(button("Restart web server", &["web-restart"]));
    items.push(button("Reset web settings", &["web-reset"]));
    if web.pin.is_some() {
        // A forgotten PIN: being in GEM (in the car) is the authorization.
        items.push(button("Remove PIN", &["web-clear-pin"]));
    }
    items.push(button("Update mibpkg (SD card or online)", &["self-update"]));
    if let Some(p) = &m.rollback_to {
        items.push(button(format!("Roll back mibpkg to {p}"), &["rollback", SELF]));
    }
    if m.has_result {
        items.push(Item::Live { label: "last result".into(), key: crate::persist::KEY_RESULT_GLOBAL });
    }
    if m.has_update_result {
        items.push(Item::Live { label: "mibpkg update".into(), key: crate::persist::result_key(SELF) });
    }
    vec![Screen { name: "mibpkg".into(), parent: m.layout.settings.gem_parent.clone(), items }]
}

/// Delete every mibpkg*.esd in engdefs. Caller holds /mnt/app rw.
pub fn remove_screens(layout: &Layout) -> Result<()> {
    let eng = layout.engdefs();
    if let Ok(rd) = fs::read_dir(&eng) {
        for e in rd.filter_map(|e| e.ok()) {
            let n = e.file_name().to_string_lossy().to_string();
            if n.starts_with("mibpkg") && n.ends_with(".esd") {
                fs::remove_file(e.path())?;
            }
        }
    }
    Ok(())
}

pub fn regenerate(ctx: &Ctx) -> Result<()> {
    let layout = &ctx.layout;
    let state = State::load(layout)?;
    let model = Model {
        layout,
        unit: &ctx.unit,
        disabled: media::disabled(layout),
        has_result: state.results.contains_key("_"),
        has_update_result: state.results.contains_key(SELF),
        rollback_to: state.packages.get(SELF).and_then(|i| i.previous.clone()),
    };
    let screens = screens(&model);
    let gem_unit = layout.unit_path(&layout.gem_dir());
    // Every file the screens need, with its content.
    let mut want: Vec<(PathBuf, String, u32)> = Vec::new();
    for sc in &screens {
        want.push((layout.engdefs().join(format!("{}.esd", sc.name)), render(sc, &gem_unit), 0o644));
        for it in &sc.items {
            if let Item::Button { action, .. } = it {
                want.push((layout.gem_dir().join(script_name(action)), script_body(action), 0o755));
            }
        }
    }
    let wanted = |p: &PathBuf| want.iter().any(|(w, _, _)| w == p);
    // Leave unchanged files alone: rewriting the .esd of a screen that is open in GEM stops its
    // live rows from updating, and every write is flash wear.
    let writes: Vec<&(PathBuf, String, u32)> =
        want.iter().filter(|(p, c, _)| fs::read_to_string(p).ok().as_deref() != Some(c.as_str())).collect();
    let mut stale: Vec<PathBuf> = Vec::new();
    for (dir, is_ours) in [
        (layout.engdefs(), (|n: &str| n.starts_with("mibpkg") && n.ends_with(".esd")) as fn(&str) -> bool),
        (layout.gem_dir(), |n: &str| n.ends_with(".sh")),
    ] {
        if let Ok(rd) = fs::read_dir(&dir) {
            for e in rd.filter_map(|e| e.ok()) {
                if is_ours(&e.file_name().to_string_lossy()) && !wanted(&e.path()) {
                    stale.push(e.path());
                }
            }
        }
    }
    if writes.is_empty() && stale.is_empty() {
        return Ok(());
    }
    let _rw = ctx.mounts.rw("/mnt/app")?;
    for (p, content, mode) in writes {
        atomic_write(p, content.as_bytes())?;
        set_mode(p, *mode)?;
    }
    for p in stale {
        fs::remove_file(&p)?;
    }
    Ok(())
}
