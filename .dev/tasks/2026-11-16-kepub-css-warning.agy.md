# 二审 .dev/tasks/2026-11-16-kepub-css-warning.md

- 仓库：/home/box/repos/sheng-ren
- 分支：task/2026-11-16-kepub-css-warning（对比 master）
- 时间：2026-11-16T11:21:42+08:00（Asia/Shanghai）
- 模型：gemini-3.8-flash-medium
- 用时：206s
- 退出码：0

## 结论

建议通过。本任务完整实现了需求方案 1（按轮汇总、逐本下沉至 `booklib list` / `audit` 生成记录）与方案 2（基于版面影响属性及无害值过滤的高精度判定）。代码实现严谨，边界处理与简写展开考虑周全，未引入性能瓶颈与安全风险；KEPUB 产物经负责 bot 验证与改动前 master 保持逐字节一致；版本号、指纹构成及 `fp_golden.txt` 均未变更；单元测试与集成测试充分覆盖了验收标准；文档同步完整。任务单中存在执行方初版 head 与后续验收修复 commit 未在「结果」章节元数据中回填的轻微不一致，不影响功能与合并。

## 问题分档

### 阻断

无

### 建议

- **任务单结果元数据未同步后续修复提交**：`.dev/tasks/2026-11-16-kepub-css-warning.md:115-121` 的「结果」小节中，`base..head` 仍停留在 `cd0dfaf`，提交列表仅包含执行方的 `3df4059` 与 `cd0dfaf`；后续负责 bot 在验收时发现 `background:none` 误报问题并提交了修复 `4e08fab` 以及记录更新 `4ad6145`（已在「审查」小节交代）。建议在合并到 master 前将「结果」小节的 head 及提交表格同步补齐。
- **死属性残留**：`crates/bookconv/src/kepub.rs:442-449` 的 `const PREFIX: [&str; 6]` 中仍包含 `"background"`。由于其上方第 439-441 行已经通过 `p.strip_prefix("background")` 进行了穷举匹配并直接 `return`，下方的 `"background"` 前缀检查为不可达死代码。建议后续重构时清理以避免维护混淆。

### 线索

- **复合选择器规范化显示中的加号空格化**：`crates/bookconv/src/kepub.rs:596-603` 的 `norm_selector` 在规范化复合选择器时使用 `.replace('+', " + ")`，若选择器内包含伪类如 `:nth-child(2n+1)`，会将其格式化为 `:nth-child(2n + 1)`。此现象仅影响 `booklib list` / 单本工具提示信息中的选择器显示文本，不影响任何匹配逻辑或产物。
- **伪元素选择器可能产生弱误报**：`crates/bookconv/src/kepub.rs:570-572` 的 `bare` 判定将冒号开头的后缀均视为 bare 元素（如 `div::before`），若其设置了 margin 但实际没有 `content` 时可能产生多报，属于已知且不阻断的设计取舍。

## 依据

- [crates/bookconv/src/kepub.rs:439-441](file:///home/box/repos/sheng-ren/crates/bookconv/src/kepub.rs#L439-L441)：`layout_prop` 针对 `background` 进行前缀截取并限制仅 `"" | "-color" | "-image"` 视为版面属性，成功解决了 `background:none` 展开后默认补齐 `repeat` / `scroll` 导致误报的问题。
- [crates/bookconv/src/kepub.rs:442-449](file:///home/box/repos/sheng-ren/crates/bookconv/src/kepub.rs#L442-L449)：`PREFIX` 数组中的 `"background"` 在上方提前返回后已无法执行到。
- [crates/bookconv/src/kepub.rs:596-603](file:///home/box/repos/sheng-ren/crates/bookconv/src/kepub.rs#L596-L603)：`norm_selector` 针对选择器字符做简单字符串替换，包含字符 `+` 时的空格化。
- [crates/library/src/bin/booklib/main.rs:720-731](file:///home/box/repos/sheng-ren/crates/library/src/bin/booklib/main.rs#L720-L731)：`built_line` 明确过滤 `RISKY_CSS_WARNING`，确保每本书的结果行下方不再打印该警告。
- [crates/library/src/bin/booklib/main.rs:762-790](file:///home/box/repos/sheng-ren/crates/library/src/bin/booklib/main.rs#L762-L790)：`RiskyCssBooks` 按照 `(device, path)` 对本轮中 `Written` 与 `Same` 状态的书目进行去重与计数，并在一轮结束时输出单行汇总。
- [crates/library/src/bin/booklib/main.rs:1153-1173](file:///home/box/repos/sheng-ren/crates/library/src/bin/booklib/main.rs#L1153-L1173)：`audit_lines` 将 `audit` 中的 `⚠ ` 开头项展示为 `提醒 ⚠ ...`，支持 `booklib list` 下钻查看。
- [.dev/tasks/2026-11-16-kepub-css-warning.md:115-121](file:///home/box/repos/sheng-ren/.dev/tasks/2026-11-16-kepub-css-warning.md#L115-L121)：任务单「结果」小节中提交记录停留在 `cd0dfaf`，未包含验收修复 commit `4e08fab` 与记录提交 `4ad6145`。

## 置信度

高。已通读全部变更代码、单测及集成测试用例，并交叉比对 `csscascade` 的简写展开机制（`background`、`border`、`margin`）与 `produce` / `main.rs` 流程中的事件传递与状态记录机制，逻辑严密完备。

## 未核实

- 未使用真书样本库执行回归测试（符合 `CONTRIBUTING.md` 约束：真书样本仅存在于维护者本机）。
- 未在真实 Kobo 设备硬件上挂载验证（符合 `CONTRIBUTING.md` 约束：测试不碰真设备，真机验证由 vinci 最终验收执行）。
