//! Host publisher tooling: `mibpkg release` builds packages into a GitHub release whose
//! index.json carries older versions forward (every entry an absolute download URL), and
//! `mibpkg verify` checks a published or local collection the way a car would.
use crate::archive::{self, sha256_file, Package};
use crate::fetch::{Fetcher, MAX_INDEX};
use crate::index::{cmp_versions, Collection, CollectionMeta, Index, IndexEntry, FORMAT};
use anyhow::{bail, Context, Result};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

/// Creates GitHub releases (real: the `gh` CLI; tests: a fake).
pub trait Publisher {
    /// Tag of the repo's latest release, None when it has none.
    fn latest_tag(&self, repo: &str) -> Result<Option<String>>;
    fn create(&self, repo: &str, tag: &str, notes: &str, files: &[PathBuf]) -> Result<()>;
}

pub struct Gh;

impl Publisher for Gh {
    fn latest_tag(&self, repo: &str) -> Result<Option<String>> {
        let out = Command::new("gh")
            .args(["release", "view", "--repo", repo, "--json", "tagName", "-q", ".tagName"])
            .output()
            .context("running gh (install the GitHub CLI and run `gh auth login`, or set GH_TOKEN)")?;
        if out.status.success() {
            let t = String::from_utf8_lossy(&out.stdout).trim().to_string();
            return Ok((!t.is_empty()).then_some(t));
        }
        let err = String::from_utf8_lossy(&out.stderr);
        if err.contains("release not found") {
            return Ok(None);
        }
        bail!("gh release view: {}", err.trim())
    }

    fn create(&self, repo: &str, tag: &str, notes: &str, files: &[PathBuf]) -> Result<()> {
        let st = Command::new("gh")
            .args(["release", "create", tag, "--repo", repo, "--title", tag, "--notes", notes])
            .args(files)
            .status()
            .context("running gh (install the GitHub CLI and run `gh auth login`, or set GH_TOKEN)")?;
        if !st.success() {
            bail!("gh release create failed; the files are still in the out directory, run again to retry");
        }
        Ok(())
    }
}

pub struct ReleaseOpts {
    pub inputs: Vec<PathBuf>,
    pub repo: String,
    pub tag: String,
    pub id: Option<String>,
    pub name: Option<String>,
    pub new_id: bool,
    pub keep: usize,
    pub out: PathBuf,
    pub assets: Vec<PathBuf>,
    pub publish: bool,
}

#[derive(Debug)]
pub struct ReleaseReport {
    pub index: Index,
    /// "name version" of each package new in this release.
    pub added: Vec<String>,
    /// "name version" dropped by --keep.
    pub dropped: Vec<String>,
    /// Entries carried over from the previous release.
    pub kept: usize,
    /// Everything in the out directory (what --publish uploads).
    pub files: Vec<PathBuf>,
    pub summary: String,
}

pub fn download_url(repo: &str, tag: &str, file: &str) -> String {
    format!("https://github.com/{repo}/releases/download/{tag}/{file}")
}

fn latest_index_url(repo: &str) -> Result<String> {
    crate::online::index_url(&format!("github:{repo}"))
}

/// The out directory, emptied — but only if it holds nothing except what a release writes
/// (.mpk files, index.json, the given assets), so a mistyped --out never wipes anything else.
fn fresh_out(out: &Path, assets: &[PathBuf]) -> Result<()> {
    if out.exists() {
        let asset_names: Vec<_> = assets.iter().filter_map(|a| a.file_name().map(|n| n.to_os_string())).collect();
        for e in fs::read_dir(out)? {
            let e = e?;
            let n = e.file_name();
            let ours = e.file_type()?.is_file()
                && (n == "index.json" || n.to_string_lossy().ends_with(".mpk") || asset_names.contains(&n));
            if !ours {
                bail!("{} is not a release output directory (it contains {}); choose another --out", out.display(), n.to_string_lossy());
            }
        }
        fs::remove_dir_all(out)?;
    }
    fs::create_dir_all(out)?;
    Ok(())
}

/// The previous release's index; None for a first release (HTTP 404).
fn previous(fetcher: &dyn Fetcher, repo: &str) -> Result<Option<Index>> {
    let url = latest_index_url(repo)?;
    match fetcher.get(&url, MAX_INDEX) {
        Ok(b) => Ok(Some(serde_json::from_slice(&b).with_context(|| format!("{url}: not a valid index.json"))?)),
        Err(e) if format!("{e:#}").contains("HTTP 404") => Ok(None),
        Err(e) => Err(e).context("fetching the previous release's index"),
    }
}

/// Build or copy each input into `out` as <name>-<version>.mpk.
fn stage(inputs: &[PathBuf], out: &Path) -> Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    for input in inputs {
        let tmp = out.join(".staging.mpk");
        let m = if input.is_dir() {
            archive::pack(input, &tmp)?
        } else {
            fs::copy(input, &tmp).with_context(|| format!("reading {}", input.display()))?;
            Package::read(&tmp).with_context(|| format!("{} is not a valid package", input.display()))?.manifest
        };
        let dest = out.join(format!("{}-{}.mpk", m.package.name, m.package.version));
        if files.contains(&dest) {
            bail!("{} {} is given twice", m.package.name, m.package.version);
        }
        fs::rename(&tmp, &dest)?;
        files.push(dest);
    }
    Ok(files)
}

