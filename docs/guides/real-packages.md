# Real packages

These packages in the [`mib2q-plugins`](https://github.com/complex256/mib2q-plugins) repository show
the two package shapes mibpkg supports.

| Product | Package | Shape | What it shows |
|---|---|---|---|
| `mib2q-tailscale` | `tailscale` | native + `[plugin]` | Multiple binaries, optional configs, a service that waits for internet, and an icon-only status item |
| `mib2q-carmqtt` | `carmqtt` | native + `[plugin]` | A required config, a service that waits for the tailnet, and a status item/page |
| `mib2q-wireguard` | `wireguard` | native | A config example and a service with an internet start condition |
| `bike-rack` | `bike-rack` | `[plugin]` | Vehicle permissions and a parking override |
| `hello-menu` | `hello-menu` | `[plugin]` | A Car row, a page and a status item |
| `inputs-demo` | `inputs-demo` | `[plugin]` | Every native input |

## One package, two halves

The Tailscale and carmqtt products ship the native daemon and the SDK status plugin in one `.mpk`.
The package name is the product; the plugin's own id is in `[plugin].id`.

### Tailscale

```toml
[package]
name = "tailscale"
version = "1.102.5"

[[file]]
src = "bin/tailscaled"
mode = "755"

[[service]]
name = "tailscale"
exec = "@pkg/bin/tailscale.sh"
args = ["serve", "@config"]
restart = "always"
after = ["internet"]
after_timeout = "5m"

[plugin]
id = "tailscale-status"
jar = "tailscale-status.jar"
api = 6
permissions = ["network", "ui.slot.statusbar"]
icons = ["tailscale-on", "tailscale-off"]
```

The package's files install under its own package directory; the plugin half installs as
`plugins/tailscale-status.jar`. The service and the status item are one version and one removal.

### carmqtt

```toml
[package]
name = "carmqtt"
version = "0.2.2"

[[file]]
src = "bin/carmqtt"
mode = "755"

[[config]]
path = "carmqtt.toml"
example = "carmqtt.toml.example"
required = true

[[service]]
name = "carmqtt"
exec = "@pkg/bin/carmqtt"
args = ["--config", "@config/carmqtt.toml"]
restart = "always"
after = ["tailnet"]
after_timeout = "5m"

[plugin]
id = "carmqtt-status"
jar = "carmqtt-status.jar"
api = 6
permissions = ["network", "ui.slot.statusbar"]
icons = ["mqtt-on", "mqtt-off"]
```

A required config means the service will not start until `mibpkg/config/carmqtt/carmqtt.toml` exists.
The package page shows the missing config and offers an editor.

## Native only

`wireguard` has no SDK plugin. It is a normal package with one service and a required config:

```toml
[[service]]
name = "mibwg"
exec = "@pkg/bin/mibwg.sh"
args = ["serve", "@config/mibwg.conf"]
restart = "always"
after = ["internet"]
after_timeout = "5m"
```

## Plugin only

The SDK samples are ordinary `[plugin]` packages. `hello-menu` shows the smallest useful page:

```toml
[package]
name = "hello-menu"
version = "1.102.5"

[plugin]
jar = "hello-menu.jar"
api = 6
permissions = ["ui.slot.carmenu", "ui.slot.statusbar"]
icons = ["hello", "hello-20"]
```

See the [SDK docs](https://complex256.github.io/mib2q-plugin-sdk/) for writing the plugin and
[Plugin packages](plugin-packages.md) for installing one.

## Building a product package

A product package is a staging directory with the native payload and the plugin stage:

```text
tailscale-1.102.5.mpk
├── manifest.toml
├── bin/
│   ├── tailscaled
│   ├── tailscale
│   ├── sntp
│   ├── tsstatus
│   └── libtsclock.so
├── authkey.example
├── accept-routes.example
├── tailscale-status.jar
├── tailscale-status.jar.stamp
└── icons/
    ├── tailscale-on.png
    └── tailscale-off.png
```

```sh
mibpkg build tailscale-stage out
mibpkg index out --id my-products --name "My products" --flat
mibpkg verify out
```

The repository's Gradle `packageDaemon` tasks assemble this from the daemon and plugin builds.
