# CI runner

Publishing a GitHub release builds the signed SD card and attaches it. The `signed card` workflow
(`.github/workflows/card-release.yml`) does this on a self-hosted runner, because the QNX cross
toolchain image holds the proprietary QNX SDP and is not pushed to any registry.

## What the workflow does

1. Checks that the release tag equals `v` plus the `Cargo.toml` version.
2. Runs `scripts/build.sh` to cross-build the QNX binary in `qnx65-armv7-toolchain:8.5-rust`.
3. Rebuilds the signed header from the `SIGNED_HEADER_GZ_B64` secret and runs
   `scripts/bundle.sh --signed-header`.
4. Checks that the card carries a `[Signature]` block and the right version.
5. Uploads `mibpkg-sd-<version>.zip` and its `.sha256` to the release.

The workflow runs on `release: published` and on manual dispatch with a tag. It never runs on
`pull_request`, so code from a fork cannot reach the runner or the secret.

## The runner

The runner is an unprivileged, isolated guest that is kept off any shared host. It has Docker, Rust,
Node and the `qnx65-armv7-toolchain:8.5-rust` image, and runs the Actions runner as a non-root user
with the labels `self-hosted, linux, x64, mibpkg-qnx`.

Load a rebuilt toolchain image into it from the build machine:

```sh
docker save qnx65-armv7-toolchain:8.5-rust | gzip -1 | ssh <runner> 'gunzip | docker load'
```

## The signed header secret

The header is a genuine signed `metainfo2.txt` from a vendor update, and it is never committed. GitHub limits a secret to
48 KB and the file is about 42 KB, so store it gzipped and base64-encoded:

```sh
gzip -9c metainfo2.txt | base64 | tr -d '\n' | gh secret set SIGNED_HEADER_GZ_B64 --repo complex256/mibpkg
```

## Rebuild a card by hand

Run the workflow from the Actions tab with an existing tag, or:

```sh
gh workflow run "signed card" -f tag=v0.15.0
```

## Related

- [Release](release.md)
- [Build](build.md)
