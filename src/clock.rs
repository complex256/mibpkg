//! Network time for TLS certificate checks. The unit's clock reads 1970 (nothing sets it), so
//! mibpkg keeps an offset (network - local seconds) and applies it only to certificate checks.
//! It always measures the offset itself: other packages (Tailscale) are optional.
use crate::fsutil::atomic_write;
use crate::paths::Layout;
use crate::status::now_secs;
use anyhow::{bail, Context, Result};
use std::net::UdpSocket;
use std::time::Duration;

/// Seconds from 1900-01-01 (NTP epoch) to 1970-01-01.
pub const NTP_UNIX_DELTA: u64 = 2_208_988_800;
pub const SERVERS: &[&str] = &["time.cloudflare.com:123", "pool.ntp.org:123"];

/// One SNTP (RFC 4330) query: returns network time minus local time, in seconds.
pub fn sntp_offset(server: &str, timeout: Duration) -> Result<i64> {
    let sock = UdpSocket::bind("0.0.0.0:0").context("udp bind")?;
    sock.set_read_timeout(Some(timeout))?;
    sock.connect(server).with_context(|| format!("sntp {server}"))?;
    let mut req = [0u8; 48];
    req[0] = 0x23; // LI 0, VN 4, mode 3 (client)
    sock.send(&req).with_context(|| format!("sntp {server}"))?;
    let mut resp = [0u8; 48];
    let n = sock.recv(&mut resp).with_context(|| format!("sntp {server}: no answer"))?;
    if n < 48 {
        bail!("sntp {server}: short answer");
    }
    let secs = u32::from_be_bytes([resp[40], resp[41], resp[42], resp[43]]) as u64;
    if secs <= NTP_UNIX_DELTA {
        bail!("sntp {server}: invalid time");
    }
    Ok((secs - NTP_UNIX_DELTA) as i64 - now_secs() as i64)
}

/// The offset this mibpkg measured earlier this boot (never another package's file).
pub fn read_offset(layout: &Layout) -> Option<i64> {
    std::fs::read_to_string(layout.run("clock_offset")).ok()?.trim().parse().ok()
}

/// The offset, querying `servers` in order (and caching the answer) when none is known yet.
pub fn ensure_offset(layout: &Layout, servers: &[&str], timeout: Duration) -> Result<i64> {
    if let Some(o) = read_offset(layout) {
        return Ok(o);
    }
    let mut last = None;
    for s in servers {
        match sntp_offset(s, timeout) {
            Ok(o) => {
                atomic_write(&layout.run("clock_offset"), o.to_string().as_bytes())?;
                return Ok(o);
            }
            Err(e) => last = Some(e),
        }
    }
    Err(last.unwrap_or_else(|| anyhow::anyhow!("no time servers configured")))
}
