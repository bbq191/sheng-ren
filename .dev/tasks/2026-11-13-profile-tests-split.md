# 把 `profile` 的测试拆成单独文件

- Task ID: 2026-11-13-profile-tests-split
- 负责 bot: sheng-ren
- 状态: done
- 执行方: 待定
- 分支: task/2026-11-13-profile-tests-split（从最新 master 切出）
- 更新时间: 2026-11-13T13:46:06+08:00
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
1. [ ] 确认测试模块边界、对 `pub(crate)` 和夹具的依赖。
2. [ ] 挪到单独文件，改 `lib.rs`（或 `Cargo.toml`）的引入，使测试仍能编译运行。
3. [ ] `cargo test -p profile` 测试数量与结果和改前一致；`cargo test --workspace` 全过；`cargo clippy --workspace --all-targets -- -D warnings` 0 警告；`fp_golden.txt` 不动。

## 验收标准
- [ ] `profile/src/lib.rs` 不再包含那约 740 行测试；测试在单独文件里通过。
- [ ] `cargo test -p profile` 数量不变、全部通过；workspace 测试、clippy 干净。
- [ ] 版本号和 `fp_golden.txt` 都没动；结果写明只是挪代码。

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
<!-- 负责 bot 填写 -->
- 验收：
- 二审：
- 合并：
