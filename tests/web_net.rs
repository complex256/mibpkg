//! Which local addresses the web server answers on.
use mibpkg::web::net::{class_of, parse_ifconfig_all, Allowed};
use mibpkg::web::settings::WebSettings;
use std::collections::BTreeMap;
use std::net::{IpAddr, Ipv4Addr};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const UNIT: &str = include_str!("fixtures/ifconfig-all-unit-guess.txt");
const MAC: &str = include_str!("fixtures/ifconfig-all-mac.txt");

fn ip(s: &str) -> IpAddr {
    s.parse().unwrap()
}

#[test]
fn parses_every_interface_with_its_ipv4_addresses() {
    let unit = parse_ifconfig_all(UNIT);
    let names: Vec<&str> = unit.iter().map(|i| i.name.as_str()).collect();
    assert_eq!(names, ["lo0", "ecm0", "uap0", "tun9"]);
    assert_eq!(unit[3].v4, vec![Ipv4Addr::new(100, 64, 0, 55)]);
    assert!(unit.iter().all(|i| i.up));
    let mac = parse_ifconfig_all(MAC);
    assert_eq!(mac[1].name, "en0");
    assert_eq!(mac[1].v4, vec![Ipv4Addr::new(10, 255, 2, 27)]);
    assert!(!mac[2].up, "en5 has no UP flag");
}

#[test]
fn classes_follow_names_and_addresses() {
    let unit = parse_ifconfig_all(UNIT);
    let none = BTreeMap::new();
    let classes: Vec<Option<&str>> = unit.iter().map(|i| class_of(i, &none)).collect();
    assert_eq!(classes, [None, None, Some("hotspot"), Some("tailscale")]);
    // An override names the hotspot interface explicitly (and only it).
    let mut over = BTreeMap::new();
    over.insert("hotspot".to_string(), vec!["ecm0".to_string()]);
    let classes: Vec<Option<&str>> = unit.iter().map(|i| class_of(i, &over)).collect();
    assert_eq!(classes, [None, Some("hotspot"), None, Some("tailscale")]);
}

fn allowed(settings: WebSettings, text: Arc<Mutex<String>>) -> Allowed {
    Allowed::new(&settings, Box::new(move || Some(text.lock().unwrap().clone())))
}

#[test]
fn default_listen_refuses_the_cellular_address() {
    let a = allowed(WebSettings::default(), Arc::new(Mutex::new(UNIT.to_string())));
    let now = Instant::now();
    assert!(a.permits_at(ip("10.255.1.1"), now));
    assert!(a.permits_at(ip("100.64.0.55"), now));
    assert!(a.permits_at(ip("127.0.0.1"), now), "loopback always");
    assert!(!a.permits_at(ip("10.99.0.4"), now), "SIM data link");
    assert!(!a.permits_at(ip("10.255.9.9"), now), "not a current address");
}

#[test]
fn listen_choices_narrow_or_widen() {
    let mut s = WebSettings::default();
    s.listen = vec!["tailscale".into()];
    let a = allowed(s, Arc::new(Mutex::new(UNIT.to_string())));
    assert!(!a.permits_at(ip("10.255.1.1"), Instant::now()));
    assert!(a.permits_at(ip("100.64.0.55"), Instant::now()));
    let mut s = WebSettings::default();
    s.listen = vec!["any".into()];
    let a = allowed(s, Arc::new(Mutex::new(String::new())));
    assert!(a.permits_at(ip("10.99.0.4"), Instant::now()));
}

#[test]
fn an_interface_that_comes_up_later_is_seen_after_the_refresh() {
    let text = Arc::new(Mutex::new(UNIT.replace("tun9", "xxx9").replace("100.64.0.55", "0.0.0.0")));
    let a = allowed(WebSettings::default(), text.clone());
    let t0 = Instant::now();
    assert!(!a.permits_at(ip("100.64.0.55"), t0));
    *text.lock().unwrap() = UNIT.to_string();
    assert!(!a.permits_at(ip("100.64.0.55"), t0 + Duration::from_secs(1)), "cached");
    // Stale: this check still answers from the old snapshot and starts a background refresh...
    assert!(!a.permits_at(ip("100.64.0.55"), t0 + Duration::from_secs(11)));
    std::thread::sleep(Duration::from_millis(200));
    // ...after which the new interface is seen.
    assert!(a.permits_at(ip("100.64.0.55"), t0 + Duration::from_secs(12)));
}

#[test]
fn urls_and_interfaces_up_are_reported() {
    let a = allowed(WebSettings::default(), Arc::new(Mutex::new(UNIT.to_string())));
    let urls = a.urls(80);
    assert!(urls.contains(&("hotspot".to_string(), "http://10.255.1.1".to_string())), "{urls:?}");
    assert!(urls.contains(&("tailscale".to_string(), "http://100.64.0.55".to_string())));
    assert_eq!(a.urls(8080)[0].1, "http://10.255.1.1:8080");
    let up = a.interfaces_up();
    assert!(up.contains(&("ecm0".to_string(), "10.99.0.4".to_string())), "{up:?}");
}

#[test]
fn a_failing_probe_permits_only_loopback() {
    let a = Allowed::new(&WebSettings::default(), Box::new(|| None));
    assert!(a.permits_at(ip("127.0.0.1"), Instant::now()));
    assert!(!a.permits_at(ip("10.255.1.1"), Instant::now()));
}

/// Captured from the car (MHI2Q, 2026-10-05): uap0 is the hotspot, mmx0 the internal vehicle
/// network, ecm0 SIM data, tun9 Tailscale.
const REAL_UNIT: &str = include_str!("fixtures/ifconfig-all-unit.txt");

#[test]
fn on_the_real_unit_only_the_hotspot_and_tailscale_are_classed() {
    let none = BTreeMap::new();
    let classes: Vec<(String, Option<&str>)> =
        parse_ifconfig_all(REAL_UNIT).iter().map(|i| (i.name.clone(), class_of(i, &none))).collect();
    let classed: Vec<(&str, &str)> = classes.iter().filter_map(|(n, c)| c.map(|c| (n.as_str(), c))).collect();
    assert_eq!(classed, [("uap0", "hotspot"), ("tun9", "tailscale")], "{classes:?}");
    let a = allowed(WebSettings::default(), Arc::new(Mutex::new(REAL_UNIT.to_string())));
    assert!(!a.permits_at(ip("10.255.0.15"), Instant::now()), "never the car's internal network (mmx0)");
    assert!(a.permits_at(ip("10.255.1.1"), Instant::now()));
}

#[test]
fn a_slow_ifconfig_never_holds_up_a_request_after_the_first() {
    // ifconfig is a process start on the unit; requests must not wait for it every 10 s.
    let text = Arc::new(Mutex::new(UNIT.to_string()));
    let t2 = text.clone();
    let a = Allowed::new(
        &WebSettings::default(),
        Box::new(move || {
            std::thread::sleep(Duration::from_millis(800));
            Some(t2.lock().unwrap().clone())
        }),
    );
    let t0 = Instant::now();
    assert!(a.permits_at(ip("10.255.1.1"), t0), "the first answer waits for one probe");
    // Stale: answered from the old snapshot at once, refreshed in the background.
    let start = Instant::now();
    assert!(a.permits_at(ip("10.255.1.1"), t0 + Duration::from_secs(11)));
    assert!(start.elapsed() < Duration::from_millis(300), "{:?}", start.elapsed());
}
