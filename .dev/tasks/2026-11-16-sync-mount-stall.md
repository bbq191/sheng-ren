# sync 在卡死的设备挂载上不再一直挂着

- Task ID: 2026-11-16-sync-mount-stall
- 负责 bot: sheng-ren
- 状态: done
- 执行方: oc-exec
- 分支: task/2026-11-16-sync-mount-stall（从 master `60d4305` 之后的最新 master 切出）
- 更新时间: 2026-11-16T14:07:37+08:00
- 引用: `2026-11-12-lock-no-device`「后续」第 2 条（「`sync` 在卡死的挂载上仍会卡（`deliver::connect` 的 `mounted()`）」）；`2026-11-12-list-no-device-stall`「约束」里留给本单的那条。

## 需求原文
> （vinci 2026-11-16，sheng-ren 聊天）按1-4先做吧
> （第 1 项，负责 bot 同日列出的）sync 卡在挂载上：设备挂载卡住时，sync 会一直挂着不动。这是实际用起来最容易碰上的问题，还没开单，建议先做这个。

现象（来自 2026-11-12 两张单）：汉王 jmtpfs 卡死时，`vertical`、`list` 都卡在 FUSE 上不返回（已修）；`sync` 仍会卡。2026-11-12 box 上用一个对 opendir/lookup 永不回应的 FUSE 小程序复现过，卡住时 `wchan=request_wait_answer`；挂载卸掉后几毫秒返回。真机上卡死场景 vinci 没复现到（2026-11-12 17:04）。

## 输入（开单时 master `60d4305` 的现状，执行时以代码为准）

### sync 在哪碰设备（负责 bot 读代码确认，没在真机上复现）
一轮 `build_all`（`crates/library/src/build_run.rs:149`）的顺序和每一步会不会进挂载点：
1. **连设备** `lib.device_status` → `Devices::target` → `deliver::connect`（`deliver.rs:176`）。`~/Devices/<链接名>` 是符号链接时，`mounted_link` 只读 `/proc/self/mountinfo` 和 readlink，**不进挂载点，不会卡**。但接着：
   - MTP：`mtp_storage(&link)`（`deliver.rs:319`）对链接做 `read_dir` 和每项 `file_type`——**进 FUSE，卡死时第一个卡住的就是这里**（中置信度：按代码和 2026-11-12 的 FUSE 复现推断）。
   - USB 大容量存储（Kobo）：连接时不进挂载点，后面第一次碰设备才卡。
2. **清残留临时文件** `clean_device_leftovers_once`（`lib.rs:563`）：`read_dir` 产物根目录。主线程。
3. **不在跟踪范围的书** `lib.retract`（`generate/records.rs:128`）：删设备上的文件。主线程。
4. **空间预检** `space_check` / `device_free`（`generate/space.rs`）：对设备上的产物 `is_file`、`metadata`，`space::probe_path` 的 `is_dir`，`statvfs`。主线程。
5. **逐本比较、生成** `prepare_typed`（`generate/flow.rs:39`）：认领设备上已有的（`flow.rs:149` `is_file`、`state.rs:219` `read_dir`、`state.rs:245` `same_bytes`、`placement.rs:131/184`）。主线程，和生成（耗 CPU、可能几分钟）混在一起。
6. **传书**：各设备一条传输线程（`transfer.rs` `Pipeline`/`file_lane`）跑 `deliver::put_file`（`deliver.rs:363`、`put_file_once:561`）：`ensure_dir`、`same_bytes` 读回、`std::io::copy`、`sync_all`、`libc::close`（jmtpfs 在 close 时才真正上传，时长和文件大小成正比）、`rename`。主线程 `Pipeline::next(true)` 等结果：**线程卡在 FUSE 上就一直等**，`dead_lane` 只认线程已退出的。`Drop for Pipeline` 还会 `join` 每条线程。
7. **传完记下来** `complete_typed`（`flow.rs:263`）：可能删设备上的旧位置。主线程。
8. **末尾删已删书的旧文件** `flush_removed`（`records.rs:158`）。主线程。

