# 拆 `library/src/generate.rs`（big-file-splits 子单 5）

- Task ID: 2026-11-13-split-generate
- 负责 bot: sheng-ren
- 状态: accepted
- 执行方: 待定
- 分支: task/2026-11-13-split-generate（从最新 master 切出）
- 更新时间: 2026-11-13T14:12:01+08:00
- 引用: 2026-11-13-big-file-splits 分步 5；依赖 2026-11-13-prepare-file-split（已合并 8101e7e）

## 需求原文
> （质量审查发现 14）十个核心文件单文件超过 1800 行，难审、难改。只做模块拆分、不改函数逻辑与公开签名，各设备产物及 `fp_golden` 应逐字节不变。执行时一次只拆一个文件；测试数量和结果不变。
> （2026-11-13）big-file-splits 按文件拆子单，本单只拆 `crates/library/src/generate.rs`。

## 输入
- 基于最新 origin/master（含 prepare-file-split 合并 `8101e7e`）。
- `crates/library/src/generate.rs`：按职责拆成 `generate/` 下的子模块（例如生成主流程、位置与认领、生成记录读改写、测试），`generate.rs` 或 `generate/mod.rs` 只留入口与再导出。具体分法按代码现状定，在结果里列出每个子模块放了什么。

## 约束
- 只用 Rust；clean-room；仓库不出现上游项目名及缩写。
- **只挪代码、不改行为**：不改函数体、分支条件、错误文案、写盘时机、公开签名。允许 `pub(crate) use` 再导出以保持 crate 内原有 `use` 路径，或同步改 crate 内引用。
- **只拆这一个文件**，不碰 big-file-splits 表里的其他文件，不顺手做别的重构。
- **不升版本号**；`fp_golden.txt` 一行不动；不改 `testdata/`。
- 不碰 `~/.local/share/booklib`，实验用 `target/` 下临时 `--library`；测试不碰真机。
- `install.sh` 不用 `--force`。本仓库不用 rustfmt 默认宽度，别对整个仓库跑 `cargo fmt`。
- 提交信息中文 + 前缀 + 正文，不加 Co-Authored-By。

## 规则（要实现的行为）
1. 拆完后 crate 能编译，对外 API 与行为不变；测试集合（名称、数量）与结果和拆前一致。
2. 每个子文件远低于 1800 行。

## 分步任务
1. [ ] 读 `generate.rs`，列出各段职责与相互调用，定拆法。
2. [ ] 挪代码到子模块，调整 `mod`/`use`，保证编译。
3. [ ] 拆前、拆后各跑一次 `cargo test -p library`，测试数量一致；`cargo test --workspace` 全过；`cargo clippy --workspace --all-targets -- -D warnings` 0 警告；确认版本号与 `fp_golden.txt` 没动。

## 验收标准
- [ ] `generate.rs` 已拆成合理子模块，每个文件远低于 1800 行。
- [ ] 测试数量与结果与拆前一致；workspace 测试、clippy 干净。
- [ ] 版本号和 `fp_golden.txt` 都没动；结果写明只是挪代码，没跑真书回归、没上真机，待本机回归确认产物逐字节不变。

## 交付物
代码提交到任务分支并推送；「结果」填好；状态改为 done 或 blocked。任务单状态改动推 master，带 `[skip-notify]`。不要合并，不要跑二审。

## 给执行方的指令
```
执行任务单 .dev/tasks/2026-11-13-split-generate.md：先把状态改为 accepted 并提交，按「分步任务」做完，填好「结果」，状态改为 done（做不下去改为 blocked 并写明原因），提交并推送分支。任务单 done 推 master 带 [skip-notify]。不要合并，不要跑二审。
```

## 结果
<!-- 执行方填写 -->
- 执行方 / 开始–结束时间：
- base..head：
- 提交：| 步骤 | 提交号 | 文件 | 说明 |
- 子模块划分：
- 测试：| 命令 | 通过 / 失败 / 未跑 | 摘要 |
- 遇到的问题：
- 偏离：无
- 未完成：
- 没跑真书回归、没上真机（纯重构，待本机回归确认产物逐字节不变）。
