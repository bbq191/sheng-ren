# 加锁时不碰设备：设备上残留临时文件改到 sync 连上那台设备后再清

- Task ID: 2026-11-12-lock-no-device
- 负责 bot: sheng-ren
- 状态: done
- 执行方: opencode-local
- 分支: lock-no-device（从最新 master 切出）
- 更新时间: 2026-11-12T10:55+08:00
- 引用: 2026-11-12-quality-audit（第 1、15 条，见该文件「核实与分组」）

## 需求原文
> （vinci 2026-11-11）`booklib vertical --on 侦探悬疑/欧美侦探/帷幕.epub 侦探悬疑/亚洲侦探/接头.epub` …… 但他一直没有返回 / 拔掉汉王就正常了
> （vinci 2026-11-12）质量审查第 1 张任务单「加锁时不碰设备」（第 1、15 条）开。

## 输入
- 原因（负责 bot 读代码确认，没在真机上复现）：
  - `booklib vertical` 是 `lock: always`（`crates/library/src/bin/booklib/main.rs:205`）。add、track、untrack、remove、dedupe 和写元数据的 meta 也都要加锁（同一文件 `lock:` 各行）。
  - `Library::lock` 在本进程第一次拿到锁时调 `clean_leftovers`（`crates/library/src/lib.rs:462-469`）。它除了清书库里的目录，还对 `self.output_dirs()` 里的每个目录调 `fsutil::clean_tmp_files`（`lib.rs:498-500`），也就是 `read_dir`（`crates/library/src/fsutil.rs:201-209`）。
  - `output_dirs()`（`crates/library/src/generate.rs:338` 起）列的是**所有**设备生成记录里产物所在的目录，注释写明"不看设备挂没挂上"。汉王的 jmtpfs 卡死时，`read_dir` 卡在 `/run/user/1000/mtp/hanvon/…` 上，命令就一直不返回。`vertical` 本身只读注册表、不连设备（`crates/library/src/vertical.rs:136-190`）。
- 这些设备目录里的 `.tmp-*` 是怎么来的：经临时文件写的设备（Kobo，USB 大容量存储，`generate.rs` `direct_write` 为 false；MTP 上只差大小写时也强制经临时文件，`deliver.rs:209-212`）在写完改名前进程被杀，会留下 `.tmp-<进程号>-<计数>-<名>`（`fsutil.rs:55` 一带）。所以设备上的清理不能直接删掉，要挪到"连上这台设备、要往上写的时候"。
- `sync` 连设备在 `crates/library/src/build_run.rs:171-186`（`lib.device_status(device)` 成功的进 `live`）；`--watch` 每轮开头 `refresh_devices`。
- 设备路径怎么认：`Devices::unplugged`（`crates/library/src/session.rs:158` 起）已经按 `mtp_base`、`usbms_base`（含 `/run/media` 和 `/media` 两处）认出一条路径在不在可拔下的设备上。
- 涉及文件（预计）：
  - `crates/library/src/lib.rs`（修改：`clean_leftovers` 只清书库自己的目录和电脑上的产物目录；新增"清这台设备上残留临时文件"的方法）
  - `crates/library/src/session.rs`（修改：抽出"这条路径在不在可拔下的设备上"给 `unplugged` 和清理共用；记哪些设备本轮或本次连接已经清过）
  - `crates/library/src/build_run.rs`（修改：设备接上后调清理）
  - `crates/library/src/fsutil.rs`（如需要：测试钩子）
  - `crates/library/tests/cli.rs` 或 `tests/flow.rs`（新增测试）
  - `docs/decisions.md`（新增一行）

