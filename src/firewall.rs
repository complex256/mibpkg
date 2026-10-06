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
use std::time::Duration;

/// The comment line that marks mibpkg's rule (the rule is the line after it).
pub const MARK: &str = "# mibpkg web interface: added and removed by mibpkg";
/// The stock rule that drops everything else addressed to the car on the hotspot.
const HOTSPOT_BLOCK: &str = "block in quick on $wlan_if from any to ($wlan_if)";
/// Not on the unit's PATH.
pub const PFCTL: &str = "/mnt/app/armle/sbin/pfctl";

fn rule(port: u16) -> String {
    format!("pass in quick on $wlan_if proto tcp from ($wlan_if:network) to ($wlan_if) port {port} keep state allow-opts")
}

pub fn pf_conf(layout: &Layout) -> PathBuf {
    layout.p("/mnt/system/etc/pf.conf")
}

/// The original pf.conf, saved before mibpkg first changed it.
pub fn backup_path(layout: &Layout) -> PathBuf {
    layout.p("/mnt/system/etc/pf.conf.mibpkg-orig")
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

/// Whether pf.conf differs from what `port` wants (a cheap read-only check).
pub fn needs_change(layout: &Layout, port: Option<u16>) -> bool {
    match fs::read_to_string(pf_conf(layout)) {
        Ok(text) => edit(&text, port).map_or(false, |new| new != text),
        Err(_) => false, // no pf.conf (not a MIB2 High?): nothing mibpkg can do
    }
}

/// Run pfctl on the unit (never in tests or under --root).
fn pfctl(layout: &Layout, args: &[&str]) -> Result<()> {
    if layout.root != Path::new("/") {
        return Ok(());
    }
    let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();
    let (ok, out) = crate::sys::run_limited(Path::new(PFCTL), &args, &[], Duration::from_secs(15)).map_err(anyhow::Error::msg)?;
    if !ok {
        bail!("pfctl {}: {}", args.join(" "), out.trim());
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
    let path = pf_conf(layout);
    let old = fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
    let new = edit(&old, port)?;
    if new == old {
        return Ok("hotspot firewall: already set".into());
    }
    // Check the new file's syntax with pfctl (-n: parse only, load nothing) before it replaces
    // the real one.
    let check = layout.run("pf.conf.check");
    atomic_write(&check, new.as_bytes())?;
    let checked = pfctl(&["-nf", &layout.unit_path(&check)]);
    let _ = fs::remove_file(&check);
    checked.context("the changed pf.conf does not load; not changing it")?;
    {
        let _rw = ctx.mounts.rw("/mnt/system")?;
        if !backup_path(layout).exists() {
            atomic_write(&backup_path(layout), old.as_bytes())?;
        }
        atomic_write(&path, new.as_bytes())?;
    }
    Ok(match port {
        Some(p) => format!("hotspot firewall: port {p} opens after the next restart of the head unit"),
        None => "hotspot firewall: closed after the next restart of the head unit".into(),
    })
}
