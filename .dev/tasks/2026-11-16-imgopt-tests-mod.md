# imgopt 测试收尾：`include!` 改成普通子模块，删掉一条重复的文档注释

- Task ID: 2026-11-16-imgopt-tests-mod
- 负责 bot: sheng-ren
- 状态: verified
- 执行方: oc-exec
- 分支: task/2026-11-16-imgopt-tests-mod（从最新 master `ef478be` 切出）
- 更新时间: 2026-11-17T09:40:00+08:00
- 引用: `2026-11-13-split-imgopt` 审查「建议（非阻断）」1、2，二审回执 `2026-11-13-split-imgopt.agy.md` 同两条

## 需求原文
> （vinci 2026-11-16，sheng-ren 聊天）按1-4先做吧
> （负责 bot 转达的第 4 项）拆 imgopt 那张单验收时留了两件事：一处重复的注释，和用 `include!` 引入的文件要改成正常的 `mod`。如果改成 `mod` 会牵扯到可见性或者其他调整，以简单稳妥为准。

## 输入（开单时 master `ef478be`，负责 bot 读代码核实）
1. **重复注释**：`crates/bookconv/src/imgopt/tests/text.rs:394` 有一行 `/// 造一张带纯白边框的图：中心是彩色渐变，四边留白。`，贴在 `downscale_for_epub_keeps_resize_dimensions_and_gray_jpeg` 的文档前面。它原属 `framed_jpeg`（拆前 `fe2973d:crates/bookconv/src/imgopt.rs:2582`），`framed_jpeg` 连同这行注释已正确放在 `tests/trim.rs`；`text.rs` 里这行是按段切开时留下的重复。
2. **`include!`**：`crates/bookconv/src/imgopt/tests/mod.rs:3-7` 用 `include!` 把 `comic.rs`、`dark.rs`、`formats.rs`、`text.rs`、`trim.rs` 拼进 `imgopt::tests`；`use` 写在 `include!` 之后，各文件保留了原模块内 4 格缩进。仓库里唯一另一处 `include!` 是 `profile` 引入构建生成代码（`profile/src/lib.rs:18`），不在本单。
3. 核实过的依赖关系（改成 `mod` 不需要动可见性）：
   - 五个文件里的测试用到的共用辅助（`jpeg_of`、`gray_jpeg_of`、`test_area`、`prep`、`decode_trim_comic`、`dark_margins`、`black_jpeg`、`rgb_jpeg`、`mean_luma`）**全部定义在 `tests/mod.rs`**；五个文件之间没有互相调用。
   - 子模块能看到祖先模块的私有项，所以每个子文件开头写 `use super::*;` 即可拿到 `tests/mod.rs` 的辅助、它的 `use`（`image`、`profile`、`std::io::Cursor` 等）和 `imgopt` 的项，不需要加 `pub` / `pub(super)`。
   - 五个文件里没有跨行字符串字面量，去掉 4 格缩进不会改任何字符串内容。
4. 现状测试：`imgopt::tests::*` 55 项，bookconv lib 共 553 项。改成子模块后完整路径多一层（如 `imgopt::tests::comic::…`），与 `split-generate` 的先例（`generate/tests.rs` 里的子模块）一致，当时验收接受。

## 方案
- `tests/mod.rs`：把 5 行 `include!("x.rs");` 换成 `mod comic; mod dark; mod formats; mod text; mod trim;`，放在 `use` 之后、辅助函数之前；模块说明和辅助函数不动（只去掉缩进，见下）。
- 五个子文件：开头已有一行 `//` 段落说明，保留并改成 `//!` 模块文档；其后加 `use super::*;`；其余每行**只去掉行首正好 4 个空格**（空行不动，其他字符一个不改）。
- `tests/mod.rs` 里辅助函数同样只去掉行首 4 格缩进。
- 删掉 `text.rs` 那一行重复的文档注释。
- 如果某个子文件 `use super::*;` 后还缺名字（例如编译器提示某个 `use` 只在 mod.rs 被 glob 导入的路径上找不到），**在该子文件里补对应 `use`**；不要改任何项的可见性，不要挪函数。出现 `unused_imports` 就把那条 `use` 挪到真正用它的子文件里。
- 选这个做法的理由：最小改动，测试本体逐字不变（`git diff -w` 只剩 `mod` / `use` / 文档注释几行），不涉及可见性。

