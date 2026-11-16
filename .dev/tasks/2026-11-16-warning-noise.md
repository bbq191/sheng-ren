# sync 的其余按书刷屏提醒：体检、质量门、KFX 警告按轮汇总

- Task ID: 2026-11-16-warning-noise
- 负责 bot: sheng-ren
- 状态: done
- 执行方: 待定
- 分支: task/2026-11-16-warning-noise（从 master `b834541` 之后的最新 master 切出）
- 更新时间: 2026-11-16T11:40:03+08:00
- 引用: 接 `2026-11-16-kepub-css-warning`（已合并 `f0f84c1`）。那张单验收时列出三类同样按书刷屏的提醒，vinci 2026-11-16：「要，最后一起收拾了」。

## 需求原文
> （vinci 2026-11-16，sheng-ren 聊天）同意把上次列的另外三类刷屏线索一起收拾：
> 1. `⚠ 体检：…`：掌阅文字书每本一两行；
> 2. `⚠ 质量门未过…`：每本书每条错误一行，KEPUB 还会再报一份；
> 3. KFX 写出器的警告（如「目录项找不到位置：…」）。
> 目标：sync 输出不再按书刷屏，同类合并成本轮结束时的一行「N 本书 …」，明细用 `booklib list` 等已有地方查，参照 kepub-css-warning 的做法。真正影响结果、要用户处理的不能藏掉，汇总里要看得出是哪几本；确属冗余的（KEPUB 重复报同一份质量门）可以去重，但不能放宽判定本身。

## 输入（开单时 master `b834541` 的现状，执行时以代码为准）

### 三类提醒的来源、触发、去向（全在 `crates/library/src/generate/produce.rs` 的 `Library::produce`）
1. **体检**：`bookconv::audit::summary(&findings, &device.id)`（`crates/bookconv/src/audit.rs:351`）每本最多两行：`体检：{reader} 上和 Kindle 不一样、没处理：…（booklib list 看全部）`、`体检：{reader} 上没验证过的写法 N 种（最多的：…）`。触发：优化器清洗层的怪癖表体检有发现（实际只有开了 `kindle_rules` 的模式＝掌阅的文字书）。KFX 路径提前 `return`，不打这两行。**明细已经记进生成记录**（`Entry::audit`，`list` 显示「体检 ✗ …」「体检 ? 没验证过 …」）。
2. **质量门**：`bookconv::check::check_epub_file`（`crates/bookconv/src/check.rs`）的 `errors`（硬失败：真 DRM、目录 href 命中率 < 80%、双 id、资源引用命中率 < 80%、OPF 不合法、mimetype 不对）。`produce` 先查优化产物，每条错误一行 `质量门未过：{e}`；KEPUB 产物再查一遍，每条一行 `质量门未过（KEPUB）：{e}`——kepub 只插 koboSpan 和包装，优化产物有的错 KEPUB 几乎一定原样再报一遍。**只警告、不拦下**（`check.rs` 模块注释：「书库目前只警告、不拦下」）：产物照常生成、照常传；不回退、不跳过。**不进生成记录**。`check` 的 `warnings`（锚点丢失等）本来就不打，不在本单范围。
3. **KFX**：`kfx::write::epub_to_kfx_from` 返回的 `warnings`：`目录项找不到位置：{label}`（`write/entities.rs:221`）、`链接目标找不到：{file}#{id}`（`:263`）、`图片格式 … 不支持：{path}` / `图片找不到或格式不支持：{path}`（`write/resources.rs:68`）、`spine 写了从右往左…`（`write/mod.rs:296`）、`epubbook::load_from` 里的（执行时 `rg` 补全）。只有 `kindle` 模式。**不进生成记录**。其中图片取不到＝KFX 里少了这张图，目录项、链接找不到＝跳转退到文件头或书头，属于**影响结果**的。
4. 去向：`warnings` → `Made` → `Transfer` → `Built::Written|Same { warnings }` → `booklib` 的 `built_line` 逐条打成结果行下面的 `  ⚠ …`（`crates/library/src/bin/booklib/main.rs` 约 `:719`）。sync 一轮结束有 `counts_summary`，上一张单加的 `RiskyCssBooks` 在其后打 KEPUB CSS 的汇总行（约 `:1958`）。
5. **指纹**：`warnings`、`Entry::audit` 都不进指纹（`crates/library/src/fingerprint.rs` 不读；上一张单已核过）。所以本单不升版本号。

