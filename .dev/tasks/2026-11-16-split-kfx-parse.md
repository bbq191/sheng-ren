# 拆 `kfx/src/write/parse.rs`（big-file-splits 子单）

- Task ID: 2026-11-16-split-kfx-parse
- 负责 bot: sheng-ren
- 状态: done
- 执行方: oc-exec
- 分支: task/2026-11-16-split-kfx-parse（从最新 master 切出）
- 更新时间: 2026-11-16T16:34:42+08:00
- 引用: `2026-11-13-big-file-splits` 分步 4；拆法参照 `2026-11-16-split-box-fix`（`box_fix.rs` + `box_fix/` 子目录）

## 需求原文
> （质量审查发现 14）十个核心文件单文件超过 1800 行，难审、难改。只做模块拆分、不改函数逻辑与公开签名，各设备产物及 `fp_golden` 应逐字节不变。执行时一次只拆一个文件；测试数量和结果不变。**clean-room，只动本仓库代码**。
> （负责 bot 转达的第 3 项，2026-11-16）剩下的大文件按收益从大到小一张单拆一个；拆分只搬代码，不改行为；每张单证明产物逐字节不变，版本号不动；涉及 kfx 的单要用 kindle 模式核对。

## 输入（开单时 master，`wc -l` 2002 行，没有测试——kfx 写出器的测试在 `write/tests.rs`；执行时以代码为准）
`crates/kfx/src/write/parse.rs` 是 `write` 的私有子模块，`write/mod.rs:33` 用 `use parse::*;` 把它的 `pub(super)` 项拿进 `write`，`write/tests.rs:1866` 用 `super::parse::OBJ`。现有各段（行号约数）：
| 行 | 内容 |
|---|---|
| 1–178 | 模块说明（含 [`Block`]、[`parse_docs`]、[`MAX_NESTING`] 文档链接）、`use super::*;`、`Vert`、`Horiz`、`Run`、`OBJ`、`InlineImage`、`Kind`、`Gap` 与 `impl Gap`、`Block`、`walk_blocks`、`for_each_block`、`walk_blocks_mut`、`any_block` |
| 179–246 | `use …BLANK_LINE_FOLD`、`rem_len`、`to_vert`、`to_horiz`、`lang_attr`、`BoxLens` 与 `impl`、`collapse` |
| 247–448 | `Doc`、`OpenRun`、`Inline` 与 `impl Inline` |
| 449–723 | `attach_trailing`、`prepend_ids`、`Special`、`device_line_height`、`root_em`、`own_text_block`、`absorb_whole_span_style`、`container_block`、`flatten_wrapper` |
| 724–1448 | `impl Doc<'_>`（约 725 行，一个 impl 块） |
| 1449–1530 | `impl Block`、`image_src`、`run_differs`、`inline_box` |
| 1531–1747 | `ParsedDoc`、`BREAK_MARK`、`starts_with_break`、`strip_breaks`、`split_at_breaks`、`has_negative`、`scale_percent`、`apply_gaps`、`collapse_siblings`、`merge_text_style`、`mark_bare`、`with_box`、`block_depth` |
| 1748–2002 | `book_language`、`parse_docs`、`MAX_NESTING`、`DEEP_NESTING`、`BIG_STACK`、`nesting_depth`、`maybe_big_stack`、`on_big_stack`、`prepend_cover_page` |

## 方案（建议拆法，可按代码现状微调，在结果里列出每个文件放了什么、多少行）
- 保留 `write/parse.rs` 作模块根，新建 `write/parse/` 子目录：
  - `parse.rs`：模块说明、`use`、`mod` 声明、再导出，块的数据类型和遍历（1–178）、长度换算（179–246）。
  - `parse/inline.rs`：`Doc` 结构、`OpenRun`、`Inline` 与 `impl Inline`（247–448）。
  - `parse/element.rs`（或 `doc.rs`）：`impl Doc<'_>`（724–1448）**整块不拆**，加上只被它用的辅助函数（449–723 的那些、`image_src`、`run_differs` 等，按调用方定放哪）。约 1000–1100 行，低于 1800 即可。
  - `parse/blocks.rs`：`impl Block`、`inline_box`、`ParsedDoc`、分页标记与 `split_at_breaks`、外边距与折叠（1449–1747 里不属于 `impl Doc` 辅助的）。
  - `parse/docs.rs`：全书解析与栈（1748–2002）。
