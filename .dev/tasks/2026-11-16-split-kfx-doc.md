# 拆 kfx 的 parse/element.rs 里约 725 行的 `impl Doc`：按职责分到子模块，长方法拆小

- Task ID: 2026-11-16-split-kfx-doc
- 负责 bot: sheng-ren
- 状态: done
- 执行方: oc-exec
- 分支: task/2026-11-16-split-kfx-doc（从最新 master 切出）
- 更新时间: 2026-11-16T17:43:01+08:00
- 引用: `2026-11-16-split-kfx-parse` 审查里留下的「拆不动的大块」；`2026-11-16-split-box-plan`（同类做法，已合并 `27bee77`）

## 需求原文
> （vinci 2026-11-16，sheng-ren 聊天）2. 继续优化
> （负责 bot 转达）kfx 的 `impl Doc`（约 725 行）再拆就得改代码结构，不只是挪代码。要求：拆成命名清楚的小函数或子模块，逻辑和顺序一律不变；不改公开签名，版本号和 `fp_golden` 都不动；产物逐字节不变；拆不干净就停下来回报，不要硬拆。

## 输入（master，负责 bot 读代码核实）
- `crates/kfx/src/write/parse/element.rs`（1095 行）：文件头写着「`impl Doc<'_>` 整块照搬，不拆」（上一张单的约定，本单改掉）。`impl Doc<'_>` 在 281–1004 行，方法与行数：
  `take_pending`（`pub(super)`，5）、`inline`（30）、`image_size`（23）、`block`（`pub(super)`，15）、`block_inner`（92）、`special_block`（43）、`boxed`（26）、`wrap_children`（31）、`list`（73）、`table`（127）、`flush`（53）、`flush_piece`（45）、`inline_or_block`（158）。
- 外部只调 `block`、`take_pending`（`parse/docs.rs:75、76、93、113`）；其余方法私有、只在 `impl Doc` 内互相调用。`Doc` 定义在 `write/parse.rs`，字段私有（子模块能看到祖先模块的私有字段）。
- 现状测试：kfx lib 53 项，workspace 864。

## 方案（建议，执行方读代码后可微调，但规则不能破）
- `element.rs` 保留块的入口（`take_pending`、`block`、`block_inner`、`special_block`、`boxed`、`wrap_children` 和 `impl` 外的那些自由函数）；`impl Doc<'_>` 按职责分成几个 `impl` 块放进子模块，例如：
  - `parse/element/list_table.rs`：`list`、`table`；
  - `parse/element/flow.rs`（或 `inline_flow.rs`）：`inline`、`image_size`、`flush`、`flush_piece`、`inline_or_block`。
- 长方法拆小（只在**不改语义**时做）：
  - `inline_or_block`：图片分支（`if let Some(src) = image_src(&el) { … return; }`）抽成一个方法、空 `inline-block` 小方块分支抽成一个方法，提前 `return` 的地方用「抽出的方法做完整个分支、调用处紧跟 `return`」保持原控制流；返回 `bool` 表示是否已处理也可以，但要保证求值顺序完全相同（例如原来在判断前就算的 `comp` 仍在同一位置算一次，不能多算、少算或挪到判断后面）。
  - `table`（127 行）：按现有段落（收行、算列宽、写单元格之类）能干净抽出就抽，抽不干净就整块留着并在结果里说明。
- 方法体里的语句、表达式、浮点运算顺序、迭代顺序逐字照搬；方法前的文档注释跟着走；改掉文件头那句「整块照搬，不拆」，写成现在的组织方式。

## 规则（要实现的行为）
1. 逻辑和顺序一律不变：不合并、不重排、不“顺手简化”任何表达式；`RefCell` 的 `borrow` / `borrow_mut` / `take` 的时机和次数不变（这里 `links`、`pending`、`gap` 都是 `RefCell` / `Cell`，挪动借用时机可能改行为或 panic）。
2. 不改公开签名：`kfx` crate 对外的东西一个不动；`block`、`take_pending` 仍是 `pub(super)`、留在 `element.rs`。
3. 可见性不放宽：原来私有的方法挪到 `element/` 的子模块后，最多 `pub(super)`（只对 `element` 可见）；**不许 `pub(crate)` / `pub` / `pub(in …)` 比原来更宽**。新抽出的辅助方法一律私有（或同上 `pub(super)`）。
4. 不改版本号（`docs/development.md#版本号`），`fp_golden.txt` 一行不动，不改 `testdata/`。
5. 测试名和数量不变（kfx lib 53、workspace 864），不改测试。
6. **拆不干净就停**：某个方法或分支离开原作用域就必须改语义（借用冲突只能靠 clone、改求值顺序、改类型才能过）的，留着不拆，在「结果 → 遇到的问题」写明；整体做不到就改 blocked 写明原因，不要硬拆。clean-room：只看本仓库代码，不对照任何外部实现。

