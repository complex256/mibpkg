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
    pub disabled: bool,
    pub supervisor_running: bool,
}

pub fn screen_name(pkg: &str) -> String {
    format!("mibpkg_{}", pkg.replace('-', "_"))
}

pub fn script_name(action: &[String]) -> String {
    format!("{}.sh", action.join("-"))
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
    let colls: Vec<&Collection> = m.scan.iter().filter_map(|(_, c)| c.as_ref().ok()).collect();
    let owned: Vec<Collection> = colls.iter().map(|c| (*c).clone()).collect();
    let updates = updates_available(m.state, &owned);

    let mut main = vec![
        text(format!("mibpkg {} - unit {}", env!("CARGO_PKG_VERSION"), m.unit.describe())),
        text(format!("supervisor: {}", if m.supervisor_running { "running" } else { "not running" })),
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
    if !updates.is_empty() {
        main.push(button(format!("Update all ({})", updates.len()), &["update-all"]));
    }
    if let Some(r) = m.state.results.get("_") {
        main.push(text(format!("last result: {r}")));
    }
    let mut out = vec![Screen { name: "mibpkg".into(), parent: m.layout.settings.gem_parent.clone(), items: main }];

    // name -> [(collection, newest entry)]
    let mut available: BTreeMap<String, Vec<(&Collection, &IndexEntry)>> = BTreeMap::new();
    for &c in &colls {
        let mut names: Vec<&str> = c.index.packages.iter().map(|e| e.name.as_str()).collect();
        names.sort();
        names.dedup();
        for n in names {
            available.entry(n.to_string()).or_default().push((c, c.latest(n).unwrap()));
        }
    }
    let mut names: Vec<String> = available.keys().cloned().collect();
    names.extend(m.state.packages.keys().cloned());
    names.sort();
    names.dedup();

    for name in names {
        let inst = m.state.packages.get(&name);
        let avail = available.get(&name).cloned().unwrap_or_default();
        let desc = avail.first().map(|(_, e)| e.description.clone()).unwrap_or_default();
        let mut items = vec![text(if desc.is_empty() { name.clone() } else { format!("{name}: {desc}") })];
        items.push(text(match inst {
            Some(i) => format!("installed: {}", i.version),
            None => "not installed".to_string(),
        }));
        for (c, e) in &avail {
            let mut l = format!("card [{}]: {}", c.id(), e.version);
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
                for (c, e) in &avail {
                    if e.compat.matches(m.unit) {
                        items.push(button(format!("Install {} from {}", e.version, c.id()), &["install", name.as_str(), c.id()]));
                    }
                }
            }
            Some(i) => {
                if let Some((c, e)) = avail.iter().find(|(c, _)| c.id() == i.collection) {
                    if cmp_versions(&e.version, &i.version) == Ordering::Greater && e.compat.matches(m.unit) {
                        items.push(button(format!("Update to {}", e.version), &["install", name.as_str(), c.id()]));
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
                    let line = match m.status.and_then(|s| s.services.get(id)) {
                        Some(s) => format!("{id}: {}", status::describe(s)),
                        None => format!("{id}: not started"),
                    };
                    items.push(text(line));
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
        if let Some(r) = m.state.results.get(&name) {
            items.push(text(format!("last result: {r}")));
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
    let st = status::read(layout);
    let model = Model {
        layout,
        unit: &ctx.unit,
        state: &state,
        status: st.as_ref(),
        scan: &scan,
        disabled: media::disabled(layout),
        supervisor_running: supervisor_pid(layout, ctx.sys).is_some(),
    };
    let screens = screens(&model);
    let _rw = ctx.mounts.rw("/mnt/app")?;
    fs::create_dir_all(layout.engdefs())?;
    remove_screens(layout)?;
    let gd = layout.gem_dir();
    if let Ok(rd) = fs::read_dir(&gd) {
        for e in rd.filter_map(|e| e.ok()) {
            if e.file_name().to_string_lossy().ends_with(".sh") {
                fs::remove_file(e.path())?;
            }
        }
    }
    fs::create_dir_all(&gd)?;
    let gem_unit = layout.unit_path(&gd);
    for s in &screens {
        atomic_write(&layout.engdefs().join(format!("{}.esd", s.name)), render(s, &gem_unit).as_bytes())?;
        for it in &s.items {
            if let Item::Button { action, .. } = it {
                let p = gd.join(script_name(action));
                atomic_write(&p, script_body(action).as_bytes())?;
                set_mode(&p, 0o755)?;
            }
        }
    }
    Ok(())
}
