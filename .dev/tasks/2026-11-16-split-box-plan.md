# 拆 wash/box_fix.rs 里约 880 行的 `plan`：按现有段落拆成命名清楚的小函数

- Task ID: 2026-11-16-split-box-plan
- 负责 bot: sheng-ren
- 状态: pending
- 执行方: oc-exec
- 分支: task/2026-11-16-split-box-plan（从最新 master 切出）
- 更新时间: 2026-11-16T17:25:00+08:00
- 引用: `2026-11-16-split-box-fix` 审查里留下的「拆不动的大函数」；`2026-11-13-big-file-splits`

## 需求原文
> （vinci 2026-11-16，sheng-ren 聊天）2. 继续优化
> （负责 bot 转达）box_fix 里的 `plan`（约 880 行）再拆就得改代码结构，不只是挪代码。要求：拆成命名清楚的小函数或子模块，逻辑和顺序一律不变；不改公开签名，版本号和 `fp_golden` 都不动；产物逐字节不变；拆不干净就停下来回报，不要硬拆。

## 输入（master，负责 bot 读代码核实）
- `crates/bookconv/src/wash/box_fix.rs`（1119 行）：`fn plan(e, media, css, reader, g) -> Plan` 在 237–1118 行，私有，只被同文件 `apply`（86 行）调用；`box_fix/tests.rs` 不直接调 `plan`（17 个测试都走 `apply` / 公开入口）。
- `plan` 的结构（行号为 master）：
  - 开头 244–250：`out`、UTF-8、`Chapter::parse` 不成就返回空 `Plan`。
  - 251–341：一批**只读闭包**：`tag`、`comp`、`has_bg`、`to_em`、`pad_rem`、`visible`、`usable`、`has_content`、`ours`、`border_css`、`avail_px`；它们只捕获 `ch`、`g`（和彼此），不捕获任何可变状态。中间 271–291 算出 `content_before: Vec<bool>`（只读）。
  - 之后按注释分成的段（每段开头都有 `//` 说明）：343–396 定宽块折外边距（`width_margins`、`narrow`）；397–409 margin:auto；410–433 页首分页；434–450 float；451–466 负上外边距；467–576 左右有框的块包表格（`framed`）；577–650 水平内边距并外边距（`h_margin`）；651–705 单元格 / 行内 px 边距；706–745 列表项内边距；746–843 横线图（`drawn`）；844–871 单元格竖直居中；872–936 单元格宽度；937–1036 同一层相邻两块的竖直间距；1037–1099 行内外边距删掉、并进类规则；1100–1116 em 按根字号。
  - 段与段之间通过 `out: Plan` 和几张中间表（`width_margins`、`narrow`、`framed`、`h_margin`、`drawn` 等）传递。

## 方案（建议，执行方读代码后可微调，但规则不能破）
- 新建子模块 `crates/bookconv/src/wash/box_fix/plan.rs`（太长可再分 `plan/` 下几个文件），`plan` 整体挪过去，`box_fix.rs` 里 `mod plan;` + `use self::plan::plan;`（`plan` 为 `pub(super)`，同 `line`、`rewrite` 的做法）。
- 一个只读上下文 `struct Cx<'a> { ch: &'a Chapter…, reader: &'a ReaderModel, g: BoxGeometry, content_before: Vec<bool> }`；原来的只读闭包改成 `impl Cx` 上的同名方法，**函数体逐字照搬**，只把对别的闭包的调用改成 `self.xxx(…)`、对捕获变量的引用改成 `self.ch` / `self.g`。
- 每个注释段抽成一个函数（名字按段的意思起，如 `width_to_margins`、`page_top_break`、`float_off`、`frame_side_borders`、`merge_h_padding`、`cell_px_margins`、`list_item_padding`、`rule_images`、`cell_middle`、`cell_widths`、`vertical_gaps`、`strip_inline`、`em_root_headings`），参数是 `&Cx`、`&mut Plan` 和它读写的那几张中间表；**段内语句、表达式、浮点运算顺序、迭代顺序逐字不变**，段前的注释原样跟着走。
- `plan` 自己只剩：开头取 `ch`、建 `Cx`、建中间表、按原顺序依次调用各段函数、返回 `out`。

