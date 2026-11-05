# 二审结论：feat/kobo-comic-fxl（05ddc2f..75711d9）

**结论：有 1 条阻断。** Kobo 百分比路径里，不管图片原来是什么比例，纯图页都写成 `width:100%;height:100%`。Kindle 有 KFX 写出器按比例缩放兜底，Kobo 没有，比例不符的页可能被拉伸变形。其余几项重点核对后没有发现阻断问题。

## 阻断

**1. Kobo 百分比路径不看图片比例，可能拉伸变形**
- `comicfxl.rs:115-139` 的 `page()` 只要判定为纯图页（`image_only_body`），`percent=true` 时就把 style 写死成 `width:100%;height:100%`，没有调用 `fills_canvas` 检查比例。
- Kindle 不会出问题：KFX 写出器不读 EPUB 样式，自己会检查比例（`kfx/src/write/layout.rs:286-296`）；模块注释 `comicfxl.rs:10-11` 写的"不会被拉伸"也只对 Kindle 成立。Kobo 由 Nickel 直接渲染这份样式，宽高都定死时，比例不符的图会被拉伸。
- 确实存在比例不符的纯图页：
  - 装饰小图（短边不到画布宽的 1/3）只裁边、不补白，见 `imgopt.rs:971` 的注释和 `imgopt.rs:2816` 的测试用例 `(png(401,1203), false)`；
  - `prepare_comic_page_for_epub` 返回 `None` 时保留原图：超过 6400 万像素、解不开、动图。
- 跟 master 比不算新回归：旧 Kobo 写 `1072px×1448px`，同样会拉伸。但这次是专门为 Kobo 写的新路径，而且注释让人以为不会拉伸。
- 修法：百分比模式只在 `fills_canvas` 成立时写 100%×100%，否则只写 `width:100%`（高度自动），或者 `object-fit:contain` 加真机验证。需要把图片尺寸传进 `page()`，或者在调用处（`optimize/mod.rs:1152`）判断。
- 置信度：
  - CSS 语义（宽高都写定就会拉伸）：**高**；
  - 真书里遇到的频率：**中**（取决于书里有多少装饰小图或无法处理的页）；
  - 实际有没有生效，还要看下面"线索 1"的高度链问题：只有 img 的父元素有确定高度时才会真的拉伸。
- 如果维护者愿意接受这个已知风险，可以降为"建议"。

## 建议

**2. 注释和文档把"不满屏"归因到单一变量，依据不足**
- `kobo.toml:21` 写"像素画布写法不满屏"，`kepub.rs:8` 和 `docs/kepub.md` 写"加了 koboSpan 整页图不满屏"。
- 但 A 包同时改了"像素画布 + 包 koboSpan"两个变量（`docs/decisions.md` 新增的 2026-11-05 条目）。B/C/E 各改了什么，任务单没写；`/workspace/kobo-ab/README.txt` 执行方也没读到（任务单第 93 行）。
- 建议改成"A（像素画布 + 包 koboSpan）不满屏，D 满屏"，不要写成单因结论。（**未核实** B/C/E 的组成。）

**3. 非漫画的固定版式书也被改了，没有真机验证**
- `kepub.rs:199-218` 的 `pre_paginated` 只看 OPF 声明，`kepub.rs:232` 对所有 pre-paginated 书的纯图页都不包 koboSpan，出版社原生的固定版式书（非漫画）也在内。
- 这些书不经过 `comicfxl`，没有高度链 CSS，和 D 的结构不一样。
- 任务单允许"pre-paginated 或漫画产物二选一"，所以不违规；但真机只验证过漫画。建议在文档里写清这个影响范围，或者改成只认 `book-type=comic`。

**4. 所有 Kobo 产物都会重建**
- `kepub::VERSION` 从 1 改成 2（`kepub.rs:18`），fp_golden 里 kobo 的文字书行也全部变了，下次 sync 会把 Kobo 上所有书重新生成、重新传一遍。
- 改版本号本身是对的（固定版式文字书的产物确实变了），但传输量和耗时风险应该在合并前告诉维护者。

## 线索

**1. 高度链没覆盖原书自己的中间层，可能和 D 不完全等价**
- `CSS_RULES_PERCENT`（`comicfxl.rs:42`）只给 html、body、`#book-columns`、`#book-inner` 写了 `height:100%`。
- 原书 img 外面常有 `<div>`/`<center>`（测试样例 `comicfxl.rs:160`、`kepub.rs:702` 就是 `<div><img>`）。这些中间层高度是 auto，img 的 `height:100%` 实际会退化成 auto。
- 对画布比例的页，结果一样能铺满；但和"真机 D 那本"的 DOM 是否一致，**未核实**（没读到 D 包）。
- 验收记录（任务单第 110 行）说"与 D 结构一致"，但没写 img 的父层结构。
- 这一条也决定了阻断 1 实际会不会触发。

