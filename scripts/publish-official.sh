#!/usr/bin/env bash
# Publish the official collection online: a flat index.json + the .mpk files as assets of a
# GitHub release in complex256/mibpkg-packages (the default online source).
#   scripts/publish-official.sh <tag> <mpk>...
set -euo pipefail
ROOT=$(cd "$(dirname "$0")/.." && pwd)
TAG=$1; shift
OUT=$ROOT/out/online
rm -rf "$OUT" && mkdir -p "$OUT"
cp "$@" "$OUT/"
"$ROOT/target/release/mibpkg" index "$OUT" --id complex256-mibpkg --name "mibpkg" --flat
gh release create "$TAG" --repo complex256/mibpkg-packages --title "$TAG" --notes "mibpkg official collection $TAG" "$OUT"/*
