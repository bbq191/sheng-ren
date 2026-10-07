# 开发

## 常用命令

```sh
cargo build --workspace
cargo test --workspace                      # 全部测试，要求全部通过（2026-10-07：506 个）
cargo test -p bookconv <测试名子串>          # 只跑名字匹配的
cargo clippy --workspace --all-targets      # 要求 0 警告

tools/regress/run.sh <epub-optimize> 目录 [--device=…]   # 真书回归（见下）
tools/regress/compare.py 旧目录 新目录                    # 比较两次回归
tools/regress/tocchk.py 旧目录 新目录                     # 核对 spine 文件数（不拆文件）和目录（层级、标签、锚点都在）
python3 tools/kf8/textcheck.py 书.azw3 优化后.epub         # AZW3 与源 EPUB 可见文字逐字比
shellcheck -x install.sh uninstall.sh tools/cargo-pkgs.sh xochitl/comic-margins.sh tools/regress/run.sh

./install.sh --tools                        # 装进 PATH 手工试
```

书库的集成测试在 `crates/library/tests/`：`flow.rs` 测入库、同步、生成、挪位置、删书、传设备；`cli.rs` 直接运行 `booklib`，测参数、报错和退出码。都不联网、**不碰真设备**：`tests/common` 把 MTP 挂载目录指到 `<书库>-dev/`（下面造 kindle、ireader 的存储目录），Move 不走 SSH（`BOOKLIB_NO_SSH=1`），要测 Move 的用 `FakeMove`（本机起一个只实现导入接口的假书架服务）。

## 真书回归

测试用真书在 `~/Documents/ereader/books/`（路径里带"漫画"的是漫画，其余是文字书）。**只读，绝不改动**：产物一律写到别处，别用 `epub-optimize` 的"输入输出同路径"。用户会自己整理这个目录，别假设书还在上次的位置。

改动前的提交构建一份 `epub-optimize`，和改动后的各跑一遍，然后比较：

```sh
git worktree add target/regress-base HEAD && (cd target/regress-base && cargo build --release -p bookconv --bin epub-optimize)
tools/regress/run.sh target/regress-base/target/release/epub-optimize 旧   # 缺省 --device=ireader；放 target/ 下，/tmp 是内存盘
cargo build --release -p bookconv --bin epub-optimize
tools/regress/run.sh target/release/epub-optimize 新
tools/regress/compare.py 旧 新
```

- `run.sh` 把全部文字书和第一卷漫画各优化一遍；开跑前核对 `epub-optimize` 是可执行文件、书目录在（`$REGRESS_BOOKS`，缺省 `~/Documents/ereader/books`），清掉输出目录里的旧产物（免得没生成的书拿旧文件去比、悄悄通过），一本书都没找到就报错退出。
- `compare.py` 按原书路径配对新旧产物，每本报一行：`SAME`（每个 zip 条目逐字节相同；先比条目名、大小、CRC，一样再比字节）、`TEXT-SAME`（有条目变了，可见文字一样）、`TEXT-DIFF`（可见文字变了，打印第一处不同）、`MISSING`/`NEW`（只有一边有）、`BROKEN`（产物读不出来：不是 zip、没有 `container.xml` 等）。优化器版本一变，每本的 `META-INF/eink-optimized` 都会变，所以升版本后全是 `TEXT-SAME`，要逐条目比才看得出哪些书真变了。
- `compare.py` 另外核对几件事，不过就退出码 1：新产物里不合法的 XML 不比旧的多；**正文图片（`<img>`、SVG `<image>`）不比原书少**（字符账只管文字，只有图的页被删了它看不出来；只有图、没有文字的注释号链接里的图不算，它们按规则换成数字）；新产物的可见文字和**原书**逐字符对账（按字符计数、不管顺序——注释会挪到章末——一个不多一个不少）。原书不在了、读不出来都报"未核对"，也算问题。字符账不计空白、U+FEFF 和控制字符（《金庸全集》原书一个损坏的 U+0010 被规范整理去掉，不算少字）。
- 字符账唯一的例外是用户定的"只有图标的注释号换成数字"：只多出 ASCII 数字、原书里有只含图片的链接、多出的位数不超过按图标个数连续编号的位数时，只注明、不算不平（以前春雪、雪国、飘上下册、绍宋、绝叫 6 本因此一直报不平）。

