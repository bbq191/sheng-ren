# booklib list 在卡死的设备挂载上不卡住

- Task ID: 2026-11-12-list-no-device-stall
- 负责 bot: sheng-ren
- 状态: accepted
- 执行方: box-agent
- 分支: list-no-device-stall（从最新 master 切出）
- 更新时间: 2026-11-12T16:19:57+08:00
- 引用: 2026-11-12-lock-no-device（「后续」第 1 条）

## 需求原文
> （lock-no-device 任务单「后续」）1. `booklib list` 在卡死的挂载上也卡住（`lib.outputs` 要 stat 设备上的产物）；
> （vinci 2026-11-12）后续那几条要开新任务单：要。
> （vinci 2026-11-12 13:32）list-no-device-stall 只用方案 B。
> （vinci 2026-11-12 16:18）改由 box 执行。

## 输入
- 背景：`Projects/sheng-ren/context.md`；原单 `.dev/tasks/2026-11-12-lock-no-device.md`。基于 origin/master `415161a`。
- 原因（负责 bot 读代码确认，**没在真机或 FUSE 复现上验证**）：
  - `list` 是 `lock: never`（`crates/library/src/bin/booklib/main.rs:137`），所以 lock-no-device 的修复管不到它。`cmd_list`（`main.rs:1018`）对每本书调 `lib.outputs(&m)`（`main.rs:1048`）。
  - `Library::outputs`（`crates/library/src/generate.rs:509-537`）对每条绝对路径的记录依次调 `self.devices.unplugged(&entry.path, &entry.root)`，再调 `entry.path.is_file()`，最后算 `plan()` 比指纹。
  - `Devices::unplugged`（`crates/library/src/session.rs:186-202`）在 `mount_check` 打开时（真实环境默认打开，`deliver.rs:41`）调 `deliver::mounted(&point)`（`crates/library/src/deliver.rs:142-155`），对挂载点和它的上一级各做一次 `std::fs::metadata`，之后还有 `root.is_dir()`。挂载点是 FUSE（jmtpfs）的根，卡死时 `metadata(point)` 会等 FUSE 守护进程回应。所以**第一个卡住的位置多半是 `mounted()` 的 stat，不是产物的 `is_file()`**，和原描述「stat 设备上的产物」不完全一样（中置信度：按 FUSE 语义推断，没有复现）。
  - `sync` 卡住（`deliver::connect`，`deliver.rs:106-138`，同样经过 `mounted()`，之后还有 `mtp_storage` 的 `read_dir`）是同一根源，但不在本单范围（见约束）。
  - 原单规则 3 已写：`list` 等命令「不碰设备上的产物目录」（当时只修了加锁路径，这条留给后续）。
- 已定做法：**方案 B**（vinci 2026-11-12 13:32 确认）。对可拔下设备上的产物一律不 stat，只比对记录指纹与 `plan()` 指纹，显示 ✓/⚠ 并注明「未核对设备」（显示文字为负责 bot 暂定，vinci 可改）。
- 已否决：
  - **A**（读 `/proc/self/mountinfo` 判断挂没挂上）：只避开对挂载点的 `metadata`，`root.is_dir()` 和 `entry.path.is_file()` 仍会进 FUSE，挂着但卡死的设备照样卡，解决不了本单的问题。
  - **C**（后台线程加超时）：卡死的 FUSE 调用可能留下 D 状态线程，进程退出时也可能卡住；并发复杂度高，本单不做。
