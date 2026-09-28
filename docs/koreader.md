# KOReader 配置

两台设备（Kindle Paperwhite 12 签名版、掌阅 Ocean 5 Pro）上都装了 KOReader，用它读 EPUB。它的配置写成仓库里的文件（`koreader/`），
用 `koreader/apply.sh` 应用到 USB 连着的设备上：可以对比差异、可以重复应用、两台设备保持一致。

![KOReader 文字书 / 漫画两套方案怎么切换](img/koreader-schemes.svg)

## 目录里有什么

| 文件 | 写进设备上的 | 内容 |
|---|---|---|
| `personal/settings.reader.patch.lua` | `settings.reader.lua` | 个人设置，以掌阅为准：排版（霞鹜文楷、字号 17、页边距、行距）、页眉页脚、中文排版微调、停用的插件等 52 项 |
| `personal/gestures.patch.lua` | `settings/gestures.lua` | 个人手势：掌阅上改过的 16 处 |
| `schemes/text.settings.patch.lua` | `settings.reader.lua` | 文字书方案的功能项（见下） |
| `schemes/comic.settings.patch.lua` | `settings.reader.lua` | 漫画方案的自动切换规则 |
| `schemes/profiles.patch.lua` | `settings/profiles.lua` | 三个配置档：「漫画·首次」「漫画」「文字」 |
| `devices/<设备 id>/settings.reader.patch.lua` | `settings.reader.lua` | 随设备不同的：状态栏字体的文件路径 |
| `devices/<设备 id>/device.conf` | — | 怎么在 MTP 挂载里找到这台设备、怎么判断 KOReader 退没退出 |
| `presets.lua` | `settings.reader.lua` | 按设备当前的状态栏生成两个状态栏预设 |
| `merge.lua`、`luaser.lua` | — | 合并器：标量覆盖、表递归合并、值为 `"__DELETE__"` 的删键 |

设备 id 与书库的设备 profile 相同（`kindle-pw12-sig`、`ireader-ocean5-pro`）。

**不同步的**（留在各设备自己的配置里）：书目录、最近打开的文件、设备标识、休眠和自动关机（Kindle 专有）、Android 专用的设置
（音量键、系统字体、`cover_image_*`——它所属的插件在 Kindle 上会被 KOReader 自动停用）、更新源。

## 文字书方案

全局设置就是文字书方案，任何没有单书设置的书都用它。排版照个人设置，另加这几项（用户 2026-09-28 选定）：

| 设置 | 值 | 作用 |
|---|---|---|
| `footnote_link_in_popup` | 开 | 点注释号在底部弹窗显示注释，不跳页 |
| `link_prefer_footnote` | 开 | 中文书的注释常没有 `epub:type`，放宽"是不是注释"的判定 |
| `larger_tap_area_to_follow_links` | 开 | 上标注释号太小，放大点击范围 |
| `swipe_to_go_back` | 开 | 跟着链接跳过去之后，左→右滑回原处；没有跳转记录时就是上一页，翻页不受影响 |
| `text_lang_fallback` | `zh-CN` | 书没标语言时按中文断行（缺省 `en-US`） |
| `full_refresh_count` | 16 | 文字页每 16 页全刷一次（缺省 6）。带图片的页 KOReader 缺省就每页全刷 |

## 漫画方案

**怎么认出漫画**：优化器识别出漫画后，在 OPF 里加 `<dc:subject>漫画</dc:subject>`（`bookconv::comic_detect::COMIC_SUBJECT`）。
KOReader 把 `dc:subject` 读成书的 keywords，配置档的自动执行按"书的元数据包含 漫画"触发。所以书放在设备上哪个目录都行，
不用把漫画和小说分开放。（2026-09-28 在 Kindle 上核实：书的单书记录里 `doc_props.keywords` 就是 `dc:subject` 的内容。）

打开一本漫画时：

- **「漫画·首次」**只在这本书第一次打开时执行（配置档的"书是新的"条件）：从右往左翻页、四边页边距 0、关顶部标题栏、
  图片用「最佳」缩放、翻页模式。这些写进这本书自己的设置，之后在书里改了（比如国漫、美漫关掉「反转翻页方向」）会记住。
  漫画缺省按日漫从右往左（用户 2026-09-28 定）；KOReader 不读 OPF 的 `page-progression-direction`，只能这样设。
