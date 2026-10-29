//! Uploaded .mpk files (for cars without internet): kept in /tmp (RAM) under a server-generated
//! id, one at a time, and inspected before anything is installed.
use crate::archive::{sha256_file, Package};
use crate::index::{cmp_versions, Collection};
use crate::paths::Layout;
use crate::state::State;
use crate::unit::UnitInfo;
use serde::Serialize;
use std::cmp::Ordering;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// An upload not installed within this time is deleted.
pub const EXPIRES: Duration = Duration::from_secs(10 * 60);

struct Pending {
    id: String,
    path: PathBuf,
    created: Instant,
}

#[derive(Default)]
pub struct Uploads {
    pending: Mutex<Option<Pending>>,
    /// An upload body is being received (one at a time: each can be 50 MB of RAM in /tmp).
    receiving: std::sync::atomic::AtomicBool,
}

/// Why `store_from` did not store anything.
#[derive(Debug)]
pub enum StoreError {
    /// Another upload is being received.
    Busy,
    Io(io::Error),
}

/// /tmp/mibpkg.upload.<id>.mpk: where the `install-file` verb looks for it.
pub fn upload_path(layout: &Layout, id: &str) -> PathBuf {
    layout.run(&format!("upload.{id}.mpk"))
}

impl Uploads {
    fn lock(&self) -> std::sync::MutexGuard<'_, Option<Pending>> {
        self.pending.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Save a new upload, replacing (and deleting) any earlier one: RAM is scarce.
    pub fn store(&self, layout: &Layout, body: &[u8]) -> io::Result<String> {
        let mut r = body;
        self.store_from(layout, &mut r, body.len() as u64).map_err(|e| match e {
            StoreError::Busy => io::Error::new(io::ErrorKind::WouldBlock, "another upload is in progress"),
            StoreError::Io(e) => e,
        })
    }

    /// Copy `len` bytes from `r` (the connection) straight into the upload file, never holding
    /// the whole body in memory. The earlier upload, if any, is deleted first.
    pub fn store_from(&self, layout: &Layout, r: &mut dyn io::Read, len: u64) -> Result<String, StoreError> {
        use std::sync::atomic::Ordering;
        if self.receiving.swap(true, Ordering::SeqCst) {
            return Err(StoreError::Busy);
        }
        let result = (|| {
            if let Some(old) = self.lock().take() {
                let _ = fs::remove_file(old.path);
            }
            let id = super::settings::random_id();
            let path = upload_path(layout, &id);
            let copied = fs::File::create(&path).and_then(|mut f| io::copy(&mut io::Read::take(&mut *r, len), &mut f));
            match copied {
                Ok(n) if n == len => {
                    *self.lock() = Some(Pending { id: id.clone(), path, created: Instant::now() });
                    Ok(id)
                }
                Ok(_) => {
                    let _ = fs::remove_file(&path);
                    Err(StoreError::Io(io::Error::new(io::ErrorKind::UnexpectedEof, "the upload was cut off")))
                }
                Err(e) => {
                    let _ = fs::remove_file(&path);
                    Err(StoreError::Io(e))
                }
            }
        })();
        self.receiving.store(false, Ordering::SeqCst);
        result
    }

    /// The file of the pending upload `id`, if it is still there.
    pub fn path(&self, id: &str) -> Option<PathBuf> {
        self.lock().as_ref().filter(|p| p.id == id && p.path.exists()).map(|p| p.path.clone())
    }

    pub fn discard(&self, id: &str) {
        let mut g = self.lock();
        if g.as_ref().map_or(false, |p| p.id == id) {
            let p = g.take().unwrap();
            let _ = fs::remove_file(p.path);
        }
    }

    pub fn sweep(&self, layout: &Layout) {
        self.sweep_at(layout, Instant::now())
    }

