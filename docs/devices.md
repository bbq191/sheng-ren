# 设备与可阅读范围

## 阅读模式（profile）

优化按**阅读模式**来：一种阅读软件在一类屏幕上的样子，一个模式出一份 EPUB。每个模式是一个 TOML 文件，文件名就是 id（`--device=` 的值）。
内置的在 `crates/profile/profiles/`，构建时整个目录嵌进程序，**增删模式只要增删文件，不用改代码**。

| 模式 | 给谁读 | 屏幕 | 阅读范围 | 黑白 / 彩色 |
|---|---|---|---|---|
| `koreader` | Kindle Paperwhite 12 代签名版、掌阅 iReader Ocean 5 Pro 上的 KOReader | 1264×1680，300ppi | 1264×1680 | 黑白（漫画转 256 级灰度） |
| `xochitl` | reMarkable Paper Pro Move 自带的阅读器 | 954×1696，264ppi | 842×1455 | 彩色 |

```toml
# crates/profile/profiles/koreader.toml
name = "KOReader（掌阅 Ocean 5 Pro、Kindle PW12 共用）"
ppi = 300
color = false            # 黑白屏：漫画转 256 级灰度
formats = ["epub"]       # 产物格式，现在只能是 epub

[screen]                 # 标称分辨率，竖屏填写（width ≤ height）
width = 1264
height = 1680

[readable.epub]          # 真实可阅读范围（可选，不写就用 [screen]）
width = 1264
height = 1680
```

- 写了不认识的字段、不认识的格式会报错，防止拼错后被悄悄忽略。
- `[readable.epub]` 不能超过屏幕尺寸。
- 阅读范围以**截图的像素**为准。截图分辨率和标称的 `[screen]` 不一定是同一个坐标系（Kindle 的截图是 1272×1696，标称 1264×1680），量出来的数照截图写。
- **自定义模式**：放在书库的 `profiles/` 目录（缺省 `~/.local/share/booklib/profiles/<id>.toml`），同 id 覆盖内置的。`booklib devices` 会列出来。

### 为什么这样分

- **掌阅和 Kindle 共用 `koreader`**：两台都是 7 英寸 1264×1680、300ppi 的黑白屏（2026-09-27 核实），读书都用 KOReader，KOReader 的配置两台也一样（见 [KOReader 配置](koreader.md)）。
  共用一份产物，文件名也就一样，KOReader 的进度同步才能在两台之间认出同一本书。
- **Kindle 自带阅读器不再出产物**（2026-09-29）：它 USB 传书只认 AZW3，而 AZW3 写出器已经删了；Kindle 上改用 KOReader 读 EPUB。
- **Move 只用自带阅读器**（2026-09-29 撤掉 Move 上的 KOReader）：KOReader 在 Move 上翻页闪得厉害。Paper Pro、Move 的屏幕刷新由 xochitl 那一层（qtfb / shim）控制，KOReader 自己的刷新设置管不到。

旧的设备 id（`kindle-pw12-sig`、`ireader-ocean5-pro`、`rmpp-move`、`rmpp-move-koreader`）已经不是阅读模式了；`kindle-pw12-sig`、`ireader-ocean5-pro` 现在只用作 `koreader/` 里的设备名（给哪台设备下发 KOReader 配置）。
拷哪个文件夹、拷到哪，见[使用指南 · 传书到设备](usage.md#传书到设备)。

## 为什么要"真实可阅读范围"

![标称屏幕与真实可阅读范围](img/readable-area.svg)

`[screen]` 是设备标称的分辨率，但阅读器要留页边距、页眉页脚，真正用来显示内容的区域更小。漫画页按这个区域缩放、补白，才能正好填满，不会被阅读器再缩一次或者留出白条。

- **有实测值就用实测值**（写在 `[readable.epub]`），没有就退回标称屏幕。
- **阅读器里的页边距设置变了，阅读范围也会变**，要重新量。
- 程序里不写死任何屏幕数字：图片缩放、漫画补白都从 profile 读。

| 模式 | 阅读范围 | 依据 |
|---|---|---|
| `koreader` | 1264×1680 | 2026-09-27 在掌阅**自带阅读器**上用测量书截屏：整页大图铺满整屏，连页眉页脚也盖住。KOReader 的漫画方案去掉页边距、隐藏状态栏，按整屏算；**在 KOReader 上还没单独实测**。图文混排时不适用 |
| `xochitl` | 842×1455 | xochitl 默认页边距 56：宽 = 954 − 2×56；高按固定上下留白 462.1pt 换算（2026-09-21 在 xochitl 上实测）。改了页边距要跟着改（28 档 → 898 宽；1 档 → 952 宽） |

## 在真机上测量

用"测量书"来量。书里有一张竖长和一张横宽的纯黑大图，阅读器会把它们等比缩小到放得下为止。竖长图显示出来的高度就是可用高度，横宽图的宽度就是可用宽度。

```sh
# 1. 生成测量书
cargo run --release -p bookconv --bin readable-probe -- 测量书.epub

# 2. 传到设备上，用要量的那个阅读软件打开，分别翻到"竖长图"和"横宽图"那两页，各截一张屏

# 3. 从截图里找最大的黑色区域，输出可以直接贴进 profile 的 TOML（[readable.epub]）
cargo run --release -p bookconv --bin readable-measure -- 竖长.png 横宽.png
```

把输出贴进对应的 profile（内置的改 `crates/profile/profiles/`，自己用的放书库的 `profiles/`），并在注释里写明测量日期和条件（哪个阅读软件、页边距设置等）。

## 加一个阅读模式

1. 写 `<id>.toml`：`name`、`ppi`、`color`、`formats = ["epub"]`、`[screen]`。
2. 在真机上用那个阅读软件量可阅读范围，写进 `[readable.epub]`。量不了就先不写，按标称屏幕处理。
3. 把阅读器的特殊行为记下来（比如 xochitl 只认同文件锚点）。以后这类差异也要做成 profile 字段，不要在算法里按模式名写分支。
4. 真机上检查文字书和漫画的效果，再写"已验证"。

## 已知的阅读器特性

| 阅读器 | 实测行为 |
|---|---|
| xochitl（Move） | 正文链接只认同一文件内的 `#锚点`；不认行内样式；NCX 的 `dtb:uid` 和 OPF 不一致时不显示目录；页边距 1 时带 class 的 `<body>` 里图片会被吃掉约 20pt 宽 |
| 掌阅自带阅读器 | 只放一张大图的页面，图片铺满整屏（`koreader` 的阅读范围就是按这个量的） |
| KOReader | 「避免章末空白页」样式调整（`docfragment_page-break-before_avoid`）开着时，拆开的文件会连成一片，节与节不分页；配置里已撤掉，2026-09-29 只在电脑上的 KOReader 里确认过，见 [KOReader 配置](koreader.md#文字书方案)。不读 OPF 的 `page-progression-direction`（漫画从右往左靠配置档设） |
