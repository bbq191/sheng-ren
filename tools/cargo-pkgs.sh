# shellcheck shell=bash disable=SC2034,SC2154  # 被 source：TOOL_PKGS、bindir 给调用方用，here 由调用方设
# install.sh、uninstall.sh 共用（source 进来用，不单独运行）。调用方先设好 here（仓库根，pwd -P）。
#
# cargo install --list 的格式：「包名 v版本 (路径):」一行，下面每个二进制缩进 4 格一行。
# 包名 library、bookconv 很普通，别处可能装着同名的包：只认从本仓库路径装的。

# 开发工具所在的包（install.sh --tools 装、--no-tools 卸）
TOOL_PKGS=(bookconv azw3 mobidict)

# cargo 把设成空值的 CARGO_INSTALL_ROOT 当成当前目录（装进 ./bin、在当前目录记账）：按没设处理。
# 空的 CARGO_HOME 它按没设处理（~/.cargo），和下面一致。
[[ -n ${CARGO_INSTALL_ROOT-} ]] || unset CARGO_INSTALL_ROOT
# cargo 的配置（install.root 等）按当前目录逐级往上找：固定在仓库根运行，从哪个目录调用脚本结果都一样
cd "$here" || exit 1

# 配置文件 $1 里的 install.root（`[install]` 下的 `root = "…"`，或顶层的 `install.root = "…"`）；没有就不打印
_config_install_root() {
  awk '
    function val(s,   q) { sub(/^[^=]*=[ \t]*/, "", s); q = substr(s, 1, 1)
      if (q != "\"" && q != "\047") return ""; s = substr(s, 2); return substr(s, 1, index(s, q) - 1) }
    /^[ \t]*\[/ { sec = $0; sub(/#.*/, "", sec); gsub(/[ \t]/, "", sec); next }
    sec == "[install]" && /^[ \t]*root[ \t]*=/ { r = val($0) }
    sec == "" && /^[ \t]*install\.root[ \t]*=/ { r = val($0) }
    END { if (r != "") print r }' "$1"
}

# cargo install 装到哪（bin 目录的上一级），和 cargo 的优先级一致：CARGO_INSTALL_ROOT > 配置里的 install.root
# （仓库目录往上各级 .cargo/config.toml 越近越优先，最后才是 $CARGO_HOME/config.toml）> $CARGO_HOME > ~/.cargo。
# 配置里的相对路径相对于 .cargo 目录的上一级。
install_root() {
  if [[ -n ${CARGO_INSTALL_ROOT-} ]]; then printf '%s\n' "$CARGO_INSTALL_ROOT"; return; fi
  local home=${CARGO_HOME:-$HOME/.cargo} d=$here root='' r f base
  local files=()
  while :; do
    files=("${d%/}/.cargo/config.toml" "${d%/}/.cargo/config" "${files[@]}")
    [[ $d == / ]] && break
    d=$(dirname "$d")
  done
  # 先低后高：后找到的盖掉先找到的
  for f in "$home/config.toml" "$home/config" "${files[@]}"; do
    [[ -f $f ]] || continue
    r=$(_config_install_root "$f") || continue
    [[ -n $r ]] || continue
    base=$(dirname "$(dirname "$f")")
    [[ $r == /* ]] || r=$base/$r
    root=$r
  done
  printf '%s\n' "${root:-$home}"
}
bindir=$(install_root)/bin

# 读一次缓存起来：在主 shell 里先调一次 cargo_list >/dev/null（$(…) 里调用是子 shell，存不住）
_cargo_list="" _cargo_listed=0
cargo_list() {
  if [[ $_cargo_listed -eq 0 ]]; then _cargo_list=$(cargo install --list); _cargo_listed=1; fi
  [[ -z $_cargo_list ]] || printf '%s\n' "$_cargo_list"
}

# 包里要装的命令：Cargo.toml 里各 [[bin]] 的 name（library 只装 booklib，bookconv 的三个都装）。
# 认 `name = "x"`、`name='x'`、行首缩进和行尾注释。
pkg_bins() {
  awk '
    /^[ \t]*\[\[[ \t]*bin[ \t]*\]\]/ { b = 1; next }
    /^[ \t]*\[/ { b = 0 }
    b && /^[ \t]*name[ \t]*=/ { s = $0; sub(/^[^=]*=[ \t]*/, "", s); q = substr(s, 1, 1)
      if (q == "\"" || q == "\047") { s = substr(s, 2); printf "%s%s", sep, substr(s, 1, index(s, q) - 1); sep = " " } }
  ' "$here/crates/$1/Cargo.toml"
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
    # 二进制那一行缩进 4 格：去掉全部前导空格再比
    [[ ${line#"${line%%[! ]*}"} == "$bin" ]] || continue
    path=${head##*(}; path=${path%)}
    [[ -d $path && $(cd "$path" && pwd -P) == "$here/crates/"* ]] && return 1
    printf '%s\n' "$head"
    return 0
  done < <(cargo_list)
  return 1
}

# 打印脚本 $1 开头的说明（第 2 行起连续的 # 注释，去掉 `# `）：--help 用，改注释不用跟着改行号
print_header() {
  awk 'NR == 1 { next } /^#/ { sub(/^# ?/, ""); print; next } { exit }' "$1"
}