结论：卡死时 sync 不返回；MTP 卡在第 1 步，USB 存储卡在第 2 步；途中才卡住的，卡在 5–8 任意一步或主线程等第 6 步。

### 现有的恢复机制（本单依赖，执行方核对）
- MTP 直接写：`prepare_file` 先在记录里记空指纹再传，写到一半被打断，下次认得出没写完、重传（`deliver.rs:619` 注释）。
- USB 存储经 `.tmp-*` 临时文件再改名；进程被杀留下的 `.tmp-*` 在下次 `sync` 连上这台时清（`lock-no-device`）。
- 书库锁是 flock，进程退出自动放开；书库里的 `.tmp-<id>-<模式>/` 下次拿到锁时清。
- 所以「进程在写设备途中结束」和 Ctrl-C 一样，已有恢复：不会把半截文件记成传好了。

## 方案

### 备选和取舍
| 方案 | 做法 | 能覆盖 | 不足 |
|---|---|---|---|
| A 只在连接时探测 | `connect` 里进挂载点的那一步放到辅助线程，限时；超时报「挂载没响应」、这台这一轮跳过 | sync 开始时已经卡死（最常见、2026-11-12 复现的就是这种） | 连上以后途中卡住照样挂 |
| B = A + 传输看门狗 | 传输线程按写进度打心跳；主线程等结果时发现某台太久没进展，就放弃这台（不再等、不 join），报错、这台这一轮不再传，别的设备照旧 | 再加上传书途中卡住（第 6 步，耗时最长、最可能碰上） | 主线程自己碰设备的那几步（2–5、7、8）卡住仍会挂 |
| **C = B + 主线程兜底（选这个）** | 主线程碰设备的地方包一层计时；超过时限由看门狗线程打出中文错误、以退出码 2 结束整个进程 | 任何一步卡住都不会无限挂起 | 兜底触发时这一轮别的设备也一起中断（等于 Ctrl-C，靠上面已有的恢复机制，下次重传）；是最后手段，概率低 |
| 否决：把所有设备 I/O 挪到每台设备一个工作线程、主线程只发请求 | 能做到卡一台只停一台 | 重构 `generate` 全部设备操作，风险和改动量都太大 |
| 否决：`poll`/非阻塞 IO、子进程探测 | 对 FUSE 的 `stat`/`readdir`/`close` 无效；子进程开销大、平台相关 | — |

选 **C**：只有它满足「不会无限挂起」；A、B 两部分处理绝大多数情况、只停卡住的那一台，兜底只管剩下的少数情况。

### 和已定事项的关系（执行方别改回去，结果里如实写）
- `docs/decisions.md` `list-no-device-stall`（:362）否决过「后台线程加超时」（怕卡死的 FUSE 调用留下 D 状态线程、进程退出时也卡）。那是针对 `list` 的，`list` 的做法（不碰设备）不动。`sync` 必须写设备，躲不开，只能限时。FUSE 请求的等待可被致命信号打断（卡住时 `wchan=request_wait_answer`，2026-11-12 复现时 SIGKILL 能结束），所以放弃的线程不会挡住进程退出——**验收要实测**。内核文件系统（USB 存储的 vfat）因为 USB 硬件卡死进入不可中断的 D 状态时，进程退出也可能等它，这是已知局限，写进文档。
- `decisions.md`（:372）「接没接上看链接和 mountinfo，不 stat 目标」不变：接没接上的判定照旧；探测是在判定「接上了」之后、限时地进挂载点看一眼能不能用。

## 规则（要实现的行为）
1. **时限**：基数 `T` 缺省 30 秒，环境变量 `BOOKLIB_DEVICE_TIMEOUT=<秒>`（正整数）可改（测试用小值；写进 `--help` 的环境变量段和 `docs/usage.md`）。
   - 连接探测：`T`。
   - 主线程单次设备操作：`2T`。
   - 传输：写的途中连续 `2T` 没有进展（没有一块写成功）算卡住；`sync_all`、`close`、`rename`、读回比较这类一次调用就要把整本传完或读完的，时限 `2T + 文件字节数 / (256 KiB/秒)`（MTP 慢，不能误判大漫画）。
