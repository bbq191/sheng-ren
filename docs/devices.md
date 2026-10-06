# 设备与阅读模式

## 阅读模式和它的字段

优化按**阅读模式**（profile）来：一种阅读软件在一块屏幕上的样子，一个模式出一份产物。每个模式是一个 TOML 文件，文件名就是 id（`--device=` 的值）。
内置的在 `crates/profile/profiles/`，编译时整个目录嵌进程序，**增删模式只要增删文件，不用改代码**。

| 模式 | 给谁读 | 产物 | 屏幕（截图像素） | 文字书阅读范围 | 漫画画布 | 屏幕 |
|---|---|---|---|---|---|---|
| `kindle` | Kindle Paperwhite 12 代签名版自带阅读器 | KFX（漫画固定版式） | 1272×1696，300ppi | 1104×1546 | 1272×1696（整屏，固定版式） | 黑白 |
| `ireader` | 掌阅 Ocean 5 Pro 自带阅读器 | EPUB | 1264×1680，300ppi | 1264×1680 | 1264×1680（整屏） | 黑白 |
| `xochitl` | reMarkable Paper Pro Move 自带阅读器 | EPUB | 954×1696，264ppi | 842×1455 | 952×1457（页边距 1） | 彩色 |

### 怪癖 → 字段

算法只有一套（`OptimizeOpts::for_profile`），阅读器之间的差别全写成字段，**不在代码里按设备名分支**。每个字段对应一条真机上踩到的怪癖：

![三台阅读器的怪癖和对应的字段](img/reader-quirks.svg)

