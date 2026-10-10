#!/usr/bin/env bash
# 安装命令行工具到 cargo 的 bin 目录（$CARGO_INSTALL_ROOT/bin、cargo 配置的 install.root 或 $CARGO_HOME/bin，缺省 ~/.cargo/bin）。
#
# 用法: ./install.sh [--tools | --no-tools]
#   缺省装：booklib（书库；查看/改写 EPUB 元数据也在里面：booklib meta --edit）
#   --tools     另装开发和排查问题用的：epub-optimize、readable-probe、readable-measure
#   --no-tools  卸掉上面这些开发工具，只留 booklib
#   kfx 包的 epub-to-kfx、kfx-dump、kfx-repack 不随 --tools 装，用 cargo run --release -p kfx --bin <命令> --
#   都不加：沿用上次的选择（装过开发工具就一起升级，免得工具停在旧版本、和 booklib 的规则对不上）
# 重复运行 = 用当前代码重新编译安装（升级）。卸载见 ./uninstall.sh。
# 先把要装的命令全部编译好再动安装目录：编译失败什么都不改；安装中途失败或被中断（Ctrl-C）就退回装之前的样子。
# 退出码：0 装好；1 没有 cargo、用 root 运行、同名命令被别的包占着或编译安装失败（已回滚）；2 参数不对。
# 直接运行，别 source（set -e、cd 会留在你的 shell 里，出错时连 shell 一起退出）
(return 0 2>/dev/null) && { echo "✗ 直接运行 ./install.sh，别 source" >&2; return 2; }
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd -P)
# shellcheck source=tools/cargo-pkgs.sh
. "$here/tools/cargo-pkgs.sh"

tools=keep
for a in "$@"; do
  case $a in
    --tools) tools=1 ;;
    --no-tools) tools=0 ;;
    -h | --help) print_header "$here/install.sh"; exit 0 ;;
    *) echo "不认识的参数 $a（见 --help）" >&2; exit 2 ;;
  esac
done
command -v cargo >/dev/null || { echo "✗ 需要 Rust 工具链（cargo）：https://rustup.rs" >&2; exit 1; }
# 复用仓库的 target/（和 cargo build --release 共享编译缓存；不设的话 cargo install 每次在临时目录里从头编）
export CARGO_TARGET_DIR=${CARGO_TARGET_DIR:-$here/target}
cargo_list >/dev/null

if [[ $tools == keep ]]; then
  tools=0
  for p in "${TOOL_PKGS[@]}"; do
    if ours_installed "$p" >/dev/null; then tools=1; echo "上次装过开发工具，这次一起升级（不要了用 --no-tools）"; break; fi
  done
fi

pkgs=(library)
if [[ $tools -eq 1 ]]; then pkgs+=("${TOOL_PKGS[@]}"); fi

# 别的包占着同名命令时停下来，不用 --force 抢过来（那会悄悄换掉别人装的东西）
conflict=0
for p in "${pkgs[@]}"; do
  for b in $(pkg_bins "$p"); do
    if other=$(foreign_owner "$b"); then
      echo "✗ 命令 $b 已经被别的包装过：$other" >&2
      echo "  确认不要了就先 cargo uninstall 它，再运行本脚本" >&2
      conflict=1
    fi
  done
done
[[ $conflict -eq 0 ]] || exit 1

# 每个包要装的命令（同一个包的二进制一次装齐：cargo 按包记账，这次没列出的二进制会留在旧版本）
declare -A bins_of=()
build_args=()
for p in "${pkgs[@]}"; do
  bins_of[$p]=$(pkg_bins "$p")
  [[ -n ${bins_of[$p]} ]] || { echo "✗ crates/$p/Cargo.toml 里没认出 [[bin]]" >&2; exit 1; }
  build_args+=(-p "$p")
  for b in ${bins_of[$p]}; do build_args+=(--bin "$b"); done
done

