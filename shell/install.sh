#!/bin/sh
# mibpkg SWDL final-script: run by the Red/Engineering menu's SW-Update flow.
# SWDL passes $1 = hardware, $2 = SD mount. This is the only hashed file on the card
# (FinalScriptChecksum in metainfo2.txt), so it stays thin: land the payload, then let
# the mibpkg binary do the real work. QNX 6.5 /bin/sh is pdksh; stay in its subset
# (no dirname, tail, nohup, date). /sbin (mount) may be off PATH.
PATH=/proc/boot:/bin:/usr/bin:/usr/sbin:/sbin:/mnt/app/armle/bin:/mnt/app/armle/usr/bin
export PATH
unset LD_PRELOAD

HW=$1
SD=$2
D=$SD/mibpkg

echo "[mibpkg-install] starting for HW $HW from $SD"

# Clear any stale success marker from an earlier run in this (RAM) boot, so it can only
# mean "this run succeeded".
rm -f /tmp/SWDLScript.Result

# GEM/MMX work happens on the MMX side (that is where /mnt/app lives). When run on RCC
# (no local lsd dir but mmx has one), run the install on MMX and CAPTURE its exit — do not
# `exec`, or this RCC shell is replaced and the RCC-side SWDLScript.Result would never be
# touched, making SWDL report failure. On MMX the test below is false, so there is no loop.
# S1-pending: confirm on-car that MMX sees the SD at "$D" (it may need a /net/rcc prefix).
if [ ! -d /mnt/app/eso/hmi/lsd ] && [ -d /net/mmx/mnt/app/eso/hmi/lsd ]; then
    echo "[mibpkg-install] running install on mmx"
    on -f mmx /bin/sh "$D/final/install.sh" "$@"
    rc=$?
    if [ $rc -eq 0 ]; then
        touch /tmp/SWDLScript.Result
        echo "[mibpkg-install] done (via mmx)"
    else
        echo "[mibpkg-install] mmx install FAILED (rc=$rc)"
    fi
    exit $rc
fi

HOME_DIR=/mnt/app/root/mibpkg
mount -uw /mnt/app || { echo "[mibpkg-install] cannot mount /mnt/app rw"; exit 1; }

[ -d "$HOME_DIR/bin" ] || mkdir -p "$HOME_DIR/bin" || { echo "[mibpkg-install] mkdir failed"; exit 1; }
for f in mibpkg launch.sh gem.sh; do
    cp "$D/payload/$f" "$HOME_DIR/bin/$f.new" && chmod 755 "$HOME_DIR/bin/$f.new" && mv "$HOME_DIR/bin/$f.new" "$HOME_DIR/bin/$f" \
        || { echo "[mibpkg-install] copying $f failed"; exit 1; }
done
if [ -f "$D/payload/ca.pem" ]; then
    cp "$D/payload/ca.pem" "$HOME_DIR/bin/ca.pem.new" && mv "$HOME_DIR/bin/ca.pem.new" "$HOME_DIR/bin/ca.pem"
fi
sync
mount -ur /mnt/app

# Persistent storage cannot exec; run mibpkg from /tmp (RAM).
cp "$HOME_DIR/bin/mibpkg" /tmp/mibpkg.new && chmod 755 /tmp/mibpkg.new && mv /tmp/mibpkg.new /tmp/mibpkg \
    || { echo "[mibpkg-install] staging mibpkg failed"; exit 1; }

# C backbone: the only step whose failure aborts the install (we exit without marking success).
if /tmp/mibpkg bootstrap; then
    echo "[mibpkg-install] bootstrap ok"
else
    echo "[mibpkg-install] bootstrap FAILED; aborting"
    exit 1
fi

# B2 layer: best-effort, never fatal. A failure here = clean C install (web UI only).
/tmp/mibpkg enable-gem || echo "[mibpkg-install] enable-gem skipped"
/tmp/mibpkg gem-patch apply --from "$D/payload/gem-patch" || echo "[mibpkg-install] gem-patch skipped"

# Stop insert-and-go from looping: disable the autostart trigger on the card.
if [ -f "$SD/Swdlautorun.txt" ]; then
    mv -f "$SD/Swdlautorun.txt" "$SD/_Swdlautorun.txt" && echo "[mibpkg-install] autostart disabled"
fi

echo "[mibpkg-install] done; open the engineering menu -> mibpkg, or the web UI"
touch /tmp/SWDLScript.Result
exit 0
