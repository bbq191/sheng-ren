# 贡献说明

测试用真书和三台设备只在维护者本机。**在别的机器上改代码前先读这份**，再按任务读 `docs/` 里对应的一篇。

## 项目一句话

电脑端电子书书库：书入库（只存索引，收 EPUB、CBZ）→ 按阅读模式生成优化过的书 → `booklib sync` 直接传到接着的设备。三个模式：`kindle`（Kindle 自带阅读器，出 KFX）、`ireader`（掌阅自带阅读器，EPUB）、`xochitl`（reMarkable Move 自带阅读器，EPUB）。文字书三台都"只修复"（profile `text_repair_only`），掌阅、Move 另加 `kindle_rules`（照 Send to Kindle 统一），Kindle 由 KFX 写出器照 Send to Kindle 的规则表做；漫画照旧完整处理。总体见 `README.md`。

## 没有测试书和设备时能做什么、不能做什么

| 能做 | 做不了（只能在维护者本机） |
|---|---|
| 改代码、`cargo test`、`cargo clippy` | **真书回归**：测试用真书在维护者本机，不在仓库里 |
| 改文档、画 SVG（`docs/img/`，改完用 `rsvg-convert` 渲染出来看一眼） | **`sync` 传设备、真机验证**：设备经本机 USB / 局域网连接 |
| 读代码、审代码、写单元测试 | **对照 Send to Kindle 样本**（`tools/kfx/s2kbatch.py`、`s2kdev.py`）：样本在 Kindle 上 |

所以：

- 推上去以后看 GitHub Actions 的 CI（测试、clippy 0 警告、shellcheck、Python 语法）是不是绿的；红了先修。
- **推到新分支，不要直接改 `master`**，由维护者在本机跑完回归、上真机看过再合。
- 改了**会影响产物**的代码（清洗、优化、漫画、KFX 写出器、书库生成流程），在提交说明里写清"**没跑真书回归、没上真机**"，并列出预计哪些书、哪类写法会变。不要声称回归通过。
- 只是重构时，要求产物逐字节不变——别的机器上没法验证这一点，所以写明"待本机回归确认逐字节不变"。
- 测试一律不碰真设备（`crates/library/tests/common` 已经把设备目录指到临时目录、`BOOKLIB_NO_SSH=1`、Move 用 `FakeMove`），别加会连真设备或网络的测试。

## 硬约束（用户定的，不许推翻）

完整的表和理由在 `docs/decisions.md#一直有效的约束`，必须记住的：

- **只用 Rust；不用 Calibre**（`ebook-convert`、DeDRM、KFX 插件都不行）。工具脚本可以用 Python，但不进产品流程。
- **clean-room**：格式读写只照公开文档和对样本的黑盒分析实现，**不读、不移植** KindleUnpack、Calibre、DeDRM_tools、KFX Input 等 GPL 代码，也不看照 GPL 代码写的讲解（deepwiki 等）。Koodo Reader（AGPL-3.0）只借鉴它用哪些数据源，不看不抄代码。
- **命名**：`sheng-ren` 只是开发代号，命令按功能命名。仓库里不写本项目是从哪个上游项目拆出来的、不出现上游项目名。写进书里的 CSS 类用 `eink-` 前缀，样式表 `eink-wash.css`/`eink-ua.css`，优化标记 `META-INF/eink-optimized`。
- **书的内容一个字不多一个字不少；不删原书内容，图片页也不能删**；图片只为适配屏幕缩放；拿不准就不处理。
- **漫画规则不变**（2026-10-08 用户定）：动漫画路径前先问用户；漫画产物要逐字节不变。黑白屏 256 级灰度、**不抖动**。
- **文字书不再强制按章分页**，原书文件结构不变；目录不变、至少索引到节。章节只按目录层级判，纯数字不能当节的依据。
- **用户定过的规则别让审计推翻**（例：远程图的处理、漫画补白撤销）——发现和决定记录冲突时，报告给用户，不要自己改回去。
- **算法里不写死屏幕数字，不按设备名写分支**：一律从 profile（`crates/profile/profiles/<id>.toml`）读，阅读器怪癖做成 profile 字段。
- **DRM 暂停**：识别出 DRM 就拒收，不要实现解密。
- **真机验证后才能说完成**；一台设备验证过不能推到另一台；"规范允许"不等于阅读器能用。没上真机的改动一律算"未验证"。

## 版本号（改了影响产物的代码要加一）

现值只写在 `docs/development.md#版本号` 的表里，别处不写：

- `bookconv::optimize::OPTIMIZE_VERSION`（文字书）、`COMIC_VERSION`（漫画；两路共用的代码改了两个都加——但漫画规则不变，所以一般不该碰）；
- `bookconv::wash::KINDLE_RULES_VERSION`（只进开了 `kindle_rules` 的掌阅、Move，不写进书）；
- `kfx::write::WRITER_VERSION`（只进 kindle）、`bookconv::convert::CONVERT_VERSION`、`bookconv::opfmeta::VERSION`；
- `library` 的 `PIPELINE_VERSION`（所有书都过期，慎用）。

注意：Kindle 上覆盖 KFX 时字节有任何不同阅读进度就清零，所以 KFX 产物会变的改动要在提交说明里写清哪些书变了。**改了指纹构成**（`crates/library/src/generate.rs`）会让用户电脑上全部书重算，先问用户。

## 命令

```sh
cargo build --workspace
cargo test --workspace                      # 要求全部通过
cargo clippy --workspace --all-targets      # 要求 0 警告
cargo test -p bookconv <测试名子串>
cargo run -p bookconv --bin epub-optimize -- --device=kindle|ireader|xochitl 输入.epub 输出.epub
cargo run --release -p kfx --bin epub-to-kfx -- 优化过的.epub 输出.kfx     # --styles 打印按 CSS 原样算的样式
cargo run --release -p kfx --bin kfx-dump -- [--type=N] [--full] 书.kfx
shellcheck -x install.sh uninstall.sh tools/cargo-pkgs.sh xochitl/comic-margins.sh tools/regress/run.sh
```

代码风格：本仓库**不用 rustfmt 的默认宽度**（行很宽），别对整个仓库跑 `cargo fmt`；照周围代码的写法、注释密度和中文注释风格写。

## 动手前先读

| 要改的 | 先读 |
|---|---|
| 命令、使用方式 | `docs/usage.md` |
| crate、模块、书库、生成流程、指纹 | `docs/architecture.md` |
| 排版与优化规则、验证情况 | `docs/typesetting.md` |
| profile 字段、阅读器怪癖、可阅读范围 | `docs/devices.md`、`docs/xochitl.md` |
| KFX 容器、写出器、Send to Kindle 规则表 | `docs/kfx.md` |
| 测试、回归方法、版本号、**共用轮子对照表**、工程约束 | `docs/development.md` |
| 用户定过的事 | `docs/decisions.md` |

架构要点：依赖单向无环 `library` → `kfx` → `bookconv` → `profile`；`mobidict` → `bookconv`。HTML 操作一律用 `bookconv::html`（容错单双引号、注释，别写只认双引号的正则）；写文件先写临时文件再改名（`util::produce_then_replace`）；读外来数据设上限（`util::read_capped`），外来数据不能让进程 panic。**共用的轮子别再各写一份**，动手前查 `docs/development.md#工程约束` 的对照表。

改了行为就同步改 `docs/`；发现的问题、得出的结论写进对应文档。
