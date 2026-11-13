# 设备路径统一为 ~/Devices/<Name>（掌阅保留并预留 Ireader）

- Task ID: 2026-11-13-devices-dir
- 负责 bot: sheng-ren
- 状态: done
- 执行方: box-agent
- 分支: task/2026-11-13-devices-dir（从最新 origin/master 切出，**不要**基于 `task/2026-11-13-vertical-default-off`）
- 更新时间: 2026-11-13T11:46:02+08:00
- 引用: vinci 2026-11-13 10:41 在 sheng-ren 聊天里批准「按方案开单执行，掌阅保留并预留 ~/Devices/Ireader」；方案 A；只读调查（`DeviceEnv`、`deliver::connect`、`mounted`、`on_removable`、`rebase_state_case`）；dotfiles `main@55b219d`

## 需求原文
> （vinci 2026-11-13 10:41，sheng-ren 聊天）按方案开单执行，掌阅保留并预留 `~/Devices/Ireader`。
>
> 方案 A（已批准）：booklib 判断设备是否接上，统一改成只查 `~/Devices/<Name>`。挂载和链接归 dotfiles，booklib 只读、绝不创建或删除这些链接。掌阅模式保留，链接名预留 `Ireader`，现在一律判为没接上。不回退旧路径。生成记录做前缀迁移（只改记录，不碰文件、不重传、不删）。`on_removable` 同时认新旧前缀，仍然只比前缀。不升版本号，不用跑真书回归。

## 输入
- 基于 origin/master `cd2500e`（功能代码与已合并的 `89715dc` / `53b5322` 相同；master 上只有任务单提交，**没有**竖排默认改横排的代码，不要把那条分支合进来）。
- 现状（`crates/library/src/deliver.rs`、`session.rs`、`generate.rs`）：
  - `DeviceEnv`：`mtp_base`（缺省 `$BOOKLIB_MTP_DIR` 或 `$XDG_RUNTIME_DIR/mtp`）、`usbms_base`（缺省 `$BOOKLIB_USBMS_DIR` 或 `/run/media/$USER`，它不在而 `/media/$USER` 在时用后者；关挂载检查又没设时用 `<mtp_base>/usbms`）、`mount_check`（`$BOOKLIB_MTP_NO_MOUNT_CHECK=1` 关）。
  - `deliver::connect`：MTP 拼 `mtp_base.join(mount)`，USBMS 拼 `usbms_base.join(label)`；开着 `mount_check` 时用 `mounted()`（`stat` 比 `st_dev` 与上一级是否同一文件系统）；MTP 再用 `mtp_storage` 探测存储子目录。记录里存的是绝对路径。
  - `Devices::on_removable` / `removable_base`：只比前缀，不碰文件系统；开着挂载检查时 `/run/media` 与 `/media` 互认。`list-no-device-stall`（`list_does_not_stat_removable_device_outputs`）和 `lock-no-device`（`lock_never_reads_device_dirs_and_sync_cleans_their_tmp_files`）的保证都靠它。`BOOKLIB_TEST_DEVICE_IO_PANIC=1` 时清理代码真读设备目录会 panic。
  - `rebase_state_case`：设备接上后，只改记录里只差大小写的根路径（`case_alias`），不碰文件、不重传、不删。换位置（真正换目录）会把旧路径记进 `old` 再删，本单的前缀迁移**绝不能**走这条路。
- 现行 profile `[deliver]`：
  - Kindle / Bigme / Hanvon / 掌阅：`kind = "mtp"`，`mount` 为小写 `kindle` / `bigme` / `hanvon` / `ireader`。
  - Kobo：`kind = "usbms"`，`label = "KOBOeReader"`（卷标），`dir = "books"`。
