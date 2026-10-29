# Recovery and DISABLE

## Stop everything with DISABLE

Create an empty file:

```text
<mibpkg SD card>/mibpkg/DISABLE
```

Reboot. The boot hook logs it and starts no package services, no supervisor and no web server. The
stock system is unaffected. Remove the file and reboot to resume.

This is the first thing to try if a package is misbehaving.

## GEM recovery buttons

Open **GEM → mibpkg**:

| Button | What it does |
|---|---|
| **Remove PIN** | Clears the web PIN and restarts the web server |
| **Reset web settings** | Deletes `web.toml`, restores defaults and restarts the web server |
| **Remove mibpkg** | Uninstalls mibpkg, restoring stock files |

## Package-level recovery

Open **Installed → package**:

- **Restart** — restart its services;
- **Rollback** — switch to the previous version;
- **Logs** — see why it failed;
- **Remove** — remove it; data is kept unless you choose to purge.

## mibpkg will not start

1. Check the card has `mibpkg/` and that `DISABLE` is not present.
2. Check **More → Health** or `mibpkg status` if the web interface works.
3. Reinstall mibpkg from the card. The installer is idempotent.
4. If the unit is unresponsive, remove the card and reboot; the stock boot path does not need mibpkg.

## Uninstall

**GEM → mibpkg → Remove mibpkg**, or:

```sh
mibpkg uninstall-self
```

Uninstall restores the stock files mibpkg backed up. It refuses while packages, plugins or vehicle
overrides are active; remove those first.

## Related

- [Safety and recovery](../explanation/safety-and-recovery.md)
- [Troubleshooting](troubleshooting.md)
