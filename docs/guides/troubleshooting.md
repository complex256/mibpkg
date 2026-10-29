# Troubleshooting

## Install

**The SD-card option is greyed out.**

The card is not FAT32 on a Master Boot Record partition. Reformat it; GUID/GPT is not readable by the
update menu.

**"MOST considered unstable" popup.**

On a bench or a car without a live MOST bus, the unit can flag the bus unstable. mibpkg suppresses it;
a single popup right after install may still appear and should not return.

**The unit keeps returning to the software-update summary.**

Pull the SD card and restart the car. It will boot normally.

**The GEM tile is missing.**

The engineering menu reads its setting at boot. On a fresh install it can take one more normal reboot
for the tile to appear. The web interface is available in the meantime if the unit is reachable.

## Packages

| Symptom | Check |
|---|---|
| Package not listed | Is it in a loaded collection? **More → Sources** and **Rescan** |
| `not compatible with this unit` | The package's `[compat]` does not match the unit |
| `checksum mismatch` | The `.mpk` changed after the index was written; republish it |
| `needs permission approval` | Install with **Install** (web) or `--approve=...`; exit 3 |
| Plugin `BLOCKED_PERMISSIONS` | Grant every declared permission on the plugin page |
| Service does not start | Check the package logs and its `after` conditions |
| `needs config` | Put the file in `mibpkg/config/<name>/` on the card, then **Rescan** |
| Update fails | The previous version is still active; read the job output |

## Web interface

| Symptom | Check |
|---|---|
| Page unreachable | Join the hotspot or Tailscale; check **Web settings** |
| 409 busy | Another action (web or GEM) is running; wait |
| Upload expired | Uploads last 10 minutes; upload again |
| Wrong PIN | **GEM → mibpkg → Remove PIN** |
| Locked out after a settings change | **GEM → mibpkg → Reset web settings** |
| Port changed, no page | Reboot once so the firewall rule is active |

## mibpkg itself

```sh
mibpkg status
mibpkg list
```

**More → Health** shows the installed version, the supervisor's version, disk space and service state.

If the web server will not come back, **GEM → mibpkg → Reset web settings**, or reinstall mibpkg from
the card.

## Getting help

Open an issue with:

- the unit model (`MHI2`/`MHI2Q`) and version line;
- the mibpkg version;
- the package name and version;
- the job output or package log;
- whether the web interface and GEM are reachable.

Remove any personal data before posting logs.

## Related

- [Recovery and DISABLE](recovery-disable.md)
- [Config, logs and restart](config-logs-restart.md)
- [Path and status](../reference/paths-and-status.md)
