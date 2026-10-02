# KOReader 配置、补丁与插件

掌阅 Ocean 5 Pro、Kindle Paperwhite 12 签名版上 KOReader 的个人配置，2026-10-02 从 git 历史（`b4ef84a^`）取回。
**只恢复配置，加几个补丁和一个感光插件**（用户定）：书库里没有 `koreader` 阅读模式，SimpleUI、进度同步（kosync）、漫画自动配置档、状态栏预设
（连同只为它写的补丁 `2-footer-preset-reclaim.lua`）、Kindle 开机启动、掌阅「KOReader 桌面」都不恢复。要找原来的样子：`git show b4ef84a^:koreader/<文件>`。

## 里面有什么

| 文件 | 写进设备的 | 内容 |
|---|---|---|
| `personal/settings.reader.patch.lua` | `settings.reader.lua` | 个人设置：排版（字号 17、页边距、两栏）、正文字体霞鹜文楷、界面字体、页眉、状态栏、中文排版微调、停用的插件、界面中文 |
| `personal/gestures.patch.lua` | `settings/gestures.lua` | 手势：长按四角（退出、休眠、截屏、全刷）、点四角（文件管理器、目录、开关触屏、交换翻页键）、左边缘前光、右边缘暖光 |
| `schemes/text.settings.patch.lua` | `settings.reader.lua` | 文字书：注释弹窗（小 2 号）、跳转后右滑回原处、撤掉和拆文件分页打架的样式调整、按中文断行、每 16 页全刷 |
| `devices/<id>/` | `settings.reader.lua` | 随设备的：状态栏字体路径、阅读背景路径；`device.conf` 写 MTP 挂载名和 KOReader 目录 |
| `patches/` | `patches/` | 用户补丁，见下 |
| `plugins/<名>/` | `plugins/<名>/` | 插件，按 `device.conf` 的 `PLUGINS` 装，见下 |
| `backgrounds/` | `backgrounds/` | 再生纸背景（设备层用中档 `recycled-medium.png`；`make.py` 生成） |

字体（`LXGWWenKai-Medium.ttf`、`京華老宋体v3.0.ttf`）从 `~/Documents/ereader/fonts/` 拷，设备上缺才拷。

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
| `kindleautobrightness.koplugin` | Kindle | [alexferrari88/kindle-auto-brightness-bridge](https://github.com/alexferrari88/kindle-auto-brightness-bridge)（MIT，取自 `a45b01c`） | 感光调亮度：灯光仍由 Kindle 自带的「自动亮度」按光线传感器调，插件让 KOReader 读亮度时取硬件当前值，手动调亮度不会从旧值跳回来。只读不写、不常驻轮询 |

- **要先在 Kindle 下拉菜单里打开「自动亮度」再进 KOReader**（KOReader 运行中才打开的，要重启 KOReader）。插件开关在 工具 → 更多工具 →「Synchronize with Kindle Auto Brightness」，Kindle 设备层已设成开。
- 色温同步（同一插件的另一个开关）没开：和 KOReader 自带的自动色温同时开，两套日程各管各的。
- 掌阅不装：插件只认 Kindle（别的设备上自己停用）。KOReader 自带的 autofrontlight 插件已从新版里删掉（只支持过 Voyage/Oasis）。
- 卸载（`--uninstall`）删掉内容和本仓库一致的插件文件，目录空了一并删。

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
- 需要 `luajit`、`gio`（gvfs）。MTP 只能经 `gio` 读写（FUSE 路径有缓存，回读会拿到旧内容）。
- 书库的 `kindle/` 产物是 AZW3，按 Kindle 自带阅读器做的；KOReader 读 KF8 的支持不完整，没在这套产物上验证过。
