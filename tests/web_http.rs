//! The hand-rolled HTTP/1.1 subset: limits first, then parsing.
use mibpkg::web::http::{read_request, write_response, ReadError, Request, Response};
use std::io::{BufReader, Cursor, Read};

/// Body limit used by most tests: 64 KB, uploads 1 MB.
fn limit(method: &str, path: &str) -> u64 {
    if method == "POST" && path == "/api/uploads" {
        1 << 20
    } else {
        64 * 1024
    }
}

fn parse(raw: &[u8]) -> (Result<Request, ReadError>, Vec<u8>) {
    let mut r = BufReader::new(Cursor::new(raw.to_vec()));
    let mut w = Vec::new();
    let got = read_request(&mut r, &mut w, limit);
    (got, w)
}

#[test]
fn a_simple_get_with_query_and_headers() {
    let (r, _) = parse(b"GET /api/logs/mibpkg%2Eweb?bytes=100&x=a%20b+c HTTP/1.1\r\nHost: 10.255.1.1\r\nX-Mibpkg: 1\r\n\r\n");
    let r = r.unwrap();
    assert_eq!(r.method, "GET");
    assert_eq!(r.path, "/api/logs/mibpkg.web");
    assert_eq!(r.query("bytes"), Some("100"));
    assert_eq!(r.query("x"), Some("a b c"));
    assert_eq!(r.header("host"), Some("10.255.1.1"));
    assert_eq!(r.header("x-mibpkg"), Some("1"));
    assert!(r.body.is_empty());
}

#[test]
fn a_post_body_is_read_by_content_length() {
    let (r, _) = parse(b"POST /api/actions HTTP/1.1\r\nContent-Length: 18\r\n\r\n{\"action\":\"check\"}trailing");
    assert_eq!(r.unwrap().body, b"{\"action\":\"check\"}");
}

#[test]
fn oversized_headers_and_bodies_are_refused_before_reading() {
    let mut big = b"GET / HTTP/1.1\r\nX-Pad: ".to_vec();
    big.extend(std::iter::repeat(b'a').take(17 * 1024));
    big.extend(b"\r\n\r\n");
    assert!(matches!(parse(&big).0, Err(ReadError::TooLarge)));
    // A body over the limit is refused from the header alone (nothing follows it here).
    let (r, _) = parse(b"POST /api/actions HTTP/1.1\r\nContent-Length: 70000\r\n\r\n");
    assert!(matches!(r, Err(ReadError::TooLarge)));
    let (r, _) = parse(b"POST /api/uploads HTTP/1.1\r\nContent-Length: 2000000\r\n\r\n");
    assert!(matches!(r, Err(ReadError::TooLarge)));
}

#[test]
fn bodies_need_a_content_length_and_chunked_is_refused() {
    assert!(matches!(parse(b"POST /api/actions HTTP/1.1\r\n\r\n").0, Err(ReadError::LengthRequired)));
    let (r, _) = parse(b"POST /api/actions HTTP/1.1\r\nTransfer-Encoding: chunked\r\n\r\n5\r\nhello\r\n0\r\n\r\n");
    assert!(matches!(r, Err(ReadError::BadRequest(_))));
    // A body shorter than promised is an error, not a hang or a truncated install.
    assert!(parse(b"POST /api/actions HTTP/1.1\r\nContent-Length: 50\r\n\r\nshort").0.is_err());
}

#[test]
fn expect_100_continue_is_answered_before_the_body() {
    let (r, w) = parse(b"POST /api/uploads HTTP/1.1\r\nExpect: 100-continue\r\nContent-Length: 4\r\n\r\nabcd");
    assert_eq!(r.unwrap().body, b"abcd");
    assert_eq!(w, b"HTTP/1.1 100 Continue\r\n\r\n");
}

#[test]
fn junk_is_a_bad_request() {
    for junk in [&b"\x16\x03\x01\x02\x00\x01\x00\x01\xfc\x03\x03"[..], b"HELLO\r\n\r\n", b"GET\r\n\r\n", b"GET nopath HTTP/1.1\r\n\r\n", b"GET /%zz HTTP/1.1\r\n\r\n"] {
        assert!(matches!(parse(junk).0, Err(ReadError::BadRequest(_)) | Err(ReadError::Io(_))), "{junk:?}");
    }
}

#[test]
fn responses_are_complete_and_gzip_when_asked() {
    let big = "x".repeat(4000);
    let mut out = Vec::new();
    write_response(&mut out, Response::text(200, "text/plain; charset=utf-8", big.clone()), true).unwrap();
    let text = String::from_utf8_lossy(&out);
    assert!(text.starts_with("HTTP/1.1 200 OK\r\n"), "{text}");
    assert!(text.contains("Content-Encoding: gzip\r\n"));
    assert!(text.contains("Connection: close\r\n"));
    let split = out.windows(4).position(|w| w == b"\r\n\r\n").unwrap() + 4;
    let mut plain = String::new();
    flate2::read::GzDecoder::new(&out[split..]).read_to_string(&mut plain).unwrap();
    assert_eq!(plain, big);

    let mut out = Vec::new();
    write_response(&mut out, Response::error(409, "busy"), false).unwrap();
    let text = String::from_utf8(out).unwrap();
    assert!(text.starts_with("HTTP/1.1 409 Conflict\r\n"));
    assert!(text.ends_with("{\"error\":\"busy\"}"), "{text}");
    assert!(text.contains("Content-Length: 16\r\n"));
}
