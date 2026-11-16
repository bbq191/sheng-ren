# 测试外移：footnote.rs、kindle_rules.rs、metadata.rs 的单元测试挪到子模块文件

- Task ID: 2026-11-16-tests-out-three
- 负责 bot: sheng-ren
- 状态: done
- 执行方: oc-exec
- 分支: task/2026-11-16-tests-out-three（从最新 master `b33cf6f` 切出）
- 更新时间: 2026-11-16T16:59:33+08:00
- 引用: `2026-11-13-big-file-splits` 第 7、8、9 步

## 需求原文
> （vinci 2026-11-16，sheng-ren 聊天）1. 开单
> （负责 bot 转达）还剩三个超过 1800 行的文件，每个文件里有 800 到 1200 行是测试，把测试挪出去就能降下来，三个合成一张小单。

## 输入（开单时 master `b33cf6f`，负责 bot 读代码核实）
| 文件 | 行数 | 测试模块（`#[cfg(test)] mod …` 起始行，都一直到文件末尾） | 测试数 |
|---|---|---|---|
| `crates/bookconv/src/htmlproc/footnote.rs` | 1879 | `mod footnote_inline_tests`（1107–1224）、`mod optimizer_footnote_tests`（1226–1879） | 5 + 25 |
| `crates/bookconv/src/wash/kindle_rules.rs` | 1856 | `mod tests`（1243–1856） | 18 |
| `crates/library/src/metadata.rs` | 1848 | `mod tests`（1009–1848） | 13 |

- 文件中间另有几个 `#[cfg(test)] fn …` 辅助（如 `footnote.rs:675` `referenced_note_frags`、`kindle_rules.rs:847` `rewrite_css`），**留在原处不动**。
- 仓库已有做法（`wash/box_fix.rs:33-34`、`imgopt.rs:22` 等）：原文件里写 `#[cfg(test)]\nmod tests;`，测试放在同名目录下 `<名>/tests.rs`，开头一行 `//!` 说明 + `use super::*;`，内容去掉 4 格缩进。测试路径不变。
- `footnote.rs` 里两个测试模块都含多行原始字符串（如 1162–1163、1186–1187、1771–1775 等），续行顶格写。
- 现状测试数：bookconv lib 553，library lib 91；`cargo test --workspace` 864。

## 方案
- `footnote.rs`：两个测试模块分别挪到 `crates/bookconv/src/htmlproc/footnote/footnote_inline_tests.rs`、`…/footnote/optimizer_footnote_tests.rs`；原处改成 `#[cfg(test)]\nmod footnote_inline_tests;`、`#[cfg(test)]\nmod optimizer_footnote_tests;`（模块名不变，测试路径不变）。
- `kindle_rules.rs` → `crates/bookconv/src/wash/kindle_rules/tests.rs`；`metadata.rs` → `crates/library/src/metadata/tests.rs`；原处 `#[cfg(test)]\nmod tests;`。
- 新文件开头一行 `//! 原 `<文件>` 的单元测试（拆分后测试路径不变：`…`）。`，然后是原模块体：**只去掉行首正好 4 个空格**；**多行字符串字面量里的续行一个字节都不改**（不管它有没有缩进）。原模块里的 `use super::*;` 等 `use` 原样保留。
- 每个文件单独一个提交。

## 规则（要实现的行为）
1. 只挪测试模块；生产代码、文件中间的 `#[cfg(test)] fn` 辅助、可见性一律不动。
2. 测试完整路径、名字和数量与改前一致（bookconv lib 553、library lib 91、workspace 864）。
3. 三个原文件都降到 1800 行以下。

## 分步任务
1. [ ] 改前 `cargo test -p bookconv --lib -- --list` 与 `cargo test -p library --lib -- --list` 存下清单。
2. [ ] 按「方案」挪 `footnote.rs`，单独提交。
3. [ ] 挪 `kindle_rules.rs`，单独提交。
4. [ ] 挪 `metadata.rs`，单独提交。
5. [ ] 改后再列，完整路径逐行比对一致；`wc -l` 记下三个原文件新行数。
6. [ ] `cargo test --workspace` 全过；`cargo clippy --workspace --all-targets -- -D warnings` 0 警告；版本号、`fp_golden.txt` 没动。

