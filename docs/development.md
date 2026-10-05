# 开发

## 常用命令

```sh
cargo build --workspace
cargo test --workspace                      # 全部测试，要求全部通过（2026-10-06：433 个）
cargo test -p bookconv <测试名子串>          # 只跑名字匹配的
cargo clippy --workspace --all-targets      # 要求 0 警告

tools/regress/run.sh <epub-optimize> 目录 [--device=…]   # 真书回归（见下）
tools/regress/compare.py 旧目录 新目录                    # 比较两次回归
python3 tools/kf8/textcheck.py 书.azw3 优化后.epub         # AZW3 与源 EPUB 可见文字逐字比
shellcheck -x install.sh uninstall.sh tools/cargo-pkgs.sh xochitl/*.sh tools/regress/run.sh

./install.sh --tools                        # 装进 PATH 手工试
```

书库的集成测试在 `crates/library/tests/`：`flow.rs` 测入库、同步、生成、挪位置、删书；`cli.rs` 直接运行 `booklib`，测参数、报错和退出码。都不联网。

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

- `run.sh` 把全部文字书和第一卷漫画各优化一遍；开跑前清掉输出目录里的旧产物（免得没生成的书拿旧文件去比、悄悄通过），一本书都没找到就报错退出。
- `compare.py` 按原书路径配对新旧产物，每本报一行：`SAME`（每个 zip 条目逐字节相同）、`TEXT-SAME`（有条目变了，可见文字一样）、`TEXT-DIFF`（可见文字变了，打印第一处不同）、`MISSING`/`NEW`（只有一边有）。优化器版本一变，每本的 `META-INF/eink-optimized` 都会变，所以升版本后全是 `TEXT-SAME`，要逐条目比才看得出哪些书真变了。
- `compare.py` 另外核对两件事，不过就退出码 1：新产物里不合法的 XML 不比旧的多；新产物的可见文字和**原书**逐字符对账（按字符计数、不管顺序——注释会挪到章末——一个不多一个不少）。原书不在了也算问题。字符账不计空白、U+FEFF 和控制字符（《金庸全集》原书一个损坏的 U+0010 被规范整理去掉，不算少字）。

| 改动的性质 | 要求 |
|---|---|
| 重构、提速、修不影响产物的问题 | EPUB 产物逐个 zip 条目字节相同（EPUB 输出是确定的） |
| 改了会影响正文的规则 | 可见文字改动前后一致；带分页和加 `--no-paginate` 一致；有变化的逐条说明原因 |
| 任何改动 | 输出的 XHTML、OPF、NCX 都是合法 XML（现在是 0 个不合法） |
| 改了漫画处理 | 拿一卷漫画比图片字节 |

**跑哪些模式**：缺省只跑 `ireader`。`kindle` 和它**文字书只差阅读范围**；漫画还有固定版式（`comic_fixed_layout`）和翻页方向（`comic_page_direction`）的不同，`xochitl` 还有彩色、注释回链、页边距 1 的不同。改了漫画、彩色、阅读范围、注释相关的处理，对应的模式也跑一遍。

**AZW3**：改了 `crates/azw3` 或会影响正文的规则后，把 `--device=kindle` 的回归产物逐本转成 AZW3，用 `textcheck.py` 和对应的优化后 EPUB 比，全部要 `SAME`（2026-09-30：28 本文字书和一卷漫画全部 `SAME`，含 134MB 的《金庸作品全集》）。只重构写出器时 AZW3 应逐字节不变（唯一 ID 和时间都是确定的）。

## 版本号

