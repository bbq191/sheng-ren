#!/usr/bin/env bash
# 编译"KOReader 桌面"安装包（掌阅用，见 README.md）。编译步骤在 ../android-build.sh（和 android-settings/ 共用）。
#
# 用法: koreader/android-home/build.sh     → target/koreader-home/koreader-home.apk
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd)
exec "$here/../android-build.sh" "$here" koreader-home 7 1.6
