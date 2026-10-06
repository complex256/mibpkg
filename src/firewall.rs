//! The car's packet filter (/mnt/system/etc/pf.conf, which is /etc/pf.conf) drops every
//! connection to the car on its Wi-Fi hotspot except a short stock list (DNS, audio, EXLAP; the
//! M.I.B. toolbox adds ssh the same way). For the web interface to be reachable on the hotspot,
//! mibpkg keeps one marked pass rule just before the stock "block everything else" line, and takes
//! it out again when the hotspot is not wanted or mibpkg is uninstalled. Changes are made on flash
//! only and apply at the next start of the head unit (see `apply` for why).
use crate::ctx::Ctx;
use crate::fsutil::atomic_write;
use crate::paths::Layout;
use crate::web::settings::WebSettings;
use anyhow::{bail, Context, Result};
use std::fs;
use std::path::{Path, PathBuf};

/// The comment line that marks mibpkg's rule (the rule is the line after it).
pub const MARK: &str = "# mibpkg web interface: added and removed by mibpkg";
/// The stock rule that drops everything else addressed to the car on the hotspot.
const HOTSPOT_BLOCK: &str = "block in quick on $wlan_if from any to ($wlan_if)";
/// Not on the unit's PATH.
pub const PFCTL: &str = "/mnt/app/armle/sbin/pfctl";

fn rule(port: u16) -> String {
    format!("pass in quick on $wlan_if proto tcp from ($wlan_if:network) to ($wlan_if) port {port} keep state allow-opts")
}

/// The rule sets the car may load: pf.conf at boot, then the connectivity manager swaps in the
/// one for the current uplink (pf.ecm0.conf when SIM data is up, seen on the unit 2026-10-05).
/// Every one of them that exists gets the rule, as the M.I.B. toolbox does for ssh.
pub const VARIANTS: &[&str] = &["pf.conf", "pf.ecm0.conf", "pf.mlan0.conf", "pf.ppp0.conf"];

pub fn pf_conf(layout: &Layout) -> PathBuf {
    layout.p("/mnt/system/etc/pf.conf")
}

/// The original pf.conf, saved before mibpkg first changed it.
pub fn backup_path(layout: &Layout) -> PathBuf {
    variant_backup(&pf_conf(layout))
}

fn variant_backup(path: &Path) -> PathBuf {
    let mut s = path.as_os_str().to_owned();
    s.push(".mibpkg-orig");
    PathBuf::from(s)
}

/// The variants present on this unit.
fn variants(layout: &Layout) -> Vec<PathBuf> {
    VARIANTS.iter().map(|v| layout.p("/mnt/system/etc").join(v)).filter(|p| p.is_file()).collect()
}

/// The port the hotspot rule should open: the web server's bound port when it is enabled and
/// listens on the hotspot (or on any interface); None = no rule.
pub fn wanted(settings: &WebSettings, bound_port: u16) -> Option<u16> {
    let hotspot = settings.listen.iter().any(|l| l == "hotspot" || l == "any");
    (settings.enabled && hotspot).then_some(bound_port)
}

/// `text` with mibpkg's block removed, then (for Some(port)) added back just before the stock
/// hotspot block line. Everything else stays byte for byte.
pub fn edit(text: &str, port: Option<u16>) -> Result<String> {
    let mut lines: Vec<&str> = text.split_inclusive('\n').collect();
    // Remove our marker and the rule line that follows it.
    if let Some(i) = lines.iter().position(|l| l.trim_end() == MARK) {
        let end = (i + 2).min(lines.len());
        lines.drain(i..end);
    }
    let Some(port) = port else { return Ok(lines.concat()) };
    let Some(at) = lines.iter().position(|l| l.trim_end() == HOTSPOT_BLOCK) else {
        bail!("pf.conf has no \"{HOTSPOT_BLOCK}\" line; not changing it (name the port by hand)");
    };
    let block = format!("{MARK}\n{}\n", rule(port));
    let mut out = lines[..at].concat();
    out.push_str(&block);
    out.push_str(&lines[at..].concat());
    Ok(out)
}