- dotfiles `main@55b219d` 的事实（本单按此落实，不要去改 dotfiles）：
  - `~/Devices/Kindle`、`~/Devices/Bigme`、`~/Devices/Hanvon` 由 `mtp-rw-mount` 建，名字是 label 首字母大写。`~/Devices/Kobo` 由 `kobo-link` 建，目标是 udiskie 的挂载点，现为 `/run/media/afu/KOBOeReader`，卷标仍是 `KOBOeReader`。
  - 掌阅已从 dotfiles 移除，将来加回会叫 `~/Devices/Ireader`。booklib 保留 `ireader` 模式，链接名预留 `Ireader`，现在一律判为没接上。
  - 新旧路径：`/run/user/1000/mtp/kindle|bigme|hanvon` ↔ `~/Devices/Kindle|Bigme|Hanvon`；`/run/media/afu/KOBOeReader` ↔ `~/Devices/Kobo`。`~/Hanvon` 是废弃的旧位置，仓库文档已过时。
  - 链接通常在拔出时删除，但不保证，所以链接在不等于设备可用。卸不掉的 FUSE 在 `/proc/self/mountinfo` 里仍显示为已挂载。这个检查只能保证不挂住；写入时仍需超时。**现有的写入行为不在本单范围内，不用改。**

## 规则（要实现的行为）
1. **`DeviceEnv` 新增 `devices_base`**：缺省 `$HOME/Devices`，可用 `$BOOKLIB_DEVICES_DIR` 覆盖。MTP 和 USBMS 都改为查这里（`devices_base.join(链接名)`）。不再用 `BOOKLIB_MTP_DIR` / `BOOKLIB_USBMS_DIR` **寻找**设备（帮助标明已废弃或不在帮助里当成现行选项）；这两项若还在环境里，只作为下面第 3、4 条的**旧前缀**来认记录，不用于 `connect`。
2. **profile 链接名与卷标分开，不要混**：
   - MTP：增加链接名字段，**或**把 `mount` 改成首字母大写的链接名（`Kindle` / `Bigme` / `Hanvon` / `Ireader`）。若改 `mount`，旧路径迁移仍要认得原来的小写 `kindle` 等，不要把旧挂载子目录名跟链接名当成同一个字段用错。
   - Kobo：保留 `label = "KOBOeReader"` 的卷标语义，**另加**链接名 `Kobo`。`connect` 用链接名拼 `~/Devices/Kobo`，不要用卷标去拼 `~/Devices/KOBOeReader`。
3. **判断是否接上**（开着 `mount_check` 时）：
   1. 对 `devices_base/<链接名>` 做 `lstat`，确认是符号链接；不是链接或不在 → 没接上。
   2. `readlink` 取目标；相对路径按**链接所在目录**解析成绝对路径。
   3. 在 `/proc/self/mountinfo` 里找挂载点与该目标**完全相等**的行；比较前先还原 `\040` 等八进制转义（mountinfo 转义规则）。
   4. **不写死**文件系统类型，**不 `stat` 目标，不进入目标**。
   5. `connect` 成功之后照旧用 `mtp_storage` 探测存储子目录（MTP）；USBMS 仍在挂载点下用 `dir`。
   6. `$BOOKLIB_MTP_NO_MOUNT_CHECK=1` 时允许普通目录当设备（给测试用），行为与现在测试夹具兼容。
   7. 链接不在就算没接上，**不回退**旧路径（`$XDG_RUNTIME_DIR/mtp/<小写>`、`/run/media/$USER/KOBOeReader`、`~/Hanvon` 等）。
   8. 产物根路径用 `~/Devices/<Name>/…` 这种**链接路径**，不用解析后的目标。记录里新写的绝对路径走链接这一边。
4. **生成记录前缀迁移**：设备接上后，把记录里旧前缀开头的路径改写成新的 `devices_base/<链接名>` 前缀（`path` / `root` / `old` / `orphans` 凡是存了绝对路径的都改）。只改记录，不碰文件，不重传，不删。照 `rebase_state_case` 的原则（每台每轮一次、接上才做）。旧前缀至少包括：
   - 旧 `mtp_base`（缺省 `$XDG_RUNTIME_DIR/mtp`，以及仍设了 `BOOKLIB_MTP_DIR` 时的那个目录）下的 `<小写 mount>`（`kindle` / `bigme` / `hanvon` / `ireader`）；
   - `/run/media/$USER/KOBOeReader` 和 `/media/$USER/KOBOeReader`（以及仍设了 `BOOKLIB_USBMS_DIR` 时的 `<label>`）；
   - 文档里出现过的废弃位置 `~/Hanvon`（若记录里有，一并改到 `~/Devices/Hanvon`）。
   **必须有测试**：新旧路径经符号链接指向同一个文件时，绝不会走「换位置」的删除（不把旧路径记进 `old` 再删），不丢书、不重传。