## 约束
- 不做：不改产物内容、传书（`deliver.rs`）、生成记录的格式和内容；不改 `crates/library/src/generate.rs`（另一张单 `2026-11-12-docs-align` 在改它的注释，两张单不能碰同一个文件；`output_dirs()` 照原样用，在 `lib.rs` 里按设备过滤）；不改 `docs/architecture.md`、`docs/development.md`、`README.md`、`AGENTS.md`、`CONTRIBUTING.md`（归 docs-align）；不处理 `sync` 在卡死的挂载上也会卡住（`deliver::connect` 的 `mounted()` 要 stat 挂载点，这是另一回事，写进「结果 → 未完成」即可）
- 不升任何版本号：`OPTIMIZE_VERSION`、`COMIC_VERSION`、`PIPELINE_VERSION` 都不动，`fp_golden.txt` 一行不变
- 只在上面的分支工作；不合并；改「涉及文件」以外的文件写进「结果 → 偏离」
- 提交信息中文，`fix:` / `test:` / `docs:` 前缀 + 正文，不加 Co-Authored-By；不读密钥、`.env`、`auth*`
- 测试一律用临时目录当设备（`BOOKLIB_MTP_DIR`、`BOOKLIB_MTP_NO_MOUNT_CHECK`），不碰真设备
- 风险点：设备行为（设备上残留临时文件的清理时机）→ 要 agy 二审

## 规则（要实现的行为）
1. **加锁时一律不碰设备**：`clean_leftovers` 照旧清书库根、`masters/` 和各条目、`output-state/`、`output/<模式>/`，以及 `output_dirs()` 里**不在可拔下的设备上**的目录（电脑上的自定义模式产物）；在 `mtp_base` / `usbms_base` 下面的一概跳过，不 `read_dir`、不 `stat`。判断只看路径前缀，不碰文件系统。
2. **设备上的残留临时文件，在 `sync` 连上这台设备后清**：`build_all` 里 `device_status` 成功、进 `live` 以后，清 `output_dirs()` 里在这台设备产物根目录（`Target::Dir` 的 `root`）下面的目录，只删 `.tmp-` 开头的**文件**（同现在的 `clean_tmp_files`）。每次这台设备"从没接上变成接上"时清一次（含进程里第一次接上），同一次连接里 `--watch` 的后续轮次不再清。在生成、传输开始之前做完，不和本进程的传输线程抢同一个文件。
3. 别的命令（vertical、track、untrack、add、remove、dedupe、meta、list、devices）**不碰设备上的产物目录**；`sync` 以外本来就会连设备的流程（如 `remove` 删产物、`retract`）照旧，不在本单范围。
4. 提示：清掉了几个设备上的残留文件可以不报；要报的话放进现有的 `BuildEvent`，不新增给人看的文字以外的输出。

## 分步任务
1. [ ] `session.rs`：抽出 `on_removable(&self, path) -> bool`（只看前缀），`unplugged` 改用它，行为不变；单元测试覆盖 `mtp_base`、`usbms_base`、`/run/media` 和 `/media` 两处、电脑上的路径。
2. [ ] `lib.rs`：`clean_leftovers` 跳过设备上的目录；新增 `clean_device_leftovers(&self, device: &Profile)`（只在 `Target::Dir` 时清那台设备 root 下面的目录）。
3. [ ] `build_run.rs`：按规则 2 接入，记住"这次连接已清过"（设备报没接上时清掉标记）。
4. [ ] 测试（**加锁绝不读设备目录**）：加一个测试钩子，例如环境变量 `BOOKLIB_TEST_DEVICE_IO_PANIC=1` 时，清理代码要对可拔下的设备上的路径做 `read_dir` 就直接 panic（只在清理这条路径上检查，不影响别的代码）。集成测试：
   - 建一个书库，往一台 MTP 测试设备同步过一本（生成记录里有设备目录），在设备产物目录里放一个 `.tmp-1-1-x.epub`；
   - 带上钩子跑 `booklib vertical --on <书>`、`track`、`add`、`meta`（写）、`remove`（另一本只在电脑上有产物的）：都成功退出，设备上的 `.tmp-*` 还在；
   - 不带钩子跑 `booklib sync`：设备上的 `.tmp-*` 被清掉，产物和生成记录逐字节和清理前一样；
   - 同一台 `sync --watch` 式的第二轮（`refresh_devices` 后再 `build_all`、设备没断开过）不再清；
   - 电脑上的自定义模式产物目录里的 `.tmp-*` 加锁时照旧被清掉。
