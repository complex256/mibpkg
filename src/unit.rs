//! Which head unit this is, for package [compat] checks.
//! Source: first line of /mnt/app/img_ver.txt, e.g. "#DEV_MMX2QC_AU_NAR_G22_061PROD".
//! MMX2QC => MHI2Q (Qualcomm); other MMX2 => MHI2 (unverified, see docs/unit-facts.md).
use crate::manifest::Compat;
use crate::paths::Layout;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct UnitInfo {
    pub unit: Option<String>,
    pub firmware: Option<String>,
}

impl UnitInfo {
    pub fn describe(&self) -> String {
        format!(
            "{} ({})",
            self.unit.as_deref().unwrap_or("unknown"),
            self.firmware.as_deref().unwrap_or("unknown firmware")
        )
    }
}

pub fn parse_img_ver(text: &str) -> UnitInfo {
    let first = text.lines().next().unwrap_or("").trim().trim_start_matches('#').trim().to_string();
    if first.is_empty() {
        return UnitInfo::default();
    }
    let unit = if first.contains("MMX2QC") {
        Some("MHI2Q")
    } else if first.contains("MMX2") {
        Some("MHI2")
    } else {
        None
    };
    UnitInfo { unit: unit.map(String::from), firmware: Some(first) }
}

pub fn detect(layout: &Layout) -> UnitInfo {
    let text = std::fs::read_to_string(layout.p("/mnt/app/img_ver.txt")).unwrap_or_default();
    let mut info = parse_img_ver(&text);
    if let Some(u) = &layout.settings.unit {
        info.unit = Some(u.clone());
    }
    info
}

impl Compat {
    pub fn matches(&self, u: &UnitInfo) -> bool {
        let unit_ok =
            self.units.is_empty() || u.unit.as_ref().map_or(false, |x| self.units.iter().any(|w| w == x));
        let fw_ok = self.firmware.is_empty()
            || u.firmware.as_ref().map_or(false, |x| self.firmware.iter().any(|w| x.contains(w.as_str())));
        unit_ok && fw_ok
    }
}

/// The HMI build: the value of the first `HMI = ...` line of /ifs/version_info.txt with all
/// whitespace removed, e.g. "HMI_AU_MIB2_HQC_NAR_G22_C10_180803".
pub fn hmi_version(layout: &Layout) -> Option<String> {
    let raw = std::fs::read(layout.p(crate::mibsdk::VERSION_INFO)).ok()?;
    let text = String::from_utf8_lossy(&raw);
    let line = text.lines().find(|l| l.strip_prefix("HMI").is_some_and(|r| r.trim_start().starts_with('=')))?;
    let v: String = line.split_once('=')?.1.chars().filter(|c| !c.is_whitespace()).collect();
    (!v.is_empty()).then_some(v)
}
