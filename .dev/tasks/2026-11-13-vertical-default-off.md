# 竖排默认改横排（推翻「中文文字书全部竖排」）

- Task ID: 2026-11-13-vertical-default-off
- 负责 bot: sheng-ren
- 状态: done
- 执行方: box-agent
- 分支: task/2026-11-13-vertical-default-off（从最新 master 切出）
- 更新时间: 2026-11-13T11:08:00+08:00
- 引用: vinci 2026-11-13 10:26 在 sheng-ren 聊天里选方案二；推翻 `docs/decisions.md` 11-11「中文文字书全部竖排」；上次只读调查（`Library::vertical_for`）

## 需求原文
> （vinci 2026-11-13 10:26，sheng-ren 聊天）只对《帷幕》《接头》两本执行了 `vertical --on`，却发现汉王上的所有中文书都是竖排。选方案二：**默认改横排**，只有 `--on`（逐书或目录 `vertical.toml`）或原书本身已经是竖排（原书的 writing-mode / primary-writing-mode 等已是竖排）时才竖排。
>
> 背景（只读调查）：Kobo、汉王 profile 的 `vertical_text = true`，`Library::vertical_for`（`crates/library/src/vertical.rs`）的链是：书的 `layout.vertical` → `vertical.toml` 最长前缀 → 自动判定 `zh_text_of`（`bookconv::vertical::is_zh_text`）。所以中文文字书默认全部竖排。这是 `docs/decisions.md` 2026-11-11 定下的「中文文字书全部竖排」。

## 输入
- 基于 origin/master `53b5322`。
- 现状要点：
  - `Library::vertical_for`（`crates/library/src/vertical.rs`）：设备没开 `vertical_text`、漫画、固定版式一律不竖排；否则 `layout.vertical` → `vertical.toml` → `zh_text_of` / `is_zh_text`。
  - 已有「原书已竖排」检测：`bookconv::vertical::is_natively_vertical`（`crates/bookconv/src/vertical.rs`）。`wash::apply_vertical` 已经用它决定「原书本来竖排的不写 `html{writing-mode:vertical-rl}`、不转书自己的 CSS 盒子」。
    - **已认**：样式表或 `<style>` 里针对 `html` / `body` / `:root` 的 `writing-mode: vertical-rl` 或 `vertical-lr`（含 `-webkit-`、`-epub-` 前缀）；`<html>` / `<body>` 行内 `style` 同样写法。局部选择器（如 `body .signature`、`html table`）不算全书竖排。
    - **还不认**：OPF 的 `<meta name="primary-writing-mode" content="vertical-rl|vertical-lr">`（用户点名要算）；单独的 spine `page-progression-direction="rtl"` 不算（漫画也会 rtl）。
  - `--auto` 现在的帮助是「删掉覆盖，回到自动判定」（`booklib/main.rs`、`docs/usage.md`），自动判定就是 `is_zh_text`。
  - 指纹：最终竖排的书在 info 段后接 `V`，`vertical_ascii = fullwidth` 再接 `f`（`generate.rs`）；不竖排的书不加这两个字母。
  - `OPTIMIZE_VERSION` 现值 68（只写在 `docs/development.md#版本号`，代码常量 `bookconv::optimize::OPTIMIZE_VERSION` 同步）。
  - 测试：`vertical_books_follow_profile_overrides_and_directory_lists`（`crates/library/tests/flow.rs`）等按旧语义写的，要改成新语义。
  - 开了 `vertical_text` 的只有 Kobo、汉王；bigme / kindle / ireader 是 `false`，`vertical_for` 开头就返回 false。

