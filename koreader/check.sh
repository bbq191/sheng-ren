#!/usr/bin/env bash
# 离线检查 koreader/ 的补丁：对一个 KOReader 目录的副本（缺省是空目录）按 apply.sh 的顺序应用一遍，再应用第二遍必须零改动；
# 结果文件都要能被 Lua 读回。不碰设备。
# 用法: koreader/check.sh <设备 id> [KOReader 目录副本]
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd)
dev=${1:?用法: check.sh <设备 id> [KOReader 目录副本]}
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
mkdir -p "$work/settings"
if [[ -n ${2:-} ]]; then
  for f in settings.reader.lua settings/gestures.lua settings/profiles.lua; do [[ -f $2/$f ]] && cp "$2/$f" "$work/$f"; done
fi
run() {
  for step in \
    "settings.reader.lua merge.lua personal/settings.reader.patch.lua" \
    "settings.reader.lua merge.lua schemes/text.settings.patch.lua" \
    "settings.reader.lua merge.lua schemes/comic.settings.patch.lua" \
    "settings.reader.lua merge.lua devices/$dev/settings.reader.patch.lua" \
    "settings.reader.lua presets.lua" \
    "settings/gestures.lua merge.lua personal/gestures.patch.lua" \
    "settings/profiles.lua merge.lua schemes/profiles.patch.lua"; do
    read -r target script patch <<<"$step"
    rc=0
    luajit "$here/$script" "$work/$target" ${patch:+"$here/$patch"} >/dev/null || rc=$?
    case $rc in 0 | 10) ;; *) echo "✗ $script $patch 出错" >&2; exit 3 ;; esac
  done
}
# 第二遍看净差异（分层覆盖：中间层改过又被后面改回来的不算），必须为零
snap() { mkdir -p "$1/settings"; for f in settings.reader.lua settings/gestures.lua settings/profiles.lua; do [[ -f $work/$f ]] && cp "$work/$f" "$1/$f"; done; }
run
snap "$work.1"
run
second=0
for f in settings.reader.lua settings/gestures.lua settings/profiles.lua; do
  rc=0; luajit "$here/diff.lua" "$work.1/$f" "$work/$f" >/dev/null || rc=$?
  [[ $rc -eq 0 ]] && second=$((second + 1))
done
rm -rf "$work.1"
for f in settings.reader.lua settings/gestures.lua settings/profiles.lua; do
  luajit -e "assert(type(dofile('$work/$f')) == 'table')" || { echo "✗ $f 读不回来" >&2; exit 4; }
done
luajit -e "
local s = dofile('$work/settings.reader.lua')
assert(s.footer_presets and s.footer_presets['文字'] and s.footer_presets['漫画'], '缺状态栏预设')
assert(s.footer_presets['漫画'].reader_footer_mode == 0, '漫画预设要隐藏状态栏')
assert(s.profiles_autoexec.ReaderReadyAll['漫画·首次'].is_new == true)
local p = dofile('$work/settings/profiles.lua')
for _, n in ipairs({'漫画·首次', '漫画', '文字'}) do assert(p[n], '缺配置档 ' .. n) end
local g = dofile('$work/settings/gestures.lua')
assert(g.gesture_reader.hold_top_left_corner.exit == true)
assert(g.gesture_reader.tap_top_right_corner == nil, '删掉的手势不能留下 __DELETE__')
"
if [[ $second -ne 0 ]]; then echo "✗ 第二遍后还有 $second 个文件有净改动（不幂等）" >&2; exit 5; fi
echo "✓ $dev：应用两遍，第二遍没有净改动；结果都能读回"
