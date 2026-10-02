# shellcheck shell=sh disable=SC2034
# install.sh / uninstall.sh / status.sh 共用。由脚本书（documents/KOReader开机启动-*.sh）或 KUAL 以 root 身份运行。
CONF=/etc/upstart/koreader-boot.conf
FLAG=/mnt/us/koreader-boot.enabled
HERE=$(cd "$(dirname "$0")/.." && pwd)
LOG=/mnt/us/koreader-boot.log
# 结果：记进 U 盘根目录的 koreader-boot.log（插上电脑就能看），屏幕上打一行字（eips 列 1、行 2）并停 6 秒——
# 脚本书运行完屏幕马上刷回书库，不停一下看不清（2026-09-29 用户反馈）
say() {
    echo "$(date '+%F %T') $1" >>"$LOG"
    echo "$1"
    eips 1 2 "$1                                  " >/dev/null 2>&1
    sleep 6
}
