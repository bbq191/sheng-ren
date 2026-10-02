#!/usr/bin/env bash
# 把 Move 书库里我们生成的漫画登记给 Move 上的页边距代理：第一次打开这本书时，由 xochitl 自己把页边距设成
# profile 的 comic_reader_margins（1），整页图左右离屏幕边缘 1px。
#
# 用法: xochitl/comic-margins.sh [--host=root@10.11.99.1] [--write]
#   不加 --write 只列出要登记的书。
#
# 认书：书里有 META-INF/eink-reader-margins（优化器只给 xochitl 模式的漫画写，内容是要设的页边距），
#       页边距（.content 的 margins）还不是这个值，而且以前没登记过。
# 每本只登记一次（记在 Move 上的 /home/root/.local/state/eink/comic-margins.done）：之后你在界面上改回去，不会再被设回来。
# 为什么不直接改 .content：运行中的 xochitl 会用内存里的状态把它盖回去（上游真机 5 次只成功 1 次），只能让 xochitl 自己设。
# 依赖 Move 上已经装好的书架服务（book-serve 的队列 comic-margins.json）和注入 xochitl 的页边距代理（shelf-comic-margins.qmd），
# 以及网页「管理→实验室→漫画页边距」开关（reading-qol.json 的 comicMinMargin）打开。
set -euo pipefail
host=root@10.11.99.1 write=0
for a in "$@"; do
  case $a in
    --host=*) host=${a#--host=} ;;
    --write) write=1 ;;
    -h | --help) sed -n '2,13p' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
    *) echo "不认识的参数 $a" >&2; exit 2 ;;
  esac
done

lib=/home/root/.local/share/remarkable/xochitl
queue=/home/root/.local/state/shelf/books/comic-margins.json
done_file=/home/root/.local/state/eink/comic-margins.done
ssh_() { ssh -o BatchMode=yes -o ConnectTimeout=5 "$host" "$@"; }

ssh_ true || { echo "✗ 连不上 $host（USB 连着是 root@10.11.99.1，Wi-Fi 是 root@<Move 的 IP>）" >&2; exit 1; }

# 前提：代理装了、队列目录在、开关开着
ssh_ "ls /home/root/xovi/exthome/qt-resource-rebuilder/shelf-comic-margins.qmd >/dev/null 2>&1" \
  || { echo "✗ Move 上没有页边距代理（shelf-comic-margins.qmd），登记了也没人执行" >&2; exit 1; }
ssh_ "test -d ${queue%/*}" || { echo "✗ Move 上没有书架服务的队列目录 ${queue%/*}" >&2; exit 1; }
# 认书靠 unzip 读书里的标记：没有 unzip 时每本都会被当成"没标记"，误报"没有要登记的漫画"
ssh_ "command -v unzip >/dev/null" || { echo "✗ Move 上没有 unzip 命令，读不了书里的标记" >&2; exit 1; }
# 开关文件在书架服务自己的数据目录里（~/.local/share/<目录>/reading-qol.json），按文件名找
if ! ssh_ "grep -qs '\"comicMinMargin\": *true' /home/root/.local/share/*/reading-qol.json"; then
  echo "✗ 「漫画页边距」开关没开（reading-qol.json 的 comicMinMargin）：代理会忽略登记。先在 Move 的网页「管理→实验室」打开" >&2
  exit 1
fi

# 列出候选：uuid|要设的页边距|现在的页边距|书名（只看 EPUB；busybox 环境，逐本读标记）
list=$(ssh_ "cd $lib || exit 1; for c in *.content; do
  u=\${c%.content}
  grep -q '\"fileType\": *\"epub\"' \$c || continue
  want=\$(unzip -p \$u.epub META-INF/eink-reader-margins 2>/dev/null) || continue
  [ -n \"\$want\" ] || continue
  grep -qx \$u $done_file 2>/dev/null && continue
  cur=\$(grep -o '\"margins\": *[0-9]*' \$c | grep -o '[0-9]*\$')
  name=\$(grep -o '\"visibleName\": *\"[^\"]*\"' \$u.metadata | cut -d'\"' -f4)
  echo \"\$u|\$want|\$cur|\$name\"
done") || { echo "✗ 读 Move 书库（$lib）失败" >&2; exit 1; }

todo=()
while IFS='|' read -r u want cur name; do
  [[ -n $u ]] || continue
  if [[ $cur == "$want" ]]; then
    echo "  = 《$name》页边距已经是 $want"
  else
    echo "  + 《$name》页边距 ${cur:-?} → $want（第一次打开时设）"
    todo+=("$u|$want")
  fi
done <<<"$list"

if [[ ${#todo[@]} -eq 0 ]]; then
  echo "✓ 没有要登记的漫画"
  exit 0
fi
if [[ $write -eq 0 ]]; then
  echo "（没写：加 --write 登记这 ${#todo[@]} 本）"
  exit 0
fi

# 合并进队列（队列里别的书保持不动），先写临时文件再改名。书架服务会边执行边从队列里删：
# 写回前核对队列没被改过（比对读时的内容），改过就不写、让你重跑，免得把它刚删掉的条目又写回去。
now=$(date +%s)
# 一次读回"校验和 + 内容"（队列还没有时校验和记 none；文件空着时内容按 []，校验和那行总以换行结尾）
qsum="if [ -f $queue ]; then md5sum < $queue | cut -d' ' -f1; else echo none; fi"
snap=$(ssh_ "$qsum; if [ -s $queue ]; then cat $queue; else echo '[]'; fi")
sum=${snap%%$'\n'*} old=${snap#*$'\n'}
[[ $snap == *$'\n'* && $sum =~ ^([0-9a-f]{32}|none)$ ]] || { echo "✗ 读回的队列不认识：$sum" >&2; exit 1; }
new=$(printf '%s' "$old" | python3 -c '
import json, sys
q = json.loads(sys.stdin.read().strip() or "[]")
have = {p["uuid"] for p in q}
for item in sys.argv[2:]:
    u, m = item.split("|")
    if u not in have:
        q.append({"uuid": u, "margins": int(m), "at": int(sys.argv[1])})
print(json.dumps(q, ensure_ascii=False))
' "$now" "${todo[@]}")
uuids=("${todo[@]%%|*}")
for u in "${uuids[@]}"; do
  [[ $u =~ ^[0-9a-fA-F-]+$ ]] || { echo "✗ 不认识的 uuid：$u" >&2; exit 1; }
done
len=$(printf '%s' "$new" | wc -c)
# 写队列和记 done 在同一次 ssh 里：先把两个临时文件都写好、核对完，再依次改名；任一步失败整体失败、临时文件删掉。
# （分两次 ssh 的话，队列写成了而记 done 那次失败，下次会重复登记。）
printf '%s' "$new" | ssh_ "set -e
  qt=$queue.eink-tmp dt=$done_file.eink-tmp
  trap 'rm -f \$qt \$dt' EXIT
  [ \"\$($qsum)\" = $sum ] || { echo '✗ 队列刚被书架服务改过，没写：再跑一次' >&2; exit 3; }
  mkdir -p ${done_file%/*}
  cat > \$qt
  [ \$((\$(wc -c < \$qt))) -eq $((len)) ] || { echo '✗ 队列没传完整，没写' >&2; exit 4; }
  { if [ -f $done_file ]; then cat $done_file; fi; printf '%s\\n' ${uuids[*]}; } > \$dt
  mv \$qt $queue
  mv \$dt $done_file"
echo "✓ 登记了 ${#todo[@]} 本：在 Move 上打开这些书，约 2 秒后页边距自动变成 1（每本只设这一次）"
