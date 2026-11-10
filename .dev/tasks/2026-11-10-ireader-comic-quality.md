# 掌阅漫画打开四项画质处理，四台漫画画质一致

- Task ID: 2026-11-10-ireader-comic-quality
- 负责 bot: sheng-ren
- 状态: pending
- 执行方: opencode-local
- 分支: feat/ireader-comic-quality（从最新 master 切出）
- 更新时间: 2026-11-10T10:30+08:00
- 引用: 无（PRD：vault `Projects/sheng-ren/prd/2026-11-10-comic-layout-direction.md` 的 A 节）

## 需求原文
> 5. 全部设备都需要一致的画质处理
>
> （sheng-ren 列的现状：掌阅「拉对比度、裁杂点、裁页码、黑底补边这四项都没开」；建议「掌阅打开已有的四项画质处理」。）

## 输入
- 背景：vault `Projects/sheng-ren/context.md`（2026-10-30 Kindle 拉满对比度、裁页码、杂点；2026-11-01 黑底页）；仓库 `docs/decisions.md`、`docs/devices.md` 漫画那张表
- 现状：`kindle.toml`、`kobo.toml`、`hanvon.toml` 都有 `comic_gamma = 1.1`、`comic_autocontrast = true`、`comic_trim_specks = true`、`comic_page_numbers = true`、`comic_dark_bg = true`、`comic_margin = 1`；`ireader.toml` 只有 `comic_gamma = 1.1`、`comic_margin = 1`。`comic_upscale` 缺省开、`image_gamma = 1.1`，四台已一样
- 涉及文件：
  - `crates/profile/profiles/ireader.toml`（修改：加四行开关和注释）
  - `crates/library/tests/fp_golden.txt`（修改：只允许 `ireader` 开头、第三列 `comic` 的行变化）
  - `crates/profile/src/lib.rs`（修改，可选：内置 profile 测试里若有断言 ireader 这四项为 false 的，改成 true；没有就补一条断言四台这四项都是 true）
  - `docs/devices.md`（修改：漫画表 `comic_autocontrast`、`comic_trim_specks`、`comic_page_numbers`、`comic_dark_bg` 四行的 ireader 列 / 说明改成开，写 2026-11-10 维护者定四台一致、掌阅没上真机比过）
  - `docs/decisions.md`（修改：加一行 2026-11-10「四台漫画画质处理一致，掌阅也开这四项」·维护者定）
  - `CONTRIBUTING.md`（修改，仅当硬约束那段写着这几项「只 Kindle」时，改成四台；别的不动）

## 约束
- 不做：不改任何算法代码（`imgopt.rs`、`comiccrop.rs`、`optimize/`）；不改 kindle/kobo/hanvon 的 profile；不加新 profile 键；不加 COMIC_VERSION（开关本来就进指纹）；不改 install.sh
- 不改掌阅的 `comic_gamma`（已是 1.1）、`comic_margin`、版式
- 只在上面的分支工作；不合并；不改「涉及文件」以外的文件，确需改时写进「结果 → 偏离」
- 提交信息用中文，`feat:` 等前缀 + 正文，不加 Co-Authored-By；正文写明「没跑真书回归、没上真机；掌阅全部漫画会重建重传，文字书不变」
- 不读密钥、`.env`、`auth*`；clean-room：不读 GPL 代码（含 KCC 源码）
- 只对改动的 Rust 文件跑 rustfmt，不跑 `cargo fmt --all`；编译用 `CARGO_TARGET_DIR=~/.cache/sheng-ren-target`
- 风险点：设备行为（掌阅全部漫画产物变化）→ 合并前 agy 二审

## 分步任务
1. [ ] `ireader.toml`：在 `comic_margin = 1` 附近加 `comic_autocontrast = true`、`comic_trim_specks = true`、`comic_page_numbers = true`、`comic_dark_bg = true`，上面一行注释：「2026-11-10 维护者定四台漫画画质处理一致（同 kindle/kobo/hanvon）；掌阅上没真机比过」。自检：`cargo test -p profile` 通过
2. [ ] 指纹金样：`UPDATE_FP_GOLDEN=1 cargo test -p library --test fp_golden`，再 `git diff --stat` 和 `git diff crates/library/tests/fp_golden.txt`；应看到只有 `ireader\t…\tcomic\t` 的行变（漫画段末尾多出 `ASPK`），约 59 行改、0 行别的设备、0 行文字书
3. [ ] `crates/profile/src/lib.rs`：内置 profile 测试里补（或改）断言：kindle、ireader、kobo、hanvon 四台的 `comic.autocontrast`、`trim_specks`、`page_numbers`、`dark_bg` 都是 true，`comic.gamma == Some(1.1)`
4. [ ] 文档：`docs/devices.md` 四行、`docs/decisions.md` 一行（必要时 `CONTRIBUTING.md` 一句）
5. [ ] `cargo test --workspace` 全过；`cargo clippy --workspace --all-targets -- -D warnings` 0 警告

## 验收标准
- [ ] 功能：`cargo run -p bookconv --bin epub-optimize -- --device=ireader <合成漫画>.epub out.epub` 的产物里，灰度页做了拉满对比度（可用现有 kindle 的同类测试改成 ireader 跑一遍，或在结果里说明怎么看的）
- [ ] 测试：`cargo test --workspace` 全部通过；clippy 0 警告
- [ ] 不回归：`fp_golden.txt` 只有 ireader 漫画行变化；kindle/kobo/hanvon 行、全部文字书行 0 变化
- [ ] 「不做」的事确实没做（`git diff --stat` 只列「涉及文件」）

## 交付物
代码提交到上面的分支并推送；「结果」填好；状态改为 done 或 blocked。

## 给执行方的指令
```
执行任务单 .dev/tasks/2026-11-10-ireader-comic-quality.md：先把状态改为 accepted 并提交，按「分步任务」做完，填好「结果」，状态改为 done（做不下去改为 blocked 并写明原因），提交并推送分支。
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

## 审查
<!-- 负责 bot 填写 -->
- 验收：
- 二审：
- 合并：
