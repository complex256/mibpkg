//! THROWAWAY spike: fetch a URL over HTTPS with the unit's libcurl via dlopen.
//! usage: curlspike <url> <ca-bundle.pem>
use std::ffi::{c_char, c_int, c_long, c_void, CStr, CString};

extern "C" {
    fn dlopen(file: *const c_char, mode: c_int) -> *mut c_void;
    fn dlsym(handle: *mut c_void, name: *const c_char) -> *mut c_void;
    fn dlerror() -> *const c_char;
}
const RTLD_NOW: c_int = 0x0002;

const CURLOPT_URL: c_int = 10002;
const CURLOPT_CAINFO: c_int = 10065;
const CURLOPT_FOLLOWLOCATION: c_int = 52;
const CURLOPT_WRITEFUNCTION: c_int = 20011;
const CURLOPT_WRITEDATA: c_int = 10001;
const CURLOPT_ERRORBUFFER: c_int = 10010;
const CURLOPT_SSL_VERIFYPEER: c_int = 64;
const CURLOPT_USERAGENT: c_int = 10018;
const CURLINFO_RESPONSE_CODE: c_int = 0x200002;
const CURLOPT_SSL_CTX_FUNCTION: c_int = 20108;
const CURLOPT_SSL_CTX_DATA: c_int = 10109;

/// Verification time for OpenSSL, set from the ctx callback (unit clock is 1970).
static mut VERIFY_TIME: i32 = 0;
static mut SSL_GET0_PARAM: Option<unsafe extern "C" fn(*mut c_void) -> *mut c_void> = None;
static mut PARAM_SET_TIME: Option<unsafe extern "C" fn(*mut c_void, i32)> = None;

extern "C" fn ssl_ctx_cb(_curl: *mut c_void, ssl_ctx: *mut c_void, _parm: *mut c_void) -> c_int {
    unsafe {
        if VERIFY_TIME != 0 {
            if let (Some(get0), Some(set)) = (SSL_GET0_PARAM, PARAM_SET_TIME) {
                let param = get0(ssl_ctx);
                if !param.is_null() {
                    set(param, VERIFY_TIME);
                }
            }
        }
    }
    0
}

type Init = unsafe extern "C" fn() -> *mut c_void;
type SetOpt = unsafe extern "C" fn(*mut c_void, c_int, ...) -> c_int;
type Perform = unsafe extern "C" fn(*mut c_void) -> c_int;
type GetInfo = unsafe extern "C" fn(*mut c_void, c_int, ...) -> c_int;
type Version = unsafe extern "C" fn() -> *const c_char;

extern "C" fn write_cb(ptr: *const u8, size: usize, n: usize, ud: *mut c_void) -> usize {
    let v = unsafe { &mut *(ud as *mut Vec<u8>) };
    v.extend_from_slice(unsafe { std::slice::from_raw_parts(ptr, size * n) });
    size * n
}

unsafe fn sym<T>(h: *mut c_void, name: &str) -> T {
    let c = CString::new(name).unwrap();
    let p = dlsym(h, c.as_ptr());
    assert!(!p.is_null(), "missing {name}");
    std::mem::transmute_copy(&p)
}

fn main() {
    let url = std::env::args().nth(1).expect("url");
    let ca = std::env::args().nth(2).expect("ca bundle");
    // optional 3rd arg: seconds to add to time() for certificate checks
    let offset: i64 = std::env::args().nth(3).map(|o| o.parse().unwrap()).unwrap_or(0);
    unsafe {
        let lib = CString::new("libcurl.so.2").unwrap();
        let h = dlopen(lib.as_ptr(), RTLD_NOW);
        if h.is_null() {
            eprintln!("dlopen: {}", CStr::from_ptr(dlerror()).to_string_lossy());
            std::process::exit(2);
        }
        if offset != 0 {
            extern "C" {
                fn time(t: *mut i32) -> i32;
            }
            VERIFY_TIME = (time(std::ptr::null_mut()) as i64 + offset) as i32;
            let ssl = dlopen(CString::new("libssl.so.2").unwrap().as_ptr(), RTLD_NOW);
            let cry = dlopen(CString::new("libcrypto.so.2").unwrap().as_ptr(), RTLD_NOW);
            SSL_GET0_PARAM = Some(sym(ssl, "SSL_CTX_get0_param"));
            PARAM_SET_TIME = Some(sym(cry, "X509_VERIFY_PARAM_set_time"));
            println!("verify time {}", VERIFY_TIME);
        }
        let version: Version = sym(h, "curl_version");
        println!("{}", CStr::from_ptr(version()).to_string_lossy());
        let init: Init = sym(h, "curl_easy_init");
        let setopt: SetOpt = sym(h, "curl_easy_setopt");
        let perform: Perform = sym(h, "curl_easy_perform");
        let getinfo: GetInfo = sym(h, "curl_easy_getinfo");
        let c = init();
        let curl_url = CString::new(url).unwrap();
        let cainfo = CString::new(ca).unwrap();
        let ua = CString::new("mibpkg-spike").unwrap();
        let mut body: Vec<u8> = Vec::new();
        let mut err = [0 as c_char; 256];
        setopt(c, CURLOPT_URL, curl_url.as_ptr());
        setopt(c, CURLOPT_CAINFO, cainfo.as_ptr());
        setopt(c, CURLOPT_SSL_VERIFYPEER, 1 as c_long);
        setopt(c, CURLOPT_FOLLOWLOCATION, 1 as c_long);
        setopt(c, CURLOPT_USERAGENT, ua.as_ptr());
        setopt(c, CURLOPT_ERRORBUFFER, err.as_mut_ptr());
        setopt(c, CURLOPT_WRITEFUNCTION, write_cb as extern "C" fn(*const u8, usize, usize, *mut c_void) -> usize);
        setopt(c, CURLOPT_WRITEDATA, &mut body as *mut Vec<u8> as *mut c_void);
        if offset != 0 {
            setopt(c, CURLOPT_SSL_CTX_FUNCTION, ssl_ctx_cb as extern "C" fn(*mut c_void, *mut c_void, *mut c_void) -> c_int);
            setopt(c, CURLOPT_SSL_CTX_DATA, std::ptr::null_mut::<c_void>());
        }
        let rc = perform(c);
        let mut code: c_long = 0;
        getinfo(c, CURLINFO_RESPONSE_CODE, &mut code as *mut c_long);
        println!("rc={rc} http={code} bytes={} err={}", body.len(), CStr::from_ptr(err.as_ptr()).to_string_lossy());
        println!("{}", String::from_utf8_lossy(&body[..body.len().min(200)]));
    }
}
