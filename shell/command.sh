#!/bin/sh
# M.I.B. release zips (tags up to V3.7.1) run /mod/command.sh from
# GEM -> M.I.B. -> Advanced Settings -> "Run individual script"; newer M.I.B.
# runs /mod/custom.sh from "Run Custom Script". FAT cannot hold a symlink,
# so this forwards to custom.sh. When the M.I.B. launcher sources this file
# while installing M.I.B. itself, $0 is the launcher and this does nothing.
case ${0##*/} in
    command.sh) exec /bin/sh "${0%/*}/custom.sh" "$@" ;;
esac