/// Whether any pf variant differs from what `port` wants (a cheap read-only check).
pub fn needs_change(layout: &Layout, port: Option<u16>) -> bool {
    variants(layout).iter().any(|p| match fs::read_to_string(p) {
        Ok(text) => edit(&text, port).map_or(false, |new| new != text),
        Err(_) => false,
    })
}

/// Run pfctl on the unit (never in tests or under --root).
fn pfctl(layout: &Layout, args: &[&str]) -> Result<()> {
    if layout.root != Path::new("/") {
        return Ok(());
    }
    // pfctl reports problems on stderr; keep it so a refusal says why. `-n` only parses, so it
    // returns quickly.
    let out = std::process::Command::new(PFCTL)
        .args(args)
        .stdin(std::process::Stdio::null())
        .output()
        .context("running pfctl")?;
    let text = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    if !out.status.success() || text.contains("pfctl:") {
        bail!("pfctl {}: {}", args.join(" "), text.trim());
    }
    Ok(())
}

/// Make pf.conf on flash match `port`. Caller holds the action lock.
///
/// The live firewall is never reloaded: `pfctl -f` re-reads pf.conf and resets the anchors the
/// car's connectivity manager fills in at runtime (mmi_data, dns_data, wlan_data go back to
/// "block all" from /etc/pf.data.default), which cut the car off the internet (seen on the unit,
/// 2026-10-05). The rule takes effect when the head unit next starts; Tailscale works meanwhile.
pub fn apply(ctx: &Ctx, port: Option<u16>) -> Result<String> {
    let layout = ctx.layout.clone();
    apply_with(ctx, port, &mut |args: &[&str]| pfctl(&layout, args))
}

/// `apply` with the pfctl runner passed in (tests record the calls).
pub fn apply_with(ctx: &Ctx, port: Option<u16>, pfctl: &mut dyn FnMut(&[&str]) -> Result<()>) -> Result<String> {
    let layout = &ctx.layout;
    let present = variants(layout);
    if present.is_empty() {
        bail!("no pf.conf in /mnt/system/etc; not a MIB2 High firewall mibpkg knows");
    }
    // Work out every change first and check each with pfctl (-n: parse only, load nothing):
    // either all variants change or none does.
    let mut changes: Vec<(PathBuf, String, String)> = Vec::new();
    for path in present {
        let old = fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
        let new = edit(&old, port).with_context(|| path.display().to_string())?;
        if new == old {
            continue;
        }
        let check = layout.run("pf.conf.check");
        atomic_write(&check, new.as_bytes())?;
        let checked = pfctl(&["-nf", &layout.unit_path(&check)]);
        let _ = fs::remove_file(&check);
        if let Err(e) = checked {
            // Some variants cannot be checked at all right now: pf.ppp0.conf shapes traffic on
            // ppp0, and without that uplink pfctl fails on the stock file too ("SIOCGIFMTU: No such
            // device or address", seen on the unit 2026-10-06). Only an edit that breaks a file
            // which checked fine before is refused.
            if pfctl(&["-nf", &layout.unit_path(&path)]).is_ok() {
                return Err(e.context(format!("the changed {} does not load; not changing anything", path.display())));
            }
        }
        changes.push((path, old, new));
    }
    if changes.is_empty() {
        return Ok("hotspot firewall: already set".into());
    }
    {
        let _rw = ctx.mounts.rw("/mnt/system")?;
        for (path, old, new) in &changes {
            let backup = variant_backup(path);
            if !backup.exists() {
                atomic_write(&backup, old.as_bytes())?;
            }
            atomic_write(path, new.as_bytes())?;
        }
    }
    Ok(match port {
        Some(p) => format!("hotspot firewall: port {p} opens after the next restart of the head unit"),
        None => "hotspot firewall: closed after the next restart of the head unit".into(),
    })
}