## 「原书已竖排」的定义和这种书的行为（本单要落实）
1. **判定**：以现有 `is_natively_vertical` 为底，**补上 OPF `primary-writing-mode` 为 `vertical-rl` 或 `vertical-lr`**（大小写不敏感；已有 `opf::meta_content`）。CSS `writing-mode` 的规则保持原样。不把单独的 spine rtl 当全书竖排。
2. **谁竖排**：在开了 `vertical_text` 的设备（Kobo、汉王）上，`vertical_for` 的默认分支不再走 `zh_text_of`，改为：原书已竖排 → 竖排，否则横排。`--on`（书或目录）仍强制竖排（固定版式、漫画仍排除）；`--off` 仍强制横排（原书已竖排的也可被关掉）。
3. **竖排时做什么**：与现在强制竖排同一条管线（引号、全角、spine rtl、补 `primary-writing-mode`、横转竖 CSS）。`apply_vertical` 已有的「原书已竖排则不写 `html{writing-mode}`、不转书自己的 CSS」保持不变。
4. **谁不竖排**：bigme / kindle / ireader（`vertical_text = false`）一律不走这条管线，行为与现在一样，包括原书已竖排的书（只修复，不动它们自己的竖排 CSS）。
5. **`--auto`**：删除该书 / 该目录的覆盖（`layout.vertical` 和 `vertical.toml` 相应条目），回到上面的默认——默认是横排，原书已竖排的仍竖排。不再「回到按是不是中文书自动竖排」。

`list` 区分「竖排（强制）/ 竖排（原书）」本单**不做**（可选，未要求）。

## 约束
- 只用 Rust；clean-room；仓库不出现上游项目名及缩写。
- 书的内容：横排默认下，不再因「是中文书」就改引号、转全角；`--on` 或原书已竖排而走竖排管线的，这两项例外仍适用（11-11 竖排管线本身不撤）。
- **推翻** 2026-11-11「中文文字书全部竖排」，写进 `docs/decisions.md`（不要悄悄改回去；11-11 那条保留并标注已推翻）。
- **`OPTIMIZE_VERSION` 加一**（68 → 69）：数值只在 `docs/development.md#版本号` 的表里写现值，代码常量按惯例同步，并在常量上方附一行变更说明。只动文字书这一路，**不加** `COMIC_VERSION`（漫画本来就不竖排）。
- 指纹：升版本后全部文字书会过期重建；先前因自动判定而竖排的书会丢掉 info 段的 `V`/`f`，产物会变成横排（引号、全角也不再改）。`--on` 的（《帷幕》《接头》）和原书已竖排的仍带 `V`/`f`、仍竖排。bigme / kindle / ireader 指纹只因版本号过期，重建后产物应逐字节不变（优化标记不写版本号）。
- `fp_golden.txt`：因版本号变化而变的，按 `docs/development.md` 用 `UPDATE_FP_GOLDEN=1` 重写并在结果里说明。
- 不碰 `~/.local/share/booklib`；实验用 `target/` 下临时 `--library`；测试不碰真机。测试用真书只读。
- `install.sh` 不用 `--force`。本仓库不用 rustfmt 默认宽度，别对整个仓库跑 `cargo fmt`。
- 提交信息中文 + 前缀 + 正文，不加 Co-Authored-By。影响产物：提交说明写清没跑真书回归 / 没上真机（若本机跑了回归，在结果里据实写，不要把没跑的写成跑了）。
- 风险点：**设备行为**（Kobo、汉王上默认由竖改横）→ **必须 agy 二审**。

## 规则（要实现的行为）
1. 默认横排。只有 `--on`（逐书或目录）或原书已竖排（见上面的定义）时，在 Kobo、汉王上才竖排。
2. 自动判定（`is_zh_text`）不再让书竖排。`is_zh_text` 可以保留给别的用途，但不得再作为 `vertical_for` 的默认分支。
3. `--auto` 语义改为「删除覆盖、回到默认」（默认横排；原书已竖排的仍竖排）。帮助文字和文档一起改。
4. Kobo、汉王一起变；bigme、kindle、ireader 不受影响（`vertical_text` 仍为 false，不要改这三份 profile）。
5. 同步改 `docs/decisions.md`、`docs/usage.md`、`docs/devices.md`、`docs/architecture.md`、`README.md`，以及会过时的 `docs/typesetting.md`。

## 分步任务
1. [x] 落实「原书已竖排」判定：扩展 `is_natively_vertical`（补 OPF `primary-writing-mode`），书库侧在 `vertical_for` 默认分支使用它，替换 `zh_text_of`。补单元测试。
2. [x] `--auto` 语义、帮助文字、`vertical_books_follow_profile_overrides_and_directory_lists` 等测试改成新语义。
3. [x] `OPTIMIZE_VERSION` 68→69（表 + 代码常量 + 变更说明）；按需要更新 `fp_golden.txt`；确认指纹：自动竖排的书掉 `V`/`f`，`--on` / 原书已竖排的还在。
4. [x] 写 `docs/decisions.md`（推翻 11-11 那条并写新决定），改 usage / devices / architecture / README，以及过时的 typesetting。
5. [x] `cargo test --workspace` 全过；`cargo clippy --workspace --all-targets -- -D warnings` 0 警告。
6. [ ] 按 `docs/development.md` 跑真书回归（见验收标准）。box 上若没有测试书，不要声称跑过，在结果里写清需在 vinci 本机跑的 fish 命令。**box 上没有测试书，未跑；命令见结果。按本单要求，不合并。**