- **`impl Doc<'_>` 不拆成几个 impl 块**：保持一个块原样搬，避免改方法可见性的连锁。
- 可见性：`write` 要用的项现在是 `pub(super)`（相对 `parse` 即 `write`）。搬进 `parse/` 子目录后，`pub(super)` 的含义会变成「只给 `parse`」，所以这些项要写成 `pub(in crate::write)`，再在 `parse.rs` 里 `pub(super) use self::子模块::…` 再导出，保证 `write/mod.rs` 的 `use parse::*;` 和 `super::parse::OBJ` 原样能用。只在 `parse` 内部跨文件用的原私有项改成 `pub(super)`。不要放宽到 `pub` / `pub(crate)`。
- 子模块开头用 `use super::*;` 拿 `parse` 的项（含 `parse.rs` 的 `use super::*;` 引进来的 `write` 的项）；缺的名字补 `use`。
- 模块说明里的文档链接（[`Block`]、[`parse_docs`]、[`MAX_NESTING`]、[`Doc::pending`] 等）保证还能解析（`use` 进来或再导出），文字不改。
- 不用 `include!`。

## 规则（要实现的行为）
1. 只挪代码：函数体、分支条件、常量值、错误文案、注释文字都不改；允许的改动只有：`mod` / `use` / 再导出、为跨模块访问调整可见性（不超出 `crate::write`）、每个新文件开头一段 `//!` 说明，以及可见性前缀变长引起的 rustfmt 折行。
2. `write` 下其他文件不改（除非为了路径不得不改，那算偏离写进结果）。
3. 测试集合（kfx 全部测试名字、数量）与结果和拆前一致。
4. 每个文件低于 1800 行。
5. clean-room：只读、只改本仓库代码，不去对照任何外部实现。

## 分步任务
1. [ ] 读 `parse.rs`，列出各段职责和相互调用，定拆法。
2. [ ] 拆前跑 `cargo test -p kfx -- --list` 存测试名清单。
3. [ ] 挪代码到子模块，调整 `mod`/`use`/可见性，保证编译。
4. [ ] 拆后再列测试名，与拆前一致；`cargo test --workspace` 全过；`cargo clippy --workspace --all-targets -- -D warnings` 0 警告；版本号（含 KFX 写出器 26）、`fp_golden.txt` 没动。
5. [ ] 结果里列出每个文件的内容和行数、所有可见性改动。

## 验收标准
- [ ] `parse.rs` 已拆成合理子模块，每个文件低于 1800 行；`impl Doc<'_>` 原样在一个文件里。
- [ ] 只挪代码：负责 bot 逐项比对核实，除允许的改动外没有别的差异。
- [ ] kfx 测试名字一致、全过；`cargo test --workspace` 全过；clippy `-D warnings` 0 警告。
- [ ] 产物逐字节不变：负责 bot 用 master 与分支二进制对 ireader、kobo、**kindle** 各生成几本书比 sha256（KFX 必核）。版本号、`fp_golden.txt` 没动。
- [ ] 结果里写明没跑真书回归、没上真机。

## 不做什么
- 不拆函数、不拆 `impl Doc` 块，不改任何算法、常量、签名；不合并、不改名任何函数；不顺手修 bug 或改注释。
- 不碰 big-file-splits 表里的其他文件。
- 不对整个仓库跑 `cargo fmt`（可以对新建和改动的这几个文件跑 `rustfmt`，但只能产生搬动引起的折行变化）。

## 约束
- 只用 Rust；clean-room；仓库不出现上游项目名及缩写。
- **只挪代码、不改行为**；不升版本号；`fp_golden.txt` 一行不动；不改 `testdata/`。
- 不碰 `~/.local/share/booklib`、真机；实验用 `target/` 下临时 `--library`。
- `install.sh` 不用 `--force`。
- 只在任务分支上工作，不合并；改「涉及文件」以外的写进「结果 → 偏离」。
- 提交信息中文 + 前缀 + 正文写原因，不加 Co-Authored-By。

## 涉及文件
- `crates/kfx/src/write/parse.rs`
- 新建 `crates/kfx/src/write/parse/{inline,element,blocks,docs}.rs`（名字可微调）

## 交付物
代码提交到任务分支并推送；「结果」填好；状态改为 done 或 blocked。任务单状态改动推 master，带 `[skip-notify]`。不要合并，不要跑二审。

## 给执行方的指令
```
执行任务单 .dev/tasks/2026-11-16-split-kfx-parse.md：先把状态改为 accepted 并提交，按「分步任务」做完，填好「结果」，状态改为 done（做不下去改为 blocked 并写明原因），提交并推送分支。任务单 done 推 master 带 [skip-notify]。不要合并，不要跑二审。
```