## 验收标准
- [ ] 三个原文件都低于 1800 行；测试在子模块文件里。
- [ ] 测试完整路径清单与改前完全一致；全过；clippy 0 警告。
- [ ] 把 master 测试模块每行去掉 4 格（多行字面量续行除外）后和新文件比，除开头 `//!` 一行外完全相同。
- [ ] 生产代码没动；产物逐字节不变（负责 bot 用 master 与分支二进制对照）；版本号、`fp_golden.txt` 没动。
- [ ] 结果里写明没跑真书回归、没上真机。

## 不做什么
- 不改测试内容、不加测试、不改可见性、不挪生产代码。
- 不对整个仓库或这几个文件跑 `cargo fmt` / rustfmt。

## 约束
- 只用 Rust；clean-room；仓库不出现上游项目名及缩写。
- **只挪代码、不改行为**；不升版本号；`fp_golden.txt` 一行不动；不改 `testdata/`。
- 不碰 `~/.local/share/booklib`、真机；实验用 `target/` 下临时 `--library`。
- `install.sh` 不用 `--force`。
- 只在任务分支上工作，不合并；改「涉及文件」以外的写进「结果 → 偏离」。
- 提交信息中文 + 前缀 + 正文写原因，不加 Co-Authored-By。

## 涉及文件
- `crates/bookconv/src/htmlproc/footnote.rs`、`crates/bookconv/src/htmlproc/footnote/{footnote_inline_tests,optimizer_footnote_tests}.rs`（新）
- `crates/bookconv/src/wash/kindle_rules.rs`、`crates/bookconv/src/wash/kindle_rules/tests.rs`（新）
- `crates/library/src/metadata.rs`、`crates/library/src/metadata/tests.rs`（新）

## 交付物
代码提交到任务分支并推送；「结果」填好；状态改为 done 或 blocked。任务单状态改动推 master，带 `[skip-notify]`。不要合并，不要跑二审。

## 给执行方的指令
```
执行任务单 .dev/tasks/2026-11-16-tests-out-three.md：先把状态改为 accepted 并提交，按「分步任务」做完，填好「结果」，状态改为 done（做不下去改为 blocked 并写明原因），提交并推送分支。任务单 done 推 master 带 [skip-notify]。不要合并，不要跑二审。
```

## 结果
- 执行方 / 开始–结束时间：box-agent（oc-exec） / 2026-11-16T16:53:23+08:00–2026-11-16T16:59:33+08:00
- base..head：`bcc27d0f4a234d8ff238083a55edf32d45c4e648..4e7c2151671168aa8076c43a99a067ee8613bc0f`
- 提交：
  | 提交号 | 说明 |
  |---|---|
  | ee70ae0 | chore: 开始执行任务单 2026-11-16-tests-out-three [skip-notify] |
  | 9403bc1 | chore(bookconv): footnote.rs 的单元测试挪到子模块文件 |
  | 45fb993 | chore(bookconv): kindle_rules.rs 的单元测试挪到子模块文件 |
  | 4e7c215 | chore(library): metadata.rs 的单元测试挪到子模块文件 |
- 改动文件：`.dev/tasks/2026-11-16-tests-out-three.md`,`crates/bookconv/src/htmlproc/footnote.rs` `crates/bookconv/src/htmlproc/footnote/footnote_inline_tests.rs`,`crates/bookconv/src/htmlproc/footnote/optimizer_footnote_tests.rs` `crates/bookconv/src/wash/kindle_rules.rs`,`crates/bookconv/src/wash/kindle_rules/tests.rs` `crates/library/src/metadata.rs`,`crates/library/src/metadata/tests.rs`
- 测试：包装器未代跑验收命令，由负责 bot 按任务单核对
- 遇到的问题：无
- 偏离：无
- 新建文件（线索）：无
- 未完成：无
- 运行记录：`/home/box/oc-runs/2026-11-16-tests-out-three-20261116-165323.json`
- 日志：`/home/box/oc-runs/2026-11-16-tests-out-three-20261116-165323.log`
- 模型：`opencode-go/deepseek-v4.1-flash`
- 用时：实际运行 6.2 分钟（墙钟 370s / 实际运行 370s；超时按实际运行计）

## 审查
<!-- 负责 bot 填写 -->
- 验收：
- 二审：
- 合并：