## 验收标准
- [x] 默认：中文文字书在 Kobo、汉王上不再因 `is_zh_text` 而竖排；`--on` 或原书已竖排的仍竖排；`--off` 强制横排。有测试覆盖（含目录清单、`--auto` 回到新默认）。（固定版式缺口见审查，尚未修。）
- [x] `is_natively_vertical` 认 writing-mode（原有）和 OPF `primary-writing-mode`（新增）；单独 spine rtl 不算。
- [x] `--auto` 帮助和文档与新语义一致。
- [x] Kobo、汉王一起变；bigme、kindle、ireader 的 profile 和竖排行为无变化。
- [x] `OPTIMIZE_VERSION` = 69（表与常量一致）；`COMIC_VERSION` 仍 68。先前自动竖排的书指纹掉 `V`/`f` 并会重建。`fp_golden.txt` 若因版本号更新，在结果里说明。
- [x] `docs/decisions.md` 写明推翻 11-11；usage / devices / architecture / README（及过时的 typesetting）已同步。
- [x] `cargo test --workspace` 全过；clippy `-D warnings` 0 警告。（负责 bot 在独立 worktree 重跑。）
- [ ] 真书回归：有测试书则在 `target/` 下临时目录跑并记录；没有则写明「真书回归需在 vinci 本机跑」和具体 fish 命令，**不要合并**（由负责 bot 执行这条）。结果里写明没上真机。**未跑，不合并。命令见结果。另有二审阻断（固定版式），见审查。**

## 交付物
代码提交到任务分支并推送；「结果」填好；状态改为 done 或 blocked。任务单状态改动推 master，带 `[skip-notify]`。不要合并，不要跑二审（负责 bot 验收和二审）。box 上没有测试书时负责 bot **不要合并**。

## 给执行方的指令
```
执行任务单 .dev/tasks/2026-11-13-vertical-default-off.md：先把状态改为 accepted 并提交，按「分步任务」做完，填好「结果」，状态改为 done（做不下去改为 blocked 并写明原因），提交并推送分支。任务单 done 推 master 带 [skip-notify]。不要合并，不要跑二审。真书回归若 box 上没有 ~/Documents/ereader/books/（或等价副本），不要声称跑过，在结果里写出给 vinci 的 fish 命令。
```

## 结果
- 执行方 / 开始–结束时间：box-agent（oc-exec） / 2026-11-13T10:46:48+08:00–2026-11-13T10:55:54+08:00
- base..head（验收时，含 rustfmt）：`c408e2e77784752a7a15ffc4351d0ef787d11dd5..caebf7cbc260e0939c363ec88feee331530e89e8`
- 提交：
  | 提交号 | 说明 |
  |---|---|
  | 5e44fce | chore: 开始执行任务单 2026-11-13-vertical-default-off [skip-notify] |
  | ecdb5d4 | feat: 竖排默认改横排，Kobo、汉王只有 --on 或原书已竖排才竖排 |
  | 9c643eb | chore: 填写任务单 2026-11-13-vertical-default-off 结果（done） [skip-notify] |
  | caebf7c | chore(bookconv): 按 rustfmt 调整原书竖排判定新增代码的换行（负责 bot 验收时：仅新代码换行，不是整库 fmt） |
