//! Which local addresses the web server answers on. The server binds every address once and
//! checks each connection's local address here, so interfaces that come up after boot
//! (Tailscale ~2 minutes in) need no rebinding.
use super::settings::WebSettings;
use std::collections::BTreeMap;
use std::net::{IpAddr, Ipv4Addr};
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// How long one `ifconfig` reading is trusted.
const REFRESH: Duration = Duration::from_secs(10);

#[derive(Debug, Clone, PartialEq)]
pub struct Iface {
    pub name: String,
    pub up: bool,
    pub v4: Vec<Ipv4Addr>,
}

/// Parse `ifconfig` with no arguments (QNX and BSD print every interface): a block per interface
/// starting with `name: flags=…<FLAGS>`, then indented lines such as `inet 1.2.3.4 …`.
pub fn parse_ifconfig_all(text: &str) -> Vec<Iface> {
    let mut out: Vec<Iface> = Vec::new();
    for line in text.lines() {
        let indented = line.starts_with(' ') || line.starts_with('\t');
        if !indented {
            if let Some((name, rest)) = line.split_once(':') {
                if !name.is_empty() && !name.contains(' ') {
                    let flags = rest.split_once('<').and_then(|(_, r)| r.split_once('>')).map(|(f, _)| f).unwrap_or("");
                    let up = flags.split(',').any(|f| f == "UP");
                    out.push(Iface { name: name.to_string(), up, v4: Vec::new() });
                }
            }
            continue;
        }
        let mut words = line.split_whitespace();
        if words.next() == Some("inet") {
            if let (Some(iface), Some(addr)) = (out.last_mut(), words.next().and_then(|a| a.parse::<Ipv4Addr>().ok())) {
                iface.v4.push(addr);
            }
        }
    }
    out
}

/// Cellular links (SIM data): never classed as hotspot, whatever their address.
const CELLULAR: &[&str] = &["ecm", "ppp", "rmnet", "wwan"];

fn in_tailnet(a: &Ipv4Addr) -> bool {
    // 100.64.0.0/10 (CGNAT space Tailscale assigns from)
    a.octets()[0] == 100 && (a.octets()[1] & 0xC0) == 64
}

/// "hotspot", "tailscale", or None. Explicit `interfaces` overrides replace the automatic rule
/// for their class: Tailscale = a tun* with a 100.64/10 address; hotspot = any other up
/// interface with a private (RFC 1918) address that is not loopback or cellular.
pub fn class_of(iface: &Iface, overrides: &BTreeMap<String, Vec<String>>) -> Option<&'static str> {
    for (class, name) in [("tailscale", "tailscale"), ("hotspot", "hotspot")] {
        if let Some(names) = overrides.get(name) {
            if names.iter().any(|n| *n == iface.name) {
                return Some(class);
            }
        }
    }
    if !iface.up {
        return None;
    }
    if !overrides.contains_key("tailscale") && iface.name.starts_with("tun") && iface.v4.iter().any(in_tailnet) {
        return Some("tailscale");
    }
    let skip = iface.name.starts_with("lo") || iface.name.starts_with("tun") || CELLULAR.iter().any(|p| iface.name.starts_with(p));
    if !overrides.contains_key("hotspot") && !skip && iface.v4.iter().any(|a| a.is_private()) {
        return Some("hotspot");
    }
    None
}

struct Snapshot {
    at: Instant,
    ifaces: Vec<Iface>,
}

/// The allowed-address check, with a short-lived cache of `ifconfig`.
pub struct Allowed {
    any: bool,
    listen: Vec<String>,
    overrides: BTreeMap<String, Vec<String>>,
    probe: Box<dyn Fn() -> Option<String> + Send + Sync>,
    snap: Mutex<Option<Snapshot>>,
}

impl Allowed {
    /// `probe` returns `ifconfig` output, or None when it could not run.
    pub fn new(settings: &WebSettings, probe: Box<dyn Fn() -> Option<String> + Send + Sync>) -> Allowed {
        Allowed {
            any: settings.any(),
            listen: settings.listen.clone(),
            overrides: settings.interfaces.clone(),
            probe,
            snap: Mutex::new(None),
        }
    }

    /// The real probe: `ifconfig` on the unit, bounded so a hung call cannot stall a connection.
    pub fn ifconfig_probe() -> Box<dyn Fn() -> Option<String> + Send + Sync> {
        Box::new(|| {
            let (ok, out) = crate::sys::run_limited(std::path::Path::new("ifconfig"), &[], &[], Duration::from_secs(3)).ok()?;
            if ok { Some(out) } else { None }
        })
    }

    fn ifaces_at(&self, now: Instant) -> Vec<Iface> {
        let mut snap = self.snap.lock().unwrap_or_else(|e| e.into_inner());
        let stale = snap.as_ref().map_or(true, |s| now.duration_since(s.at) >= REFRESH);
        if stale {
            let ifaces = (self.probe)().map(|t| parse_ifconfig_all(&t)).unwrap_or_default();
            *snap = Some(Snapshot { at: now, ifaces });
        }
        snap.as_ref().map(|s| s.ifaces.clone()).unwrap_or_default()
    }

    pub fn permits(&self, local: IpAddr) -> bool {
        self.permits_at(local, Instant::now())
    }

    /// Loopback always; everything under "any"; otherwise only current addresses of an interface
    /// whose class is listed.
    pub fn permits_at(&self, local: IpAddr, now: Instant) -> bool {
        if local.is_loopback() || self.any {
            return true;
        }
        let IpAddr::V4(v4) = local else { return false };
        self.ifaces_at(now).iter().any(|i| {
            i.v4.contains(&v4) && class_of(i, &self.overrides).map_or(false, |c| self.listen.iter().any(|l| l == c))
        })
    }

    /// Whether `local` would still be allowed under other settings (the lock-out guard).
    pub fn would_permit(&self, local: IpAddr, other: &WebSettings) -> bool {
        if local.is_loopback() || other.any() {
            return true;
        }
        let IpAddr::V4(v4) = local else { return false };
        self.ifaces_at(Instant::now()).iter().any(|i| {
            i.v4.contains(&v4) && class_of(i, &other.interfaces).map_or(false, |c| other.listen.iter().any(|l| l == c))
        })
    }

    /// (class, URL) for every address the server is reachable at, for the UI and GEM.
    pub fn urls(&self, port: u16) -> Vec<(String, String)> {
        let suffix = if port == 80 { String::new() } else { format!(":{port}") };
        let mut out = Vec::new();
        for i in self.ifaces_at(Instant::now()) {
            if let Some(class) = class_of(&i, &self.overrides) {
                if self.any || self.listen.iter().any(|l| l == class) {
                    for a in &i.v4 {
                        out.push((class.to_string(), format!("http://{a}{suffix}")));
                    }
                }
            }
        }
        out
    }

    /// (interface, address) of every up interface, so a user can see what to put in
    /// `interfaces` when the hotspot is not recognised.
    pub fn interfaces_up(&self) -> Vec<(String, String)> {
        self.ifaces_at(Instant::now())
            .into_iter()
            .filter(|i| i.up)
            .flat_map(|i| i.v4.iter().map(|a| (i.name.clone(), a.to_string())).collect::<Vec<_>>())
            .collect()
    }
}
