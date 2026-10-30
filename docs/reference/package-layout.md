# Package layout

A package has two layouts: the source directory you build, and the directory mibpkg installs.

## Source

```text
hello/
├── manifest.toml
├── requires.toml
├── bin/
│   └── hello
├── hello.conf.example
├── icons/
│   ├── hello.png
│   └── hello-20.png
├── hello-status.jar
├── hello-status.jar.stamp
├── README.md
└── .github/workflows/mibpkg-release.yml
```

| Path | Purpose |
|---|---|
| `manifest.toml` | Package metadata and payload list |
| `requires.toml` | Optional dependencies and rollback policy, such as the oldest `mibsdk` the plugin runs on |
| `bin/` | Programs copied to the package directory |
| `*.example` | Config examples |
| `icons/` | Plugin icons, declared in `[plugin].icons` |
| `<id>.jar` + `<id>.jar.stamp` | Plugin payload |
| `README.md` | Repository documentation, not installed |

`mibpkg build` includes every regular file in the directory except dotfiles. It refuses symlinks.

## The `.mpk`

```text
hello-1.0.0.mpk                 gzip'd ustar
├── manifest.toml
├── requires.toml
├── bin/hello
├── hello.conf.example
├── hello-status.jar
├── hello-status.jar.stamp
└── icons/hello.png
```

Limits: 128 MiB unpacked, individual paths up to 100 bytes, files only (no symlinks/devices).

## On the unit

| Path | Contents |
|---|---|
| `/mnt/app/root/mibpkg/` | mibpkg home |
| `/mnt/app/root/mibpkg/pkgs/<name>/<version>/` | The installed package files |
| `/mnt/app/root/mibpkg/pkgs/<name>/current` | The active version |
| `/mnt/app/root/mibpkg/state/config/<name>/` | Live config files |
| `/mnt/app/root/mibpkg/state/backups/` | Once-only stock-file backups |
| `/mnt/app/root/mibpkg/state/` | State, boot entries, GEM data |
| `/mnt/app/root/mibpkg/boot.d/` | Boot scripts |
| `/mnt/app/root/mibpkg/gem/` | Generated GEM screens |
| `/mnt/persist/mibsdk/` | SDK plugin data |
| `/tmp/mibpkg.<name>.<service>.log` | Service stdout/stderr |
| `/tmp/mibpkg.log` | Action log (RAM) |

### Product package with an SDK plugin

```text
pkgs/tailscale/1.102.5/
├── manifest.toml
├── bin/tailscaled
├── bin/tailscale
└── ...

/mnt/app/eso/hmi/lsd/mibsdk/
├── plugins/tailscale-status.jar
├── icons/tailscale-status/<sha16>.png
└── grants.properties
```

The native half is under `pkgs/tailscale`; the plugin half is in the SDK's directories under its own
plugin id. Both belong to the `tailscale` package: one version, one removal.

## Versions and rollback

mibpkg keeps the current version and the previous version:

```text
pkgs/tailscale/
├── current              contains "1.102.5"
├── 1.3.0/
└── 1.102.5/
```

Rollback switches `current` and restores that version's files, services and plugin half.

## Related

- [manifest.toml](manifest.md)
- [Paths and status](paths-and-status.md)
- [Safety and recovery](../explanation/safety-and-recovery.md)
