# Service conditions

A service can wait for a condition before it starts.

```toml
[[service]]
name = "carmqtt"
exec = "@pkg/bin/carmqtt"
args = ["--config", "@config/carmqtt.toml"]
restart = "always"
after = ["tailnet"]
after_timeout = "5m"
```

## Aliases

| Alias | Default meaning |
|---|---|
| `internet` | A default route exists |
| `tailnet` | A route to `100.100.100.100` through `tun*` |
| `hmi-ready` | `/tmp/smFlagStartupDone` exists |

`settings.toml` `[conditions]` can override an alias per unit:

```toml
[conditions]
hmi-ready = "file:/tmp/startupFinishedFlag"
```

## Raw checks

| Form | Meaning |
|---|---|
| `file:/absolute/path` | The file exists |
| `iface:<name>` | The interface is up and has an address |
| `route:<dest>[@<iface prefix>]` | A route to `dest` exists, optionally through an interface |

Examples:

```toml
after = ["file:/mnt/persist/ready", "iface:uap0"]
after = ["route:100.100.100.100@tun"]
after = ["route:192.168.1.0"]
```

Interface names are 1–15 characters of `[a-z0-9]`.

## Timeout

`after_timeout = "30s"`, `"5m"` or `"2h"` starts the service anyway after that time. The range is 1 second
to 24 hours. Without it, a service waits indefinitely.

## Restart policy

| Value | Meaning |
|---|---|
| `always` | Restart whenever it exits (default) |
| `on-failure` | Restart only on a non-zero exit |
| `never` | Do not restart |

## Phases

Only the `default` phase is implemented. A service with another phase is refused with “requires a newer
mibpkg”.

## Related

- [manifest.toml](manifest.md)
- [Boot and supervisor](../explanation/boot-and-supervisor.md)
