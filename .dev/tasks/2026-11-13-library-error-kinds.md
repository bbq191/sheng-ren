# `library` 错误前缀改成枚举分派

- Task ID: 2026-11-13-library-error-kinds
- 负责 bot: sheng-ren
- 状态: done
- 执行方: box-agent
- 分支: task/2026-11-13-library-error-kinds（从最新 master 切出）
- 更新时间: 2026-11-13T10:43+08:00
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
1. [x] 摸清全部 `starts_with(NO_SPACE|UNMOUNTED|BUSY)` 及构造这些错误的地点。
2. [x] 引入错误种类，改分派与构造；对外字符串不变。
3. [x] 用搜索确认 `library` 里不再用这三条前缀做控制流（测试断言文案可以保留）。
4. [x] `cargo test --workspace` 全过；`cargo clippy --workspace --all-targets -- -D warnings` 0 警告；确认 `fp_golden.txt` 和版本号都没动。

## 验收标准
- [x] 控制流不再依赖这三条错误字符串前缀；给人看的提示逐字不变。
- [x] 版本号不变；`fp_golden.txt` 一行没动。
- [x] `cargo test --workspace` 全过；clippy `-D warnings` 0 警告。
- [x] 结果里写明没跑真书回归、没上真机。

## 交付物
代码提交到任务分支并推送；「结果」填好；状态改为 done 或 blocked。任务单状态改动推 master，带 `[skip-notify]`。不要合并，不要跑二审。

## 给执行方的指令
```
执行任务单 .dev/tasks/2026-11-13-library-error-kinds.md：先把状态改为 accepted 并提交，按「分步任务」做完，填好「结果」，状态改为 done（做不下去改为 blocked 并写明原因），提交并推送分支。任务单 done 推 master 带 [skip-notify]。不要合并，不要跑二审。
```

## 结果
- 执行方 / 开始–结束时间：box-agent（oc-exec） / 2026-11-13T10:26:12+08:00–2026-11-13T10:33:00+08:00；负责 bot 验收后补 `0615f4a`（rustfmt 换行）
- base..head：`eb694dac7beeeb4ad49faaf5feddc4b8f23e1776..8a72b03`（实现至回执；合并见审查栏）
- 提交：
  | 提交号 | 说明 |
  |---|---|
  | 620cf07 | chore: 开始执行任务单 2026-11-13-library-error-kinds [skip-notify] |
  | 529c6cc | refactor(library): 错误按种类分派，不再匹配提示文字前缀 |
  | 13c85f5 | chore: 填写任务单结果（done） [skip-notify] |
  | 0615f4a | chore(library): 按 rustfmt 调整错误种类相关换行（负责 bot 验收时补） |
  | 8a72b03 | docs: library-error-kinds agy 二审回执 [skip-notify] |
- 改动文件：任务单及回执；`crates/library/src/{error.rs,build_run.rs,catalog.rs,deliver.rs,generate.rs,lib.rs,sources.rs,space.rs,transfer.rs}`
- 测试：负责 bot 在独立 worktree 重跑 `cargo test --workspace` 全过；`cargo clippy --workspace --all-targets -- -D warnings` 0 警告；`cargo fmt --all -- --check` 在 `0615f4a` 之后干净。合并后再跑 `cargo test --workspace` 全过。
- 遇到的问题：oc-exec 产出最初 fmt 不干净，已补换行。
- 偏离：无（见审查栏：包装器先标 9 个、修好后剩 catalog.rs / deliver.rs / error.rs，经核对都是本单必然要碰的定义点或构造点，不是真超范围）
- 未完成：无
- 没跑真书回归、没上真机（纯重构，待本机回归确认产物逐字节不变）。
- 运行记录：`/home/box/oc-runs/2026-11-13-library-error-kinds-20261113-102612.json`
- 日志：`/home/box/oc-runs/2026-11-13-library-error-kinds-20261113-102612.log`
- 模型：`opencode-go/deepseek-v4.1-flash`

