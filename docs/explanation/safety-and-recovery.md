# Safety and recovery

mibpkg changes the head unit's software. The design assumes something will go wrong.

## Atomic installs

An install writes a new version directory and switches `current` only after everything is staged,
config is imported, scripts have run and the plugin half (if any) is deployed. Before the switch, a
failure removes the new directory and leaves the old version active.

Updates keep the previous version for rollback.

## Backups

The installer and some actions edit stock files:

- `startup.sh` (the boot hook);
- the bundle list;
- GEM screens;
- firewall config.

mibpkg stores a once-only backup of each file it touches under `state/backups/`. Uninstall restores
them. A backup is written before the first change and never overwritten by a later install.

## Rollback

`mibpkg rollback <name>` switches `current` to the previous version and restores its files, services and
plugin half. A plugin's grants never grow on rollback: a permission that only the old version declares
needs approval.

## Remount protocol

`/mnt/app` is normally read-only. Several processes remount it, so mibpkg uses a holder protocol:

1. Take `/tmp/mibpkg.rwlock` (create-new, pid inside).
2. Create `/tmp/mibpkg.rw.app.<pid>`.
3. `mount -uw /mnt/app`.
4. On release, remove the holder and run `mount -ur` only when no live holder remains.

Other tools that write `/mnt/app` should follow the same protocol. A 0.7 supervisor did not; that is why
mibpkg re-asserts the mount before each write and verifies critical writes by reading them back.

## DISABLE

`mibpkg/DISABLE` on the card stops everything at boot: no supervisor, no services, no web server. It is
the safest way to get a misbehaving unit back to stock behaviour without uninstalling.

## Vehicle overrides

The SDK can hold a vehicle override (for example parking tones). Its ledger restores the prior value
when the plugin is removed, disabled or the ignition goes off. mibpkg refuses to remove the SDK while an
override is pending unless `--force` is used.

## What mibpkg does not do

- It does not sandbox a package. A package runs as root, like the rest of the unit's software.
- It does not protect against a malicious package. Install only sources you trust.
- It does not make a bad firmware update safe. Backups are for mibpkg's own changes.

## Related

- [Recovery and DISABLE](../guides/recovery-disable.md)
- [Package layout](../reference/package-layout.md)
- [Boot and supervisor](boot-and-supervisor.md)
