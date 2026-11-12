# 文档对齐：版本号、设备清单、分支规则，大小写判断补注释

- Task ID: 2026-11-12-docs-align
- 负责 bot: sheng-ren
- 状态: done
- 执行方: opencode-local
- 分支: docs-align（从最新 master 切出）
- 更新时间: 2026-11-12T11:00+08:00
- 引用: 2026-11-12-quality-audit（第 2、3、4 条，以及「核实与分组」里的小改动 1–5）

## 需求原文
> （vinci 2026-11-12）质量审查第 2 张任务单「文档对齐」（第 2、3、4 条）和第 1 张一起开。分支规则以 `Projects/工作流.md` 为准：负责 bot 验收、拿到 agy 二审后自己合并；任务单文件可以直接提交在 master。这张单要把 `AGENTS.md`、`CONTRIBUTING.md` 改成跟它一致。小改动都并进这张单，不另做。

## 输入
- 核实记录：`.dev/tasks/2026-11-12-quality-audit.md` 的「核实与分组」。
- 现在的事实（写文档时以代码为准，自己再核一遍）：
  - 版本号：`kfx/src/write/mod.rs:116` `WRITER_VERSION = "26"`；`bookconv/src/kepub.rs:19` `VERSION = 2`；`OPTIMIZE_VERSION` / `COMIC_VERSION` 68（`bookconv/src/optimize/mod.rs:171`、`:208`）。
  - 内置模式五个：`kindle`（KFX，MTP `documents/`）、`ireader`（掌阅，EPUB，MTP `documents/`）、`kobo`（KEPUB，USB 大容量存储，卷标 `KOBOeReader`，`books/`）、`hanvon`（汉王，EPUB，MTP `Documents/`）、`bigme`（EPUB，MTP `books/`），以 `crates/profile/profiles/*.toml` 的 `[deliver]` 为准。
  - 分支和合并：开发在任务分支上做；任务单文件（`.dev/tasks/<ID>.md`）可以直接提交在 master；负责 bot 验收，vinci 手动跑 agy 二审，没有阻断时负责 bot 用 `git merge --no-ff` 合并；vinci 做最终验收（真机）。流程的正本是维护者笔记库里的 `Projects/工作流.md`。
- 涉及文件（只改这些）：
  1. `docs/development.md:105`：版本表 `kfx::write::WRITER_VERSION` 现值 25 → 26（顺带核对表里其他现值和代码一致）。
  2. `CONTRIBUTING.md`：
     - `:7` "两个模式：kindle、ireader" 改成五个内置模式（各一句：阅读器、格式）；`xochitl` 已删的说明保留一句；
     - `:20` 分支规则改成上面那套（保留"影响产物的改动写明没跑真书回归、没上真机"）；
     - `:44-47` 版本号清单补上 `bookconv::kepub::VERSION`（只进出 KEPUB 的模式）；
     - `:58` `--device=kindle|ireader` 改成列出五个模式（或写"模式 id，见 `crates/profile/profiles/`"）。
  3. `AGENTS.md:8`（第 4 条）：分支规则改成和 `CONTRIBUTING.md` 一致（短一句，指向 `CONTRIBUTING.md`）。
  4. `docs/architecture.md`：
     - `:181` 产物位置补全：Kindle、掌阅 `documents/`，汉王 `Documents/`，Bigme `books/`（MTP），Kobo `books/`（USB 大容量存储）；
     - `:232` "进程被杀留下的临时文件下次拿到锁时清掉"改成：书库里的（和电脑上的产物目录）下次拿到锁时清，设备上的由 `sync` 连上那台设备后清（任务单 `2026-11-12-lock-no-device`）。**这一行描述的是 lock-no-device 合并以后的行为**：本单合并要排在它之后，负责 bot 合并时核对。
  5. `README.md:31`「前提」一段：补上汉王、Bigme 的 jmtpfs 挂载点（`$XDG_RUNTIME_DIR/mtp/hanvon`、`…/mtp/bigme`，以 profile 的 `mount` 为准）。
  6. `crates/library/src/generate.rs`：**只改注释**，给 `case_twin`（`:1508`）、`case_alias`（`:1521`）、`distinct_on_disk`（`:1539`）写清各管哪种情况：`case_twin` 是同一目录里只差文件名大小写，是 `case_alias` 的特例；`case_alias` 是逐段只差大小写，可以跨目录；`distinct_on_disk` 只列一个目录，判断不了跨目录的别名，所以跨目录时不调它。

