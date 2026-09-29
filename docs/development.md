# 开发

## 常用命令

```sh
cargo build --workspace
cargo test --workspace                      # 全部测试
cargo test -p bookconv <测试名子串>          # 只跑名字匹配的测试
cargo clippy --workspace --all-targets      # 要求没有警告

koreader/check.sh                            # KOReader 配置离线检查（改了 koreader/ 下的文件后跑；不带参数查全部设备）
koreader/snap.sh 书.epub [--pages=N]          # 用本机 KOReader 按设备配置逐页截图（见下文）
koreader/snap.sh 书.epub --links             # 查每个书内链接在 KOReader 里是注释弹窗还是跳转
tools/regress/run.sh 某版/epub-optimize 目录   # 真书回归：全部文字书 + 一卷漫画各优化一遍（见下文）
tools/regress/compare.py 旧目录 新目录          # 比较两次回归的产物
shellcheck -x koreader/*.sh install.sh uninstall.sh

./install.sh --tools                         # 把命令装进 PATH，手工试用
```

## 真书回归检查

测试用真书在 `~/Documents/ereader/books/`（路径里带"漫画"的是漫画，其余是文字书）。**只读，绝不改动**：
产物一律写到临时目录，不要用 `epub-optimize` 的"输入输出同路径"就地覆盖。用户会自己整理这个目录，别假设书还在上次的位置。

先在改动前的提交构建一份 `epub-optimize` 另存，再和改动后的各跑一遍，然后比较：

```sh
git worktree add /tmp/base HEAD && (cd /tmp/base && cargo build --release -p bookconv --bin epub-optimize)
tools/regress/run.sh /tmp/base/target/release/epub-optimize 旧            # 缺省 --device=koreader
cargo build --release -p bookconv --bin epub-optimize
tools/regress/run.sh target/release/epub-optimize 新
tools/regress/compare.py 旧 新                                           # 改了彩色或阅读范围相关的处理，--device=xochitl 也跑一遍
```

`compare.py` 每本书报一行：`SAME`（每个 zip 条目逐字节相同）、`TEXT-SAME`（有条目变了，但可见文字一样）、`TEXT-DIFF`（可见文字变了，打印第一个不同处）。
另外核对两件事，不过就算失败、退出码 1：新产物里不合法的 XML 不比旧的多；新产物的可见文字和**原书**逐字符对账（按字符计数，不管顺序——注释会挪到章末——但一个字不多一个字不少）。

| 改动的性质 | 要求 |
|---|---|
| 重构、提速、修不影响产物的问题 | EPUB 产物**逐个 zip 条目字节相同**（EPUB 输出是确定的，同样的输入每次都一样） |
| 改了会影响正文的规则 | 每本书 spine 里各 XHTML 的**可见文字**（去标签、还原字符引用后的字符序列）改动前后一致；带分页和加 `--no-paginate` 一致；有变化的逐条说明原因 |
| 任何改动 | 输出的 XHTML、OPF、NCX 都是合法 XML，不合法的数量不比改动前多（现在约 85 个，来自《绝叫》、金庸全集原书的缺陷） |
| 改了漫画处理 | 拿一卷漫画比图片字节 |

《绝叫》过不了质量门是原书自身的问题，不是回归。

## 在电脑上预览 KOReader 分页

`koreader/snap.sh` 用本机装的 KOReader（`/usr/lib/koreader`，或用 `KOREADER_HOME` 指定；还要 `luajit`）按某台设备的 KOReader 配置打开一本书，逐页截图。
改了分页、注释相关的规则或 KOReader 配置后，不用拷到真机就能先看一眼。

```sh
koreader/snap.sh 产物.epub                                  # 缺省按 ireader-ocean5-pro 的配置，截前 20 页
koreader/snap.sh 产物.epub kindle-pw12-sig --pages=60 --out=target/snap
```

