#!/bin/sh
# M.I.B. -> Advanced Settings -> Run Custom Script: installs or repairs mibpkg.
# Lives at /mod/custom.sh on the SD card, next to /mod/mibpkg/{mibpkg,launch.sh,gem.sh}.
# QNX 6.5 /bin/sh is pdksh; this stays inside its portable subset.
PATH=/proc/boot:/bin:/usr/bin:/usr/sbin:/sbin:/mnt/app/armle/bin:/mnt/app/armle/usr/bin
export PATH
unset LD_PRELOAD

case $0 in */*) D=${0%/*} ;; *) D=. ;; esac
D=$(cd "$D" && pwd) || exit 1

# GEM runs on MMX; forward a manual RCC launch (false once on MMX, so no loop).
if [ ! -d /mnt/app/eso/hmi/lsd ] && [ -d /net/mmx/mnt/app/eso/hmi/lsd ]; then
    exec on -f mmx /bin/sh "$D/custom.sh" "$@"
fi

SRC=$D/mibpkg
HOME_DIR=/mnt/app/root/mibpkg
LOG=/tmp/mibpkg.log

for f in mibpkg launch.sh gem.sh; do
    if [ ! -f "$SRC/$f" ]; then
        echo "mibpkg: missing $SRC/$f on the card"
        exit 1
    fi
done

mount -uw /mnt/app || { echo "mibpkg: cannot mount /mnt/app read-write"; exit 1; }
ok=1
[ -d "$HOME_DIR/bin" ] || mkdir -p "$HOME_DIR/bin" || ok=0
for f in mibpkg launch.sh gem.sh; do
    cp "$SRC/$f" "$HOME_DIR/bin/$f.new" && chmod 755 "$HOME_DIR/bin/$f.new" && mv "$HOME_DIR/bin/$f.new" "$HOME_DIR/bin/$f" || ok=0
done
# CA roots for online sources; optional so older cards still bootstrap.
if [ -f "$SRC/ca.pem" ]; then
    cp "$SRC/ca.pem" "$HOME_DIR/bin/ca.pem.new" && mv "$HOME_DIR/bin/ca.pem.new" "$HOME_DIR/bin/ca.pem" || ok=0
fi
sync
mount -ur /mnt/app
if [ $ok != 1 ]; then
    echo "mibpkg: copying files to $HOME_DIR failed"
    exit 1
fi

cp "$HOME_DIR/bin/mibpkg" /tmp/mibpkg.new && chmod 755 /tmp/mibpkg.new && mv /tmp/mibpkg.new /tmp/mibpkg || exit 1
if /tmp/mibpkg bootstrap >>"$LOG" 2>&1; then
    echo "mibpkg installed: open the engineering menu -> mibpkg"
else
    echo "mibpkg bootstrap failed, see $LOG"
    exit 1
fi
