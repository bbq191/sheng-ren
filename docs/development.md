# 开发

## 常用命令

```sh
cargo build --workspace
cargo test --workspace                      # 全部测试
cargo test -p bookconv <测试名子串>          # 只跑名字匹配的测试
cargo clippy --workspace --all-targets      # 要求没有警告

python3 tools/kf8/dump.py 文件.azw3          # 看 KF8 结构
python3 tools/kf8/indexes.py 文件.azw3       # 看片段/骨架/目录索引
python3 tools/kf8/textcheck.py 文件.azw3 源.epub   # AZW3 与源 EPUB 的正文按阅读顺序逐字核对

koreader/check.sh                            # KOReader 配置离线检查（改了 koreader/ 下的文件后跑；不带参数查全部设备）
shellcheck -x koreader/*.sh install.sh uninstall.sh

./install.sh --tools                         # 把命令装进 PATH，手工试用
```

## 真书回归检查

测试用真书在 `~/Documents/ereader/books/`（`haodoo/`、`小说/`、`原版小说/`、`收藏/` 是文字书，`漫画/` 是大体积漫画）。**只读，绝不改动**：
产物一律写到临时目录，不要用 `epub-optimize` 的"输入输出同路径"就地覆盖。用户会自己整理这个目录，别假设书还在上次的位置。

先在改动前的提交构建一份 `epub-optimize`、`epub-to-azw3` 另存（比如 `git worktree add` 一个临时目录去编），再和改动后的各跑一遍全部文字书和一卷漫画（设备用 `kindle-pw12-sig`，顺带出 AZW3）。

| 改动的性质 | 要求 |
|---|---|
| 重构、提速、修不影响产物的问题 | EPUB 产物**逐个 zip 条目字节相同**（EPUB 输出是确定的，同样的输入每次都一样） |
| 改了会影响正文的规则 | 每本书 spine 里各 XHTML 的**可见文字**（去标签、还原字符引用后的字符序列）改动前后一致；带分页和加 `--no-paginate` 一致；有变化的逐条说明原因 |
| 任何改动 | 输出的 XHTML、OPF、NCX 都是合法 XML，不合法的数量不比改动前多（v27 起是 0；此前的 85 个来自《绝叫》、金庸全集原书的缺陷，规范整理修掉了） |
| 改了漫画处理 | 拿一卷漫画比图片字节 |
| 改了 AZW3 写出器 | `tools/kf8/textcheck.py` 核对文字；想保持产物不变时，把 PDB 的时间、记录 0 的 uid、EXTH 113/504 的 asin（以及书名没有 ASCII 字符时 PDB 名里的 `book_<uid>`）清零后逐字节比较，不一致就给 `WRITER_VERSION` 加一 |

回归工具在 `tools/regress/`（`run.sh` 跑全部测试书、`compare.py` 比较两次产物：SAME / TEXT-SAME / TEXT-DIFF、不合法 XML 数、对原书的字符账）。
v27 起《金庸全集》对原书的字符账会少一个 U+0010（原书里的损坏控制字符，XML 不允许，规范整理去掉了），这是预期的。

## 版本号

改了会影响产物的代码，要把对应的版本号加一，书库据此把旧产物判为过期：

| 改了什么 | 版本号 |
|---|---|
| 清洗、优化、图片处理 | `bookconv::optimize::OPTIMIZE_VERSION`（附一行变更说明） |
| AZW3 写出 | `azw3::WRITER_VERSION` |
| 格式转换（MOBI/AZW3/FB2/CBZ → EPUB、有文字层的 PDF → EPUB、PDF 写出与裁边） | `bookconv::convert::CONVERT_VERSION`（附一行变更说明）。只进非 EPUB 来源的指纹，EPUB 书不受影响 |
| 书库生成流程本身 | `library` 的 `PIPELINE_VERSION`（所有书都会过期，慎用） |

## 工程约束

- **只用 Rust，不用 Calibre**（包括 `ebook-convert`、DeDRM 插件、KFX Output 插件）。
- **clean-room**：AZW3 写出、KF8/MOBI 读取、以后的解 DRM，都只照公开的格式说明和对样本文件的黑盒分析实现，不读也不移植 GPL 代码（DeDRM_tools、KFX Input、KindleUnpack、Calibre）。许可证事实要下载 LICENSE 文件确认，不凭印象。
- **命名**：命令和二进制按功能命名。写进书里的 CSS 类统一用 `eink-` 前缀（`eink-flush`、`eink-center`），样式表叫 `eink-wash.css`，优化标记是 `META-INF/eink-optimized`。
- **不写死屏幕数字**：一律从 profile 读，调用方传 `Profile::readable(格式)`。`OptimizeOpts` 没有缺省设备，用 `OptimizeOpts::new(阅读范围)` 创建。
- **真机验证后才能说完成**：改变阅读效果的规则，没在目标设备上看过，就不写"已验证"；一台设备上验证过不能推到另外两台。单元测试只能证明逻辑没错。
- **HTML 操作一律用 `bookconv::html`**（容错单双引号、注释、不把 `data-id` 当 `id`），不写只认双引号的正则。
- **不可信输入不能让进程崩溃**：书的字节全是外来数据，数值相加用 `checked_add`、切片用 `get`，解压设上限。第三方解析器（pdf-extract）的 panic 用 `catch_unwind` 接住。
- **以实测为准**：Kindle、掌阅、xochitl 的实测行为，踩到一条就记进[设备与可阅读范围](devices.md#已知的阅读器特性)。

## DRM

用途是给用户自己购买的书做格式转换和个人备份。现状：入库时识别出带 DRM 的书就拒收。

**2026-09-27 暂停**。用户手上只有 Kindle 旧格式（AZW/AZW3/MOBI）的加密书，但 Kindle 旧格式 DRM 找不到足够的独立公开资料：

- 维基百科只有 PC1 密码本身。
- 一篇 2008 年的博客只讲了流程，没有密钥常量、从序列号算 PID 的方法和凭证记录的结构。
- 细节齐全的只有一份照 DeDRM 的 GPL 代码写的讲解，照它实现等于间接移植 GPL 代码。

重启前要在几条路里选一条：先确认手上的文件是旧格式还是 KFX（新设备"下载并通过 USB 传输"拿到的多是 KFX，KFX 暂不考虑）；把 DRM 单独做成 GPL 模块；或者交给外部工具解完再入库。

## 参考资料

算法和规则的历史依据在上游项目的白皮书里（EPUB 优化规范、bookconv 实现细节、已删除的 Python 管线）。上游仓库的本机位置不写进本仓库。上游文档里的类名、文件名是旧前缀，对照时换成 `eink-`。
