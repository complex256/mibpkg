//! The small part of HTTP/1.1 the web interface needs: one request per connection, bodies by
//! Content-Length only, limits checked before anything is read into memory.
use serde::Serialize;
use std::io::{self, BufRead, Read, Write};

/// Request line plus headers.
pub const MAX_HEAD: u64 = 16 * 1024;

#[derive(Debug)]
pub struct Request {
    pub method: String,
    /// Percent-decoded path, without the query string.
    pub path: String,
    pub query: Vec<(String, String)>,
    /// Header names are lowercased.
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl Request {
    pub fn header(&self, name: &str) -> Option<&str> {
        let name = name.to_ascii_lowercase();
        self.headers.iter().find(|(k, _)| *k == name).map(|(_, v)| v.as_str())
    }

    pub fn query(&self, key: &str) -> Option<&str> {
        self.query.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_str())
    }
}

#[derive(Debug)]
pub enum ReadError {
    /// Head or body larger than allowed (413).
    TooLarge,
    /// POST/PUT without Content-Length (411).
    LengthRequired,
    /// Not HTTP we understand (400).
    BadRequest(String),
    /// The connection failed or timed out (no response is possible).
    Io(io::Error),
}

impl From<io::Error> for ReadError {
    fn from(e: io::Error) -> Self {
        ReadError::Io(e)
    }
}

fn bad(why: &str) -> ReadError {
    ReadError::BadRequest(why.to_string())
}

/// Decode %XX escapes (and '+' as a space when `plus` is set). Invalid escapes or invalid
/// UTF-8 are errors, never silently passed through.
fn percent_decode(s: &str, plus: bool) -> Result<String, ReadError> {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'%' => {
                let hex = bytes.get(i + 1..i + 3).ok_or_else(|| bad("bad % escape"))?;
                let hex = std::str::from_utf8(hex).map_err(|_| bad("bad % escape"))?;
                out.push(u8::from_str_radix(hex, 16).map_err(|_| bad("bad % escape"))?);
                i += 3;
            }
            b'+' if plus => {
                out.push(b' ');
                i += 1;
            }
            b => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8(out).map_err(|_| bad("not UTF-8"))
}

/// Read one line (up to '\n') without letting the head grow past `MAX_HEAD`.
fn read_head_line(r: &mut impl BufRead, used: &mut u64) -> Result<String, ReadError> {
    let mut line = Vec::new();
    let left = MAX_HEAD.saturating_sub(*used);
    let n = r.by_ref().take(left + 1).read_until(b'\n', &mut line)?;
    *used += n as u64;
    if *used > MAX_HEAD {
        return Err(ReadError::TooLarge);
    }
    if n == 0 || !line.ends_with(b"\n") {
        return Err(bad("connection closed in the middle of the request"));
    }
    let text = String::from_utf8(line).map_err(|_| bad("request head is not UTF-8"))?;
    Ok(text.trim_end_matches(['\r', '\n']).to_string())
}

/// Read one request. `w` is the same connection, used only to answer `Expect: 100-continue`.
/// `body_limit(method, path)` is the largest body allowed for that request.
pub fn read_request(r: &mut impl BufRead, w: &mut impl Write, body_limit: impl Fn(&str, &str) -> u64) -> Result<Request, ReadError> {
    let (mut req, length) = read_head(r, |m, p| body_limit(m, p))?;
    read_body(r, w, &mut req, length)?;
    Ok(req)
}

/// Read the request line and headers only, and the body length they announce (already checked
/// against `body_limit`). The body is left unread, so a request can be refused (wrong host,
/// no PIN, …) before a single body byte is read.
pub fn read_head(r: &mut impl BufRead, body_limit: impl Fn(&str, &str) -> u64) -> Result<(Request, u64), ReadError> {
    let mut used = 0u64;

    // Request line: METHOD TARGET HTTP/1.x
    let line = read_head_line(r, &mut used)?;
    let mut parts = line.split(' ');
    let (method, target, version) = match (parts.next(), parts.next(), parts.next(), parts.next()) {
        (Some(m), Some(t), Some(v), None) => (m, t, v),
        _ => return Err(bad("malformed request line")),
    };
    if !version.starts_with("HTTP/1.") || method.is_empty() || !method.bytes().all(|b| b.is_ascii_uppercase()) {
        return Err(bad("not an HTTP/1.x request"));
    }
    if !target.starts_with('/') {
        return Err(bad("the request target must be a path"));
    }

    // Path and query.
    let (raw_path, raw_query) = target.split_once('?').unwrap_or((target, ""));
    let path = percent_decode(raw_path, false)?;
    let mut query = Vec::new();
    for pair in raw_query.split('&').filter(|p| !p.is_empty()) {
        let (k, v) = pair.split_once('=').unwrap_or((pair, ""));
        query.push((percent_decode(k, true)?, percent_decode(v, true)?));
    }

    // Headers until the blank line.
    let mut headers = Vec::new();
    loop {
        let line = read_head_line(r, &mut used)?;
        if line.is_empty() {
            break;
        }
        let (k, v) = line.split_once(':').ok_or_else(|| bad("malformed header"))?;
        headers.push((k.trim().to_ascii_lowercase(), v.trim().to_string()));
    }
    let req = Request { method: method.to_string(), path, query, headers, body: Vec::new() };

    // Body: Content-Length only.
    if req.header("transfer-encoding").is_some() {
        return Err(bad("chunked request bodies are not supported"));
    }
    let length = match req.header("content-length") {
        Some(v) => Some(v.parse::<u64>().map_err(|_| bad("bad Content-Length"))?),
        None => None,
    };
    let length = match length {
        Some(n) => n,
        None if req.method == "POST" || req.method == "PUT" => return Err(ReadError::LengthRequired),
        None => 0,
    };
    if length > body_limit(&req.method, &req.path) {
        return Err(ReadError::TooLarge);
    }
    Ok((req, length))
}

