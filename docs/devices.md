# 设备与可阅读范围

## 阅读模式（profile）

优化按**阅读模式**来：一种阅读软件在一类屏幕上的样子，一个模式出一份产物。每个模式是一个 TOML 文件，文件名就是 id（`--device=` 的值）。
内置的在 `crates/profile/profiles/`，构建时整个目录嵌进程序，**增删模式只要增删文件，不用改代码**。

| 模式 | 给谁读 | 产物 | 屏幕（截图坐标） | 文字书阅读范围 | 漫画画布 | 黑白 / 彩色 |
|---|---|---|---|---|---|---|
| `kindle` | Kindle Paperwhite 12 代签名版自带阅读器 | AZW3 | 1272×1696，300ppi | 1104×1546 | 1272×1696（固定版式，整屏） | 黑白（漫画转 256 级灰度） |
| `ireader` | 掌阅 iReader Ocean 5 Pro 自带阅读器 | EPUB | 1264×1680，300ppi | 1264×1680 | 1264×1680（整屏） | 黑白（漫画转 256 级灰度） |
| `xochitl` | reMarkable Paper Pro Move 自带阅读器 | EPUB | 954×1696，264ppi | 842×1455 | 952×1457（页边距设成 1） | 彩色 |

三个模式用的是同一套优化规则（`OptimizeOpts::for_profile`），区别全在 profile 的字段里——阅读器各有各的怪癖，每个怪癖对应一个字段：

![三台阅读器的怪癖和对应的字段](img/reader-quirks.svg)

```toml
# crates/profile/profiles/ireader.toml（节选）
name = "掌阅自带阅读器（iReader Ocean 5 Pro）"
ppi = 300
color = false                  # 黑白屏：漫画转 256 级灰度
formats = ["epub"]             # 产物格式：epub 或 azw3
notes = "jump"
note_icons = "number"
comic_margin = 1
comic_page_direction = "ltr"

[screen]                       # 屏幕，按截图的像素写（width ≤ height）
width = 1264
height = 1680

[readable.epub]                # 真实可阅读范围（可选，不写就用 [screen]）；键是产物格式，kindle 写 [readable.azw3]
width = 1264
height = 1680
```