- 改动文件（实现至 rustfmt，相对 origin/master）：`.dev/tasks/2026-11-13-vertical-default-off.md`、`README.md`、`crates/bookconv/src/optimize/mod.rs`、`crates/bookconv/src/vertical.rs`、`crates/library/src/bin/booklib/main.rs`、`crates/library/src/generate.rs`、`crates/library/src/lib.rs`、`crates/library/src/session.rs`、`crates/library/src/vertical.rs`、`crates/library/tests/common/mod.rs`、`crates/library/tests/flow.rs`、`crates/library/tests/fp_golden.txt`、`docs/architecture.md`、`docs/decisions.md`、`docs/development.md`、`docs/devices.md`、`docs/typesetting.md`、`docs/usage.md`
- 测试（负责 bot，独立 worktree `/tmp/sheng-ren-review-vertical-default-off`，`CARGO_TARGET_DIR=/home/box/repos/sheng-ren/target`，`rustup run stable`，rustc 1.99.1）：
  - `cargo test --workspace` 全过：bookbase 30、bookconv 553、csscascade 16、kfx 53、library 79 / bin 7 / cli 14 / flow 60 / fp_golden 1、profile 19。
  - `cargo clippy --workspace --all-targets -- -D warnings`：0 警告。
  - `cargo fmt --all -- --check`：仅 `crates/bookconv/src/vertical.rs` 新增换行不过；已只 fmt 该文件并提交 `caebf7c`，之后 `--check` 干净。
- 版本号：`OPTIMIZE_VERSION` 68→69（`crates/bookconv/src/optimize/mod.rs:175` 常量与上方 v69 变更说明，与 `docs/development.md` 版本号表第 100 行一致）；`COMIC_VERSION` 仍 68。profile toml 未改。
- `fp_golden.txt`：`UPDATE_FP_GOLDEN=1` 重写，仍 660 行。文字书段 68→69，漫画段仍 c68。旧文件 131 行带 `Vf`，新文件 **0 行**带 `Vf`：黄金对拍用的是横排 `sample_epub`、没有 `--on`、也不是原书竖排；翻 `vertical_text` 的 mutation 也不再出 `V`/`f`（只开能力、默认仍横排，符合新语义）。执行方写「Kobo/掌阅/Kindle 行掉 V/f」不精确：掌阅 / Kindle / Bigme 默认本来就没有 `V`/`f`，掉的是 Kobo / 汉王全部文字书行，以及把 `vertical_text` 翻成 true 的 mutation 行。`--on` / 原书已竖排仍带 `Vf` 由 `flow.rs` 的 `vertical_books_follow_profile_overrides_and_directory_lists` 断言，黄金文件本身不再覆盖这条路径。
- 遇到的问题：无（执行方）。负责 bot 验收另见审查：二审阻断未修。
- 偏离（包装器报 9 个文件；任务单没逐个列文件，经对照「输入 / 规则 / 分步」**全部在范围内，不是真超范围**）：
  | 文件 | 在范围内？ | 说明 |
  |---|---|---|
  | `README.md` | 是 | 规则第 5 条要求同步 |
  | `crates/bookconv/src/optimize/mod.rs` | 是 | 分步 3：`OPTIMIZE_VERSION` 68→69 及变更说明 |
  | `crates/library/src/bin/booklib/main.rs` | 是 | 分步 2：`--auto` 帮助与命令输出 |
  | `crates/library/src/lib.rs` | 是 | 注释同步（`zh_text` / layout 不再决定竖排），无逻辑改动 |
  | `crates/library/src/session.rs` | 是 | 注释同步（`ContentFacts.zh_text` 不再用于竖排），无逻辑改动 |
  | `crates/library/tests/common/mod.rs` | 是 | 新语义测试夹具 `sample_epub_vertical` |
  | `docs/architecture.md` | 是 | 规则第 5 条要求 |
  | `docs/devices.md` | 是 | 规则第 5 条要求 |
  | `docs/typesetting.md` | 是 | 规则第 5 条「以及会过时的 typesetting」 |
- 未完成：真书回归未跑；默认分支漏掉固定版式排除（见审查），未修。
- 运行记录：`/home/box/oc-runs/2026-11-13-vertical-default-off-20261113-104648.json`
- 日志：`/home/box/oc-runs/2026-11-13-vertical-default-off-20261113-104648.log`
- 模型：`opencode-go/deepseek-v4.1-flash`
- **没跑真书回归、没上真机。** box 上没有 `~/Documents/ereader/books/`（搜过 `/home/box`、`/workspace`，无测试书；`REGRESS_BOOKS` 未设）。**真书回归需在 vinci 本机跑。** 按本单要求 **不合并**。