## 设计（开单时定的，执行方照做；有更小的等价做法可以，但要在结果里写明理由）

### A. 明细进生成记录（`produce.rs`）
`audit` 清单（已有字段，`serde(default)`，旧记录照读）里追加，`list` 已经会把 `⚠ ` 开头的行显示成「提醒 ⚠ …」（上一张单的 `audit_lines`）：
- 质量门：每条错误一行 `⚠ 质量门未过：{e}`；KEPUB 的那份**去重**：和优化产物那份**文字完全相同**的错误不再记、不再报，只有 KEPUB 新出现的才记 `⚠ 质量门未过（KEPUB）：{e}`。两次检查本身照做，判定不改。
- KFX：每条一行 `⚠ KFX：{w}`，同样的去重；超过 20 条只记前 20 条，再加一行 `⚠ KFX：另有 N 条（同类，未逐条记）`（链接目标找不到的书可能上百条，生成记录不能被撑大）。
- 体检：已经在记录里，不动。

### B. sync 输出按轮汇总（`main.rs`）
1. 把上一张单的 `RiskyCssBooks` 推广成按类别的汇总（比如 `WarningSummary`），一轮里按（设备, 产物路径）去重计书本数，同时记书名；只算 `Built::Written` + `Built::Same`（这一轮真生成了的）。
2. `produce` 往 `warnings` 放的仍是给 sync 看的标记，`built_line` 认出这四类（KEPUB CSS、体检、质量门、KFX）就**不逐本打**；认法用固定前缀或 `bookconv`/`kfx`/`library` 里的公开常量，不要靠模糊匹配。别的 warning 照旧逐本打。
3. 一轮结束（`counts_summary` 之后，`--watch` 每轮同样；某类没有就不打那一行）：
   - 体检（只是提示）：`⚠ 体检：N 本书有和 Kindle 不一样没处理、或没验证过的写法（只是提示，书照常生成；明细用 booklib list 看）`——不列书名。
   - 质量门（结构性问题，要让人看得出是哪几本）：`⚠ 质量门未过：N 本书（《甲》、《乙》… 等）——产物照常生成、照常传；每本的问题用 booklib list 看`，书名最多列 5 本，多的写「等 N 本」。
   - KFX（会影响 Kindle 上的结果：少图、跳转不准）：`⚠ KFX：N 本书转换时有问题（《甲》、《乙》… 等；如图片取不到、目录项或链接找不到位置）——每本的明细用 booklib list 看`，书名同样最多 5 本。
   - KEPUB CSS：沿用上一张单的那一行，不改措辞。
   - 书名同名时带上设备（`[kindle]`）也行，执行方定，结果里说明。
4. 退出码不变（这几类本来就是 `Ok` 行、不计失败）。真正的失败（`BuildEvent::Failed`、传输错误等）不碰，照旧逐本打。

### C. 不影响产物
只改提醒的记录和显示：EPUB / KEPUB / KFX 产物逐字节不变；生成记录多几行 `audit`（不进指纹）。**不升任何版本号**，`fp_golden.txt`、指纹构成不动。

## 约束
- 只用 Rust；clean-room；仓库不出现上游项目名及缩写。
- 书的内容一个字不多一个字不少；**产物逐字节不变**。
- 不升版本号；不改指纹构成；`fp_golden.txt` 不动。
- **不放宽任何判定**：质量门、体检、KFX 写出器里产生警告的条件一条都不改，只改记录和显示（KEPUB 质量门去重只去和优化产物文字完全相同的那几条）。
- 不碰 `~/.local/share/booklib`、`~/Documents/ereader/books`、真机；实验用 `target/` 下临时 `--library`；测试不碰真设备、不联网。
- `install.sh` 不用 `--force`。别对整个仓库跑 `cargo fmt`。
- 提交信息中文 + 前缀 + 正文写原因，不加 Co-Authored-By。
- 单本命令行工具（`epub-optimize`、`epub-to-kfx`、`epub-to-kepub` 等）的逐本输出不在范围，不改。

