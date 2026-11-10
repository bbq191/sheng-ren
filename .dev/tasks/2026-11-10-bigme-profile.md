# 新增 Bigme B7 Pro 黑白版的 profile（bigme，EPUB）

- Task ID: 2026-11-10-bigme-profile
- 负责 bot: sheng-ren
- 状态: pending
- 执行方: opencode-local
- 分支: feat/bigme-profile（从 master `aa86ba4` 切出）
- 更新时间: 2026-11-10T16:30+08:00
- 引用: 无（照 `.dev/tasks/2026-11-04-hanvon-profile.md`、`2026-11-06-hanvon-device-facts.md` 的做法，这次真机量完一次写全）

## 需求原文
> 接到新设备，bigme b7pro 8+256g 黑白版，请将其加入 profile（vinci 2026-11-10）

## 输入
- 背景：vault `Projects/sheng-ren/context.md`（2026-11-04 / 11-06 汉王接入；2026-11-10 漫画 PRD A–D、Kobo 书架结论、掌阅退货）；仓库 `docs/devices.md`、`docs/decisions.md`
- 参照：`crates/profile/profiles/hanvon.toml`（同为安卓 + MTP + 自带阅读器 + EPUB，真机核对过的写法：每项注明来源 / 真机 / 推断）
- 模式 id：**`bigme`**（短、和品牌一致；以后换别的 Bigme 机型再看要不要带型号）。`name = "Bigme 自带阅读器（B7 Pro 黑白版）"`
- 公开资料（sheng-ren 2026-11-10 查，未上真机）：
  - 屏幕：7" E Ink **Carta 1300 黑白**，1264×1680，300 ppi，36 级冷暖前光（Root-Nation B7 Pro BW 评测 `root-nation.com/en/gadgets-en/e-readers-en/en-bigme-b7-pro-bw-review/`；Read with Pro 规格页 `readwithpro.com/devices/bigme-b7-pro-bw` 引官方页 1680×1264 / 300 ppi / 黑白；Android Blog Italia 评测 `androidblog.it/228612/recensione-bigme-b7-pro-bw/` 写明 BW 是 Carta 1300、彩色版才是 Kaleido 3）。**注意**：彩色版 B7 Pro 是 Kaleido 3，黑白 1264×1680 / 彩色 632×840（150 ppi），别混；vinci 的是黑白版
  - 系统：Android 14（Bigme OS 5.0），带 Google Play，可装第三方应用（同上；akiba.jpn.org 评测）
  - 自带阅读：「书库」默认列「内部存储 > Books」和「Download」里的文件，用自带的 xReader / xReader Pro 打开（note.com B751C 设置笔记；eWritable Bigme 固件 6.4、B6 评测）；支持 EPUB、EPUB3、PDF、AZW3、MOBI、CBZ、CBR、FB2、DOCX 等（Root-Nation、eWritable）；SD 卡上的文件要先在 xReader 打开一次才进书库（Reddit r/Bigme）
  - USB：USB-C（USB 2.0），安卓 MTP，接上后要在设备通知里选「文件传输」（通用安卓行为，World of eReader B6 Color 评测）
  - **未核实**：USB VID:PID、jmtpfs 挂载后的存储名、xReader 对侧载 EPUB 封面 / 书架框的处理、rtl 翻页方向、漫画整页是否铺满、注释弹窗