5. **`on_removable`**：同时认新前缀 `devices_base` 和所有第 4 条的旧前缀，仍然**只比前缀**，不加 `readlink` 或 `stat`。保证 `list` 和加锁在卡死的挂载上依旧不碰设备。`list_does_not_stat_removable_device_outputs` 和 `lock_never_reads_device_dirs_and_sync_cleans_their_tmp_files` 必须继续通过（夹具按新链接名改好）。`unplugged` 等靠 `removable_base` 的逻辑要与新前缀一致，仍不在卡死挂载上 `stat`。
6. **掌阅**：`ireader` 模式保留；链接名预留 `Ireader`。真实环境中（没建 `~/Devices/Ireader`、也不回退旧 `mtp/ireader`）一律没接上。测试覆盖这一点。
7. **booklib 只读这些链接**：绝不创建、不删除、不改 `~/Devices/<Name>`。挂载和链接归 dotfiles。
8. **测试与夹具**：
   - `dev_dir` / `documents()` 改成按链接名建目录；测试不碰真实的 `~/Devices`；CLI 测试设 `BOOKLIB_DEVICES_DIR`。
   - 补单元测试：mountinfo 解析（含 `\040` 转义）；链接不在；链接在但目标不在 mountinfo；相对链接（相对链接所在目录解析）；Kobo 标签与链接名分开；ireader 在真实规则下永远没接上；前缀迁移不走换位置删除（见第 4 条）。
   - `$BOOKLIB_MTP_NO_MOUNT_CHECK=1` 的测试路径继续能用普通目录。
9. **文档与帮助**：`README.md`、`docs/usage.md`、`docs/devices.md`、`docs/architecture.md` 和 `booklib` 帮助文字都改成 `~/Devices/<Name>`。删掉过时的 `~/Hanvon` 和「`BOOKLIB_MTP_DIR=~/Devices` 让 `…/Bigme` 对上 `mount = "bigme"`」那条**错误建议**。`docs/decisions.md` 新增一条：设备路径统一为 `~/Devices/<Name>`，挂载和链接归 dotfiles，booklib 只读、绝不创建或删除这些链接，并写明不削弱 `lock-no-device` 和 `list-no-device-stall` 的保证。没上真机，写进决定。
10. **不升版本号**（`OPTIMIZE_VERSION`、`COMIC_VERSION` 及其它都不动）。`[deliver]` 不进指纹，`fp_golden.txt` **不应变化**（变了就停下来说明原因，不要擅自 `UPDATE_FP_GOLDEN`）。不用跑真书回归。不碰 `~/.local/share/booklib`，不碰真机，测试用临时 `--library` / `BOOKLIB_DEVICES_DIR`。`install.sh` 不用 `--force`。不要对整个仓库跑 `cargo fmt`。现有写入/超时行为本单不改。

## 约束
- 只用 Rust；clean-room；仓库不出现上游项目名及缩写。
- **从 origin/master 切分支**，不要基于、不要合并 `task/2026-11-13-vertical-default-off`。
- 提交信息中文 + 前缀 + 正文，不加 Co-Authored-By。本单不改产物内容：提交和结果写明没跑真书回归、没上真机。
- 风险点：设备接上判定和生成记录路径 → **必须 agy 二审**。
- 本仓库不用 rustfmt 默认宽度，只 fmt 自己改过的文件。

## 要改的文件（请只改这些；新建文件先写在结果里说明）
执行时请把改动限制在下面列出的文件。若必须新建模块（例如 `crates/library/src/mountinfo.rs`）或改 `crates/library/src/lib.rs` 的 `mod` 声明，在结果里写明，并说明为什么清单里没有却必须碰。不要改 `fp_golden.txt`、不要改版本号常量、不要改 `task/2026-11-13-vertical-default-off` 相关文件。

