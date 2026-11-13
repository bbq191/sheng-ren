# 二审 .dev/tasks/2026-11-13-vertical-default-off.md

- 仓库：/tmp/sheng-ren-review-vertical-default-off
- 分支：task/2026-11-13-vertical-default-off（对比 master）
- 时间：2026-11-13T11:19:05+08:00（Asia/Shanghai）
- 模型：gemini-3.8-flash-medium
- 用时：215s
- 退出码：0

## 结论

建议通过（无代码阻断）。

上轮二审指出的核心阻断缺陷（`Library::vertical_for` 默认分支漏掉固定版式检查，导致自带竖排样式的固定版式书会被误判为竖排）已在提交 `c3d02c1` 中得到彻底修复：固定版式与漫画检查被前置到判定链最前端短路，任何分支（单书覆盖、目录覆盖、默认原书判定）均不再放行固定版式书，并补全了专用测试用例 `fixed_layout_native_vertical_is_never_set_vertical`。

实现整体严格落实了需求（Kobo 与汉王默认改横排，仅逐书/目录 `--on` 或原书自带竖排才走竖排管线，扩展 OPF `primary-writing-mode` 识别，排除单纯 spine rtl，推翻 11-11 决定并规范同步全套文档、版本号 68→69 及黄金指纹）。因开发环境无真书样本集，按任务单既定约束**不应由 bot 自动合并**，待负责 bot 补齐任务单对 `c3d02c1` 的二次验收记录后，由 vinci 在本机执行真书回归通过后合并。

---

## 问题分档

### 阻断

- 无。

### 建议

- **任务单结果与验收记录滞后于最新提交**：任务单 [.dev/tasks/2026-11-13-vertical-default-off.md](file:///tmp/sheng-ren-review-vertical-default-off/.dev/tasks/2026-11-13-vertical-default-off.md#L66-L181) 尚未根据修复提交 `c3d02c1` 更新。其「验收标准」第 1 项仍附带“（固定版式缺口见审查，尚未修）”；「结果」章节的提交列表和 `base..head` 仍停留在 `caebf7c`；「未完成」仍写着“默认分支漏掉固定版式排除（见审查），未修”；「审查」栏依然是 11:08 的未通过记录。建议负责 bot 重新运行检查，将任务单内容与状态更新至 `c3d02c1` 并将该阻断项核销。
- **严格遵循真书回归未跑不合并约束**：当前测试环境没有真书样本集（`~/Documents/ereader/books/`），未执行 `tools/regress/run.sh` 等真书回归测试。任务单结果中已给出了更正后的 fish 回归命令。必须严格执行任务单约束，分支保持不合并状态，交由 vinci 在本机执行回归通过后再执行 `--no-ff` 合并。
- **`is_natively_vertical` 缺少进程内缓存（保持上轮建议）**：在 Kobo 与汉王设备上，无覆盖配置的书在 `plan` 时会通过 `Library::is_natively_vertical` 读取 EPUB 骨架 entries。虽只读骨架且本单未要求持久化，未来若有性能考量，可考虑在 `session.facts` 中增加基于 SHA 的内存缓存以优化大批量规划耗时。

### 线索

- **原件临时离线容错降级**：`Library::vertical_for` 中 `is_fixed_layout` 与 `is_natively_vertical` 在原件无法读取时均采用了 `.unwrap_or(false)`。对于原书竖排的书籍，若原件临时离线，指纹计算会安全回退为横排（不含 `V`/`f`），待原件恢复后会重新恢复。此设计避免了规划阶段因 I/O 报错 panic，行为合理。

---

## 依据

- [crates/library/src/vertical.rs:149-169](file:///tmp/sheng-ren-review-vertical-default-off/crates/library/src/vertical.rs#L149-L169)：`c3d02c1` 将 `if self.comic_of(meta)? || self.is_fixed_layout(meta).unwrap_or(false) { return Ok(false); }` 统一提升到 `vertical_for` 函数最前，确保单书覆盖、目录覆盖以及默认的 `is_natively_vertical` 分支一律排除固定版式和漫画。
- [crates/library/tests/flow.rs:895-968](file:///tmp/sheng-ren-review-vertical-default-off/crates/library/tests/flow.rs#L895-L968)：新增 `fixed_layout_native_vertical_is_never_set_vertical` 测试，覆盖了“自带竖排样式的固定版式书默认横排”以及“强制 `--on` 也不能竖排”的完整断言。
- [crates/bookconv/src/vertical.rs:170-180](file:///tmp/sheng-ren-review-vertical-default-off/crates/bookconv/src/vertical.rs#L170-L180)：`is_natively_vertical` 增加了对 OPF `primary-writing-mode` 的大小写不敏感解析，且单测验证了 spine rtl 不计入竖排。
- [crates/bookconv/src/optimize/mod.rs:175](file:///tmp/sheng-ren-review-vertical-default-off/crates/bookconv/src/optimize/mod.rs#L175) & [docs/development.md:100](file:///tmp/sheng-ren-review-vertical-default-off/docs/development.md#L100)：`OPTIMIZE_VERSION` 规范升级为 69，附变更说明；`COMIC_VERSION` 保持 68 不变。
- [docs/decisions.md:1995-2000](file:///tmp/sheng-ren-review-vertical-default-off/docs/decisions.md#L1995-L2000)：完整记录推翻 2026-11-11 决定并说明理由，全套文档（`README.md`、`usage.md`、`devices.md`、`architecture.md`、`typesetting.md`）已完整同步。
- [.dev/tasks/2026-11-13-vertical-default-off.md:85-181](file:///tmp/sheng-ren-review-vertical-default-off/.dev/tasks/2026-11-13-vertical-default-off.md#L85-L181)：任务单记录仅记载至 `caebf7c`，未同步包含 `c3d02c1` 的最新状态。

---

## 置信度

高。

对照两次二审 diff，修复代码逻辑严密、针对性单测覆盖到位，与项目排版设计规范完全吻合；文档、版本号与指纹对拍均已核验无误。

---

## 未核实

- 未在只读环境中运行 `cargo test` 与 `cargo clippy`（只读二审约束）。
- 未在真机（Kobo Clara BW、汉王 Clear 6 Turbo）及真书样本集上验证实际渲染效果（需按规范交由 vinci 本机执行回归）。
