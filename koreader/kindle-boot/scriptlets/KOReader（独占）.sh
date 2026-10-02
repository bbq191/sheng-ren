#!/bin/sh
# Name: KOReader（独占）
# Author: sheng-ren
# 书库里点开就运行：停掉亚马逊界面、独占打开 KOReader；在 KOReader 里退出就整机重启回 Kindle 自带界面（见 koreader/kindle-boot/）。
# 要在后台跑：停界面时会连带杀掉从书库启动的这个脚本。
RUN=/mnt/us/extensions/koreader-boot/bin/run.sh
if command -v setsid >/dev/null 2>&1; then
    setsid /bin/sh "$RUN" >/dev/null 2>&1 </dev/null &
else
    nohup /bin/sh "$RUN" >/dev/null 2>&1 </dev/null &
fi
