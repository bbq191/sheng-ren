# 二审 .dev/tasks/2026-11-13-track-listing.md

- 仓库：/home/box/repos/sheng-ren
- 分支：task/2026-11-13-track-listing（对比 master）
- 时间：2026-11-14T09:38:03+08:00（Asia/Shanghai）
- 模型：gemini-3.8-flash-medium
- 用时：277s
- 退出码：0

## 结论

建议通过。代码实现清晰严谨，完全契合需求原文与任务单约束；不仅复用了既有的 `Tracked::rel` 路径推导逻辑，且修复提交 `5b955ab` 妥善解决了软链接/相对路径规范化对齐书根目录的边界问题；单元测试与 CLI 测试覆盖完备；未改动版本号及 `fp_golden.txt`，未超范围修改其他文件。合并前仅需由负责 bot 同步补正任务单中的提交元数据并补齐验收记录。

---

## 问题分档

### 阻断

无。

### 建议

1. **任务单 `## 结果` 元数据需同步更新**：任务单 [.dev/tasks/2026-11-13-track-listing.md](file:///home/box/repos/sheng-ren/.dev/tasks/2026-11-13-track-listing.md#L86-L92) 中的 `base..head` 仍记录为截止到 `c0eee0c`，提交表格也缺少后续的修复提交 `5b955ab`（以及填写结果提交 `598bd6b`）。合并前应将 head 更新为 `5b955ab` 并将提交补齐。
2. **任务单 `## 审查` 需补齐验收记录**：任务单 [.dev/tasks/2026-11-13-track-listing.md](file:///home/box/repos/sheng-ren/.dev/tasks/2026-11-13-track-listing.md#L102-L106) 下的 `- 验收：` 当前为空，负责 bot 应按惯例补齐验收通过记录并注明「没跑真书回归、没上真机」。

### 线索

1. **帮助烟测断言模式放宽**：[crates/library/tests/cli.rs](file:///home/box/repos/sheng-ren/crates/library/tests/cli.rs#L175-L177) 中对 `booklib track -h` 不含 untrack 的烟测断言由 `!out.contains("untrack")` 调整为 `!out.contains("] untrack ")`，以允许在 track 帮助正文中提示「要从某几台拿掉用 booklib untrack...」。后续并行的 `2026-11-13-help-rewrite` 任务涉及帮助重写时需留意该断言的匹配约束。

---

## 依据

1. **需求 1（列表与完成提示设备位置）**：
   - [crates/library/src/sources.rs:420-426](file:///home/box/repos/sheng-ren/crates/library/src/sources.rs#L420-L426)：新增 `entry_placement`，通过探测路径复用底层 `Tracked::rel`，确保位置判定与实际同步生成完全一致，无独立分支逻辑。
   - [crates/library/src/bin/booklib/main.rs:1329-1340](file:///home/box/repos/sheng-ren/crates/library/src/bin/booklib/main.rs#L1329-L1340)：`placement_shown` 正确处理了空路径映射为「根目录」、单本不带斜杠、目录带尾部斜杠 `/` 的格式要求。
   - [crates/library/src/bin/booklib/main.rs:1238-1248](file:///home/box/repos/sheng-ren/crates/library/src/bin/booklib/main.rs#L1238-L1248)：完成提示在 `5b955ab` 中加入 `canonicalize` 处理，杜绝了软链接与相对路径因前缀不匹配误判为根目录的漏洞。
   - [crates/library/src/bin/booklib/main.rs:1377-1393](file:///home/box/repos/sheng-ren/crates/library/src/bin/booklib/main.rs#L1377-L1393)：同名 `--top` 目录的合并告警提示逻辑和文案保持完整。

2. **需求 2（`track --device=x` 无路径时设备过滤）**：
   - [crates/library/src/bin/booklib/main.rs:1212-1217](file:///home/box/repos/sheng-ren/crates/library/src/bin/booklib/main.rs#L1212-L1217)：无路径时通过 `device_ids(ctx.args, lib)` 提取模式并传给 `print_tracked(lib, &devices)`；未知模式自然流转到 `parse_devices` 触发用法报错（退出码 1）；`all` 或未指定时返回空列表列出全部。
   - [crates/library/src/bin/booklib/main.rs:1209](file:///home/box/repos/sheng-ren/crates/library/src/bin/booklib/main.rs#L1209) 及 [1264](file:///home/box/repos/sheng-ren/crates/library/src/bin/booklib/main.rs#L1264)：`--root=` 变更后及带路径的 `track`/`untrack` 执行后，均传入 `&[]`，依然输出全部设备列表。

3. **需求 3（帮助重写）**：
   - [crates/library/src/bin/booklib/main.rs:81-118](file:///home/box/repos/sheng-ren/crates/library/src/bin/booklib/main.rs#L81-L118)：完全符合「用法三种 → 参数 → 设备上放在哪（含 `--top` 示例） → 同一路径再 track 规则 → 例子」的结构，涵盖了覆盖优先级、共用书根约束及只加不减原则。

4. **文档同步与约束核验**：
   - [docs/usage.md:95-101, 343](file:///home/box/repos/sheng-ren/docs/usage.md#L95-L101)：文档示例及说明文字已同步更新为新文案和放法描述。
   - 依赖项与版本号：`Cargo.toml` 与 `crates/library/tests/fp_golden.txt` 均未触碰，未引入外部依赖，符合 clean-room 和纯 Rust 约束。

---

## 置信度

高。已全量比对分支相对于 master 的完整 diff、Git commit log、任务单及 `docs/` 文档，并对路径规范化、目录/单本在设备放置推导上的边界条件（第一层、第二层、根目录、软链接）进行了静态代码推演。

---

## 未核实

1. 二审员运行环境受限于只读约束，未在本地执行 `cargo test --workspace` 与 `cargo clippy --workspace --all-targets -- -D warnings`（需由负责 bot 在验收或合并时核对命令输出）。
2. 未跑真书回归、未上真机（符合任务单约束：改动仅涉及 CLI 列表文案、命令行参数过滤及帮助文本，不影响产物生成）。