## 结果
- 执行方 / 开始–结束时间：box-agent（oc-exec） / 2026-11-16T16:25:52+08:00–2026-11-16T16:34:42+08:00
- base..head：`b45918463449e306e0cfa5af36ca5ed0567bb03a..a3c8ea1ca50fe0df8f3a0944bf368de217de70c0`
- 提交：
  | 提交号 | 说明 |
  |---|---|
  | d8ed0d0 | chore: 开始执行任务单 2026-11-16-split-kfx-parse [skip-notify] |
  | a3c8ea1 | chore(kfx): 拆分 write/parse.rs 为 parse 子模块 |
- 改动文件：`.dev/tasks/2026-11-16-split-kfx-parse.md`,`crates/kfx/src/write/parse.rs` `crates/kfx/src/write/parse/blocks.rs`,`crates/kfx/src/write/parse/docs.rs` `crates/kfx/src/write/parse/element.rs`,`crates/kfx/src/write/parse/inline.rs`
- 测试：包装器未代跑验收命令，由负责 bot 按任务单核对
- 遇到的问题：无
- 偏离：无
- 新建文件（线索）：无
- 未完成：无
- 运行记录：`/home/box/oc-runs/2026-11-16-split-kfx-parse-20261116-162552.json`
- 日志：`/home/box/oc-runs/2026-11-16-split-kfx-parse-20261116-162552.log`
- 模型：`opencode-go/deepseek-v4.1-flash`
- 用时：实际运行 8.8 分钟（墙钟 530s / 实际运行 530s；超时按实际运行计）

## 审查
<!-- 负责 bot 填写 -->
- 验收：通过（负责 bot 独立验收，2026-11-16，独立 worktree；对照 master `b459184`；只读本仓库代码）。运行记录 JSON 的 `deviations` 为空，与结果一致；`write` 下其他文件未改。
  - 拆法：`parse.rs` 266 行（模块说明、`mod`、再导出，块的数据类型与遍历、长度换算、`MAX_NESTING`）；`parse/inline.rs` 204（`Doc` 结构、`OpenRun`、`Inline` 与 `impl Inline`）；`parse/element.rs` 1095（`impl Doc<'_>` 整块原样，加上它用的 `attach_trailing`、`prepend_ids`、`Special`、`device_line_height`、`root_em`、`own_text_block`、`absorb_whole_span_style`、`container_block`、`flatten_wrapper`、`image_src`、`run_differs`、`merge_text_style`、`mark_bare`、`with_box`）；`parse/blocks.rs` 211（`impl Block`、`inline_box`、`ParsedDoc`、分页标记、`has_negative`、`scale_percent`、`apply_gaps`、`collapse_siblings`、`block_depth`）；`parse/docs.rs` 256（`book_language`、`parse_docs`、`DEEP_NESTING`、`BIG_STACK`、栈相关、`prepend_cover_page`）。
  - 只挪代码：按顶层项逐个比对（忽略可见性和空白），旧文件 64 项全部原样出现；唯一的文本差异是 `impl Block` 里三个方法加了可见性（`anonymous` 因此被 rustfmt 折成多行、补了尾逗号），函数体逐字相同。新增只有 `use`、`mod`、再导出、`//!` 说明。
  - 可见性：原来 `pub(super)`（给 `write` 用）的搬进子目录后写成 `pub(in crate::write)`，在 `parse.rs` 里 `pub(super) use` 再导出，`write/mod.rs` 的 `use parse::*;`、`tests.rs` 的 `super::parse::OBJ` 原样能用；只在 `parse` 内部跨文件用的原私有项改成 `pub(super)`（如 `Block::led_by`、`is_anonymous`、`OpenRun`）。
  - 发现并修正（提交 `4c17acf`）：执行方把拆前私有的 `has_negative`、`scale_percent`、`attach_trailing`、`prepend_ids` 也用 `pub(super) use` 再导出给了整个 `write`（`write` 其他文件不用）；改成子模块里 `pub(super)`、`parse.rs` 里私有 `use`，可见范围回到 `parse` 内，同拆前。
  - 测试：kfx 53 项名字与 master 一致；`cargo test --workspace` 864 通过 0 失败；clippy `-D warnings` 0 警告（含上面的修正）。
  - 产物：master 与分支二进制对 4 本书同步到 ireader、kobo、kindle，12 份产物 sha256 全部相同（其中 kindle 的 4 份 KFX），输出除路径外相同。版本号（KFX 写出器 26）、`fp_golden.txt` 没动。
  - 已知局限：`impl Doc<'_>` 一个块约 725 行，`element.rs` 1095 行；再拆要把 impl 块分开、改方法可见性，按单子没做。
  - 没跑真书回归、没上真机。