| 字段（缺省） | 意思 | `kindle` | `ireader` | `xochitl` | 为什么（实测怪癖） |
|---|---|---|---|---|---|
| `formats`（必填） | 产物格式 | `["kfx"]` | `["epub"]` | `["epub"]` | Kindle 自带阅读器 USB 传书不认 EPUB；`kfx` 是先按同样规则优化出 EPUB 再转（`azw3` 写出器还在，书库不用） |
| `color`（必填） | 彩色屏 | `false` | `false` | `true` | 黑白屏的漫画转 256 级灰度 |
| `[screen]`、`[readable.<格式>]` | 屏幕、真实可阅读范围（不写 = 屏幕） | 1272×1696、1104×1546 | 整屏 | 954×1696、842×1455 | 阅读器各自留页边距、页眉页脚，见[可阅读范围](#可阅读范围) |
| `notes`（必填） | `"jump"` 点标号跳到章末；`"popup"` 标成弹窗注释 | jump | popup | jump | 掌阅自带阅读器认弹窗（2026-10-05 真机）；Kindle 的 AZW3 不认（KFX 由写出器另写弹窗，见 [KFX](kfx.md)）；xochitl 只认同文件跳转 |
| `note_icons`（`"keep"`） | 只有图标的注释标号：保留图标，或换成上标数字 | number | number | number | xochitl 里只有图的链接点不了、CSS 限不住图标；另两台也用数字，都验证过能跳 |
| `note_backlinks`（`true`） | 保留注释里"跳回正文"的回链 | 保留 | 保留 | 去掉 | Kindle 点注释后没有可靠的"返回"，要靠回链；xochitl 遇到互相链接的一对会整对丢掉（正向也点不动） |
| `comic_margin`（1） | 漫画的图到画布四边的白边（像素） | 1 | 1 | 0 | xochitl 页边距设成 1 后自己就留了 1px |
| `[comic_readable]`（= 阅读范围） | 漫画画布 | 1272×1696 | 不写 | 952×1457 | 漫画要贴屏幕边，画布和文字书的阅读范围不同 |
| `comic_fixed_layout`（`false`） | 漫画写成固定版式 | `true` | — | — | Kindle 流式排版强制留页边距（最小档左右 101px），固定版式才按画布 1:1 整页显示 |
| `background_images`（`false`） | 保留 CSS 背景图（`background` 简写拆成背景色、背景图、重复、位置等分项） | `true` | `true` | — | xochitl 不认 `no-repeat`，把背景图平铺满页盖住正文；掌阅不认 `background` 简写（2026-10-05 真机对照书），拆开写就正常 |
| `background_sizing`（`true`） | 保留背景图时也留 `background-size`、`background-attachment` | 留 | 去掉 | — | 掌阅写了尺寸会把图挤变形；Kindle 留着（和 Amazon 写法一致） |
| `comic_page_direction`（照原书） | 漫画翻页方向改成 `"ltr"`/`"rtl"` | — | `"ltr"` | — | 掌阅遇到往右翻的书，整页图四周留左右 92、上下 124px |
| `comic_reader_margins`（不写） | 漫画在阅读器里要设的页边距：写标记给登记脚本、文字页补回留白 | — | — | 1 | xochitl 四周留白 CSS 改不动，只有页边距设置管用（界面只有 28/56/112 三档） |

- 这些字段都进指纹：改了哪个，受影响的书都算过期、下次重建。
- **掌阅自带阅读器的词典**（2026-10-06 真机）：内置有道词典（存储根 `dict/` 下的 `c2170331.ydd`、`e2170331.ydd` 等），也能查在线翻译；自己的 **MOBI 词典直接拷进 `iReader/dict/`** 就能在查词里用（《牛津高阶英语双解》68MB、《现代汉语词典》9MB ✓，不用转格式；社区说法是单本不超过 100MB，没验证上限）。（`mobi-dict-to-stardict` 转 StarDict 是当初给 KOReader 用的，现在两台都不装 KOReader。）
- **掌阅自带阅读器的字体**（2026-10-06 真机）：字体都在共享存储的 `iReader/fonts/`，自带的（方正新书宋等，删了会自动重新下载）和自己导入的放在一起；**字体列表显示的就是文件名去掉扩展名**，不读字体里登记的名字，自带字体的文件名本来就是中文（`方正新书宋.TTF`）。所以导入的字体想显示中文名，就把文件改成中文名（`SourceHanSerifSC-SemiBold.otf` → `思源宋体 SemiBold.otf`）。阅读器设置（`text_book_config.db` 的 `fontFamily`）记的也是这个名字，改名后用到它的书要重新选一次字体。
- **掌阅开 adb**：设置里藏了「关于本机」，要装「设置入口」小应用（代码在 koreader-setup 仓库的 `android-settings/`，独立的安卓应用，不需要 KOReader）打开它、连点版本号开开发者选项，再开 USB 调试；adb shell 是 root；每次开机 USB 调试被关。
- 不做成字段、各模式一律处理的怪癖：掌阅自带阅读器遇到文件名里有 `*`、`:` 这类字符的封面图（《春雪》《飘》原书的 `**::…jpg`），书架没有封面缩略图（2026-10-06），清洗时一律改名（见[排版 · 目录与其它修复](typesetting.md#6-目录与其它修复)）。
- 写了不认识的字段、格式，`comic_page_direction` 写了别的值，都会报错，防止拼错后被悄悄忽略。
- 屏幕、阅读范围、漫画画布都要写成竖屏（宽 ≤ 高），阅读范围和画布不能超过屏幕，否则报错。
- **按截图的像素写**：Kindle 截图是 1272×1696（设备实际的显示缓冲区），标称 1264×1680，写的是前者。
- **自定义模式**：放在书库的 `profiles/<id>.toml`，同 id 覆盖内置的；`booklib devices` 会列出来。

```toml
# crates/profile/profiles/ireader.toml（节选）
name = "掌阅自带阅读器（iReader Ocean 5 Pro）"
ppi = 300
color = false
formats = ["epub"]
notes = "jump"
note_icons = "number"
comic_margin = 1
comic_page_direction = "ltr"

[screen]
width = 1264
height = 1680

[readable.epub]          # kindle 写 [readable.azw3]
width = 1264
height = 1680
```

**不对应字段、所有模式统一处理的怪癖**（以后某台不一样了再做成字段）：

| 阅读器 | 怪癖 | 统一的处理 |
|---|---|---|
| xochitl | 正文链接只认同一文件里的 `#锚点` | 注释搬到同一文件的章末（[排版 · 注释](typesetting.md#3-注释点标号跳到章末比正文小一号)） |
| xochitl | 不认行内样式；`padding` 不认 | 居中、居右换成类；补留白用 `margin` |
| xochitl | NCX 的 `dtb:uid` 和 OPF 不一致时不显示目录 | 对齐成一致 |
| xochitl | 背景图会平铺满页、盖住正文 | 去掉背景图 |
| xochitl | 页边距 1 时，带类的 `<body>` 里图会被吃掉约 20pt 宽 | 漫画图页去掉 body 的类 |
| Kindle | 把 `<head>` 里散落的文字显示在章首 | AZW3 的 `<head>` 只留 title、meta、link、style、base |
| 三台 | 不支持 CSS 断字 | 规则留着，不插软连字符 |

其它实测行为（不需要处理）：Kindle 侧载书归"文档"分类时封面最稳；Kindle 书旁 `.sdr` 里的进度只写不读；掌阅用中文字体显示英文时弯引号是全角宽；xochitl 每本书排出的 PDF 最后多一页空白、页边距设置只对单本书、USB 网页上传约 88MB 以上的书回 413。

### 为什么这样分

- **按设备自带的阅读器分模式**：Kindle 单独一个，因为 USB 传书不认 EPUB（认 AZW3、KFX，现在出 KFX）、强制留页边距、漫画要写成固定版式；掌阅屏幕和 Kindle 一样是 7 英寸 300ppi 黑白屏，但读 EPUB、整页图铺满整屏；Move 是彩色屏、怪癖最多。
- 旧的设备 id（`kindle-pw12-sig`、`ireader-ocean5-pro`、`rmpp-move`、`rmpp-move-koreader`、`koreader`）已经不是模式了，书库里它们的旧产物不再管理。来龙去脉见[决定记录](decisions.md)。

## 可阅读范围

![标称屏幕与真实可阅读范围](img/readable-area.svg)

**原理**：`[screen]` 是整块屏幕，但阅读器要留页边距、页眉页脚，真正显示内容的区域更小。图按这个区域缩放，才能正好放下，不会被阅读器再缩一次或留出白条。

- 有实测值就用实测值（`[readable.<格式>]`），没有就退回整屏。
- **阅读器里的页边距设置变了，阅读范围也会变**，要重新量。
- 程序里不写死屏幕数字，一律从模式读。

### 各模式的数字

**`kindle`：文字书 1104×1546，漫画整屏 1272×1696。**
- 2026-09-27 用测量书（转成 AZW3）截屏实测：左右页边距各 84，上下页眉页脚各 75。
- 页边距调到最小后左右还有 101px，所以流式排版下离屏幕 1px 做不到；漫画写成固定版式，Kindle 按画布 1:1 整页显示：测试书截图和页面图逐像素对齐，四边偏差 0（2026-09-30）。
- 测量书里比页面窄的图靠左，实际文字书里的插图不靠左（2026-10-01）。

**`ireader`：1264×1680（整屏）。**
- 2026-09-27 截屏实测：只放一张大图的页面，掌阅把图铺满整屏（连页眉页脚区域也盖住），四边留白 0，所以 1px 白边就是离屏幕 1px。图文混排时图受正文页边距限制，不适用。
- 书声明了往右翻（`page-progression-direction="rtl"`）时不铺满，四周留左右 92、上下 124px（2026-09-30：只差这一个属性的两本测试书，一本铺满、一本留边；固定版式、页面写法都不影响）。所以漫画改成往左翻，之后测试书截图离屏幕 0–1px。

**`xochitl`：文字书 842×1455，漫画 952×1457。**
- 默认页边距 56：宽 = 954 − 2×56；高按固定的上下留白换算（2026-09-21 实测）。改成 28 档 → 宽 898。
- 读 xochitl 排出的 PDF 核实（2026-09-29）：整页图原像素放在 (56, 112)，离屏幕左右 56、上 112、下 129px；`@page{margin:0}`、负外边距、去行高都不起作用，只有页边距设置能缩左右。
- 页边距设成 1 时图框左上角在 (1, 112)、宽 952、高 1457：952×1457 的页原样显示，离屏幕左右各 1px、上 112、下 127px（上下是 xochitl 固定留的）；文字页的字离边约 58px。

怎么用这些数字做到离屏幕 1px，见[排版 · 离屏幕边缘 1px](typesetting.md#离屏幕边缘-1px三种做法)。

### 在真机上测量

用"测量书"：书里有一张竖长和一张横宽的纯黑大图，阅读器会把它们等比缩小到放得下为止。竖长图显示的高度就是可用高度，横宽图的宽度就是可用宽度。

```sh
# 1. 生成测量书（Kindle 还要转成 AZW3）
readable-probe 测量书.epub
epub-to-azw3 测量书.epub 测量书.azw3

# 2. 拷到设备上，用要量的阅读软件打开，翻到"竖长图""横宽图"两页各截一张屏

# 3. 从截图里找最大的黑色区域，输出可以直接贴进 TOML
readable-measure --device=kindle 竖长.png 横宽.png
```

给了 `--device` 就按它的产物格式写段名（Kindle 是 `[readable.azw3]`，其余 `[readable.epub]`）。贴进对应的模式文件，注释里写明测量日期和条件（哪个阅读软件、页边距设置）。

## 加一个阅读模式

1. 写 `<id>.toml`：`name`、`ppi`、`color`、`formats`、`notes`、`[screen]`。
2. 在真机上用那个阅读软件量可阅读范围，写进 `[readable.<格式>]`；量不了先不写，按整屏处理。
3. 把阅读器的特殊行为记进上面的表；需要不同处理的做成字段，不在算法里按模式名写分支。
4. 真机上看过文字书和漫画，再在[验证情况](typesetting.md#验证情况)里写"✓"。

## KOReader

**现状：两台都不装 KOReader（2026-10-06 用户定）**，三台都用自带阅读器：Kindle 读 `kindle/` 的 KFX，掌阅读 `ireader/` 的 EPUB，Move 读 `xochitl/`。2026-10-02～10-05 Kindle、掌阅曾日常开机直接进 KOReader（独占）；10-05 Kindle 上 `koreader/` 不在了（像是重置过）。下面是当时的记录，留作参考。

- **读哪份产物**：KOReader 读 `ireader/` 的 EPUB（KOReader 不认 `.azw3`），拷到存储根的 `books/`（KOReader 的起始目录）。`kindle` 模式的 KFX 只在回到 Kindle 自带阅读器时用。
- **没有单独的模式**：`ireader` 的阅读范围是在掌阅自带阅读器上量的，KOReader 里没单独量，漫画离屏幕是不是 1px 没验证。
- **进度同步**：两台的 KOReader 经自建的同步服务（KOReader 的 kosync 协议）按**文件名**认书、同步进度，所以 `ireader/` 产物的文件名要稳定。服务端归 vksight 仓库，设备上的设置在 koreader-setup 仓库。
- **词典**：用自己手上的 MOBI 词典，经本仓库的 `mobi-dict-to-stardict` 转成 StarDict（网上现成的 StarDict 版是未授权转制，不用）。
- **设备上的配置**（个人设置、手势、字体、插件、开机独占、USB 传书、前光）都在单独的仓库 **koreader-setup**，本仓库不管。
- Move 上不用 KOReader：屏幕刷新由 xochitl 那一层控制，翻页闪得厉害。

自带阅读器之间不能同步进度，见附录。

## 附录：调研

### xochitl 怎么存 EPUB 和阅读进度（2026-09-29 真机摸底，只读）

Move 系统版本 20260827；数据目录 `/home/root/.local/share/remarkable/xochitl/`，每本书一个 UUID、一组同名文件：

| 文件 | 内容 |
|---|---|
| `<uuid>.epub` | 传上去的原书，原样保存 |
| `<uuid>.pdf` | **xochitl 按当前设置把整本书排成的 PDF**，屏幕上显示的是它（阅读位置 = PDF 页码） |
| `<uuid>.content` | JSON：`fileType: "epub"`、排版设置（`fontName`、`lineHeight`、`margins`、`textScale`）、`pageCount`、`pages`、`redirectionPageMap` |
| `<uuid>.metadata` | JSON：`visibleName`（书名）、**`lastOpenedPage`（阅读进度，从 0 起：记 80 = 屏幕上第 81 页）**、`lastOpened`、`lastModified` |
| `<uuid>.epubindex` | 二进制（Qt 数据流）：头部有页面尺寸、字号、页边距、行距、字体名；之后按 EPUB 里的文件分组，记每个文件从 PDF 第几页开始、文件里各锚点在第几页——EPUB 位置 ↔ PDF 页码的对照表 |
| `<uuid>.local`、`.pagedata`、`.thumbnails/` | 本地标记、每页模板、缩略图 |
| `<uuid>.tombstone` | 删掉的书留下的标记 |

- 翻页后马上写 `.metadata`（书没关也写）。
- 改字号、字体、行距、页边距会整本重排：PDF、总页数、对照表都换新的。
- 用户没开 reMarkable 云同步。

### 自带阅读器之间能不能同步进度（2026-09-30 真机）

**不能原生同步**：Kindle 的云同步只管亚马逊推送的书，掌阅的云同步只在掌阅账号内，都没有公开接口互通。退一步"电脑当中转、连线时对齐"也只做得到一个方向：

| 设备 | 读进度 | 写进度 |
|---|---|---|
| Kindle（未越狱） | ✓ 书旁 `<书名>.sdr/<书名><哈希>.azw3f` 里的 `lpr`（最后读到）、`fpr`（读到最远），值是 AZW3 解压后正文的**字节偏移**；AZW3 是我们写的，能换算成全书第几个字。只有打开过的书才有 | ✗ 改文件 Kindle 不认、之后还会覆盖掉；删书再拷回也当新书从头开始 |
| 掌阅（未 root） | ✗ 共享存储里没有进度文件，`iReader/backup/ireader2.db` 是加密的；当时 ADB 没开（后来开发者模式能打开，adb shell 是 root，没再查） | ✗ |
| Move（xochitl） | ✓ `.metadata` 的 `lastOpenedPage` + `.epubindex` 对照表 | 可能能：像漫画页边距那样让 xochitl 自己设（没试） |

所以自带阅读器之间最多做到"连上电脑时，把 Kindle 的进度换算后推给 Move"，没有做。当时日常的进度同步靠 KOReader（见上，现在不装了）。

### 重拷书以后进度还在不在（2026-10-06 真机）

| 设备 | 结果 |
|---|---|
| Kindle 自带阅读器（KFX） | **字节有任何不同就清零**（只差写进书里的版本号也一样），逐字节相同才保留。所以 KFX 里不写写出器版本，内容没变的书重建后逐字节相同，见 [kfx.md](kfx.md#阅读进度2026-10-06-真机) |
| 掌阅自带阅读器（EPUB） | **字节变了也保留**：同一本《罗杰疑案》只改 `META-INF/eink-optimized` 的版本号、重新打包覆盖，两次都还停在原来的位置。所以 EPUB 里照旧记优化器版本 |
| Move | 没测（覆盖 xochitl 的书要走它自己的上传） |
