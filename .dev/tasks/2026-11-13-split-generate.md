# 拆 `library/src/generate.rs`（big-file-splits 子单 5）

- Task ID: 2026-11-13-split-generate
- 负责 bot: sheng-ren
- 状态: done
- 执行方: 待定
- 分支: task/2026-11-13-split-generate（从最新 master 切出）
- 更新时间: 2026-11-13T14:38:00+08:00
- 引用: 2026-11-13-big-file-splits 分步 5；依赖 2026-11-13-prepare-file-split（已合并 8101e7e）

## 需求原文
> （质量审查发现 14）十个核心文件单文件超过 1800 行，难审、难改。只做模块拆分、不改函数逻辑与公开签名，各设备产物及 `fp_golden` 应逐字节不变。执行时一次只拆一个文件；测试数量和结果不变。
> （2026-11-13）big-file-splits 按文件拆子单，本单只拆 `crates/library/src/generate.rs`。

## 输入
- 基于最新 origin/master（含 prepare-file-split 合并 `8101e7e`）。
- `crates/library/src/generate.rs`：按职责拆成 `generate/` 下的子模块（例如生成主流程、位置与认领、生成记录读改写、测试），`generate.rs` 或 `generate/mod.rs` 只留入口与再导出。具体分法按代码现状定，在结果里列出每个子模块放了什么。

## 约束
- 只用 Rust；clean-room；仓库不出现上游项目名及缩写。
- **只挪代码、不改行为**：不改函数体、分支条件、错误文案、写盘时机、公开签名。允许 `pub(crate) use` 再导出以保持 crate 内原有 `use` 路径，或同步改 crate 内引用。
- **只拆这一个文件**，不碰 big-file-splits 表里的其他文件，不顺手做别的重构。
- **不升版本号**；`fp_golden.txt` 一行不动；不改 `testdata/`。
- 不碰 `~/.local/share/booklib`，实验用 `target/` 下临时 `--library`；测试不碰真机。
- `install.sh` 不用 `--force`。本仓库不用 rustfmt 默认宽度，别对整个仓库跑 `cargo fmt`。
- 提交信息中文 + 前缀 + 正文，不加 Co-Authored-By。

## 规则（要实现的行为）
1. 拆完后 crate 能编译，对外 API 与行为不变；测试集合（名称、数量）与结果和拆前一致。
2. 每个子文件远低于 1800 行。

## 分步任务
1. [ ] 读 `generate.rs`，列出各段职责与相互调用，定拆法。
2. [ ] 挪代码到子模块，调整 `mod`/`use`，保证编译。
3. [ ] 拆前、拆后各跑一次 `cargo test -p library`，测试数量一致；`cargo test --workspace` 全过；`cargo clippy --workspace --all-targets -- -D warnings` 0 警告；确认版本号与 `fp_golden.txt` 没动。

## 验收标准
- [ ] `generate.rs` 已拆成合理子模块，每个文件远低于 1800 行。
- [ ] 测试数量与结果与拆前一致；workspace 测试、clippy 干净。
- [ ] 版本号和 `fp_golden.txt` 都没动；结果写明只是挪代码，没跑真书回归、没上真机，待本机回归确认产物逐字节不变。

## 交付物
代码提交到任务分支并推送；「结果」填好；状态改为 done 或 blocked。任务单状态改动推 master，带 `[skip-notify]`。不要合并，不要跑二审。

## 给执行方的指令
```
执行任务单 .dev/tasks/2026-11-13-split-generate.md：先把状态改为 accepted 并提交，按「分步任务」做完，填好「结果」，状态改为 done（做不下去改为 blocked 并写明原因），提交并推送分支。任务单 done 推 master 带 [skip-notify]。不要合并，不要跑二审。
```

## 结果
- 执行方 / 开始–结束时间：box-agent（oc-exec） / 2026-11-13T14:12:01+08:00–2026-11-13T14:30:19+08:00
- base..head：`13bd5d6330a18e18396961fbd53a9846754f126c..b1e817a65eb49a5967c8bcdd3b532eaf6d550f99`
- 提交：
  | 提交号 | 说明 |
  |---|---|
  | 4bbe173 | chore: 开始执行任务单 2026-11-13-split-generate [skip-notify] |
  | b1e817a | chore(generate): 把 generate.rs 拆成 generate/ 子模块 |
- 改动文件：`.dev/tasks/2026-11-13-split-generate.md`,`crates/library/src/generate.rs` `crates/library/src/generate/flow.rs`,`crates/library/src/generate/fsops.rs` `crates/library/src/generate/placement.rs`,`crates/library/src/generate/plan.rs` `crates/library/src/generate/produce.rs`,`crates/library/src/generate/records.rs` `crates/library/src/generate/space.rs`,`crates/library/src/generate/state.rs` `crates/library/src/generate/tests.rs`,`crates/library/src/generate/types.rs`
- 测试：包装器未代跑验收命令，由负责 bot 按任务单核对
- 遇到的问题：无
- 偏离：
  - `crates/library/src/generate/flow.rs`
  - `crates/library/src/generate/fsops.rs`
  - `crates/library/src/generate/placement.rs`
  - `crates/library/src/generate/plan.rs`
  - `crates/library/src/generate/produce.rs`
  - `crates/library/src/generate/records.rs`
  - `crates/library/src/generate/space.rs`
  - `crates/library/src/generate/state.rs`
  - `crates/library/src/generate/tests.rs`
  - `crates/library/src/generate/types.rs`
- 未完成：无
- 运行记录：`/home/box/oc-runs/2026-11-13-split-generate-20261113-141201.json`
- 日志：`/home/box/oc-runs/2026-11-13-split-generate-20261113-141201.log`
- 模型：`opencode-go/deepseek-v4.1-flash`


