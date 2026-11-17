# 拆 `csscascade/src/lib.rs`（big-file-splits 子单）

- Task ID: 2026-11-16-split-csscascade
- 负责 bot: sheng-ren
- 状态: verified
- 执行方: oc-exec
- 分支: task/2026-11-16-split-csscascade（从最新 master 切出）
- 更新时间: 2026-11-17T09:40:00+08:00
- 引用: `2026-11-13-big-file-splits` 分步 2；拆法参照已合并的 `2026-11-13-split-imgopt`、`2026-11-13-split-generate`

## 需求原文
> （质量审查发现 14）十个核心文件单文件超过 1800 行，难审、难改。只做模块拆分、不改函数逻辑与公开签名，各设备产物及 `fp_golden` 应逐字节不变。执行时一次只拆一个文件；测试数量和结果不变。
> （负责 bot 转达的第 3 项，2026-11-16）剩下的大文件按收益从大到小一张单拆一个；拆分只搬代码，不改行为；每张单证明产物逐字节不变，版本号不动。

## 输入（开单时 master，`wc -l` 3060 行，执行时以代码为准）
`crates/csscascade/src/lib.rs` 现有各段（行号约数）：
| 行 | 内容 |
|---|---|
| 1–300 | 模块说明；`pub mod decl; pub mod ua;`；`Decl`、`Rule`、`Need`、`Decls`、`Origins`、`ElementKeys`、`Rules`、`RuleIndex`、`MAX_MEDIA_NESTING`、`resolve_urls`、`leading_url`、`Sheet`、`Origin` |
| 301–710 | CSS 文本：`Lex`、`lex`、`Lexer`、`scan_css`、`strip_comments`、`RuleSpan`、`rule_spans`、`rule_head`、`rule_selector`、`split_leading_statements`、`last_compound`、`compound_tag_classes`、`font_face_rules`、`is_font_face`、`matching_close`、`parse_decls`、`split_top`、`tokens`、`background_tokens`、`tokens_with` |
| 711–1071 | 简写展开：`BoxSides`、`box_sides`、`four_values`、`expand_shorthand`、`FontShorthand`、`parse_font_shorthand`、`font_shorthand`、`BG_INITIAL`、`background_shorthand`、`parse_bg_position`、`parse_bg_size`、`BORDER_STYLES` |
| 1072–1175 | `specificity`、`body_font_size`、`weighted_mode` |
| 1176–1374 | `@media`：`MediaEnv`、`MEDIA_TYPES`、`media_ok`、`media_query`、`media_feature`、`media_px` |
| 1375–1567 | `impl Sheet`（层叠主体） |
| 1568–1733 | `BLOCK_TAGS`、`is_block`、`presentational_hints`、`FontFace`、`font_faces` |
| 1734–2018 | 值：`Len`、`split_number`、`parse_len`、`PT_PER_EM`、`PT_PER_BOX_PX`、`DEVICE_PX_EM` 等设备静态量与 `set_*`/`device_*`、`parse_box_len`、`pub use bookbase::color::…`、`Computed` 结构、`Shadow`、`parse_shadow`、`Border`、`BorderWidth`、`parse_border_width` |
| 2019–2500 | `impl Computed`（两段） |
| 2502–3060 | `#[cfg(test)] mod tests`（16 个测试） |

- 外部用法：`bookconv` 里 `pub use csscascade as cascade;`，`kfx` 里 `pub use bookconv::cascade as css;`，两边都按 `csscascade::<名字>` / `cascade::<名字>` / `css::<名字>` 用根路径下的项。**所有现在根路径下能用的 `pub` 项拆完后必须仍在根路径能用**（`pub use 子模块::…` 再导出），签名不变。

## 方案（建议拆法，可按代码现状微调，在结果里列出每个文件放了什么、多少行）
- `lib.rs`：模块说明、`mod` 声明、`pub use` 再导出、层叠核心的类型（`Decl`、`Rule`、`Need`、`Rules`、`RuleIndex`、`Sheet`、`Origin`、`Decls`、`Origins`、`ElementKeys` 等）。若仍偏长，可把 `Rules`/`Need`/`RuleIndex` 放 `rules.rs`、`impl Sheet` 放 `sheet.rs`。
- `text.rs`：CSS 文本扫描、拆分、规则位置（301–710 段）。
- `shorthand.rs`：简写展开（711–1071 段）。
- `media.rs`：`@media` 求值（1176–1374 段）。
- `html.rs`（或 `element.rs`）：块级标签、`is_block`、`presentational_hints`；`font_faces`/`FontFace` 放这里或 `text.rs`，择一。
- `value.rs`：`Len`、长度解析、设备静态量、`Shadow`、`Border` 等值类型；`specificity`、`body_font_size`、`weighted_mode` 放在最贴近用途的模块。
- `computed.rs`：`Computed` 结构与两段 `impl Computed`。
- `tests.rs`：原 `mod tests` 整体搬出，`lib.rs` 里 `#[cfg(test)] mod tests;`，测试名和路径（`tests::…`）不变。
- 模块之间用到原先私有的项，改成 `pub(crate)`（不要改成 `pub`）；原先 `pub` 的保持 `pub` 并在根路径再导出。子模块本身是私有 `mod`（`decl`、`ua` 原来就是 `pub mod`，保持）。
- 不用 `include!`。

