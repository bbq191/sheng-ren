# 开发

## 常用命令

```sh
cargo build --workspace
cargo test --workspace                      # 全部测试（要求全部通过）
cargo test -p bookconv <测试名子串>          # 只跑名字匹配的测试
cargo clippy --workspace --all-targets      # 要求没有警告

tools/regress/run.sh 某版/epub-optimize 目录   # 真书回归：全部文字书 + 一卷漫画各优化一遍（见下文）
tools/regress/compare.py 旧目录 新目录          # 比较两次回归的产物
python3 tools/kf8/textcheck.py 书.azw3 优化后.epub   # AZW3 与源 EPUB 的可见文字逐字比对（见 AZW3 写出器）
shellcheck -x install.sh uninstall.sh xochitl/*.sh tools/regress/run.sh

./install.sh --tools                         # 把命令装进 PATH，手工试用
```

书库的集成测试在 `crates/library/tests/`：`flow.rs` 测书库流程（入库、跟踪同步、生成、挪位置、删书），`cli.rs` 直接运行 `booklib`，测参数解析、报错和退出码
（含 `meta --fetch` 不联网的两条路和 `meta --edit` 的查看、改写、空值删除、出错不动原文件），共用的造书和跑命令的函数在 `common/mod.rs`。都不联网。

## 真书回归检查

测试用真书在 `~/Documents/ereader/books/`（路径里带"漫画"的是漫画，其余是文字书）。**只读，绝不改动**：
产物一律写到临时目录，不要用 `epub-optimize` 的"输入输出同路径"就地覆盖。用户会自己整理这个目录，别假设书还在上次的位置。

先在改动前的提交构建一份 `epub-optimize` 另存，再和改动后的各跑一遍，然后比较：

```sh
git worktree add target/regress-base HEAD && (cd target/regress-base && cargo build --release -p bookconv --bin epub-optimize)
tools/regress/run.sh target/regress-base/target/release/epub-optimize 旧   # 缺省 --device=ireader；/tmp 是内存盘，别放那里
cargo build --release -p bookconv --bin epub-optimize
tools/regress/run.sh target/release/epub-optimize 新
tools/regress/compare.py 旧 新                                           # 改了彩色或阅读范围相关的处理，--device=xochitl 也跑一遍
```

`compare.py` 按原书路径配对新旧两次的产物（两边的 `index.txt`；两次之间增删、改名了书也不会错配），每本书报一行：
`SAME`（每个 zip 条目逐字节相同）、`TEXT-SAME`（有条目变了，但可见文字一样）、`TEXT-DIFF`（可见文字变了，打印第一个不同处）、`MISSING`/`NEW`（只有一边有）。
优化器版本号一变，每本书的 `META-INF/eink-optimized` 都跟着变，所以升了版本后全是 `TEXT-SAME`，要逐条目比才看得出哪些书真的变了。
另外核对两件事，不过就算失败、退出码 1：新产物里不合法的 XML 不比旧的多；新产物的可见文字和**原书**逐字符对账（按字符计数，不管顺序——注释会挪到章末——但一个字不多一个字不少）。

| 改动的性质 | 要求 |
|---|---|
| 重构、提速、修不影响产物的问题 | EPUB 产物**逐个 zip 条目字节相同**（EPUB 输出是确定的，同样的输入每次都一样） |
| 改了会影响正文的规则 | 每本书 spine 里各 XHTML 的**可见文字**（去标签、还原字符引用后的字符序列）改动前后一致；带分页和加 `--no-paginate` 一致；有变化的逐条说明原因 |
| 任何改动 | 输出的 XHTML、OPF、NCX 都是合法 XML，不合法的数量不比改动前多（v27 起是 0；此前的 85 个来自《绝叫》、金庸全集原书的缺陷，规范整理修掉了） |
| 改了漫画处理 | 拿一卷漫画比图片字节 |

字符账不计空白、U+FEFF 和控制字符：《金庸全集》原书里有一个损坏的 U+0010（XML 不允许，规范整理去掉了），不算少字。

`kindle` 模式和 `ireader` 用同一套规则，只是阅读范围不同，所以回归缺省只跑 `ireader`；改了漫画的阅读范围相关处理时 `--device=kindle` 也跑一遍。
AZW3 这一步另外核对：改了 `crates/azw3` 或会影响正文的规则后，把 `--device=kindle` 的回归产物逐本转成 AZW3，用 `tools/kf8/textcheck.py` 和对应的优化后 EPUB 比，全部要 `SAME`
（2026-09-30：28 本文字书和一卷漫画全部 `SAME`，包括 134MB 的《金庸作品全集》）。只重构写出器时，AZW3 应逐字节不变：
书库生成时唯一 ID、时间由书 id 和入库时间固定，单独用 `epub-to-azw3` 时由 OPF 标识符和 `dcterms:modified` 派生，都是确定的。

