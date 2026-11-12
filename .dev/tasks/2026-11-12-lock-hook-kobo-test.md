# 加锁不读设备的测试钩子真正生效，并补 Kobo（usbms）连接清理的自动化用例

- Task ID: 2026-11-12-lock-hook-kobo-test
- 负责 bot: sheng-ren
- 状态: verified
- 执行方: opencode-local
- 分支: lock-hook-kobo-test（从最新 master 切出）
- 更新时间: 2026-11-12T16:14+08:00
- 引用: 2026-11-12-lock-no-device（「审查 → 二审」建议 1、2；「后续」第 3、4 条）

## 需求原文
> （lock-no-device 二审，负责 bot 结论）建议 1（测试钩子永远不会触发……实际是 `lib.rs:511-545`）：属实。不影响功能，转后续单：把检查挪到跳过之前，或者放进 `fsutil::clean_tmp*`。
> 建议 2（缺 Kobo usbms 的自动化用例）：属实，第 1 轮已手工验过，转后续。
> （vinci 2026-11-12）后续那几条要开新任务单：要。

## 输入
- 背景：`Projects/sheng-ren/context.md`；原单 `.dev/tasks/2026-11-12-lock-no-device.md` 与回执 `.agy.md`。基于 origin/master `415161a`。
- 原因（负责 bot 读代码确认）：
  - `clean_tmp_on_computer`（`crates/library/src/lib.rs:511-517`）和 `clean_tmp_files_on_computer`（`lib.rs:520-526`）第一行都是 `if !self.cleanup_path_allowed(dir) { return; }`，之后才调 `panic_if_test_device_read`（`lib.rs:535-544`）。钩子只在 `on_removable(dir)` 为真时 panic，而走到它的路径 `on_removable` 一定为假，所以**永远不会触发**。`clean_leftovers` 开头对书库根的跳过（`lib.rs:486-488`）也没有钩子。
  - 现有集成用例 `lock_never_reads_device_dirs_and_sync_cleans_their_tmp_files`（`crates/library/tests/flow.rs:3010-3136`）带钩子跑 vertical/track/add/meta/remove，断言设备上 `.tmp-*` 还在。因为钩子不触发，用例实际证明的只是"没删"，没有证明"没读"。
  - **注意：照原话"把检查挪到跳过之前"不可行**：`output_dirs()` 本来就包含设备目录，挪到跳过之前后，正确跳过的路径也会 panic，测试会永远失败。钩子必须放在真正发生 I/O 的地方，也就是 `fsutil::clean_tmp`（`crates/library/src/fsutil.rs:188-199`）和 `fsutil::clean_tmp_files`（`fsutil.rs:203-211`）的 `read_dir` 之前。`fsutil` 不知道设备前缀（`Devices::on_removable` 在 `crates/library/src/session.rs:163`），所以要传一个判定，或者由调用方包一层。
  - Kobo：`direct_write`（`crates/library/src/generate.rs:1486-1488`）对 `usbms_base` 下的目标返回 false，所以写 Kobo 时经过 `fsutil::tmp_sibling`（`deliver.rs:199` 等处）落 `.tmp-<进程号>-<计数>-<名>`，这才是设备上会残留 `.tmp-*` 的主要路径。现有集成用例只覆盖 ireader（MTP）。测试夹具已经有 Kobo 存储目录的造法：`dev.join("usbms/KOBOeReader")`（`flow.rs:2846`、`flow.rs:3156`），`usbms_base` 是 `<dev>/usbms`（`flow.rs:97`）；Kobo profile 的 `[deliver] kind = "usbms"`、`label = "KOBOeReader"`、`dir = "books"`（`crates/profile/profiles/kobo.toml`）。
- 涉及文件（预计）：
  - `crates/library/src/fsutil.rs`（修改：`clean_tmp` / `clean_tmp_files` 在 `read_dir` 前调用一个"可不可以读"的判定，或新增带判定的变体）
  - `crates/library/src/lib.rs`（修改：钩子挪到实际 I/O 处，删掉 `clean_*_on_computer` 里的死调用；`clean_device_leftovers` 不带钩子）
  - `crates/library/tests/flow.rs`（修改 / 新增用例）

