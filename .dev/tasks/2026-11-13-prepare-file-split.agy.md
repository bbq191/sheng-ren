# 二审 .dev/tasks/2026-11-13-prepare-file-split.md

- 仓库：/home/box/repos/sheng-ren
- 分支：task/2026-11-13-prepare-file-split（对比 master）
- 时间：2026-11-13T14:09:25+08:00（Asia/Shanghai）
- 模型：gemini-3.8-flash-medium
- 用时：67s
- 退出码：0

## 结论

建议通过。本次改动严格遵循纯重构原则，将 `prepare_file` 中混杂的路径与位置计算（`resolve_placement`）、最新状态检查与历史拷贝认领（`settle_fresh`）、写前预登记与哈希核对（`stage_registration`）拆为单一职责函数；顺手重构的 `edit_state` 完整保留了 5 处调用点的读改写时机与免写条件。补齐的 MTP 直写中断恢复测试用例切实验证了预登记行为，且未影响已有测试。版本号与 golden 产物未动，无阻断性问题。

## 问题分档

### 阻断

- 无

### 建议

- 无

### 线索

- 无

## 依据

- **拆分逻辑与写盘时机保持 100% 一致**：
  - `crates/library/src/generate.rs:807-846`（`resolve_placement`）：提取输出目录、历史条目 `prev`、拷贝识别 `copied` 与预占文件名 `taken`，并在函数退出时及时释放 `state` 的引用绑定，等价于改前的 `drop(state)`，避免写盘前持锁。
  - `crates/library/src/generate.rs:850-891`（`settle_fresh`）：对应原代码前两个早返回分支（已有文件仍最新时触发 `finish` 清理旧残留；指纹未变且检测到手工拷贝时做哈希核对并 `Adopted`），返回值 `Option<Built>` 语义与原 `Step::Done` 分支完全吻合。
  - `crates/library/src/generate.rs:897-949`（`stage_registration`）：MTP 直写及位置变动时的空指纹预登记、哈希比对与 `put_entry` 写盘条件逐行平移自原逻辑，未改动任何判断条件与分支流向。
  - `crates/library/src/generate.rs:952-1040`（`prepare_file`）：仅负责主干调用与组装 `Work`/`transfer`，设备掉线检查文案、临时目录异常清理等均保持原位。
- **读改写模板统一（发现 8）行为等价**：
  - `crates/library/src/generate.rs:1235-1248`（`edit_state`）：基于 `Option` 返回值控制是否写盘。
  - `rebase_state_case`（`:349-353`）、`remove_outputs`（`:456-460`）、`hand_over_outputs`（`:471-480`）、`retract`（`:490-495`）、`flush_removed`（`:526-532`）的无变更早退逻辑（如无条目、目标已存在、未迁移等）均返回 `None` 且不触发写盘；仅在存在实际修改时写盘并返回修改结果，与原代码语义严格一致。
- **测试覆盖与偏离核实**：
  - `crates/library/tests/flow.rs:1568-1606`：新增 `direct_write_stages_record_before_transfer` 测试，覆盖 MTP 直写中断前留空指纹预登记、断点续传及完成后补全指纹的完整生命周期。符合任务单分步任务 2（「需要时为各情况补 flow 测试」），未改动任何既有测试断言。
- **约束核对**：
  - 未改动各 crate 版本号；`fp_golden.txt` 未做修改；提交说明符合规范，无 Co-Authored-By。

## 置信度

高。本次改动为纯函数与结构体内重构，代码逻辑分支逐行对照无遗漏，错误处理与写盘时机完全一致，测试覆盖充分。

## 未核实

- 依据任务单约束与只读环境要求，未在物理真机与全量真书库上运行端到端真机同步回归（已在任务单及验收记录中明确标注「没跑真书回归、没上真机，待本机回归确认产物与生成记录逐字节不变」）。
