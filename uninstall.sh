#!/usr/bin/env bash
# 卸载 install.sh 装的命令（booklib、ebook-meta 以及 --tools 装的那些）。
#
# 用法: ./uninstall.sh
# 只删命令本身。书库（索引、找来的封面、生成的产物）、KOReader 配置备份、设备上的东西都不动，只告诉你在哪：
#   不要书库了就自己删那个目录；要撤掉设备上 KOReader 的方案设置，用 koreader/apply.sh <设备> --uninstall --write。
set -euo pipefail

case ${1:-} in
  "") ;;
  -h | --help) sed -n '2,6p' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
  *) echo "不认识的参数 $1（见 --help）" >&2; exit 2 ;;
esac
command -v cargo >/dev/null || { echo "✗ 没有 cargo：命令是用 cargo install 装的，卸载也要用它" >&2; exit 1; }

# cargo install --list 的格式：「包名 版本 (路径):」一行，下面每个二进制缩进一行。只卸本项目的三个包。
removed=0
for pkg in library bookconv azw3; do
  if cargo install --list | grep -q "^$pkg v"; then
    cargo uninstall --quiet "$pkg"
    echo "✓ 已卸载 $pkg"
    removed=1
  fi
done
[[ $removed -eq 1 ]] || echo "= 没有找到装过的命令"

lib=${BOOKLIB_DIR:-${XDG_DATA_HOME:-$HOME/.local/share}/booklib}
[[ -d $lib ]] && echo "  书库还在：${lib/#$HOME/\~}（$(du -sh "$lib" 2>/dev/null | cut -f1)）。不要了就自己删这个目录。"
backup=${KOREADER_BACKUP:-$HOME/Documents/ereader/koreader-backup}
[[ -d $backup ]] && echo "  KOReader 配置备份还在：${backup/#$HOME/\~}"
echo "  设备上 KOReader 的方案设置要撤掉：koreader/apply.sh <设备 id> --uninstall --write"
