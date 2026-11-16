# 设备上新子目录里的残留临时文件清不掉；连接清理只删我们自己命名的临时文件

- Task ID: 2026-11-16-device-leftover-subdir
- 负责 bot: sheng-ren
- 状态: done
- 执行方: oc-exec
- 分支: task/2026-11-16-device-leftover-subdir（从 master `d36f605` 之后的最新 master 切出）
- 更新时间: 2026-11-16T14:48:40+08:00
- 引用: `2026-11-12-lock-no-device` 验收「后续」与二审线索 1；`2026-11-12-device-tmp-pid-guard`（连接清理按进程号保护）

## 需求原文
> （vinci 2026-11-16，sheng-ren 聊天）按1-4先做吧
> （第 2 项，负责 bot 同日列出的）新子目录里的残留文件：书放进新的子目录后，旧文件有时清不掉，留在设备上。
> （负责 bot 转达的要求）删设备上的文件是破坏性操作：只能删 booklib 自己管理或生成的文件（有记录的，或带我们自己的临时文件命名的），用户自己放在设备上的文件绝对不能碰；写进验收标准并有测试覆盖。测试不碰真机。

现象来源（没有真机上的报告，是读代码和二审发现的）：
- `2026-11-12-lock-no-device` 验收第 144 行：「`output_dirs()` 只列有过产物记录的目录。第一次往一个新子目录写、写到一半被杀时，那个目录不在列表里，`.tmp-*` 不会被清。master 也一样。」
- 同单二审 `lock-no-device.agy.md` 线索 1：「未记录到 state 的新子目录中的残留无法被清理」。负责 bot 当时判定属实、转后续。

## 输入（开单时 master `d36f605`，负责 bot 读代码并在 box 上复现）

### 根因
1. **只扫有记录的目录**：`Library::clean_device_leftovers`（`crates/library/src/lib.rs:565`）只清 `output_dirs()`（`crates/library/src/generate/records.rs:78`）里、在这台设备产物根目录下面的目录。`output_dirs()` 只来自生成记录（各书的位置、旧位置、`orphans`）。
2. **新书往设备上暂存写时不预登记**：`stage_registration`（`crates/library/src/generate/placement.rs:160`）对没有记录的新书，只有 MTP 直接写（`direct`）才先写一条记录；USB 存储（Kobo，`staged`）经 `fsutil::tmp_sibling` 写 `.tmp-<进程号>-<计数>-<名>` 再改名，写之前没有记录。另外大小写只差的改名（`deliver.rs:421`、`:518`）在 MTP 上也会落 `.tmp-*`。
3. 所以：第一次往一个**新子目录**写、写到一半进程被杀（或 2026-11-16 起的挂载卡死兜底退出、被放弃的传输线程），`.tmp-*` 留在一个没有任何记录的目录里。下一次 `sync` 连上时清理在生成之前，不扫这个目录；这一轮传成后记录才有这个目录，**再下一次** `sync` 才清掉。如果这期间这本书被 `untrack`、改了目录、或一直生成失败，就**永远清不掉**。
4. **顺带发现、更要紧**：`fsutil::clean_tmp_files`（`fsutil.rs:227`）删的是**所有 `.tmp-` 开头的文件**，名字里解析不出进程号的「照删」（`2026-11-12-device-tmp-pid-guard` 定的，`docs/decisions.md:363`；单测 `device_cleanup_keeps_tmp_files_of_live_owner` 断言 `.tmp-nope-1-x.epub` 要删）。用户自己放的 `.tmp-我的笔记.txt` 这类文件只要在扫到的目录里就会被删。

### box 上复现（master `d36f605` 二进制，`target/leftover-repro`，普通目录当 Kobo，`BOOKLIB_MTP_NO_MOUNT_CHECK=1`）
- 书在跟踪目录的 `新目录/` 下，设备上 `books/新目录/` 预先放一个 `.tmp-999999-0-丙.kepub.epub`（进程号不存在，模拟被杀）和一个用户文件 `.tmp-我的笔记.txt`。
- 第一次 `sync`：书传上了，两个 `.tmp-*` 都还在（这个目录没记录，不扫）。
- 第二次 `sync`：两个都被删了——**包括用户的 `.tmp-我的笔记.txt`**。

### 临时文件命名的历史
`tmp_sibling` 从 2026-09-28（`724f2c8`）起就是 `.tmp-<进程号>-<计数>-<名>`，设备直传是 10 月才有的，所以设备上 booklib 留下的临时文件都符合这个格式；收紧到只认这个格式不会漏掉我们自己的。

## 方案

