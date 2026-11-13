# 拆 `prepare_file`，顺手整理重复的读改写

- Task ID: 2026-11-13-prepare-file-split
- 负责 bot: sheng-ren
- 状态: done
- 执行方: 待定
- 分支: task/2026-11-13-prepare-file-split（从最新 master 切出）
- 更新时间: 2026-11-13T14:10:57+08:00
- 引用: 2026-11-12-quality-audit 发现 9（主）、发现 8（可选顺手；核实结论是单独开单不值）

## 需求原文
> （质量审查发现 9，已核实 ✅）`library/src/generate.rs` 的 `prepare_file`（master `49139b5` 约 `:764-929`，约 166 行）把认领、挪位置、预登记、比较是不是一样都混在一起。
> （发现 8，核实 ⚠）「读记录—克隆—改—写回」在 `remove_outputs`、`hand_over_outputs`、`retract`、`flush_removed`、`rebase_state_case` 里各写了一遍，但 `put_entry` / `put_state` 已是公共函数，能省的样板不多，单独开单不值；可以顺手做。
> （2026-11-13）quality-audit 收尾拆单。本单是生成和传书的核心路径，必须二审。

## 输入
- 基于 origin/master `49139b5`。
- 审查建议的拆法（可按代码现状微调，行为必须不变）：`resolve_placement`（路径与历史位置）、`try_adopt_or_match_fresh`（最新判定与认领）、`stage_direct_write_registration`（MTP 直写防中断预登记与哈希核对）。
- 发现 8 若顺手做：抽出统一的「读缓存 → 克隆 → 闭包修改 → 校验并原子落盘」，不要为了抽而抽。做或不做写进结果。
- 与 `2026-11-13-big-file-splits` 都可能改 `generate.rs`：本单先做；拆大文件里的 `generate.rs` 等本单合并后再动。

## 约束
- 只用 Rust；clean-room；仓库不出现上游项目名及缩写。
- **纯重构，对外行为与产物 100% 保持一致**：认领、挪位置、预登记、比较是否相同、生成记录格式和内容都不许变。
- **不升版本号**；`fp_golden.txt` 一行不动。若测试表明产物或记录变了则停下报告。
- 不碰 `~/.local/share/booklib`，实验用 `target/` 下临时 `--library`；测试不碰真机。
- `install.sh` 不用 `--force`。本仓库不用 rustfmt 默认宽度，别对整个仓库跑 `cargo fmt`。
- 提交信息中文 + 前缀 + 正文，不加 Co-Authored-By。
- 风险点：生成和传书的核心路径 → 必须 agy 二审。

## 规则（要实现的行为）
1. 把 `prepare_file` 拆成几个单一职责的辅助函数（或等价结构），主函数只串联步骤。拆完后每个子步骤能单独读懂、便于测试。
2. 不改变任何分支条件、错误文案、写盘时机。
3. 发现 8 可选：仅在确实能去掉重复、且不让控制流更难跟的前提下整理读改写样板。

## 分步任务
1. [ ] 阅读 `prepare_file` 全部分支，列出子步骤和调用关系，再动手。
2. [ ] 拆函数，保持行为不变；需要时为各情况补 `flow`（或同级）测试。
3. [ ] （可选）整理重复的读改写；做或不做写进结果。
4. [ ] `cargo test --workspace` 全过；`cargo clippy --workspace --all-targets -- -D warnings` 0 警告；确认 `fp_golden.txt` 和版本号都没动。

## 验收标准
- [ ] `prepare_file` 不再把认领、挪位置、预登记、比较混在同一个过长函数里；行为与改前一致。
- [ ] 已有 `flow` / 生成记录相关测试全部通过；若有新情况已补覆盖。
- [ ] 版本号不变；`fp_golden.txt` 一行没动。
- [ ] `cargo test --workspace` 全过；clippy `-D warnings` 0 警告。
- [ ] 结果里写明没跑真书回归、没上真机，待本机回归确认产物与生成记录逐字节不变。必须 agy 二审。

## 交付物
代码提交到任务分支并推送；「结果」填好；状态改为 done 或 blocked。任务单状态改动推 master，带 `[skip-notify]`。不要合并，不要跑二审。