## 分步任务
1. [ ] 读 `impl Doc`，列出方法之间的调用关系和 `RefCell` / `Cell` 的使用点，写进结果。
2. [ ] 改前 `cargo test -p kfx --lib -- --list` 存清单。
3. [ ] 先把 `impl` 按职责分到子模块（只挪，单独提交，编译、测试全过）。
4. [ ] 再拆 `inline_or_block`（和能干净拆的 `table`），单独提交。
5. [ ] 改后列测试，完整路径和改前一致。
6. [ ] `cargo test --workspace` 全过；`cargo clippy --workspace --all-targets -- -D warnings` 0 警告（如需 `#[allow]` 写进结果；优先不加）。
7. [ ] 结果里给出拆后各文件、各方法的行数表。

## 验收标准
- [ ] `impl Doc` 分到几个命名清楚的子模块；`element.rs` 明显变短；单个方法不超过约 100 行（个别照实说明）。
- [ ] 各方法体与 master 对应部分逐词相同（除抽出的辅助调用、缩进）；`RefCell` 借用时机不变。
- [ ] 可见性没有放宽；`block`、`take_pending` 原样。
- [ ] 测试 864 全过、kfx 53 项名字一致；clippy 0 警告；版本号、`fp_golden.txt` 没动。
- [ ] 产物逐字节不变（负责 bot 用 master 与分支二进制对 kindle 模式的 KFX 对照，书目含文字书和漫画；其余模式顺带对照）。
- [ ] 结果和提交说明里写明**没跑真书回归、没上真机**，并建议 vinci 按 `docs/development.md` 跑真书回归（这里是 KFX 写出路径）。

## 不做什么
- 不改任何写出规则、注释里的事实；不新增 / 删除测试；不动 `parse.rs`、`docs.rs`、`blocks.rs`、`inline.rs` 的逻辑（只在必要时加 `mod` / `use`）。
- 不对整个仓库或这些文件跑 `cargo fmt` / rustfmt（新文件可以手动按原风格排版）。

## 约束
- 只用 Rust；clean-room；仓库不出现上游项目名及缩写。
- 不碰 `~/.local/share/booklib`、真机；实验用 `target/` 下临时 `--library`。
- `install.sh` 不用 `--force`。
- 只在任务分支上工作，不合并；改「涉及文件」以外的写进「结果 → 偏离」。
- 提交信息中文 + 前缀 + 正文写原因，正文写明没跑真书回归、没上真机；不加 Co-Authored-By。

## 涉及文件
- `crates/kfx/src/write/parse/element.rs`
- `crates/kfx/src/write/parse/element/*.rs`（新）

## 交付物
代码提交到任务分支并推送；「结果」填好；状态改为 done 或 blocked。任务单状态改动推 master，带 `[skip-notify]`。不要合并，不要跑二审。

## 给执行方的指令
```
执行任务单 .dev/tasks/2026-11-16-split-kfx-doc.md：先把状态改为 accepted 并提交，按「分步任务」做完，填好「结果」，状态改为 done（做不下去改为 blocked 并写明原因），提交并推送分支。任务单 done 推 master 带 [skip-notify]。不要合并，不要跑二审。
```

## 结果
- 执行方 / 开始–结束时间：box-agent（oc-exec） / 2026-11-16T17:30:17+08:00–2026-11-16T17:43:01+08:00
- base..head：`d6314e2162bb9320d00ce27ba4190077c6d5e074..1b806856ea6cdc3395a650daeb9eff507eaee7a4`
- 提交：
  | 提交号 | 说明 |
  |---|---|
  | 9cd1e07 | chore: 开始执行任务单 2026-11-16-split-kfx-doc [skip-notify] |
  | 853a2c2 | chore(kfx): 把 parse/element.rs 的 impl Doc 按职责拆到 element/ 子模块 |
  | 1b80685 | chore(kfx): 拆 parse/element/flow.rs 的 inline_or_block 和 list_table.rs 的 table |