## 审查
- 验收：**通过**（sheng-ren bot，2026-11-13 10:36 +08:00，审 `origin/master...0615f4a`）。没跑真书回归、没上真机。下面全是 box 上独立 worktree 核对。工具链 rustup stable rustc 1.99.1。按 `Projects/_templates/review-checklist.md`「项目 bot 验收」逐项：结果已填、状态 done；分步任务完成；提交中文前缀加正文、无 Co-Authored-By、执行方未合并。
  - 包装器「偏离」判断（先报 9 个，脚本修好后剩 3 个）。**真实超范围：无，不挡合并。** 九个文件都在 `crates/library` 内，未碰 `bookconv` / `kfx`。对照任务单「分步任务 1」（摸清 starts_with 及构造地点）和「规则 1」（按种类分派）：

    | 文件 | 在范围内？ | 理由 |
    |---|---|---|
    | `build_run.rs` | 是（脚本修好后不再标） | 任务单「输入」里的分派点：`starts_with(NO_SPACE)` / `starts_with(UNMOUNTED)` 改为 `kind()` |
    | `transfer.rs` | 是（脚本修好后不再标） | 任务单「输入」里的分派点：`lane_step` 的 `starts_with(NO_SPACE)` 改为 `Kind::NoSpace`；构造「前面的书已经写满」仍用原格式字符串 |
    | `generate.rs` | 是（脚本修好后不再标） | 任务单「输入」：`UNMOUNTED` 构造改为 `Error::new(Kind::Unmounted, format!("{UNMOUNTED}：…"))`，文案不变；并加 `prepare_typed` / `complete_typed` 把种类传到分派处 |
    | `lib.rs` | 是（脚本修好后不再标） | 声明 `mod error`、`pub use error::Error`；`BUSY` 常量原文 `"文件正在写入"` 未改，只改了注释 |
    | `sources.rs` | 是（脚本修好后不再标） | 任务单「输入」里的分派点：`starts_with(BUSY)` 改为 `kind() == Kind::Busy` |
    | `space.rs` | 是（脚本修好后不再标） | `NO_SPACE = "空间满"` 未改，只改了注释指向 `Kind::NoSpace` |
    | **`error.rs`** | **是（脚本仍标超范围，误报）** | **本单要引入的错误种类定义本身**：新建 `Kind`（`pub(crate)`）和 `Error`（`Display` 输出 `message`）。不是无关改动 |
    | **`catalog.rs`** | **是（脚本仍标超范围，误报）** | **`BUSY` 的构造点**（分步任务 1 要求摸清并改）：`add_file_only` 改为 `Error::new(Kind::Busy, format!("{BUSY}（读的过程中大小或修改时间变了），等写完再试"))`，文案与改前逐字相同。对外 `add_file` 仍 `Result<_, String>`，经 `to_string()` 转出。顺带把同函数里 `NOT_UTF8`、DRM 错误包成 `Error::other`，字符串不变 |
    | **`deliver.rs`** | **是（脚本仍标超范围，误报）** | **`NO_SPACE` 的构造点**（`io_error`：`format!("{}：{what}: {e}", NO_SPACE)` 打上 `Kind::NoSpace`；非满盘仍 `format!("{what}: {e}")`）。写途中掉线的 `gone()` 文案 `"设备断开了（{} 没挂着），没传"` 及「写的途中断开」拼接也未改，只带上 `Kind::Unmounted`。测试同时断言 `kind` 和 `as_str().starts_with(NO_SPACE)` |

  - 对外错误文本：**逐字未变。** 三个常量仍是 `"空间满"`、`"设备中途断开（挂载点不在了）"`、`"文件正在写入"`。所有相关 `format!` 与改前相同；diff 里没有被删改的用户可见字符串（只新增了测试里已有的 `"传到设备"` / `"坏了"` 再断言一遍）。`BuildEvent`、`add_file` / `prepare` / `build` 仍向调用方交出 `error.to_string()` / `Display`。`deliver` 的 `space_tests` 仍断言文案以 `NO_SPACE` 开头。
  - 公开类型有变化（不是文案变化）：`Transfer::run`、`Event::Done`、`Library::complete` 的错误类型从 `String` 改为 `library::Error`，为了把种类传到分派处。`Display` 仍是原来那一行。工作区里只有 `library` 自己和它的测试用这些 API，`bookconv` / `kfx` 未引用。业务分派与改前一致（`complete` 路径仍只对 `NoSpace` 特殊处理；`gone()` 那条文案本来就不以 `UNMOUNTED` 常量开头，以前也不会被当成掉线记入 `dropped`）。
  - 逐条验收标准：
    - [x] `library/src` 里这三条前缀不再用于控制流（仅测试断言文案和注释还提到）；分派改看 `kind()`。
    - [x] 版本号不变；`fp_golden.txt` 一行没动；`git diff origin/master...HEAD -- crates/library/tests/fp_golden.txt docs/development.md` 为空。
    - [x] `cargo test --workspace` 全过：bookbase lib 30，bookconv lib 553，csscascade lib 16，kfx lib 53，library lib 79 / bin 7 / cli 14 / flow 60 / fp_golden 1，profile lib 19。`cargo clippy --workspace --all-targets -- -D warnings` 0 警告。oc-exec 产出最初 fmt 不干净（仅新增换行），负责 bot 提交 `0615f4a` 后检查干净。
    - [x] 新增行无上游项目名。结果栏已补「没跑真书回归、没上真机」。
  - 阻断：无。
- 二审：**无阻断**（agy-review，2026-11-13 10:41 +08:00，模型 gemini-3.8-flash-medium，用时 297s，回执 `8a72b03`）。结论「建议通过」。负责 bot 已读回执并核对引用：建议 1（结果漏免责声明、「偏离」误填、提交列表滞后）在本栏和结果栏补正，不挡合并；建议 2（给 Busy / Unmounted 补单元断言）不挡合并，未在本单加测；线索（`generate.rs` 里 `UNMOUNTED` 注释贴在 `SpaceCheck` 上方）是 master 上原有的错位，本次未改，不在范围。未核实项只有真书回归和真机；测试由负责 bot 已重跑。
- 合并：`89715dc`（2026-11-13 10:43 +08:00，`--no-ff` 合入当时 origin/master `eb694da`）。合并后再跑 `cargo test --workspace` 全过（计数与验收相同）。已推 origin/master。状态保持 **done**（不是 verified），**待 vinci 验收后改 verified**。没跑真书回归、没上真机（纯重构，待本机回归确认产物逐字节不变）。