# 第一步：一次编译全部要装的命令（几个包并行编，比逐个 cargo install 快）。失败就停，安装目录一点没动。
# 和 cargo install 同一个 target/、同样 --locked 和 release：下面装的时候不再重编，只是拷贝。
echo "编译 ${pkgs[*]}…"
cargo build --release --locked "${build_args[@]}" || exit 1

# 第二步：装之前留一份现状，装到一半失败或被中断时退回去——不然会剩下 booklib 是新的、开发工具还是旧的，规则对不上。
# 留的是这次要装的命令文件和 cargo 的记账文件（.crates.toml、.crates2.json）；放在 bin 目录里，退回时同一文件系统内改名。
# 被 kill -9 这种来不及退回的，留下的备份下次运行时清掉（重新装一遍就是完整的新版本）。
backup=$bindir/.sheng-ren-install-backup
installing=0
absent=() # 装之前不存在的命令：退回时删掉
rollback() {
  echo "✗ 安装没完成，退回到装之前的样子" >&2
  local f b
  for b in "${absent[@]}"; do rm -f "$bindir/$b"; done
  for f in "$backup"/*; do # 命令文件（记账文件是点开头的，* 不匹配，下面单独退）
    if [[ -e $f ]]; then mv -f "$f" "$bindir/"; fi
  done
  for f in .crates.toml .crates2.json; do
    if [[ -e $backup/$f ]]; then
      mv -f "$backup/$f" "$rootdir/$f"
    elif [[ -e $backup/$f.absent ]]; then
      rm -f "$rootdir/$f" # 装之前没有：这次装的时候才建的
    fi
  done
}
on_exit() {
  local code=$?
  if [[ $installing -eq 1 && $code -ne 0 ]]; then rollback || echo "✗ 退回也失败了：重新运行 ./install.sh 会装成完整的新版本" >&2; fi
  rm -rf "$backup"
}
trap on_exit EXIT
trap 'exit 130' INT
trap 'exit 143' TERM

rm -rf "$backup"
mkdir -p "$backup"
for f in .crates.toml .crates2.json; do
  if [[ -e $rootdir/$f ]]; then cp -p "$rootdir/$f" "$backup/"; else : >"$backup/$f.absent"; fi
done
for p in "${pkgs[@]}"; do
  for b in ${bins_of[$p]}; do
    if [[ -e $bindir/$b ]]; then cp -p "$bindir/$b" "$backup/$b"; else absent+=("$b"); fi
  done
done

# 第三步：安装（编好的直接拷过去）
installing=1
for p in "${pkgs[@]}"; do
  echo "安装 ${bins_of[$p]}…"
  args=()
  for b in ${bins_of[$p]}; do args+=(--bin "$b"); done
  cargo install --locked --path "$here/crates/$p" "${args[@]}" || exit 1
done
installing=0

if [[ $tools -eq 0 ]]; then
  for p in "${TOOL_PKGS[@]}"; do
    if spec=$(ours_installed "$p"); then
      cargo uninstall --quiet "$spec"
      echo "✓ 卸掉开发工具 $(pkg_bins "$p")"
    fi
  done
fi

echo "✓ 已装到 ${bindir/#"$HOME"/\~}/"
# 按 PATH 里实际找到的那个判断
found=$(command -v booklib || true)
if [[ -z $found ]]; then
  case ${SHELL##*/} in
    fish) echo "  booklib 不在 PATH 里：运行 fish_add_path '$bindir'" ;;
    *) echo "  booklib 不在 PATH 里：把 export PATH=\"$bindir:\$PATH\" 加进 shell 的配置文件（fish 用 fish_add_path '$bindir'）" ;;
  esac
elif [[ $(realpath "$found") != "$(realpath "$bindir/booklib" 2>/dev/null || true)" ]]; then
  echo "  注意：PATH 里先找到的是 $found，不是刚装的 $bindir/booklib；把 $bindir 放到 PATH 前面，或删掉旧的那个"
fi
echo "  开始用：booklib --help，完整用法见 docs/usage.md"
