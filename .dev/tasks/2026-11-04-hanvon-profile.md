# 新增汉王 Clear 6 Turbo 的 profile（hanvon，EPUB）

- Task ID: 2026-11-04-hanvon-profile
- 负责 bot: sheng-ren
- 状态: accepted
- 执行方: opencode-local
- 分支: feat/hanvon-profile（从最新 master 切出）
- 更新时间: 2026-11-04T19:00+08:00
- 引用: 无

## 需求原文
> 下面增加汉王clear6turbo的profile
>
> 用哪个阅读软件看书？用汉王自带阅读器；设备到手了吗？设备还没到；先出什么格式？出 EPUB

## 输入
- 背景：vault `Projects/sheng-ren/context.md`；仓库 `docs/devices.md`、`docs/decisions.md`
- 参照：`crates/profile/profiles/kobo.toml`（未上真机的预配置写法：每项标来源 / 推断 / 待确认）、`crates/profile/profiles/ireader.toml`（同为安卓 + MTP + EPUB）
- 已知参数（公开资料，未上真机）：6 寸 E Ink Carta 1300，1448×1072，300 ppi，黑白，安卓 14，USB-C（来源：知乎评测 zhuanlan.zhihu.com/p/1916870323984393879、Lukas Lab、太平洋电脑网参数页）。注意 IT之家写的 1024×758 是 Clear 6，不是 Turbo
- 涉及文件：
  - `crates/profile/profiles/hanvon.toml`（新增）
  - `crates/profile/src/lib.rs`（BUILTIN 加 hanvon；`builtin_profiles_parse` 等断言内置 id 列表的测试更新）
  - `crates/library/tests/fp_golden.rs`、`fp_golden.txt`（若遍历全部内置设备，补 hanvon 并更新金样）
  - `crates/library/src/bin/booklib/main.rs`（帮助里设备表加一行，和现有行对齐）
  - `README.md`、`docs/devices.md`、`docs/usage.md`（设备表各加一行，标「未上真机」）

## 约束
- 不做：不出 KEPUB/KFX；不加汉王专属的新 profile 键或新转换逻辑；不改 ireader/kindle/kobo 的任何值；不改 install.sh；不配 MTP 自动挂载（挂载规则不在本仓库）
- profile 值：screen 1072×1448（竖屏）；color=false；formats=["epub"]；deliver kind="mtp"、mount="hanvon"、dir 照汉王存储实际情况待确认，先写 "documents" 并标（待确认）；文字书沿用三台统一规则（text_repair_only、kindle_rules）；漫画伽马、对比度、页码、黑底等沿用 kobo.toml 的取值；notes="jump"、note_icons="number"（推断，最稳）；em_px/line_px/content_px/cover_thumb_aspect/readable 不写（待真机量）。每一项写注释注明来源 / 推断 / 待确认
- 只在上面的分支工作；不合并；不改「涉及文件」以外的文件，确需改时写进「结果 → 偏离」
- 提交信息用中文，`feat:` 等前缀 + 正文，不加 Co-Authored-By
- 不读密钥、`.env`、`auth*`；clean-room：不读 GPL 代码
- 只对改动的文件跑 rustfmt，不跑 `cargo fmt --all`；编译用 `CARGO_TARGET_DIR=~/.cache/sheng-ren-target`
- 风险点：设备行为（新增设备的投递路径）→ 合并前要 agy 二审

## 分步任务
1. [x] 新增 `hanvon.toml`，按「约束」写值和注释；自检：`cargo test -p profile` 通过
2. [x] `lib.rs` 注册 hanvon，更新内置 id 断言；自检：`cargo test -p profile`
3. [x] 指纹金样、CLI 帮助、补全里的设备列表补 hanvon；自检：`cargo test --workspace`、`booklib --help` 设备表对齐
4. [x] README、devices.md、usage.md 加一行，标「未上真机」
5. [x] `cargo clippy --workspace --all-targets -- -D warnings` 0 警告

## 验收标准
- [x] 功能：`booklib` 认出 `hanvon` 设备，`--device=hanvon` 可用，产物为 EPUB，尺寸按 1072×1448
- [x] 测试：`cargo test --workspace` 全部通过；clippy 0 警告
- [x] 不回归：ireader/kindle/kobo 的指纹金样不变（只多 hanvon 的行）
- [x] 「不做」的事确实没做

## 交付物
代码提交到上面的分支并推送；「结果」填好；状态改为 done 或 blocked。

## 给执行方的指令
```
执行任务单 .dev/tasks/2026-11-04-hanvon-profile.md：先把状态改为 accepted 并提交，按「分步任务」做完，填好「结果」，状态改为 done（做不下去改为 blocked 并写明原因），提交并推送分支。
```

## 结果

