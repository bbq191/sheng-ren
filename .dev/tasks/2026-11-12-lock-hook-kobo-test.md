# 加锁不读设备的测试钩子真正生效，并补 Kobo（usbms）连接清理的自动化用例

- Task ID: 2026-11-12-lock-hook-kobo-test
- 负责 bot: sheng-ren
- 状态: accepted
- 执行方: opencode-local
- 分支: lock-hook-kobo-test（从最新 master 切出）
- 更新时间: 2026-11-12T13:30+08:00
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
