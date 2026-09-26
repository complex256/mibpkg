# mibpkg

Package manager for MIB2 High head units (MHI2 / MHI2Q, QNX 6.5). Install, update and remove
software from an SD card using the green engineering menu (GEM); a supervisor starts and
watches the installed services.

## For users

1. Unzip `mibpkg-sd-<version>.zip` onto the M.I.B. SD card (FAT32).
2. If a package needs a config file (e.g. carmqtt), put it on the card at
   `mibpkg/config/<package>/<file>` (an example ships in the package).
3. GEM -> M.I.B. -> Advanced Settings -> Run Custom Script. This installs mibpkg once.
4. GEM -> mibpkg: install, update, remove, restart, roll back.

Emergency stop: create an empty file `mibpkg/DISABLE` on the card and reboot; no package
services start while it is present.

## For developers

- `cargo test` — host tests (all unit paths are mapped under a temp root).
- `scripts/build.sh` — cross-build for the unit (Docker, `qnx65-armv7-toolchain:8.5-rust`).
- `mibpkg build <src-dir> <out-dir>` — build a `.mpk` from a directory with `manifest.toml`.
- `mibpkg index <collection-dir> --id ID --name NAME` — write `index.json`.
- `scripts/bundle.sh [extra.mpk ...]` — SD bundle with the official collection.

Design: `docs/superpowers/specs/2026-09-26-mibpkg-design.md`. Backlog: `docs/ideas.md`.
Verified unit facts: `docs/unit-facts.md`.
