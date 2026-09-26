#![allow(dead_code)]
use mibpkg::paths::Layout;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

pub const IMG_VER: &str = "#DEV_MMX2QC_AU_NAR_G22_061PROD\n636\n";

pub const STARTUP: &str = "#!/bin/sh\nstart_late_drivers()\n{\n    echo late\n    inetd > /dev/null 2>&1\n    echo after\n}\nstart_late_drivers\n";

pub struct Fixture {
    pub dir: tempfile::TempDir,
    pub layout: Layout,
}

impl Fixture {
    pub fn root(&self) -> &Path {
        self.dir.path()
    }
    /// The SD card mount inside the fake root.
    pub fn sd(&self) -> PathBuf {
        self.root().join("fs/sda0")
    }
}

pub fn fixture() -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let r = dir.path();
    for d in ["mnt/app/root", "mnt/app/eso/hmi/engdefs", "mnt/system/etc/boot", "tmp", "fs/sda0"] {
        fs::create_dir_all(r.join(d)).unwrap();
    }
    fs::write(r.join("mnt/app/img_ver.txt"), IMG_VER).unwrap();
    fs::write(r.join("mnt/system/etc/boot/startup.sh"), STARTUP).unwrap();
    fs::set_permissions(r.join("mnt/system/etc/boot/startup.sh"), fs::Permissions::from_mode(0o755)).unwrap();
    let layout = Layout::load(r).unwrap();
    Fixture { dir, layout }
}

/// A package source dir with one service binary (a shell script) at bin/<name>.
/// `extra` is appended to the manifest.
pub fn pkg_src(dir: &Path, name: &str, version: &str, body: &str, extra: &str) -> PathBuf {
    let s = dir.join(format!("src-{name}-{version}"));
    fs::create_dir_all(s.join("bin")).unwrap();
    let manifest = format!(
        "[package]\nname = \"{name}\"\nversion = \"{version}\"\ndescription = \"{name} test package\"\n\n\
         [[file]]\nsrc = \"bin/{name}\"\nmode = \"755\"\n\n\
         [[service]]\nname = \"{name}\"\nexec = \"@pkg/bin/{name}\"\n{extra}"
    );
    fs::write(s.join("manifest.toml"), manifest).unwrap();
    fs::write(s.join(format!("bin/{name}")), format!("#!/bin/sh\n{body}\n")).unwrap();
    fs::set_permissions(s.join(format!("bin/{name}")), fs::Permissions::from_mode(0o755)).unwrap();
    s
}

/// Pack `srcs` into <media_root>/mibpkg/collections/<id>/ and write its index.
pub fn collection(media_root: &Path, id: &str, srcs: &[PathBuf]) -> PathBuf {
    let dir = media_root.join("mibpkg/collections").join(id);
    fs::create_dir_all(dir.join("packages")).unwrap();
    for s in srcs {
        let tmp = dir.join("packages/tmp.mpk");
        let m = mibpkg::archive::pack(s, &tmp).unwrap();
        fs::rename(&tmp, dir.join(format!("packages/{}-{}.mpk", m.package.name, m.package.version))).unwrap();
    }
    let idx = mibpkg::index::build_index(&dir, id, &format!("{id} packages")).unwrap();
    mibpkg::index::write_index(&dir, &idx).unwrap();
    dir
}
