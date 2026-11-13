# 竖排默认改横排（推翻「中文文字书全部竖排」）

- Task ID: 2026-11-13-vertical-default-off
- 负责 bot: sheng-ren
- 状态: accepted
- 执行方: box-agent
- 分支: task/2026-11-13-vertical-default-off（从最新 master 切出）
- 更新时间: 2026-11-13T10:46:48+08:00
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
1. [ ] 落实「原书已竖排」判定：扩展 `is_natively_vertical`（补 OPF `primary-writing-mode`），书库侧在 `vertical_for` 默认分支使用它，替换 `zh_text_of`。补单元测试。
2. [ ] `--auto` 语义、帮助文字、`vertical_books_follow_profile_overrides_and_directory_lists` 等测试改成新语义。
3. [ ] `OPTIMIZE_VERSION` 68→69（表 + 代码常量 + 变更说明）；按需要更新 `fp_golden.txt`；确认指纹：自动竖排的书掉 `V`/`f`，`--on` / 原书已竖排的还在。
4. [ ] 写 `docs/decisions.md`（推翻 11-11 那条并写新决定），改 usage / devices / architecture / README，以及过时的 typesetting。
5. [ ] `cargo test --workspace` 全过；`cargo clippy --workspace --all-targets -- -D warnings` 0 警告。
6. [ ] 按 `docs/development.md` 跑真书回归（见验收标准）。box 上若没有测试书，不要声称跑过，在结果里写清需在 vinci 本机跑的 fish 命令。

## 验收标准
- [ ] 默认：中文文字书在 Kobo、汉王上不再因 `is_zh_text` 而竖排；`--on` 或原书已竖排的仍竖排；`--off` 强制横排。有测试覆盖（含目录清单、`--auto` 回到新默认）。
- [ ] `is_natively_vertical` 认 writing-mode（原有）和 OPF `primary-writing-mode`（新增）；单独 spine rtl 不算。
- [ ] `--auto` 帮助和文档与新语义一致。
- [ ] Kobo、汉王一起变；bigme、kindle、ireader 的 profile 和竖排行为无变化。
- [ ] `OPTIMIZE_VERSION` = 69（表与常量一致）；`COMIC_VERSION` 仍 68。先前自动竖排的书指纹掉 `V`/`f` 并会重建。`fp_golden.txt` 若因版本号更新，在结果里说明。
- [ ] `docs/decisions.md` 写明推翻 11-11；usage / devices / architecture / README（及过时的 typesetting）已同步。
- [ ] `cargo test --workspace` 全过；clippy `-D warnings` 0 警告。
- [ ] 真书回归：有测试书则在 `target/` 下临时目录跑并记录；没有则写明「真书回归需在 vinci 本机跑」和具体 fish 命令，**不要合并**（由负责 bot 执行这条）。结果里写明没上真机。

## 交付物
代码提交到任务分支并推送；「结果」填好；状态改为 done 或 blocked。任务单状态改动推 master，带 `[skip-notify]`。不要合并，不要跑二审（负责 bot 验收和二审）。box 上没有测试书时负责 bot **不要合并**。

## 给执行方的指令
```
执行任务单 .dev/tasks/2026-11-13-vertical-default-off.md：先把状态改为 accepted 并提交，按「分步任务」做完，填好「结果」，状态改为 done（做不下去改为 blocked 并写明原因），提交并推送分支。任务单 done 推 master 带 [skip-notify]。不要合并，不要跑二审。真书回归若 box 上没有 ~/Documents/ereader/books/（或等价副本），不要声称跑过，在结果里写出给 vinci 的 fish 命令。
```

## 结果
<!-- 执行方填写 -->
- 执行方 / 开始–结束时间：
- base..head：
- 提交：| 步骤 | 提交号 | 文件 | 说明 |
- 测试：| 命令 | 通过 / 失败 / 未跑 | 摘要 |
- 版本号：OPTIMIZE_VERSION ？；fp_golden 是否更新：
- 真书回归：
- 遇到的问题：
- 偏离：无
- 未完成：
- 没上真机。

## 审查
<!-- 负责 bot 填写 -->
- 验收：
- 二审：
- 合并：
