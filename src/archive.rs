//! .mpk = gzip'd ustar holding manifest.toml + payload. Hand-rolled (regular
//! files and directories only) so no platform crates are needed on QNX 6.5.
use crate::manifest::{parse_mode, safe_rel, Manifest};
use anyhow::{bail, Context, Result};
use flate2::{read::GzDecoder, write::GzEncoder, Compression};
use sha2::{Digest, Sha256};
use std::fs;
use std::io::{Read, Write};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

const BLOCK: usize = 512;

struct Entry {
    path: PathBuf,
    mode: u32,
    dir: bool,
    data: Vec<u8>,
}

pub struct Package {
    pub manifest: Manifest,
    entries: Vec<Entry>,
}

/// Build `out` from `src_dir` (which must hold manifest.toml). Dotfiles are skipped.
pub fn pack(src_dir: &Path, out: &Path) -> Result<Manifest> {
    let text = fs::read_to_string(src_dir.join("manifest.toml"))
        .with_context(|| format!("reading {}/manifest.toml", src_dir.display()))?;
    let m = Manifest::parse(&text)?;
    let mut needed: Vec<&str> = m.files.iter().map(|f| f.src.as_str()).collect();
    needed.extend(m.configs.iter().filter_map(|c| c.example.as_deref()));
    needed.extend([&m.scripts.post_install, &m.scripts.pre_remove].into_iter().flatten().map(|s| s.as_str()));
    for n in needed {
        if !src_dir.join(n).is_file() {
            bail!("{n} is named in manifest.toml but missing from {}", src_dir.display());
        }
    }
    let mut rels = Vec::new();
    collect(src_dir, Path::new(""), &mut rels)?;
    rels.sort();
    let mut gz = GzEncoder::new(
        fs::File::create(out).with_context(|| format!("creating {}", out.display()))?,
        Compression::best(),
    );
    for rel in &rels {
        let full = src_dir.join(rel);
        let meta = fs::metadata(&full)?;
        let name = rel.to_str().context("non-UTF-8 path")?;
        let mode = meta.permissions().mode() & 0o777;
        if meta.is_dir() {
            write_header(&mut gz, &format!("{name}/"), mode, 0, b'5')?;
        } else {
            let data = fs::read(&full)?;
            write_header(&mut gz, name, mode, data.len() as u64, b'0')?;
            gz.write_all(&data)?;
            pad(&mut gz, data.len())?;
        }
    }
    gz.write_all(&[0u8; BLOCK * 2])?;
    gz.finish()?;
    Ok(m)
}

fn collect(base: &Path, rel: &Path, out: &mut Vec<PathBuf>) -> Result<()> {
    for e in fs::read_dir(base.join(rel))? {
        let e = e?;
        if e.file_name().to_string_lossy().starts_with('.') {
            continue;
        }
        let r = rel.join(e.file_name());
        let ft = e.file_type()?;
        if ft.is_symlink() {
            bail!("symlinks are not allowed in packages: {}", r.display());
        }
        out.push(r.clone());
        if ft.is_dir() {
            collect(base, &r, out)?;
        }
    }
    Ok(())
}

fn write_header(w: &mut impl Write, name: &str, mode: u32, size: u64, kind: u8) -> Result<()> {
    if name.len() > 100 {
        bail!("path too long for a package (max 100 bytes): {name}");
    }
    let mut h = [0u8; BLOCK];
    h[..name.len()].copy_from_slice(name.as_bytes());
    octal(&mut h[100..108], mode as u64);
    octal(&mut h[108..116], 0);
    octal(&mut h[116..124], 0);
    octal(&mut h[124..136], size);
    octal(&mut h[136..148], 0);
    h[156] = kind;
    h[257..263].copy_from_slice(b"ustar\0");
    h[263..265].copy_from_slice(b"00");
    h[148..156].copy_from_slice(b"        ");
    let sum: u32 = h.iter().map(|b| *b as u32).sum();
    h[148..156].copy_from_slice(format!("{sum:06o}\0 ").as_bytes());
    w.write_all(&h)?;
    Ok(())
}

fn octal(field: &mut [u8], v: u64) {
    let n = field.len() - 1;
    let s = format!("{:0width$o}", v, width = n);
    field[..n].copy_from_slice(s.as_bytes());
    field[n] = 0;
}

fn pad(w: &mut impl Write, len: usize) -> Result<()> {
    let r = len % BLOCK;
    if r != 0 {
        w.write_all(&vec![0u8; BLOCK - r])?;
    }
    Ok(())
}

