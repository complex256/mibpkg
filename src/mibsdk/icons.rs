//! Plugin and SDK icons: the PNG rules (the same as the SDK build's `PngCheck`, on the same shared
//! vectors in `tests/fixtures/icons/`), and the content-addressed install of a package's icons into
//! one directory: `<sha16>.png` per icon plus `index.properties` (`<name>=<sha16>,<w>,<h>`, sorted
//! by name, LF). `<sha16>` is the first 16 lowercase hex characters of SHA-256 of the file bytes.
use super::sdk_pkg::on_app;
use super::{put, ICONS};
use crate::archive::Package;
use crate::ctx::Ctx;
use crate::paths::Layout;
use anyhow::{anyhow, bail, Context, Result};
use flate2::{Crc, Decompress, FlushDecompress, Status};
use sha2::{Digest, Sha256};
use std::fs;
use std::io;
use std::path::Path;

/// The largest icon file accepted.
pub const MAX_BYTES: usize = 32768;
/// The largest width and height (`mibsdk.api.Ui.ROW_ICON_MAX_PX`); status icons are held to 20 at runtime.
pub const MAX_PX: u32 = 72;
/// At most this many icons per plugin, and in the SDK's built-in set.
pub const PLUGIN_MAX: usize = 8;
pub const SDK_MAX: usize = 32;
pub const INDEX: &str = "index.properties";
const FILE_MODE: u32 = 0o644;
const SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', b'\r', b'\n', 0x1a, b'\n'];