2. **连接探测**（`deliver::connect`，所有 `Target::Dir`，开不开挂载检查都做）：mountinfo 判定接上以后，把「进挂载点」的那部分（MTP 的 `mtp_storage`；USB 存储至少对链接 `read_dir` 一次）放到辅助线程，限时 `T`。超时：`Err("挂载没响应（<链接> <T> 秒没回应），这一轮跳过；拔下重插或重新挂载后再同步")`（措辞执行方可微调，要中文、带路径、说怎么办）。走现有 `DeviceAbsent` 路径（`sync` 报一次、`--watch` 每台只报一次；`booklib devices` 也显示这个原因）。超时的辅助线程不 join，放着。
3. **还没返回的放弃线程**：进程里记一份「挂载点 → 还有没返回的放弃线程」（线程结束时自己清掉标记）。有的话这台直接按规则 2 的错误跳过（措辞写「上次访问还没返回」），**不再起新线程、不再碰这台**——`--watch` 不会每轮堆一个卡死线程，也不会和还在写的旧线程抢同一个文件。线程返回后，下一轮照常探测。
4. **传输看门狗**（`transfer.rs`）：每条传输线程有一份进度（最近一次进展的时刻、当前这步的时限），`put_file` 的复制改成分块循环、每块写成功就更新；进入规则 1 里的长调用前设好那一步的时限。`Pipeline::next` 等结果时（`wait` 和不 `wait` 都看）发现某台在传、而且超过时限没有进展：把这条线程放弃（从 `lanes` 拿掉，丢掉 `JoinHandle`，不 join；记进规则 3 的那份），返回新事件（例如 `Event::Stalled { device, count, secs }`），手上和排队的 `count` 件都算没传成。`build_run` 按「这台中途掉线」处理（进 `counts.dropped`、`failed += count`、后面的书不再给这台生成），报一行，例如 `✗ <设备>：挂载没响应（<secs> 秒没有进展），这一轮不再往它传；<count> 本没传成，记录没改，下次重传`。别的设备照常传完。被放弃的线程之后若交回结果，一律丢掉（记录不能改；`busy` 计数不能被减成负数或减错别的设备）。`Drop for Pipeline` 不 join 放弃的线程。
5. **主线程兜底**：主线程上碰设备的调用（「输入」里的 2、3、4、7、8，以及 5 里认领时碰设备的那几处——只包设备操作，**不能把生成本身包进去**）在开始前登记「设备、在做什么、开始时刻」，结束后取消。一个看门狗线程（第一次登记时起，整个进程一个）每秒看一次，超过 `2T` 的：往 stderr 打 `✗ <设备> 挂载没响应（<在做什么> <秒> 秒没返回），booklib 退出；没传完的书记录没改，下次接上会重传`，然后 `std::process::exit(2)`（退出码表不变，2 就是「有书处理失败」）。
6. **不留半写文件被当成完好**：放弃或退出时，正在写的那本记录不改（直接写的已有空指纹，暂存写的是 `.tmp-*`）。执行方要核对这两条确实成立，并加测试：放弃一条线程后，生成记录里那本不是「传好了」，下一次（解除卡死后）`sync` 会重传或认领逐字节相同的那份。
7. **不卡的设备行为不变**：没超时的情况下，连接、生成、传输、记录、输出、退出码都和现在一样；产物逐字节不变。
8. **测试钩子**：环境变量 `BOOKLIB_TEST_DEVICE_HANG=<目录>`，设备 I/O 进到这个目录下的路径时（连接探测、传输写、主线程兜底包住的那几处）就一直 sleep，模拟卡死的挂载；照 `BOOKLIB_TEST_DEVICE_IO_PANIC` 的写法，只在这些入口检查，正式路径没设就不影响。名字可以调整，结果里写明。

