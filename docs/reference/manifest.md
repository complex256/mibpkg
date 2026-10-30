# manifest.toml

Every `.mpk` contains a `manifest.toml`. It describes the package, its files, services, configs and
— for SDK packages — the plugin or SDK metadata.

```toml
[package]
name = "hello"
version = "1.0.0"
description = "One line shown in the web interface"

[compat]
units = ["MHI2Q"]

[[file]]
src = "bin/hello"
mode = "755"

[[service]]
name = "hello"
exec = "@pkg/bin/hello"
restart = "always"
```

## `[package]`

| Field | Required | Rules |
|---|---|---|
| `name` | Yes | 1–40 characters, `[a-z0-9-]`, starting with a letter or digit. Reserved: `restart`, `reload`, `stop`, `selfupdate`, `pid`, `lock`, `status`, `log`, `rw`, `rwlock`, `uploaded` |
| `version` | Yes | Semver (`1.2.3`, `1.2.3-beta.1`). A version is never reused |
| `description` | No | One line |

`mibsdk` is reserved for the SDK package. Only a manifest with `[sdk]` may use it.

## `[compat]`

| Field | Meaning |
|---|---|
| `units` | Unit models that may install the package, e.g. `["MHI2Q"]` |
| `firmware` | Firmware strings the package accepts |

Omit a list to allow everything.

## `[[file]]`

| Field | Meaning |
|---|---|
| `src` | A relative path inside the package |
| `mode` | Octal mode, default `644`; use `755` for programs |

Every `src` must exist in the package. Symlinks and `..` are refused.

Files are installed under the package directory:

```text
/mnt/app/root/mibpkg/pkgs/<name>/<version>/<src>
```

## `[[config]]`

| Field | Meaning |
|---|---|
| `path` | Path below the package's config directory |
| `example` | Optional example file shipped in the package |
| `required` | The service will not start until the file exists |

The live file lives at:

```text
/mnt/app/root/mibpkg/state/config/<name>/<path>
```

A card can supply it at `mibpkg/config/<name>/<path>`. If both the card and the unit changed it,
`config-resolve` or `--config=keep|card` settles the conflict.

## `[[service]]`

| Field | Meaning |
|---|---|
| `name` | 1–40 `[a-z0-9-]`, unique within the package |
| `exec` | Must start with `@pkg/` and name a file in `[[file]]` |
| `args` | Arguments; `@pkg` and `@config` are expanded |
| `phase` | Currently only `default` |
| `restart` | `always` (default), `on-failure`, `never` |
| `after` | Start conditions; all must hold |
| `after_timeout` | Start anyway after `30s`, `5m`, `2h` (1 s–24 h) |

See [Service conditions](service-conditions.md).

## `[scripts]`

| Field | Meaning |
|---|---|
| `post_install` | Runs after the files are staged, before the version is committed |
| `pre_remove` | Runs before removal; a failure leaves the package installed |
| `timeout` | Per-script limit, default `120s`. `30s`, `5m` or `2h` |

Scripts are relative paths inside the package. They run with a timeout. A wrapped system installer
that has its own save/sync wait should raise it, for example `timeout = "5m"`.

## `[plugin]`

A plugin package carries one SDK plugin. It may be plugin-only, or combined with native files,
configs, services and scripts.

| Field | Required | Meaning |
|---|---|---|
| `id` | No | Plugin id; defaults to the package name. 2–40 `[a-z0-9-]` |
| `jar` | Yes | Must be `<id>.jar` |
| `api` | Yes | Newest plugin API level; at least 1 |
| `permissions` | Yes | Permission ids |
| `icons` | No | Icon names, `[a-z0-9-]{1,32}`, at most 8; each needs `icons/<name>.png` |

```toml
[package]
name = "carmqtt"
version = "1.2.0"

[[file]]
src = "bin/carmqtt"
mode = "755"

[[service]]
name = "carmqtt"
exec = "@pkg/bin/carmqtt"
args = ["--config", "@config/carmqtt.toml"]
after = ["tailnet"]

[plugin]
id = "carmqtt-status"
jar = "carmqtt-status.jar"
api = 6
permissions = ["network", "ui.slot.statusbar"]
icons = ["mqtt-on", "mqtt-off"]
```

The plugin jar and its `.stamp` are installed as `plugins/<id>.jar`; the icons as
`icons/<id>/<sha16>.png`. The package name remains the install/update identity.

A plugin may also carry a `requires.toml` at the package root:

```toml
[mibsdk]
min = "0.6.3"
```

This is a dependency on the SDK package, not just an API range. mibpkg refuses to install the
plugin on an older `mibsdk`, and refuses to install or roll back an SDK that is older than an
installed plugin's requirement. `update-all` updates `mibsdk` before the plugins. The file is
separate from `manifest.toml` so older mibpkg releases, which reject unknown manifest fields,
still install the package. Missing `requires.toml` means no version constraint.

A wrapped system installer can disable `mibpkg rollback` because a file switch is not enough for
it; its own uninstall script must run first:

```toml
[rollback]
supported = false
```

The web interface hides the rollback action, and `mibpkg rollback` refuses with instructions to
remove and install the older package instead.

## `[sdk]`

Reserved for the SDK package (`name = "mibsdk"`). It is declarative: no files, services, configs or
scripts.

| Field | Meaning |
|---|---|
| `api`, `min_api` | Plugin API range |
| `hmi` | HMI builds the SDK supports |
| `jars` | SDK jars, names starting `mibsdk-` |
| `stamp` | One stamp for all jars |
| `configurator` | Configurator class |
| `bundle` | Bundle-list line, `<name>=<class>` |
| `tracer_configurator` | DSITracer configurator class |
| `hmicar_stock`, `hmicar` | `HMICar=` line, before/after |
| `hmisystem_stock`, `hmisystem` | Optional `HMISystem=` line, both or neither |
| `owned` | Class paths only the SDK may define |
| `properties` | `sdk.properties` lines |
| `permissions` | Permission catalog file |
| `icons` | Built-in icon names, at most 32 |

## `[[patch]]`

Reserved; not implemented. A package using it is refused with “requires a newer mibpkg”.

## Placeholders

| Placeholder | Expands to |
|---|---|
| `@pkg` | The installed package version directory |
| `@config` | The package's config directory |

## Examples

- [Real packages](../guides/real-packages.md)
- [Your first package](../guides/your-first-package.md)
- [Package layout](package-layout.md)
