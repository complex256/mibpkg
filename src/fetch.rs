//! HTTPS downloads through the unit's own libcurl (7.56 + OpenSSL 1.0.2j), loaded with dlopen:
//! no Rust TLS stack builds for QNX 6.5 (spikes/README.md). Certificates are checked against our
//! CA bundle at network time (the unit clock is 1970) via CURLOPT_SSL_CTX_FUNCTION.
use crate::paths::Layout;
use anyhow::{bail, Context, Result};
use std::cell::RefCell;
use std::collections::HashMap;
use std::ffi::{c_char, c_int, c_long, c_void, CStr, CString};
use std::path::PathBuf;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::OnceLock;

pub const MAX_INDEX: u64 = 1 << 20;
pub const MAX_PACKAGE: u64 = 50 << 20;

pub trait Fetcher {
    fn get(&self, url: &str, max_bytes: u64) -> Result<Vec<u8>>;
}

/// No network access in this context (host tests, or a command that never fetches).
pub struct Unavailable;
impl Fetcher for Unavailable {
    fn get(&self, url: &str, _max: u64) -> Result<Vec<u8>> {
        bail!("online fetching is not available here ({url})")
    }
}

/// Test double: URL -> body; records every request.
#[derive(Default)]
pub struct MemoryFetcher {
    pub files: RefCell<HashMap<String, Vec<u8>>>,
    pub calls: RefCell<Vec<String>>,
}
impl Fetcher for MemoryFetcher {
    fn get(&self, url: &str, max: u64) -> Result<Vec<u8>> {
        self.calls.borrow_mut().push(url.to_string());
        match self.files.borrow().get(url) {
            Some(b) if b.len() as u64 > max => bail!("{url}: larger than {max} bytes"),
            Some(b) => Ok(b.clone()),
            None => bail!("{url}: HTTP 404"),
        }
    }
}

extern "C" {
    fn dlopen(file: *const c_char, mode: c_int) -> *mut c_void;
    fn dlsym(handle: *mut c_void, name: *const c_char) -> *mut c_void;
    fn dlerror() -> *const c_char;
    fn time(t: *mut TimeT) -> TimeT;
}
#[cfg(target_os = "nto")]
type TimeT = i32; // QNX 6.5 time_t is 32-bit
#[cfg(not(target_os = "nto"))]
type TimeT = i64;
const RTLD_NOW: c_int = 0x0002;
#[cfg(target_os = "nto")]
const RTLD_GLOBAL: c_int = 0x0100;
#[cfg(not(target_os = "nto"))]
const RTLD_GLOBAL: c_int = 0x0008;

const CURLOPT_URL: c_int = 10002;
const CURLOPT_CAINFO: c_int = 10065;
const CURLOPT_FOLLOWLOCATION: c_int = 52;
const CURLOPT_WRITEFUNCTION: c_int = 20011;
const CURLOPT_WRITEDATA: c_int = 10001;
const CURLOPT_ERRORBUFFER: c_int = 10010;
const CURLOPT_USERAGENT: c_int = 10018;
const CURLOPT_CONNECTTIMEOUT: c_int = 78;
const CURLOPT_LOW_SPEED_LIMIT: c_int = 19;
const CURLOPT_LOW_SPEED_TIME: c_int = 20;
const CURLOPT_NOSIGNAL: c_int = 99;
const CURLOPT_SSL_CTX_FUNCTION: c_int = 20108;
const CURLINFO_RESPONSE_CODE: c_int = 0x200002;

struct Api {
    init: unsafe extern "C" fn() -> *mut c_void,
    setopt: unsafe extern "C" fn(*mut c_void, c_int, ...) -> c_int,
    perform: unsafe extern "C" fn(*mut c_void) -> c_int,
    getinfo: unsafe extern "C" fn(*mut c_void, c_int, ...) -> c_int,
    cleanup: unsafe extern "C" fn(*mut c_void),
    get0_param: Option<unsafe extern "C" fn(*mut c_void) -> *mut c_void>,
    set_time: Option<unsafe extern "C" fn(*mut c_void, TimeT)>,
}

static API: OnceLock<std::result::Result<Api, String>> = OnceLock::new();
static VERIFY_TIME: AtomicI64 = AtomicI64::new(0);