/// Answer `Expect: 100-continue` (some clients wait for it before sending the body).
pub fn send_continue(req: &Request, w: &mut impl Write) -> io::Result<()> {
    if req.header("expect").map_or(false, |e| e.eq_ignore_ascii_case("100-continue")) {
        w.write_all(b"HTTP/1.1 100 Continue\r\n\r\n")?;
        w.flush()?;
    }
    Ok(())
}

/// Read a body of `length` bytes into memory (small bodies; uploads are streamed to a file).
pub fn read_body(r: &mut impl BufRead, w: &mut impl Write, req: &mut Request, length: u64) -> Result<(), ReadError> {
    if length > 0 {
        send_continue(req, w)?;
        let mut body = vec![0u8; length as usize];
        r.read_exact(&mut body)?;
        req.body = body;
    }
    Ok(())
}

pub struct Response {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl Response {
    pub fn new(status: u16, content_type: &str, body: Vec<u8>) -> Response {
        Response { status, headers: vec![("Content-Type".into(), content_type.into())], body }
    }

    pub fn text(status: u16, content_type: &str, body: impl Into<String>) -> Response {
        Response::new(status, content_type, body.into().into_bytes())
    }

    pub fn json(status: u16, value: &impl Serialize) -> Response {
        let body = serde_json::to_vec(value).unwrap_or_else(|_| b"{}".to_vec());
        Response::new(status, "application/json", body)
    }

    /// `{"error": "<plain words>"}`
    pub fn error(status: u16, message: impl Into<String>) -> Response {
        Response::json(status, &serde_json::json!({ "error": message.into() }))
    }

    pub fn with_header(mut self, name: &str, value: impl Into<String>) -> Response {
        self.headers.push((name.to_string(), value.into()));
        self
    }
}

fn reason(status: u16) -> &'static str {
    match status {
        100 => "Continue",
        200 => "OK",
        202 => "Accepted",
        204 => "No Content",
        304 => "Not Modified",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        409 => "Conflict",
        411 => "Length Required",
        413 => "Payload Too Large",
        429 => "Too Many Requests",
        500 => "Internal Server Error",
        503 => "Service Unavailable",
        _ => "Status",
    }
}

/// Text-like bodies of at least this many bytes are compressed when the browser accepts gzip.
const GZIP_MIN: usize = 512;

fn compressible(r: &Response) -> bool {
    let ct = r.headers.iter().find(|(k, _)| k.eq_ignore_ascii_case("content-type")).map(|(_, v)| v.as_str()).unwrap_or("");
    r.body.len() >= GZIP_MIN && (ct.starts_with("text/") || ct.contains("json") || ct.contains("javascript"))
}

/// Write the whole response; every connection closes afterwards.
pub fn write_response(w: &mut impl Write, mut r: Response, gzip_ok: bool) -> io::Result<()> {
    if gzip_ok && compressible(&r) {
        let mut enc = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        enc.write_all(&r.body)?;
        r.body = enc.finish()?;
        r.headers.push(("Content-Encoding".into(), "gzip".into()));
        r.headers.push(("Vary".into(), "Accept-Encoding".into()));
    }
    let mut head = format!("HTTP/1.1 {} {}\r\n", r.status, reason(r.status));
    for (k, v) in &r.headers {
        head.push_str(&format!("{k}: {v}\r\n"));
    }
    head.push_str(&format!("Content-Length: {}\r\nConnection: close\r\n\r\n", r.body.len()));
    w.write_all(head.as_bytes())?;
    w.write_all(&r.body)?;
    w.flush()
}