执行方日志里的 fish 有两处错误，不要照抄：路径写成了 `~/repos/sheng-ren`（vinci 本机是 `~/Projects/sheng-ren`）；以及把 `tools/regress/run.sh` 在汉王 / Kobo 上的默认路径说成「应变更横排」——`epub-optimize` **只有** `--vertical` 才走竖排管线，自动竖排只在 `booklib` 的 `Library::vertical_for`。标准 `run.sh` 不加 `--vertical`，改动前后五个模式都应当是 `SAME`。把 master 默认（不加 `--vertical`）和新版本加 `--vertical` 去比《帷幕》《接头》也不对：旧的 `epub-optimize` 同样不会因中文自动竖排，两边本来就不该一样。下面是更正后的命令（fish；仓库 `~/Projects/sheng-ren`；书只读；产物写到 `target/`；不碰 `~/.local/share/booklib`、不往真机传）。书的实际文件名请先 `ls ~/Documents/ereader/books | rg '帷幕|接头'` 确认，不一定是 `帷幕.epub`。

```fish
cd ~/Projects/sheng-ren
git fetch origin
git switch task/2026-11-13-vertical-default-off
git pull
# 旧版从当时 master 出（竖排改动之前）；worktree 不要用 HEAD（那是新分支）
git worktree add target/regress-base origin/master
(cd target/regress-base; and cargo build --release -p bookconv --bin epub-optimize; and cargo build --release -p library --bin booklib)
cargo build --release -p bookconv --bin epub-optimize
cargo build --release -p library --bin booklib

# 1）标准真书回归（docs/development.md）：epub-optimize 默认路径。
#    五个模式都应 SAME（优化标记自 v54 不写版本号）。不要预期汉王 / Kobo「变横排」。
for d in ireader bigme kindle hanvon kobo
    tools/regress/run.sh target/regress-base/target/release/epub-optimize target/regress/旧-$d --device=$d
    tools/regress/run.sh target/release/epub-optimize target/regress/新-$d --device=$d
    tools/regress/compare.py target/regress/旧-$d target/regress/新-$d
    tools/regress/tocchk.py target/regress/旧-$d target/regress/新-$d
end

# 2）可选：竖排管线本身有没有改——两边都加 --vertical，也应 SAME。
for d in hanvon kobo
    tools/regress/run.sh target/regress-base/target/release/epub-optimize target/regress/旧-$d-v --device=$d --vertical
    tools/regress/run.sh target/release/epub-optimize target/regress/新-$d-v --device=$d --vertical
    tools/regress/compare.py target/regress/旧-$d-v target/regress/新-$d-v
    tools/regress/tocchk.py target/regress/旧-$d-v target/regress/新-$d-v
end

# 3）默认横排是 booklib 的改动。临时书库 + 假设备（关挂载检查且不设 BOOKLIB_USBMS_DIR 时，
#    USB 存储落到 $BOOKLIB_MTP_DIR/usbms，不会写到真插着的 Kobo）。不要用默认书库。
set lib $PWD/target/regress-lib
set dev $PWD/target/regress-lib-dev
rm -rf $lib $dev $lib-old
mkdir -p $dev/hanvon/"Internal Storage" $dev/usbms/KOBOeReader
set -x BOOKLIB_NO_NET 1
set -x BOOKLIB_MTP_DIR $dev
set -x BOOKLIB_MTP_NO_MOUNT_CHECK 1
# 挑一本普通中文文字书（不是帷幕/接头、原书也不是竖排），以及帷幕/接头的实际路径
set books ~/Documents/ereader/books
# 先用新 booklib：普通书在汉王 / Kobo 上 list 应显示「横排」；vertical --on 之后应「竖排」
target/release/booklib --library=$lib track $books
target/release/booklib --library=$lib sync --no-meta --no-build
target/release/booklib --library=$lib list
# 再开一个旧书库对照：同一本普通中文书在旧 booklib 的汉王 / Kobo 上应是「竖排」
target/regress-base/target/release/booklib --library=$lib-old track $books
target/regress-base/target/release/booklib --library=$lib-old sync --no-meta --no-build
target/regress-base/target/release/booklib --library=$lib-old list
# 若要看产物：先确认只打到 $dev（booklib devices），再
#   target/release/booklib --library=$lib sync --device=hanvon --no-meta --force 某本书
# 不要对 ~/.local/share/booklib 跑，也不要在没改环境变量时对真机 sync。
```

