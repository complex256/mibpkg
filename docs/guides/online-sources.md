# Online sources

mibpkg can read package collections from a GitHub release. A car with internet checks them for
updates.

## Add a source

**More → Sources → Add source**, then enter:

```text
github:owner/repo
```

The repository must publish a release with an `index.json` and `.mpk` assets. The `mibpkg release`
command and the GitHub Action produce that layout.

The official collection is `github:complex256/mibpkg-packages`. It is built in and always enabled.

## What the car fetches

For `github:owner/repo`, mibpkg downloads:

```text
https://github.com/owner/repo/releases/latest/download/index.json
```

The index lists each package's name, version, file, SHA-256, size, compatibility and plugin metadata.
The package files are downloaded from the same release (or from absolute `https://` URLs in the index).

## The card's source list

A card can carry `mibpkg/sources.toml`:

```toml
[[source]]
name = "My packages"
source = "github:owner/repo"
```

Sources on the card appear under **Available** without a rescan. **More → Sources** can enable,
disable or remove them.

## Updates

The supervisor checks online sources when the car has internet. **Updates → Update all** installs newer
compatible versions. A plugin that adds a permission must be updated individually so the approval sheet
can be shown.

## Verify a collection

Before pointing cars at a repository, check what they will see:

```sh
mibpkg verify github:owner/repo
mibpkg verify github:owner/repo --latest
mibpkg verify github:owner/repo --unit MHI2Q
```

`verify` reports missing packages, bad checksums, version problems and incompatible entries. It exits
non-zero when there are problems.

## TLS and time

Online updates use the unit's TLS stack through `libcurl` and the CA bundle installed with mibpkg. The
unit's clock is unset at boot, so mibpkg uses SNTP to establish a verification time. A car that has
never had a working network may need an online check after the clock is set.

## Troubleshooting

| Symptom | Check |
|---|---|
| No updates | Is the source enabled? Does the release have `index.json`? |
| Checksum mismatch | Re-run `mibpkg release`; do not edit a published asset |
| TLS error | Is the clock set? Is `ca.pem` present? |
| 404 | The repository or release tag is wrong, or the release is still propagating |

## Related

- [Publishing a collection](publishing-a-collection.md)
- [Collection index](../reference/collection-index.md)
- [Online updates](../explanation/online-updates.md)
