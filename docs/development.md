# 开发

## 常用命令

```sh
cargo build --workspace
cargo test --workspace                      # 全部测试，要求全部通过（2026-10-19：670 个通过、没有忽略的）
cargo test -p bookconv <测试名子串>          # 只跑名字匹配的
cargo clippy --workspace --all-targets      # 要求 0 警告

tools/regress/run.sh <epub-optimize> 目录 [--device=…]   # 真书回归（见下）
python3 tools/display/sample.py move|ireader 书.epub 输出目录 --title=章名; sample.py kindle 输出目录; sample.py compare 参照图 图…   # 实际显示抽样：三台同一处章首按像素量每块的位置（见文件说明；Kindle 截图要真机手截）
tools/regress/compare.py 旧目录 新目录                    # 比较两次回归
tools/quirks/probe.py move                               # 怪癖测试书：生成、导入 Move、拉回排出的 PDF 逐行量（make 只生成、measure 量现成的 PDF）；结论写进 crates/bookconv/data/css-quirks.toml
tools/quirks/probe.py ireader                            # 同上，掌阅：adb 推进 documents/测试/、点开、逐页截屏（标记是条码图）逐行量
#   两个都可加 --only=88-99（只跑这几个探针）、--optimize=target/release/epub-optimize（先按那台的模式优化，看修过以后的样子）
tools/regress/tocchk.py 旧目录 新目录                     # 核对 spine 文件数（不拆文件）和目录（层级、标签、锚点都在）
shellcheck -x install.sh uninstall.sh tools/cargo-pkgs.sh xochitl/comic-margins.sh tools/regress/run.sh

./install.sh --tools                        # 装进 PATH 手工试
```

**CI**（`.github/workflows/ci.yml`，2026-10-09）：每次推送、PR 在 GitHub Actions 上跑 `cargo test --workspace`、`cargo clippy --workspace --all-targets -D warnings`、全部 `*.sh` 的 shellcheck、全部 `*.py` 的语法检查（CI 用最新稳定版；本机 2026-10-09 起用 nightly，nightly 的 clippy 检查更多，本机 0 警告时 CI 一般也是 0 警告。新版 clippy 加了检查可能突然报警告，照改即可，改完核对产物逐字节不变）。公开仓库用 GitHub 托管的标准机器不收费。真书回归、真机验证 CI 做不了，仍在本机。

书库的集成测试在 `crates/library/tests/`：`flow.rs` 测入库、同步、生成、挪位置、删书、传设备；`cli.rs` 直接运行 `booklib`，测参数、报错和退出码。都不联网、**不碰真设备**：`tests/common` 把 MTP 挂载目录指到 `<书库>-dev/`（下面造 kindle、ireader 的存储目录），Move 不走 SSH（`BOOKLIB_NO_SSH=1`），要测 Move 的用 `FakeMove`（本机起一个只实现导入接口的假书架服务）。

## 真书回归

测试用真书在 `~/Documents/ereader/books/`（路径里带"漫画"的是漫画，其余是文字书）。**只读，绝不改动**：产物一律写到别处，别用 `epub-optimize` 的"输入输出同路径"。用户会自己整理这个目录，别假设书还在上次的位置。

改动前的提交构建一份 `epub-optimize`，和改动后的各跑一遍，然后比较：

```sh
git worktree add target/regress-base HEAD && (cd target/regress-base && cargo build --release -p bookconv --bin epub-optimize)
tools/regress/run.sh target/regress-base/target/release/epub-optimize target/regress/旧   # 缺省 --device=ireader；放 target/ 下，/tmp 是内存盘
cargo build --release -p bookconv --bin epub-optimize
tools/regress/run.sh target/release/epub-optimize target/regress/新
tools/regress/compare.py target/regress/旧 target/regress/新
tools/regress/tocchk.py target/regress/旧 target/regress/新
```

- `run.sh` 把全部文字书和第一卷漫画各优化一遍；开跑前核对 `epub-optimize` 是可执行文件、书目录在（`$REGRESS_BOOKS`，缺省 `~/Documents/ereader/books`），清掉输出目录里的旧产物（免得没生成的书拿旧文件去比、悄悄通过），一本书都没找到就报错退出。
- `compare.py` 按原书路径配对新旧产物，每本报一行：`SAME`（每个 zip 条目逐字节相同；先比条目名、大小、CRC，一样再比字节）、`TEXT-SAME`（有条目变了，可见文字一样）、`TEXT-DIFF`（可见文字变了，打印第一处不同）、`MISSING`/`NEW`（只有一边有）、`BROKEN`（产物读不出来：不是 zip、没有 `container.xml` 等）。书里的优化标记 `META-INF/eink-optimized` 2026-10-09（v54）起只写 `full`/`core`、不写版本号，升版本号本身不再让每本都变成 `TEXT-SAME`，`SAME` 就是真没变。
- `compare.py` 另外核对几件事，不过就退出码 1：新产物里不合法的 XML 不比旧的多；**正文图片（`<img>`、SVG `<image>`）不比原书少**（字符账只管文字，只有图的页被删了它看不出来；只有图、没有文字的注释号链接里的图不算，它们按规则换成数字）；新产物的可见文字和**原书**逐字符对账（按字符计数、不管顺序——注释会挪到章末——一个不多一个不少）。原书不在了、读不出来都报"未核对"，也算问题。字符账不计空白、U+FEFF 和控制字符（《金庸全集》原书一个损坏的 U+0010 被规范整理去掉，不算少字）。
- 字符账唯一的例外是用户定的"只有图标的注释号换成数字"：只多出 ASCII 数字、原书里有只含图片的链接、多出的位数不超过按图标个数连续编号的位数时，只注明、不算不平（以前春雪、雪国、飘上下册、绍宋、绝叫 6 本因此一直报不平）。

