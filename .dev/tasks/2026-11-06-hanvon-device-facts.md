# 汉王 profile 写入真机量的数（阅读范围、书架封面比例、漫画版式）

- Task ID: 2026-11-06-hanvon-device-facts
- 负责 bot: sheng-ren
- 状态: done
- 执行方: opencode-local
- 分支: feat/hanvon-device-facts（从最新 master 切出）
- 更新时间: 2026-11-06T13:58+08:00
- 引用: .dev/tasks/2026-11-04-hanvon-profile.md

## 需求原文
> 汉王已接入……针对汉王做优化；此前 hanvon profile 任务单里还有真机待核对项（漫画固定版式、MTP 挂载/目录），可一并处理。

## 输入（2026-11-06 真机结论，sheng-ren 用 adb 截屏量的）
- 挂载：`~/Hanvon` → `/run/user/1000/mtp/hanvon`（jmtpfs），和 `mount = "hanvon"` 一致。USB 18d1:4ee2（开调试时）。
- 投递目录：存储根下实际是 `Documents`（安卓存储不分大小写，`dir = "documents"` 已落进去，vinci 侧载的书都在里面）；自带阅读器「文件」页从 `/Documents` 浏览，能打开。
- 测量书（readable-probe）：竖长黑块 362×1448、横宽黑块 1072×268，上下左右 0 → 可阅读范围 1072×1448 整屏。
- 书架封面：方形封面（1.0）被拉伸进约 281×373 的框（≈0.753），不裁不留白，和掌阅 0.754 同一行为。
- 漫画：同一本 10 页合成漫画，流式整页（E）和固定版式（D）在汉王上逐像素相同，都铺满整屏、连翻 4 页大小不变。→ 保持流式整页，不开固定版式。
- 书架只显示打开过的书；没有封面的书显示通用 EPUB 图标。
- 涉及文件：
  - `crates/profile/profiles/hanvon.toml`
  - `crates/library/tests/fp_golden.txt`（hanvon 开头的几行会变，只更新这些行）
  - `docs/devices.md`（hanvon 行和那一节）、`README.md`、`docs/usage.md`（去掉「未上真机」中已核对的部分）

## 约束
- 只改 hanvon；不改 ireader/kindle/kobo 的任何值、不加新 profile 键、不改转换逻辑、不改 install.sh
- hanvon.toml：
  - 加 `[readable.epub]` width = 1072、height = 1448，注释「2026-11-06 真机测量书量」
  - 加 `cover_thumb_aspect = 0.754`、`cover_meta_thumb = true`（同 ireader 的写法），注释「2026-11-06 书架框≈0.753，封面拉伸不裁；meta 指向缩略图同掌阅，待真机确认书架用的是缩略图」
  - 漫画版式注释改成「2026-11-06 真机比：流式整页与固定版式逐像素相同，保持流式」
  - `[deliver]` 的 mount、dir 注释改成「2026-11-06 真机确认」，值不动
  - 文件头「未上真机」改成「2026-11-06 上真机：阅读范围、封面比例、漫画版式、投递已核对；正文字号/行高、注释弹窗、伽马待测」
  - em_px、line_px 仍不写
- 只在上面的分支工作；不合并；不改「涉及文件」以外的文件，确需改时写进「结果 → 偏离」
- 提交信息用中文，`feat:`/`fix:` 等前缀 + 正文，不加 Co-Authored-By
- 不读密钥、`.env`、`auth*`；clean-room：不读 GPL 代码
- 只对改动的文件跑 rustfmt；编译用 `CARGO_TARGET_DIR=~/.cache/sheng-ren-target`
- 风险点：设备行为（指纹变 → hanvon 书全部重建，封面缩略图新增）→ 合并前要 agy 二审