## 约束
- 不做：不改任何代码行为（`generate.rs` 只动 `//` / `///` 注释，`git diff` 里不能有非注释行）；不改 `docs/decisions.md`（归 lock-no-device）；不改 `crates/library/src/lib.rs`、`session.rs`、`build_run.rs`、`fsutil.rs`、`tests/`（归 lock-no-device）；不改 `docs/devices.md`、`docs/usage.md`（汉王那单已改过）；不重写文档里 Move / xochitl 的历史记录（那是另一张单的事）
- 两张单不能碰同一个文件：本单只改上面「涉及文件」列的 6 个文件
- 只在上面的分支工作；不合并；改「涉及文件」以外的文件写进「结果 → 偏离」
- 提交信息中文，`docs:` 前缀 + 正文，不加 Co-Authored-By；不读密钥、`.env`、`auth*`
- 风险点：无（只改文档和注释）

## 分步任务
1. [ ] `docs/development.md` 版本表：逐行和代码核对现值，改 `WRITER_VERSION`；有别的不一致一起改，写进「结果」。
2. [ ] `CONTRIBUTING.md` 四处。
3. [ ] `AGENTS.md` 第 4 条。
4. [ ] `docs/architecture.md` 两处。
5. [ ] `README.md:31`。
6. [ ] `generate.rs` 三个函数的注释；自检：`git diff master..HEAD -- crates/library/src/generate.rs | grep '^[+-]' | grep -v '^[+-][+-]' | grep -v '^[+-]\s*//'` 没有输出。
7. [ ] 全文搜一遍 `README.md`、`AGENTS.md`、`CONTRIBUTING.md`、`docs/architecture.md`、`docs/development.md` 里还有没有"两个模式""只有 kindle、ireader"之类的过时说法，有就列进「结果 → 未完成」（不在本单文件里的不改）。

## 验收标准
- [ ] 上面 6 个文件的每一处都改了，说法和代码、`profiles/*.toml` 一致
- [ ] `AGENTS.md`、`CONTRIBUTING.md` 的分支规则和上面「输入」里那套一致，两处互不矛盾
- [ ] `git diff --stat master..HEAD` 只有这 6 个文件和任务单；`generate.rs` 只有注释变化
- [ ] `cargo test --workspace` 全过；`cargo clippy --workspace --all-targets -- -D warnings` 0 警告；`cargo fmt --all -- --check` 干净；`fp_golden.txt` 不变

## 交付物
提交到上面的分支并推送；「结果」填好；状态改为 done 或 blocked。

## 给执行方的指令
```
执行任务单 .dev/tasks/2026-11-12-docs-align.md：先把状态改为 accepted 并提交，按「分步任务」做完，填好「结果」，状态改为 done（做不下去改为 blocked 并写明原因），提交并推送分支。
```

## 结果
<!-- 执行方填写 -->
- 执行方 / 开始–结束时间：opencode-local / 2026-11-12 10:55–11:00 +08:00
- base..head：`848967c..36f86ec`（`848967c` 是切出时的 master；结果提交在分支 tip）
- 提交：

  | 步骤 | 提交号 | 文件 | 说明 |
  | --- | --- | --- | --- |
  | 接受 | `3f9165c` | 任务单 | 状态改 accepted |
  | 1–7 | `36f86ec` | `docs/development.md`、`CONTRIBUTING.md`、`AGENTS.md`、`docs/architecture.md`、`README.md`、`crates/library/src/generate.rs` | 版本号、五个内置模式、分支规则、产物位置、临时文件清理、挂载点、大小写判断注释 |
  | 结果 | 本提交 | 任务单 | 填结果、状态改为 done |

