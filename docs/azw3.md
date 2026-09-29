# AZW3 写出器

Kindle PW12 用 USB 传书只认 AZW3（KF8），不认 EPUB。`crates/azw3` 把已按 Kindle 优化过的 EPUB 转成 AZW3，只做格式转换，不改内容。

![AZW3 结构](img/azw3.svg)

## 来源：clean-room

- **公开文档**：MobileRead Wiki 的 MOBI 页面，包括 PDB 容器、MOBI 头、EXTH、尾随字节、INDX/TAGX、FLIS/FCIS/EOF。
- **黑盒数据分析**：KF8 特有的部分（片段/骨架/目录索引的标签、FDST、`kindle:pos`/`embed`/`flow` 地址、记录 0 的 8KB 填充）MobileRead 没有文档，都是从样本 AZW3 文件的字节推出来的。分析脚本在 `tools/kf8/`。
- **不看** KindleUnpack、Calibre 的代码。它们是 GPL-3.0，照抄会让产物变成 GPL 衍生作品。读取侧 `bookconv::convert::{palm, kf8}` 也是 clean-room 实现，用来做往返校验。

## 怎么写

| 步骤 | 模块 | 做法 |
|---|---|---|
| 读 EPUB | `book.rs` | 元数据（OPF Dublin Core）、spine 里的 XHTML、CSS、图片（JPEG/PNG/GIF）、封面、目录（NCX，没有就用 EPUB3 nav 里 `epub:type` 含 `toc` 的那个） |
| 排版 | `text.rs` | 每个 XHTML 拆成**骨架**（`<html><head>…<body aid="N"></body></html>`）和**片段**（body 里的内容），依次排成"骨架、片段、骨架、片段…"。CSS 各自一条流 |
| 改写引用 | `text.rs` | 图片 → `kindle:embed:资源序号`；样式表 → `kindle:flow:流序号`；书内链接 → `kindle:pos:fid:片段号:off:片段内偏移`（base32）。链接的路径和锚点先还原字符引用再百分号解码；书里原有的 `aid` 属性（任何引号写法）去掉 |
| 索引 | `indx.rs`、`container.rs` | 片段索引、骨架索引、目录索引（INDX + TAGX + CNCX） |
| 压缩 | `palmdoc.rs` | 每 4096 字节一条记录，PalmDOC 压缩（哈希链找回指，每个 3 字节前缀最多看最近 64 处），多字节字符跨记录时加尾随字节 |
| 组装 | `container.rs` | 记录 0（PalmDOC 头 + MOBI 头 + EXTH）、文字记录、索引、图片、FDST、FLIS、FCIS、EOF |

**链接回填**：链接指向的偏移要等全部文档排完才知道，所以先写一个等长的占位串，并**记下占位串的位置**；排完后按位置回填。回填不改变任何偏移，也不会误改正文里恰好相同的文字。

**目录索引**：条目先放完第 0 层、再放第 1 层（和样本一致），父子关系用条目下标表示。去掉无效目录项后层级可能断档（0 直接到 2），读入时先修成每项最多比前一项深一级。

## 取舍

- **不嵌字体**：去掉 `@font-face`，字体交给阅读器设置。
- **SVG 图片、WebP 等不支持**：转换时给出警告，引用保持原样。
- **归类**：缺省"文档"（PDOC），侧载书的封面显示最稳；`--ebok` 归到"书籍"。
- **唯一 ID**：书库生成时取自书的 id，时间取入库时间。重建出来还是"同一本书"，Kindle 上的阅读进度不丢。单独用 `epub-to-azw3` 时按当前时间生成。
- 日漫写 EXTH 527 = rtl（未在真机验证）。
- 找不到目标 id 的链接、目录项会落到所在章节开头，并给出警告。

## 验证

- `tests/roundtrip.rs`：用 bookconv 的 KF8 读取器读回来，核对索引、链接偏移、目录层级。
- `tools/kf8/textcheck.py 文件.azw3 源.epub`：EPUB 按 spine 顺序、AZW3 按片段顺序取正文的可见文字（去掉标签、注释、script/style，还原字符引用，不计空白），
  整本书的字符序列逐字比对，不一致时报第一处差异、退出码 1。2026-09-28 起比的是字符序列（以前只比各字符的个数，顺序错乱查不出来），当天 10 本真书一致。
- **Kindle PW12 真机**（2026-09-27）：测量书；《疯探》（封面、章标题独立一页、字号字体可调、目录跳转）；《桥头楼上》（节缩进挂在章下、跳转正确）；《ABC谋杀案》（注释标号跳转与返回）；135MB 漫画能打开、翻页正常。

改了会影响产物字节的地方，要把 `azw3::WRITER_VERSION` 加一（见[开发 · 版本号](development.md#版本号)）；怎么确认一次改动没改变产物，见[开发 · 真书回归检查](development.md#真书回归检查)。