- 新增 `crates/profile/profiles/hanvon.toml`：汉王 Clear 6 Turbo 自带阅读器，屏幕 1072×1448、黑白、只出 EPUB。屏幕、安卓 14 等来自公开资料；注释跳转、图标换数字、漫画参数标了「推断 / 待确认」；`em_px`、`line_px`、`content_px`、`cover_thumb_aspect`、`[readable.epub]` 按任务要求没写。
- 内置注册：`builtin_profiles_parse` 的 id 列表加 `hanvon`；`lib.rs` 顶部说明改成四份。补全的设备列表从 `Registry` 动态取，不用另改。
- 指纹金样：`fp_golden.rs` 的遍历加 `hanvon`，`fp_golden.txt` 只多出 118 行 hanvon；kindle/ireader/kobo 原有行未变。
- CLI：`booklib sync --help` 的设备表加 `hanvon` 一行，和现有行对齐；`booklib --help` 开头也补了汉王。
- 文档：`README.md`、`docs/devices.md`（设备表加一行，另补一节说明）、`docs/usage.md`（「产物放在哪」加一行）都标了「预配置，未上真机」。

验证（本机，`CARGO_TARGET_DIR=~/.cache/sheng-ren-target`）：
- `cargo test -p profile` 通过。
- `cargo test -p library --test fp_golden` 通过。
- `cargo test --workspace` 全部通过。
- `cargo clippy --workspace --all-targets -- -D warnings` 0 警告。
- `booklib sync --help` 设备表逐行看过，列对齐。

未验证 / 风险：
- **没上真机，也没跑真书回归**（本机没有维护者的真书）。汉王自带阅读器上的显示效果、注释跳转、漫画参数、书架缩略图比例都没验证。
- `[deliver]` 的 `mount = "hanvon"`、`dir = "documents"` 是推断（按掌阅惯例和任务约束），实际存储目录和 jmtpfs 挂载点名待真机确认。设备行为风险，合并前需要 agy 二审。
- 没做：KEPUB/KFX、汉王专属 profile 键或转换逻辑、MTP 自动挂载、改 `install.sh`；ireader/kindle/kobo 的 profile 值和原有指纹金样都没动。

偏离：
- `docs/devices.md` 除了设备表一行，还照 Kobo 的写法补了一节「汉王 Clear 6 Turbo」（来源、传书、格式、漫画、未量项），仍在该文件内。
- `crates/profile/src/lib.rs` 顶部说明和一条枚举注释里的「三份 / 三个模式」改成四份 / 内置模式；`crates/library/src/bin/booklib/main.rs` 的 `--help` 开头补了汉王。都在涉及文件内。

## 审查
<!-- 负责 bot 填写 -->
- 验收：**通过**（sheng-ren，2026-11-04，box 上 a184cd8 重跑）
  - 功能：`hanvon.toml` 屏幕 1072×1448、formats=["epub"]、deliver mtp/hanvon/documents；`builtin_profiles_parse` 断言 4 个内置 id
  - 测试：`cargo test --workspace` 全过（profile 515、library 63 等，0 失败）；`cargo clippy --workspace --all-targets -- -D warnings` exit 0
  - 不回归：`fp_golden.txt` 只有新增行，0 行删除；ireader/kindle/kobo 的 profile 文件未改
  - 「不做」：未动 install.sh、未加新键、未出 KEPUB/KFX
  - 提交合规：中文前缀加正文，无 Co-Authored-By，未合并
  - 偏离两处（devices.md 补一节、lib.rs/main.rs 说明文字）都在涉及文件内，接受
- 二审：回执 `.dev/tasks/2026-11-04-hanvon-profile.agy.md`，无阻断。sheng-ren 逐条核对：
  - 建议 1（测试没覆盖验收标准）：属实，`cli.rs:254` 只验 ireader、kindle；`lib.rs` 有 kobo 属性断言、没有 hanvon 的。**采纳**
  - 建议 2（notes 注释写「同 kobo/ireader」）：属实，`ireader.toml:13` 是 popup。**采纳**
  - 线索 1（comic_fixed_layout 在安卓 EPUB 阅读器上可能被状态栏裁切）：属实，`ireader.toml` 没开固定版式，同是安卓 EPUB 阅读器，按掌阅的做法更稳。**采纳**：先关掉，等真机再比
  - 线索 2（挂载名、目录）：等真机，dotfiles 届时加挂载规则；已记在 context 待办。不改
- **返工（状态退回 pending，同一执行方 opencode-local）**：
  1. `crates/profile/profiles/hanvon.toml`：删掉 `comic_fixed_layout = true` 和 `[comic_readable]` 一节（照 ireader，漫画走流式整页），注释写明「同 ireader；固定版式待真机比」；`notes` 的注释改成「推断：同 kobo（ireader 是 popup，汉王弹窗待真机测）」
  2. `crates/profile/src/lib.rs`：照 kobo 的断言给 hanvon 补一段，断言 deliver 为 Mtp{mount:"hanvon",dir:"documents"}，以及 (1072,1448,false,Format::Epub)，comic_fixed_layout 为 false
  3. `crates/library/tests/cli.rs` 的 `devices_lists_the_builtin_modes`：列表加 ("kobo","KEPUB")、("hanvon","EPUB")（按 `booklib devices` 的实际输出核对写法）
  4. 指纹金样随之更新（只允许 hanvon 的行变化）；`cargo test --workspace`、clippy 0 警告
  5. 填写「结果」的返工部分，状态改为 done，提交推送
