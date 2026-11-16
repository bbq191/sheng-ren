# 拆 `library/src/bin/booklib/main.rs`（big-file-splits 子单）

- Task ID: 2026-11-16-split-booklib-main
- 负责 bot: sheng-ren
- 状态: done
- 执行方: oc-exec
- 分支: task/2026-11-16-split-booklib-main（从最新 master 切出）
- 更新时间: 2026-11-16T16:01:32+08:00
- 引用: `2026-11-13-big-file-splits` 分步 6（审查建议「各子命令的 `cmd_*` 抽到子模块」）；拆法参照 `2026-11-16-split-csscascade`、`2026-11-13-split-imgopt`

## 需求原文
> （质量审查发现 14）十个核心文件单文件超过 1800 行，难审、难改。只做模块拆分、不改函数逻辑与公开签名，各设备产物及 `fp_golden` 应逐字节不变。执行时一次只拆一个文件；测试数量和结果不变。
> （负责 bot 转达的第 3 项，2026-11-16）剩下的大文件按收益从大到小一张单拆一个；拆分只搬代码，不改行为；每张单证明产物逐字节不变，版本号不动。

## 输入（开单时 master，`wc -l` 2421 行；比审查时的 1891 行多，主要是 sync-mount-stall 加的工作进程和 warning-noise 加的汇总）
`crates/library/src/bin/booklib/main.rs` 是二进制 crate 的入口（同目录已有 `complete.rs`、`meta_edit.rs` 两个子模块），现有各段（行号约数）：
| 行 | 内容 |
|---|---|
| 1–37 | 模块说明、`mod complete; mod meta_edit;`、`use`、`struct Command` |
| 38–301 | `COMMANDS` 命令表（帮助文字、选项、`check`/`run` 函数指针） |
| 302–453 | `HELP_HEAD`、`HELP_TAIL`、`synopsis`、`full_help`、`command_usage`、`version`、`command_word`、`print_help_if_asked`、`BUILD_MERGED`、`usage_error`、`fail` |
| 454–521 | `Args` 与 `impl Args` |
| 522–533 | `added_line` |
| 534–624 | `parse_devices`、`forget_reconnected`、`space_freed`、`mark_dropped` |
| 625–1004 | 输出：`counts_summary`、`event_line`、`built_line`、`is_round_summary_warning`、`WarningSummary`、`WarnedBooks`、`info_summary`、`show_meta`、`path_hint` |
| 1005–1177 | `meta_edit_args`、`main`、`supervise`（工作进程）、`Ctx` |
| 1178–1224 | 用法检查：`check_options`、`check_sync`、`check_meta`、`always`、`never`、`meta_writes`、`check_vertical` |
| 1225–1454 | `cmd_vertical`、`cmd_devices`、`cmd_add`、`audit_lines`、`cmd_list`、`cmd_dedupe` |
| 1455–1690 | 跟踪：`cmd_track`、`cmd_untrack`、`device_ids`、`track_or_untrack`、`resolve_track_path`、`with_book_ext`、`placement_shown`、`print_tracked` |
| 1691–1822 | `cmd_remove`、`cmd_meta`、`meta_show`、`meta_fetch` |
| 1823–2146 | 同步：`SyncPlan`、`SyncState`、`cmd_sync`、`impl SyncState` |
| 2147–2421 | `#[cfg(test)] mod tests`（8 个测试） |

## 方案（建议拆法，可按代码现状微调，在结果里列出每个文件放了什么、多少行）
- `main.rs`：模块说明、`mod` 声明、`main`、`supervise`、`meta_edit_args`、`Ctx`、`Args`、`usage_error`、`fail`、用法检查函数（`check_*`、`always`、`never`、`meta_writes`）。
- `help.rs`：`Command`、`COMMANDS`、`HELP_HEAD`、`HELP_TAIL`、`synopsis`、`full_help`、`command_usage`、`version`、`command_word`、`print_help_if_asked`、`BUILD_MERGED`。
- `report.rs`：结果行与汇总的格式化（`added_line`、`counts_summary`、`event_line`、`built_line`、`is_round_summary_warning`、`WarningSummary`、`WarnedBooks`、`path_hint`）。
- `books.rs`：`cmd_vertical`、`cmd_devices`、`cmd_add`、`audit_lines`、`cmd_list`、`cmd_dedupe`、`cmd_remove`。
- `meta.rs`：`cmd_meta`、`meta_show`、`meta_fetch`、`info_summary`、`show_meta`（注意已有 `meta_edit.rs`，不要合并进去）。
- `track.rs`：跟踪那一段（1455–1690）。
- `sync.rs`：`parse_devices`、`forget_reconnected`、`space_freed`、`mark_dropped`、`SyncPlan`、`SyncState`、`cmd_sync`。
- `tests.rs`：原 `mod tests` 整体搬出，`main.rs` 里 `#[cfg(test)] mod tests;`，测试名和路径（`tests::…`）不变；测试里需要的名字用 `use crate::子模块::…` 引入。
- 这是二进制 crate，没有对外接口：跨模块访问原先私有的项改成 `pub(crate)`（不要用 `pub`）。函数指针表 `COMMANDS` 引用的 `check_*`、`cmd_*` 用 `use` 引入。
- 不用 `include!`。

