#!/usr/bin/env bash
# 把 koreader/ 里的配置应用到一台设备上的 KOReader，或者撤销、还原。设备经 USB（MTP：掌阅、Kindle）连着。
#
# 用法: koreader/apply.sh <设备 id> [--restore[=<时间>] | --uninstall] [--write] [--closed]
#   缺省（应用）：个人设置 → 文字书方案 → 漫画方案 → 设备差异 → 状态栏预设，依次合并进设备上的配置；缺的字体、补丁拷过去。
#   --restore[=<时间>]：把配置还原成某次备份（缺省最近一次；<时间> 是备份目录名，如 2026-09-28_153012）。
#   --uninstall：撤销方案、设备差异、状态栏预设和我们的补丁（patches/ 里内容和本仓库一致的才删）；个人设置、字体保留。
#                撤销的键还原成第一次应用前的值（最早的备份）；没有备份就删掉，由 KOReader 用缺省值。之后在设备上手改过的键不动。
#   --write：真的写。不带就只列出会改什么（dry run）。
#   --closed：声明已在设备上退出 KOReader（Android 上从电脑看不出来，不给就在终端里问）。
#
# 写入前把设备上要动的文件备份到 $KOREADER_BACKUP/<时间>/<设备 id>/（缺省 ~/Documents/ereader/koreader-backup）；
# 每写一个文件就回读核对，任何一步失败都把已写的文件还原（原来没有的删掉）。字体从 $KOREADER_FONTS（缺省 ~/Documents/ereader/fonts）拷。
# KOReader 运行中不能写：它退出时会把内存里的设置写回文件，覆盖掉这里写的。
# 需要 luajit 和 gio（gvfs）。
set -euo pipefail

KO_HERE=$(cd "$(dirname "$0")" && pwd)
# shellcheck source=lib.sh
source "$KO_HERE/lib.sh"

dev=${1:-}
mode=apply restore_from='' write=0 closed_flag=0
for a in "${@:2}"; do
  case $a in
    --write) write=1 ;;
    --closed) closed_flag=1 ;;
    --uninstall) mode=uninstall ;;
    --restore) mode=restore ;;
    --restore=*) mode=restore restore_from=${a#--restore=} ;;
    *) echo "不认识的参数 $a" >&2; exit 2 ;;
  esac
done
if [[ -z $dev || ! -f $KO_HERE/devices/$dev/device.conf ]]; then
  echo "用法: $0 <设备 id> [--restore[=<时间>] | --uninstall] [--write] [--closed]；设备: $(ls "$KO_HERE/devices" | tr '\n' ' ')" >&2
  exit 2
fi
command -v luajit >/dev/null || { echo "✗ 缺 luajit" >&2; exit 1; }
font_src=${KOREADER_FONTS:-$HOME/Documents/ereader/fonts}
backup_root=${KOREADER_BACKUP:-$HOME/Documents/ereader/koreader-backup}
show() { echo "${1/#$HOME/\~}"; }

KO_TMP=$(mktemp -d)
trap 'rm -rf "$KO_TMP"' EXIT
ko_connect "$dev"
dev_has settings.reader.lua || { echo "✗ $dev 的 KOReader 目录里没有 settings.reader.lua（KOReader 没装、或还没运行过一次）" >&2; exit 1; }

# ── 取回设备上的原文件。读失败（不是"没有这个文件"）就停：拿空表合并写回去会把整份配置冲掉 ──
orig=$KO_TMP/orig new=$KO_TMP/new
mkdir -p "$orig/settings" "$orig/patches" "$new/settings"
for f in "${KO_FILES[@]}"; do
  if dev_has "$f"; then
    dev_get "$f" "$orig/$f" || { echo "✗ 读不出设备上的 $f（连接不稳？）" >&2; exit 1; }
    cp "$orig/$f" "$new/$f"
  fi
done

