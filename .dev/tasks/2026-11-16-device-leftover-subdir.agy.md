# 二审 .dev/tasks/2026-11-16-device-leftover-subdir.md

- 仓库：/home/box/repos/sheng-ren
- 分支：task/2026-11-16-device-leftover-subdir（对比 master）
- 时间：2026-11-16T14:57:59+08:00（Asia/Shanghai）
- 模型：gemini-3.8-flash-medium
- 用时：177s
- 退出码：0

## 结论

建议通过。任务单与实际代码变更紧扣需求原文，无遗漏需求与超范围改动。方案采用纯路径计算合并计划目录与历史记录，将设备端文件删除收紧为只认本工具特定命名的普通文件并附带死进程与符号链接保护，边界控制严密。测试设计充分覆盖了用户文件保留、符号链接与目录不删、死进程残留清理及计划外目录隔离等关键断言。针对推翻既有决策（修改 2026-11-12 决策中“名字解析不出进程号照删”为“设备上不合格式一律不碰”）已在文档中明确标注并待维护者确认。无阻断问题。

---

## 问题分档

### 阻断

- 无

### 建议

1. **任务单结果表格「新建文件」描述微小出入**：[`.dev/tasks/2026-11-16-device-leftover-subdir.md`](file:///home/box/repos/sheng-ren/.dev/tasks/2026-11-16-device-leftover-subdir.md#L118) 中「新建文件（线索）」填了「无」，但本分支实际新建了集成测试文件 [`crates/library/tests/leftover.rs`](file:///home/box/repos/sheng-ren/crates/library/tests/leftover.rs)（改动文件列表中已包含）。虽不影响代码正确性，建议在任务单归档时保持口径统一。
2. **决策变更待确认**：[`.dev/tasks/2026-11-16-device-leftover-subdir.md`](file:///home/box/repos/sheng-ren/.dev/tasks/2026-11-16-device-leftover-subdir.md#L133) 和 [`docs/decisions.md`](file:///home/box/repos/sheng-ren/docs/decisions.md#L379) 均如实标明本单收紧了设备清理规则，推翻了 2026-11-12 `device-tmp-pid-guard` 中设备上「名字不合规照删」的判定。合并前需由维护者（vinci）做最终确认。

### 线索

1. **PID 复用边界**：[`crates/library/src/fsutil.rs:270`](file:///home/box/repos/sheng-ren/crates/library/src/fsutil.rs#L270) 通过 `/proc/<pid>` 存在性判定进程存活。若写到一半被杀的进程号恰好在下次连接前被系统复用分配给一个无关长生命周期进程，该残留文件会在本轮连接被跳过，需等待该无关进程结束后才能在后续 sync 中清理。这符合“宁可误留、绝不误删”的设计准则，属于预期内的低概率场景。
2. **计划目录过滤与跟踪黑名单**：[`crates/library/src/generate/placement.rs:76`](file:///home/box/repos/sheng-ren/crates/library/src/generate/placement.rs#L76) 计算计划目录时过滤了 `self.wants(m, &device.id)`。若某书已被排除在该设备跟踪范围之外，其目录不会加入清理扫描列表，与 `build_all` 后续传输范围完全一致。

---

## 依据

- **严格模式匹配与只认普通文件**：[`crates/library/src/fsutil.rs:247-276`](file:///home/box/repos/sheng-ren/crates/library/src/fsutil.rs#L247-L276)
  `device_tmp_owner` 纯函数严格检查 `.tmp-` 前缀、两段纯数字与非空文件名，非 UTF-8 或解析溢出均返回 `None`；`clean_device_tmp_files` 使用 `e.file_type().is_ok_and(|t| t.is_file())` 严格排除目录与符号链接，不跟随符号链接，防止误删用户符号链接或其目标文件。
- **扫描目录集合安全计算与过滤**：[`crates/library/src/generate/placement.rs:70-83`](file:///home/box/repos/sheng-ren/crates/library/src/generate/placement.rs#L70-L83) 与 [`crates/library/src/lib.rs:567-582`](file:///home/box/repos/sheng-ren/crates/library/src/lib.rs#L567-L582)
  `planned_output_dirs` 仅做路径推导不访问设备；`clean_device_leftovers` 使用 `BTreeSet` 去重合并记录目录与计划目录，且严格使用 `dir.starts_with(root)` 限制在设备产物根目录下，非目标目录跳过且不存在时不建目录、不报错。
- **连接后仅清理一次且惰性求值**：[`crates/library/src/lib.rs:586-597`](file:///home/box/repos/sheng-ren/crates/library/src/lib.rs#L586-L597) 与 [`crates/library/src/build_run.rs:199-201`](file:///home/box/repos/sheng-ren/crates/library/src/build_run.rs#L199-L201)
  提交 `5378dd5` 将计划目录计算延后至闭包内，在 `begin_device_cleaned` 为 true 时才真正计算，避免 `--watch` 后续轮次无谓的重复计算。
- **用户文件保护与回归测试完整**：[`crates/library/src/fsutil.rs:358-432`](file:///home/box/repos/sheng-ren/crates/library/src/fsutil.rs#L358-L432) 与 [`crates/library/tests/leftover.rs:42-154`](file:///home/box/repos/sheng-ren/crates/library/tests/leftover.rs#L42-L154)
  单测及集成测试逐个覆盖并断言各种变形文件名（中文、无 pid、无 count、畸形段、普通文件）、同名子目录、同名符号链接及其目标内容保持不变，覆盖 Kobo（USB 存储）与掌阅（MTP）两类场景。
- **文档与决策同步**：[`docs/decisions.md:379`](file:///home/box/repos/sheng-ren/docs/decisions.md#L379)、[`docs/usage.md:368`](file:///home/box/repos/sheng-ren/docs/usage.md#L368) 与 [`docs/architecture.md:232`](file:///home/box/repos/sheng-ren/docs/architecture.md#L232)
  文档准确说明了新子目录清理逻辑、严格命名匹配规则以及“中断后立刻 untrack”的已知局限。

---

## 置信度

高。核心逻辑为纯函数过滤与严格文件类型判定，改动路径清晰独立；集成测试在隔离环境下完整断言了误删防护和正常清理双向行为，未修改电脑端加锁清理逻辑与产物构建流水线。

---

## 未核实

- 真机验证与真书回归：根据项目规范，二审环境不具备真实电子书阅读器硬件与真书库，改动未上真机；代码由测试用临时目录配合挂载检查绕过进行模拟验证，符合任务单及 `CONTRIBUTING.md` 约束。