| 字段 | 意思 | 缺省 | 谁不一样、为什么 |
|---|---|---|---|
| `formats` | 产物格式，`["epub"]` 或 `["azw3"]`；`azw3` 是先按同样的规则优化出 EPUB 再转 | 必填 | `kindle` 是 AZW3：USB 传书只认它 |
| `color` | 彩色屏；黑白屏的漫画转 256 级灰度 | 必填 | `xochitl` 彩色 |
| `[screen]`、`[readable.<格式>]` | 屏幕、真实可阅读范围（见下一节） | 阅读范围不写 = 屏幕 | 三台各量各的 |
| `notes` | `"jump"`：点标号跳到章末注释；`"popup"`：标 `epub:type` 弹窗 | 必填 | 三台都是 `jump`；`popup` 留给自定义模式 |
| `note_icons` | 只有小图标的注释标号：`"keep"` 保留图标（限一个字高）、`"number"` 换成上标数字 | `keep` | 三台都是 `number`：xochitl 上只有图的链接点不了、CSS 限不住图标；Kindle、掌阅也用数字，三台都验证过 |
| `note_backlinks` | 保留原书注释里"跳回正文"的回链 | `true` | `xochitl` 是 `false`：它遇到标号和注释互相链接的一对会整对丢掉（正向也点不动）；Kindle 点注释后没有可靠的"返回"，要靠回链（2026-09-30 真机） |
| `comic_margin` | 漫画的图到画布四边的白边（像素） | 1 | `xochitl` 是 0：页边距设成 1 后 xochitl 自己留了 1px |
| `[comic_readable]` | 漫画画布；不写就用产物格式的阅读范围 | — | `kindle` 整屏 1272×1696、`xochitl` 952×1457 |
| `comic_fixed_layout` | 漫画写成固定版式（`bookconv::comicfxl`） | `false` | `kindle` 是 `true`：流式排版下 Kindle 强制留页边距（最小档左右还有 101px），固定版式才能整页铺满 |
| `comic_page_direction` | 漫画的翻页方向改成 `"ltr"` 或 `"rtl"` | 照原书 | `ireader` 是 `"ltr"`：掌阅遇到往右翻（日漫都这样写）的书会四周留边、铺不满；用户选了铺满，日漫在掌阅上也往左翻 |
| `comic_reader_margins` | 漫画在阅读器里要设成的页边距：写标记 `META-INF/eink-reader-margins` 供登记脚本用，文字页补回留白（`bookconv::comicpad`） | 不写 | `xochitl` 是 1：xochitl 的四周留白 CSS 改不动，只能靠页边距（见[使用指南](usage.md#move-上的漫画登记页边距)） |

- 这些字段都进指纹，改了哪个，书库里受影响的书都算过期、下次重建。
- 写了不认识的字段、不认识的格式、`comic_page_direction` 写了别的值，都会报错，防止拼错后被悄悄忽略。
- `[readable.<格式>]`、`[comic_readable]` 不能超过 `[screen]`。
- **屏幕和阅读范围都按截图的像素写**：Kindle 截图是 1272×1696（设备实际的显示缓冲区），标称 1264×1680，写的是前者。
- **自定义模式**：放在书库的 `profiles/` 目录（缺省 `~/.local/share/booklib/profiles/<id>.toml`），同 id 覆盖内置的。`booklib devices` 会列出来。

### 为什么这样分

- **阅读模式按三台设备的自带阅读器做**（2026-09-30 用户定，当时掌阅、Kindle 上的 KOReader 卸掉了；2026-10-02 又装回来，见下）；Move 上的 KOReader 2026-09-29 就撤了（屏幕刷新由 xochitl 那一层控制，翻页闪得厉害）。
- **Kindle 单独一个模式**：USB 传书只认 AZW3，自带阅读器留页边距、页眉页脚，漫画要写成固定版式。
- **掌阅单独一个模式**：屏幕和 Kindle 一样是 7 英寸 300ppi 黑白屏（2026-09-27 核实），但读 EPUB，整页图铺满整屏。

旧的设备 id（`kindle-pw12-sig`、`ireader-ocean5-pro`、`rmpp-move`、`rmpp-move-koreader`、`koreader`）已经不是阅读模式了；书库里它们的旧产物不再管理。
拷哪个文件夹、拷到哪，见[使用指南 · 传书到设备](usage.md#传书到设备)。

**KOReader 又装回来了**（2026-10-02，掌阅、Kindle，两台都开机独占进 KOReader）：阅读模式不变，书照样按自带阅读器生成。两台的 KOReader 都读 `ireader/` 的 EPUB
（KOReader 不认 `.azw3`），放存储根的 `books/`，进度经自建的 sync.vksight.com 按文件名同步——**所以 `ireader/` 产物的文件名要稳定**；词典用自己的 MOBI 转成 StarDict（本仓库的 `mobi-dict-to-stardict`）。
`ireader` 的阅读范围（1264×1680、整页图铺满）是在掌阅自带阅读器上量的，**KOReader 里没单独量过**，漫画离屏幕边缘是不是 1px 没验证；`kindle` 模式（AZW3）只在回到自带阅读器时用。
设备上 KOReader 的配置、插件、开机独占、掌阅安卓小应用 2026-10-03 拆到单独的仓库 koreader-setup（本机 `~/Projects/koreader-setup`）。

## 为什么要"真实可阅读范围"

![标称屏幕与真实可阅读范围](img/readable-area.svg)

`[screen]` 是设备标称的分辨率，但阅读器要留页边距、页眉页脚，真正用来显示内容的区域更小。漫画页按这个区域缩放、补白，才能正好填满，不会被阅读器再缩一次或者留出白条。

- **有实测值就用实测值**（写在 `[readable.<格式>]`），没有就退回标称屏幕。
- **阅读器里的页边距设置变了，阅读范围也会变**，要重新量。
- 程序里不写死任何屏幕数字：图片缩放、漫画补白都从 profile 读。

各模式的阅读范围从哪来：

**`kindle`：文字书 1104×1546，漫画整屏 1272×1696（固定版式）**。
- 2026-09-27 用测量书（转成 AZW3）在 Kindle 真机上截屏实测：左右页边距各 84，上下页眉页脚各 75。测量书里比页面窄的图靠左放，实际文字书里的插图不靠左（2026-10-01 真机）。
  2026-09-30 用户把页边距调到最小后截图，左右还有 101px（每次截图角落也会叫出页脚），所以流式排版下离屏幕 1px 做不到。
- 漫画写成固定版式：每页画布就是整屏 1272×1696，Kindle 按 1:1 整页显示（2026-09-30 测试书截图和页面图逐像素对齐，四边偏差 0）。

**`ireader`：1264×1680（整屏）**。2026-09-27 用测量书在掌阅真机上截屏实测：只放一张大图的页面，掌阅把图铺满整屏（连页眉页脚区域也盖住），
四边留白都是 0，所以 `comic_margin = 1` 就是离屏幕 1px。图文混排时图片受正文页边距限制，不适用。
但书声明了往右翻（`page-progression-direction="rtl"`）时，掌阅不铺满、四周留左右 92、上下 124px（2026-09-30 真机：只差这一个属性的两本测试书，
一本铺满、一本留边；固定版式、页面写法都不影响），所以 `ireader` 的漫画改成往左翻。

**`xochitl`：842×1455（文字书），漫画 952×1457**。
- xochitl 默认页边距 56：宽 = 954 − 2×56；高按固定上下留白 462.1pt 换算（2026-09-21 实测）。改了页边距要跟着改（28 档 → 898 宽）。
- 2026-09-29 读 xochitl 排出的 PDF 核实：整页图原像素放在 (56, 112)，离屏幕左右 56、上 112、下 129px；比这大的图缩到宽 842、高最多约 1457；
  `@page{margin:0}`、负外边距、去行高都不起作用，只有页边距设置能缩左右。
- **漫画按页边距 1 排**（`[comic_readable]` 952×1457）：界面上没有这一档，靠 Move 上的页边距代理设（见[用法](usage.md#move-上的漫画登记页边距)）。
  这时图框左上角在 (1, 112)、宽 952、高 1457；2026-09-29 真机排出的 PDF 里 952×1457 的页原样显示，离屏幕左右各 1px、上 112、下 127px
  （上下是 xochitl 固定留的，做不到 1px）；文字页、混排页的字离边约 58px。

漫画在三台上怎么做到离屏幕 1px，见[排版规则 · 离屏幕边缘 1px](typesetting.md#离屏幕边缘-1px三台各一种做法)。

## xochitl 怎么存 EPUB 和阅读进度（2026-09-29 真机摸底，只读）

Move 系统版本 20260827；数据目录 `/home/root/.local/share/remarkable/xochitl/`，每本书一个 UUID，一组同名文件：

| 文件 | 内容 |
|---|---|
| `<uuid>.epub` | 传上去的原书，原样保存 |
| `<uuid>.pdf` | **xochitl 按当前设置把整本书排成的 PDF**，屏幕上显示的是它（阅读位置 = 这个 PDF 的页码） |
| `<uuid>.content` | JSON：`fileType: "epub"`、排版设置（`fontName`（用户用霞鹜文楷）、`lineHeight`、`margins`、`textScale`）、`pageCount`（总页数）、`pages`（每页一个 UUID）、`redirectionPageMap` |
| `<uuid>.metadata` | JSON：`visibleName`（书名）、**`lastOpenedPage`（阅读进度，从 0 起：记 80 = 屏幕上 Page 81）**、`lastOpened`、`lastModified` |
| `<uuid>.epubindex` | 二进制（Qt 数据流：大端、字符串是 4 字节长度 + UTF-16）。头部：标识 `rM epub index`、版本 2、页面尺寸 954×1696、字号缩放、页边距、行距、字体名；之后按 EPUB 里的文件分组：**每个文件从 PDF 第几页开始、文件里各锚点（如我们加的 `#eink-…`）在第几页**——EPUB 位置 ↔ PDF 页码的对照表 |
| `<uuid>.local`、`.pagedata`、`.thumbnails/` | 本地标记、每页模板、缩略图 |
| `<uuid>.tombstone` | 删掉的书留下的标记（只有删除时间） |

- **进度写入时机**：翻页后马上写 `.metadata`（书没关着也写；翻 3 页后读到 `lastOpenedPage` 80，对应屏幕 81）。
- 改字号、字体、行距、页边距会整本重排：PDF、总页数、对照表都换新的。
- 用户没开 reMarkable 云同步。

## 阅读进度能不能在三台之间同步（2026-09-30 真机实测）

**不能原生三端同步**：Kindle 的云同步只管亚马逊推送的书（"发送到 Kindle"的个人文档），掌阅的云同步只在掌阅账号内，都没有公开接口互通。
退一步"电脑当中转、连线时对齐"也只做得到一个方向：

| 设备 | 读进度 | 写进度 |
|---|---|---|
| Kindle（未越狱） | ✓ 书旁边 `<书名>.sdr/<书名><哈希>.azw3f` 里的 `lpr`（最后读到）、`fpr`（读到最远）是十进制字符串，值是 AZW3 解压后正文的**字节偏移**（《白夜行》407967 = 第六章第 4 节开头）。AZW3 是我们写出来的，能精确换算成全书第几个字。只有打开过的书才有这个文件 | ✗ 书开着时改文件，Kindle 照旧停在自己记的位置（真正的进度存在别处），之后把它覆盖掉；删书再拷回同一个文件（`.sdr` 留着），Kindle 当新书从头开始，也不读它 |
| 掌阅（未 root） | ✗ 共享存储里没有进度文件，`iReader/backup/ireader2.db` 是加密的，ADB 没开；阅读器的数据在应用私有目录 | ✗ |
| Move（xochitl） | ✓ `.metadata` 的 `lastOpenedPage` + `.epubindex` 的对照表（见上一节） | 大概能：要像漫画页边距那样让 xochitl 自己设（没试） |

所以能做的最多是"连上电脑时，把 Kindle 的进度按全书第几个字换算后推给 Move"。要不要做由用户定。

## 在真机上测量

用"测量书"来量。书里有一张竖长和一张横宽的纯黑大图，阅读器会把它们等比缩小到放得下为止。竖长图显示出来的高度就是可用高度，横宽图的宽度就是可用宽度。

```sh
# 1. 生成测量书（Kindle 还要转成 AZW3）
cargo run --release -p bookconv --bin readable-probe -- 测量书.epub
cargo run --release -p azw3 --bin epub-to-azw3 -- 测量书.epub 测量书.azw3

# 2. 传到设备上，用要量的那个阅读软件打开，分别翻到"竖长图"和"横宽图"那两页，各截一张屏

# 3. 从截图里找最大的黑色区域，输出可以直接贴进 profile 的 TOML
cargo run --release -p bookconv --bin readable-measure -- --device=kindle 竖长.png 横宽.png
```

给了 `--device` 就按这个模式的产物格式写段名（Kindle 是 `[readable.azw3]`，其余 `[readable.epub]`）；不给写 `[readable.epub]`。
把输出贴进对应的 profile（内置的改 `crates/profile/profiles/`，自己用的放书库的 `profiles/`），并在注释里写明测量日期和条件（哪个阅读软件、页边距设置等）。

## 加一个阅读模式

1. 写 `<id>.toml`：`name`、`ppi`、`color`、`formats`（`["epub"]` 或 `["azw3"]`）、`notes`、`[screen]`。
2. 在真机上用那个阅读软件量可阅读范围，写进 `[readable.<格式>]`。量不了就先不写，按标称屏幕处理。
3. 把阅读器的特殊行为记下来（比如 xochitl 只认同文件锚点、掌阅遇到往右翻的书不铺满）。以后这类差异也要做成 profile 字段，不要在算法里按模式名写分支。
4. 真机上检查文字书和漫画的效果，再写"已验证"。

## 已知的阅读器特性

| 阅读器 | 实测行为 |
|---|---|
| xochitl（Move） | 正文链接只认同一文件内的 `#锚点`；不认行内样式；NCX 的 `dtb:uid` 和 OPF 不一致时不显示目录；页边距 1 时带 class 的 `<body>` 里图片会被吃掉约 20pt 宽（漫画的图页因此去掉 body 的类）；`padding` 一律不认，`margin` 用 pt 生效；页边距设置只对单本书；只有图、没有字的链接点了没反应，外链 CSS 的 `height:1em` 限不住图片；不支持 CSS 断字；每本书排出的 PDF 最后多一页空白；USB 网页上传约 88MB 以上的书被拒（413） |
| Kindle 自带阅读器（PW12） | USB 传书只认 AZW3，不认 EPUB；侧载书归"文档"分类时封面最稳；流式排版强制留页边距（最小档左右 101px），固定版式按画布 1:1 整页显示，日漫（rtl）从右往左翻；会把 `<head>` 里散落的文字显示在章首；点注释跳过去后没有可靠的"返回"，靠注释里的回链回来；书旁 `.sdr` 里的阅读进度只写不读；不支持 CSS 断字 |
| 掌阅自带阅读器 | 只放一张大图的页面，图片铺满整屏（`ireader` 的阅读范围就是按这个量的）；但书声明了从右往左翻（`page-progression-direction="rtl"`）时不铺满，四周留左右 92、上下 124px；不支持 CSS 断字；用中文字体显示英文时弯引号是全角宽 |
