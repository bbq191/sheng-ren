#!/usr/bin/env bash
# 把 koreader/ 里的配置应用到一台设备上的 KOReader。设备经 USB（MTP 挂载，掌阅、Kindle）或 SSH（reMarkable Move）连着。
#
# 用法: koreader/apply.sh <设备 id> [--write] [--closed]
#   不带 --write：只列出会改哪些键、缺哪些字体（dry run），什么都不写。
#   --closed：声明已在设备上退出 KOReader（Android 上从电脑看不出来，不给这个参数就在终端里问）。
#   带 --write：先把设备上的原配置备份到 ~/Documents/ereader/koreader-backup/<时间>/<设备 id>/，写入，回读核对；
#               核对不一致就用备份还原。缺的字体从本机字体目录拷过去（见下）。
#
# 应用顺序（后面的覆盖前面的）：
#   settings.reader.lua  ← personal/ → schemes/text → schemes/comic → devices/<id>/ → 按当前状态栏生成两个预设（presets.lua）
#   settings/gestures.lua ← personal/gestures
#   settings/profiles.lua ← schemes/profiles
# 字体：device.conf 的 FONTS 列出这台设备要有的字体文件（在 KOReader 的 fonts/ 下），缺的从 $KOREADER_FONTS
#       （缺省 ~/Documents/ereader/koreader-fonts/）拷。
# 需要 luajit；MTP 设备要 gio（gvfs），SSH 设备要 ssh/scp。KOReader 运行中不能写：它退出时会把内存里的设置写回文件。
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
  echo "用法: $0 <设备 id> [--write] [--closed]；设备: $(ls "$here/devices" | tr '\n' ' ')" >&2
  exit 2
fi
TRANSPORT=mtp
FONTS=()
# shellcheck source=/dev/null
source "$here/devices/$dev/device.conf"
font_src=${KOREADER_FONTS:-$HOME/Documents/ereader/koreader-fonts}

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

# ── 设备文件操作（路径都相对 KOReader 目录）──
# MTP：一律经 gio 读写。gvfs 的 FUSE 路径（/run/user/…/gvfs/…）对读过的文件有缓存，gio 换掉文件后 FUSE 还会返回旧内容
# （2026-09-28 Kindle 实测：写入 19524 字节，经 FUSE 读回的是旧的 11487 字节），所以内容一律不经 FUSE 读。
# SSH：scp 进同目录的临时文件再 mv，写到一半断了不留半个配置。
ssh_opts=(-o BatchMode=yes -o ConnectTimeout=5)
case $TRANSPORT in
  mtp)
    uri=$(gio mount -li 2>/dev/null | grep -o "activation_root=mtp://${MTP_HOST_PREFIX}[^ ]*" | head -1 | cut -d= -f2 || true)
    [[ -z $uri ]] && { echo "✗ 没找到 $dev：USB 连上并解锁设备后再试" >&2; exit 1; }
    gio info "$uri" >/dev/null 2>&1 || gio mount "$uri" 2>/dev/null || true
    base="${uri%/}/$KOREADER_DIR"
    dev_get() { gio copy "$base/$1" "$2" 2>/dev/null; }
    dev_has() { gio info "$base/$1" >/dev/null 2>&1; }
    dev_put() { # MTP 不支持覆盖写：先删再拷
      if dev_has "$2"; then gio remove "$base/$2"; fi
      gio copy "$1" "$base/$2"
    }
    ;;
  ssh)
    ssh "${ssh_opts[@]}" "$SSH_HOST" true 2>/dev/null || { echo "✗ 连不上 $dev（ssh $SSH_HOST）：USB 连上、屏幕解锁后再试" >&2; exit 1; }
    base=$KOREADER_DIR
    dev_get() { scp -q "${ssh_opts[@]}" "$SSH_HOST:$base/$1" "$2" 2>/dev/null; }
    dev_has() { ssh "${ssh_opts[@]}" "$SSH_HOST" "test -e '$base/$1'"; }
    dev_put() { scp -q "${ssh_opts[@]}" "$1" "$SSH_HOST:$base/$2.tmp" && ssh "${ssh_opts[@]}" "$SSH_HOST" "mv '$base/$2.tmp' '$base/$2'"; }
    ;;
  *) echo "✗ device.conf 的 TRANSPORT 只能是 mtp 或 ssh" >&2; exit 2 ;;
esac
dev_has settings.reader.lua || { echo "✗ $dev 的 KOReader 目录里没有 settings.reader.lua（KOReader 没装、或还没运行过一次）" >&2; exit 1; }

# ── KOReader 在不在运行 ──
closed=$closed_flag
case $RUNNING_CHECK in
  crashlog) # crash.log 里最后一次启动之后有没有"Tearing down UIManager"
    closed=0
    if dev_get crash.log "$work/crash.log"; then
      start=$(grep -an "It's KOReader!" "$work/crash.log" | tail -1 | cut -d: -f1 || true)
      stop=$(grep -an "Tearing down UIManager" "$work/crash.log" | tail -1 | cut -d: -f1 || true)
      [[ -n $stop && ( -z $start || $stop -gt $start ) ]] && closed=1
    fi
    ;;
  proc) # 有没有进程在跑这个 KOReader 目录下的 reader.lua
    closed=0
    # 模式写成 reade[r].lua：这条命令自己的命令行里是字面的 "reade[r].lua"，不会被自己匹配上
    ssh "${ssh_opts[@]}" "$SSH_HOST" "cat /proc/[0-9]*/cmdline 2>/dev/null | tr '\\0' ' ' | grep -q '$base/reade[r].lua'" || closed=1
    ;;