## 分步任务
1. [ ] 核对「输入」里每一步的位置和会不会进挂载点；有漏的补进结果。
2. [ ] 新模块（例如 `crates/library/src/stall.rs`）：时限、限时跑一个闭包（辅助线程 + `recv_timeout`）、放弃线程的登记（规则 3）、主线程兜底的登记和看门狗（规则 5）、测试钩子（规则 8）。
3. [ ] `deliver::connect` 接上探测（规则 2、3）。
4. [ ] `deliver::put_file` 分块复制打心跳；`transfer.rs` 的进度、`Pipeline::next` 放弃线程、新事件、`Drop` 不 join 放弃的（规则 4）；`build_run.rs` 处理新事件；`booklib` 的输出行。
5. [ ] 主线程设备操作包上兜底（规则 5）。
6. [ ] 测试（见验收标准）。`docs/usage.md`（sync、设备那几段，`:375` 附近加一条「挂载卡死」）、`docs/decisions.md` 加一行（含选 C 的理由、和 `list-no-device-stall` 否决「线程超时」的关系、D 状态的已知局限）、`--help` 环境变量段。
7. [ ] `cargo test --workspace` 全过；`cargo clippy --workspace --all-targets -- -D warnings` 0 警告；确认版本号、`fp_golden.txt`、指纹构成都没动。

## 验收标准
- [ ] 单测：限时跑闭包——按时返回的拿到结果；超时的返回超时且不 join；放弃登记在线程结束后清掉。
- [ ] 单测：`Pipeline` 里一条线程卡住（测试里用一直 sleep 的假传输）、另一条正常：`next(true)` 在时限后返回 `Stalled`，正常那条的结果照常拿到，`in_flight` 归零，`drop` 不挂；卡住的那条之后交回的结果被丢掉。
- [ ] 集成测试（`crates/library/tests/`，临时书库、临时目录当设备，子进程**带超时**，测试本身永远不会挂住）：
  - 设备目录在 `BOOKLIB_TEST_DEVICE_HANG` 下、`BOOKLIB_DEVICE_TIMEOUT=1`：`booklib sync` 在几秒内结束，报「挂载没响应」，那台没传；另一台正常设备照常传完；退出码按现有规则。
  - 传输途中卡住（钩子只对写正式文件生效，或别的办法）：sync 在时限内结束、报这台没传成；生成记录里那本不是传好了；去掉钩子再 `sync`，那本传上且内容逐字节对。
  - `booklib devices` 对卡住的设备在时限内返回并显示原因。
- [ ] 不卡的时候：现有测试全部照过，不改已有断言（要改的逐条写原因）。
- [ ] 产物逐字节不变（负责 bot 验收时用改动前 master 二进制对照）；版本号不变；`fp_golden.txt` 一行没动；指纹构成没动。
- [ ] `docs/usage.md`、`docs/decisions.md`、`--help` 同步。
- [ ] `cargo test --workspace` 全过；clippy `-D warnings` 0 警告。
- [ ] 结果里写明：没跑真书回归、没上真机；卡死场景只在 box 上用钩子（和负责 bot 验收时的 FUSE 小程序）模拟过。

## 不做什么
- 不改「接没接上」的判定（mountinfo + 链接），不改 `list`、加锁清理的「不碰设备」做法。
- 不重构成「每台设备一个工作线程承担全部设备 I/O」。
- 不改产物、生成记录格式、指纹、版本号；不改退出码表。
- 不自动卸载、重挂设备，不碰 dotfiles 管的链接。
- 不处理内核文件系统因硬件卡死进入不可中断 D 状态时进程退不出（写进文档当已知局限）。
- 新子目录里清不掉的残留文件是下一张单（第 2 项），这里不碰。

## 约束
- 只用 Rust；clean-room；仓库不出现上游项目名及缩写。
- 书的内容一个字不多一个字不少；**产物逐字节不变**。
- 不升版本号；不改指纹构成；`fp_golden.txt` 不动。
- 不碰 `~/.local/share/booklib`、`~/Documents/ereader/books`、真机；实验用 `target/` 下临时 `--library`；测试不碰真设备、不联网，子进程测试都带超时。
- `install.sh` 不用 `--force`。别对整个仓库跑 `cargo fmt`。
- 只在任务分支上工作，不合并；改「涉及文件」以外的写进「结果 → 偏离」。
- 提交信息中文 + 前缀 + 正文写原因，不加 Co-Authored-By。
- 风险点：设备行为（放弃线程、进程兜底退出）→ 要 agy 二审。

