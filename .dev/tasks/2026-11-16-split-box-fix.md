# 拆 `bookconv/src/wash/box_fix.rs`（big-file-splits 子单）

- Task ID: 2026-11-16-split-box-fix
- 负责 bot: sheng-ren
- 状态: done
- 执行方: oc-exec
- 分支: task/2026-11-16-split-box-fix（从最新 master 切出）
- 更新时间: 2026-11-16T16:17:33+08:00
- 引用: `2026-11-13-big-file-splits` 分步 3；拆法参照 `2026-11-13-split-imgopt`（`imgopt.rs` + `imgopt/` 子目录）

## 需求原文
> （质量审查发现 14）十个核心文件单文件超过 1800 行，难审、难改。只做模块拆分、不改函数逻辑与公开签名，各设备产物及 `fp_golden` 应逐字节不变。执行时一次只拆一个文件；测试数量和结果不变。
> （负责 bot 转达的第 3 项，2026-11-16）剩下的大文件按收益从大到小一张单拆一个；拆分只搬代码，不改行为；每张单证明产物逐字节不变，版本号不动。

## 输入（开单时 master，`wc -l` 2009 行，执行时以代码为准）
`crates/bookconv/src/wash/box_fix.rs` 现有各段（行号约数）：
| 行 | 内容 |
|---|---|
| 1–30 | 模块说明（含指向 [`frame_open`] 的文档链接）、`use` |
| 31–158 | `CSS_NAME`、`RULE_W`、`pub struct BoxGeometry`、`type Frame`、`LineStyle`、`Line` 与 `impl Line`、`LineKey`、`Plan`、`frame_open`、`FRAME_CLOSE` |
| 159–279 | `pub(super) fn apply`（入口，`wash/mod.rs:899` 调用） |
| 280–528 | `Out`、`Job`、`Resolved`、`resolve`、`rewrite`、`list_indent_extra_px`、`MAX_TOP_PAD_REM`、`NARROW_SHARE`、`big_top_pad`、`pub(super) fn page_top_rule_px`（`wash/top_spacer.rs:429` 调用） |
| 529–566 | `skipped`、`heading`、`fmt` |
| 567–1449 | `fn plan`（**一个约 880 行的函数**） |
| 1450–2009 | `#[cfg(test)] mod tests`（17 个测试） |

- 外部用法：`wash/mod.rs` 的 `pub use self::box_fix::BoxGeometry;`、`box_fix::apply(…)`；`wash/top_spacer.rs` 的 `super::box_fix::page_top_rule_px`；`wash/mod.rs` 文档注释里提到 `box_fix::page_top_rule_px`、`box_fix::list_indent_extra_px`。这些路径拆完后都要照旧能用（在 `box_fix.rs` 里再导出）。

## 方案（建议拆法，可按代码现状微调，在结果里列出每个文件放了什么、多少行）
- 保留 `wash/box_fix.rs` 作模块根，新建 `wash/box_fix/` 子目录（同 `imgopt.rs` + `imgopt/` 的做法）：
  - `box_fix.rs`：模块说明、`use`、`mod` 声明、再导出、`CSS_NAME`、`BoxGeometry`、`Plan`、`apply`、`plan`，以及只被 `plan` 用的小函数（`skipped`、`heading`、`fmt`，也可放进别的子模块，按调用方定）。
  - `box_fix/line.rs`：横线图与包框：`RULE_W`、`Frame`、`LineStyle`、`Line` 与 `impl Line`、`LineKey`、`frame_open`、`FRAME_CLOSE`。
  - `box_fix/rewrite.rs`：`Out`、`Job`、`Resolved`、`resolve`、`rewrite`、`list_indent_extra_px`、`MAX_TOP_PAD_REM`、`NARROW_SHARE`、`big_top_pad`、`page_top_rule_px`。
  - `box_fix/tests.rs`：原 `mod tests` 整体搬出，`box_fix.rs` 里 `#[cfg(test)] mod tests;`，测试名和路径（`wash::box_fix::tests::…`）不变。
