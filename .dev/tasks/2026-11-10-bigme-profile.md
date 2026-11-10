# 新增 Bigme B7 Pro 黑白版的 profile（bigme，EPUB）

- Task ID: 2026-11-10-bigme-profile
- 负责 bot: sheng-ren
- 状态: done
- 执行方: opencode-local
- 分支: feat/bigme-profile（从 master `aa86ba4` 切出；测量填完后把已含停用阅读模式的 master `dc1eafe` 合进来）
- 更新时间: 2026-11-10T18:28+08:00
- 引用: 无（照 `.dev/tasks/2026-11-04-hanvon-profile.md`、`2026-11-06-hanvon-device-facts.md` 的做法，这次真机量完一次写全）

## 需求原文
> 接到新设备，bigme b7pro 8+256g 黑白版，请将其加入 profile（vinci 2026-11-10）

## 输入
- 背景：vault `Projects/sheng-ren/context.md`（2026-11-04 / 11-06 汉王接入；2026-11-10 漫画 PRD A–D、Kobo 书架结论、掌阅退货）；仓库 `docs/devices.md`、`docs/decisions.md`
- 参照：`crates/profile/profiles/hanvon.toml`（同为安卓 + MTP + 自带阅读器 + EPUB，真机核对过的写法：每项注明来源 / 真机 / 推断）；漫画固定版式照 `kobo.toml` 的方案 D
- 模式 id：**`bigme`**（短、和品牌一致；以后换别的 Bigme 机型再看要不要带型号）。`name = "Bigme 自带阅读器（B7 Pro 黑白版）"`
- 公开资料（sheng-ren 2026-11-10 查；屏幕分辨率、Android 14 已由真机 `wm size` / `getprop` 核对）：
  - 屏幕：7" E Ink **Carta 1300 黑白**，1264×1680，300 ppi，36 级冷暖前光（Root-Nation B7 Pro BW 评测 `root-nation.com/en/gadgets-en/e-readers-en/en-bigme-b7-pro-bw-review/`；Read with Pro 规格页 `readwithpro.com/devices/bigme-b7-pro-bw` 引官方页 1680×1264 / 300 ppi / 黑白；Android Blog Italia 评测 `androidblog.it/228612/recensione-bigme-b7-pro-bw/` 写明 BW 是 Carta 1300、彩色版才是 Kaleido 3）。**注意**：彩色版 B7 Pro 是 Kaleido 3，黑白 1264×1680 / 彩色 632×840（150 ppi），别混；vinci 的是黑白版
  - 系统：Android 14（Bigme OS 5.0），带 Google Play，可装第三方应用（同上；akiba.jpn.org 评测）
  - 自带阅读：「书库」默认列「内部存储 > Books」和「Download」里的文件，用自带的 xReader / xReader Pro 打开（note.com B751C 设置笔记；eWritable Bigme 固件 6.4、B6 评测）；支持 EPUB、EPUB3、PDF、AZW3、MOBI、CBZ、CBR、FB2、DOCX 等（Root-Nation、eWritable）；SD 卡上的文件要先在 xReader 打开一次才进书库（Reddit r/Bigme）
  - USB：USB-C（USB 2.0），安卓 MTP，接上后要在设备通知里选「文件传输」（通用安卓行为，World of eReader B6 Color 评测）
