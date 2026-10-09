#!/usr/bin/env bash
# 真书回归：用某一版 epub-optimize 把全部文字书和一卷漫画各优化一遍，产物按编号放进输出目录，供 compare.py 比较。
#
# 用法: tools/regress/run.sh <epub-optimize 路径> <输出目录> [--device=<模式>] [epub-optimize 的其它参数…]
#   书从 $REGRESS_BOOKS（缺省 ~/Documents/ereader/books）里找：路径含"漫画"的算漫画（只取排序后第一卷），其余是文字书。
#   原书只读：产物只写到输出目录。缺省模式 ireader；kindle 模式的产物还要再转 KFX 才是设备上的书（KFX 回归见 docs/development.md#kfx）。
#   输出目录里：NN.epub（文字书）、comic.epub、NN.log、index.txt（编号 → 原书路径；compare.py 按它配对新旧、做"对原书"核对）、
#   device.txt（用的哪个模式）。
# 典型用法（改动前后各跑一次再比）：
#   git worktree add target/regress-base HEAD && (cd target/regress-base && cargo build --release -p bookconv --bin epub-optimize)
#   tools/regress/run.sh target/regress-base/target/release/epub-optimize target/regress/旧
#   cargo build --release -p bookconv --bin epub-optimize && tools/regress/run.sh target/release/epub-optimize target/regress/新
#   tools/regress/compare.py target/regress/旧 target/regress/新
set -euo pipefail
bin=${1:?用法见文件头} out=${2:?用法见文件头}
shift 2
dev=--device=ireader
args=()
for a in "$@"; do case $a in --device=*) dev=$a ;; *) args+=("$a") ;; esac; done
books=${REGRESS_BOOKS:-$HOME/Documents/ereader/books}
[[ -x $bin && -f $bin ]] || { echo "✗ $bin 不是可执行文件" >&2; exit 2; }
[[ -d $books ]] || { echo "✗ 没有书目录 $books" >&2; exit 2; }
mkdir -p "$out"
# 清掉上次留下的产物：不清的话这次没生成的书会拿旧 NN.epub 去比，悄悄通过
rm -f -- "$out"/*.epub "$out"/*.log "$out/index.txt" "$out/device.txt"
: >"$out/index.txt"
echo "$dev" >"$out/device.txt"
fail=0
i=0
while IFS= read -r -d '' f; do
  i=$((i + 1)); n=$(printf %02d $i)
  "$bin" "$dev" "${args[@]}" "$f" "$out/$n.epub" </dev/null >"$out/$n.log" 2>&1 || { echo "✗ $n $f（见 $n.log）"; fail=1; }
  printf '%s\t%s\n' "$n" "$f" >>"$out/index.txt"
done < <(find "$books" -name '*.epub' -not -path '*漫画*' -print0 | sort -z)
# 全读进数组再取第一个：管道里读完第一个就走的话，漫画多时 sort 写不完收到 SIGPIPE，pipefail 下整个脚本就退出了
mapfile -d '' comics < <(find "$books" -path '*漫画*' -name '*.epub' -print0 | sort -z)
comic=${comics[0]-}
if [[ $i -eq 0 && -z $comic ]]; then
  echo "✗ $books 里一本 EPUB 都没找到" >&2
  exit 2
fi
if [[ -n $comic ]]; then
  "$bin" "$dev" "${args[@]}" "$comic" "$out/comic.epub" </dev/null >"$out/comic.log" 2>&1 || { echo "✗ 漫画 $comic"; fail=1; }
  printf 'comic\t%s\n' "$comic" >>"$out/index.txt"
fi
echo "$([[ $fail -eq 0 ]] && echo ✓ || echo ✗) $i 本文字书$([[ -n $comic ]] && echo " + 1 卷漫画") → $out"
exit $fail