esac

# ── 取回设备上的原文件，在临时目录里合并 ──
mkdir -p "$work/orig/settings" "$work/new/settings"
files=(settings.reader.lua settings/gestures.lua settings/profiles.lua)
for f in "${files[@]}"; do
  if dev_get "$f" "$work/orig/$f"; then cp "$work/orig/$f" "$work/new/$f"; fi
done

# 分层合并（个人设置 → 方案 → 设备 → 预设），中间层改过、后面又改回来的键不算改动：只看原文件和最终结果的净差异。
run() { # run <脚本> [参数…]：跑一步合并；0 有改动、10 没改动都算正常
  local rc=0
  luajit "$@" >/dev/null || rc=$?
  [[ $rc -eq 0 || $rc -eq 10 ]] || { echo "✗ 合并出错（$*）" >&2; exit 3; }
}
run "$here/merge.lua" "$work/new/settings.reader.lua" "$here/personal/settings.reader.patch.lua"
run "$here/merge.lua" "$work/new/settings.reader.lua" "$here/schemes/text.settings.patch.lua"
run "$here/merge.lua" "$work/new/settings.reader.lua" "$here/schemes/comic.settings.patch.lua"
run "$here/merge.lua" "$work/new/settings.reader.lua" "$here/devices/$dev/settings.reader.patch.lua"
run "$here/presets.lua" "$work/new/settings.reader.lua"
run "$here/merge.lua" "$work/new/settings/gestures.lua" "$here/personal/gestures.patch.lua"
run "$here/merge.lua" "$work/new/settings/profiles.lua" "$here/schemes/profiles.patch.lua"
changed=()
for f in "${files[@]}"; do
  [[ -f $work/orig/$f ]] || : >"$work/orig/$f.none"
  rc=0
  out=$(luajit "$here/diff.lua" "$work/orig/$f" "$work/new/$f") || rc=$?
  if [[ $rc -eq 0 ]]; then
    echo "── $f"
    printf '%s\n' "$out" | sed "s|^|  |"
    changed+=("$f")
  elif [[ $rc -ne 10 ]]; then
    echo "✗ 比较 $f 出错" >&2; exit 3
  fi
done

# ── 字体 ──
missing_fonts=()
for font in "${FONTS[@]}"; do
  dev_has "fonts/$font" || missing_fonts+=("$font")
done
for font in "${missing_fonts[@]}"; do
  if [[ -f $font_src/$font ]]; then echo "── 字体 fonts/$font：设备上没有，会从 ${font_src/#$HOME/\~}/ 拷过去"
  else echo "✗ 字体 fonts/$font：设备上没有，本机 ${font_src/#$HOME/\~}/ 里也没有，请先放进去" >&2; exit 1; fi
done

if [[ ${#changed[@]} -eq 0 && ${#missing_fonts[@]} -eq 0 ]]; then echo "= $dev 已是最新，不用改"; exit 0; fi
if [[ $write -eq 0 ]]; then echo "（dry run：以上是会改的；确认后加 --write 写入）"; exit 0; fi

# ── 写入 ──
if [[ $closed -eq 0 ]]; then
  case $RUNNING_CHECK in
    crashlog) echo "✗ KOReader 看起来还在运行（crash.log 里最后一次启动之后没有退出记录）。先在设备上退出 KOReader 再写。" >&2; exit 1 ;;
    proc) echo "✗ KOReader 正在运行。先在设备上退出 KOReader 再写。" >&2; exit 1 ;;
  esac
  read -r -p "确认已在设备上彻底关闭 KOReader（最近任务里划掉）？[y/N] " ans
  [[ $ans == y || $ans == Y ]] || { echo "没写。"; exit 1; }
fi
for font in "${missing_fonts[@]}"; do
  dev_put "$font_src/$font" "fonts/$font"
  echo "✓ 字体 fonts/$font 已拷到设备"
done
[[ ${#changed[@]} -eq 0 ]] && exit 0
backup=~/Documents/ereader/koreader-backup/$(date +%Y-%m-%d_%H%M%S)/$dev
mkdir -p "$backup/settings"
for f in "${files[@]}"; do if [[ -f $work/orig/$f ]]; then cp -p "$work/orig/$f" "$backup/$f"; fi; done
readback() { rm -f "$work/back"; dev_get "$1" "$work/back" && cmp -s "$work/new/$1" "$work/back"; }
restore() { # restore <出问题的文件>
  echo "✗ $1 回读核对不一致（应为 $(stat -c %s "$work/new/$1") 字节，设备上读回 $(stat -c %s "$work/back" 2>/dev/null || echo 读不到)），用备份还原" >&2
  for f in "${changed[@]}"; do
    if [[ -f $backup/$f ]]; then dev_put "$backup/$f" "$f"; fi
  done
  exit 4
}
for f in "${changed[@]}"; do
  dev_put "$work/new/$f" "$f"
  readback "$f" || restore "$f"
done
echo "✓ $dev：写入 ${changed[*]}，回读核对一致。原配置备份在 ${backup/#$HOME/\~}"
