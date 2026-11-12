# 二审 .dev/tasks/2026-11-12-device-tmp-pid-guard.md

- 仓库：/home/box/repos/sheng-ren
- 分支：device-tmp-pid-guard（对比 master）
- 时间：2026-11-12T16:49:47+08:00（Asia/Shanghai）
- 模型：gemini-3.8-flash-medium
- 用时：145s
- 退出码：0

## 结论

建议通过。本次改动范围清晰、实现克制保守，完整实现了连接清理跳过存活进程临时文件的需求；加锁清理不做进程号保护的决定符合当前架构与多书库隔离设计；未改动任何产物生成与传输逻辑，无阻断性问题。

## 问题分档

### 阻断

- 无

### 建议

- 无（负责 bot 一审中关于容器环境/不同 PID 命名空间下 `/proc` 隔离局限的建议已足够充分）。

### 线索

- 无

## 依据

- **需求对照与约束遵从**：
  - 任务单紧扣 `2026-11-12-lock-no-device` 二审建议 3（多书库写同一设备冲突问题）。
  - `clean_tmp_files` 新增 `skip_live_owner: bool` 参数（[crates/library/src/fsutil.rs:221](file:///home/box/repos/sheng-ren/crates/library/src/fsutil.rs#L221)），加锁清理传 `false`、设备连接清理传 `true`（[crates/library/src/lib.rs:523](file:///home/box/repos/sheng-ren/crates/library/src/lib.rs#L523)、[crates/library/src/lib.rs:553](file:///home/box/repos/sheng-ren/crates/library/src/lib.rs#L553)），完全符合「加锁清理保持原样、连接清理按进程号保护」的定则。
  - 未动产物生成、流水线、版本号，不影响真实书籍产物。
- **解析与存活判定实现**：
  - `tmp_owner_pid`（[crates/library/src/fsutil.rs:202-209](file:///home/box/repos/sheng-ren/crates/library/src/fsutil.rs#L202-L209)）准确剥离前缀 `.tmp-` 并截取第一段，严格校验非空、纯 ASCII 数字并解析为 `u32`，任何畸形格式均回退为 `None`。
  - `tmp_owner_alive`（[crates/library/src/fsutil.rs:212-214](file:///home/box/repos/sheng-ren/crates/library/src/fsutil.rs#L212-L214)）利用 `/proc/<pid>` 的存在性检查，进程号转字符串安全且无注入风险；非 Linux 环境或未挂载 `/proc` 时优雅降级为 `false`（照旧清理），方向保守安全。
- **改动范围与测试**：
  - 改动严格限制在 `fsutil.rs`、`lib.rs`、`tests/flow.rs`、`decisions.md` 及任务单文件，无超范围修改。
  - `tests/flow.rs` 将夹具的假进程号调整为 `u32::MAX`（[crates/library/tests/flow.rs:3166](file:///home/box/repos/sheng-ren/crates/library/tests/flow.rs#L3166)、[crates/library/tests/flow.rs:3283](file:///home/box/repos/sheng-ren/crates/library/tests/flow.rs#L3283)），避免了在 Linux 下 PID 1/2（init/kthreadd 存活）导致断言失败，该修改已在任务单「偏离」中如实申报并说明原因。
  - `fsutil.rs` 新增了包含存活进程、死亡进程、格式畸形、缺失 PID 等场景的单元测试（[crates/library/src/fsutil.rs:317-337](file:///home/box/repos/sheng-ren/crates/library/src/fsutil.rs#L317-L337)），逻辑覆盖完整。

## 置信度

高。核心逻辑改动集中且简明，代码与测试均已逐行核验，安全边界与边界条件分析明确。

## 未核实

- 本次为只读二审，未运行 `cargo test` / `cargo clippy` 等命令（依据负责 bot 第一轮审查给出的测试执行结果）。
- 未上真机验证（符合任务单「测试用临时目录当设备，不碰真机」约束）。