/// An icon name: `[a-z0-9-]{1,32}`.
pub fn valid_name(s: &str) -> bool {
    (1..=32).contains(&s.len()) && s.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

/// The package file an icon name needs.
pub fn file_of(name: &str) -> String {
    format!("icons/{name}.png")
}

/// Plugin `id`'s icon directory on the unit.
pub fn plugin_dir(id: &str) -> String {
    format!("{ICONS}/{id}")
}

/// Check `png` against the icon rules with width and height at most `max_px`: its (width, height),
/// or the rule it breaks.
pub fn png_check(png: &[u8], max_px: u32) -> Result<(u32, u32)> {
    if png.len() > MAX_BYTES {
        bail!("file is {} bytes, more than {MAX_BYTES}", png.len());
    }
    if png.len() < SIGNATURE.len() || png[..SIGNATURE.len()] != SIGNATURE {
        bail!("no PNG signature");
    }
    let mut header: Option<(u32, u32, usize)> = None;
    let mut idat = Vec::new();
    // IDAT chunks must be consecutive: `idat_done` once another chunk follows them.
    let (mut idat_seen, mut idat_done, mut iend) = (false, false, false);
    let mut pos = SIGNATURE.len();
    while pos < png.len() {
        if iend {
            bail!("data after IEND");
        }
        if png.len() - pos < 12 {
            bail!("truncated chunk at offset {pos}");
        }
        let len = u32_at(png, pos) as usize;
        let ty = String::from_utf8_lossy(&png[pos + 4..pos + 8]).into_owned();
        if len > png.len() - pos - 12 {
            bail!("truncated {ty} chunk at offset {pos}");
        }
        let mut crc = Crc::new();
        crc.update(&png[pos + 4..pos + 8 + len]);
        if crc.sum() != u32_at(png, pos + 8 + len) {
            bail!("bad CRC in {ty} chunk at offset {pos}");
        }
        let data = &png[pos + 8..pos + 8 + len];
        match ty.as_str() {
            _ if header.is_none() && ty != "IHDR" => bail!("first chunk is {ty}, not IHDR"),
            "IHDR" if header.is_some() => bail!("a second IHDR chunk at offset {pos}"),
            "IHDR" => header = Some(ihdr(data, max_px)?),
            "acTL" | "fcTL" | "fdAT" => bail!("animated PNG ({ty} chunk)"),
            "IDAT" if idat_done => bail!("IDAT chunks are not consecutive (another IDAT at offset {pos})"),
            "IDAT" => {
                idat_seen = true;
                idat.extend_from_slice(data);
            }
            _ => {
                idat_done = idat_seen;
                iend = ty == "IEND";
            }
        }
        pos += 12 + len;
    }
    if !iend {
        bail!("no IEND chunk");
    }
    let (w, h, channels) = header.context("no IHDR chunk")?;
    let stride = 1 + w as usize * channels;
    let rows = inflate_exactly(&idat, h as usize * stride)?;
    if let Some(r) = rows.chunks(stride).position(|row| row[0] > 4) {
        bail!("row {r} has filter type {}, must be 0..4", rows[r * stride]);
    }
    Ok((w, h))
}

/// IHDR's (width, height, channels).
fn ihdr(d: &[u8], max_px: u32) -> Result<(u32, u32, usize)> {
    if d.len() != 13 {
        bail!("IHDR is {} bytes, not 13", d.len());
    }
    let (w, h) = (u32_at(d, 0), u32_at(d, 4));
    let (depth, color, compression, filter, interlace) = (d[8], d[9], d[10], d[11], d[12]);
    if depth != 8 {
        bail!("bit depth {depth}, must be 8");
    }
    let channels = match color {
        6 => 4,
        4 => 2,
        _ => bail!("colour type {color}, must be 6 (RGBA) or 4 (grey + alpha)"),
    };
    if compression != 0 {
        bail!("compression {compression}, must be 0");
    }
    if filter != 0 {
        bail!("filter {filter}, must be 0");
    }
    if interlace != 0 {
        bail!("interlace {interlace}, must be 0");
    }
    if w < 1 || h < 1 || w > max_px || h > max_px {
        bail!("size {w}x{h}, width and height must be 1..{max_px}");
    }
    Ok((w, h, channels))
}

/// Inflate `idat`, one complete zlib stream with nothing after it, to exactly `expected` bytes.
/// The output buffer is allocated once at expected + 1 bytes and never grows, so a small file
/// that inflates to megabytes costs no more than a valid one.
fn inflate_exactly(idat: &[u8], expected: usize) -> Result<Vec<u8>> {
    let mut z = Decompress::new(true);
    let mut out = Vec::with_capacity(expected + 1);
    let ended = loop {
        let (in_before, out_before) = (z.total_in(), z.total_out());
        let status = z
            .decompress_vec(&idat[z.total_in() as usize..], &mut out, FlushDecompress::None)
            .map_err(|e| anyhow!("IDAT is not valid zlib data: {e}"))?;
        if status == Status::StreamEnd {
            break true;
        }
        if out.len() == out.capacity() || (z.total_in() == in_before && z.total_out() == out_before) {
            break false;
        }
    };
    if out.len() > expected {
        bail!("IDAT inflates to more than the expected {expected} bytes");
    }
    if !ended {
        bail!("IDAT is not a complete zlib stream ({} of {expected} bytes)", out.len());
    }
    if (z.total_in() as usize) < idat.len() {
        bail!("{} bytes of IDAT data after the end of the zlib stream", idat.len() - z.total_in() as usize);
    }
    if out.len() != expected {
        bail!("IDAT inflates to {} bytes, expected {expected}", out.len());
    }
    Ok(out)
}

fn u32_at(b: &[u8], at: usize) -> u32 {
    u32::from_be_bytes([b[at], b[at + 1], b[at + 2], b[at + 3]])
}

/// One declared icon that passed `png_check`: its bytes, their sha16 and its size.
#[derive(Debug, Clone)]
pub struct Checked {
    pub name: String,
    pub sha16: String,
    pub w: u32,
    pub h: u32,
    pub bytes: Vec<u8>,
}

/// Where a package's files are read from for `check_from`.
#[derive(Clone, Copy)]
pub enum Source<'a> {
    /// An unpacked package (its version directory).
    Dir(&'a Path),
    /// A package read into memory, not unpacked yet.
    Package(&'a Package),
}

/// `check_from` an unpacked package directory.
pub fn check_all(pkg_dir: &Path, names: &[String]) -> Result<Vec<Checked>> {
    check_from(Source::Dir(pkg_dir), names)
}

/// Read and `png_check` every declared icon (`icons/<name>.png`) and compute its sha16. Writes
/// nothing. The error is `icon <name>: <rule>`.
pub fn check_from(src: Source, names: &[String]) -> Result<Vec<Checked>> {
    let mut out = Vec::with_capacity(names.len());
    for name in names {
        let rel = file_of(name);
        let bytes = match src {
            Source::Dir(d) => match fs::read(d.join(&rel)) {
                Ok(b) => b,
                Err(e) if e.kind() == io::ErrorKind::NotFound => bail!("icon {name}: the package has no {rel}"),
                Err(e) => bail!("icon {name}: cannot read {rel} ({e})"),
            },
            Source::Package(p) => p.file(&rel).with_context(|| format!("icon {name}: the package has no {rel}"))?.to_vec(),
        };
        let (w, h) = png_check(&bytes, MAX_PX).map_err(|e| anyhow!("icon {name}: {e:#}"))?;
        out.push(Checked { name: name.clone(), sha16: sha16(&bytes), w, h, bytes });
    }
    Ok(out)
}

/// The first 16 lowercase hex characters of SHA-256 of `bytes`.
pub fn sha16(bytes: &[u8]) -> String {
    Sha256::digest(bytes)[..8].iter().map(|b| format!("{b:02x}")).collect()
}

/// `index.properties` for `checked`: `<name>=<sha16>,<w>,<h>`, sorted by name, LF.
pub fn index_text(checked: &[Checked]) -> String {
    let mut lines: Vec<String> = checked.iter().map(|c| format!("{}={},{},{}\n", c.name, c.sha16, c.w, c.h)).collect();
    lines.sort();
    lines.concat()
}

/// Install `checked` into `dir_abs`: every `<sha16>.png`, then `index.properties`, last the
/// removal of every other file in the directory. Nothing checked removes the directory.
/// The caller holds /mnt/app read-write.
pub fn install(ctx: &Ctx, dir_abs: &str, checked: &[Checked]) -> Result<()> {
    if checked.is_empty() {
        return remove(ctx, dir_abs);
    }
    let dir = ctx.layout.p(dir_abs);
    on_app(ctx, &format!("cannot create {dir_abs}"), &mut || fs::create_dir_all(&dir))?;
    let mut keep = vec![INDEX.to_string()];
    for c in checked {
        let file = format!("{}.png", c.sha16);
        put(ctx, &format!("{dir_abs}/{file}"), &c.bytes, FILE_MODE)?;
        keep.push(file);
    }
    put(ctx, &format!("{dir_abs}/{INDEX}"), index_text(checked).as_bytes(), FILE_MODE)?;
    drop_files_except(ctx, dir_abs, &keep)
}

/// Remove `dir_abs` and everything in it (absent is fine). The caller holds /mnt/app read-write.
pub fn remove(ctx: &Ctx, dir_abs: &str) -> Result<()> {
    let dir = ctx.layout.p(dir_abs);
    on_app(ctx, &format!("cannot remove {dir_abs}"), &mut || match fs::remove_dir_all(&dir) {
        Err(e) if e.kind() != io::ErrorKind::NotFound => Err(e),
        _ => Ok(()),
    })
}

/// The regular files in `dir_abs` (name, bytes), sorted by name; None when there is no directory.
pub fn snapshot(l: &Layout, dir_abs: &str) -> Result<Option<Vec<(String, Vec<u8>)>>> {
    let dir = l.p(dir_abs);
    let entries = match fs::read_dir(&dir) {
        Ok(e) => e,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e).with_context(|| format!("listing {dir_abs}")),
    };
    let mut files = Vec::new();
    for e in entries {
        let e = e.with_context(|| format!("listing {dir_abs}"))?;
        if !e.file_type().with_context(|| format!("listing {dir_abs}"))?.is_file() {
            continue;
        }
        let name = e.file_name().to_string_lossy().into_owned();
        let bytes = fs::read(e.path()).with_context(|| format!("reading {dir_abs}/{name}"))?;
        files.push((name, bytes));
    }
    files.sort();
    Ok(Some(files))
}

/// Put `dir_abs` back as `snap` (from `snapshot`) holds it. The caller holds /mnt/app read-write.
pub fn restore(ctx: &Ctx, dir_abs: &str, snap: &Option<Vec<(String, Vec<u8>)>>) -> Result<()> {
    let Some(files) = snap else { return remove(ctx, dir_abs) };
    let dir = ctx.layout.p(dir_abs);
    on_app(ctx, &format!("cannot create {dir_abs}"), &mut || fs::create_dir_all(&dir))?;
    for (name, bytes) in files {
        put(ctx, &format!("{dir_abs}/{name}"), bytes, FILE_MODE)?;
    }
    let keep: Vec<String> = files.iter().map(|(n, _)| n.clone()).collect();
    drop_files_except(ctx, dir_abs, &keep)
}

/// Remove the regular files in `dir_abs` not named in `keep`.
fn drop_files_except(ctx: &Ctx, dir_abs: &str, keep: &[String]) -> Result<()> {
    let dir = ctx.layout.p(dir_abs);
    for e in fs::read_dir(&dir).with_context(|| format!("listing {dir_abs}"))? {
        let e = e.with_context(|| format!("listing {dir_abs}"))?;
        let name = e.file_name().to_string_lossy().into_owned();
        if keep.contains(&name) || !e.file_type().with_context(|| format!("listing {dir_abs}"))?.is_file() {
            continue;
        }
        let p = e.path();
        on_app(ctx, &format!("cannot remove {dir_abs}/{name}"), &mut || match fs::remove_file(&p) {
            Err(e) if e.kind() != io::ErrorKind::NotFound => Err(e),
            _ => Ok(()),
        })?;
    }
    Ok(())
}
