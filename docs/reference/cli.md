# CLI reference

`mibpkg` has host commands (build and publish) and on-unit commands (install, supervise, web).

```text
mibpkg [--root DIR] [--no-supervisor] <command>
```

| Global flag | Meaning |
|---|---|
| `--root DIR` | Operate on a fake root; used by tests and development |
| `--no-supervisor` | Do not start or reload the supervisor after an action |

## Host commands

| Command | What it does |
|---|---|
| `mibpkg build <src-dir> <out-dir>` | Build `<name>-<version>.mpk` from a directory with `manifest.toml` |
| `mibpkg index <collection-dir> --id ID --name NAME [--flat]` | Write `index.json` for a collection |
| `mibpkg release <input>... --repo OWNER/REPO --tag TAG [opts]` | Build a release and optionally publish it |
| `mibpkg verify <source> [--latest] [--unit MHI2Q]` | Check a collection or SWDL card the way a car would |
| `mibpkg new <name> [--dir DIR]` | Scaffold a buildable package |
| `mibpkg swdl-card <out> --payload DIR [--seed DIR] --version V [opts]` | Assemble an SD-card installer |
| `mibpkg gem-patch build --stock JAR --patched JAR [--out DIR]` | Build a GEM patch set |

### `build`

```sh
mibpkg build . out
```

Validates `manifest.toml`, checks every declared file exists, then writes
`out/<name>-<version>.mpk`. It refuses symlinks, unsafe paths and packages over 128 MiB unpacked.

### `index`

```sh
mibpkg index my-collection --id my-packages --name "My packages"
mibpkg index flat-dir --id my-packages --name "My packages" --flat
```

Reads `packages/*.mpk` (or the directory itself with `--flat`), checks names and versions, and writes
`index.json`.

### `release`

```sh
mibpkg release pkg/ other.mpk \
  --repo owner/repo \
  --tag v1.2.0 \
  --id my-packages \
  --name "My packages" \
  --keep 3 \
  --publish
```

| Option | Meaning |
|---|---|
| `--repo` | `owner/repo` (required) |
| `--tag` | Release tag (required) |
| `--id` | Collection id; required on the first release |
| `--name` | Human name |
| `--new-id` | Allow changing the collection id |
| `--keep` | Versions per package to keep (default 3) |
| `--out` | Output directory (default `out/release-<tag>`) |
| `--asset` | Extra file to attach |
| `--publish` | Create the GitHub release |

### `verify`

```sh
mibpkg verify github:owner/repo
mibpkg verify github:owner/repo --latest
mibpkg verify out/release-v1.2.0
mibpkg verify /path/to/card
mibpkg verify --unit MHI2Q github:owner/repo
```

Exits non-zero when a package is missing, a checksum is wrong, a version is invalid or a package is
incompatible with `--unit`.

### `new`

```sh
mibpkg new hello
mibpkg new hello --dir projects/hello
```

Writes a manifest, a runnable sample service, a README, `.gitignore` and a release workflow. See
[Your first package](../guides/your-first-package.md).

### `swdl-card`

```sh
mibpkg swdl-card out/card \
  --payload payload/ \
  --seed seed/ \
  --version 1.0.0 \
  --signed-header header.txt
```

`--no-mib` omits the custom-script path. Without a signed header the card works through the custom-script
method, but the engineering-menu entry will not be accepted.

## On-unit commands

### Package actions

```sh
mibpkg list
mibpkg status
mibpkg install <name> [collection]
mibpkg install-file <name> <upload-id> [collection|-]
mibpkg remove <name> [--purge|--purge-data] [--force]
mibpkg rollback <name> [--approve=<perm,...>]
mibpkg restart <name>
mibpkg rescan
mibpkg update-all
mibpkg config-resolve <name> keep|card [path]
```

| Flag | Meaning |
|---|---|
| `--approve=<perm,...>` | Approve a plugin's permissions |
| `--purge`, `--purge-data` | Delete config and data on removal |
| `--force` | Override a refusal (for example a pending override) |
| `--config=keep\|card` | Settle config conflicts on `install`, `rescan`, `update-all` |

An install or update that needs permission approval exits **3** and lists the permissions.

### SD card and online

```sh
mibpkg rescan
mibpkg online-check
mibpkg sntp [host:port]
```

### Web server

```sh
mibpkg web
```

Runs the web server in the foreground. The supervisor normally runs it as `mibpkg.web`.

### SDK verbs

```sh
mibpkg sdk status
mibpkg sdk grant <plugin-id> <perm,...>
mibpkg sdk push <plugin-id>
mibpkg sdk remove <plugin-id> [--purge]
mibpkg sdk safe-mode on|off
mibpkg sdk boot-state set-wrap <n>
mibpkg sdk data-clear <plugin-id>
```

A bundled plugin cannot be removed with `sdk remove`; remove its product package instead.

### Installer and GEM

```sh
mibpkg bootstrap
mibpkg enable-gem
mibpkg disable-gem
mibpkg gem-patch apply|status|revert [--from DIR]
mibpkg uninstall-self [--purge] [--force]
mibpkg supervise
```

`supervise` is the long-running supervisor; the boot hook starts it.

### GEM actions

The GEM buttons call:

```text
mibpkg gem install|install-file|remove|rollback|restart|self-update|config-set|config-resolve|sdk
mibpkg gem source-add|source-remove|web-settings|web-reset|web-restart|web-clear-pin
```

These record `OK:` or `FAILED:` in the GEM screen. A busy lock shows `BUSY:` and changes nothing.

## Exit codes

| Code | Meaning |
|---|---|
| 0 | Success |
| 1 | Failed, or `verify` found problems |
| 2 | Bad usage |
| 3 | Permissions need approval |

A GEM action exits 0 even when its result is `FAILED:`; the screen carries the result. The web API uses
HTTP status codes and job results.

## Related

- [Packages](../guides/packages.md)
- [Paths and status](paths-and-status.md)
- [Web API](web-api.md)