- **`plan` 整体留在 `box_fix.rs`，不拆函数**：拆一个函数要改控制流和参数传递，不是「只搬代码」，不在本单范围。所以 `box_fix.rs` 拆完约 1100 行，低于 1800 即可，不要求 ≤ 800。
- 子模块之间、以及 `wash` 其他文件用到的项：原先私有的改成 `pub(super)`（只给 `box_fix` 用）或 `pub(in crate::wash)`（`wash` 其他文件要用的，如 `page_top_rule_px`），在 `box_fix.rs` 里 `use` / 再导出，保证 `super::box_fix::page_top_rule_px`、`box_fix::apply`、`box_fix::BoxGeometry` 这些原路径不变。不要放宽成 `pub`；`BoxGeometry` 原来就是 `pub`，保持。
- 模块说明里的 [`frame_open`] 链接：保证 `box_fix.rs` 里能解析（`use` 进来即可），文字不改。
- 不用 `include!`。

## 规则（要实现的行为）
1. 只挪代码：函数体、分支条件、常量值、注释文字都不改；允许的改动只有：`mod` / `use` / 再导出、为跨模块访问放宽可见性（不到 `pub`）、每个新文件开头一段 `//!` 说明、搬出测试后的缩进变化（以及因此产生的 rustfmt 折行）。
2. `wash` 其他文件不改（除非为了路径不得不改，那算偏离写进结果）。
3. 测试集合（名字、数量 17）与结果和拆前一致。
4. 每个文件低于 1800 行。

## 分步任务
1. [ ] 读 `box_fix.rs`，列出各段职责和相互调用，定拆法。
2. [ ] 拆前跑 `cargo test -p bookconv --lib wash::box_fix -- --list` 存测试名清单。
3. [ ] 挪代码到子模块，调整 `mod`/`use`/可见性，保证编译。
4. [ ] 拆后再列测试名，与拆前一致；`cargo test --workspace` 全过；`cargo clippy --workspace --all-targets -- -D warnings` 0 警告；版本号、`fp_golden.txt` 没动。
5. [ ] 结果里列出每个文件的内容和行数、所有可见性改动。

## 验收标准
- [ ] `box_fix.rs` 已拆成合理子模块，每个文件低于 1800 行；`plan` 原样留在一个文件里。
- [ ] 只挪代码：负责 bot 逐项比对核实，除允许的改动外没有别的差异。
- [ ] 测试 17 项名字一致、全过；`cargo test --workspace` 全过；clippy `-D warnings` 0 警告。
- [ ] 产物逐字节不变（负责 bot 用 master 与分支二进制对 ireader、kobo、kindle 各生成几本书比 sha256；本文件只在 Move 这类不认内边距/边框的模式和统一文字书路径上生效，负责 bot 另用 Move 模式核对）。版本号、`fp_golden.txt` 没动。
- [ ] 结果里写明没跑真书回归、没上真机。

## 不做什么
- 不拆 `plan` 函数，不改任何算法、常量、函数签名；不合并、不改名任何函数；不顺手修 bug 或改注释。
- 不碰 big-file-splits 表里的其他文件。
- 不对整个仓库跑 `cargo fmt`（可以对新建和改动的这几个文件跑 `rustfmt`，但只能产生搬动引起的缩进、折行变化）。

## 约束
- 只用 Rust；clean-room；仓库不出现上游项目名及缩写。
- **只挪代码、不改行为**；不升版本号；`fp_golden.txt` 一行不动；不改 `testdata/`。
- 不碰 `~/.local/share/booklib`、真机；实验用 `target/` 下临时 `--library`。
- `install.sh` 不用 `--force`。
- 只在任务分支上工作，不合并；改「涉及文件」以外的写进「结果 → 偏离」。
- 提交信息中文 + 前缀 + 正文写原因，不加 Co-Authored-By。

## 涉及文件
- `crates/bookconv/src/wash/box_fix.rs`
- 新建 `crates/bookconv/src/wash/box_fix/{line,rewrite,tests}.rs`（名字可微调）

## 交付物
代码提交到任务分支并推送；「结果」填好；状态改为 done 或 blocked。任务单状态改动推 master，带 `[skip-notify]`。不要合并，不要跑二审。

## 给执行方的指令
```
执行任务单 .dev/tasks/2026-11-16-split-box-fix.md：先把状态改为 accepted 并提交，按「分步任务」做完，填好「结果」，状态改为 done（做不下去改为 blocked 并写明原因），提交并推送分支。任务单 done 推 master 带 [skip-notify]。不要合并，不要跑二审。
```

