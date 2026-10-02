# 设置入口（掌阅）——摸底：能不能开 USB 调试

掌阅 Ocean 5 Pro 的设置里没有"关于本机 → 版本号"，开不了开发者选项和 USB 调试（2026-09-29 用户确认），所以停不了系统应用，
「KOReader 桌面」（`../android-home/`）独占只是换了桌面（2026-10-02 停用）。社区线索（见 `../README.md` 掌阅一节）说掌阅可能只是把入口藏了。
这个小应用直接用 Intent 打开这些页面，看页面本身还在不在：

- 常规入口：开发者选项、关于本机（找到版本号连点 7 次）、设置搜索（两种）、掌阅桌面的应用信息页（社区说右上角有个看不见的搜索按钮）、设置首页。
- 设置应用里名字带 develop、about、search、usb、adb、wireless 等的页面，每个一个按钮（只列能从外面打开的）。
- 顶上显示型号、安卓版本、构建类型、开发者选项和 USB 调试现在开没开、设置应用和桌面的包名。
- 报告 `Android/data/local.eink.settingsprobe/files/report.txt`：上面这些、设置应用的全部页面、全部已装应用（系统应用标 S；以后要停用哪个先得知道包名），
  每点一个按钮追加一行结果。USB 连电脑就能读（MTP 上看不见的话，重新插一下线）。

自己不联网、不改任何设置，打开页面以后改不改由你在系统页面里点。

## 编译、装

```sh
koreader/android-settings/build.sh     # → target/settings-probe/settings-probe.apk（编译步骤在 ../android-build.sh）
```

拷到掌阅的 `Download/`，在掌阅的文件管理器里点它安装（和 KOReader、KOReader 桌面一样），桌面上多一个「设」图标。

## 打开了开发者选项以后

1. 打开「USB 调试」（Android 11 起还有「无线调试」，USB 认不出来时试它）。
2. 电脑上 `adb devices` 看认不认得到。认得到的话，先看 `adb shell pm list packages -s`，**一次停一个**：`adb shell pm disable-user --user 0 <包名>`，
   出问题用 `adb shell pm enable <包名>` 恢复——所以 USB 调试和桌面上的设置入口都别停。
3. 停哪些、KOReader 当桌面怎么配，等摸清能停再定。

真机（2026-10-02，1.0；1.1 修「掌阅桌面的应用信息」误开 KOReader 桌面，1.2 从系统页回来时刷新状态、有变化记进报告，1.3 报告加用户限制和 getprop）：Ocean 5 Pro 是安卓 14（API 34）、`user` 版本、release-keys，设置应用就是 `com.android.settings`
（掌阅另有自己的设置 `com.szzy.ireader.settings`）。**「开发者选项」「关于本机」两个页面都能用 Intent 打开**：掌阅只藏了入口，页面还在。
当时 `development_settings_enabled` 这一项不存在（从没开过）、USB 调试关；电脑上 USB 只有 MTP 接口、`adb devices` 是空的。
点「开发者选项」不进页面，弹对话框"请先启用开发者选项"（掌阅加的拦截；系统没有 `no_debugging_features` 用户限制）。
getprop：`ro.debuggable=0`、`ro.adb.secure=1`、`init.svc.adbd=stopped`、`sys.usb.config=mtp`、引导锁着（`ro.boot.flash.locked=1`、`sys.oem_unlock_allowed=0`），
固件 `20260825-…-4.0.0.211`（国行 cn）。拿不到 root，但 USB 调试开了以后 shell 权限的 `pm disable-user` 不要 root。
**开发者模式打得开**（2026-10-02 用户实测）：用本应用进「关于本机」连点版本号，提示已启用；之后「开发者选项」才放行。
**USB 调试打得开，而且 adb shell 就是 root**（2026-10-02 真机：`id` → `uid=0(root) … context=u:r:su:s0`，虽然 `ro.debuggable=0`）。
开了 USB 调试后 USB 设备号从 `0e8d:2008` 变成 `0e8d:201d`（MTP + ADB 两个接口），电脑上按 2008 认设备的自动挂载规则不再触发。
**安全**：USB 调试开着时，任何插上线、被授权过的电脑都有 root；不用时可以关掉（`pm disable-user` 停掉的应用关了调试也保持停用）。
当时（掌阅桌面是默认桌面、刚用过掌阅阅读器）掌阅自己的进程合计约 700MB PSS（阅读器 178、掌阅系统界面 101、掌阅设置 89、应用市场 74、桌面 70、输入法 66、
词典 50、系统升级 23、智能助手 19、音乐 13、TTS 12、小i 12），总内存 4GB、可用 1.6GB。
掌阅的系统应用：桌面 `com.szzy.ireader.ink.launcher`、阅读器 `com.zhangyue.iReader.Eink`、应用市场、小i、智能助手等（包名前缀 `com.szzy.ireader`）。
