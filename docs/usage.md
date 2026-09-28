# 使用指南

## 安装

```sh
cd 本仓库
cargo install --path crates/library     # 装到 ~/.cargo/bin/booklib
```

不想安装也可以直接跑：`cargo run --release -p library --bin booklib -- <命令>`。更新代码后重新执行一次 `cargo install` 即可。

## 书库在哪

缺省是 `~/.local/share/booklib`。要换位置，任选一种：

- 每条命令加 `--library=目录`
- 设置环境变量 `BOOKLIB_DIR`
- 设置了 `XDG_DATA_HOME` 时，缺省位置是 `$XDG_DATA_HOME/booklib`

目录里有什么、索引怎么指向原件，见[架构 · 书库](architecture.md#书库)。

## 命令一览

| 命令 | 作用 |
|---|---|
| `booklib add <文件或网址>...` | 一次性入库单个文件或网址 |
| `booklib track <目录>...` / `untrack` | 跟踪书目录（递归）/ 不再跟踪 |
| `booklib sync [--prune] [--device=…] [--watch]` | 把跟踪的目录镜像进书库，可顺带生成 |
| `booklib list [书名片段或 id...]` | 列出书，以及给各设备生成的产物是否最新 |
| `booklib build --device=<设备> [--force] [--out=目录] [书名片段或 id...]` | 按设备生成 |
| `booklib meta [--force] [--clear] [书名片段或 id...]` | 联网补元数据（简介、标签、原作名），没封面的顺带找封面 |
| `booklib remove <id>...` | 从书库删掉一本书（索引和它的所有产物；原件不动） |
| `booklib dedupe [目录...]` | 早期版本入库的书改成只存索引 |
| `booklib devices` | 列出可用设备 |

退出码：`0` 全部成功；`1` 命令写错了；`2` 有书处理失败（或书库打不开、没有匹配的书）。

### add：入库

```sh
booklib add 三体.epub 某词典.mobi 乱马01.cbz 论文.pdf https://example.com/post
```

**`add` 和 `track` 的分工**：`add` 是一次性的，只收单个文件或网址，入库后原件再怎么变书库都不管（原件改了生成时会停下提示）；
整个书目录用 `track` 登记、`sync` 镜像，之后目录里增、删、改、挪都能同步。给 `add` 一个目录会提示改用 `track`。
两者可以混用：`add` 过的书后来落进了跟踪的目录，`sync` 按内容认得出，不会重复入库。

书库**只存索引**：每本书一个 `meta.json`，记着原件在哪、内容哈希、大小和修改时间、书名作者，不复制原件。生成时读原件：

| 输入 | 生成时 |
|---|---|
| EPUB | 直接用原件 |
| MOBI / AZW / AZW3 / PRC / FB2 | 当场转成 EPUB（转换结果不存） |
| CBZ 漫画 | 当场转成每页一张原图的 EPUB |
| PDF | 有文字层的转 EPUB；扫描件、漫画 PDF 只能给支持 PDF 的设备 |
| 网址 | 没有原件：入库时抓正文和图片组成 EPUB，**这一种存在书库里** |

入库时也会完整检查一遍（能不能转换、有没有 DRM、是不是有效的 EPUB），不能用的当场拒收。

- 同一个文件重复入库会显示 `= 已在库里`（按内容判断，文件改名也认得出）。原件移动过的话，在新位置重新 `add` 一次就更新了位置。
- **原件是书库的一部分**：原件删了，这本书就没法再生成；原件被改了，生成时会发现并停下来（见下文"原件的核对"）。
- 带 DRM 的书会被拒收并说明原因（解 DRM 还没做）。只加密了字体的"伪 DRM"不算，照常入库。

### track / sync：跟踪原件目录

`add` 是一次性的。想让书库一直跟着你的书目录走，用跟踪：

```sh
booklib track ~/Documents/ereader/books          # 登记要跟踪的目录（可以多个）
booklib sync                                     # 把跟踪的目录镜像进书库
booklib sync --device=kindle-pw12-sig,ireader-ocean5-pro   # 同步后接着生成（只重建有变化的）
booklib sync --device=all --watch                # 一直运行，每 60 秒检查一次（--watch=300 改间隔）
```

`sync` 对每个文件：

| 情况 | 处理 |
|---|---|
| 新文件 | 入库 |
| 大小和修改时间没变 | 跳过，不重读（所以反复 sync 很快） |
| 内容变了 | 入库新版本，旧版本的条目连同产物删掉 |
| 改名、移动（仍在跟踪的目录里） | 按内容认出来，不重复入库 |
| 原件删了 | 只报告（这本书没法再生成了）；加 `--prune` 才从书库删掉 |
| 入库失败（DRM、文件损坏） | 报一次错；文件没变就不再重试 |
| 整个目录不在（比如 U 盘没插） | 什么都不动 |

`untrack` 不再跟踪一个目录，已入库的书保留。`--watch` 适合放在后台一直跑；也可以不用它，改用 systemd 定时器或 cron 定期执行 `booklib sync --device=all`。

### build：按设备生成

```sh
booklib build --device=kindle-pw12-sig                        # 全部书
booklib build --device=kindle-pw12-sig,ireader-ocean5-pro 三体  # 多台设备、只生成书名含"三体"的
booklib build --device=all --force                             # 全部设备、全部重建
booklib build --device=kindle-pw12-sig --out="/run/user/1000/gvfs/mtp:host=Amazon_Kindle_…/Internal Storage/documents"   # 生成后直接拷到连着的 Kindle
```

- `--device` 可以写多次，也可以用逗号分隔；`all` 表示全部设备。写成 `--device kindle-pw12-sig`（空格）会报错。
- 书的选择：书名片段或 id 前缀（`list` 第一列），不写就是全部书。
- 每本书记一个**指纹**（原件内容、设备、阅读范围、处理程序版本等）。都没变就显示 `= 已是最新` 并跳过，所以可以放心反复运行。`--force` 强制重建。
- 产物总是生成在 `书库/output/<设备 id>/书名.epub|azw3|pdf`。两本书同名时，后一本加上 `[id 前 6 位]`，不会互相覆盖；目录里已有的同名文件如果不是本工具生成的，也不会被覆盖。
- `--out=目录`：生成后再**拷**过去。只有一台设备时直接放进这个目录，多台时放进 `目录/<设备 id>/`。拷过的会记下来：没变的不重拷，书名变了删掉旧文件，`remove` 时一起删；`list` 里显示为 `✓ 已拷` / `⚠ 旧版` / `? 不在`（设备没连上）。
- **路径里有空格要整个加引号**（比如 Kindle 的 `Internal Storage`）。没加引号时，后半截会被当成书名，结果"没有匹配的书"，这时会提示你。
- 用 USB 连着的 Kindle 在 Linux 上是 MTP 挂载（`/run/user/1000/gvfs/mtp:host=…`），不支持普通写文件，程序会自动改用 `gio copy`。
- 输出里的 `⚠ 质量门未过` 只是提示，产物照样生成。出现时说明原书结构有问题（比如 XHTML 不合法），可以在设备上看看效果。

### 原件的核对

生成前先看原件：

- 大小和修改时间没变 → 直接用，不重读。
- 变了 → 重算哈希。内容一样（比如只是被 touch 过）照常生成；内容不一样就停下，提示先 `sync`（跟踪的目录会把它换成新版本）或重新 `add`。不会拿改过的内容冒充原来那本书。
- 原件不在了 → 提示移动过就 `sync` 或在新位置重新 `add`，不要了就 `remove`。

### list：看书库和产物

```text
3fa9c1e07b2d  epub   三体 — 刘慈欣
      ireader-ocean5-pro   ✓ 最新  output/ireader-ocean5-pro/三体.epub
      kindle-pw12-sig      ⚠ 过期  output/kindle-pw12-sig/三体.azw3
```

- `✓ 最新`：和现在 build 出来的会一样。
- `⚠ 过期`：处理规则或设备参数变了，`build` 会重建它。
- `? 未知`：产物文件被删了，或者设备配置已经不在了。
- 书名下面出现 `✗ 原件不在了` / `⚠ 原件可能改过`：见上文"原件的核对"。

### meta：联网补元数据和封面

```sh
booklib meta                  # 所有书（找过的跳过）
booklib meta 白夜行            # 只找某本
booklib meta --clear 雪人      # 找错了：去掉找来的元数据和封面
booklib meta --force 雪人      # 重找
```

```text
✓ 白夜行
      元数据 ← 豆瓣 https://book.douban.com/subject/10554308/；原作名 白夜行；简介 500 字；标签 悬疑推理、日系推理、…；参考版本 南海出版公司 2013-1-1 刘姿君 译
      封面 ← 豆瓣 白夜行 [日] 东野圭吾 2013（https://img3.doubanio.com/…）
```

**元数据**先找豆瓣条目（书名、作者的比对规则同下面的封面），取内容简介、标签、原作名，以及那个条目的出版社、出版年、ISBN、译者；
豆瓣没有就用 Wikidata 上的原作：原作名、首次出版年。

**写进书里的只有简介（`dc:description`）和标签（`dc:subject`），而且只在书里没有时才补**；书名、作者一律用书自己的，正文不动。
出版社、ISBN、译者是豆瓣**那个版本**的，不一定是你手上这本（好读的书多是台湾译本，豆瓣条目多是大陆版），所以只记在 `meta.json` 里（`info.edition`）给你参考，不写进书。
简介来自豆瓣，是简体字；Kindle 在书的"关于本书"里显示它（AZW3 的 EXTH 103）。

**封面**只给书里没有封面的书找。来源按顺序试（参照 Koodo Reader 用的书目源；它是 AGPL-3.0，只借鉴"用哪些源"，代码没有照搬）：

1. **豆瓣**：中文版封面，最贴近你手上的书。按书名搜，书名繁简转换后比对（好读是繁体、豆瓣多是简体），允许只差卷次后缀；作者去掉国籍前缀（"[美]"）后核对。取大图，几个候选里挑分辨率最高的（老条目只有两百多像素宽的小图，太小的不要）。豆瓣没有公开 API，用的是网页的搜索接口，请求很少且节流。
2. **原作封面**（Wikidata + Open Library）：在 Wikidata 按书名找作品，书名对得上的里面挑作者名最像的；找不到再先找作者、在他的作品里按书名找。好读用 `．` 分隔人名，Wikidata 用 `·` 或 `‧`，比较前都去掉；译名用字不同（歐威爾/奧威爾）按字重合度判断。然后按作品的 Open Library ID、英文名 + 作者、原文名、日文名找封面图，再不行用 Wikimedia Commons 上的作品图片。
3. **都找不到就生成一张**：书名在上、作者头像（Wikidata 上的作者照片）居中、作者名在下。样式（配色、边框）按书挑，每本不一样，同一本每次生成都一样；配色都是深浅对比强的，黑白屏上也清楚。没有作者照片时，中间画作者名的第一个字。输出里标 `◇`。

下载下来的图要能解码、够大、竖版比例，否则试下一张。

书名会试两个：书里的书名，和原件文件名里的书名（`作者《书名》` 取书名号里的）。所以把文件改成更通行的译名（比如《瘟疫》改成《鼠疫》）也能帮它找到。

生成封面要系统里有中文字体（缺省用思源宋体繁体 Noto Serif CJK TC，`fc-match` 找）；换字体用环境变量 `BOOKLIB_COVER_FONT=字体文件[:序号]`。

- 找到的封面存在书库条目里（`masters/<id>/cover.jpg`），`meta.json` 记着匹配到哪个条目或作品、从哪下载的，输出里也会列出来，方便核对。
- **原件不动**。生成产物时，书里没有封面才把它放进去（只在 OPF 里声明封面图，不加封面页，正文不变）。封面、简介、标签变了，产物判为过期，下次 `build` 重建。
- 所有请求间隔 1.2 秒（Wikidata 限速严，被限速时按它给的时间等），只需要每本书跑一次。
- 找不到原作封面的常见原因：书名是这个译本独有的；出版社自编的选集（《歐亨利短篇小說選》）没有对应的原作；原作在 Open Library 上也没有封面图。这些会生成封面。

2026-09-27 实测好读的 18 本没封面的书：11 本用豆瓣的中文版封面（《一九八四》《動物農莊》《白夜行》《雪人》《斜屋犯罪》等），2 本用原作封面（《ABC謀殺案》《鼠疫》），5 本生成（台湾自编的选集、《13級階梯》）。全程约 12 分钟（Wikidata 限速）。

Google Books、Hardcover 也是可用的源，但都要自己申请 API key，暂时没接。

### remove：删书

```sh
booklib remove 3fa9c1e07b2d
```

删除这本书的索引和它在各设备、各 `--out` 目录下的产物。需要 `list` 里显示的完整 id（删除操作不做模糊匹配，免得删错）。你的原件不受影响。

### dedupe：迁移早期版本的书库

早期版本入库时会把原件（或转换结果）存一份在书库里。升级后运行一次：

```sh
booklib dedupe ~/Documents/ereader
```

对每个还存着副本的条目，先看 `meta.json` 记着的原件位置，再在给出的目录里（递归）找内容相同的文件；找到了就改成只存索引、删掉书库里的副本。找不到原件的保留副本并列出来（不然这本书就没了）。网址入库的书没有原件，不动。可以反复运行。

### devices：列出设备

```text
ireader-ocean5-pro   掌阅 iReader Ocean 5 Pro  屏幕 1264×1680  epub
kindle-pw12-sig      Kindle Paperwhite 12 代签名版  屏幕 1264×1680  azw3
rmpp-move            reMarkable Paper Pro Move  屏幕 954×1696  epub/pdf
```

书库的 `profiles/` 目录里放 `<id>.toml` 可以加自定义设备，或覆盖内置设备的参数（比如你在阅读器里改了页边距）。写法见[设备与可阅读范围](devices.md)。

## 传书到设备

| 设备 | 拷什么 | 注意 |
|---|---|---|
| Kindle PW12 | `output/kindle-pw12-sig/*.azw3` 拷到 Kindle 的 `documents/`（或直接 `build --out="…/Internal Storage/documents"`） | USB 传书**只认 AZW3，不认 EPUB**（真机实测）。书出现在"文档"分类里，封面正常 |
| 掌阅 Ocean 5 Pro | `output/ireader-ocean5-pro/*.epub` | |
| reMarkable Move | `output/rmpp-move/*.epub`（图片型 PDF 是 `.pdf`） | 用 reMarkable 自带的传书方式 |

Kindle 重建后的 AZW3 仍被认作同一本书（唯一 ID 取自书的 id），覆盖旧文件即可。

**用 KOReader 读**（三台设备上都装了）：掌阅、Kindle 拷 `output/ireader-ocean5-pro/*.epub`——两台屏幕都是 1264×1680，KOReader 读的是 EPUB；
Move 生成 `--device=rmpp-move-koreader`，拷进 Move 的 `~/xovi/exthome/appload/koreader/books/`（Move 没有 MTP，`--out` 拷不过去，用 `scp`）。
Kindle 上的书目录是 `koreader/resources/books/`，掌阅是 `koreader/books/`。漫画会被 KOReader 自动套上漫画设置（从右往左、铺满整屏），
设备上的 KOReader 配置见 [KOReader 配置](koreader.md)。

## 空间占用

- **书库几乎不占空间**：只有每本书一个 `meta.json`（几 KB），网址入库的书另存一份 EPUB。书的内容只在你的原件目录里有一份。
- **`output/` 里的产物**是处理后的新文件，会占空间。它们随时能用 `build` 重新生成，拷到设备上以后不需要了可以整个删掉。

## 单独的命令行工具

书库之外，底层的每一步也能单独用。开发和排查问题时有用：

```sh
cargo run --release -p bookconv --bin epub-optimize -- --device=kindle-pw12-sig 输入.epub 输出.epub
cargo run --release -p azw3 --bin epub-to-azw3 -- 已优化.epub 输出.azw3     # --ebok 归到"书籍"
cargo run --release -p bookconv --bin cbz2pdf -- --device=rmpp-move 漫画.cbz 输出.pdf
cargo run --release -p bookconv --bin readable-probe -- 测量书.epub          # 见"设备与可阅读范围"
cargo run --release -p bookconv --bin readable-measure -- 竖长.png 横宽.png
```

`epub-optimize`、`cbz2pdf` 要求 `--device=<设备 id>`，不写或写错会列出可用的 id；它们按该设备对应格式的阅读范围处理。

## 常见问题

**为什么一台设备的产物全变成"过期"了？**
处理规则升级了（优化器、AZW3 写出器或生成流程的版本号变了），或者设备配置改了。运行一次 `build` 即可。

**我在阅读器里改了页边距，要做什么？**
可阅读范围跟着变了。漫画要按新范围重新量一次、写进 `书库/profiles/<设备 id>.toml`，再 `build`。文字书不受影响。

**图片型 PDF（扫描件、漫画 PDF）为什么只能给 Move？**
它没有文字层，转不成可重排的 EPUB，只能给支持 PDF 的设备（Move）裁白边后原样投递。给 Kindle、掌阅生成会报错。

**提示"另一个 booklib 正在使用书库"？**
同一时间只允许一个会改动书库的命令运行（`list`、`devices` 不受限）。等前一个结束再试。如果确定没有别的 booklib 在跑，这个提示不会出现：锁在进程退出时自动释放。

**原件放在 U 盘或移动硬盘上可以吗？**
可以。没插上时生成会提示原件不在，插上后照常生成；`sync` 遇到整个目录不在也什么都不动。

**`list` 提示某个条目的 meta.json 读不出来？**
通常是写入时断电。`booklib remove <id>` 删掉它，或者重新 `add` 同一个原件覆盖它。
