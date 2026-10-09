#!/usr/bin/env bash
# 安装命令行工具到 cargo 的 bin 目录（$CARGO_INSTALL_ROOT/bin、cargo 配置的 install.root 或 $CARGO_HOME/bin，缺省 ~/.cargo/bin）。
#
# 用法: ./install.sh [--tools | --no-tools]
#   缺省装：booklib（书库；查看/改写 EPUB 元数据也在里面：booklib meta --edit）
#   --tools     另装开发和排查问题用的：epub-optimize、readable-probe、readable-measure，
#               以及转 MOBI 词典的 mobi-dict-to-stardict
#   --no-tools  卸掉上面这些开发工具，只留 booklib
#   都不加：沿用上次的选择（装过开发工具就一起升级，免得工具停在旧版本、和 booklib 的规则对不上）
# 重复运行 = 用当前代码重新编译安装（升级）。卸载见 ./uninstall.sh。
# 退出码：0 装好；1 没有 cargo、同名命令被别的包占着或编译安装失败；2 参数不对。
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

# 同一个包的二进制一次装齐：cargo 按包记账，这次没列出的二进制会留在旧版本。
# 从本地路径装的包每次都会重新编译安装（没改过的代码不重编，只是重新拷一遍），不用 --force。
for p in "${pkgs[@]}"; do
  bins=$(pkg_bins "$p")
  [[ -n $bins ]] || { echo "✗ crates/$p/Cargo.toml 里没认出 [[bin]]" >&2; exit 1; }
  echo "编译安装 $bins…"
  args=()
  for b in $bins; do args+=(--bin "$b"); done
  cargo install --locked --path "$here/crates/$p" "${args[@]}"
done

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
