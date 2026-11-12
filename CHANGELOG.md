# Changelog

All notable changes to mibpkg. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/); versions follow semver.

## 0.13.1 — 2026-11-11

### Fixed

- **A page the car cannot answer says so and offers Try again**, instead of showing an empty list or
  "everything is up to date": Discover, Updates, Installed, Sources, Web settings, Internet, Logs and
  the config editor.
- **The config editor's result appears under the Save button** (and scrolls into view), not above the
  textarea where the action was.
- **Installing from a file asks for plugin permissions in the same bottom sheet as the package
  page**; it no longer opens an inline block that pushed the Install button around.
- The installed list no longer repeats the update version beside a button that already says Update.
- A service row folds its Logs/Restart actions under the name at 320px instead of truncating both.

### Changed

- Bottom sheets rise from the edge with a short animation (off for reduced motion).
- Light mode: the primary blue is darker (link and tab-label contrast), and an off switch has a
  visible track.
- Links show a keyboard focus ring; small buttons grew to 36px; list rows respond to a tap.

## 0.13.0 — 2026-11-10

### Changed

- **The web interface is rebuilt on React, Tailwind and shadcn/ui.** The views under `web/src/` are
  React + TypeScript components with shadcn primitives (`Button`, `Card`, `Switch`, `Sheet`,
  `Collapsible`, `DropdownMenu`, `Badge`, `Alert`) and Lucide icons, bundled by Vite into
  `web/dist` and embedded in the binary as before. A checkout still builds with plain `cargo` —
  the built bundle is committed, and `scripts/bundle.sh` rebuilds it when npm is available. No
  request leaves the car: fonts, icons, styles and scripts all ship inside the binary.
- **The package page fits one phone screen.** A header card carries the icon, description, version,
  size and source with the main action; a merged **Service & configuration** card holds each
  service (state, logs, restart), each config file (missing/conflict, edit) and one **Permissions**
  row; permissions, removal and rollout approvals open in bottom sheets; and source, offers,
  scripts and last result fold into **Technical details**. Roll back and Remove live under a
  `•••` menu.
- **Discover stops repeating the installed list.** While browsing it shows only what can be
  installed; the search box covers installed packages too. With nothing to install it offers
  **Check for updates** and **Install from file**.
- **Updates focuses on pending work**: pending updates and **Update all** first, with
  **Up to date (N)** collapsed.
- **Web settings hides its raw network detail.** Interfaces and the manual hotspot-interface
  override sit in **Advanced network interfaces**, open only when no address is recognised or an
  override is already set.
- **Remove mibpkg** is a bottom sheet with the acknowledgement switch inside it.
- Touch targets, press feedback, keyboard focus and the dark/light palette follow the shadcn
  tokens; the tab bar is hidden on the PIN pad.

### Fixed

- The package page goes back to the tab it was opened from (`‹ Updates`, `‹ Installed`, …).
- Sources shows its job output above the lists, and every action's button spins (or greys out)
  while the car takes it.

## 0.12.3 — 2026-11-10

### Changed

- **The `More → Internet` page looks like the rest of the interface.** It has a `‹ More` back link;
  its text fields and checkboxes use the same `.field` and `.choice` rows as Web settings and the
  permission sheets, and a connected uplink is shown in a raised card with its live addresses.
- **The PIN pad is not a place to navigate from.** The tab bar is hidden while the page is locked,
  and the empty PIN field shows dim placeholder dots instead of a lone dash.
- **Touch and layout polish.** Larger tap areas for pills and round buttons, press feedback on rows,
  buttons and the PIN pad, visible keyboard focus, labels that no longer break mid-word in the
  information lists, and a package action pill that tolerates two lines.

### Fixed

- **The package page goes back where it came from.** A package opened from **Updates** or
  **Installed** now shows `‹ Updates` / `‹ Installed` instead of always `‹ Discover`, matching the
  tab that stays highlighted.
- **A change started in Sources, Updates or More is visible where it was tapped.** The job output on
  Sources is drawn above the lists, and the button that started a job spins (or greys out) while the
  car takes it: Sources, Check for updates, Update all, Rescan SD card, Save and restart, and the
  config-conflict choices. A second tap can no longer start the same action twice.

## 0.12.2 — 2026-11-10

### Changed

- **A package action in progress is visible on the package page.** The page's main button becomes a
  disabled spinner (App Store style) while a job for that package runs — including one started from a
  row in Discover, Updates or Installed — and every other action on the page waits for it to finish.
  **Update** and **Get** on a row show the spinner while the car takes the request. A plugin update
  that asks for a new permission now opens the package page instead of starting, so its approval sheet
  is shown.

## 0.12.1 — 2026-11-10

### Fixed

- **Coexistence of `mibsdk` and `mibpkg`-managed `carplay-rgi`, while refusing unmanaged `carplay-rgi` installs.**
  `sdk_pkg::check` now allows installing and updating `mibsdk` when `carplay-rgi` is installed and managed by
  `mibpkg` (`check_owned_classes` and `bundles::check_line` still enforce no class or bundle-line overlap),
  while continuing to refuse when an unmanaged (non-`mibpkg`) `carplay-rgi` (`*carplay*` jar in `lsd/jars` or
  `*.carplay-stock` files under `/mnt/app/eso/hmi`) is present. Installing the `carplay-rgi` package through
  `mibpkg` now also refuses if an unmanaged `carplay-rgi` is already on the unit.

## 0.12.0 — 2026-11-09

### Added

- **`mibpkg sdk catalog`**, and the snapshot it writes: `/tmp/mibpkg.catalog`. Every package action and
  every supervised boot rewrite it too. It is what the SDK's Plugins menu (Installed, Updates, Available)
  reads, so the car can show what is installed and offered, and install or update a package from the MMI,
  without the SDK talking to mibpkg.

## 0.11.4 — 2026-11-08

### Fixed

- **Permissions on a combined product package.** The web interface's permission page for `tailscale` (a
  package that carries the plugin `tailscale-status`) showed every permission as revoked, and saving it was
  refused with "tailscale is not a plugin package; only plugins have permissions". `grants.properties` is
  keyed by plugin id, so the catalog now reads the grants for the manifest's `[plugin].id`, and
  `mibpkg sdk grant` accepts a plugin id or the name of an installed package that carries one, writing the
  line for the plugin. `mibpkg sdk data-clear` follows the same id, and `mibpkg sdk status` lists a combined
  package's plugin under its plugin id.

## 0.11.3 — 2026-10-29

### Added

- **Per-package script timeout.** `[scripts] timeout = "5m"` raises the limit for a
  `post_install` or `pre_remove` script. The default stays 120 s. Wrapped system installers that do
  their own save/sync wait need the larger limit; the bench CarPlay uninstaller exceeded 120 s on
  the unit's flash.

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
