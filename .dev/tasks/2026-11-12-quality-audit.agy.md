## 审查总评

本次审查覆盖 `sheng-ren` master（`e954de4`）的全部 6 个 crate（约 8.2 万行 Rust 代码）及全套核心文档。项目架构清晰、工程纪律严明，指纹防回归网（`fp_golden`）与 clean-room 约束执行较好；但核心痛点集中在：**本地锁机制越权扫描远程设备导致命令卡死**（已准确定位 2026-11-11 汉王挂起根因）、**已废弃设备（Move/xochitl）在清洗层与 profile 遗留大量僵尸逻辑**、**`library` crate 深度依赖字符串匹配错误处理（Stringly-typed Errors）**、**超长文件（10 个文件超 1800 行）与复杂多 bool 参数缺乏结构化抽象**，以及**文档未及时同步最新支持设备与写出器版本号**。

---

## 编号发现

### 1. 进程锁清理残留临时文件越权扫描远程设备导致命令挂起
- **位置**：`crates/library/src/lib.rs:462-502`、`crates/library/src/generate.rs:338-354`、`crates/library/src/fsutil.rs:201-209`、`crates/library/src/bin/booklib/main.rs:205`
- **类别**：边界耦合 / 阻断
- **严重度**：高
- **改动量**：小（≤20 行、单文件）
- **设备行为风险**：低。仅影响本进程加锁时的清理范围，不改动产物生成、指纹与传输。验证办法：`cargo test -p library`，并在 MTP 模拟脱机环境下运行 `booklib vertical` 验证零设备 IO。
- **建议**：`clean_leftovers()` 中的 `clean_tmp_files` 不应扫描生成记录中的外部设备目录（或在非生成传输类纯元数据命令如 `vertical`/`track` 下跳过产物目录清理；或严格限定只清理电脑本地产物目录，远程 MTP 临时文件交由设备传输连接生命周期管理）。
- **核实结论**：**已彻底排查并证实**。2026-11-11 vinci 运行 `booklib vertical --on` 卡在汉王 MTP 上的根本原因：`vertical` 命令声明为 `lock: always`（`main.rs:205`）；`Library::lock()` 首次拿锁必定调用 `clean_leftovers()`（`lib.rs:466`）；该函数遍历 `self.output_dirs()`（包含各设备生成记录中的产物目录，如 `/run/user/1000/mtp/hanvon/...`），并对其执行 `fsutil::clean_tmp_files(&dir)`，直接调用 `std::fs::read_dir(dir)` 阻塞在未响应的 MTP FUSE 文件系统上。

---

### 2. `docs/development.md` 版本号速查表与代码实际版本号矛盾
- **位置**：`docs/development.md:105`、`crates/kfx/src/write/mod.rs:116`
- **类别**：文档
- **严重度**：高
- **改动量**：小（单文件改动 1 行）
- **设备行为风险**：无。纯文档修正。
- **建议**：将 `docs/development.md:105` 中 `kfx::write::WRITER_VERSION` 的现值从 `25` 更新为 `26`（对齐 2026-11-11 维护者定流式文字书翻页方向时的版本提升），消除“现值只写在这张表里”的自相矛盾。

---

### 3. `CONTRIBUTING.md` 等核心文档设备列表严重过时
- **位置**：`CONTRIBUTING.md:7, 45-47, 58`、`README.md:31`、`docs/architecture.md:181`
- **类别**：文档
- **严重度**：中
- **改动量**：中（修改 3 处文档）
- **设备行为风险**：无。纯文档修正。
- **建议**：更新 `CONTRIBUTING.md`，将“两个模式：kindle、ireader”更正为现行五个内置模式（补上 `kobo`、`hanvon`、`bigme`），并在版本号清单补入 `kepub::VERSION`；更新 `docs/architecture.md:181` 与 `README.md:31`，补全汉王（`Documents/`）、Kobo（`books/`）、Bigme（`books/`）的挂载与产物路径规则。

---

### 4. `AGENTS.md` 分支规则与任务单工作流冲突
- **位置**：`AGENTS.md:4`、`CONTRIBUTING.md:20`
- **类别**：文档
- **严重度**：低
- **改动量**：小
- **设备行为风险**：无。
- **建议**：将 `AGENTS.md` 第 4 条中“推新分支，不直接改 master”补充注明：“在任务单流程（`.dev/tasks/<ID>.md`）内按项目工作流执行”，消除与负责 bot 在 master/任务分支直接演进规则的语义分歧。

