#!/usr/bin/env bash
# 离线检查 koreader/ 的补丁，不碰设备。对每台设备（缺省全部）：
#   1. 对一个 KOReader 目录的副本（缺省是空目录）按 apply.sh 的顺序应用一遍，再应用第二遍必须零净改动；结果都要能被 Lua 读回；
#   2. 卸载（unmerge）后：方案、设备、状态栏预设都撤掉，个人设置还在；拿应用前的副本当"原始配置"时能完全还原；
#   3. patches/*.lua 能编译，device.conf 语法正确；luaser 的序列化和合并边界（inf/nan、大整数、布尔键、数组整体替换）。
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
  for f in "${KO_FILES[@]}"; do if [[ -f $1/$f ]]; then cp "$1/$f" "$2/$f"; fi; done
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
for p in "$KO_HERE"/patches/*.lua; do
  [[ -f $p ]] || continue
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
assert(s.footer_presets and s.footer_presets['文字'] and s.footer_presets['漫画'], '缺状态栏预设')
assert(s.footer_presets['漫画'].reader_footer_mode == 0, '漫画预设要隐藏状态栏')
assert(s.profiles_autoexec.ReaderReadyAll['漫画·首次'].is_new == true)
local p = dofile('$w/cur/settings/profiles.lua')
for _, n in ipairs({'漫画·首次', '漫画', '文字'}) do assert(p[n], '缺配置档 ' .. n) end
local g = dofile('$w/cur/settings/gestures.lua')
assert(g.gesture_reader.hold_top_left_corner.exit == true)
assert(g.gesture_reader.tap_top_right_corner == nil, '删掉的手势不能留下 __DELETE__')
" || { echo "✗ $dev：应用结果不对" >&2; exit 4; }

  # 2. 卸载：没有原始配置 → 方案、预设撤掉，个人设置保留
  snap "$w/cur" "$w/un"
  ko_unmerge_all "$w/un" "$tmp/no-backups" "$dev" >/dev/null
  luajit -e "
local s = dofile('$w/un/settings.reader.lua')
local auto = s.profiles_autoexec or {}
assert(not (auto.ReaderReadyAll or {})['漫画·首次'] and not (auto.CloseDocumentAll or {})['文字'], '漫画自动套用规则没撤掉')
assert(not (s.footer_presets or {})['文字'] and not (s.footer_presets or {})['漫画'], '状态栏预设没撤掉')
assert(s.fontmap and s.fontmap.cfont, '个人设置（界面字体）不能撤')
local p = dofile('$w/un/settings/profiles.lua')
assert(p['漫画·首次'] == nil and p['文字'] == nil, '配置档没撤掉')
" || { echo "✗ $dev：卸载结果不对" >&2; exit 7; }
  # 有原始配置（= 应用前的副本）时：卸载后等于"原始配置 + 个人设置"
  mkdir -p "$tmp/backups/t0" && rm -rf "$tmp/backups/t0/$dev" && cp -r "$w/before" "$tmp/backups/t0/$dev"
  snap "$w/cur" "$w/un2"
  ko_unmerge_all "$w/un2" "$tmp/backups" "$dev" >/dev/null
  snap "$w/before" "$w/expect"
  luajit "$KO_HERE/merge.lua" "$w/expect/settings.reader.lua" "$KO_HERE/personal/settings.reader.patch.lua" >/dev/null || true
  luajit "$KO_HERE/merge.lua" "$w/expect/settings/gestures.lua" "$KO_HERE/personal/gestures.patch.lua" >/dev/null || true
  for f in "${KO_FILES[@]}"; do
    rc=0; out=$(luajit "$KO_HERE/diff.lua" "$w/expect/$f" "$w/un2/$f") || rc=$?
    [[ $rc -eq 10 ]] || { echo "✗ $dev：卸载后 $f 没还原成原始配置 + 个人设置：" >&2; echo "$out" >&2; exit 7; }
  done
  echo "✓ $dev：应用两遍第二遍零净改动；结果能读回；卸载能撤掉方案并保留个人设置"
done