- `crates/library/src/deliver.rs`（`DeviceEnv`、`connect`、`mounted` / mountinfo、必要时测例）
- `crates/library/src/session.rs`（`on_removable` / `removable_base` / `unplugged`、`Devices`、测例）
- `crates/library/src/generate.rs`（前缀迁移，仿 `rebase_state_case`；相关测例；`DeviceEnv { … }` 字面量）
- `crates/library/src/bin/booklib/main.rs`（帮助里的环境变量和路径说明）
- `crates/library/src/lib.rs`（仅当需要 `mod` 新文件或更新模块文档时）
- `crates/library/tests/common/mod.rs`（`dev_dir`、`documents()`、CLI 环境设 `BOOKLIB_DEVICES_DIR`）
- `crates/library/tests/flow.rs`（夹具、`list_does_not_stat_removable_device_outputs`、`lock_never_reads_device_dirs_and_sync_cleans_their_tmp_files`、前缀迁移不丢书的测试）
- `crates/library/tests/cli.rs`（若它设了 `BOOKLIB_MTP_DIR` 或假定旧路径；`devices` 相关测例）
- `crates/profile/src/lib.rs`（`Deliver` 链接名字段或 `mount` 语义、解析与测例）
- `crates/profile/profiles/kindle.toml`
- `crates/profile/profiles/bigme.toml`
- `crates/profile/profiles/hanvon.toml`
- `crates/profile/profiles/ireader.toml`
- `crates/profile/profiles/kobo.toml`
- `README.md`
- `docs/usage.md`
- `docs/devices.md`
- `docs/architecture.md`
- `docs/decisions.md`
- `.dev/tasks/2026-11-13-devices-dir.md`（本任务单，填结果）

`tools/quirks/`、`tools/display/` 里若只是过时注释、且改注释即可，可以顺手改；不要改探针逻辑。不必为了本单主动去改它们。

## 分步任务
1. [ ] 扩展 `DeviceEnv`（`devices_base`、`BOOKLIB_DEVICES_DIR`）和 profile 链接名（Kobo 的 `label` 与 `link` 分开；MTP 按规则 2）。`connect` 改为查 `~/Devices/<Name>`（或测试里的 `BOOKLIB_DEVICES_DIR`），不回退旧路径。
2. [ ] 实现「链接 + mountinfo」的接上判定（`lstat`、`readlink`、相对路径按链接目录解析、mountinfo 完全相等并还原八进制转义；不 stat 目标、不写死 fs 类型）。`BOOKLIB_MTP_NO_MOUNT_CHECK=1` 仍允许普通目录。`connect` 成功后照旧 `mtp_storage`。产物根用链接路径。
3. [ ] 生成记录前缀迁移（规则 4），并补「新旧路径指向同一文件时不走换位置删除、不丢书」的测试。
4. [ ] `on_removable` 同时认新前缀和全部旧前缀，只比前缀。既有 `list-no-device-stall`、`lock-no-device` 测试继续通过。
5. [ ] 夹具和单元测试（规则 8）：`dev_dir` / `documents()`、不碰真实 `~/Devices`、mountinfo / 链接各种情况、Kobo 标签与链接名、ireader 没接上。
6. [ ] 文档和帮助（规则 9），`docs/decisions.md` 新写一条。不升版本号；确认 `fp_golden.txt` 无变化。
7. [ ] `cargo test --workspace` 全过；`cargo clippy --workspace --all-targets -- -D warnings` 0 警告。结果里写明没跑真书回归、没上真机。

