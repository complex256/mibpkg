# Collection index

A collection is a directory with `index.json` and package files. Online collections are GitHub releases
with the same layout.

## Directory

```text
my-collection/
├── index.json
└── packages/
    ├── hello-1.0.0.mpk
    └── hello-1.1.0.mpk
```

A flat collection puts the `.mpk` files next to `index.json` and uses `--flat` with `mibpkg index`.

## `index.json`

```json
{
  "format": 1,
  "collection": {
    "id": "my-packages",
    "name": "My packages"
  },
  "packages": [
    {
      "name": "hello",
      "version": "1.0.0",
      "file": "packages/hello-1.0.0.mpk",
      "sha256": "…",
      "size": 12345,
      "description": "Hello package",
      "compat": { "units": ["MHI2Q"] },
      "uses_scripts": false
    }
  ],
  "signatures": []
}
```

| Field | Meaning |
|---|---|
| `format` | `1` |
| `collection.id` | `[a-z0-9-]{1,40}`; `uploaded` is reserved |
| `collection.name` | Shown in the web interface |
| `packages[].name` | Package name |
| `packages[].version` | Semver |
| `packages[].file` | Relative path or absolute `https://` URL |
| `packages[].sha256` | Lowercase hex SHA-256 |
| `packages[].size` | Bytes |
| `packages[].compat` | Same shape as `[compat]` |
| `packages[].uses_scripts` | Whether the package runs scripts |
| `packages[].kind` | `plugin` or `sdk`, absent for a plain package |
| `packages[].permissions` | A plugin's declared permissions |
| `packages[].min_sdk` | A plugin's oldest supported `mibsdk` version, when declared |
| `signatures` | Reserved; ignored in v1 |

## Build it

```sh
mibpkg index my-collection --id my-packages --name "My packages"
```

The collection id and name are validated. Each file name must be `<name>-<version>.mpk` and match the
manifest inside.

## Online URLs

An online index may use absolute `https://` URLs in `file`. A relative path resolves against the release's
download directory. `mibpkg release` writes absolute URLs for every release asset.

## Source list

A card can carry `mibpkg/sources.toml`:

```toml
[[source]]
name = "My packages"
source = "github:owner/repo"
```

`mibpkg release` can attach a `sources.json` asset with the same entries; new cars read it from the
official collection.

## Verify

```sh
mibpkg verify my-collection
mibpkg verify github:owner/repo --latest
```

`verify` checks the index format, file names, checksums, versions, compatibility and plugin metadata.

## Related

- [Publishing a collection](../guides/publishing-a-collection.md)
- [Online sources](../guides/online-sources.md)
- [Online updates](../explanation/online-updates.md)
