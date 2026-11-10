## 结论

分支 `feat/comic-gutter-crop`（`d50a889..49a6b67`）整体实现质量优秀，前两轮验收发现的阻断问题（彩色页误判、16 位 RGB 灰度舍入漂移）已彻底修复，核心约束、边界防护与算法取整均严密无误，**无阻断问题，二审通过**。

---

## 问题分档

### 阻断（0 项）
无。

### 建议（1 项）
1. **灰度封面页未做排除处理**：彩色封面因阻断 1 修复后天然不收，但若是灰度多格构图的封面（如部分特装版/爱藏版黑白封面），开启 `--gutter-crop` 后仍会执行格间空白裁切。C 单跨页拆分已显式跳过封面，本单是否需要在 `streaming.rs` 中一并跳过封面页，建议在真机 A/B 对比后视实际排版视觉效果决定是否补充。
   - 依据：[`crates/bookconv/src/imgopt.rs:1315`](file:///home/afu/Projects/sheng-ren/crates/bookconv/src/imgopt.rs#L1315-L1328)、[`crates/bookconv/src/optimize/streaming.rs:1225`](file:///home/afu/Projects/sheng-ren/crates/bookconv/src/optimize/streaming.rs#L1225-L1244)

### 线索（1 项）
1. **底噪与偏暗纸色（< 247）页完全不收**：空白行判定严格卡死 `DARK_BELOW = 247`（任意通道 < 247 即算内容），扫描底噪较重或纸底发黄泛灰（如底色 240–245）的扫描版漫画将完全匹配不到空白带，导致裁切量为 0。此为「宁少裁不切错」的预设取舍，在制作 A/B 测试书抽检时需留意此现象。
   - 依据：[`crates/bookconv/src/comiccrop.rs:17`](file:///home/afu/Projects/sheng-ren/crates/bookconv/src/comiccrop.rs#L17)、[`crates/bookconv/src/comiccrop.rs:76`](file:///home/afu/Projects/sheng-ren/crates/bookconv/src/comiccrop.rs#L76)

---

## 重点核查依据与逐条结论

### 1. 原图彩色判定与多格式支持（判对，关着不多扫）
- **判定时机与性能**：[`decode_comic`](file:///home/afu/Projects/sheng-ren/crates/bookconv/src/imgopt.rs#L717-L721) 中设置了 `need_gray_content = page.gutter_crop`。开关关闭时直接置 `true`，完全不调用 `decoded.to_rgb8()`，零额外像素扫描；仅在开关开启且输入包含彩色通道时，才多拷一份通过 [`Page8::is_gray_content`](file:///home/afu/Projects/sheng-ren/crates/bookconv/src/imgopt.rs#L1493-L1507) 扫描原图色彩分布。
- **裁切门控**：[`process_comic_page`](file:///home/afu/Projects/sheng-ren/crates/bookconv/src/imgopt.rs#L1315) 使用传入的 `gray_content` 字段作为门控，在黑白屏设备（`grayscale = true`）预先转为灰度图后，仍能准确拦截彩色内容页，避免彩页在黑白设备上被收。
- **各输入格式分析**：
  - **16 位灰度（L16 / La16）**：`gray == true`，直接为 `gray_content = true`，判定正确。
  - **16 位 RGB（Rgb16）**：走到 else 分支，`decoded.to_rgb8()` 降至 8 位后扫描各通道极差。若为彩色则为 `false`；若各像素 $R=G=B$ 则为 `true`，判定正确。
  - **RGBA（8 位 / 16 位）**：先经 [`flatten_alpha_on_white`](file:///home/afu/Projects/sheng-ren/crates/bookconv/src/imgopt.rs#L848-L872) 将透明区域在纯白底上合成并转为 8 位 RGB，再判 `is_gray_content`，避免透明背景中的脏像素干扰，判定正确。
  - **调色板（Indexed PNG）**：解码器直接解出 `Rgb8` 或 `Rgba8`，彩色调色板判 `false`，灰度调色板判 `true`，判定正确。
  - **CMYK JPEG**：解码器转为 `Rgb8`，彩色 CMYK 判 `false`，黑版灰度 CMYK 判 `true`，判定正确。
- 依据：[`crates/bookconv/src/imgopt.rs:688-734`](file:///home/afu/Projects/sheng-ren/crates/bookconv/src/imgopt.rs#L688-L734)、[`crates/bookconv/src/imgopt.rs:1315-1328`](file:///home/afu/Projects/sheng-ren/crates/bookconv/src/imgopt.rs#L1315-L1328)、[`crates/bookconv/src/imgopt.rs:3510-3690`](file:///home/afu/Projects/sheng-ren/crates/bookconv/src/imgopt.rs#L3510-L3690)

### 2. 开关关闭产物逐字节不变与版本/指纹控制（符合预期）
- **转灰度路径一致性**：在 `49a6b67` 中，[`decode_comic`](file:///home/afu/Projects/sheng-ren/crates/bookconv/src/imgopt.rs#L722-L726) 的转换逻辑恢复为原生的 `if gray || to_gray { Page8::Gray(into_luma8(decoded)) } else { Page8::Rgb(decoded.into_rgb8()) }`。16 位 RGB 图保持调用 `DynamicImage::into_luma8`（由 `image` 库直接完成 16 位到灰度的转换），彻底消除了此前先转 `rgb8` 再转灰度带来的舍入漂移，保证开关关闭时与 master 逐字节一致。
- **COMIC_VERSION 升版与指纹**：
  - `COMIC_VERSION` 由 66 升为 67，更新记录已如实写入注释与文档。
  - 指纹段 [`comic_seg`](file:///home/afu/Projects/sheng-ren/crates/library/src/fingerprint.rs#L120) 中 `(device.comic.gutter_crop, 'L')` 仅在开关为 true 时追加 `'L'`。
  - 四台预置设备在 `crates/profile/profiles/*.toml` 中均未声明该键，解析时 [`ProfileFile`](file:///home/afu/Projects/sheng-ren/crates/profile/src/lib.rs#L475) 缺省为 `false`，默认指纹不包含 `'L'`。
  - [`fp_golden.txt`](file:///home/afu/Projects/sheng-ren/crates/library/tests/fp_golden.txt#L470-L496) 中文字书旧行 0 变化；四台漫画默认行仅 `c66`→`c67`，不含 `'L'`；仅新增的 `comic_gutter_crop` mutation 行出现 `'L'`。
- 依据：[`crates/bookconv/src/optimize/mod.rs:197-199`](file:///home/afu/Projects/sheng-ren/crates/bookconv/src/optimize/mod.rs#L197-L199)、[`crates/library/src/fingerprint.rs:111-125`](file:///home/afu/Projects/sheng-ren/crates/library/src/fingerprint.rs#L111-L125)、[`crates/library/tests/fp_golden.rs:199-201`](file:///home/afu/Projects/sheng-ren/crates/library/tests/fp_golden.rs#L199-L201)、[`crates/library/tests/fp_golden.txt:495-496`](file:///home/afu/Projects/sheng-ren/crates/library/tests/fp_golden.txt#L495-L496)

### 3. 空白带裁切算法、取整与内容保护（算法严密）
- **内容行保护**：[`gutter_bands`](file:///home/afu/Projects/sheng-ren/crates/bookconv/src/comiccrop.rs#L67-L95) 扫描 `darkness_plane`（RGB 图每个像素取三个通道最小值，等价于任一通道 < 247 即算暗）。整行只要存在 1 个像素暗于 247，整行即被标记为内容。淡线（246）、杂点（0–246）、对话框跨格线条、画面细节所在的行绝不会被计入空白行。
- **带边界与边缘保护**：首行空白（`y0 == 0`，页顶）与尾行空白（`y1 == h`，页底）被显式跳过，交由已有白边逻辑管辖；厚度小于页高 1% 的细缝直接过滤。
- **收量控制与比例换算**：
  - 留白：每条带至少留 `keep = max(ceil(h * 0.005), 4)` 像素。
  - 目标收量：$T = h - w \times (H_{area} / W_{area})$。当 $T \le 0$（宽度已受限）直接返回空，绝不多收。
  - 总量约束：实际收量取 $\min(S, T, 0.15 \times h)$ 向下取整；若变化量不足页高 1% 则不收。
  - 分摊与取整：先按可用比例整数除法分摊，余量从可收额度最大的带开始逐像素补齐（`avail.saturating_sub(cuts)`），无死循环或溢出漏洞。
  - 切口对称性：每条带裁切上下各半（`top = cut / 2`, `bottom = cut - top`），留白固定居中，且切口区间在带内部互不重叠。
  - 行拼接：[`remove_rows`](file:///home/afu/Projects/sheng-ren/crates/bookconv/src/imgopt.rs#L1551-L1587) 仅剔除切口内的空白行，保持其余图像行原样无损拼合。
- 依据：[`crates/bookconv/src/comiccrop.rs:56-168`](file:///home/afu/Projects/sheng-ren/crates/bookconv/src/comiccrop.rs#L56-L168)、[`crates/bookconv/src/imgopt.rs:1551-1587`](file:///home/afu/Projects/sheng-ren/crates/bookconv/src/imgopt.rs#L1551-L1587)、[`crates/bookconv/src/comiccrop.rs:815-894`](file:///home/afu/Projects/sheng-ren/crates/bookconv/src/comiccrop.rs#L815-L894)

### 4. 跨页拆分与两半收空白处理（拆分判定与顺序不变）
- **拆分与排序**：[`streaming.rs:940-944`](file:///home/afu/Projects/sheng-ren/crates/bookconv/src/optimize/streaming.rs#L940-L944) 的跨页判定仍按原图头信息执行；[`streaming.rs:1048-1052`](file:///home/afu/Projects/sheng-ren/crates/bookconv/src/optimize/streaming.rs#L1048-L1052) 保持 RTL 右半先 / LTR 左半先的插入顺序不变。
- **两半独立裁切**：[`prepare_comic_spread`](file:///home/afu/Projects/sheng-ren/crates/bookconv/src/imgopt.rs#L1223-L1265) 解码原图并记录 `gray_content`，正中切分后左右两半各自组装 `ComicSrc` 传入 `process_comic_page`，各自独立计算 `gutter_bands` 和 `gutter_cuts`。
- **测试验证**：[`optimize/tests.rs:954-1020`](file:///home/afu/Projects/sheng-ren/crates/bookconv/src/optimize/tests.rs#L954-L1020) 明确验证了跨页拆出的两半各自页高缩小、均独立收缩格间空白。
- 依据：[`crates/bookconv/src/optimize/streaming.rs:845`](file:///home/afu/Projects/sheng-ren/crates/bookconv/src/optimize/streaming.rs#L845)、[`crates/bookconv/src/optimize/streaming.rs:947-956`](file:///home/afu/Projects/sheng-ren/crates/bookconv/src/optimize/streaming.rs#L947-L956)、[`crates/bookconv/src/imgopt.rs:1215-1267`](file:///home/afu/Projects/sheng-ren/crates/bookconv/src/imgopt.rs#L1215-L1267)、[`crates/bookconv/src/optimize/tests.rs:954-1020`](file:///home/afu/Projects/sheng-ren/crates/bookconv/src/optimize/tests.rs#L954-L1020)

### 5. Kobo 固定版式与画布尺寸（1072×1448 画布，不拉伸）
- **装饰小图防退化**：装饰小图门控位于 [`process_comic_page:1304`](file:///home/afu/Projects/sheng-ren/crates/bookconv/src/imgopt.rs#L1304)，在裁切之前执行；由于裁切只删行不改宽，且收量上限保证收完后 $h' \ge w \times (H_{area} / W_{area}) > w$，页面短边依然是原图宽（$\ge area.width / 3$），绝不会在收完后退化为装饰小图。
- **画布尺寸保证**：收完后调用 [`comic_layout`](file:///home/afu/Projects/sheng-ren/crates/bookconv/src/imgopt.rs#L935-L950) 与 [`paste_on`](file:///home/afu/Projects/sheng-ren/crates/bookconv/src/imgopt.rs#L1377)，产物画布尺寸严格锁定为 `canvas_w: 1072, canvas_h: 1448` 并按原比例居中补白；XHTML 的 `width: 100%; height: 100%` 铺满该画布，图像本身不会出现拉伸变形。
- 依据：[`crates/bookconv/src/imgopt.rs:935-950`](file:///home/afu/Projects/sheng-ren/crates/bookconv/src/imgopt.rs#L935-L950)、[`crates/bookconv/src/imgopt.rs:1304-1382`](file:///home/afu/Projects/sheng-ren/crates/bookconv/src/imgopt.rs#L1304-L1382)

---

## 置信度

- **评级**：高
- **理由**：本次二审通过纯源码静态追踪，全流程排查了从 CLI 参数、profile 解析、解码、颜色空间分析、空白带检测、整数比例分摊、行拼接，到流式跨页封装与指纹对比的全部代码逻辑，前序两轮返工引入的边缘漏洞在本次 diff 中均有严密代码和单元测试支撑。

---

## 未核实

1. **真机视觉效果**：未在 Kindle 真机上实测《迷宮飯》《死亡筆記》A/B 测试书的实际阅读体感（依任务单流程待合并后由 vinci 手动验证）。
2. **本机真书存储路径**：二审为只读文件审查，未在本地磁盘实际执行 `ls`/`stat` 核验 fish 脚本中指定的书库样本文件物理存在性。
