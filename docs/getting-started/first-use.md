# First use

## Open the web interface

1. Join the car's Wi-Fi hotspot from your phone, or reach the car over Tailscale.
2. In the car, open **GEM → mibpkg**. It shows the address, for example `http://10.255.1.1`.
3. Open that address in your phone's browser.

![Discover](../screenshots/discover.png)

The interface has four tabs:

| Tab | What it does |
|---|---|
| **Discover** | Browse packages on the card and online collections |
| **Updates** | See available updates and update everything |
| **Installed** | Inspect, configure, restart, roll back or remove packages |
| **More** | Install from file, logs, sources, web settings, health |

!!! note "No SIM data?"
    The car's hotspot still works, so you can install packages from the card or a file. Online sources
    and updates need an internet uplink. A phone-hotspot or USB-tethering uplink is under investigation
    and is not part of 0.10.0.

## Install a package

1. Open **Discover**.
2. Tap a package.
3. Tap **Install**.
4. Approve any permissions the package asks for.

A plugin package's permissions are shown in plain words. A package that adds a permission needs
explicit approval.

## Install a `.mpk` from your phone

1. Open **More → Install from file**.
2. Choose the `.mpk` file.
3. Review the package, its compatibility and any permissions.
4. Install.

Uploads expire after 10 minutes if you do not install them.

## Update everything

Open **Updates → Update all**. A plugin update that adds a permission is not approved for you; install
that plugin individually with **Install** or **Update** so you can review the new permission.

## Set a PIN

Open **More → Web settings**. A PIN is 4 to 12 digits. If you forget it, use
**GEM → mibpkg → Remove PIN**. If the web settings lock you out, use **GEM → mibpkg → Reset web
settings**.

## Emergency stop

Create an empty file `mibpkg/DISABLE` on the SD card and reboot. No package services start while it is
present. Remove it and reboot to resume.

## Next

- [Packages](../guides/packages.md)
- [Web interface](../guides/web-interface.md)
- [Recovery and DISABLE](../guides/recovery-disable.md)
