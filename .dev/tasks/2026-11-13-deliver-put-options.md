# `put_file` 的 bool 参数改成具名选项

- Task ID: 2026-11-13-deliver-put-options
- 负责 bot: sheng-ren
- 状态: done
- 执行方: box-agent
- 分支: task/2026-11-13-deliver-put-options（从最新 master 切出）
- 更新时间: 2026-11-13T10:25+08:00
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
1. [x] 定义选项类型，改三个函数的签名和内部传参。
2. [x] 改 `deliver.rs` 测试和 `transfer.rs` 调用点。
3. [x] `cargo test --workspace` 全过（重点 `deliver`、`flow` 的大小写和写满用例）；`cargo clippy --workspace --all-targets -- -D warnings` 0 警告；确认 `fp_golden.txt` 和版本号都没动。

## 验收标准
- [x] 调用点不再按位置传递那三个 bool；语义与改前一致。
- [x] 版本号不变；`fp_golden.txt` 一行没动。
- [x] `cargo test --workspace` 全过；clippy `-D warnings` 0 警告。
- [x] 结果里写明没跑真书回归、没上真机，待本机回归确认产物逐字节不变。

## 交付物
代码提交到任务分支并推送；「结果」填好；状态改为 done 或 blocked。任务单状态改动推 master，带 `[skip-notify]`。不要合并，不要跑二审。

## 给执行方的指令
```
执行任务单 .dev/tasks/2026-11-13-deliver-put-options.md：先把状态改为 accepted 并提交，按「分步任务」做完，填好「结果」，状态改为 done（做不下去改为 blocked 并写明原因），提交并推送分支。任务单 done 推 master 带 [skip-notify]。不要合并，不要跑二审。
```

## 结果
- 执行方 / 开始–结束时间：box-agent（oc-exec） / 2026-11-13T10:13:16+08:00–2026-11-13T10:16:00+08:00；负责 bot 验收后补 `ded0ccc`（rustfmt 换行）、`9a1e3d8`（keep_src 注释）
- base..head：`cc7a372000e7af6c1ee76391bce97f1e2dbc2d61..4c07c09357adef26c5f971dab48459c321da1922`（实现至回执；合并见审查栏）
- 提交：
  | 提交号 | 说明 |
  |---|---|
  | 9a52292 | chore: 开始执行任务单 2026-11-13-deliver-put-options [skip-notify] |
  | 4dce406 | chore(library): put_file 的 bool 参数改成具名选项 |
  | e2f6a07 | chore: 填写任务单结果（done） [skip-notify] |
  | ded0ccc | chore(library): 按 rustfmt 调整 put_file 调用的换行（负责 bot 验收时补） |
  | 9a1e3d8 | docs(library): 修正 PutOptions.keep_src 注释反义（二审建议，合并前改） |
  | 4c07c09 | docs: deliver-put-options agy 二审回执 [skip-notify] |
- 改动文件：`.dev/tasks/2026-11-13-deliver-put-options.md`、`.dev/tasks/2026-11-13-deliver-put-options.agy.md`、`crates/library/src/deliver.rs`、`crates/library/src/transfer.rs`
- 测试：负责 bot 在独立 worktree 重跑 `cargo test --workspace` 全过；`cargo clippy --workspace --all-targets -- -D warnings` 0 警告；`cargo fmt --all -- --check` 在 `ded0ccc` 之后干净。合并后再跑 `cargo test --workspace` 全过。
- 遇到的问题：oc-exec 产出最初 fmt 不干净，已补换行。
- 偏离：无（oc-exec 把任务范围内的 `deliver.rs` / `transfer.rs` 误标为偏离，实际就是本单要改的文件）
- 未完成：无
- 没跑真书回归、没上真机（纯重构，待本机回归确认产物逐字节不变）。
- 运行记录：`/home/box/oc-runs/2026-11-13-deliver-put-options-20261113-101316.json`
- 日志：`/home/box/oc-runs/2026-11-13-deliver-put-options-20261113-101316.log`
- 模型：`opencode-go/deepseek-v4.1-flash`

## 审查
- 验收：**通过**（sheng-ren bot，2026-11-13 10:19 +08:00，审 `origin/master...ded0ccc`，后补注释修正 `9a1e3d8`）。没跑真书回归、没上真机。下面全是 box 上独立 worktree 核对。工具链 rustup stable rustc 1.99.1。按 `Projects/_templates/review-checklist.md`「项目 bot 验收」逐项：结果已填、状态 done；分步任务完成；提交中文前缀加正文、无 Co-Authored-By、执行方未合并。
  - 偏离两个文件：`crates/library/src/deliver.rs`、`crates/library/src/transfer.rs`。这正是任务单「输入」和「分步任务」指定要改的文件。oc-exec 把它们标成偏离，是包装器对 scope_files 的提取问题，不是超范围。真实偏离：无。
  - 逐条验收标准：
    - [x] 调用点不再按位置传三个 bool：`put_file` / `put_file_retry` / `put_file_once` 改为 `PutOptions`；全部 12 处测试调用和 `transfer.rs` 的 bool 值与改前 1:1 对应。内部覆盖：`case_twin` 传 `keep_src: false, ..opts`；`case_alias` 传 `keep_src: true, direct: false, ..opts`（强制不走直接写，与改前一致）。传输逻辑、重试、比较、删除保护未改。
    - [x] 版本号不变；`fp_golden.txt` 一行没动；`git diff origin/master...HEAD -- crates/library/tests/fp_golden.txt docs/development.md` 为空。
    - [x] `cargo test --workspace` 全过：bookbase lib 30，bookconv lib 553，csscascade lib 16，kfx lib 53，library lib 79 / bin 7 / cli 14 / flow 60 / fp_golden 1，profile lib 19。`cargo clippy --workspace --all-targets -- -D warnings` 0 警告。oc-exec 产出最初 `cargo fmt --all -- --check` 不干净（仅新增签名和测试调用换行），负责 bot 调整后提交 `ded0ccc`，之后检查干净。
    - [x] 新增行无上游项目名。结果栏已补「没跑真书回归、没上真机」。
  - 阻断：无。合并前按二审建议修正了 `PutOptions.keep_src` 注释反义（`9a1e3d8`）：原写「放好之后删掉来源」，实际 `true` 是保留来源。
- 二审：**无阻断**（agy-review，2026-11-13 10:22 +08:00，模型 gemini-3.8-flash-medium，用时 147s，回执 `4c07c09`）。结论「建议通过」。负责 bot 已读回执并核对引用：建议 1（keep_src 注释）已核实并修正；建议 2–4（结果漏免责声明、「偏离」误填、提交列表滞后和复选框）在本栏和结果栏补正，不挡合并；线索（负责 bot 重跑测试）已在验收中完成。未核实项只有真书回归和真机。
- 合并：`ab68bda`（2026-11-13 10:24 +08:00，`--no-ff` 合入当时 origin/master `cc7a372`）。合并后再跑 `cargo test --workspace` 全过（计数与验收相同）。已推 origin/master。状态保持 **done**（不是 verified），**待 vinci 验收后改 verified**。没跑真书回归、没上真机（纯重构，待本机回归确认产物逐字节不变）。
