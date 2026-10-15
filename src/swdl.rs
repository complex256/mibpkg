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

/// Validate a built card the way a car's SWDL would care about: the manifest exists, its
/// FinalScriptChecksum matches the install.sh bytes, the payload is present, and every
/// patches.toml `file =` is on the card. Returns the problems (empty = ok).
pub fn verify_card(dir: &Path) -> Result<Vec<String>> {
    let mut p = Vec::new();
    let meta = fs::read_to_string(dir.join("metainfo2.txt")).context("reading metainfo2.txt")?;
    match fs::read(dir.join("mibpkg/final/install.sh")) {
        Ok(b) => {
            let want = sha1_hex(&b);
            if !meta.contains(&format!("FinalScriptChecksum = \"{want}\"")) {
                p.push(format!("FinalScriptChecksum hash does not match install.sh (expected {want})"));
            }
        }
        Err(_) => p.push("missing mibpkg/final/install.sh".into()),
    }
    for f in ["mibpkg", "launch.sh", "gem.sh"] {
        if !dir.join("mibpkg/payload").join(f).exists() {
            p.push(format!("missing payload file {f}"));
        }
    }
    let gp = dir.join("mibpkg/payload/gem-patch");
    let toml = gp.join("patches.toml");
    if toml.exists() {
        for line in fs::read_to_string(&toml)?.lines() {
            if let Some(f) = line.trim().strip_prefix("file = ").map(|s| s.trim_matches('"')) {
                if !gp.join(f).exists() {
                    p.push(format!("patches.toml references missing {f}"));
                }
            }
        }
    }
    Ok(p)
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
