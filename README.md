# mibpkg

Package manager for MIB2 High head units (MHI2 / MHI2Q, QNX 6.5). Install, update and remove
software from an SD card using the green engineering menu (GEM); a supervisor starts and
watches the installed services.

## For users

1. Unzip `mibpkg-sd-<version>.zip` onto the M.I.B. SD card (FAT32).
2. If a package needs a config file (e.g. carmqtt), put it on the card at
   `mibpkg/config/<package>/<file>` (an example ships in the package). Changes made later in
   the web interface are kept: a card file is only imported again once you change it on the card.
3. GEM -> M.I.B. -> Advanced Settings -> Run Custom Script. This installs mibpkg once.
4. Open the web interface (below) to install, update, remove, restart and roll back.
   GEM -> mibpkg shows its address and has the recovery buttons.

### Install from the engineering menu (no M.I.B.)

mibpkg can also install itself directly from the Red/Engineering menu's SW-Update flow, with
no M.I.B. on the unit. Build a card and write it to a FAT32 SD:

```
mibpkg swdl-card out/card --payload <dir with mibpkg, launch.sh, gem.sh, ca.pem> --version X.Y
# optional: --seed <collection-dir> to auto-install a package set on first boot
mibpkg verify out/card        # checks the manifest hash, payload and patches.toml
```

Write `out/card` to the SD, insert in **SD1**, then Red/Engineering menu -> SW Update -> select the
SD -> "mibpkg installer vX.Y" -> START. The unit reboots a few times and comes up with mibpkg
running; open the web interface (below). On units whose stock GEM renders it, the in-car
`mibpkg` picker also appears — this part is best-effort (it enables the engineering menu and, where
a matching jar patch ships, patches the GEM jar); if it can't, the install still completes and the
web interface is the primary interface.

### Web interface

mibpkg also serves an app-store-style page from the car, with everything GEM offers plus
installing a `.mpk` from your phone, logs, config files and online sources.

1. Join the car's Wi-Fi hotspot (or reach the car over Tailscale).
2. GEM -> mibpkg shows the address, e.g. `http://10.255.1.1`.
3. Open it in the phone's browser.

By default it answers only on the hotspot and Tailscale (never the SIM data link); Web settings
can change that and set a PIN. Forgot the PIN? GEM -> mibpkg -> Remove PIN. Locked out
another way? GEM -> mibpkg -> Reset web settings.

Emergency stop: create an empty file `mibpkg/DISABLE` on the card and reboot; no package
services start while it is present.

## For developers

- `cargo test` — host tests (all unit paths are mapped under a temp root).
- `scripts/build.sh` — cross-build for the unit (Docker, `qnx65-armv7-toolchain:8.5-rust`).
- `mibpkg build <src-dir> <out-dir>` — build a `.mpk` from a directory with `manifest.toml`.
- `mibpkg index <collection-dir> --id ID --name NAME` — write `index.json`.
- `scripts/bundle.sh [extra.mpk ...]` — SD bundle with the official collection.
- `scripts/web-fixture.sh <dir>` — a fake unit root for working on the web interface on a Mac
  (`web/` is plain HTML/CSS/JS, no build step; `MIBPKG_WEB_DIR=web` serves it from disk).
- `node --test web/tests/*.test.mjs` — front-end unit tests (Node's built-in runner, nothing to install).

Design: `docs/superpowers/specs/2026-09-26-mibpkg-design.md`; web interface:
`docs/superpowers/specs/2026-10-05-web-interface-design.md`. Backlog: `docs/ideas.md`.
Verified unit facts: `docs/unit-facts.md`.
