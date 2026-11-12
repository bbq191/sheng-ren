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

## 核实与分组

负责 bot（sheng-ren）2026-11-12 对着 master `6540e28` 逐条核对回执 `.dev/tasks/2026-11-12-quality-audit.agy.md`。✅ 属实 / ⚠ 部分属实 / ❌ 不属实。结果：**✅ 10 条、⚠ 5 条、❌ 0 条**；回执里的"小改动清单"第 3 条判为 ❌（不是小改动，见下）。

### 逐条核实

| # | 发现 | 核实 | 说明（路径:行号） |
|---|---|---|---|
| 1 | 加锁时清理临时文件会读设备目录，导致命令卡住 | ✅ | `booklib vertical` 是 `lock: always`（`main.rs:205`）。`Library::lock` 本进程第一次拿锁时调 `clean_leftovers`（`lib.rs:462-469`），它对 `output_dirs()` 里记录的每个产物目录做 `clean_tmp_files`（`lib.rs:498-500`），也就是 `read_dir`（`fsutil.rs:201-209`）；`output_dirs` 不看设备挂没挂（`generate.rs:337-354` 注释原话）。汉王的 jmtpfs 卡死时，`read_dir` 就卡在那里。`vertical` 自己只读注册表，不连设备（`library/src/vertical.rs:136-190`）。**这就是 2026-11-11 卡住的原因**（代码推断，没在真机上复现）。所有加锁的命令（add、track、remove、meta 写、vertical、sync）都会走到这里。不同意回执的修法"小、单文件"：直接不清设备目录，会让 Kobo（经临时文件写）被杀进程后留下的 `.tmp-*` 再也没人清，所以设备目录要改到 `sync` 连上设备以后再清。这是设备行为改动，要开任务单。 |
| 2 | `docs/development.md` 版本表里 `WRITER_VERSION` 写的是 25 | ✅ | 表在 `development.md:105`，代码是 `kfx/src/write/mod.rs:116` = `"26"`。 |
| 3 | 设备列表过时 | ⚠ | `CONTRIBUTING.md:7` 还写"两个模式 kindle、ireader" ✅；`:44-47` 版本号清单缺 `bookconv::kepub::VERSION` ✅；`docs/architecture.md:181` 只写了 Kindle、掌阅 ✅。`README.md` 的设备表（`:25-27`）已经是对的，只有 `:31`「前提」一段没写汉王、Bigme 的挂载点，比回执说的轻。 |
| 4 | `AGENTS.md` 的分支规则和工作流冲突 | ✅ | `AGENTS.md:8`（第 4 条）、`CONTRIBUTING.md:20` 写"推新分支，不直接改 master，维护者跑完回归再合"；`Projects/工作流.md` 定的是负责 bot 验收、二审后自己合并，任务单在任务分支上提交。另外这几天的任务单都直接提交在 master，和工作流第 1 步不一致。要 vinci 定以哪个为准。 |
| 5 | 掌阅、Bigme 目录名大小写和设备不一致 | ✅ | `ireader.toml:110`、`bigme.toml:52`。按要求只记录，不改。 |
| 6 | Move 时代的 CSS 怪癖字段和清洗代码是死代码 | ⚠ | 五台内置 profile 都没开 `margin_top=false`、`display_none=false`、`em_root`、`heading_font_keywords`、`trailing_br_line`、`line_height_except_p` 等（`profiles/*.toml` 里搜不到），只有测试夹具 `profile/testdata/xochitl.toml` 用到（16 处 `fixture("xochitl")`）。但是：①`wash/box_fix.rs` 不是只给 Move 用的，掌阅的 `px_margins = false`（`ireader.toml:65`）也走这里；②这些字段是书库 `profiles/` 里自定义模式能用的公开配置（`profile/src/lib.rs:81` 一带的说明），删掉就是行为改动。哪些能删要逐个字段查，并由 vinci 定。 |
| 7 | `put_file` 一组 bool 参数容易传错 | ✅ | `deliver.rs:188`、`:216`、`:368`：`keep_src`、`compare`、`direct` 加 `mount`。2026-11-12 汉王那单的阻断就是 `direct` 没跟着改。 |
| 8 | "读记录—克隆—改—写回"重复 | ⚠ | 重复属实：`remove_outputs`（`generate.rs:411`）、`hand_over_outputs`（`:429`）、`retract`（`:453`）、`flush_removed`（`:486`）、`rebase_state_case`（`:295`）。但 `put_entry` / `put_state`（`:1079` 起）已经是公共函数，`finish` 里还有别的逻辑。能省的样板不多，可以顺手做，单独开单不值。 |
| 9 | `prepare_file` 过长 | ✅ | `generate.rs:726-889`，约 160 行，认领、挪位置、预登记、比较是不是一样都在里面。 |
| 10 | 三个大小写判断函数的关系没写清 | ✅ | `case_twin`（同一目录、只差文件名大小写）是 `case_alias`（逐段只差大小写）的特例；`distinct_on_disk` 只列一个目录，不能判断跨目录的别名（`generate.rs:1491-1552`）。只需要补注释。 |
| 11 | 控制流靠匹配错误字符串的前缀 | ✅ | `NO_SPACE`（`space.rs:7`）在 `build_run.rs:327`、`transfer.rs:228` 按前缀分派；`UNMOUNTED`（`generate.rs:1482`）在 `build_run.rs:405`；`BUSY`（`lib.rs:262`）在 `sources.rs:929`。回执引用的 `generate.rs:59` 不对（那是 `Step` 枚举）。 |
| 12 | `let _ =` 吞掉错误 | ⚠ | `deliver.rs:393`：直接写之前删 dest 没成功的话，紧接着的 `File::create` 会报错，错误不会被掩盖，补上价值低。`:280`、`:203` 是出错后"尽量改回原名"的恢复动作，本来就该忽略结果。不单独做。 |
| 13 | `kfx` 依赖整个 `bookconv` | ⚠ | 依赖属实（`kfx/Cargo.toml:11`），但 `kfx` 用到的不只是 zip 和层叠，还有 `imgopt`、`html`、`util`（`kfx/src` 里搜 `bookconv::`）。KFX 写在优化之后，依赖 `bookconv` 是正常方向，不是"倒挂"。拆开只省编译时间，不建议做。 |
| 14 | 十个文件超过 1800 行 | ✅ | 行数核对无误。`profile/src/lib.rs` 的测试从 `:1083` 起约 740 行，最容易先拆；`bookconv` 的 `wash/tests.rs`、`optimize/tests.rs` 已经是单独文件，可以照这个办法拆。 |
| 15 | 缺少"不连设备的命令不碰设备"的测试 | ✅ | 跟着 1 一起做。 |

