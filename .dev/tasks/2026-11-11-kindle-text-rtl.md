# Kindle 文字书横排却从右往左翻：流式 KFX 不再照 spine 写 rtl

- Task ID: 2026-11-11-kindle-text-rtl
- 负责 bot: sheng-ren
- 状态: done
- 执行方: opencode-local
- 分支: kindle-text-rtl（从最新 master `0c92e5c` 切出）
- 更新时间: 2026-11-11T11:52+08:00
- 引用: 无（竖排 PRD：vault `Projects/sheng-ren/prd/2026-11-11-vertical-text.md`，本单是从它拆出来的单独修复，不做竖排）

## 需求原文
> Kindle 翻页方向问题先单独修

（背景：原书本来竖排、spine 写了 `page-progression-direction="rtl"` 的文字书，我们的 KFX 写出器只会写横排，Kindle 上就成了「横排文字 + 从右往左翻页」。vinci 2026-11-11 定：先单独修，竖排以后另做。）

## 输入
- 背景：vault `Projects/sheng-ren/context.md`；仓库 `docs/kfx.md`、`docs/decisions.md`（2026-10-15/16「三台都照书自己写的方向翻页」、2026-11-10 日漫 Bangumi rtl）
- 复现（sheng-ren 2026-11-11 在 box 上用 master `0c92e5c` 跑）：测试书是自写的最小 EPUB 3，`html{writing-mode:vertical-rl}` + `<spine page-progression-direction="rtl">` + `<meta name="primary-writing-mode" content="vertical-rl"/>`（vault 外的 box 测试包 `vkit` 01 号；下面「分步任务 1」里照这个写法自己合成，不依赖那个文件）：
  ```
  epub-optimize --device=kindle 竖排01.epub k01.epub   # spine 照旧 rtl
  epub-to-kfx k01.epub k01.kfx
  kfx-dump k01.kfx  →  $538 文档数据：$192: '$376'（横排）, $560: '$559'（从右往左）
  ```
  同一本去掉 spine rtl（09 号）→ `$192: '$376', $560: '$557'`（从左往右）。即横排写法 + rtl 翻页只因 spine 的 rtl 而来。未上真机。
- 方向从哪来：
  - `crates/bookconv/src/epubbook.rs:213` `meta.rtl = spine_rtl(&opf_text)`（`spine_rtl` 在 `:82`，口径是 `direction::spine_direction`）
  - `crates/kfx/src/write/mod.rs:287` `let direction = if book.meta.rtl { DIR_RTL } else { DIR_LTR };`
  - 用处：`crates/kfx/src/write/entities.rs:474`（固定版式文档数据）、`:483`（流式文档数据），`crates/kfx/src/write/layout.rs:204`（固定版式整页图的版面模板）。排版方向 `$192` 在 `entities.rs:475/482`、`layout.rs:205` 写死 `WRITING_HORIZONTAL`（`$376`），本单不动
  - 固定版式判定：`crates/kfx/src/write/layout.rs:66` `fixed_canvas`（OPF 里有固定版式声明）；纯图页判定：`layout.rs:128` `single_image`
- 漫画 rtl 现状（不能坏）：Kindle 的漫画都走固定版式（`kindle.toml` `comic_fixed_layout = true`，`comicfxl::opf` 写声明），Bangumi 日漫 rtl 经 OPF spine → `meta.rtl` → `$560 $559`；有测试 `crates/kfx/src/write/tests.rs:559`（固定版式 rtl 写 `$559`）和 `crates/library/tests/flow.rs:2838`（四台日漫 rtl，含 KFX 的 `$560`）
- 涉及文件：
  - `crates/kfx/src/write/mod.rs`（修改：定 `direction` 的那一行改成按规则取；`WRITER_VERSION` 25 → 26，加一条说明）
  - `crates/kfx/src/write/tests.rs`（新增测试）
  - `docs/kfx.md`、`docs/decisions.md`（各加一条）

## 约束
- 不做：竖排（`$192` 照旧横排）；不改 `crates/bookconv` 的 EPUB/KEPUB 产物（汉王、Kobo、Bigme 照旧原样带 spine rtl——那几台阅读器自己按书里的 writing-mode 排，竖排书 rtl 是对的）；不改漫画、日漫 rtl 的任何行为；不改 `epubbook::Meta::rtl` 的口径
- 只在上面的分支工作；不合并；不改「涉及文件」以外的文件，确需改时写进「结果 → 偏离」
- 提交信息用中文，`fix:` / `test:` / `docs:` 等前缀 + 正文，不加 Co-Authored-By
- 不读密钥、`.env`、`auth*`
- 风险点：**设备行为**（Kindle 上 spine 写 rtl 的流式文字书改成从左往右翻）。不是小改动：改设备行为、跨 4 个文件，要 agy 二审

