# 新增汉王 Clear 6 Turbo 的 profile（hanvon，EPUB）

- Task ID: 2026-11-04-hanvon-profile
- 负责 bot: sheng-ren
- 状态: pending
- 执行方: opencode-local
- 分支: feat/hanvon-profile（从最新 master 切出）
- 更新时间: 2026-11-04T10:45+08:00
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
1. [ ] 新增 `hanvon.toml`，按「约束」写值和注释；自检：`cargo test -p profile` 通过
2. [ ] `lib.rs` 注册 hanvon，更新内置 id 断言；自检：`cargo test -p profile`
3. [ ] 指纹金样、CLI 帮助、补全里的设备列表补 hanvon；自检：`cargo test --workspace`、`booklib --help` 设备表对齐
4. [ ] README、devices.md、usage.md 加一行，标「未上真机」
5. [ ] `cargo clippy --workspace --all-targets -- -D warnings` 0 警告

## 验收标准
- [ ] 功能：`booklib` 认出 `hanvon` 设备，`--device=hanvon` 可用，产物为 EPUB，尺寸按 1072×1448
- [ ] 测试：`cargo test --workspace` 全部通过；clippy 0 警告
- [ ] 不回归：ireader/kindle/kobo 的指纹金样不变（只多 hanvon 的行）
- [ ] 「不做」的事确实没做

## 交付物
代码提交到上面的分支并推送；「结果」填好；状态改为 done 或 blocked。

## 给执行方的指令
```
执行任务单 .dev/tasks/2026-11-04-hanvon-profile.md：先把状态改为 accepted 并提交，按「分步任务」做完，填好「结果」，状态改为 done（做不下去改为 blocked 并写明原因），提交并推送分支。
```

## 结果
<!-- 执行方填写 -->

## 审查
<!-- 负责 bot 填写 -->