## 审查
<!-- 负责 bot 填写 -->
- 验收：**通过**（sheng-ren bot，2026-11-13 14:31–14:38 Asia/Shanghai，独立 worktree `/tmp/sheng-ren-review-sg` @ `d7df517`，实现提交 `b1e817a`，对照 origin/master `13bd5d6`）。**没跑真书回归、没上真机**；待 vinci 本机回归确认产物逐字节不变。按 `Projects/_templates/review-checklist.md`「项目 bot 验收」：结果已填、状态 done；分步 1–3 完成（任务单勾选未改，工作已做完）；提交中文前缀加正文、无 Co-Authored-By、执行方未合并。工具链 rustc 1.99.1。
  - 偏离：oc-exec 把 10 个新建的 `generate/` 子模块文件标成偏离（旧算法把新建文件也算进去）。逐个判断：`flow.rs`、`fsops.rs`、`placement.rs`、`plan.rs`、`produce.rs`、`records.rs`、`space.rs`、`state.rs`、`tests.rs`、`types.rs` 均为任务要求的新建子模块，合法。已有文件只改了 `crates/library/src/generate.rs`（被拆的入口，改成 44 行的模块说明、`mod` 与再导出）和任务单本身；没有范围外改动的已有文件。真实偏离：无。
  - 子模块划分（各文件远低于 1800 行）：`generate.rs` 44 行（入口与再导出）；`types.rs` 150（`PIPELINE_VERSION`、`Built`、`Step`、`Made`、`Produced`、`OutputStatus`、`Plan`、`Slot`、`RecordState`、`SpaceCheck`、`UNMOUNTED`、`State`、`StateEntry`、`Placement`）；`plan.rs` 194（指纹与计划：`plan`、`output_format`、`comic_of`、`injected_info_sig`、`own_dc_of`、`fingerprint`）；`placement.rs` 238（产物位置与认领：`output_dir`、`resolve_placement`、`settle_fresh`、`stage_registration`、`output_overlap`、`device_safe_rel`）；`flow.rs` 306（主流程：`build`、`prepare`、`prepare_file`、`transfer`、`complete`、`release_prepared*`、`direct_write`）；`produce.rs` 148（`produce`、`prepared_input`、`epub_input`、`kfx_id`）；`records.rs` 307（生成记录读写删除：`rebase_state_case`、`remove_outputs`、`hand_over_outputs`、`retract`、`outputs`、`edit_state`、`finish` 等）；`state.rs` 303（`State`/`StateEntry` 的位置改写：`migrate_prefixes`、`delete_placements`、`file_name_for` 等）；`space.rs` 94（`space_check`、`device_free`）；`fsops.rs` 118（`carry_sidecar`、`copy_dir`、`case_twin`、`case_alias`、`distinct_on_disk`、`remove_product`）；`tests.rs` 391（原 `remove_tests`、`kfx_id_tests`）。划分按职责，合理。
  - 只挪代码：从 master 的 `generate.rs` 抽出 96 个 item（fn/struct/enum/const/type/mod，含 impl 方法与测试），与新子模块 96 个一一对应，无漏无多。去可见性与空白后，83 个文本完全相同。其余 13 个差异均属允许范围：① `Slot.taken` 的类型从 `BTreeSet<String>` 写成 `std::collections::BTreeSet<String>`（use 路径，类型相同）；② 测试里 `super::foo` / `use super::{...}` 改为 `super::super::<子模块>::foo` 或文件头统一 use（断言、条件、文案未变）；③ `tests.rs` 新增模块说明注释。函数体、分支条件、错误文案、公开签名均未改。`git diff --color-moved` 与逐项比对结论一致。
  - 可见性：跨子模块需要的私有类型、字段、函数改为 `pub(super)`（如 `Made`/`Plan`/`Slot`/`RecordState`/`Placement` 及其字段，以及 `plan`、`resolve_placement`、`settle_fresh`、`stage_registration`、`produce`、`finish`、`kfx_id`、`direct_write` 等），均为允许的放宽，没有改成对外的 `pub`。原先就是 `pub` 的（`Built`、`Step`、`OutputStatus`、`SpaceCheck` 以及 `Library` 上已有的 `pub` 方法）保持 `pub`；原先 `pub(crate)` 的（`PIPELINE_VERSION`、`State`、`StateEntry`、`output_overlap`、`case_twin`、`case_alias`）由入口 `pub(crate) use` 再导出，crate 内原路径不变。`UNMOUNTED` 仍是 `pub(crate)`，放在私有子模块 `types` 里、未再导出，但 crate 内本来就只有 `flow.rs` 使用，不影响。没有把私有项导出到 crate 外。
  - 版本号未动（OPTIMIZE 69、COMIC 68、KINDLE_RULES 42、CONVERT 2、WRITER 26、PIPELINE 5）；`crates/library/tests/fp_golden.txt` 与 master 逐字节相同；testdata 未动。
  - 测试：`cargo test -p library -- --list` 两边都是 167 项。函数名与数量一致；其中 12 个单测的完整路径因放入 `generate/tests.rs`（任务要求的测试子模块）而多了一层 `tests::`（`generate::remove_tests::*` → `generate::tests::remove_tests::*`，`generate::kfx_id_tests::*` 同理），去掉此前缀后清单与 master 完全一致。`cargo test --workspace` 全过：bookbase lib 30，bookconv lib 553，csscascade lib 16，kfx lib 53，library lib 82 / bin 7 / cli 14 / flow 63 / fp_golden 1，profile lib 19。`cargo clippy --workspace --all-targets -- -D warnings` 0 警告。未对整个仓库跑 `cargo fmt`。
  - 阻断：无。
- 二审：
- 合并：