| 方案 | 做法 | 好处 | 不足 |
|---|---|---|---|
| A 预登记 | USB 存储上的新书也在写之前记一条指纹为空的记录（同 MTP） | 记录一有，`output_dirs()` 就含这个目录 | 改记录语义：`list` 会多出「⚠ 过期」的半成品项；多写一次记录；书被 `untrack` 后记录去掉，目录又不在了；已有的残留管不到 |
| B 递归扫整个产物根目录 | 连上时对设备产物根目录递归找 `.tmp-*` | 什么都扫得到 | MTP 上递归读目录慢、碰卡死挂载的面大；扫的范围包括用户自己的目录 |
| **C（选这个）扫「记录里的目录 ∪ 这一轮计划放书的目录」+ 只删严格符合我们命名的** | 清理时目录集合加上这台设备这一轮要放书的目录（按跟踪设置算出的 `output_dir`，纯计算、不碰设备）；删除条件收紧成名字严格是 `.tmp-<数字>-<数字>-<非空>` 的普通文件、主人进程不在 | 正好覆盖「新子目录」；只读将要写的那几个目录；用户文件不会被误删 | 书被 `untrack` 之前那次被打断、且之后再没有书放进那个目录时仍清不掉（极少；结果里写成已知局限） |

选 C。不选 A（改记录语义、影响 `list`）；不选 B（读的范围太大、MTP 慢、还要读用户目录）。

### 和已定事项的冲突（如实写进结果，报 vinci 确认）
- `docs/decisions.md:363`（`device-tmp-pid-guard`，vinci 2026-11-12 批的方案 A）写了「名字里解析不出进程号的照删」。本单对**设备上**的连接清理收紧为「只删严格符合 `.tmp-<进程号>-<计数>-<名>` 的」，方向是误留不误删。理由：负责 bot 转达的硬要求「用户自己放在设备上的文件绝对不能碰」；booklib 自 2026-09-28 起设备上的临时文件都带进程号，收紧不会漏掉自己的。**电脑上的加锁清理（产物目录、书库目录）不在本单范围，保持原样。** `decisions.md` 新增一行写明这一条修改了 11-12 那一行的「照删」部分，原行不删。

## 规则（要实现的行为）
1. **扫哪些目录**：`clean_device_leftovers` 的目录集合 = 现有的 `output_dirs()` 里在这台设备产物根目录下面的 ∪ 这一轮这台设备要放的书的产物目录（对 `build_all` 里这台设备要生成的书——在跟踪范围、格式支持——调用现有的 `output_dir` 算法，只做路径计算，不碰设备）。都限定在产物根目录（`Target::Dir` 的 `root`）下面。不递归，只看每个目录本层。目录不在就跳过（不建、不报错）。
2. **删什么**：只删同时满足的：普通文件（不是目录、不跟符号链接）；名字严格匹配 `.tmp-<1 个以上 ASCII 数字>-<1 个以上 ASCII 数字>-<至少 1 个字符>`；名字里的进程号对应的进程不在（沿用 `/proc/<pid>` 判断）。别的一律不碰，包括 `.tmp-` 开头但不合格式的、没有进程号的、子目录。
3. **不删目录**：清空了的子目录也不删（可能是用户建的）；现有「删书时顺带删变空的目录」的行为不动。
4. **时机不变**：仍是 `sync` 连上这台设备后、生成和传输之前、每次连接一次（`clean_device_leftovers_once`），仍包在 `device_op` 主线程兜底里；目录多时每个目录 `stall::beat()` 一次。
5. **电脑上的清理**（`clean_leftovers` 的加锁清理、`clean_tmp`）不改。实现上如果 `clean_tmp_files` 是两边共用的，给它加参数或拆出设备专用的严格版本，电脑那边行为逐字节不变。
6. 产物、生成记录格式、指纹、版本号都不变。

## 分步任务
1. [ ] 核对「输入」里的位置、行号；确认除 `tmp_sibling` 之外设备上不会有别的 booklib 临时文件命名（`rg` 全仓库），结果里列出。
2. [ ] `fsutil`：严格的临时文件名判断（纯函数）和设备用的清理（规则 2）；单测覆盖各种名字。
3. [ ] `lib.rs` / `build_run.rs`：清理目录集合加上这一轮计划放书的目录（规则 1）；`clean_device_leftovers_once` 需要的话改签名，把这台设备这一轮的书传进去。
4. [ ] 集成测试（`crates/library/tests/`，临时目录当设备，子进程带超时）：
   - 书第一次放进设备上的新子目录，那里预先有一个死进程的 `.tmp-<pid>-0-<名>`：**第一次** `sync` 就清掉，书照常传上。
   - 同一目录里的用户文件 **一个都不能少、内容不变**：`.tmp-我的笔记.txt`、`.tmp-abc-1-x.epub`、`.tmp--1-x.epub`、`.tmp-1-x.epub`、`.tmp-12-34-`（名字段为空）、普通的 `笔记.txt`、名为 `.tmp-<死pid>-0-x` 的**目录**、指向别处的名为 `.tmp-<死pid>-0-y` 的符号链接（目标文件也不能动）。
   - 不在计划目录也不在记录里的用户目录（例如 `books/用户自己的/`）里的 `.tmp-<死pid>-0-x.epub` 不碰（不扫那里）。
   - 活着的进程号的 `.tmp-*` 仍然留着（沿用现有保护）。
   - 已有记录的目录照旧清（回归）。
   - Kobo（USB 存储）和 ireader（MTP）各至少一个用例。