5. [ ] 测试（**不会卡死**）：上面跑子进程的测试都要带超时（例如 `spawn` 后最多等 30 秒，超时 `kill` 并判失败），测试本身永远不会挂住；如能做到，再加一个用 FIFO 或不可读目录冒充设备产物目录的用例，证明加锁时根本不去碰它。
6. [ ] `docs/decisions.md` 加一行「2026-11-12 加锁时不碰设备；设备上的残留临时文件改到 sync 连上那台后清」，写清原因（汉王 MTP 卡死时 `vertical` 不返回）。

## 验收标准
- [ ] 带钩子时 vertical / track / add / meta / remove 都不读设备目录（测试证明），设备上的 `.tmp-*` 留着
- [ ] `sync` 连上设备后清掉它的 `.tmp-*`，每次连接只清一次；产物、生成记录逐字节不变
- [ ] 子进程测试都有超时，不会挂住
- [ ] `OPTIMIZE_VERSION` / `COMIC_VERSION` / `PIPELINE_VERSION` 不变；`fp_golden.txt` 不重写就通过；`git diff master..HEAD -- crates/library/src/generate.rs crates/library/src/deliver.rs crates/bookconv` 为空
- [ ] `cargo test --workspace` 全过；`cargo clippy --workspace --all-targets -- -D warnings` 0 警告；`cargo fmt --all -- --check` 干净
- [ ] 结果里写明没上真机。真机由 vinci 验：汉王插着、挂载卡住（或拔掉后挂载点还在）时，`booklib vertical --auto <一本书>` 马上返回；Kobo 插着跑一次 `booklib sync` 正常；`sync` 在卡住的汉王上仍会卡（本单不管），记下现象

## 交付物
代码提交到上面的分支并推送；「结果」填好；状态改为 done 或 blocked。

## 给执行方的指令
```
执行任务单 .dev/tasks/2026-11-12-lock-no-device.md：先把状态改为 accepted 并提交，按「分步任务」做完，填好「结果」，状态改为 done（做不下去改为 blocked 并写明原因），提交并推送分支。
```

## 结果
<!-- 执行方填写 -->
- 执行方 / 开始–结束时间：opencode-local / 2026-11-12 10:45–10:55 +08:00
- base..head：`848967c..d8a7567`（`848967c` 是切出时的 master；结果提交在分支 tip）
- 提交：

  | 步骤 | 提交号 | 文件 | 说明 |
  | --- | --- | --- | --- |
  | 接受 | `43d1357` | 任务单 | 状态改 accepted |
  | 1–6 | `d8a7567` | `session.rs`、`lib.rs`、`build_run.rs`、`tests/common/mod.rs`、`tests/flow.rs`、`docs/decisions.md` | 加锁不碰设备、sync 连上后清设备临时文件、测试和决定记录 |
  | 结果 | 本提交 | 任务单 | 填结果、状态改为 done |

