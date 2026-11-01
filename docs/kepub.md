# KEPUB 写出器（2026-11-01，未上真机）

Kobo 自带阅读器（Nickel）看到 `.kepub.epub` 扩展名就用 Kobo WebKit 引擎打开（翻页快、有阅读统计）；`.fxl.kepub.epub` 是固定版式。
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
- 图片自己一段：`<span class="koboSpan" id="kobo.段.1"><img …/></span>`（`<img></img>` 连闭合一起包）；
- 不包：`script style svg math head title rt rp textarea audio video canvas noscript template iframe object` 里的文字；`pre` 不切句，每段文字包一次；
- 不改：OPF、CSS、图片（压缩数据原样拷）、导航文档（nav、ncx、目录页）、已经是 KEPUB 的章节；`mimetype` 第一个、不压缩；
- **不加** Kobo 的样式补丁（会改书的版式，维护者定）。书里 CSS 有裸 `div`/`span` 或 `body >` 选择器时只报警告（可能匹配到包装和 koboSpan）。

产物再过一次质量门。书库指纹里格式段是 `kepub<VERSION>`（`bookconv::kepub::VERSION`，改写法就加一）。Kobo 没有 `.sdr`，换目录不搬。

## 真机 A/B

两个待真机定的问题，书库缺省分别是：漫画出 `.kepub.epub`（不是 `.fxl.kepub.epub`）；没有文字的页（封面、漫画页）也加包装。
测试版用命令行做（fish）：

```fish
cargo build --release
set B target/release   # 或 $CARGO_TARGET_DIR/release
$B/epub-optimize --device=kobo 原书.epub /tmp/x.opt.epub
$B/epub-to-kepub /tmp/x.opt.epub /tmp/x.kepub.epub                          # 和书库产物同一做法
$B/epub-to-kepub /tmp/x.opt.epub /tmp/x.fxl.kepub.epub                      # 固定版式：只是文件名不同
$B/epub-to-kepub --no-wrap-image-pages /tmp/x.opt.epub /tmp/x.nowrap.kepub.epub  # 图片页不包装
```

把几份拷到 Kobo 的 `books/` 比：整页图有没有上下边距、翻页速度、能不能正常打开、进度记不记。定了再改缺省。
