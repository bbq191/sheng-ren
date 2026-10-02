#!/bin/sh
# Name: KOReader（独占）
# Author: sheng-ren
# 书库里点开：停掉亚马逊界面、打开 KOReader（和开机自启一样的"独占"方式）；退出 KOReader 时界面回来。
# 书库里原来那本「KOReader」（kpm 装的）是普通方式：亚马逊界面留在后台。见 koreader/kindle-boot/。
# 脱离脚本书的进程再启动：停界面时脚本书的运行器也会被停掉，不脱离的话 KOReader 可能跟着被关。
# （Kindle 的 busybox 不一定带 setsid，没有就用 nohup）
# 启动前、退出后各清一次停界面时偶尔生成的崩溃诊断包（documents/ 下以日期命名的 .txt/.tgz，见 bin/clean-dumps.sh）。
CLEAN=/mnt/us/extensions/koreader-boot/bin/clean-dumps.sh
RUN="[ -f $CLEAN ] && /bin/sh $CLEAN; /bin/sh /mnt/us/koreader/koreader.sh --kual --framework_stop; [ -f $CLEAN ] && /bin/sh $CLEAN"
if command -v setsid >/dev/null 2>&1; then
    setsid /bin/sh -c "$RUN" >/dev/null 2>&1 </dev/null &
else
    nohup /bin/sh -c "$RUN" >/dev/null 2>&1 </dev/null &
fi
