# 设备与可阅读范围

## 设备配置（profile）

每台设备是一个 TOML 文件，文件名就是设备 id。内置设备在 `crates/profile/profiles/`，构建时整个目录嵌进程序，**增删设备只要增删文件，不用改代码**。

```toml
# crates/profile/profiles/kindle-pw12-sig.toml
name = "Kindle Paperwhite 12 代签名版"
ppi = 300
color = false            # 黑白屏：漫画转 256 级灰度
formats = ["azw3"]       # 能投递的格式；第一个流式格式（epub/azw3）用于文字书

[screen]                 # 标称分辨率，竖屏填写（width ≤ height）
width = 1264
height = 1680

[readable.azw3]          # 真实可阅读范围（按格式写，可选）
width = 1104
height = 1546
```

- `comic_margin = 1`（可选，缺省 1）：漫画页图到可阅读范围四边的白边（像素）。图保比缩放进"阅读范围 − 2×白边"的框、居中，
  受限的那条边两侧正好这么宽；须小于阅读范围短边的 1/4。改了它，书库里的漫画和文字书都算过期（进指纹）。
- 写了不认识的字段会报错，防止拼错字段名后被悄悄忽略。
- `[readable.<格式>]` 只能写 `formats` 里有的格式，也不能超过屏幕尺寸；`formats` 不能有重复项。
- 阅读范围以**截图的像素**为准。Kindle 的截图是 1272×1696，和标称的 `[screen]` 1264×1680 不是同一个坐标系，量出来的数照截图写。
- **自定义设备**：放在书库的 `profiles/` 目录（缺省 `~/.local/share/booklib/profiles/<id>.toml`），同 id 覆盖内置设备。`booklib devices` 会列出来。

**同一台设备、不同的阅读软件，可阅读范围不同，就分成两个 profile**：`rmpp-move`（Move 自带的 xochitl，实测 842×1455）和
`rmpp-move-koreader`（Move 上的 KOReader：漫画方案四边页边距 0、隐藏状态栏，整页漫画铺满 954×1696，按设置推算、还没实测）。
文字书在两个 profile 下的产物逐字节相同，只有漫画页的补白比例不同。掌阅、Kindle 上的 KOReader 用 `ireader-ocean5-pro` 的产物（两台屏幕都是 1264×1680，
掌阅的阅读范围本来就是整屏）；`kindle-pw12-sig` 是给 Kindle 自带阅读器的 AZW3。拷哪个产物、拷到哪，见[使用指南 · 传书到设备](usage.md#传书到设备)。

## 为什么要"真实可阅读范围"

![标称屏幕与真实可阅读范围](img/readable-area.svg)

`[screen]` 是设备标称的分辨率，但阅读器要留页边距、页眉页脚，真正用来显示内容的区域更小。漫画页按这个区域缩放、补白，才能正好填满，不会被阅读器再缩一次或者留出白条。

- **有实测值就用实测值**（写在 `[readable.<格式>]`），没有就退回标称屏幕。
- **阅读器里的页边距设置变了，阅读范围也会变**，要重新量。
- 程序里不写死任何屏幕数字：图片缩放、漫画补白、PDF 页面尺寸都从 profile 读。

| 设备 | 格式 | 阅读范围 | 依据 |
|---|---|---|---|
| reMarkable Move（xochitl） | EPUB | 842×1455 | xochitl 默认页边距 56：宽 = 954 − 2×56；高按固定上下留白 462.1pt 换算（早期在 xochitl 上实测） |
| reMarkable Move | PDF | 954×1696 | 页面尺寸等于屏幕时左右留白为 0 |
| Kindle PW12 | AZW3 | 1104×1546 | 2026-09-27 测量书截屏：左右页边距各 84，上下页眉页脚各 75。截图分辨率是 1272×1696，不是标称的 1264×1680 |
| 掌阅 Ocean 5 Pro | EPUB | 1264×1680 | 2026-09-27 测量书截屏：整页大图铺满整屏，连页眉页脚也盖住。图文混排时不适用 |
| reMarkable Move（KOReader） | EPUB | 954×1696 | 按 KOReader 漫画方案的设置推算（四边页边距 0、隐藏状态栏），还没用测量书实测 |

## 在真机上测量

用"测量书"来量。书里有一张竖长和一张横宽的纯黑大图，阅读器会把它们等比缩小到放得下为止。竖长图显示出来的高度就是可用高度，横宽图的宽度就是可用宽度。

```sh
# 1. 生成测量书（Kindle 还要转成 AZW3）
cargo run --release -p bookconv --bin readable-probe -- 测量书.epub
cargo run --release -p azw3 --bin epub-to-azw3 -- 测量书.epub 测量书.azw3

# 2. 传到设备上，分别翻到"竖长图"和"横宽图"那两页，各截一张屏

# 3. 从截图里找最大的黑色区域，输出可以直接贴进 profile 的 TOML
cargo run --release -p bookconv --bin readable-measure -- --format=azw3 竖长.png 横宽.png
```

把输出贴进对应的 profile（内置设备改 `crates/profile/profiles/`，自己用的放书库的 `profiles/`），并在注释里写明测量日期和条件（页边距设置等）。

## 加一台新设备

1. 写 `<id>.toml`：`name`、`ppi`、`color`、`formats`、`[screen]`。
2. 在真机上量可阅读范围，写进 `[readable.<格式>]`。量不了就先不写，按标称屏幕处理。
3. 把阅读器的特殊行为记下来（比如 Kindle 的窄图靠左、xochitl 只认同文件锚点）。以后这类差异也要做成 profile 字段，不要在算法里按设备名写分支。
4. 真机上检查文字书和漫画的效果，再写"已验证"。

## 已知的阅读器特性

| 阅读器 | 实测行为 |
|---|---|
| Kindle PW12 | USB 传书只认 AZW3，不认 EPUB；比页面窄的图片靠左不居中；侧载书归"文档"分类时封面最稳 |
| 掌阅 | 只放一张大图的页面，图片铺满整屏 |
| xochitl（Move） | 正文链接只认同一文件内的 `#锚点`；不认行内样式；NCX 的 `dtb:uid` 和 OPF 不一致时不显示目录；页边距 1 时带 class 的 `<body>` 里图片会被吃掉约 20pt 宽 |
