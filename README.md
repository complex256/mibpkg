# mibpkg

**An app store for the MIB2 High head unit.** Install, update, remove and roll back software from a
phone-friendly web page served by the car, or from the in-car engineering menu. A built-in supervisor
starts your software at boot and keeps it running.

<p align="center">
  <img src="docs/screenshots/discover.png" width="240" alt="mibpkg Discover screen">
</p>

- **No PC at the car.** Everything runs on the unit; you drive it from your phone's browser.
- **One SD card, two install methods** — the engineering menu, or the custom-script method.
- **Offline-first**, with optional online updates when the car has internet.
- **Safe by design** — atomic installs with backups, one-tap rollback, a `DISABLE` card, and a web
  interface locked to the car's hotspot and Tailscale by default.

New here? **[Install guide →](https://complex256.github.io/mibpkg/getting-started/install/)**

## What it looks like

<table>
  <tr>
    <td align="center"><img src="docs/screenshots/discover.png" width="240" alt="Discover"><br><b>Discover</b><br>Browse what's on the card and online.</td>
    <td align="center"><img src="docs/screenshots/updates.png" width="240" alt="Updates"><br><b>Updates</b><br>One tap to update everything.</td>
    <td align="center"><img src="docs/screenshots/more.png" width="240" alt="More"><br><b>More</b><br>Logs, sources, settings, health.</td>
  </tr>
</table>

## Quick start

1. Download `mibpkg-sd-<version>.zip`.
2. Unzip it to the **root** of a FAT32 SD card partitioned with **Master Boot Record**.
3. Insert the card in **SD1**, open the engineering menu → **Update → Standard**, select **mibpkg**,
   and start.
4. When the unit settles, join its Wi-Fi hotspot and open **GEM → mibpkg** for the web address.

The full walkthrough, including the custom-script method, is in
[Getting started](https://complex256.github.io/mibpkg/getting-started/).

## Features

- **Web interface served from the car** — search, install, update, remove, restart and roll back
  from a phone.
- **Install a `.mpk` from a phone**, from the SD card, or from an online source.
- **Online collections** — add a package repository by `github:owner/repo`; the car checks for
  updates when it has internet.
- **Boot supervisor** — services start at boot in the right order, wait for the network, Tailscale,
  the HMI, a file or a route, and restart if they die.
- **One package per product** — a product's native service and its SDK status plugin ship together
  as one `.mpk`.
- **Recoverable** — once-only backups of every stock file it touches, one-tap rollback, a `DISABLE`
  card that stops everything, and recovery buttons for a forgotten PIN or broken settings.

## Supported units

| | |
|---|---|
| Unit | **MIB2 High**, model `MHI2` or `MHI2Q` |
| OS | QNX 6.5 |
| Package format | `.mpk` (gzip'd tar with `manifest.toml`) |
| Web interface | Hotspot and Tailscale by default |

The engineering menu's **Version information** screen shows the model. The line starts with `MHI2`
or `MHI2Q` on a supported unit. Other MIB2 variants and other platforms are not supported.

## The web interface

1. Join the car's Wi-Fi hotspot, or reach it over Tailscale.
2. **GEM → mibpkg** shows the address, for example `http://10.255.1.1`.
3. Open it in a phone browser.

The interface has four tabs:

| Tab | What it does |
|---|---|
| **Discover** | Browse the card and online collections; install a package |
| **Updates** | See available updates and update everything |
| **Installed** | Inspect, configure, restart, roll back or remove packages |
| **More** | Install from file, logs, sources, web settings, health |

By default it answers only on the hotspot and Tailscale. **More → Web settings** can change that and
set a PIN. Forgot the PIN? **GEM → mibpkg → Remove PIN.**

**Emergency stop:** create an empty file `mibpkg/DISABLE` on the card and reboot. No package services
start while it is present.

## Documentation

| Start here | |
|---|---|
| [Getting started](https://complex256.github.io/mibpkg/getting-started/) | Install and first use |
| [Web interface](https://complex256.github.io/mibpkg/guides/web-interface/) | Every screen and action |
| [Packages](https://complex256.github.io/mibpkg/guides/packages/) | Install, update, remove, roll back |
| [Your first package](https://complex256.github.io/mibpkg/guides/your-first-package/) | Build and publish a `.mpk` |
| [Real packages](https://complex256.github.io/mibpkg/guides/real-packages/) | Tailscale, carmqtt, wireguard and the SDK samples |
| [CLI reference](https://complex256.github.io/mibpkg/reference/cli/) | Every host and on-unit command |
| [manifest.toml](https://complex256.github.io/mibpkg/reference/manifest/) | Every table and rule |
| [Architecture](https://complex256.github.io/mibpkg/explanation/architecture/) | How the pieces fit together |

## For developers

```sh
cargo test                 # host tests
scripts/build.sh           # cross-build for the unit (Docker)
scripts/bundle.sh          # build the SD-card image
```

Build a package:

```sh
mibpkg new mytool
mibpkg build mytool out
```

Publish a collection:

```sh
mibpkg release . --repo <owner>/<repo> --tag v1.0.0 --id <collection-id> --name "<name>" --publish
```

See [Contributing](https://complex256.github.io/mibpkg/contributing/) for the full build, test and
release flow.

## License

Version 0.10.0. Licensed under MIT. Nothing here is affiliated with or endorsed by the vehicle or
head-unit manufacturers.
