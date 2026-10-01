//! Generated GEM (green engineering menu) screens: static .esd files in
//! /eso/hmi/engdefs whose buttons run one-line scripts in <home>/gem/.
use crate::config::missing_required;
use crate::control::supervisor_pid;
use crate::ctx::Ctx;
use crate::fsutil::{atomic_write, set_mode};
use crate::index::{cmp_versions, Collection, IndexEntry};
use crate::install::{current_manifest, updates_available, SELF};
use crate::media;
use crate::paths::{Layout, HOME};
use crate::state::State;
use crate::status::{self, Status};
use crate::unit::UnitInfo;
use anyhow::Result;
use std::cmp::Ordering;
use std::collections::BTreeMap;
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
    pub state: &'a State,
    pub status: Option<&'a Status>,
    pub scan: &'a [(PathBuf, Result<Collection>)],
    /// Cached online collections.
    pub online: &'a [Collection],
    pub disabled: bool,
    pub supervisor_running: bool,
}

pub fn screen_name(pkg: &str) -> String {
    format!("mibpkg_{}", pkg.replace('-', "_"))
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

pub fn screens(m: &Model) -> Vec<Screen> {
    // Card and online collections, tagged with where they came from.
    let mut sources: Vec<(&Collection, &str)> = m.scan.iter().filter_map(|(_, c)| c.as_ref().ok()).map(|c| (c, "card")).collect();
    sources.extend(m.online.iter().map(|c| (c, "online")));
    let owned: Vec<Collection> = sources.iter().map(|(c, _)| (*c).clone()).collect();
    let updates = updates_available(m.state, &owned, m.unit);

    let mut main = vec![
        text(format!("mibpkg {} - unit {}", env!("CARGO_PKG_VERSION"), m.unit.describe())),
        Item::Live { label: "supervisor".into(), key: crate::persist::KEY_SUPERVISOR },
        Item::Live { label: "online".into(), key: crate::persist::KEY_ONLINE },
    ];
    if m.disabled {
        main.push(text("DISABLE marker on the card: no services are started"));
    }
    if m.scan.is_empty() {
        main.push(text("No collections on SD/USB (expected mibpkg/collections/<id>/index.json)"));
    }
    for (dir, c) in m.scan {
        main.push(match c {
            Ok(c) => text(format!("card: {} [{}] - {} packages", c.index.collection.name, c.id(), c.index.packages.len())),
            Err(e) => text(format!("card: unreadable collection {}: {e:#}", dir.display())),
        });
    }
    main.push(text(format!("installed: {}", m.state.packages.len())));
    main.push(button("Rescan SD card", &["rescan"]));
    main.push(button("Check for updates", &["check"]));
    if !updates.is_empty() {
        main.push(button(format!("Update all ({})", updates.len()), &["update-all"]));
    }
    if m.state.results.contains_key("_") {
        main.push(Item::Live { label: "last result".into(), key: crate::persist::KEY_RESULT_GLOBAL });
    }
    let mut out = vec![Screen { name: "mibpkg".into(), parent: m.layout.settings.gem_parent.clone(), items: main }];

    // name -> [(collection, newest entry, origin)]
    let mut available: BTreeMap<String, Vec<(&Collection, &IndexEntry, &str)>> = BTreeMap::new();
    for &(c, origin) in &sources {
        let mut names: Vec<&str> = c.index.packages.iter().map(|e| e.name.as_str()).collect();
        names.sort();
        names.dedup();
        for n in names {
            available.entry(n.to_string()).or_default().push((c, c.latest(n).unwrap(), origin));
        }
    }
    let mut names: Vec<String> = available.keys().cloned().collect();
    names.extend(m.state.packages.keys().cloned());
    names.sort();
    names.dedup();

    for name in names {
        let inst = m.state.packages.get(&name);
        let avail = available.get(&name).cloned().unwrap_or_default();
        let desc = avail.first().map(|(_, e, _)| e.description.clone()).unwrap_or_default();
        // Newest version that fits this unit, per collection id, across card and online.
        let best_in = |id: &str| -> Option<&IndexEntry> {
            avail
                .iter()
                .filter(|(c, _, _)| c.id() == id)
                .filter_map(|(c, _, _)| c.latest_compatible(&name, m.unit))
                .max_by(|a, b| cmp_versions(&a.version, &b.version))
        };
        let mut ids: Vec<&str> = avail.iter().map(|(c, _, _)| c.id()).collect();
        ids.sort();
        ids.dedup();
        let mut items = vec![text(if desc.is_empty() { name.clone() } else { format!("{name}: {desc}") })];
        items.push(text(match inst {
            Some(i) => format!("installed: {}", i.version),
            None => "not installed".to_string(),
        }));
        for (c, e, origin) in &avail {
            let mut l = format!("{origin} [{}]: {}", c.id(), e.version);
            if !e.compat.matches(m.unit) {
                l.push_str(" (not for this unit)");
            }
            if e.uses_scripts {
                l.push_str(" (!) runs scripts");
            }
            items.push(text(l));
        }
        match inst {
            None => {
                for id in &ids {
                    if let Some(e) = best_in(id) {
                        items.push(button(format!("Install {} from {id}", e.version), &["install", name.as_str(), id]));
                    }
                }
            }
            Some(i) => {
                if let Some(e) = best_in(&i.collection) {
                    if cmp_versions(&e.version, &i.version) == Ordering::Greater {
                        items.push(button(format!("Update to {}", e.version), &["install", name.as_str(), i.collection.as_str()]));
                    }
                }
                if !i.services.is_empty() {
                    items.push(button("Restart", &["restart", name.as_str()]));
                }
                if let Some(p) = &i.previous {
                    items.push(button(format!("Roll back to {p}"), &["rollback", name.as_str()]));
                }
                if name != SELF {
                    items.push(button("Remove", &["remove", name.as_str()]));
                }
                for id in &i.services {
                    items.push(Item::Live { label: id.clone(), key: crate::persist::service_key(id) });
                }
                if let Ok(man) = current_manifest(m.layout, &name) {
                    let missing = missing_required(m.layout, &man);
                    if !missing.is_empty() {
                        items.push(text(format!(
                            "needs config: {} - put it on the card in mibpkg/config/{name}/ then Rescan",
                            missing.join(", ")
                        )));
                    }
                }
            }
        }
        if m.state.results.contains_key(&name) {
            items.push(Item::Live { label: "last result".into(), key: crate::persist::result_key(&name) });
        }
        out.push(Screen { name: screen_name(&name), parent: "mibpkg".into(), items });
    }
    out
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
    let scan = media::scan(layout);
    let online = crate::online::collections(layout);
    let st = status::read(layout);
    let model = Model {
        layout,
        unit: &ctx.unit,
        state: &state,
        status: st.as_ref(),
        scan: &scan,
        online: &online,
        disabled: media::disabled(layout),
        supervisor_running: supervisor_pid(layout, ctx.sys).is_some(),
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
