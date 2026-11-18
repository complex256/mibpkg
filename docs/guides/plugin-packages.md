# Plugin packages

A **plugin package** is a `.mpk` that carries one SDK plugin. It is declarative: the manifest names the
jar, the plugin API level, its permissions and its icons. The SDK runtime loads the jar; mibpkg installs
it.

Plugin-only packages are built with the
[mibsdk plugin build](https://complex256.github.io/mib2q-plugin-sdk/): the `mibsdk.plugin` Gradle
convention produces the stamped jar and the `.mpk`.

## Manifest

```toml
[package]
name = "carmqtt-status"
version = "1.0.0"
description = "MQTT status item and page"

[compat]
units = ["MHI2Q"]

[plugin]
jar = "carmqtt-status.jar"
api = 6
permissions = ["network", "ui.slot.statusbar"]
icons = ["mqtt-on", "mqtt-off"]
```

| Field | Meaning |
|---|---|
| `jar` | The jar at the package root; its stamp is `<jar>.stamp` |
| `api` | The newest plugin API the plugin uses |
| `permissions` | The permissions the user approves |
| `icons` | PNGs at `icons/<name>.png`, at most 8 |

To give the package a page in the store, put a `README.md` next to `manifest.toml`, or set
`long_description` or `readme` under `[package]`. See [Store page](../reference/manifest.md).

A product package can add `id` when the plugin id differs from the package name. See
[Real packages](real-packages.md).

## Build one

From an SDK plugin project:

```sh
./gradlew mibpkgPackage -Pmibsdk.mibpkg=/path/to/mibpkg
```

Output: `build/mibsdk/<id>-<version>.mpk`.

The jar, its stamp and the icons are also available separately for a combined product package.

## Install it

1. Install the SDK package (`mibsdk`) first. A plugin needs the runtime.
2. Install the plugin `.mpk` from **Discover**, **More → Install from file**, or `mibpkg install-file`.
3. Review the permissions and approve.

mibpkg checks:

- the SDK is installed and supports the plugin's `api`;
- the unit's HMI build is one the SDK lists;
- every declared permission is in the installed SDK catalog;
- every icon exists and passes the PNG rules;
- the jar's stamp is valid.

## Dormant plugins

An SDK can drop old plugin API levels (`min_api` goes up). When you update `mibsdk` to such a version,
mibpkg does not refuse because of the plugins you already have. It updates the SDK, and every installed
plugin whose `api` is outside the new range goes **dormant**:

- the runtime does not load it (`BLOCKED_INVALID` in the SDK's Installed page);
- the package stays installed with its files, grants and data, and a combined product package keeps
  running its daemon;
- `mibpkg list` marks it (`[plugin dormant: needs plugin api 6; the installed SDK supports 9..9]`), the
  web interface shows "Plugin dormant" in the row and a notice on the package page, and the SDK install
  result lists it.

`update-all` puts the SDK first and then installs each plugin's update, so a plugin with a build for the new
SDK is dormant only for the length of that one run. Whatever is still dormant afterwards is reported with
"no build for this SDK is available in any collection". Dormancy is worked out each time from the installed
packages, not stored: installing a build that fits turns the plugin back on, and rolling the SDK back does
too. A plugin that *requires* a newer `mibsdk` than the one being installed (`requires.toml`) still refuses
the install, since no update of the plugin can fix that.

A plugin that asks for a permission the user has not approved is installed but stays
`BLOCKED_PERMISSIONS` until it is granted. The approval sheet normally prevents that: the install stops
with exit **3** until the permissions are approved.

## Update and remove

Update by installing a higher version. The SDK runtime hot-loads the new jar.

```sh
mibpkg remove <plugin-or-product>
```

For a plugin-only package, removal deletes the jar, its grants line, its icons and its status-line
settings. Its data is kept unless `--purge` is used. For a combined product package, removal takes the
native service too.

## Permissions and revocation

A plugin runs only when **all** of its declared permissions are granted. Revoking one stops the plugin
until it is granted again. Grant from the plugin's page in the web interface, or:

```sh
mibpkg sdk grant <plugin-id> network,ui.slot.statusbar
mibpkg sdk grant <plugin-id> ""
```

A combined product package carries its plugin under another id (`tailscale` carries `tailscale-status`);
either the product name or the plugin id may be used.

## Related

- [Real packages](real-packages.md)
- [SDK plugin docs](https://complex256.github.io/mib2q-plugin-sdk/)
- [manifest.toml](../reference/manifest.md)
