//! One-time migration of hand installs (filled in Task 12).
use crate::ctx::Ctx;
use anyhow::Result;

pub fn migrate(_ctx: &Ctx, _pkg: &str) -> Result<Option<String>> {
    Ok(None)
}