## 给执行方的指令
```
执行任务单 .dev/tasks/2026-11-13-prepare-file-split.md：先把状态改为 accepted 并提交，按「分步任务」做完，填好「结果」，状态改为 done（做不下去改为 blocked 并写明原因），提交并推送分支。任务单 done 推 master 带 [skip-notify]。不要合并，不要跑二审。
```

## 结果
- 执行方 / 开始–结束时间：box-agent（oc-exec） / 2026-11-13T13:53:54+08:00–2026-11-13T14:02:14+08:00
- base..head：`0dc43d8f44b6bddf070f0de7c6e8d9eaa972af7c..9e42dbf9c8ccae1d68f4db51e7ebf46216bfd392`
- 提交：
  | 提交号 | 说明 |
  |---|---|
  | ea9c0a9 | chore: 开始执行任务单 2026-11-13-prepare-file-split [skip-notify] |
  | 9e42dbf | chore(library): 拆开 prepare_file、统一生成记录的读改写 |
- 改动文件：`.dev/tasks/2026-11-13-prepare-file-split.md`,`crates/library/src/generate.rs` `crates/library/tests/flow.rs`
- 测试：包装器未代跑验收命令，由负责 bot 按任务单核对
- 遇到的问题：无
- 偏离：
  - `crates/library/tests/flow.rs`
- 未完成：无
- 运行记录：`/home/box/oc-runs/2026-11-13-prepare-file-split-20261113-135354.json`
- 日志：`/home/box/oc-runs/2026-11-13-prepare-file-split-20261113-135354.log`
- 模型：`opencode-go/deepseek-v4.1-flash`

## 审查
<!-- 负责 bot 填写 -->
- 验收：**通过**（sheng-ren bot，2026-11-13 14:03–14:07 Asia/Shanghai，独立 worktree `/tmp/sheng-ren-review-pfs` @ `b598ea3`，实现提交 `9e42dbf`，对照 origin/master `0dc43d8`）。**没跑真书回归、没上真机**；待 vinci 本机回归确认产物与生成记录逐字节不变。按 `Projects/_templates/review-checklist.md`「项目 bot 验收」：结果已填、状态 done；分步 1–4 完成；提交中文前缀加正文、无 Co-Authored-By、执行方未合并。工具链 rustc 1.99.1。
  - 偏离：oc-exec 把 `crates/library/tests/flow.rs` 标成偏离 1 个文件。任务单分步 2 允许「需要时为各情况补 flow 测试」。核对：只在 `interrupted_move_is_finished_next_time` 与 `legacy_device_prefix_is_migrated_in_records_only` 之间新增 `direct_write_stages_record_before_transfer`（41 行），未改任何已有断言或夹具。真实偏离：无。该新测试在 master `0dc43d8` 的 worktree（只临时应用这一处新增，不改实现）上同样通过（1 passed），证明 MTP 直写预登记与打断恢复的行为与改前一致。执行方开发中该测试曾红，是因为写成了 `complete(*t, Ok(true))` 而没先 `t.run()`，只改了测试本身。
  - `prepare_file` 拆分，逐分支对照旧代码（master `0dc43d8` 约 `:793-962`）：
    - `resolve_placement`：`output_dir`、`prev`、`copied`（以前放电脑上被用户拷上设备的文件名）、`taken`、第一次 `file_name_for(..., same_as=None)` 与改前一致。`state` 的 Rc 在函数返回时释放，等同旧代码的 `drop(state)`，发生在后续写盘之前。
    - `RecordState`：`unchanged` 为 prev 且 `!force && fingerprint` 相同；`done` 为 unchanged 且 `path.is_file()`。与旧闭包语义相同。
    - `settle_fresh`：① `done` 且 path==out：有未删旧位置就 `finish`，返回 `UpToDate`；② unchanged、done 为空、copied 有值、out 是文件，哈希为空或核对通过：认领，`finish`，返回 `Adopted`。条件、sha256、entry 字段（path/root/fingerprint/old/sha/audit）、写盘（`finish`）、早返回均一致。主函数再包成 `Step::Done`。
    - 设备中途断开：同样的 `Unmounted` 文案 `{UNMOUNTED}：{}`，不生成、不改记录。
    - `made(..., record.done)`、`direct_write`、mount、第二次 `file_name_for(..., Some(&made.src))` 一致。
    - `stage_registration`：预登记整段原样迁入。`Some(p)`：old 过滤、同一位置重建保留指纹和 sha、`same` 的哈希+大小判断、`direct && !same` 时清指纹、`fingerprint.is_empty() || old != p.old` 时 `put_entry`（空指纹则 sha 也清空）。`None`：`reserve`，仅 `direct` 才 `put_entry`。写盘时机不变。
    - 之后 `compare`、把 fingerprint 写回、`transfer`、`same` 则 `complete_typed(..., Ok(false))`、出错删 tmp，均一致。`?` 传播：子函数返回 `String`，主函数返回 `Error`，与改前相同。建议名称 `try_adopt_or_match_fresh` / `stage_direct_write_registration` 按任务单「可按代码现状微调」写成 `settle_fresh` / `stage_registration`，职责对应，可接受。
  - 发现 8（`edit_state`）：**保留，不建议拆回**。五个调用点仍是「读缓存 → 克隆 → 改 → 有变化才原子写」，与改前一致：
    - `rebase_state_case`：`changed.then_some(())`，未变不写。
    - `flush_removed`：orphans 空或 `target` 失败仍先提前返回 0；`n > 0` 才写，否则 `unwrap_or(0)` 且不写。`target()` 的 Rc 仍在 Devices 缓存里，提前丢掉局部绑定不影响。
    - `remove_outputs` / `retract`：没有该书则不写（旧 `continue` / `Ok(false)`）；有则 `remove` + `delete_into_orphans` 后写。retract 返回值 `removed.is_some()` 等同旧的 true/false。
    - `hand_over_outputs`：`to` 已有或 `from` 没有则不写；否则清指纹、改记到 `to`，再写。
    - 写盘改走 `put_state`（先 `create_dir_all` 再 `states.put`），旧代码这五处直接 `states.put`。会写盘的路径上都已有记录文件，目录本来就在，`create_dir_all` 是空操作；「没有变化不写」与失败时不更新缓存的语义不变。控制流更短，不难跟，无阻断。
  - 版本号未动（OPTIMIZE 69、COMIC 68、KINDLE_RULES 42、CONVERT 2、WRITER 26、PIPELINE 5）；`fp_golden.txt` 与 master 逐字节相同；testdata 未动。提交 `chore(library)` 可接受（纯重构）。
  - 测试：`cargo test --workspace` 全过：bookbase lib 30，bookconv lib 553，csscascade lib 16，kfx lib 53，library lib 82 / bin 7 / cli 14 / flow 63（改前 62 + 本单 1）/ fp_golden 1，profile lib 19。`cargo clippy --workspace --all-targets -- -D warnings` 0 警告。未对整个仓库跑 `cargo fmt`。
  - 阻断：无。