**2. 纯图页没有 koboSpan 后，阅读进度、书签是否正常，未核实**
- 需求原文的"侧载书签/笔记可用"没说明是在文字书上还是漫画 D 上测的。
- 建议真机复测时专门翻漫画中间页，看进度条和书签。

**3. `docs/kepub.md` 末尾链接的锚点没核实**
- 改成了 `devices.md#kobo-clara-bw2026-11-05-真机确认漫画方案-d`，`devices.md` 里的标题有没有同步改，没核实（diff 输出中间被截断，没看到）。

**4. `COMIC_VERSION` 没加一**
- 改成靠指纹里的 `F` 加 `kepub2` 触发重建（任务单第 98 行），偏离已经写明。
- 逻辑上能保证 Kobo 漫画重建；Kindle 产物没变，不加版本号是合理的。

## 重点项逐条

| 重点 | 结论 | 依据 | 置信度 |
|---|---|---|---|
| kepub 纯图页不加 koboSpan | **符合**：固定版式、纯图页、开着包装时，只加包装、返回 0 个 span；关了包装就整章不动；文字页照常按句加 | `kepub.rs:232-242`、`kepub.rs:445`；测试 `kepub.rs:647`、`kepub.rs:702-716` | 高 |
| comicfxl 百分比路径会不会拉伸 | **有风险**，见阻断 1 / 线索 1 | `comicfxl.rs:130-138`、`imgopt.rs:971`、`imgopt.rs:2816` | 中 |
| Kindle 像素路径不变 | **按代码看等价**：`percent=false` 时 style 字符串、`css_rules(false)==CSS_RULES`、memfind 去重逻辑都和原来一致；`comic_fixed_layout_percent` 只出现在 `kobo.toml`；`image_only_body` 里统计 img 改用 `is_local`，只有带命名空间前缀的 `<x:img>` 会有差别，而且找 img 时仍用 `t.is("img")`，`page()` 的输出不变 | `comicfxl.rs:44-51`、`comicfxl.rs:90-106`、`optimize/mod.rs:1062-1064`、`optimize/mod.rs:1229`；git grep 结果 | 高（代码阅读）；Kindle 真书逐字节回归**未核实** |
| `comic_fixed_layout_percent` 偏离 | **合理，已写进「偏离」**：profile 校验要求和 `comic_fixed_layout` 一起开；指纹 `F` 只在 `f` 里面追加；没开的模式指纹不变 | `profile/src/lib.rs:708-713`、`fingerprint.rs:105`、`kobo.toml:21` | 高 |
| fp_golden 只有 kobo 行变 | **符合**：只有一个改动块 `@@ -355,118 +355,118 @@`，看到的首尾行都以 kobo 开头（`kepub1→kepub2`，漫画段 `f→fF`） | `crates/library/tests/fp_golden.txt:355-472` | 高（中段输出被截断，没有逐行看） |
| 设备行为风险 | 没动 install.sh、投递、USBMS（diff stat 里没有这些文件）；风险集中在阻断 1、建议 3/4 和线索 2 | `git diff --stat` | 高 |
| 超范围改动 | README、docs 三篇、`epub_to_kepub.rs` 注释、`optimize/tests.rs`、`fingerprint.rs`、`profile/src/lib.rs` 都已列进「偏离」，没有漏报 | 任务单第 96-97 行 | 高 |

## 未核实
- 没有跑过任何测试、clippy 或真书回归；Kindle 逐字节不变只是读代码的结论。
- 没读到 `/workspace/kobo-ab/` 里的 D 包和 README，所以 B/C/E 的组成、D 里 img 的父层结构都没核实。
- `docs/devices.md` 新锚点对应的标题、`README.md`、`docs/architecture.md`、`docs/development.md` 的改动没有逐行看。
- `optimize/tests.rs:415` 的集成测试只看了断言片段（第 449 行），没看完整的测试书结构。
- fp_golden 里 `kobo comic_fixed_layout` 那一行（关掉固定版式时）改后的值没看到。

## 置信度
- 阻断 1：CSS 语义层面**高**，真书里实际触发的频率**中**。
- Kindle 不变、fp_golden 只有 kobo 行：**高**（读代码加 diff 块头）。
- 其余线索：**低到中**，需要真机或 D 包来确认。