fn parse_octal(f: &[u8]) -> Result<u64> {
    let s = std::str::from_utf8(f)?.trim_matches(|c| c == '\0' || c == ' ');
    if s.is_empty() {
        return Ok(0);
    }
    u64::from_str_radix(s, 8).with_context(|| format!("bad octal field {s:?}"))
}

fn cstr(f: &[u8]) -> Result<String> {
    let end = f.iter().position(|b| *b == 0).unwrap_or(f.len());
    Ok(std::str::from_utf8(&f[..end])?.to_string())
}

fn read_entries(mpk: &Path) -> Result<Vec<Entry>> {
    let file = fs::File::open(mpk).with_context(|| format!("opening {}", mpk.display()))?;
    let mut r = GzDecoder::new(file);
    let mut out = Vec::new();
    let mut h = [0u8; BLOCK];
    loop {
        r.read_exact(&mut h).context("truncated package")?;
        if h.iter().all(|b| *b == 0) {
            break;
        }
        let stored = parse_octal(&h[148..156])? as u32;
        let mut hc = h;
        hc[148..156].copy_from_slice(b"        ");
        if hc.iter().map(|b| *b as u32).sum::<u32>() != stored {
            bail!("corrupt package header");
        }
        let name = cstr(&h[0..100])?;
        let prefix = cstr(&h[345..500])?;
        let full = if prefix.is_empty() { name } else { format!("{prefix}/{name}") };
        let path = safe_rel(full.trim_end_matches('/'))?;
        let mode = (parse_octal(&h[100..108])? as u32) & 0o777;
        let size = parse_octal(&h[124..136])? as usize;
        match h[156] {
            b'0' | 0 => {
                let mut data = vec![0u8; size];
                r.read_exact(&mut data).context("truncated package")?;
                let rem = size % BLOCK;
                if rem != 0 {
                    let mut p = vec![0u8; BLOCK - rem];
                    r.read_exact(&mut p).context("truncated package")?;
                }
                out.push(Entry { path, mode, dir: false, data });
            }
            b'5' => out.push(Entry { path, mode, dir: true, data: Vec::new() }),
            k => bail!("unsupported entry type {:?} for {full} (only files and directories)", k as char),
        }
    }
    Ok(out)
}

impl Package {
    pub fn read(mpk: &Path) -> Result<Package> {
        let entries = read_entries(mpk)?;
        let m = entries
            .iter()
            .find(|e| !e.dir && e.path == Path::new("manifest.toml"))
            .context("package has no manifest.toml")?;
        let manifest = Manifest::parse(std::str::from_utf8(&m.data).context("manifest.toml is not UTF-8")?)?;
        for f in &manifest.files {
            if !entries.iter().any(|e| !e.dir && e.path == Path::new(&f.src)) {
                bail!("package is missing {} named in its manifest", f.src);
            }
        }
        Ok(Package { manifest, entries })
    }

    pub fn unpacked_size(&self) -> u64 {
        self.entries.iter().map(|e| e.data.len() as u64).sum()
    }

    pub fn extract_to(&self, dest: &Path) -> Result<()> {
        if dest.exists() {
            bail!("{} already exists", dest.display());
        }
        fs::create_dir_all(dest)?;
        for e in &self.entries {
            let p = dest.join(&e.path);
            if e.dir {
                fs::create_dir_all(&p)?;
                fs::set_permissions(&p, fs::Permissions::from_mode(e.mode | 0o700))?;
            } else {
                if let Some(d) = p.parent() {
                    fs::create_dir_all(d)?;
                }
                fs::write(&p, &e.data)?;
                fs::set_permissions(&p, fs::Permissions::from_mode(e.mode))?;
            }
        }
        // Manifest modes win over archive modes for listed files.
        for f in &self.manifest.files {
            fs::set_permissions(dest.join(&f.src), fs::Permissions::from_mode(parse_mode(&f.mode)?))?;
        }
        Ok(())
    }
}

