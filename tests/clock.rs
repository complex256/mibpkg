mod common;
use mibpkg::clock::{ensure_offset, read_offset, sntp_offset, NTP_UNIX_DELTA};
use std::fs;
use std::net::UdpSocket;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// A one-shot SNTP server that answers with local time + `ahead` seconds.
fn fake_ntp(ahead: u64) -> String {
    let sock = UdpSocket::bind("127.0.0.1:0").unwrap();
    let addr = sock.local_addr().unwrap().to_string();
    std::thread::spawn(move || {
        let mut req = [0u8; 48];
        let (_, peer) = sock.recv_from(&mut req).unwrap();
        let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs();
        let mut resp = [0u8; 48];
        resp[0] = 0x24; // LI 0, VN 4, mode 4 (server)
        resp[40..44].copy_from_slice(&((now + ahead + NTP_UNIX_DELTA) as u32).to_be_bytes());
        sock.send_to(&resp, peer).unwrap();
    });
    addr
}

#[test]
fn sntp_offset_measures_the_difference() {
    let o = sntp_offset(&fake_ntp(1000), Duration::from_secs(2)).unwrap();
    assert!((998..=1002).contains(&o), "{o}");
}

#[test]
fn sntp_times_out_on_silence() {
    let silent = UdpSocket::bind("127.0.0.1:0").unwrap();
    let addr = silent.local_addr().unwrap().to_string();
    assert!(sntp_offset(&addr, Duration::from_millis(300)).is_err());
}

#[test]
fn only_our_offset_file_counts_and_ensure_caches() {
    let f = common::fixture();
    assert_eq!(read_offset(&f.layout), None);
    // Tailscale is an optional, separate package: its offset file is never used.
    fs::write(f.layout.p("/tmp/ts_clock_offset"), "1790638943\n").unwrap();
    assert_eq!(read_offset(&f.layout), None);
    fs::write(f.layout.run("clock_offset"), "42").unwrap();
    assert_eq!(read_offset(&f.layout), Some(42));
    // With our file present, ensure_offset does not query (the server list is unreachable).
    assert_eq!(ensure_offset(&f.layout, &["127.0.0.1:9"], Duration::from_millis(200)).unwrap(), 42);
    fs::remove_file(f.layout.run("clock_offset")).unwrap();
    let o = ensure_offset(&f.layout, &["127.0.0.1:9", &fake_ntp(500)], Duration::from_millis(500)).unwrap();
    assert!((498..=502).contains(&o), "falls through to the second server: {o}");
    assert_eq!(read_offset(&f.layout), Some(o), "cached for later calls");
}
