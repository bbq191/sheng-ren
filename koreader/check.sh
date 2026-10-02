#!/usr/bin/env bash
# 离线检查 koreader/ 的补丁，不碰设备。对每台设备（缺省全部）：
#   1. 对一个 KOReader 目录的副本（缺省是空目录）按 apply.sh 的顺序应用一遍，再应用第二遍必须零净改动；结果都要能被 Lua 读回；
#   2. 卸载（unmerge）后：文字书方案、设备差异撤掉，个人设置还在；拿应用前的副本当"原始配置"时能完全还原；
#   3. patches/*.lua、plugins/*/*.lua 能编译，device.conf 语法正确；luaser 的序列化和合并边界（inf/nan、大整数、布尔键、数组整体替换）。
# 用法: koreader/check.sh [设备 id] [KOReader 目录副本]
set -euo pipefail
KO_HERE=$(cd "$(dirname "$0")" && pwd)
# shellcheck source=lib.sh
source "$KO_HERE/lib.sh"
command -v luajit >/dev/null || { echo "✗ 缺 luajit" >&2; exit 1; }
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT

devs=("${1:-}")
[[ -z ${1:-} ]] && mapfile -t devs < <(ls "$KO_HERE/devices")
copy_src=${2:-}

snap() { # snap <从> <到>
  mkdir -p "$2/settings"
  local f
  for f in "${KO_FILES[@]}"; do if [[ -f $1/$f ]]; then mkdir -p "$(dirname "$2/$f")"; cp "$1/$f" "$2/$f"; fi; done
}
net_changes() { # net_changes <旧目录> <新目录>：有净差异的文件数
  local f n=0 rc
  for f in "${KO_FILES[@]}"; do
    rc=0; luajit "$KO_HERE/diff.lua" "$1/$f" "$2/$f" >/dev/null || rc=$?
    [[ $rc -eq 0 ]] && n=$((n + 1))
  done
  echo $n
}

# ── 与设备无关的检查 ──
for p in "$KO_HERE"/patches/*.lua "$KO_HERE"/plugins/*/*.lua; do
  luajit -b "$p" /dev/null || { echo "✗ $(basename "$p") 编译不过" >&2; exit 6; }