## 约束
- 不做：不改加锁清理和连接清理的**行为**（哪些目录清、何时清、只删 `.tmp-*` 文件，都不变）；不改 `deliver.rs`、`generate.rs`、`session.rs`、`build_run.rs`（如确需改，写进「结果 → 偏离」）；不改文档。
- 不升任何版本号：本单不影响产物（`docs/development.md`「版本号」一节只在改了影响产物的代码时才加），`OPTIMIZE_VERSION`、`COMIC_VERSION`、`PIPELINE_VERSION` 不动，`fp_golden.txt` 一行不变。
- 只在上面的分支工作；不合并；改「涉及文件」以外的文件写进「结果 → 偏离」。
- 提交信息中文，`test:` / `fix:` 前缀 + 正文，不加 Co-Authored-By；不读密钥、`.env`、`auth*`。
- 测试一律用临时目录当设备（`tests/common`：`BOOKLIB_MTP_DIR`、`BOOKLIB_MTP_NO_MOUNT_CHECK`），不碰真机。
- 风险点：只改测试钩子和测试，不改设备行为；但钩子落在清理函数里、挨着设备 I/O，**建议仍走 agy 二审**（负责 bot 判断：中）。

## 规则（要实现的行为）
1. 钩子 `BOOKLIB_TEST_DEVICE_IO_PANIC=1` 打开时，加锁清理**真要对可拔下设备上的路径做 `read_dir`** 就 panic；正确跳过的路径不 panic。环境变量名保持不变。
2. 连接清理（`clean_device_leftovers`，`lib.rs:548-557`）本来就要读设备，不受钩子影响。
3. 钩子必须**能被证明会触发**：要有一个用例故意让清理读设备路径，断言 panic（或子进程非 0 退出）。

## 分步任务
1. [ ] `fsutil.rs`：给 `clean_tmp` / `clean_tmp_files` 加一个读目录前的判定（例如参数 `guard: impl Fn(&Path)`，或另写 `clean_tmp_guarded`），在 `read_dir` 之前调用。具体签名由执行方定，写进结果。
2. [ ] `lib.rs`：`clean_tmp_on_computer` / `clean_tmp_files_on_computer` 把 `panic_if_test_device_read` 作为判定传给 fsutil。跳过逻辑照旧，删掉现在那两处走不到的直接调用；`clean_device_leftovers` 不带钩子。
3. [ ] 单元测试（`fsutil.rs` 或 `lib.rs` 的 `#[cfg(test)]`）：传入一个在设备路径上 panic 的判定，直接调清理函数清一个设备路径，用 `#[should_panic]` 或 `catch_unwind` 断言会 panic，证明钩子接在 I/O 上、不是死代码。
4. [ ] 集成测试：现有 `lock_never_reads…`（`flow.rs:3010`）不改断言，只确认仍然通过（现在它真的在证明"没读"了）。
5. [ ] 集成测试（Kobo usbms）：新用例，用 `<dev>/usbms/KOBOeReader` 当 Kobo。track 一本 → 生成到 Kobo（产物在 `KOBOeReader/books/` 下） → 在产物目录放 `.tmp-1-1-x.kepub.epub`：
   - 带钩子跑 `vertical --on`、`add`：退出 0，`.tmp-*` 还在；
   - 不带钩子跑 `sync --device=kobo --no-meta`：`.tmp-*` 被清掉，产物和 `output-state/kobo.json` 逐字节不变；
   - 同一进程 `refresh_devices` → `build_round` 第二轮不再清。可仿照 `flow.rs:3115-3135`。
   - 子进程都经过 `common::booklib_env`，带 30 秒超时（`tests/common/mod.rs` 的 `COMMAND_TIMEOUT`，原单写在 `:117-135`，本单没逐行核对）。
