#!/bin/sh
# mibpkg boot entry, run from the one startup.sh hook line (detached, no stdio).
# Persistent storage cannot exec binaries, so mibpkg runs from /tmp (RAM).
# /tmp cannot hold directories: every runtime file is a flat /tmp/mibpkg.* name.
# pdksh subset only; the unit has no dirname, tail, nohup or date.
HOME_DIR=${MIBPKG_HOME:-/mnt/app/root/mibpkg}
RUN=${MIBPKG_RUN:-/tmp}
ROOTS=${MIBPKG_MEDIA:-"/fs/sda0 /fs/sdb0 /fs/sd0 /fs/sd1 /fs/usb0"}
WAIT=${MIBPKG_WAIT:-15}
LOG=$RUN/mibpkg.log

# Give the media system a moment to mount a card, so a DISABLE card is seen.
i=0
found=0
while [ $i -lt "$WAIT" ]; do
    for r in $ROOTS; do
        [ -d "$r" ] && found=1
    done
    [ $found = 1 ] && break
    sleep 1
    i=$((i + 1))
done

for r in $ROOTS; do
    if [ -f "$r/mibpkg/DISABLE" ]; then
        echo "mibpkg: $r/mibpkg/DISABLE present, starting nothing" >>"$LOG"
        exit 0
    fi
done

cp "$HOME_DIR/bin/mibpkg" "$RUN/mibpkg.new" || exit 1
chmod 755 "$RUN/mibpkg.new"
mv "$RUN/mibpkg.new" "$RUN/mibpkg" || exit 1
exec "$RUN/mibpkg" $MIBPKG_ARGS supervise >>"$LOG" 2>&1