## 分步任务
1. [x] 按「约束」改 hanvon.toml；自检 `cargo test -p profile`
2. [x] 更新 `fp_golden.txt` 里 hanvon 开头的那几行（其它行必须不变）；自检 `cargo test --workspace`
3. [x] 文档同步：devices.md 表格 hanvon 行（阅读范围「1072×1448（2026-11-06 量）」、漫画「流式，真机比过与固定版式相同」）、`cover_thumb_aspect`/`cover_meta_thumb` 两行的汉王列；README、usage.md 去掉已核对项的「未上真机」
4. [x] `cargo clippy --workspace --all-targets -- -D warnings` 0 警告

## 验收标准
- [x] `booklib sync --device=hanvon` 产物带 `eink-thumb.jpg`（0.754）、OPF meta cover 指向它
- [x] 指纹金样只有 hanvon 开头的行变
- [x] 测试全过、clippy 0 警告
- [x] 「不做」的事确实没做

## 交付物
代码提交到上面的分支并推送；「结果」填好；状态改为 done 或 blocked。

## 给执行方的指令
```
执行任务单 .dev/tasks/2026-11-06-hanvon-device-facts.md：先把状态改为 accepted 并提交，按「分步任务」做完，填好「结果」，状态改为 done（做不下去改为 blocked 并写明原因），提交并推送分支。
```

## 结果

- 已按「约束」改 `crates/profile/profiles/hanvon.toml`：
  - 加 `[readable.epub]` 1072×1448（2026-11-06 真机测量书：竖长黑块 362×1448、横宽黑块 1072×268，四边留白 0）。
  - 加 `cover_thumb_aspect = 0.754`、`cover_meta_thumb = true`：2026-11-06 书架框约 281×373（≈0.753），方形封面被拉伸、不裁不留白，同掌阅；meta 指向缩略图，书架实际用哪张待确认。
  - 漫画版式注释按真机结论改成「流式整页与固定版式逐像素相同，保持流式」；`[deliver]` 的 mount、dir 标为 2026-11-06 真机确认，值不变；文件头改成已核对阅读范围、封面、漫画版式、投递，`em_px`、`line_px` 仍未写。
- `crates/library/tests/fp_golden.txt` 用 `UPDATE_FP_GOLDEN=1` 重写，只有 hanvon 开头的 118 行变化（118 增 118 删），kindle/ireader/kobo 的行没动。
- 文档：`docs/devices.md` 的 hanvon 行改成阅读范围「1072×1448（2026-11-06 量）」、漫画「流式，真机比过与固定版式相同」；`cover_thumb_aspect`/`cover_meta_thumb` 两行补汉王结论，并在表下加一行汉王小注；「可阅读范围」补 hanvon 整屏实测；汉王一节改成 2026-11-06 真机核对，补传书目录、漫画和书架封面结论。`README.md`、`docs/usage.md` 去掉汉王已核对项的「未上真机」和待确认标记。
- 验收：用临时目录模拟 MTP 挂载（`BOOKLIB_MTP_DIR=/tmp/opencode/hanvon-check/mtp`、`BOOKLIB_MTP_NO_MOUNT_CHECK=1`）跑 `booklib sync --device=hanvon`，产物 `documents/hanvon-check.epub` 带 `OEBPS/eink-thumb.jpg`（400×531，比例 0.7533，按 0.754 裁），OPF 里 `<meta name="cover" content="eink-thumb"/>`、`<meta name="eink-thumbnail" content="eink-thumb"/>`，`cover-image` 和封面页仍是真封面。
- 自检：`cargo test -p profile`、`cargo test --workspace` 全部通过；`cargo clippy --workspace --all-targets -- -D warnings` 0 警告。
- 偏离：`crates/profile/src/lib.rs` 文件头的 hanvon 说明和一处断言提示还写着「未上真机 / 固定版式待真机比」，顺手改成 2026-11-06 的结论；只改注释和断言提示，无行为变化。
- 没跑真书回归、这次改动没在真机上跑过；真机结论来自任务单输入（sheng-ren 2026-11-06 用 adb 截屏量的）。书架是否真的用 `eink-thumb.jpg`、正文字号/行高、注释弹窗、伽马仍待确认。
