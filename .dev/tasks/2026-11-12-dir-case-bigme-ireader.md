# bigme / 掌阅产物目录改成设备真名 `Books` / `Documents`

- Task ID: 2026-11-12-dir-case-bigme-ireader
- 负责 bot: sheng-ren
- 状态: pending
- 执行方: box-agent
- 分支: task/2026-11-12-dir-case-bigme-ireader（从最新 master 切出）
- 更新时间: 2026-11-12T17:10+08:00
- 引用: 2026-11-12-hanvon-documents-case（后续第 ② 条）；2026-11-12-quality-audit 发现 5

## 需求原文
> （vinci 2026-11-12）bigme下有books错误的同步文件夹，应该是Books
> （bot 说明是和汉王同样的问题：`bigme.toml` 写小写 `books`，设备真名是 `Books/`；11-10 加模式时以为安卓存储不分大小写就没事，后来汉王现场发现 MTP 会按写入时的大小写再登记一个同内容的小写目录。提出照搬汉王 `d94eec0` 的做法，并指出掌阅是同样的问题：`documents` vs `Documents/`。`docs/decisions.md` 同日修汉王时写过「掌阅、Bigme 的 dir 本次不改」，要改就得在 decisions 里补一笔。）
> （vinci 2026-11-12 17:08）bigme 和掌阅一起改，开单执行

## 输入
- 基于 origin/master `e2ea644`。汉王那次做法见提交 `d94eec0`（`fix(hanvon): 产物目录改成 Documents，只差大小写的目录只改记录`）和任务单 `.dev/tasks/2026-11-12-hanvon-documents-case.md`。
- 现状：
  - `crates/profile/profiles/bigme.toml:52`：`dir = "books"`，注释写了 xReader 认 `Books/`、安卓存储不分大小写。
  - `crates/profile/profiles/ireader.toml:110`：`dir = "documents"`，注释写了 Air 3 Pro 自带是 `Documents/`、外部存储不分大小写（2026-11-02）。
  - 汉王已经改成 `Documents`，并有 `case_alias` 生成记录迁移（只改记录、不碰设备上的文件）和删除保护（`put_file` / `delete_placements`）。这套逻辑按路径大小写工作，不绑死设备 id，改 palme / 掌阅的 `dir` 会直接复用。
  - `[deliver]` 不进指纹（`docs/devices.md`「不影响产物内容，不进指纹」），改 `dir` 不变指纹、不升版本号，`fp_golden.txt` 不变。
- 冲突：`docs/decisions.md` 2026-11-12「汉王产物目录改成 Documents」那条写明「Kindle、掌阅、Bigme、Kobo 的 dir 不动」「掌阅、Bigme 也是小写名、可能有同样的重影，本次不改，靠删除保护兜底」。本次推翻其中掌阅、Bigme 的「不改」，要在 decisions 追加一条写清，不能悄悄改回去。Kindle、Kobo 仍不改。
- 测试夹具：`crates/library/tests/common/mod.rs` 的 `documents()` 把产物根目录写死成 `<存储>/documents`，掌阅改成 `Documents` 后，所有用这个函数查掌阅路径的用例都会看错目录，必须改成按 profile 的 `dir` 取值（Kindle 仍是小写 `documents`）。

## 约束
- 只用 Rust；clean-room；仓库不出现上游项目名及缩写。
- 不碰 `~/.local/share/booklib`，实验用 `target/` 下临时 `--library`；测试不碰真机。
- **不升版本号**：`[deliver]` 不进指纹，产物内容不变；`fp_golden.txt` 一行不动。若测试表明产物变了则停下报告。
- **不写删除或重命名设备上旧目录的迁移**。存储不分大小写，删 `books` 就是删 `Books`。多出来的小写 MTP 登记重新挂载后应该会消失。
- `install.sh` 不用 `--force`。
- cargo 用 rustup stable（系统 1.85 太旧）：`PATH` 里放上 `$HOME/.cargo/bin`。
- 只在上面的分支工作；不合并、不跑二审；提交信息中文 + 前缀 + 正文，不加 Co-Authored-By；身份 `afu` / `377810+bbq191@users.noreply.github.com`。
- 本仓库不用 rustfmt 默认宽度，别对整个仓库跑 `cargo fmt`；提交前用 `cargo fmt --check` 只检查本次改动是否干净。

