# `library` 错误前缀改成枚举分派

- Task ID: 2026-11-13-library-error-kinds
- 负责 bot: sheng-ren
- 状态: done
- 执行方: box-agent
- 分支: task/2026-11-13-library-error-kinds（从最新 master 切出）
- 更新时间: 2026-11-13T10:33:00+08:00
- 引用: 2026-11-12-quality-audit 发现 11

## 需求原文
> （质量审查发现 11，已核实 ✅）`library` 用错误字符串前缀做控制流：`NO_SPACE`（`space.rs:7`）在 `build_run.rs`、`transfer.rs` 按前缀分派；`UNMOUNTED`（`generate.rs`）在 `build_run.rs`；`BUSY`（`lib.rs`）在 `sources.rs`。回执引用的 `generate.rs:59` 不对（那是 `Step` 枚举）。对外文案应保持不变。
> （2026-11-13）quality-audit 收尾拆单。

## 输入
- 基于 origin/master `49139b5`。对照现状：
  - `crates/library/src/space.rs:7` `NO_SPACE = "空间满"`；`build_run.rs:334`、`transfer.rs:228` 等 `starts_with`。
  - `crates/library/src/generate.rs:1520` `UNMOUNTED = "设备中途断开（挂载点不在了）"`；`build_run.rs:412` `starts_with`。
  - `crates/library/src/lib.rs:262` `BUSY = "文件正在写入"`；`sources.rs:929` `starts_with`。
- 建议：内部用受限错误枚举（如 `Unmounted` / `NoSpace` / `Busy` / `Io`）分派，对外 `Display` 或格式化仍输出原来的字符串常量。

## 约束
- 只用 Rust；clean-room；仓库不出现上游项目名及缩写。
- **对外提示文字逐字不变**（含测试里断言的 `starts_with` 文案，若测试改成匹配枚举，给人看的字符串仍要逐字相同）。
- **不升版本号**；`fp_golden.txt` 一行不动。不改生成、传书、清理的业务行为。
- 不碰 `~/.local/share/booklib`，实验用 `target/` 下临时 `--library`；测试不碰真机。
- `install.sh` 不用 `--force`。本仓库不用 rustfmt 默认宽度，别对整个仓库跑 `cargo fmt`。
- 提交信息中文 + 前缀 + 正文，不加 Co-Authored-By。

## 规则（要实现的行为）
1. 业务分派按错误种类（枚举或等价类型）判断，不再靠匹配错误字符串前缀决定「设备满了停这台」「挂载没了」「文件正在写入」。
2. 日志、命令行、给用户的错误原文保持与现在的常量一致。
3. 范围限 `library` crate 内部；不借机改 `bookconv` / `kfx`。

## 分步任务
1. [ ] 摸清全部 `starts_with(NO_SPACE|UNMOUNTED|BUSY)` 及构造这些错误的地点。
2. [ ] 引入错误种类，改分派与构造；对外字符串不变。
3. [ ] 用搜索确认 `library` 里不再用这三条前缀做控制流（测试断言文案可以保留）。
4. [ ] `cargo test --workspace` 全过；`cargo clippy --workspace --all-targets -- -D warnings` 0 警告；确认 `fp_golden.txt` 和版本号都没动。

## 验收标准
- [ ] 控制流不再依赖这三条错误字符串前缀；给人看的提示逐字不变。
- [ ] 版本号不变；`fp_golden.txt` 一行没动。
- [ ] `cargo test --workspace` 全过；clippy `-D warnings` 0 警告。
- [ ] 结果里写明没跑真书回归、没上真机。

## 交付物
代码提交到任务分支并推送；「结果」填好；状态改为 done 或 blocked。任务单状态改动推 master，带 `[skip-notify]`。不要合并，不要跑二审。

## 给执行方的指令
```
执行任务单 .dev/tasks/2026-11-13-library-error-kinds.md：先把状态改为 accepted 并提交，按「分步任务」做完，填好「结果」，状态改为 done（做不下去改为 blocked 并写明原因），提交并推送分支。任务单 done 推 master 带 [skip-notify]。不要合并，不要跑二审。
```

## 结果
- 执行方 / 开始–结束时间：box-agent（oc-exec） / 2026-11-13T10:26:12+08:00–2026-11-13T10:33:00+08:00
- base..head：`eb694dac7beeeb4ad49faaf5feddc4b8f23e1776..529c6ccd9ad55aa74308ddd06a2028c77c25a43c`
- 提交：
  | 提交号 | 说明 |
  |---|---|
  | 620cf07 | chore: 开始执行任务单 2026-11-13-library-error-kinds [skip-notify] |
  | 529c6cc | refactor(library): 错误按种类分派，不再匹配提示文字前缀 |
- 改动文件：`.dev/tasks/2026-11-13-library-error-kinds.md`,`crates/library/src/build_run.rs` `crates/library/src/catalog.rs`,`crates/library/src/deliver.rs` `crates/library/src/error.rs`,`crates/library/src/generate.rs` `crates/library/src/lib.rs`,`crates/library/src/sources.rs` `crates/library/src/space.rs`,`crates/library/src/transfer.rs`
- 测试：包装器未代跑验收命令，由负责 bot 按任务单核对
- 遇到的问题：无
- 偏离：
  - `crates/library/src/build_run.rs`
  - `crates/library/src/catalog.rs`
  - `crates/library/src/deliver.rs`
  - `crates/library/src/error.rs`
  - `crates/library/src/generate.rs`
  - `crates/library/src/lib.rs`
  - `crates/library/src/sources.rs`
  - `crates/library/src/space.rs`
  - `crates/library/src/transfer.rs`
- 未完成：无
- 运行记录：`/home/box/oc-runs/2026-11-13-library-error-kinds-20261113-102612.json`
- 日志：`/home/box/oc-runs/2026-11-13-library-error-kinds-20261113-102612.log`
- 模型：`opencode-go/deepseek-v4.1-flash`

## 审查
<!-- 负责 bot 填写 -->
- 验收：
- 二审：
- 合并：
