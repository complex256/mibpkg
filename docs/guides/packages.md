# Packages

A package is a `.mpk`: a gzip'd tar with a `manifest.toml` and its payload. mibpkg keeps every
installed version until it needs the space, so a rollback is usually one tap.

## Install

From the web interface: **Discover → package → Install**.

From the command line:

```sh
mibpkg install <name> [collection]
mibpkg install-file <name> <upload-id> [collection|-]
```

`install-file` is what the web upload and the phone flow use: the file is in `/tmp/mibpkg.upload.<id>.mpk`
and is consumed once.

A plugin package asks for its permissions. Without an approval list, mibpkg prints the permissions and
exits **3**:

```sh
mibpkg install tailscale --approve=network,ui.slot.statusbar
```

`update-all` never approves on your behalf.

## Where a package comes from

mibpkg loads collections from:

- the card: `mibpkg/collections/<id>/`;
- the seed collection installed with mibpkg;
- online sources (`github:owner/repo`).

The same package name can exist in several collections. mibpkg prefers the collection it came from on
updates; a fresh install asks when several match.

## Update

```sh
mibpkg update-all
mibpkg install <name>
```

An update keeps the package's config unless both the card and the unit changed it; see
[Config, logs and restart](config-logs-restart.md). A version is never reused: publish a higher version
for every change.

## Roll back

```sh
mibpkg rollback <name>
```

The previous version is kept after an update. Rolling back restores its files, services, config
handling and — for a plugin — its jar, grants and icons. Grants never grow on a rollback; a permission
that only the previous version declares needs approval.

A package that wraps a system installer can disable rollback with `[rollback] supported = false` in
`requires.toml`. For those packages, remove the current package (which runs its uninstall script) and
install the older one instead.

## Remove

```sh
mibpkg remove <name>
mibpkg remove <name> --purge
```

Removal stops the services and deletes the package files. `--purge` also deletes the package's config
and data. For a plugin, data is kept unless purged; the jar, grants line, icon directory and status-line
settings are removed.

## Services and config

A package may declare:

- `[[file]]` — files copied into the package directory;
- `[[config]]` — writable config files under `mibpkg/config/<name>/`;
- `[[service]]` — programs the supervisor starts and restarts;
- `[scripts]` — `post_install` and `pre_remove` hooks.

See [manifest.toml](../reference/manifest.md) and [Service conditions](../reference/service-conditions.md).

## One package per product

A product's native daemon and its SDK status plugin can ship together in one package:

```toml
[package]
name = "tailscale"

[[file]]
src = "bin/tailscaled"
mode = "755"

[[service]]
name = "tailscale"
exec = "@pkg/bin/tailscale.sh"

[plugin]
id = "tailscale-status"
jar = "tailscale-status.jar"
api = 6
permissions = ["network", "ui.slot.statusbar"]
```

Installing `tailscale` installs the service and the status plugin in one transaction. Removing it removes
both. See [Real packages](real-packages.md).

## Next

- [Plugin packages](plugin-packages.md)
- [Online sources](online-sources.md)
- [Troubleshooting](troubleshooting.md)