## 涉及文件（预计）
- `crates/library/src/stall.rs`（新）、`crates/library/src/lib.rs`（挂模块）
- `crates/library/src/deliver.rs`（`connect`、`put_file` 分块复制）
- `crates/library/src/transfer.rs`、`crates/library/src/build_run.rs`
- `crates/library/src/generate/*.rs`、`crates/library/src/lib.rs`（主线程设备操作包兜底）
- `crates/library/src/bin/booklib/main.rs`（新事件的输出行、`--help` 环境变量）
- `crates/library/tests/`（新增用例）、`crates/library/tests/common/mod.rs`（如需要）
- `docs/usage.md`、`docs/decisions.md`

## 结果
- 执行方 / 开始–结束时间：box-agent（oc-exec） / 2026-11-16T13:53:41+08:00–2026-11-16T14:07:37+08:00
- base..head：`863fe269dfcb60f526d1c6a154c62100152526ff..96bab7f4d134a0614915874353b86ef7c41c1daa`
- 提交：
  | 提交号 | 说明 |
  |---|---|
  | 9002af9 | chore: 开始执行任务单 2026-11-16-sync-mount-stall [skip-notify] |
  | 96bab7f | fix: sync 在卡死的设备挂载上不再一直挂着 |
- 改动文件：`.dev/tasks/2026-11-16-sync-mount-stall.md`,`crates/library/src/bin/booklib/main.rs` `crates/library/src/build_run.rs`,`crates/library/src/deliver.rs` `crates/library/src/generate.rs`,`crates/library/src/generate/flow.rs` `crates/library/src/generate/records.rs`,`crates/library/src/generate/space.rs` `crates/library/src/lib.rs`,`crates/library/src/stall.rs` `crates/library/src/transfer.rs`,`crates/library/tests/stall.rs` `docs/decisions.md`,`docs/usage.md`
- 测试：包装器未代跑验收命令，由负责 bot 按任务单核对
- 遇到的问题：无
- 偏离：
  - `crates/library/src/generate.rs`
  - （负责 bot 验收补）`crates/library/src/fsutil.rs`、`crates/library/src/generate/state.rs`：验收修正 `5f06963` 改的（书库锁描述符登记、`sha256_file` / `clean_tmp_files` / `delete_placements` 里的 `stall::beat()`），执行方没改这两个文件；agy 指出漏报
- 新建文件（线索）：无
- 未完成：无
- 运行记录：`/home/box/oc-runs/2026-11-16-sync-mount-stall-20261116-135341.json`
- 日志：`/home/box/oc-runs/2026-11-16-sync-mount-stall-20261116-135341.log`
- 模型：`opencode-go/deepseek-v4.1-flash`
- 用时：实际运行 13.9 分钟（墙钟 836s / 实际运行 836s；超时按实际运行计）

