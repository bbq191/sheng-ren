# KOReader 桌面（掌阅）

> **2026-10-02 重新启用，真独占**（用户定）：掌阅能开开发者模式和 USB 调试，adb shell 是 root（见 `../android-settings/README.md`），
> `exclusive.sh --write` 把本应用设成默认桌面，停用掌阅桌面、掌阅阅读器和一批系统应用；`--undo --write` 恢复。
> 同一天早些时候曾因"停不了系统应用"停用过本应用。

## 独占（`exclusive.sh`）

- 停用（`pm disable-user`，数据保留）：A 档 应用市场、系统升级、小i、智能助手、音乐&录音、朗读服务、图库、换机助手、inkShare、产线；
  B 档 掌阅阅读器、词典&翻译；C 档 iReader 桌面。不停：掌阅系统界面（控制中心、前光）、掌阅设置、iReader 输入法。
- 执行前的状态存在 `~/.local/state/sheng-ren/ireader-exclusive/before.txt`，`--undo` 只启用本脚本停掉的。
- **真机**（2026-10-02）：按主页键进 KOReader ✓；重启后开机直接进 KOReader ✓，停用、默认桌面、开发者模式都保留，**USB 调试每次开机被关掉**
  （要用 adb 时：下拉控制中心 → 设置 → 应用 →「设置入口」→ 打开 →「开发者选项」里再开）。
- **常驻应用停不掉**：系统升级（`abupdate`）和音乐&录音是 `PERSISTENT`，系统开机照样拉起；`pm disable-user`、root 的 `pm disable`（这两个现在是 enabled=2）
  都不管用，root `kill` 后立刻被拉起。剩下的办法是 `pm uninstall -k --user 0`（系统分区里的包不动、数据保留，`cmd package install-existing` 装回），
  2026-10-02 用户自己对音乐&录音执行了：进程随即结束、不再被拉起，`pm list packages -u` 里还在、`/data/data/…` 保留 ✓（重启后待确认）。
  系统升级同日用户也执行了：卸载没结束已在跑的进程，root `kill` 后不再被拉起 ✓。两个都只在 `pm list packages -u` 里（重启后待确认）。
  `--undo` 会先 `install-existing` 装回再启用。
- **回桌面**：从屏幕底部中间往上滑（掌阅自己的手势区，2026-10-02 用户实测）。
- 下发配置：`apply.sh` 前先 `adb shell pm disable-user --user 0 local.eink.koreaderhome`、在 KOReader 里「退出」、写完 `pm enable` 再 `cmd package set-home-activity local.eink.koreaderhome/.HomeActivity`（2026-10-02 这样下发了按太阳调前光）。jmtpfs 挂载会让 adb 掉线，`adb kill-server` 后恢复。
- 掌阅桌面停了以后没有应用列表：「设置入口」等要从 设置 → 应用 → 某应用 → 打开。
- 下发 KOReader 配置不用再切桌面：`adb shell am force-stop org.koreader.launcher` 关掉 KOReader 再写（apply.sh 还没接上，待做）。

掌阅 Ocean 5 Pro 没有开发者选项、没有 USB 调试（2026-09-29 用户确认），装不了也停用不了系统应用；但「设置 → 应用 → 默认应用 → 桌面」可以换。
KOReader 安卓版自己不能当桌面（它的清单里没有 HOME 类别），所以做一个只有几十 KB 的桌面应用：

- **KOReader 就是桌面**（1.4，用户 2026-09-29：退几次都回 KOReader，不要回掌阅桌面）：开机、在 KOReader 里「退出」，都（重新）打开 KOReader。
- **回原生系统**：从屏幕顶端下拉系统控制中心 → 设置 → 默认应用 → 桌面改回「iReader 桌面」（2026-09-29 掌阅实测能用）。
- **重回独占**（1.5，用户 2026-10-02 要）：掌阅桌面上多了个图标「KOReader 独占」，点它打开系统的默认桌面设置页（没有就退到「默认应用」页、再退到设置首页），
  选「KOReader 桌面」，回桌面就进了 KOReader。已经是默认桌面时点它直接打开 KOReader。安卓不许普通应用自己改默认桌面，只能带你到那一页。
  KOReader 一启动就崩的话会被一直重开，也用这个出口。
- **下发配置（`apply.sh ireader-ocean5-pro`）前**：先按上面把默认桌面改回「iReader 桌面」，再在 KOReader 里退出（它才真正退出），
  写完再把默认桌面改回「KOReader 桌面」。KOReader 还开着的话，它退出时会把内存里的设置写回去，盖掉刚下发的。
- 版本经过：1.0 按主页键（掌阅没有主页键）；1.1 每次开机只开一次、按时钟认开机（联网校时后误判，退不出去）；1.2 按系统开机次数认；
  1.3 退出回 KOReader、连着退出两次回掌阅桌面；1.4 退几次都回 KOReader（用户要）；1.5 加「KOReader 独占」图标重回独占；1.6 两个入口都用文字「Kr」做图标（`icon/kr.svg`，编译时 `rsvg-convert` 转 PNG，用户 2026-10-02 要）。
- 找不到 KOReader（包名 `org.koreader.launcher`、`org.koreader.launcher.fdroid`，或名字带 koreader 的应用）时也打开掌阅桌面，不会卡住。
- 自己不显示任何界面，不常驻，不联网，不要任何权限。

这是安卓应用，只能用 Java 写（Rust 要配安卓 NDK，为十几行逻辑不值得）。

## 编译

```sh
koreader/android-home/build.sh     # → target/koreader-home/koreader-home.apk（编译步骤在 ../android-build.sh，和 android-settings/ 共用）
```

要 Java 17+、`rsvg-convert`（转图标）和安卓 SDK（`~/.local/share/android-sdk`，或 `$ANDROID_HOME`）：
`sdkmanager --sdk_root=… "build-tools;34.0.0" "platforms;android-34"`（命令行工具从 Google 的 `commandlinetools-linux-*_latest.zip` 解出来，要接受 Google 的 SDK 许可）。
签名密钥第一次编译时生成在 `~/.local/share/android-keystore/`（不进仓库）；升级要用同一个密钥签，丢了就得先在掌阅上卸掉旧的再装。

## 装到掌阅

1. USB 连上，把 `koreader-home.apk` 拷到掌阅的 `Download/`（2026-10-02 拷的 1.6 版叫 `koreader-home-1.6.apk`，同一个密钥签，直接覆盖安装；MTP 上删旧文件失败时换个名字拷）。
2. 掌阅的文件管理器里点它安装（掌阅允许装外来的安装包：KOReader 就是这么装的）。
3. 「设置 → 应用 → 默认应用 → 桌面」选「KOReader 桌面」。
4. 重启掌阅：开机直接进 KOReader；在 KOReader 里退出，KOReader 重新打开。

**恢复**：下拉控制中心 → 设置 →「默认应用 → 桌面」改回「iReader 桌面」→ 卸载「KOReader 桌面」。

真机：改回 iReader 桌面这个出口 2026-09-29 掌阅实测能用；1.5 的「KOReader 独占」图标（掌阅有没有默认桌面设置页）**还没在真机上试过**。掌阅是深度定制的安卓，也可能过一阵自己把默认桌面改回去。
