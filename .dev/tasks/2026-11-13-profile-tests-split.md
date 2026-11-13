# 把 `profile` 的测试拆成单独文件

- Task ID: 2026-11-13-profile-tests-split
- 负责 bot: sheng-ren
- 状态: done
- 执行方: box-agent（oc-exec）
- 分支: task/2026-11-13-profile-tests-split（从最新 master 切出）
- 更新时间: 2026-11-13T13:53:12+08:00
- 引用: 2026-11-12-quality-audit 发现 14 的一部分（「核实与分组」建议单 `profile-tests-split`）

## 需求原文
> （质量审查发现 14，已核实 ✅）十个文件超过 1800 行。其中 `crates/profile/src/lib.rs` 的测试从 `:1083` 起约 740 行（master `49139b5` 全文件 1832 行），最容易先拆；`bookconv` 的 `wash/tests.rs`、`optimize/tests.rs` 已经是单独文件，可以照这个办法拆。
> （2026-11-13）quality-audit 收尾拆单。本单只挪 `profile` 测试，其余大文件见 `2026-11-13-big-file-splits`。

## 输入
- 基于 origin/master `49139b5`。
- `crates/profile/src/lib.rs`：`#[cfg(test)]` 自 `:1083` 起直到文件末尾。挪走测试后，实现部分大约会降到 1100 行以下，不必再把实现本身拆进「拆大文件」那张单。
- 照 `bookconv` 已有做法：测试放到同 crate 的单独文件（如 `src/tests.rs` 或 `tests/` 集成测试，选与周围代码一致、改动最小的一种）。注意测试里用到的 `pub(crate)` / 夹具路径挪了之后仍能编译。

## 约束
- 只用 Rust；clean-room；仓库不出现上游项目名及缩写。
- **只挪测试代码，不改断言、不改 profile 行为、不改夹具语义**。`git diff` 应几乎只有文件挪动。
- **不升版本号**；`fp_golden.txt` 一行不动。
- 不碰 `~/.local/share/booklib`；测试不碰真机。
- `install.sh` 不用 `--force`。本仓库不用 rustfmt 默认宽度，别对整个仓库跑 `cargo fmt`。
- 提交信息中文 + 前缀 + 正文，不加 Co-Authored-By。

## 规则（要实现的行为）
1. 把 `lib.rs` 里的测试模块（或等价内容）挪到单独文件，公开 API 与测试数量、结果都不变。
2. 不顺手重构实现，不改 `testdata/`，除非路径引用必须跟着改（应尽量避免）。

## 分步任务
1. [x] 确认测试模块边界、对 `pub(crate)` 和夹具的依赖。
2. [x] 挪到单独文件，改 `lib.rs`（或 `Cargo.toml`）的引入，使测试仍能编译运行。
3. [x] `cargo test -p profile` 测试数量与结果和改前一致；`cargo test --workspace` 全过；`cargo clippy --workspace --all-targets -- -D warnings` 0 警告；`fp_golden.txt` 不动。

## 验收标准
- [x] `profile/src/lib.rs` 不再包含那约 740 行测试；测试在单独文件里通过。
- [x] `cargo test -p profile` 数量不变、全部通过；workspace 测试、clippy 干净。
- [x] 版本号和 `fp_golden.txt` 都没动；结果写明只是挪代码。

## 交付物
代码提交到任务分支并推送；「结果」填好；状态改为 done 或 blocked。任务单状态改动推 master，带 `[skip-notify]`。不要合并，不要跑二审。

## 给执行方的指令
```
执行任务单 .dev/tasks/2026-11-13-profile-tests-split.md：先把状态改为 accepted 并提交，按「分步任务」做完，填好「结果」，状态改为 done（做不下去改为 blocked 并写明原因），提交并推送分支。任务单 done 推 master 带 [skip-notify]。不要合并，不要跑二审。
```

## 结果
- 执行方 / 开始–结束时间：box-agent（oc-exec） / 2026-11-13T13:43:12+08:00–2026-11-13T13:46:06+08:00
- base..head：`145d9d963ee745bb83ddb8f8c4bbeadd436a08f0..6ac05cc8f37aab7a215b3f8c363da6efdacf632c`
- 提交：
  | 提交号 | 说明 |
  |---|---|
  | e73044d | chore: 开始执行任务单 2026-11-13-profile-tests-split [skip-notify] |
  | 6ac05cc | chore(profile): 把单测挪到单独的 tests.rs |
