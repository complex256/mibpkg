# Changelog

All notable changes to mibpkg. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/); versions follow semver.

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
