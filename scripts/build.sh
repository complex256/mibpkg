#!/usr/bin/env bash
# Cross-build mibpkg for MIB2 High (QNX 6.5, armv7) in the qnx65-armv7-toolchain image.
#   scripts/build.sh    -> target/armv7-unknown-nto-qnx650/release/mibpkg
# Needs the image built with Rust (../qnx65-armv7-toolchain: host-scripts/qnx-run.sh build 8.5-rust)
# and Docker Desktop with Rosetta on (the amd64 rustc segfaults without it).
set -euo pipefail
ROOT=$(cd "$(dirname "$0")/.." && pwd)
IMG=${QNX_IMAGE:-qnx65-armv7-toolchain:8.5-rust}
docker run --rm -v "$ROOT":/src/mibpkg -w /src "$IMG" build-std /src/mibpkg
ls -l "$ROOT/target/armv7-unknown-nto-qnx650/release/mibpkg"
