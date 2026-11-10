# 漫画裁格间空白：去掉格与格之间整行的空白（保守，不切淡线），先出 A/B 测试书

- Task ID: 2026-11-10-comic-gutter-crop
- 负责 bot: sheng-ren
- 状态: pending
- 执行方: opencode-local
- 分支: feat/comic-gutter-crop（2026-11-10 已把 master `d50a889`（含 A 画质统一、B 日漫 rtl、C 跨页拆分）`--no-ff` 合进来：`bdc880f`；在这个分支上接着做，不 rebase）
- 更新时间: 2026-11-10T14:15+08:00
- 引用: 无（PRD：vault `Projects/sheng-ren/prd/2026-11-10-comic-layout-direction.md` 的 D 节）

## 需求原文
> 4. 需要裁格间空白
>
> （sheng-ren 此前提醒：小屏能多露出画面，但要小心切到淡线，得先上真机比。）

## 输入
- 背景：vault `Projects/sheng-ren/context.md`（2026-10-30 裁页码、杂点；「缩略图太小会把淡色内容看成被裁掉」的坑）；仓库 `docs/decisions.md`、`docs/typesetting.md` 漫画部分
- KCC 只作功能参照：README 写它有 `--interpanelcrop`（0 关 / 1 横向 / 2 横竖），「Crop empty sections」。**不读 KCC 源码**，下面的做法是从头设计的
- 原理：漫画页一般比阅读范围瘦高（高度受限）。去掉格与格之间横向整行的空白，页变矮，保比缩进阅读范围时整体变大。页已经不比阅读范围瘦高（宽度受限）时收横向空白没用
- 现有口径：`crates/bookconv/src/comiccrop.rs` 的 `DARK_BELOW = 247`（比它暗才算内容，淡线、浅网点都算）；裁白边 `imgopt.rs` `trim_comic` / `trim_bounds_with`；黑底判定 `comiccrop::dark_background` / `imgopt::dark_fill`；处理顺序在 `imgopt.rs:1247` `process_comic_page`（切页码 → 裁白边 → 黑底判定 → 装饰小图分支 → 对比度 / 伽马 → 缩放补白）；整页走 `imgopt.rs:1165` `prepare_comic_page_limited`、C 单拆出的跨页两半走 `imgopt.rs:1195` `prepare_comic_spread`，两条都调 `process_comic_page`
- 涉及文件：
  - `crates/bookconv/src/comiccrop.rs`（新增：找空白带、算收多少的纯函数和常量，单元测试）
  - `crates/bookconv/src/imgopt.rs`（修改：`ComicPage` 加开关；`process_comic_page` 在裁白边之后、黑底判定和装饰小图分支之后、对比度 / 缩放之前收空白带）
  - `crates/bookconv/src/optimize/mod.rs`（修改：`OptimizeOpts::comic_gutter_crop`、`for_profile`；COMIC_VERSION 加一并写历史）
  - `crates/bookconv/src/optimize/streaming.rs`（只在传开关需要时改）
  - `crates/bookconv/src/bin/epub_optimize.rs`（修改：加 `--gutter-crop` 选项，强制开，给 A/B 测试书用；同 `--keep-color` 的先例）
  - `crates/profile/src/lib.rs`（修改：新键 `comic_gutter_crop: bool`，缺省 false）
  - `crates/library/src/fingerprint.rs`（修改：开了的模式漫画段加新字母，如 `L`）
  - `crates/library/tests/fp_golden.rs`、`fp_golden.txt`（加 mutation；旧行只因 COMIC_VERSION 变化而变：漫画行变、文字书行 0 变化）
  - `docs/typesetting.md`、`docs/devices.md`、`docs/decisions.md`、`docs/development.md#版本号`
  - **不改** `crates/profile/profiles/*.toml`：四台先都不开，真机比过再另开单打开

