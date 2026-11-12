# 二审 .dev/tasks/2026-11-12-lock-hook-kobo-test.md

- 仓库：/home/box/repos/sheng-ren
- 分支：lock-hook-kobo-test（对比 master）
- 时间：2026-11-12T16:14:25+08:00（Asia/Shanghai）
- 模型：gemini-3.8-flash-medium
- 用时：284s
- 退出码：0

## 结论

建议通过。

本分支完整修复了上一轮审查中测试钩子永远无法触发的死代码问题，并补齐了 Kobo（usbms）设备残留清理的自动化集成测试：
1. `fsutil::clean_tmp` 与 `clean_tmp_files` 增加了 `guard` 前置判定闭包，并在 `read_dir` 执行之前最优先调用，真正将 `BOOKLIB_TEST_DEVICE_IO_PANIC` 测试钩子接到实际文件系统 I/O 入口处；
2. `lib.rs` 删除了原来跳过判定后无法到达的死调用，在加锁清理时将设备读检查作为 guard 传入，同时保证设备连接清理（`clean_device_leftovers`）正常执行不带钩子；
3. 新增的 `flow.rs::lock_keeps_kobo_usbms_tmp_until_sync_connects` 用例完整覆盖了 Kobo usbms 挂载目录在加锁命令下不读不删、`sync` 连接后自动清理、产物与状态文件逐字节不变、同一次连接第二轮不再重复清理的完整闭环；
4. 改动范围严格受控，未触及生产环境生成逻辑，未提升任何产物版本号，符合工程约束。

---

## 问题分档

### 阻断

无。

### 建议

- 单元测试未来可考虑进一步强化对「`read_dir` 未发生」的证明：在 [fsutil.rs:271-293](file:///home/box/repos/sheng-ren/crates/library/src/fsutil.rs#L271-L293) 的 `clean_tmp_guard_runs_before_read_dir` 中，断言了 guard 会 panic 且 `tmp.is_file()`。虽然审查代码可知 `guard(dir)` 严格位于函数体第一行（`read_dir` 之前），但单测断言「文件未被删」只能直接证明未执行到删除逻辑。目前该保障已有集成测试与负责 bot 第 6 步自检（注释跳过时立即触发 panic 退出 101）兜底，因此不阻断合并，仅作为后续完善单测防御性的参考。

### 线索

无。

---

## 依据

- [fsutil.rs:187-199](file:///home/box/repos/sheng-ren/crates/library/src/fsutil.rs#L187-L199)、[fsutil.rs:204-213](file:///home/box/repos/sheng-ren/crates/library/src/fsutil.rs#L204-L213)：`clean_tmp` 与 `clean_tmp_files` 函数签名增加 `guard: impl FnOnce(&Path)`，并在第一行调用 `guard(dir)`，确保在执行 `std::fs::read_dir(dir)` 前先完成读检查。
- [lib.rs:511-524](file:///home/box/repos/sheng-ren/crates/library/src/lib.rs#L511-L524)：`clean_tmp_on_computer` 与 `clean_tmp_files_on_computer` 保留了正确的 `cleanup_path_allowed` 过滤，并将 `self.panic_if_test_device_read(dir)` 作为 guard 传给 fsutil，消除了原先跳过之后永远无法走到的直接调用。
- [lib.rs:550-555](file:///home/box/repos/sheng-ren/crates/library/src/lib.rs#L550-L555)：`clean_device_leftovers` 传 `|_| {}`，保证连接清理设备不受测试钩子影响。
- [fsutil.rs:271-293](file:///home/box/repos/sheng-ren/crates/library/src/fsutil.rs#L271-L293)：单元测试直接使用 `catch_unwind` 验证了 `clean_tmp` 和 `clean_tmp_files` 在判定 panic 时能够即刻中断，证明判定接入有效。
- [flow.rs:3149-3230](file:///home/box/repos/sheng-ren/crates/library/tests/flow.rs#L3149-L3230)：新集成测试构造了 `<dev>/usbms/KOBOeReader` 夹具，带 `BOOKLIB_TEST_DEVICE_IO_PANIC=1` 验证加锁命令（`vertical --on`、`add`）下退出 0 且临时文件残留，不带钩子 `sync --device=kobo` 清除临时文件且产物及 `output-state/kobo.json` 逐字节一致，且多轮生成中同一次连接只清一次。
- 提交规范与文件范围：改动仅涉及任务单、`fsutil.rs`、`lib.rs`、`flow.rs`，未修改版本号常量与 `fp_golden.txt`，提交信息符合中文规范，无外部依赖变更。

---

## 置信度

高。

改动点高度集中，函数签名与调用链均已完整静态审查，I/O 顺序明确，测试逻辑严密且与任务单验收标准逐条对应。

---

## 未核实

- 本次为只读审查，未执行 shell 命令，测试套件通过（`cargo test`）、静态检查 0 警告（`cargo clippy`）及格式检查（`cargo fmt`）基于一审负责 bot 的执行回执与 CI 记录。
- 没上真机（本任务改动属于测试钩子与夹具测试，按规范不需要真机）。