## 审查
- 负责 bot 验收（2026-11-16，独立 worktree，基于 `c4e0f07`）：**有一处根本问题，已在任务分支上修正（`5f06963`），修正后通过**。
  - 逐个看了 diff：连接探测（`deliver::connect` 的 `probe`）、放弃线程登记（`stall::outstanding`）、传输心跳和 `Pipeline` 放弃线程（不 join、之后的结果丢掉）、`build_run` 的 `Stalled`、主线程 `device_op` 包住的位置（清残留、`retract`、`flush_removed`、空间预检、认领、选文件名、核对、`complete` 删旧位置、记录迁移）都和任务单一致；生成本身没被包进去。
  - **真卡死复现**（scratch FUSE 小程序，不进仓库：内存文件系统，触发后每个请求都永不回应；`~/Devices/Ireader` 是指向它的符号链接，开着挂载检查，`BOOKLIB_DEVICE_TIMEOUT=3`，临时书库在 `target/` 下）：
    1. 一开始就卡死：master `sync --device=ireader` 20 秒后被 SIGKILL（卡在 opendir）。执行方的版本 3 秒报「挂载没响应…这一轮跳过」、打完汇总，**但进程退不掉**：主线程退出后进程是僵尸，探测线程 D 状态停在 `request_wait_answer`，`kill -9` 也结束不了，要等 FUSE 守护进程回应或被杀。命令行照样回不来，书库锁（flock）也不放。也就是说任务单「FUSE 请求的等待可被致命信号打断」的推断**不成立**（请求已交给守护进程后内核不可中断地等），执行方写进 `decisions.md` 的「box 上用钩子实测过」也不对（钩子只是 sleep，碰不到这个问题）。
    2. 修正：`sync`、`devices` 改由前台进程起工作进程跑，工作进程退出一律走 `library::exit_process`（先放书库锁、刷输出、把退出码写进 `BOOKLIB_EXIT_FD` 管道），前台读到就退出。修正后：一开始就卡死 → 3.1 秒返回、退出码 0（同「没接上」，不算失败），工作进程留在后台成僵尸；紧接着另一个要加锁的命令照常拿到锁。
    3. 传书途中卡死（第 2 个写请求起卡住，两本书）：7 秒返回、退出码 2；传输线程卡在 write，主线程认领下一本时也卡住，兜底看门狗打 `✗ [ireader] 挂载没响应（选设备上的文件名 6 秒没返回），booklib 退出…`。生成记录里那本的指纹是空的（认得出没写完）。杀掉守护进程、重新挂一个好的后再 `sync`：两本都传上，设备上文件的 sha256 和记录一致；后台僵尸随守护进程结束消失。
    4. 只有一本书、传输途中卡死：6 秒报 `✗ [ireader] 挂载没响应（6 秒没有进展），这一轮不再往它传；1 本没传成…`，失败 1 本、退出码 2，进程正常回来。（修正前这里要 15 秒：跨文件系统改名失败后沿用了长调用时限，已改。）
    5. `--watch=2` 卡死 14 秒：「挂载没响应」只报一次，后台僵尸里只有 1 条卡住的线程（没有每轮堆一条）。
  - 其余修正（同在 `5f06963`）：主线程兜底改成按「没有进展」计时——`same_bytes`、`sha256_file` 每读一块、成批删除和清残留每个文件调 `stall::beat()`（只刷新本线程登记的操作），读回大漫画、删一长串旧文件不会被误判卡死而结束整个 sync；没开挂载检查、MTP 链接不在时恢复原来的「不在」措辞；兜底那行改成 `✗ [设备]`；单测 `watch_registers_and_clears_main_thread_ops` 不再假设全局登记表只有自己一项（并行跑会偶发失败）；`docs/usage.md`、`docs/decisions.md` 按实测重写，`--help` 措辞调整。
  - 二审后（agy 建议 1、2、线索 1）：`stall.rs` 模块注释改成实测结论；偏离补登 `fsutil.rs`、`generate/state.rs`；放弃一条传输线程时给它打取消标记，卡住的那件返回后排着的不再写设备、线程直接退出（`--watch` 进程还活着、设备又回应时不会多写）。
  - 已知局限（不阻断，写进文档）：没开挂载检查（测试）时传输线程的放弃登记用产物路径、连接探测用链接，`--watch` 下一轮认不出「上次还没返回」，只影响测试环境；卡死期间每跑一次 `sync` 后台可能多留一个僵尸工作进程，设备回应或拔掉后消失。
  - 产物逐字节不变：同两本测试书（一本带 2MB 图），master 与分支二进制分别 `sync` 到 ireader、kobo、kindle（普通目录当设备），6 个产物 sha256 全同；输出文字也相同。版本号、`fp_golden.txt`、指纹构成都没动。
  - `cargo test --workspace` 860 通过 0 失败；`cargo clippy --workspace --all-targets -- -D warnings` 0 警告。
  - 没跑真书回归、没上真机；卡死场景只在 box 上用测试钩子和 FUSE 小程序模拟过（真的 jmtpfs 卡死时是不是同样停在 `request_wait_answer`、拔掉设备后僵尸是否消失，要 vinci 真机看）。
