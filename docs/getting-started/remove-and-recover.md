# Remove and recover

## Remove a package

Open **Installed**, select the package and choose **Remove**. A plugin's data is kept by default so a
reinstall starts where it left off. Choose **Remove and delete data** to purge it.

From the command line:

```sh
mibpkg remove <name>
mibpkg remove <name> --purge
```

## Remove mibpkg

Use **GEM → mibpkg → Remove mibpkg**, or:

```sh
mibpkg uninstall-self
mibpkg uninstall-self --purge
```

Uninstalling restores the stock files mibpkg backed up. It refuses while packages, plugins or vehicle
overrides are still active; remove those first, or use `--force` only if you understand the risk.

## Stop everything without uninstalling

Create an empty file `mibpkg/DISABLE` on the SD card and reboot:

```sh
mkdir -p /path/to/card/mibpkg
: > /path/to/card/mibpkg/DISABLE
```

The boot hook logs it and starts nothing. Remove the file and reboot to resume.

## Forgotten PIN

**GEM → mibpkg → Remove PIN.** This clears the PIN and restarts the web server.

## Locked out of the web interface

**GEM → mibpkg → Reset web settings.** This deletes `web.toml`, restores the defaults and restarts the
web server.

## A package keeps failing

1. Open **Installed → the package → Logs**.
2. Check **More → Health**.
3. Roll back to the previous version from the package page.
4. If the package will not start, remove it; its data is kept by default.

## Next

- [Recovery and DISABLE](../guides/recovery-disable.md)
- [Troubleshooting](../guides/troubleshooting.md)
- [Safety and recovery](../explanation/safety-and-recovery.md)