## 规则（要实现的行为）
1. 只挪代码：函数体、分支条件、常量值、错误文案、注释文字都不改；允许的改动只有：`mod` / `use` / `pub use`、为跨模块访问把私有项改成 `pub(crate)`、每个新文件开头一段 `//!` 说明、搬出测试后的缩进变化。
2. 根路径下的 `pub` 项集合不变（名字、种类、签名）；crate 外调用方文件不改。
3. 测试集合（名字、数量 16）与结果和拆前一致。
4. 每个文件远低于 1800 行（目标各 ≤ 800）。

## 分步任务
1. [ ] 读 `lib.rs`，列出各段职责和相互调用，定拆法。
2. [ ] 拆前跑 `cargo test -p csscascade -- --list` 存测试名清单。
3. [ ] 挪代码到子模块，调整 `mod`/`use`/可见性，保证编译。
4. [ ] 拆后再列测试名，与拆前一致；`cargo test --workspace` 全过；`cargo clippy --workspace --all-targets -- -D warnings` 0 警告；版本号、`fp_golden.txt` 没动。
5. [ ] 结果里列出每个文件的内容和行数、所有可见性改动（哪些项从私有改成 `pub(crate)`）。

## 验收标准
- [ ] `lib.rs` 已拆成合理子模块，每个文件远低于 1800 行。
- [ ] 只挪代码：负责 bot 用 `git diff --color-moved` 和逐项比对核实，除允许的改动外没有别的差异。
- [ ] 根路径 `pub` 项全部仍可用（负责 bot 用一个引用全部原 `pub` 名字的临时程序编译核实）；crate 外文件未改。
- [ ] 测试 16 项名字一致、全过；`cargo test --workspace` 全过；clippy `-D warnings` 0 警告。
- [ ] 产物逐字节不变（负责 bot 用 master 与分支二进制对 ireader、kobo、kindle 各生成几本书比 sha256；本 crate 影响 KFX 和掌阅的 `kindle_rules`，kindle 必核）。版本号、`fp_golden.txt` 没动。
- [ ] 结果里写明没跑真书回归、没上真机。

## 不做什么
- 不改任何算法、正则、常量、函数签名；不合并、不改名任何函数；不顺手修 bug 或改注释。
- 不拆 `decl.rs`、`ua.rs`；不碰 big-file-splits 表里的其他文件。
- 不对整个仓库跑 `cargo fmt`（可以对新建和改动的这几个文件跑 `rustfmt`，但只能产生搬动引起的缩进变化；有别的格式变化就不要用）。

## 约束
- 只用 Rust；clean-room；仓库不出现上游项目名及缩写。
- **只挪代码、不改行为**；不升版本号；`fp_golden.txt` 一行不动；不改 `testdata/`。
- 不碰 `~/.local/share/booklib`、真机；实验用 `target/` 下临时 `--library`。
- `install.sh` 不用 `--force`。
- 只在任务分支上工作，不合并；改「涉及文件」以外的写进「结果 → 偏离」。
- 提交信息中文 + 前缀 + 正文写原因，不加 Co-Authored-By。

## 涉及文件
- `crates/csscascade/src/lib.rs`
- 新建 `crates/csscascade/src/{text,shorthand,media,html,value,computed,tests}.rs`（名字可微调）及可能的 `rules.rs`、`sheet.rs`

## 交付物
代码提交到任务分支并推送；「结果」填好；状态改为 done 或 blocked。任务单状态改动推 master，带 `[skip-notify]`。不要合并，不要跑二审。

## 给执行方的指令
```
执行任务单 .dev/tasks/2026-11-16-split-csscascade.md：先把状态改为 accepted 并提交，按「分步任务」做完，填好「结果」，状态改为 done（做不下去改为 blocked 并写明原因），提交并推送分支。任务单 done 推 master 带 [skip-notify]。不要合并，不要跑二审。
```

## 结果
- 执行方 / 开始–结束时间：box-agent（oc-exec） / 2026-11-16T15:31:07+08:00–2026-11-16T15:40:23+08:00
- base..head：`ee7d2e548c1bd8013d1d9024d948fc7c9525c3ec..b12920fc8370d0164f2c09671c6822eebc8e5f26`
- 提交：
  | 提交号 | 说明 |
  |---|---|
  | 2ee0f41 | chore: 开始执行任务单 2026-11-16-split-csscascade [skip-notify] |
  | b12920f | chore(csscascade): 拆 lib.rs 为按职责划分的子模块 |
