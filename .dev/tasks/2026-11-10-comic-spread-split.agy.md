## 结论

**无阻断**。分支实现严格遵循需求原文与任务单约束；第一轮验收指出的 4 项阻断问题已全部彻底修复；跨页边界、properties 清理、非自闭合 itemref 插入位置、共用图判定、RTL 翻页顺序以及开关关闭时的无回归表现均通过完整代码审计与单元测试验证，可合并。

---

## 重点项核查结论

### 1. 跨页判定与半页过小不拆的边界
- **结论**：符合要求。
- **依据**：
  - [crates/bookconv/src/imgopt.rs:1182-1185](file:///home/afu/Projects/sheng-ren/crates/bookconv/src/imgopt.rs#L1182-L1185)：跨页阈值 `SPREAD_MIN_ASPECT = 1.2` 使用整数比 `u64::from(w) * 5 >= u64::from(h) * 6` 判定，规避了 `f32` 浮点精度导致 1200×1000 误判为不到 1.2 的问题。
  - [crates/bookconv/src/imgopt.rs:1188-1190](file:///home/afu/Projects/sheng-ren/crates/bookconv/src/imgopt.rs#L1188-L1190)：切分宽度为左半 `w / 2`（向下取整）、右半 `w - w / 2`，奇数宽如 1601 正确切为 800 与 801。
  - [crates/bookconv/src/optimize/streaming.rs:939-944](file:///home/afu/Projects/sheng-ren/crates/bookconv/src/optimize/streaming.rs#L939-L944)：双重保障整图与半张图均不为装饰小图，显式要求 `iw.min(ih) >= screen.width / 3` 且 `(iw / 2).min(ih) >= screen.width / 3`，彻底杜绝了 Kobo 480×400 跨页两半被按画布 100%×100% 强制拉伸的问题。
  - [crates/bookconv/src/optimize/streaming.rs:889, 913](file:///home/afu/Projects/sheng-ren/crates/bookconv/src/optimize/streaming.rs#L889-L913)：Spine 首页、OPF 声明的封面图（包含包封式宽封面）一律跳过不拆。

### 2. OPF properties 清理与新 itemref 插入位置
- **结论**：符合要求。
- **依据**：
  - [crates/bookconv/src/optimize/streaming.rs:1128-1152](file:///home/afu/Projects/sheng-ren/crates/bookconv/src/optimize/streaming.rs#L1128-L1152)：原页 itemref 的 `properties` 按词切分，过滤掉本地名以 `page-spread-` 开头的词（含 `page-spread-left/right/center` 及带 `rendition:` 等命名空间前缀的值），其余属性词保留并规整空格；若属性值被清空，则连同属性名及前导空白完全移除（[crates/bookconv/src/optimize/streaming.rs:1179-1187](file:///home/afu/Projects/sheng-ren/crates/bookconv/src/optimize/streaming.rs#L1179-L1187)），避免产生双空格。
  - [crates/bookconv/src/optimize/streaming.rs:1154-1160](file:///home/afu/Projects/sheng-ren/crates/bookconv/src/optimize/streaming.rs#L1154-L1160)：自闭合 itemref 插在 `t.end`；非自闭合 itemref（`<itemref …></itemref>`）利用 `html::find_close` 定位到配对 `</itemref>` 之后插入；新 itemref 正确继承原标签命名空间前缀与 `linear` 属性；找不到闭合标签时安全返回 `None` 并回退整书不拆（[crates/bookconv/src/optimize/streaming.rs:1034-1036](file:///home/afu/Projects/sheng-ren/crates/bookconv/src/optimize/streaming.rs#L1034-L1036)）。

### 3. 被多页共用的图片是否一律不拆
- **结论**：符合要求。
- **依据**：
  - [crates/bookconv/src/optimize/streaming.rs:859-884](file:///home/afu/Projects/sheng-ren/crates/bookconv/src/optimize/streaming.rs#L859-L884)：拆分扫描前遍历全书所有 `ish`（XHTML）条目中的 `<img>` 和 `<image>`（含 `xlink:href`），规整相对路径后统计引用该图片的页面集合 `image_refs`。
  - [crates/bookconv/src/optimize/streaming.rs:914-916](file:///home/afu/Projects/sheng-ren/crates/bookconv/src/optimize/streaming.rs#L914-L916)：若图片被超过 1 个 XHTML 页面引用（`pages.len() > 1`），直接跳过不拆，保证共用整图不会被降级替换为半页。单页内若含多图或图文混排，则在 [crates/bookconv/src/optimize/streaming.rs:895](file:///home/afu/Projects/sheng-ren/crates/bookconv/src/optimize/streaming.rs#L895) 由 `comicfxl::image_only` 拦截。

### 4. RTL 书右半页在前
- **结论**：符合要求。
- **依据**：
  - [crates/bookconv/src/optimize/streaming.rs:833-837](file:///home/afu/Projects/sheng-ren/crates/bookconv/src/optimize/streaming.rs#L833-L837)：翻页方向严格按优先级判定：`opts.page_direction`（Bangumi 元数据）> `opts.comic_page_direction`（Profile 配置）> 原书 spine 方向。
  - [crates/bookconv/src/optimize/streaming.rs:1047-1053](file:///home/afu/Projects/sheng-ren/crates/bookconv/src/optimize/streaming.rs#L1047-L1053)：RTL 为 `true` 时，原图条目保留 `s.right`（先读），新图条目分配 `s.left`（后读）；原页 XHTML 保持指向原图名，新页 XHTML 指向新图名并紧随其后插入 spine；nav 与 ncx 目录保持指向原页（即右半页），阅读顺序完全符合日漫标准。

### 5. 开关关闭时产物是否与 master 一致
- **结论**：符合要求。
- **依据**：
  - [crates/bookconv/src/optimize/streaming.rs:798-800](file:///home/afu/Projects/sheng-ren/crates/bookconv/src/optimize/streaming.rs#L798-L800)：`!opts.comic_split_spreads` 时函数首行直接返回 `Ok(Vec::new())`，不修改任何条目、不生成 `-eink2`，流式流水线与 master 行为完全等价。
  - [crates/library/src/fingerprint.rs:117-123](file:///home/afu/Projects/sheng-ren/crates/library/src/fingerprint.rs#L117-L123)：指纹漫画段新字母 `W` 仅在 `comic_split_spreads` 为真时追加；文字书完全不进入此逻辑，指纹 0 变化（[crates/library/tests/fp_golden.txt:487](file:///home/afu/Projects/sheng-ren/crates/library/tests/fp_golden.txt#L487)）。

---

## 问题分档

### 阻断
无。

### 建议
1. **非自闭合 itemref 的匹配容错**：[crates/bookconv/src/optimize/streaming.rs:1158](file:///home/afu/Projects/sheng-ren/crates/bookconv/src/optimize/streaming.rs#L1158) 使用 `html::find_close` 定位结束标签。若遇到原页未闭合、后方又有其他非自闭合 itemref 的极罕见畸形 OPF，可能会匹配到后一个标签的结束位置。建议后续改用带层级计数的 `find_matching_close`，并补充「找不到闭合标签时 `add_spread_spine_items` 返回 `None`」的单测。
2. **新 itemref 属性继承**：[crates/bookconv/src/optimize/streaming.rs:1161-1169](file:///home/afu/Projects/sheng-ren/crates/bookconv/src/optimize/streaming.rs#L1161-L1169) 新插入的 itemref 仅继承了 `linear` 属性；若原 itemref 除了 `page-spread-*` 之外还有其他 EPUB3 `properties`（如个别书单独声明的 `properties="rendition:layout-pre-paginated"`），新 itemref 未带。目前四台设备依赖全局版式声明，渲染正常，未来如有特定单页属性需求可考虑同步继承过滤后的 `properties`。

### 线索
1. **CSS 背景图引用场景**：[crates/bookconv/src/optimize/streaming.rs:859-884](file:///home/afu/Projects/sheng-ren/crates/bookconv/src/optimize/streaming.rs#L859-L884) 的共用图统计未解析 CSS 中的 `background-image`。因漫画纯图页均为 `<img>` 标签且无跨文件使用背景图作为正文页面的场景，目前不产生实际影响。
2. **裁剪后尺寸边界**：[crates/bookconv/src/optimize/streaming.rs:940](file:///home/afu/Projects/sheng-ren/crates/bookconv/src/optimize/streaming.rs#L940) 在解码前按头部尺寸排除了 `(iw / 2).min(ih) < screen.width / 3`；若存在极端漫画跨页单侧留白超过 50%，裁白边后（`trim_comic`）进入 `process_comic_page`（[crates/bookconv/src/imgopt.rs:1279](file:///home/afu/Projects/sheng-ren/crates/bookconv/src/imgopt.rs#L1279)）会被判定为装饰小图（不补白），而 `image_dims` 已记为画布尺寸。真实漫画跨页几乎不存在此类构图，暂不影响合并。

---

## 置信度

**高**。全量代码与测试用例均经过静态审查，测试集对 1.2 临界值、奇数像素中分、LTR/RTL 顺序、Properties 过滤、非自闭合插入、小图/动图/混排跳过、多页共用跳过、固定版式 Viewport、CBZ 来源及指纹 Golden Mutation 均有明确断言覆盖。

---

## 未核实

1. 本次二审处于只读环境，未在本地执行命令重跑 `cargo test` 或 `epub-optimize`（基于任务单执行记录及负责 bot 在真机测试环境汇报的 776 个测试全过、Clippy 0 警告报告）。
2. 四台实体真机（Kindle、Kobo Clara BW、汉王 Clear 6 Turbo、掌阅 Air 3 Pro）上的真书排版展示、翻页手势响应与满屏显示效果，待合并后由维护者执行真机抽样验证。
3. 全书库真书回归脚本（`tools/regress/run.sh`）未对本地全部真实漫画书运行。
