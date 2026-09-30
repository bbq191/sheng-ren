#!/usr/bin/env bash
# 真书回归：用某一版 epub-optimize 把全部文字书和一卷漫画各优化一遍，产物按编号放进输出目录，供 compare.py 比较。
#
# 用法: tools/regress/run.sh <epub-optimize 路径> <输出目录> [--device=<模式>] [epub-optimize 的其它参数…]
#   书从 $REGRESS_BOOKS（缺省 ~/Documents/ereader/books）里找：路径含"漫画"的算漫画（只取排序后第一卷），其余是文字书。
#   原书只读：产物只写到输出目录。缺省模式 ireader（Kindle 的规则和它一样、只是阅读范围不同，AZW3 转换另有 azw3 crate 的回读测试）。
#   输出目录里：NN.epub（文字书）、comic.epub、NN.log、index.txt（编号 → 原书路径；compare.py 按它配对新旧、做"对原书"核对）、
#   device.txt（用的哪个模式）。
# 典型用法（改动前后各跑一次再比）：
#   git worktree add target/regress-base HEAD && (cd target/regress-base && cargo build --release -p bookconv --bin epub-optimize)
#   tools/regress/run.sh target/regress-base/target/release/epub-optimize 旧
#   cargo build --release -p bookconv --bin epub-optimize && tools/regress/run.sh target/release/epub-optimize 新
#   tools/regress/compare.py 旧 新
set -euo pipefail
bin=${1:?用法见文件头} out=${2:?用法见文件头}
shift 2
dev=--device=ireader
args=()
for a in "$@"; do case $a in --device=*) dev=$a ;; *) args+=("$a") ;; esac; done
books=${REGRESS_BOOKS:-$HOME/Documents/ereader/books}
mkdir -p "$out"
: >"$out/index.txt"
echo "$dev" >"$out/device.txt"
fail=0
i=0
while IFS= read -r -d '' f; do
  i=$((i + 1)); n=$(printf %02d $i)
  "$bin" "$dev" "${args[@]}" "$f" "$out/$n.epub" >"$out/$n.log" 2>&1 || { echo "✗ $n $f（见 $n.log）"; fail=1; }
  printf '%s\t%s\n' "$n" "$f" >>"$out/index.txt"
done < <(find "$books" -name '*.epub' -not -path '*漫画*' -print0 | sort -z)
comic=$(find "$books" -path '*漫画*' -name '*.epub' -print0 | sort -z | { IFS= read -r -d '' f || true; printf '%s' "$f"; })
if [[ -n $comic ]]; then
  "$bin" "$dev" "${args[@]}" "$comic" "$out/comic.epub" >"$out/comic.log" 2>&1 || { echo "✗ 漫画 $comic"; fail=1; }
  printf 'comic\t%s\n' "$comic" >>"$out/index.txt"
fi
echo "$([[ $fail -eq 0 ]] && echo ✓ || echo ✗) $i 本文字书$([[ -n $comic ]] && echo " + 1 卷漫画") → $out"
exit $fail
