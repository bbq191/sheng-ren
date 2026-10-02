# KOReader 配置、补丁与插件

掌阅 Ocean 5 Pro、Kindle Paperwhite 12 签名版上 KOReader 的个人配置，2026-10-02 从 git 历史（`b4ef84a^`）取回。
**恢复配置，加几个补丁、一个感光插件、进度同步、词典，开机直接进 KOReader（Kindle 普通模式）**（用户定）：书库里没有 `koreader` 阅读模式，SimpleUI、漫画自动配置档、状态栏预设
（连同只为它写的补丁 `2-footer-preset-reclaim.lua`）不恢复。要找原来的样子：`git show b4ef84a^:koreader/<文件>`。

## 里面有什么

| 文件 | 写进设备的 | 内容 |
|---|---|---|
| `personal/settings.reader.patch.lua` | `settings.reader.lua` | 个人设置：排版（字号 17、页边距、两栏）、正文和界面字体霞鹜新晰黑＋（2026-10-02 从文楷换）、页眉、状态栏、中文排版微调、停用的插件、界面中文 |
| `personal/defaults.custom.patch.lua` | `defaults.custom.lua` | 高级默认值：图标缩小（`DGENERIC_ICON_SIZE` 40 → 32；菜单分类图标、标题栏、按钮、底部设置面板都按它算） |
| `personal/gestures.patch.lua` | `settings/gestures.lua` | 手势：长按四角（退出、休眠、截屏、全刷）、点四角（文件管理器、目录、开关触屏、交换翻页键）、左边缘前光、右边缘暖光 |
| `schemes/text.settings.patch.lua` | `settings.reader.lua` | 文字书：注释弹窗（小 2 号）、跳转后右滑回原处、撤掉和拆文件分页打架的样式调整、按中文断行、每 16 页全刷 |
| `schemes/kosync.patch.lua` | `settings/kosync.lua` | 进度同步：服务器 `https://sync.vksight.com`、按文件名认书、自动同步，见下 |
| `devices/<id>/` | `settings.reader.lua` | 随设备的：状态栏字体路径、阅读背景路径、起始目录 `home_dir`；`device.conf` 写 MTP 挂载名、KOReader 目录、放书的目录、要装的插件 |
| `patches/` | `patches/` | 用户补丁，见下 |
| `plugins/<名>/` | `plugins/<名>/` | 插件，按 `device.conf` 的 `PLUGINS` 装，见下 |
| `backgrounds/` | `backgrounds/` | 再生纸背景（设备层用中档 `recycled-medium.png`；`make.py` 生成） |

字体（`LXGWNeoXiHeiPlus.ttf`、`京華老宋体v3.0.ttf`）从 `~/Documents/ereader/fonts/` 拷，设备上缺才拷（换下来的文楷留在设备上不删）。

合并规则：标量覆盖、表递归、`"__DELETE__"` 删键；设备上其余设置不动。应用两遍第二遍零改动。

## 补丁（`patches/`）

