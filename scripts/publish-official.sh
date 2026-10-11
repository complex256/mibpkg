#!/usr/bin/env bash
# Publish the official collection (complex256-mibpkg) and the community source list as a release
# of complex256/mibpkg-packages. Older versions stay listed (mibpkg release carries them forward).
#   scripts/publish-official.sh <tag> <mpk>...
set -euo pipefail
ROOT=$(cd "$(dirname "$0")/.." && pwd)
TAG=$1; shift
cargo build --release --manifest-path "$ROOT/Cargo.toml"
"$ROOT/target/release/mibpkg" release "$@" \
    --repo complex256/mibpkg-packages --tag "$TAG" \
    --id complex256-mibpkg --name "mibpkg" \
    --asset "$ROOT/packaging/sources.json" \
    --out "$ROOT/out/release-$TAG" --publish
"$ROOT/target/release/mibpkg" verify github:complex256/mibpkg-packages --latest
