# Boot and supervisor

## Boot

The card installer adds a hook to `startup.sh`. On boot it:

1. checks for `mibpkg/DISABLE`;
2. starts the supervisor in the background;
3. the supervisor starts the web server and the installed services.

The stock boot path still runs; mibpkg is an add-on, not a replacement.

## Supervisor

The supervisor:

- reads the installed packages and their `[[service]]` entries;
- waits for each service's `after` conditions;
- starts services in the package order;
- restarts them according to `restart`;
- requests a reload when a package is installed, updated or removed;
- runs the web server as `mibpkg.web`;
- rewrites GEM screens and the status values.

It is a long-running process. `mibpkg supervise` runs it in the foreground.

## Start conditions

Common conditions:

| Condition | Waits for |
|---|---|
| `internet` | A default route |
| `tailnet` | A route to `100.100.100.100` through `tun*` |
| `hmi-ready` | The HMI startup marker |
| `file:/path` | A file |
| `iface:name` | An interface with an address |
| `route:dest` | A route |

`after_timeout` starts the service anyway after the given time. See
[Service conditions](../reference/service-conditions.md).

## Restarts

| Policy | Behaviour |
|---|---|
| `always` | Restart whenever it exits |
| `on-failure` | Restart on a non-zero exit |
| `never` | Do not restart |

A restart is requested through a marker in `/tmp`; the supervisor picks it up within a few seconds.

## Logs

Service stdout/stderr goes to `/tmp/mibpkg.<package>.<service>.log`. The web page tails it. `/tmp` is
RAM, so logs are capped and do not survive a reboot.

## If a service will not start

1. Check the package's config: a required file may be missing.
2. Check its `after` conditions: the network or tailnet may not be up yet.
3. Read its log.
4. Restart it from the package page or `mibpkg restart <name>`.

## Related

- [Service conditions](../reference/service-conditions.md)
- [Config, logs and restart](../guides/config-logs-restart.md)
- [Architecture](architecture.md)
