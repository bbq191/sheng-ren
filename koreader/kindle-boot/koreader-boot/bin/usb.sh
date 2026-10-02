#!/bin/sh
# USB 传书（2026-10-02 用户要）：KOReader 已经退出、亚马逊界面停着，在这里等用户插线、让系统自己开 MTP，拔线后返回（run.sh 再起 KOReader）。
# KOReader 开着时不能开 USB：它的程序文件在书库分区上，电脑接管时 KOReader 会 SIGBUS 崩溃（2026-10-02 实验），所以要先退出。
#
# 什么都不碰，让系统走正常流程（2026-10-02 真机，USB 诊断抓的正常插线日志）：插线 → powerd 发 usbConfigured → volumd 等 800ms 看有没有人
# 要推迟（没人）→ DRIVE_MODE_ON → mtp-responder（tizen-mtp）自己启用 MTP（mtp.sh 把控制器交给 mtpgadget）。亚马逊界面（blanket）只画提示画面，
# 不参与。koreader.sh 退出时已经恢复了 volumd（KOReader 运行时把它暂停）。
# 前两版照社区 zen-mtp 的做法自己重启 mtp、把控制器接到 mtpgadget：电脑认得到设备，但 volumd 收不到 usbConfigured、走"超时"分支不启用 MTP，
# mtp-responder 停在 mtp_state 0、所有请求都跳过，资源管理器进不去。
# 第三版什么都不碰，系统确实开了 MTP，但随后 volumd 发 userstoreIsLikelyToUnMount、等有人回"可以接管书库"（写它的 lipc 属性
# userstoreReadyToUnMount），10 秒没人回就走 "plug_in Timeout … Never Mind"、不进 DRIVE_MODE_ON，MTP 又被关掉。正常时回话的是亚马逊界面
# （它先关掉书库里打开的文件）；独占时界面停着，由这里替它回——KOReader 已经退出，书库里没有打开的文件。
# 连上 = mtpgadget 接着控制器、状态 configured/suspended；拔线后系统自己把控制器收回。下次 KOReader 起来又会暂停 volumd，插线不再露出 USB。
# 屏幕提示用 eips，只能写 ASCII。
GDIR=/sys/kernel/config/usb_gadget/mtpgadget
UDC=
for u in /sys/class/udc/*; do [ -e "$u" ] && UDC=${u##*/} && break; done
# 详细日志写书库分区（重启后还在：要读它得先退出 KOReader，退出就重启，/tmp 会清空）
LOG=/mnt/us/koreader-boot-usb.log
log() { echo "$(date '+%F %T') $*" >>"$LOG"; }
# 一行现状：mtpgadget 接的控制器和状态、充电口（有没有插线）、volumd（T = 被暂停）
snap() {
    a=$(cat "$GDIR/UDC" 2>/dev/null)
    st=$(cat "/sys/class/udc/$UDC/state" 2>/dev/null)
    ps_=""
    for f in /sys/class/power_supply/*/online; do [ -r "$f" ] && ps_="$ps_ ${f#/sys/class/power_supply/}=$(cat "$f")"; done
    vol=$(ps -o stat,comm 2>/dev/null | awk '$2 == "volumd" {print $1}')
    echo "mtpgadget.UDC=[${a}] state=${st} power:${ps_% } volumd=${vol:-?}"
}
say() { eips -c >/dev/null 2>&1; eips -c >/dev/null 2>&1; i=3; for line in "$@"; do eips 2 "$i" "$line" >/dev/null 2>&1; i=$((i + 2)); done; }
connected() {
    [ -n "$(cat "$GDIR/UDC" 2>/dev/null)" ] || return 1
    case $(cat "/sys/class/udc/$UDC/state" 2>/dev/null) in configured | suspended) return 0 ;; esac
    return 1
}
finish() {
    [ -n "$WAITER" ] && kill "$WAITER" 2>/dev/null
    log "结束：$1（$(snap)）"
    echo "$(date '+%F %T') USB 传书：$1" >>/mnt/us/koreader-boot.log
    { echo "--- 系统日志里 volumd/mtp 相关的最后 40 行"
      grep -a -i -E "volumd|mtp-responder|mtp:|usbConfigured|DRIVE_MODE" /var/log/messages 2>/dev/null | grep -v BroadcastController | tail -40; echo "---"; } >>"$LOG"
    say "Back to KOReader ..."
    exit 0
}

echo "======== $(date '+%F %T')" >>"$LOG"
log "开始 UDC=$UDC：$(snap)"
if [ ! -d "$GDIR" ] || [ -z "$UDC" ]; then
    say "USB transfer is not supported on this Kindle." "(no $GDIR)"
    sleep 5
    finish "没有 mtpgadget 或 UDC，不支持"
fi
# volumd 应该已经被 koreader.sh 恢复了；万一还停着就恢复（不然插线没人处理）
case $(ps -o stat,comm 2>/dev/null | awk '$2 == "volumd" {print $1}') in T*) killall -CONT volumd 2>/dev/null && log "volumd 还停着，恢复了" ;; esac
trap '' TERM HUP

# 替亚马逊界面回话：每收到一次 userstoreIsLikelyToUnMount 就回 userstoreReadyToUnMount=1（插线前就开始等，免得事件先到）
WAITER=
(
    while lipc-wait-event -s 130 com.lab126.volumd userstoreIsLikelyToUnMount >/dev/null 2>&1; do
        lipc-set-prop com.lab126.volumd userstoreReadyToUnMount 1 >/dev/null 2>&1
        log "收到 userstoreIsLikelyToUnMount，回了 userstoreReadyToUnMount=1（rc=$?）"
    done
) &
WAITER=$!
say "USB transfer: plug in the cable." "Unplug it to return to KOReader."
# 等系统开好 MTP、电脑连上（最多 120 秒），每秒记一次现状（有变化才记）
t=0 last=""
until connected || [ $t -ge 120 ]; do
    sleep 1
    t=$((t + 1))
    now=$(snap)
    [ "$now" != "$last" ] && log "${t}s $now" && last=$now
done
connected || finish "120 秒内电脑没连上"
log "电脑连上了（${t} 秒）"
say "USB transfer: connected." "Unplug the cable to return to KOReader."
gone=0
while [ $gone -lt 2 ]; do
    sleep 2
    if connected; then gone=0; else gone=$((gone + 1)); fi
    now=$(snap)
    [ "$now" != "$last" ] && log "连着 $now" && last=$now
done
finish "拔线"