### 建议的任务单（按先后顺序）

| 顺序 | 短名 | 包含 | 改动量 | 设备行为风险 | 怎么验证 |
|---|---|---|---|---|---|
| 1 | `lock-no-device-io`：加锁时不碰设备 | 1、15 | 小–中（`lib.rs`、`generate.rs` 或 `build_run.rs`，加测试） | 低：只改设备上残留 `.tmp-*` 什么时候清（改成 `sync` 连上设备后清，只清这台），产物、传书、记录都不变 | 新测试：设备目录不可读或计数钩子下，`vertical` / `track` / `meta` 不读设备目录；`sync` 照旧清掉设备上的 `.tmp-*`；`fp_golden` 不变；真机：汉王卡住时 `booklib vertical` 马上返回 |
| 2 | `docs-sync`：文档对齐 | 2、3、4 | 小（4 个文档） | 无 | 人工对照；`AGENTS.md` / `CONTRIBUTING.md` 的分支规则等 vinci 定了再改 |
| 3 | `deliver-put-options`：`put_file` 参数改成具名选项 | 7、10 | 中（`deliver.rs` 和调用点） | 低：纯重构，但在传书路径上 | `cargo test --workspace`（`deliver`、`flow` 的大小写和写满测试），`fp_golden` 不变，agy 二审 |
| 4 | `profile-tests-split`：把 profile 的测试拆成单独文件 | 14 的一部分 | 中（只挪代码） | 无 | `cargo test -p profile` 测试数量不变，`git diff --stat` 只有挪动 |
| 5 | `library-error-kinds`：错误前缀改成枚举 | 11 | 中（library 内跨几个文件） | 低：对外文案不变 | `cargo test --workspace`，用 `grep` 确认不再按前缀分派，提示文字逐字不变 |
| 6 | `prepare-file-split`：拆 `prepare_file`，顺带整理重复的读改写 | 9、8 | 中 | 中：生成和传书的核心路径（认领、挪位置、预登记） | 全部测试，加 `flow` 测试覆盖每一种情况；`fp_golden` 不变；必须 agy 二审 |
| 7 | `big-file-splits`：拆大文件 | 14 其余 | 大（一次一个文件） | 无（只挪代码） | 每个文件单独一张单：测试数量和结果不变、产物对拍逐字节不变 |
| 8 | `move-quirks-review`：清理 Move 时代的怪癖字段 | 6 | 大 | 行为改动（书库自定义模式里用这些字段的会变） | 先只出清单，vinci 定删哪些；删的要改测试夹具并对拍五台的产物 |

不做：12（价值低）、13（不值）、5（vinci 另定）。

### 负责 bot 可以自己做的小改动（≤20 行、单文件、不改行为，只改文档或注释）
1. `docs/development.md:105`：`WRITER_VERSION` 现值 25 → 26。
2. `CONTRIBUTING.md:7`、`:44-47`：内置模式改成五台（kindle、ireader、kobo、hanvon、bigme），版本号清单补上 `bookconv::kepub::VERSION`（同一个文件，两处）。
3. `docs/architecture.md:181`：产物位置补上汉王 `Documents/`、Kobo `books/`、Bigme `books/`。
4. `README.md:31`：「前提」补上汉王、Bigme 的 jmtpfs 挂载点（`…/mtp/hanvon`、`…/mtp/bigme`）。
5. `crates/library/src/generate.rs`：给 `case_twin` / `case_alias` / `distinct_on_disk` 补注释，写清各自管哪种情况（只改注释）。

回执的小改动第 3 条（`lib.rs:499` 删掉清理设备目录）**不算小改动**：会改变设备上残留临时文件的清理，归第 1 张任务单。第 4 条（`deliver.rs:393`）价值低，不做。

### 要 vinci 定的
- `AGENTS.md` / `CONTRIBUTING.md` 的"新分支、维护者合并"和 `Projects/工作流.md` 的"负责 bot 合并"以哪个为准；任务单提交在 master 还是任务分支。
- 第 8 张单（Move 时代的怪癖字段）要不要做。