6. [ ] 自检：临时把 `lib.rs` 里任一处 `cleanup_path_allowed` 跳过注释掉，`cargo test -p library --test flow lock_never_reads` 应当失败；恢复后通过。只在本地验证，不提交，结果里写上做过。

## 验收标准
- [ ] 钩子能触发：第 3 步的用例证明，第 6 步的自检做过并写进结果
- [ ] Kobo usbms 用例覆盖"加锁不删、sync 连上后清、同一连接只清一次"
- [ ] 清理行为不变：`git diff master..HEAD -- crates/library/src/deliver.rs crates/library/src/generate.rs crates/library/src/session.rs crates/library/src/build_run.rs crates/bookconv` 为空
- [ ] 版本号不变；`fp_golden.txt` 不重写也能通过
- [ ] `cargo test --workspace` 全过；`cargo clippy --workspace --all-targets -- -D warnings` 0 警告；`cargo fmt --all -- --check` 干净
- [ ] 结果里写明没上真机（本单不需要真机）

## 交付物
代码提交到上面的分支并推送；「结果」填好；状态改为 done 或 blocked。

## 给执行方的指令
```
执行任务单 .dev/tasks/2026-11-12-lock-hook-kobo-test.md：先把状态改为 accepted 并提交，按「分步任务」做完，填好「结果」，状态改为 done（做不下去改为 blocked 并写明原因），提交并推送分支。
```

## 结果
<!-- 执行方填写 -->
- 执行方 / 开始–结束时间：opencode-local / 2026-11-12 14:05–14:10 +08:00
- base..head：`15a4cff..f77ae5b`（`15a4cff` 是切出时的最新 master，含 `415161a` 合并后加的任务单提交；结果提交在分支 tip）
- 提交：

  | 步骤 | 提交号 | 文件 | 说明 |
  | --- | --- | --- | --- |
  | 接受 | `ae4b418` | 任务单 | 状态改 accepted |
  | 1–6 | `f77ae5b` | `fsutil.rs`、`lib.rs`、`tests/flow.rs` | 钩子接到清理的 `read_dir` 前、单元测试、Kobo usbms 集成测试 |
  | 结果 | 本提交 | 任务单 | 填结果、状态改为 done |

- 实现：
  - **钩子接在 I/O 上**：`fsutil::clean_tmp(dir, guard: impl FnOnce(&Path))`、`clean_tmp_files(dir, guard)` 在 `read_dir` 之前调 `guard(dir)`。`lib.rs` 的 `clean_tmp_on_computer` / `clean_tmp_files_on_computer` 仍然先做 `cleanup_path_allowed` 跳过，再把 `panic_if_test_device_read` 作为判定传进去；删掉了原来跳过之后才调、永远走不到的直接调用。`clean_device_leftovers` 传 `|_| {}`，连接清理不受钩子影响。
  - **单元测试**：`fsutil::tests::clean_tmp_guard_runs_before_read_dir` 在设备路径上让判定 panic，`catch_unwind` 断言 `clean_tmp`、`clean_tmp_files` 都会 panic，且目录里的 `.tmp-*` 还在，证明判定在 `read_dir` 之前。
  - **Kobo 集成测试**：`flow.rs::lock_keeps_kobo_usbms_tmp_until_sync_connects`，用 `<dev>/usbms/KOBOeReader` 当设备，产物在 `books/` 下：
    - 带 `BOOKLIB_TEST_DEVICE_IO_PANIC=1` 跑 `vertical --on`、`add`：都退出 0，`.tmp-1-1-x.kepub.epub` 还在（`vertical --on` 用在书目录下一个空的「别的目录」上，避免改到被测书的竖排指纹；命令照样加锁、清理照样会看到 Kobo 产物目录）；
    - 不带钩子跑 `sync --device=kobo --no-meta`：`.tmp-*` 被清掉，Kobo 产物和 `output-state/kobo.json` 逐字节不变；
    - 同一进程 `refresh_devices()` → `build_round`：第一次接上清，第二轮不再清。
  - **自检（第 6 步）**：临时注释掉 `clean_tmp_files_on_computer` 里的 `cleanup_path_allowed` 跳过，跑 `cargo test -p library --test flow lock_never_reads` 失败，报 `清理残留临时文件时读了设备上的目录：…/lib-dev/ireader/Internal Storage/documents`；恢复后同测试通过。临时改动没有提交。