## 规则（要实现的行为）
KFX 写出器现在只会写横排。横排的书按从右往左翻是错的，所以：
1. **固定版式**（`fixed_canvas(book).is_some()`）：照旧，`meta.rtl` → `$559`。漫画、日漫走这里。
2. **流式、而且每个 spine 文档都是纯图页**（流式漫画，比如 `epub-to-kfx` 直接转的、没走固定版式的漫画）：照旧，`meta.rtl` → `$559`。
3. **其余流式书**（有文字的书）：一律 `$557`（从左往右），不管 spine 写没写 rtl。spine 写了 rtl 而被改成 ltr 时，在 `warnings` 里加一行（如「spine 写了从右往左，KFX 只会横排，按从左往右翻」），方便以后查。
4. 等以后 KFX 写出器支持竖排（竖排 PRD 第二期）再按排版方向放开，本单在代码注释里写明这条。

## 分步任务
1. [ ] `crates/kfx/src/write/tests.rs` 先写测试（照 `:559` 固定版式 rtl 那个测试的造书方式）：
   - a. 流式文字书 + spine rtl（+ `html{writing-mode:vertical-rl}` 样式）→ 文档数据 `$560` 是 `DIR_LTR`、`$192` 仍是 `WRITING_HORIZONTAL`，`warnings` 里有那一行
   - b. 流式文字书、spine 没写 rtl → `$560` 是 `DIR_LTR`，`warnings` 没有那一行（产物与改前逐字节相同，可对比改前的字节或关键字段）
   - c. 固定版式 + spine rtl → 仍是 `DIR_RTL`（已有 `:559` 测试覆盖，确认不改它也能过）
   - d. 流式、两页都是纯图页 + spine rtl → 仍是 `DIR_RTL`
   - 自检：先跑 `cargo test -p kfx`，a、d 中至少 a 应失败（证明测到了 bug）
2. [ ] `crates/kfx/src/write/mod.rs:287`：按「规则」取 `direction`。纯图页判定复用 `layout.rs` 的口径（一个文档只有一个块、块是图），不要另写一套；需要的话在 `layout.rs` 抽一个小函数供两处共用（这时把 `layout.rs` 记进「偏离」）。自检：`cargo test -p kfx` 全过
3. [ ] `WRITER_VERSION` 25 → 26，说明照前面几条的格式写：改了什么、哪些书的字节会变（只有 spine 写了 rtl 的流式文字书；别的书逐字节不变）。自检：`cargo test --workspace` 全过（`library` 的指纹测试、`fp_golden.txt` 如果因写出器版本变化而变，照实更新并在「结果」里写变了几行、为什么）
4. [ ] 文档：`docs/kfx.md` 写出器版本一节加 26；`docs/decisions.md` 加一条「2026-11-11 Kindle 流式文字书一律从左往右翻（KFX 只写横排）；固定版式、纯图流式漫画照旧照 spine」，注明推翻 10-15/10-16「照书的方向翻」里 Kindle 流式文字书这一种情况
5. [ ] 合成回归：用测试 a 的那本书走 `epub-optimize --device=kindle` → `epub-to-kfx` → `kfx-dump`，确认 `$560: '$557'`；再拿一本 rtl 漫画（测试 c 或 `flow.rs:2838` 那种造法）确认仍是 `$559`。把命令和输出贴进「结果」

## 验收标准
- [ ] 功能：spine 写 rtl 的流式文字书，KFX 文档数据 `$560` 为 `$557`，`warnings` 有提示；固定版式和纯图流式漫画的 rtl 不变
- [ ] 测试：`cargo test --workspace` 全部通过；`cargo clippy --workspace --all-targets -- -D warnings` 0 警告；新增测试 a、b、d 都在，a 在改代码前失败
- [ ] 不回归：`crates/library/tests/flow.rs` 的日漫 rtl 测试（`bangumi_japanese_comic_builds_rtl_and_others_stay_as_is`）照过；spine 没写 rtl 的书 KFX 逐字节不变；EPUB / KEPUB 产物逐字节不变（本单不碰 bookconv）
- [ ] 「不做」的事确实没做：`$192` 仍写死横排；没改 `bookconv`
- [ ] 结果里写明「没跑真书回归、没上真机」（真机由 vinci 验：sync 一本原书竖排的台版书，点右侧是下一页）

## 交付物
代码提交到上面的分支并推送；「结果」填好；状态改为 done 或 blocked。

## 给执行方的指令
```
执行任务单 .dev/tasks/2026-11-11-kindle-text-rtl.md：先把状态改为 accepted 并提交，按「分步任务」做完，填好「结果」，状态改为 done（做不下去改为 blocked 并写明原因），提交并推送分支。
```