- 现状：内置 4 个模式 hanvon / ireader / kindle / kobo（ireader 另有任务单 `2026-11-10-profile-disable` 在本机停用，内置的仍在）；画质统一的键四台一致（`crates/profile/src/lib.rs` 的 `builtin_profiles_share_comic_quality`、`builtin_profiles_split_comic_spreads`、`builtin_profiles_take_comic_direction_from_metadata`）
- **真机测量（开工前由 sheng-ren 和 vinci 做，结果填在下面；没填完执行方不开工，直接改 blocked）**：
  - 测量包：box 上 `/workspace/bigme-kit/`（`/workspace/bigme-kit.tar`），内容见包里 `README.txt`
  - [ ] 用哪个阅读软件：自带 xReader / xReader Pro / 其它：（待填，缺省按自带 xReader）
  - [ ] USB：`lsusb` 行（VID:PID，开 / 不开 USB 调试各一）、jmtpfs 挂载点（dotfiles 加挂载规则，约定 `~/Bigme` → `/run/user/1000/mtp/bigme`）、存储名：（待填）
  - [ ] 投递目录：自带书库认哪个目录（`Books` / `Documents` / 别的），子目录里的书进不进书库：（待填）
  - [ ] 可阅读范围（测量书 `readable-probe` 截屏，`readable-measure` 量）：（待填）
  - [ ] 书架封面：框像素尺寸和比例、比例不对的封面拉伸 / 裁切 / 留白、取 meta cover / cover-image / 第一页、网格和列表是否一样（封面比例 0.50 / 0.75 / 1.00 + 封面来源探针）：（待填）
  - [ ] 漫画版式：同一本合成漫画流式整页 vs 固定版式，哪个铺满、连翻大小是否不变：（待填）
  - [ ] rtl：原书 `page-progression-direction="rtl"` 的漫画，点屏幕哪侧 / 按哪个实体键是下一页；跨页拆出的两半先右后左：（待填）
- 涉及文件：
  - `crates/profile/profiles/bigme.toml`（新增）
  - `crates/profile/src/lib.rs`（`builtin_profiles_parse` 的 id 列表加 `bigme`；三个「画质统一 / 拆跨页 / 按元数据定方向」测试的设备列表加 `bigme`；照 hanvon 补一段 bigme 断言：deliver、`(1264, 1680, false, Format::Epub)`、`comic.fixed_layout` 按测量；文件头模式列表补一句）
  - `crates/library/tests/fp_golden.rs`、`fp_golden.txt`（遍历加 `bigme`，重写金样）
  - `crates/library/tests/cli.rs`（`devices_lists_the_builtin_modes` 加 `("bigme", "EPUB")`）
  - `crates/library/src/bin/booklib/main.rs`（帮助里的设备表加一行，和现有行对齐）
  - `README.md`、`docs/devices.md`（设备表一行、字段表相关列 / 说明、「可阅读范围」、新增「Bigme B7 Pro 黑白版」一节）、`docs/usage.md`（「产物放在哪」加一行）

## 约束
- profile 值（照下面写，每项注释写来源：公开资料 / 2026-11-xx 真机 / 推断）：
  - `color = false`、`formats = ["epub"]`（只出 EPUB）
  - `[screen]` width = 1264、height = 1680（竖屏；公开资料）
  - 文字书照另几台的统一规则：`text_repair_only = true`、`kindle_rules = true`、`notes = "jump"`、`note_icons = "number"`、`caption_fit = true`、`background_images = true`、`image_gamma = 1.1`
  - 漫画画质和另几台一样（vinci：所有设备同一套画质处理）：`comic_gamma = 1.1`、`comic_autocontrast`、`comic_trim_specks`、`comic_page_numbers`、`comic_dark_bg` 都 `true`、`comic_margin = 1`、`comic_split_spreads = true`、`comic_meta_direction = true`；`comic_gutter_crop` 不写（缺省关，等 Kindle A/B）；`comic_page_direction` 不写（照原书）
  - 按「真机测量」写：`[readable.epub]`（量出的可阅读范围；整屏也写，同 hanvon）；漫画版式（流式和固定版式一样或流式更好 → 不开 `comic_fixed_layout`，同汉王；只有固定版式铺满才开）；书架（框固定且拉伸 / 裁切 → 写 `cover_thumb_aspect`（四舍五入三位），书架取 meta cover 时再写 `cover_meta_thumb = true`；跟着封面比例走 → 都不写，同 Kobo）；`[deliver]` `kind = "mtp"`、`mount = "bigme"`、`dir = <量出的目录，小写写法照汉王，安卓存储不分大小写>`
  - `em_px`、`line_px`、`content_px` 不写
