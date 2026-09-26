fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match mibpkg::cli::run(&args) {
        Ok(code) => std::process::exit(code),
        Err(e) => {
            eprintln!("mibpkg: {e:#}");
            std::process::exit(1);
        }
    }
}

/// QNX 6.5's libgcc has no `_Unwind_FindEnclosingFunction`, which std's backtrace code
/// (reached through anyhow) references. Backtraces are off on this target; hand back the pc.
#[cfg(target_os = "nto")]
#[no_mangle]
pub extern "C" fn _Unwind_FindEnclosingFunction(pc: *mut core::ffi::c_void) -> *mut core::ffi::c_void {
    pc
}