- 测试：

  | 命令 | 通过 / 失败 / 未跑 | 摘要 |
  | --- | --- | --- |
  | `cargo test --workspace` | 通过 | 所有 crate 全过、0 失败（library：lib 78、bin 7、cli 14、flow 53、fp_golden 1） |
  | `cargo test -p library --lib fsutil::tests::clean_tmp_guard` | 通过 | 新单测 |
  | `cargo test -p library --test flow lock_` | 通过 | 原来的 `lock_never_reads…` + 新的 Kobo 用例 |
  | `cargo clippy --workspace --all-targets -- -D warnings` | 通过 | 0 警告 |
  | `cargo fmt --all -- --check` | 通过 | 干净 |
  | `git diff master..HEAD -- crates/library/src/deliver.rs crates/library/src/generate.rs crates/library/src/session.rs crates/library/src/build_run.rs crates/bookconv` | 通过 | 空；清理行为相关文件没动 |
  | `cargo test -p library --test fp_golden`（含在 workspace 里） | 通过 | `fp_golden.txt` 未改 |
- 遇到的问题：无。第 6 步证实了“钩子真的会触发”，也证实原来的跳过还在（恢复后不 panic）。
- 偏离：无。
- 未完成：
  - 没上真机；本单只改测试钩子和测试，不需要真机。
  - 钩子落在清理函数、挨着设备 I/O，按任务单风险点仍建议 agy 二审（负责 bot 判断为中）。
  - 版本号没动（`OPTIMIZE_VERSION` / `COMIC_VERSION` / `PIPELINE_VERSION`），`fp_golden.txt` 一行没变。

