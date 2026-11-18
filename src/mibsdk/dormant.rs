//! Dormant plugins: installed plugin packages whose plugin api is outside the installed SDK's
//! `min_api..=api` range. The SDK's runtime refuses to load such a plugin (`BLOCKED_INVALID`), so it
//! does nothing until an update built for the installed SDK arrives; its package, files, grants and
//! data stay. Dormancy is derived on every read from the installed packages, never stored: it ends
//! the moment a compatible build installs, and returns if the SDK is rolled back out from under it.
use super::installed_sdk;
use crate::install;
use crate::manifest::SdkSpec;
use crate::paths::Layout;
use crate::state::State;
use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Dormant {
    /// The package that carries the plugin (a combined product package keeps running its daemon).
    pub package: String,
    /// The plugin id.
    pub plugin: String,
    /// The plugin api the installed build declares.
    pub api: u32,
    pub sdk_min_api: u32,
    pub sdk_api: u32,
}

impl Dormant {
    pub fn reason(&self) -> String {
        format!(
            "needs plugin api {}; the installed SDK supports {}..{}",
            self.api, self.sdk_min_api, self.sdk_api
        )
    }
}

/// Installed plugin packages whose api is outside `sdk`'s range.
pub fn against(layout: &Layout, sdk: &SdkSpec) -> Vec<Dormant> {
    let Ok(state) = State::load(layout) else { return Vec::new() };
    let mut out = Vec::new();
    for name in state.packages.keys() {
        let Ok(m) = install::current_manifest(layout, name) else { continue };
        let (Some(plugin), Some(id)) = (m.plugin.as_ref(), m.plugin_id()) else { continue };
        if plugin.api < sdk.min_api || plugin.api > sdk.api {
            out.push(Dormant {
                package: name.clone(),
                plugin: id.to_string(),
                api: plugin.api,
                sdk_min_api: sdk.min_api,
                sdk_api: sdk.api,
            });
        }
    }
    out
}

/// The dormant plugins under the installed SDK (none when no SDK package is installed).
pub fn current(layout: &Layout) -> Vec<Dormant> {
    match installed_sdk(layout) {
        Some((_, sdk, _)) => against(layout, &sdk),
        None => Vec::new(),
    }
}

/// The lines an SDK install or rollback adds to its result: which plugins now wait for an update.
pub fn notes(layout: &Layout) -> Vec<String> {
    let d = current(layout);
    if d.is_empty() {
        return Vec::new();
    }
    let mut out = vec![format!(
        "dormant until updated (their data is kept; they run again once a build for this SDK installs): {}",
        d.iter().map(|x| format!("{} ({})", x.plugin, x.reason())).collect::<Vec<_>>().join(", ")
    )];
    out.push("update them with: mibpkg update-all".to_string());
    out
}
