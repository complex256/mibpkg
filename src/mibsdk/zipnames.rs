//! The entry names of a zip (a jar), from its central directory, with std only: the unit has no
//! `unzip`, and mibpkg takes no zip crate. Used by the SDK's owned-classes guard, which must fail
//! closed, so anything this reader does not understand (zip64, a truncated or inconsistent
//! directory) is an error, never an empty list.
use anyhow::{bail, Context, Result};
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

const EOCD_SIG: u32 = 0x0605_4b50;
const CDH_SIG: u32 = 0x0201_4b50;
const EOCD_LEN: usize = 22;
const CDH_LEN: usize = 46;
/// The end record sits within the last 64 KiB (the longest comment) + 22 bytes.
const TAIL: u64 = 0xffff + EOCD_LEN as u64;

fn u16_at(b: &[u8], i: usize) -> usize {
    usize::from(u16::from_le_bytes([b[i], b[i + 1]]))
}

fn u32_at(b: &[u8], i: usize) -> u32 {
    u32::from_le_bytes([b[i], b[i + 1], b[i + 2], b[i + 3]])
}

/// Every central-directory entry name of the zip at `path`, in directory order.
pub fn list(path: &Path) -> Result<Vec<String>> {
    let mut f = File::open(path).with_context(|| format!("opening {}", path.display()))?;
    let len = f.metadata()?.len();
    let tail_len = len.min(TAIL);
    let tail_start = len - tail_len;
    f.seek(SeekFrom::Start(tail_start))?;
    let mut tail = vec![0u8; tail_len as usize];
    f.read_exact(&mut tail)?;
    // The last end record whose comment fits in the file.
    let eocd = (0..tail.len().saturating_sub(EOCD_LEN - 1))
        .rev()
        .find(|&i| u32_at(&tail, i) == EOCD_SIG && i + EOCD_LEN + u16_at(&tail, i + 20) <= tail.len())
        .context("no end of central directory record (not a zip)")?;
    let e = &tail[eocd..];
    let (disk, cd_disk) = (u16_at(e, 4), u16_at(e, 6));
    let (on_disk, total) = (u16_at(e, 8), u16_at(e, 10));
    let (cd_size, cd_off) = (u64::from(u32_at(e, 12)), u64::from(u32_at(e, 16)));
    if disk != 0 || cd_disk != 0 || on_disk != total {
        bail!("multi-disk zip");
    }
    if total == 0xffff || cd_size == 0xffff_ffff || cd_off == 0xffff_ffff {
        bail!("zip64 is not supported");
    }
    let eocd_pos = tail_start + eocd as u64;
    if cd_off + cd_size > eocd_pos {
        bail!("the central directory lies outside the file");
    }
    f.seek(SeekFrom::Start(cd_off))?;
    let mut cd = vec![0u8; cd_size as usize];
    f.read_exact(&mut cd)?;
    let mut names = Vec::with_capacity(total);
    let mut i = 0;
    for _ in 0..total {
        if i + CDH_LEN > cd.len() || u32_at(&cd, i) != CDH_SIG {
            bail!("bad central directory entry {}", names.len());
        }
        let n = u16_at(&cd, i + 28);
        let end = i + CDH_LEN + n + u16_at(&cd, i + 30) + u16_at(&cd, i + 32);
        if end > cd.len() {
            bail!("central directory entry {} runs past the directory", names.len());
        }
        names.push(String::from_utf8_lossy(&cd[i + CDH_LEN..i + CDH_LEN + n]).into_owned());
        i = end;
    }
    if i != cd.len() {
        bail!("the central directory holds more than {total} entries");
    }
    Ok(names)
}