# ── 算出新配置 ──
case $mode in
  apply) ko_merge_all "$new" "$dev" ;;
  restore)
    if [[ -n $restore_from ]]; then src=$backup_root/$restore_from/$dev
    else src=$(ls -d "$backup_root"/*/"$dev" 2>/dev/null | sort | tail -1 || true); fi
    [[ -n $src && -d $src ]] || { echo "✗ 没有 $dev 的备份（$(show "$backup_root")/<时间>/$dev/）" >&2; exit 1; }
    echo "从备份还原：$(show "$src")"
    for f in "${KO_FILES[@]}"; do [[ -f $src/$f ]] && cp "$src/$f" "$new/$f"; done
    ;;
  uninstall)
    ko_unmerge_all "$new" "$backup_root" "$dev"
    ;;
esac

# 净差异：中间层改过、后面又改回来的键不算
changed=()
for f in "${KO_FILES[@]}"; do
  [[ -f $new/$f ]] || continue
  rc=0
  out=$(luajit "$KO_HERE/diff.lua" "$orig/$f" "$new/$f") || rc=$?
  if [[ $rc -eq 0 ]]; then
    echo "── $f"
    printf '%s\n' "$out" | sed "s|^|  |"
    changed+=("$f")
  elif [[ $rc -ne 10 ]]; then
    echo "✗ 比较 $f 出错" >&2; exit 3
  fi
done

# ── 字体（只在应用时补缺的；卸载不删字体：可能是用户自己放的）──
missing_fonts=()
if [[ $mode == apply ]]; then
  for font in "${FONTS[@]}"; do
    dev_has "fonts/$font" && continue
    [[ -f $font_src/$font ]] || { echo "✗ 字体 fonts/$font：设备上没有，本机 $(show "$font_src")/ 里也没有，请先放进去" >&2; exit 1; }
    echo "── 字体 fonts/$font：设备上没有，会从 $(show "$font_src")/ 拷过去"
    missing_fonts+=("$font")
  done
fi

# ── 用户补丁（koreader/patches/*.lua ↔ 设备的 patches/）──
put_patches=() rm_patches=()
for p in "$KO_HERE"/patches/*.lua; do
  [[ -f $p ]] || continue
  name=$(basename "$p") on_dev=0
  if dev_has "patches/$name"; then
    dev_get "patches/$name" "$orig/patches/$name" || { echo "✗ 读不出设备上的 patches/$name" >&2; exit 1; }
    on_dev=1
  fi
  case $mode in
    apply)
      [[ $on_dev -eq 1 ]] && cmp -s "$p" "$orig/patches/$name" && continue
      echo "── 补丁 patches/$name：$([[ $on_dev -eq 1 ]] && echo 内容不同，会更新 || echo 设备上没有，会拷过去)"
      put_patches+=("$name") ;;
    uninstall)
      [[ $on_dev -eq 1 ]] || continue
      if cmp -s "$p" "$orig/patches/$name"; then echo "── 补丁 patches/$name：会删掉"; rm_patches+=("$name")
      else echo "   补丁 patches/$name：和本仓库的不一样（在设备上改过？），不动"; fi ;;
  esac
done
if [[ $mode == restore && -d $src/patches ]]; then
  for p in "$src"/patches/*.lua; do
    [[ -f $p ]] || continue
    name=$(basename "$p")
    [[ -f $orig/patches/$name ]] && cmp -s "$p" "$orig/patches/$name" && continue
    echo "── 补丁 patches/$name：还原成备份里的"
    cp "$p" "$KO_TMP/restore-$name"
    put_patches+=("$name")
  done
fi

if [[ ${#changed[@]} -eq 0 && ${#missing_fonts[@]} -eq 0 && ${#put_patches[@]} -eq 0 && ${#rm_patches[@]} -eq 0 ]]; then
  echo "= $dev 不用改"; exit 0
fi
if [[ $write -eq 0 ]]; then echo "（dry run：以上是会改的；确认后加 --write 写入）"; exit 0; fi

# ── KOReader 必须已退出 ──
state=$(ko_running) || exit 1
case $state in
  running) echo "✗ KOReader 正在运行（$RUNNING_CHECK 检查）。先在设备上用 KOReader 菜单里的「退出」关掉它再写。" >&2; exit 1 ;;
  unknown)
    if [[ $closed_flag -eq 0 ]]; then
      [[ -t 0 ]] || { echo "✗ 从电脑看不出 $dev 上的 KOReader 是否在运行：确认已退出后加 --closed 再跑" >&2; exit 1; }
      read -r -p "确认已在设备上用 KOReader 菜单里的「退出」关掉了它（在最近任务里划掉不一定结束进程）？[y/N] " ans || ans=
      [[ $ans == y || $ans == Y ]] || { echo "没写。"; exit 1; }
    fi ;;
esac

# ── 备份：要动的配置和补丁（原来没有的记下来，回滚时删掉）──
backup=$backup_root/$(date +%Y-%m-%d_%H%M%S)/$dev
mkdir -p "$backup/settings" "$backup/patches"
for f in "${changed[@]}"; do [[ -f $orig/$f ]] && cp -p "$orig/$f" "$backup/$f"; done
for name in "${put_patches[@]}" "${rm_patches[@]}"; do
  [[ -f $orig/patches/$name ]] && cp -p "$orig/patches/$name" "$backup/patches/$name"
done

# ── 写入。每个文件写完回读核对；失败就把已经写过的还原 ──
touched=()
verify() { # verify <本地> <设备路径>：回读逐字节比较
  rm -f "$KO_TMP/back"
  dev_get "$2" "$KO_TMP/back" && cmp -s "$1" "$KO_TMP/back"
}
rollback() {
  echo "✗ $1，把已写的文件还原：" >&2
  local rel bad=0
  for rel in "${touched[@]}"; do
    if [[ -f $backup/$rel ]]; then
      if dev_put "$backup/$rel" "$rel" && verify "$backup/$rel" "$rel"; then echo "  ↺ $rel 已还原" >&2
      else echo "  ✗ $rel 还原失败，备份在 $(show "$backup/$rel")" >&2; bad=1; fi
    else
      if dev_rm "$rel"; then echo "  ↺ $rel 原来没有，已删掉" >&2
      else echo "  ✗ $rel 删不掉" >&2; bad=1; fi
    fi
  done
  [[ $bad -eq 0 ]] || echo "  有文件没还原成功：按上面的备份路径手动处理" >&2
  exit 4
}
put_verified() { # put_verified <本地> <设备路径>
  touched+=("$2")
  dev_put "$1" "$2" || rollback "写 $2 失败"
  verify "$1" "$2" || rollback "$2 回读核对不一致"
}

for font in "${missing_fonts[@]}"; do # 字体大，按大小核对；失败只删掉半个文件，不影响配置
  if dev_put "$font_src/$font" "fonts/$font" && [[ $(dev_size "fonts/$font") == "$(stat -c %s "$font_src/$font")" ]]; then
    echo "✓ 字体 fonts/$font 已拷到设备"
  else
    dev_rm "fonts/$font" || true
    echo "✗ 字体 fonts/$font 没拷成功（大小不对），什么都没改" >&2; exit 4
  fi
done
if [[ ${#put_patches[@]} -gt 0 ]]; then
  dev_mkdir patches
  for name in "${put_patches[@]}"; do
    local_src=$KO_HERE/patches/$name
    [[ $mode == restore ]] && local_src=$KO_TMP/restore-$name
    put_verified "$local_src" "patches/$name"
    echo "✓ 补丁 patches/$name 已写入，回读一致"
  done
fi
for name in "${rm_patches[@]}"; do
  touched+=("patches/$name")
  dev_rm "patches/$name" || rollback "删 patches/$name 失败"
  echo "✓ 补丁 patches/$name 已删掉"
done
for f in "${changed[@]}"; do
  put_verified "$new/$f" "$f"
done
[[ ${#changed[@]} -gt 0 ]] && echo "✓ $dev：写入 ${changed[*]}，回读一致"
echo "  改动前的文件备份在 $(show "$backup")"