| 改动的性质 | 要求 |
|---|---|
| 重构、提速、修不影响产物的问题 | EPUB 产物逐个 zip 条目字节相同（EPUB 输出是确定的） |
| 改了会影响正文的规则 | 可见文字改动前后一致；spine 文件数和原书一样（不拆文件，原有的空页清理除外）；目录（NCX）条目数、层级、标签前后一致，每条的目标文件和锚点都在（`tocchk.py`）；有变化的逐条说明原因 |
| 任何改动 | 输出的 XHTML、OPF、NCX 都是合法 XML（现在是 0 个不合法） |
| 改了漫画处理 | 拿一卷漫画比图片字节 |

**跑哪些模式**：缺省只跑 `ireader`。`kindle` 的文字书和它差在阅读范围、注释写法（`jump` 对 `popup`）、背景图尺寸（kindle 保留 `background-size`/`fixed`），而且最后还要转一步 KFX（见下面 [KFX](#kfx)）；漫画还有固定版式（`comic_fixed_layout`）和翻页方向（`comic_page_direction`）的不同。`xochitl` 还有彩色、去掉回链和背景图、页边距 1 的不同。改了注释、背景、漫画、彩色、阅读范围相关的处理，对应的模式也跑一遍。

### KFX

KFX 没有逐字比文字的工具，靠"新旧写出器的产物逐字节比"（KFX 输出也是确定的：唯一 ID 由 OPF 标识符派生，书里不写写出器版本）：

```sh
# 优化器产物用 --device=kindle 的回归目录（上面 run.sh 加 --device=kindle）
(cd target/regress-base && cargo build --release -p kfx --bin epub-to-kfx)
cargo build --release -p kfx --bin epub-to-kfx
mkdir -p target/kfx-old target/kfx-new
for f in 新/*.epub; do n=$(basename "$f" .epub)
  target/regress-base/target/release/epub-to-kfx "$f" "target/kfx-old/$n.kfx" >/dev/null
  target/release/epub-to-kfx "$f" "target/kfx-new/$n.kfx" >/dev/null
  cmp -s "target/kfx-old/$n.kfx" "target/kfx-new/$n.kfx" || echo "变了：$n"
done
```

| 改动的性质 | 要求 |
|---|---|
| 重构、提速写出器 | 同一份优化后 EPUB，新旧写出器的 KFX 逐字节相同（2026-10-06 提速省内存那次：《北斗之拳》卷01 峰值 1.45→0.73GB，金庸全集 0.87→0.56GB、1.7→0.7 秒，阿加莎全集 3.3→1.4 秒，产物逐字节不变） |
| 改了写出器的行为 | 只有该变的书变了，逐本说明（比如写出器 7 只有《福尔摩斯探案全集》变；写出器 8 同一个 `--id` 下全部逐字节不变，书库产物因唯一 ID 改取书 id 全变）；`WRITER_VERSION` 加一；上真机看过再写 ✓ |
| 改了容器读写（`ion`、`container`） | `kfx-repack` 解开再打包设备上的样本（`target/kfx-samples/` 等）全部逐字节相同 |

改了优化器而 KFX 字节变了的书，覆盖到 Kindle 上会丢进度（见[设备 · 重拷书以后进度还在不在](devices.md#重拷书以后进度还在不在)），值得在提交说明里写清是哪些书。

### AZW3（书库已不用）

改了 `crates/azw3` 后，把 `--device=kindle` 的回归产物逐本转成 AZW3，用 `textcheck.py` 和对应的优化后 EPUB 比，全部要 `SAME`（2026-09-30：28 本文字书和一卷漫画全部 `SAME`，含 134MB 的《金庸作品全集》）。只重构写出器时 AZW3 应逐字节不变（唯一 ID 和时间都是确定的）。

## 版本号

改了会影响产物的代码，把对应的版本号加一，书库据此把旧产物判为过期（每个版本号进指纹的哪一段，见[架构 · 指纹](architecture.md#指纹)）。**现值只写在这张表里**，别处不再写：

| 改了什么 | 版本号 | 现值 | 过期的书 |
|---|---|---|---|
| 清洗、优化、图片处理 | `bookconv::optimize::OPTIMIZE_VERSION`（附一行变更说明） | 51 | 全部 |
| CBZ → EPUB 的转换 | `bookconv::convert::CONVERT_VERSION`（附一行变更说明） | 2 | 只有 CBZ 来源的 |
| 生成时往书里补封面、简介、标签 | `bookconv::opfmeta::VERSION` | 5 | 只有补过东西的 |
| EPUB → AZW3 | `azw3::WRITER_VERSION` | 4 | 书库已不出 AZW3（`epub-to-azw3` 还在） |
| EPUB → KFX | `kfx::write::WRITER_VERSION` | 9 | 只有 `kindle` 模式的 |
| 书库生成流程本身 | `library` 的 `PIPELINE_VERSION`（慎用） | 5 | 全部 |

## 工程约束

用户定的硬约束（只用 Rust、不用 Calibre，clean-room，命名，内容一字不改……）见[决定记录 · 一直有效的约束](decisions.md#一直有效的约束)。写代码时另外要守的：

- **不写死屏幕数字**：一律从模式读。优化选项用 `OptimizeOpts::for_profile(模式)` 创建（书库和 `epub-optimize` 同一个起点）；测试里才用 `OptimizeOpts::new(阅读范围)`。不按设备名写分支，阅读器的怪癖做成模式字段。
- **真机验证后才能说完成**：改变阅读效果的规则，没在目标设备上看过就不写"✓"；一台验证过不能推到别的；电脑上的检查不能代替真机。结果写进[验证情况](typesetting.md#验证情况)。
- **共用的轮子别再各写一份**（2026-09-30 全系统审计收拢过）：

  | 要做的事 | 用这个 |
  |---|---|
  | HTML 操作（容错单双引号、注释、不把 `data-id` 当 `id`；别写只认双引号的正则） | `bookconv::html`；加类 `html::add_class`；不分大小写查找 `html::{find_ci, contains_ci}` |
  | 写 EPUB / 读条目 / 读整本（元数据、spine、CSS、图片、目录） | `epubzip::EpubWriter` / `read_entries_from` / `epubbook::load`（AZW3、KFX 共用；从文件读用 `epubbook::load_from`，不整本读进内存） |
  | manifest 路径；书里链接解析（还原字符引用 → 拆锚点 → 百分号解码 → 规整路径） | `ManifestItem::path`；`epubzip::resolve_link` |
  | 原子写文件（书库也用） | `util` 的 `produce_then_replace`、`commit` |
  | DRM 判定；全角转半角；图片格式识别；哈希 | `wash::encrypted_targets`；`util::to_halfwidth`；`util::image_kind`；`util::fnv64` |
  | `@font-face` 规则匹配（清洗层、AZW3 写出器共用） | `wash::font_face_re` |
  | install / uninstall 共用的包列表和路径 | `tools/cargo-pkgs.sh` |
  | 逐文件独立的步骤多线程做（结果按原顺序；`f` 必须是纯的） | `util::par_map`（只读）/ `par_map_mut`（就地改） |
  | 在别的线程先压好一个 zip 条目，再按顺序写 | `epubzip::Precompressed` + `EpubWriter::put_precompressed` |
- **不可信输入不能让进程崩溃**：书的字节全是外来数据，数值相加用 `checked_add`、切片用 `get`；图片解码器 panic 由 `imgopt::guard` 兜住。**读外来数据设上限，超过就报错、不截断照用**（读用 `util::read_capped`）：
  - zip 条目解压 `epubzip::MAX_ENTRY_BYTES`（256MB，EPUB 与 CBZ 共用）；
  - 远程图下载 `netimg::MAX_IMAGE_BYTES`（20MB，超过算抓不到）；
  - 网址入库的网页 20MB（`article.rs`）。
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

## DRM

用途是给用户自己买的书做格式转换和个人备份。现状：入库时识别出 DRM 就拒收。

**2026-09-27 暂停**。用户手上只有 Kindle 旧格式（AZW/AZW3/MOBI）的加密书，但这种 DRM 找不到足够的独立公开资料：

- 维基百科只有 PC1 密码本身；
- 一篇 2008 年的博客只讲了流程，没有密钥常量、从序列号算 PID 的方法和凭证结构；
- 细节齐全的只有一份照 DeDRM 的 GPL 代码写的讲解，照它实现等于间接移植 GPL 代码。

而且入库只收 EPUB、CBZ，解了 DRM 也还得先转成 EPUB。重启前要用户在几条路里选一条：先确认手上的文件是旧格式还是 KFX（KFX 的 DRM 不考虑；DRM-free 的 KFX 写出器见 [KFX](kfx.md)）；把 DRM 单独做成 GPL 模块；或交给外部工具解完、转成 EPUB 再入库。

## 参考资料

算法和规则的历史依据在上游项目的白皮书里。上游仓库的本机位置不写进本仓库；上游文档里的类名、文件名是旧前缀，对照时换成 `eink-`。
