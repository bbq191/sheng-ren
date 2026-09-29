# shellcheck shell=sh disable=SC2034
# install.sh / uninstall.sh / status.sh 共用。KUAL 以 root 身份运行。
CONF=/etc/upstart/koreader-boot.conf
FLAG=/mnt/us/koreader-boot.enabled
HERE=$(cd "$(dirname "$0")/.." && pwd)
# 在屏幕上打一行字（eips 列 1、行 2；后面补空格盖掉上次的字）
say() { eips 1 2 "$1                                  " >/dev/null 2>&1; }
