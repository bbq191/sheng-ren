# shellcheck shell=bash disable=SC2034,SC2154  # 被 source：TOOL_PKGS 给调用方用，here 由调用方设
# install.sh、uninstall.sh 共用（source 进来用，不单独运行）。调用方先设好 here（仓库根，pwd -P）。
#
# cargo install --list 的格式：「包名 v版本 (路径):」一行，下面每个二进制缩进 4 格一行。
# 包名 library、bookconv 很普通，别处可能装着同名的包：只认从本仓库路径装的。

# 开发工具所在的包（install.sh --tools 装、--no-tools 卸）
TOOL_PKGS=(bookconv azw3 mobidict)

# 读一次缓存起来：在主 shell 里先调一次 cargo_list >/dev/null（$(…) 里调用是子 shell，存不住）
_cargo_list=
cargo_list() { [[ -n $_cargo_list ]] || _cargo_list=$(cargo install --list); printf '%s\n' "$_cargo_list"; }

# 包里要装的命令：按 Cargo.toml 的 [[bin]] name（library 只装 booklib，bookconv 的三个都装）
pkg_bins() {
  awk '/^\[\[bin\]\]/ {b=1; next} /^\[/ {b=0} b && /^name *=/ {gsub(/^name *= *"|".*$/, ""); printf "%s ", $0}' "$here/crates/$1/Cargo.toml" | sed 's/ $//'
}

# 本仓库装的这个包：有就打印 cargo uninstall 用的精确包规格（路径#包名@版本）并返回 0。
# 仓库经符号链接装过的，cargo 记的是逻辑路径：按包名匹配后再比真实路径。
ours_installed() {
  local pkg=$1 line ver path
  while IFS= read -r line; do
    [[ $line =~ ^$pkg\ v([^ ]+)\ \((.+)\):$ ]] || continue
    ver=${BASH_REMATCH[1]} path=${BASH_REMATCH[2]}
    [[ -d $path && $(cd "$path" && pwd -P) == "$here/crates/$pkg" ]] || continue
    printf 'path+file://%s#%s@%s\n' "$path" "$pkg" "$ver"
    return 0
  done < <(cargo_list)
  return 1
}

# 命令 $1 若由别的（不是本仓库路径的）包装着，打印「包名 v版本 (来源)」并返回 0
foreign_owner() {
  local bin=$1 head='' line path
  while IFS= read -r line; do
    if [[ $line != ' '* ]]; then head=${line%:}; continue; fi
    [[ ${line# *} == "$bin" ]] || continue
    path=${head##*(}; path=${path%)}
    [[ -d $path && $(cd "$path" && pwd -P) == "$here/crates/"* ]] && return 1
    printf '%s\n' "$head"
    return 0
  done < <(cargo_list)
  return 1
}
