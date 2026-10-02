#!/usr/bin/env bash
# 编译掌阅用的小安卓应用（android-home/、android-settings/ 共用）。不用 Gradle，直接调安卓 SDK 的工具。
#
# 用法: koreader/android-build.sh <应用目录> <安装包名> <versionCode> <versionName>   → target/<安装包名>/<安装包名>.apk
#   应用目录里要有 AndroidManifest.xml、src/（Java）、icon/*.svg（第一个 SVG 当图标，清单里引用 @drawable/ic_launcher）。
# 要：Java 17+、rsvg-convert、安卓 SDK（缺省 ~/.local/share/android-sdk，或 $ANDROID_HOME）里的 build-tools;34.0.0 和 platforms;android-34。
# 签名密钥第一次编译时生成，放在 ~/.local/share/android-keystore/koreader-home.jks（不进仓库，几个应用共用）。以后升级要用同一个密钥签，
# 否则装不上去（得先卸掉旧的）。
set -euo pipefail
[[ $# -eq 4 ]] || { sed -n '4,5p' "$0" | sed 's/^# \{0,1\}//' >&2; exit 2; }
app=$(cd "$1" && pwd) name=$2 vcode=$3 vname=$4
repo=$(cd "$(dirname "$0")/.." && pwd)
SDK=${ANDROID_HOME:-$HOME/.local/share/android-sdk}
BT=$SDK/build-tools/34.0.0
JAR=$SDK/platforms/android-34/android.jar
[[ -x $BT/aapt2 && -f $JAR ]] || { echo "✗ 缺安卓 SDK：$SDK 里要有 build-tools;34.0.0 和 platforms;android-34（见 android-home/README.md）" >&2; exit 1; }
KS_DIR=$HOME/.local/share/android-keystore
KS=$KS_DIR/koreader-home.jks
out=$repo/target/$name
rm -rf "$out" && mkdir -p "$out/classes" "$out/dex"

if [[ ! -f $KS ]]; then
  mkdir -p "$KS_DIR" && chmod 700 "$KS_DIR"
  head -c 24 /dev/urandom | base64 | tr -d '/+=' >"$KS_DIR/koreader-home.pass"
  chmod 600 "$KS_DIR/koreader-home.pass"
  keytool -genkeypair -keystore "$KS" -storepass:file "$KS_DIR/koreader-home.pass" -alias koreader-home \
    -keyalg RSA -keysize 3072 -validity 36500 -dname "CN=KOReader Home" >/dev/null 2>&1
  chmod 600 "$KS"
  echo "新生成了签名密钥 $KS（丢了以后升级要先卸载旧版）"
fi

# 图标：icon/*.svg → PNG → aapt2 编译成资源（drawable-nodpi，各种屏幕密度都用这一张，系统自己缩放）
command -v rsvg-convert >/dev/null || { echo "✗ 缺 rsvg-convert（librsvg）：图标要用它把 SVG 转成 PNG" >&2; exit 1; }
icon=$(find "$app/icon" -name '*.svg' | sort | head -1)
[[ -n $icon ]] || { echo "✗ $app/icon/ 里没有 SVG 图标" >&2; exit 1; }
mkdir -p "$out/res/drawable-nodpi"
rsvg-convert -w 192 -h 192 "$icon" -o "$out/res/drawable-nodpi/ic_launcher.png"
"$BT/aapt2" compile -o "$out/res.zip" --dir "$out/res"
"$BT/aapt2" link --manifest "$app/AndroidManifest.xml" -I "$JAR" -o "$out/base.apk" "$out/res.zip" \
  --min-sdk-version 24 --target-sdk-version 34 --version-code "$vcode" --version-name "$vname"
mapfile -t srcs < <(find "$app/src" -name '*.java')
javac -nowarn -Xlint:-options -source 11 -target 11 -classpath "$JAR" -d "$out/classes" "${srcs[@]}"
mapfile -t classes < <(find "$out/classes" -name '*.class')
"$BT/d8" --lib "$JAR" --min-api 24 --output "$out/dex" "${classes[@]}"
cp "$out/base.apk" "$out/unsigned.apk"
(cd "$out/dex" && zip -qj "$out/unsigned.apk" classes.dex)
"$BT/zipalign" -f 4 "$out/unsigned.apk" "$out/aligned.apk"
"$BT/apksigner" sign --ks "$KS" --ks-pass "file:$KS_DIR/koreader-home.pass" --out "$out/$name.apk" "$out/aligned.apk"
"$BT/apksigner" verify "$out/$name.apk"
echo "✓ $out/$name.apk（$(stat -c %s "$out/$name.apk") 字节）"