## 验收标准
- [ ] 真实约定下，只认 `~/Devices/Kindle|Bigme|Hanvon|Kobo`（及预留的 `Ireader`）；链接不在或目标不在 mountinfo 则没接上；不回退旧路径。ireader 在没有 `~/Devices/Ireader` 时永远没接上。
- [ ] 接上判定：先确认符号链接，再 readlink，再 mountinfo 精确匹配（含 `\040` 等转义）；不 stat 目标、不进入目标、不写死文件系统类型。测试覆盖：链接不在、链接在但未挂上、相对链接、转义。
- [ ] Kobo：`label` 仍是 `KOBOeReader`，链接名是 `Kobo`，两者不混。MTP 链接名为首字母大写。
- [ ] 生成记录前缀迁移：只改记录，不碰文件、不重传、不删；新旧路径指向同一文件时不走换位置删除。有测试。
- [ ] `on_removable` 认新旧前缀、只比前缀；`list_does_not_stat_removable_device_outputs` 和 `lock_never_reads_device_dirs_and_sync_cleans_their_tmp_files` 通过。
- [ ] booklib 不创建、不删除 `~/Devices` 下的链接。测试不碰真实 `~/Devices`。
- [ ] 文档和帮助已改；错误的 `BOOKLIB_MTP_DIR=~/Devices` 建议和过时的 `~/Hanvon` 已删；`decisions.md` 有新决定，并写明不削弱上述两项保证。没上真机。
- [ ] 版本号不变；`fp_golden.txt` 不变。`cargo test --workspace` 全过；clippy `-D warnings` 0 警告。没跑真书回归（本单不需要）。

## 交付物
代码提交到任务分支并推送；「结果」填好；状态改为 done 或 blocked。任务单状态改动推 master，带 `[skip-notify]`。不要合并，不要跑二审（负责 bot 验收和二审）。

## 给执行方的指令
```
执行任务单 .dev/tasks/2026-11-13-devices-dir.md：先把状态改为 accepted 并提交，按「分步任务」做完，填好「结果」，状态改为 done（做不下去改为 blocked 并写明原因），提交并推送分支。任务单 done 推 master 带 [skip-notify]。不要合并，不要跑二审。分支必须从 origin/master 切，不要基于 vertical-default-off。改动尽量只碰任务单「要改的文件」里列出的路径。不升版本号，不要改 fp_golden.txt。结果里写明没跑真书回归、没上真机。
```

## 结果
- 执行方 / 开始–结束时间：box-agent（oc-exec） / 2026-11-13T11:23:28+08:00–2026-11-13T11:46:02+08:00
- base..head：`a14200cc4c5a403302ba33e21f4041352925175f..60e4b84`（feat `62f43de` + rustfmt `60e4b84`）
- 提交：
  | 提交号 | 说明 |
  |---|---|
  | f45db23 | chore: 开始执行任务单 2026-11-13-devices-dir [skip-notify] |
  | 62f43de | feat: 设备路径统一为 ~/Devices/<链接名>（正文写明没跑真书回归、没上真机） |
  | 4d81f68 | chore: 填写任务单 2026-11-13-devices-dir 结果（done） [skip-notify] |
  | 60e4b84 | style: 按 rustfmt 整理 devices-dir 改动的折行 |
- 改动文件：任务单清单内各文件，外加清单未列但必要的 `crates/library/src/transfer.rs`（见偏离说明）
- 测试（负责 bot 在 `/tmp/sheng-ren-review-devices-dir` @ 60e4b84 代跑，`CARGO_TARGET_DIR=/home/box/repos/sheng-ren/target`，rustup stable）：
  - `cargo test --workspace` 全过：bookbase 30、bookconv 553、csscascade 16、kfx 53、library 82 / bin 7 / cli 14 / flow 61（含 `legacy_device_prefix_is_migrated_in_records_only`）/ fp_golden 1、profile 19
  - `cargo clippy --workspace --all-targets -- -D warnings` 0 警告
  - `cargo fmt --all -- --check` 最初因折行失败，只对改动文件 `cargo fmt` 并提交 60e4b84 后再检通过
  - `fp_golden.txt` 无变化；本分支基于竖排合并前的 `a14200c`，`OPTIMIZE_VERSION` 仍为 68（合并进已含竖排的 master 后应为 69，COMIC 仍 68）
  - **没跑真书回归、没上真机**（本单不动产物内容；`[deliver]` 不进指纹）
- 遇到的问题：无
- 偏离：oc-exec 报 14 个文件。其中 13 个本就在任务单「要改的文件」里，属解析误报。真正多改的是 `crates/library/src/transfer.rs`：根路径都改到 `devices_base` 后，原先用「路径是否以 `usbms_base` 开头」判断的 `direct_write` 会失效，故给 `Target::Dir` 加了 `staged`（USBMS true / MTP false），`transfer.rs` 只多一个字段传递。必要，不算出界问题。
- 未完成：无
- 运行记录：`/home/box/oc-runs/2026-11-13-devices-dir-20261113-112328.json`
- 日志：`/home/box/oc-runs/2026-11-13-devices-dir-20261113-112328.log`
- 模型：`opencode-go/deepseek-v4.1-flash`

