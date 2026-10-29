# Build

## Host binary

```sh
cargo build --release
./target/release/mibpkg --help
```

## Unit binary

The unit binary is cross-compiled for QNX 6.5 ARMv7 in a Docker image:

```sh
scripts/build.sh
```

It needs Docker Desktop with Rosetta enabled on Apple silicon and the
`qnx65-armv7-toolchain:8.5-rust` image.

## SD-card image

```sh
scripts/build.sh
scripts/bundle.sh
```

Output:

```text
out/sd/
out/mibpkg-sd-<version>.zip
```

`scripts/bundle.sh` builds the host binary, stages the unit binary, builds the official collection and
calls `mibpkg swdl-card`. Pass `--signed-header FILE` to make the engineering-menu entry accepted.

## Web assets

`web/` is plain HTML, CSS and JavaScript with no build step. The files are embedded in the binary as
written.

For local development:

```sh
scripts/web-fixture.sh /tmp/mibpkg-fixture
MIBPKG_WEB_DIR=web ./target/debug/mibpkg --root /tmp/mibpkg-fixture web
```

## Layout

| Path | What it is |
|---|---|
| `src/cli.rs` | Command dispatch and host commands |
| `src/install.rs` | Install, update, remove, rollback |
| `src/manifest.rs` | `manifest.toml` parsing and validation |
| `src/archive.rs` | `.mpk` pack/read/extract |
| `src/publish.rs` | `release` and `verify` |
| `src/index.rs` | Collections and `index.json` |
| `src/supervisor.rs` | Service supervision |
| `src/conditions.rs` | Start conditions |
| `src/web/` | Web server and API |
| `src/mibsdk/` | SDK, plugin and icon package handling |
| `src/gem*`, `src/hook.rs` | GEM screens and boot hook |
| `web/` | Web interface source |
| `packaging/` | Official manifest, CA bundle, source list |
| `scripts/` | Build, bundle and release helpers |
| `shell/` | Boot and GEM scripts installed on the unit |

## Related

- [Test](test.md)
- [Release](release.md)
