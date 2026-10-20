//! manifest.toml: what a package contains and what it runs.
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::{Component, Path, PathBuf};

/// Boot phases this mibpkg implements. Anything else "requires a newer mibpkg".
pub const KNOWN_PHASES: &[&str] = &["default"];

/// Package names that would collide with mibpkg's own /tmp/mibpkg.<name>* files.
pub const RESERVED: &[&str] = &["restart", "reload", "stop", "selfupdate", "pid", "lock", "status", "log", "rw", "rwlock"];

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub package: PackageMeta,
    #[serde(default)]
    pub compat: Compat,
    #[serde(default, rename = "file")]
    pub files: Vec<FileEntry>,
    #[serde(default, rename = "config")]
    pub configs: Vec<ConfigEntry>,
    #[serde(default, rename = "service")]
    pub services: Vec<ServiceEntry>,
    #[serde(default)]
    pub scripts: Scripts,
    /// Reserved for stock-file patches (not implemented in v1).
    #[serde(default)]
    pub patch: Option<toml::Value>,
    /// A mibsdk plugin: one jar the SDK runtime loads. Declarative, like [sdk].
    #[serde(default)]
    pub plugin: Option<PluginSpec>,
    /// The mibsdk runtime itself (package `mibsdk`).
    #[serde(default)]
    pub sdk: Option<SdkSpec>,
}

/// `[plugin]`. The package name is the plugin id; the jar's stamp is `<jar>.stamp`.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PluginSpec {
    pub jar: String,
    pub api: u32,
    pub permissions: Vec<String>,
}

