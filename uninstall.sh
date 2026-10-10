#!/usr/bin/env bash
# 卸载 install.sh 装的命令（booklib 以及 --tools 装的那些）。
#
# 用法: ./uninstall.sh
# 只删命令本身。书库、设备上的书都不动，只告诉你书库在哪：不要了就自己删那个目录。
# 重复运行无害：已经卸了的再运行只是提示没找到。
# 退出码：0 卸完（或本来就没装）；1 没有 cargo、用 root 运行或卸载失败；2 参数不对。
# 直接运行，别 source（set -e、cd 会留在你的 shell 里，出错时连 shell 一起退出）
(return 0 2>/dev/null) && { echo "✗ 直接运行 ./uninstall.sh，别 source" >&2; return 2; }
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd -P)
# shellcheck source=tools/cargo-pkgs.sh
. "$here/tools/cargo-pkgs.sh"

case ${1:-} in
  "") ;;
  -h | --help) print_header "$here/uninstall.sh"; exit 0 ;;
  *) echo "不认识的参数 $1（见 --help）" >&2; exit 2 ;;
esac
[[ $# -le 1 ]] || { echo "多余的参数 $2（见 --help）" >&2; exit 2; }
command -v cargo >/dev/null || { echo "✗ 没有 cargo：命令是用 cargo install 装的，卸载也要用它" >&2; exit 1; }
cargo_list >/dev/null

# 本仓库装的包一次卸掉（一次 cargo uninstall，不是每个包起一次 cargo）
specs=() names=()
for d in "$here"/crates/*/; do
  p=$(basename "$d")
  if spec=$(ours_installed "$p"); then
    specs+=("$spec")
    names+=("$(pkg_bins "$p")")
  fi
done
if [[ ${#specs[@]} -gt 0 ]]; then
  cargo uninstall --quiet "${specs[@]}"
  echo "✓ 已卸载 ${names[*]}"
else
  echo "= 没有找到从本仓库装的命令"
fi

# 同名命令由别的地方装着（比如仓库挪过位置、或者别的项目）：只提示，不替你删
for d in "$here"/crates/*/; do
  for b in $(pkg_bins "$(basename "$d")"); do
    if other=$(foreign_owner "$b"); then echo "  还有一个 $b 不是从这里装的：$other（要删就 cargo uninstall 它）"; fi
  done
done

# 书库缺省位置和 booklib 一致：$BOOKLIB_DIR，否则 $XDG_DATA_HOME/booklib，否则 ~/.local/share/booklib（空值按没设处理）
lib=${BOOKLIB_DIR:-${XDG_DATA_HOME:-$HOME/.local/share}/booklib}
if [[ -d $lib ]]; then
  echo "  书库还在：${lib/#"$HOME"/\~}（$(du -sh "$lib" 2>/dev/null | cut -f1)）。不要了就自己删这个目录；书目录旁边生成的 kindle/ ireader/ xochitl/ 也一样。"
fi