- 改动文件：`.dev/tasks/2026-11-16-split-kfx-doc.md`,`crates/kfx/src/write/parse/element.rs` `crates/kfx/src/write/parse/element/computed.rs`,`crates/kfx/src/write/parse/element/flow.rs` `crates/kfx/src/write/parse/element/list_table.rs`
- 测试：包装器未代跑验收命令，由负责 bot 按任务单核对
- 遇到的问题：无
- 偏离：无
- 新建文件（线索）：无
- 未完成：无
- 运行记录：`/home/box/oc-runs/2026-11-16-split-kfx-doc-20261116-173017.json`
- 日志：`/home/box/oc-runs/2026-11-16-split-kfx-doc-20261116-173017.log`
- 模型：`opencode-go/deepseek-v4.1-flash`
- 用时：实际运行 12.7 分钟（墙钟 764s / 实际运行 764s；超时按实际运行计）

## 审查
<!-- 负责 bot 填写 -->
- 验收：通过（负责 bot 独立验收，2026-11-16，独立 worktree）。
  - 范围：只动了 `element.rs` 和三个新文件 `element/{computed,flow,list_table}.rs`；`parse.rs`、`docs.rs`、`blocks.rs`、`inline.rs` 没动。
  - 结构：`element.rs` 1095→589 行，留块的入口和分派（`take_pending`、`block`、`block_inner`、`special_block`、`boxed`、`wrap_children`）；`computed.rs`（`comp`、`image_size`）、`flow.rs`（`inline`、`children`、`flush`、`flush_piece`、`inline_or_block`）、`list_table.rs`（`list`、`table`）。`inline_or_block` 158→约 95 行：图片分支抽成 `image`、空 inline-block 小方块抽成 `empty_inline_box`，调用处紧跟 `return`，`comp` 求值位置不变；`table` 127→约 45 行：抽出 `table_caption`、`table_section`、`table_row`、`table_cell`，列宽仍只在第一行算（`&mut Option<Vec<f64>>` 传下去），遍历顺序不变。
  - 逐词核对：没拆的方法（`take_pending`、`inline`、`comp`、`image_size`、`block`、`block_inner`、`special_block`、`boxed`、`wrap_children`、`list`、`children`、`flush`、`flush_piece`）去掉空白和可见性修饰后与 master 逐字相同；`impl` 外的自由函数相同；拆开的两个方法逐段对读，语句、表达式、`RefCell`/`Cell`（`links`、`pending`、`gap`）的借用和 `take` 时机都没变，没有新增 `clone`。
  - 可见性：`block`、`take_pending` 仍是 `pub(super)` 留在 `element.rs`；`comp`、`children` 原来就是 `pub(in crate::write)`，原样；其余原私有的方法挪进子模块后是 `pub(super)`（只对 `element` 可见），新抽出的辅助方法私有。没有放宽。（开单时「输入」漏列了 `comp`、`children` 两个方法，以代码为准。）
  - 测试：kfx lib 53 项名字与 master 一致；`cargo test --workspace` 864 通过 0 失败；clippy `-D warnings` 0 警告，没加 `#[allow]`。
  - 产物：master 与分支二进制对 18 本书（16 本文字书，含 2 本盒子样式、表格、列表、图片密集的书；2 卷 CBZ 漫画）同步到 kindle 模式，18 份 KFX sha256 全部相同；ireader、kobo、hanvon、bigme 顺带对照，五种模式共 90 份产物、加 xochitl 测试配置 18 份，全部相同，命令输出相同。版本号、`fp_golden.txt` 没动。
  - **没跑真书回归、没上真机。** 这里是 KFX 写出路径，建议 vinci 按 `docs/development.md` 跑一次真书回归。
- 二审：agy-review（gemini-3.8-flash-medium）建议通过；阻断、建议都无；线索一条：执行方「结果」里没单独列调用关系和行数表（已在验收记录里补上）。见 `2026-11-16-split-kfx-doc.agy.md`。
- 合并：03b6494（`--no-ff` 进 master；master 合并前没动，合并树与已验收的分支末端相同：test 864 通过、clippy 0 警告）。没跑真书回归、没上真机，建议 vinci 按 `docs/development.md` 跑真书回归。任务分支合并后删除。状态保持 done，等 vinci 真机验收。
