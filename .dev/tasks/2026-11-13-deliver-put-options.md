# `put_file` 的 bool 参数改成具名选项

- Task ID: 2026-11-13-deliver-put-options
- 负责 bot: sheng-ren
- 状态: pending
- 执行方: 待定
- 分支: task/2026-11-13-deliver-put-options（从最新 master 切出）
- 更新时间: 2026-11-13T09:45+08:00
- 引用: 2026-11-12-quality-audit 发现 7（「核实与分组」建议单 `deliver-put-options`）

## 需求原文
> （质量审查发现 7，已核实 ✅）`library/src/deliver.rs` 里 `put_file` / `put_file_retry` / `put_file_once` 带 `keep_src`、`compare`、`direct` 三个 bool 和一个 `mount`，组合多、容易传错。2026-11-12 汉王任务单的阻断就出在 `direct` 没跟着改。
> （2026-11-13）quality-audit 收尾：把仍存在的发现拆成后续任务单。本单只做参数重构，发现 10（大小写判断注释）已由 `docs-align` `415161a` 做完，不在范围里。

## 输入
- 基于 origin/master `49139b5`。
- 现状（行号以该提交为准）：
  - `crates/library/src/deliver.rs:188` `put_file(src, dest, keep_src, compare, direct, mount)`
  - `:216` `put_file_retry` 同样六个参数；`:368` `put_file_once` 无 `mount`，仍有三个 bool。
  - 库内调用：同文件多处测试，以及 `crates/library/src/transfer.rs:51`。
- 审查建议：改成具名结构体（如 `PutOptions<'a>`）或枚举，避免参数位传错。

## 约束
- 只用 Rust；clean-room；仓库不出现上游项目名及缩写。
- **纯重构，不改变传书行为**：三个 bool 和 `mount` 的语义保持原样（含 MTP 只差大小写时强制经临时文件的逻辑）。
- **不升版本号**；`fp_golden.txt` 一行不动。若测试表明产物或生成记录变了则停下报告。
- 不碰 `~/.local/share/booklib`，实验用 `target/` 下临时 `--library`；测试不碰真机。
- `install.sh` 不用 `--force`。本仓库不用 rustfmt 默认宽度，别对整个仓库跑 `cargo fmt`。
- 提交信息中文 + 前缀 + 正文，不加 Co-Authored-By。

## 规则（要实现的行为）
1. 把 `keep_src`、`compare`、`direct`、`mount` 收进具名选项（结构体或等价写法），`put_file` / `put_file_retry` / `put_file_once` 及全部调用点改用它。对外行为不变。
2. 不顺手改传输逻辑、重试、比较、删除保护。

## 分步任务
1. [ ] 定义选项类型，改三个函数的签名和内部传参。
2. [ ] 改 `deliver.rs` 测试和 `transfer.rs` 调用点。
3. [ ] `cargo test --workspace` 全过（重点 `deliver`、`flow` 的大小写和写满用例）；`cargo clippy --workspace --all-targets -- -D warnings` 0 警告；确认 `fp_golden.txt` 和版本号都没动。

## 验收标准
- [ ] 调用点不再按位置传递那三个 bool；语义与改前一致。
- [ ] 版本号不变；`fp_golden.txt` 一行没动。
- [ ] `cargo test --workspace` 全过；clippy `-D warnings` 0 警告。
- [ ] 结果里写明没跑真书回归、没上真机，待本机回归确认产物逐字节不变。

## 交付物
代码提交到任务分支并推送；「结果」填好；状态改为 done 或 blocked。任务单状态改动推 master，带 `[skip-notify]`。不要合并，不要跑二审。

## 给执行方的指令
```
执行任务单 .dev/tasks/2026-11-13-deliver-put-options.md：先把状态改为 accepted 并提交，按「分步任务」做完，填好「结果」，状态改为 done（做不下去改为 blocked 并写明原因），提交并推送分支。任务单 done 推 master 带 [skip-notify]。不要合并，不要跑二审。
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
