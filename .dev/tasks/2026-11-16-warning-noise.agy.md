# 二审 .dev/tasks/2026-11-16-warning-noise.md

- 仓库：/home/box/repos/sheng-ren
- 分支：task/2026-11-16-warning-noise（对比 master）
- 时间：2026-11-16T11:47:43+08:00（Asia/Shanghai）
- 模型：gemini-3.8-flash-medium
- 用时：207s
- 退出码：0

## 结论

建议通过。任务单与实际改动完全对齐需求原文，核心目标（体检、质量门、KFX 警告从按书刷屏改为按轮每类一行汇总，明细记入生成记录由 `booklib list` 查看）完整实现；KEPUB 质量门去重、KFX 警告去重与 20 条截断上限、多设备同书归并、最多列 5 本等边界逻辑均有对应的单元测试和集成测试覆盖；产物生成逻辑与判定未放宽，版本号、指纹与 `fp_golden.txt` 保持不动，clean-room 约束无破损。

## 问题分档

### 阻断

无

### 建议

- [crates/library/src/bin/booklib/main.rs](file:///home/box/repos/sheng-ren/crates/library/src/bin/booklib/main.rs#L1254-L1255) 与 [L1342](file:///home/box/repos/sheng-ren/crates/library/src/bin/booklib/main.rs#L1342)：`audit_lines` 的注释仍写为「`⚠ `（KEPUB CSS 提醒）逐条原文」，以及调用处的「体检（生成时记下的）和 KEPUB CSS 提醒」。本单已将质量门错误与 KFX 警告同样以 `⚠ ` 前缀写入 `audit` 清单，建议后续有顺风车提交时同步注释描述（如「体检与各类提醒（KEPUB CSS、质量门、KFX）」），避免误导代码阅读者。

### 线索

- [crates/library/src/bin/booklib/main.rs](file:///home/box/repos/sheng-ren/crates/library/src/bin/booklib/main.rs#L790-L804)：`WarnedBooks` 按 `title` 归并命中设备与计算本数。因 `BuildEvent::Built` 未携带 `book_id`，若书库中存在完全同名的不同书籍，汇总行中会被合并计数并合为一项。负责 bot 已在任务单审查记录中注明该已知局限，且 `booklib list` 仍按条目独立展示，不影响明细正确性。
- [.dev/tasks/2026-11-16-warning-noise.md](file:///home/box/repos/sheng-ren/.dev/tasks/2026-11-16-warning-noise.md#L60)：分步任务 1 提到「`rg` 补全 KFX 写出器和 `epubbook::load_from` 的全部 warning 写法，写进结果」，执行方在填写的「结果」中未展开列举枚举文本。但实现上在 [produce.rs](file:///home/box/repos/sheng-ren/crates/library/src/generate/produce.rs#L137-L140) 直接接纳了 `epub_to_kfx_from` 返回的警告列表并进行去重上限处理，属于通用接纳无需过滤字符串，负责 bot 亦在审查中核验了三类来源，不影响实际功能。

## 依据

- **需求对照与未放宽判定**：[.dev/tasks/2026-11-16-warning-noise.md](file:///home/box/repos/sheng-ren/.dev/tasks/2026-11-16-warning-noise.md#L11-L16) 要求三类刷屏提醒按轮汇总、KEPUB 质量门与优化产物完全相同的错误去重且不放宽判定。[crates/library/src/generate/produce.rs](file:///home/box/repos/sheng-ren/crates/library/src/generate/produce.rs#L27-L33) 实现 `kepub_gate_audit_lines` 只过滤文本完全一致的条目，且优化产物与 KEPUB 产物两次 `check_epub_file` 均照常执行（[L91](file:///home/box/repos/sheng-ren/crates/library/src/generate/produce.rs#L91)、[L110](file:///home/box/repos/sheng-ren/crates/library/src/generate/produce.rs#L110)）。
- **KFX 截断与去重**：[crates/library/src/generate/produce.rs](file:///home/box/repos/sheng-ren/crates/library/src/generate/produce.rs#L37-L56) `kfx_audit_lines` 按出现顺序去重，最多记录 20 条，超出部分追加「另有 N 条（同类，未逐条记）」，单测在 [L246-L263](file:///home/box/repos/sheng-ren/crates/library/src/generate/produce.rs#L246-L263) 验证完备。
- **汇总统计与输出展示**：[crates/library/src/bin/booklib/main.rs](file:///home/box/repos/sheng-ren/crates/library/src/bin/booklib/main.rs#L762-L829) 提取统一的 `WarningSummary` 与 `WarnedBooks`，在 `built_line` 过滤四大标记，并在一轮末尾输出汇总行；体检不带书名，质量门与 KFX 带有书名与命中的设备列表，超过 5 本标注「等 N 本」。单测在 [main.rs](file:///home/box/repos/sheng-ren/crates/library/src/bin/booklib/main.rs#L2119-L2232) 覆盖了多类汇总、同书多设备归并、上限 5 本等场景。
- **集成测试与文档**：[crates/library/tests/cli.rs](file:///home/box/repos/sheng-ren/crates/library/tests/cli.rs#L1063-L1110) 新增集成测试，验证损坏目录的 EPUB 在 `sync` 下仅有一行质量门汇总且逐本结果不打，在 `list` 下显示「提醒 ⚠ 质量门未过」且 KEPUB 不重复；[docs/usage.md](file:///home/box/repos/sheng-ren/docs/usage.md#L156-L160) 与 [docs/kepub.md](file:///home/box/repos/sheng-ren/docs/kepub.md#L30) 同步了相应行为。

## 置信度

高。仔细核对了全部 commit 与 diff，结合代码实现逐行审阅了边界分支、单测断言和集成测试场景，确认与需求及仓库硬约束一致。

## 未核实

- 受只读二审限制（不执行 shell 命令），未在本地实际运行 `cargo test --workspace` 与 `cargo clippy`，以代码走查及负责 bot 在任务单中核验记录的「851 通过 0 失败、clippy 0 警告」为据。
- 任务单范围内未跑真书回归、未上真机（本改动仅涉及提醒记录与 CLI 输出格式，产物内容与指纹保持逐字节不变）。
