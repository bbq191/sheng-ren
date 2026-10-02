#!/bin/sh
# shellcheck source=common.sh
# 删掉开机任务和开关文件。
. "$(dirname "$0")/common.sh"
rm -f "$FLAG"
if [ -f "$CONF" ]; then
  mntroot rw || { say "根分区改不成可写，只删了开关文件（开机不会再自启）"; exit 1; }
  rm -f "$CONF"; sync
  mntroot ro
fi
say "卸掉了：开机回到 Kindle 自带界面"
