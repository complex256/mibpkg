# Release

## Version

`Cargo.toml` carries the version. Bump it for every release; a version is never reused.

## Checklist

1. Bump `version` in `Cargo.toml`.
2. Add a `CHANGELOG.md` entry.
3. Update the docs that name the version.
4. Run the gates:

   ```sh
   cargo test
   npm --prefix web test
   python3 tools/check_docs.py
   mkdocs build --strict
   ```

5. Tag and push:

   ```sh
   git tag v0.10.0
   git push origin main --tags
   ```

6. Create a GitHub release for the tag. Publishing it starts two workflows:
   `.github/workflows/host-release.yml` attaches the Linux host binary `mibpkg-host-linux-x86_64`,
   and `.github/workflows/card-release.yml` builds the signed SD card on the self-hosted runner and
   attaches `mibpkg-sd-<version>.zip`. See [CI runner](ci-runner.md).

To build a card on your own machine instead, run `scripts/build.sh` and then
`scripts/bundle.sh --signed-header /path/to/header.txt`.

## Docs

The docs workflow publishes the site to `gh-pages` on a push to `main` or a `v*` tag. GitHub Pages must
serve the `gh-pages` branch.

## Official collection

The official packages are published separately:

```sh
scripts/publish-official.sh <tag> <mpk>...
```

This releases `complex256/mibpkg-packages` and verifies the index.

## The action

`action.yml` downloads the host binary from the mibpkg release. A release must exist before
`complex256/mibpkg@v1` can be used by another repository.

## Related

- [Build](build.md)
- [Test](test.md)
- [Publishing a collection](../guides/publishing-a-collection.md)
