# 二审 .dev/tasks/2026-11-13-vertical-default-off.md

- 仓库：/tmp/sheng-ren-review-vertical-default-off
- 分支：task/2026-11-13-vertical-default-off（对比 master）
- 时间：2026-11-13T11:06:49+08:00（Asia/Shanghai）
- 模型：gemini-3.8-flash-medium
- 用时：427s
- 退出码：0

## 结论

建议返工。本次改动整体方向准确落实了维护者需求（Kobo、汉王默认改横排，仅逐书/目录 `--on` 与原书自带竖排才竖排，推翻 11-11 决定并规范同步了文档、版本号与指纹），但核心判定链存在一处**阻断性缺陷**：`Library::vertical_for` 的默认分支漏掉了对「固定版式（fixed-layout）」的排除检查，违背了“漫画、固定版式一律不竖排”的铁律，导致自带竖排样式的固定版式书会被错误纳入竖排改写管线。此外，任务单「结果」缺少给 vinci 的真书回归 fish 命令与“没上真机”声明，提交信息也需同步。修复上述问题后方可合并。

## 问题分档

### 阻断

- **默认分支漏掉固定版式排除检查**：[crates/library/src/vertical.rs:168-170](file:///tmp/sheng-ren-review-vertical-default-off/crates/library/src/vertical.rs#L168-L170) 的 `Library::vertical_for` 在单书覆盖和目录覆盖分支均显式检查了 `!self.is_fixed_layout(meta)?`，但新增的默认分支直接使用了 `Ok(self.is_natively_vertical(meta).unwrap_or(false))`；同时 [crates/bookconv/src/vertical.rs:142-180](file:///tmp/sheng-ren-review-vertical-default-off/crates/bookconv/src/vertical.rs#L142-L180) 中的底层 `is_natively_vertical` 仅检查 CSS 和 OPF 的 writing-mode，并未排除固定版式。若书籍为自带竖排样式的固定版式 EPUB（非 CBZ 漫画），默认分支会误判为竖排并进入竖排优化管线（注入 `eink-vertical.css`、改写正文引号与全角），违背了“漫画、固定版式一律不竖排”的设计铁律与文档声明。应在默认分支增加 `&& !self.is_fixed_layout(meta).unwrap_or(false)`，或在 `vertical_for` 函数开头前置短路排除固定版式。

### 建议

- **`is_natively_vertical` 缺乏进程内缓存**：在 Kobo 和汉王设备上，计算指纹（`Library::plan`）会频繁调用 `vertical_for`。[crates/library/src/vertical.rs:125-133](file:///tmp/sheng-ren-review-vertical-default-off/crates/library/src/vertical.rs#L125-L133) 的 `is_natively_vertical` 每次都会解包读取原件全部文本条目。旧有的 `zh_text_of` 会缓存在 `meta.zh_text` 和 `session.facts` 中，虽本单未要求持久化存入 `Meta`，但建议在 [crates/library/src/session.rs](file:///tmp/sheng-ren-review-vertical-default-off/crates/library/src/session.rs) 的 `ContentFacts` 中增加基于 SHA 的内存缓存，避免在批量 `space_check` / `prepare` / `produce` 时重复对相同原件进行解包 I/O。
- **任务单结果章节存在遗漏与同步滞后**：[.dev/tasks/2026-11-13-vertical-default-off.md:83-108](file:///tmp/sheng-ren-review-vertical-default-off/.dev/tasks/2026-11-13-vertical-default-off.md#L83-L108) 中的 `## 结果` 删除了模板要求的 `真书回归：`、`版本号：`、`没上真机。` 字段，未按指令给出 vinci 本机跑真书回归所需的 fish 命令；且 `base..head` 和提交表格仅记录到 `ecdb5d4`，未同步分支后续提交（`9c643eb` 与 `caebf7c`）。建议补齐声明与命令。

### 线索

- **原件临时离线时指纹抖动**：`vertical_for` 默认分支对 `is_natively_vertical` 采用了 `.unwrap_or(false)`。若原书竖排但原件介质临时离线（如外接存储未挂载），算出的指纹会临时缺失 `V`/`f`，待重新挂载后再计算又会重新带上 `V`/`f` 触发重新生成。虽属合理的容错降级，但负责人可留意是否需要结合历史生成记录保持稳定。

## 依据

- [crates/library/src/vertical.rs:146-170](file:///tmp/sheng-ren-review-vertical-default-off/crates/library/src/vertical.rs#L146-L170)：`vertical_for` 注释明确写明“设备没开 `vertical_text`、漫画、固定版式一律不竖排”，第 160 行和第 165 行均调用了 `!self.is_fixed_layout(meta)?`，但第 169 行默认分支仅写了 `Ok(self.is_natively_vertical(meta).unwrap_or(false))`，丢失了对固定版式的防护。
- [crates/bookconv/src/vertical.rs:142-180](file:///tmp/sheng-ren-review-vertical-default-off/crates/bookconv/src/vertical.rs#L142-L180)：底层 `is_natively_vertical` 仅检查 CSS 和 OPF meta，不包含 `is_fixed_layout` 检查。
- [docs/typesetting.md:1769](file:///tmp/sheng-ren-review-vertical-default-off/docs/typesetting.md#L1769)：文档明确定性“只有 `--on`（逐书或目录 `vertical.toml`）或原书本来就竖排才走竖排管线：不是漫画、不是固定版式；原书任一样式表...”。
- [crates/library/src/vertical.rs:125-133](file:///tmp/sheng-ren-review-vertical-default-off/crates/library/src/vertical.rs#L125-L133)：`Library::is_natively_vertical` 每次无缓存直接打开文件解包，影响批量指纹计算性能。
- [.dev/tasks/2026-11-13-vertical-default-off.md:73-86](file:///tmp/sheng-ren-review-vertical-default-off/.dev/tasks/2026-11-13-vertical-default-off.md#L73-L86)：验收标准与执行指令要求在缺少测试书时写明给 vinci 的 fish 命令并在结果里说明“没上真机”，现结果部分缺失该项。

## 置信度

高。代码逻辑、注释声明、文档规范对照明确，缺失条件判断可直接推导缺陷场景。

## 未核实

- 未在本机运行 `cargo test --workspace` 与 `cargo clippy`（只读二审约束禁止运行 shell 命令）。
- 未在真实阅读设备（Kobo Clara BW、汉王 Clear 6 Turbo）及真书样本集上验证生成产物在阅读器中的实际排版效果。
