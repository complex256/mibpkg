#!/usr/bin/env bash
# Build the unified SD card bundle: ONE card that installs via the Red/Engineering menu (SWDL)
# AND via M.I.B. "Run Custom Script", carrying the official (complex256-mibpkg) collection.
#   scripts/bundle.sh [--signed-header FILE] [extra.mpk ...]  -> out/sd/, out/mibpkg-sd-<version>.zip
# Needs scripts/build.sh output (QNX binary). Builds the host mibpkg itself.
# Card assembly lives in `mibpkg swdl-card`; this only builds the collection and calls it.
# Without --signed-header the M.I.B. path still works, but the Red-menu entry is unsigned (it
# shows a signature error if opened); pass a genuine signed metainfo2 header for a card that
# installs both ways.
set -euo pipefail
ROOT=$(cd "$(dirname "$0")/.." && pwd)
QNX_BIN=$ROOT/target/armv7-unknown-nto-qnx650/release/mibpkg
[ -f "$QNX_BIN" ] || { echo "missing $QNX_BIN - run scripts/build.sh first" >&2; exit 1; }

HEADER=""
MPKS=()
while [ $# -gt 0 ]; do
    case "$1" in
        --signed-header) HEADER=$2; shift 2 ;;
        *) MPKS+=("$1"); shift ;;
    esac
done

VERSION=$(sed -n 's/^version = "\(.*\)"/\1/p' "$ROOT/Cargo.toml" | head -1)
cargo build --release --manifest-path "$ROOT/Cargo.toml"
HOST=$ROOT/target/release/mibpkg

OUT=$ROOT/out
rm -rf "$OUT/sd" "$OUT/stage" "$OUT/payload" "$OUT/seed"
mkdir -p "$OUT/stage/mibpkg/bin" "$OUT/payload" "$OUT/seed/complex256-mibpkg/packages"

# Payload, shared by both entry points.
cp "$QNX_BIN" "$OUT/payload/mibpkg"
cp "$ROOT/shell/launch.sh" "$ROOT/shell/gem.sh" "$ROOT/packaging/ca.pem" "$OUT/payload/"
chmod 755 "$OUT/payload/mibpkg" "$OUT/payload/launch.sh" "$OUT/payload/gem.sh"
chmod 644 "$OUT/payload/ca.pem"

# Official collection: mibpkg as a package (self-update path) plus any extra .mpk given.
sed "s/@VERSION@/$VERSION/" "$ROOT/packaging/mibpkg/manifest.toml.in" > "$OUT/stage/mibpkg/manifest.toml"
cp "$QNX_BIN" "$OUT/stage/mibpkg/bin/mibpkg"
cp "$ROOT/shell/launch.sh" "$ROOT/shell/gem.sh" "$ROOT/packaging/ca.pem" "$OUT/stage/mibpkg/bin/"
chmod 755 "$OUT/stage/mibpkg/bin/mibpkg" "$OUT/stage/mibpkg/bin/launch.sh" "$OUT/stage/mibpkg/bin/gem.sh"
chmod 644 "$OUT/stage/mibpkg/bin/ca.pem"
"$HOST" build "$OUT/stage/mibpkg" "$OUT/seed/complex256-mibpkg/packages"
for extra in "${MPKS[@]:-}"; do
    [ -n "$extra" ] && cp "$extra" "$OUT/seed/complex256-mibpkg/packages/"
done
"$HOST" index "$OUT/seed/complex256-mibpkg" --id complex256-mibpkg --name "mibpkg"

# Assemble the unified card (both entry points by default).
HDR_ARGS=()
[ -n "$HEADER" ] && HDR_ARGS=(--signed-header "$HEADER")
"$HOST" swdl-card "$OUT/sd" --payload "$OUT/payload" --seed "$OUT/seed/complex256-mibpkg" --version "$VERSION" "${HDR_ARGS[@]}"
mkdir -p "$OUT/sd/mibpkg/config"

(cd "$OUT/sd" && rm -f "../mibpkg-sd-$VERSION.zip" && zip -qr "../mibpkg-sd-$VERSION.zip" .)
echo "bundle: $OUT/sd  ($OUT/mibpkg-sd-$VERSION.zip)"
