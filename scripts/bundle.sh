#!/usr/bin/env bash
# Build the SD card bundle: M.I.B. bootstrap + the complex256 collection.
#   scripts/bundle.sh [extra.mpk ...]     -> out/sd/, out/mibpkg-sd-<version>.zip
# Needs scripts/build.sh output (QNX binary). Builds the host mibpkg itself.
set -euo pipefail
ROOT=$(cd "$(dirname "$0")/.." && pwd)
QNX_BIN=$ROOT/target/armv7-unknown-nto-qnx650/release/mibpkg
[ -f "$QNX_BIN" ] || { echo "missing $QNX_BIN - run scripts/build.sh first" >&2; exit 1; }
VERSION=$(sed -n 's/^version = "\(.*\)"/\1/p' "$ROOT/Cargo.toml" | head -1)
cargo build --release --manifest-path "$ROOT/Cargo.toml"
HOST=$ROOT/target/release/mibpkg

OUT=$ROOT/out
rm -rf "$OUT/sd" "$OUT/stage"
mkdir -p "$OUT/stage/mibpkg/bin" "$OUT/sd/mod/mibpkg" "$OUT/sd/mibpkg/config"
COLL=$OUT/sd/mibpkg/collections/complex256
mkdir -p "$COLL/packages"

# mibpkg as a package (self-update path)
sed "s/@VERSION@/$VERSION/" "$ROOT/packaging/mibpkg/manifest.toml.in" > "$OUT/stage/mibpkg/manifest.toml"
cp "$QNX_BIN" "$OUT/stage/mibpkg/bin/mibpkg"
cp "$ROOT/shell/launch.sh" "$ROOT/shell/gem.sh" "$ROOT/packaging/ca.pem" "$OUT/stage/mibpkg/bin/"
chmod 755 "$OUT/stage/mibpkg/bin/"*
chmod 644 "$OUT/stage/mibpkg/bin/ca.pem"
"$HOST" build "$OUT/stage/mibpkg" "$COLL/packages"

for extra in "$@"; do
    cp "$extra" "$COLL/packages/"
done
"$HOST" index "$COLL" --id complex256 --name "complex256 packages"

# M.I.B. bootstrap
cp "$ROOT/shell/custom.sh" "$ROOT/shell/command.sh" "$OUT/sd/mod/"
cp "$QNX_BIN" "$ROOT/shell/launch.sh" "$ROOT/shell/gem.sh" "$ROOT/packaging/ca.pem" "$OUT/sd/mod/mibpkg/"

(cd "$OUT/sd" && rm -f "../mibpkg-sd-$VERSION.zip" && zip -qr "../mibpkg-sd-$VERSION.zip" .)
echo "bundle: $OUT/sd  ($OUT/mibpkg-sd-$VERSION.zip)"
