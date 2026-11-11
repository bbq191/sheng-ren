## 结论

**无阻断**。实现完全对齐需求原文与任务单规则：Kindle 流式文字书翻页方向已改为 LTR 并记录警告；固定版式漫画与流式纯图漫画仍保留 RTL，日漫 rtl 不受影响；金样仅将 Kindle 的 124 行指纹由 `kfx25` 变更为 `kfx26`；无越界改动。可由负责 bot 合并。

---

## 问题分档

### 阻断
无。

### 建议
无。

### 线索
- **【线索】未走优化器的流式漫画若夹带文字页会全局降为 LTR**
  - **依据**：`crates/kfx/src/write/mod.rs:293-298`
  - **分析**：当漫画未声明固定版式（即非 `fixed_canvas.is_some()`）且夹带文字页（如 HTML 版权页、作者后记）时，`parsed.iter().all(single_image_page)` 判定为 `false`，全书翻页方向 `$560` 会降级为 `DIR_LTR`。
  - **影响评估**：
    1. KFX 的 `$560`（翻页方向）属于全书唯一的文档级数据（`crates/kfx/src/write/entities.rs:483`），不支持按章节局部设置；若保留 RTL，流式横排文字页将陷入“横排文字 + 从右往左翻”的严重违和。
    2. 在正常书库链路中，Kindle 漫画受 `kindle.toml`（`comic_fixed_layout = true`）保护走固定版式，直接命中 `fixed_canvas.is_some()` 分支保持 RTL，不受单图判定影响。
    3. 仅当外部使用 `epub-to-kfx` 命令行裸转未优化的流式日漫、且其中含文字页时才会出现漫画翻页变 LTR。当前处理符合任务单预定规则，影响可控。

---

## 重点审查项核实

1. **rtl 流式文字书是否一律改 LTR 并警告**
   - **核实结果**：是。
   - **依据**：
     - `crates/kfx/src/write/mod.rs:291-298`：`!book.meta.rtl` 走 `DIR_LTR`；`book.meta.rtl` 为真但非固定版式、非纯图页时走 `else`，加入警告 `"spine 写了从右往左，KFX 只会横排，按从左往右翻"` 并设置 `direction = DIR_LTR`。
     - `crates/kfx/src/write/entities.rs:482-483`：文档级数据写入 `DOC_WRITING_MODE = WRITING_HORIZONTAL`（`$376`）与 `DOC_DIRECTION = DIR_LTR`（`$557`）。
     - `crates/kfx/src/write/tests.rs:2133-2159`：新增测试 `streaming_text_ignores_spine_rtl` 验证了 `$560` 为 `DIR_LTR`、`$192` 保持横排且警告包含对应关键字。

2. **固定版式和流式纯图漫画是否仍保留 RTL，日漫 rtl 不回退**
   - **核实结果**：是。
   - **依据**：
     - `crates/kfx/src/write/mod.rs:293-294`：`fixed_canvas.is_some() || parsed.iter().all(|(_, _, _, b)| single_image_page(b))` 判定命中时直接写入 `DIR_RTL`（`$559`），且不追加警告。
     - `crates/kfx/src/write/layout.rs:66-68`：抽出的公共函数 `single_image_page` 口径为 `blocks.len() == 1 && matches!(blocks[0].kind, Kind::Image { .. })`，与版面模板判定（`layout.rs:133`）完全复用，无两套逻辑。
     - `crates/kfx/src/write/tests.rs:559-640`（固定版式 RTL 守护测试 `fixed_layout_comic_like_amazon`）、`tests.rs:2182-2197`（流式纯图页测试 `streaming_image_pages_keep_spine_rtl`）以及 `crates/library/tests/flow.rs:2838`（端到端日漫 RTL 测试 `bangumi_japanese_comic_builds_rtl_and_others_stay_as_is`）均保证 RTL 不回退。

3. **金样是否只把 kfx25 改成 kfx26**
   - **核实结果**：是。
   - **依据**：
     - `crates/kfx/src/write/mod.rs:116`：`WRITER_VERSION` 由 25 升为 26。
     - `crates/library/tests/fp_golden.txt:249-372`：变动范围严格限定在 `kindle` 设备的 124 行（全量 124 行末尾由 `kfx25@...` 更新为 `kfx26@...`），第 1–248 行（`bigme`, `hanvon`）及 373–620 行（`ireader` 等）无任何修改。

4. **改动范围与规范合规**
   - **依据**：
     - `crates/kfx/src/write/layout.rs` 抽离函数改动已在任务单报备（`.dev/tasks/2026-11-11-kindle-text-rtl.md:122`）。
     - 未修改 `crates/bookconv/`，非 Kindle 设备的 EPUB / KEPUB 产物逐字节不变。
     - 文档 `docs/kfx.md:403`、`docs/decisions.md:351` 记录完整。

---

## 置信度

**高**。逻辑分支清晰，判定条件完备，涉及的四种状态组合（普通流式 LTR、竖排流式转 LTR、固定版式 RTL、流式纯图 RTL）均有单元测试精准覆盖，金样改动行数与范围均逐行核对无误。

---

## 未核实

1. **真机实际翻页交互**：Kindle PW12 真机打开竖排文字书后“点右侧为下一页”的实际设备行为（按任务单约束留待 vinci 合并后验证）。
2. **命令实际运行结果**：本次审查严格执行只读约束，未在本机重跑 `cargo test` / `cargo clippy`，测试全通结论基于负责 bot 的验收记录（`.dev/tasks/2026-11-11-kindle-text-rtl.md:91-102`）。
