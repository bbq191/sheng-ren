# shellcheck shell=bash disable=SC2034,SC2329
# lib.sh —— apply.sh、check.sh 共用（source 进来用，不单独运行）。调用方先设好 KO_HERE（koreader/ 目录）。

# 设备上归我们管的三个配置文件（相对 KOReader 目录）
KO_FILES=(settings.reader.lua settings/gestures.lua settings/profiles.lua)

# ko_layers <设备 id>：按应用顺序输出每一层「目标文件 脚本 补丁 类别」。
#   类别 personal = 个人设置（卸载时保留），scheme = 文字书/漫画方案和设备差异（卸载时撤销），presets = 状态栏预设（卸载时撤销）。
ko_layers() {
  cat <<EOF
settings.reader.lua merge.lua personal/settings.reader.patch.lua personal
settings.reader.lua merge.lua schemes/text.settings.patch.lua scheme
settings.reader.lua merge.lua schemes/comic.settings.patch.lua scheme
settings.reader.lua merge.lua devices/$1/settings.reader.patch.lua scheme
settings.reader.lua presets.lua - presets
settings/gestures.lua merge.lua personal/gestures.patch.lua personal
settings/profiles.lua merge.lua schemes/profiles.patch.lua scheme
EOF
}

# ko_merge_all <目录> <设备 id>：把各层依次合并进 <目录> 下的三个配置文件（文件不存在就从空表开始）。
ko_merge_all() {
  local dir=$1 target script patch kind rc
  mkdir -p "$dir/settings"
  while read -r target script patch kind; do
    rc=0
    if [[ $patch == - ]]; then luajit "$KO_HERE/$script" "$dir/$target" >/dev/null || rc=$?
    else luajit "$KO_HERE/$script" "$dir/$target" "$KO_HERE/$patch" >/dev/null || rc=$?; fi
    [[ $rc -eq 0 || $rc -eq 10 ]] || { echo "✗ 合并出错（$script $patch）" >&2; return 3; }
  done < <(ko_layers "$2")
}