| 改动的性质 | 要求 |
|---|---|
| 重构、提速、修不影响产物的问题 | EPUB 产物逐个 zip 条目字节相同（EPUB 输出是确定的） |
| 改了会影响正文的规则 | 可见文字改动前后一致；spine 文件数和原书一样（不拆文件，原有的空页清理除外）；目录（NCX）条目数、层级、标签前后一致，每条的目标文件和锚点都在（`tocchk.py`）；有变化的逐条说明原因 |
| 任何改动 | 输出的 XHTML、OPF、NCX 都是合法 XML（现在是 0 个不合法） |
| 改了漫画处理 | 拿一卷漫画比图片字节 |

**跑哪些模式**：缺省只跑 `ireader`。三个模式的文字书都只修复，差别在：

| 模式 | 文字书和 `ireader` 差在 | 漫画和 `ireader` 差在 |
|---|---|---|
| `kindle` | 不加 `kindle_rules`（规则在 KFX 写出器里），最后还要转一步 KFX（见下面 [KFX](#kfx)） | 阅读范围、固定版式（`comic_fixed_layout`）、不改翻页方向、背景图保留尺寸 |
| `xochitl` | 另加 `repair_note_links`（注释搬进本章、去回链、图标号换数字） | 彩色、页边距 1（`comic_reader_margins`）、去掉背景图和回链、不改翻页方向 |

背景图、`rgba()`、图注、透明图这几个字段对文字书不起作用（只修复）；背景图、`rgba()` 还管漫画（清洗层两路都过）。注释方式（`notes`）只修复时也不生效，但仍进文字书、漫画两路的指纹，改了照样过期。阅读范围对文字书只影响 KFX 的 `@media` 求值。改了 `kindle_rules`、注释、漫画、彩色、阅读范围相关的处理，对应的模式也跑一遍。

Move 的层叠顺序和 CSS 不一样（`!important` 不提优先级；优先级一样先比规则在各自样式表里的序号，见 [xochitl.md](xochitl.md)）：改了往 Move 产物里写样式表的规则，用 `cargo run --release -p bookconv --example xochitl_order -- 回归目录/xochitl/*.epub` 列出照 CSS 和照 Move 层叠不一样的属性（标题的外边距 Move 本来不认，可以不管）。

回归脚本统计 spine 时不算原书标了 `linear="no"` 的目录页（`regresslib.spine_paths`；产物把它拿出了 spine，文件还在），补的封面页 `eink-cover.xhtml` 由 `tocchk.py` 单独注明、不算拆文件。

### KFX

KFX 没有逐字比文字的工具，靠"新旧写出器的产物逐字节比"（KFX 输出也是确定的：唯一 ID 由 OPF 标识符派生，书里不写写出器版本）：

```sh
# 优化器产物用 --device=kindle 的回归目录（上面 run.sh 加 --device=kindle）
(cd target/regress-base && cargo build --release -p kfx --bin epub-to-kfx)
cargo build --release -p kfx --bin epub-to-kfx
mkdir -p target/kfx-old target/kfx-new
for f in target/regress/新/*.epub; do n=$(basename "$f" .epub)
  target/regress-base/target/release/epub-to-kfx "$f" "target/kfx-old/$n.kfx" >/dev/null
  target/release/epub-to-kfx "$f" "target/kfx-new/$n.kfx" >/dev/null
  cmp -s "target/kfx-old/$n.kfx" "target/kfx-new/$n.kfx" || echo "变了：$n"
done
```

| 改动的性质 | 要求 |
|---|---|
| 重构、提速写出器 | 同一份优化后 EPUB，新旧写出器的 KFX 逐字节相同（2026-10-06 提速省内存那次：《北斗之拳》卷01 峰值 1.45→0.73GB，金庸全集 0.87→0.56GB、1.7→0.7 秒，阿加莎全集 3.3→1.4 秒，产物逐字节不变） |
| 照 Send to Kindle 的规则（写出器、掌阅/Move 的 `kindle_rules`） | **改了 `kindle_rules` 或写出器就跑 `tools/kfx/unify.py ~/Documents/ereader/books target/unify`**（掌阅、Move 对我们的 Kindle KFX 逐项一致率，不要 Amazon 样本；比基线 `tools/kfx/unify-baseline.tsv` 差、或新书低于门槛就退出码 1，有意改了就 `--update` 写回基线一起提交；新出现的偏差按[五类](kfx.md#三台统一的回归门槛2026-10-12)归类，属于漏洞的修掉）；`tools/kfx/s2kbatch.py`（写出器对 Amazon 样本逐属性）、`tools/kfx/s2kdev.py`（三台对 Amazon 样本逐项一致率），见 [KFX · 三台和 Send to Kindle 的一致性](kfx.md#三台和-send-to-kindle-的一致性)；样本在 Kindle 的 `documents/Downloads/Items01/` |
| 三台版式（2026-10-15 用户定的验收重点：居中、竖排、横线、分页、封面不多页、漫画留边） | **跑 `tools/kfx/layout.py ~/Documents/ereader/books target/layout`**：以 Kindle KFX 为准，逐个文字块查掌阅、Move 的对齐、窄块竖排、块宽、上下横线/竖线/`<hr>`、另起一页、空白页、封面页（只有图的节的个数和位置，合集分书封面不多页；Move/掌阅 `break_margin` 估算插的分页在 Kindle 上是版面自己挪页、KFX 里不分节，并回前一节）（读 `epub-to-kfx --styles` 的 `#sec`/`#open`/`#box`/`#rule`/`#hr` 行；阅读器怪癖按 profile 的 `padding`、`block_width`、`borders` 折进来；边框颜色和底色灰度差不到 24 的不算；Move 搬进本章的注释、KINDLE_RULES 18 估算的分页不算）。一处不一样就列，不打总分；基线 `tools/kfx/layout-baseline.tsv` 记每本每台各类的处数，多了退出码 1。**漫画跑 `tools/kfx/comics.py ~/Documents/ereader/books target/comics`**：页数、画布和阅读范围同比例（不拉伸）、翻页方向照书（profile `comic_page_direction` 例外）、Kindle KFX 固定版式 1:1 居中算硬性；受限边离屏幕 1–2px、居中按像素量（JPEG 振铃、纸色淡边会差几 px），列出来看图 |
| 改了写出器的行为 | 只有该变的书变了，逐本说明（比如写出器 7 只有《福尔摩斯探案全集》变；写出器 8 同一个 `--id` 下全部逐字节不变，书库产物因唯一 ID 改取书 id 全变）；`WRITER_VERSION` 加一；上真机看过再写 ✓ |
| 改了容器读写（`ion`、`container`） | `kfx-repack` 解开再打包设备上的样本（`target/kfx-samples/` 等）全部逐字节相同 |

改了优化器而 KFX 字节变了的书，覆盖到 Kindle 上会丢进度（见[设备 · 重拷书以后进度还在不在](devices.md#重拷书以后进度还在不在)），值得在提交说明里写清是哪些书。

## 版本号

改了会影响产物的代码，把对应的版本号加一，书库据此把旧产物判为过期（每个版本号进指纹的哪一段，见[架构 · 指纹](architecture.md#指纹)）。**现值只写在这张表里**，别处不再写：

| 改了什么 | 版本号 | 现值 | 过期的书 |
|---|---|---|---|
| 文字书的清洗、优化、图片处理 | `bookconv::optimize::OPTIMIZE_VERSION`（附一行变更说明） | 62 | 文字书 |
| 漫画的处理（裁边、缩放补白、灰度、固定版式……） | `bookconv::optimize::COMIC_VERSION`（附一行变更说明；2026-10-08 从上一行分出来） | 56 | 漫画 |
| 掌阅、Move 照 Send to Kindle 的规则统一（`kindle_rules`） | `bookconv::wash::KINDLE_RULES_VERSION`（附一行变更说明；只进指纹的 `u` 段） | 41 | 只有开了 `kindle_rules` 的模式的文字书 |
| CBZ → EPUB 的转换 | `bookconv::convert::CONVERT_VERSION`（附一行变更说明） | 2 | 只有 CBZ 来源的 |
| 生成时往书里补封面、简介、标签、作者国籍、译者、出版社、日期、ISBN，书名换成网站书名 | `bookconv::opfmeta::VERSION` | 6 | 只有补过东西的 |
| EPUB → KFX | `kfx::write::WRITER_VERSION` | 24 | 只有 `kindle` 模式的（漫画、全图书产物逐字节不变，设备上不重传） |
| 书库生成流程本身 | `library` 的 `PIPELINE_VERSION`（慎用） | 5 | 全部 |

两路共用的代码（清洗层、EPUB 3 规范整理、写 zip……）改了影响产物时，`OPTIMIZE_VERSION`、`COMIC_VERSION` 都加一；只动了一路的只加那一路的。
profile 里设备模型的字段（`display_none`、`trailing_br_line`、`line_height_except_p`、`heading_font_keywords`、`[heading_sizes]`、`margin_top`、`[page_top]`，`bookconv::reader_model`）和 `em_px` 以哈希进指纹（`m` 段），改了它们（比如重量了 Move 的标题字号）不用加版本号。

**版本号写进指纹、不写进书**（2026-10-09 用户定）：书里的优化标记 `META-INF/eink-optimized` 只写 `full`/`core`，KFX 里的创建器版本固定写 `1`。所以加了版本号以后书会重新生成，但内容没变的书生成出来逐字节相同，设备上不重传；只有真变了的书才传。改了影响产物的代码一律加版本号，不因为"书库里的书都没碰到"省掉（用户定）。

## 工程约束

**重构的安全网（2026-10-15）**：
- 指纹：`cargo test -p library --test fp_golden`（见 [架构 · 指纹](architecture.md#指纹)）；有意改了指纹写法才用 `UPDATE_FP_GOLDEN=1` 重写 `fp_golden.txt` 一起提交。
- 产物：重构前后各跑一次三台的 `tools/regress/run.sh`（`--device=kindle|ireader|xochitl`）和 kindle 产物的 `epub-to-kfx`，EPUB、KFX 逐字节 `cmp`。
- profile 的 CSS 怪癖字段收在 `Profile::css`（`profile::CssQuirks`，TOML 里照旧平铺）；「三台以哪台为准」只在 `profile::reference()` 定，`OptimizeOpts::for_profile_against` 可另给。
- 只修复时清洗选项带哪些，由 `WashOpts::repair_from` 逐字段解构决定：给 `WashOpts` 加字段编译不过，逼着决定带不带（以前手写列表漏带过 `media`，KINDLE_RULES 7）。
- `kindle_rules` 各步的开头用 `wash::scan_chapters`（全书样式表 + 逐章并行），新写的文件放 `wash::beside_opf`；别再各写一份。
- `tools/` 的脚本读 profile 用 `toollib.profile(模式)`（`epub-optimize --print-profile`），别再按行扫 TOML 自己填缺省。

用户定的硬约束（只用 Rust、不用 Calibre，clean-room，命名，内容一字不改……）见[决定记录 · 一直有效的约束](decisions.md#一直有效的约束)。写代码时另外要守的：

- **不写死屏幕数字**：一律从模式读。优化选项用 `OptimizeOpts::for_profile(模式)` 创建（书库和 `epub-optimize` 同一个起点）；测试里才用 `OptimizeOpts::new(阅读范围)`。不按设备名写分支，阅读器的怪癖做成模式字段。
- **真机验证后才能说完成**：改变阅读效果的规则，没在目标设备上看过就不写"✓"；一台验证过不能推到别的；电脑上的检查不能代替真机。结果写进[验证情况](typesetting.md#验证情况)。
- **共用的轮子别再各写一份**（2026-09-30 全系统审计收拢过）。下表写的是实际所在的 crate；2026-10-15 拆 crate 以后 `bookconv` 照旧
  再导出 `util`、`html`、`color`（`bookbase`）、`cascade`（`csscascade`）和 `epubzip` 里的路径函数（`bookbase::epubpath`），
  `bookconv` 里面的代码写 `crate::util::…`、`crate::cascade::…` 照样用：

  | 要做的事 | 用这个 |
  |---|---|
  | HTML 操作（容错单双引号、注释、不把 `data-id` 当 `id`；别写只认双引号的正则） | `bookbase::html`；加类 `html::add_class`；不分大小写查找 `html::{find_ci, contains_ci}` |
  | 写 EPUB / 读条目 / 读整本（元数据、spine、CSS、图片、目录） | `epubzip::EpubWriter` / `read_entries_from` / `epubbook::load`（KFX 写出器用；从文件读用 `epubbook::load_from`，不整本读进内存） |
  | manifest 路径；书里链接解析（还原字符引用 → 拆锚点 → 百分号解码 → 规整路径） | `ManifestItem::path`；`bookbase::epubpath::resolve_link`（`epubzip::resolve_link` 再导出） |
  | 是不是书外链接（`http:`、`mailto:`、`data:`、`//`…；按 RFC 3986 的协议名判断，原书文件名里的 `:` 不算） | `html::is_external`（别再写 `contains("://")`、`contains(':')`） |
  | 原子写文件（书库也用） | `bookbase::util` 的 `produce_then_replace`、`commit` |
  | DRM 判定；全角转半角；图片格式识别；哈希 | `wash::encrypted_targets`；`util::to_halfwidth`；`util::image_kind`；`util::fnv64` |
  | `@font-face` 规则（清洗层剔除死字体、字体分析、KFX 写出器共用） | `csscascade::font_face_rules`、`csscascade::is_font_face` |
  | install / uninstall 共用的包列表和路径 | `tools/cargo-pkgs.sh` |
  | 解码外来图片（读头 → EXIF 方向 → 解码上限 → 解码 → 摆正，兜住 panic） | `imgopt` 里的 `ImgHead`；crate 外用 `imgopt::decode_capped` / `decode_oriented`（别再直接 `image::load_from_memory`） |
  | 逐文件独立的步骤多线程做（结果按原顺序；`f` 必须是纯的） | `bookbase::util::par_map`（只读）/ `par_map_mut`（就地改）；写回要按章节顺序编号的，拆成「并行算编辑 → 串行编号 → 并行改写」三段（`wash/box_fix.rs`、`blank_fold.rs`、`inline_css.rs` 的写法） |
  | 在别的线程先压好一个 zip 条目，再按顺序写 | `epubzip::Precompressed` + `EpubWriter::put_precompressed` |
  | 原样拷原书一个 zip 条目的压缩数据（不解压不重压；改过名的写成新名） | `EpubWriter::raw_copy` / `raw_copy_as` |
  | 并行线程数（图片 worker、`par_map` 共用；CPU 核数，封顶 16） | `bookbase::util::worker_count()`（`imgpool::worker_count` 再导出） |
  | 选择器最后一段（标签、类、有没有 id/伪类） | `csscascade::last_compound`、`csscascade::compound_tag_classes`（2026-10-08 审计从 5 处收拢，2026-10-10 挪进层叠引擎） |
  | 非 UTF-8 的 CSS 按单字节读写 | `util::latin1_decode` / `latin1_encode` |
  | 内容层报错（要分清取消、超上限、损坏、IO、DRM 的；别再拿错误串开头判断） | `bookconv::BookError`（`BookError::io`/`zip` 带说明，`context` 加前缀不改变体；文字和以前的错误串逐字相同） |
  | 颜色解析、对比度、半透明叠色（优化器和 KFX 写出器共用） | `bookbase::color`（`ensure_contrast`、`over`） |
  | 改 `style` 属性（字符引用先还原、改完再转义） | `html::edit_style_attrs`（别直接拿 `edit_attrs` 的原文去切声明） |
  | 取看得见的文字（认全部 HTML 命名实体） | `html::plain_text`、`html::has_visible`、`html::unescape_entities` |
  | container.xml 里的 OPF 路径 | `wash::opf::container_opf_path` |
  | CSS 里的 `url(…)`（样式表、`<style>`、`style` 属性；2026-10-09 从三份收拢） | `html::css_urls` |
  | GBK、Big5、UTF-16 的 XHTML、OPF、NCX 转 UTF-8 | `wash::transcode_entries`（优化器读完书先转，后面一律按 UTF-8） |
  | 搜索结果的书名、作者对不对得上（豆瓣、QQ 阅读共用） | `library::matching::hit_matches` |
  | 标签缺省样式（优化器的 `eink-ua.css`、KFX 写出器共用一张表） | 表在 `csscascade::ua`，写成样式表的 `bookconv::uastyle::ua_css` |
  | 一种 CSS 写法在掌阅、Move 上和 Kindle 比怎么样（验证过的结论，体检、怪癖测试书共用） | `crates/bookconv/data/css-quirks.toml` + `bookconv::audit`（新结论写进表里，别再只写在文档的文字里；按属性分节、第一条对上的算，挪顺序后跑 `cargo test -p bookconv no_dead_rows`，前后各用 `QUIRKS_SNAPSHOT=文件` 导出每种写法的结论比对，2026-10-16 整理时 33600 种一字不差） |
  | Kindle 上的样子：没写行高的倍数 `LH_EM`、行高下限、百分比折页宽 32em、空段折叠系数；全书正文行距 `base_line_height`、行高归一 `line_height_ratio`、正文字体 `body_font`；竖直长度换根 em `vertical_rem`；全书样式表 `book_css`；一章的元素表 `Chapter`（层叠只写一遍，`<body>` 里的元素按文档序编号，`matches_tags` 对上标签扫描才改源码）。KFX 写出器和掌阅、Move 的 `kindle_rules` 系列 pass 共用（2026-10-13 从写出器和 7 个 pass 收拢，产物逐字节不变） | `bookconv::kindle_model`（新的照 Kindle 改写的 pass 一律从 `Chapter` 起步，别再自己走 DOM、抄常量） |
  | 元素层叠出来的声明、来源（属性名 → 值；短键用快速哈希） | `csscascade::Decls`、`Origins`，一次算两张用 `Sheet::cascade_split`（2026-10-15） |
  | scraper 元素上查我们自己加的 `data-eink-*` 属性 | `kindle_model::attr_of`（scraper 的 `attr()` 对不在静态表里的名字要拿全局锁，多线程反而变慢） |
  | `kindle_rules` 的一步：全书样式表 + 逐章并行分析；新写的样式表、垫图放哪 | `wash::scan_chapters`；`wash::beside_opf`；新的一步加进步骤表 `KINDLE_RULE_PASSES`（写明开关条件和 `after`） |
  | 一轮生成的调度（设备没接上跳过、排满推迟、`--watch` 记住失败），结果报成事件 | `library::build_all` + `BuildEvent`（措辞在命令行 `booklib`，库里不拼提示文字） |
  | 指纹里设备那几段 | `library/src/fingerprint.rs`（改了跑 `cargo test -p library --test fp_golden`） |
  | 联网请求的失败分类（网站说没有 / 临时的 / 别的） | `library::net::NetError`（临时的自动记进本轮的 `transient_error`） |
  | 读写样式表的规则（层叠、清洗层、优化器共用一套解析：引号、注释、转义按 CSS 规范，`@media` 里的也给出、条件由调用方定，位置是原文的，改写只换规则体） | `csscascade::rule_spans`；判断选择器先过 `csscascade::rule_selector`（去注释和开头的 `@charset`/`@import` 语句）（旧正则 `wash::css_rule_re` 2026-10-10 删了） |
  | CSS 文本：去注释、按分隔符拆、按顶层空白切词、四值简写、`background` 简写切词（2026-10-10 审计从成对重复收成一份） | `csscascade::{strip_comments, split_top, tokens, box_sides, background_tokens}`（底下是同一个扫描器，CSS 字符串遇换行结束） |
  | `<style>` 块（开标签按标签扫、内容按原样文字到 `</style` 为止） | `html::style_blocks`、改写用 `html::edit_style_blocks`（`html::style_block_re` 只剩 `optimize::streaming` 在用） |
  | Python 工具：命令行参数；EPUB 属性（单双引号）、OPF 定位；回归目录配对、spine 解析；读 KFX、走 storyline；EPUB 文字块的样式 | `tools/toollib.py`（`cli_args`/`cli_opts`、`attr`、`opf_path`，全部脚本共用）；`tools/regress/regresslib.py`（compare、tocchk、pair 共用）；`tools/kfx/kfx.py` 的 `load` 调 `kfx-dump --json`（Python 不再自己解 Ion，2026-10-09），`by_type`/`pools`/`reading_order`/`text_nodes` 按阅读顺序走 storyline（pair、s2kcmp、tree 共用，2026-10-10）；样式用 `epub-to-kfx --styles`（s2kdev），不在 Python 里算 CSS |
- **不可信输入不能让进程崩溃**：书的字节全是外来数据，数值相加用 `checked_add`、切片用 `get`；图片解码器 panic 由 `imgopt::guard` 兜住。**读外来数据设上限，超过就报错、不截断照用**（读用 `util::read_capped`）：
  - zip 条目解压 `epubzip::MAX_ENTRY_BYTES`（256MB，EPUB 与 CBZ 共用）；
  - 远程图下载 `netimg::MAX_IMAGE_BYTES`（20MB，超过算抓不到）；
  - 网址入库的网页 20MB（`article.rs`）；
  - CBZ 页面图片解压后合计 `convert::cbz::MAX_CBZ_TOTAL_BYTES`（4GiB）。

  图片 worker 每件活整个兜住 panic（读 zip 也是外部输入），panic 了只这本书报错，不摔掉整个 `sync`。
- `produce_then_replace` 产出途中出错或 panic 都删掉临时文件。
- **踩到的阅读器怪癖**记进[设备 · 怪癖 → 字段](devices.md#怪癖--字段)；用户定的事记进[决定记录](decisions.md)。

## 性能

2026-10-07 一轮提速（产物逐字节不变：38 本真书 × 三种模式的 EPUB 和 kindle 的 KFX 共 152 个产物与改动前逐字节相同）。
24 核电脑、release、`epub-optimize --device=…`（KFX 是 `epub-to-kfx`），耗时秒 / 峰值内存 MB：

| 样本 | 模式 | 改前 | 改后 |
|---|---|---|---|
| 金庸作品全集（122MB） | kindle / ireader / xochitl | 2.04/97 · 2.13/77 · 2.44/122 | 0.69/91 · 0.60/90 · 0.94/110 |
| 阿加莎全集（27MB，2410 个文件） | kindle / ireader / xochitl | 3.20/253 · 3.13/236 · 2.88/242 | 1.33/249 · 1.28/234 · 1.14/249 |
| 杀死一只知更鸟插图版（78MB） | kindle / ireader / xochitl | 1.45/142 · 1.45/151 · 1.69/184 | 1.07/115 · 1.03/123 · 1.28/173 |
| 绍宋（30MB） | kindle / ireader / xochitl | 0.81/309 · 0.79/241 · 0.71/259 | 0.45/327 · 0.44/256 · 0.39/249 |
| 福尔摩斯全集（23MB） | kindle / ireader / xochitl | 0.49/26 · 0.47/26 · 0.45/26 | 0.26/42 · 0.26/44 · 0.23/42 |
| 死亡筆記 卷01（280MB） | kindle / ireader / xochitl | 2.79/414 · 2.86/413 · 2.72/350 | 2.10/379 · 2.12/379 · 2.17/347 |
| 北斗之拳 卷01（287MB） | kindle / ireader / xochitl | 2.26/220 · 2.24/207 · 2.2–6.2/268 | 1.67/213 · 1.64/214 · 1.74/224 |
| 哆啦A夢 卷01（87MB，彩色） | kindle / ireader / xochitl | 2.40/136 · 2.33/148 · 2.24/194 | 0.79/128 · 0.78/130 · 1.06/159 |
| KFX：金庸 / 阿加莎 / 死亡筆記 / 北斗 | kindle | 0.72/417 · 1.29/469 · 0.36/421 · 0.48/479 | 0.54/404 · 0.89/419 · 0.37/218 · 0.39/246 |

- 漫画：JPEG 直接编（省掉按通用表编一遍、解回一遍）、彩色转灰度查表，每页 CPU 少约 25%；彩色漫画转灰度（哆啦A夢）快 3 倍。
  黑白漫画受并行像素额度和 8 个 worker 限制，墙钟少约 25%。
- 文字书：清洗层逐文件步骤多线程、deflate 交给 worker、读骨架多线程、目录驱动定章节去掉了按条目重扫全文件的平方级查找。
- KFX：各文档多线程解析（解析结果在主线程复制一份，不然 worker 分配区里的空洞让峰值涨三成）；容器写出边拷边放，漫画峰值减半。
- 内存：多线程时 glibc 每个线程一个分配区，小书峰值多十来 MB（福尔摩斯 26 → 42MB，`MALLOC_ARENA_MAX=1` 时 27MB）；其余持平或更低。

2026-10-09 全系统审计又一轮（EPUB 产物逐字节不变；KFX 只有写出器 15 修的几本变了）：

- 图片 worker、`par_map` 的线程上限 8 → 16（`imgpool::MAX_WORKERS`；漫画 1/4/8/12/16/24 线程墙钟 5.19/1.53/0.94/0.89/0.75/0.79 秒，同时解码的像素仍受额度约束）。
- 质量门（`check`）每个文件并行扫一遍（以前串行扫三遍）：金庸全集优化加质量门 801 → 569ms。
- KFX：对比度调整按颜色对缓存、样式去重复用编码缓冲、Ion 编码少分配、分析三遍合一遍：金庸 0.58 → 0.51s，阿加莎 0.98 → 0.73s。
- 传书：MTP 上直接写正式文件名（jmtpfs 的改名是整份下载再上传，以前经临时文件改名等于每本传三遍），见[架构 · 生成](architecture.md#生成sync-的第二步generaters)。

2026-10-15 又一轮（EPUB、KFX 产物逐字节不变：三台 29 本文字书 + 1 卷漫画，加 kindle 的 KFX，共 120 个）。全部 48 本真书（29 本文字书 + 19 卷漫画）× 三台 + KFX 的墙钟合计 148.9 → 104.6 秒：

| 样本 | 模式 | 改前 | 改后 |
|---|---|---|---|
| 文字书合计（29 本） | ireader / xochitl / kindle | 17.5 · 19.4 · 4.6 | 10.1 · 11.9 · 4.2 |
| 漫画合计（19 卷） | ireader / xochitl / kindle | 33.2 · 30.8 · 33.1 | 23.1 · 21.9 · 22.8 |
| 古龙作品全集（50MB HTML，1506 个章节文件） | ireader / xochitl | 6.09 · 6.65 | 2.48 · 3.00 |
| 死亡筆記 卷04（374 页，每页 418 万像素） | ireader | 3.03 | 1.98 |

- **多线程反而变慢的锁**：scraper 的 `Element::attr(名字)` 每次都把名字转成 `LocalName`，不在 html5ever 静态表里的名字（我们的
  `data-eink-div`、`data-eink-fs`）要进全局动态字符串表，一把全局锁。`reader_tag` 几乎每个元素每步都查一次，24 个线程全挤在锁上：
  《古龙》`box_fix` 单线程 1.5 秒、24 线程 2.9 秒。改用 `kindle_model::attr_of`（按字符串比，判定同 `attr()`，有单测）后 0.13 秒，
  别的步骤也快一倍以上。**查自己加的属性一律用 `attr_of`，别用 scraper 的 `attr()`**（静态表里的 `id`、`href` 这些没有锁，无所谓）。
  scraper 的 `classes()` 第一次也要进那张表，但层叠匹配类选择器时已经调过、结果缓存了，清洗层里再调不再拿锁。
- **漫画并行额度**：`imgpool::PIXEL_BUDGET` 3600 万 → 7200 万像素（实测数据见常量的文档）：以前按「典型漫画页 100–200 万像素」定，
  《死亡筆記》每页 418 万，只够 8 张同时做；峰值内存不变（581 → 583MB）。
- **DOM、层叠结果跨步骤缓存：评估后不做**（同日，数据见下）。`kindle_rules` 的十几步每步都把每章重新解析、重算层叠，但每步都改书：
  《古龙》`kindle_rules`、`blank_fold`、`block_style`、`top_spacer`、`box_fix` 各改了 88%–100% 的章，样式表也跟着变（任何一章链着的
  样式表变了，它的层叠就作废），《梁羽生》《绍宋》同样，能命中的只有改书前后紧挨着的一两步；`Chapter` 里 scraper 的 `Html` 和
  `Rc<Computed>` 又不能跨线程，缓存要自己管线程归属。收益小、风险大，改成让每次层叠本身变快（下一条）。
- **层叠提速**（同日；单线程《古龙》Move 15.5 → 12.4 秒，24 核总耗时 104.6 → 102.3 秒——24 核下大书的墙钟主要卡在各步写回的
  串行段，见最后一条）：
  - 规则按选择器最右一节分桶（`csscascade::RuleIndex`）：以前每个元素把每条规则都预筛一遍，《古龙》433 万次元素层叠测了 2.68 亿条
    规则、只有 2.5% 过预筛。分桶只缩小要试的范围，候选照旧预筛 + 完整匹配（单测 `prefilter_is_only_a_necessary_condition`：分桶加预筛和全部逐条完整匹配，逐元素比）。
  - 层叠结果的表（`csscascade::Decls`、`Origins`）用快速哈希 `bookbase::util::FastHasher`（FxHash 的算法）：换算计算值
    （`Computed::derive`）每个元素查几十次属性，SipHash 占了一半（2.2 → 1.1 秒）。不影响产物：标准库 `HashMap` 的遍历顺序本来就
    每次运行不同。
  - `Sheet::cascade_split` 层叠时一次填好声明表和来源表（以前先建一张再拆成两张）。
  - `block_style` 写回各章并行（先串行定好每章改什么，再 `par_map_mut`；只有改动计数要加总）。
- **串行段并行化**（同一天再一轮；文字书 24 核合计掌阅 9.4 → 8.7 秒、Move 11.0 → 10.1 秒，《古龙》Move 3.09 → 2.68 秒）：
  - 写回要按章节顺序编类号的几步改成三段——各章并行算好编辑（只读，要编号的先记「登记了什么」）、串行按章的顺序、章里登记的
    先后编号（和以前逐章做时一样，产物逐字节不变）、各章并行改写：`box_fix`（块的类、包框表格的类共用一套号，横线图一套号；横线图
    在找闭合标签**之前**登记，找不到闭合标签的不插图但占号，照旧）、`blank_fold`、`inline_css`（以前整步串行）。`top_spacer` 的类名
    由高算出来、不用编号，直接并行。`block_style` 的写回、`body_only_classes`、`DivGuard::of` 逐章并行。
  - `empty_pages` 判空页的捷径：全书样式表没有 `display:none`、这章原文没有 `display` 时没有元素是隐藏的，DOM 里有字就不是空页，
    不用层叠（0.18 → 0.02 秒；单测 `kindle_empty_pages_shortcut_agrees_with_full_check`）。
  - 步骤表常驻计时：设环境变量 `EINK_PASS_TIMING`（值不限），`epub-optimize` 每步往标准错误打 `pass\t步骤名\t秒`。
- **还没做**：`kindle_rules`（大书约 0.5 秒）里查露出来的字体（`exposed_fonts`）要在改写后的书上整本重算一遍层叠（约 0.24 秒）：
  删哪些声明取决于第一遍统计出的正文字体，没法和第一遍合成一遍；共用的样式表一改所有章都受影响，只重算受影响的章也省不下。
  其余各步的时间主要在逐章解析和层叠（已并行），24 核约快 10 倍，再往上要减少 DOM、字符串的分配。
  查热点：`EINK_PASS_TIMING=1` 看每步；要分开看逐章分析和串行段，在 `wash::scan_chapters` 里临时计时；`taskset -c 0` 跑单线程对照。

## DRM

用途是给用户自己买的书做格式转换和个人备份。现状：入库时识别出 DRM 就拒收。

**2026-10-16 用户定不做了**（不是暂停，以后不重启；识别出 DRM 照旧拒收）。下面是 2026-09-27 暂停时的情况，留作记录。

**2026-09-27 暂停**。用户手上只有 Kindle 旧格式（AZW/AZW3/MOBI）的加密书，但这种 DRM 找不到足够的独立公开资料：

- 维基百科只有 PC1 密码本身；
- 一篇 2008 年的博客只讲了流程，没有密钥常量、从序列号算 PID 的方法和凭证结构；
- 细节齐全的只有一份照 DeDRM 的 GPL 代码写的讲解，照它实现等于间接移植 GPL 代码。

而且入库只收 EPUB、CBZ，解了 DRM 也还得先转成 EPUB。重启前要用户在几条路里选一条：先确认手上的文件是旧格式还是 KFX（KFX 的 DRM 不考虑；DRM-free 的 KFX 写出器见 [KFX](kfx.md)）；把 DRM 单独做成 GPL 模块；或交给外部工具解完、转成 EPUB 再入库。

## 参考资料

算法和规则的历史依据在上游项目的白皮书里。上游仓库的本机位置不写进本仓库；上游文档里的类名、文件名是旧前缀，对照时换成 `eink-`。
