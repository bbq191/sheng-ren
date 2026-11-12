# 二审 .dev/tasks/2026-11-12-dir-case-bigme-ireader.md

- 仓库：/home/box/repos/sheng-ren-dir-case-bigme-ireader
- 分支：task/2026-11-12-dir-case-bigme-ireader（对比 master）
- 时间：2026-11-12T17:24:52+08:00（Asia/Shanghai）
- 模型：gemini-3.8-flash-medium
- 用时：276s
- 退出码：0

## 结论

**建议通过**。本改动严格对照 vinci 需求原文与任务单约束，将 Bigme 与掌阅的产物投递目录修改为设备真名 `Books` 与 `Documents`；复用了已有的 `case_alias` 大小写迁移及删除保护机制；重构了测试夹具并为汉王、掌阅、Bigme 三台设备完整覆盖了 9 组大小写迁移与保护用例；同步更新了命令行帮助、使用指南、设备规范、架构说明、审计清单与决定记录（明确推翻同日旧决定）；未变动版本号、指纹与正文优化逻辑，`crates/bookconv`、`crates/kfx` 及 `fp_golden.txt` 无任何改动；任务单结果按工作流已在 `master` 正确填报并标记 `done`。

## 问题分档

### 阻断

无

### 建议

1. **`booklib add` 命令帮助文字中的目录描述可进一步精确**：`crates/library/src/bin/booklib/main.rs:118` 在 `add` 的详细说明中仍写有「放设备 documents/ 顶层」，虽然不影响功能，但各设备根目录现已包含 `documents/`、`Documents/`、`books/`、`Books/`，未来可适时统一调整为「放各设备产物目录顶层」以保持整体口径一致。
2. **测试公共夹具 `dev_dir` 可考虑预建其他设备的存储目录**：`crates/library/tests/common/mod.rs:14-16` 目前仅预建了 `kindle` 和 `ireader` 的 `Internal Storage`。虽然 `flow.rs` 中的汉王和 Bigme 测试都在用例内自行建立了存储路径，但若未来有其他用例通过 `common::documents(lib, "bigme")` 访问未自行建目录的路径，可能会依赖父目录存在。

### 线索

