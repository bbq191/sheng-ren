# 二审 .dev/tasks/2026-11-16-sync-mount-stall.md

- 仓库：/home/box/repos/sheng-ren
- 分支：task/2026-11-16-sync-mount-stall（对比 master）
- 时间：2026-11-16T14:33:35+08:00（Asia/Shanghai）
- 模型：gemini-3.8-flash-medium
- 用时：296s
- 退出码：0

## 结论

建议通过。

本次改动完整覆盖了需求原文中“设备挂载卡住时 sync 一直挂着不动”的问题。实现上采用方案 C（连接探测时限 $T$ + 传输看门狗时限 $2T$ 及大文件长调用放宽 + 主线程兜底看门狗 $2T$ 配合 `stall::beat()`），并在负责 bot 验收中通过真实卡死的 FUSE 小程序发现了内核不可中断等待（`request_wait_answer`）导致主线程退出后进程变僵尸、书库锁（flock）滞留的底层问题，及时补充了 `supervise` 前台工作进程代理与 `exit_process` 显式释放锁并传递退出码的完整闭环。代码设计严谨，未传完记录指纹置空与临时文件机制确保不留半写文件，产物与指纹逐字节保持不变，单元测试与集成测试覆盖完备。发现的局部文档不一致与偏离漏报均不阻断合并。

---

## 问题分档

### 阻断

无。

### 建议

1. **`stall.rs` 模块级注释与实测结论矛盾**：`crates/library/src/stall.rs:15-16` 的模块注释仍保留了执行方早期的未证实假设（*“FUSE 请求的等待可以被致命信号打断……所以 MTP 上放弃的线程不挡进程退出”*），与负责 bot 在验收复现中实测发现内核不可中断等待、改由工作进程隔离的结论（见 `crates/library/src/stall.rs:250-255`、`main.rs:1085` 及 `docs/decisions.md:1948`）直接冲突。建议在合并前或后续维护中同步修正该处注释。
2. **任务单「偏离」漏报 `fsutil.rs`**：任务单 `.dev/tasks/2026-11-16-sync-mount-stall.md:130` 的「结果 → 偏离」仅记录了 `crates/library/src/generate.rs`，漏报了 `crates/library/src/fsutil.rs`（该文件改动了 `lock` 登记、`Lock::drop` 注销锁描述符 `note_lock_fd`，以及在 `sha256_file` 和 `clean_tmp_files` 中添加 `stall::beat()`）。建议在任务单中补全偏离记录。

### 线索

1. **传输管道缓冲任务在卡死恢复后的处理时序**：`crates/library/src/transfer.rs:1617` 在 `stalled_lane` 触发放弃时关掉并丢弃了发送端 `tx`，但 `file_lane` 的 `rx` 队列中若在卡死前已积压了后续任务（通道深度 `LANE_DEPTH=2`），当卡死的第一件请求最终从内核返回时，工作线程仍会从 `rx` 读出第二件并调用 `t.run()`。虽然由于 `Pipeline::next` 检查了 `abandoned` 集合，该件交回的结果会被安全丢弃且不修改生成记录，同时 `StallGuard` 也会阻止该设备在线程未返回前被下一轮 sync 碰触，但负责人可留意该边缘时序在设备突然复活时是否存在多余的设备端写 I/O。

---

## 依据

- `crates/library/src/stall.rs:15-16`：模块注释描述为“FUSE 请求的等待可以被致命信号打断……不挡进程退出”，与 `crates/library/src/stall.rs:250-255`、`crates/library/src/bin/booklib/main.rs:1085-1087`、`docs/decisions.md:1948` 及 `.dev/tasks/2026-11-16-sync-mount-stall.md:143` 记载的实测结论相反。
- `crates/library/src/fsutil.rs:599,607,617,626`：修改了 `sha256_file`、`Lock::drop`、`lock` 及 `clean_tmp_files`，该文件不在任务单第 110 行「涉及文件（预计）」中，第 130 行「结果 → 偏离」亦未登记。
- `crates/library/src/transfer.rs:1617-1621,1647-1663`：`stalled_lane` 移除了 `self.lanes` 并丢弃 `tx`，但 `file_lane` 的接收端 `rx` 仍会把已缓冲的任务执行完才退出。

---

## 置信度

高。

已完整比对任务需求原文、方案决策记录、全部 16 处改动文件的代码实现逻辑、`supervise` 工作进程代理机制（`pipe2`、`pre_exec`、信号与 I/O 刷新时序）、并发看门狗状态机，以及新增的 4 项集成测试和 4 项单元测试，并在只读环境下完成了推演核验。

---

## 未核实

- 真实硬件设备（汉王/掌阅）因拔出或硬件卡死导致真实 jmtpfs 进程异常断开时的内核态反应，未在物理真机上验证（负责 bot 已使用 scratch FUSE 小程序进行了卡死模拟，真机验收留由维护者最终确认）。