pub fn sha256_file(path: &Path) -> Result<(String, u64)> {
    let mut f = fs::File::open(path).with_context(|| format!("opening {}", path.display()))?;
    let mut h = Sha256::new();
    let mut buf = vec![0u8; 64 * 1024];
    let mut n = 0u64;
    loop {
        let k = f.read(&mut buf)?;
        if k == 0 {
            break;
        }
        h.update(&buf[..k]);
        n += k as u64;
    }
    Ok((h.finalize().iter().map(|b| format!("{b:02x}")).collect(), n))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    const M: &str = "[package]\nname = \"demo\"\nversion = \"1.0.0\"\n\n[[file]]\nsrc = \"bin/demo\"\nmode = \"755\"\n\n[[service]]\nname = \"demo\"\nexec = \"@pkg/bin/demo\"\n";

    fn src_dir(t: &Path) -> PathBuf {
        let s = t.join("src");
        fs::create_dir_all(s.join("bin")).unwrap();
        fs::write(s.join("manifest.toml"), M).unwrap();
        fs::write(s.join("bin/demo"), b"#!/bin/sh\necho hi\n").unwrap();
        fs::set_permissions(s.join("bin/demo"), fs::Permissions::from_mode(0o755)).unwrap();
        fs::write(s.join(".DS_Store"), b"junk").unwrap();
        s
    }

    #[test]
    fn pack_read_extract_roundtrip() {
        let t = tempfile::tempdir().unwrap();
        let s = src_dir(t.path());
        let out = t.path().join("demo-1.0.0.mpk");
        let m = pack(&s, &out).unwrap();
        assert_eq!(m.package.name, "demo");
        let p = Package::read(&out).unwrap();
        assert_eq!(p.manifest.package.version, "1.0.0");
        assert!(p.unpacked_size() > 0);
        let dest = t.path().join("x/1.0.0");
        p.extract_to(&dest).unwrap();
        assert_eq!(fs::read(dest.join("bin/demo")).unwrap(), b"#!/bin/sh\necho hi\n");
        assert_eq!(fs::metadata(dest.join("bin/demo")).unwrap().permissions().mode() & 0o777, 0o755);
        assert!(!dest.join(".DS_Store").exists(), "dotfiles are not packed");
        assert!(p.extract_to(&dest).is_err(), "refuses an existing destination");
    }

    #[test]
    fn pack_rejects_missing_file_and_symlink() {
        let t = tempfile::tempdir().unwrap();
        let s = src_dir(t.path());
        fs::remove_file(s.join("bin/demo")).unwrap();
        assert!(pack(&s, &t.path().join("a.mpk")).is_err());
        let s2 = src_dir(&t.path().join("two"));
        std::os::unix::fs::symlink("/etc/passwd", s2.join("bin/link")).unwrap();
        assert!(pack(&s2, &t.path().join("b.mpk")).is_err());
    }

    fn raw_archive(path: &Path, entries: &[(&str, u8, &[u8])]) {
        let mut gz = GzEncoder::new(fs::File::create(path).unwrap(), Compression::fast());
        for (name, kind, data) in entries {
            write_header(&mut gz, name, 0o644, data.len() as u64, *kind).unwrap();
            gz.write_all(data).unwrap();
            pad(&mut gz, data.len()).unwrap();
        }
        gz.write_all(&[0u8; BLOCK * 2]).unwrap();
        gz.finish().unwrap();
    }

    #[test]
    fn read_rejects_traversal_links_and_missing_manifest() {
        let t = tempfile::tempdir().unwrap();
        let a = t.path().join("a.mpk");
        raw_archive(&a, &[("manifest.toml", b'0', M.as_bytes()), ("../evil", b'0', b"x")]);
        assert!(Package::read(&a).is_err());
        raw_archive(&a, &[("manifest.toml", b'0', M.as_bytes()), ("bin/l", b'2', b"")]);
        assert!(Package::read(&a).is_err());
        raw_archive(&a, &[("bin/demo", b'0', b"x")]);
        assert!(Package::read(&a).is_err());
    }

    #[test]
    fn read_rejects_truncated_archive() {
        let t = tempfile::tempdir().unwrap();
        let s = src_dir(t.path());
        let out = t.path().join("demo.mpk");
        pack(&s, &out).unwrap();
        let bytes = fs::read(&out).unwrap();
        fs::write(&out, &bytes[..bytes.len() / 2]).unwrap();
        assert!(Package::read(&out).is_err());
    }

    #[test]
    fn sha256_of_known_input() {
        let t = tempfile::tempdir().unwrap();
        let f = t.path().join("f");
        fs::write(&f, b"abc").unwrap();
        let (h, n) = sha256_file(&f).unwrap();
        assert_eq!(h, "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
        assert_eq!(n, 3);
    }
}
