# 二审 .dev/tasks/2026-11-13-library-error-kinds.md

- 仓库：/tmp/sheng-ren-review-library-error-kinds
- 分支：task/2026-11-13-library-error-kinds（对比 master）
- 时间：2026-11-13T10:41:30+08:00（Asia/Shanghai）
- 模型：gemini-3.8-flash-medium
- 用时：297s
- 退出码：0

## 结论

建议通过。代码改动精准、严密，将 `NO_SPACE`、`UNMOUNTED`、`BUSY` 三处字符串前缀控制流完整替换为内部错误种类枚举 `Kind`（`Kind::NoSpace` / `Kind::Unmounted` / `Kind::Busy`），对外 `Display` 输出文本保持逐字一致；`Kind` 严格封装在 `crates/library` 内部，未超范围污染依赖 crate；未改动版本号及黄金指纹；测试中对错误种类与外露文本均有正确断言。仅任务单的「结果」填写存在格式与说明遗漏，由负责 bot 在 master 合并前直接补齐即可，不阻断代码合并。

---

## 问题分档

### 阻断

无。

### 建议

1. **补齐任务单中的免责声明与纠正偏离栏**：
   - 任务单验收标准第 4 条明确要求「结果里写明没跑真书回归、没上真机」，原模板中带有的 `- 没跑真书回归、没上真机（纯重构，待本机回归确认产物逐字节不变）。` 在填写结果时被覆盖删除，导致该验收项未在结果中体现。
   - 「偏离」栏被误填为修改的代码文件列表，实际实现并无需求偏离，应写「无」。
   - 提交表格仅列至 `529c6cc`，分支后续的 `13c85f5`（填写结果）与 `0615f4a`（rustfmt 换行调整）未补进表格。建议负责 bot 合并前在任务单中一并修正。
2. **未来可考虑补充 `Kind::Busy` 与 `Kind::Unmounted` 的单元断言**：
   - 本次改动在 `deliver.rs` 与 `transfer.rs` 中为 `Kind::NoSpace` 补充了严格的种类与前缀断言；而 `Kind::Busy` 与 `Kind::Unmounted` 目前主要通过既有流程保证，未来可在对应模块单元测试中补充对种类的直接测试用例。

### 线索

1. **历史遗留注释错位**：
   - `crates/library/src/generate.rs:1531-1532` 的注释 `/// 生成时发现设备的挂载点不在了（中途拔线）：build_all 认这个开头，把设备记成掉线。` 紧贴在 `SpaceCheck` 枚举上方，而 `UNMOUNTED` 字符串常量在第 1549 行。本次重构未改动此处，后续维护时可顺手将其移至常量或对应方法上。

---

## 依据

- [`.dev/tasks/2026-11-13-library-error-kinds.md#L45`](file:///tmp/sheng-ren-review-library-error-kinds/.dev/tasks/2026-11-13-library-error-kinds.md#L45)：验收标准要求「结果里写明没跑真书回归、没上真机」，结果章节（第 55–80 行）遗漏该说明，且「偏离」项被误填为文件列表。
- [`crates/library/src/error.rs#L8-L47`](file:///tmp/sheng-ren-review-library-error-kinds/crates/library/src/error.rs#L8-L47)：引入 `Kind` 枚举与 `Error` 包装结构体，`Kind` 与构造方法保持 `pub(crate)`，对外仅暴露实现 `std::error::Error` 与 `Display` 的 `Error` 类型，封装边界良好。
- [`crates/library/src/deliver.rs#L324-L338`](file:///tmp/sheng-ren-review-library-error-kinds/crates/library/src/deliver.rs#L324-L338)：`io_error` 在判定 `is_no_space(e)` 为真时打上 `Kind::NoSpace`，文案维持 `NO_SPACE` 常量开头不变；`space_tests`（第 492–530 行）同时断言了 `kind == Kind::NoSpace` 与 `as_str().starts_with(...)`。
- [`crates/library/src/transfer.rs#L229-L244`](file:///tmp/sheng-ren-review-library-error-kinds/crates/library/src/transfer.rs#L229-L244)：`lane_step` 分派不再依赖 `starts_with`，改为 `r.as_ref().is_err_and(|e| e.kind() == Kind::NoSpace)`，单测（第 252–266 行）完整覆盖停止逻辑。
- [`crates/library/src/build_run.rs#L335`](file:///tmp/sheng-ren-review-library-error-kinds/crates/library/src/build_run.rs#L335) 与 [L413](file:///tmp/sheng-ren-review-library-error-kinds/crates/library/src/build_run.rs#L413)：`build_all` 分派分别使用 `error.kind() == Kind::NoSpace` 与 `error.kind() == Kind::Unmounted`，抛弃前缀字符串匹配；事件上报调用 `error.to_string()` 维持对外文案不变。
- [`crates/library/src/catalog.rs#L130-L134`](file:///tmp/sheng-ren-review-library-error-kinds/crates/library/src/catalog.rs#L130-L134) 与 [`crates/library/src/sources.rs#L927-L935`](file:///tmp/sheng-ren-review-library-error-kinds/crates/library/src/sources.rs#L927-L935)：`add_file_only` 在文件写中时返回带 `Kind::Busy` 的错误，`sources.rs` 同步流程通过 `e.kind() == Kind::Busy` 判断并保留旧记录，文案保持 `BUSY` 常量一致。

---

## 置信度

高。

理由：通过文件只读检查完整对照了分支上 4 个提交的 diff，逐项核实了 3 个错误前缀的定义、产生、错误种类封装以及业务分派点，确认对外字符串格式化、错误传播、资源回收及并发通道类型均正确无误，改动完全处于 `crates/library` 范围之内。

---

## 未核实

1. 因处于只读二审模式且禁止运行 shell 命令，未在本地执行 `cargo test --workspace` 与 `cargo clippy --workspace --all-targets -- -D warnings`，需依赖 CI 及负责 bot 确认测试与 0 警告状态。
2. 按照 `CONTRIBUTING.md` 规则，真书回归与真机验证仅限维护者本机进行，二审未验证真机/真书。
