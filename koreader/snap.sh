#!/usr/bin/env bash
# 在电脑上用本机 KOReader 按设备的配置打开一本书、逐页截图，不用真机就能看分页、注释、漫画边距。
#
# 用法: koreader/snap.sh <书.epub> [<设备 id>] [--pages=<页数>] [--out=<目录>] [--links]
#   设备缺省 ireader-ocean5-pro；窗口按设备屏幕尺寸（devices/<id>/device.conf 的 SCREEN_W/SCREEN_H，缺省 1264×1680）。
#   截图写到 --out（缺省临时目录，结束时打印位置）：pNNN.png + info.txt（总页数、每页开头的 xpointer）。
#   --links：不截图，检查前 --pages 页（缺省改成 100000，即全书）上的书内链接在 KOReader 里点了是弹窗还是跳转
#            （用 KOReader 自己的注释判定），写 links.txt，末行合计。
# 隔离运行：KO_HOME 指向临时目录，里面是按 apply.sh 同样的顺序从空配置合并出来的设置（个人 → 方案 → 设备 → 预设），
# 不碰本机 KOReader 的设置；书先复制到临时目录，KOReader 写的 .sdr 不会落到原书旁边。窗口不显示（SDL offscreen）。
# 要本机装了 KOReader（/usr/lib/koreader 或 $KOREADER_HOME）和 luajit。
# 注意：本机没装设备上的字体（霞鹜文楷等）时会退回 KOReader 自带字体，行数和真机不同；分页位置（哪个文件从新页开始）不受影响。
set -euo pipefail
KO_HERE=$(cd "$(dirname "$0")" && pwd)
# shellcheck source=lib.sh
source "$KO_HERE/lib.sh"

book='' dev=ireader-ocean5-pro pages='' out='' links=0
for a in "$@"; do
  case $a in
    --pages=*) pages=${a#--pages=} ;;
    --out=*) out=${a#--out=} ;;
    --links) links=1 ;;
    -h | --help) sed -n '2,13p' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
    -*) echo "不认识的参数 $a" >&2; exit 2 ;;
    *) if [[ -z $book ]]; then book=$a; else dev=$a; fi ;;
  esac
done
[[ -f $book ]] || { echo "用法: $0 <书.epub> [<设备 id>] [--pages=N] [--out=目录] [--links]" >&2; exit 2; }
[[ -n $pages ]] || { [[ $links -eq 1 ]] && pages=100000 || pages=20; }
[[ -f $KO_HERE/devices/$dev/device.conf ]] || { echo "✗ 没有设备 $dev" >&2; exit 2; }
ko_home=${KOREADER_HOME:-/usr/lib/koreader}
[[ -x $ko_home/luajit && -f $ko_home/reader.lua ]] || { echo "✗ 没找到本机 KOReader（$ko_home）；装好后再试，或设 KOREADER_HOME" >&2; exit 1; }
command -v luajit >/dev/null || { echo "✗ 缺 luajit" >&2; exit 1; }

SCREEN_W=1264 SCREEN_H=1680
# shellcheck source=/dev/null
source "$KO_HERE/devices/$dev/device.conf"
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
mkdir -p "$tmp/home/patches" "$tmp/book"
ko_merge_all "$tmp/home" "$dev"
cp "$KO_HERE/snap/2-snap.lua" "$tmp/home/patches/"
cp "$book" "$tmp/book/"
[[ -n $out ]] || out=$(mktemp -d)
mkdir -p "$out"
out=$(cd "$out" && pwd)

(cd "$ko_home" && KO_HOME=$tmp/home EMULATE_READER_W=$SCREEN_W EMULATE_READER_H=$SCREEN_H SDL_VIDEO_DRIVER=offscreen \
  KOSNAP_OUT=$out KOSNAP_PAGES=$pages KOSNAP_LINKS=$links timeout 600 ./luajit reader.lua "$tmp/book/$(basename "$book")") >"$out/koreader.log" 2>&1 || true
if [[ $links -eq 1 ]]; then
  [[ -f $out/links.txt ]] || { echo "✗ 没查到链接，见 $out/koreader.log" >&2; exit 1; }
  echo "✓ $(tail -1 "$out/links.txt")，明细在 $out/links.txt"
else
  [[ -f $out/info.txt ]] || { echo "✗ 没截到图，见 $out/koreader.log" >&2; exit 1; }
  echo "✓ $(head -1 "$out/info.txt")，截图在 $out"
fi
