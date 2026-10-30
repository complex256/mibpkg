# mibpkg

**An app store and package manager for the MIB2 High head unit.**

mibpkg installs, updates, removes and rolls back software on the car. It serves a phone-friendly web
interface from the unit itself, and also has an in-car menu. A supervisor starts your services at boot
and restarts them if they die.

![The mibpkg Discover screen](screenshots/discover.png)

## Start here

<div class="grid cards" markdown>

-   **[Install](getting-started/install.md)**

    Put the SD card together and install mibpkg from the engineering menu.

-   **[First use](getting-started/first-use.md)**

    Join the hotspot and open the web interface.

-   **[Packages](guides/packages.md)**

    Install, update, remove and roll back software.

-   **[Your first package](guides/your-first-package.md)**

    Build a `.mpk`, test it, and publish a collection cars can update from.

</div>

## What it does

- **Web interface on the car** — Discover, Updates, Installed and More, from a phone.
- **`.mpk` packages** — a gzip'd tar with a `manifest.toml`, files, services, configs and scripts.
- **One package per product** — a native daemon and its SDK status plugin can ship together.
- **Online sources** — add a collection by `github:owner/repo`; the car checks for updates.
- **Recovery** — atomic installs, backups, rollback, a `DISABLE` card and web-settings recovery.

## Supported units

| | |
|---|---|
| Unit | **MIB2 High**, model `MHI2` or `MHI2Q` |
| OS | QNX 6.5 |
| Package format | `.mpk` |
| Web interface | Hotspot and Tailscale by default |

See [Units](reference/units.md) for compatibility and `[compat]`.

## Status

mibpkg 0.11.1. The docs describe the current `main` branch.
