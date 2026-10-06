# AZW3 写出器

Kindle 自带阅读器 USB 传书不认 EPUB，认 AZW3（KF8）和侧载的 KFX。**书库 2026-10-05 起给 Kindle 出 KFX（见 [kfx.md](kfx.md)），AZW3 写出器不再进书库**，命令行 `epub-to-azw3` 还在。`crates/azw3` 把已经按 `kindle` 模式优化过的 EPUB 转成 AZW3，**只转格式，不改内容**。

## 用法

书库不再调用它（2026-09-30～10-05 书库的 `kindle` 模式用它出 AZW3）。现在单独用（`./install.sh --tools` 装）：先按 `kindle` 模式优化出 EPUB，再转：

```sh
epub-optimize --device=kindle 输入.epub 优化后.epub
epub-to-azw3 [--ebok] 优化后.epub 输出.azw3
```

AZW3 只是**产物**格式，不是输入：入库仍然只收 EPUB、CBZ。

![AZW3 结构](img/azw3.svg)

## 来源：clean-room

- **公开文档**：MobileRead Wiki 的 MOBI 页面（PDB 容器、MOBI 头、EXTH、尾随字节、INDX/TAGX、FLIS/FCIS/EOF）。
- **黑盒分析**：KF8 特有的部分（片段/骨架/目录索引的标签、FDST、`kindle:pos`/`embed`/`flow` 地址、记录 0 的 8KB 填充）没有公开文档，是从样本 AZW3 文件的字节推出来的。分析脚本在 `tools/kf8/`。
- **不看** KindleUnpack、Calibre 的代码（GPL-3.0，照抄会让产物变成 GPL 衍生作品）。
- 读取侧 `azw3::read::{palm, kf8}` 也是 clean-room，**只给写出器回读自检和测试用**，不是输入格式。

## 结构

| 步骤 | 模块 | 做法 |
|---|---|---|
| 读 EPUB | `bookconv::epubbook`（和 KFX 写出器共用） | 元数据、spine 里的 XHTML、CSS、图片（JPEG/PNG/GIF；静态 WebP 转成 PNG，像素不变）、封面、目录（NCX，没有就用 EPUB 3 nav） |
| 排版 | `text.rs` | 每个 XHTML 拆成**骨架**（`<html><head>…<body aid="N"></body></html>`）和**片段**（body 里的内容），依次排成"骨架、片段、骨架、片段…"；CSS 各自一条流 |
| 改写引用 | `text.rs` | 图片 → `kindle:embed:序号`；样式表 → `kindle:flow:序号`；书内链接 → `kindle:pos:fid:片段号:off:偏移`（base32）。标签属性用 `bookconv::html` 扫；链接目标认 `id` 和 `<a name>` |
| 索引 | `indx.rs`、`container.rs` | 片段索引、骨架索引、目录索引（INDX + TAGX + CNCX） |
| 压缩 | `palmdoc.rs` | 每 4096 字节一条记录，PalmDOC 压缩，多字节字符跨记录时加尾随字节 |
| 组装 | `container.rs` | 记录 0（PalmDOC 头 + MOBI 头 + EXTH）、文字记录、索引、图片（含封面和缩略图）、FDST、FLIS、FCIS、EOF |

- **链接回填**：链接指向的偏移要等全部排完才知道，所以先写等长的占位串、记下它的位置，排完按位置回填。不改变任何偏移，也不会误改正文里恰好相同的文字。
- **目录索引**：先放完第 0 层、再放第 1 层（和样本一致），父子关系用条目下标表示；层级断档（0 直接到 2）的先修成每项最多深一级。

## 取舍

- **`<head>` 里只留 title、meta、link、style、base**：EPUB 阅读器不显示 `<head>`，Kindle 却会把里面散落的文字显示在章首（《绝叫》原书每章 `<head>` 漏进一段样式代码，Kindle 上满页代码）。正文不动。
- **漫画写成固定版式**：OPF 里有 `<meta name="fixed-layout" content="true"/>` 时，这一组声明（`fixed-layout`、`book-type`、`orientation-lock`、`original-resolution`、`zero-gutter`、`zero-margin`）原样写成 EXTH 122–128。`epub-optimize --device=kindle` 给漫画写上这组声明，画布整屏 1272×1696（为什么见[排版 · 离屏幕边缘 1px](typesetting.md#离屏幕边缘-1px三种做法)）。没有这个声明的书一概不写。
- **日漫从右往左翻**：spine 写了 `page-progression-direction="rtl"`（带 `opf:` 前缀的也认）时写 EXTH 527 = rtl。
- **不嵌字体**：去掉 `@font-face`，字体交给阅读器。
- **SVG 图片、动画 WebP 不支持**：给出警告，引用保持原样。
- **归类**：缺省"文档"（PDOC），侧载书的封面显示最稳；`--ebok` 归"书籍"。
- **唯一 ID 稳定**：由 OPF 的唯一标识符派生、时间取 `dcterms:modified`：同一本书每次转出来逐字节一样。（历史：书库出 AZW3 时由书 id 和入库时间派生，重建出来 Kindle 仍认作同一本书。）
- 找不到目标的链接、目录项落到所在章节开头，并给出警告。

## 电脑上的核对

- `crates/azw3` 的单元测试和 `tests/roundtrip.rs`：用 `azw3::read` 读回来，核对索引、链接偏移、目录层级。
- `tools/kf8/textcheck.py 书.azw3 优化后.epub`：两边按阅读顺序取可见文字（去标签、还原字符引用、不计空白），整本逐字比对，一致报 `SAME`，不一致报第一处差异、退出码 1。源 EPUB 用**优化后的**那份。
- `tools/kf8/dump.py`（记录 0、MOBI 头、EXTH）、`indexes.py`（FDST、各索引）用来看文件内部，`kf8lib.py` 是它们共用的只读解析。这些脚本和 `epub-to-azw3` 都认 `-h`/`--help`。

什么时候要跑这些核对、版本号什么时候加一，见[开发](development.md#真书回归)；真机上看过什么，见[排版 · 验证情况](typesetting.md#验证情况)。
