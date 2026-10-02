# shellcheck shell=bash disable=SC2034,SC2329
# lib.sh —— apply.sh、check.sh 共用（source 进来用，不单独运行）。调用方先设好 KO_HERE（koreader/ 目录）。

# 设备上归我们管的配置文件（相对 KOReader 目录）
KO_FILES=(settings.reader.lua settings/gestures.lua settings/kosync.lua defaults.custom.lua)

# ko_layers <设备 id>：按应用顺序输出每一层「目标文件 脚本 补丁 类别」。
#   类别 personal = 个人设置（卸载时保留），scheme = 文字书方案和设备差异（卸载时撤销）。
ko_layers() {
  cat <<EOF
settings.reader.lua merge.lua personal/settings.reader.patch.lua personal
settings.reader.lua merge.lua schemes/text.settings.patch.lua scheme
settings.reader.lua merge.lua devices/$1/settings.reader.patch.lua scheme
settings/gestures.lua merge.lua personal/gestures.patch.lua personal
defaults.custom.lua merge.lua personal/defaults.custom.patch.lua personal
settings/kosync.lua merge.lua schemes/kosync.patch.lua scheme
EOF
}

# ko_merge_all <目录> <设备 id>：把各层依次合并进 <目录> 下的配置文件（文件不存在就从空表开始）。
ko_merge_all() {
  local dir=$1 target script patch kind rc
  mkdir -p "$dir/settings"
  while read -r target script patch kind; do
    rc=0
    mkdir -p "$(dirname "$dir/$target")"
    if [[ $patch == - ]]; then luajit "$KO_HERE/$script" "$dir/$target" >/dev/null || rc=$?
    else luajit "$KO_HERE/$script" "$dir/$target" "$KO_HERE/$patch" >/dev/null || rc=$?; fi
    [[ $rc -eq 0 || $rc -eq 10 ]] || { echo "✗ 合并出错（$script $patch）" >&2; return 3; }
  done < <(ko_layers "$2")
}

# ko_unmerge_all <目录> <备份根目录> <设备 id>：撤销方案、设备层（见 unmerge.lua），个人设置保留。
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
# ko_connect <设备 id>：读 devices/<id>/device.conf，连上设备，定义下面几个原语：
#   dev_has <路径>        存在返回 0
#   dev_get <路径> <本地> 读到本地文件；失败返回非 0 且不留半个文件
#   dev_put <本地> <路径> 写入（原子：写完才替换）
#   dev_rm <路径>         删除（不存在也算成功）
#   dev_mkdir <路径>      建目录（已存在也算成功）
#   dev_size <路径>       输出字节数
#   mnt_has / mnt_mkdir <路径>  同上，但相对设备存储的挂载点（放书的目录 BOOKS_DIR 用）
# MTP：一律经 gio 读写。gvfs 的 FUSE 路径（/run/user/…/gvfs/…）对读过的文件有缓存，gio 换掉文件后 FUSE 还会返回旧内容
# （2026-09-28 Kindle 实测：写入 19524 字节，经 FUSE 读回的是旧的 11487 字节），所以内容一律不经 FUSE 读。
# MTP 不支持覆盖写和改名：替换已有文件时先拷一份 .tmp，再删旧的、拷正式名——中途失败时设备上至少留着 .tmp 或旧文件。
ko_connect() {
  local dev=$1
  TRANSPORT=mtp
  FONTS=()
  PLUGINS=()
  BOOKS_DIR=
  RUNNING_CHECK=ask
  # shellcheck source=/dev/null
  source "$KO_HERE/devices/$dev/device.conf"
  # 备用：KO_LOCAL_ROOT 指向已经挂好的设备存储（如 jmtpfs 挂载点，下面有「Internal Storage」之类的目录），按普通文件读写。
  # 电脑上没有 gvfs-mtp 时用（2026-10-02 起）：jmtpfs <挂载点> && KO_LOCAL_ROOT=<挂载点> koreader/apply.sh …
  if [[ -n ${KO_LOCAL_ROOT:-} ]]; then
    [[ -d $KO_LOCAL_ROOT/$KOREADER_DIR ]] || { echo "✗ $KO_LOCAL_ROOT/$KOREADER_DIR 不存在（挂载点不对，或不是 $dev）" >&2; return 1; }
    KO_MOUNT=$KO_LOCAL_ROOT
    KO_BASE="$KO_LOCAL_ROOT/$KOREADER_DIR"
    dev_has() { [[ -e $KO_BASE/$1 ]]; }
    dev_get() { rm -f "$2"; cp "$KO_BASE/$1" "$2" 2>/dev/null || { rm -f "$2"; return 1; }; }
    dev_rm() { rm -f "$KO_BASE/$1" 2>/dev/null; ! [[ -e $KO_BASE/$1 ]]; }
    # MTP 挂载上改名不可靠：和 gio 那套一样先拷 .tmp、再删旧的、拷正式名
    dev_put() {
      if ! dev_has "$2"; then cp "$1" "$KO_BASE/$2"; return; fi
      dev_rm "$2.tmp" && cp "$1" "$KO_BASE/$2.tmp" || return 1
      dev_rm "$2" && cp "$1" "$KO_BASE/$2" && dev_rm "$2.tmp"
    }
    dev_mkdir() { dev_has "$1" || mkdir "$KO_BASE/$1"; }
    dev_size() { stat -c %s "$KO_BASE/$1" 2>/dev/null; }
    mnt_has() { [[ -e $KO_MOUNT/$1 ]]; }
    mnt_mkdir() { mkdir -p "$KO_MOUNT/$1"; }
    return 0
  fi
  case $TRANSPORT in
    mtp)
      command -v gio >/dev/null || { echo "✗ 缺 gio（gvfs）" >&2; return 1; }
      local uri
      uri=$(gio mount -li 2>/dev/null | grep -o "activation_root=mtp://${MTP_HOST_PREFIX}[^ ]*" | head -1 | cut -d= -f2 || true)
      [[ -z $uri ]] && { echo "✗ 没找到 $dev：USB 连上并解锁设备后再试" >&2; return 1; }
      gio info "$uri" >/dev/null 2>&1 || gio mount "$uri" 2>/dev/null || true
      KO_MOUNT="${uri%/}"
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
      mnt_has() { gio info "$KO_MOUNT/$1" >/dev/null 2>&1; }
      mnt_mkdir() { gio mkdir -p "$KO_MOUNT/$1"; }
      ;;
    *) echo "✗ device.conf 的 TRANSPORT 只能是 mtp" >&2; return 2 ;;
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
    *) echo unknown ;;
  esac
}
