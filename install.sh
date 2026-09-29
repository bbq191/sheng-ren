#!/usr/bin/env bash
# 安装命令行工具到 cargo 的 bin 目录（$CARGO_INSTALL_ROOT/bin 或 $CARGO_HOME/bin，缺省 ~/.cargo/bin）。
#
# 用法: ./install.sh [--tools]
#   缺省装：booklib（书库）、ebook-meta（查看/改写 EPUB 元数据）
#   --tools 另装开发和排查问题用的：epub-optimize、readable-probe、readable-measure
# 重复运行 = 用当前代码重新编译安装（升级）。卸载见 ./uninstall.sh。
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd)

tools=0
for a in "$@"; do
  case $a in
    --tools) tools=1 ;;
    -h | --help) sed -n '2,7p' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
    *) echo "不认识的参数 $a（见 --help）" >&2; exit 2 ;;
  esac
done
command -v cargo >/dev/null || { echo "✗ 需要 Rust 工具链（cargo）：https://rustup.rs" >&2; exit 1; }
bindir=${CARGO_INSTALL_ROOT:-${CARGO_HOME:-$HOME/.cargo}}/bin
# 复用仓库的 target/（和 cargo build --release 共享编译缓存；不设的话 cargo install 每次在临时目录里从头编）
export CARGO_TARGET_DIR=${CARGO_TARGET_DIR:-$here/target}

# Calibre 也有一个叫 ebook-meta 的命令：同名时谁生效取决于 PATH 里的先后
other=$(command -v ebook-meta || true)
if [[ -n $other && $other != "$bindir/ebook-meta" ]]; then
  echo "注意：PATH 里已有另一个 ebook-meta（$other，可能是 Calibre 的）。装好后执行哪个取决于 PATH 顺序。" >&2
fi

# 同一个包的二进制一次装齐（cargo 按包记账，分几次装会互相覆盖记录）；--force：代码改过也重新装
install() { cargo install --locked --force --quiet --path "$here/crates/$1" "${@:2}"; }
names=(ebook-meta)
[[ $tools -eq 1 ]] && names+=(epub-optimize readable-probe readable-measure)
bins=()
for n in "${names[@]}"; do bins+=(--bin "$n"); done
echo "编译安装 booklib…"
install library --bin booklib
echo "编译安装 ${names[*]}…"
install bookconv "${bins[@]}"

echo "✓ 已装到 ${bindir/#$HOME/\~}/"
case ":$PATH:" in
  *":$bindir:"*) ;;
  *) echo "  这个目录不在 PATH 里：把 export PATH=\"$bindir:\$PATH\" 加进 shell 的配置文件（fish：fish_add_path $bindir）" ;;
esac
missing=()
for c in luajit gio; do command -v "$c" >/dev/null || missing+=("$c"); done
[[ ${#missing[@]} -eq 0 ]] || echo "  给设备上的 KOReader 下发配置（koreader/apply.sh）还需要：${missing[*]}"
echo "  开始用：booklib --help，完整用法见 docs/usage.md"