## 规则（要实现的行为）
1. 只挪代码：函数体、分支条件、帮助文字、输出文字、退出码、常量值、注释文字都不改；允许的改动只有：`mod` / `use`、私有项改 `pub(crate)`、每个新文件开头一段 `//!` 说明、搬出测试后的缩进变化（以及因此产生的 rustfmt 折行）。
2. 命令行行为一字不差：`booklib --help`、`booklib help <命令>`、`--version`、各命令输出、退出码都不变。
3. 测试集合（名字、数量：main 8 个、complete 4 个）与结果和拆前一致。
4. 每个文件远低于 1800 行（目标各 ≤ 800）。

## 分步任务
1. [ ] 读 `main.rs`，列出各段职责和相互调用，定拆法。
2. [ ] 拆前跑 `cargo test -p library --bin booklib -- --list` 存测试名清单。
3. [ ] 挪代码到子模块，调整 `mod`/`use`/可见性，保证编译。
4. [ ] 拆后再列测试名，与拆前一致；`cargo test --workspace` 全过；`cargo clippy --workspace --all-targets -- -D warnings` 0 警告；版本号、`fp_golden.txt` 没动。
5. [ ] 结果里列出每个文件的内容和行数、所有可见性改动。

## 验收标准
- [ ] `main.rs` 已拆成合理子模块，每个文件远低于 1800 行。
- [ ] 只挪代码：负责 bot 逐项比对核实，除允许的改动外没有别的差异。
- [ ] 测试名字、数量一致、全过；`cargo test --workspace` 全过；clippy `-D warnings` 0 警告。
- [ ] 命令行输出不变（负责 bot 用 master 与分支二进制对照 `--help`、各命令 `help`、`--version` 去掉提交号后、`list`、`devices`、`track` 列表、`sync` 输出）。
- [ ] 产物逐字节不变（负责 bot 用 master 与分支二进制对 ireader、kobo、kindle 各生成几本书比 sha256）。版本号、`fp_golden.txt` 没动。
- [ ] 结果里写明没跑真书回归、没上真机。

## 不做什么
- 不改任何命令的行为、帮助文字、输出格式；不合并、不改名任何函数；不顺手修 bug 或改注释。
- 不动 `complete.rs`、`meta_edit.rs` 的内容（只在需要时改它们里面的 `use super::…` 路径，算偏离写进结果）；不碰 big-file-splits 表里的其他文件。
- 不对整个仓库跑 `cargo fmt`（可以对新建和改动的这几个文件跑 `rustfmt`，但只能产生搬动引起的缩进、折行变化；有别的格式变化就不要用）。

## 约束
- 只用 Rust；clean-room；仓库不出现上游项目名及缩写。
- **只挪代码、不改行为**；不升版本号；`fp_golden.txt` 一行不动；不改 `testdata/`。
- 不碰 `~/.local/share/booklib`、真机；实验用 `target/` 下临时 `--library`。
- `install.sh` 不用 `--force`。
- 只在任务分支上工作，不合并；改「涉及文件」以外的写进「结果 → 偏离」。
- 提交信息中文 + 前缀 + 正文写原因，不加 Co-Authored-By。

## 涉及文件
- `crates/library/src/bin/booklib/main.rs`
- 新建 `crates/library/src/bin/booklib/{help,report,books,meta,track,sync,tests}.rs`（名字可微调）

## 交付物
代码提交到任务分支并推送；「结果」填好；状态改为 done 或 blocked。任务单状态改动推 master，带 `[skip-notify]`。不要合并，不要跑二审。

## 给执行方的指令
```
执行任务单 .dev/tasks/2026-11-16-split-booklib-main.md：先把状态改为 accepted 并提交，按「分步任务」做完，填好「结果」，状态改为 done（做不下去改为 blocked 并写明原因），提交并推送分支。任务单 done 推 master 带 [skip-notify]。不要合并，不要跑二审。
```

## 结果
- 执行方 / 开始–结束时间：box-agent（oc-exec） / 2026-11-16T15:49:10+08:00–2026-11-16T16:01:32+08:00
- base..head：`233271c825a5594ddc79b6e594cd49afbd5febaa..15cbce112956a2b70eed62642b04136886afad4e`
- 提交：
  | 提交号 | 说明 |
  |---|---|
  | 2f6e4c9 | chore: 开始执行任务单 2026-11-16-split-booklib-main [skip-notify] |
  | 15cbce1 | chore(library): 拆 booklib/main.rs 成子模块 |
