# 全项目代码与文档质量审查（第一阶段：agy 只读审查）

- Task ID: 2026-11-12-quality-audit
- 负责 bot: sheng-ren
- 类型: 审查要求（只读，不是开发任务单；不改代码、不建分支）
- 执行方: agy（vinci 手动运行，`agy --mode plan -p`）
- 基于: master `9e38e74`
- 更新时间: 2026-11-12T10:20+08:00
- 回执: `.dev/tasks/2026-11-12-quality-audit.agy.md`

## 需求原文
> （vinci 2026-11-12）对 sheng-ren 做一次全项目代码 / 文档质量审查，第一阶段由 agy 只读审查。

## 目标
找出下面几类问题，给负责 bot 拆成以后的任务单用。**只找问题、给建议，不写代码。**

1. **重复逻辑**：同一件事写了两遍以上（判断、路径处理、CSS / HTML 处理、错误文案、测试夹具）。
2. **模块边界不清、耦合**：一个模块知道别的模块的内部细节，依赖方向反了，`pub(crate)` 用得太宽，状态散在几处。
3. **过长的函数 / 文件**：一个函数做了好几件事，难测、难审。
4. **死代码**：没人调用的函数、分支、配置字段、测试钩子，已删功能（如 Move / xochitl）的残留。
5. **错误处理不一致**：`String` 错误、`let _ =` 吞错、`unwrap` / `expect`、`panic`，同类错误的文案和处理方式不一样。
6. **抽象放错位置**：只在**确实能消除重复**时，才建议抽出类型、trait 或模式（如一组 bool 参数改成枚举、重复的"读—改—写记录"改成一个函数）。不要为了"设计模式"而建议抽象。
7. **文档过时或互相矛盾**：`README.md`、`AGENTS.md`、`CONTRIBUTING.md`、`docs/*.md` 与代码对不上，或者几篇之间说法不一样。
8. **测试缺口**：关键路径没有测试，测试只测了容易的情况，测试模拟不真实。

## 范围
所有 crate 加上文档。`tools/`（Python 脚本）只看和 Rust 代码或文档对不上的地方，优先级最低。

| crate | 源码行数（约） | 说明 |
|---|---|---|
| `crates/bookconv` | 51k | EPUB 清洗、优化、图片、竖排、KEPUB；bin：`epub_optimize`、`epub_to_kepub`、`readable_measure`、`readable_probe` |
| `crates/library` | 14k | 书库、`booklib` 命令行、生成记录、传到设备 |
| `crates/kfx` | 9.7k | KFX 写出器、Ion；bin：`epub_to_kfx`、`kfx_dump`、`kfx_repack` |
| `crates/csscascade` | 3.2k | CSS 层叠 |
| `crates/bookbase` | 2.5k | 公共 HTML / 路径 / 颜色工具 |
| `crates/profile` | 1.8k | 设备 profile（`profiles/*.toml`）和校验 |

文档：`README.md`、`AGENTS.md`、`CONTRIBUTING.md`、`docs/architecture.md`、`docs/decisions.md`、`docs/development.md`、`docs/devices.md`、`docs/kepub.md`、`docs/kfx.md`、`docs/typesetting.md`、`docs/usage.md`。

## 已知热点（负责 bot 在这几张任务单里注意到的，请重点看，也不要只看这些）
- **大文件**：`bookconv/src/imgopt.rs`（约 3900 行）、`csscascade/src/lib.rs`（约 3060 行）、`bookconv/src/wash/box_fix.rs`（约 2000 行）、`kfx/src/write/parse.rs`（约 2000 行）、`library/src/generate.rs`（约 1960 行）、`library/src/bin/booklib/main.rs`（约 1890 行）、`bookconv/src/htmlproc/footnote.rs`、`wash/kindle_rules.rs`、`library/src/metadata.rs`、`profile/src/lib.rs`（各约 1800 行）。看要不要拆、怎么拆才不改行为。
- **`library/src/generate.rs`**：`prepare_file`（`:726` 起，约 160 行）里认领、挪位置、预登记、比较是不是一样都混在一起；"读记录—克隆—改—写回"这套在 `remove_outputs`、`retract`、`hand_over_outputs`、`flush_removed`、`finish`、`rebase_state_case` 里各写了一遍；`case_twin` / `case_alias` / `distinct_on_disk` 三个大小写判断的关系。
- **`library/src/deliver.rs`**：`put_file` / `put_file_retry` / `put_file_once`（`:188`、`:368` 起）带 `keep_src`、`compare`、`direct` 三个 bool 和一个 `mount`，组合多、容易传错（2026-11-12 汉王任务单的阻断就出在 `direct` 没跟着改）。
- **两处 `vertical.rs`**：`bookconv/src/vertical.rs`（约 1000 行，排版）和 `library/src/vertical.rs`（约 220 行，判定和开关）的分工；2026-11-11 vinci 跑 `booklib vertical --on` 时卡在汉王的 MTP 挂载上，按理这条命令只改书库记录、不该碰设备，原因没查到，请顺带找一下是哪一步访问了设备（`未核实` 也写出来）。
- **文档**：`docs/architecture.md:181` 只写了 Kindle、掌阅放 `documents/`，没有汉王（现在是 `Documents/`）、Kobo（`books/`）、Bigme；`AGENTS.md` 第 4 条"推新分支，不直接改 master"和现在的任务单工作流（任务单直接提交在 master）是否冲突；`docs/development.md:96` 说版本号"现值只写在这张表里"，看别处还有没有写旧值的；文档里提到 Move / xochitl 的有约 200 处，哪些是该留的历史、哪些已经误导。
- 掌阅 `ireader.toml:110`（`documents`）、Bigme `bigme.toml:52`（`books`）和设备真名大小写不一致，**只记成发现，不建议改设备行为**（要不要改由 vinci 另定）。

## 约束
- **只读**：不执行任何命令，不写文件，不改代码。
- **不建议改设备产物**：所有建议都必须保持各设备的产物不变。任何重构都要让 `crates/library/tests/fp_golden.txt` 和所有设备的产物**逐字节不变**；做不到的不算重构，要标明是"行为改动"，并说明为什么值得。
- **clean-room**：不参考、不引用 KCC、Calibre 等 GPL 转换器的源码或实现。
- 不读密钥、`.env`、`auth*`。
- `docs/decisions.md` 里定下的事不当成问题报，除非代码和它对不上，或者它和别的文档矛盾。

## 输出格式
开头一段总评（3–5 行），然后是编号的发现，每条：

```
N. 标题
   - 位置：路径:行号（可多处）
   - 类别：重复 / 边界耦合 / 过长 / 死代码 / 错误处理 / 抽象 / 文档 / 测试缺口
   - 严重度：高 / 中 / 低
   - 改动量：小（≤20 行、单文件）/ 中（一个模块内）/ 大（跨模块）
   - 设备行为风险：无 / 低 / 中 / 高（会不会改产物、传书、删除、记录迁移），并写怎么验证没改（如 fp_golden、逐字节对拍、哪些测试）
   - 建议：怎么改（一两句）
   - 未核实：有就写「未核实」和原因
```

最后一节「建议分组」：把发现按以后的任务单分组（每组一个短名、包含哪几条编号、建议先后顺序、每组是"纯重构（产物不变）"还是"文档"还是"补测试"），小改动可以单独列成"小改动清单"。

## 后续
负责 bot 读回执，逐条核对引用位置（`未核实` 的不当成已确认），再和 vinci 一起挑出要做的，按组另开任务单。
