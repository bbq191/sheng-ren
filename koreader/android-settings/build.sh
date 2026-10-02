#!/usr/bin/env bash
# 编译"设置入口"安装包（掌阅用，见 README.md）。编译步骤在 ../android-build.sh。
#
# 用法: koreader/android-settings/build.sh     → target/settings-probe/settings-probe.apk
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd)
exec "$here/../android-build.sh" "$here" settings-probe 5 1.4