## 结果
- 执行方 / 开始–结束时间：box-agent（oc-exec） / 2026-11-16T16:08:51+08:00–2026-11-16T16:17:33+08:00
- base..head：`b9724beb8f1b8842720d03fd33e9b0460539aa7d..3cf0d5c43298ebc797be0424d051cd3e2923506b`
- 提交：
  | 提交号 | 说明 |
  |---|---|
  | c0f6b8b | chore: 开始执行任务单 2026-11-16-split-box-fix [skip-notify] |
  | 3cf0d5c | chore(wash): 拆 box_fix.rs 为 line/rewrite/tests 子模块 |
- 改动文件：`.dev/tasks/2026-11-16-split-box-fix.md`,`crates/bookconv/src/wash/box_fix.rs` `crates/bookconv/src/wash/box_fix/line.rs`,`crates/bookconv/src/wash/box_fix/rewrite.rs` `crates/bookconv/src/wash/box_fix/tests.rs`
- 测试：包装器未代跑验收命令，由负责 bot 按任务单核对
- 遇到的问题：无
- 偏离：无
- 新建文件（线索）：无
- 未完成：无
- 运行记录：`/home/box/oc-runs/2026-11-16-split-box-fix-20261116-160851.json`
- 日志：`/home/box/oc-runs/2026-11-16-split-box-fix-20261116-160851.log`
- 模型：`opencode-go/deepseek-v4.1-flash`
- 用时：实际运行 8.7 分钟（墙钟 522s / 实际运行 522s；超时按实际运行计）

## 审查
<!-- 负责 bot 填写 -->
- 验收：通过（负责 bot 独立验收，2026-11-16，独立 worktree；对照 master `b9724be`）。运行记录 JSON 的 `deviations` 为空，与结果一致；`wash` 其他文件未改。
  - 拆法：`box_fix.rs` 1119 行（模块说明、`use`、`mod`、`CSS_NAME`、`BoxGeometry`、`Plan`、`apply`、`skipped`、`heading`、`fmt`、`plan`——`plan` 约 880 行，按单子要求整体保留）；`box_fix/line.rs` 95（`RULE_W`、`Frame`、`LineStyle`、`Line`、`LineKey`、`frame_open`、`FRAME_CLOSE`）；`box_fix/rewrite.rs` 256（`Out`、`Job`、`Resolved`、`resolve`、`rewrite`、`list_indent_extra_px`、`MAX_TOP_PAD_REM`、`NARROW_SHARE`、`big_top_pad`、`page_top_rule_px`）；`box_fix/tests.rs` 559。
  - 只挪代码：按顶层项逐个比对（去掉可见性差异后），旧文件 56 项除 `mod tests` 外全部原样出现，没有文本差异；新增只有 `use`、`mod`、再导出、`//!` 说明。`tests.rs` 与原 `mod tests` 去掉 4 格缩进后逐行相同，只多一行 `//!` 说明。
  - 可见性：子模块里给 `box_fix` 用的改成 `pub(super)`；`page_top_rule_px` 原来 `pub(super)`（给 `top_spacer.rs` 用），现在 `pub(in crate::wash)` 并在 `box_fix.rs` 里 `pub(super) use` 再导出，`super::box_fix::page_top_rule_px` 原路径照旧，可见范围同拆前。
  - 发现并修正（提交 `d46610a`）：执行方把原本私有的 `frame_open` 用 `pub(super) use` 再导出给整个 `wash`，实际只有 `box_fix/rewrite.rs` 用；收回成 `line.rs` 里 `pub(super)`、`box_fix.rs` 里私有 `use`，可见范围回到 `box_fix` 内，模块说明里的 [`frame_open`] 链接照样能解析。
  - 测试：box_fix 17 项名字与 master 一致；`cargo test --workspace` 864 通过 0 失败；clippy `-D warnings` 0 警告（含上面的修正）。
  - 产物：master 与分支二进制对 4 本书同步到 ireader、kobo、kindle，12 份产物 sha256 全部相同，输出除路径外相同。另把测试用的 `xochitl`（Move）配置放进临时书库的 `profiles/` 当自定义模式（box_fix 主要在它上面生效），同样 4 本生成的 4 份产物 sha256 全部相同（文字书乙生成了 `eink-box.css`，确认 box_fix 走到了）。版本号、`fp_golden.txt` 没动。
  - 已知局限：`plan` 一个函数约 880 行，`box_fix.rs` 仍有 1119 行；再拆就要改控制流，不是只挪代码，留给 vinci 决定要不要另开单。
  - 没跑真书回归、没上真机。
