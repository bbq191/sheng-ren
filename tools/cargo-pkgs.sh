# shellcheck shell=bash disable=SC2034,SC2154  # 被 source：TOOL_PKGS、rootdir、bindir 给调用方用，here 由调用方设
# install.sh、uninstall.sh 共用（source 进来用，不单独运行）。调用方先设好 here（仓库根，pwd -P）。
#
# cargo install --list 的格式：「包名 v版本 (路径):」一行，下面每个二进制缩进 4 格一行。
# 包名 library、bookconv 很普通，别处可能装着同名的包：只认从本仓库路径装的。

# 开发工具所在的包（install.sh --tools 装、--no-tools 卸）
TOOL_PKGS=(bookconv)

# cargo 把设成空值的 CARGO_INSTALL_ROOT 当成当前目录（装进 ./bin、在当前目录记账）：按没设处理。
# 空的 CARGO_HOME 它按没设处理（~/.cargo），和下面一致。
[[ -n ${CARGO_INSTALL_ROOT-} ]] || unset CARGO_INSTALL_ROOT
# cargo 的配置（install.root 等）按当前目录逐级往上找：固定在仓库根运行，从哪个目录调用脚本结果都一样
cd "$here" || exit 1

# 不用 root 跑别人的仓库：sudo ./install.sh 会把仓库的 target/ 写成 root 的（之后自己 cargo build 报权限错），
# 命令也装进 /root/.cargo/bin 而不是自己的。仓库本来就属于 root 的（容器里）照常。
if [[ $EUID -eq 0 && $(stat -c %u "$here") -ne 0 ]]; then
  echo "✗ 别用 root（sudo）运行：命令会装进 root 的目录、仓库的 target/ 会变成 root 的；用仓库主人的账号直接运行" >&2
  exit 1
fi

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
rootdir=$(install_root)
bindir=$rootdir/bin

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
    [[ -d $path && $(cd "$path" 2>/dev/null && pwd -P) == "$here/crates/$pkg" ]] || continue
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
    [[ -d $path && $(cd "$path" 2>/dev/null && pwd -P) == "$here/crates/"* ]] && return 1
    printf '%s\n' "$head"
    return 0
  done < <(cargo_list)
  return 1
}

# 各包要装的命令名（[`pkg_bins`]）读一次缓存起来：install、uninstall 每个包要用好几次，不每次起一个 awk。
# 和 cargo_list 一样要在主 shell 里先调 load_bins（$(…) 是子 shell，存不住）。
declare -A _bins=()
load_bins() {
  local d
  for d in "$here"/crates/*/; do
    d=${d%/}
    _bins[${d##*/}]=$(pkg_bins "${d##*/}")
  done
}
bins() {
  printf '%s' "${_bins[$1]-}"
}

# 装之前看能不能写安装目录：bin 目录和记账文件所在的目录（还没建的看最近一级已有的上级目录）。不能写就在编译之前停下，
# 别编译好几分钟以后才在备份那一步报 mkdir 的权限错。
require_writable_root() {
  local d
  for d in "$rootdir" "$bindir"; do
    while [[ ! -e $d ]]; do d=$(dirname "$d"); done
    if [[ ! -d $d || ! -w $d ]]; then
      echo "✗ 没有权限写 $d（安装目录 ${rootdir/#"$HOME"/\~}）：换个能写的位置（CARGO_INSTALL_ROOT 或 cargo 配置的 install.root），别用 sudo" >&2
      exit 1
    fi
  done
}

# 同一个安装目录同时只跑一个 install.sh / uninstall.sh：两个一起跑时共用 bin 下的备份目录，后一个开头清掉前一个的备份、
# 结束时又删掉对方的，前一个装到一半失败就退不回去了。锁的是安装目录本身（flock 一个只读打开的目录），不另留锁文件；
# 进程退出锁就放了（kill -9 也一样）。没有 flock（不是 Linux 的 util-linux）就不锁，照旧能用。
lock_install_root() {
  command -v flock >/dev/null || return 0
  mkdir_tracked "$rootdir"
  exec 9<"$rootdir"
  if ! flock -n 9; then
    echo "✗ 另一个 install.sh / uninstall.sh 正在往 ${rootdir/#"$HOME"/\~} 装或卸，等它做完再运行" >&2
    exit 1
  fi
}

# 脚本自己建的目录（上级在前）：没装成（编译失败、中断、回滚）时由 remove_made_dirs 删掉还空着的，不留空目录。原来就有的不记。
made_dirs=()
mkdir_tracked() {
  local d=$1 new=()
  while [[ ! -e $d ]]; do
    new=("$d" "${new[@]}")
    d=$(dirname "$d")
  done
  [[ ${#new[@]} -eq 0 ]] && return 0
  mkdir -p "$1"
  made_dirs+=("${new[@]}")
}
# 从最深的往上删：只删空的（rmdir 删不掉非空目录，装成了的目录里有东西，删不到）。`cargo install --list` 会在新建的安装目录里
# 写两个空的记账文件（什么都没装）：这次新建的目录里空的记账文件先删掉，原来就有的目录不碰。
remove_made_dirs() {
  local i d f
  for ((i = ${#made_dirs[@]} - 1; i >= 0; i--)); do
    d=${made_dirs[i]}
    for f in "$d/.crates.toml" "$d/.crates2.json"; do
      if [[ -f $f && ! -s $f ]]; then rm -f "$f"; fi
    done
    rmdir "$d" 2>/dev/null || true
  done
}

# 打印脚本 $1 开头的说明（第 2 行起连续的 # 注释，去掉 `# `）：--help 用，改注释不用跟着改行号
print_header() {
  awk 'NR == 1 { next } /^#/ { sub(/^# ?/, ""); print; next } { exit }' "$1"
}