# ko_unmerge_all <目录> <备份根目录> <设备 id>：撤销方案、设备、预设层（见 unmerge.lua），个人设置保留。
#   每个文件的"原始配置"取备份里最早的那一份（第一次写它之前的样子）；从没备份过就按 KOReader 缺省。
ko_unmerge_all() {
  local dir=$1 backups=$2 dev=$3 f target script patch kind rc
  for f in "${KO_FILES[@]}"; do
    local args=()
    while read -r target script patch kind; do
      [[ $target == "$f" ]] || continue
      case $kind in
        personal) args+=("+$KO_HERE/$patch") ;;
        scheme) args+=("-$KO_HERE/$patch") ;;
        presets) args+=(presets) ;;
      esac
    done < <(ko_layers "$dev")
    [[ -f $dir/$f ]] || continue
    local orig
    orig=$(ls -d "$backups"/*/"$dev/$f" 2>/dev/null | sort | head -1 || true)
    echo "   $f：撤销的键还原成$([[ -n $orig ]] && echo "应用前的值（${orig/#$HOME/\~}）" || echo " KOReader 缺省值（没有应用前的备份）")"
    rc=0
    luajit "$KO_HERE/unmerge.lua" "$dir/$f" "${orig:--}" "${args[@]}" >/dev/null || rc=$?
    [[ $rc -eq 0 || $rc -eq 10 ]] || { echo "✗ 撤销出错（$f）" >&2; return 3; }
  done
}

# ── 设备文件操作（路径都相对 KOReader 目录）──
# ko_connect <设备 id> <临时目录>：读 devices/<id>/device.conf，连上设备，定义下面几个原语：
#   dev_has <路径>        存在返回 0
#   dev_get <路径> <本地> 读到本地文件；失败返回非 0 且不留半个文件
#   dev_put <本地> <路径> 写入（原子：写完才替换）
#   dev_rm <路径>         删除（不存在也算成功）
#   dev_mkdir <路径>      建目录（已存在也算成功）
#   dev_size <路径>       输出字节数
# MTP：一律经 gio 读写。gvfs 的 FUSE 路径（/run/user/…/gvfs/…）对读过的文件有缓存，gio 换掉文件后 FUSE 还会返回旧内容
# （2026-09-28 Kindle 实测：写入 19524 字节，经 FUSE 读回的是旧的 11487 字节），所以内容一律不经 FUSE 读。
# MTP 不支持覆盖写和改名：替换已有文件时先拷一份 .tmp，再删旧的、拷正式名——中途失败时设备上至少留着 .tmp 或旧文件。
# SSH：不用 scp（OpenSSH 9 起 scp 走 SFTP，设备上未必有 sftp-server），一律 ssh + cat；连接复用（ControlMaster），
# 一次 apply 只握手一次。写入先写同目录 .tmp 再 mv。
ko_connect() {
  local dev=$1 tmp=$2
  TRANSPORT=mtp
  FONTS=()
  RUNNING_CHECK=ask
  # shellcheck source=/dev/null
  source "$KO_HERE/devices/$dev/device.conf"
  case $TRANSPORT in
    mtp)
      command -v gio >/dev/null || { echo "✗ 缺 gio（gvfs）" >&2; return 1; }
      local uri
      uri=$(gio mount -li 2>/dev/null | grep -o "activation_root=mtp://${MTP_HOST_PREFIX}[^ ]*" | head -1 | cut -d= -f2 || true)
      [[ -z $uri ]] && { echo "✗ 没找到 $dev：USB 连上并解锁设备后再试" >&2; return 1; }
      gio info "$uri" >/dev/null 2>&1 || gio mount "$uri" 2>/dev/null || true
      KO_BASE="${uri%/}/$KOREADER_DIR"
      dev_has() { gio info "$KO_BASE/$1" >/dev/null 2>&1; }
      dev_get() { rm -f "$2"; gio copy "$KO_BASE/$1" "$2" 2>/dev/null || { rm -f "$2"; return 1; }; }
      dev_rm() { ! dev_has "$1" || gio remove "$KO_BASE/$1"; }
      dev_put() { # 设备上原来没有：直接拷（失败了由调用方回读核对发现、删掉）
        if ! dev_has "$2"; then gio copy "$1" "$KO_BASE/$2"; return; fi
        dev_rm "$2.tmp" && gio copy "$1" "$KO_BASE/$2.tmp" || return 1
        dev_rm "$2" && gio copy "$1" "$KO_BASE/$2" && dev_rm "$2.tmp"
      }
      dev_mkdir() { dev_has "$1" || gio mkdir "$KO_BASE/$1"; }
      dev_size() { gio info -a standard::size "$KO_BASE/$1" 2>/dev/null | awk '/standard::size:/ {print $2}'; }
      ;;
    ssh)
      command -v ssh >/dev/null || { echo "✗ 缺 ssh" >&2; return 1; }
      KO_SSH=(ssh -o BatchMode=yes -o ConnectTimeout=5 -o ControlMaster=auto -o "ControlPath=$tmp/ssh-%C" -o ControlPersist=60 "$SSH_HOST")
      "${KO_SSH[@]}" true 2>/dev/null || { echo "✗ 连不上 $dev（ssh $SSH_HOST）：连上网络、屏幕解锁后再试" >&2; return 1; }
      KO_BASE=$KOREADER_DIR
      dev_has() { "${KO_SSH[@]}" "test -e '$KO_BASE/$1'"; }
      dev_get() { "${KO_SSH[@]}" "cat '$KO_BASE/$1'" >"$2" 2>/dev/null || { rm -f "$2"; return 1; }; }
      dev_put() { "${KO_SSH[@]}" "cat >'$KO_BASE/$2.tmp' && mv '$KO_BASE/$2.tmp' '$KO_BASE/$2'" <"$1"; }
      dev_rm() { "${KO_SSH[@]}" "rm -f '$KO_BASE/$1'"; }
      dev_mkdir() { "${KO_SSH[@]}" "mkdir -p '$KO_BASE/$1'"; }
      dev_size() { "${KO_SSH[@]}" "wc -c <'$KO_BASE/$1'" 2>/dev/null | tr -d ' '; }
      ;;
    *) echo "✗ device.conf 的 TRANSPORT 只能是 mtp 或 ssh" >&2; return 2 ;;
  esac
}

# ko_running：输出 closed / running / unknown（Android 从电脑看不出来，要问用户）。连不上设备返回非 0。
ko_running() {
  case $RUNNING_CHECK in
    crashlog) # crash.log 里最后一次启动之后有没有"Tearing down UIManager"
      local log=$KO_TMP/crash.log start stop
      dev_has crash.log || { echo closed; return 0; } # 没有 crash.log = 从没运行过
      dev_get crash.log "$log" || { echo "✗ 读不出设备上的 crash.log" >&2; return 1; }
      start=$(grep -an "It's KOReader!" "$log" | tail -1 | cut -d: -f1 || true)
      stop=$(grep -an "Tearing down UIManager" "$log" | tail -1 | cut -d: -f1 || true)
      if [[ -n $stop && (-z $start || $stop -gt $start) ]]; then echo closed; else echo running; fi
      ;;
    proc) # 有没有进程在跑 reader.lua，且命令行或工作目录是这个 KOReader 目录（启动脚本常先 cd 再跑 ./reader.lua）
      # 模式写成 reade[r].lua：这条命令自己的命令行里是字面的 "reade[r].lua"，不会被自己匹配上
      local out rc=0
      out=$("${KO_SSH[@]}" "for p in /proc/[0-9]*; do c=\$(tr '\\0' ' ' <\$p/cmdline 2>/dev/null); case \"\$c\" in *reade[r].lua*) echo \"\$c \$(readlink \$p/cwd)\";; esac; done" 2>/dev/null) || rc=$?
      [[ $rc -eq 0 ]] || { echo "✗ 查 KOReader 进程失败（ssh 返回 $rc）" >&2; return 1; }
      if grep -qF "$KO_BASE" <<<"$out"; then echo running; else echo closed; fi
      ;;
    *) echo unknown ;;
  esac
}
