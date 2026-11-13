# 重写 `booklib --help` 与各子命令帮助

- Task ID: 2026-11-13-help-rewrite
- 负责 bot: sheng-ren
- 状态: pending
- 执行方: 待定
- 分支: task/2026-11-13-help-rewrite（从已合并 track-listing 的最新 master 切出）
- 更新时间: 2026-11-13T17:50+08:00
- 引用: vinci 2026-11-13 在 sheng-ren 聊天里：「booklib --help 的帮助信息请重写，结构太混乱，说明不清晰」。**待 2026-11-13-track-listing 合并后再开跑**（track 的帮助已由那张单按「用法 → 参数 → 放法例子 → 重复 track 规则 → 例子」重写，本单只做风格统一，不要回退那张单的写法与语义）。

## 需求原文
> （vinci 2026-11-13，sheng-ren 聊天）`booklib --help` 的帮助信息请重写，结构太混乱，说明不清晰。范围：booklib 顶层 `--help` 与各子命令帮助（track 已由 track-listing 重写，只做风格统一）。不改任何命令行为和参数名。

## 输入
- **待 track-listing 合并后执行**：基于那时的 origin/master（须已含 track-listing 的合并提交），不要在 track-listing 之前的 master 上切分支，也不要把未合并的 track-listing 分支合进来凑合开工。
- 帮助文本都在 `crates/library/src/bin/booklib/main.rs`：`COMMANDS` 表（约 `:38` 起，每个命令的 `synopsis` / `brief` / `detail`）、`HELP_HEAD` / `HELP_TAIL`（约 `:213`）、拼装函数（约 `:243`：`synopsis`、顶层帮助、子命令帮助 `用法：\n{synopsis}\n\n{brief}{detail}`）。新增或调整段落只改这些给人看的字符串和必要的排版，不改 `opts` / `flags` / `check` / `lock` / `run`。
- 十个子命令：`sync`、`track`、`untrack`、`add`、`list`、`meta`、`remove`、`dedupe`、`devices`、`vertical`。`meta --edit --help` 若有单独的说明，一并按同一风格整理。

### 开单时盘点的结构与问题（执行时按合并后的文本再核对一遍，有出入以代码为准）

开单时（master `ff2bcc5`，track 帮助尚未重写）的观察：

1. **顶层 `booklib --help`**：`HELP_HEAD`（一句话介绍、用法、日常三条：`track` 登记、`sync`、`devices`）+ 各命令的 synopsis 和 brief + `HELP_TAIL`（通用选项、环境变量、退出码）。结构有头尾，但命令列表里 synopsis 长短悬殊（`sync` 一行堆满选项，`meta` 三种写法并排），brief 有的是一句话、有的把规则塞进去，扫一眼不容易抓住「日常用 sync」。
2. **子命令帮助内部不统一**：都由「用法 + brief + detail」拼出，但 detail 各写各的——`sync` 有「第一步 / 第二步 / 选项 / 例子」；`track` 是大段连写（将由 track-listing 重写）；`untrack`、`add`、`remove`、`dedupe`、`devices` 只有几句话，没有参数说明和例子；`list`、`meta`、`vertical` 有说明但没有统一的小标题。本单要把各子命令（含顶层）统一成：**用法 / 一句话说明 / 参数 / 例子**（没有参数或没有必要举例的命令可以省略对应节，但有的要用同样的小标题和顺序）。`track` 在 track-listing 之后已经接近这个结构，本单只把用词、小标题与其他命令对齐，不改那张单写准的语义和例子。
3. **用词与术语**：帮助里「模式」「设备」「阅读模式」混用；放法用语以 track-listing 为准（不要把已改掉的「从这一层起」当列表后缀再写回去）。术语与 `docs/usage.md`、`docs/devices.md` 现行说法一致（设备路径是 `~/Devices/<Name>`，链接由 dotfiles 管，booklib 只读）。
4. **已知过时**：`add` 的 detail 写「放设备 documents/ 顶层」（`main.rs:118` 附近）。这是 Kindle 的产物目录名；各设备实际是 Kindle `documents/`、掌阅/汉王 `Documents/`、Kobo `books/`、Bigme `Books/`，`docs/usage.md` 已写「跟踪目录顶层的书、`add` 进来的书、网址书放各设备产物目录顶层」。核实后改掉这条过时说明。执行时再搜一遍帮助文本里过时的说法（已删除的命令或设备、旧挂载路径、已并入 `sync` 但仍当现行功能写的，等），只改现行帮助，不要改 `docs/decisions.md` 里的历史记录。
5. **测试**：`crates/library/tests/cli.rs` 有帮助烟测（顶层含 `booklib [--library=目录] sync` 和 `devices`；`sync --help` 含 `--watch` 且不含 `add` / `untrack`；`meta --help` 含三种子用法且不含 ` sync `；`track -h` 含 `track [--device=` 和 `<目录或书>` 且不含 `untrack`；不认识的命令、`build` 已并入 sync、用错时只给该命令的写法）。重写后每个子命令 `--help`（以及 `-h`、`booklib help <命令>`）都能正常输出；烟测保持意图，必要时改断言字面，不要删。