- 改动文件：`.dev/tasks/2026-11-13-profile-tests-split.md`,`crates/profile/src/lib.rs` `crates/profile/src/tests.rs`
- 测试：包装器未代跑验收命令，由负责 bot 按任务单核对
- 遇到的问题：无
- 偏离：
  - `crates/profile/src/tests.rs`
- 未完成：无
- 运行记录：`/home/box/oc-runs/2026-11-13-profile-tests-split-20261113-134312.json`
- 日志：`/home/box/oc-runs/2026-11-13-profile-tests-split-20261113-134312.log`
- 模型：`opencode-go/deepseek-v4.1-flash`

## 审查
- 验收：**通过**（sheng-ren bot，2026-11-13 13:47–13:50 Asia/Shanghai，独立 worktree `/tmp/sheng-ren-review-pts` @ `e5b0fe7`，对照 origin/master `145d9d9`）。没跑真书回归、没上真机（纯重构）。按 `Projects/_templates/review-checklist.md`「项目 bot 验收」逐项：结果已填、状态 done；分步任务完成；提交中文前缀加正文、无 Co-Authored-By、执行方未合并。工具链 rustc 1.99.1。
  - 偏离：oc-exec 把 `crates/profile/src/tests.rs` 标成偏离 1 个文件。这正是任务单要求新建的测试文件（「照 bookconv 已有做法：测试放到同 crate 的单独文件（如 `src/tests.rs`）」），不是超范围。真实偏离：无。
  - 只挪测试：实现提交 `6ac05cc` 只改 `lib.rs` / 新增 `tests.rs`。把 master 上 `mod tests { ... }` 去一层缩进后，与 `tests.rs` 正文逐字节相同（19 个 `#[test]` + 辅助函数 `temp_profile_dir` 名称与顺序一致）。`lib.rs` 实现部分与 master 相同，只在文末改成 `#[cfg(test)] mod tests;`（引入方式与 `bookconv` 的 `wash/tests.rs`、`optimize/tests.rs` 一致），并多一个空行。断言、夹具语义、profile 行为均未改。未改版本号、`fp_golden.txt`、`testdata/`、`profiles/`。
  - 逐条验收标准：
    - [x] `profile/src/lib.rs` 不再包含那约 740 行测试（现 1123 行，实现止于 1120 行）；测试在 `src/tests.rs`（757 行）里通过。
    - [x] `cargo test -p profile` 改前（master `145d9d9`）与改后均为 19 passed。`cargo test --workspace` 全过：bookbase lib 30，bookconv lib 553，csscascade lib 16，kfx lib 53，library lib 82 / bin 7 / cli 14 / flow 62 / fp_golden 1，profile lib 19。`cargo clippy --workspace --all-targets -- -D warnings` 0 警告。对改动文件跑 `rustfmt --check` 会按默认宽度把 `tests.rs` 里几处原有折行并回去，与仓库「不用 rustfmt 默认宽度」约定一致，未改格式。
    - [x] 版本号和 `fp_golden.txt` 都没动；只是挪代码。
  - 阻断：无。
- 二审：**无阻断**（agy-review，2026-11-13 13:51:30 Asia/Shanghai，模型 gemini-3.8-flash-medium，用时 52s，回执 `.dev/tasks/2026-11-13-profile-tests-split.agy.md`）。结论「建议通过」。负责 bot 已读回执并核对引用：`lib.rs:1122-1123` 的 `mod tests;`、`tests.rs` 1–757 行与 19 个测试及 `temp_profile_dir`、kobo 的 `include_str!` 在 710 行，均核实。ireader 的 `include_str!` 实际在 675 行，回执写成 665，差 10 行，路径与内容核对无误。建议与线索均无。未核实项只有 agy 未亲自跑测试与 clippy（验收与合并后已重跑）。
- 合并：`4ab7903`（2026-11-13 13:52 Asia/Shanghai，`--no-ff` 合入当时 origin/master `145d9d9`）。合并后再跑 `cargo test --workspace` 全过（计数与验收相同），`cargo clippy --workspace --all-targets -- -D warnings` 0 警告。已推 origin/master。状态保持 **done**（不是 verified），**待 vinci 验收后改 verified**。没跑真书回归、没上真机（纯重构，待本机回归确认产物逐字节不变）。未删除远程任务分支。