- 现状：内置 4 个模式 hanvon / ireader / kindle / kobo（ireader 的停用任务 `2026-11-10-profile-disable` 已合进 master `dc1eafe`，书库 toml 可停用内置模式；内置的 ireader 仍在程序里）；画质统一的键现在四台一致（`crates/profile/src/lib.rs` 的 `builtin_profiles_share_comic_quality`、`builtin_profiles_split_comic_spreads`、`builtin_profiles_take_comic_direction_from_metadata`），本任务给这三个测试加上 `bigme`
- **真机测量**（2026-11-10，sheng-ren 在 vinci 的 alienware 上用 adb 截屏量；阅读软件为自带 xReader，默认页边距。七项已填完，执行方可开工）：
  - 测量包：box 上 `/workspace/bigme-kit/`（`/workspace/bigme-kit.tar`），内容见包里 `README.txt`。固定版式那本按 Kobo 方案 D 生成（`comic_fixed_layout` + `comic_fixed_layout_percent`，画布 1264×1680）
  - [x] 用哪个阅读软件：自带 **xReader**（设备上还有 `koreader` 目录，本次按 xReader 量；若以后主要用 KOReader 再另开任务）。型号 `ro.product.model = HiBreak`，Android 14，`wm size` 1264×1680，密度 300，与公开资料一致
  - [x] USB：MTP `0e8d:2008`（MediaTek Inc. MT65xx/67xx MTP mode；`lsusb` 显示 MediaTek Inc. Cyrus Technology CS 24）。adb 序列号 `B7BPR0L2GGDA007000052`（开了 USB 调试才能 adb）。jmtpfs 真机挂在 `~/Devices/Bigme`（即 `/home/afu/Devices/Bigme`），存储名「内部共享存储空间」。booklib 的 `[deliver].mount = "bigme"` 仍照汉王写法，缺省认 `$XDG_RUNTIME_DIR/mtp/bigme`（`/run/user/1000/mtp/bigme`）；自动挂载规则在 dotfiles（本仓库不配）。现在实际挂载点和约定不一致，合并且配好规则之前，可用 `BOOKLIB_MTP_DIR=/home/afu/Devices` 让 `…/Bigme` 对上 `mount = "bigme"`，或把 `~/Devices/Bigme` 挂/链到 `/run/user/1000/mtp/bigme`
  - [x] 投递目录：xReader 书架认存储根下的 **`Books/`**（测试包放进 `Books/测试/` 后出现在书架，子目录里的书进书库）。安卓存储不分大小写，`dir = "books"`（小写写法照汉王的 `documents`）。完整路径：`~/Devices/Bigme/内部共享存储空间/Books/`
  - [x] 可阅读范围（流式 EPUB，xReader 默认页边距）：**1069×1424**。竖长黑块 357×1424（上留 96、下留 160，含页脚）；横宽黑块 1069×268，正好 4:1（左 96、右 99）。`readable-measure` 曾报 1145×952，是把左侧笔工具栏边框算进了黑块，已避开工具栏手工校正。→ `[readable.epub]` width=1069 height=1424。改了页边距要重测
  - [x] 书架封面：网格框实测 240×296 像素（宽高比 ≈0.81）。封面保持原比例，按框宽缩放、顶部对齐，不拉伸（圆还是圆）：1.00 和 0.75 的封面比框矮，下面留白；0.50 的比框高，上下被裁，刻度少了一截。封面来源探针显示 **C**＝正文第一页，xReader 不认 cover-image 和 meta cover。因此汉王那种 `cover_meta_thumb` / eink-thumb 在 Bigme 上不管用；**不写** `cover_thumb_aspect`、`cover_meta_thumb`。要让封面铺满格子，得让正文第一页的封面图本身做成 0.81，这会改到书里的封面页——**待定，本次不做**（不在范围）。本次量的是书架网格，列表未单独截
  - [x] 漫画版式：两本都按 1264×1680 整屏生成、spine rtl。流式缩进页边距框，标记范围 1001×1420，四边刻度都在，不裁不拉伸。固定版式（方案 D）不受页边距限制，只给页脚留空，标记范围 1105×1532（大约大 10%），四边刻度都在，不裁不拉伸。**决定（sheng-ren 建议，vinci 可改）：漫画用固定版式（方案 D）**，因为画面更大。→ `comic_fixed_layout = true`、`comic_fixed_layout_percent = true`；`[comic_readable]` width=1264 height=1680（与试测包画布一致；设备把 1264×1680 的页缩到约 1105×1532，给页脚留空）
  - [x] rtl：流式、固定版式都没认 spine 里的 rtl——点左侧无反应，点右侧才翻到下一页。日漫要在 Bigme 上点左侧翻下一页，只能去 xReader 自己的设置里改翻页区域或方向，归 eink-settings，booklib 改不了。booklib 仍和其他设备一样按元数据写 rtl（`comic_meta_direction = true`，不写 `comic_page_direction`）。跨页拆页顺序正确：先 4R（右半，先读）再 5L（左半，后读）；xReader 把封面也算一页（4R 显示 5/11）
