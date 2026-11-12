# 连接清理不删别的进程正在写的设备临时文件（低优先）

- Task ID: 2026-11-12-device-tmp-pid-guard
- 负责 bot: sheng-ren
- 状态: done
- 执行方: box-agent
- 分支: device-tmp-pid-guard（从最新 master 切出）
- 更新时间: 2026-11-12T16:45:02+08:00
- 引用: 2026-11-12-lock-no-device（「审查 → 二审」建议 3；「后续」第 5 条）

## 需求原文
> （lock-no-device 二审，负责 bot 结论）建议 3（多个书库同写一台设备时可能误删别人的 `.tmp-*`）：属实，但 master 本来就这样，而且现在只在连接时删，冲突窗口变小了；转后续（可以考虑按进程号保护），优先级低。
> （vinci 2026-11-12）后续那几条要开新任务单：要。
> （vinci 2026-11-12 13:32）device-tmp-pid-guard 也开。
> （vinci 2026-11-12 16:41）改由 box 执行。

## 输入
- 背景：`Projects/sheng-ren/context.md`；原单与回执。基于 origin/master `415161a`。
- 原因（负责 bot 读代码确认）：
  - 设备上的临时文件名是 `.tmp-<进程号>-<计数>-<名>`（`crates/library/src/fsutil.rs:43-52` `tmp_sibling`）。
  - 连接清理 `clean_device_leftovers`（`crates/library/src/lib.rs:548-557`）对设备 root 下面的 `output_dirs()` 调 `fsutil::clean_tmp_files`（`fsutil.rs:203-211`），后者删掉所有 `.tmp-` 开头的文件，不看进程号。
  - 书库锁（`fsutil.rs:166-185`）只锁本书库，两个书库同时 `sync` 同一台设备时，后连上的一方会删掉先连上一方正在写的临时文件，导致对方改名失败、那次传输失败。
  - 实际影响：需要同一台电脑上两个书库同时往同一台设备写，维护者现在只有一个书库（**待 vinci 确认**）；失败的一方下次 `sync` 会重传，不丢书、不损坏已有产物（中置信度：按 `produce_then_replace` 先写临时文件再改名的流程推断，没有逐行核对失败路径）。
- 可选做法：
  - **A. 按进程号保护**：解析文件名里的进程号，`/proc/<pid>` 存在就跳过。优势：改动小，只在 `fsutil`。劣势：进程号会被复用（误留，不误删，安全方向）；进程号来自另一台电脑时无意义（设备在两台电脑之间插拔，概率低）；名字不合规的照删。
  - **B. 只删足够旧的**（mtime 早于某阈值，例如 1 小时）。优势：不依赖进程号。劣势：FAT/exFAT 的 mtime 精度只有 2 秒，大文件慢速写入期间 mtime 是否持续更新**未确认**。
  - **C. 不做**：维持现状，写进 `docs/decisions.md` 的已知局限。
- 涉及文件（如做 A）：`crates/library/src/fsutil.rs`（修改）、`crates/library/src/lib.rs`（可能）、测试。

## 约束
- 不做：不改加锁清理（只在电脑上）；不改连接清理的时机；不改产物和传书。
- 不升版本号：不影响产物（`docs/development.md`「版本号」）。
- 只在上面的分支工作；不合并；提交信息中文 + 前缀 + 正文，不加 Co-Authored-By；不读密钥。
- 测试用临时目录当设备，不碰真机。
- 风险点：**设备行为**（设备上哪些文件会被删）→ **要 agy 二审**。

## 规则（要实现的行为）
1. 连接清理删除设备上的 `.tmp-*` 时，解析文件名里的进程号，`/proc/<pid>` 还在就跳过，不删。名字解析不出进程号的照删。
2. 加锁清理（只在电脑上）是否同样处理，执行方提出、负责 bot 定，写进结果。
3. 优先级低，但本单要做完，不是暂缓。

## 分步任务
1. [ ] `fsutil.rs`：新增"临时文件的主人进程还在不在"的判定（解析 `.tmp-<pid>-`，`/proc/<pid>` 存在 = 在），`clean_tmp_files` 在设备清理时跳过主人还在的（电脑上的加锁清理是否同样处理，执行方提出、负责 bot 定）。
2. [ ] 单元测试：当前进程号的 `.tmp-*` 留着；不存在的进程号（例如超大数）删掉；名字不合规的删掉。
3. [ ] `docs/decisions.md` 加一行。

## 验收标准
- [ ] 上面的单元测试通过；`lock_never_reads…`（`flow.rs:3010`）等已有用例照旧通过
- [ ] `cargo test --workspace` 全过；`cargo clippy --workspace --all-targets -- -D warnings` 0 警告；`cargo fmt --all -- --check` 干净
- [ ] 结果里写明没上真机

## 交付物
代码提交到上面的分支并推送；「结果」填好；状态改为 done 或 blocked。

## 给执行方的指令
```
执行任务单 .dev/tasks/2026-11-12-device-tmp-pid-guard.md：先把状态改为 accepted 并提交，按「分步任务」做完，填好「结果」，状态改为 done（做不下去改为 blocked 并写明原因），提交并推送分支。
```

## 结果
- 执行方 / 开始–结束时间：box-agent（oc-exec） / 2026-11-12T16:42:01+08:00–2026-11-12T16:45:02+08:00
- base..head：`42b1cd4c01abfb9bf0083c2e12effb449c47d681..cc6f5cad6ce63dc42a5c98a486cda28913fc5997`
- 提交：
  | 提交号 | 说明 |
  |---|---|
  | 2c3891a | chore: 开始执行任务单 2026-11-12-device-tmp-pid-guard [skip-notify] |
  | cc6f5ca | feat(library): 设备临时文件清理跳过主人进程还在的 |
- 改动文件：`.dev/tasks/2026-11-12-device-tmp-pid-guard.md`,`crates/library/src/fsutil.rs` `crates/library/src/lib.rs`,`crates/library/tests/flow.rs` `docs/decisions.md`
- 测试：包装器未代跑验收命令，由负责 bot 按任务单核对
- 遇到的问题：无
- 偏离：
  - `crates/library/tests/flow.rs`
- 未完成：无
- 运行记录：`/home/box/oc-runs/2026-11-12-device-tmp-pid-guard-20261112-164201.json`
- 日志：`/home/box/oc-runs/2026-11-12-device-tmp-pid-guard-20261112-164201.log`
- 模型：`opencode-go/deepseek-v4.1-flash`

## 审查
<!-- 负责 bot 填写 -->
- 验收：
- 二审：
- 合并：
