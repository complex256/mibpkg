#!/usr/bin/env bash
# Build a fake unit root for working on the web interface on a Mac, then print how to run it.
#
#   scripts/web-fixture.sh /tmp/mibroot
#
# The root has:
#   - carmqtt 0.1.2 installed from the SD card collection, missing its required broker.toml,
#     with 0.1.3 waiting on the card (an update);
#   - gps-logger 1.0.0 installed with a config conflict: logger.toml was edited on the car
#     and changed on the card too (logger.toml.card waits for Keep / Use card);
#   - a "community" card collection with tpms-overlay and its own dashcam-sync;
#   - an online collection (cached index) with carplay-rgi and another dashcam-sync,
#     so dashcam-sync is offered by two sources;
#   - web.toml on port 8099;
#   - <root>/../uploads/hello-1.0.0.mpk to try "Install from file".
set -euo pipefail

ROOT=${1:?usage: web-fixture.sh <empty-or-new-dir>}
REPO=$(cd "$(dirname "$0")/.." && pwd)

cargo build -q --manifest-path "$REPO/Cargo.toml"
BIN="$REPO/target/debug/mibpkg"

rm -rf "$ROOT"
mkdir -p "$ROOT"/{mnt/app/root,mnt/app/eso/hmi/engdefs,mnt/system/etc/boot,tmp,fs/sda0}
printf '#DEV_MMX2QC_AU_NAR_G22_061PROD\n636\n' >"$ROOT/mnt/app/img_ver.txt"
printf '#!/bin/sh\necho stock startup\n' >"$ROOT/mnt/system/etc/boot/startup.sh"
WORK=$(mktemp -d)

# pkg <name> <version> <description> [extra manifest text]: a package source dir.
pkg() {
    local name=$1 version=$2 desc=$3 extra=${4:-}
    local dir="$WORK/src/$name-$version"
    mkdir -p "$dir/bin"
    cat >"$dir/manifest.toml" <<EOF
[package]
name = "$name"
version = "$version"
description = "$desc"

[[file]]
src = "bin/$name"
mode = "755"

[[service]]
name = "main"
exec = "@pkg/bin/$name"
$extra
EOF
    printf '#!/bin/sh\nwhile true; do echo "%s %s alive"; sleep 5; done\n' "$name" "$version" >"$dir/bin/$name"
    chmod 755 "$dir/bin/$name"
    echo "$dir"
}

# collection <dir> <id> <name> <src...>: build packages into a card collection and index it.
collection() {
    local dir=$1 id=$2 name=$3
    shift 3
    mkdir -p "$dir/packages"
    for src in "$@"; do
        "$BIN" build "$src" "$dir/packages" >/dev/null
    done
    "$BIN" index "$dir" --id "$id" --name "$name" >/dev/null
}

CARD="$ROOT/fs/sda0/mibpkg/collections"
CONF='[[config]]
path = "broker.toml"
example = "broker.toml.example"
required = true

[[file]]
src = "broker.toml.example"'

C012=$(pkg carmqtt 0.1.2 "Car data to Home Assistant over MQTT" "$CONF")
C013=$(pkg carmqtt 0.1.3 "Car data to Home Assistant over MQTT" "$CONF")
for d in "$C012" "$C013"; do printf 'host = "homeassistant.local"\nport = 1883\n' >"$d/broker.toml.example"; done

# Install 0.1.2 from the card, then put 0.1.3 on the card as the update.
collection "$CARD/complex256-carmqtt" complex256-carmqtt "carmqtt" "$C012"
"$BIN" --root "$ROOT" --no-supervisor install carmqtt complex256-carmqtt
rm -rf "$CARD/complex256-carmqtt"
collection "$CARD/complex256-carmqtt" complex256-carmqtt "carmqtt" "$C012" "$C013"

# gps-logger: config imported from the card, edited on the car, then changed on the card.
LOG_CONF='[[config]]
path = "logger.toml"'
collection "$CARD/complex256-gps" complex256-gps "gps-logger" "$(pkg gps-logger 1.0.0 "Logs GPS fixes to the SD card" "$LOG_CONF")"
LOG_CARD="$ROOT/fs/sda0/mibpkg/config/gps-logger"
mkdir -p "$LOG_CARD"
printf '[log]\ninterval = 5\npath = "/fs/sda0/gps"\n' >"$LOG_CARD/logger.toml"
"$BIN" --root "$ROOT" --no-supervisor install gps-logger complex256-gps >/dev/null
printf '[log]\ninterval = 1\npath = "/fs/sda0/gps"\n\n[sources.ubx]\nenabled = true\npriority = 20\n' \
    >"$ROOT/mnt/app/root/mibpkg/state/config/gps-logger/logger.toml"
printf '[log]\ninterval = 10\npath = "/fs/sda0/gps"\nformat = "gpx"\n' >"$LOG_CARD/logger.toml"
"$BIN" --root "$ROOT" --no-supervisor rescan

collection "$CARD/community" community "Community packages" \
    "$(pkg tpms-overlay 0.2.0 "Tyre pressures on the map")" \
    "$(pkg dashcam-sync 0.3.0 "Copies dashcam clips when parked")"

# An online collection, as `online-check` would have cached it.
ONLINE="$WORK/online"
mkdir -p "$ONLINE"
"$BIN" build "$(pkg carplay-rgi 1.0.0 "CarPlay route guidance in the virtual cockpit")" "$ONLINE" >/dev/null
"$BIN" build "$(pkg dashcam-sync 0.4.0 "Copies dashcam clips when parked")" "$ONLINE" >/dev/null
"$BIN" index "$ONLINE" --id complex256 --name "complex256 packages" --flat >/dev/null
mkdir -p "$ROOT/mnt/app/root/mibpkg/state/online"
python3 - "$ONLINE/index.json" "$ROOT/mnt/app/root/mibpkg/state/online/complex256.json" <<'EOF'
import json, sys
index = json.load(open(sys.argv[1]))
cached = {"source": "github:complex256/mibpkg-packages",
          "base_url": "https://github.com/complex256/mibpkg-packages/releases/latest/download/",
          "index": index}
json.dump(cached, open(sys.argv[2], "w"), indent=2)
EOF

printf 'port = 8099\n' >"$ROOT/mnt/app/root/mibpkg/web.toml"

# A file to upload.
mkdir -p "$ROOT/../uploads"
"$BIN" build "$(pkg hello 1.0.0 "Says hello every few seconds")" "$ROOT/../uploads" >/dev/null

# The supervisor runs services and the web server from <root>/tmp/mibpkg, like /tmp/mibpkg on the car.
cp "$BIN" "$ROOT/tmp/mibpkg"
rm -rf "$WORK"

cat <<EOF
Fixture ready in $ROOT

  Everything, like the car (supervisor + services + web on port 8099):
    $ROOT/tmp/mibpkg --root $ROOT supervise

  Only the web server, serving web/ from disk so edits show on reload:
    MIBPKG_WEB_DIR=$REPO/web $BIN --root $ROOT web

  Then open http://localhost:8099/
EOF
