# Online updates

mibpkg can check GitHub releases for new package versions.

## An uplink is required

Online updates need an internet uplink: SIM data, a working Wi-Fi client, or another route. The car's
hotspot is local only. Use **More → Internet** to save a phone hotspot as the car's uplink; the mibsdk
package applies it within a second and keeps the car's hotspot available. Without an uplink, use the
card or a phone upload; **Discover** and **Updates** still show the local collections.

## Sources

A source is a GitHub repository with releases made by `mibpkg release`:

- `index.json`
- package `.mpk` files
- optional assets such as `sources.json`

Cars add `github:owner/repo` in **More → Sources**, or read it from the card's `mibpkg/sources.toml`.

The official collection is built in.

## Check

The supervisor checks sources when the car has internet and periodically after boot. The web interface
also refreshes on **Updates**.

For each package mibpkg keeps the newest compatible version across the loaded collections. An update is
offered when the collection has a higher semver version than the installed one.

## Download and verify

For a remote package, mibpkg:

1. resolves the index URL (GitHub's `releases/latest/download` redirect);
2. downloads the index;
3. downloads the package;
4. checks its SHA-256 and size;
5. checks the manifest, compatibility and, for plugins, the SDK and permissions;
6. installs it.

A failed check changes nothing.

## Time and TLS

The unit's clock is unset at boot, so mibpkg uses SNTP to establish a verification time. TLS uses the
unit's `libcurl` and the CA bundle installed with mibpkg. A car that has never had a working network may
need an online check after the clock is set.

## Cache

The last index and packages are cached so **Discover** and **Updates** work offline. A source that cannot
be reached is reported in **More → Sources** and **Health**; the card collections still work.

## Publishing

`mibpkg release`:

- builds each package;
- carries older versions forward (`--keep`, default 3);
- writes absolute download URLs;
- creates the GitHub release;
- waits for the new index to become live.

A version already published with different contents is refused. Bump the version.

## Related

- [Online sources](../guides/online-sources.md)
- [Publishing a collection](../guides/publishing-a-collection.md)
- [Collection index](../reference/collection-index.md)
