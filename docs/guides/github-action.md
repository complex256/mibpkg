# GitHub Action

The repository ships a composite action, `complex256/mibpkg@v1`, that builds packages and publishes a
GitHub release cars can update from.

## Workflow

`mibpkg new` writes `.github/workflows/mibpkg-release.yml`:

```yaml
name: mibpkg release
on:
  push:
    tags: ['v*']
permissions:
  contents: write
jobs:
  release:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      # Build your program into the package directory first.
      - uses: complex256/mibpkg@v1
        with:
          packages: .
```

Push a tag:

```sh
git tag v1.0.0
git push --tags
```

## Inputs

| Input | Default | Meaning |
|---|---|---|
| `packages` | `.` | Space-separated package directories or `.mpk` files |
| `tag` | the pushed tag | Release tag |
| `args` | empty | Extra `mibpkg release` flags (`--id`, `--name`, `--keep`, ...) |

## First release

The first release needs a collection id and name:

```yaml
- uses: complex256/mibpkg@v1
  with:
    packages: .
    args: --id my-packages --name "My packages"
```

Later releases reuse the published collection.

## What it does

1. Downloads the Linux host `mibpkg` from the mibpkg release.
2. Runs `mibpkg release <packages> --repo <this repo> --tag <tag> ... --publish`.
3. Creates the GitHub release with `index.json` and the package files.

## Permissions

The workflow needs `contents: write` to create the release.

## Related

- [Publishing a collection](publishing-a-collection.md)
- [Your first package](your-first-package.md)
