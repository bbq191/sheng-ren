## 结论

**无阻断问题，准予合并**。分支改动严格遵循「增删设备 = 增删 TOML」架构，未引入任何新 profile 键或转换逻辑，已有四台设备配置与产物指纹零变更；真机测量参数、固定版式方案 D 与投递路径均已在配置、测试及文档中完整对齐。

---

## 重点审查项核对结论

1. **投递路径与大小写 / 挂载点差异**：
   - 投递配置为 `mount = "bigme"`、`dir = "books"`（[`crates/profile/profiles/bigme.toml:47-50`](file:///home/afu/Projects/sheng-ren/crates/profile/profiles/bigme.toml#L47-L50)）。
   - 缺省挂载点 `/run/user/1000/mtp/bigme` 与 2026-11-10 真机 `~/Devices/Bigme` 的差异，以及过渡期使用 `BOOKLIB_MTP_DIR=/home/afu/Devices` 或软链接的说明已详尽写入文档与注释（[`crates/profile/profiles/bigme.toml:49`](file:///home/afu/Projects/sheng-ren/crates/profile/profiles/bigme.toml#L49)、[`docs/devices.md:367`](file:///home/afu/Projects/sheng-ren/docs/devices.md#L367)、[`docs/usage.md:327`](file:///home/afu/Projects/sheng-ren/docs/usage.md#L327)）。
   - [`install.sh`](file:///home/afu/Projects/sheng-ren/install.sh#L1-L166) 全文未动，未硬编码任何特定设备挂载点。
   - 安卓文件系统与 MTP 规范不区分大小写，`dir = "books"` 沿用汉王 `documents` 映射真机 `Documents/` 的成功先例。
2. **Profile 键与现有设备未动**：
   - `bigme.toml` 使用的全部为现有通用键，无 Bigme 专属私有键（[`crates/profile/profiles/bigme.toml:5-50`](file:///home/afu/Projects/sheng-ren/crates/profile/profiles/bigme.toml#L5-L50)）。
   - 代码层未加设备分支逻辑；[`crates/profile/profiles/hanvon.toml`](file:///home/afu/Projects/sheng-ren/crates/profile/profiles/hanvon.toml#L1-L39)、[`ireader.toml`](file:///home/afu/Projects/sheng-ren/crates/profile/profiles/ireader.toml#L1-L50)、[`kindle.toml`](file:///home/afu/Projects/sheng-ren/crates/profile/profiles/kindle.toml#L1-L50)、[`kobo.toml`](file:///home/afu/Projects/sheng-ren/crates/profile/profiles/kobo.toml#L1-L44) 零修改。
3. **漫画画质、方案 D、封面缩略图与 gutter crop**：
   - 漫画画质参数（`comic_gamma = 1.1`、`image_gamma = 1.1`、`comic_autocontrast`、`comic_trim_specks`、`comic_page_numbers`、`comic_dark_bg`、`comic_margin = 1`、`comic_split_spreads = true`、`comic_meta_direction = true`）与另四台完全一致（[`crates/profile/profiles/bigme.toml:13-21`](file:///home/afu/Projects/sheng-ren/crates/profile/profiles/bigme.toml#L13-L21)），并已加入 `lib.rs` 的三个统一断言测试（[`crates/profile/src/lib.rs:1300-1331`](file:///home/afu/Projects/sheng-ren/crates/profile/src/lib.rs#L1300-L1331)）。
   - 选用固定版式方案 D（`comic_fixed_layout = true`、`comic_fixed_layout_percent = true`），断言严格匹配（[`crates/profile/profiles/bigme.toml:22-23`](file:///home/afu/Projects/sheng-ren/crates/profile/profiles/bigme.toml#L22-L23)、[`crates/profile/src/lib.rs:1281-1284`](file:///home/afu/Projects/sheng-ren/crates/profile/src/lib.rs#L1281-L1284)）。
   - 未配置 `cover_thumb_aspect` 与 `cover_meta_thumb`，注释明确记录探针 C（正文第一页）事实并有断言守护（[`crates/profile/profiles/bigme.toml:27-29`](file:///home/afu/Projects/sheng-ren/crates/profile/profiles/bigme.toml#L27-L29)、[`crates/profile/src/lib.rs:1285-1289`](file:///home/afu/Projects/sheng-ren/crates/profile/src/lib.rs#L1285-L1289)）。
   - 未配置 `comic_gutter_crop`，字段按 serde default 缺省保持 `false`（[`crates/profile/profiles/bigme.toml:25`](file:///home/afu/Projects/sheng-ren/crates/profile/profiles/bigme.toml#L25)）。
4. **真机测量值准确性**：
   - 屏幕标称与真机 `wm size`：1264×1680（[`crates/profile/profiles/bigme.toml:31-33`](file:///home/afu/Projects/sheng-ren/crates/profile/profiles/bigme.toml#L31-L33)）。
   - 文字书可阅读范围：1069×1424，注释详细说明上留 96、下留 160、左 96、右 99 及校正工具栏误报背景（[`crates/profile/profiles/bigme.toml:35-39`](file:///home/afu/Projects/sheng-ren/crates/profile/profiles/bigme.toml#L35-L39)）。
   - 漫画画布：1264×1680，注释记录标记范围约 1105×1532（[`crates/profile/profiles/bigme.toml:41-44`](file:///home/afu/Projects/sheng-ren/crates/profile/profiles/bigme.toml#L41-L44)）。
   - 色彩与格式：`color = false`、`formats = ["epub"]`（[`crates/profile/profiles/bigme.toml:6-7`](file:///home/afu/Projects/sheng-ren/crates/profile/profiles/bigme.toml#L6-L7)）。
   - `lib.rs` 对应断言严格核对（[`crates/profile/src/lib.rs:1268-1280`](file:///home/afu/Projects/sheng-ren/crates/profile/src/lib.rs#L1268-L1280)）。
5. **「结果」与 diff 一致性及金样验证**：
   - 实际改动涉及文件与任务单「结果」表格完全吻合，无超范围修改。
   - [`crates/library/tests/fp_golden.txt`](file:///home/afu/Projects/sheng-ren/crates/library/tests/fp_golden.txt#L1-L124) 仅在最前部新增 124 行（全为 `bigme`），其余 496 行现有设备指纹完全未变，删除 0 行，与执行记录一致。

---

## 问题分档

### 阻断（0 项）
无。

### 建议（4 项）
1. **`HELP_HEAD` 概括文案漏列 Bigme**：
   [`crates/library/src/bin/booklib/main.rs:194`](file:///home/afu/Projects/sheng-ren/crates/library/src/bin/booklib/main.rs#L194) 的全局帮助头部仍写「按 Kindle、掌阅、Kobo、汉王的自带阅读器优化」，虽然同文件第 53 行的 `sync` 设备表已补充 `bigme`，建议在此处一并补齐。
2. **三个统一测试注释仍写「四台」**：
   [`crates/profile/src/lib.rs:1297, 1314, 1322`](file:///home/afu/Projects/sheng-ren/crates/profile/src/lib.rs#L1297) 的注释说明仍保留「维护者定四台漫画……」，但循环数组中已扩充为包含 `bigme` 的五台，建议注释同步更新为「五台」。
3. **`docs/devices.md` 列表项排版断档**：
   [`docs/devices.md:286`](file:///home/afu/Projects/sheng-ren/docs/devices.md#L286) 将 Bigme 方案 D 摘要直接插入在两个无序列表项之间，缺少 `- ` 列表标记，导致阅读渲染断档，建议调整为列表项或移至专节。
4. **停用模式测试可顺手断言 Bigme 仍存在**：
   [`crates/library/tests/cli.rs:283`](file:///home/afu/Projects/sheng-ren/crates/library/tests/cli.rs#L283) 的 `devices_skip_disabled_profile` 用例断言停用 `ireader` 后其他设备依然在，列表中包含了 `kindle`、`kobo`、`hanvon`，可补上 `("bigme", "EPUB")`（同 `lib.rs:1532` 的做法）。

### 线索（1 项）
1. **真机首轮 sync 目录合并行为确认**：
   [`crates/profile/profiles/bigme.toml:50`](file:///home/afu/Projects/sheng-ren/crates/profile/profiles/bigme.toml#L50) 配置的小写 `dir = "books"` 在绝大多数 Android MTP 实现中会自动归入现有的 `Books/`，但需在合并后真机实机传输时观察设备端是否直接落入 `Books/` 而未新建平行目录。

---

## 依据
- [`crates/profile/profiles/bigme.toml:1-50`](file:///home/afu/Projects/sheng-ren/crates/profile/profiles/bigme.toml#L1-L50)
- [`crates/profile/src/lib.rs:1-12, 1166-1168, 1259-1331, 1532`](file:///home/afu/Projects/sheng-ren/crates/profile/src/lib.rs#L1259-L1331)
- [`crates/library/tests/fp_golden.rs:227`](file:///home/afu/Projects/sheng-ren/crates/library/tests/fp_golden.rs#L227)
- [`crates/library/tests/fp_golden.txt:1-124, 125-620`](file:///home/afu/Projects/sheng-ren/crates/library/tests/fp_golden.txt#L1-L124)
- [`crates/library/tests/cli.rs:254-265, 283`](file:///home/afu/Projects/sheng-ren/crates/library/tests/cli.rs#L254-L265)
- [`crates/library/src/bin/booklib/main.rs:53, 194`](file:///home/afu/Projects/sheng-ren/crates/library/src/bin/booklib/main.rs#L53)
- [`README.md:27`](file:///home/afu/Projects/sheng-ren/README.md#L27)
- [`docs/devices.md:122, 286, 307-309, 361-373`](file:///home/afu/Projects/sheng-ren/docs/devices.md#L122)
- [`docs/usage.md:325-327`](file:///home/afu/Projects/sheng-ren/docs/usage.md#L325-L327)
- [`install.sh:1-166`](file:///home/afu/Projects/sheng-ren/install.sh#L1-L166)
- [`.dev/tasks/2026-11-10-bigme-profile.md:87-146`](file:///home/afu/Projects/sheng-ren/.dev/tasks/2026-11-10-bigme-profile.md#L87-L146)

---

## 置信度
**高**。依据直接来源于静态只读比对代码仓库核心文件、配置 TOML、金样数据集及测试文件，与任务单记录及架构约束完全闭合。

---

## 未核实
1. **真机挂载及首次真实投递**：因受指令约束本轮未执行任何命令，真机当前挂在 `~/Devices/Bigme`，`sync` 投递至真机的流程需留待合并后按任务单由 vinci 进行真机验证。
2. **自动化测试命令实际执行**：未在当前会话中实际调用 `cargo test` / `cargo clippy`，测试通过结论基于静态代码审查与任务单中 opencode 及负责 bot 的重跑记录。
