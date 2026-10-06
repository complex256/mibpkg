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
    Install { package: String, collection: Option<String> },
    Rollback { package: String },
    Restart { package: String },
    Remove { package: String, #[serde(default)] purge: bool },
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
        Action::Install { package, collection } => {
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
    })
}
