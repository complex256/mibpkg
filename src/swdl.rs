//! Host-side builder for the standalone SWDL install card: mibpkg's own SWDL package
//! (metainfo2.txt + a thin hash-registered install.sh), independent of M.I.B. The
//! FinalScriptChecksum is computed from the exact install.sh bytes embedded here, so it
//! can never drift from what the card ships.
use crate::sha1::sha1_hex;
use anyhow::{Context, Result};
use std::fs;
use std::path::Path;

pub const INSTALL_SH: &str = include_str!("../shell/install.sh");
pub const CUSTOM_SH: &str = include_str!("../shell/custom.sh");
pub const COMMAND_SH: &str = include_str!("../shell/command.sh");

/// Build a card tree at `out`: metainfo2.txt, the install.sh final-script, the boot
/// payload, an optional seed collection, and a disabled autostart marker. When `with_mib`,
/// also lay down the M.I.B. "Run Custom Script" entry (/mod) so one card installs either way.
pub fn build_card(out: &Path, payload: &Path, seed: Option<&Path>, version: &str, signed_header: Option<&str>, with_mib: bool) -> Result<()> {
    let final_dir = out.join("mibpkg/final");
    fs::create_dir_all(&final_dir)?;
    fs::write(final_dir.join("install.sh"), INSTALL_SH)?;
    let hash = sha1_hex(INSTALL_SH.as_bytes());
    // SWDL only offers a group that has a real file to copy. Write a small marker and describe
    // it as the group's one File entry (mirrors how a stock update lists itself).
    let marker = format!("mibpkg installer v{version}\n");
    let vdir = out.join("mibpkg/version");
    fs::create_dir_all(&vdir)?;
    fs::write(vdir.join("MARKER"), &marker)?;
    let marker_sha = sha1_hex(marker.as_bytes());
    fs::write(out.join("metainfo2.txt"), metainfo(version, &hash, signed_header, &marker_sha, marker.len()))?;
    copy_tree(payload, &out.join("mibpkg/payload")).context("copying payload")?;
    if let Some(s) = seed {
        // Land the seed where the supervisor's media scan looks (mibpkg/collections/<id>),
        // so first-boot install finds it; the subdir name is not significant to the scan.
        copy_tree(s, &out.join("mibpkg/collections/seed")).context("copying seed collection")?;
    }
    fs::write(out.join("_Swdlautorun.txt"), "")?; // autostart marker, disabled by default
    if with_mib {
        // M.I.B. "Run Custom Script" entry: /mod/custom.sh (+ the command.sh compat shim) and a
        // copy of the payload it lands. The seed collection at mibpkg/collections/ is shared.
        let mod_dir = out.join("mod");
        fs::create_dir_all(&mod_dir)?;
        fs::write(mod_dir.join("custom.sh"), CUSTOM_SH)?;
        fs::write(mod_dir.join("command.sh"), COMMAND_SH)?;
        copy_tree(payload, &mod_dir.join("mibpkg")).context("copying M.I.B. payload")?;
    }
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
    // If the M.I.B. entry is present, its payload must be too.
    if dir.join("mod/custom.sh").exists() && !dir.join("mod/mibpkg/mibpkg").exists() {
        p.push("mod/custom.sh present but mod/mibpkg/mibpkg is missing".into());
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

fn metainfo(version: &str, hash: &str, signed_header: Option<&str>, marker_sha: &str, marker_len: usize) -> String {
    // SWDL validates the signature of the FIRST signed document, then our second [common]
    // (with the skip flags + FinalScript) is the operative install. Without a genuine
    // Audi-signed header prepended, the signature check rejects the card.
    let prefix = match signed_header {
        Some(h) => format!("{}\n\n", h.trim_end()),
        None => String::new(),
    };
    // The version/0/default/File entry's Source "../../MARKER" resolves (logically, from
    // mibpkg/version/0/default/) to mibpkg/version/MARKER on the card. This one real file is
    // what makes SWDL offer the group as an installable update and enable START.
    format!(
        "{prefix}[common]\n\
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
DownloadGroup = \"mibpkg\"\n\n\
[mibpkg\\version\\0\\default\\File]\n\
checkUpdate = \"true\"\n\
CheckSumSize = \"524288\"\n\
CheckSum = \"{marker_sha}\"\n\
FileSize = \"{marker_len}\"\n\
Version = \"1\"\n\
Source = \"../../MARKER\"\n\
Destination = \"/HBpersistence/SWDL/Log/service/mibpkg\"\n\
DisplayName = \"mibpkg installer\"\n\
DeleteDestinationDirBeforeCopy = \"false\"\n\
UpdateOnlyExisting = \"false\"\n"
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
