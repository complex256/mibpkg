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

#[derive(Debug, Clone)]
pub struct Collection {
    pub dir: PathBuf,
    pub index: Index,
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
        for e in &index.packages {
            // Names become file names and script arguments on the unit.
            if !valid_name(&e.name) {
                bail!("{}: package name {:?} must be 1-40 of [a-z0-9-]", p.display(), e.name);
            }
            semver::Version::parse(&e.version)
                .with_context(|| format!("{}: {} version {:?} is not semver", p.display(), e.name, e.version))?;
            safe_rel(&e.file).with_context(|| format!("{}: package {}", p.display(), e.name))?;
        }
        Ok(Collection { dir: dir.to_path_buf(), index })
    }

    pub fn id(&self) -> &str {
        &self.index.collection.id
    }

    pub fn archive_path(&self, e: &IndexEntry) -> PathBuf {
        self.dir.join(&e.file)
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

pub fn cmp_versions(a: &str, b: &str) -> Ordering {
    match (semver::Version::parse(a), semver::Version::parse(b)) {
        (Ok(x), Ok(y)) => x.cmp(&y),
        _ => a.cmp(b),
    }
}

/// Host tooling: index every packages/*.mpk in `dir`.
pub fn build_index(dir: &Path, id: &str, name: &str) -> Result<Index> {
    if !valid_name(id) {
        bail!("collection id {id:?} must be 1-40 of [a-z0-9-]");
    }
    let mut files: Vec<PathBuf> = fs::read_dir(dir.join("packages"))
        .with_context(|| format!("reading {}/packages", dir.display()))?
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
            file: format!("packages/{fname}"),
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
