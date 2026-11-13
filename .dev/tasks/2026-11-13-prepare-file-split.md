# 拆 `prepare_file`，顺手整理重复的读改写

- Task ID: 2026-11-13-prepare-file-split
- 负责 bot: sheng-ren
- 状态: accepted
- 执行方: 待定
- 分支: task/2026-11-13-prepare-file-split（从最新 master 切出）
- 更新时间: 2026-11-13T13:53:54+08:00
- 引用: 2026-11-12-quality-audit 发现 9（主）、发现 8（可选顺手；核实结论是单独开单不值）

## 需求原文
> （质量审查发现 9，已核实 ✅）`library/src/generate.rs` 的 `prepare_file`（master `49139b5` 约 `:764-929`，约 166 行）把认领、挪位置、预登记、比较是不是一样都混在一起。
> （发现 8，核实 ⚠）「读记录—克隆—改—写回」在 `remove_outputs`、`hand_over_outputs`、`retract`、`flush_removed`、`rebase_state_case` 里各写了一遍，但 `put_entry` / `put_state` 已是公共函数，能省的样板不多，单独开单不值；可以顺手做。
> （2026-11-13）quality-audit 收尾拆单。本单是生成和传书的核心路径，必须二审。

## 输入
- 基于 origin/master `49139b5`。
- 审查建议的拆法（可按代码现状微调，行为必须不变）：`resolve_placement`（路径与历史位置）、`try_adopt_or_match_fresh`（最新判定与认领）、`stage_direct_write_registration`（MTP 直写防中断预登记与哈希核对）。
- 发现 8 若顺手做：抽出统一的「读缓存 → 克隆 → 闭包修改 → 校验并原子落盘」，不要为了抽而抽。做或不做写进结果。
- 与 `2026-11-13-big-file-splits` 都可能改 `generate.rs`：本单先做；拆大文件里的 `generate.rs` 等本单合并后再动。

## 约束
- 只用 Rust；clean-room；仓库不出现上游项目名及缩写。
- **纯重构，对外行为与产物 100% 保持一致**：认领、挪位置、预登记、比较是否相同、生成记录格式和内容都不许变。
- **不升版本号**；`fp_golden.txt` 一行不动。若测试表明产物或记录变了则停下报告。
- 不碰 `~/.local/share/booklib`，实验用 `target/` 下临时 `--library`；测试不碰真机。
- `install.sh` 不用 `--force`。本仓库不用 rustfmt 默认宽度，别对整个仓库跑 `cargo fmt`。
- 提交信息中文 + 前缀 + 正文，不加 Co-Authored-By。
- 风险点：生成和传书的核心路径 → 必须 agy 二审。

## 规则（要实现的行为）
1. 把 `prepare_file` 拆成几个单一职责的辅助函数（或等价结构），主函数只串联步骤。拆完后每个子步骤能单独读懂、便于测试。
2. 不改变任何分支条件、错误文案、写盘时机。
3. 发现 8 可选：仅在确实能去掉重复、且不让控制流更难跟的前提下整理读改写样板。

## 分步任务
1. [ ] 阅读 `prepare_file` 全部分支，列出子步骤和调用关系，再动手。
2. [ ] 拆函数，保持行为不变；需要时为各情况补 `flow`（或同级）测试。
3. [ ] （可选）整理重复的读改写；做或不做写进结果。
4. [ ] `cargo test --workspace` 全过；`cargo clippy --workspace --all-targets -- -D warnings` 0 警告；确认 `fp_golden.txt` 和版本号都没动。

## 验收标准
- [ ] `prepare_file` 不再把认领、挪位置、预登记、比较混在同一个过长函数里；行为与改前一致。
- [ ] 已有 `flow` / 生成记录相关测试全部通过；若有新情况已补覆盖。
- [ ] 版本号不变；`fp_golden.txt` 一行没动。
- [ ] `cargo test --workspace` 全过；clippy `-D warnings` 0 警告。
- [ ] 结果里写明没跑真书回归、没上真机，待本机回归确认产物与生成记录逐字节不变。必须 agy 二审。

## 交付物
代码提交到任务分支并推送；「结果」填好；状态改为 done 或 blocked。任务单状态改动推 master，带 `[skip-notify]`。不要合并，不要跑二审。

## 给执行方的指令
```
执行任务单 .dev/tasks/2026-11-13-prepare-file-split.md：先把状态改为 accepted 并提交，按「分步任务」做完，填好「结果」，状态改为 done（做不下去改为 blocked 并写明原因），提交并推送分支。任务单 done 推 master 带 [skip-notify]。不要合并，不要跑二审。
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