## 审查
<!-- 负责 bot 填写 -->
- 验收：**第 1 轮通过**（sheng-ren bot，2026-11-12 16:09 +08:00，审 `15a4cff..c03a565`，实现 `f77ae5b`）。没上真机；本单不需要真机，下面全是 box 上的临时目录。box 原有系统 rustc 1.85.1，编不过当前依赖（要 1.88）；本轮用刚装的 rustup stable rustc 1.99.1（与 CI 的 latest stable 同一通道）重跑测试和 clippy。
  - 逐条验收标准：
    - [x] 钩子能触发。
      - 单元测试 `fsutil::tests::clean_tmp_guard_runs_before_read_dir`（`fsutil.rs:271`）：判定在设备路径上 panic 时，`clean_tmp` / `clean_tmp_files` 都会 panic，`.tmp-*` 还在。含在本轮 `cargo test --workspace` 的 library lib 78 里，通过。
      - 第 6 步自检由负责 bot 重做：临时注释掉 `clean_tmp_files_on_computer`（`lib.rs:519-523`）里的 `cleanup_path_allowed` 跳过，`cargo test -p library --test flow lock_never_reads` 失败，子进程退出 101，stderr 为「清理残留临时文件时读了设备上的目录：…/lib-dev/ireader/Internal Storage/documents（BOOKLIB_TEST_DEVICE_IO_PANIC=1）」。`git checkout` 还原后同测试通过。临时改动没有提交。
    - [x] Kobo usbms 用例覆盖「加锁不删、sync 连上后清、同一连接只清一次」：`flow.rs:3149` `lock_keeps_kobo_usbms_tmp_until_sync_connects`。夹具是 `<dev>/usbms/KOBOeReader`，产物目录断言为 `KOBOeReader/books`；带钩子跑 `vertical --on`（空的「别的目录」）和 `add` 退出 0 且 `.tmp-1-1-x.kepub.epub` 还在；不带钩子 `sync --device=kobo --no-meta` 后临时文件被清、产物和 `output-state/kobo.json` 逐字节不变；同一进程 `refresh_devices` → `build_round` 第一轮清、第二轮不清。含在本轮 flow 53 里，通过。
    - [x] 清理行为不变：`git diff 15a4cff..c03a565 -- crates/library/src/deliver.rs crates/library/src/generate.rs crates/library/src/session.rs crates/library/src/build_run.rs crates/bookconv` 为空。相对 origin/master 也只改了任务单、`fsutil.rs`、`lib.rs`、`tests/flow.rs`。
    - [x] 版本号不变；`fp_golden.txt` 不重写也能通过。diff 里没有 `*_VERSION` 改动，`OPTIMIZE_VERSION`/`COMIC_VERSION`/`PIPELINE_VERSION` 仍是 68/68/5；`git diff origin/master -- crates/library/tests/fp_golden.txt` 为空；本轮 `cargo test -p library --test fp_golden` 通过。
    - [x] `cargo test --workspace` 全过（0 失败）：bookbase lib 30，bookconv lib 553，csscascade lib 16，kfx lib 53，library lib 78 / bin 7 / cli 14 / flow 53 / fp_golden 1，profile lib 19。`cargo clippy --workspace --all-targets -- -D warnings` 0 警告。`cargo fmt --all -- --check` 干净。
    - [x] 结果里写明了没上真机；本单不需要真机。
  - 代码核对：
    - `clean_tmp` / `clean_tmp_files`（`fsutil.rs:187`、`:204`）在 `read_dir` 之前调用 `guard(dir)`。
    - `clean_tmp_on_computer` / `clean_tmp_files_on_computer`（`lib.rs:511-523`）仍先做 `cleanup_path_allowed` 跳过，再把 `panic_if_test_device_read` 传给 fsutil；原来跳过之后那两处走不到的直接调用已删。
    - `clean_device_leftovers`（`lib.rs:546-553`）传 `|_| {}`，连接清理不受钩子影响。
    - 环境变量名仍是 `BOOKLIB_TEST_DEVICE_IO_PANIC`。`clean_tmp` / `clean_tmp_files` 全仓库只有上述调用点，签名都已改完。
    - 提交合规：中文前缀加正文、无 Co-Authored-By、执行方没有合并，工作只在任务分支上。
  - 阻断：无。
  - 建议（不挡合并）：单元测试证明了判定会 panic、临时文件没被删，但「文件还在」只能说明没删到，不能单独证明 `read_dir` 本身没发生（若有人把 `guard` 挪到 `read_dir` 之后、删除之前，这个用例仍会过）。真正证明「加锁路径会触发钩子」的是第 6 步自检和第 4 步集成用例。不必为本点再改。
- 二审：**无阻断**（agy-review，2026-11-12 16:14 +08:00，模型 gemini-3.8-flash-medium，用时 284s，回执 `df22a74`）。结论「建议通过」。负责 bot 逐条核对引用，均与当前代码一致：
  - `fsutil.rs:187-199`、`:204-213`：`guard(dir)` 在 `read_dir` 之前，属实。
  - `lib.rs:511-524`：跳过逻辑保留，钩子作为 guard 传入，属实。
  - `lib.rs:550-555`：`clean_device_leftovers` 传 `|_| {}`，属实（函数从 `:546` 起）。
  - `fsutil.rs:271-293`：单元测试用 `catch_unwind` 断言 panic，属实。
  - `flow.rs:3149-3230`：Kobo usbms 集成用例覆盖加锁不删、sync 后清、产物与生成记录不变、同一连接只清一次，属实。
  - 「未改版本号、未改 fp_golden、提交规范」属实。
  - 建议（不挡合并）：与第 1 轮验收相同，单测「文件还在」不能单独证明 `read_dir` 未发生；已有第 6 步自检和集成用例兜底。
  - 「未核实」只有没重跑测试和没上真机：测试第 1 轮由负责 bot 重跑过；本单不需要真机。
- 合并：无阻断，本单不需要真机，状态改为 verified。负责 bot 2026-11-12 16:16 +08:00 用 `--no-ff` 合入 master，合并提交 `a485186`。合并结果上 `cargo test --workspace` 全过。
