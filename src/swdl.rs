//! Host-side builder for the standalone SWDL install card: mibpkg's own SWDL package
//! (metainfo2.txt + a thin hash-registered install.sh), independent of M.I.B. The
//! FinalScriptChecksum is computed from the exact install.sh bytes embedded here, so it
//! can never drift from what the card ships.
use crate::sha1::sha1_hex;
use anyhow::{Context, Result};
use std::fs;
use std::path::Path;

pub const INSTALL_SH: &str = include_str!("../shell/install.sh");

/// Build a card tree at `out`: metainfo2.txt, the install.sh final-script, the boot
/// payload, an optional seed collection, and a disabled autostart marker.
pub fn build_card(out: &Path, payload: &Path, seed: Option<&Path>, version: &str) -> Result<()> {
    let final_dir = out.join("mibpkg/final");
    fs::create_dir_all(&final_dir)?;
    fs::write(final_dir.join("install.sh"), INSTALL_SH)?;
    let hash = sha1_hex(INSTALL_SH.as_bytes());
    fs::write(out.join("metainfo2.txt"), metainfo(version, &hash))?;
    copy_tree(payload, &out.join("mibpkg/payload")).context("copying payload")?;
    if let Some(s) = seed {
        copy_tree(s, &out.join("mibpkg/collection")).context("copying seed collection")?;
    }
    fs::write(out.join("_Swdlautorun.txt"), "")?; // autostart marker, disabled by default
    Ok(())
}

fn metainfo(version: &str, hash: &str) -> String {
    format!(
        "[common]\n\
vendor = \"complex256\"\n\
release = \"mibpkg installer v{version}\"\n\
skipCheckSignatureAndVariant = \"true\"\n\
skipMetaCRC = \"true\"\n\
skipFileCopyCrc = \"true\"\n\
skipCheckVariant = \"true\"\n\
skipCheckRegion = \"true\"\n\
skipSaveTrainName = \"true\"\n\
variant = \"FM?-*-*-*-*\"\n\
region = \"RoW\"\n\
FinalScript = \"./mibpkg/final/install.sh\"\n\
FinalScriptChecksum = \"{hash}\"\n\
FinalScriptMaxTime = \"300\"\n\
FinalScriptName = \"mibpkg install\"\n\n\
[mibpkg]\n\
VendorInfo = \"complex256\"\n\
DeviceDescription = \"mibpkg installer\"\n\
DownloadGroup = \"mibpkg\"\n"
    )
}

fn copy_tree(src: &Path, dst: &Path) -> Result<()> {
    fs::create_dir_all(dst)?;
    for e in fs::read_dir(src).with_context(|| format!("reading {}", src.display()))? {
        let e = e?;
        let (from, to) = (e.path(), dst.join(e.file_name()));
        if e.file_type()?.is_dir() {
            copy_tree(&from, &to)?;
        } else {
            fs::copy(&from, &to).with_context(|| format!("copying {}", from.display()))?;
        }
    }
    Ok(())
}