合并之后、本机安装新 booklib，已有书库里《帷幕》《接头》的 `--on` 覆盖仍在，应继续竖排；其它先前因「中文文字书」自动竖排的，会掉 `V`/`f`、重建为横排。

## 审查
- 验收：**不通过**（有阻断，sheng-ren bot，2026-11-13 11:08 +08:00，审 `origin/master...caebf7c`）。**没跑真书回归、没上真机；按本单要求不合并。** 下面是 box 上独立 worktree 核对。工具链 rustup stable rustc 1.99.1。按 `Projects/_templates/review-checklist.md`「项目 bot 验收」：结果已填、状态 done；分步 1–5 完成，分步 6（真书回归）因 box 无测试书未跑；提交中文前缀加正文、无 Co-Authored-By、执行方未合并。实现主体符合方案二和任务单（默认横排、`--on` / 原书已竖排才竖排、`--auto` 回到新默认、PWM 扩展、spine rtl 不算、`is_zh_text` 只留入库记录、Kobo 与汉王一起变、三份 profile 未改、11-11 保留并标注已推翻、文档同步、版本号 69/68 一致）。测试、clippy、fmt 见结果。主体逻辑抽查：`vertical_for` 默认分支改为 `is_natively_vertical(...).unwrap_or(false)`（读不到原件按横排）；`is_natively_vertical` 在 CSS 循环之后用 `opf::meta_content` 认 `primary-writing-mode`（`vertical-rl` / `vertical-lr`，trim + 大小写不敏感）；flow 测试覆盖默认横排、原书竖排夹具、`--on` / `--off` / `--auto`、目录清单、Bigme/Kindle 不受影响，并断言 `Vf`。
    - 包装器「偏离 9 个文件」：见结果表，**9 个都在范围内**。
    - `OPTIMIZE_VERSION` 已加一且与 `docs/development.md` 一致（69）；`COMIC_VERSION` 68。
    - **阻断（负责 bot 独立核对后同意二审）**：`Library::vertical_for` 默认分支漏了 `is_fixed_layout`。旧默认走 `zh_text_of` → `is_zh_text`，函数开头 `if is_fixed_layout(entries) { return false }`，所以固定版式书不会因是中文而自动竖排。`--on` 和目录覆盖分支仍有 `!self.is_fixed_layout(meta)?`。新默认只问 `is_natively_vertical`，而它只看 CSS / OPF 的 writing-mode，不排除固定版式。非漫画、自带竖排样式或 `primary-writing-mode` 的固定版式 EPUB，会走进竖排管线（挂 `eink-vertical.css`、改引号、转全角），违背任务单「漫画、固定版式一律不竖排」、函数注释和 `docs/typesetting.md`。应在默认分支同样排除（例如 `Ok(self.is_natively_vertical(meta).unwrap_or(false) && !self.is_fixed_layout(meta)?)`，或在函数开头对固定版式短路），并补测试。此缺陷修完并再验收、再二审之前 **不合并**（即便本机回归通过也不应合入未修版本）。
- 二审：**有阻断**（agy-review，2026-11-13 11:06 +08:00，模型 gemini-3.8-flash-medium，用时 427s，回执对照 `caebf7c`，文件 `.dev/tasks/2026-11-13-vertical-default-off.agy.md`）。结论「建议返工」。负责 bot 已读回执并核对引用：阻断（默认分支漏固定版式，`crates/library/src/vertical.rs` 约 168–170 行）属实，见上；建议 1（给 `is_natively_vertical` 加进程内 SHA 缓存）不挡、本单未做；建议 2（结果缺 fish 命令、「没上真机」、提交表滞后）已在本栏和结果栏补正；线索（原件临时读不到时 `unwrap_or(false)` 导致指纹抖动）是容错，不挡，负责 bot 知悉。未核实项：agy 只读未跑测试（负责 bot 已重跑）；真书回归和真机仍未做。
- 合并：**未合并。** 两条独立原因都足够：① box 上没跑真书回归；② 二审阻断（固定版式）未修。状态保持 **done**（不是 verified），代码留在 `task/2026-11-13-vertical-default-off`，远程分支保留。修完固定版式并在 vinci 本机跑过回归之后，再重新验收、重新二审，才能考虑合并。
