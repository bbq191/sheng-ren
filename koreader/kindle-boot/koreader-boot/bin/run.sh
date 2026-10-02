#!/bin/sh
# 独占跑 KOReader，退出就整机重启、停在 Kindle 自带界面（用户 2026-10-02 要）。开机任务和脚本书「KOReader（独占）」都调它，
# 要脱离调用方在后台跑（setsid）：停亚马逊界面时会连带杀掉从界面启动的进程。
# 独占 = koreader.sh --framework_stop：进 KOReader 前停掉亚马逊界面，退出时 `start lab126_gui` 拉回来。一次开机里第三次拉回时
# 主界面解锁后画不出来、白屏（2026-10-02 真机，亚马逊闭源程序里的问题；社区也有同样的报告，没人解决）。所以在 koreader.sh 的 PATH
# 最前面垫一个 start（start-shim.sh），把拉回界面换成重启：界面一次也不重启，白屏的条件就没了。
# USB 传书（2026-10-02）：KOReader 里点「USB 传书」（plugins/usbtransfer.koplugin）会留下 /tmp/koreader-boot.usb 再退出；
# 垫片看到它就不重启，这里接着开 MTP（bin/usb.sh，拔线才返回），然后再起 KOReader——仍经垫片。
BIN=$(cd "$(dirname "$0")" && pwd)
SHIM=/var/tmp/koreader-boot-shim
USB_FLAG=/tmp/koreader-boot.usb
# 垫片拷到 /var/tmp 再加执行权限（书库分区上的文件不一定能直接执行；koreader.sh 自己也是这么做的）。拷不过去就不独占，停在自带界面。
mkdir -p "$SHIM" && cp -f "$BIN/start-shim.sh" "$SHIM/start" && chmod 755 "$SHIM/start" || exit 1
[ -f "$BIN/clean-dumps.sh" ] && /bin/sh "$BIN/clean-dumps.sh"
rm -f "$USB_FLAG"
touch /mnt/us/koreader-boot.running
while :; do
    # KOREADER_BOOT=1：告诉 USB 传书插件，KOReader 是本脚本起的（退出后有人接着开 MTP、再把它起回来）
    KOREADER_BOOT=1 PATH="$SHIM:$PATH" /bin/sh /mnt/us/koreader/koreader.sh --kual --framework_stop
    [ -e "$USB_FLAG" ] || break
    /bin/sh "$BIN/usb.sh"
    rm -f "$USB_FLAG"
done
# 正常走不到这里（垫片直接重启了）。走到了说明垫片没接管，koreader.sh 已经照原样拉回了界面：收尾。
rm -f /mnt/us/koreader-boot.running
[ -f "$BIN/clean-dumps.sh" ] && /bin/sh "$BIN/clean-dumps.sh"
exit 0
