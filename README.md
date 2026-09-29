# 电子书入库与按设备优化工具

在电脑上把各种格式的电子书收进一个**书库**，再按阅读器的屏幕和格式特点**生成优化版**，用 USB 拷到设备上读。

![总体流程](docs/img/overview.svg)

- **收书**：EPUB、MOBI/AZW/AZW3/PRC、FB2、CBZ 漫画、PDF、网页链接都能入库，书库**只存索引**（原件在哪、内容哈希、书名作者），不复制原件，几十本书的书库只有几百 KB。同一本书重复入库、改名移动都认得出来。
- **文字书**：去掉书里写死的字体、字号、行高，设备上调字号字体才能生效；按中文或英文的阅读习惯排版；注释比正文小一号；章标题独立一页，节与节、节与章之间分页，目录里节缩进挂在章下面。
- **漫画**：保留原画质，按设备真实的可阅读范围裁边、缩放、补白，黑白屏转 256 级灰度（不抖动），夹在漫画里的文字页照常保留。
- **没有封面的书联网补封面**：豆瓣中文版封面 → 原作封面（Wikidata + Open Library）→ 都没有就生成（书名 + 作者头像）。原件不动。
- **一次生成多台设备**，只重建有变化的书；可以跟踪书目录，新书、改过的书自动同步进书库。

> 书里的文字一个不多、一个不少，图片还是原来那张（只为适配屏幕缩放），作者的强调（颜色、加粗）保留。拿不准的情况就不处理。

## 支持的设备

| 设备 | 设备 id | 产物 | 屏幕 |
|---|---|---|---|
| reMarkable Paper Pro Move（自带阅读器 xochitl） | `rmpp-move` | EPUB（图片型 PDF 出 PDF） | 彩色，954×1696 |
| reMarkable Paper Pro Move（KOReader） | `rmpp-move-koreader` | EPUB | 彩色，954×1696 |
| Kindle Paperwhite 12 代签名版（2024） | `kindle-pw12-sig` | AZW3 | 黑白，1264×1680 |
| 掌阅 iReader Ocean 5 Pro | `ireader-ocean5-pro` | EPUB | 黑白，1264×1680 |

掌阅、Kindle 上的 KOReader 读 `ireader-ocean5-pro` 的 EPUB。加一台新设备只要写一个配置文件，见[设备与可阅读范围](docs/devices.md)。

## 快速开始

需要 Rust 工具链（`cargo`）。

```sh
./install.sh                                  # 安装 booklib、ebook-meta 命令（卸载：./uninstall.sh）

booklib add ~/Downloads/三体.epub 漫画.cbz https://example.com/article
booklib track ~/Documents/ereader/books       # 整个书目录：登记跟踪（add 只收单个文件）
booklib sync --device=kindle-pw12-sig,ireader-ocean5-pro   # 同步进书库并生成
booklib list                                  # 看每本书给哪些设备生成过、是否最新
```

产物在 `~/.local/share/booklib/output/<设备 id>/`，拷到设备上即可。完整用法见[使用指南](docs/usage.md)。

## 文档

| 文档 | 内容 |
|---|---|
| [使用指南](docs/usage.md) | 安装与卸载、各命令、联网补元数据、传书到设备、常见问题 |
| [排版与优化规则](docs/typesetting.md) | 文字书、漫画各做了什么，为什么这样做 |
| [设备与可阅读范围](docs/devices.md) | 设备配置、怎么在真机上量可阅读范围、加新设备 |
| [架构](docs/architecture.md) | 各 crate 的职责、数据怎么流动、书库怎么存 |
| [KOReader 配置](docs/koreader.md) | 三台设备上 KOReader 的个人设置、文字书与漫画两套方案、怎么应用 |
| [AZW3 写出器](docs/azw3.md) | Kindle 格式是怎么写出来的 |
| [开发](docs/development.md) | 测试、真书回归检查、工程约束 |

## 现状

- Kindle PW12 真机验证通过：文字书（封面、章标题独立一页、字号字体可调、目录与节缩进、注释跳转与返回）和 135MB 漫画。
- reMarkable Move、掌阅的分页与排版还没在真机上验证。
- KOReader（三台都装了）：配置用 `koreader/apply.sh` 下发，能预览、回滚、撤销。漫画按书的"漫画"标签自动套用从右往左、铺满整屏，Kindle 上核实过标签能被读到；界面字体在掌阅上确认生效，其余两台还没看。
- 带 DRM 的书现在拒收（解 DRM 暂停，见[开发](docs/development.md#drm)）。