---

### 5. `ireader.toml` 与 `bigme.toml` 产物目录大小写与设备真机目录不一致
- **位置**：`crates/profile/profiles/ireader.toml:110`、`crates/profile/profiles/bigme.toml:52`
- **类别**：文档 / 边界耦合
- **严重度**：低
- **改动量**：无（按要求仅记录发现，不建议改动设备行为）
- **设备行为风险**：高（若改动会触发设备端目录迁移或重复目录）。
- **建议**：保持现有配置不变。汉王此前已在 `hanvon.toml:42` 改为 `Documents` 并引入 `rebase_state_case` 迁移；掌阅（`documents` vs `Documents`）与 Bigme（`books` vs `Books`）依赖底层 Android 存储不分大小写特性正常工作，维持现状由维护者另定。

---

### 6. 已废弃 Move/xochitl 设备在 `CssQuirks` 与清洗层残留近 20 个僵尸怪癖逻辑
- **位置**：`crates/profile/src/lib.rs:179-300`、`crates/bookconv/src/wash/box_fix.rs:1-2009`、`crates/bookconv/src/wash/top_spacer.rs:1-366`、`crates/bookconv/src/wash/root_em.rs:1-176`、`crates/bookconv/src/wash/heading_div.rs:1-140`、`crates/bookconv/src/wash/inline_css.rs:1-340`
- **类别**：死代码 / 抽象
- **严重度**：中
- **改动量**：大（跨 crate）
- **设备行为风险**：低。现存 5 台内置设备全部走默认值（`padding=true`, `borders=true`, `margin_top=true`, `display_none=true` 等），仅在单测 `profile::fixture("xochitl")` 中激活。验证办法：`cargo test --workspace`，比对 `fp_golden.txt` 与全部设备产物。
- **建议**：现阶段保留 `Profile` 字段反序列化兼容，但在 `bookconv/src/wash/` 中将完全为 Move 服务的极端补丁（如不画边框插横线图、单格表格包裹、页首垫透明空白图等）做隔离标注或下沉，后续开辟专门任务单清理孤立代码。

---

### 7. `library/src/deliver.rs` 中 `put_file` 系列函数的布尔参数盲区（Boolean Blindness）
- **位置**：`crates/library/src/deliver.rs:188-214, 216-255, 368-374`
- **类别**：抽象 / 重复
- **严重度**：中
- **改动量**：中（`deliver.rs` 及调用点）
- **设备行为风险**：无。纯内部参数重构。验证办法：`cargo test -p library` 确保传输与大小写覆盖用例全部通过。
- **建议**：将 `keep_src: bool`, `compare: bool`, `direct: bool`, `mount: Option<&Path>` 封装为显式命名结构体（如 `PutOptions<'a>`）或枚举模式，消除参数位传递顺序颠倒的风险（曾导致 2026-11-12 汉王任务单阻断）。

---

### 8. `library/src/generate.rs` 中“读记录—克隆—修改—写回”在 7 个方法中重复
- **位置**：`crates/library/src/generate.rs:417-429` (`remove_outputs`)、`435-449` (`hand_over_outputs`)、`453-465` (`retract`)、`486-506` (`flush_removed`)、`1079-1083` (`put_entry`)、`1087-1113` (`finish`)、`295-318` (`rebase_state_case`)
- **类别**：重复 / 抽象
- **严重度**：中
- **改动量**：中（`generate.rs` 内部）
- **设备行为风险**：无。生成记录逻辑与输出逐字节一致。验证办法：`cargo test -p library`。
- **建议**：在 `Library` 或 `Records` 内部抽出 `mutate_state<F>(&self, sp: &Path, f: F) -> Result<(), String>` 辅助方法，统一“从缓存读出 -> 克隆 -> 闭包变异 -> 校验并原子落盘”流程，消除重复样板代码。

---

### 9. `library/src/generate.rs` 中 `prepare_file` 函数过长且职责高度耦合
- **位置**：`crates/library/src/generate.rs:726-889`（约 164 行）
- **类别**：过长 / 边界耦合
- **严重度**：中
- **改动量**：中（单模块内重构）
- **设备行为风险**：无。验证办法：`cargo test -p library` 及 `fp_golden` 对拍。
- **建议**：拆分为独立子步骤：`resolve_placement`（路径与历史位置决策）、`try_adopt_or_match_fresh`（最新判定与文件认领）、`stage_direct_write_registration`（MTP 直写防中断预登记与哈希核对），降低单个函数的圈复杂度。