- 涉及文件：
  - `crates/profile/profiles/bigme.toml`（新增）
  - `crates/profile/src/lib.rs`（`builtin_profiles_parse` 的 id 列表加 `bigme`；三个「画质统一 / 拆跨页 / 按元数据定方向」测试的设备列表加 `bigme`；照 hanvon 补一段 bigme 断言：deliver、`(1264, 1680, false, Format::Epub)`、`output_readable` 1069×1424、`comic.fixed_layout` / `fixed_layout_percent` 均为 true、不写封面缩略图；文件头模式列表补一句）
  - `crates/library/tests/fp_golden.rs`、`fp_golden.txt`（遍历加 `bigme`，重写金样）
  - `crates/library/tests/cli.rs`（`devices_lists_the_builtin_modes` 加 `("bigme", "EPUB")`）
  - `crates/library/src/bin/booklib/main.rs`（帮助里的设备表加一行，和现有行对齐）
  - `README.md`、`docs/devices.md`（设备表一行、字段表相关列 / 说明、「可阅读范围」、新增「Bigme B7 Pro 黑白版」一节）、`docs/usage.md`（「产物放在哪」加一行）

## 约束
- profile 值（照下面写，每项注释写来源：公开资料 / 2026-11-10 真机 / 推断）：
  - `color = false`、`formats = ["epub"]`（只出 EPUB）
  - `[screen]` width = 1264、height = 1680（竖屏；公开资料，2026-11-10 真机 `wm size` 核对）
  - 文字书照另几台的统一规则：`text_repair_only = true`、`kindle_rules = true`、`notes = "jump"`、`note_icons = "number"`、`caption_fit = true`、`background_images = true`、`image_gamma = 1.1`
  - 漫画画质和另几台一样（vinci：所有设备同一套画质处理）：`comic_gamma = 1.1`、`comic_autocontrast`、`comic_trim_specks`、`comic_page_numbers`、`comic_dark_bg` 都 `true`、`comic_margin = 1`、`comic_split_spreads = true`、`comic_meta_direction = true`；`comic_gutter_crop` 不写（缺省关，等 Kindle A/B）；`comic_page_direction` 不写（照原书）
  - 按 2026-11-10 真机测量写（注释写日期和条件：xReader、默认页边距）：
    - `[readable.epub]` width=1069 height=1424（流式文字书；注释写明四边留白：左 96、右 99、上 96、下 160 含页脚，以及 `readable-measure` 把左侧笔工具栏算进去的误报已手工校正）
    - `[comic_readable]` width=1264 height=1680（漫画固定版式画布，整屏；注释写真机上标记范围 1105×1532，只给页脚留空，不裁不拉伸）
    - `comic_fixed_layout = true`、`comic_fixed_layout_percent = true`（Kobo 方案 D；真机比流式大约大 10%，选用固定版式）
    - `cover_thumb_aspect`、`cover_meta_thumb` 都不写（探针 C＝正文第一页，eink-thumb 不管用；第一页封面是否做成 0.81 待定，本次不做。注释写明书架框 240×296、保持比例、顶对齐、0.50 上下裁、0.75/1.00 下留白）
    - `[deliver]` `kind = "mtp"`、`mount = "bigme"`、`dir = "books"`（注释写：缺省挂载点 `$XDG_RUNTIME_DIR/mtp/bigme`；2026-11-10 真机 jmtpfs 挂在 `~/Devices/Bigme`，存储名「内部共享存储空间」，产物目录 `Books/`，安卓存储不分大小写）
  - `em_px`、`line_px`、`content_px` 不写