- **「漫画」**每次打开都执行：载入状态栏预设「漫画」，隐藏状态栏和进度条，画面用满整屏高度。

关闭漫画时执行**「文字」**：载入状态栏预设「文字」，状态栏恢复原样。

**状态栏预设**：载入预设会整张换掉状态栏设置，连字体文件路径一起（源码 `ReaderFooter:loadPreset`），而字体路径随设备不同，
所以预设不写死，由 `presets.lua` 在每次应用时按设备当前的状态栏生成：「文字」= 现在的状态栏，「漫画」= 同样的设置但隐藏。
在设备上改了状态栏，再跑一次 `apply.sh` 让预设跟上。

**已知限制**

- 已经打开过的书各自存了单书设置，「漫画·首次」不会再执行：在书的菜单里「重置设置」后重开。
- 设备上现有的漫画是 2026-09-28 之前生成的，没有漫画标签：重新生成（`booklib build`，优化规则 v24 会判为过期）并拷到设备上才会触发。
- 漫画读到一半 KOReader 崩了、没走到"关书"，状态栏会一直隐藏，打开再关闭任意一本漫画就恢复。
- 每次切换状态栏预设会弹一条"已载入预设"的小提示。

## 怎么应用

先连上设备（USB，解锁屏幕），**在设备上退出 KOReader**——它退出时会把内存里的设置写回文件，运行中改了会被盖掉。

```sh
koreader/apply.sh kindle-pw12-sig              # 只列出会改哪些键（dry run）
koreader/apply.sh kindle-pw12-sig --write      # 写入
koreader/apply.sh ireader-ocean5-pro --write --closed   # 掌阅：声明已退出 KOReader（不给 --closed 就在终端里问）
koreader/check.sh kindle-pw12-sig              # 离线检查：对空目录应用两遍，第二遍必须零改动
```

`--write` 时：

1. 判断 KOReader 退没退出。Kindle 看 `crash.log`：最后一次启动（`It's KOReader!`）之后有没有 `Tearing down UIManager`，没有就拒绝写。
   Android 上从电脑看不出来，要你确认。
2. 设备上的原配置备份到 `~/Documents/ereader/koreader-backup/<时间>/<设备 id>/`。
3. 有改动的文件先删再拷（MTP 不支持覆盖写），再**经 gio 读回**核对，不一致就用备份还原。

**回读为什么要经 gio**：gvfs 的 FUSE 路径（`/run/user/<uid>/gvfs/…`）对读过的文件有缓存，用 gio 换掉文件后，经 FUSE 读到的还是旧内容
（2026-09-28 Kindle 实测：写进去 19524 字节，经 FUSE 读回 11487 字节——旧文件的大小；经 gio 读回正确）。
书库投递（`deliver.rs`）只经 FUSE 看文件大小决定跳不跳过，缓存旧了最多多拷一次，没有正确性问题。

## 字体

配置里只写字体名（正文）或字体文件路径（状态栏），字体文件要自己放进设备的 `koreader/fonts/`：

| 字体 | 用在 | 掌阅 | Kindle |
|---|---|---|---|
| 霞鹜文楷 `LXGWWenKai-Regular.ttf`（族名 LXGW WenKai） | 正文 | 有 | 2026-09-28 从掌阅拷过去 |
| 京華老宋体 | 状态栏 | v2.0 `京華老宋体.ttf`（族名 KingHwa_OldSong） | v3.0 `京華老宋体v3.0.ttf`（族名 KingHwaOldSong） |

## 键名怎么核

网上流传的 KOReader 配置模板里有不少键在 KOReader 里并不存在，写进去会被静默忽略。本目录的每个键都按设备上 KOReader
（v2026.07.2）的 Lua 源码核过：设备上 `koreader/frontend/`、`koreader/plugins/` 就是源码，拷回电脑 grep。
`G_reader_settings:readSetting("键")` 这类调用就是全局设置的键；配置档能用的动作在 `frontend/dispatcher.lua` 的 `settingsList`。
KOReader 升级后键名可能变，改配置前重新核一遍。