- 配置按 `apply.sh` 同样的顺序从空配置合并出来（个人 → 方案 → 设备 → 预设），放在临时的 `KO_HOME` 里，**不碰本机 KOReader 自己的设置**；书先复制到临时目录，`.sdr` 不会落到原书旁边。窗口不显示（SDL offscreen），窗口大小缺省 1264×1680（`device.conf` 里可以写 `SCREEN_W`、`SCREEN_H`，现在两台都用缺省）。
- 输出：`pNNN.png` 每页一张；`info.txt` 第一行是总页数，之后每页开头的 xpointer（`DocFragment[n]` = spine 里第 n 个文件，看哪一页从新文件开始就知道分页对不对）；`koreader.log` 是 KOReader 的日志。不给 `--out` 就写到临时目录，结束时打印位置。
- 本机没装设备上的字体（霞鹜文楷等）时退回 KOReader 自带的字体，每页的行数和真机不同；哪个文件从新的一页开始不受影响。
- `--links`：不截图，逐页找出书内链接，调用 KOReader 自己的注释判定（`ReaderLink:showAsFootnotePopup`，设置里的 `footnote_link_in_popup`、`link_prefer_footnote` 照样生效），把每个链接记成"弹窗"或"跳转"写进 `links.txt`，末行是合计。缺省查全书，大书用 `--pages=N` 限定页数。
- **只是预览，不算真机验证**：2026-09-29 用它确认了撤掉「避免章末空白页」后节与节分页正确，以及带 `noteref`/`footnote` 语义的注释在两种模式下都弹窗；掌阅、Kindle 上还没看。

## 版本号

改了会影响产物的代码，要把对应的版本号加一，书库据此把旧产物判为过期：

| 改了什么 | 版本号 |
|---|---|
| 清洗、优化、图片处理 | `bookconv::optimize::OPTIMIZE_VERSION`（附一行变更说明） |
| CBZ → EPUB 的转换 | `bookconv::convert::CONVERT_VERSION`（附一行变更说明）。只进 CBZ 来源的指纹，EPUB 书不受影响 |
| 书库生成流程本身 | `library` 的 `PIPELINE_VERSION`（所有书都会过期，慎用） |

## 工程约束

- **只用 Rust，不用 Calibre**（包括 `ebook-convert`、DeDRM 插件、KFX Output 插件）。
- **clean-room**：以后的解 DRM（以及万一重新做 Kindle 格式的读写），都只照公开的格式说明和对样本文件的黑盒分析实现，不读也不移植 GPL 代码（DeDRM_tools、KFX Input、KindleUnpack、Calibre）。许可证事实要下载 LICENSE 文件确认，不凭印象。
- **命名**：命令和二进制按功能命名。写进书里的 CSS 类统一用 `eink-` 前缀（`eink-flush`、`eink-center`），样式表叫 `eink-wash.css`，优化标记是 `META-INF/eink-optimized`。
- **不写死屏幕数字**：一律从 profile 读，调用方传 `Profile::readable(格式)`。`OptimizeOpts` 没有缺省设备，用 `OptimizeOpts::new(阅读范围)` 创建。
- **真机验证后才能说完成**：改变阅读效果的规则，没在目标设备上看过，就不写"已验证"；一台设备上验证过不能推到别的设备，电脑上 KOReader 的截图（`snap.sh`）也不能代替真机。单元测试只能证明逻辑没错。
- **HTML 操作一律用 `bookconv::html`**（容错单双引号、注释、不把 `data-id` 当 `id`），不写只认双引号的正则。
- **不可信输入不能让进程崩溃**：书的字节全是外来数据，数值相加用 `checked_add`、切片用 `get`，解压设上限。
- **以实测为准**：KOReader、xochitl 等阅读器的实测行为，踩到一条就记进[设备与可阅读范围](devices.md#已知的阅读器特性)。

## DRM

用途是给用户自己购买的书做格式转换和个人备份。现状：入库时识别出带 DRM 的书就拒收。

**2026-09-27 暂停**。用户手上只有 Kindle 旧格式（AZW/AZW3/MOBI）的加密书，但 Kindle 旧格式 DRM 找不到足够的独立公开资料
（2026-09-29 起连不加密的 MOBI/AZW3 也不收了，解了 DRM 也还得先转成 EPUB）：

- 维基百科只有 PC1 密码本身。
- 一篇 2008 年的博客只讲了流程，没有密钥常量、从序列号算 PID 的方法和凭证记录的结构。
- 细节齐全的只有一份照 DeDRM 的 GPL 代码写的讲解，照它实现等于间接移植 GPL 代码。

重启前要在几条路里选一条：先确认手上的文件是旧格式还是 KFX（新设备"下载并通过 USB 传输"拿到的多是 KFX，KFX 暂不考虑）；把 DRM 单独做成 GPL 模块；或者交给外部工具解完、转成 EPUB 再入库。

## 参考资料

算法和规则的历史依据在上游项目的白皮书里（EPUB 优化规范、bookconv 实现细节、已删除的 Python 管线）。上游仓库的本机位置不写进本仓库。上游文档里的类名、文件名是旧前缀，对照时换成 `eink-`。