---

### 10. `case_twin` / `case_alias` / `distinct_on_disk` 语义边界重叠且缺乏跨目录支持
- **位置**：`crates/library/src/generate.rs:1508-1552`、`crates/library/src/deliver.rs:198-213`
- **类别**：边界耦合 / 抽象
- **严重度**：低
- **改动量**：小
- **设备行为风险**：低。验证办法：运行 `deliver` 与 `generate` 的大小写测试用例（`case_only_rename_keeps_current` 等）。
- **建议**：理清层级关系：`case_twin` 仅是 `case_alias` 在同父目录下的约束特例；`distinct_on_disk` 当前只检查单层父目录，跨目录大小写别名（如 `documents` 与 `Documents`）无法用其判定，应在注释和函数签名中明确限定其适用域。

---

### 11. `library` 依赖“字符串前缀匹配”进行业务流程控制（Stringly-typed Errors）
- **位置**：`crates/library/src/lib.rs:261` (`BUSY`)、`crates/library/src/generate.rs:1482` (`UNMOUNTED`)、`crates/library/src/space.rs:30` (`NO_SPACE`)、`crates/library/src/generate.rs:59`
- **类别**：错误处理 / 边界耦合
- **严重度**：中
- **改动量**：中
- **设备行为风险**：无。外部展示字符串可通过 `Display` 维持 100% 不变。验证办法：`cargo test -p library`。
- **建议**：在 `library` 内部定义受限错误枚举（如 `TransferErrorKind::{Unmounted, NoSpace, Busy, Io}`），业务分派按枚举判定，对外打日志或返回时统一渲染为原字符串常量，消除文案变动引发控制流静默断裂的隐患。

---

### 12. `deliver.rs` 与 `generate.rs` 存在 `let _ =` 静默吞错
- **位置**：`crates/library/src/deliver.rs:393`、`crates/library/src/deliver.rs:280`、`crates/library/src/deliver.rs:203`
- **类别**：错误处理
- **严重度**：低
- **改动量**：小
- **设备行为风险**：低。验证办法：`cargo test -p library`。
- **建议**：在 `deliver.rs:393` 的 `direct` 模式写入前，`let _ = std::fs::remove_file(dest)` 若遇到除 `NotFound` 外的权限或只读错误时应记录或抛出，避免破坏后续写入且掩盖真实错误。

---

### 13. `kfx` crate 依赖过宽（直接依赖 51k 行的 `bookconv`）
- **位置**：`crates/kfx/Cargo.toml:11`、`crates/kfx/src/lib.rs:12-13`
- **类别**：边界耦合
- **严重度**：低
- **改动量**：大（跨 crate 依赖整理）
- **设备行为风险**：无。
- **建议**：`kfx` 仅需 EPUB 容器解包与 CSS 层叠计算，未来可考虑将 `bookconv::epubzip`/`epubbook` 下沉为基础件，解除 `kfx` 对整个清洗/图片优化大包的倒挂依赖。

---

### 14. 多个核心文件单文件体量过大（≥1800 行）
- **位置**：
  - `crates/bookconv/src/imgopt.rs`（3916 行）
  - `crates/csscascade/src/lib.rs`（3060 行）
  - `crates/bookconv/src/wash/box_fix.rs`（2009 行）
  - `crates/kfx/src/write/parse.rs`（2002 行）
  - `crates/library/src/generate.rs`（1963 行）
  - `crates/library/src/bin/booklib/main.rs`（1887 行）
  - `crates/bookconv/src/htmlproc/footnote.rs`（1879 行）
  - `crates/bookconv/src/wash/kindle_rules.rs`（1856 行）
  - `crates/library/src/metadata.rs`（1848 行）
  - `crates/profile/src/lib.rs`（1824 行）