- 不做：不出 KFX / KEPUB；不加 Bigme 专属的新 profile 键或新转换逻辑；不改 hanvon / ireader / kindle / kobo 的任何值；不开 `comic_gutter_crop`；不写 `cover_thumb_aspect` / `cover_meta_thumb`；不把正文第一页封面改成 0.81（待定）；不改 xReader 翻页方向（归 eink-settings）；不改 install.sh；不配 MTP 自动挂载（规则在 dotfiles）
- 只在上面的分支工作；不合并进 master；不改「涉及文件」以外的文件，确需改时写进「结果 → 偏离」
- 提交信息用中文，`feat:` 等前缀 + 正文，不加 Co-Authored-By；正文写「没跑真书回归、新设备的产物没上真机看」
- 不读密钥、`.env`、`auth*`；clean-room：不读 GPL 代码
- 只对改动的 Rust 文件跑 rustfmt；编译用 `CARGO_TARGET_DIR=~/.cache/sheng-ren-target`
- 风险点：设备行为（新设备的投递路径；接上后第一次 sync 给它生成全部跟踪的书）→ 合并前 agy 二审。master 已含 `feat/profile-disable`（改了 `crates/profile/src/lib.rs`、`crates/library/tests/cli.rs`），本分支验收前须先把 master 合进来（sheng-ren 在填写测量后合）

## 分步任务
1. [ ] 核对「真机测量」七项都已填（2026-11-10 已填完）；若发现没填完 → blocked
2. [ ] 新增 `bigme.toml`，按「约束」写值和注释（可读范围 1069×1424、漫画方案 D、`[comic_readable]` 1264×1680、`dir = "books"`、不写封面缩略图）；自检：`cargo test -p profile`
3. [ ] `lib.rs`：注册（`build.rs` 自动收 `profiles/*.toml`，只改断言）、id 列表加 `bigme`（按现有字母序应是 `bigme, hanvon, ireader, kindle, kobo`）、三个统一测试加 `bigme`、补 bigme 断言段：`Deliver::Mtp { mount: "bigme", dir: "books" }`、`(1264, 1680, false, Format::Epub)`、`output_readable` 为 1069×1424、`comic.fixed_layout` 与 `fixed_layout_percent` 均为 true、`cover_thumb_aspect` 为 `None`；文件头模式列表补一句（2026-11-10 真机核对阅读范围、书架、漫画版式、投递）；自检：`cargo test -p profile`
4. [ ] 指纹金样：`fp_golden.rs` 遍历加 `bigme`，`UPDATE_FP_GOLDEN=1 cargo test -p library --test fp_golden`；`git diff --numstat crates/library/tests/fp_golden.txt` 应**只有新增**（只多 `bigme` 开头的行，约 118 行，同 hanvon 当时），删除 0 行，别的设备 0 变化
5. [ ] `cli.rs`、`main.rs` 帮助设备表；自检：`cargo test --workspace`，`booklib devices` 列出 `bigme  … EPUB  屏幕 1264×1680  阅读范围 1069×1424  黑白`，`booklib sync --help` 设备表对齐
6. [ ] 合成测试书跑一遍：`cargo run --release -p bookconv --bin epub-optimize -- --device=bigme 书.epub 出.epub`（文字书一本、漫画一本），解开看：文字书按 `[readable.epub]` 1069×1424；漫画页尺寸按 `[comic_readable]` 1264×1680，OPF 为固定版式方案 D（百分比例、viewport 1264×1680）；**没有** `eink-thumb`、也不改书里封面页比例。结果里贴摘要。再用临时目录模拟 MTP（`BOOKLIB_MTP_DIR=…`、`BOOKLIB_MTP_NO_MOUNT_CHECK=1`）跑 `booklib sync --device=bigme`，产物落在 `<mount>/<存储名>/books/<子目录>/`（测试里存储名可自建；dir 是 `books`）
7. [ ] 文档：README、devices.md、usage.md 加 bigme。标「2026-11-10 真机核对：阅读范围 1069×1424（xReader 默认页边距）、书架框 240×296 且不写缩略图（探针 C＝第一页）、漫画固定版式方案 D、投递 `Books/`；正文字号 / 行高、注释弹窗、伽马待测」。devices.md / usage.md 写明缺省挂载点 `/run/user/1000/mtp/bigme`，以及 2026-11-10 真机实际挂在 `~/Devices/Bigme`、可用 `BOOKLIB_MTP_DIR=/home/afu/Devices` 或等 dotfiles 加规则。rtl 翻页方向归 xReader 设置（eink-settings）；第一页封面做成 0.81 待定。照 hanvon 的写法
8. [ ] `cargo clippy --workspace --all-targets -- -D warnings` 0 警告

