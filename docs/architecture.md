# 架构

## 总体

![总体流程](img/overview.svg)

三条设计原则：

- **设备 profile 是核心抽象**。屏幕、阅读范围、黑白彩色、投递格式都从 profile 读；阅读器的怪癖以后也做成 profile 字段，不在算法里按设备名写分支。
- **书库只存索引，原件是唯一内容**。每台设备的产物都从原件派生，随时能重建；生成前核对原件还是入库时那本书（内容变了就停下，不拿改过的内容冒充原书）。
- **质量门**。产物生成后检查 XHTML 合法性、引用完整性；没通过给出警告。

## Crate 与依赖

![crate 依赖](img/crates.svg)

| crate | 职责 | 命令 |
|---|---|---|
| `library` | 书库：入库（索引）、跟踪同步、按设备生成、产物指纹 | `booklib` |
| `azw3` | EPUB → AZW3（KF8）写出器，clean-room 实现，见 [AZW3 写出器](azw3.md) | `epub-to-azw3` |
| `bookconv` | 内容层：格式转换、清洗、优化、图片处理、质量门。不管书库，只按调用方传入的阅读范围和选项处理 | `epub-optimize`、`cbz2pdf`、`readable-probe`、`readable-measure`、`cover-fix`、`ebook-meta` |
| `profile` | 设备参数，TOML 构建时嵌入，见[设备与可阅读范围](devices.md)；命令行的 `--device=` 用 `device_from_args` 解析 | |
| `pdf-extract` | PDF 文字层提取，pdf-extract 0.12.1 的本地 MIT fork（修改处注释标 `fork：`，汇总在 `src/lib.rs` 头注释）。`bookconv::pdf_ingest` 调它时用 `catch_unwind` 包住，损坏 PDF 触发的 panic 报"PDF 解析失败"，不会带崩整个进程 | |
| `drm` | 空壳，解 DRM 暂停 | |

依赖单向无环：`library` → `azw3` → `bookconv` → `profile`、`pdf-extract`。

### bookconv 模块

| 模块 | 职责 |
|---|---|
| `convert/` | MOBI/AZW/AZW3/PRC（`palm` 容器与 KF8 编码工具、`mobi` 旧格式、`kf8` 新格式）、FB2、CBZ → EPUB；`common` 放各转换器共用的小工具（图片格式识别等）；`pdfwrite::PdfPieceWriter` 逐页写 PDF，可直接写到文件（边写边落盘）。转换结果的版本号是 `CONVERT_VERSION` |
| `article` | 网页 → EPUB（正文抽取 + 图片保留原图；按 HTTP 头 / `<meta charset>` 识别编码） |
| `pdf_ingest/` | PDF 分类（有文字层 / 扫描件 / 漫画）、有文字层的转 EPUB、图片型的裁白边 |
| `optimize/` | 按设备优化 EPUB 的主流程：流式读写（大漫画不整本进内存）、逐文件变换、图片并行处理 |
| `wash/` | 清洗层：字体字号解锁、按语言排版、章节分页、目录修复与生成、全书 id 去重、章尾空白页。全书改链接统一走 `rewrite_book_links`（不改 OPF 的 `<item href>`） |
| `wash/normalize` | 规范整理（清洗层最后一步）：XHTML 修成合法 XML、OPF 升级到 EPUB 3、按 NCX 生成 nav（或按 nav 生成 NCX）、guide 写成 landmarks；`content_properties`/`apply_content_properties` 由优化器在写 OPF 时按最终内容标 manifest 的 `properties` |
| `wash/opf` | OPF 的读与改：往 manifest、metadata 里插入，删 item（连同 spine 引用），找封面；跟随原文件的命名空间前缀。清洗层、优化器、漫画标签、`ebook-meta` 共用这一份 |
| `html` | 容错的 XHTML 工具：标签扫描（跳过注释/CDATA）、属性读写（单双引号、无引号）、纯文本、可见内容判断、CSS 声明切分。清洗层和注释处理都用它 |
| `htmlproc/` | XHTML 处理规则：注释、对比度、重复 id |
| `imgopt` / `imgpool` | 图片处理（缩放、漫画单趟处理、灰度）与并发池 |
| `opfmeta` | EPUB 元数据（Dublin Core、封面）的读取与改写：只动 OPF 和封面图，其余条目原样拷。`ebook-meta` 命令和书库生成时补简介/标签/封面共用 |
| `comic_detect` | 判断一本书是不是漫画 |
| `check` | EPUB 质量门；按路径检查（`check_epub_file`）时图片不读进内存 |
| `epub` / `epubzip` | EPUB 组装（转换器用，写一份资源释放一份）与读取；zip 内路径工具：`resolve_href`（链接 → zip 路径与锚点）、`href_to`（生成相对 href，百分号编码） |
| `ncx` | NCX 目录解析、页码分段兜底书签；`rewrite_content_srcs` 逐条改目标，`replace_nav_map` 只换 navMap、其余原样 |
| `netimg` / `direction` / `probe` | 远程图抓取；翻页方向；测量可阅读范围用的"测量书" |
| `util` / `naming` | 转义、文件名、书名规整；原子写（`produce_then_replace`、`write_atomic`：先写临时文件再改名）；`util::cli` 是各命令共用的出错退出、读写文件、SIGPIPE 处理 |

