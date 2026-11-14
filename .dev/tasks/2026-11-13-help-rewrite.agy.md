# 二审 .dev/tasks/2026-11-13-help-rewrite.md

- 仓库：/home/box/repos/sheng-ren
- 分支：task/2026-11-13-help-rewrite（对比 master）
- 时间：2026-11-14T23:26:31+08:00（Asia/Shanghai）
- 模型：gemini-3.8-flash-medium
- 用时：190s
- 退出码：0

## 结论

建议通过（无阻断问题）。

本次改动完整覆盖了 vinci 的需求原文，将顶层 `--help` 及 10 个子命令帮助严格统一为「用法 / 一句话说明 / 参数 / 例子」等标准结构，用词和全角冒号标点保持一致；纠正了 `add` 中 `documents/` 的已知过时表述，且未回退 `track-listing` 已重写的 `track` 帮助语义与规则；未改动任何命令行参数定义、选项集合、默认值或底层业务逻辑；不升版本号、未动产物、未改动 `fp_golden.txt`；在 `cli.rs` 中补充了针对全部 10 个子命令在 `--help`、`-h`、`help <cmd>` 形式下的烟测断言。

代码逻辑与风险控制均无问题。任务单中存在少量元数据待负责 bot 在验收合并时同步补齐（见建议项）。

---

## 问题分档

### 阻断

- 无

### 建议

1. **任务单结果元数据与验收记录补齐**：
   分支在执行方提交 `736522f` 后，负责 bot 验收时追加了修正提交 `aa5c694`（统一小节标题与对齐）以及任务单状态提交 `181daeb`。目前 [.dev/tasks/2026-11-13-help-rewrite.md](file:///home/box/repos/sheng-ren/.dev/tasks/2026-11-13-help-rewrite.md#L71-L92) 中的 `base..head` 仍记录为 `..736522f`，提交列表未收录 `181daeb` 和 `aa5c694`，末尾漏了原模板中的「没跑真书回归、没上真机（只改帮助文字，不影响产物）」声明，且 `## 审查` 下的 `- 验收：` 尚为空白。建议负责 bot 编写验收记录时，一并更新 `base..head`、补全提交记录与免测说明（与上一张单 `2026-11-13-track-listing.md` 的做法一致）。
2. **`meta_edit.rs` 偏离记录释疑**：
   执行方在任务单的「偏离」一栏记录了 `crates/library/src/bin/booklib/meta_edit.rs`。但实际上任务单第 17 行（「输入」一节）已明确交代「`meta --edit --help` 若有单独的说明，一并按同一风格整理」，因此改动该文件完全在任务输入预期之内，并非超范围偏离。建议负责 bot 在验收结论中注明该项为符合预期的整理，消除歧义。

### 线索

- 无

---

## 依据

1. **结构统一与语义未回退**：
   - [crates/library/src/bin/booklib/main.rs#L41-L298](file:///home/box/repos/sheng-ren/crates/library/src/bin/booklib/main.rs#L41-L298)：全部 10 个子命令均对齐为「用法 / 一句话说明 / 参数 / 例子」结构（`devices` 无参数，对齐为「说明 / 例子」）。`track` 的详细规则和参数说明保持不变，仅将与顶层 `用法：` 冲突的小标题 `用法三种：` 优化调整为 `三种写法：`（[main.rs#L82](file:///home/box/repos/sheng-ren/crates/library/src/bin/booklib/main.rs#L82)），语义完全未回退。
   - [crates/library/src/bin/booklib/main.rs#L301-L348](file:///home/box/repos/sheng-ren/crates/library/src/bin/booklib/main.rs#L301-L348)：顶层帮助将原本散落在 `HELP_HEAD` 的日常使用例子整理移入 `HELP_TAIL` 的 `例子：` 小节，并统一小节名为 `通用参数：`，排版清晰规整。
2. **修正过时说明**：
   - [crates/library/src/bin/booklib/main.rs#L158](file:///home/box/repos/sheng-ren/crates/library/src/bin/booklib/main.rs#L158)：`add` 的 detail 将原先片面的 `放设备 documents/ 顶层` 修正为与各设备实际目录及现行文档一致的 `放各设备产物目录顶层`。
3. **未改变任何命令参数与行为**：
   - [crates/library/src/bin/booklib/main.rs#L71-L298](file:///home/box/repos/sheng-ren/crates/library/src/bin/booklib/main.rs#L71-L298)：各命令的 `opts`、`flags`、`check`、`lock`、`run` 均未做任何改动；[main.rs#L392-L422](file:///home/box/repos/sheng-ren/crates/library/src/bin/booklib/main.rs#L392-L422) 的帮助解析和退出逻辑保持原样，只改动了给用户展示的字符串常量。
4. **元数据改动**：
   - [crates/library/src/bin/booklib/meta_edit.rs#L13-L33](file:///home/box/repos/sheng-ren/crates/library/src/bin/booklib/meta_edit.rs#L13-L33)：统一了全角冒号 `用法：`、`参数（...）：`、`说明：`，并补全了 `书.epub|书库id或书名` 及 `--get-cover` 的说明。
5. **测试覆盖**：
   - [crates/library/tests/cli.rs#L179-L194](file:///home/box/repos/sheng-ren/crates/library/tests/cli.rs#L179-L194)：新增针对全部 10 个子命令在 `--help`、`-h`、`booklib help <cmd>` 场景下的循环测试，断言退出码为 0，且输出包含 `用法：\n` 和 `\n例子：`，足以验证验收标准。
6. **文档同步更新**：
   - [docs/usage.md#L67](file:///home/box/repos/sheng-ren/docs/usage.md#L67)：帮助格式说明已同步更新为新结构的「一句话说明 / 用法 / 命令 / 通用参数 / 环境变量 / 退出码 / 例子」。

---

## 置信度

高。

理由：本任务不涉及算法、协议与文件 I/O 等复杂逻辑，改动全为静态文本常量、测试断言与文档说明。针对 diff、任务单和代码仓库进行了逐行核对，所有硬约束（版本号、`fp_golden.txt`、clean-room 原则、命令行为不变）全部得到遵守。

---

## 未核实

- 本次为只读二审，未运行 shell 命令（未实际执行 `cargo test --workspace` 与 `cargo clippy --workspace --all-targets -- -D warnings`），该部分需由负责 bot 在验收合并环节实际执行核实。
- 按任务单约定，未跑真书回归、未上真机验证（本次改动仅影响帮助文本展示，不改变任何转换和产物逻辑）。