- 实现：
  - **只看前缀认设备**：`Devices::on_removable(path)` 用和 `unplugged` 相同的挂载目录（`mtp_base`、`usbms_base`，开着挂载检查时含 `/run/media` 与 `/media` 互认），按 `Path::starts_with` 比，不碰文件系统；`unplugged` 改为复用同一个前缀判断，行为不变。
  - **加锁清理跳过设备**：`clean_leftovers` 先看库根，在任何 `read_dir` / `stat` 之前返回；书库各目录和 `output_dirs()` 里落在设备挂载目录下的一概跳过，电脑上的自定义模式产物目录照旧清。测试钩子 `BOOKLIB_TEST_DEVICE_IO_PANIC=1` 放在清理入口，真要读设备路径时 panic。
  - **sync 连上后清设备**：`build_all` 里 `device_status` 成功、进 `live` 后，在生成和传输之前对每台设备调一次 `clean_device_leftovers_once`；只清 `output_dirs()` 里在 `Target::Dir` 的 `root` 下面的目录、只删 `.tmp-*` 文件。`Devices` 新记一份“这次连接已清过”的集合，`refresh_devices` 不清它；设备报没接上时清掉标记，下次接上重新清。
  - **文档**：`docs/decisions.md` 2026-11-12 加一行，写明汉王 jmtpfs 卡死时 `vertical` 不返回的原因和这次的清理时机。

- 测试：

  | 命令 | 通过 / 失败 / 未跑 | 摘要 |
  | --- | --- | --- |
  | `cargo test -p library` | 通过 | lib 77（含 3 个 `session` 单测）、bin 7、cli 14、flow 52、fp_golden 1，0 失败 |
  | `cargo test --workspace` | 通过 | 所有 crate 全过，0 失败 |
  | `cargo clippy --workspace --all-targets -- -D warnings` | 通过 | 0 警告 |
  | `cargo fmt --all -- --check` | 通过 | 干净 |
  | `git diff master..HEAD -- crates/library/src/generate.rs crates/library/src/deliver.rs crates/bookconv crates/library/tests/fp_golden.txt` | 通过 | 空；版本号、金样都没动 |
  | `cargo test -p library --test flow lock_never_reads` | 通过 | 新集成用例 |

- 集成用例覆盖（`flow.rs::lock_never_reads_device_dirs_and_sync_cleans_their_tmp_files`，全部用临时目录当设备）：
  - 带 `BOOKLIB_TEST_DEVICE_IO_PANIC=1` 跑 `vertical --on`、`track`、`add`、`meta --fetch --clear`、`remove`（删的是一本只在电脑上有产物的书）：都退出 0，设备上的 `.tmp-*` 还在，电脑上的自定义模式产物目录里的 `.tmp-*` 每次都被清掉。
  - 不带钩子跑 `booklib sync --device=ireader --no-meta`：设备上的 `.tmp-*` 被清掉，产物和 `output-state/ireader.json` 逐字节不变。
  - 同一进程里 `refresh_devices()` → `build_all`：第一次接上清，第二轮不再清；把设备存储目录挪走再挪回来（模拟拔插）后重新清。
  - 子进程测试统一加了 30 秒超时（`tests/common/mod.rs`），超时 kill 并判失败，不会挂住。
- 遇到的问题：无。
- 偏离：
  - 改了 `crates/library/tests/common/mod.rs`（子进程超时），不在「涉及文件」预测里；新用例和超时按分步任务 5 做。
  - 测试钩子放在 `lib.rs` 的清理入口，不在 `fsutil.rs`；`fsutil` 不知道设备前缀，放这里也不用改清理函数的签名。
  - 没做 FIFO / 不可读目录冒充设备的用例（分步任务 5 的可选项）：钩子已经能在清理真去读设备路径时 panic；FIFO 不能当目录，不可读目录在 root 下也挡不住读。
- 未完成：
  - **没上真机、没跑真书回归**。真机由 vinci 验：汉王插着、挂载卡住（或拔掉后挂载点还在）时，`booklib vertical --auto <一本书>` 应马上返回；Kobo 插着跑一次 `booklib sync` 正常；`sync` 在卡住的汉王上仍会卡（`deliver::connect` 的 `mounted()` 要 stat 挂载点，本单不管），记下现象。
  - 设备上残留临时文件的清理时机属于设备行为，按任务单要求要 agy 二审。

## 审查
<!-- 负责 bot 填写 -->
- 验收：
- 二审：
- 合并：
