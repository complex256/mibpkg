# Publishing a collection

A collection is a GitHub release with an `index.json` and the `.mpk` files. Cars add it as
`github:owner/repo` and check for updates when they have internet.

## First release

```sh
mibpkg release . \
  --repo <owner>/<repo> \
  --tag v0.1.0 \
  --id my-packages \
  --name "My packages" \
  --publish
```

| Flag | Meaning |
|---|---|
| `--repo` | `owner/repo` that will host the release |
| `--tag` | Release tag; use `v` + semver |
| `--id` | Collection id, `[a-z0-9-]{1,40}` |
| `--name` | Human name shown in the web interface |
| `--keep` | How many versions of each package to keep (default 3) |
| `--out` | Output directory (default `out/release-<tag>`) |
| `--asset` | Extra file to attach (for example `sources.json`) |
| `--publish` | Create the GitHub release and wait for the index to be live |

The first release must pass `--id`. Later releases reuse the published id unless you pass `--new-id`.

## What it writes

```text
out/release-v0.1.0/
├── index.json
├── hello-0.1.0.mpk
└── ...
```

`index.json` lists every package version kept, with an absolute download URL, SHA-256 and size. Older
versions are carried forward, so a car can still roll back to them.

## Later releases

```sh
mibpkg release . --repo <owner>/<repo> --tag v0.2.0 --publish
```

The collection id and name are read from the previous release. `--keep` defaults to 3 versions per
package. A version already published with different contents is refused: bump the version.

## GitHub CLI

`mibpkg release` uses `gh`. Install the GitHub CLI and run `gh auth login`, or set `GH_TOKEN`.

## Verify

```sh
mibpkg verify out/release-v0.2.0
mibpkg verify github:<owner>/<repo> --latest
```

`verify` checks the index, file names, checksums, versions, compatibility and plugin metadata. It exits
non-zero on problems.

## Publish the source list

The optional `sources.json` asset tells new cars which collections to offer. `mibpkg new` and the
official collection include one. Attach it with `--asset packaging/sources.json`.

## Related

- [Collection index](../reference/collection-index.md)
- [Online sources](online-sources.md)
- [GitHub Action](github-action.md)
