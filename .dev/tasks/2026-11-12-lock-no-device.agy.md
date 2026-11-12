## 结论

**无阻断**。加锁路径（`clean_leftovers`）彻底剥离了设备路径的 I/O 操作；`sync` 连上后按设备清理且单连接只清一次的机制正确生效；传输线程生命周期受 `build_all` 内局部 `Pipeline` 的 `join` 保护，本进程内绝无抢占 `.tmp-*` 的可能。唯一较明显缺陷是测试钩子存在控制流死代码，但不影响代码功能本身。

---

## 问题分档

### 阻断
无。

### 建议
1. **测试钩子为死代码，未真正证明“加锁不读设备”**
   - **依据**：`crates/library/src/lib.rs:178-183`、`lib.rs:187-192`、`lib.rs:201-210`
   - **分析**：`clean_tmp_on_computer` 和 `clean_tmp_files_on_computer` 均在第一行执行 `if !self.cleanup_path_allowed(dir) { return; }`。对于设备路径，`cleanup_path_allowed` 返回 `false` 并直接 `return`，控制流根本走不到下一行的 `panic_if_test_device_read(dir)`；而对于电脑本地路径，`self.devices.on_removable(dir)` 必为 `false`，进入该函数后 `if self.devices.on_removable(dir)` 也不会命中。因此该钩子在控制流上是绝对不可达的死代码，`flow.rs:3055` 能够退出 0 属于同义反复，自动化测试本身未能真正证明“加锁时未发生 read_dir/stat”（该断言仅依赖负责 bot 补充的 FUSE 卡死复现）。建议将钩子逻辑移至跳过之前，或直接下沉至 `fsutil::clean_tmp_files`。
2. **缺少 USBMS（如 Kobo）的自动化测试**
   - **依据**：`crates/library/tests/flow.rs:489-626`
   - **分析**：新加的集成测试用例只覆盖了 MTP 设备（`ireader`），而实际默认走临时文件机制（`direct_write = false`）落盘的核心设备是 Kobo。虽然负责 bot 手工执行过 CLI 验证，但自动化测试集中仍缺少此路径的覆盖。
3. **多书库并发写入同一设备时可能误删**
   - **依据**：`crates/library/src/fsutil.rs:201-209`、`crates/library/src/lib.rs:219-221`
   - **分析**：`clean_tmp_files` 遍历目标目录下所有 `.tmp-*` 并直接 `remove_file`。当两套独立书库（各有独立锁）同时向同一台设备 sync 时，后连上的一方在执行 `clean_device_leftovers` 时会删掉先连上一方正在写入的临时文件。虽然 master 既有逻辑在每次加锁时同样会删，本分支将其收敛到连接时已缩短了冲突窗口，但尚未做到基于活跃进程 PID 的保护。

### 线索
1. **未记录到 state 的新子目录中的残留无法被清理**
   - **依据**：`crates/library/src/generate.rs:338-353`、`crates/library/src/lib.rs:218`
   - **分析**：`clean_device_leftovers` 依赖 `output_dirs()`，而后者只遍历已写入生成记录（state）的目录。如果首次向某子目录写书且在生成记录产生前进程被杀，该目录不在 `output_dirs()` 中，其残留 `.tmp-*` 不会被清理。此为 master 既有行为，非本分支引入。
2. **工作区外部并发切换分支**
   - **依据**：`.git/logs/HEAD:517-518`
   - **分析**：在 11:08:24 工作区被外部操作切至 `docs-align` 分支，本次二审通过 Git 对象库直接比对 `lock-no-device`（`848967c..f24365f`，实现位于 `d8a7567`）的完整提交历史与 diff，未改动工作区任何文件。

---

## 重点问题逐条核对

1. **清理时机与并发争用**
   - **时机与单连接一次**：在 `crates/library/src/build_run.rs:188-190`，设备检测为 `live` 后、传输线程 `Pipeline::default()`（`build_run.rs:290`）启动前触发；每次连接由 `session.rs:341` `begin_device_cleaned` 保证只清一次；设备掉线时 `build_run.rs:118` 调 `forget_device_cleaned` 移除标记，重连后可再次触发。
   - **--watch 掉线重连与线程争用**：传输管道 `Pipeline` 作用域为 `build_all` 局部变量（`build_run.rs:290`），并在函数结束前完全回收（`build_run.rs:469-471` `pipe.next(true)`），析构时强制 `join` 全部子线程（`crates/library/src/transfer.rs:184-191`）。因此下一轮（含掉线重连轮）进入清理时，上一轮传输线程早已完全终止，**本进程内绝不可能与清理抢占同一个 `.tmp-*`**。
   - **多书库并发**：见【建议 2】，属既有局限，非新增问题。
2. **session.rs 路径判定与 master unplugged 的等价性**
   - **依据**：`crates/library/src/session.rs:285-338` 对照 master `session.rs:158-186`
   - **结论**：**完全等价，无漏认**。`removable_base` 复用了相同的挂载点向量构造规则（包含 `mtp_base`、`usbms_base` 以及 `mount_check` 时 `/run/media` 与 `/media` 的互认），严格按路径长度降序排序（保证 `<mtp_base>/usbms` 优先匹配），采用标准库 `Path::starts_with` 路径分量匹配，行为与原 `unplugged` 一致。
3. **lib.rs 加锁路径设备 I/O 核查**
   - **依据**：`crates/library/src/lib.rs:462-502`
   - **结论**：**无任何设备 I/O**。`lock()` 仅操作书库根与本地 `output-state`；`clean_leftovers` 开头即判断 `self.cleanup_path_allowed(&self.root)`；在遍历 `output_dirs()` 时，`clean_tmp_files_on_computer` 在第一步直接通过 `self.cleanup_path_allowed`（纯路径前缀内存比较，不调用任何底层系统调用）将落在可拔下设备上的目录跳过，未执行任何 `read_dir` 或 `stat`。
4. **测试钩子有效性**
   - **依据**：`crates/library/src/lib.rs:181`、`lib.rs:190`、`lib.rs:201-210`
   - **结论**：**无法证明验收标准**。控制流逻辑冲突导致该钩子为死代码（详见【建议 1】）。

---

## 置信度
**高**。通读了 `848967c..f24365f` 完整 diff，并逐行核验了 `session.rs`、`lib.rs`、`build_run.rs`、`transfer.rs`、`fsutil.rs` 的跨模块调用与并发控制流。

---

## 未核实
1. **真机挂载验证**：未在真实汉王（MTP/jmtpfs 卡死现场）或真实 Kobo 真机上跑过完整命令。
2. **真实书库全量回归**：未对真实生产电子书库执行带真实设备的 `sync` 全量回归。
