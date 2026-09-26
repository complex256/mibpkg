fn main() {
    // Touch every dependency so the cross-build smoke test compiles and links them all.
    let _ = anyhow::anyhow!("x");
    let _ = flate2::Compression::best();
    let _ = semver::Version::parse("1.0.0");
    let _ = serde_json::json!({});
    let _ = <sha2::Sha256 as sha2::Digest>::new();
    let _ = toml::from_str::<toml::Value>("a = 1");
    println!("mibpkg {}", env!("CARGO_PKG_VERSION"));
}

/// QNX 6.5's libgcc has no `_Unwind_FindEnclosingFunction`, which std's backtrace code
/// (reached through anyhow) references. Backtraces are off on this target; hand back the pc.
#[cfg(target_os = "nto")]
#[no_mangle]
pub extern "C" fn _Unwind_FindEnclosingFunction(pc: *mut core::ffi::c_void) -> *mut core::ffi::c_void {
    pc
}