unsafe fn open(path: &str, global: bool) -> std::result::Result<*mut c_void, String> {
    let c = CString::new(path).unwrap();
    let h = dlopen(c.as_ptr(), RTLD_NOW | if global { RTLD_GLOBAL } else { 0 });
    if h.is_null() {
        let e = dlerror();
        return Err(if e.is_null() { format!("dlopen {path}") } else { CStr::from_ptr(e).to_string_lossy().into() });
    }
    Ok(h)
}

unsafe fn sym<T: Copy>(h: *mut c_void, name: &str) -> std::result::Result<T, String> {
    let c = CString::new(name).unwrap();
    let p = dlsym(h, c.as_ptr());
    if p.is_null() {
        return Err(format!("missing symbol {name}"));
    }
    Ok(std::mem::transmute_copy(&p))
}

fn load(lib: &str, preload: &[String]) -> std::result::Result<Api, String> {
    unsafe {
        let mut ssl = None;
        let mut crypto = None;
        for p in preload {
            let h = open(p, true)?;
            if p.contains("libssl") {
                ssl = Some(h);
            }
            if p.contains("libcrypto") {
                crypto = Some(h);
            }
        }
        let h = open(lib, false)?;
        Ok(Api {
            init: sym(h, "curl_easy_init")?,
            setopt: sym(h, "curl_easy_setopt")?,
            perform: sym(h, "curl_easy_perform")?,
            getinfo: sym(h, "curl_easy_getinfo")?,
            cleanup: sym(h, "curl_easy_cleanup")?,
            get0_param: match ssl { Some(s) => sym(s, "SSL_CTX_get0_param").ok(), None => None },
            set_time: match crypto { Some(c) => sym(c, "X509_VERIFY_PARAM_set_time").ok(), None => None },
        })
    }
}

struct Sink {
    buf: Vec<u8>,
    max: u64,
    over: bool,
}

extern "C" fn write_cb(ptr: *const u8, size: usize, n: usize, ud: *mut c_void) -> usize {
    let sink = unsafe { &mut *(ud as *mut Sink) };
    let len = size * n;
    if sink.buf.len() as u64 + len as u64 > sink.max {
        sink.over = true;
        return 0; // makes curl abort the transfer
    }
    sink.buf.extend_from_slice(unsafe { std::slice::from_raw_parts(ptr, len) });
    len
}

/// Sets OpenSSL's verification time to network time (the unit clock is 1970).
extern "C" fn ssl_ctx_cb(_curl: *mut c_void, ssl_ctx: *mut c_void, _p: *mut c_void) -> c_int {
    if let Some(Ok(api)) = API.get() {
        if let (Some(get0), Some(set)) = (api.get0_param, api.set_time) {
            unsafe {
                let param = get0(ssl_ctx);
                if !param.is_null() {
                    set(param, VERIFY_TIME.load(Ordering::Relaxed) as TimeT);
                }
            }
        }
    }
    0
}

pub struct CurlFetcher {
    pub lib: String,
    /// Loaded first with RTLD_GLOBAL so libcurl finds them without LD_LIBRARY_PATH.
    pub preload: Vec<String>,
    pub ca_bundle: Option<PathBuf>,
    /// Network minus local seconds; when set, certificates are checked at local time + offset.
    pub clock_offset: Option<i64>,
    /// Connect timeout, and the longest a transfer may stall before it is given up.
    pub timeout_secs: i64,
    /// Tests only: accept http:// URLs.
    pub allow_http: bool,
    /// Without `clock_offset`: where an offset measured this boot is kept, and the SNTP servers
    /// to measure it with when there is none (empty = trust the system clock).
    pub clock_file: Option<PathBuf>,
    pub time_servers: Vec<String>,
}

const UNIT_LIB: &str = "/mnt/app/armle/usr/lib";

impl CurlFetcher {
    pub fn for_unit(layout: &Layout, clock_offset: Option<i64>) -> CurlFetcher {
        let l = |n: &str| format!("{UNIT_LIB}/{n}");
        CurlFetcher {
            lib: l("libcurl.so.2"),
            preload: vec![l("libz.so.2"), l("libcrypto.so.2"), l("libssl.so.2"), l("libcares.so.1")],
            ca_bundle: Some(unit_ca(layout)),
            clock_offset,
            timeout_secs: 30,
            allow_http: false,
            clock_file: Some(layout.run("clock_offset")),
            time_servers: crate::clock::SERVERS.iter().map(|s| s.to_string()).collect(),
        }
    }