- 不做：不出 KFX / KEPUB；不加 Bigme 专属的新 profile 键或新转换逻辑；不改 hanvon / ireader / kindle / kobo 的任何值；不开 `comic_gutter_crop`；不改 install.sh；不配 MTP 自动挂载（规则在 dotfiles）
- 只在上面的分支工作；不合并；不改「涉及文件」以外的文件，确需改时写进「结果 → 偏离」
- 提交信息用中文，`feat:` 等前缀 + 正文，不加 Co-Authored-By；正文写「没跑真书回归、新设备的产物没上真机看」
- 不读密钥、`.env`、`auth*`；clean-room：不读 GPL 代码
- 只对改动的 Rust 文件跑 rustfmt；编译用 `CARGO_TARGET_DIR=~/.cache/sheng-ren-target`
- 风险点：设备行为（新设备的投递路径；接上后第一次 sync 给它生成全部跟踪的书）→ 合并前 agy 二审。和 `feat/profile-disable` 都改 `crates/profile/src/lib.rs`、`crates/library/tests/cli.rs`：后合的那支先把 master 合进来再验收

## 分步任务
1. [ ] 核对「真机测量」七项都已填；没填完 → blocked
2. [ ] 新增 `bigme.toml`，按「约束」写值和注释；自检：`cargo test -p profile`
3. [ ] `lib.rs`：注册（`build.rs` 自动收 `profiles/*.toml`，只改断言）、id 列表、三个统一测试加 `bigme`、bigme 断言段；自检：`cargo test -p profile`
4. [ ] 指纹金样：`fp_golden.rs` 遍历加 `bigme`，`UPDATE_FP_GOLDEN=1 cargo test -p library --test fp_golden`；`git diff --numstat crates/library/tests/fp_golden.txt` 应**只有新增**（只多 `bigme` 开头的行，约 118 行，同 hanvon 当时），删除 0 行，别的设备 0 变化
5. [ ] `cli.rs`、`main.rs` 帮助设备表；自检：`cargo test --workspace`，`booklib devices` 列出 `bigme  … EPUB  屏幕 1264×1680  阅读范围 <量的值>  黑白`，`booklib sync --help` 设备表对齐
6. [ ] 合成测试书跑一遍：`cargo run --release -p bookconv --bin epub-optimize -- --device=bigme 书.epub 出.epub`（文字书一本、漫画一本），解开看漫画页尺寸按阅读范围、OPF 封面声明符合第 2 步（结果里贴摘要）；再用临时目录模拟 MTP（`BOOKLIB_MTP_DIR=…`、`BOOKLIB_MTP_NO_MOUNT_CHECK=1`）跑 `booklib sync --device=bigme`，产物落在 `<mount>/<存储名>/<dir>/<子目录>/`
7. [ ] 文档：README、devices.md、usage.md 加 bigme（标「YYYY-MM-DD 真机核对：…；正文字号 / 行高、注释弹窗、伽马待测」，照 hanvon 的写法）
8. [ ] `cargo clippy --workspace --all-targets -- -D warnings` 0 警告

## 验收标准
- [ ] 功能：`booklib devices` 认出 `bigme`；`--device=bigme` 产物为 EPUB，漫画按测量的阅读范围排；投递路径符合测量
- [ ] 画质：`bigme` 在三个统一测试里，和另几台同一套漫画处理（伽马 1.1、拉满对比度、去杂点、裁页码、黑底页、拆跨页、按元数据定方向；裁格间空白缺省关）
- [ ] 测试：`cargo test --workspace` 全部通过；clippy 0 警告
- [ ] 不回归：`fp_golden.txt` 只多 bigme 的行、删除 0 行；hanvon / ireader / kindle / kobo 的 profile 文件没动
- [ ] 「不做」的事确实没做
- [ ] 真机（vinci，合并后）：dotfiles 加好挂载规则后 `booklib sync --device=bigme` 传上去，自带书库里能看到、能打开；封面、漫画铺满、rtl 翻页符合测量

## 交付物
代码提交到上面的分支并推送；「结果」填好；状态改为 done 或 blocked。

## 给执行方的指令
```
执行任务单 .dev/tasks/2026-11-10-bigme-profile.md：先把状态改为 accepted 并提交，按「分步任务」做完，填好「结果」，状态改为 done（做不下去改为 blocked 并写明原因），提交并推送分支。
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
