# Config, logs and restart

## Config files

A package declares config files in its manifest:

```toml
[[config]]
path = "carmqtt.toml"
example = "carmqtt.toml.example"
required = true
```

The example is shipped in the package. The live file lives in the package's config directory:

```text
/mnt/app/root/mibpkg/state/config/<package>/<path>
```

On the card, you can supply a config at:

```text
mibpkg/config/<package>/<path>
```

**Rescan** imports new card configs. A config can be:

- **required** — the service will not start until it exists;
- **optional** — the example is copied only when the user asks, or when the package needs it.

## Config conflicts

If both the card and the unit changed the same config, mibpkg refuses to guess. Open the package page
and choose:

- **keep the current** — the unit's file wins;
- **use the card's** — the card's file wins.

From the command line:

```sh
mibpkg config-resolve <name> keep
mibpkg config-resolve <name> card
mibpkg config-resolve <name> keep <path>
```

`--config=keep|card` on `install`, `rescan` and `update-all` settles conflicts for that run.

## Logs

Each package's stdout/stderr is captured under `/tmp/mibpkg.<package>.<service>.log`. The web interface
shows it under **Installed → package → Logs**. **More → Logs** has the web server and supervisor logs.

`/tmp` is RAM: logs are capped and disappear on reboot.

From the command line:

```sh
mibpkg status
mibpkg list
```

## Restart a service

From the package page: **Restart**.

From the command line:

```sh
mibpkg restart <name>
```

The supervisor restarts the package's services without reinstalling it.

## Rescan

After changing the SD card, open **More → Rescan** or run:

```sh
mibpkg rescan
```

Rescan reloads collections and imports new card configs. It does not install anything.

## Related

- [Packages](packages.md)
- [Service conditions](../reference/service-conditions.md)
- [Paths and status](../reference/paths-and-status.md)
