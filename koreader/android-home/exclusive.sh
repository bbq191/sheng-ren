#!/usr/bin/env bash
# 掌阅 KOReader 独占（用户 2026-10-02 定）：经 adb 把「KOReader 桌面」设成默认桌面，停用掌阅桌面、掌阅阅读器和一批用不到的系统应用。
# 前提：掌阅开了开发者模式和 USB 调试（见 ../android-settings/README.md），USB 连上、电脑已授权。掌阅的 adb shell 是 root。
#
# 用法: koreader/android-home/exclusive.sh            只列出要做什么
#       koreader/android-home/exclusive.sh --write    执行（先把现在的状态存到 ~/.local/state/sheng-ren/ireader-exclusive/）
#       koreader/android-home/exclusive.sh --undo --write   恢复：启用停掉的应用，默认桌面改回 iReader 桌面
# 停用用的是 pm disable-user：数据都留着，pm enable 就回来；关掉 USB 调试也保持停用。
set -euo pipefail

HOME_ACT=local.eink.koreaderhome/.HomeActivity
IREADER_HOME=com.szzy.ireader.ink.launcher
# A 档：跟读书无关
A=(com.szzy.ireader.ink.appmarket   # 应用市场
   com.szzy.ireader.ink.abupdate    # 系统升级（停掉也防止新固件把 root 的 adb 堵上；代价是收不到修复）
   com.szzy.ireader.ink.ai          # 小i
   com.szzy.ireader.smartassistant  # 智能助手
   com.szzy.ireader.ink.musicplayer # 音乐&录音
   com.szzy.ireader.ink.tts         # 朗读服务
   com.szzy.ireader.gallery         # 图库
   com.szzy.ireader.ink.wifidirect  # 换机助手
   com.szzy.ireader.ink.share       # inkShare
   com.szzy.ireader.ink.product)    # 产线（工厂测试）
# B 档：掌阅自带阅读器和词典（用 KOReader 读书）
B=(com.zhangyue.iReader.Eink com.szzy.ireader.ink.translatelexicon)
# C 档：掌阅桌面（先把 KOReader 桌面设成默认）
C=("$IREADER_HOME")
# 不停：掌阅系统界面（控制中心、前光）、掌阅设置、iReader 输入法、安卓自己的组件。
# 注意：系统升级、音乐&录音是常驻（PERSISTENT）应用，停用了系统开机照样拉起（2026-10-02 真机），见 README.md。
PKGS=("${A[@]}" "${B[@]}" "${C[@]}")
STATE=${XDG_STATE_HOME:-$HOME/.local/state}/sheng-ren/ireader-exclusive

write=0 undo=0
for a in "$@"; do
  case $a in
    --write) write=1 ;;
    --undo) undo=1 ;;
    -h | --help) sed -n '2,9p' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
    *) echo "不认识的参数 $a" >&2; exit 2 ;;
  esac
done

ash() { adb shell "$@" | tr -d '\r'; }
[[ $(adb get-state 2>/dev/null) == device ]] || { echo "✗ adb 没连上掌阅（USB 调试开了、线插着、电脑授权了吗）" >&2; exit 1; }
[[ $(ash getprop ro.product.model) == "Ocean 5 Pro" ]] || { echo "✗ 连着的不是掌阅 Ocean 5 Pro" >&2; exit 1; }
disabled=$(ash pm list packages -d | sed 's/^package://')
is_disabled() { grep -qx "$1" <<<"$disabled"; }
installed() { ash pm path "$1" >/dev/null 2>&1; }
run() { echo "  $*"; [[ $write -eq 0 ]] || ash "$@" >/dev/null; }

if [[ $undo -eq 1 ]]; then
  echo "恢复："
  for p in "${PKGS[@]}"; do
    # 本脚本停之前就是停用的（记在 before.txt 里），不去启用
    if [[ -f $STATE/before.txt ]] && grep -qx "disabled $p" "$STATE/before.txt"; then continue; fi
    # 常驻应用是用户手工 pm uninstall -k --user 0 去掉的（见 README.md）：先装回来（系统分区里的包、数据都还在）
    installed "$p" || run cmd package install-existing "$p"
    is_disabled "$p" && run pm enable "$p"
  done
  run cmd package set-home-activity "$IREADER_HOME"
else
  installed local.eink.koreaderhome || { echo "✗ 掌阅上没装「KOReader 桌面」（android-home/build.sh 编译后装上）" >&2; exit 1; }
  installed org.koreader.launcher || { echo "✗ 掌阅上没装 KOReader" >&2; exit 1; }
  if [[ $write -eq 1 && ! -f $STATE/before.txt ]]; then
    mkdir -p "$STATE"
    for p in "${PKGS[@]}"; do echo "$(is_disabled "$p" && echo disabled || echo enabled) $p"; done >"$STATE/before.txt"
    echo "原来的状态存在 $STATE/before.txt"
  fi
  echo "独占："
  run cmd package set-home-activity "$HOME_ACT"
  for p in "${PKGS[@]}"; do
    if ! installed "$p"; then echo "  = $p 没装，跳过"; continue; fi
    is_disabled "$p" && { echo "  = $p 已经停用"; continue; }
    run pm disable-user --user 0 "$p"
  done
fi

if [[ $write -eq 1 ]]; then
  home=$(ash cmd package resolve-activity -c android.intent.category.HOME -a android.intent.action.MAIN | awk -F= '/packageName=/ {print $2; exit}')
  echo "✓ 完成。默认桌面：$home；停用的：$(ash pm list packages -d | sed 's/^package://' | tr '\n' ' ')"
else
  echo "（dry run：加 --write 才真的做）"
fi