- **任务分支工作区中任务单未拉取 master 上的最新状态**：按本仓库工作流（[CONTRIBUTING.md:20](file:///home/box/repos/sheng-ren-dir-case-bigme-ireader/CONTRIBUTING.md#L20)），任务单文件 `.dev/tasks/2026-11-12-dir-case-bigme-ireader.md` 的结果与 `done` 状态已由执行方直接提交至 `master`（提交 [945b6ff](file:///home/box/repos/sheng-ren/.git/logs/refs/heads/master#L15)）；任务分支 `task/2026-11-12-dir-case-bigme-ireader` 本身保持纯净代码提交（[b2a6606](file:///home/box/repos/sheng-ren/.git/logs/refs/heads/task/2026-11-12-dir-case-bigme-ireader#L2)）。合并任务分支后两边自动汇合，不属于问题，仅提示核验人员知悉两处文件状态差异的原因。

## 依据

1. **Bigme 与掌阅 profile 变更**：
   - [crates/profile/profiles/bigme.toml:52](file:///home/box/repos/sheng-ren-dir-case-bigme-ireader/crates/profile/profiles/bigme.toml#L52)：`dir = "Books"`，注释写明避免 MTP 多登记同名小写目录并复用大小写迁移；
   - [crates/profile/profiles/ireader.toml:110](file:///home/box/repos/sheng-ren-dir-case-bigme-ireader/crates/profile/profiles/ireader.toml#L110)：`dir = "Documents"`，同步补齐说明。
2. **单元测试与断言**：
   - [crates/profile/src/lib.rs:1256-1263](file:///home/box/repos/sheng-ren-dir-case-bigme-ireader/crates/profile/src/lib.rs#L1256-L1263)：断言掌阅 `deliver` 为 `Documents`；
   - [crates/profile/src/lib.rs:1352-1360](file:///home/box/repos/sheng-ren-dir-case-bigme-ireader/crates/profile/src/lib.rs#L1352-L1360)：断言 Bigme `deliver` 为 `Books`。
3. **命令行帮助与提示同步**：
   - [crates/library/src/bin/booklib/main.rs:4](file:///home/box/repos/sheng-ren-dir-case-bigme-ireader/crates/library/src/bin/booklib/main.rs#L4) 与 [crates/library/src/bin/booklib/main.rs:50-53](file:///home/box/repos/sheng-ren-dir-case-bigme-ireader/crates/library/src/bin/booklib/main.rs#L50-L53)：`booklib sync` 帮助中掌阅与汉王显示 `Documents/<子目录>/`，Bigme 显示 `Books/<子目录>/`（同时修正了汉王帮助中残留的小写 `documents/`）。
4. **集成测试夹具与大小写迁移覆盖**：
   - [crates/library/tests/common/mod.rs:22-29](file:///home/box/repos/sheng-ren-dir-case-bigme-ireader/crates/library/tests/common/mod.rs#L22-L29)：`documents()` 动态从模式的 `deliver` 读取 `dir`，不再写死 `documents`；
   - [crates/library/tests/flow.rs:3490-3662](file:///home/box/repos/sheng-ren-dir-case-bigme-ireader/crates/library/tests/flow.rs#L3490-L3662)：抽取了通用夹具 `case_rebase_keeps_books_and_record`、`case_sensitive_copies_and_keeps_old`、`case_migrates_orphans_too`。通过 `placement_strings` 与 `has_dir_component` 精准核对路径段，有效规避了 Bigme 测试中状态 JSON 根键名 `"books"` 对字符串匹配的误判干扰；
   - [crates/library/tests/flow.rs:3664-3707](file:///home/box/repos/sheng-ren-dir-case-bigme-ireader/crates/library/tests/flow.rs#L3664-L3707)：分别对 `hanvon`（`documents`→`Documents`）、`ireader`（`documents`→`Documents`）、`bigme`（`books`→`Books`）执行了完整的 3 类测试（不分大小写只改记录、大小写敏感保留旧文件并复制、孤儿记录迁移后删除）。
5. **文档与决定记录**：
   - [docs/decisions.md:360](file:///home/box/repos/sheng-ren-dir-case-bigme-ireader/docs/decisions.md#L360)：汉王条目补充「掌阅、Bigme 2026-11-12 已改，见同日下条」；
   - [docs/decisions.md:366](file:///home/box/repos/sheng-ren-dir-case-bigme-ireader/docs/decisions.md#L366)：追加独立决定条目，明确推翻同日关于掌阅与 Bigme `dir` 不动的决定并记录原因；
   - [docs/usage.md:183](file:///home/box/repos/sheng-ren-dir-case-bigme-ireader/docs/usage.md#L183)、[docs/usage.md:320](file:///home/box/repos/sheng-ren-dir-case-bigme-ireader/docs/usage.md#L320)、[docs/usage.md:336-344](file:///home/box/repos/sheng-ren-dir-case-bigme-ireader/docs/usage.md#L336-L344)、[docs/devices.md:188](file:///home/box/repos/sheng-ren-dir-case-bigme-ireader/docs/devices.md#L188)、[docs/devices.md:262](file:///home/box/repos/sheng-ren-dir-case-bigme-ireader/docs/devices.md#L262)、[docs/devices.md:370](file:///home/box/repos/sheng-ren-dir-case-bigme-ireader/docs/devices.md#L370)、[docs/architecture.md:181](file:///home/box/repos/sheng-ren-dir-case-bigme-ireader/docs/architecture.md#L181)、[README.md:24-27](file:///home/box/repos/sheng-ren-dir-case-bigme-ireader/README.md#L24-L27) 全面同步；
   - [.dev/tasks/2026-11-12-quality-audit.md:86](file:///home/box/repos/sheng-ren-dir-case-bigme-ireader/.dev/tasks/2026-11-12-quality-audit.md#L86)：发现 5 状态已补充任务单链接与执行说明。
6. **任务单执行结果**：
   - [.dev/tasks/2026-11-12-dir-case-bigme-ireader.md](file:///home/box/repos/sheng-ren/.dev/tasks/2026-11-12-dir-case-bigme-ireader.md#L70-L91)（提交至 master）：详细记录了提交号、`cargo test`、`cargo clippy`、`cargo fmt` 通过情况，并对可选任务 8（连接清理时机）在本次不做的技术理由进行了详尽记录，注明未上真机并留待 vinci 做真机验收。

## 置信度

**高**。已完整比对 Git 历史日志、默认分支与任务分支实际提交内容、代码差异及相关文档，验证逻辑清晰严密，无破坏性改动。

## 未核实

1. **真机与真书验证**：按项目规范（[CONTRIBUTING.md:9-15](file:///home/box/repos/sheng-ren-dir-case-bigme-ireader/CONTRIBUTING.md#L9-L15)），测试用真书和真机环境只在维护者本机，二审环境不具备 USB MTP 物理设备挂载与真书库，真机验证（接上 Bigme 与掌阅后已有书籍不重传、进度不丢、同步到 `Books/` 与 `Documents/`）留待负责人 vinci 最终验收。
