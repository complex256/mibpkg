# mibpkg

**An app store for your MIB2 High head unit.** Install, update, remove and roll back software on
an Audi/VW MIB2 (MHI2 / MHI2Q, QNX 6.5) from a phone-friendly web page served by the car — or from
the in-car engineering menu. A built-in supervisor starts your software at boot and keeps it
running.

<p align="center">
  <img src="docs/screenshots/discover.png" width="300" alt="mibpkg Discover screen">
</p>

- **No PC at the car.** Everything runs on the unit; you drive it from your phone's browser.
- **One SD card, two ways to install** — the Red/Engineering menu (no M.I.B. required) *or* M.I.B.
  "Run Custom Script".
- **Offline-first**, with optional online updates for cars that have internet.
- **Safe by design** — atomic installs with backups, one-tap rollback, a DISABLE card, and a web
  interface locked to the car's hotspot and Tailscale by default.

New here? **[Follow the step-by-step install guide → INSTALL.md](INSTALL.md)**

## What it looks like

<table>
  <tr>
    <td align="center"><img src="docs/screenshots/discover.png" width="230" alt="Discover"><br><b>Discover</b><br>Browse what's on the card and online.</td>
    <td align="center"><img src="docs/screenshots/updates.png" width="230" alt="Updates"><br><b>Updates</b><br>One tap to update everything.</td>
    <td align="center"><img src="docs/screenshots/more.png" width="230" alt="More"><br><b>More</b><br>Logs, sources, settings, health.</td>
  </tr>
</table>

## Features

- **App-store web interface** served from the car — search, install, update, remove, restart and
  roll back, all from your phone.
- **Install a `.mpk` straight from your phone**, or from the SD card, or from an online source.
- **Online sources** — add a community package repo by `github:owner/repo`; the car checks for
  updates when it has internet.
- **Boot supervisor** — your packages start at boot, in the right order (a package can wait for the
  network, Tailscale, the HMI, or a file), and get restarted if they die.
- **Multiple install methods from one card** — the engineering-menu SWDL flow and M.I.B. "Run
  Custom Script", built by a single command.
- **Recoverable** — once-only backups of every stock file it touches, one-tap rollback, a `DISABLE`
  card that stops everything, and GEM recovery buttons for a forgotten PIN or broken settings.

## Install

mibpkg ships as a single SD-card image (`mibpkg-sd-<version>.zip`) that installs **either** way:

1. **Engineering menu (no M.I.B. needed)** — unzip onto a FAT32 (MBR) SD, insert in **SD1**, then
   **Red/Engineering menu → SW Update → "mibpkg installer" → START.** The unit reboots and comes up
   running mibpkg.
2. **M.I.B. "Run Custom Script"** — unzip onto your M.I.B. SD, then **GEM → M.I.B. → Advanced
   Settings → Run Custom Script.**

Either way, open **GEM → mibpkg** (or the web interface below) when it's done.

➡️ **The full, illustrated, novice-friendly walkthrough is in [INSTALL.md](INSTALL.md).**

## The web interface

mibpkg serves an app-store-style page from the car with everything the in-car menu offers plus
installing a `.mpk` from your phone, live logs, config files and online sources.

<table>
  <tr>
    <td align="center"><img src="docs/screenshots/installed.png" width="230" alt="Installed"><br><b>Installed</b> — what's running, at a glance.</td>
    <td align="center"><img src="docs/screenshots/sources.png" width="230" alt="Sources"><br><b>Sources</b> — add a community package repo.</td>
  </tr>
</table>

1. Join the car's Wi-Fi hotspot (or reach the car over Tailscale).
2. **GEM → mibpkg** shows the address, e.g. `http://10.255.1.1`.
3. Open it in your phone's browser.

By default it answers only on the hotspot and Tailscale (never the SIM data link); **Web settings**
can change that and set a PIN. Forgot the PIN? **GEM → mibpkg → Remove PIN.** Locked out another
way? **GEM → mibpkg → Reset web settings.**

**Emergency stop:** create an empty file `mibpkg/DISABLE` on the card and reboot — no package
services start while it is present.

## For developers

- `cargo test` — host tests (all unit paths are mapped under a temp root).
- `scripts/build.sh` — cross-build for the unit (Docker, `qnx65-armv7-toolchain:8.5-rust`).
- `scripts/bundle.sh [--signed-header FILE] [extra.mpk ...]` — build the SD card image
  (`out/sd/`, `out/mibpkg-sd-<version>.zip`) with the official collection. One card installs via
  both the engineering menu and M.I.B.; pass a genuine signed metainfo2 header to make the
  engineering-menu entry accepted (see below).
- `mibpkg swdl-card <out> --payload DIR [--seed DIR] --version V [--signed-header FILE] [--no-mib]`
  — assemble the card directly. `--no-mib` emits a pure SWDL card.
- `mibpkg build <src-dir> <out-dir>` — build a `.mpk` from a directory with `manifest.toml`.
- `mibpkg index <collection-dir> --id ID --name NAME` — write `index.json`.
- `mibpkg verify <card-dir | github:owner/repo | index.json>` — check a card or a collection the
  way the car would.
- `scripts/web-fixture.sh <dir>` — a fake unit root for working on the web interface on a Mac
  (`web/` is plain HTML/CSS/JS, no build step; `MIBPKG_WEB_DIR=web` serves it from disk).
- `node --test web/tests/*.test.mjs` — front-end unit tests (Node's built-in runner).

**The signed header:** the engineering-menu (SWDL) flow requires a genuine, Audi-signed `metainfo2`
header prepended to the card's manifest — the signatures can't be forged, so mibpkg does not ship
one. Supply your own (from a real update you own) at build time with `--signed-header`; it is never
committed. Without it the M.I.B. path still works. See [INSTALL.md](INSTALL.md) for details.

Design: `docs/superpowers/specs/2026-09-26-mibpkg-design.md`; web interface:
`docs/superpowers/specs/2026-10-05-web-interface-design.md`; SWDL installer:
`docs/superpowers/specs/2026-10-14-swdl-installer-design.md`. Backlog: `docs/ideas.md`.
Verified unit facts: `docs/unit-facts.md`.
