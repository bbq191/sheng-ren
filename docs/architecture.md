# 架构

## 总体

![总体流程](img/overview.svg)

三条设计原则：

- **阅读模式（设备 profile）是核心抽象**。屏幕、阅读范围、黑白彩色都从 profile 读；阅读器的怪癖做成 profile 字段（注释回链、漫画固定版式、翻页方向……，
  见[设备 · 阅读模式](devices.md#阅读模式profile)的对照图），不在算法里按设备名写分支。现在三个：`kindle`（产物 AZW3）、`ireader`、`xochitl`（产物 EPUB），都是设备自带的阅读器，用同一套优化规则。
- **书库只存索引，原件是唯一内容**。每个阅读模式的产物都从原件派生，随时能重建；生成前核对原件还是入库时那本书（内容变了就停下，不拿改过的内容冒充原书）。
- **质量门**。产物生成后检查 XHTML 合法性、引用完整性；没通过给出警告。

## Crate 与依赖

![crate 依赖](img/crates.svg)

| crate | 职责 | 命令 |
|---|---|---|
| `library` | 书库：入库（索引）、跟踪同步、按阅读模式生成、产物放在哪、产物指纹 | `booklib` |
| `bookconv` | 内容层：CBZ → EPUB、网页 → EPUB、清洗、优化、图片处理、质量门。不管书库，只按调用方传入的阅读范围和选项处理 | `epub-optimize`、`readable-probe`、`readable-measure`、`ebook-meta` |
| `azw3` | EPUB → AZW3（KF8）写出器，clean-room，见 [AZW3 写出器](azw3.md)。书库生成 `kindle` 模式时把优化好的 EPUB 转一次。`azw3::read` 是 KF8 读取器，只给写出器回读自检和测试用 | `epub-to-azw3` |
| `profile` | 阅读模式的参数，TOML 构建时嵌入，见[设备与可阅读范围](devices.md)；命令行的 `--device=` 用 `device_from_args` 解析 | |
| `drm` | 空壳，解 DRM 暂停 | |

依赖单向无环：`library` → `azw3` → `bookconv` → `profile`（`library` 也直接用 `bookconv`、`profile`）；`drm` 独立。
输入只收 EPUB、CBZ（和网址）；AZW3 只是 `kindle` 模式的产物格式。

### bookconv 模块

| 模块 | 职责 |
|---|---|
| `convert/` | CBZ → 每页一张原图的 EPUB（`cbz`：第一页就是封面，OPF 标"漫画"；`check_cbz` 是入库时的轻量检查）；`common` 放共用的小工具（图片格式识别等）。转换结果的版本号是 `CONVERT_VERSION` |
| `article` | 网页 → EPUB（正文抽取 + 图片保留原图；按 HTTP 头 / `<meta charset>` 识别编码） |
| `optimize/` | 按阅读模式优化 EPUB 的主流程：流式读写（大漫画不整本进内存）、逐文件变换、图片并行处理 |
| `wash/` | 清洗层：字体字号解锁、按语言排版、章节分页、目录修复与生成、全书 id 去重、章尾空白页。全书改链接统一走 `rewrite_book_links`（不改 OPF 的 `<item href>`） |
| `wash/normalize` | 规范整理（清洗层最后一步）：XHTML 修成合法 XML、OPF 升级到 EPUB 3、按 NCX 生成 nav（或按 nav 生成 NCX）、guide 写成 landmarks；`content_properties`/`apply_content_properties` 由优化器在写 OPF 时按最终内容标 manifest 的 `properties` |
| `wash/opf` | OPF 的读与改：往 manifest、metadata 里插入，删 item（连同 spine 引用），找封面；跟随原文件的命名空间前缀（`<opf:item>` 也认）。manifest 项的 zip 路径一律用 `ManifestItem::path`（先还原 `&amp;` 再百分号解码）。清洗层、优化器、漫画标签、`ebook-meta` 共用这一份 |
| `html` | 容错的 XHTML 工具：标签扫描（跳过注释/CDATA）、属性读写（单双引号、无引号）、加类（`add_class`，已有就不加）、纯文本、可见内容判断、CSS 声明切分。全仓库的 HTML 操作都用它（AZW3 写出器也是） |
| `htmlproc/` | XHTML 处理规则：注释搬移与编号（`footnote`、`footnote_cycles`）、字体锁（`fontlock`）、重复 id（`basic`） |
| `cssunlock` | 解开字体、字号、行高的锁（样式表、`<style>`、`style=""` 三处同一张表） |
| `imgopt` / `imgpool` / `jpegopt` | 图片处理（按 EXIF 方向摆正、缩放、漫画单趟处理、灰度）；并发池（按像素额度限内存）；JPEG 哈夫曼表无损重做 |
| `opfmeta` | EPUB 元数据（Dublin Core、封面）的读取与改写：只读、只重写文字条目，图片等原样拷（不解压不重压）；`ebook-meta` 再做一遍 EPUB 3 规范整理，书库生成时补简介/标签/封面不做（优化器会做）。版本号 `opfmeta::VERSION` 进书库指纹 |
| `comic_detect` | 判断一本书是不是漫画（OPF 标了"漫画"的不看张数） |
| `comicfxl` | 漫画写成固定版式（kindle）：OPF 的版式声明、每页 `viewport`、整页图按画布显示 |
| `comicpad` | 漫画设成阅读器页边距 1 后各页的补救（文字页、混排页留边，图页去掉 body 的类） |
| `check` | EPUB 质量门；按路径检查（`check_epub_file`）时图片不读进内存 |
| `epub` / `epubzip` | EPUB 组装（CBZ 转换、网页抓取用，写一份资源释放一份）与读写：`EpubWriter`（mimetype 放第一个且不压缩、图片存储其余压缩、原样拷贝条目，全仓库写 EPUB 都用它）、`read_entries_from`（读条目，可只读文字）；zip 内路径工具：`resolve_href`（链接 → zip 路径与锚点）、`resolve_rel`、`href_to`（生成相对 href，百分号编码） |
| `ncx` | NCX 目录解析、页码分段兜底书签；`rewrite_content_srcs` 逐条改目标，`replace_nav_map` 只换 navMap、其余原样 |
| `netimg` / `direction` / `probe` | 远程图抓取；翻页方向；测量可阅读范围用的"测量书" |
| `util` / `naming` | 转义、全角转半角、文件名、书名规整；原子写（`produce_then_replace`、`write_atomic`、`commit`：先写临时文件、落盘、改名、落盘目录，书库也用这一份）；`util::cli` 是各命令共用的出错退出、读写文件、SIGPIPE 处理 |

### library 模块

| 模块 | 职责 |
|---|---|
| `lib.rs` | 条目（`Meta`）、入库、原件核对、迁移（`dedupe`）、删除 |
| `sources.rs` | 跟踪目录（`track`）与同步（`sync`） |
| `generate.rs` | 生成计划与指纹、产物放在哪（跟踪目录旁镜像 / 书库 `output/`）、与模式无关的中间文件（`PreparedInput`，一本书几个模式共用）、按阅读模式生成（AZW3 模式再调 `azw3` 转一次）、生成记录 |
| `metadata.rs` | `booklib meta`：联网补元数据、调度找封面；生成时往书里补缺的封面、简介、标签 |
| `douban.rs` / `wikidata.rs` | 书目源：豆瓣（搜索建议 + 条目页）、Wikidata（作品、作者照片） |
| `net.rs` / `matching.rs` | 节流重试的 HTTP；书名人名比对（全半角、繁简、译名用字） |
| `cover.rs` / `covergen.rs` | 找原作封面（Open Library / Commons）、生成封面 |
| `fsutil.rs` | 书库的临时文件命名（`.tmp-<进程号>-<计数>-<名>`，写入走 `bookconv::util` 的原子写）、JSON 读写与读缓存、记录文件核对（`check_json`）、流式哈希、进程锁、残留临时文件清理 |

## 书库

![书库目录结构](img/library.svg)

### 入库（`Library::add_file` / `add_url`）

1. 边读原件边算 SHA-256（不整本读进内存），前 12 位作 id；读之前、读完各看一次大小和修改时间，变了就报"文件正在写入"。文件名不是 UTF-8 的拒收。
   已有这个 id 就返回"已在库里"：记着的原件位置已经不在时，改记成这个新位置；就是这个位置的，刷新记着的大小和修改时间。
2. 检查能不能用：只收 EPUB、CBZ（按扩展名，别的格式直接拒收）；EPUB 直接从文件读 zip，只读非图片条目，取书名作者、检查 DRM；CBZ 只读 zip 目录，看有没有页面图片（不整本转换，书名取文件名）。不能用的当场拒收。
3. 在 `masters/.tmp-<id>/` 里写好 `meta.json`（原件绝对路径、SHA-256、大小、修改时间、书名作者），最后一步改名成 `masters/<id>/`。

网址没有原件：抓下来的 EPUB 存成 `masters/<id>/master.epub`。早期版本入库的条目也存着副本，`dedupe` 找到原件后改成只存索引。

### 跟踪与同步（`Library::track` / `sync`）

`sources.json` 记着跟踪的目录，以及其中每个文件上次看到时的大小、修改时间和对应的书 id。`sync` 遍历跟踪的目录（只看 `.epub`、`.cbz`，跳过隐藏文件和目录）：大小和修改时间都没变的跳过；变了或新出现的走一遍入库（按内容 id 判断是新书、已有的书，还是同一路径上的新版本）；上次有、这次没有的，按 id 判断是移动改名还是真删了。入库失败的也记下来（id 为空），文件没变就不再重试。

- 新版本入库失败时，旧版本继续跟踪；旧版本删不掉的记在 `sources.json` 的 `stale` 里，下次再删。
- 同一内容在跟踪目录里还有一份时，索引改记成还在的那份，不算原件不在。
- 跟踪的目录不能互相包含；同一个文件一轮只处理一次。
- 没有任何变化时不写 `sources.json`。
- 早期版本收进来、现在不再支持的格式（MOBI、PDF 等）：文件还在就只是不再遍历它，不算原件不在，条目不删。
- `--watch` 跨轮记住已经报过的问题和失败的生成（`SyncMemo`）：只在第一轮、本轮有增改删、`masters/`（及各条目目录）、`output-state/` 或 `sources.json` 的修改时间变了时才生成（`Library::change_stamp`）；
  生成失败的书按（书、模式、指纹、原件状态）记住，都没变就不重试；原件不在只报一次。长期运行时每轮只有一次目录遍历和 stat。

### 元数据与封面（`Library::fetch_metadata`，`metadata.rs`）

豆瓣搜索建议接口（繁简转换后比对书名、核对作者）→ 条目页取简介、标签、原作名和版本信息（`douban.rs`）；书里没封面的，前几个条目里挑分辨率最高的大图。
豆瓣没有 → Wikidata（书名 → 作品，核对作者；或作者 → 作品，核对书名；`wikidata.rs`）取原作名、首次出版年，Open Library / Wikimedia Commons 取原作封面 → 封面都没有就生成（`covergen.rs`：书名 + 作者照片，字体 `fc-match` 找）。
结果存进 `meta.json` 的 `info`、`cover`（封面图是条目里的 `cover.jpg` 或 `cover.png`；先存 `meta.json`，成功后才删旧图）。
Wikidata 出错时，豆瓣已经找到的元数据照样存下，只有封面这一步报错、下次再找。

HTTP（`net.rs`）在一次运行里各本书共用：请求间隔 1.2 秒；429 按 `Retry-After` 重试，5xx、超时重试几次，4xx 不重试；
连不上的网站记下来，之后发给它的请求立即失败；接连两个不同网站连不上、其间没有请求成功，算断网，`meta` 中止整轮。
网络出错（区别于"查了，没有"）的书报错、不生成封面、不存不完整的结果，下次再查。

流程图见[使用指南 · meta](usage.md#meta联网补元数据和封面)。

生成时 `metadata::with_additions` 把书里**没有的**封面、`dc:description`、`dc:subject` 补进 OPF（其余条目原样拷）；版本信息（出版社、ISBN、译者）不写进书。封面哈希和补进去的简介标签的哈希都进指纹。
`booklib meta` 跳过漫画（CBZ 一律算；EPUB 按优化器同一套判定，只读文字部分）。

### 生成（`Library::build`，`generate.rs`）

![生成一本书](img/build.svg)

1. **计划**：只支持 EPUB、CBZ 来源（早期的其它格式报"不再支持"，`booklib` 事先跳过它们）；取阅读模式产物格式的阅读范围（`kindle` 是 `[readable.azw3]`），算指纹。
2. **产物放在哪**：原件在跟踪目录 `D` 里的，放 `D/../<模式 id>/<原件相对 D 的子目录>/`；`add` 进来的、网址书放 `<书库>/output/<模式 id>/`。
   产物根目录落在某个跟踪目录里面时报错（生成出来的书会被当成新书入库）。文件名 `书名.epub`（AZW3 模式 `书名.azw3`），撞名（不分大小写）或目录里有不认识的同名文件时加 `[id 前 6 位]`；本书已经用着的名字一直用下去。
3. **指纹没变、产物还在原位** → 跳过。指纹没变、只是位置变了（原件移动改名、书名改了）→ 把旧产物挪过去，不重新生成。
4. **先登记再生成**：位置变了时，先把记录改成新位置（指纹留空 = 没完成）、旧位置记进待删；中途被打断的话，下次还认得新位置上的文件是这本书的，旧文件也还会删。
5. **核对原件**：大小和修改时间没变直接用；变了重算哈希，内容一样就更新记录，不一样就报错停下；原件不在也报错。同一次运行里核对过的原件不再核对（多个模式生成同一本书）。
6. 与模式无关的中间文件（CBZ 转出来的 EPUB、补了元数据的 EPUB）放在书库的 `.tmp-<id>-src/`，同一本书接着给别的模式生成时直接用
   （`booklib` 按"书在外层、模式在内层"的顺序生成，一轮结束删掉）；`bookconv::optimize` 流式优化（`OptimizeOpts::for_profile`，三个模式同一套规则）直接写成目标旁边的临时文件 → 质量门 → 落盘后改名到位（`fsutil::commit`）。
   AZW3 模式先把优化结果写进临时目录、过质量门，再用 `azw3::epub_to_azw3_with_warnings` 转成 AZW3 写到目标旁边的临时文件，改名到位。AZW3 的唯一 ID 取书 id 的前 8 位十六进制、时间取入库时间，重建出来 Kindle 仍认作同一本书；转换的警告（不支持的图片、找不到目标的链接）跟着这本书输出。
7. 补上指纹，删掉记录里待删的旧位置（删不掉的留着下次再删），并在产物根目录以内删掉变空的目录。

生成记录是 `<书库>/output-state/<模式 id>.json`：书 id → 产物绝对路径、产物根目录、指纹、待删的旧位置。**只删这里记着的文件**，产物目录里不认识的文件一概不动。
`remove` 和 `sync --prune`、新版本替换旧版本时，按记录删掉这本书在各模式下的产物。

指纹由这些拼成，任何一项变了产物就算过期：

```
原件 SHA-256 | 找来的封面 | 补进去的简介标签（补过东西的再带 i+opfmeta 版本） | 生成流程版本（CBZ 来源再加格式转换版本） | 优化器版本 | 注释方式（jump/popup，图标换数字再带 #，保留回链再带 <） | 模式 id | 阅读范围+漫画白边（+漫画画布、阅读器页边距、翻页方向改写、固定版式） | 黑白/彩色 | 格式（AZW3 再带写出器版本，如 azw33）
```

改了会影响产物的代码时，要把对应的版本号加一，见[开发 · 版本号](development.md#版本号)。

### 可靠性

| 风险 | 做法 |
|---|---|
| 写到一半断电 | `meta.json`、`sources.json`、`output-state/<模式>.json` 和产物都先写临时文件（`.tmp-<进程号>-<计数>-<名>`）、落盘（fsync），再改名、落盘目录；进程被杀留下的 `.tmp-*` 下次拿到锁时清掉（书库外的产物目录只删 `.tmp-` 开头的文件） |
| `meta.json` 还是坏了 | `list` 报出来；`remove` 能删；重新 `add` 同一原件会替换它 |
| `sources.json`、生成记录坏了 | 加锁时核对（`fsutil::check_json`），读不出来就拒绝运行：当成空的写回去会丢掉全部记录（旧产物再也认不出来） |
| 两个 booklib 同时运行 | `.lock` 文件锁，会改动书库的命令拿不到锁就退出；`list` 不持锁，也不写书库（早期条目缺的字段只在持锁时补写） |
| 原件被改 | 生成前核对：大小、修改时间变了就重算哈希，内容不同就停下，提示 `sync` 换成新版本 |
| 原件移动、删除 | 按内容 id 认出移动（`sync`、重新 `add` 更新路径）；删除的 `list`、`sync` 报出来 |
| 产物重名 | 不区分大小写地判断撞名，撞了加 id 后缀；不覆盖不是本工具生成的同名文件 |
| 删错用户的文件 | 产物放在书库外（原件目录旁边），所以只删生成记录里记着的文件，删空目录也只在产物根目录以内 |
| 文件名过长 | 书名按字节截断到 200 字节（ext4 上限 255 字节） |

## 优化流程与内存

`optimize::optimize_epub_file_streaming` 分两个阶段：

1. **阶段一**：非图片条目整份读进来，图片（jpg/png/gif/webp）只记名字和大小。清洗、HTML 变换、注释搬移都在这里完成；注释收集后先核对每条都有章节接收，再搬。
2. **阶段二**：按条目顺序写出。图片这时才从源文件逐张读回，交给 `imgpool` 并行处理，按原顺序取回写进 zip，处理完立刻丢掉。
   同时处理的图总像素有上限（3600 万），超过上限的大页独占全部额度、一张一张来。书里有远程图、或漫画里有 GIF/WebP 页时，
   OPF 推迟到最后写（补 manifest 项、改转了格式的图的 media-type）。

所以峰值内存约为"全书文字 + 同时在处理的几张图"，不随漫画页数增长。并行处理的结果和逐张顺序处理逐字节相同。

各步骤的内容见[排版与优化规则](typesetting.md)。
