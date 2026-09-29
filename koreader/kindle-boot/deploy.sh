#!/usr/bin/env bash
# 把 KUAL 扩展 koreader-boot/ 经 USB（MTP）拷到 Kindle 的 extensions/，之后在 Kindle 上 KUAL → 「KOReader 开机启动 → 装上」。
# 开机任务要写进 /etc/upstart/（根分区），电脑经 MTP 写不到，只能在 Kindle 上由 KUAL 以 root 执行一次。
#
# 用法: koreader/kindle-boot/deploy.sh [--write]      不带 --write 只列出要拷什么
#       koreader/kindle-boot/deploy.sh --remove --write  从 Kindle 删掉这个扩展（先在 KUAL 里"卸掉"开机任务）
# 装上以后开机：亚马逊界面起来 → 打开 KOReader 并停掉亚马逊界面（省电省内存）→ 退出 KOReader 时界面回来。
# 临时关掉：删掉 Kindle U 盘根目录的 koreader-boot.enabled（本脚本 --disable --write 也行）。
set -euo pipefail
KO_HERE=$(cd "$(dirname "$0")/.." && pwd)
# shellcheck source=../lib.sh
source "$KO_HERE/lib.sh"
EXT_SRC="$KO_HERE/kindle-boot/koreader-boot"
write=0 mode=install
for a in "$@"; do
  case $a in
    --write) write=1 ;;
    --remove) mode=remove ;;
    --disable) mode=disable ;;
    -h | --help) sed -n '2,9p' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
    *) echo "不认识的参数 $a" >&2; exit 2 ;;
  esac
done
ko_connect kindle-pw12-sig
root=${KO_BASE%/koreader}
has() { gio info "$root/$1" >/dev/null 2>&1; }
has extensions || { echo "✗ Kindle 上没有 extensions/（KUAL 没装？）：先装 KUAL" >&2; exit 1; }
has koreader/koreader.sh || { echo "✗ Kindle 上没有 koreader/koreader.sh" >&2; exit 1; }

case $mode in
  disable)
    if has koreader-boot.enabled; then
      echo "删 koreader-boot.enabled（开机不再自启，开机任务还在）"
      [[ $write -eq 1 ]] && gio remove "$root/koreader-boot.enabled"
    else echo "= 开关文件本来就没有"; fi ;;
  remove)
    if has extensions/koreader-boot; then
      echo "删 extensions/koreader-boot/（先在 KUAL 里「卸掉」，不然 /etc/upstart 里的开机任务还在）"
      if [[ $write -eq 1 ]]; then
        for f in bin/common.sh bin/install.sh bin/uninstall.sh bin/status.sh bin koreader-boot.conf menu.json config.xml; do
          has "extensions/koreader-boot/$f" && gio remove "$root/extensions/koreader-boot/$f"
        done
        gio remove "$root/extensions/koreader-boot"
      fi
    else echo "= 扩展本来就没有"; fi ;;
  install)
    files=(config.xml menu.json koreader-boot.conf bin/common.sh bin/install.sh bin/uninstall.sh bin/status.sh)
    for f in "${files[@]}"; do echo "拷 extensions/koreader-boot/$f"; done
    if [[ $write -eq 1 ]]; then
      has extensions/koreader-boot || gio mkdir "$root/extensions/koreader-boot"
      has extensions/koreader-boot/bin || gio mkdir "$root/extensions/koreader-boot/bin"
      for f in "${files[@]}"; do
        has "extensions/koreader-boot/$f" && gio remove "$root/extensions/koreader-boot/$f"
        gio copy "$EXT_SRC/$f" "$root/extensions/koreader-boot/$f"
        [[ $(gio info -a standard::size "$root/extensions/koreader-boot/$f" | awk '/standard::size:/ {print $2}') == $(stat -c %s "$EXT_SRC/$f") ]] \
          || { echo "✗ $f 拷过去大小不对" >&2; exit 1; }
      done
    fi ;;
esac
if [[ $write -eq 1 ]]; then
  echo "✓ 完成"
  [[ $mode == install ]] && echo "  下一步：拔掉 USB，在 Kindle 上打开 KUAL →「KOReader 开机启动」→「装上」，然后重启 Kindle 试。"
else
  echo "（dry run：加 --write 才真的写）"
fi
