# 拆 `bookconv/src/imgopt.rs`（big-file-splits 子单 1）

- Task ID: 2026-11-13-split-imgopt
- 负责 bot: sheng-ren
- 状态: done
- 执行方: sheng-ren（box executor，接手中断的 oc-exec）
- 分支: task/2026-11-13-split-imgopt（从最新 master 切出）
- 更新时间: 2026-11-13T17:05:00+08:00
- 引用: 2026-11-13-big-file-splits 分步 1；前一张子单 2026-11-13-split-generate 已合并 0d85255

## 需求原文
> （质量审查发现 14）十个核心文件单文件超过 1800 行，难审、难改。只做模块拆分、不改函数逻辑与公开签名，各设备产物及 `fp_golden` 应逐字节不变。执行时一次只拆一个文件；测试数量和结果不变。
> （2026-11-13）big-file-splits 按文件拆子单，本单只拆 `crates/bookconv/src/imgopt.rs`。

## 输入
- 基于最新 origin/master（含 split-generate 合并 `0d85255`）。
- `crates/bookconv/src/imgopt.rs`（约 3900 行）：按职责拆成 `imgopt/` 下的子模块（审查建议如页面缓冲 / 漫画处理 / 封面缩略图 / 测试），`imgopt.rs` 只留入口与再导出。**不改任何图像算法、参数、常量、像素处理顺序**，产物必须逐字节不变。可参照已合并的 `crates/library/src/generate/` 拆法。具体分法按代码现状定，在结果里列出每个子模块放了什么。

## 约束
- 只用 Rust；clean-room；仓库不出现上游项目名及缩写。
- **只挪代码、不改行为**：不改函数体、分支条件、错误文案、写盘时机、公开签名。允许 `pub(crate) use` 再导出以保持 crate 内原有 `use` 路径，或同步改 crate 内引用。
- **只拆这一个文件**，不碰 big-file-splits 表里的其他文件，不顺手做别的重构。
- **不升版本号**；`fp_golden.txt` 一行不动；不改 `testdata/`。
- 不碰 `~/.local/share/booklib`，实验用 `target/` 下临时 `--library`；测试不碰真机。
- `install.sh` 不用 `--force`。本仓库不用 rustfmt 默认宽度，别对整个仓库跑 `cargo fmt`。
- 提交信息中文 + 前缀 + 正文，不加 Co-Authored-By。

## 规则（要实现的行为）
1. 拆完后 crate 能编译，对外 API 与行为不变；测试集合（名称、数量）与结果和拆前一致。
2. 每个子文件远低于 1800 行。

## 分步任务
1. [x] 读 `imgopt.rs`，列出各段职责与相互调用，定拆法。
2. [x] 挪代码到子模块，调整 `mod`/`use`，保证编译。
3. [x] 拆前、拆后各跑一次 `cargo test -p bookconv`，测试数量一致；`cargo test --workspace` 全过；`cargo clippy --workspace --all-targets -- -D warnings` 0 警告；确认版本号与 `fp_golden.txt` 没动。

## 验收标准
- [x] `imgopt.rs` 已拆成合理子模块，每个文件远低于 1800 行。
- [x] 测试数量与结果与拆前一致；workspace 测试、clippy 干净。
- [x] 版本号和 `fp_golden.txt` 都没动；结果写明只是挪代码，没跑真书回归、没上真机，待本机回归确认产物逐字节不变。

## 交付物
代码提交到任务分支并推送；「结果」填好；状态改为 done 或 blocked。任务单状态改动推 master，带 `[skip-notify]`。不要合并，不要跑二审。

## 给执行方的指令
```
执行任务单 .dev/tasks/2026-11-13-split-imgopt.md：先把状态改为 accepted 并提交，按「分步任务」做完，填好「结果」，状态改为 done（做不下去改为 blocked 并写明原因），提交并推送分支。任务单 done 推 master 带 [skip-notify]。不要合并，不要跑二审。
```

