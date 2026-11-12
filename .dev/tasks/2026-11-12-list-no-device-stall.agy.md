# 二审 .dev/tasks/2026-11-12-list-no-device-stall.md

- 仓库：/home/box/repos/sheng-ren
- 分支：list-no-device-stall（对比 master）
- 时间：2026-11-12T16:38:14+08:00（Asia/Shanghai）
- 模型：gemini-3.8-flash-medium
- 用时：90s
- 退出码：0

## 结论

建议通过。本轮改动严格落实了方案 B（可拔下设备上的产物不碰文件系统、不调用 `mounted()`，仅比对指纹并附加「未核对设备」标记），范围收敛准确，无超范围修改；测试设计了防御性 panic 钩子验证未触碰设备路径，覆盖了指纹匹配、指纹变更/为空、PC 本地模式对比及相对路径旧记录等核心分支；文档与决策记录同步完整，无阻断合并的问题。

---

## 问题分档

### 阻断

无

### 建议

无

### 线索

1. **设备在线感知语义改变**：在方案 B 落地后，设备拔掉且仅残留生成记录时，`list` 会依然显示 `✓ 最新 未核对设备`（不再显示 `? 未知`），这符合设计预期与决策记录，CLI 帮助与用户文档也已明确说明；后续若用户习惯用 `list` 探查设备是否已挂载，需注意此语义已转移至 `sync`。

---

## 依据

1. **需求原文与任务单对照**：
   - 需求指定采用「方案 B」（[2026-11-12-list-no-device-stall.md](file:///home/box/repos/sheng-ren/.dev/tasks/2026-11-12-list-no-device-stall.md#L11-L15)）；
   - 任务单正确划定了「不做」范围（不改动 `sync` 连接逻辑、不改加锁清理、不引入挂载表扫描或方案 C 线程超时），与 `master` 对比，`crates/library/src/deliver.rs`、`crates/library/src/session.rs` 和 `crates/bookconv` 均无任何 diff。
2. **逻辑实现与边界条件**：
   - [crates/library/src/generate.rs:519-544](file:///home/box/repos/sheng-ren/crates/library/src/generate.rs#L519-L544)：对可拔下设备先经 `self.devices.on_removable(&entry.path)` 做纯路径前缀匹配，完全绕过了 `output_unplugged` 和 `output_is_file`；指纹比对覆盖了当前指纹及 `legacy` 指纹，并在 profile 停用时通过 `.or_else(|| profile::get(&dev_id))` 进行内置参数回退计算；相对路径继续返回 `None`；非可拔设备（电脑自定义模式）仍保持文件核对逻辑。
   - [crates/library/src/bin/booklib/main.rs:1051-1057](file:///home/box/repos/sheng-ren/crates/library/src/bin/booklib/main.rs#L1051-L1057)：`cmd_list` 依据 `(o.fresh, o.device_unchecked)` 组合精确匹配文案，确保 `(None, _)` 输出 `? 未知`，仅在设备路径且有结论时附加「未核对设备」。
3. **测试充分性**：
   - [crates/library/tests/flow.rs:137-231](file:///home/box/repos/sheng-ren/crates/library/tests/flow.rs#L137-L231)：新增 `list_does_not_stat_removable_device_outputs`，通过 `BOOKLIB_TEST_DEVICE_STAT_PANIC=1` 环境变量配合 [crates/library/src/generate.rs:558-567](file:///home/box/repos/sheng-ren/crates/library/src/generate.rs#L558-L567) 守卫，一旦在设备路径调用 stat/is_file 即触发 panic；完整断言了设备最新/过期、PC 最新/过期、相对路径未知的文案表现。
   - 历史用例断言调整合理：`add_build_skip_remove`（[flow.rs:92-113](file:///home/box/repos/sheng-ren/crates/library/tests/flow.rs#L92-L113)）与 `disabled_profile_is_forgotten_by_sync`（[flow.rs:2843-2927](file:///home/box/repos/sheng-ren/crates/library/tests/flow.rs#L2843-L2927)）均对方案 B 下的新行为作了精准修正，没有盲目删除已有断言。
4. **范围与版本控制**：
   - 变动仅限任务单涉及的 6 个文件；
   - 版本号常量与 `fp_golden.txt` 未作任何改动；
   - 决策记录（[docs/decisions.md:362](file:///home/box/repos/sheng-ren/docs/decisions.md#L362)）及用法文档（[docs/usage.md:58](file:///home/box/repos/sheng-ren/docs/usage.md#L58)、[docs/usage.md:180-195](file:///home/box/repos/sheng-ren/docs/usage.md#L180-L195)）同步更新到位。

---

## 置信度

高。代码改动集中清晰，涉及的边界情况（相对路径、内置模式回退、未核对标记）与测试用例、文档均完全闭环一致。

---

## 未核实

1. **真实卡死挂载点的真机表现**：本审查在只读环境下进行，且自动化测试依赖环境变量 guard 与本地临时目录模拟，未在真实 FUSE（jmtpfs）卡死的物理设备/真机环境下做挂载即时返回验证（按任务单约定，该项由 vinci 后续进行真机验收）。