5. [ ] 更新现有单测 `device_cleanup_keeps_tmp_files_of_live_owner` 中「名字不合规的照删」的断言（设备清理改为不删），结果里写明原因。电脑上加锁清理若共用旧函数，它的现有测试保持不变。
6. [ ] `docs/decisions.md` 新增一行（写明修改了 `device-tmp-pid-guard` 那行的「照删」部分、理由、已知局限）；`docs/usage.md`、`docs/architecture.md:232` 里描述设备上清理的句子同步。
7. [ ] `cargo test --workspace` 全过；`cargo clippy --workspace --all-targets -- -D warnings` 0 警告；版本号、`fp_golden.txt`、指纹构成都没动。

## 验收标准
- [ ] **只删 booklib 自己的文件**：设备清理只删名字严格符合 `.tmp-<进程号>-<计数>-<名>`、主人进程不在的普通文件；用户文件（上面列的各种名字、目录、符号链接、普通文件）在测试里逐个断言仍在且内容不变。
- [ ] **只扫该扫的目录**：记录里的目录和这一轮计划放书的目录；别的用户目录里即使有合格式的 `.tmp-*` 也不碰（测试断言）。
- [ ] 新子目录里的残留在**第一次** `sync` 连上时就清掉（测试断言，Kobo、ireader 各一）。
- [ ] 不删目录（测试断言清理后新子目录本身还在）。
- [ ] 电脑上的加锁清理行为不变（现有测试不改）。
- [ ] 产物逐字节不变（负责 bot 用改动前 master 二进制对照）；版本号、`fp_golden.txt`、指纹构成没动。
- [ ] 文档同步；`decisions.md` 写清和 11-12 那行的关系。
- [ ] `cargo test --workspace` 全过；clippy `-D warnings` 0 警告。
- [ ] 结果里写明没跑真书回归、没上真机。

## 不做什么
- 不递归扫整个设备，不读计划和记录以外的目录。
- 不删空目录，不删任何不符合严格命名的文件。
- 不预登记 USB 存储上的新书（不改记录语义）。
- 不改电脑上的加锁清理；不改清理时机；不改产物、记录格式、指纹、版本号。
- 不处理「被打断后书立刻被 untrack、那个目录再也不放书」的残留（已知局限，写进文档）。

## 约束
- 只用 Rust；clean-room；仓库不出现上游项目名及缩写。
- 书的内容一个字不多一个字不少；**产物逐字节不变**。
- 不碰 `~/.local/share/booklib`、`~/Documents/ereader/books`、真机；实验用 `target/` 下临时 `--library`；测试不碰真设备、不联网，子进程测试都带超时。
- `install.sh` 不用 `--force`。别对整个仓库跑 `cargo fmt`。
- 只在任务分支上工作，不合并；改「涉及文件」以外的写进「结果 → 偏离」。
- 提交信息中文 + 前缀 + 正文写原因，不加 Co-Authored-By。
- 风险点：设备上删文件（破坏性）→ 要 agy 二审。

## 涉及文件（预计）
- `crates/library/src/fsutil.rs`
- `crates/library/src/lib.rs`、`crates/library/src/build_run.rs`
- `crates/library/src/generate/placement.rs`（如需把 `output_dir` 开放给清理用）
- `crates/library/tests/`（新用例，可新建 `tests/leftover.rs`）、`crates/library/tests/common/mod.rs`（如需要）
- `docs/decisions.md`、`docs/usage.md`、`docs/architecture.md`

## 结果
- 执行方 / 开始–结束时间：box-agent（oc-exec） / 2026-11-16T14:41:09+08:00–2026-11-16T14:48:40+08:00
- base..head：`c5b611e060000ef45039819f4b832f41cd2e9da7..9cc69aedc3e6c4d21ca418f202ac41b10801f142`
- 提交：
  | 提交号 | 说明 |
  |---|---|
  | daa1a6c | chore: 开始执行任务单 2026-11-16-device-leftover-subdir [skip-notify] |
  | 9cc69ae | fix(设备清理): 新子目录的残留临时文件也清，只删我们自己命名的 |
