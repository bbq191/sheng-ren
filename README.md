# 电子书入库与按阅读模式优化工具

**一句话**：在电脑上登记你的书目录（EPUB、CBZ 漫画），`booklib sync` 按三台阅读器各自的脾气生成合适的书，**直接传到插着、连着的设备上**；之后书目录里加书、改书、删书，再 `sync` 一次设备就跟着变。电脑上不留产物。

![总体流程](docs/img/overview.svg)

## 它做什么

| | 做法 |
|---|---|
| **收书** | EPUB、CBZ 漫画、网页链接。书库**只存索引**（原件在哪、内容哈希、书名作者），不复制原件；改名、挪位置按内容认得出 |
| **文字书：只修复** | 修成合规的 EPUB 3、目录修到节（按书/卷、章、节嵌套），**文字、图片、样式一概不动**，不拆原书的文件。Kindle 由 KFX 写出器照 Send to Kindle 的规则排；掌阅、Move 照同一套规则补几条（标签缺省样式、正文用阅读器字体……）；Move 另外保证注释点得动 |
| **漫画** | 原画质，按阅读器真正能显示的范围缩放补白，图离屏幕边缘 1px；黑白屏转 256 级灰度（不抖动） |
| **补简介封面** | `meta --fetch` 联网找简介、标签（豆瓣 → QQ 阅读 → Wikidata），没封面的书找封面，都没有就生成一张。只补进产物，原件不动 |
| **跟着书目录走** | 只重建、只重传有变化的书；原件换了内容，设备上那份原地换新；原件删了，设备上那份也删 |

> 底线：书里的文字一个不多、一个不少；图片还是原来那张，只为适配屏幕缩放；拿不准的就不处理。

## 三台设备

| 模式 | 给谁读 | 产物 | 怎么传 |
|---|---|---|---|
| `kindle` | Kindle Paperwhite 12 代签名版自带阅读器 | KFX | USB（MTP），放 `documents/<子目录>/` |
| `ireader` | 掌阅 Ocean 5 Pro 自带阅读器 | EPUB | USB（MTP），放 `documents/<子目录>/` |
| `xochitl` | reMarkable Paper Pro Move 自带阅读器 | EPUB（彩色） | SSH（USB 线或 Wi-Fi），交给 Move 上的书架服务，按子目录建文件夹 |

前提：Kindle、掌阅插上后要被 jmtpfs 挂到 `$XDG_RUNTIME_DIR/mtp/kindle`、`…/mtp/ireader`（可用 `BOOKLIB_MTP_DIR` 改）；Move 上要装好带导入接口的书架服务（另一个仓库）。没接上的设备这次跳过，下次接上再传。

三个模式是同一套代码，区别（屏幕、阅读范围、阅读器的怪癖、各台另加的规则、怎么传）都写在模式的配置里，见[设备与阅读模式](docs/devices.md)。

## 快速开始

需要 Rust 工具链（`cargo`）。

```sh
./install.sh                                  # 装 booklib（升级也是再跑一次）
booklib track ~/Documents/ereader/books       # 登记书目录（只需一次，可以登记多个）
booklib sync                                  # 同步进书库，为接着的设备生成并传上去
booklib list                                  # 看每本书在各设备上的产物、是不是最新
```

日常就是改完书目录再跑 `booklib sync`（或者让 `booklib sync --watch` 一直开着）。其它常用的：

```sh
booklib sync --device=kindle 三体     # 只给一台、只做书名含"三体"的
booklib sync --force                  # 全部重建（内容没变的不会重传）
booklib meta --fetch                  # 联网补简介、标签、封面
booklib devices                       # 看哪台设备接上了
booklib --help                        # 全部命令；booklib sync --help 看单个命令
```

详细用法、输出怎么看、常见问题见[使用指南](docs/usage.md)。

## 文档

| 文档 | 讲什么 |
|---|---|
| [使用指南](docs/usage.md) | 安装、各命令、产物放在哪（直接传设备）、常见问题 |
| [排版与优化规则](docs/typesetting.md) | 文字书（只修复 + 各台另加的）、漫画各做了什么、为什么；**真机验证情况** |
| [设备与阅读模式](docs/devices.md) | 模式的字段、各阅读器的怪癖、可阅读范围怎么量、重拷书以后进度还在不在 |
| [xochitl 阅读器踩坑](docs/xochitl.md) | Move 自带阅读器认什么、不认什么、怎么验证 |
| [KFX](docs/kfx.md) | Kindle 产物怎么从 EPUB 转成 KFX：容器结构、对照样本推出的写法、真机结论 |
| [架构](docs/architecture.md) | 代码怎么分、书库怎么存、生成流程、指纹 |
| [开发](docs/development.md) | 测试、真书回归、版本号、工程约束、DRM |
| [决定记录](docs/decisions.md) | 用户定过的事（按日期）和待定的事 |

## 几个词

| 词 | 意思 |
|---|---|
| 书库 | 电脑上的一个目录（缺省 `~/.local/share/booklib`），只记原件在哪、内容哈希、书名作者、找来的封面 |
| 阅读模式（profile） | 一种阅读器的参数：屏幕、可阅读范围、黑白还是彩色、产物格式、注释怎么处理、怎么传到设备。一个模式出一份产物 |
| 可阅读范围 | 阅读器除去页边距、页眉页脚后真正显示内容的区域 |
| 产物 | 按某个模式生成的书，直接传到设备上（电脑上不留） |
| 指纹 | 决定产物要不要重建的一串值（原件哈希、规则版本、模式参数……），任何一项变了才重建 |
| 质量门 | 产物生成后的自检（XHTML 合法、引用都找得到、没有 DRM）；没过只警告，产物照样传 |

## 现状

- 现行规则三台都在真机上看过，逐条见[验证情况](docs/typesetting.md#验证情况)。
- 自带阅读器之间不能同步阅读进度（见[设备 · 附录](docs/devices.md#自带阅读器之间能不能同步进度2026-09-30-真机)）。
- 带 DRM 的书拒收（解 DRM 暂停，见[开发 · DRM](docs/development.md#drm)）；MOBI、AZW3、PDF 不收，先转成 EPUB。

## 许可证

[MIT](LICENSE)。