## 验收标准
- [ ] 功能：`booklib devices` 认出 `bigme`；`--device=bigme` 产物为 EPUB；文字书按阅读范围 1069×1424 排；漫画为固定版式方案 D、画布 1264×1680；投递路径 `mount = "bigme"`、`dir = "books"`
- [ ] 画质：`bigme` 在三个统一测试里，和另几台同一套漫画处理（伽马 1.1、拉满对比度、去杂点、裁页码、黑底页、拆跨页、按元数据定方向；裁格间空白缺省关）；另开固定版式方案 D；不写 `cover_thumb_aspect` / `cover_meta_thumb`
- [ ] 测试：`cargo test --workspace` 全部通过；clippy 0 警告
- [ ] 不回归：`fp_golden.txt` 只多 bigme 的行、删除 0 行；hanvon / ireader / kindle / kobo 的 profile 文件没动
- [ ] 「不做」的事确实没做
- [ ] 真机（vinci，合并后）：dotfiles 加好挂载规则（或暂用 `BOOKLIB_MTP_DIR=/home/afu/Devices`）后 `booklib sync --device=bigme` 传上去，xReader 书库里能看到、能打开；封面（取正文第一页）、漫画固定版式铺满程度、拆页顺序符合测量。点左侧翻下一页要在 xReader 设置里改，不在这次验收。第一页封面做成 0.81 待定，也不在这次

## 交付物
代码提交到上面的分支并推送；「结果」填好；状态改为 done 或 blocked。

## 给执行方的指令
```
执行任务单 .dev/tasks/2026-11-10-bigme-profile.md：先把状态改为 accepted 并提交，按「分步任务」做完，填好「结果」，状态改为 done（做不下去改为 blocked 并写明原因），提交并推送分支。
```

## 结果
<!-- 执行方填写 -->
- 执行方 / 开始–结束时间：opencode-local / 2026-11-10 18:15–18:21 +08:00
- base..head：`aa86ba4..HEAD`（`dc1eafe` 已合进来；代码到 `70ba776`，另有任务单 accepting / 本结果提交）
- 提交：

  | 步骤 | 提交号 | 文件 | 说明 |
  | --- | --- | --- | --- |
  | 接受 | `288f73a` | `.dev/tasks/2026-11-10-bigme-profile.md` | 状态改 accepted |
  | 2、3 | `1985144` | `crates/profile/profiles/bigme.toml`、`crates/profile/src/lib.rs` | 新 profile；id 列表、三个统一测试加 bigme；bigme 断言 |
  | 4、5 | `82c51d2` | `crates/library/tests/fp_golden.rs`、`fp_golden.txt`、`crates/library/tests/cli.rs` | 金样遍历加 bigme；`devices` 用例加 `("bigme", "EPUB")` |
  | 5、7 | `70ba776` | `README.md`、`crates/library/src/bin/booklib/main.rs`、`docs/devices.md`、`docs/usage.md` | 设备表、帮助设备表、字段说明、设备一节、可阅读范围、产物位置 |
  | 结果 | 本提交 | `.dev/tasks/2026-11-10-bigme-profile.md` | 填结果、状态 done |

- 测试：

  | 命令 | 通过 / 失败 / 未跑 | 摘要 |
  | --- | --- | --- |
  | `cargo test -p profile` | 通过 | 17 passed；`builtin_profiles_parse` 认 `bigme`，三个统一测试（画质 / 拆跨页 / 按元数据定方向）都含 bigme |
  | `UPDATE_FP_GOLDEN=1 cargo test -p library --test fp_golden` | 通过 | `git diff --numstat` = `124 0`：只新增 bigme 行、删除 0 行，别的设备指纹不变 |
  | `cargo test -p library --test fp_golden` | 通过 | 重写后复跑通过 |
  | `cargo test --workspace` | 通过 | 789 passed，0 failed |
  | `cargo clippy --workspace --all-targets -- -D warnings` | 通过 | 0 警告 |
  | `cargo fmt --all -- --check` | 通过 | 干净 |
  | `booklib devices`（临时书库 + 假 MTP） | 通过 | `bigme  Bigme 自带阅读器（B7 Pro 黑白版）  EPUB  屏幕 1264×1680  阅读范围 1069×1424  黑白`，并显示接上假 MTP |
  | `booklib sync --help` | 通过 | 设备表新增 `bigme  Bigme（USB）  EPUB  存储根目录 books/<子目录>/`，和各行列对齐 |
  | 合成书跑 `epub-optimize --device=bigme` | 通过 | 见下面「合成书 / 假 MTP」 |
  | 假 MTP 跑 `booklib sync --device=bigme` | 通过 | 产物落到 `<mount>/bigme/内部共享存储空间/books/子目录/测试文字书.epub` |