- 改动文件：`.dev/tasks/2026-11-16-device-leftover-subdir.md`,`crates/library/src/build_run.rs` `crates/library/src/fsutil.rs`,`crates/library/src/generate/placement.rs` `crates/library/src/lib.rs`,`crates/library/tests/leftover.rs` `docs/architecture.md`,`docs/decisions.md` `docs/usage.md`
- 测试：包装器未代跑验收命令，由负责 bot 按任务单核对
- 遇到的问题：无
- 偏离：无
- 新建文件（线索）：`crates/library/tests/leftover.rs`（集成测试；负责 bot 验收时补记）
- 未完成：无
- 运行记录：`/home/box/oc-runs/2026-11-16-device-leftover-subdir-20261116-144109.json`
- 日志：`/home/box/oc-runs/2026-11-16-device-leftover-subdir-20261116-144109.log`
- 模型：`opencode-go/deepseek-v4.1-flash`
- 用时：实际运行 7.5 分钟（墙钟 451s / 实际运行 451s；超时按实际运行计）

## 审查
<!-- 负责 bot 填写 -->
- 验收：通过（负责 bot 独立验收，2026-11-16，独立 worktree）。逐条核对验收标准：
  - **只删 booklib 自己的文件**：`fsutil::device_tmp_owner` 严格解析 `.tmp-<数字>-<数字>-<非空>`（非 UTF-8、进程号溢出 u32 都返回 `None`、不删）；`clean_device_tmp_files` 用 `DirEntry::file_type()`（不跟符号链接）只认普通文件，主人进程在（`/proc/<pid>`）就留；不递归、不删目录。单测 `device_tmp_name_is_strict` 和改过的设备清理单测、集成测试 `tests/leftover.rs` 逐个断言用户文件（`.tmp-我的笔记.txt`、`.tmp-abc-1-x.epub`、`.tmp--1-x.epub`、`.tmp-1-x.epub`、`.tmp-12-34-`、`笔记.txt`）仍在且内容不变，名字合规的目录、符号链接（含目标文件）不动，活进程的临时文件留着。
  - **只扫该扫的目录**：`clean_device_leftovers` = 记录目录 ∪ `planned_output_dirs`，`starts_with(root)` 过滤后去重；`planned_output_dirs` 只按跟踪设置和记录算路径、不碰设备，只算 `supported()` 且 `wants()` 的书。`mtp_leaves_unplanned_user_directory_alone` 和 Kobo/ireader 用例都断言计划外用户目录里合格式的 `.tmp-*` 不动。
  - **第一次 sync 就清**：Kobo、ireader 各一个用例，断言第一次 sync 前这台没有生成记录。box 上复现对照（`target/leftover-repro2`，Kobo，新子目录预放死进程临时文件 + `.tmp-我的笔记.txt` + 计划外目录的合格式临时文件）：master 第一次 sync 残留还在、第二次把残留和用户的 `.tmp-我的笔记.txt` 一起删了；本分支第一次就清掉残留，两次 sync 后用户文件和计划外目录都在。
  - 清理仍在 `device_op` 里、生成和传输之前、每次连接一次；每删一个 `stall::beat()`。电脑上的加锁清理（`clean_tmp`、`clean_tmp_files`）没改。
  - 产物：改动前 master（`c5b611e`）与本分支二进制对 2 本小书 `sync --device=ireader,kobo`（普通目录当设备、`BOOKLIB_MTP_NO_MOUNT_CHECK=1`）共 4 份产物逐字节相同，输出除路径外相同；kindle 要挂载检查、普通目录接不上，没对照（同是 USB 存储路径，与 Kobo 同一段代码）。版本号、`fp_golden.txt`、指纹构成都没动；没有新出现上游项目名。
  - 文档：`architecture.md`、`usage.md` 同步；`decisions.md` 新增一行，写明修改 11-12 `device-tmp-pid-guard` 行里「名字不合规的照删」（设备上改为不碰），原行未删——**这是改了一条已定决策，等 vinci 确认**。
  - 发现并修正（fix 提交 `5378dd5`）：`--watch` 同一连接后续轮次不清理，但每轮仍把所有书的计划目录算一遍；改为传闭包，真要清时才算，行为不变。
  - `cargo test --workspace` 864 通过 0 失败；clippy `-D warnings` 0 警告。
  - 已知局限（任务单「不做什么」已列）：书被打断后立刻 `untrack`、那个目录再不放书时，残留仍清不掉。
  - 没跑真书回归、没上真机；设备一律用 `target/` 下的临时目录模拟。
- 二审：agy-review（gemini-3.8-flash-medium）建议通过，无阻断；见 `2026-11-16-device-leftover-subdir.agy.md`。建议两条：「新建文件」漏记 `tests/leftover.rs`（已补）；改了 11-12 已定决策，需 vinci 确认（已在 decisions.md 标明，留给 vinci）。线索两条（进程号复用时残留晚清、计划目录与跟踪范围一致）都是预期行为。