    /// The offset for certificate checks: the given one, else this boot's measurement, else a
    /// fresh SNTP query (an error when no server answers: without it every certificate looks
    /// "not yet valid" on a unit whose clock reads 1970).
    pub fn verify_offset(&self) -> Result<Option<i64>> {
        if self.clock_offset.is_some() || self.time_servers.is_empty() {
            return Ok(self.clock_offset);
        }
        let Some(file) = &self.clock_file else { return Ok(None) };
        let servers: Vec<&str> = self.time_servers.iter().map(String::as_str).collect();
        crate::clock::ensure_offset_at(file, &servers, std::time::Duration::from_secs(5)).map(Some).context("no network time")
    }
}

/// The CA bundle of the installed mibpkg package, falling back to bin/: a unit upgraded by an
/// older mibpkg (whose self-update copies only the binary and scripts) has it only there.
fn unit_ca(layout: &Layout) -> PathBuf {
    match crate::install::current_dir(layout, "mibpkg").map(|d| d.join("bin/ca.pem")) {
        Some(p) if p.exists() => p,
        _ => layout.bin_dir().join("ca.pem"),
    }
}

impl Fetcher for CurlFetcher {
    fn get(&self, url: &str, max_bytes: u64) -> Result<Vec<u8>> {
        let https = url.starts_with("https://");
        if !https && !(self.allow_http && url.starts_with("http://")) {
            bail!("{url}: only https:// URLs are allowed");
        }
        let api = match API.get_or_init(|| load(&self.lib, &self.preload)) {
            Ok(a) => a,
            Err(e) => bail!("libcurl unavailable: {e}"),
        };
        let offset = if https { self.verify_offset()? } else { None };
        unsafe {
            let c = (api.init)();
            if c.is_null() {
                bail!("curl_easy_init failed");
            }
            let curl_url = CString::new(url)?;
            let ua = CString::new(format!("mibpkg/{}", env!("CARGO_PKG_VERSION")))?;
            let ca = self.ca_bundle.as_ref().map(|p| CString::new(p.to_string_lossy().as_bytes()).unwrap());
            let mut err = [0 as c_char; 256];
            let mut sink = Sink { buf: Vec::new(), max: max_bytes, over: false };
            (api.setopt)(c, CURLOPT_URL, curl_url.as_ptr());
            (api.setopt)(c, CURLOPT_FOLLOWLOCATION, 1 as c_long);
            (api.setopt)(c, CURLOPT_USERAGENT, ua.as_ptr());
            // Bound connecting and stalls, not the whole transfer: a 10 MB package over weak LTE
            // can take minutes and is fine as long as bytes keep arriving.
            (api.setopt)(c, CURLOPT_CONNECTTIMEOUT, self.timeout_secs as c_long);
            (api.setopt)(c, CURLOPT_LOW_SPEED_LIMIT, 1 as c_long);
            (api.setopt)(c, CURLOPT_LOW_SPEED_TIME, self.timeout_secs as c_long);
            (api.setopt)(c, CURLOPT_NOSIGNAL, 1 as c_long);
            (api.setopt)(c, CURLOPT_ERRORBUFFER, err.as_mut_ptr());
            (api.setopt)(c, CURLOPT_WRITEFUNCTION, write_cb as extern "C" fn(*const u8, usize, usize, *mut c_void) -> usize);
            (api.setopt)(c, CURLOPT_WRITEDATA, &mut sink as *mut Sink as *mut c_void);
            if let Some(ca) = &ca {
                (api.setopt)(c, CURLOPT_CAINFO, ca.as_ptr());
            }
            if let Some(off) = offset {
                VERIFY_TIME.store(time(std::ptr::null_mut()) as i64 + off, Ordering::Relaxed);
                (api.setopt)(c, CURLOPT_SSL_CTX_FUNCTION, ssl_ctx_cb as extern "C" fn(*mut c_void, *mut c_void, *mut c_void) -> c_int);
            }
            let rc = (api.perform)(c);
            let mut code: c_long = 0;
            (api.getinfo)(c, CURLINFO_RESPONSE_CODE, &mut code as *mut c_long);
            (api.cleanup)(c);
            if sink.over {
                bail!("{url}: larger than {max_bytes} bytes");
            }
            if rc != 0 {
                bail!("{url}: curl error {rc}: {}", CStr::from_ptr(err.as_ptr()).to_string_lossy());
            }
            if code != 200 {
                bail!("{url}: HTTP {code}");
            }
            Ok(sink.buf)
        }
    }
}
