#!/bin/sh
# 垫在 koreader.sh 的 PATH 最前面的 start（run.sh 拷成 /var/tmp/koreader-boot-shim/start）。
# KOReader 退出（或崩溃）后 koreader.sh 执行 `start lab126_gui` 拉回亚马逊界面；这里换成整机重启，并留下记号
# koreader-boot.native，开机任务看到它这次就不自启，开机停在 Kindle 自带界面（只这一次）。别的 start 原样转给系统的 start。
if [ "$1" = "lab126_gui" ]; then
    # KOReader 是为 USB 传书退出的（插件留的记号）：界面不拉、也不重启，run.sh 接着开 MTP、传完再起 KOReader
    [ -e /tmp/koreader-boot.usb ] && exit 0
    touch /mnt/us/koreader-boot.native
    rm -f /mnt/us/koreader-boot.running
    CLEAN=/mnt/us/extensions/koreader-boot/bin/clean-dumps.sh
    [ -f "$CLEAN" ] && /bin/sh "$CLEAN"
    eips -c >/dev/null 2>&1
    eips 1 2 "Rebooting to Kindle home ..." >/dev/null 2>&1
    sync
    exec reboot
fi
for d in /sbin /usr/sbin /bin /usr/bin; do
    [ -x "$d/start" ] && exec "$d/start" "$@"
done
echo "start-shim: 找不到系统的 start" >&2
exit 127
