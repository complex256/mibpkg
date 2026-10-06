//! The optional PIN: sessions after a correct PIN, a growing lockout after wrong ones, and the
//! Host check that keeps other websites (DNS rebinding) out.
use super::settings::{check_pin, random_id};
use std::collections::HashMap;
use std::net::{IpAddr, Ipv6Addr};
use std::sync::Mutex;
use std::time::{Duration, Instant};

pub const COOKIE: &str = "mibpkg_session";
/// A session unused this long must log in again.
pub const SESSION_IDLE: Duration = Duration::from_secs(7 * 86_400);
/// Wrong PINs allowed before logins are locked.
const FREE_TRIES: u32 = 5;
const FIRST_LOCK: Duration = Duration::from_secs(60);
const MAX_LOCK: Duration = Duration::from_secs(3600);

#[derive(Debug)]
pub enum LoginError {
    Wrong,
    /// Too many wrong PINs: try again after this long.
    Locked(Duration),
}

#[derive(Default)]
struct Inner {
    sessions: HashMap<String, Instant>,
    failures: u32,
    locked_until: Option<Instant>,
}

/// Sessions and the lockout, in memory only (a web restart logs everyone out).
#[derive(Default)]
pub struct Auth {
    inner: Mutex<Inner>,
}

/// The session token in a Cookie header, if any.
fn token_from(cookie_header: Option<&str>) -> Option<&str> {
    cookie_header?.split(';').map(str::trim).find_map(|c| c.strip_prefix(COOKIE)?.strip_prefix('='))
}

/// The Set-Cookie value for a new session.
pub fn session_cookie(token: &str) -> String {
    format!("{COOKIE}={token}; HttpOnly; SameSite=Strict; Path=/; Max-Age={}", SESSION_IDLE.as_secs())
}

impl Auth {
    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Check a PIN; on success return a new session token. `stored` None = no PIN set.
    pub fn login(&self, stored: Option<&str>, pin: &str, now: Instant) -> Result<String, LoginError> {
        let mut g = self.lock();
        if let Some(until) = g.locked_until.filter(|u| *u > now) {
            return Err(LoginError::Locked(until - now));
        }
        let ok = stored.map_or(true, |s| check_pin(s, pin));
        if !ok {
            g.failures += 1;
            if g.failures >= FREE_TRIES {
                // 60 s after the 5th wrong PIN, doubling with each further one, at most an hour.
                let doublings = (g.failures - FREE_TRIES).min(10);
                g.locked_until = Some(now + (FIRST_LOCK * 2u32.pow(doublings)).min(MAX_LOCK));
            }
            return Err(LoginError::Wrong);
        }
        g.failures = 0;
        g.locked_until = None;
        let token = random_id();
        g.sessions.insert(token.clone(), now);
        Ok(token)
    }

    /// Whether the request carries a live session (and refresh its idle timer).
    pub fn check(&self, cookie_header: Option<&str>, now: Instant) -> bool {
        let Some(token) = token_from(cookie_header) else { return false };
        let mut g = self.lock();
        g.sessions.retain(|_, last| now.saturating_duration_since(*last) < SESSION_IDLE);
        match g.sessions.get_mut(token) {
            Some(last) => {
                *last = now;
                true
            }
            None => false,
        }
    }

    pub fn logout(&self, cookie_header: Option<&str>) {
        if let Some(token) = token_from(cookie_header) {
            self.lock().sessions.remove(token);
        }
    }
}

/// The Host header must name an IP address or localhost (with an optional port). A page from
/// another site that points its own domain at the car's address (DNS rebinding) sends its
/// domain here and is refused.
pub fn host_ok(host: Option<&str>) -> bool {
    let Some(host) = host else { return false };
    let name = if let Some(rest) = host.strip_prefix('[') {
        // [v6] or [v6]:port
        let Some((addr, after)) = rest.split_once(']') else { return false };
        if !(after.is_empty() || after.strip_prefix(':').map_or(false, |p| p.parse::<u16>().is_ok())) {
            return false;
        }
        return addr.parse::<Ipv6Addr>().is_ok();
    } else {
        match host.rsplit_once(':') {
            Some((n, port)) => {
                if port.parse::<u16>().is_err() {
                    return false;
                }
                n
            }
            None => host,
        }
    };
    name == "localhost" || name.parse::<IpAddr>().is_ok()
}
