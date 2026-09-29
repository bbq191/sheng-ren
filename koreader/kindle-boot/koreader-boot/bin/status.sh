#!/bin/sh
# shellcheck source=common.sh
. "$(dirname "$0")/common.sh"
if [ -f "$CONF" ] && [ -f "$FLAG" ]; then say "开机启动：开"
elif [ -f "$CONF" ]; then say "开机启动：已装，但开关文件没了（关）"
else say "开机启动：没装"; fi