### library 模块

| 模块 | 职责 |
|---|---|
| `lib.rs` | 条目（`Meta`）、入库、原件核对、迁移（`dedupe`）、删除 |
| `sources.rs` | 跟踪目录（`track`）与同步（`sync`） |
| `generate.rs` | 生成计划与指纹、按设备生成、产物记录 |
| `deliver.rs` | 拷到别处（`--out`，含 MTP 挂载） |
| `metadata.rs` | `booklib meta`：联网补元数据、调度找封面；生成时往书里补缺的封面、简介、标签 |
| `douban.rs` / `wikidata.rs` | 书目源：豆瓣（搜索建议 + 条目页）、Wikidata（作品、作者照片） |
| `net.rs` / `matching.rs` | 节流重试的 HTTP；书名人名比对（全半角、繁简、译名用字） |
| `cover.rs` / `covergen.rs` | 找原作封面（Open Library / Commons）、生成封面 |
| `fsutil.rs` | 原子写（临时文件 + fsync + 改名）、JSON 读写与读缓存、流式哈希、进程锁、残留临时文件清理 |

## 书库

![书库目录结构](img/library.svg)

### 入库（`Library::add_file` / `add_url`）

1. 边读原件边算 SHA-256（不整本读进内存），前 12 位作 id；读之前、读完各看一次大小和修改时间，变了就报"文件正在写入"。文件名不是 UTF-8 的拒收。
   已有这个 id 就返回"已在库里"：记着的原件位置已经不在时，改记成这个新位置；就是这个位置的，刷新记着的大小和修改时间。
2. 完整检查一遍能不能用：EPUB 直接从文件读 zip，只读非图片条目，取书名作者、检查 DRM；MOBI/FB2/CBZ 先转一遍 EPUB（结果不存）；PDF 判定有没有文字层。不能用的当场拒收。
3. 在 `masters/.tmp-<id>/` 里写好 `meta.json`（原件绝对路径、SHA-256、大小、修改时间、书名作者），最后一步改名成 `masters/<id>/`。

网址没有原件：抓下来的 EPUB 存成 `masters/<id>/master.epub`。早期版本入库的条目也存着副本，`dedupe` 找到原件后改成只存索引。

### 跟踪与同步（`Library::track` / `sync`）

`sources.json` 记着跟踪的目录，以及其中每个文件上次看到时的大小、修改时间和对应的书 id。`sync` 遍历跟踪的目录：大小和修改时间都没变的跳过；变了或新出现的走一遍入库（按内容 id 判断是新书、已有的书，还是同一路径上的新版本）；上次有、这次没有的，按 id 判断是移动改名还是真删了。入库失败的也记下来（id 为空），文件没变就不再重试。

- 新版本入库失败时，旧版本继续跟踪；旧版本删不掉的记在 `sources.json` 的 `stale` 里，下次再删。
- 同一内容在跟踪目录里还有一份时，索引改记成还在的那份，不算原件不在。
- 跟踪的目录不能互相包含；同一个文件一轮只处理一次。
- 没有任何变化时不写 `sources.json`。
- `--watch` 跨轮记住已经报过的问题和失败的生成（`SyncMemo`）：只在第一轮、本轮有增改删、书库或产物目录的修改时间变了、`--out` 目录出现或消失时才生成；
  生成失败的书按（书、设备、指纹、原件状态）记住，都没变就不重试；原件不在只报一次。长期运行时每轮只有一次目录遍历和 stat。

### 元数据与封面（`Library::fetch_metadata`，`metadata.rs`）

豆瓣搜索建议接口（繁简转换后比对书名、核对作者）→ 条目页取简介、标签、原作名和版本信息（`douban.rs`）；书里没封面的，前几个条目里挑分辨率最高的大图。
豆瓣没有 → Wikidata（书名 → 作品，核对作者；或作者 → 作品，核对书名；`wikidata.rs`）取原作名、首次出版年，Open Library / Wikimedia Commons 取原作封面 → 封面都没有就生成（`covergen.rs`：书名 + 作者照片，字体 `fc-match` 找）。
结果存进 `meta.json` 的 `info`、`cover`（封面图是条目里的 `cover.jpg`）。

