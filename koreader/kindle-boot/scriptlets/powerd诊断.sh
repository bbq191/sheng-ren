#!/bin/sh
# Name: powerd 诊断
# Author: sheng-ren
# 书库里点开就运行：把 powerd 的接口、状态、程序里的状态名和 USB 事件写到 U 盘根目录的 powerd-diag.txt（2026-10-02，USB 传书摸底：
# 独占时 powerd 收到 hal 的 usbConfigured 却不转发，见 koreader/README.md）。只读，不改任何设置。
OUT=/mnt/us/powerd-diag.txt
{
    echo "==== $(date '+%F %T')"
    echo "==== lipc：com.lab126.powerd 的属性和当前值"
    lipc-probe -v com.lab126.powerd 2>&1 | head -80
    echo "==== powerd 当前状态"
    for p in state status powerStatus isCharging; do printf '%s=' "$p"; lipc-get-prop com.lab126.powerd "$p" 2>&1 | head -3; done
    echo "==== powerd 程序里的状态名、USB 事件"
    b=$(command -v powerd); [ -f "$b" ] || b=/usr/bin/powerd
    echo "-- $b"
    grep -a -o -E "[A-Za-z_:.]*([Uu]sb|USB|[Cc]onfigured)[A-Za-z_:.]*" "$b" 2>/dev/null | sort -u | head -120
    echo "-- 状态机"
    grep -a -o -E "(e_|s_|STATE_|state_)[A-Za-z_]+|[A-Za-z_]*(ScreenSaver|screenSaver|readyToSuspend|ReadyToSuspend|suspended|Suspend|active|Active)[A-Za-z_]*" "$b" 2>/dev/null | sort -u | head -150
    echo "==== powerd 程序里的解锁、密码、锁屏相关名字（插线后等'Password is Valid'才发 usbConfigured）"
    grep -a -o -E "[A-Za-z_:.]*([Pp]assword|[Pp]asscode|PASSCODE|[Uu]nlock|[Ll]ockscreen|LockScreen|[Aa]uthenticat|DRIVE_MODE)[A-Za-z_:. ]*" "$b" 2>/dev/null | sort -u | head -80
    echo "==== 解锁那一刻前后 5 秒的全部系统日志（找谁告诉 powerd 解锁了）"
    t=$(grep -a "Password is Valid" /var/log/messages 2>/dev/null | tail -1 | cut -d' ' -f1)
    echo "解锁时间：$t"
    if [ -n "$t" ]; then
        # 时间戳形如 261002:195320.922：取冒号后的时分秒，换成秒数，打出前 5 秒到后 2 秒的行
        grep -a -E "^[0-9]+:[0-9]{6}\." /var/log/messages 2>/dev/null | awk -v t="$t" '
            function sec(x) { split(x, a, ":"); h = substr(a[2], 1, 2); m = substr(a[2], 3, 2); s = substr(a[2], 5, 2); return h * 3600 + m * 60 + s }
            BEGIN { c = sec(t) }
            { v = sec($1); if (v >= c - 5 && v <= c + 2) print }' | grep -v -E "metric|BroadcastController|bd71827|clamp_soc" | cut -c1-220 | tail -120
    fi
    echo "==== 解锁管道 authenticator_pipe：powerd 里的路径、设备上的命名管道、谁开着它"
    grep -a -o -E "/[A-Za-z0-9_./-]*(auth|pipe|fifo|Auth)[A-Za-z0-9_./-]*" "$b" 2>/dev/null | sort -u
    for d in /var/run /var/local /var/tmp /tmp /run /dev; do find "$d" -maxdepth 3 -type p 2>/dev/null; done | sort -u | while read -r f; do
        echo "-- 命名管道 $f"; ls -l "$f"
        for fd in /proc/[0-9]*/fd/*; do [ "$(readlink "$fd" 2>/dev/null)" = "$f" ] && echo "   开着它：pid ${fd#/proc/} $(cat "/proc/$(echo "${fd#/proc/}" | cut -d/ -f1)/cmdline" 2>/dev/null | tr '\0' ' ')"; done
    done
    echo "==== 哪些程序、库里出现 authenticator（谁往管道里写）"
    for f in /usr/bin/* /usr/sbin/* /bin/* /sbin/* /usr/lib/*.so* /usr/java/lib/*.jar /opt/amazon/*/*; do
        [ -f "$f" ] && grep -a -q -E "authenticator|AUTHENTICATOR" "$f" 2>/dev/null && echo "$f"
    done 2>/dev/null | head -20
    echo "==== 这些程序里和 authenticator、密码校验有关的字符串（写进管道的格式）"
    for f in /usr/bin/* /usr/sbin/* /usr/lib/*.so*; do
        [ -f "$f" ] || continue
        [ "$f" = "$b" ] && continue
        grep -a -q "authenticator" "$f" 2>/dev/null || continue
        echo "-- $f"
        grep -a -o -E "[A-Za-z0-9_:./%-]*(authenticat|Authenticat|password|Password|passcode|valid|Valid)[A-Za-z0-9_:./% -]*" "$f" 2>/dev/null | sort -u | head -60
    done
    echo "==== /var/run（解锁管道 authenticator_pipe 平时在不在）"
    ls -la /var/run/ 2>&1 | head -60
    echo "==== 系统日志里 powerd 的 usb / 状态切换（最后 120 行）"
    grep -a "powerd\[" /var/log/messages 2>/dev/null | grep -a -i -E "usb|sm:|state|screensaver|suspend" | grep -v metric | tail -120
} >"$OUT" 2>&1
sync
eips 1 2 "powerd-diag.txt written                  " >/dev/null 2>&1
sleep 3