- 合成书 / 假 MTP（`/tmp/opencode/bigme-test/`，不提交）：
  - 文字书一本（一张 2000×1500 插图 + 正文）：`epub-optimize v66` 正常；profile 的 `[readable.epub]` 是 1069×1424（`--print-profile` 和 `booklib devices` 都核对到）。文字书开 `text_repair_only`，按规则正文图片、样式不动，所以产物里插图没有缩放——这是只修复路径的预期，不是按 1069 宽重排。
  - 漫画一本（3 张整页图、OPF `dc:subject` 漫画）：产物 3 张图都是 **1264×1680** 灰度；OPF `rendition:layout = pre-paginated`、`fixed-layout = true`、`original-resolution = 1264x1680`、`book-type = comic`；每页 `<meta name="viewport" content="width=1264, height=1680">`、`<img style="width:100%;height:100%">`（方案 D）；**没有** `eink-thumb`，也没有为封面单独改书里的封面页比例。
  - 假 MTP：`BOOKLIB_MTP_DIR=…/mtp BOOKLIB_MTP_NO_MOUNT_CHECK=1 booklib track …; booklib sync --device=bigme`，产物落到 `<mount>/bigme/内部共享存储空间/books/子目录/`（`dir = "books"`、存储名自建）。

- 遇到的问题：
  - 文字书的「按 1069×1424」只能在 profile / `booklib devices` 层核对：`text_repair_only` 下正文图片和样式一概不动，不会按阅读范围缩放（和 README 的「文字书只修复」一致）。漫画的 1264×1680 和固定版式已在产物里逐项核对。
- 偏离：无（只改「涉及文件」里的文件）；`lib.rs` 的 `dir_disables_builtin_mode` 顺手把 bigme 加进「其余内置模式都在」的断言，只加强测试、不改行为。
- 未完成：
  - 真机验证（vinci，合并后）：dotfiles 配好挂载规则（或暂用 `BOOKLIB_MTP_DIR=/home/afu/Devices`）后 `booklib sync --device=bigme` 传上去，xReader 书库能看到、能打开；看封面（正文第一页）、漫画固定版式铺满程度、拆页顺序。点左侧翻下一页要在 xReader 设置里改，第一页封面做成 0.81 待定，都不在本单。

