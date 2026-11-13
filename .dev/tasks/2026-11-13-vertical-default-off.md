# 竖排默认改横排（推翻「中文文字书全部竖排」）

- Task ID: 2026-11-13-vertical-default-off
- 负责 bot: sheng-ren
- 状态: verified
- 执行方: box-agent
- 分支: task/2026-11-13-vertical-default-off（从最新 master 切出）
- 更新时间: 2026-11-13T11:43:00+08:00
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
6. [x] 按 `docs/development.md` 跑真书回归（见验收标准）。vinci 2026-11-13 本机已跑：ireader、bigme、kindle、hanvon、kobo 五个模式，以及 hanvon、kobo 的 `--vertical`，全部 SAME，坏链接 0，不合法 XML 0。临时书库那一步没能验证横竖排方向（list 不显示方向，`--no-build` 不生成产物）。没上真机。

## 验收标准
- [x] 默认：中文文字书在 Kobo、汉王上不再因 `is_zh_text` 而竖排；`--on` 或原书已竖排的仍竖排；`--off` 强制横排。有测试覆盖（含目录清单、`--auto` 回到新默认）。固定版式（含原书已竖排的）默认和 `--on` 都不竖排，见 `fixed_layout_native_vertical_is_never_set_vertical`。
- [x] `is_natively_vertical` 认 writing-mode（原有）和 OPF `primary-writing-mode`（新增）；单独 spine rtl 不算。
- [x] `--auto` 帮助和文档与新语义一致。
- [x] Kobo、汉王一起变；bigme、kindle、ireader 的 profile 和竖排行为无变化。
- [x] `OPTIMIZE_VERSION` = 69（表与常量一致）；`COMIC_VERSION` 仍 68。先前自动竖排的书指纹掉 `V`/`f` 并会重建。`fp_golden.txt` 若因版本号更新，在结果里说明。
- [x] `docs/decisions.md` 写明推翻 11-11；usage / devices / architecture / README（及过时的 typesetting）已同步。
- [x] `cargo test --workspace` 全过；clippy `-D warnings` 0 警告。（负责 bot 在独立 worktree 重跑。）
- [x] 真书回归：vinci 2026-11-13 本机已跑（详见结果）。没上真机。待 vinci 真机验收后改 verified。

## 交付物
代码提交到任务分支并推送；「结果」填好；状态改为 done 或 blocked。任务单状态改动推 master，带 `[skip-notify]`。不要合并，不要跑二审（负责 bot 验收和二审）。box 上没有测试书时负责 bot **不要合并**。

## 给执行方的指令
```
执行任务单 .dev/tasks/2026-11-13-vertical-default-off.md：先把状态改为 accepted 并提交，按「分步任务」做完，填好「结果」，状态改为 done（做不下去改为 blocked 并写明原因），提交并推送分支。任务单 done 推 master 带 [skip-notify]。不要合并，不要跑二审。真书回归若 box 上没有 ~/Documents/ereader/books/（或等价副本），不要声称跑过，在结果里写出给 vinci 的 fish 命令。
```

## 结果
- 执行方 / 开始–结束时间：box-agent（oc-exec） / 2026-11-13T10:46:48+08:00–2026-11-13T10:55:54+08:00；负责 bot 返工修复固定版式阻断：2026-11-13 11:09–11:20 +08:00
- base..head（含 rustfmt 与固定版式修复）：`c408e2e77784752a7a15ffc4351d0ef787d11dd5..c3d02c127218c3d859ba80d344a48867c95a368d`
- 提交：
  | 提交号 | 说明 |
  |---|---|
  | 5e44fce | chore: 开始执行任务单 2026-11-13-vertical-default-off [skip-notify] |
  | ecdb5d4 | feat: 竖排默认改横排，Kobo、汉王只有 --on 或原书已竖排才竖排 |
  | 9c643eb | chore: 填写任务单 2026-11-13-vertical-default-off 结果（done） [skip-notify] |
  | caebf7c | chore(bookconv): 按 rustfmt 调整原书竖排判定新增代码的换行 |
  | 5e74479 | chore: 填写任务单 2026-11-13-vertical-default-off 审查（有阻断，不合并） [skip-notify] |
  | c3d02c1 | fix: 竖排判定在判断链最前排除固定版式和漫画 |
