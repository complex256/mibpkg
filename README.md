# mibpkg

**An app store for your MIB2 High head unit.** Install, update, remove and roll back software on
an Audi/VW MIB2 (MHI2 / MHI2Q, QNX 6.5) from a phone-friendly web page served by the car — or from
the in-car engineering menu. A built-in supervisor starts your software at boot and keeps it
running.

<p align="center">
  <img src="docs/screenshots/discover.png" width="240" alt="mibpkg Discover screen">
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
    <td align="center"><img src="docs/screenshots/discover.png" width="240" alt="Discover"><br><b>Discover</b><br>Browse what's on the card and online.</td>
    <td align="center"><img src="docs/screenshots/updates.png" width="240" alt="Updates"><br><b>Updates</b><br>One tap to update everything.</td>
    <td align="center"><img src="docs/screenshots/more.png" width="240" alt="More"><br><b>More</b><br>Logs, sources, settings, health.</td>
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
    <td align="center"><img src="docs/screenshots/installed.png" width="240" alt="Installed"><br><b>Installed</b> — what's running, at a glance.</td>
    <td align="center"><img src="docs/screenshots/sources.png" width="240" alt="Sources"><br><b>Sources</b> — add a community package repo.</td>
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

## Plugin SDK packages

From 0.8 mibpkg can install the MIB SDK (`mibsdk`) and the plugins that run on it as ordinary
packages; the `hmisystem` and `icons` keys below need 0.9.
A manifest has one of two extra tables:

- **`[sdk]`** - the SDK package itself (named `mibsdk`; the name is refused for any manifest without
  `[sdk]`). It carries the API range (`api`, `min_api`), the supported `hmi` builds, the `jars`,
  one `stamp`, the configurator, the bundle line and the owned class prefixes. Installing it patches
  the unit's bundle list and writes `sdk.properties`; it is refused on an HMI build it does not list.
  This replaces the `[[patch]]` idea in `docs/ideas.md`.

  The install points `HMICar=` (`hmicar_stock` -> `hmicar`) at the SDK's Car activator. From 0.9 an
  `[sdk]` may also set `hmisystem_stock` and `hmisystem` (both or neither), and `HMISystem=` is then
  managed the same way: rewritten only if it reads exactly `HMISystem=<hmisystem_stock>` (or already
  the SDK value, on an update), its line break kept; refused when it is anything else, missing or
  duplicated. Without the two keys the line is copied byte for byte. Uninstall restores the stock
  bundle list byte for byte, and after a firmware change refuses without `--force` while either line
  still references the SDK.
- **`[plugin]`** - one plugin (`jar`, `api`, `permissions`). The package name is the plugin id:
  2-40 characters, first `[a-z0-9]`, the rest `[a-z0-9-]`. The runtime accepts one character more
  (41), so a 41-character id still loads when pushed by hand but cannot be packaged; this is
  intended. A plugin installs only when `sdk.min_api <= plugin.api <= sdk.api`.

mibpkg 0.7 refuses `[plugin]` and `[sdk]` manifests, so a unit must be on 0.8 first. mibpkg 0.8
refuses a `[sdk]` or `[plugin]` that has `hmisystem_stock`/`hmisystem` or `icons` ("unknown field"),
so such a package needs 0.9.

**Icons.** From 0.9 `[plugin]` and `[sdk]` may list `icons = ["<name>", ...]`: names `[a-z0-9-]{1,32}`,
no duplicates, at most 8 for a plugin and 32 for the SDK's built-in set. Each name needs the package
file `icons/<name>.png` (`mibpkg build` refuses a package without it). At install every icon must
pass the same PNG rules as the SDK build's `PngCheck`, or the install is refused with
`icon <name>: <rule>; nothing changed`:

- at most 32768 bytes, the PNG signature;
- IHDR first and only once: 8-bit, colour type 6 (RGBA) or 4 (grey + alpha), compression, filter
  and interlace 0, width and height 1..72;
- every chunk complete with a correct CRC; no `acTL`/`fcTL`/`fdAT`; IDAT chunks consecutive; IEND
  last, with no byte after it;
- the IDAT data is one complete zlib stream, with nothing after its end, that inflates to exactly
  `height * (1 + width * channels)` bytes (inflation stops at that size + 1), and every row filter
  is 0..4.

