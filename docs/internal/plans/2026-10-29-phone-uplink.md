# Phone uplink: investigation

Date: 2026-10-29
Status: implemented as core SDK api 7; bench verification interrupted by a unit power loss

## Resolution (2026-10-29)

The helper-daemon design in this file is superseded. The stock firmware already exposes
`org.dsi.ifc.networking.DSIWLAN` with `ROLES_ROLE_AP_CLIENT_COMBINED`, a client connect request and a
scan listener. The feature is **core SDK**, not a package:

- `Plugins → SDK → Internet` in the car and **More → Internet** in mibpkg's web UI both edit
  `/mnt/persist/mibsdk/wifi.toml`.
- `wifi.toml` is `enabled`, `ssid`, `password`; on first run `enabled = true` when no cellular link
  has an address.
- `adapter-hqc` talks to `DSIWLAN`; it never runs a helper process or `wpa_cli`.
- API 7 adds `ctx.wifi()`, `WifiStatus`, `WifiNetwork`, `WifiChanged`, `Features.WIFI`,
  `network.wifi`, and `Ui.textInput(title, initial, maxLen, secret, onDone)`.
- The car's access point stays up; the client is the uplink only.

### Bench results (before the power loss)

| Test | Result |
|---|---|
| AP+STA concurrency | `mlan0` on `bigdata` (10.20.0.23) while `uap0` is an AP at 10.173.189.1 |
| Tools present | `wpa_supplicant`, `/mnt/app/armle/usr/sbin/wpa_cli`, `/usr/sbin/dhcp.client` |
| `bigdata` uplink | default via 10.20.0.1; DNS and `mibpkg online-check` work |
| Manual `beepboop` switch | associate + DHCP (192.168.141.125/16) + DNS + ping 8.8.8.8 (~24 ms) worked |
| `keypanel` route setup | the only thing it does for the uplink is kick `dhcp.client`; no route/PF script |
| `DSIWLAN` registration | the service appears ~8 s after boot; `network.wifi` enters the feature set |
| `wifi.toml` external edit | the SDK saw the change and called the uplink (status file showed `connecting`) |
| SSH after a reboot | **verified**: the SDK's address watcher ran `dhcp.client` and the unit came back at 10.20.0.23 with no RCC DHCP kick |
| Manual IPv4 | **verified**: `dhcp=false`, `ip=10.20.0.222`, netmask `255.255.255.0`, gateway `10.20.0.1` gave SSH, default route and DNS on `.222`; the watchdog restored DHCP afterwards |
| DSI connect | the stock DSI service did not switch `mlan0` to `beepboop` even with `setRole(4)`, a scan and the scanned BSSID. It does deliver `updateRole=4`, `updateProfile` and `updateConnectedNetwork`; the bench's `mlan0` is driven by `wpa_supplicant` directly, so the stock DSI backend may not own it. The adapter now falls back to the stock `wpa_cli` after 8 s without an address. |
| `wpa_cli` fallback | **verified**: `wifi.toml` with `beepboop` -> DSI attempt -> `DSI did not connect; trying wpa_cli` -> `wpa_cli selected beepboop` -> `mlan0 address 192.168.141.125`; SSH to that address, DNS, `ping 8.8.8.8` and `mibpkg online-check` all worked; `uap0` stayed up at 10.173.189.1. |
| Boot-time reconnect | the unit currently runs 0.7.3 on `beepboop`; the next boot will retry the saved SSID after 30 s and use the same fallback. |

The bench unit is currently online on the phone hotspot at `192.168.141.125`; `wifi.toml` keeps `beepboop` with DHCP. The adapter reads the *actual* SSID from `wpa_cli status` (the stock DSI reports `bigdata` as empty), disables further DSI attempts after the first fallback, and clears the old lease before the fallback's DHCP. To return the bench to the lab network, set `ssid = ""` in `wifi.toml` and select the `bigdata` network, or use the car's Internet page.