pub fn release(o: &ReleaseOpts, fetcher: &dyn Fetcher, publisher: &dyn Publisher) -> Result<ReleaseReport> {
    latest_index_url(&o.repo)?; // validates owner/repo
    if o.tag.starts_with('-') || o.tag.is_empty() || !o.tag.bytes().all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b)) {
        bail!("tag {:?}: use letters, digits, '.', '_' and '-'", o.tag);
    }
    let prev = previous(fetcher, &o.repo)?;
    let id = match (&o.id, &prev) {
        (Some(id), Some(p)) if *id != p.collection.id && !o.new_id => bail!(
            "the published collection id is {:?}; changing it to {id:?} makes every car treat it as a different collection (pass --new-id if you mean it)",
            p.collection.id
        ),
        (Some(id), _) => id.clone(),
        (None, Some(p)) => p.collection.id.clone(),
        (None, None) => bail!("first release of {}: pass --id <collection-id> (and --name)", o.repo),
    };
    let name = o.name.clone().or_else(|| prev.as_ref().map(|p| p.collection.name.clone())).unwrap_or_else(|| id.clone());

    fresh_out(&o.out, &o.assets)?;
    let staged = stage(&o.inputs, &o.out)?;

    // Previous entries, with 0.3.x-era relative files made absolute against the previous release.
    let mut entries: Vec<IndexEntry> = Vec::new();
    let mut prev_tag: Option<String> = None;
    for mut e in prev.map(|p| p.packages).unwrap_or_default() {
        if !e.file.starts_with("https://") {
            if prev_tag.is_none() {
                prev_tag = Some(publisher.latest_tag(&o.repo)?.context("the previous index has relative package paths but the repo has no latest release tag")?);
            }
            e.file = download_url(&o.repo, prev_tag.as_deref().unwrap(), &e.file);
        }
        entries.push(e);
    }
    let kept_before = entries.len();

    let mut added = Vec::new();
    for f in &staged {
        let pkg = Package::read(f)?;
        let m = &pkg.manifest;
        let (sha256, size) = sha256_file(f)?;
        if let Some(old) = entries.iter().find(|e| e.name == m.package.name && e.version == m.package.version) {
            if old.sha256 != sha256 {
                bail!("{} {} is already published with different contents; bump the version", m.package.name, m.package.version);
            }
            fs::remove_file(f)?; // identical: keep the published file, upload nothing
            continue;
        }
        let fname = f.file_name().unwrap().to_string_lossy().to_string();
        entries.push(IndexEntry::new(m, &pkg.requires()?, download_url(&o.repo, &o.tag, &fname), sha256, size));
        added.push(format!("{} {}", m.package.name, m.package.version));
    }

    // Keep the newest `keep` versions of each name.
    let mut by_name: BTreeMap<String, Vec<IndexEntry>> = BTreeMap::new();
    for e in entries {
        by_name.entry(e.name.clone()).or_default().push(e);
    }
    let mut packages = Vec::new();
    let mut dropped = Vec::new();
    for (_, mut v) in by_name {
        v.sort_by(|a, b| cmp_versions(&b.version, &a.version));
        for (i, e) in v.into_iter().enumerate() {
            if i < o.keep.max(1) {
                packages.push(e);
            } else {
                dropped.push(format!("{} {}", e.name, e.version));
            }
        }
    }
    // A version given now but already outside the newest `keep` would be uploaded and never listed.
    if let Some(d) = dropped.iter().find(|d| added.contains(d)) {
        bail!("{d} is older than the newest {} version(s) kept; raise --keep or leave it out", o.keep.max(1));
    }
    packages.sort_by(|a, b| a.name.cmp(&b.name).then(cmp_versions(&a.version, &b.version)));
    let index = Index { format: FORMAT, collection: CollectionMeta { id, name }, packages, signatures: vec![] };
    Collection::from_online(index.clone(), "https://github.com/").context("the new index would be refused by cars")?;

    let mut text = serde_json::to_string_pretty(&index)?;
    text.push('\n');
    fs::write(o.out.join("index.json"), text)?;
    for a in &o.assets {
        let n = a.file_name().context("asset without a file name")?;
        if n == "index.json" || o.out.join(n).exists() {
            bail!("asset {} would replace a release file", a.display());
        }
        fs::copy(a, o.out.join(n)).with_context(|| format!("copying {}", a.display()))?;
    }
    let mut files: Vec<PathBuf> = fs::read_dir(&o.out)?.filter_map(|e| e.ok().map(|e| e.path())).collect();
    files.sort();

    let kept = index.packages.len() - added.len();
    let summary = format!(
        "{}: added {}; kept {} older; dropped {}",
        o.tag,
        if added.is_empty() { "nothing".to_string() } else { added.join(", ") },
        kept.min(kept_before),
        if dropped.is_empty() { "nothing".to_string() } else { dropped.join(", ") }
    );
    if o.publish {
        publisher.create(&o.repo, &o.tag, &summary, &files)?;
    }
    Ok(ReleaseReport { index, added, dropped, kept, files, summary })
}