- 改动文件：`.dev/tasks/2026-11-16-split-csscascade.md`,`crates/csscascade/src/computed.rs` `crates/csscascade/src/html.rs`,`crates/csscascade/src/lib.rs` `crates/csscascade/src/media.rs`,`crates/csscascade/src/rules.rs` `crates/csscascade/src/sheet.rs`,`crates/csscascade/src/shorthand.rs` `crates/csscascade/src/tests.rs`,`crates/csscascade/src/text.rs` `crates/csscascade/src/value.rs`
- 测试：包装器未代跑验收命令，由负责 bot 按任务单核对
- 遇到的问题：无
- 偏离：无
- 新建文件（线索）：无
- 未完成：无
- 运行记录：`/home/box/oc-runs/2026-11-16-split-csscascade-20261116-153107.json`
- 日志：`/home/box/oc-runs/2026-11-16-split-csscascade-20261116-153107.log`
- 模型：`opencode-go/deepseek-v4.1-flash`
- 用时：实际运行 9.3 分钟（墙钟 556s / 实际运行 556s；超时按实际运行计）

## 审查
<!-- 负责 bot 填写 -->
- 验收：通过（负责 bot 独立验收，2026-11-16，独立 worktree；对照 master `ee7d2e5`）。
  - 拆法（执行方结果没列，负责 bot 补）：`lib.rs` 84 行（模块说明、`mod` 声明、根路径 `pub use` 再导出、`Decl`、`Decls`、`Origins`、`resolve_urls`、`leading_url`）；`rules.rs` 238（`Rule`、`Need`、`ElementKeys`、`Rules`、`RuleIndex`、`MAX_MEDIA_NESTING`）；`sheet.rs` 217（`Sheet`、`Origin`、`impl Sheet`）；`text.rs` 505（CSS 文本扫描拆分、规则位置、`parse_decls`、`tokens`、`specificity`）；`shorthand.rs` 365（简写展开、`BORDER_STYLES`）；`media.rs` 206（`@media` 求值）；`html.rs` 173（块级标签、表现属性、嵌入字体）；`value.rs` 118（`Len`、长度解析、设备静态量）；`computed.rs` 673（`Computed` 与两段 `impl`、`Shadow`、`Border`、`body_font_size`、`weighted_mode`）；`tests.rs` 562。最大 673 行。
  - 只挪代码：按顶层项逐个比对（去掉 `pub(crate)` 差异后），旧文件 111 项全部在新文件里原样出现；唯一文本差异是 `matching_close` 加了 `pub(crate)` 后签名被 rustfmt 折成多行（函数体逐字相同）。其余新增只有 `use`、`mod`、`pub use`、各文件 `//!` 说明。测试：`tests.rs` 与原 `mod tests` 去掉 4 格缩进后逐行相同，只多了开头的 `use`，以及一行因缩进变短被 rustfmt 合并成一行的 `let html = …`（内容不变）。
  - 可见性：只有私有 → `pub(crate)`（`Rule`、`Need`、`ElementKeys`、`RuleIndex` 及其字段和方法、`Rules` 的 `list`/`index` 字段、`Sheet::parts`、`resolve_urls`、`leading_url`、`split_number`、`PT_PER_BOX_PX`、`MAX_MEDIA_NESTING`、`rule_head`、`matching_close`、`four_values`、`expand_shorthand`、`BORDER_STYLES`）；没有新的 `pub`。
  - 公开接口：master 根路径下 50 个 `pub` 项与新根路径导出的集合完全相同（不多不少）；另用一个临时程序 `use csscascade::{全部 50 个名字, decl, ua, 颜色函数}` 对分支编译通过。crate 外文件未改。
  - 测试：csscascade 16 项名字与 master 一致；`cargo test --workspace` 864 通过 0 失败；clippy `-D warnings` 0 警告。
  - 产物：master 与分支二进制对 4 本书（两本文字书含 CSS 简写、`@media`、`!important`、选择器、注释、表格、多种图片，两卷 CBZ 漫画）同步到 ireader、kobo、kindle，12 份产物 sha256 全部相同，输出除路径外相同。版本号、`fp_golden.txt` 没动。
  - 没跑真书回归、没上真机。
- 二审：agy-review（gemini-3.8-flash-medium）建议通过；阻断、建议、线索都无。见 `2026-11-16-split-csscascade.agy.md`。
- 合并：8a1f775（`--no-ff` 进 master；master 合并前没动，合并树与已验收的分支末端相同：test 864 通过、clippy 0 警告）。没跑真书回归、没上真机。任务分支合并后删除。状态保持 done，等 vinci 真机验收。

- 最终验收：**通过**（vinci，2026-11-17，上海时间）。
  - 真书回归：本轮脚本对比的是 `4b1659f` vs `a750b6f`，基线已含本单改动，**不覆盖**本单，未记真书回归通过。
  - 真机：按建议安装当前 master，并做同步与文字/漫画抽查（打开、目录、注释、图片、翻页，以及同步/重复同步）。vinci 告知「已验证没有问题」。只记录这一报告；未记录具体设备型号、书名或专项故障测试。挂载卡死、僵尸工作进程、30/60 秒超时阈值等边界未验证。
  - 状态改为 verified。
