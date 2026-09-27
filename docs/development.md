# 开发

## 常用命令

```sh
cargo build --workspace
cargo test --workspace                      # 全部测试
cargo test -p bookconv <测试名子串>          # 只跑名字匹配的测试
cargo clippy --workspace --all-targets      # 要求没有警告

python3 tools/kf8/dump.py 文件.azw3          # 看 KF8 结构
python3 tools/kf8/indexes.py 文件.azw3       # 看片段/骨架/目录索引
python3 tools/kf8/textcheck.py 文件.azw3 源.epub   # AZW3 与源 EPUB 文字逐字符核对
```

## 真书回归检查

测试用真书在 `~/Documents/ereader/books/`（`haodoo/`、`小说/` 是文字书，`漫画/` 是大体积漫画）。**只读，绝不改动**：产物一律写到临时目录，不要用 `epub-optimize` 的"输入输出同路径"就地覆盖。

改了会影响正文的规则后：

1. 把全部文字书用改动前、改动后的 `epub-optimize` 各跑一遍，再用改动后的加 `--no-paginate` 跑一遍。
2. 比较每本书 spine 里各 XHTML 的**可见文字**（去标签、还原字符引用后的字符序列）：改动前后一致，带分页和不带分页一致。
3. 检查每个输出的 XHTML 都是合法 XML。
4. 改了漫画处理的，拿一卷漫画对比图片字节。
5. 改了 AZW3 写出器的，用 `tools/kf8/textcheck.py` 核对文字。

《绝叫》过不了质量门是原书自身的问题，不是回归。

## 版本号

改了会影响产物的代码，要把对应的版本号加一，书库据此把旧产物判为过期：

| 改了什么 | 版本号 |
|---|---|
| 清洗、优化、图片处理 | `bookconv::optimize::OPTIMIZE_VERSION`（附一行变更说明） |
| AZW3 写出 | `azw3::WRITER_VERSION` |
| 书库生成流程本身 | `library` 的 `PIPELINE_VERSION` |

## 工程约束

- **只用 Rust，不用 Calibre**（包括 `ebook-convert`、DeDRM 插件、KFX Output 插件）。
- **clean-room**：AZW3 写出、KF8/MOBI 读取、以后的解 DRM，都只照公开的格式说明和对样本文件的黑盒分析实现，不读也不移植 GPL 代码（DeDRM_tools、KFX Input、KindleUnpack、Calibre）。许可证事实要下载 LICENSE 文件确认，不凭印象。
- **命名**：命令和二进制按功能命名。写进书里的 CSS 类统一用 `eink-` 前缀（`eink-flush`、`eink-center`），样式表叫 `eink-wash.css`，优化标记是 `META-INF/eink-optimized`。
- **不写死屏幕数字**：一律从 profile 读，调用方传 `Profile::readable(格式)`。`OptimizeOpts` 没有缺省设备，用 `OptimizeOpts::new(阅读范围)` 创建。
- **真机验证后才能说完成**：改变阅读效果的规则，没在目标设备上看过，就不写"已验证"。单元测试只能证明逻辑没错。
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