/// Poll `url` until it serves `want` (GitHub's latest-release redirect lags ~20 s).
pub fn wait_live(fetcher: &dyn Fetcher, url: &str, want: &[u8], timeout: Duration, every: Duration) -> bool {
    let start = Instant::now();
    loop {
        if fetcher.get(url, MAX_INDEX).map_or(false, |b| b == want) {
            return true;
        }
        if start.elapsed() >= timeout {
            return false;
        }
        std::thread::sleep(every);
    }
}

#[derive(Debug, Default)]
pub struct VerifyReport {
    pub checked: usize,
    pub problems: Vec<String>,
    pub notes: Vec<String>,
}

/// Check a source (github:… / https://…index.json), a local index.json, or a release output dir
/// the way a car would. Err only when no index can be read at all.
pub fn verify(target: &str, latest_only: bool, unit: Option<&crate::unit::UnitInfo>, fetcher: &dyn Fetcher) -> Result<VerifyReport> {
    let path = Path::new(target);
    let (bytes, base, local) = if path.exists() {
        let file = if path.is_dir() { path.join("index.json") } else { path.to_path_buf() };
        let dir = file.parent().map(Path::to_path_buf).unwrap_or_default();
        (fs::read(&file).with_context(|| format!("reading {}", file.display()))?, "https://local.invalid/".to_string(), Some(dir))
    } else {
        let url = crate::online::index_url(target)?;
        (fetcher.get(&url, MAX_INDEX)?, crate::online::base_of(&url), None)
    };
    let mut r = VerifyReport::default();
    let index: Index = serde_json::from_slice(&bytes).context("not a valid index.json")?;
    let coll = match Collection::from_online(index, &base) {
        Ok(c) => c,
        Err(e) => {
            r.problems.push(format!("index: {e:#}"));
            return Ok(r);
        }
    };
    let mut entries: Vec<&IndexEntry> = coll.index.packages.iter().collect();
    if latest_only {
        entries.retain(|e| coll.latest(&e.name).map_or(false, |l| l.version == e.version));
    }
    // One temp file per call: verify can run on several threads of one process (tests).
    static CALLS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let n = CALLS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let tmp = std::env::temp_dir().join(format!("mibpkg-verify-{}-{n}.mpk", std::process::id()));
    for e in entries {
        r.checked += 1;
        let what = format!("{} {}", e.name, e.version);
        if let Some(u) = unit {
            if !e.compat.matches(u) {
                r.notes.push(format!("{what}: not for {}", u.unit.as_deref().unwrap_or("this unit")));
            }
        }
        // Local: a release output dir holds the file by its basename; an SD card collection by its
        // relative path (packages/<file>.mpk).
        let basename = e.file.rsplit('/').next().unwrap_or(&e.file);
        let local_file = local.as_ref().and_then(|dir| {
            let rel = (!e.file.contains("://")).then(|| dir.join(&e.file));
            rel.into_iter().chain([dir.join(basename)]).find(|p| p.is_file())
        });
        let got = match &local_file {
            Some(p) => fs::read(p).map_err(anyhow::Error::from),
            _ => match coll.location(e) {
                crate::index::Location::Remote(url) => fetcher.get(&url, crate::fetch::MAX_PACKAGE),
                crate::index::Location::Local(p) => fs::read(&p).map_err(anyhow::Error::from),
            },
        };
        let bytes = match got {
            Ok(b) => b,
            Err(err) => {
                r.problems.push(format!("{what}: {err:#}"));
                continue;
            }
        };
        if let Err(err) = fs::write(&tmp, &bytes) {
            r.problems.push(format!("{what}: writing a temp file: {err}"));
            continue;
        }
        let (sha, size) = match sha256_file(&tmp) {
            Ok(x) => x,
            Err(err) => {
                r.problems.push(format!("{what}: {err:#}"));
                continue;
            }
        };
        if size != e.size || sha != e.sha256 {
            r.problems.push(format!("{what}: size/sha256 do not match the index (got {size} bytes, sha256 {sha})"));
            continue;
        }
        match Package::read(&tmp) {
            Err(err) => r.problems.push(format!("{what}: not a valid package: {err:#}")),
            Ok(p) => {
                let m = &p.manifest;
                if m.package.name != e.name || m.package.version != e.version {
                    r.problems.push(format!("{what}: the file holds {} {}", m.package.name, m.package.version));
                } else if m.uses_scripts() != e.uses_scripts {
                    r.problems.push(format!("{what}: uses_scripts is {} in the index but {} in the package", e.uses_scripts, m.uses_scripts()));
                }
            }
        }
    }
    let _ = fs::remove_file(&tmp);
    Ok(r)
}