## 约束
- 只用 Rust；clean-room；仓库不出现上游项目名及缩写。
- **不改任何命令行为、参数名、选项集合、默认值或错误文案**（用法错误提示除非与过时帮助直接矛盾，否则不要顺手改）。只改帮助文本及其排版。
- **不升版本号**；`fp_golden.txt` 一行不动。不影响产物。
- 不碰 `~/.local/share/booklib`，实验用 `target/` 下临时 `--library`；测试不碰真机。
- `install.sh` 不用 `--force`。本仓库不用 rustfmt 默认宽度，别对整个仓库跑 `cargo fmt`。
- 提交信息中文 + 前缀 + 正文，不加 Co-Authored-By。
- 不要回退 track-listing 已重写的 track 帮助语义；不要把本单范围扩到改命令实现。

## 规则（要实现的行为）
1. 顶层 `--help` 与各子命令 `--help` 用统一结构：用法 / 一句话说明 / 参数 / 例子。用词统一，术语与现行 docs 一致。
2. 修正核实过的过时说明（至少包括 `add` 的 `documents/`）；执行时搜出的同类问题一并改。
3. 帮助必须仍准确描述现有行为。拿不准的写法对照 `docs/usage.md` 和代码，不要 invent 新语义。
4. 每个子命令的 `--help`、`-h`、`booklib help <命令>` 都能正常输出（退出码 0）。

## 分步任务
1. [ ] 确认本单基于的 master 已含 track-listing 的合并；按合并后的文本复核上面的盘点，有出入在结果里写明。
2. [ ] 定统一的小节标题和用词（与 track-listing 的 track 帮助对齐），重写 `HELP_HEAD` / 各命令 brief 与 detail / `HELP_TAIL`；track 只做风格统一。
3. [ ] 核实并修正过时说明（`add` 的 `documents/` 等）。
4. [ ] 同步更新 `docs/` 里抄录或描述这些帮助的现行段落（若有）。
5. [ ] 确认每个子命令 `--help` 正常输出；更新帮助烟测断言（如需要）。`cargo test --workspace` 全过；`cargo clippy --workspace --all-targets -- -D warnings` 0 警告；确认版本号和 `fp_golden.txt` 都没动。

## 验收标准
- [ ] 顶层与各子命令帮助结构统一（用法 / 一句话说明 / 参数 / 例子），用词统一，术语与 docs 一致。
- [ ] track 帮助在 track-listing 重写之后只做了风格统一，语义未回退。
- [ ] 已知过时说明（至少 `add` 的 `documents/`）已核实并改正；没有把历史决定记录改写成现行行为。
- [ ] 未改变任何命令行为或参数名。
- [ ] 每个子命令 `--help`（及 `-h`、`booklib help <命令>`）正常输出；相关测试全过。
- [ ] 版本号不变；`fp_golden.txt` 一行没动。
- [ ] `cargo test --workspace` 全过；clippy `-D warnings` 0 警告。
- [ ] 结果里写明没跑真书回归、没上真机。

## 交付物
代码提交到任务分支并推送；「结果」填好；状态改为 done 或 blocked。任务单状态改动推 master，带 `[skip-notify]`。不要合并，不要跑二审。

## 给执行方的指令
```
执行任务单 .dev/tasks/2026-11-13-help-rewrite.md：须待 2026-11-13-track-listing 已合并进 master 后再做。先把状态改为 accepted 并提交，按「分步任务」做完，填好「结果」，状态改为 done（做不下去改为 blocked 并写明原因），提交并推送分支。任务单 done 推 master 带 [skip-notify]。不要合并，不要跑二审。不要回退 track-listing 的 track 帮助语义。
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
- 没跑真书回归、没上真机（只改帮助文字，不影响产物）。

## 审查
<!-- 负责 bot 填写 -->
- 验收：
- 二审：
- 合并：
