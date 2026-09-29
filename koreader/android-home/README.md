# KOReader 桌面（掌阅）

掌阅 Ocean 5 Pro 没有开发者选项、没有 USB 调试（2026-09-29 用户确认），装不了也停用不了系统应用；但「设置 → 应用 → 默认应用 → 桌面」可以换。
KOReader 安卓版自己不能当桌面（它的清单里没有 HOME 类别），所以做一个只有几十 KB 的桌面应用：

- **每次开机自动打开一次 KOReader**；KOReader 的启动页是 SimpleUI 主页。
- **之后再"回桌面"**（在 KOReader 里「退出」、系统的回桌面手势）→ 打开掌阅原来的桌面。掌阅没有主页键（2026-09-29），
  最早的设计（按主页键开 KOReader、连按两次回掌阅桌面）会让"退出 KOReader → 回桌面 → 又打开 KOReader"退不出去，所以改成和 Kindle 一样"每次开机只自启一次"。
  这次开机打开过没有按系统的开机次数（`Settings.Global.BOOT_COUNT`，安卓 7 起）记在自己的设置里；另有保险：开机 3 分钟后一律进掌阅桌面。
  1.1 版按"当前时间 − 开机后经过的时间"认开机，开机后联网校时一跳就当成新的一次开机，退出 KOReader 又被打开、退不出去（2026-09-29 掌阅实测），1.2 改掉。
- **被关在 KOReader 里时的出口**：从屏幕顶端往下滑拉出系统控制中心 → 设置 → 默认应用 → 桌面改回「iReader 桌面」。
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
4. 重启掌阅：开机应该直接进 KOReader；在 KOReader 里「退出」应该回到掌阅桌面。

**恢复**：在 KOReader 里退出回到掌阅桌面 →「默认应用 → 桌面」改回「iReader 桌面」→ 卸载「KOReader 桌面」。

**没在真机上试过**（2026-09-29）：掌阅是深度定制的安卓，可能不认第三方桌面、或者过一阵自己改回去。