## 结果
- 执行方 / 开始–结束时间：sheng-ren（box executor，接手中断的 oc-exec） / 2026-11-13T16:49:00+08:00–2026-11-13T16:56:30+08:00
- 做法：续做上一轮 oc-exec 留在工作树里的未提交半成品（不是重拆）。先用脚本把拆后全部函数 / impl / const / 测试块与 fe2973d 的 `imgopt.rs` 逐块比对，确认无遗漏、函数体逐字一致，只补验收、提交与推送。
- 拆法（原 3916 行，按职责）：
  | 文件 | 行数 | 内容 |
  |---|---:|---|
  | `crates/bookconv/src/imgopt.rs` | 61 | 模块说明、入口、`test_screen`，以及 `pub` / `pub(crate) use` 再导出（保持 crate 内原路径与公开签名） |
  | `imgopt/decode.rs` | 377 | 文件头、解码、缩放、透明合成（`ImgHead`、`decode_*`、`header_dims`、`webp_to_png`、`flatten_*`、`fir_lanczos3`、像素上限与 JPEG 质量常量等） |
  | `imgopt/page8.rs` | 313 | 归一后的整页 `Page8` 与灰度转换（`into_luma8`、`rgb8_into_luma8`、`darkness_plane`） |
  | `imgopt/comic.rs` | 739 | 裁边、漫画页解码与排版、跨页拆分、伽马与自动对比度（`TrimMode`、`decode_page`、`prepare_comic_page_*`、`prepare_comic_spread`、`gamma_lut`、`autocontrast_lut` 等） |
  | `imgopt/thumbnail.rs` | 153 | 书架封面缩略图（`cover_thumbnail`、`thumb_aspect_fits`） |
  | `imgopt/textbook.rs` | 158 | 文字书插图、背景图与条目元数据（`downscale_for_epub*`、`page_tile`、`darken_gray_image`、`is_page_image` 等） |
  | `imgopt/tests/mod.rs` | 117 | 测试入口与共用造图辅助；用 `include!` 内联下面各段，测试路径仍是 `imgopt::tests::*` |
  | `imgopt/tests/comic.rs` | 458 | 漫画页准备、灰度、伽马、自动对比度、跨页宽高 |
  | `imgopt/tests/dark.rs` | 317 | 深色背景填充、16 位灰度、跨页对半 |
  | `imgopt/tests/formats.rs` | 323 | GIF/WebP、EXIF、公开页 API、亮度换算 |
  | `imgopt/tests/text.rs` | 625 | 封面缩略图、文字书降采样、排版边距 |
  | `imgopt/tests/trim.rs` | 371 | 裁边、透明合成、解码上限 |
- 逐块比对（对照 `fe2973d` 的 `imgopt.rs`）：抽出 179 个叶子项（fn / struct / enum / const / impl 及方法、全部测试函数与辅助函数）。0 遗漏、0 多余。143 个全文逐字相同；其余 36 个差异全部是允许的可见性调整（跨子模块把原先私有的字段或函数改成 `pub(crate)`，函数体、分支条件、错误文案、常量值未改）。原文件 243 块注释全部保留。公开 `pub` / 原有 `pub(crate)` 项均由入口再导出，签名不变。
- 测试：在临时 worktree `/tmp/sheng-ren-worktrees/fe2973d`（基线 `fe2973d`）和拆后各跑 `cargo test -p bookconv`，都是 **553 通过 / 0 失败**；其中 `imgopt::tests::*` **55 项，名称与结果完全一致**。`cargo test --workspace` 全过：bookbase lib 30，bookconv lib 553，csscascade lib 16，kfx lib 53，library lib 82 / bin 7 / cli 14 / flow 63 / fp_golden 1，profile lib 19。`cargo clippy --workspace --all-targets -- -D warnings` **0 警告**。
- 版本号未动（OPTIMIZE 69、COMIC 68、KINDLE_RULES 42、CONVERT 2、WRITER 26、PIPELINE 5）；`crates/library/tests/fp_golden.txt` 与基线逐字节相同；`testdata/` 未动。未对整个仓库跑 `cargo fmt`。
- **只是挪代码，没跑真书回归、没上真机，待本机回归确认产物逐字节不变。**
- base..head：`fe2973d..07002437f871fc3b2af723f2e8b47fd4e838b7f9`（实现提交 `0700243`）
- 提交：
  | 提交号 | 说明 |
  |---|---|
  | 5146a9b | chore: 开始执行任务单 2026-11-13-split-imgopt [skip-notify]（第一轮 oc-exec） |
  | 3944183 | chore: 填写任务单 2026-11-13-split-imgopt 结果（blocked） [skip-notify] |
  | 9f718b8 | chore: 开始执行任务单 2026-11-13-split-imgopt [skip-notify]（第二轮 oc-exec） |
  | 88bffc8 | chore: 填写任务单 2026-11-13-split-imgopt 结果（blocked） [skip-notify] |
  | 0700243 | chore(bookconv): 把 imgopt.rs 拆成 imgopt/ 子模块 |