- 取舍（「设备没接上」）：方案 B 不调用 `mounted()`、不 `stat` 挂载点或产物，只靠 `Devices::on_removable`（`session.rs:163-166`，纯路径前缀）判断一条记录是不是在可拔下的设备上。前缀判断分不出「没接上」和「接上了」，因此 `list` **不再根据接没接上来显示 `? 未知`**。设备路径上的记录一律按指纹显示 ✓/⚠，并注明「未核对设备」。原先「? 未知（设备没接上，或设备上那本不在了）」对设备路径不再适用：文件在不在也要 `is_file()`，同样不碰。空挂载点里正好有同名文件时，以前靠 `unplugged` + `mounted()` 避免显示「最新」（`generate.rs:515-517` 注释，agy 发现），本单放弃这项核对，改用「未核对设备」标明没查过文件。
- 涉及文件（预计）：
  - `crates/library/src/generate.rs`（修改：`outputs`；可能给 `OutputStatus` 加「未在设备上核对」的标记，供 `list` 附加说明）
  - `crates/library/src/bin/booklib/main.rs`（修改：`cmd_list` 的显示，以及 `list` 帮助 `main.rs:127-140`）
  - `crates/library/tests/flow.rs` / `tests/cli.rs`（修改 / 新增用例，见下方要重审的用例）
  - `docs/decisions.md`（新增一行）、`docs/usage.md`（`list` 的状态含义）
  - 不改 `session.rs` 的 `unplugged`、不改 `deliver.rs`（删产物、`sync` 连设备仍用原来的 `mounted()`）

## 约束
- 不做：不改 `sync` / `deliver::connect` 的连接逻辑（后续第 2 条，另议）；不改产物、生成记录格式、传书；不改加锁清理；不引入方案 A 的挂载表判断，也不做方案 C。
- 不升版本号：不影响产物，`OPTIMIZE_VERSION`、`COMIC_VERSION`、`PIPELINE_VERSION` 不动，`fp_golden.txt` 不变（`docs/development.md`「版本号」）。
- 只在上面的分支工作；不合并；改「涉及文件」以外的文件写进「结果 → 偏离」。
- 提交信息中文，`fix:` / `test:` / `docs:` 前缀 + 正文，不加 Co-Authored-By；不读密钥、`.env`、`auth*`。
- 测试一律用临时目录当设备，不碰真机。
- 风险点：**设备行为**（`list` 显示的设备状态不再核对文件是否存在、不再区分接没接上）→ **要 agy 二审**。

## 规则（要实现的行为）
1. `booklib list` 的必经路径上，对可拔下设备上的路径（`Devices::on_removable`，只看前缀）不做任何 `stat` / `read_dir` / `metadata` / `is_file` / `is_dir` / `mounted()`。
2. 这类记录：不调用 `unplugged`，不算文件在不在；用记录里的指纹和 `plan()` 的指纹比较（与现在 `outputs` 后半段相同，含 `legacy`）。一致则 `fresh = Some(true)`（✓ 最新），不一致或指纹为空则 `fresh = Some(false)`（⚠ 过期）；`plan()` 失败则仍为 `None`（? 未知）。显示时在状态后注明「未核对设备」（暂定文案，vinci 可改）。实现上可给 `OutputStatus` 增加标记，或在 `cmd_list` 里对 `on_removable` 的路径附加这句；具体写法由执行方定，写进结果。
3. 不在可拔下设备上的产物（电脑上的自定义模式等）：行为不变，仍按 `unplugged` / `is_file` / 比指纹显示 ✓/⚠，不附加「未核对设备」。
4. 相对路径的旧记录（2026-10-31 以前 Move 上的）：照旧不查、`fresh = None`，显示 `? 未知`。
5. 更新 `list` 帮助（`main.rs:127-140`）和 `docs/usage.md`：设备上的 ✓/⚠ 表示生成记录与当前计划一致，没有核对设备上文件还在不在，也不表示设备此刻接上了。

## 分步任务
1. [ ] 改 `generate.rs` 的 `outputs`：路径经 `on_removable` 判定为设备上的，跳过 `unplugged` 和 `is_file`，只比指纹（规则 2）；需要的话给 `OutputStatus` 加标记。电脑上的路径和相对路径照规则 3、4。
2. [ ] 改 `cmd_list` 和 `list` 帮助：设备路径的 ✓/⚠ 后面注明「未核对设备」。
3. [ ] 测试：仿照 `BOOKLIB_TEST_DEVICE_IO_PANIC` 的思路，让 `outputs` / `list` 一旦对可拔路径做 stat 就失败（钩子只覆盖本单改到的路径，不要误伤 `sync`）。子进程走 `common::booklib_env`，带超时。至少覆盖：设备记录指纹一致 → ✓ 最新 未核对设备；指纹改过或为空 → ⚠ 过期 未核对设备；电脑上自定义模式产物照旧能显示已核对的 ✓/⚠；相对路径仍是 `? 未知`。
4. [ ] 重审并改下表已有断言，结果里逐条写改了什么、为什么。`docs/decisions.md` 加一行（含「不再用 list 判断设备接没接上、不再核对立文件是否还在」）。`docs/usage.md` 同步。

