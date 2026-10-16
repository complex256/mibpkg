//! Turn the green engineering menu on/off: write the block-5F dev-mode adaptation
//! (persistence key 3221356557 ns 0) then the commit key (1 ns 0), mirroring M.I.B.'s
//! two UDP writes. This is path (a) stock `pc` via Sys::persist_write; spike S2 decides
//! whether `pc` reaches the byte-typed key or path (b) (own writer) is needed behind
//! this same interface.
use crate::sys::Sys;
use anyhow::Result;

pub const GEM_DEV_KEY: u32 = 3221356557;
pub const COMMIT_KEY: u32 = 1;

fn set(sys: &dyn Sys, value: u8) -> Result<()> {
    // The flag is a BYTE (ns0 key 3221356557); a string write hangs pc (verified on-car).
    sys.persist_write_byte(0, GEM_DEV_KEY, value)?;
    sys.persist_write_byte(0, COMMIT_KEY, 0)?; // store changes to persistence
    Ok(())
}

pub fn enable(sys: &dyn Sys) -> Result<()> {
    set(sys, 1)
}

pub fn disable(sys: &dyn Sys) -> Result<()> {
    set(sys, 0)
}