## 规则（要实现的行为）
1. 不改任何测试的函数体、断言、辅助函数实现、测试名；只改模块组织、缩进和那一行重复注释。
2. 测试数量不变：`imgopt` 下仍是 55 项、名字（最后一段）与改前一一对应；bookconv lib 仍 553。
3. 生产代码（`imgopt.rs` 和 `imgopt/` 下非 `tests/` 的文件）一行不动。

## 分步任务
1. [ ] 改前跑 `cargo test -p bookconv --lib imgopt:: -- --list`，存下测试名清单。
2. [ ] 按「方案」改 `tests/mod.rs` 和五个子文件，删掉重复注释。
3. [ ] 改后再列一次，按最后一段名字比对：55 项一一对应、无增无减。
4. [ ] `git diff -w master` 核对：除 `mod` / `use super::*;` / `//`→`//!` / 删掉的那行注释 / 可能补的 `use` 之外没有别的改动，写进结果。
5. [ ] `cargo test --workspace` 全过；`cargo clippy --workspace --all-targets -- -D warnings` 0 警告；版本号、`fp_golden.txt` 没动。

## 验收标准
- [ ] 仓库里 `crates/bookconv/src/imgopt/` 下不再有 `include!`；五个测试文件是普通子模块。
- [ ] `text.rs` 里那行重复的文档注释已删；`trim.rs` 里 `framed_jpeg` 的注释还在。
- [ ] `git diff -w` 只剩模块组织相关的几行；测试函数体逐字不变。
- [ ] `imgopt` 测试 55 项名字一一对应、全过；bookconv lib 553；`cargo test --workspace` 全过；clippy `-D warnings` 0 警告。
- [ ] 生产代码没动；产物逐字节不变（负责 bot 用 master 与分支二进制对照 ireader / kobo / kindle）；版本号、`fp_golden.txt` 没动。
- [ ] 结果里写明没跑真书回归、没上真机。

## 不做什么
- 不改可见性，不挪函数到别的文件，不改测试内容，不新增测试。
- 不动 `profile` 的 `include!`（构建生成代码，惯例不同）。
- 不对整个仓库跑 `cargo fmt`，也不对这几个文件跑 rustfmt（会改出别的格式差异）；只做去掉 4 格缩进。
- 不顺手改别处注释（`tests/comic.rs` 开头 `guard` 相关文档注释贴在 `webp_to_png_takes_first_frame_of_animation` 前，是拆前原文就这样，本单不动）。

## 约束
- 只用 Rust；clean-room；仓库不出现上游项目名及缩写。
- **只挪代码、不改行为**；不升版本号；`fp_golden.txt` 一行不动；不改 `testdata/`。
- 不碰 `~/.local/share/booklib`、真机；实验用 `target/` 下临时 `--library`。
- `install.sh` 不用 `--force`。
- 只在任务分支上工作，不合并；改「涉及文件」以外的写进「结果 → 偏离」。
- 提交信息中文 + 前缀 + 正文写原因，不加 Co-Authored-By。

## 涉及文件
- `crates/bookconv/src/imgopt/tests/mod.rs`
- `crates/bookconv/src/imgopt/tests/{comic,dark,formats,text,trim}.rs`

## 交付物
代码提交到任务分支并推送；「结果」填好；状态改为 done 或 blocked。任务单状态改动推 master，带 `[skip-notify]`。不要合并，不要跑二审。

## 给执行方的指令
```
执行任务单 .dev/tasks/2026-11-16-imgopt-tests-mod.md：先把状态改为 accepted 并提交，按「分步任务」做完，填好「结果」，状态改为 done（做不下去改为 blocked 并写明原因），提交并推送分支。任务单 done 推 master 带 [skip-notify]。不要合并，不要跑二审。
```