## 版本号

改了会影响产物的代码，要把对应的版本号加一，书库据此把旧产物判为过期：

| 改了什么 | 版本号 |
|---|---|
| 清洗、优化、图片处理 | `bookconv::optimize::OPTIMIZE_VERSION`（附一行变更说明） |
| CBZ → EPUB 的转换 | `bookconv::convert::CONVERT_VERSION`（附一行变更说明）。只进 CBZ 来源的指纹，EPUB 书不受影响 |
| 生成时往书里补封面、简介、标签（`metadata::inject`） | `bookconv::opfmeta::VERSION`。只进补过东西的书的指纹（`i` + 版本） |
| EPUB → AZW3 的转换 | `azw3::WRITER_VERSION`。只进 AZW3 模式（`kindle`）的指纹 |
| 书库生成流程本身 | `library` 的 `PIPELINE_VERSION`（所有书都会过期，慎用） |

## 工程约束

- **只用 Rust，不用 Calibre**（包括 `ebook-convert`、DeDRM 插件、KFX Output 插件）。
- **clean-room**：解 DRM（以后重启时）、Kindle 格式的读写（`crates/azw3`，含读取器 `azw3::read`，见 [AZW3 写出器](azw3.md)），都只照公开的格式说明和对样本文件的黑盒分析实现，不读也不移植 GPL 代码（DeDRM_tools、KFX Input、KindleUnpack、Calibre）。许可证事实要下载 LICENSE 文件确认，不凭印象。
- **命名**：命令和二进制按功能命名。写进书里的 CSS 类统一用 `eink-` 前缀（`eink-flush`、`eink-center`），样式表叫 `eink-wash.css`，优化标记是 `META-INF/eink-optimized`。
- **不写死屏幕数字**：一律从 profile 读。优化选项用 `OptimizeOpts::for_profile(模式)` 创建（书库和 `epub-optimize` 同一个起点）；测试里才用 `OptimizeOpts::new(阅读范围)`。
- **真机验证后才能说完成**：改变阅读效果的规则，没在目标设备上看过，就不写"已验证"；一台设备上验证过不能推到别的设备，电脑上的检查（回归、`textcheck.py`）也不能代替真机。单元测试只能证明逻辑没错。
- **HTML 操作一律用 `bookconv::html`**（容错单双引号、注释、不把 `data-id` 当 `id`），不写只认双引号的正则；AZW3 写出器、质量门、DRM 判定也一样。
  OPF manifest 项的路径用 `ManifestItem::path`（先还原 `&amp;` 再百分号解码），写 EPUB 用 `epubzip::EpubWriter`，写文件用 `util` 的原子写——别再各写一份。
- **不可信输入不能让进程崩溃**：书的字节全是外来数据，数值相加用 `checked_add`、切片用 `get`，解压设上限。
- **以实测为准**：Kindle、掌阅、xochitl 等阅读器的实测行为，踩到一条就记进[设备与可阅读范围](devices.md#已知的阅读器特性)。

## DRM

用途是给用户自己购买的书做格式转换和个人备份。现状：入库时识别出带 DRM 的书就拒收。

**2026-09-27 暂停**。用户手上只有 Kindle 旧格式（AZW/AZW3/MOBI）的加密书，但 Kindle 旧格式 DRM 找不到足够的独立公开资料
（入库只收 EPUB、CBZ，连不加密的 MOBI/AZW3 也不收，解了 DRM 也还得先转成 EPUB）：

- 维基百科只有 PC1 密码本身。
- 一篇 2008 年的博客只讲了流程，没有密钥常量、从序列号算 PID 的方法和凭证记录的结构。
- 细节齐全的只有一份照 DeDRM 的 GPL 代码写的讲解，照它实现等于间接移植 GPL 代码。

重启前要在几条路里选一条：先确认手上的文件是旧格式还是 KFX（新设备"下载并通过 USB 传输"拿到的多是 KFX，KFX 暂不考虑）；把 DRM 单独做成 GPL 模块；或者交给外部工具解完、转成 EPUB 再入库。

## 参考资料

算法和规则的历史依据在上游项目的白皮书里（EPUB 优化规范、bookconv 实现细节、已删除的 Python 管线）。上游仓库的本机位置不写进本仓库。上游文档里的类名、文件名是旧前缀，对照时换成 `eink-`。