## 分步任务
1. [ ] 核对「输入」里三类的来源、触发和行号；`rg` 补全 KFX 写出器和 `epubbook::load_from` 的全部 warning 写法，写进结果。
2. [ ] `produce.rs`：按 A 记进 `audit`、KEPUB 质量门去重、KFX 去重和 20 条上限；给 sync 的标记。
3. [ ] `main.rs`：按 B 推广汇总、`built_line` 过滤、一轮结束的各行。
4. [ ] 测试（见验收标准）；`docs/usage.md`（sync 输出说明约 `:156-158`、`list` 说明约 `:193-196`）、`docs/kepub.md` 若提到质量门报两遍也同步。
5. [ ] `cargo test --workspace` 全过；`cargo clippy --workspace --all-targets -- -D warnings` 0 警告；确认版本号和 `fp_golden.txt` 都没动。

## 验收标准
- [ ] 单测：汇总的纯函数——一轮里体检 3 本、质量门 2 本、KFX 1 本、外加一条不认识的 warning：逐本结果行下面只剩那条不认识的；每类各一行、N 对；质量门、KFX 两行带书名，超过 5 本时写「等 N 本」；同一本（同设备同路径）出现两次只算一次。
- [ ] 单测：KEPUB 质量门与优化产物相同的错误只记、只报一次；KEPUB 新出现的照记。KFX 超过 20 条时记录截断成 20 + 1 行。
- [ ] 集成测试（`crates/library/tests/cli.rs`，临时书库，不碰真设备）：至少一类（如质量门：造一本目录 href 全部指向不存在文件的书，`--device=kobo` 或 `--device=ireader`）sync 只出一行汇总、带书名，`list` 在那本下面显示「提醒 ⚠ 质量门未过：…」，KEPUB 不重复。
- [ ] 产物逐字节不变（负责 bot 验收时用改动前 master 二进制对照，至少 ireader、kobo、kindle 各一组）。
- [ ] 版本号不变；`fp_golden.txt` 一行没动；指纹构成没动。
- [ ] `docs/usage.md` 现行段落已同步。
- [ ] `cargo test --workspace` 全过；clippy `-D warnings` 0 警告。
- [ ] 结果里写明没跑真书回归、没上真机。

## 涉及文件
- `crates/library/src/generate/produce.rs`
- `crates/library/src/bin/booklib/main.rs`
- `crates/library/tests/cli.rs`、`crates/library/tests/common/mod.rs`（如需要）
- `docs/usage.md`（及 `docs/kepub.md`、`docs/kfx.md` 若有现行描述）

## 交付物
代码提交到任务分支并推送；「结果」填好；状态改为 done 或 blocked。任务单状态改动推 master，带 `[skip-notify]`。不要合并，不要跑二审。

## 给执行方的指令
```
执行任务单 .dev/tasks/2026-11-16-warning-noise.md。先把状态改为 accepted 并提交，按「设计」「分步任务」做完，填好「结果」，状态改为 done（做不下去改为 blocked 并写明原因），提交并推送分支。任务单 done 推 master 带 [skip-notify]。不要合并，不要跑二审。不升版本号，不改指纹，不放宽任何判定，产物要逐字节不变。
```

## 结果
- 执行方 / 开始–结束时间：box-agent（oc-exec） / 2026-11-16T11:27:16+08:00–2026-11-16T11:40:03+08:00
- base..head：`7fbc1c2070a8f6a3b3dd807589275f57f2e4e8e3..02ed19f1b8965c9e41d188ed25b4aca9a4e6efb3`
- 提交：
  | 提交号 | 说明 |
  |---|---|
  | 74c9b34 | chore: 开始执行任务单 2026-11-16-warning-noise [skip-notify] |
  | 02ed19f | feat(library): sync 的体检、质量门、KFX 提醒按轮汇总 |
