# KEPUB 写出器（2026-11-05，Clara BW 真机确认方案 D）

Kobo 自带阅读器（Nickel）看到 `.kepub.epub` 扩展名就用 Kobo WebKit 引擎打开（翻页快、有阅读统计）。固定版式
（漫画）看 OPF 的 `rendition:layout = pre-paginated`（优化器 `comicfxl` 写的），**不再靠 `.fxl.kepub.epub` 文件名区分**
（2026-11-05 真机确认，见下面「真机结论」）。
模式 `kobo` 的产物是 KEPUB：书库先跑同一个优化器出 EPUB，再交给 `bookconv::kepub::epub_to_kepub`（`crates/bookconv/src/kepub.rs`）。

## clean-room

只照公开资料写：Kobo 官方 epub-spec 的 README、kepubify 在 pkg.go.dev 上的包文档（MIT，只看文档不看源码）、epub.studio 的格式说明、
公开博客里 Kobo 官方转换样本的对比。不读 Calibre（Kobo 驱动、插件）等 GPL 代码，也不读 kepubify 源码。

## 做什么

只在原文上**插入**标签，原文一个字节不改（剥掉插入的标签和输入逐字节相等，单元测试和样书检查都守着）：

- 章节 `<body>` 内包 `<div id="book-columns"><div id="book-inner">…</div></div>`（body 自己的属性不动）；
- 文字按句包 `<span class="koboSpan" id="kobo.段.句">`，每个文件从 1 数。块元素（`p`、`div`、`h1`… 和 `caption`、`dt`、`dd`、`address`）开或闭、
  图片之后换新段；`br` 不换段。句末：`。！？!?…`，ASCII `.` 要后面是空白、收尾引号或结尾；连着的句末标点并成一句，
  后面的收尾引号括号（`」』”’）》】"'`）和空白归前一句。只有空白的文字不包。字符引用（`&#8230;`、`&#160;`）解码后判断，但绝不切开；
- 图片自己一段：`<span class="koboSpan" id="kobo.段.1"><img …/></span>`（`<img></img>` 连闭合一起包）。**例外**：优化器
  产出的漫画固定版式（pre-paginated + `book-type=comic`）的纯图页，`<img>` 外面**不加** koboSpan，包装照加。2026-11-05
  Clara BW 真机 D 满屏；A 同时改了像素画布、纯图页也包 koboSpan、残留 FXL/错 viewport 等多处，B/C/E 各测不同变量，
  **不能归因到单一差异**。出版社原生的固定版式非漫画书不在例外里，行为与改前一致；
- 不包：`script style svg math head title rt rp textarea audio video canvas noscript template iframe object` 里的文字；`pre` 不切句，每段文字包一次；
- 不改：OPF、CSS、图片（压缩数据原样拷）、导航文档（nav、ncx、目录页）、已经是 KEPUB 的章节；`mimetype` 第一个、不压缩；
- **不加** Kobo 的样式补丁（会改书的版式，维护者定）。书里 CSS 有裸 `div`/`span` 或 `body >` 选择器时只报警告（可能匹配到包装和 koboSpan）。
- **竖排**（2026-11-11）：优化器产出的竖排书（`eink-vertical.css`、全角文字）照旧只插 koboSpan / 包装，不拆全角串、不进 `rt`/`rp`，也不碰 `<link>`；`eink-vertical.css` 原样拷。没上真机，vinci 同步后在 Clara BW 上看竖排翻页和弹窗注释。

产物再过一次质量门。书库指纹里格式段是 `kepub<VERSION>`（`bookconv::kepub::VERSION`，改写法就加一）。Kobo 没有 `.sdr`，换目录不搬。

## 真机结论（2026-11-05，Clara BW fw 4.45）

维护者用合成的 A/B 包（`/workspace/kobo-ab/`，包不在仓库）在真机上比：

- **A（旧流水线：图片写像素画布尺寸、纯图页也包 koboSpan、残留 FXL/错 viewport 等多处）坏**（整页图不满屏）；
- B、C、E 正常；**只有 D 满屏**，选 D。A 同时改了好几个变量，B/C/E 各测哪一项没有完整记录，所以**不能把 A 的不满屏
  归因到单一差异**（不是“只因像素”，也不是“只因 koboSpan”）；D 是整套结构一起换。

D 的结构（真机满屏那本）：

1. OPF：`rendition:layout = pre-paginated`、`original-resolution = 1072x1448`，以及现有的 `fixed-layout` / `book-type` /
   `orientation-lock` / `zero-gutter` / `zero-margin`；
2. 每页 viewport 与画布一致：`width=1072, height=1448`；
3. 保留 `#book-columns` > `#book-inner` 包装；
4. **纯图页**的 `<img>` 外面不加 `koboSpan`；文字页照旧按句加；
5. 比例和画布一致的 img 写 `width:100%;height:100%`；比例不符（装饰小图、处理不了的原图）只写
   `max-width:100%;max-height:100%` 居中，不拉伸。比例按**排版后**尺寸判定：非装饰页（源图短边 ≥ 阅读范围宽 1/3，
   会被放大/缩小/补白到画布）直接按画布，装饰小图（短边 < 1/3，保持原尺寸）按源图。html/body（以及
   `#book-columns`、`#book-inner`）的高度链写 100%。

文字 KEPUB 正常；侧载 kepub 书的书签、笔记可用。缺省即 D：漫画产物始终是固定版式（profile `comic_fixed_layout` +
`comic_fixed_layout_percent`），不再靠 `.fxl` 文件名；`epub-to-kepub` 从 OPF 认漫画固定版式（`book-type=comic`）。

**2026-11-05 的 `kepub::VERSION` 2 会让下次 `sync` 重建全部 Kobo 书（含文字书）**：指纹格式段 `kepub1`→`kepub2`
变了，全部过期、都要重新生成。漫画产物会变、会重传；文字书内容没变，重建后按逐字节比较通常不重传，但都要重新生成/比较
一遍。给 vinci 看。

复现：

```fish
cargo build --release
set B target/release   # 或 $CARGO_TARGET_DIR/release
$B/epub-optimize --device=kobo 原书.epub /tmp/x.opt.epub
$B/epub-to-kepub /tmp/x.opt.epub /tmp/x.kepub.epub
```

`epub-to-kepub --no-wrap-image-pages` 仍可关掉纯图页的包装，只用于真机 A/B。

CSS 怪癖（EPUB 和 KEPUB 在 Nickel 里是两套排版引擎，可能不一样）用 `tools/quirks/probe.py kobo` 量，见[设备 · Kobo](devices.md#kobo-clara-bw2026-11-05-真机确认漫画方案-d)。