## 结果
<!-- 执行方填写 -->
- 执行方 / 开始–结束时间：opencode-local / 2026-11-11 11:41–11:51 +08:00
- base..head：`0c92e5c..HEAD`（代码到 `cd3c23b`，另有任务单 accepting / 本结果提交）
- 提交：

  | 步骤 | 提交号 | 文件 | 说明 |
  | --- | --- | --- | --- |
  | 接受 | `d8c1d11` | `.dev/tasks/2026-11-11-kindle-text-rtl.md` | 状态改 accepted |
  | 1、2、3 | `164d791` | `crates/kfx/src/write/mod.rs`、`layout.rs`、`tests.rs` | 按规则取方向；`single_image_page` 两处共用；`WRITER_VERSION` 26；三个测试 |
  | 3 | `f5f97d5` | `crates/library/tests/fp_golden.txt` | kindle 124 行 `kfx25` → `kfx26`，其它设备不变 |
  | 4 | `cd3c23b` | `docs/kfx.md`、`docs/decisions.md` | 写出器 26 和 2026-11-11 决定 |
  | 结果 | 本提交 | `.dev/tasks/2026-11-11-kindle-text-rtl.md` | 填结果、状态 done |

- 测试：

  | 命令 | 通过 / 失败 / 未跑 | 摘要 |
  | --- | --- | --- |
  | `cargo test -p kfx streaming_`（改代码前） | 失败（预期） | a `streaming_text_ignores_spine_rtl` 失败：`$560` 仍是 `$559`；b、d 通过——证明测到了 bug，且固定版式以外的纯图页本来就对 |
  | `cargo test -p kfx`（改完） | 通过 | 53 passed；a/b/d 和已有的固定版式 rtl 测试都过 |
  | `cargo test -p library --test fp_golden` | 通过 | 金样重写后复跑通过 |
  | `cargo test --workspace` | 通过 | 792 passed，0 failed；`bangumi_japanese_comic_builds_rtl_and_others_stay_as_is` 照过 |
  | `cargo clippy --workspace --all-targets -- -D warnings` | 通过 | 0 警告 |
  | `cargo fmt --all -- --check` | 通过 | 干净 |
  | `git diff --numstat crates/library/tests/fp_golden.txt` | — | `124 124`：只有 kindle 的 124 行 `kfx25` → `kfx26`，别的设备 0 变化 |

- 合成回归（`/tmp/opencode/kfx-rtl/`，不提交）：
  - 竖排文字书（`html{writing-mode:vertical-rl}` + `<spine page-progression-direction="rtl">` + `primary-writing-mode=vertical-rl`）：
    ```text
    epub-optimize --device=kindle vertical.epub vertical-k.epub
    epub-to-kfx vertical-k.epub vertical.kfx
    kfx-dump vertical.kfx
    → $538 … $192: '$376', $560: '$557'
    epub-to-kfx 打印：警告：spine 写了从右往左，KFX 只会横排，按从左往右翻
    ```
  - 同一本用 master `0c92e5c`（worktree）的写出器转：`$560: '$559'`；新旧 KFX 字节不同（符合预期）。
  - rtl 固定版式漫画（`fixed-layout` + `original-resolution 1264x1680` + spine rtl）：`$560: '$559'`、`$192: '$376'`，仍是 rtl。
  - 逐字节对比（同一份输入，master `0c92e5c` 和本分支各转一遍）：
    - spine 没写 rtl 的流式文字书 `cmp` 相同；
    - rtl 固定版式漫画 `cmp` 相同；
    - rtl 流式文字书不同（就是要改的那一种）。
  - `git diff --name-only 0c92e5c..HEAD` 里没有 `crates/bookconv/`，EPUB/KEPUB 产物没碰。

- 遇到的问题：
  - 没有。
- 偏离：按任务单允许的方式在 `crates/kfx/src/write/layout.rs` 抽出 `single_image_page` 给版面判定和方向判定共用（任务单说这时把 layout.rs 记进偏离）。
- 未完成：
  - 没跑真书回归、没上真机。真机（vinci，合并后）：sync 一本原书竖排的台版书，点右侧是下一页。

## 审查
<!-- 负责 bot 填写 -->
- 验收：通过（sheng-ren，2026-11-11，box 复跑 `78efe5a`）
  - `cargo fmt --all -- --check` 干净；`cargo clippy --workspace --all-targets -- -D warnings` 0 警告；`cargo test --workspace` 全过（kfx 53、library 537 等，0 failed）。
  - 金样：`diff <(git show 0c92e5c:crates/library/tests/fp_golden.txt | sed 's/kfx25/kfx26/g') crates/library/tests/fp_golden.txt` 无输出——除写出器版本号外 0 变化，别的设备没动。
  - 方向逻辑 `crates/kfx/src/write/mod.rs`：spine 不是 rtl → LTR；固定版式或全部文档都是单图页 → 保留 RTL；其余流式文字书 → LTR 并给警告。和任务单规则一致；`single_image_page`（`layout.rs`）与版面判定共用，没有两套判定。
  - 三个新测试覆盖 bug 本身（改前失败）、不写 rtl 的文字书、流式纯图页保留 rtl；已有的漫画 rtl 守护测试照过。
  - 留给二审的线索：流式漫画里只要夹一页文字（版权页、作者后记），整本就按 LTR 翻。Kindle 漫画走固定版式，影响应很小，请二审核实。
- 二审：回执 `.dev/tasks/2026-11-11-kindle-text-rtl.agy.md`；阻断 / 建议 / 线索，及采纳与否和理由
- 合并：PR 链接、合并提交号；状态改为 verified