## 规则（要实现的行为）
1. `bigme.toml` 的 `[deliver] dir` 改成 `Books`，`ireader.toml` 改成 `Documents`。注释改成与汉王一致的说明（小写名让 MTP 多登记一个同内容的目录，改成设备真名；旧记录由大小写迁移原样接过来）。
2. 复用现成的 `case_alias` 迁移和删除保护：设备接上后只改生成记录，不重传、不删书。大小写敏感的设备上新路径不在就不迁移，走原来的挪位置流程（宁可多留旧文件）。
3. `crates/profile/src/lib.rs` 里对 bigme / 掌阅 `dir` 的断言改成新值。
4. `booklib sync` 帮助（`crates/library/src/bin/booklib/main.rs`）：bigme `Books/`、掌阅 `Documents/`、汉王 `Documents/`（汉王帮助里还写着小写 `documents/`，一并改掉）。
5. 比照汉王 `flow.rs` 三则测试（不分大小写符号链接模拟、大小写敏感双目录、orphans 迁移），为 bigme 和 ireader 补测试；可抽共用夹具。
6. 文档更新现状：`docs/usage.md`、`docs/devices.md`、`docs/architecture.md`、`README.md`。`docs/decisions.md` 追加一条 2026-11-12 决定，明确推翻同日「掌阅、Bigme dir 本次不改」，理由是 vinci 真机发现 bigme 下出现错误的 `books` 同步目录；同日那条标「已改」。历史观察（如掌阅书架数据库里的「documents」分组）保持原样，只改描述**当前**投递目录的文字。
7. 更新 `.dev/tasks/2026-11-12-quality-audit.md` 发现 5 的状态：vinci 已定一起改，本单在做。
8. 可选：若改动很小，把 `build_run` 的连接清理放到 `rebase_state_case` 之后，以覆盖首次迁移的 `.tmp-*`；不小则不做，在本单结果里记成后续。

## 分步任务
1. [ ] 改 `bigme.toml`、`ireader.toml` 的 `dir` 和注释；更新 `profile/src/lib.rs` 断言。
2. [ ] 改 `booklib` 帮助文字（含汉王仍写小写的那行）。
3. [ ] 改 `common::documents()`，让它按该模式 profile 的 `dir` 拼路径，已有用它查掌阅路径的用例跟着过。
4. [ ] `flow.rs`：抽共用夹具（或复制后参数化），为 bigme（`books`→`Books`）和 ireader（`documents`→`Documents`）各补三则测试，覆盖汉王那三种情形。已有汉王三则保持通过。
5. [ ] 更新 `docs/usage.md`、`docs/devices.md`、`docs/architecture.md`、`README.md`、`docs/decisions.md`；更新 quality-audit 发现 5。
6. [ ] 评估可选的连接清理时机；做或不做写进结果。
7. [ ] `cargo test --workspace` 全过；`cargo clippy --workspace --all-targets -- -D warnings` 0 警告；`cargo fmt --check`。确认 `fp_golden.txt` 和版本号都没动。

## 验收标准
- [ ] bigme 投递到 `Books/`，掌阅投递到 `Documents/`；旧小写记录在不分大小写的设备上只改记录、inode/mtime 不变、再同步仍 UpToDate；大小写敏感时复制到新目录并保留旧文件；orphans 也会迁到新根再删。
- [ ] 帮助文字和文档现状与 profile 一致；decisions 写清推翻同日决定。
- [ ] 版本号不变；`fp_golden.txt` 一行没动；`git diff` 对 `crates/bookconv`、`crates/kfx` 为空。
- [ ] `cargo test --workspace` 全过；clippy `-D warnings` 0 警告。
- [ ] 结果里写明没上真机、没跑真书回归（改的是投递目录，不影响正文；说明理由）。真机由 vinci 验：接上 Bigme / 掌阅后，同步目录是 `Books/` / `Documents/`，已有的书不重传、不丢。

## 交付物
代码提交到上面的分支并推送；「结果」填好；状态改为 done 或 blocked。任务单状态改动推 master，带 `[skip-notify]`。不要合并，不要跑二审。

## 给执行方的指令
```
执行任务单 .dev/tasks/2026-11-12-dir-case-bigme-ireader.md：先把状态改为 accepted 并提交，按「分步任务」做完，填好「结果」，状态改为 done（做不下去改为 blocked 并写明原因），提交并推送分支。任务单 done 推 master 带 [skip-notify]。不要合并，不要跑二审。
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
- 真机 / 真书回归：没上真机，没跑真书回归。

## 审查
<!-- 负责 bot 填写 -->
- 验收：
- 二审：
- 合并：