改了会影响产物的代码，把对应的版本号加一，书库据此把旧产物判为过期（每个版本号进指纹的哪一段，见[架构 · 指纹](architecture.md#指纹)）。**现值只写在这张表里**，别处不再写：

| 改了什么 | 版本号 | 现值 | 过期的书 |
|---|---|---|---|
| 清洗、优化、图片处理 | `bookconv::optimize::OPTIMIZE_VERSION`（附一行变更说明） | 39 | 全部 |
| CBZ → EPUB 的转换 | `bookconv::convert::CONVERT_VERSION`（附一行变更说明） | 2 | 只有 CBZ 来源的 |
| 生成时往书里补封面、简介、标签 | `bookconv::opfmeta::VERSION` | 4 | 只有补过东西的 |
| EPUB → AZW3 | `azw3::WRITER_VERSION` | 4 | 书库已不出 AZW3（`epub-to-azw3` 还在） |
| EPUB → KFX | `kfx::write::WRITER_VERSION` | 4 | 只有 `kindle` 模式的 |
| 书库生成流程本身 | `library` 的 `PIPELINE_VERSION`（慎用） | 5 | 全部 |

## 工程约束

- **只用 Rust，不用 Calibre**（包括 `ebook-convert`、DeDRM、KFX 插件）。
- **clean-room**：解 DRM（以后重启时）和 Kindle 格式的读写，只照公开的格式说明和对样本的黑盒分析实现，不读不移植 GPL 代码（DeDRM_tools、KFX Input、KindleUnpack、Calibre）。Koodo Reader 是 AGPL-3.0：只借鉴它用哪些数据源。许可证事实要下载 LICENSE 确认。
- **命名**：命令按功能命名。写进书里的 CSS 类用 `eink-` 前缀，样式表叫 `eink-wash.css`，优化标记 `META-INF/eink-optimized`。
- **不写死屏幕数字**：一律从模式读。优化选项用 `OptimizeOpts::for_profile(模式)` 创建（书库和 `epub-optimize` 同一个起点）；测试里才用 `OptimizeOpts::new(阅读范围)`。
- **真机验证后才能说完成**：改变阅读效果的规则，没在目标设备上看过就不写"✓"；一台验证过不能推到别的；电脑上的检查不能代替真机。结果写进[验证情况](typesetting.md#验证情况)。
- **共用的轮子别再各写一份**：HTML 操作用 `bookconv::html`（容错单双引号、注释、不把 `data-id` 当 `id`），不写只认双引号的正则；manifest 路径用 `ManifestItem::path`；写 EPUB 用 `epubzip::EpubWriter`；写文件用 `util` 的原子写；DRM 判定用 `wash::encrypted_targets`。
- **不可信输入不能让进程崩溃**：书的字节全是外来数据，数值相加用 `checked_add`、切片用 `get`，解压设上限。
- **踩到的阅读器怪癖**记进[设备 · 怪癖 → 字段](devices.md#怪癖--字段)；用户定的事记进[决定记录](decisions.md)。

## DRM

用途是给用户自己买的书做格式转换和个人备份。现状：入库时识别出 DRM 就拒收。

**2026-09-27 暂停**。用户手上只有 Kindle 旧格式（AZW/AZW3/MOBI）的加密书，但这种 DRM 找不到足够的独立公开资料：

- 维基百科只有 PC1 密码本身；
- 一篇 2008 年的博客只讲了流程，没有密钥常量、从序列号算 PID 的方法和凭证结构；
- 细节齐全的只有一份照 DeDRM 的 GPL 代码写的讲解，照它实现等于间接移植 GPL 代码。

而且入库只收 EPUB、CBZ，解了 DRM 也还得先转成 EPUB。重启前要用户在几条路里选一条：先确认手上的文件是旧格式还是 KFX（KFX 的 DRM 不考虑；DRM-free 的 KFX 写出器见 [KFX](kfx.md)）；把 DRM 单独做成 GPL 模块；或交给外部工具解完、转成 EPUB 再入库。

## 参考资料

算法和规则的历史依据在上游项目的白皮书里。上游仓库的本机位置不写进本仓库；上游文档里的类名、文件名是旧前缀，对照时换成 `eink-`。
