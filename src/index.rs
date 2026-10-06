//! A collection = a directory with index.json + packages/*.mpk.
use crate::archive::{sha256_file, Package};
use crate::fsutil::atomic_write;
use crate::manifest::{safe_rel, valid_name, Compat};
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::fs;
use std::path::{Path, PathBuf};

pub const FORMAT: u32 = 1;

/// Collection id recorded for packages installed from an uploaded file; no index may use it.
pub const RESERVED_COLLECTION: &str = "uploaded";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Index {
    pub format: u32,
    pub collection: CollectionMeta,
    pub packages: Vec<IndexEntry>,
    /// Reserved for signature verification (after online updates); ignored in v1.
    #[serde(default)]
    pub signatures: Vec<serde_json::Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CollectionMeta {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexEntry {
    pub name: String,
    pub version: String,
    pub file: String,
    pub sha256: String,
    pub size: u64,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub compat: Compat,
    #[serde(default)]
    pub uses_scripts: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Location {
    Local(PathBuf),
    Remote(String),
}

#[derive(Debug, Clone)]
pub struct Collection {
    pub dir: PathBuf,
    pub index: Index,
    /// Set for collections fetched online: the index URL's directory.
    pub base_url: Option<String>,
}

impl Collection {
    pub fn load(dir: &Path) -> Result<Collection> {
        let p = dir.join("index.json");
        let text = fs::read_to_string(&p).with_context(|| format!("reading {}", p.display()))?;
        let index: Index = serde_json::from_str(&text).with_context(|| format!("parsing {}", p.display()))?;
        if index.format != FORMAT {
            bail!("{}: index format {} is not supported by this mibpkg", p.display(), index.format);
        }
        if !valid_name(&index.collection.id) {
            bail!("{}: collection id {:?} must be 1-40 of [a-z0-9-]", p.display(), index.collection.id);
        }
        reserved_check(&index.collection.id)?;
        for e in &index.packages {
            // Names become file names and script arguments on the unit.
            if !valid_name(&e.name) {
                bail!("{}: package name {:?} must be 1-40 of [a-z0-9-]", p.display(), e.name);
            }
            semver::Version::parse(&e.version)
                .with_context(|| format!("{}: {} version {:?} is not semver", p.display(), e.name, e.version))?;
            safe_rel(&e.file).with_context(|| format!("{}: package {}", p.display(), e.name))?;
        }
        Ok(Collection { dir: dir.to_path_buf(), index, base_url: None })
    }

    /// A collection fetched from `base_url` (the index URL's directory). Package files are safe
    /// relative paths (resolved against base_url) or absolute https:// URLs.
    pub fn from_online(index: Index, base_url: &str) -> Result<Collection> {
        if index.format != FORMAT {
            bail!("online index format {} is not supported by this mibpkg", index.format);
        }
        if !valid_name(&index.collection.id) {
            bail!("collection id {:?} must be 1-40 of [a-z0-9-]", index.collection.id);
        }
        reserved_check(&index.collection.id)?;
        for e in &index.packages {
            if !valid_name(&e.name) {
                bail!("package name {:?} must be 1-40 of [a-z0-9-]", e.name);
            }
            semver::Version::parse(&e.version).with_context(|| format!("{} version {:?}", e.name, e.version))?;
            if !e.file.starts_with("https://") {
                if e.file.contains("://") {
                    bail!("{}: package URL must be https://", e.name);
                }
                safe_rel(&e.file).with_context(|| format!("package {}", e.name))?;
            }
        }
        Ok(Collection { dir: PathBuf::new(), index, base_url: Some(base_url.to_string()) })
    }

    pub fn location(&self, e: &IndexEntry) -> Location {
        match &self.base_url {
            None => Location::Local(self.dir.join(&e.file)),
            Some(_) if e.file.starts_with("https://") => Location::Remote(e.file.clone()),
            Some(base) => Location::Remote(format!("{base}{}", e.file)),
        }
    }

    pub fn id(&self) -> &str {
        &self.index.collection.id
    }

    pub fn archive_path(&self, e: &IndexEntry) -> PathBuf {
        self.dir.join(&e.file)
    }

    /// Newest version of `name` that runs on this unit.
    pub fn latest_compatible(&self, name: &str, unit: &crate::unit::UnitInfo) -> Option<&IndexEntry> {
        self.index
            .packages
            .iter()
            .filter(|e| e.name == name && e.compat.matches(unit))
            .max_by(|a, b| cmp_versions(&a.version, &b.version))
    }

    /// Newest version of `name` in this collection.
    pub fn latest(&self, name: &str) -> Option<&IndexEntry> {
        self.index
            .packages
            .iter()
            .filter(|e| e.name == name)
            .max_by(|a, b| cmp_versions(&a.version, &b.version))
    }
}

fn reserved_check(id: &str) -> Result<()> {
    if id == RESERVED_COLLECTION {
        bail!("collection id {id:?} is reserved (mibpkg records uploaded packages under it)");
    }
    Ok(())
}

pub fn cmp_versions(a: &str, b: &str) -> Ordering {
    match (semver::Version::parse(a), semver::Version::parse(b)) {
        (Ok(x), Ok(y)) => x.cmp(&y),
        _ => a.cmp(b),
    }
}

/// Host tooling: index every packages/*.mpk in `dir`.
pub fn build_index(dir: &Path, id: &str, name: &str) -> Result<Index> {
    index_dir(&dir.join("packages"), id, name, "packages/")
}

/// Host tooling: index every *.mpk next to the index (online collections: release assets are flat).
pub fn build_index_flat(dir: &Path, id: &str, name: &str) -> Result<Index> {
    index_dir(dir, id, name, "")
}

fn index_dir(pkgs: &Path, id: &str, name: &str, prefix: &str) -> Result<Index> {
    if !valid_name(id) {
        bail!("collection id {id:?} must be 1-40 of [a-z0-9-]");
    }
    reserved_check(id)?;
    let mut files: Vec<PathBuf> = fs::read_dir(pkgs)
        .with_context(|| format!("reading {}", pkgs.display()))?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().map_or(false, |x| x == "mpk"))
        .collect();
    files.sort();
    let mut packages = Vec::new();
    for f in files {
        let pkg = Package::read(&f).with_context(|| format!("reading {}", f.display()))?;
        let m = &pkg.manifest;
        let fname = f.file_name().unwrap().to_string_lossy().to_string();
        let want = format!("{}-{}.mpk", m.package.name, m.package.version);
        if fname != want {
            bail!("{fname} holds {} {}; it must be named {want}", m.package.name, m.package.version);
        }
        let (sha256, size) = sha256_file(&f)?;
        packages.push(IndexEntry {
            name: m.package.name.clone(),
            version: m.package.version.clone(),
            file: format!("{prefix}{fname}"),
            sha256,
            size,
            description: m.package.description.clone(),
            compat: m.compat.clone(),
            uses_scripts: m.uses_scripts(),
        });
    }
    Ok(Index {
        format: FORMAT,
        collection: CollectionMeta { id: id.into(), name: name.into() },
        packages,
        signatures: Vec::new(),
    })
}

pub fn write_index(dir: &Path, index: &Index) -> Result<()> {
    let mut text = serde_json::to_string_pretty(index)?;
    text.push('\n');
    atomic_write(&dir.join("index.json"), text.as_bytes())?;
    Ok(())
}
