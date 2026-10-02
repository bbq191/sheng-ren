#!/bin/sh
# Name: USB 诊断
# Author: sheng-ren
# 书库里点开就运行：把 USB 传书摸底要的信息写到 U 盘根目录的 usb-diag.txt（2026-10-02，见 koreader/kindle-boot/bin/usb.sh）。
# 只读：不改任何设置。要在亚马逊界面开着、USB 正常连过一次之后点（抓正常插线时 volumd、mtp-responder、界面之间的对话）。
OUT=/mnt/us/usb-diag.txt
{
    echo "==== $(date '+%F %T')"
    echo "==== 系统日志：volumd / mtp / userstore / usb 事件（最后 200 行）"
    grep -a -i -E "volumd|mtp|userstore|IPCF4|usbPlug|usbconfigured|usb_plug|blanket|appmgr.*usb" /var/log/messages 2>/dev/null | tail -200
    echo "==== lipc：volumd、mtp 相关服务的属性"
    for s in com.lab126.volumd com.lab126.mtp com.lab126.mtp-responder com.lab126.usbnet; do
        echo "-- $s"; lipc-probe -v "$s" 2>&1 | head -40
    done
    echo "==== lipc 服务列表"
    lipc-probe -a 2>&1 | head -80
    echo "==== 和 mtp 有关的程序、任务"
    ls -la /usr/bin /usr/sbin /bin /sbin 2>/dev/null | grep -i -E "mtp|volum"
    ls /etc/upstart 2>/dev/null | grep -i -E "mtp|volum|usb"
    echo "-- /etc/upstart/mtp.conf"; cat /etc/upstart/mtp.conf 2>/dev/null
    echo "==== volumd、mtp-responder 程序里的名字（MTP 启停、userstore、事件）"
    for b in $(command -v volumd) /usr/bin/volumd /usr/sbin/volumd $(command -v mtp-responder) /usr/bin/mtp-responder /usr/sbin/mtp-responder; do
        [ -f "$b" ] || continue
        echo "-- $b"
        grep -a -o -E "[A-Za-z_:.]*(Mtp|MTP|mtp|userstore|Userstore|IPCF4)[A-Za-z_:.]*" "$b" 2>/dev/null | sort -u | head -150
    done
    echo "==== ps"
    ps 2>/dev/null | grep -i -E "mtp|volum|blanket|appmgr|framework|lab126" | grep -v grep
} >"$OUT" 2>&1
sync
eips 1 2 "usb-diag.txt written                    " >/dev/null 2>&1
sleep 3
