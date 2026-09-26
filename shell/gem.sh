#!/bin/sh
# Target of every mibpkg GEM button: gem.sh <action> [args...]
# The button scripts in /mnt/app/root/mibpkg/gem/ exec this with fixed arguments.
# GEM and boot environments may not have /sbin (mount) on PATH.
PATH=/proc/boot:/bin:/usr/bin:/usr/sbin:/sbin:/mnt/app/armle/bin:/mnt/app/armle/usr/bin:$PATH
export PATH
HOME_DIR=${MIBPKG_HOME:-/mnt/app/root/mibpkg}
RUN=${MIBPKG_RUN:-/tmp}
if [ ! -x "$RUN/mibpkg" ]; then
    cp "$HOME_DIR/bin/mibpkg" "$RUN/mibpkg.new" || exit 1
    chmod 755 "$RUN/mibpkg.new"
    mv "$RUN/mibpkg.new" "$RUN/mibpkg" || exit 1
fi
exec "$RUN/mibpkg" $MIBPKG_ARGS gem "$@" >>"$RUN/mibpkg.log" 2>&1