They are installed content-addressed: `/mnt/app/eso/hmi/lsd/mibsdk/icons/<plugin-id>/<sha16>.png`
(the SDK's in `.../mibsdk/icons-sdk/`), `<sha16>` being the first 16 lowercase hex characters of
the file's SHA-256, plus `index.properties` with one `<name>=<sha16>,<w>,<h>` line per icon, sorted
by name, LF. An update writes the new files, then the index, then removes the files the index no longer
lists; if the icons cannot be written, the jar, grants line and icons are put back as they were.
`mibpkg remove <plugin>` deletes the plugin's icon directory once the SDK has confirmed the unload
(or did not answer), and a re-run finishes a removal that left only that directory. Removing the
SDK removes `icons-sdk/` and the (then empty) `icons/`. `sdk push` installs the jar only and leaves
icons alone.

**Reflections.** From 0.9.1, for each declared name mibpkg also reads the optional
`icons/<name>.r.png` (the SDK build writes one for every plugin icon taller than 20 px, the drawer's
reflection). It is never listed in `icons = [...]` and does not count toward the 8 / 32 limits. If
present it must pass the same rules (else `icon <name>.r: <rule>; nothing changed`), is installed
content-addressed beside its icon, and adds the index line `<name>.r=<sha16>,<w>,<h>`, sorted with the
rest by key (`hello` < `hello-20` < `hello.r`). If absent, no line is written; a `.r` file for an
undeclared name is ignored. Older runtimes skip the `.r` key, because it is not a valid icon name, so
they keep showing the icon without a reflection; mibpkg 0.9.0 ignores the extra file.

**Approval.** A plugin install or update that adds a permission needs an explicit approval list
that covers it. Without one, `install`, `install-file`, `rollback` and the GEM equivalents list the
permissions and exit **3**; re-run with `--approve=<perm,...>`. `update-all` never approves on your
behalf and refuses `--approve`: update each such plugin with `mibpkg install <pkg> --approve=...`.
Seeding never installs `[sdk]` or `[plugin]` packages. The web interface shows one row per
permission in plain words and asks before installing or updating.

**Revoking.** Runtime enforcement is all-or-nothing today: a plugin whose declared permissions are
not all granted is `BLOCKED_PERMISSIONS` and does not run. So revoking any one permission (web
Revoke, or `mibpkg sdk grant <id> <fewer,perms>`) stops the plugin until the permission is granted
again. Optional permissions (a manifest-level `optional` list the runtime tolerates ungranted) are a
follow-up, not part of 0.8.

**Data and removal.** `mibpkg remove <plugin>` keeps `/mnt/persist/mibsdk/data/<id>.properties`;
`--purge` (alias `--purge-data`) deletes it once the SDK confirms the unload. Removal also drops
the plugin's on/off switch and its status-line settings (hidden flag, place in the saved order),
so a reinstall starts enabled and shown.

**`mibpkg sdk` verbs** (all but `status` need the SDK installed):

| Verb | What it does |
|---|---|
| `sdk status` | the live `HMICar=` and `HMISystem=` lines, SDK, plugins, grants, safe mode and boot state; read-only |
| `sdk grant <id> <perm,...>` | set a plugin's grants (an empty list revokes all) |
| `sdk push <id>` | install a development jar from `/tmp/mibpkg.push.<id>.jar` (+ `.stamp`) |
| `sdk remove <id> [--purge]` | remove a plugin |
| `sdk safe-mode on\|off` | create or delete the `SAFE_MODE` marker |
| `sdk boot-state set-wrap <n>` | set the wrap-boot counter |
| `sdk data-clear <id>` | delete a plugin's data file |

### Other tools that write `/mnt/app`

`/mnt/app` is normally read-only, and several processes remount it. mibpkg shares the remount with
a holder protocol; tools that follow it never flip the mount under each other:

1. Take `/tmp/mibpkg.rwlock` (create it with `create_new`, your pid inside; remove it when done).
2. Create your holder file `/tmp/mibpkg.rw.app.<pid>` (use `system` instead of `app` for
   `/mnt/system`), then `mount -uw /mnt/app`.
3. On release, take the lock, remove your holder file, and run `mount -ur /mnt/app` only if no live
   `mibpkg.rw.app.*` file remains (files of dead pids do not count).

Re-assert `mount -uw` before **each** write and verify the write by reading it back: other processes
remount `/mnt/app` read-only at any time (bench e2e), and a 0.7 supervisor still running during an
upgrade does not honour holders.

## For developers

- `cargo test` — host tests (all unit paths are mapped under a temp root).
- `scripts/build.sh` — cross-build for the unit (Docker, `qnx65-armv7-toolchain:8.5-rust`).
- `scripts/bundle.sh [--signed-header FILE] [extra.mpk ...]` — build the SD card image
  (`out/sd/`, `out/mibpkg-sd-<version>.zip`) with the official collection. One card installs via
  both the engineering menu and M.I.B.; pass a genuine signed metainfo2 header to make the
  engineering-menu entry accepted (see below).
- `mibpkg swdl-card <out> --payload DIR [--seed DIR] --version V [--signed-header FILE] [--no-mib]`
  — assemble the card directly. `--no-mib` emits a pure SWDL card.
- `mibpkg build <src-dir> <out-dir>` — build a `.mpk` from a directory with `manifest.toml` (plain, `[plugin]` or `[sdk]`).
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