- 改动文件：`.dev/tasks/2026-11-16-split-booklib-main.md`,`crates/library/src/bin/booklib/books.rs` `crates/library/src/bin/booklib/help.rs`,`crates/library/src/bin/booklib/main.rs` `crates/library/src/bin/booklib/meta.rs`,`crates/library/src/bin/booklib/report.rs` `crates/library/src/bin/booklib/sync.rs`,`crates/library/src/bin/booklib/tests.rs` `crates/library/src/bin/booklib/track.rs`
- 测试：包装器未代跑验收命令，由负责 bot 按任务单核对
- 遇到的问题：无
- 偏离：无
- 新建文件（线索）：无
- 未完成：无
- 运行记录：`/home/box/oc-runs/2026-11-16-split-booklib-main-20261116-154910.json`
- 日志：`/home/box/oc-runs/2026-11-16-split-booklib-main-20261116-154910.log`
- 模型：`opencode-go/deepseek-v4.1-flash`
- 用时：实际运行 12.4 分钟（墙钟 742s / 实际运行 742s；超时按实际运行计）

## 审查
<!-- 负责 bot 填写 -->
- 验收：通过（负责 bot 独立验收，2026-11-16，独立 worktree；对照 master `233271c`）。运行记录 JSON 的 `deviations` 为空，与结果一致；`complete.rs`、`meta_edit.rs` 未改。
  - 拆法（执行方结果没列，负责 bot 补）：`main.rs` 339 行（模块说明、`mod`、`main`、`supervise`、`meta_edit_args`、`Ctx`、`Args`、`usage_error`、`fail`、用法检查函数）；`help.rs` 420（`Command`、`COMMANDS`、帮助文字与 `synopsis` 等、`version`、`command_word`、`print_help_if_asked`、`BUILD_MERGED`）；`report.rs` 303（`added_line`、`counts_summary`、`event_line`、`built_line`、`WarningSummary`、`WarnedBooks`、`path_hint` 等）；`books.rs` 254（`cmd_vertical`、`cmd_devices`、`cmd_add`、`audit_lines`、`cmd_list`、`cmd_dedupe`、`cmd_remove`）；`meta.rs` 210（`cmd_meta`、`meta_show`、`meta_fetch`、`info_summary`、`show_meta`）；`track.rs` 242；`sync.rs` 423（`parse_devices`、`forget_reconnected`、`space_freed`、`mark_dropped`、`SyncPlan`、`SyncState`、`cmd_sync`）；`tests.rs` 279。最大 423 行。
  - 只挪代码：按顶层项逐个比对（去掉 `pub(crate)` 差异后），旧文件 86 项除原 `use library::{…}` 和 `mod tests` 外全部原样出现在新文件里，没有任何文本差异；新增只有 `use`、`mod`、各文件 `//!` 说明。`tests.rs` 与原 `mod tests` 去掉 4 格缩进后逐行相同，只是把 `use super::*;` 换成了具体的 `use`。可见性只有私有 → `pub(crate)`（各 `cmd_*`、`Command` 及字段、`COMMANDS`、`synopsis`、`command_word`、`print_help_if_asked`、`BUILD_MERGED`、`report.rs` 的格式化函数与 `WarningSummary` 方法、`sync.rs` 的四个辅助函数、`audit_lines`、`info_summary`、`path_hint`），没有新的 `pub`。
  - 测试：booklib 二进制 12 项（main 8、complete 4）名字与 master 一致；`cargo test --workspace` 864 通过 0 失败；clippy `-D warnings` 0 警告。
  - 命令行输出：master 与分支二进制各跑 40 种调用（`--help`、`-h`、`--version`、`version`、`help`、10 个命令的 `help <命令>` 与 `<命令> --help`、未知命令、`build`、错误选项、`list`、`list 甲`、`devices`、`track`、`meta`、`vertical`、`dedupe`、`sync`、`sync --no-build`、`remove` / `untrack` 找不到的），输出和退出码逐字相同；差异只有 `--version` 里的提交号，以及 `sync` 一段里两条结果行先后不同（设备传输线程并发的先后，排序后相同）。
  - 产物：master 与分支二进制对 4 本书同步到 ireader、kobo、kindle，12 份产物 sha256 全部相同。版本号、`fp_golden.txt` 没动。
  - 没跑真书回归、没上真机。
- 二审：agy-review（gemini-3.8-flash-medium）建议通过；无阻断、无建议。线索一条（执行方结果漏列各文件行数，验收已补）已处理。见 `2026-11-16-split-booklib-main.agy.md`。