- 改动文件：`.dev/tasks/2026-11-13-split-imgopt.md`，`crates/bookconv/src/imgopt.rs`，以及新建的 `crates/bookconv/src/imgopt/{decode,page8,comic,thumbnail,textbook}.rs` 与 `crates/bookconv/src/imgopt/tests/{mod,comic,dark,formats,text,trim}.rs`
- 遇到的问题：前两轮 oc-exec 因模型接口断连中断，工作树留下未提交半成品；本轮在该半成品上核对后验收通过，未重拆。
- 偏离：无（新建的 `imgopt/` 子模块均为本单要求的拆分产物；已有文件只改了被拆的入口 `imgopt.rs` 和任务单本身）。测试文件用 `include!` 而不是 `mod`，是为了测试名称仍为 `imgopt::tests::*`，与拆前一致。
- 未完成：无。真书回归与真机验收待 vinci。
- 运行记录（中断的两轮 oc-exec，供对照）：`/home/box/oc-runs/2026-11-13-split-imgopt-20261113-144350.json`、`/home/box/oc-runs/2026-11-13-split-imgopt-20261113-160920.json`

## 审查
<!-- 负责 bot 填写 -->
- 验收：**通过**（sheng-ren bot，2026-11-13 16:57–17:05 Asia/Shanghai，仓库 `/home/box/repos/sheng-ren` @ `b1fc7b1`，实现提交 `0700243`，对照 origin/master `fe2973d`）。独立核对，未采信执行方结论。**没跑真书回归、没上真机**；待 vinci 本机回归确认产物逐字节不变。按 `Projects/_templates/review-checklist.md`「项目 bot 验收」：结果已填、状态 done；分步 1–3 完成；提交中文前缀加正文、无 Co-Authored-By、执行方未合并。工具链 rustc 1.99.1。
  - 范围：`fe2973d..0700243` 只改了被拆的入口 `crates/bookconv/src/imgopt.rs`、新建的 `imgopt/{decode,page8,comic,thumbnail,textbook}.rs` 与 `imgopt/tests/{mod,comic,dark,formats,text,trim}.rs`，以及任务单本身。没有范围外的已有文件。真实偏离：无。
  - 子模块划分（各文件远低于 1800 行）：`imgopt.rs` 61 行（模块说明、入口、`test_screen`、`pub` / `pub(crate) use` 再导出）；`decode.rs` 377（文件头、解码、缩放、透明合成：`ImgHead`、`decode_*`、`header_dims`、`webp_to_png`、`flatten_*`、`fir_lanczos3`、像素上限与 JPEG 质量常量）；`page8.rs` 313（`Page8` 与灰度转换：`into_luma8`、`rgb8_into_luma8`、`darkness_plane`）；`comic.rs` 739（裁边、漫画页解码与排版、跨页拆分、伽马与自动对比度：`TrimMode`、`decode_page`、`prepare_comic_page_*`、`prepare_comic_spread`、`gamma_lut`、`autocontrast_lut` 等）；`thumbnail.rs` 153（`cover_thumbnail`、`thumb_aspect_fits`）；`textbook.rs` 158（文字书插图、背景图与条目元数据：`downscale_for_epub*`、`page_tile`、`darken_gray_image`、`is_page_image` 等）；`tests/mod.rs` 117（测试入口与共用造图辅助，`include!` 内联各段）；`tests/comic.rs` 458、`tests/dark.rs` 317、`tests/formats.rs` 323、`tests/text.rs` 625、`tests/trim.rs` 371。划分按职责，合理。
  - 只挪代码（独立用 `syn` 抽出并逐块比对，对照 `fe2973d:crates/bookconv/src/imgopt.rs`）：叶子项（fn / struct / enum / const / 含 impl 方法与全部测试）基线 179、拆后 179，0 遗漏、0 多余。144 个全文逐字相同；34 个差异全部是允许的可见性放宽（跨子模块把原先私有的类型、字段或函数改成 `pub(crate)`，函数体、分支条件、错误文案、常量值未改）。未见改成对外的 `pub`。原先就是 `pub` 的项（含 `Page8` / `PageDecode` / `ComicPage` / `TrimMode` 的定义与公开字段、以及 `Page8` 上原先 `pub` 的方法）保持 `pub`，并由入口 `pub use` 再导出，签名与 crate 外路径不变；原先 `pub(crate)` 的（`guard`、`test_screen`、`TEST_ANIM_WEBP`）由入口 `pub(crate) use` 或仍留在入口，crate 内原路径不变。crate 外调用方文件未改。
  - 注释：基线 432 条注释文本全部保留（0 遗漏）。多余的是各新文件的模块说明，以及下面这条建议里的重复文档注释。
  - 建议（非阻断）：
    1. `imgopt/tests/text.rs` 第 394 行多了一条 `/// 造一张带纯白边框的图：中心是彩色渐变，四边留白。`，贴在 `downscale_for_epub_keeps_resize_dimensions_and_gray_jpeg` 的文档前面。这条注释原属 `framed_jpeg`（现已正确放在 `tests/trim.rs`），是按段切开时留在切分点上的重复，函数体本身与基线逐字相同。建议以后删掉这行，本次不改代码。
    2. 测试用 `include!` 把 `comic.rs` 等拼进 `tests/mod.rs`，目的是测试路径仍为 `imgopt::tests::*`，与拆前一致，也满足本单「测试集合（名称、数量）与结果和拆前一致」。仓库里另一处 `include!` 是 `profile` 的生成代码，惯例不同；前一张 `split-generate` 把测试放进普通子模块 `generate/tests.rs`（`mod remove_tests` 等），完整路径多了一层 `tests::`，当时验收接受。`include!` 能工作（clippy 0 警告、测试名不变），但文件保留了原模块内的缩进、`use` 写在 `include!` 之后，对 IDE 跳转也不如普通 `mod` 自然。若改成普通子模块（`mod comic;` 等），路径会变成 `imgopt::tests::comic::*`，按 generate 的先例可接受，也更清晰。本次不改代码。
  - 版本号未动（OPTIMIZE 69、COMIC 68、KINDLE_RULES 42、CONVERT 2、WRITER 26、PIPELINE 5）；`crates/library/tests/fp_golden.txt` 与 master 逐字节相同；testdata 未动。
  - 测试：本机重跑 `cargo test --workspace` 全过：bookbase lib 30，bookconv lib **553**，csscascade lib 16，kfx lib 53，library lib 82 / bin 7 / cli 14 / flow 63 / fp_golden 1，profile lib 19。其中 `imgopt::tests::*` **55 项，名称与 `fe2973d` 原文 `#[test]` 清单完全一致**，全部通过。`cargo clippy --workspace --all-targets -- -D warnings` **0 警告**。未对整个仓库跑 `cargo fmt`。
  - 阻断：无。
- 二审：
- 合并：