## 审查
<!-- 负责 bot 填写 -->
- 验收：**通过**（sheng-ren，2026-11-10 18:28 +08:00，box 上 `692aef7` 重跑；范围 `dc1eafe..692aef7`，`dc1eafe`＝`origin/master` 合流点）
  - 范围：`git diff --stat dc1eafe..692aef7` 只有任务单、`crates/profile/profiles/bigme.toml`（新增）、`crates/profile/src/lib.rs`、`crates/library/tests/fp_golden.rs`、`fp_golden.txt`（124 行新增、0 行删除，前缀全是 `bigme`）、`crates/library/tests/cli.rs`、`crates/library/src/bin/booklib/main.rs`、`README.md`、`docs/devices.md`、`docs/usage.md`。hanvon / ireader / kindle / kobo 的 profile 文件、`install.sh` 都没动。和「涉及文件」一致。
  - 分步任务 1–8：真机七项已填；`bigme.toml` 值与注释按约束写完（`[readable.epub]` 1069×1424，`[comic_readable]` 1264×1680，方案 D，`dir = "books"`，不写封面缩略图和 `comic_gutter_crop` / `comic_page_direction`）；`lib.rs` 注册、字母序 id 列表、三个统一测试和 bigme 断言都在；金样、CLI / 帮助、文档、clippy 均已做。仓库里没有 `.dev/exec-log.md`，以本单「结果」为执行记录，命令已在 box 上重跑，对得上。
  - 功能 / 画质（对照验收标准和真机值）：
    - `booklib devices`（临时 `BOOKLIB_DIR`）：`bigme  Bigme 自带阅读器（B7 Pro 黑白版）  EPUB  屏幕 1264×1680  阅读范围 1069×1424  黑白`（`bigme.toml:5-7,31-39`；断言 `lib.rs:1259-1289`）
    - 假 MTP（`BOOKLIB_MTP_DIR` + `BOOKLIB_MTP_NO_MOUNT_CHECK=1`）：`devices` 显示接上 `…/mtp/bigme/内部共享存储空间/books`；`sync --device=bigme --no-meta` 产物落到该 `books/` 下。`dir = "books"` 小写照汉王 `documents` 的写法（`bigme.toml:49-50`，`docs/devices.md:367`）：安卓存储不分大小写，真机目录是 `Books/`。`deliver.rs` 的 `dir_present` / `ensure_dir` 按确切文件名判断，没有给目录做 `fix_case`；汉王真机已是「写 documents、落 Documents」。第一次真机 sync 确认会进已有的 `Books/` 即可，不挡验收。
    - 漫画：`epub-optimize --device=bigme` 处理 kit 里流式测试漫画，全部页图 1264×1680 灰度；OPF `rendition:layout=pre-paginated`、`fixed-layout=true`、`original-resolution=1264x1680`、`book-type=comic`；每页 viewport `width=1264, height=1680`、`img style="width:100%;height:100%"`，`eink-wash.css` 里 html/body 高度 100%（方案 D）；spine 保留 `page-progression-direction="rtl"`；跨页拆出 `p4` + `p4-eink2`（右半在前）。**没有** `eink-thumb`，封面页由 1200×1700 缩到画布 1264×1680（约 0.75），没有改成 0.81。
    - 三个统一测试含 `bigme`（`lib.rs:1300-1331`）：伽马 1.1、拉满对比度、去杂点、裁页码、黑底页、拆跨页、按元数据定方向。金样 comic 指纹 `…+1fFbG1.1ASPKW`，与 kobo 同一套画质旗标（有方案 D 的 `fF`，无裁格间空白的 `L`）。`comic_gutter_crop` / `cover_meta_thumb` 缺省 false（`lib.rs:477,563` 的 `#[serde(default)]`），toml 未写。
  - 测试：`cargo test --workspace` 789 过 0 失败（含 `fp_golden`）；`cargo clippy --workspace --all-targets -- -D warnings` 0 警告；`cargo fmt --all -- --check` 干净。与「结果」一致。
  - 「不做」：不出 KFX / KEPUB；未加新 profile 键或新转换逻辑；未改另外四台的值；未开 gutter crop；未写封面缩略图；未改正文封面比例；未改 xReader 翻页；未改 `install.sh`；未配 MTP 自动挂载。真机验证（挂载规则 / 传书后 xReader 打开）按任务单留给合并后，记在「未完成」。
  - 提交合规：中文前缀加正文，无 `Co-Authored-By`，未合并进 master。`feat:` 正文写了「没跑真书回归、新设备的产物没上真机看」。
  - 「结果」与 diff 对得上。文字书在 `text_repair_only` 下不按 1069 宽重排，只在 profile / `devices` 层核对，写得诚实。`偏离：无` 同时交代了 `dir_disables_builtin_mode` 把 `bigme` 加进「其余内置模式都在」的断言（`lib.rs:1532`），只加强测试、在涉及文件内，接受。
  - 不阻断：
    - 建议：`docs/devices.md:286` 把 Bigme 方案 D 的摘要插在「可阅读范围」两条列表项中间，没有做成列表项，读起来断档；同一内容已在表格（`:122`）、「各模式的数字」（`:307-309`）和设备专节（`:361-372`）写过，可把这一段挪进专节或改成列表项。
    - 建议：`lib.rs:1297-1322` 三个统一测试的注释还写「四台」，列表已经有五台（含 bigme）。
  - 下一步：agy 二审（本单改设备行为：新设备投递路径，接上后第一次 sync 会给它生成全部跟踪的书），范围 `dc1eafe..` 到本审查提交。没有阻断再合并改 `verified`。
- 二审：
- 合并：