- 实现：
  - **版本表**：`kfx::write::WRITER_VERSION` 25 → 26；逐项核对 `OPTIMIZE_VERSION` 68、`COMIC_VERSION` 68、`KINDLE_RULES_VERSION` 42、`CONVERT_VERSION` 2、`opfmeta::VERSION` 6、`kepub::VERSION` 2、`PIPELINE_VERSION` 5，和代码一致，没改别的。
  - **CONTRIBUTING**：内置模式改成五个（kindle KFX、ireader EPUB、kobo KEPUB、hanvon EPUB、bigme EPUB，xochitl 删除说明保留）；分支规则改成「任务分支开发、任务单可直接提交 master、负责 bot 验收、agy 二审无阻断后负责 bot `git merge --no-ff`、vinci 最终真机验收」；版本号清单补 `bookconv::kepub::VERSION`；`epub-optimize --device=` 列出五个模式。
  - **AGENTS**：第 4 条改成同一套分支/合并规则，并保留「影响产物的改动写明没跑真书回归、没上真机」。
  - **architecture**：生成产物位置补全 Kindle/掌阅 `documents/`、汉王 `Documents/`、Bigme `books/`、Kobo `books/`；可靠性表里临时文件改成书库和电脑上产物目录加锁时清、设备上的由 `sync` 连上后清（按任务单要求写的是 `lock-no-device` 合并后的行为）。
  - **README**：前提补上汉王、Bigme 的 jmtpfs 挂载点 `…/mtp/hanvon`、`…/mtp/bigme`。
  - **generate.rs**：只补 `case_twin`、`case_alias`、`distinct_on_disk` 的注释；`case_twin` 写明是 `case_alias` 的特例，`case_alias` 写明可跨目录，`distinct_on_disk` 写明只列一个目录、判断不了跨目录别名所以只用同一目录的 twins。
  - **全文检查时顺带修正**（都在本单文件里）：`docs/architecture.md` 第 7 行内置模式从三个补成五个、指纹一节「内置四个模式」改成五个、MTP 传输列表补上汉王和 Bigme。

- 测试：

  | 命令 | 通过 / 失败 / 未跑 | 摘要 |
  | --- | --- | --- |
  | `cargo test --workspace` | 通过 | 所有 crate 全过、0 失败（bookconv 553、csscascade 16、kfx 53、library 74+7+14+51+1、profile 19） |
  | `cargo clippy --workspace --all-targets -- -D warnings` | 通过 | 0 警告 |
  | `cargo fmt --all -- --check` | 通过 | 干净 |
  | `git diff master..HEAD -- crates/library/src/generate.rs \| grep '^[+-]' \| grep -v '^[+-][+-]' \| grep -v '^[+-]\s*//'` | 通过 | 无输出，`generate.rs` 只有注释变化 |
  | `git diff --stat master..HEAD` | 通过 | 只有 6 个文件和任务单；`fp_golden.txt` 未改 |
- 遇到的问题：无。
- 偏离：无（只在「涉及文件」的 6 个文件里改；全文检查发现的三处过时说法也在这几个文件里，一并改了）。
- 未完成：
  - 全文检查剩下的 `docs/development.md:57`「两个模式」是回归对比 Kindle/掌阅的上下文、`:120` 的 `--device=kindle|ireader` 是回归脚本示例，都不是「内置模式总数」，没改；`docs/kfx.md` 等处的「写出器 25」是历史记录，按约束不动。
  - `docs/architecture.md` 的临时文件清理写的是 `2026-11-12-lock-no-device` 合并后的行为；本分支从 `848967c` 切出时那张单还没合并，**合并顺序要把本单排在 `lock-no-device` 之后**（任务单要求负责 bot 合并时核对）。
  - 只改文档和注释，没上真机、没跑真书回归。

## 审查
<!-- 负责 bot 填写 -->
- 验收：
- 二审：
- 合并：
