#!/usr/bin/env bash
# 安装命令行工具到 cargo 的 bin 目录（$CARGO_INSTALL_ROOT/bin 或 $CARGO_HOME/bin，缺省 ~/.cargo/bin）。
#
# 用法: ./install.sh [--tools]
#   缺省装：booklib（书库；查看/改写 EPUB 元数据也在里面：booklib meta --edit）
#   --tools 另装开发和排查问题用的：epub-optimize、epub-to-azw3、readable-probe、readable-measure
#   以前用 --tools 装过的，不加 --tools 重跑也会一起升级（免得开发工具停在旧版本、和 booklib 的规则对不上）
# 重复运行 = 用当前代码重新编译安装（升级）。卸载见 ./uninstall.sh。
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd)

tools=0
for a in "$@"; do
  case $a in
    --tools) tools=1 ;;
    -h | --help) sed -n '2,8p' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
    *) echo "不认识的参数 $a（见 --help）" >&2; exit 2 ;;
  esac
done
command -v cargo >/dev/null || { echo "✗ 需要 Rust 工具链（cargo）：https://rustup.rs" >&2; exit 1; }
bindir=${CARGO_INSTALL_ROOT:-${CARGO_HOME:-$HOME/.cargo}}/bin
# 复用仓库的 target/（和 cargo build --release 共享编译缓存；不设的话 cargo install 每次在临时目录里从头编）
export CARGO_TARGET_DIR=${CARGO_TARGET_DIR:-$here/target}

# 本仓库装过的包：cargo install --list 里「包名 版本 (路径):」一行，下面每个二进制缩进一行
installed=$(cargo install --list)
if [[ $tools -eq 0 ]] && grep -A20 -F "($here/crates/bookconv):" <<<"$installed" | grep -qx '    epub-optimize'; then
  echo "上次装过开发工具，这次一起升级"
  tools=1
fi

# 以前的版本单独装过 ebook-meta（在 bookconv 包里，2026-10-01 并进 booklib meta --edit）：整个包先卸掉，
# 不然 cargo 按包记账，重装 bookconv 时没列出的 ebook-meta 会留在旧版本
line=$(grep -F " ($here/crates/bookconv):" <<<"$installed" | grep '^bookconv v' || true)
if [[ -n $line ]] && grep -A20 -F "($here/crates/bookconv):" <<<"$installed" | grep -qx '    ebook-meta'; then
  ver=${line#bookconv v}
  ver=${ver%% *}
  cargo uninstall --quiet "path+file://$here/crates/bookconv#bookconv@$ver"
  echo "卸掉了旧的 ebook-meta（改用 booklib meta --edit）"
fi

# 同一个包的二进制一次装齐：cargo 按包记账，这次没列出的二进制会留在旧版本。--force：代码改过也重新装
cargo_install() { cargo install --locked --force --quiet --path "$here/crates/$1" "${@:2}"; }
echo "编译安装 booklib…（第一次要编几分钟，中间不出声）"
cargo_install library --bin booklib
if [[ $tools -eq 1 ]]; then
  echo "编译安装 epub-optimize readable-probe readable-measure…"
  cargo_install bookconv --bin epub-optimize --bin readable-probe --bin readable-measure
  echo "编译安装 epub-to-azw3…"
  cargo_install azw3 --bin epub-to-azw3
fi

echo "✓ 已装到 ${bindir/#$HOME/\~}/"
# 用 command -v 判断而不是比对目录：~/.cargo/config.toml 里的 install.root 也能改安装位置
if ! command -v booklib >/dev/null; then
  echo "  booklib 不在 PATH 里：把 export PATH=\"$bindir:\$PATH\" 加进 shell 的配置文件（fish：fish_add_path $bindir）"
fi
echo "  开始用：booklib --help，完整用法见 docs/usage.md"