| 补丁 | 来源 | 做什么 |
|---|---|---|
| `2-ui-font-size.lua` | 本仓库 | 界面缺省字号整体小 2 号（改 `Font.sizemap`；文件列表、目录、键盘不受影响） |
| `2-browser-folder-cover.lua` | [sebdelsol/KOReader.patches](https://github.com/sebdelsol/KOReader.patches)（MIT，取自 `0a2a57b`） | 封面网格里的文件夹显示封面：按当前排序取第一本的封面；文件夹里放 `.cover.jpg` 等可自定 |
| `2-screensaver-always-full-refresh.lua` | [omer-faruq/koreader-user-patches](https://github.com/omer-faruq/koreader-user-patches)（GPL-3.0，取自 `4b93300`） | 任何屏保类型进入前都全刷（次数在屏保菜单里设，缺省 1；0 = 不刷） |
| `2-recursive-file-counts.lua` | 同上 | 文件夹后的书数连子目录一起算（显示"本层/总数"） |

- 第三方补丁原样拷来、不改；许可证在 `patches/LICENSES/`。更新：重新下载同名文件替换，`apply.sh` 按内容不同就重拷。
- 不收 sebdelsol 的 `2-screensaver-cover.lua`（用户 2026-10-02 定）：它整个替换 KOReader 的 `Screensaver.show`（照 2025-07 的 KOReader 抄的，之后没更新），KOReader 改了这个函数就会丢改动或出错。
- 补丁出错时 KOReader 会跳过它照常启动；工具（扳手）→ 更多工具 → 补丁管理（Patch management）可以逐个停用。

## 插件（`plugins/`）

| 插件 | 装在 | 来源 | 做什么 |
|---|---|---|---|
| `autolight.koplugin`「自动前光」 | Kindle | 本仓库（2026-10-02） | **按光线传感器自动调前光**：亮着灯时每 60 秒和每次唤醒读一次光照（直接读传感器芯片 opt3001 的 `/sys/bus/iio/devices/iio:deviceN/in_illuminance_input`，读不到才退回 powerd 的 `alsLux`），按曲线设亮度（**两头低、中间高**，用户 2026-10-02：墨水屏靠反射环境光，越亮前光越没用；PW12 共 0–24 档：全黑 4、3 lux 6、昏暗室内 30 lux 10（最高）、150 lux 9、500 lux 5、2000 lux 以上 1，之间按对数插值；亮处只降到最低档、不关灯），差 2 档以上才改；手动调过亮度会记成偏好偏移。灯关着不动 |
| `kindleautobrightness.koplugin` | Kindle | [alexferrari88/kindle-auto-brightness-bridge](https://github.com/alexferrari88/kindle-auto-brightness-bridge)（MIT，取自 `a45b01c`） | 现在只用它的**色温同步**（KOReader 读暖光时取 Kindle 按时间表设的值）；亮度同步关了 |

- **为什么自己写自动前光**（2026-10-02 用户反馈"好像没生效"）：上面那个桥接插件只让 KOReader 读亮度时取硬件值，真正调灯的是 Kindle 自带的「自动亮度」；
  开机独占时亚马逊界面被停掉，自带的自动调光不一定还在跑（中等把握，没从电脑上验证），KOReader 唤醒时也会把亮度设回它记着的值。
  所以让 KOReader 自己调。**2026-10-02 真机诊断**：亚马逊界面在跑时 powerd 的 `alsLux` 和芯片读数同步（遮住 0～1 lux、室内 28、对着亮处 744，
  自带自动亮度在 28 lux 时设 9 档），KOReader 独占时 `alsLux` 不再刷新（一直 24），芯片读数照样实时，所以改成直接读芯片。
- **真机 ✓**（2026-10-02）：独占时读到传感器实时读数，亮度随环境变（两头低、中间高的曲线）。
- **用的时候把 Kindle 自带的「自动亮度」关掉**（普通模式下两边会一起调）。开关和读数在 工具 → 更多工具 →「自动前光」：
  菜单里显示「现在：光照 N lux（来源），亮度 M（自动值 K）」，来源是「传感器」才是实时读数，`powerd` 是退回的（独占时可能是旧值），`?` 是读不到；偏好偏移点一下清零。
- 曲线是估的（没有 Kindle 自带曲线的公开数据；只对过一个点：自带自动亮度 28 lux 时 9 档），用一阵觉得整体偏亮偏暗，手动调一次它就记住；要改曲线改 `main.lua` 的 `CURVE` 表。
- 色温同步在独占时可能同样没有效果（Kindle 的定时暖光可能也跟着界面停了）；不行的话改用 KOReader 自带的「自动色温」（autowarmth，按日出日落或固定时间），两者别同时开。
- 掌阅不装：两个插件都只认 Kindle（别的设备上自己停用）。
- 卸载（`--uninstall`）删掉内容和本仓库一致的插件文件，目录空了一并删。

## 停用的插件

关掉用不到的（2026-10-02 用户要，`personal/` 的 `plugins_disabled`）：zip 浏览、闲置调暗（和自动前光打架）、Kobo 待机、自动翻页、电池统计、书籍快捷手势、calibre、云存储、封面导出、按目录调设置、导出笔记、外接键盘、hello、HTTP 调试、日语、保持唤醒、移到归档、新闻下载、OPDS、速读辅助、配置档、二维码、阅读计时、SSH、系统状态、终端、文本编辑、wallabag；
Kindle 另停 hotkeys（没有实体键，掌阅有翻页键所以留着）。
留着的：自动休眠、自动色温、封面浏览（文件夹封面补丁靠它）、手势、进度同步、阅读统计（状态栏剩余时间靠它）、生词本、补丁管理。
插件管理里可以随时再打开。

## Kindle 开机直接进 KOReader，回原生、再回去

用户 2026-10-02 要：开机直接进 KOReader，回到原生系统后也要能一步回去。Kindle 先用了普通模式（亚马逊界面留在后台），
同一天查完社区方案后用户定**改成独占、退出即整机重启**（见下面"Kindle 独占：退出即重启"）。
掌阅的「KOReader 桌面」（`android-home/`）**已停用**：见本节最后；能不能开 USB 调试在用 `android-settings/` 摸底。

| | Kindle（`kindle-boot/`） |
|---|---|
| 开机 | 开机任务在亚马逊界面起来后停掉界面、独占打开 KOReader（`bin/run.sh`，后台跑） |
| 回原生系统 | KOReader 菜单里「退出」→ 整机重启，这次开机停在自带界面 |
| 再回 KOReader | 书库里点脚本书「KOReader（独占）」，或再重启一次 |
| 装 | `kindle-boot/deploy.sh --write` 拷文件，再在 Kindle 书库里点「KOReader 开机启动：装上」（开机任务要写进根分区，只能在 Kindle 上以 root 跑） |
| 关掉 | 书库里点「KOReader 开机启动：卸掉」 |

**Kindle 独占：退出即重启**（2026-10-02 用户定，**还没上真机**）：`koreader.sh --kual --framework_stop` 照旧停界面，`bin/run.sh` 在它的 PATH
最前面垫一个 `start`（`bin/start-shim.sh`，拷到 `/var/tmp/koreader-boot-shim/start`）。KOReader 退出（或崩溃）后 koreader.sh 执行 `start lab126_gui`
拉回界面时，垫片改成：留记号 `koreader-boot.native`、清诊断包、`sync`、`reboot`。开机任务看到记号就这次不自启（只一次）。界面一次也不重启，下面的白屏条件就没了。
别的 `start` 原样转给系统的。垫片没接管时（PATH 被改掉之类）koreader.sh 照原样拉回界面，`run.sh` 收尾，最坏就是旧的独占行为。
- 代价：每次回原生都要等整机重启；KOReader 运行时本来就不能用 USB（见下），所以拷书、跑 `apply.sh` 也要先退出（重启）。
- KOReader 菜单里的重启、关机不经过垫片，留着"运行中"标记，下次开机按逃生口停在自带界面。
- koreader.sh 里的停启框架逻辑是照上游 master 读的（`start lab126_gui` 是按 PATH 找的、PATH 不重设）；设备上的版本（v2026.07.2 nightly）上真机时核对。

**Kindle 为什么不用原来的独占**（2026-10-02 真机，看 Kindle 自动生成的诊断包里的系统日志）：
- 独占 = 进 KOReader 时停掉亚马逊界面、退出时重启它（`koreader.sh --framework_stop`）。开机后**第三次**重启界面时，解锁之后亚马逊主界面程序
  `KPPMainAppV2` 不画界面，一直白屏，约 10 分钟后被系统杀掉。前两次都正常；它的"崩溃循环检测"每次都判正常退出，不是这个原因。问题在亚马逊闭源程序里，查不下去。
- 普通模式不停、不重启界面，触发条件没了。代价：亚马逊界面在后台多占内存和电。
- 9/29 写的「KOReader（独占）」和今天出白屏的是同一个启动方式（`koreader.sh --kual --framework_stop`），当时只验证过开机进 KOReader、没在一次开机里连着进出三次，
  不是"以前好、后来坏"。独占下的解法只剩"退出即整机重启"或"解锁后补开主界面"（没把握）。用户先定保持普通模式，查完社区方案（下一条）后改选退出即重启（见上）。
- 停界面时主界面程序偶尔在关闭途中段错误，系统往 `documents/` 写崩溃诊断包（`Oct_02_10.06.28_2026.txt`、约 3MB 的 `.tgz`、`.sdr`，含设备信息和 Wi-Fi 日志，别外传）。
  `bin/run.sh` 在启动 KOReader 前、垫片在重启前各跑一次 `bin/clean-dumps.sh` 清掉（只删名字完全是「月_日_时.分.秒_年」的这几样；用户要的自动清理）。
- 临时取证脚本 `bin/exit-log.sh` 已从仓库去掉（`deploy.sh --remove` 仍会清它）。脚本书「KOReader（独占）」2026-10-02 换成调 `bin/run.sh` 重新加回来。
- **社区方案调研**（2026-10-02）：没找到能解决这个白屏的现成办法。Kindle Modding 社区（Discord，answeroverflow 存档）有人报「无界面 KOReader 退出后白屏、要强制重启」，附的也是 `KPPMainAppV2` 崩溃日志；zen-os issue #539（Kindle Basic 2024、5.19.6）退出后界面起不来；都没有找到根因。meepcat55/Kindle-KOReader-On-Boot（KUAL 开机自启）明说「不停 Kindle 界面，停了好像会失败」，跟我们的普通模式是同一个做法；MobileRead 上懂行的人反对改 upstart 做独占（KOReader 启动就崩会一直重启）。mireq/KOReader-without-framework-support 只是模拟 KAF 让电源键能用，管不到退出白屏。

**Kindle 开机任务的其他要点**：
- **KOReader 要在后台跑**（2026-10-02 真机：原来在开机任务里前台跑，独占时退出会死锁白屏）：开机任务是"亚马逊界面已启动"触发的，koreader.sh 退出时要重启界面，
  界面又要等这个任务结束，互相等。改成 `setsid` 后台跑、任务马上结束。现在退出改成重启、不再拉回界面，但垫片失效时仍会拉回，所以照旧后台跑。
  改了开机任务要在 Kindle 书库里再点一次「装上」才生效。
- **逃生口**：自启的 KOReader 没正常退出就关机（卡死后长按电源键、没电）→ 下次开机跳过自启、停在自带界面，只跳一次。
  要回原生界面又退不出来时，就在 KOReader **运行中**长按电源键重启（先点退出会把标记清掉）。

**Kindle 上 KOReader 运行时不能用 USB**（2026-10-02 实验，结论高把握）：KOReader 启动时挂起 `volumd`（负责插线的服务）。实验在 KOReader 运行中恢复它：
插线后系统进入 USB 模式、把书库分区（`/mnt/us`，一层 FUSE 文件服务）锁住，`blanket` 弹出 USB 提示窗盖住屏幕，约一分钟后 KOReader 访问自己在书库分区上的
程序文件时收到 SIGBUS 崩溃。MTP 服务 `tizen-mtp` 本身独立于亚马逊界面，但锁分区是系统设计，绕不开（除非把 KOReader 整个搬出书库分区，系统分区放不下）。
所以拷书、跑 `apply.sh` 前仍要先「退出」回到自带界面再插线。

- **真机**（2026-10-02）：普通模式开机直接进 KOReader、退出回亚马逊界面 ✓。独占＋退出即重启：**待验证**（开机进、退出重启停在自带界面、脚本书再进、连着进出三次以上）。
- 电脑上没有 gvfs-mtp 时：`KO_LOCAL_ROOT=/run/user/1000/mtp/kindle kindle-boot/deploy.sh --write`（jmtpfs 挂好的目录）。

**掌阅独占（2026-10-02 晚些时候，用户定）**：开发者模式、USB 调试都打得开，adb shell 是 root；`android-home/exclusive.sh --write`
设 KOReader 桌面为默认、停用掌阅桌面和阅读器等 13 个系统应用，真机开机直接进 KOReader ✓。常驻的系统升级、音乐停用无效，用户手工 `pm uninstall -k --user 0` 去掉；USB 调试每次开机被关。
详见 `android-home/README.md`。下面这段是同一天早些时候的情况：

**掌阅不再把 KOReader 设成桌面**（2026-10-02 用户定，后被上面一段取代）：「KOReader 桌面」1.6（`android-home/`：设成默认桌面，开机、退出都回 KOReader；
「KOReader 独占」图标带到默认桌面设置页；图标「Kr」）真机能用，但掌阅没有 USB 调试、系统应用一个也停不了，"独占"只是换了桌面，
不省内存也不省电；每次下发配置还得先把桌面改回 iReader 桌面、退出、写完再改回来。撤掉：默认桌面改回「iReader 桌面」、卸载「KOReader 桌面」，
KOReader 点它自己的图标打开。代码留在 `android-home/` 作参考（`build.sh` 照样能编）。

社区线索（2026-10-02 调研，没在真机上试过）：akhan23wgu/iReader-Zhangyue-USBDebug（Ocean 2）让掌阅桌面崩溃，在弹出的「应用信息」页右上角有个看不见的搜索按钮，搜「关于」→ 连点版本号打开开发者选项 → 开 USB 调试（作者说电脑还是认不出 adb）。Manhhao/Ocean5Pro 在 Ocean 5 Pro 上直接用 adb，还有 `adb shell stop`、往 `/product` 里 `mount --bind`（要 root 权限的 shell），但没写怎么开的调试，可能是海外版固件。能开 adb 的话，`pm disable-user --user 0 <包名>` 一般不用 root 就能停系统应用，那时独占才有意义。
**摸底工具 `android-settings/`「设置入口」**（2026-10-02，**还没在真机上试**）：直接用 Intent 打开开发者选项、关于本机、设置搜索、掌阅桌面的应用信息页，
以及设置应用里名字相关的所有页面，看掌阅是只藏了入口还是连页面都删了；设备信息、设置应用全部页面、已装应用写进 report.txt。见 `android-settings/README.md`。

## 读什么书、放哪

- **两台都读书库 `ireader/` 里的 EPUB**：Kindle 上的 KOReader 不认 `.azw3`（源码里电子书扩展名只注册了 `azw`、`mobi`），
  `kindle/` 的 AZW3 只给 Kindle 自带阅读器。两台屏幕都是 7 英寸 300ppi，文字书共用一份；漫画用自带阅读器看（在 KOReader 里铺不满）。
- **放在存储根下的 `books/`**（用户 2026-10-02 定）：Kindle `/mnt/us/books`、掌阅 `/storage/emulated/0/books`，KOReader 起始目录指向它，
  `apply.sh` 按 `device.conf` 的 `BOOKS_DIR` 建好。Kindle 自带书库只扫 `documents/`，放 `books/` 的 EPUB 不会混进去。拷书由自己来。

## 进度同步

- 服务器 `sync.vksight.com` 是自建的：程序、部署、账号管理都在 **vksight 仓库 `ops/kosync/`**（用户 2026-10-02 定：同步服务归 vksight）。这里只放设备上的设置。
- **按文件名认书**（不按内容）：书重新生成文件字节会变，按内容认会断；两台的文件名一样（都拷 `ireader/` 的同一份）才对得上。
- 每台要**在设备上登录一次**：工具 → 进度同步 → 登录（不要点注册，服务器不开放注册）。密码只在设备上输，不进仓库。
- 别的设备读得更靠后时问一下再跳，更靠前时不跳。Kindle 上 `wifi_enable_action = turn_on`，不然 KOReader 启动时会关掉自动同步。
- 进度是 KOReader 的 xpointer（指向 DOM 位置）：书按新规则重新生成后，旧进度可能落到附近。

## 词典

- 手上的《牛津高阶双解》《现代汉语词典》是 MOBI，KOReader 只认 StarDict；网上的 StarDict 版都是未经授权的转制，**用自己的 MOBI 转**（用户 2026-10-02 定，只自己用，产物不进仓库）：
  ```sh
  cargo run --release -p mobidict --bin mobi-dict-to-stardict -- ~/Documents/ereader/dict/现代汉语词典.mobi ~/Documents/ereader/dict/stardict
  ```
  转出来放在 `~/Documents/ereader/dict/stardict/<名>/`（`$KOREADER_DICTS` 可改），`apply.sh` 拷到两台的 `data/dict/`（缺的或大小不同的才拷，卸载不删）。
- 转换内容：词头索引的全部词条（含《现代汉语词典》的异体字别名，如 㕑 → 厨）；`filepos` 互查链接换成 `bword://`；图片不带。
- **没转词形变化索引**：MOBI 的变形表是按规则变换词头的，没解。这本牛津把 `ran` 这类不规则变形收成了词头；
  `books` 这类规则变形靠 KOReader 的模糊查找（查 `books` 会给出 `book`）。
- 2026-10-02 本机用 KOReader 自带的 `sdcv` 查过：厨、㕑、唱名（两条）、AA制、run、Run、because、'cause 都对。

## 怎么用

先在设备上装好 KOReader 并打开一次（要有 `settings.reader.lua`），USB 连上电脑、解锁。

```sh
koreader/apply.sh kindle-pw12-sig             # 只列出会改什么（dry run）
koreader/apply.sh kindle-pw12-sig --write     # 写入：先备份到 ~/Documents/ereader/koreader-backup/，每个文件回读核对，失败回滚
koreader/apply.sh ireader-ocean5-pro --write  # 掌阅：从电脑看不出 KOReader 是否在运行，会问一句（或加 --closed）
koreader/apply.sh <设备> --restore[=<时间>]   # 还原成某次备份
koreader/apply.sh <设备> --uninstall          # 撤掉文字书方案、设备差异和本仓库的补丁，个人设置、字体保留
koreader/check.sh                             # 离线自检，不碰设备
```

- **写之前要在 KOReader 菜单里「退出」**：它退出时把内存里的设置写回文件，运行中改的会被覆盖。掌阅上在最近任务里划掉不一定结束进程。
- 需要 `luajit`、`gio`（gvfs-mtp）。gvfs 的 MTP 只能经 `gio` 读写（它的 FUSE 路径有缓存，回读会拿到旧内容）。
- **没有 gvfs-mtp 时**（2026-10-02 起电脑上卸了）：用 `jmtpfs` 挂好的目录，`KO_LOCAL_ROOT=<挂载点> koreader/apply.sh …`，按普通文件读写。
  这台电脑插上设备会被自己配的 jmtpfs 自动挂载：Kindle `/run/user/1000/mtp/kindle`、掌阅 `/run/user/1000/mtp/ireader`，直接把 `KO_LOCAL_ROOT` 设成它。
- 词典大（牛津 .dict 约 111MB），MTP 拷要一两分钟。
