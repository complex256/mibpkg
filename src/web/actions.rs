//! What a browser may ask the car to do. The request is typed JSON parsed into `Action`; the
//! server builds the `mibpkg gem` argv itself from validated fields, so no client text is ever
//! a command, an option, or a path.
use crate::manifest::valid_name;
use crate::paths::Layout;
use crate::state::State;
use serde::Deserialize;
use std::collections::BTreeSet;

#[derive(Debug, Deserialize)]
#[serde(tag = "action", rename_all = "kebab-case", deny_unknown_fields)]
pub enum Action {
    // Empty braces, not unit variants: serde only rejects unknown fields on struct variants.
    Rescan {},
    Check {},
    UpdateAll {},
    /// `approve`: the plugin permissions the user approved on the sheet (`--approve=…`).
    Install { package: String, collection: Option<String>, #[serde(default)] approve: Vec<String> },
    Rollback { package: String },
    Restart { package: String },
    Remove { package: String, #[serde(default)] purge: bool },
    ConfigResolve { package: String, config: usize, choice: String },
    UninstallSelf { #[serde(default)] purge: bool },
    /// Set an installed plugin's grants to exactly `permissions` (empty revokes all).
    Grant { id: String, permissions: Vec<String> },
}

/// Names that exist right now, so a request can only name real things.
#[derive(Debug, Default)]
pub struct Known {
    pub installed: BTreeSet<String>,
    pub available: BTreeSet<String>,
    pub collections: BTreeSet<String>,
}

impl Known {
    pub fn load(layout: &Layout) -> Known {
        let state = State::load(layout).unwrap_or_default();
        let colls = crate::catalog::collections(layout);
        Known {
            installed: state.packages.keys().cloned().collect(),
            available: colls.iter().flat_map(|c| c.index.packages.iter().map(|e| e.name.clone())).collect(),
            collections: colls.iter().map(|c| c.id().to_string()).collect(),
        }
    }
}

/// A validated action: the argv after `gem`, plus what to show the user.
#[derive(Debug)]
pub struct Validated {
    pub label: String,
    pub package: Option<String>,
    pub argv: Vec<String>,
}

pub fn parse_action(body: &[u8]) -> Result<Action, String> {
    serde_json::from_slice(body).map_err(|e| format!("not a valid action: {e}"))
}

fn installed(known: &Known, name: &str) -> Result<String, String> {
    if valid_name(name) && known.installed.contains(name) {
        Ok(name.to_string())
    } else {
        Err(format!("{name:?} is not an installed package"))
    }
}

/// A permission id the web passes on: `[A-Za-z0-9._-]+`, starting with a letter or digit so it
/// can never read as an option, and short.
fn permission(p: &str) -> Result<&str, String> {
    if p.len() <= 64 && crate::mibsdk::catalog::is_permission(p) && p.as_bytes()[0].is_ascii_alphanumeric() {
        Ok(p)
    } else {
        Err(format!("{p:?} is not a permission"))
    }
}

/// `perms` validated and joined with commas, as `--approve=` and `sdk grant` take them.
fn permission_list(perms: &[String]) -> Result<String, String> {
    Ok(perms.iter().map(|p| permission(p)).collect::<Result<Vec<_>, _>>()?.join(","))
}

/// The `--approve=…` argument for an approval list; None for an empty list (the install then
/// asks rather than approving nothing).
pub fn approve_flag(perms: &[String]) -> Result<Option<String>, String> {
    let list = permission_list(perms)?;
    Ok((!list.is_empty()).then(|| format!("--approve={list}")))
}

pub fn validate(a: &Action, known: &Known) -> Result<Validated, String> {
    let v = |label: String, package: Option<&str>, argv: Vec<&str>| Validated {
        label,
        package: package.map(String::from),
        argv: argv.into_iter().map(String::from).collect(),
    };
    Ok(match a {
        Action::Rescan {} => v("Rescan SD card".into(), None, vec!["rescan"]),
        Action::Check {} => v("Check for updates".into(), None, vec!["check"]),
        Action::UpdateAll {} => v("Update all".into(), None, vec!["update-all"]),
        Action::Install { package, collection, approve } => {
            if !valid_name(package) || !known.available.contains(package) {
                return Err(format!("{package:?} is not offered by any collection"));
            }
            let mut argv = vec!["install", package.as_str()];
            if let Some(c) = collection {
                if !valid_name(c) || !known.collections.contains(c) {
                    return Err(format!("{c:?} is not a known collection"));
                }
                argv.push(c);
            }
            let flag = approve_flag(approve)?;
            argv.extend(flag.as_deref());
            let verb = if known.installed.contains(package) { "Update" } else { "Install" };
            v(format!("{verb} {package}"), Some(package), argv)
        }
        Action::Rollback { package } => {
            let p = installed(known, package)?;
            v(format!("Roll back {p}"), Some(&p), vec!["rollback", &p])
        }
        Action::Restart { package } => {
            let p = installed(known, package)?;
            v(format!("Restart {p}"), Some(&p), vec!["restart", &p])
        }
        Action::Remove { package, purge } => {
            let p = installed(known, package)?;
            let mut argv = vec!["remove", p.as_str()];
            if *purge {
                argv.push("--purge");
            }
            v(format!("Remove {p}"), Some(&p), argv)
        }
        Action::ConfigResolve { package, config, choice } => {
            let p = installed(known, package)?;
            let label = match choice.as_str() {
                "keep" => format!("Keep the car's config of {p}"),
                "card" => format!("Use the card's config for {p}"),
                _ => return Err(format!("{choice:?} is not keep or card")),
            };
            let n = config.to_string();
            v(label, Some(&p), vec!["config-resolve", &p, choice, &n])
        }
        Action::UninstallSelf { purge } => {
            let mut argv = vec!["uninstall-self"];
            if *purge {
                argv.push("--purge");
            }
            let label = if *purge { "Remove mibpkg and delete all its files" } else { "Remove mibpkg" };
            v(label.into(), None, argv)
        }
        Action::Grant { id, permissions } => {
            let p = installed(known, id)?;
            let list = permission_list(permissions)?;
            v(format!("Change permissions of {p}"), Some(&p), vec!["sdk", "grant", &p, &list])
        }
    })
}
