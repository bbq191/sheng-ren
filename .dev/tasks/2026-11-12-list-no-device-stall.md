# booklib list 在卡死的设备挂载上不卡住

- Task ID: 2026-11-12-list-no-device-stall
- 负责 bot: sheng-ren
- 状态: verified
- 执行方: box-agent
- 分支: list-no-device-stall（从最新 master 切出）
- 更新时间: 2026-11-12T16:38+08:00
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
- 执行方 / 开始–结束时间：box-agent（oc-exec） / 2026-11-12T16:19:57+08:00–2026-11-12T16:32:40+08:00
- base..head：`81e5657cde777cd8eaeee0ef5fdb0d346f373cbc..09759c0d8857e492445f6414a50e099601b37e2f`
- 提交：
  | 提交号 | 说明 |
  |---|---|
  | b90af6e | chore: 开始执行任务单 2026-11-12-list-no-device-stall |
  | 6cb7e6d | fix: list 不再 stat 可拔下设备上的产物 |
  | 37f0e5a | test: list 不读设备路径，设备状态注明未核对 |
  | 90f3101 | docs: list 设备上的状态不再核对文件 |
  | 09759c0 | test: 补电脑上产物过期仍不注未核对设备 |
- 改动文件：`.dev/tasks/2026-11-12-list-no-device-stall.md`,`crates/library/src/bin/booklib/main.rs` `crates/library/src/generate.rs`,`crates/library/tests/flow.rs` `docs/decisions.md`,`docs/usage.md`
- 测试：包装器未代跑验收命令，由负责 bot 按任务单核对
- 遇到的问题：无
- 偏离：无
- 未完成：无
- 运行记录：`/home/box/oc-runs/2026-11-12-list-no-device-stall-20261112-161957.json`
- 日志：`/home/box/oc-runs/2026-11-12-list-no-device-stall-20261112-161957.log`
- 模型：`opencode-go/deepseek-v4.1-flash`