- 改动文件（实现至修复，相对 origin/master 的功能文件）：`.dev/tasks/2026-11-13-vertical-default-off.md`、`.dev/tasks/2026-11-13-vertical-default-off.agy.md`、`README.md`、`crates/bookconv/src/optimize/mod.rs`、`crates/bookconv/src/vertical.rs`、`crates/library/src/bin/booklib/main.rs`、`crates/library/src/generate.rs`、`crates/library/src/lib.rs`、`crates/library/src/session.rs`、`crates/library/src/vertical.rs`、`crates/library/tests/common/mod.rs`、`crates/library/tests/flow.rs`、`crates/library/tests/fp_golden.txt`、`docs/architecture.md`、`docs/decisions.md`、`docs/development.md`、`docs/devices.md`、`docs/typesetting.md`、`docs/usage.md`
- 固定版式修复（`c3d02c1`）：`Library::vertical_for` 把 `comic_of` 与 `is_fixed_layout` 的排除提到判断链最前（`vertical_text` 之后、覆盖与默认之前），`--on`、目录覆盖、默认分支共用。读不到原件时 `is_fixed_layout` 用 `unwrap_or(false)`，与默认「读不到按横排」一致。补夹具 `sample_epub_vertical_fixed`（原书 CSS 竖排 + OPF `fixed-layout=true`，非漫画）和测试 `fixed_layout_native_vertical_is_never_set_vertical`：汉王、Kobo 上默认和 `--on` 都不竖排（无 `Vf`、不挂 `eink-vertical.css`、`vertical_for` 为 false）。
- 测试（负责 bot，独立 worktree `/tmp/sheng-ren-review-vertical-default-off`，`CARGO_TARGET_DIR=/home/box/repos/sheng-ren/target`，`rustup run stable`，rustc 1.99.1；修复后再跑一遍）：
  - `cargo test --workspace` 全过：bookbase 30、bookconv 553、csscascade 16、kfx 53、library 79 / bin 7 / cli 14 / flow **61**（新增 1 个）/ fp_golden 1、profile 19。
  - `cargo clippy --workspace --all-targets -- -D warnings`：0 警告。
  - `cargo fmt --all -- --check`：干净（只 fmt 了本修复改过的文件）。
- 版本号：`OPTIMIZE_VERSION` 68→69（`crates/bookconv/src/optimize/mod.rs:175` 与 `docs/development.md` 版本号表第 100 行一致）；`COMIC_VERSION` 仍 68。profile toml 未改。
- `fp_golden.txt`：本次固定版式修复**没有再改**（相对 `caebf7c` / `c3d02c1` 之前，文件无 diff；`fp_golden` 测试仍通过）。黄金对拍用的是横排流式 `sample_epub`，不是固定版式，指纹路径不变。先前因版本号和默认横排而重写的说明仍有效：660 行，文字书 68→69，漫画 c68；旧 131 行 `Vf` 现为 0。
- 遇到的问题：上轮二审阻断（默认分支漏固定版式）已在 `c3d02c1` 修好。
- 偏离（包装器报 9 个文件；经对照**全部在范围内**，见上轮审查表，本次修复未新增范围外文件）。
- 未完成：临时书库步骤未能验证横竖排方向；没上真机。待 vinci 真机验收后改 verified。
- 运行记录：`/home/box/oc-runs/2026-11-13-vertical-default-off-20261113-104648.json`
- 日志：`/home/box/oc-runs/2026-11-13-vertical-default-off-20261113-104648.log`
- 模型：执行方 `opencode-go/deepseek-v4.1-flash`；二审 `gemini-3.8-flash-medium`
- 真书回归（vinci 2026-11-13 本机）：ireader、bigme、kindle、hanvon、kobo 五个模式，以及 hanvon、kobo 的 `--vertical`，全部 SAME，坏链接 0，不合法 XML 0。临时书库那一步没能验证横竖排方向（list 不显示方向，`--no-build` 不生成产物）。没上真机。
- 合并：`--no-ff` 合入 master，提交 `9777de2105aa2d08dbab6dc4db4af9c167e4efaf`（`a14200c` + `7854761`），已推 origin/master。合并后在 `/tmp/sheng-ren-merge-vertical-default-off` 用 `rustup run stable` 重跑：`cargo test --workspace` 全过（bookbase 30、bookconv 553、csscascade 16、kfx 53、library 79 / bin 7 / cli 14 / flow 61 / fp_golden 1、profile 19）；`cargo clippy --workspace --all-targets -- -D warnings` 0 警告。`fp_golden.txt` 与任务分支一致：660 行，文字书 69，漫画 c68，`Vf` 0。状态保持 **done**，待 vinci 真机验收后改 verified。远程任务分支保留，未删。

给 vinci 本机的回归命令（最终版，可直接复制粘贴；fish；仓库 `~/Projects/sheng-ren`，不是 `~/repos/sheng-ren`）。测试书只读；产物写到 `target/`；不碰 `~/.local/share/booklib`、不往真机传。

说明：`epub-optimize` **只有**加 `--vertical` 才走竖排管线，自动竖排只在 `booklib` 的 `Library::vertical_for`。标准 `tools/regress/run.sh` 不加该旗标，改动前后五个模式都应当是 `SAME`，不要预期汉王 / Kobo「变横排」。书的实际文件名请先 `ls ~/Documents/ereader/books | rg '帷幕|接头'` 确认。

