//! Dependency-free binary delta for jar patches: ship a diff, not the whole jar.
//! Format: MAGIC | base_sha1(20) | out_len(u64 LE) | ops, where an op is
//! 0x01 COPY off(u64 LE) len(u64 LE) from `old`, or 0x02 INSERT len(u64 LE) bytes.
//! `apply` refuses a mismatched base so a patch can never land on the wrong jar.
//! The encoder is a simple common prefix/suffix split; the on-car size is finalized
//! with spike S2, and a better `encode` can replace this without touching `apply`.
use crate::sha1::sha1_hex;
use anyhow::{bail, Result};

const MAGIC: &[u8; 4] = b"MPD1";

pub fn encode(old: &[u8], new: &[u8]) -> Vec<u8> {
    let pre = old.iter().zip(new).take_while(|(a, b)| a == b).count();
    let max_suf = old.len().min(new.len()) - pre;
    let suf = (0..max_suf)
        .take_while(|i| old[old.len() - 1 - i] == new[new.len() - 1 - i])
        .count();
    let mut p = Vec::new();
    p.extend_from_slice(MAGIC);
    p.extend_from_slice(&hex20(&sha1_hex(old)));
    p.extend_from_slice(&(new.len() as u64).to_le_bytes());
    if pre > 0 {
        copy(&mut p, 0, pre);
    }
    let mid = &new[pre..new.len() - suf];
    if !mid.is_empty() {
        p.push(0x02);
        p.extend_from_slice(&(mid.len() as u64).to_le_bytes());
        p.extend_from_slice(mid);
    }
    if suf > 0 {
        copy(&mut p, (old.len() - suf) as u64, suf);
    }
    p
}

pub fn apply(old: &[u8], patch: &[u8]) -> Result<Vec<u8>> {
    let mut r = patch;
    if take(&mut r, 4)? != MAGIC {
        bail!("delta: bad magic");
    }
    let base = take(&mut r, 20)?.to_vec();
    if hex20(&sha1_hex(old)) != base[..] {
        bail!("delta: base hash mismatch (wrong jar)");
    }
    // out_len is patch-controlled: never trust it for allocation. Reject what cannot fit in
    // usize (the 32-bit armle target truncates a u64 `as usize`), and cap the reservation.
    let out_len = usize::try_from(u64_le(&mut r)?).map_err(|_| anyhow::anyhow!("delta: output length too large"))?;
    const CAP: usize = 256 * 1024 * 1024;
    let mut out = Vec::with_capacity(out_len.min(CAP));
    while !r.is_empty() {
        match r[0] {
            0x01 => {
                r = &r[1..];
                let off = usize::try_from(u64_le(&mut r)?).map_err(|_| anyhow::anyhow!("delta: copy offset too large"))?;
                let len = usize::try_from(u64_le(&mut r)?).map_err(|_| anyhow::anyhow!("delta: copy length too large"))?;
                let end = off
                    .checked_add(len)
                    .filter(|e| *e <= old.len())
                    .ok_or_else(|| anyhow::anyhow!("delta: copy out of range"))?;
                out.extend_from_slice(&old[off..end]);
            }
            0x02 => {
                r = &r[1..];
                let len = usize::try_from(u64_le(&mut r)?).map_err(|_| anyhow::anyhow!("delta: insert length too large"))?;
                out.extend_from_slice(take(&mut r, len)?);
            }
            b => bail!("delta: bad op {b:#x}"),
        }
        // Stop the moment an op pushes past the declared length: never grow unbounded.
        if out.len() > out_len {
            bail!("delta: output longer than declared length");
        }
    }
    if out.len() != out_len {
        bail!("delta: length mismatch");
    }
    Ok(out)
}

fn copy(p: &mut Vec<u8>, off: u64, len: usize) {
    p.push(0x01);
    p.extend_from_slice(&off.to_le_bytes());
    p.extend_from_slice(&(len as u64).to_le_bytes());
}

fn hex20(h: &str) -> [u8; 20] {
    let mut o = [0u8; 20];
    for i in 0..20 {
        o[i] = u8::from_str_radix(&h[i * 2..i * 2 + 2], 16).unwrap();
    }
    o
}

fn take<'a>(r: &mut &'a [u8], n: usize) -> Result<&'a [u8]> {
    if r.len() < n {
        bail!("delta: truncated");
    }
    let (a, b) = r.split_at(n);
    *r = b;
    Ok(a)
}

fn u64_le(r: &mut &[u8]) -> Result<u64> {
    let b = take(r, 8)?;
    Ok(u64::from_le_bytes(b.try_into().unwrap()))
}
