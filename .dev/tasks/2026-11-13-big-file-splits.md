# 拆超过 1800 行的大文件（一次一个）

- Task ID: 2026-11-13-big-file-splits
- 负责 bot: sheng-ren
- 状态: pending
- 执行方: 待定
- 分支: task/2026-11-13-big-file-splits（从最新 master 切出；也可按文件另开子单、另切分支）
- 更新时间: 2026-11-13T09:45+08:00
- 引用: 2026-11-12-quality-audit 发现 14 其余（「核实与分组」建议单 `big-file-splits`；每个文件单独验收）

## 需求原文
> （质量审查发现 14，已核实 ✅）十个核心文件单文件超过 1800 行，难审、难改。若只做模块拆分、不改函数逻辑与公开签名，各设备产物及 `fp_golden` 应逐字节不变。
> （建议）执行时一次只拆一个文件；测试数量和结果不变。`profile/src/lib.rs` 已另开 `2026-11-13-profile-tests-split`，不在本单。
> （2026-11-13）quality-audit 收尾拆单。

## 输入
- 基于 origin/master `49139b5`。行数以该提交为准（`wc -l`）：

| 文件 | 行数 | 审查建议的拆法（可按现状微调） | 依赖 |
|---|---|---|---|
| `crates/bookconv/src/imgopt.rs` | 3916 | 按职责拆子模块（如页面缓冲 / 漫画处理 / 封面缩略图），不要改算法 | 无 |
| `crates/csscascade/src/lib.rs` | 3060 | 简写展开、计算值、选择器匹配等拆子模块 | 无 |
| `crates/bookconv/src/wash/box_fix.rs` | 2009 | 按现有段落拆子模块，行为不变 | 无 |
| `crates/kfx/src/write/parse.rs` | 2002 | 按现有段落拆子模块；**clean-room，只动本仓库代码** | 无 |
| `crates/library/src/generate.rs` | 2003 | 按模块拆文件 | **等 `2026-11-13-prepare-file-split` 合并后再动** |
| `crates/library/src/bin/booklib/main.rs` | 1891 | 各子命令的 `cmd_*` 抽到子模块 | 无 |
| `crates/bookconv/src/htmlproc/footnote.rs` | 1879 | 按现有段落拆子模块 | 无 |
| `crates/bookconv/src/wash/kindle_rules.rs` | 1856 | 按现有段落拆子模块 | 无 |
| `crates/library/src/metadata.rs` | 1848 | 按现有段落拆子模块 | 无 |

- `profile/src/lib.rs`（1832 行）不在本表：先做 `2026-11-13-profile-tests-split`。挪走测试后实现部分预计低于 1800 行，不再为拆而拆。

## 约束
- 只用 Rust；clean-room；仓库不出现上游项目名及缩写。KFX / 清洗相关文件尤其不要去对照任何 GPL 实现。
- **只挪代码、不改行为**：不改公开签名的语义、不改分支条件、不改产物。允许 `mod` 再导出以保持原 `use` 路径，或同步改 crate 内引用，但对外行为必须一致。
- **一次只拆一个文件**，每个文件单独提交（或单独子单）。不要在同一个提交里拆多个文件。
- **不升版本号**；`fp_golden.txt` 一行不动。若对拍或测试表明产物变了则停下报告。
- 不碰 `~/.local/share/booklib`，实验用 `target/` 下临时 `--library`；测试不碰真机。
- 改了可能影响产物的模块时，结果里写明没跑真书回归、待本机回归确认逐字节不变。
- `install.sh` 不用 `--force`。本仓库不用 rustfmt 默认宽度，别对整个仓库跑 `cargo fmt`。
- 提交信息中文 + 前缀 + 正文，不加 Co-Authored-By。
- 若一次只做其中几个文件：做完的勾上，其余留在本单或再开子单；全部完成前不要把本单标成 done。

## 规则（要实现的行为）
1. 每个文件拆完后，原入口仍能编译；测试集合与结果和拆前一致。
2. 不顺手做发现 7 / 9 / 11 的重构（那些已有专门的单）。
3. `generate.rs` 必须在 `prepare-file-split` 合入之后再拆，避免两单抢同一个文件。

## 分步任务
1. [ ] `imgopt.rs`
2. [ ] `csscascade/src/lib.rs`
3. [ ] `wash/box_fix.rs`
4. [ ] `kfx/src/write/parse.rs`
5. [ ] `generate.rs`（`prepare-file-split` 已合并）
6. [ ] `booklib/main.rs`
7. [ ] `htmlproc/footnote.rs`
8. [ ] `wash/kindle_rules.rs`
9. [ ] `metadata.rs`

每个文件：拆分 → 该 crate / workspace 测试通过 → clippy `-D warnings` 0 警告 → 单独提交 → 在结果里记下提交号。

## 验收标准
- [ ] 上表每个文件都已拆成合理模块（或已说明为何某文件不再需要拆，例如拆完相邻单后已低于阈值）。
- [ ] 每个文件对应的测试数量和结果与拆前一致；`cargo test --workspace` 全过；clippy 0 警告。
- [ ] 版本号和 `fp_golden.txt` 都没动。
- [ ] 结果里写明没跑真书回归、没上真机，待本机回归确认产物逐字节不变。

## 交付物
代码提交到任务分支并推送；「结果」填好；状态改为 done 或 blocked。任务单状态改动推 master，带 `[skip-notify]`。不要合并，不要跑二审。

## 给执行方的指令
```
执行任务单 .dev/tasks/2026-11-13-big-file-splits.md：先把状态改为 accepted 并提交，按「分步任务」做完，填好「结果」，状态改为 done（做不下去改为 blocked 并写明原因），提交并推送分支。任务单 done 推 master 带 [skip-notify]。不要合并，不要跑二审。
```

## 结果
<!-- 执行方填写 -->
- 执行方 / 开始–结束时间：
- base..head：
- 提交：| 步骤 | 提交号 | 文件 | 说明 |
- 测试：| 命令 | 通过 / 失败 / 未跑 | 摘要 |
- 遇到的问题：
- 偏离：无
- 未完成：
- 没跑真书回归、没上真机（纯重构，待本机回归确认产物逐字节不变）。

## 审查
<!-- 负责 bot 填写 -->
- 验收：
- 二审：
- 合并：
