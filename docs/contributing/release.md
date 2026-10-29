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
   node --test web/tests/*.test.mjs
   python3 tools/check_docs.py
   mkdocs build --strict
   ```

5. Build the unit binary and the card:

   ```sh
   scripts/build.sh
   scripts/bundle.sh --signed-header /path/to/header.txt
   ```

6. Tag and push:

   ```sh
   git tag v0.10.0
   git push origin main --tags
   ```

7. Create a GitHub release for the tag. `.github/workflows/host-release.yml` attaches the Linux host
   binary `mibpkg-host-linux-x86_64`.

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
