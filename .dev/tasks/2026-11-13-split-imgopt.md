# 拆 `bookconv/src/imgopt.rs`（big-file-splits 子单 1）

- Task ID: 2026-11-13-split-imgopt
- 负责 bot: sheng-ren
- 状态: blocked
- 执行方: 待定
- 分支: task/2026-11-13-split-imgopt（从最新 master 切出）
- 更新时间: 2026-11-13T16:08:37+08:00
- 引用: 2026-11-13-big-file-splits 分步 1；前一张子单 2026-11-13-split-generate 已合并 0d85255

## 需求原文
> （质量审查发现 14）十个核心文件单文件超过 1800 行，难审、难改。只做模块拆分、不改函数逻辑与公开签名，各设备产物及 `fp_golden` 应逐字节不变。执行时一次只拆一个文件；测试数量和结果不变。
> （2026-11-13）big-file-splits 按文件拆子单，本单只拆 `crates/bookconv/src/imgopt.rs`。

## 输入
- 基于最新 origin/master（含 split-generate 合并 `0d85255`）。
- `crates/bookconv/src/imgopt.rs`（约 3900 行）：按职责拆成 `imgopt/` 下的子模块（审查建议如页面缓冲 / 漫画处理 / 封面缩略图 / 测试），`imgopt.rs` 只留入口与再导出。**不改任何图像算法、参数、常量、像素处理顺序**，产物必须逐字节不变。可参照已合并的 `crates/library/src/generate/` 拆法。具体分法按代码现状定，在结果里列出每个子模块放了什么。

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
1. [ ] 读 `imgopt.rs`，列出各段职责与相互调用，定拆法。
2. [ ] 挪代码到子模块，调整 `mod`/`use`，保证编译。
3. [ ] 拆前、拆后各跑一次 `cargo test -p bookconv`，测试数量一致；`cargo test --workspace` 全过；`cargo clippy --workspace --all-targets -- -D warnings` 0 警告；确认版本号与 `fp_golden.txt` 没动。

## 验收标准
- [ ] `imgopt.rs` 已拆成合理子模块，每个文件远低于 1800 行。
- [ ] 测试数量与结果与拆前一致；workspace 测试、clippy 干净。
- [ ] 版本号和 `fp_golden.txt` 都没动；结果写明只是挪代码，没跑真书回归、没上真机，待本机回归确认产物逐字节不变。

## 交付物
代码提交到任务分支并推送；「结果」填好；状态改为 done 或 blocked。任务单状态改动推 master，带 `[skip-notify]`。不要合并，不要跑二审。

## 给执行方的指令
```
执行任务单 .dev/tasks/2026-11-13-split-imgopt.md：先把状态改为 accepted 并提交，按「分步任务」做完，填好「结果」，状态改为 done（做不下去改为 blocked 并写明原因），提交并推送分支。任务单 done 推 master 带 [skip-notify]。不要合并，不要跑二审。
```

## 结果
- 执行方 / 开始–结束时间：box-agent（oc-exec） / 2026-11-13T14:43:50+08:00–2026-11-13T16:08:37+08:00
- base..head：`fe2973d6eecfe97c5ac39b46ec27236f14584ee8..fe2973d6eecfe97c5ac39b46ec27236f14584ee8`
- 提交：
  | 提交号 | 说明 |
  |---|---|
  | 5146a9b | chore: 开始执行任务单 2026-11-13-split-imgopt [skip-notify] |
- 改动文件：`.dev/tasks/2026-11-13-split-imgopt.md`
- 测试：包装器未代跑验收命令，由负责 bot 按任务单核对
- 遇到的问题：执行被中断（信号或包装器异常退出）
- 偏离：无
- 新建文件（线索）：无
- 未完成：现场未跑完，详见日志
- 运行记录：`/home/box/oc-runs/2026-11-13-split-imgopt-20261113-144350.json`
- 日志：`/home/box/oc-runs/2026-11-13-split-imgopt-20261113-144350.log`
- 模型：`opencode-go/deepseek-v4.1-flash`