- 二审：**无阻断**（agy-review，2026-11-13 14:09:25 Asia/Shanghai，模型 gemini-3.8-flash-medium，用时 67s，回执 `.dev/tasks/2026-11-13-prepare-file-split.agy.md`）。结论「建议通过」。负责 bot 已读回执并核对引用：`resolve_placement` 807–846、`settle_fresh` 850–891、`stage_registration` 897–949、`prepare_file` 952–1040、`edit_state` 1235–1248，以及五个调用点（`rebase_state_case` 349–353、`remove_outputs` 456–460、`hand_over_outputs` 471–480、`retract` 490–495、`flush_removed` 526–532）均核实。新 flow 测试 1568–1606 核实。建议与线索均无。未核实项：agy 只读未跑测试（验收与合并后已重跑）；真书回归和真机仍未做（验收已标明）。agy 写「避免写盘前持锁」是措辞：释放的是缓存 State 的 Rc，不是进程锁，不影响结论。
- 合并：`8101e7e`（2026-11-13 14:09:49 Asia/Shanghai，`--no-ff` 合入当时 origin/master `0dc43d8`，另一父母 `b1a7f73`）。合并后再跑 `cargo test --workspace` 全过（计数与验收相同：bookbase lib 30，bookconv lib 553，csscascade lib 16，kfx lib 53，library lib 82 / bin 7 / cli 14 / flow 63 / fp_golden 1，profile lib 19），`cargo clippy --workspace --all-targets -- -D warnings` 0 警告。已推 origin/master。状态保持 **done**（不是 verified），**待 vinci 本机回归确认产物与生成记录逐字节不变后改 verified**。没跑真书回归、没上真机。未删除远程任务分支。