```fish
cd ~/Projects/sheng-ren
git fetch origin
git switch task/2026-11-13-vertical-default-off
git pull --ff-only origin task/2026-11-13-vertical-default-off

# 旧版从当时 master 出（竖排改动之前）；已有该 worktree 则跳过
if not test -d target/regress-base
    git worktree add target/regress-base origin/master
end
(cd target/regress-base; and cargo build --release -p bookconv --bin epub-optimize; and cargo build --release -p library --bin booklib)
cargo build --release -p bookconv --bin epub-optimize
cargo build --release -p library --bin booklib

# 1）标准真书回归（docs/development.md）：epub-optimize 默认路径。五个模式都应 SAME。
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
set -gx BOOKLIB_NO_NET 1
set -gx BOOKLIB_MTP_DIR $dev
set -gx BOOKLIB_MTP_NO_MOUNT_CHECK 1
# 新 booklib：普通中文书（不是帷幕/接头、原书也不是竖排）在汉王 / Kobo 的 list 应显示「横排」
target/release/booklib --library=$lib track ~/Documents/ereader/books
target/release/booklib --library=$lib sync --no-meta --no-build
target/release/booklib --library=$lib list
# 旧 booklib 对照：同一本普通中文书在汉王 / Kobo 上应是「竖排」
target/regress-base/target/release/booklib --library=$lib-old track ~/Documents/ereader/books
target/regress-base/target/release/booklib --library=$lib-old sync --no-meta --no-build
target/regress-base/target/release/booklib --library=$lib-old list
# 若要看产物：先 booklib devices 确认只打到 $dev，再
#   target/release/booklib --library=$lib sync --device=hanvon --no-meta --force 某本书
# 不要对 ~/.local/share/booklib 跑，也不要在没改环境变量时对真机 sync。
```

合并之后、本机安装新 booklib，已有书库里《帷幕》《接头》的 `--on` 覆盖仍在，应继续竖排；其它先前因「中文文字书」自动竖排的，会掉 `V`/`f`、重建为横排。原书已竖排、但声明了固定版式的文字书，默认和 `--on` 都不走竖排管线。

## 审查
- 验收：**通过**（sheng-ren bot，2026-11-13 11:20 +08:00，审 `origin/master...c3d02c1`）。上轮阻断（默认分支漏固定版式）已在 `c3d02c1` 修好并补测。**没跑真书回归、没上真机；按本单要求不合并。** 独立 worktree 核对，rustup stable rustc 1.99.1。按 `Projects/_templates/review-checklist.md`：结果已填、状态 done；分步 1–5 完成（含返工），分步 6 因 box 无测试书未跑；提交中文前缀加正文、无 Co-Authored-By、执行方与负责 bot 均未合并。实现符合方案二和任务单；`vertical_for` 现于判断链最前排除漫画和固定版式，所有分支共用；`--on` / 目录 / 默认都不再放行固定版式；新测试覆盖汉王、Kobo 的默认与 `--on`。版本号 69/68 一致。测试、clippy、fmt 见结果。`fp_golden.txt` 本次修复未再改。
    - 包装器「偏离 9 个文件」：上轮已核，全部在范围内；本次修复只改 `vertical.rs`、`tests/common/mod.rs`、`tests/flow.rs`，在范围内。
    - `OPTIMIZE_VERSION` = 69，与 `docs/development.md` 一致；`COMIC_VERSION` 68。
- 二审：**无阻断**（agy-review，2026-11-13 11:19 +08:00，模型 gemini-3.8-flash-medium，用时 215s，回执对照 `c3d02c1`，文件 `.dev/tasks/2026-11-13-vertical-default-off.agy.md`）。结论「建议通过（无代码阻断）」。负责 bot 已读回执并核对引用：上轮阻断已修属实（`vertical.rs` 约 149–169 行前置排除；新测试实际在 `flow.rs` 约 3487–3555 行，回执写的 895–968 是行号笔误，不影响结论）。建议 1（任务单滞后于 `c3d02c1`）已在本栏和结果栏补正；建议 2（真书回归未跑则不合并）已执行；建议 3（`is_natively_vertical` 进程内缓存）不挡、本单未做。线索（原件读不到时 `unwrap_or(false)` 指纹回退横排）是有意容错，不挡。未核实项：agy 只读未跑测试（负责 bot 已重跑）；真书回归和真机仍未做。
- 合并：已于 2026-11-13 以 `--no-ff` 合入 master，提交 `9777de2105aa2d08dbab6dc4db4af9c167e4efaf`，已推 origin/master。vinci 本机真书回归已通过（五个模式及 hanvon/kobo `--vertical` 全部 SAME，坏链接 0，不合法 XML 0）；临时书库步骤未能验证方向；没上真机。状态保持 **done**（不是 verified）。待 vinci 真机验收后改 verified。远程任务分支保留，未删。

- 真机验收：vinci 2026-11-13 13:42 (+08:00) 确认通过（掌阅经 ~/Devices/Ireader 实测 list/sync 正常；其余设备以 vinci 确认为准），状态改 verified。
