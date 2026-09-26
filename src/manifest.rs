//! manifest.toml: what a package contains and what it runs.
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::{Component, Path, PathBuf};

/// Boot phases this mibpkg implements. Anything else "requires a newer mibpkg".
pub const KNOWN_PHASES: &[&str] = &["default"];

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
        self.version()?;
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
}

pub fn valid_name(s: &str) -> bool {
    !s.is_empty() && s.len() <= 40 && s.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
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
