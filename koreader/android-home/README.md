# KOReader 桌面（掌阅）

掌阅 Ocean 5 Pro 没有开发者选项、没有 USB 调试（2026-09-29 用户确认），装不了也停用不了系统应用；但「设置 → 应用 → 默认应用 → 桌面」可以换。
KOReader 安卓版自己不能当桌面（它的清单里没有 HOME 类别），所以做一个只有几十 KB 的桌面应用：

- **KOReader 就是桌面**（1.3，用户 2026-09-29）：开机、在 KOReader 里「退出」，都（重新）打开 KOReader。
- **出口：连着退出两次**——被这里打开的 KOReader 10 秒内又退出 → 进掌阅原来的桌面。下发配置（`apply.sh`）前要这样真正退出 KOReader，
  不然它还在运行，退出时会把设置写回去。KOReader 一启动就崩也会在第二次落进这个出口，不会死循环。
- 另一个出口：从屏幕顶端下拉系统控制中心 → 设置 → 默认应用 → 桌面改回「iReader 桌面」（2026-09-29 掌阅实测能用）。
- 版本经过：1.0 按主页键（掌阅没有主页键）；1.1 每次开机只开一次、按时钟认开机（联网校时后误判，退不出去）；1.2 按系统开机次数认（能用，
  但用户想要退出也回 KOReader）；1.3 现在的做法。
- 找不到 KOReader（包名 `org.koreader.launcher`、`org.koreader.launcher.fdroid`，或名字带 koreader 的应用）时也打开掌阅桌面，不会卡住。
- 自己不显示任何界面，不常驻，不联网，不要任何权限。

这是安卓应用，只能用 Java 写（Rust 要配安卓 NDK，为十几行逻辑不值得）。

## 编译

```sh
koreader/android-home/build.sh     # → target/android-home/koreader-home.apk
```

要 Java 17+ 和安卓 SDK（`~/.local/share/android-sdk`，或 `$ANDROID_HOME`）：
`sdkmanager --sdk_root=… "build-tools;34.0.0" "platforms;android-34"`（命令行工具从 Google 的 `commandlinetools-linux-*_latest.zip` 解出来，要接受 Google 的 SDK 许可）。
签名密钥第一次编译时生成在 `~/.local/share/android-keystore/`（不进仓库）；升级要用同一个密钥签，丢了就得先在掌阅上卸掉旧的再装。

## 装到掌阅

1. USB 连上，把 `koreader-home.apk` 拷到掌阅的 `Download/`（2026-09-29 拷的 1.1 版叫 `koreader-home-1.1.apk`；MTP 上删旧文件失败时换个名字拷）。
2. 掌阅的文件管理器里点它安装（掌阅允许装外来的安装包：KOReader 就是这么装的）。
3. 「设置 → 应用 → 默认应用 → 桌面」选「KOReader 桌面」。
4. 重启掌阅：开机直接进 KOReader；退出一次，KOReader 重新打开；打开后 10 秒内再退出，进掌阅桌面。

**恢复**：在 KOReader 里退出回到掌阅桌面 →「默认应用 → 桌面」改回「iReader 桌面」→ 卸载「KOReader 桌面」。

**没在真机上试过**（2026-09-29）：掌阅是深度定制的安卓，可能不认第三方桌面、或者过一阵自己改回去。