After the 0.6.5 install/restart the unit stopped answering ping and the RCC serial went silent, with
CAN `usb xmit fail` messages on the Pi. That is the documented bench undervoltage/power weakness; the
unit needs a physical power cycle before the DSI connect test can be repeated.

## Problem

A car without an active SIM data plan still has the car's Wi-Fi hotspot, so the mibpkg web interface
works from a phone. Local installs from the card or a phone file work. Online collections, updates and
`online-check` need an internet uplink.

The desired feature: point the head unit's Wi-Fi client at a phone hotspot and use it as the internet
uplink, with an SDK plugin for status and control.

## What is known

| Fact | Evidence |
|---|---|
| The car hotspot is `uap0` at `10.255.1.1` | unit facts, 2026-10-05 |
| A client Wi-Fi interface `mlan0` exists but has no address while the hotspot is up | `ifconfig` on the unit |
| SIM data is `ecm0`; `ecm1` also exists with no address | `ifconfig` on the unit |
| `dhcp.client` exists in the IFS | `cache/ifs2/.../usr/sbin/dhcp.client` |
| `connectionmanager` supervises uplinks and loads `pf.<uplink>.conf` | unit facts, connectivity JSON |
| `/etc/wpa_action.sh` exists as a stock action hook | start-conditions design |
| The web UI is reachable on the hotspot even without internet | verified on the unit |
| The mibpkg supervisor starts services at boot and can wait for conditions | current supervisor |

Not yet verified on a unit:

- whether `wpa_supplicant`, `wpa_cli`, `wpa_passphrase`, `iw` or `iwconfig` exist and where;
- whether `mlan0` and `uap0` can be up **at the same time** (AP+STA concurrency);
- whether the stock `connectionmanager` owns `mlan0` and overwrites routes or the interface state;
- whether `ecm1` accepts USB tethering from a phone (RNDIS/ECM) and gets DHCP;
- which uplink the stock firewall treats as internet.

## The blocking question: AP+STA concurrency

If the radio cannot run the hotspot and a client connection at the same time, connecting to the phone
hotspot would turn off the car hotspot and the phone would lose the web page it is using to configure
the uplink. That is a UX blocker, not a detail.

Bench test (read-only until the connect step):

1. With the hotspot up, confirm `uap0` has `10.255.1.1` and a phone reaches `http://10.255.1.1/`.
2. Bring up `mlan0`. If `wpa_cli`/`iw` exists, scan for a test AP.
3. Connect `mlan0` to the test AP and obtain DHCP.
4. Check whether `uap0` is still up and the phone still reaches the page.
5. Check `netstat -rn`, `ifconfig`, `net.external`, `pfctl -sr` and the connectionmanager logs.
6. Repeat after a reboot, and with the stock connectionmanager running.

If concurrency fails, test **USB tethering** instead: connect a phone with USB tethering to the unit's
USB port, check for a new interface (`ecm1`/`usb0`), run `dhcp.client` on it, and see whether `uap0`
stays up. USB tethering keeps the hotspot and is likely the better fallback.

## Proposed design A: Wi-Fi client uplink (if AP+STA works)

One combined package, `wifi-uplink`:

```toml
[package]
name = "wifi-uplink"
version = "0.1.0"

[[file]]
src = "bin/mibwifi"
mode = "755"

[[config]]
path = "wifi.toml"
example = "wifi.toml.example"
required = true

[[service]]
name = "wifi-uplink"
exec = "@pkg/bin/mibwifi"
args = ["serve", "@config/wifi.toml"]
restart = "always"
after = ["hmi-ready"]
after_timeout = "2m"

[plugin]
id = "wifi-uplink-status"
jar = "wifi-uplink-status.jar"
api = 6
permissions = ["network", "ui.slot.statusbar", "ui.slot.carmenu"]
icons = ["wifi-on", "wifi-off"]
```

`wifi.toml`:

```toml
ssid = "Phone hotspot"
# Either a passphrase (converted to a PSK) or a precomputed PSK:
password = "…"
# psk = "…"
```

Why config in mibpkg, not in the car UI:

- the mibpkg web interface already edits package configs (`Installed → package → Configs`);
- the phone is already on the car hotspot, so this works before the uplink exists;
- the SDK speller is visible and capped at 32 characters, a poor fit for a Wi-Fi password.

Daemon `mibwifi` (native, Rust, QNX ARMv7):

- reads `wifi.toml`;
- writes a `wpa_supplicant` config or calls `wpa_cli` to join `mlan0`;
- runs `dhcp.client` on `mlan0`;
- verifies internet (DNS or a small HEAD/GET to a configured endpoint);
- keeps the interface up and reconnects on loss;
- exposes a loopback JSON status on `127.0.0.1:8099`:
  - `GET /status` — state, SSID, address, signal, last error;
  - `POST /reconnect` — retry;
  - `POST /forget` — drop the connection.
- never writes `pf.conf`; the stock connectivity manager owns uplink/firewall selection. If a route
  priority change is needed, it is a separate, explicit change.

SDK plugin `wifi-uplink-status`:

- one status item: connected / connecting / off;
- a Car-drawer row or a Plugins-menu page with state, address and **Reconnect** / **Forget**;
- uses `ctx.net()` on loopback (needs `network`; no outbound permission);
- no password entry in the car UI. The config is edited in the mibpkg web interface.

## Fallback design B: USB tethering

If Wi-Fi AP+STA is not supported, a `usb-uplink` package can do the same for a phone's USB tethering
interface. It keeps `uap0` up, so the web interface stays reachable. The plugin/status/config shape is
the same; only the daemon's interface discovery and DHCP differ.

## Risks and open questions

- **AP+STA concurrency** — the main blocker; test first.
- **Tool availability** — no `wpa_supplicant`/`wpa_cli` means writing a supplicant or using a stock
  service; a large dependency. Check the app partition, not only the IFS.
- **connectionmanager ownership** — it may reset `mlan0`, routes or `pf` when it sees a new uplink.
- **Route priority** — SIM (`ecm0`) vs Wi-Fi (`mlan0`). If both are up, which default route wins? The
  user may want Wi-Fi when the phone is present, SIM otherwise.
- **Firewall** — the stock `pf.<uplink>.conf` profiles differ. A new uplink may not have the hotspot
  pass rule, and reloading `pf.conf` at runtime is known to break the stock anchors.
- **Password storage** — a passphrase on the persistent partition is sensitive. Prefer a PSK or a
  file mode 600; document it.
- **Phone hotspot band/channel** — AP+STA concurrency often requires the AP and STA on the same
  channel or band.
- **Boot order** — the phone may not be present at boot; the daemon must retry.
- **Captive portals** — a phone hotspot with a login page is not a usable uplink.
- **Stock UI** — the car may already have a Wi-Fi client screen; use it if it exists rather than
  duplicating it.
- **Security** — a package runs as root. The daemon must only accept loopback control requests, and
  the plugin must not expose the password.

## Experiment checklist

- [ ] `ifconfig`, `netstat -rn`, `pfctl -sr`, `ps` on a unit with no SIM and the hotspot up.
- [ ] Find `wpa_supplicant`, `wpa_cli`, `wpa_passphrase`, `iw`, `iwconfig`, `dhcp.client`.
- [ ] Test Wi-Fi client connect with `uap0` up; check phone reachability.
- [ ] Test USB tethering on `ecm1`/`usb0`; check phone reachability.
- [ ] Check what `connectionmanager` logs and whether it resets the interface.
- [ ] Check route priority with SIM and Wi-Fi both up.
- [ ] Check DNS and TLS through the phone.
- [ ] Check behavior across sleep/wake and reboot.
- [ ] Measure time from connect to first successful HTTPS request.

## Decision

Do not ship this as a plugin until AP+STA concurrency is proven or USB tethering is chosen. A plugin
alone cannot do it: the SDK cannot run `wpa_supplicant` or `dhcp.client`. The package must include a
native daemon, with the SDK plugin as the status/control surface.
