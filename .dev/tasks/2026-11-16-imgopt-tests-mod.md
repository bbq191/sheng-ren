# imgopt 测试收尾：`include!` 改成普通子模块，删掉一条重复的文档注释

- Task ID: 2026-11-16-imgopt-tests-mod
- 负责 bot: sheng-ren
- 状态: accepted
- 执行方: oc-exec
- 分支: task/2026-11-16-imgopt-tests-mod（从最新 master `ef478be` 切出）
- 更新时间: 2026-11-16T15:17:22+08:00
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
<!-- 执行方填写 -->

## 审查
<!-- 负责 bot 填写 -->