## 审查
<!-- 负责 bot 填写 -->
- 验收：**第 1 轮通过**（sheng-ren bot，2026-11-12 16:36 +08:00，审 `81e5657..fb0fbea`，实现 `6cb7e6d`，测试 `37f0e5a`/`09759c0`，文档 `90f3101`）。没上真机；真机由 vinci 验汉王挂载卡住时 `booklib list` 马上返回。下面全是 box 上的临时目录。工具链为 rustup stable rustc 1.99.1。
  - 逐条验收标准：
    - [x] 测试证明 `list` / `outputs` 不对可拔路径做 stat / read_dir / `mounted()`。
      - 新用例 `flow.rs:137` `list_does_not_stat_removable_device_outputs`：带 `BOOKLIB_TEST_DEVICE_STAT_PANIC=1` 跑 `list`，指纹一致 → `✓ 最新 未核对设备`，改过或为空 → `⚠ 过期 未核对设备`；电脑上自定义模式 `pc` 的 ✓/⚠ 都不附这句；相对路径旧记录仍是 `? 未知`。钩子只包在 `output_unplugged` / `output_is_file`（`generate.rs:558-576`），不经过 `sync`。子进程走 `booklib_env`，有 30 秒超时。含在本轮 flow 54 里，通过。
      - 负责 bot 重做钩子自检：临时把 `let on_device = self.devices.on_removable(...)` 改成恒为假，同测试失败，子进程退出 101，报「booklib list 对设备上的产物做了 stat：…/ireader/Internal Storage/documents/甲.epub」。`git checkout` 还原后同测试通过。未提交。
      - 另用已编译的 `target/debug/booklib` 做独立检查（不进仓库）：造临时书库并生成到假 ireader 后，删掉设备上的 `甲.epub`、再把 `Internal Storage` 和 `documents` chmod 000，`list` 都在 7ms 内退出 0，仍显示 `✓ 最新 未核对设备`。
    - [x] 设备路径：指纹一致显示 ✓ 最新 未核对设备，不一致或指纹为空显示 ⚠ 过期 未核对设备；电脑上的产物不附这句；相对路径仍是 `? 未知`。见上条用例和 `cmd_list`（`main.rs:1051-1056`）。
    - [x] 上表用例已重审（执行方结果栏的对照被 oc-exec 填结果时写得很短，下面由负责 bot 按 diff 补核，没有偷偷删断言）：
      - `add_build_skip_remove`（`flow.rs:9`）：空挂载点那段改为只比指纹，ireader 改过指纹 → `Some(false)` 且 `device_unchecked`，kindle 没改 → `Some(true)`；**必须改的断言已改对**。
      - `audit_findings_are_kept_in_the_generation_record`（`:401`）：只查 `audit` 字段，未改，本轮测试通过。
      - `interrupted_move_is_finished_next_time`（`:1531`）：ireader 路径、指纹留空时 `fresh == Some(false)`，只比指纹仍对，未改 list 文案，本轮通过。
      - `removed_mode_leftovers_are_cleaned_up`（`:2737`）：相对路径旧记录 `fresh == None`（`:2808` 仍在），照规则 4，未改，本轮通过。
      - `disabled_profile_is_forgotten_by_sync`（`:2822`）：假 MTP 路径改到 `dev_dir` 下，使 `on_removable` 为真；`fresh` 改为 `Some(false)`，list 文案改为 `⚠ 过期 未核对设备`。**必须改的断言已改对**。
      - `tests/cli.rs` 没有设备路径的 `fresh` / list 文案断言，未改，cli 14 通过。
    - [x] 版本号不变；`git diff 81e5657..HEAD -- crates/library/src/deliver.rs crates/library/src/session.rs crates/bookconv` 为空。`PIPELINE_VERSION`/`OPTIMIZE_VERSION`/`COMIC_VERSION` 仍是 5/68/68；`fp_golden.txt` 相对 origin/master 无 diff，本轮 `fp_golden` 通过。
    - [x] `cargo test --workspace` 全过（0 失败）：bookbase lib 30，bookconv lib 553，csscascade lib 16，kfx lib 53，library lib 78 / bin 7 / cli 14 / flow 54 / fp_golden 1，profile lib 19。`cargo clippy --workspace --all-targets -- -D warnings` 0 警告。`cargo fmt --all -- --check` 干净。
    - [x] 结果里写了没遇到问题，但没写「没上真机」；本单需要 vinci 做真机验收，审查里补记。`list` 帮助（`main.rs:127-140`）和 `docs/usage.md` 已按单更新；`docs/decisions.md:362` 加了方案 B 那一行。
  - 代码核对：
    - `outputs`（`generate.rs:511-554`）对绝对路径先 `on_removable`（只比前缀，见 `session.rs:163-181`），设备路径跳过 `unplugged` / `is_file`，只比 `plan()` 指纹（含 `legacy`；停用的模式可回退 `profile::get`）。相对路径仍是 `fresh = None`。电脑路径仍走 `output_unplugged` / `output_is_file`。
    - `OutputStatus.device_unchecked` 在 `on_device` 时为真；`cmd_list` 据此附加「未核对设备」。不在跟踪范围时仍用原来的 `✗ 不在这台的跟踪范围` 覆盖状态（原有行为）。
    - 提交合规：中文前缀加正文、无 Co-Authored-By、执行方没有合并，工作只在任务分支上。涉及文件与预计一致，无偏离。
  - 阻断：无。
  - 建议（不挡合并）：包装器填的「结果」偏短，对照表是负责 bot 按 diff 补核的；钩子只包着 `outputs` 里那两处包装函数，`space_check` 等 sync 路径仍会 stat 设备文件，这在本单范围外。
- 二审：**无阻断**（agy-review，2026-11-12 16:38 +08:00，模型 gemini-3.8-flash-medium，用时 90s，回执 `400481e`）。结论「建议通过」。负责 bot 逐条核对引用，均与当前代码一致：
  - `generate.rs:519-544`：设备路径只做 `on_removable` 前缀判断，绕过 `unplugged`/`is_file`，只比指纹（含 legacy，停用模式可回退 `profile::get`），属实。
  - `main.rs:1051-1057`：按 `(fresh, device_unchecked)` 显示文案，属实。
  - `flow.rs:137-231`、`generate.rs:558-567`：新用例和统计钩子，属实。
  - `flow.rs:92-113`、`:2843-2927`：`add_build_skip_remove` 与 `disabled_profile_is_forgotten_by_sync` 的断言已按方案 B 改对，没有删掉该有的断言，属实。
  - `docs/decisions.md:362`、`docs/usage.md:58`、`:180-195`：文档已同步，属实。禁止文件无 diff、版本号和 fp_golden 未改，属实。
  - 线索 1（拔掉设备仍可能显示 ✓ 最新 未核对设备）：属实，这是方案 B 的既定取舍，帮助和 usage 已写明。
  - 「未核实」只有真机卡死挂载：第 1 轮测试已重跑；真机验收归 vinci。
- 合并：已合并 `e9667ea`（2026-11-12 16:39 +08:00，`--no-ff`），待最终验收。合并结果上 `cargo test --workspace` 全过。按工作流 verified（负责 bot 验收通过、二审无阻断、已合并）；真机由 vinci 最终验收：汉王挂载卡住时 `booklib list` 马上返回。
