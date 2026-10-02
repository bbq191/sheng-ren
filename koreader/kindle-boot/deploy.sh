#!/usr/bin/env bash
# 把 koreader-boot/ 经 USB（MTP）拷到 Kindle 的 extensions/，两本脚本书拷到 documents/；之后在 Kindle 书库里点开
# 「KOReader 开机启动：装上」（装了 KUAL 的也可以在 KUAL 菜单里点）。
# 开机任务要写进 /etc/upstart/（根分区），电脑经 MTP 写不到，只能在 Kindle 上以 root 执行一次（脚本书就是 root）。
# 2026-09-29 这台 Kindle 是 KindleModding 的 kpm 装的 KOReader（documents/KOReader.sh 脚本书），没有 KUAL。
#
# 用法: koreader/kindle-boot/deploy.sh [--write]      不带 --write 只列出要拷什么
#       koreader/kindle-boot/deploy.sh --remove --write  从 Kindle 删掉这个扩展（先在书库里"卸掉"开机任务）
#       电脑上没有 gvfs-mtp 时：KO_LOCAL_ROOT=/run/user/1000/mtp/kindle koreader/kindle-boot/deploy.sh …（jmtpfs 挂好的目录）
# 装上以后开机：亚马逊界面起来 → 停掉亚马逊界面、独占打开 KOReader → 在 KOReader 里退出就整机重启，停在 Kindle 自带界面；
# 自带界面里点脚本书「KOReader（独占）」再回去（2026-10-02 起，见 koreader/README.md）。
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
    -h | --help) sed -n '2,11p' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
    *) echo "不认识的参数 $a" >&2; exit 2 ;;
  esac
done
ko_connect kindle-pw12-sig
root=${KO_BASE%/koreader}
# 设备上的文件操作（路径相对 U 盘根目录）：gio，或 KO_LOCAL_ROOT 时按普通文件
if [[ -n ${KO_LOCAL_ROOT:-} ]]; then
  has() { [[ -e $root/$1 ]]; }
  mk() { mkdir -p "$root/$1"; }
  del() { if [[ -d $root/$1 ]]; then rmdir "$root/$1"; else rm -f "$root/$1"; fi; }
  put() { cp "$1" "$root/$2"; }
  size() { stat -c %s "$root/$1"; }
else
  has() { gio info "$root/$1" >/dev/null 2>&1; }
  mk() { gio mkdir "$root/$1"; }
  del() { gio remove "$root/$1"; }
  put() { gio copy "$1" "$root/$2"; }
  size() { gio info -a standard::size "$root/$1" | awk '/standard::size:/ {print $2}'; }
fi
has extensions || mk extensions
has koreader/koreader.sh || { echo "✗ Kindle 上没有 koreader/koreader.sh" >&2; exit 1; }

case $mode in
  disable)
    if has koreader-boot.enabled; then
      echo "删 koreader-boot.enabled（开机不再自启，开机任务还在）"
      [[ $write -eq 1 ]] && del koreader-boot.enabled
    else echo "= 开关文件本来就没有"; fi ;;
  remove)
    if has extensions/koreader-boot; then
      echo "删 extensions/koreader-boot/ 和两本脚本书（先在书库里点「KOReader 开机启动：卸掉」，不然 /etc/upstart 里的开机任务还在）"
      if [[ $write -eq 1 ]]; then
        for f in bin/common.sh bin/install.sh bin/uninstall.sh bin/status.sh bin/clean-dumps.sh bin/run.sh bin/start-shim.sh bin/usb.sh bin/exit-log.sh bin koreader-boot.conf menu.json config.xml; do
          has "extensions/koreader-boot/$f" && del "extensions/koreader-boot/$f"
        done
        del extensions/koreader-boot
        for f in "KOReader开机启动-装上.sh" "KOReader开机启动-卸掉.sh" "KOReader（独占）.sh"; do has "documents/$f" && del "documents/$f"; done
      fi
    else echo "= 扩展本来就没有"; fi ;;
  install)
    files=(config.xml menu.json koreader-boot.conf bin/common.sh bin/install.sh bin/uninstall.sh bin/status.sh bin/clean-dumps.sh bin/run.sh bin/start-shim.sh bin/usb.sh)
    for f in "${files[@]}"; do echo "拷 extensions/koreader-boot/$f"; done
    scriptlets=("KOReader开机启动-装上.sh" "KOReader开机启动-卸掉.sh" "KOReader（独占）.sh")
    for f in "${scriptlets[@]}"; do echo "拷 documents/$f（书库里显示成一本书，点开就运行）"; done
    if [[ $write -eq 1 ]]; then
      has extensions/koreader-boot || mk extensions/koreader-boot
      has extensions/koreader-boot/bin || mk extensions/koreader-boot/bin
      for f in "${files[@]}"; do
        has "extensions/koreader-boot/$f" && del "extensions/koreader-boot/$f"
        put "$EXT_SRC/$f" "extensions/koreader-boot/$f"
        [[ $(size "extensions/koreader-boot/$f") == $(stat -c %s "$EXT_SRC/$f") ]] || { echo "✗ $f 拷过去大小不对" >&2; exit 1; }
      done
      for f in "${scriptlets[@]}"; do
        has "documents/$f" && del "documents/$f"
        put "$KO_HERE/kindle-boot/scriptlets/$f" "documents/$f"
      done
    fi ;;
esac
if [[ $write -eq 1 ]]; then
  echo "✓ 完成"
  [[ $mode == install ]] && echo "  下一步：拔掉 USB，在 Kindle 书库里点开「KOReader 开机启动：装上」（开机任务改了，要再点一次），看到「装好了」后重启 Kindle 试。"
else
  echo "（dry run：加 --write 才真的写）"
fi