done
luajit -e "
package.path = '$KO_HERE/?.lua;' .. package.path
local L = require('luaser')
local t = { [1] = 1, a = 0/0, b = math.huge, c = -math.huge, d = 2^63, e = 0.1, [true] = 'x', [false] = 'y' }
local back = assert(loadstring('return ' .. L.serialize(t)))()
assert(back.a ~= back.a and back.b == math.huge and back.c == -math.huge and back.d == 2^63 and back.e == 0.1)
assert(back[true] == 'x' and back[false] == 'y')
local dst = { m = { 'x', 'y', 'z' }, keep = 1 }
L.merge(dst, { m = { 'a', 'b' }, keep = L.DELETE })
assert(#dst.m == 2 and dst.m[2] == 'b' and dst.keep == nil, '数组要整体替换、__DELETE__ 要删键')
local s = {} L.set(s, { 'x', 'y' }, 1) L.set(s, { 'x', 'y' }, nil)
assert(next(s) == nil, '删到空的表要清掉')
" || { echo "✗ luaser 边界检查没过" >&2; exit 6; }

for dev in "${devs[@]}"; do
  [[ -f $KO_HERE/devices/$dev/device.conf ]] || { echo "✗ 没有设备 $dev" >&2; exit 2; }
  bash -n "$KO_HERE/devices/$dev/device.conf" || { echo "✗ devices/$dev/device.conf 语法错" >&2; exit 6; }
  w=$tmp/$dev
  mkdir -p "$w/before/settings"
  [[ -n $copy_src ]] && snap "$copy_src" "$w/before"
  snap "$w/before" "$w/cur"

  # 1. 两遍应用：第二遍零净改动
  ko_merge_all "$w/cur" "$dev"
  snap "$w/cur" "$w/once"
  ko_merge_all "$w/cur" "$dev"
  second=$(net_changes "$w/once" "$w/cur")
  [[ $second -eq 0 ]] || { echo "✗ $dev：第二遍后还有 $second 个文件有净改动（不幂等）" >&2; exit 5; }
  for f in "${KO_FILES[@]}"; do
    luajit -e "assert(type(dofile('$w/cur/$f')) == 'table')" || { echo "✗ $dev：$f 读不回来" >&2; exit 4; }
  done
  luajit -e "
local s = dofile('$w/cur/settings.reader.lua')
local g = dofile('$w/cur/settings/gestures.lua')
assert(g.gesture_reader.hold_top_left_corner.exit == true)
local r = g.gesture_reader
assert(r.hold_top_right_corner.suspend and not r.hold_top_right_corner.toggle_frontlight, '长按右上 = 休眠，原来的开关前光要去掉')
assert(r.tap_top_right_corner.toc and not r.tap_top_right_corner.toggle_bookmark, '点右上 = 目录，缺省的书签要去掉')
assert(r.one_finger_swipe_right_edge_up.increase_frontlight_warmth == 0 and not r.one_finger_swipe_right_edge_up.full_refresh, '右边缘 = 暖光')
assert(r.short_diagonal_swipe == nil, '删掉的手势不能留下 __DELETE__')
assert(not dofile('$KO_HERE/luaser.lua').serialize(g):find('__DELETE__'), '结果里不能留 __DELETE__')
local k = dofile('$w/cur/settings/kosync.lua').settings
assert(k.custom_server == 'https://sync.vksight.com' and k.checksum_method == 1 and k.auto_sync == true and k.sync_forward and k.sync_backward, '进度同步设置不对')
local t = s.style_tweaks
assert(t and t.cjk_tailored and not t['docfragment_page-break-before_avoid '] and not t['h2_page-break-before_always']
  and not t['footnote-inpage_epub'], '分页、弹窗注释要撤掉的样式调整还在（或整张表没了：没了 KOReader 会退回缺省，页内注释又开了）')
" || { echo "✗ $dev：应用结果不对" >&2; exit 4; }

  # 2. 卸载：没有原始配置 → 方案、设备差异撤掉，个人设置保留
  snap "$w/cur" "$w/un"
  ko_unmerge_all "$w/un" "$tmp/no-backups" "$dev" >/dev/null
  luajit -e "
local s = dofile('$w/un/settings.reader.lua')
assert(s.fontmap and s.fontmap.cfont, '个人设置（界面字体）不能撤')
assert(s.footnote_link_in_popup == nil and s.cre_background_image == nil, '文字书方案、设备差异没撤掉')
assert(s.start_with == nil, '不该再设启动进 SimpleUI')
" || { echo "✗ $dev：卸载结果不对" >&2; exit 7; }
  # 有原始配置（= 应用前的副本）时：卸载后等于"原始配置 + 个人设置"
  mkdir -p "$tmp/backups/t0" && rm -rf "$tmp/backups/t0/$dev" && cp -r "$w/before" "$tmp/backups/t0/$dev"
  snap "$w/cur" "$w/un2"
  ko_unmerge_all "$w/un2" "$tmp/backups" "$dev" >/dev/null
  snap "$w/before" "$w/expect"
  while read -r target script patch kind; do # 个人设置层（卸载保留的）照样合并进预期结果
    [[ $kind == personal ]] || continue
    mkdir -p "$(dirname "$w/expect/$target")"
    luajit "$KO_HERE/$script" "$w/expect/$target" "$KO_HERE/$patch" >/dev/null || true
  done < <(ko_layers "$dev")
  for f in "${KO_FILES[@]}"; do
    rc=0; out=$(luajit "$KO_HERE/diff.lua" "$w/expect/$f" "$w/un2/$f") || rc=$?
    [[ $rc -eq 10 ]] || { echo "✗ $dev：卸载后 $f 没还原成原始配置 + 个人设置：" >&2; echo "$out" >&2; exit 7; }
  done
  echo "✓ $dev：应用两遍第二遍零净改动；结果能读回；卸载能撤掉方案并保留个人设置"
done
