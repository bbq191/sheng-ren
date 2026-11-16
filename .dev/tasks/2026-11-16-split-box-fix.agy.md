# 二审 .dev/tasks/2026-11-16-split-box-fix.md

- 仓库：/home/box/repos/sheng-ren
- 分支：task/2026-11-16-split-box-fix（对比 master）
- 时间：2026-11-16T16:24:49+08:00（Asia/Shanghai）
- 模型：gemini-3.8-flash-medium
- 用时：131s
- 退出码：0

## 结论

建议通过。本任务严格遵守「只搬代码、不改逻辑」的重构约束，将超过 2000 行的 `box_fix.rs` 成功拆分为模块根与 `line`、`rewrite`、`tests` 三个子模块，拆分后所有文件行数均远低于 1800 行上限（最大的 `box_fix.rs` 为 1119 行，按任务约定完整保留约 880 行的 `plan` 函数）。负责 bot 及时收拢了 `frame_open` 的可见性，且通过了 17 项单元测试一致性、全工作区测试、clippy 零警告以及多设备（含 Move 模式）产物 sha256 逐字节比对，未发现逻辑偏离或风险。

## 问题分档

### 阻断

无。

### 建议

无。

### 线索

- **任务单背景提及的 `list_indent_extra_px` 再导出**：任务单输入章节提到「`wash/mod.rs` 文档注释里提到 `box_fix::page_top_rule_px`、`box_fix::list_indent_extra_px`。这些路径拆完后都要照旧能用（在 `box_fix.rs` 里再导出）」。实际实现中，`list_indent_extra_px` 在 `rewrite.rs` 为 `pub(super)`，在 `box_fix.rs` 中仅为私有 `use`，未向 `wash` 再导出。经核实：拆分前 master 中的 `list_indent_extra_px` 原本就是 `box_fix.rs` 的私有函数（非 `pub(super)`），且 `wash/mod.rs` 中仅在注释普通行内代码块内提及（非 intra-doc 链接），代码中无外部调用，因此当前可见性与 master 完全一致，无需补充再导出。

## 依据

- `.dev/tasks/2026-11-16-split-box-fix.md:27`：输入描述中列出外部引用与文档注释提及项。
- `crates/bookconv/src/wash/box_fix.rs:38-40`：`list_indent_extra_px` 被私有 `use` 引入供 `plan` 使用，`page_top_rule_px` 保持 `pub(super) use` 导出给 `top_spacer.rs`。
- `crates/bookconv/src/wash/box_fix/line.rs:1181` 与 `crates/bookconv/src/wash/box_fix.rs:37`：提交 `d46610a` 将 `frame_open` 收敛为 `line.rs` 中 `pub(super)` 与 `box_fix.rs` 中私有 `use`，可见性收回模块内部，同时维持了模块说明中文档链接 `[`frame_open`]` 的解析。
- `crates/bookconv/src/wash/box_fix/tests.rs:1-559`：全部 17 项测试原样搬迁，路径仍为 `wash::box_fix::tests::...`。

## 置信度

高。已完整阅读提示词、任务单全文、提交记录及全部关联文件 diff，逐项核对代码搬迁前后的一致性与可见性范围，逻辑与约束完全吻合。

## 未核实

- 未在真机环境运行或验证真实阅读器渲染（任务单及提交信息已如实声明「没跑真书回归、没上真机」）。
- 作为只读二审员，未在本地执行 `cargo test` 或二进制产物哈希比对命令，测试及 sha256 结果基于任务单审查记录与执行记录。