- **类别**：过长
- **严重度**：中
- **改动量**：大
- **设备行为风险**：无。若仅做文件模块拆分、不改函数逻辑与公开签名，各设备产物及 `fp_golden` 逐字节不变。验证办法：`cargo test --workspace`、`git diff` 验证无逻辑变更。
- **建议**：
  - `imgopt.rs`：拆分为 `page8.rs`（SIMD/缓冲操作）、`comic.rs`（漫画处理流）、`thumb.rs`（封面缩略图）；
  - `csscascade/src/lib.rs`：将简写展开、计算值求解、选择器匹配拆为子模块；
  - `booklib/main.rs`：将各子命令的 `cmd_*` 提取到 `subcommands/` 子目录；
  - `profile/src/lib.rs`：测试代码（占近 700 行）移入 `tests/` 或独立测试模块。

---

### 15. 纯元数据命令操作与脱机设备容错测试缺口
- **位置**：`crates/library/tests/`、`crates/library/src/lib.rs:462`
- **类别**：测试缺口
- **严重度**：中
- **改动量**：中
- **设备行为风险**：无。纯补测试。
- **建议**：在 `crates/library/tests/` 补充集成测试：
  1. 验证 `booklib vertical`、`track`、`untrack` 在任何设备挂载点不可用/脱机或无响应时，绝不尝试访问挂载路径；
  2. 验证 `Library::lock()` 时不会扫描设备挂载点目录。

---

## 建议分组

### 第一组：【阻断修复】加锁防设备挂起与参数重构（纯重构，产物不变）
- **优先级**：P0（最高，解决生产环境真机卡死隐患）
- **包含编号**：1, 7, 8, 12
- **性质**：纯重构（各设备产物逐字节不变、`fp_golden` 不变）
- **主要内容**：
  - 修正 `Library::clean_leftovers()`：避免扫描 `output_dirs()` 中的远程/离线设备路径；
  - 将 `deliver::put_file` 的多 bool 参数封装为选项结构体；
  - 在 `generate.rs` 引入 `mutate_state` 消除“读-改-写”重复；
  - 完善 `remove_file` 错误检查。

### 第二组：【文档修正】文档与版本号全面对齐（文档）
- **优先级**：P1
- **包含编号**：2, 3, 4, 5
- **性质**：文档更新
- **主要内容**：
  - 更新 `docs/development.md` 版本号表中 `WRITER_VERSION` 为 26；
  - 更新 `CONTRIBUTING.md`、`README.md`、`docs/architecture.md` 中的内置设备清单（补入 Kobo、汉王、Bigme）及路径规则；
  - 对齐 `AGENTS.md` 的分支工作流说明。

### 第三组：【架构瘦身】错误类型结构化与大文件模块化拆分（纯重构，产物不变）
- **优先级**：P2
- **包含编号**：9, 10, 11, 14
- **性质**：纯重构（代码拆分与错误重构，对外行为与产物 100% 保持一致）
- **主要内容**：
  - `prepare_file` 函数拆分为 3 个子步骤；
  - `library` 错误枚举化（替换字符串前缀匹配控制流）；
  - 模块化拆分 `imgopt.rs`、`csscascade/src/lib.rs`、`booklib/main.rs`。

### 第四组：【安全网】补充设备隔离与容错测试（补测试）
- **优先级**：P2
- **包含编号**：15
- **性质**：补测试
- **主要内容**：
  - 补充 `vertical`/`track` 纯元数据命令零设备访问回归测试；
  - 补充锁清理在 MTP 模拟故障目录下的安全跳过测试。

### 第五组：【后续研讨】废弃 Move/xochitl 残留清理与依赖优化（行为改动/需维护者定夺）
- **优先级**：P3
- **包含编号**：6, 13
- **性质**：需维护者评估（清理 Move 怪癖字段可能涉及测试用例重整）
- **主要内容**：评估是否保留 `testdata/xochitl.toml` 及 `CssQuirks` 中专为 Move 设计的近 20 个死代码字段。

---

## 小改动清单（可即刻实施）

1. **`docs/development.md:105`**：将 `WRITER_VERSION` 的 `25` 改为 `26`（1 行改动）。
2. **`CONTRIBUTING.md:7`**：补充 `kobo`、`hanvon`、`bigme`，修正“两个模式”字样（1 行改动）。
3. **`crates/library/src/lib.rs:499`**：在 `clean_leftovers()` 中，加锁清理范围仅限 `self.root` 下的本地目录，移除对 `self.output_dirs()` 中远程设备路径的遍历，直接解决命令卡死汉王 MTP 问题（3 行改动）。
4. **`crates/library/src/deliver.rs:393`**：将 `let _ = std::fs::remove_file(dest)` 改为对非 NotFound 错误进行显式记录（2 行改动）。