## 结果
- 执行方 / 开始–结束时间：box-agent（oc-exec） / 2026-11-16T15:17:22+08:00–2026-11-16T15:22:25+08:00
- base..head：`1c0d39c1449d3bc5aee786d0bd7379b37341acae..80e063cbf094bffe0c2d31b5719857e6049ea54a`
- 提交：
  | 提交号 | 说明 |
  |---|---|
  | 711e5f3 | chore: 开始执行任务单 2026-11-16-imgopt-tests-mod [skip-notify] |
  | 80e063c | chore(bookconv): imgopt 测试改用普通子模块，删掉重复的文档注释 |
- 改动文件：`.dev/tasks/2026-11-16-imgopt-tests-mod.md`,`crates/bookconv/src/imgopt/tests/comic.rs` `crates/bookconv/src/imgopt/tests/dark.rs`,`crates/bookconv/src/imgopt/tests/formats.rs` `crates/bookconv/src/imgopt/tests/mod.rs`,`crates/bookconv/src/imgopt/tests/text.rs` `crates/bookconv/src/imgopt/tests/trim.rs`
- 测试：包装器未代跑验收命令，由负责 bot 按任务单核对
- 遇到的问题：无
- 偏离：无
- 新建文件（线索）：无
- 未完成：无
- 运行记录：`/home/box/oc-runs/2026-11-16-imgopt-tests-mod-20261116-151722.json`
- 日志：`/home/box/oc-runs/2026-11-16-imgopt-tests-mod-20261116-151722.log`
- 模型：`opencode-go/deepseek-v4.1-flash`
- 用时：实际运行 5.0 分钟（墙钟 303s / 实际运行 303s；超时按实际运行计）

## 审查
<!-- 负责 bot 填写 -->
- 验收：通过（负责 bot 独立验收，2026-11-16，独立 worktree）。
  - 范围：只改了 `imgopt/tests/` 下 6 个文件和任务单；生产代码一行没动。仓库 `crates/bookconv` 下已没有 `include!`。
  - 逐行核对（脚本把 master 每行去掉行首 4 格后和分支比）：除了 5 个子文件开头 `//`→`//!` + `use super::*;`、`mod.rs` 里 5 行 `include!` 换成 5 行 `mod`、`text.rs` 删掉那行重复注释之外，没有任何别的差异；没有补 `use`、没改可见性。`trim.rs` 里 `framed_jpeg` 的注释还在。
  - 测试名：master（`1c0d39c`）与分支各 `--list`，`imgopt` 下都是 55 项，最后一段名字完全一致；完整路径多一层子模块（如 `imgopt::tests::comic::…`），同 split-generate 先例。
  - 产物：master 与分支二进制对 4 本书（两本文字书含 CSS、注释、表格、多种图片，两卷 CBZ 漫画）同步到 ireader、kobo、kindle，共 12 份产物 sha256 全部相同，命令输出除路径外相同。版本号、`fp_golden.txt` 没动。
  - `cargo test --workspace` 864 通过 0 失败；clippy `-D warnings` 0 警告。
  - 过程说明：第一次 oc-exec 因为负责 bot 这边的工具调用超时被连带结束，改动未提交，已丢弃并重跑；重跑一次完成。
  - 没跑真书回归、没上真机。
- 二审：agy-review（gemini-3.8-flash-medium）建议通过；阻断、建议、线索都无。见 `2026-11-16-imgopt-tests-mod.agy.md`。
- 合并：be46ea4（`--no-ff` 进 master；master 合并前没动，合并树与已验收的分支末端相同：test 864 通过、clippy 0 警告）。没跑真书回归、没上真机。任务分支合并后删除。状态保持 done，等 vinci 真机验收。

- 最终验收：**通过**（vinci，2026-11-17，上海时间）。
  - 真书回归：本轮脚本对比的是 `4b1659f` vs `a750b6f`，基线已含本单改动，**不覆盖**本单，未记真书回归通过。
  - 真机：按建议安装当前 master，并做同步与文字/漫画抽查（打开、目录、注释、图片、翻页，以及同步/重复同步）。vinci 告知「已验证没有问题」。只记录这一报告；未记录具体设备型号、书名或专项故障测试。挂载卡死、僵尸工作进程、30/60 秒超时阈值等边界未验证。
  - 状态改为 verified。
