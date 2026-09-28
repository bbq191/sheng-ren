#!/usr/bin/env bash
# 把 koreader/ 里的配置应用到一台 USB 连着的设备（MTP 挂载）上的 KOReader。
#
# 用法: koreader/apply.sh <设备 id> [--write] [--closed]
#   不带 --write：只列出会改哪些键（dry run），什么都不写。
#   --closed：声明已在设备上退出 KOReader（Android 上从电脑看不出来，不给这个参数就在终端里问）。
#   带 --write：先把设备上的原配置备份到 ~/Documents/ereader/koreader-backup/<时间>/<设备 id>/，写入，回读核对；
#               核对不一致就用备份还原。
#
# 应用顺序（后面的覆盖前面的）：
#   settings.reader.lua  ← personal/ → schemes/text → schemes/comic → devices/<id>/ → 按当前状态栏生成两个预设（presets.lua）
#   settings/gestures.lua ← personal/gestures
#   settings/profiles.lua ← schemes/profiles
# 需要 luajit、gio（gvfs）。KOReader 运行中不能写：它退出时会把内存里的设置写回文件，把改动盖掉。
set -euo pipefail

here=$(cd "$(dirname "$0")" && pwd)
dev=${1:-}
write=0
closed_flag=0
for a in "${@:2}"; do
  case $a in
    --write) write=1 ;;
    --closed) closed_flag=1 ;;
    *) echo "不认识的参数 $a" >&2; exit 2 ;;
  esac
done
if [[ -z $dev || ! -f $here/devices/$dev/device.conf ]]; then
  echo "用法: $0 <设备 id> [--write]；设备: $(ls "$here/devices" | tr '\n' ' ')" >&2
  exit 2
fi
# shellcheck source=/dev/null
source "$here/devices/$dev/device.conf"

# ── 找设备 ──
gvfs=/run/user/$(id -u)/gvfs
mount=$(ls -d "$gvfs"/mtp:host="$MTP_HOST_PREFIX"* 2>/dev/null | head -1 || true)
if [[ -z $mount ]]; then
  uri=$(gio mount -li 2>/dev/null | grep -o "activation_root=mtp://${MTP_HOST_PREFIX}[^ ]*" | head -1 | cut -d= -f2 || true)
  [[ -n $uri ]] && gio mount "$uri" 2>/dev/null && sleep 2
  mount=$(ls -d "$gvfs"/mtp:host="$MTP_HOST_PREFIX"* 2>/dev/null | head -1 || true)
fi
[[ -z $mount ]] && { echo "✗ 没找到 $dev：USB 连上并解锁设备后再试" >&2; exit 1; }
ko="$mount/$KOREADER_DIR"
[[ -f $ko/settings.reader.lua ]] || { echo "✗ $ko 里没有 settings.reader.lua（KOReader 没装、或还没运行过一次）" >&2; exit 1; }

# ── KOReader 在不在运行 ──
closed=$closed_flag
case $RUNNING_CHECK in
  crashlog)
    start=$(grep -an "It's KOReader!" "$ko/crash.log" 2>/dev/null | tail -1 | cut -d: -f1 || true)
    stop=$(grep -an "Tearing down UIManager" "$ko/crash.log" 2>/dev/null | tail -1 | cut -d: -f1 || true)
    closed=0
    [[ -n $stop && ( -z $start || $stop -gt $start ) ]] && closed=1
    ;;
esac

# ── 取回设备上的原文件，在临时目录里合并 ──
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
mkdir -p "$work/orig/settings" "$work/new/settings"
files=(settings.reader.lua settings/gestures.lua settings/profiles.lua)
for f in "${files[@]}"; do
  # 经 gio 读（不经 FUSE 路径读内容，见下面 readback 的说明）
  if [[ -f $ko/$f ]]; then gio copy "$ko/$f" "$work/orig/$f"; cp "$work/orig/$f" "$work/new/$f"; fi
done

changed=()
step() { # step <目标文件> <脚本> [参数…]：跑一步合并，打印改动
  local target=$1; shift
  local out rc=0
  out=$(luajit "$@") || rc=$?
  if [[ $rc -eq 0 ]]; then
    printf '%s\n' "$out" | sed "s|^|  |"
    [[ " ${changed[*]-} " == *" $target "* ]] || changed+=("$target")
  elif [[ $rc -ne 10 ]]; then
    echo "✗ 合并出错（$*）" >&2; exit 3
  fi
}
merge() { echo "── $1 ← ${2#$here/}"; step "$1" "$here/merge.lua" "$work/new/$1" "$2"; }
merge settings.reader.lua "$here/personal/settings.reader.patch.lua"
merge settings.reader.lua "$here/schemes/text.settings.patch.lua"
merge settings.reader.lua "$here/schemes/comic.settings.patch.lua"
merge settings.reader.lua "$here/devices/$dev/settings.reader.patch.lua"
echo "── settings.reader.lua ← 状态栏预设"; step settings.reader.lua "$here/presets.lua" "$work/new/settings.reader.lua"
merge settings/gestures.lua "$here/personal/gestures.patch.lua"
merge settings/profiles.lua "$here/schemes/profiles.patch.lua"

if [[ ${#changed[@]} -eq 0 ]]; then echo "= $dev 已是最新，不用改"; exit 0; fi
if [[ $write -eq 0 ]]; then echo "（dry run：以上是会改的键；确认后加 --write 写入）"; exit 0; fi

# ── 写入 ──
if [[ $closed -eq 0 ]]; then
  if [[ $RUNNING_CHECK == crashlog ]]; then
    echo "✗ KOReader 看起来还在运行（crash.log 里最后一次启动之后没有退出记录）。先在设备上退出 KOReader 再写。" >&2
    exit 1
  fi
  read -r -p "确认已在设备上彻底关闭 KOReader（最近任务里划掉）？[y/N] " ans
  [[ $ans == y || $ans == Y ]] || { echo "没写。"; exit 1; }
fi
backup=~/Documents/ereader/koreader-backup/$(date +%Y-%m-%d_%H%M%S)/$dev
mkdir -p "$backup/settings"
for f in "${files[@]}"; do [[ -f $work/orig/$f ]] && cp -p "$work/orig/$f" "$backup/$f"; done
# 回读核对要经 gio 读：gvfs 的 FUSE 路径（$ko/…）对读过的文件有缓存，gio 换掉文件后 FUSE 还会返回旧内容
# （2026-09-28 Kindle 实测：写入 19524 字节，经 FUSE 读回的是旧的 11487 字节，经 gio 读回正确）。
readback() { rm -f "$work/back"; gio copy "$ko/$1" "$work/back" 2>/dev/null && cmp -s "$work/new/$1" "$work/back"; }
restore() { # restore <出问题的文件>
  echo "✗ $1 回读核对不一致（应为 $(stat -c %s "$work/new/$1") 字节，设备上读回 $(stat -c %s "$work/back" 2>/dev/null || echo 读不到)），用备份还原" >&2
  for f in "${changed[@]}"; do
    gio remove "$ko/$f" 2>/dev/null || true
    [[ -f $backup/$f ]] && gio copy "$backup/$f" "$ko/$f"
  done
  exit 4
}
for f in "${changed[@]}"; do
  [[ -f $ko/$f ]] && gio remove "$ko/$f"   # MTP 不支持覆盖写，先删再拷
  gio copy "$work/new/$f" "$ko/$f"
  readback "$f" || restore "$f"
done
echo "✓ $dev：写入 ${changed[*]}，回读核对一致。原配置备份在 ${backup/#$HOME/\~}"