## 约束
- 不做：不裁竖向（整列）空白（vinci 2026-11-10 确认只裁横向）；不动页顶、页底的空白（裁白边管）；不改 profiles/*.toml；不改裁白边、杂点、页码、黑底的现有算法和常量；不做条漫；不改 install.sh
- 空白行：整行没有一个像素比 `DARK_BELOW`（247）暗（RGB 任一分量 < 247 就算暗）。**不放过杂点**（有一个暗点这一行就不算空白；宁可少裁）
- 空白带：连续空白行，高 ≥ 页高 × `GUTTER_MIN_BAND`（0.01），且上下两侧都有内容（不在页顶、页底）
- 每条带收完后留 `max(页高 × GUTTER_KEEP (0.005), 4px)`，留下的放在带的正中（上下各一半）
- 总量：所有带可收的量加起来 S；目标收量 T = 页高 − 页宽 × (阅读范围高 / 阅读范围宽)（收到页面比例正好等于阅读范围比例为止）；T ≤ 0（已经宽度受限）→ 不收；实际收 `min(S, T, 页高 × GUTTER_MAX_TOTAL (0.15))`，不够收满时各带按可收量的比例分摊（取整后误差给最高的那条带）
- 不收的页：黑底页（`dark_fill` 判出来的）、装饰小图（短边 < 阅读范围宽 1/3）、彩色内容页（`!is_gray_content()`）、动图；收完高度变化 < 页高 1% 的也不收（不值得重编码）
- 阅读范围用这一页实际要缩进去的那个（`area`，固定版式就是画布）
- 常量都写注释（含义、为什么这么定、2026-11-10 待真机比）
- 跨页拆分（C 单，已在 master）：拆出的两半也走 `process_comic_page`，开了就照常收空白带，不另写逻辑；C 单的 `comic_split_spreads`、指纹 `W` 和拆分判定都不改
- 指纹 / 版本：COMIC_VERSION 66 → 67（历史写一行，注明「四台都没开，产物不变，只为新键」）；新字母只在开了的模式出现
- 只在上面的分支工作；不合并；不改「涉及文件」以外的文件，确需改时写进「结果 → 偏离」
- 提交信息用中文，`feat:` 等前缀 + 正文，不加 Co-Authored-By；正文写「没跑真书回归、没上真机；四台 profile 都没开，产物不变（COMIC_VERSION 加一会让漫画重算一次，字节应不变、不重传，待本机回归确认）」
- 不读密钥、`.env`、`auth*`；clean-room：不读 GPL 代码（含 KCC 源码）
- 只对改动的 Rust 文件跑 rustfmt；编译用 `CARGO_TARGET_DIR=~/.cache/sheng-ren-target`
- 风险点：漫画规则（有意改漫画产物的新例外，开之前要 vinci 真机看）→ 合并前 agy 二审

## 分步任务
1. [ ] `comiccrop.rs`：`pub(crate) fn gutter_bands(px: &[u8], w: u32, h: u32) -> Vec<(u32, u32)>`（灰度平面上的空白带 `[y0, y1)`）和 `pub(crate) fn gutter_cuts(bands, w, h, area) -> Vec<(u32, u32)>`（每条带要删掉的行区间），按「约束」实现。单元测试：
   - 三格页（白底，格之间 40px 白带）在 1072×1448 范围下收掉的量 = min(S, T, 15%)，每条带留 ≥ max(0.5% 页高, 4px)
   - 白带里有一条 1px、灰度 246 的淡线：这条带被分成两段或不算（淡线那一行不删）
   - 白带里有一个 1px 的暗点：那一行不算空白
   - 页已经比阅读范围宽（T ≤ 0）：不收
   - 带高 < 1% 页高：不收
   - 页顶、页底的白：不算带
2. [ ] `imgopt.rs`：`ComicPage` 加 `gutter_crop: bool`（`streaming.rs` 里跨页两半用的 `ComicPage` 也要传）；在 `process_comic_page` 里裁白边、黑底判定、装饰小图分支之后，`!bg` 且灰度内容时按 `gutter_cuts` 把这些行删掉（拼接剩下的行），再照常对比度 / 伽马 / 缩放补白。开关关着时代码路径和现在完全一样
3. [ ] `optimize/mod.rs`、`profile/src/lib.rs`：新键、`OptimizeOpts::comic_gutter_crop`、COMIC_VERSION 加一；`epub_optimize.rs` 加 `--gutter-crop`（usage、文件头注释一起改）
4. [ ] `optimize/tests.rs`：合成漫画书一页（白底三格），开 / 关各跑一遍：关的产物和现在逐字节相同；开的那页图里格的画面比关的大（量格的高度，结果里写数字）；再加一张 1600×1100 的白底多格跨页，开了以后照常拆成两页、两半都收了空白带
5. [ ] `fingerprint.rs` 新字母；`fp_golden.rs` 加 mutation；`UPDATE_FP_GOLDEN=1 cargo test -p library --test fp_golden`；`git diff`：漫画行只因 COMIC_VERSION 变，文字书行 0 变化
6. [ ] 文档：`docs/typesetting.md`（新一节，写清三道兜底）、`docs/devices.md`（新键一行，四台「不开，待真机比」）、`docs/decisions.md`（一行：2026-11-10 维护者要裁格间空白，先 A/B）、`docs/development.md#版本号`
7. [ ] 在「结果」里写好给 vinci 本机出 A/B 测试书的 fish 命令（《迷宮飯》《死亡筆記》各一卷，只出 Kindle：`epub-optimize --device=kindle 书.epub 关.epub`、`epub-optimize --device=kindle --gutter-crop 书.epub 开.epub`，再各转 KFX），**不自己上设备**
8. [ ] `cargo test --workspace` 全过；`cargo clippy --workspace --all-targets -- -D warnings` 0 警告

## 验收标准
- [ ] 功能：开了的页只删格间整行空白；淡线、杂点所在的行一行都不删；每条带至少留最小间距；总量不超过 15%、不超过收到阅读范围比例所需
- [ ] 功能：黑底页、宽度受限的页、小图、彩色页不动
- [ ] 测试：`cargo test --workspace` 全部通过；clippy 0 警告；第 1、4 步每条都有断言
- [ ] 不回归：开关关着产物逐字节不变（测试证明）；四台 profile 没开；文字书指纹 0 变化
- [ ] 「不做」的事确实没做
- [ ] 真机（vinci，合并后）：Kindle 上比《迷宮飯》《死亡筆記》各一卷的 A/B 测试书（vinci 2026-11-10 定），专门看留白构图、无格框的页；比过再另开单定四台开不开

> 2026-11-10 合入 master 后核对（sheng-ren）：需求、约束没变，只按 master 现状改了行号 / 函数名（`process_comic_page`）、COMIC_VERSION 现值（66）、跨页两半也要收（第 4 步多一条断言）。四台缺省不开、只裁横向，同 PRD D 节。

## 交付物
代码提交到上面的分支并推送；「结果」填好；状态改为 done 或 blocked。

## 给执行方的指令
```
执行任务单 .dev/tasks/2026-11-10-comic-gutter-crop.md：先把状态改为 accepted 并提交，按「分步任务」做完，填好「结果」，状态改为 done（做不下去改为 blocked 并写明原因），提交并推送分支。
```

## 结果
<!-- 执行方填写 -->
- 执行方 / 开始–结束时间：
- base..head：
- 提交：| 步骤 | 提交号 | 文件 | 说明 |
- 测试：| 命令 | 通过 / 失败 / 未跑 | 摘要 |
- 遇到的问题：
- 偏离：无
- 未完成：

## 审查
<!-- 负责 bot 填写 -->
- 验收：
- 二审：
- 合并：