/// `[sdk]`: the values the SDK install writes into the unit's bundle list and sdk.properties.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SdkSpec {
    pub api: u32,
    pub min_api: u32,
    pub hmi: Vec<String>,
    pub jars: Vec<String>,
    /// One stamp for all jars, e.g. "mibsdk-unit.stamp".
    pub stamp: String,
    pub configurator: String,
    /// A bundle-list line, `<name>=<activator class>`.
    pub bundle: String,
    pub tracer_configurator: String,
    /// Class names; the bundle-list line is `HMICar=<value>`.
    pub hmicar_stock: String,
    pub hmicar: String,
    /// Class path prefixes only the SDK may define, e.g. "de/audi/tghu/car/hmi/".
    pub owned: Vec<String>,
    /// sdk.properties lines.
    #[serde(default)]
    pub properties: Vec<String>,
    /// The permission catalog file, e.g. "permissions.json".
    pub permissions: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Plain,
    Plugin,
    Sdk,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PackageMeta {
    pub name: String,
    pub version: String,
    #[serde(default)]
    pub description: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Compat {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub units: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub firmware: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FileEntry {
    pub src: String,
    #[serde(default = "default_mode")]
    pub mode: String,
}

fn default_mode() -> String {
    "644".into()
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConfigEntry {
    pub path: String,
    #[serde(default)]
    pub example: Option<String>,
    #[serde(default)]
    pub required: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ServiceEntry {
    pub name: String,
    pub exec: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default = "default_phase")]
    pub phase: String,
    #[serde(default)]
    pub restart: Restart,
    /// Start conditions (all must hold). See conditions.rs.
    #[serde(default)]
    pub after: Vec<String>,
    /// Start anyway after waiting this long ("30s", "5m", "2h"); omitted = wait forever.
    #[serde(default)]
    pub after_timeout: Option<String>,
}

fn default_phase() -> String {
    "default".into()
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Restart {
    #[default]
    Always,
    OnFailure,
    Never,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Scripts {
    pub post_install: Option<String>,
    pub pre_remove: Option<String>,
}

impl Manifest {
    pub fn parse(text: &str) -> Result<Manifest> {
        let m: Manifest = toml::from_str(text).context("invalid manifest.toml")?;
        m.validate()?;
        Ok(m)
    }

    pub fn version(&self) -> Result<semver::Version> {
        semver::Version::parse(&self.package.version)
            .with_context(|| format!("version {:?} is not semver (like 1.2.0)", self.package.version))
    }

    pub fn kind(&self) -> Kind {
        match (&self.plugin, &self.sdk) {
            (Some(_), _) => Kind::Plugin,
            (None, Some(_)) => Kind::Sdk,
            (None, None) => Kind::Plain,
        }
    }

    /// Files a [plugin] or [sdk] package must carry (`archive::pack` checks them).
    pub fn kind_files(&self) -> Vec<String> {
        match (&self.plugin, &self.sdk) {
            (Some(p), _) => vec![p.jar.clone(), format!("{}.stamp", p.jar)],
            (None, Some(s)) => {
                let mut v = s.jars.clone();
                v.push(s.stamp.clone());
                v.push(s.permissions.clone());
                v
            }
            (None, None) => Vec::new(),
        }
    }

    pub fn uses_scripts(&self) -> bool {
        self.scripts.post_install.is_some() || self.scripts.pre_remove.is_some()
    }

    fn validate(&self) -> Result<()> {
        if self.patch.is_some() {
            bail!("package uses [[patch]], which requires a newer mibpkg");
        }
        if !valid_name(&self.package.name) {
            bail!("package name {:?} must be 1-40 of [a-z0-9-]", self.package.name);
        }
        if RESERVED.contains(&self.package.name.as_str()) {
            bail!("package name {:?} is reserved (mibpkg uses it for its own /tmp files)", self.package.name);
        }
        self.version()?;
        self.validate_kind()?;
        for f in &self.files {
            safe_rel(&f.src)?;
            parse_mode(&f.mode)?;
        }
        for c in &self.configs {
            safe_rel(&c.path)?;
            if let Some(e) = &c.example {
                safe_rel(e)?;
            }
        }
        let mut seen = HashSet::new();
        for s in &self.services {
            if !valid_name(&s.name) {
                bail!("service name {:?} must be 1-40 of [a-z0-9-]", s.name);
            }
            if !seen.insert(s.name.as_str()) {
                bail!("service {} is declared twice", s.name);
            }
            if !KNOWN_PHASES.contains(&s.phase.as_str()) {
                bail!("service {} uses boot phase {:?}, which requires a newer mibpkg", s.name, s.phase);
            }
            for a in &s.after {
                crate::conditions::validate(a).with_context(|| format!("service {}", s.name))?;
            }
            if let Some(t) = &s.after_timeout {
                if s.after.is_empty() {
                    bail!("service {}: after_timeout needs an `after` list", s.name);
                }
                crate::conditions::parse_timeout(t).with_context(|| format!("service {}", s.name))?;
            }
            let rel = s
                .exec
                .strip_prefix("@pkg/")
                .with_context(|| format!("service {}: exec must start with @pkg/", s.name))?;
            safe_rel(rel)?;
            if !self.files.iter().any(|f| f.src == rel) {
                bail!("service {}: exec {} is not listed in [[file]]", s.name, s.exec);
            }
        }
        for s in [&self.scripts.post_install, &self.scripts.pre_remove].into_iter().flatten() {
            safe_rel(s)?;
        }
        Ok(())
    }

    fn validate_kind(&self) -> Result<()> {
        let name = self.package.name.as_str();
        if self.plugin.is_some() && self.sdk.is_some() {
            bail!("a package is either [plugin] or [sdk], not both");
        }
        if self.sdk.is_none() && name == SDK_PACKAGE {
            bail!("the name {SDK_PACKAGE} is reserved for the SDK package");
        }
        if self.kind() == Kind::Plain {
            return Ok(());
        }
        if !self.files.is_empty() || !self.services.is_empty() || !self.configs.is_empty() || self.uses_scripts() {
            bail!("plugin and sdk packages are declarative: no files, services, configs or scripts");
        }
        if let Some(p) = &self.plugin {
            if name.len() < 2 {
                bail!("plugin id {name:?} must be 2-40 characters");
            }
            file_name("[plugin] jar", &p.jar, ".jar")?;
            if p.jar != format!("{name}.jar") {
                bail!("[plugin] jar {:?} must be {name}.jar (the plugin id)", p.jar);
            }
            if p.api < 1 {
                bail!("[plugin] api must be at least 1");
            }
            for perm in &p.permissions {
                if perm.is_empty() || !perm.bytes().all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b)) {
                    bail!("permission {perm:?} must be 1 or more of [A-Za-z0-9._-]");
                }
            }
        }
        if let Some(s) = &self.sdk {
            if name != SDK_PACKAGE {
                bail!("the SDK package must be named {SDK_PACKAGE}, not {name:?}");
            }
            for (key, v) in [("jars", &s.jars), ("hmi", &s.hmi), ("owned", &s.owned)] {
                if v.is_empty() {
                    bail!("[sdk] {key} must not be empty");
                }
            }
            for j in &s.jars {
                file_name("[sdk] jars entry", j, ".jar")?;
                if !j.starts_with("mibsdk-") || j.starts_with("mibsdk-spike-") {
                    bail!("[sdk] jars entry {j:?} must start with mibsdk- (and not mibsdk-spike-)");
                }
            }
            for h in &s.hmi {
                if h.is_empty() || !h.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_') {
                    bail!("[sdk] hmi entry {h:?} must be 1 or more of [A-Za-z0-9_]");
                }
            }
            file_name("[sdk] stamp", &s.stamp, ".stamp")?;
            file_name("[sdk] permissions", &s.permissions, ".json")?;
            if s.min_api < 1 || s.min_api > s.api {
                bail!("[sdk] needs 1 <= min_api <= api (min_api {}, api {})", s.min_api, s.api);
            }
            for (key, v) in [
                ("configurator", &s.configurator),
                ("tracer_configurator", &s.tracer_configurator),
                ("hmicar_stock", &s.hmicar_stock),
                ("hmicar", &s.hmicar),
            ] {
                if !class_name(v) {
                    bail!("[sdk] {key} {v:?} must be a Java class name ([A-Za-z_$][A-Za-z0-9_$.]*)");
                }
            }
            let bundle_ok = s.bundle.split_once('=').is_some_and(|(k, class)| {
                !k.is_empty() && k.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_') && k != "HMICar" && class_name(class)
            });
            if !bundle_ok {
                bail!("[sdk] bundle {:?} must be <[A-Za-z0-9_]+>=<class name>, and not HMICar", s.bundle);
            }
            for p in &s.properties {
                property_line(p).with_context(|| format!("[sdk] properties entry {p:?}"))?;
            }
            for o in &s.owned {
                let ok = !o.is_empty()
                    && !o.starts_with('/')
                    && o.bytes().all(|b| b.is_ascii_alphanumeric() || b"_$/".contains(&b));
                if !ok {
                    bail!("[sdk] owned prefix {o:?} must be a class path like de/audi/tghu/car/hmi/ (no leading /)");
                }
            }
        }
        Ok(())
    }
}

/// The SDK package's name. Only a manifest with [sdk] may use it (it is not in RESERVED).
pub const SDK_PACKAGE: &str = "mibsdk";

/// One safe path component ending in `ext`.
fn file_name(what: &str, v: &str, ext: &str) -> Result<()> {
    if v.chars().any(|c| c.is_control() || c.is_whitespace()) {
        bail!("{what} {v:?} must not contain spaces or control characters");
    }
    let p = safe_rel(v).with_context(|| format!("{what} {v:?}"))?;
    if p.components().count() != 1 || !v.ends_with(ext) || v.len() == ext.len() {
        bail!("{what} {v:?} must be a single file name ending {ext}");
    }
    Ok(())
}

/// `[A-Za-z_$][A-Za-z0-9_$.]*`
fn class_name(s: &str) -> bool {
    let mut b = s.bytes();
    b.next().is_some_and(|c| c.is_ascii_alphabetic() || c == b'_' || c == b'$')
        && b.all(|c| c.is_ascii_alphanumeric() || b"_$.".contains(&c))
}

fn has_control(s: &str) -> bool {
    s.chars().any(char::is_control)
}

/// One sdk.properties line: `key=value`, no line continuation (an odd run of trailing
/// backslashes), and not `chain.configurator`, which the SDK install writes itself.
fn property_line(p: &str) -> Result<()> {
    if has_control(p) {
        bail!("must be a single line");
    }
    if !p.contains('=') {
        bail!("must be key=value");
    }
    if p.bytes().rev().take_while(|b| *b == b'\\').count() % 2 == 1 {
        bail!("must not end with an escaping backslash");
    }
    // java.util.Properties ends a key at the first '=', ':' or whitespace.
    let key = p.trim_start().split(|c: char| c == '=' || c == ':' || c.is_whitespace()).next().unwrap_or("");
    if key == "chain.configurator" {
        bail!("chain.configurator is written by the SDK install, not the manifest");
    }
    Ok(())
}

/// 1-40 of [a-z0-9-], starting with a letter or digit: names become file names and command
/// arguments, so one must never look like an option such as `--root`.
pub fn valid_name(s: &str) -> bool {
    let first_ok = s.bytes().next().map_or(false, |b| b.is_ascii_lowercase() || b.is_ascii_digit());
    first_ok && s.len() <= 40 && s.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

pub fn parse_mode(s: &str) -> Result<u32> {
    u32::from_str_radix(s, 8)
        .ok()
        .filter(|m| *m <= 0o777)
        .with_context(|| format!("invalid file mode {s:?} (octal, like \"755\")"))
}

/// A relative path that cannot escape its base directory.
pub fn safe_rel(p: &str) -> Result<PathBuf> {
    if p.is_empty() {
        bail!("empty path");
    }
    let path = Path::new(p);
    for c in path.components() {
        if !matches!(c, Component::Normal(_)) {
            bail!("unsafe path {p:?}: must be relative, without . or ..");
        }
    }
    Ok(path.to_path_buf())
}

/// Replace the @pkg / @config placeholders.
pub fn resolve(s: &str, pkg_dir: &str, config_dir: &str) -> String {
    s.replace("@pkg", pkg_dir).replace("@config", config_dir)
}