## 审查
- 验收：2026-11-13 11:48–11:56 Asia/Shanghai，独立 worktree `/tmp/sheng-ren-review-devices-dir` @ `60e4b84`（对照方案 A 与任务单全文，`git diff origin/master...HEAD` 的 merge-base 为 `a14200c`）。
  - 实现符合方案：`DeviceEnv.devices_base`（缺省 `$HOME/Devices`，`BOOKLIB_DEVICES_DIR` 可改）；MTP 的 `link`（首字母大写）与小写 `mount`（只认旧前缀）分开；Kobo 保留 `label = "KOBOeReader"`，另加 `link = "Kobo"`，两者不混。接上判定：先 `lstat` 确认符号链接，再 `readlink`（相对路径按链接所在目录解析，只做字面规整、不 `canonicalize`、不 stat 目标），再要求目标与 `/proc/self/mountinfo` 第 5 列完全相等（先还原 `\040` 等八进制转义）；不写死文件系统类型、不进入目标。链接不在或目标不在 mountinfo 就算没接上，**不回退**旧路径。产物根用链接路径。`BOOKLIB_MTP_NO_MOUNT_CHECK=1` 仍允许普通目录。`connect` 成功后 MTP 照旧 `mtp_storage`。booklib 不创建、不删除这些链接（有断言）。测试不碰真实 `~/Devices`，CLI 设 `BOOKLIB_DEVICES_DIR`。
  - 前缀迁移只改记录（`path`/`root`/`old`/`orphans`），不碰文件、不重传、不删；迁移后与当前位置相同的旧位置从待删里去掉，不走「换位置」删除。集成测试 `legacy_device_prefix_is_migrated_in_records_only` 通过：同一文件迁前缀后仍是 `Built::UpToDate`，文件仍在，记录已换成新前缀，不留 `"old"`。
  - `on_removable` 同时认 `devices_base` 和全部旧前缀（含 `/run/media/$USER`、`/media/$USER`、废弃的 `~/Hanvon`），仍然只比前缀。`list_does_not_stat_removable_device_outputs`、`lock_never_reads_device_dirs_and_sync_cleans_their_tmp_files` 继续通过。掌阅模式保留，链接名预留 `Ireader`；无链接时一律没接上。
  - 单元测试覆盖：mountinfo 八进制转义与精确匹配、相对链接、链接不在、链接在但目标不在 mountinfo、Kobo 标签与链接名分开、ireader 在无链接或非符号链接时没接上。
  - 文档（README、usage、devices、architecture、帮助文字）已改成 `~/Devices/<Name>`；过时的 `~/Hanvon` 作为现行挂载和错误的 `BOOKLIB_MTP_DIR=~/Devices` 建议已删；`decisions.md` 新增一条，并写明不削弱 `lock-no-device` 和 `list-no-device-stall`。
  - 测试、clippy、fmt、版本与指纹见上方「结果」。**没跑真书回归、没上真机**。
- 二审：agy-review，模型 `gemini-3.8-flash-medium`，2026-11-13 11:56:11 Asia/Shanghai，用时 338s，结论「建议通过」，阻断无。建议与线索见 `.dev/tasks/2026-11-13-devices-dir.agy.md`（`BOOKLIB_DEVICES_DIR` 为纯相对路径时相对链接可能对不上 mountinfo；`~/Hanvon` 把 `$HOME` 加入 `removable_base` 的隐式约定），均不阻断，本轮不改代码。
- 合并：无阻断，将 `--no-ff` 并入当时的 `origin/master`（`26227e0`，已含他人合并的 vertical-default-off `9777de2`；本单分支基于竖排合并前的 `a14200c`，不基于 vertical 分支）。合并 sha 填在并入并再测、推送之后。状态保持 done，**待 vinci 验收后改 verified**。
