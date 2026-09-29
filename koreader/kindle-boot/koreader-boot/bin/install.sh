#!/bin/sh
# shellcheck source=common.sh
# 把开机任务装进 /etc/upstart/，并建开关文件。根分区平时只读：mntroot rw 写完再 mntroot ro。
. "$(dirname "$0")/common.sh"
if [ ! -f /mnt/us/koreader/koreader.sh ]; then say "没找到 /mnt/us/koreader，先装 KOReader"; exit 1; fi
mntroot rw || { say "根分区改不成可写（mntroot rw 失败），没装"; exit 1; }
cp "$HERE/koreader-boot.conf" "$CONF.tmp" && mv "$CONF.tmp" "$CONF"
ok=$?
sync
mntroot ro
if [ $ok -ne 0 ]; then say "写 $CONF 失败，没装"; exit 1; fi
touch "$FLAG"
say "装好了：下次开机直接进 KOReader"
