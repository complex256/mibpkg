# Paths and status

## Paths

| Path | What lives there |
|---|---|
| `/mnt/app/root/mibpkg/` | mibpkg home: packages, state, boot scripts, GEM screens |
| `/mnt/app/root/mibpkg/pkgs/<name>/<version>/` | Installed package files |
| `/mnt/app/root/mibpkg/pkgs/<name>/current` | Active version |
| `/mnt/app/root/mibpkg/state/config/<name>/` | Live config files |
| `/mnt/app/root/mibpkg/state/backups/` | Once-only backups of stock files |
| `/mnt/app/root/mibpkg/boot.d/` | Boot entries |
| `/mnt/app/root/mibpkg/gem/` | Generated GEM screens |
| `/mnt/persist/mibsdk/` | SDK plugin data |
| `/mnt/app/eso/hmi/lsd/mibsdk/` | SDK install root |
| `/mnt/app/eso/hmi/lsd/jars/` | SDK jars |
| `/tmp/mibpkg.log` | Action log (RAM) |
| `/tmp/mibpkg.<name>.<service>.log` | Service stdout/stderr |
| `/tmp/mibpkg.<name>.*` | Runtime markers, uploads, temp files |

`/tmp` is `/dev/shmem`: files only, no directories, and everything is lost on reboot.

## `mibpkg status`

Prints the unit and mibpkg state. It is read-only.

## `mibpkg list`

Lists installed packages with their version and collection.

## Web health

**More → Health** shows:

- the installed mibpkg version;
- the supervisor's version;
- free space;
- running services;
- online-source state.

## Logs

- **More → Logs** — web server and supervisor;
- **Installed → package → Logs** — one package;
- `/tmp/mibpkg.log` — actions;
- `/tmp/mibpkg.<name>.<service>.log` — services.

Logs are capped and volatile.

## Related

- [Package layout](package-layout.md)
- [Config, logs and restart](../guides/config-logs-restart.md)
- [CLI](cli.md)