## 规则（要实现的行为）
1. 逻辑和顺序一律不变：每段的语句照搬，段的调用顺序和原来一样；不合并、不重排、不“顺手简化”任何表达式（包括 `max`/`round`/除法顺序、`HashMap` / `BTreeMap` 的选择和遍历方式）。
2. 不改公开签名：`apply`、`BoxGeometry`、`page_top_rule_px` 等对 `wash` 以外可见的东西一个不动；新加的项一律私有，最多 `pub(super)`（只为让 `box_fix.rs` 调到 `plan`）；**不许出现 `pub(crate)` / `pub`**，不放宽任何已有项的可见性。
3. 不改版本号（`docs/development.md#版本号`），`fp_golden.txt` 一行不动，不改 `testdata/`。
4. 测试名和数量不变（bookconv lib 553、workspace 864），不改测试。
5. **拆不干净就停**：如果某段离开闭包 / 原作用域就必须改语义（比如借用冲突只能靠 clone、改遍历顺序、改类型才能过），这一段留在 `plan` 里不拆，在「结果 → 遇到的问题」写明原因；如果整体做不到，状态改 blocked 并写明，不要硬拆。

## 分步任务
1. [ ] 读 `plan`，列出每个闭包捕获了什么、每段读写哪些中间表，写进结果。
2. [ ] 改前 `cargo test -p bookconv --lib -- --list` 存清单。
3. [ ] 按「方案」建 `Cx` 和各段函数，挪到 `box_fix/plan.rs`；可以分两三个提交（先挪文件、再拆段），每个提交都能编译、测试全过。
4. [ ] 改后列测试，完整路径和改前一致。
5. [ ] `cargo test --workspace` 全过；`cargo clippy --workspace --all-targets -- -D warnings` 0 警告（`too_many_arguments` 等如要 `#[allow]`，写进结果说明；优先用小结构体打包中间表，不改逻辑）。
6. [ ] 结果里给出拆后各函数的行数表，`plan` 本身的行数。

## 验收标准
- [ ] `plan` 拆成若干命名清楚的函数，单个函数不超过约 150 行（个别段本来就长的照实说明）；各段语句与 master 对应段逐字相同（除闭包调用改成方法调用、缩进）。
- [ ] 可见性没有放宽；没有 `pub(crate)` / `pub` 新项。
- [ ] 测试 864 全过、名字一致；clippy 0 警告；版本号、`fp_golden.txt` 没动。
- [ ] 产物逐字节不变（负责 bot 用 master 与分支二进制对 kindle、ireader、kobo、hanvon、bigme 五种模式、文字书和漫画对照）。
- [ ] 结果和提交说明里写明**没跑真书回归、没上真机**，并建议 vinci 按 `docs/development.md` 跑真书回归（这里是正文清洗路径）。

## 不做什么
- 不改任何清洗规则、阈值、注释里的事实；不新增 / 删除测试；不动 `line.rs`、`rewrite.rs` 的逻辑。
- 不对整个仓库或这些文件跑 `cargo fmt` / rustfmt（新文件可以手动按原风格排版）。

## 约束
- 只用 Rust；clean-room；仓库不出现上游项目名及缩写；不对照任何外部实现。
- 不碰 `~/.local/share/booklib`、真机；实验用 `target/` 下临时 `--library`。
- `install.sh` 不用 `--force`。
- 只在任务分支上工作，不合并；改「涉及文件」以外的写进「结果 → 偏离」。
- 提交信息中文 + 前缀 + 正文写原因，正文写明没跑真书回归、没上真机；不加 Co-Authored-By。

## 涉及文件
- `crates/bookconv/src/wash/box_fix.rs`
- `crates/bookconv/src/wash/box_fix/plan.rs`（新，或 `box_fix/plan/` 下几个新文件）

## 交付物
代码提交到任务分支并推送；「结果」填好；状态改为 done 或 blocked。任务单状态改动推 master，带 `[skip-notify]`。不要合并，不要跑二审。

## 给执行方的指令
```
执行任务单 .dev/tasks/2026-11-16-split-box-plan.md：先把状态改为 accepted 并提交，按「分步任务」做完，填好「结果」，状态改为 done（做不下去改为 blocked 并写明原因），提交并推送分支。任务单 done 推 master 带 [skip-notify]。不要合并，不要跑二审。
```

## 结果
<!-- 执行方填写 -->

## 审查
<!-- 负责 bot 填写 -->
- 验收：
- 二审：
- 合并：
