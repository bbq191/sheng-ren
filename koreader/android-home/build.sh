#!/usr/bin/env bash
# 编译"KOReader 桌面"安装包（掌阅用，见 README.md）。不用 Gradle，直接调安卓 SDK 的工具。
#
# 用法: koreader/android-home/build.sh     → target/android-home/koreader-home.apk
# 要：Java 17+、安卓 SDK（缺省 ~/.local/share/android-sdk，或 $ANDROID_HOME）里的 build-tools;34.0.0 和 platforms;android-34。
# 签名密钥第一次编译时生成，放在 ~/.local/share/android-keystore/koreader-home.jks（不进仓库）。以后升级要用同一个密钥签，
# 否则装不上去（得先卸掉旧的）。
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd)
repo=$(cd "$here/../.." && pwd)
SDK=${ANDROID_HOME:-$HOME/.local/share/android-sdk}
BT=$SDK/build-tools/34.0.0
JAR=$SDK/platforms/android-34/android.jar
[[ -x $BT/aapt2 && -f $JAR ]] || { echo "✗ 缺安卓 SDK：$SDK 里要有 build-tools;34.0.0 和 platforms;android-34（见 README.md）" >&2; exit 1; }
KS_DIR=$HOME/.local/share/android-keystore
KS=$KS_DIR/koreader-home.jks
out=$repo/target/android-home
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

"$BT/aapt2" link --manifest "$here/AndroidManifest.xml" -I "$JAR" -o "$out/base.apk" \
  --min-sdk-version 21 --target-sdk-version 34 --version-code 1 --version-name 1.0
mapfile -t srcs < <(find "$here/src" -name '*.java')
javac -nowarn -Xlint:-options -source 11 -target 11 -classpath "$JAR" -d "$out/classes" "${srcs[@]}"
mapfile -t classes < <(find "$out/classes" -name '*.class')
"$BT/d8" --lib "$JAR" --min-api 21 --output "$out/dex" "${classes[@]}"
cp "$out/base.apk" "$out/unsigned.apk"
(cd "$out/dex" && zip -qj "$out/unsigned.apk" classes.dex)
"$BT/zipalign" -f 4 "$out/unsigned.apk" "$out/aligned.apk"
"$BT/apksigner" sign --ks "$KS" --ks-pass "file:$KS_DIR/koreader-home.pass" --out "$out/koreader-home.apk" "$out/aligned.apk"
"$BT/apksigner" verify "$out/koreader-home.apk"
echo "✓ $out/koreader-home.apk（$(stat -c %s "$out/koreader-home.apk") 字节）"
