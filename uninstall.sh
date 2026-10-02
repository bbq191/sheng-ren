#!/usr/bin/env bash
# 卸载 install.sh 装的命令（booklib 以及 --tools 装的那些）。
#
# 用法: ./uninstall.sh
# 只删命令本身。书库（索引、找来的封面、生成的产物）、设备上的东西都不动，只告诉你在哪：
#   不要书库了就自己删那个目录。
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd)

case ${1:-} in
  "") ;;
  -h | --help) sed -n '2,6p' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
  *) echo "不认识的参数 $1（见 --help）" >&2; exit 2 ;;
esac
command -v cargo >/dev/null || { echo "✗ 没有 cargo：命令是用 cargo install 装的，卸载也要用它" >&2; exit 1; }

# cargo install --list 的格式：「包名 版本 (路径):」一行，下面每个二进制缩进一行。
# 包名 library、bookconv 很普通，别处可能装着同名的包：只认从本仓库路径装的，按「路径#包名@版本」精确卸载。
installed=$(cargo install --list)
removed=0
for pkg in library bookconv azw3 mobidict; do
  line=$(grep -F " ($here/crates/$pkg):" <<<"$installed" | grep "^$pkg v" || true)
  [[ -n $line ]] || continue
  ver=${line#"$pkg v"}
  ver=${ver%% *}
  cargo uninstall --quiet "path+file://$here/crates/$pkg#$pkg@$ver"
  echo "✓ 已卸载 $pkg"
  removed=1
done
[[ $removed -eq 1 ]] || echo "= 没有找到从本仓库装的命令"

lib=${BOOKLIB_DIR:-${XDG_DATA_HOME:-$HOME/.local/share}/booklib}
if [[ -d $lib ]]; then
  echo "  书库还在：${lib/#$HOME/\~}（$(du -sh "$lib" 2>/dev/null | cut -f1)）。不要了就自己删这个目录。"
fi