    /// Delete an expired upload and any upload file this process does not know about.
    pub fn sweep_at(&self, layout: &Layout, now: Instant) {
        let mut g = self.lock();
        if g.as_ref().map_or(false, |p| p.created + EXPIRES < now) {
            let p = g.take().unwrap();
            let _ = fs::remove_file(p.path);
        }
        let keep = g.as_ref().map(|p| p.path.clone());
        let tmp = layout.p("/tmp");
        if let Ok(rd) = fs::read_dir(&tmp) {
            for e in rd.filter_map(|e| e.ok()) {
                let name = e.file_name().to_string_lossy().to_string();
                if name.starts_with("mibpkg.upload.") && Some(e.path()) != keep {
                    let _ = fs::remove_file(e.path());
                }
            }
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Candidate {
    pub collection: String,
    /// The collection's display name.
    pub name: String,
    /// Its newest version of the package.
    pub version: String,
    /// The uploaded file is byte-for-byte one of this collection's packages.
    pub matches_file: bool,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct Inspection {
    pub upload_id: String,
    pub name: Option<String>,
    pub version: Option<String>,
    pub description: String,
    pub size: u64,
    pub sha256: String,
    /// "fresh", "upgrade", "same" or "older".
    pub kind: Option<String>,
    pub installed_version: Option<String>,
    pub installed_collection: Option<String>,
    pub compatible: bool,
    pub uses_scripts: bool,
    pub services: Vec<String>,
    /// Reasons the file cannot be installed, in plain words. Empty = installable.
    pub problems: Vec<String>,
    pub candidates: Vec<Candidate>,
    /// Collection id to record, or "-" for none.
    pub preselected: Option<String>,
    /// The user must choose (several collections have the name and none matches the file).
    pub ask: bool,
    /// "plugin" or "sdk"; None for a plain package. (`kind` above is fresh/upgrade/same/older.)
    pub package_kind: Option<String>,
    /// The plugin id when this package carries a `[plugin]` table; may differ from `name`.
    pub plugin_id: Option<String>,
    /// A plugin's declared permissions, in plain words.
    pub permissions: Vec<crate::catalog::Perm>,
    /// The permissions installing this file asks the user to approve (what it adds over the
    /// installed version; a revoked one stays revoked and is not asked again).
    pub needs_approval: Vec<String>,
    /// Declared permissions the user revoked that stay revoked: the plugin stays blocked after
    /// installing this until they are granted on its page.
    pub stays_blocked: Vec<String>,
}

/// What installing a plugin declaring `declared` asks for, as `plugin_pkg::check` plans it: the
/// permissions it adds over the installed package's manifest (all of them for a fresh install).
fn approval_plan(layout: &Layout, package: &str, plugin_id: &str, declared: &[String]) -> crate::mibsdk::grants::Approval {
    let installed = State::load(layout).unwrap_or_default().packages.contains_key(package);
    let old = if installed { crate::install::current_manifest(layout, package).ok() } else { None };
    let declared_old = old.as_ref().and_then(|o| o.plugin.as_ref()).map(|p| p.permissions.as_slice());
    let current = crate::mibsdk::grants::read(layout).ok().and_then(|mut g| g.remove(plugin_id)).unwrap_or_default();
    crate::mibsdk::grants::plan(declared, declared_old, &current)
}

/// Official collections win ties when choosing between collections.
fn official(id: &str) -> bool {
    id == "complex256" || id.starts_with("complex256-")
}

pub fn inspect(layout: &Layout, unit: &UnitInfo, colls: &[Collection], id: &str, path: &Path) -> Inspection {
    let mut i = Inspection { upload_id: id.to_string(), ..Default::default() };
    if let Ok((sha, size)) = sha256_file(path) {
        i.sha256 = sha;
        i.size = size;
    }
    let pkg = match Package::read(path) {
        Ok(p) => p,
        Err(e) => {
            i.problems.push(format!("not a valid mibpkg package ({e:#})"));
            return i;
        }
    };
    let m = &pkg.manifest;
    let name = m.package.name.clone();
    i.name = Some(name.clone());
    i.version = Some(m.package.version.clone());
    i.description = m.package.description.clone();
    i.compatible = m.compat.matches(unit);
    i.uses_scripts = m.uses_scripts();
    i.services = m.services.iter().map(|s| format!("{name}.{}", s.name)).collect();
    i.package_kind = m.kind().name();
    if let Some(plugin) = &m.plugin {
        let plugin_id = plugin.id(&name).to_string();
        i.plugin_id = Some(plugin_id.clone());
        let cat = crate::mibsdk::catalog::installed(layout).ok().flatten();
        i.permissions = crate::catalog::permissions(cat.as_deref(), &plugin.permissions, None);
        let plan = approval_plan(layout, &name, &plugin_id, &plugin.permissions);
        i.stays_blocked = crate::mibsdk::grants::missing(&plugin.permissions, &plan.grant);
        i.needs_approval = plan.ask;
    }
    if !i.compatible {
        i.problems.push(format!("{name} {} is not compatible with this unit ({})", m.package.version, unit.describe()));
    }

    let state = State::load(layout).unwrap_or_default();
    if let Some(inst) = state.packages.get(&name) {
        i.installed_version = Some(inst.version.clone());
        i.installed_collection = Some(inst.collection.clone());
        i.preselected = Some(inst.collection.clone());
        let kind = match cmp_versions(&m.package.version, &inst.version) {
            Ordering::Greater => "upgrade",
            Ordering::Equal => {
                i.problems.push(format!("{name} {} is already installed", inst.version));
                "same"
            }
            Ordering::Less => {
                i.problems.push(format!("this is older than the installed {} (use Roll back instead)", inst.version));
                "older"
            }
        };
        i.kind = Some(kind.into());
        return i;
    }
    i.kind = Some("fresh".into());

    // Collections that offer this name. Card and online copies of one id count once.
    for c in colls {
        let Some(latest) = c.latest(&name) else { continue };
        let matches = c.index.packages.iter().any(|e| e.name == name && e.sha256 == i.sha256);
        match i.candidates.iter_mut().find(|x| x.collection == c.id()) {
            Some(x) => {
                x.matches_file |= matches;
                if cmp_versions(&latest.version, &x.version) == Ordering::Greater {
                    x.version = latest.version.clone();
                }
            }
            None => i.candidates.push(Candidate {
                collection: c.id().to_string(),
                name: c.index.collection.name.clone(),
                version: latest.version.clone(),
                matches_file: matches,
            }),
        }
    }

    // Preselection: exact file match, else the only one, else the newest (official wins ties).
    let matched = i.candidates.iter().find(|c| c.matches_file).map(|c| c.collection.clone());
    i.preselected = Some(match (matched, i.candidates.len()) {
        (Some(id), _) => id,
        (None, 0) => "-".into(),
        (None, 1) => i.candidates[0].collection.clone(),
        (None, _) => {
            i.ask = true;
            i.candidates
                .iter()
                .max_by(|a, b| cmp_versions(&a.version, &b.version).then(official(&a.collection).cmp(&official(&b.collection))))
                .map(|c| c.collection.clone())
                .unwrap()
        }
    });
    i
}
