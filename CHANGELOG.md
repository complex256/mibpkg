# Changelog

All notable changes to mibpkg. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/); versions follow semver.

## 0.11.2 — 2026-10-29

### Added

- **Per-package rollback policy.** `requires.toml` may carry `[rollback] supported = false`. The web
  interface then hides the rollback action and `mibpkg rollback` refuses, telling the user to remove
  the package and install the older one. This is for packages that wrap a system installer: their
  uninstall script must run before any file switch.

## 0.11.1 — 2026-10-29

### Added

- **Package dependencies on the SDK.** A package may carry `requires.toml` with `[mibsdk] min = "..."`
  (the SDK build writes the version a plugin was compiled against). mibpkg refuses to install a plugin
  on an older SDK, refuses an SDK update or rollback that would be older than an installed plugin's
  requirement, and updates `mibsdk` before plugins in `update-all`. The file is a sidecar rather than a
  manifest field, so older mibpkg releases still install the package and simply ignore the constraint.
- `min_sdk` in collection index entries, filled from `requires.toml`, so the web interface and
  `mibpkg verify` see the same requirement.

## 0.11.0 — 2026-10-29

### Added

- **More → Internet.** The web UI now edits the core mibsdk Wi-Fi uplink: hotspot name, password and
  on/off switch. It writes the same `/mnt/persist/mibsdk/wifi.toml` as the car's
  **Plugins → SDK → Internet** page, so either surface applies within a second. The page chooses DHCP
  or a manual IPv4 address, netmask, gateway and optional DNS; with DHCP on it shows the live lease
  (IP, netmask, gateway, DNS). The password is never returned by the API.
- **A no-uplink banner on Discover** linking to the Internet page when the uplink is on but not
  connected.
- `GET /api/wifi` and `PUT /api/wifi`, plus `wifi` in `GET /api/status`.

## 0.10.0 — 2026-10-28

### Added

- **One package per product.** A `[plugin]` table may now sit alongside `[[file]]`, `[[config]]`,
  `[[service]]` and `[scripts]`, so a native daemon and its SDK status plugin install, update, roll
  back and remove as one `.mpk`.
- **`[plugin].id`.** A product package can carry a plugin whose id differs from the package name
  (`tailscale` carrying `tailscale-status`). The plugin id is used for the jar, stamp, grants, icons,
  data and reload marker.
- **A runnable scaffold.** `mibpkg new` writes an executable sample service, `.gitignore`, a README
  and a release workflow.
- **A documentation site.** README, install guide, guides, reference, architecture and contributing
  pages, published with MkDocs Material.

### Changed

- `mibpkg sdk remove` refuses a plugin that belongs to a combined product package and points at the
  package.
- SDK uninstall reports the product packages that block it.
- The web upload page plans plugin approvals by plugin id.

### Fixed

- The scaffold's sample service is executable, so `mibpkg new` builds without a manual edit.
- Public docs contain no OEM vehicle-brand names.

## 0.9.1

- Optional plugin icon reflections (`<name>.r`).
- SDK package handling, plugin icons, grants and the status-bar preferences.

## 0.9.0

- SDK and plugin packages, icon installation and permission approval.

## 0.8.0

- Plugin SDK package support.

## 0.7.0 and earlier

- Package installs, the supervisor, the web interface, online sources and GEM screens.