- 改动文件：`.dev/tasks/2026-11-16-warning-noise.md`,`crates/library/src/bin/booklib/main.rs` `crates/library/src/generate.rs`,`crates/library/src/generate/produce.rs` `crates/library/src/lib.rs`,`crates/library/tests/cli.rs` `crates/library/tests/common/mod.rs`,`crates/library/tests/flow.rs` `docs/architecture.md`,`docs/kepub.md` `docs/usage.md`
- 测试：包装器未代跑验收命令，由负责 bot 按任务单核对
- 遇到的问题：无
- 偏离：
  - `crates/library/src/generate.rs`
  - `crates/library/src/lib.rs`
  - `crates/library/tests/flow.rs`
  - `docs/architecture.md`
- 新建文件（线索）：无
- 未完成：无
- 运行记录：`/home/box/oc-runs/2026-11-16-warning-noise-20261116-112716.json`
- 日志：`/home/box/oc-runs/2026-11-16-warning-noise-20261116-112716.log`
- 模型：`opencode-go/deepseek-v4.1-flash`
- 用时：实际运行 12.8 分钟（墙钟 767s / 实际运行 767s；超时按实际运行计）

## 审查
<!-- 负责 bot 填写 -->
- 验收：通过（负责 bot 独立验收，2026-11-16）。逐条核对：
  - 三类来源与「输入」一致：体检＝`audit::summary`（只 ireader 这类开 `kindle_rules` 的文字书有，KFX 路径本来不打）；质量门＝`check_epub_file` 的 `errors`，只警告不拦、照常生成照常传，原来不进生成记录；KFX＝`epub_to_kfx_from` 的 warnings（图片取不到、目录项/链接找不到位置、spine 从右往左等），原来不进生成记录。三类都不进指纹。
  - 判定没放宽：质量门两次检查照做，KEPUB 只去掉和优化产物文字完全相同的错误；KFX 只做完全相同去重和 20 条记录上限（sync 照样计这本书）；体检判定不动。
  - 偏离 4 个文件逐个核对，都合理、不涉及产物和判定：`crates/library/src/generate.rs`、`crates/library/src/lib.rs` 只是把三个标记常量 `AUDIT_WARNING`/`GATE_WARNING`/`KFX_WARNING` 导出给 `booklib` 认；`crates/library/tests/flow.rs` 的 `audit_findings_are_kept_in_the_generation_record` 原来断言逐本的体检文字，改成断言标记，生成记录里体检清单的断言保留，意图没变；`docs/architecture.md` 的 `audit` 一行同步成「按轮汇总、清单进生成记录」。
  - 实跑（`target/noisetest` 临时书库，设备目录指到临时目录，`--device=ireader,kobo,kindle`，5 本小书：普通、目录 href 指向不存在的文件、引用不存在的图、列表项行内 px 左边距、死锚点）：改动前逐本打 ⚠ 共 15 行（其中 kobo 的质量门 KEPUB 原样重报 3 行）；改动后逐本行下面都没有，末尾三行汇总：体检 1 本、质量门 2 本（带书名和设备）、KFX 1 本（带书名）。`list` 在对应书下逐条显示「提醒 ⚠ 质量门未过：…」「提醒 ⚠ KFX：…」，KEPUB 不重复。
  - 产物：改动前 master（`7fbc1c2`）release 二进制与改动后对同样 5 本、三台共 15 份产物（ireader EPUB 5、kobo KEPUB 5、kindle KFX 5）逐字节相同。
  - 版本号、指纹构成、`fp_golden.txt` 都没动；没有新出现上游项目名。
  - 发现并修正（fix 提交）：汇总按（设备, 路径）计数，一本书三台都没过质量门算成 3 本、前 5 个名额被同一本占掉、另一本看不到。改为按书名归并、列出命中设备，本数按书计；补单测 `warning_summary_groups_devices_of_the_same_book`，docs/usage.md 同步。已知局限：`BuildEvent::Built` 不带书 id，两本同名的书在汇总行里会合成一项（明细以 `list` 为准）。
  - `cargo test --workspace` 851 通过 0 失败；clippy `-D warnings` 0 警告。
  - 未跑真书回归、未上真机（只改提醒的记录和显示，产物不变）。