HTTP（`net.rs`）在一次运行里各本书共用：请求间隔 1.2 秒；429 按 `Retry-After` 重试，5xx、超时重试几次，4xx 不重试；
连不上的网站记下来，之后发给它的请求立即失败；接连两个不同网站连不上、其间没有请求成功，算断网，`meta` 中止整轮。
网络出错（区别于"查了，没有"）的书报错、不生成封面、不存不完整的结果，下次再查。

![元数据和封面](img/metadata.svg)

生成时 `metadata::with_additions` 把书里**没有的**封面、`dc:description`、`dc:subject` 补进 OPF（其余条目原样拷）；版本信息（出版社、ISBN、译者）不写进书。封面哈希和补进去的简介标签的哈希都进指纹。

### 生成（`Library::build`）

1. **计划**：确定产物格式（设备首选的流式格式；图片型 PDF 只能给支持 PDF 的设备）、阅读范围，算指纹。
2. **指纹没变且产物还在** → 跳过。
3. **核对原件**：大小和修改时间没变直接用；变了重算哈希，内容一样就更新记录，不一样就报错停下；原件不在也报错。同一次运行里核对过的原件不再核对（多台设备生成同一本书）。
4. 在 `output/<设备>/.tmp-<id>/` 里：（MOBI/FB2/CBZ、有文字层的 PDF 当场转 EPUB）→ `bookconv::optimize` 流式优化 → 质量门 →（Kindle）`azw3::epub_to_azw3`。
5. 成品改名到位，更新 `.state.json`。
6. 给了 `--out` 时拷过去（`Library::deliver`）：普通文件系统先写临时文件再改名；MTP 挂载（错误码 EOPNOTSUPP 或路径在 gvfs 下）不支持普通写，改用 `gio copy`；其它错误直接报错，不删目标位置的旧文件。拷过的记在 `deliveries.json`（目标路径 → 书、设备、指纹、大小），没变的不重拷。

指纹由这些拼成，任何一项变了产物就算过期：

```
原件 SHA-256 | 找来的封面 | 补进去的简介标签 | 生成流程版本（非 EPUB 来源再加格式转换版本） | 优化器版本 | AZW3 写出器版本 | 设备 id | 阅读范围 | 黑白/彩色 | 格式
```

改了会影响产物的代码时，要把对应的版本号加一，见[开发 · 版本号](development.md#版本号)。

### 可靠性

| 风险 | 做法 |
|---|---|
| 写到一半断电 | `meta.json`、`sources.json`、`deliveries.json`、`.state.json` 和产物都先写临时文件（`.tmp-<进程号>-<计数>-<名>`）、落盘（fsync），再改名、落盘目录；进程被杀留下的 `.tmp-*` 下次拿到锁时清掉 |
| `meta.json` 还是坏了 | `list` 报出来；`remove` 能删；重新 `add` 同一原件会替换它 |
| 两个 booklib 同时运行 | `.lock` 文件锁，会改动书库的命令拿不到锁就退出；`list` 不持锁，也不写书库（早期条目缺的字段只在持锁时补写） |
| 原件被改 | 生成前核对：大小、修改时间变了就重算哈希，内容不同就停下，提示 `sync` 换成新版本 |
| 原件移动、删除 | 按内容 id 认出移动（`sync`、重新 `add` 更新路径）；删除的 `list`、`sync` 报出来 |
| 产物重名 | 不区分大小写地判断撞名，撞了加 id 后缀；不覆盖不是本工具生成的同名文件 |
| 文件名过长 | 书名按字节截断到 200 字节（ext4 上限 255 字节） |

## 优化流程与内存

`optimize::optimize_epub_file_streaming` 分两个阶段：

1. **阶段一**：非图片条目整份读进来，图片只记名字和大小（GIF、WebP 不做图片处理，这时就整份读进来原样写出）。清洗、HTML 变换、注释搬移都在这里完成；注释收集后先核对每条都有章节接收，再搬。
2. **阶段二**：按条目顺序写出。图片这时才从源文件逐张读回，交给 `imgpool` 并行处理，按原顺序取回写进 zip，处理完立刻丢掉。

所以峰值内存约为"全书文字 + 同时在处理的几张图"，不随漫画页数增长。并行处理的结果和逐张顺序处理逐字节相同。

各步骤的内容见[排版与优化规则](typesetting.md)。
