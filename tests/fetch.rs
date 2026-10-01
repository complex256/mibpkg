use mibpkg::fetch::{CurlFetcher, Fetcher, MemoryFetcher};
use std::io::{Read, Write};
use std::net::TcpListener;

/// Serves each connection one canned response; returns the base URL.
fn server(status: &'static str, body: Vec<u8>) -> String {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/", l.local_addr().unwrap());
    std::thread::spawn(move || {
        for s in l.incoming() {
            let mut s = s.unwrap();
            let mut buf = [0u8; 2048];
            let _ = s.read(&mut buf);
            let head = format!("HTTP/1.0 {status}\r\nContent-Length: {}\r\n\r\n", body.len());
            let _ = s.write_all(head.as_bytes());
            let _ = s.write_all(&body);
        }
    });
    url
}

fn host_curl() -> CurlFetcher {
    CurlFetcher {
        lib: "/usr/lib/libcurl.4.dylib".into(),
        preload: vec![],
        ca_bundle: None,
        clock_offset: None,
        timeout_secs: 5,
        allow_http: true,
    }
}

#[test]
#[cfg(target_os = "macos")]
fn curl_fetches_bodies_and_reports_http_errors() {
    let ok = server("200 OK", b"hello index".to_vec());
    assert_eq!(host_curl().get(&format!("{ok}index.json"), 1024).unwrap(), b"hello index");
    let missing = server("404 Not Found", b"nope".to_vec());
    let e = host_curl().get(&format!("{missing}x"), 1024).unwrap_err();
    assert!(format!("{e:#}").contains("404"), "{e:#}");
    let big = server("200 OK", vec![b'x'; 5000]);
    let e = host_curl().get(&big, 1000).unwrap_err();
    assert!(format!("{e:#}").contains("larger than"), "{e:#}");
}

#[test]
fn only_https_unless_allowed() {
    let mut c = host_curl();
    c.allow_http = false;
    let e = c.get("http://example.com/index.json", 10).unwrap_err();
    assert!(format!("{e:#}").contains("https"), "{e:#}");
    assert!(c.get("ftp://example.com/x", 10).is_err());
}

#[test]
fn memory_fetcher_serves_and_records() {
    let m = MemoryFetcher::default();
    m.files.borrow_mut().insert("https://h/a".into(), b"A".to_vec());
    assert_eq!(m.get("https://h/a", 10).unwrap(), b"A");
    assert!(format!("{:#}", m.get("https://h/b", 10).unwrap_err()).contains("404"));
    assert!(m.get("https://h/a", 0).is_err(), "size cap applies");
    assert_eq!(*m.calls.borrow(), vec!["https://h/a", "https://h/b", "https://h/a"]);
}