要重审的已有用例（`crates/library/tests/flow.rs`，行号相对 `415161a`）：

| 位置 | 用例 | 现状 | 方案 B 下 |
|---|---|---|---|
| `:81-91`、`:104-111` | `add_build_skip_remove` | 设备产物 `fresh == Some(true)`，改指纹后 `Some(false)` | `fresh` 的真假多半仍对（只比指纹、测试里文件也在），但若断言了 `list` 文案或「已核对」，要加上「未核对设备」；空挂载点那段（`:113-126`）现在 `mount_check=true` 时要求全部 `fresh.is_none()`，方案 B 不再调用 `mounted()`，指纹一致时会变成 `Some(true)`，**必须改断言** |
| `:323-356` | `audit_findings_are_kept_in_the_generation_record` | 只查 `audit` 字段 | 多半不用改；跑一遍确认 |
| `:1449` | `interrupted_move_is_finished_next_time` | 指纹留空时 `fresh == Some(false)` | 只比指纹仍是 `Some(false)`，多半不用改；若经过 `list` 文案则要加注 |
| `:2703` | `removed_mode_leftovers_are_cleaned_up`（相对路径旧记录） | `fresh == None` | 照规则 4，应保持 `None` |
| `:2794-2816` | `disabled_profile_is_forgotten_by_sync` | 指向不存在 MTP 目录的旧记录 `fresh == None`，`list` 输出含 `? 未知` | 不再 `is_file()`，指纹 `"旧"` 对不上 `plan()`，应为 `Some(false)`，`list` 应是 ⚠ 过期 未核对设备，**必须改断言** |

如执行时发现 `tests/cli.rs` 或其它文件也断言了设备路径的 `fresh` / `list` 文案，一并改，写进结果。

## 验收标准
- [ ] 测试证明 `list` / `outputs` 不对可拔路径做 stat / read_dir / `mounted()`
- [ ] 设备路径：指纹一致显示 ✓ 最新 未核对设备，不一致或指纹为空显示 ⚠ 过期 未核对设备；电脑上的产物不附这句；相对路径仍是 `? 未知`
- [ ] 上表用例已重审，结果里有对照
- [ ] 版本号不变；`git diff master..HEAD -- crates/library/src/deliver.rs crates/library/src/session.rs crates/bookconv` 为空（若给 `OutputStatus` 加字段而不动 `session.rs` / `deliver.rs`）
- [ ] `cargo test --workspace` 全过；`cargo clippy --workspace --all-targets -- -D warnings` 0 警告；`cargo fmt --all -- --check` 干净
- [ ] 结果里写明没上真机。真机由 vinci 验：汉王挂载卡住时 `booklib list` 马上返回

## 交付物
代码提交到上面的分支并推送；「结果」填好；状态改为 done 或 blocked。

## 给执行方的指令
```
执行任务单 .dev/tasks/2026-11-12-list-no-device-stall.md：先把状态改为 accepted 并提交，按「分步任务」做完，填好「结果」，状态改为 done（做不下去改为 blocked 并写明原因），提交并推送分支。
```

## 结果
<!-- 执行方填写 -->
- 执行方 / 开始–结束时间：
- base..head：
- 提交：| 步骤 | 提交号 | 文件 | 说明 |
- 测试：| 命令 | 通过 / 失败 / 未跑 | 摘要 |
- 遇到的问题：
- 偏离：无
- 未完成：

## 审查
<!-- 负责 bot 填写 -->
- 验收：
- 二审：
- 合并：
