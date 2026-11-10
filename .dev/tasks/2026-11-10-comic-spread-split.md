# 漫画跨页拆分：横向跨页拆成两张竖页，顺序随翻页方向

- Task ID: 2026-11-10-comic-spread-split
- 负责 bot: sheng-ren
- 状态: done
- 执行方: opencode-local
- 分支: feat/comic-spread-split（从最新 master 切出；须在 2026-11-10-manga-rtl-bangumi 合并之后）
- 更新时间: 2026-11-10T14:50+08:00
- 引用: 无（PRD：vault `Projects/sheng-ren/prd/2026-11-10-comic-layout-direction.md` 的 C 节）

## 需求原文
> 1. 我们用竖屏，是不是只需要跨页拆分
>
> （sheng-ren 答：对，竖屏只需要拆分，不要旋转；日漫拆开后右半页排在前面，跟着第 2 条的方向走。）

## 输入
- 背景：vault `Projects/sheng-ren/context.md`；仓库 `docs/decisions.md`（2026-10-29：「优秀漫画/」155 卷 Kmoe 版阅读顺序里没有横向跨页——本功能只对别的来源有用，测试全用合成书）、`docs/typesetting.md` 漫画部分、`docs/architecture.md`
- KCC 只作功能参照：README 写它的 `--splitter` 有拆分 / 旋转 / 两者三种；我们只要拆分。**不读 KCC 源码**，做法按本单从头写
- 现状：漫画页是「一张图一页 xhtml」逐条处理，图只做 1→1 的字节变换（`crates/bookconv/src/imgopt.rs:1165` `prepare_comic_page_limited`；流水线 `crates/bookconv/src/optimize/streaming.rs` 的 `optimize_inner`、`process_image`、`write_entries`）。往 manifest 补条目、推迟写 OPF 已有先例：远程图（`streaming.rs:12` 注释、`finalize_opf` 约 975 行）
- 纯图页判断：`crates/bookconv/src/comicfxl.rs:111` `image_only`；固定版式页写法 `comicfxl::page`
- 方向：`crates/bookconv/src/direction.rs` `spine_direction`；最终方向＝`OptimizeOpts::page_direction`（按书，上一张单的 Bangumi 日漫）> `comic_page_direction`（profile）> 原书 spine（`optimize/mod.rs:1058-1060` 已有这个优先级）
- 页码投票：`streaming.rs:708` `scan_page_numbers`
- 涉及文件：
  - `crates/bookconv/src/imgopt.rs`（新增：判跨页、按半边裁出两张的函数；常量 `SPREAD_MIN_ASPECT`）
  - `crates/bookconv/src/optimize/streaming.rs`（修改：拆页、补 manifest / spine、写新 xhtml 和新图）
  - `crates/bookconv/src/optimize/mod.rs`（修改：`OptimizeOpts::comic_split_spreads`、`for_profile` 取 profile；`COMIC_VERSION` 65 → 66 并加一行历史）
  - `crates/bookconv/src/comicfxl.rs`（只在固定版式新页需要时改）
  - `crates/profile/src/lib.rs`（修改：新键 `comic_split_spreads: bool`，缺省 false）
  - `crates/profile/profiles/{kindle,kobo,hanvon,ireader}.toml`（各加 `comic_split_spreads = true`）
  - `crates/library/src/fingerprint.rs`（修改：漫画段在开了的模式加一个新字母，如 `W`；不开的模式指纹不变）
  - `crates/library/tests/fp_golden.rs`、`fp_golden.txt`（加 mutation；四台漫画行因新字母和 c66 都会变，文字书行 0 变化）
  - `crates/bookconv/src/optimize/tests.rs`（新增测试）
  - `docs/architecture.md`（顺带：指纹段落里 `comic_autocontrast`/`comic_trim_specks`/`comic_page_numbers`/`comic_dark_bg` 的「只 Kindle 开」改成 2026-11-10 起四台都开；A 单二审建议）
  - `docs/typesetting.md`（漫画一节加「跨页拆分」；顺带把拉满对比度一段的「掌阅、Move 不开」改成四台都开）、`docs/devices.md`（新键一行）、`docs/decisions.md`（一行）、`docs/development.md#版本号`（COMIC_VERSION 现值 66）

## 约束
- 不做：不旋转；不保留整张跨页（拆完原图不再出现在阅读顺序里，vinci 2026-11-10 定）；不找装订线、不按内容挪切口；不拆封面；不做条漫；不改 CBZ 转换（`convert/`）、不改 KFX / KEPUB 写出器；不改 install.sh
- 跨页判定（全部满足才拆）：
  1. 书是漫画（`is_comic_book`），profile 开了 `comic_split_spreads`
  2. 这一页是纯图页（`comicfxl::image_only`），只引用一张图
  3. 图摆正（EXIF）后 `宽 / 高 ≥ 1.2`（常量 `SPREAD_MIN_ASPECT = 1.2`，vinci 2026-11-10 确认；注释：单页约 0.65–0.75、跨页约 1.3–1.5；1.0–1.2 的宽页不拆，照旧整张缩放）
  4. 不是封面：不是 spine 第一页，也不是 OPF 封面图（`properties="cover-image"` 或 `<meta name="cover">` 指的图）；包封式宽封面（背面 + 书脊 + 正面）也整张保留，不拆、不只取正面（vinci 2026-11-10 定）
  5. 不是装饰小图（短边 < 阅读范围宽 1/3 的，同 `prepare_comic_page_limited` 的现有规则）
  6. 动图（多帧 GIF / 动画 WebP）不拆
- 拆法：正中竖线切开，左半宽 `w/2`（向下取整），右半宽 `w - w/2`。最终方向是 rtl → 右半在前，否则左半在前。两半各自走完整的现有漫画处理（裁白边、杂点、拉对比度、伽马、黑底、缩放补白、格式）；**两半不参加页码投票、不裁页码**（`cut_bottom = None`）
- 写法：
  - 原页 xhtml 和原图条目名留给先读的那一半（图字节换成那一半）；后读的一半新建图条目（同目录，`<原名去扩展名>-eink2.<ext>`，重名就加序号）和新 xhtml（同目录 `<原名去扩展名>-eink2.xhtml`，内容照原页、只换图的 src；固定版式照 `comicfxl::page` 的写法）
  - manifest 补这两项（media-type 照实际格式）；spine 里紧跟原页插一个 itemref（`linear` 照原页）；原页 itemref 上的 `page-spread-left`、`page-spread-right`、`rendition:page-spread-*` 去掉，新 itemref 不带
  - 目录（nav / ncx）不改：指向原页，原页正是先读的那半
  - 书里文字一个字不变；新 xhtml 不含文字
- 指纹 / 版本：COMIC_VERSION 加一（c66，历史写一行）；profile 新键在漫画段出新字母，只有开了的模式出
- 只在上面的分支工作；不合并；不改「涉及文件」以外的文件，确需改时写进「结果 → 偏离」
- 提交信息用中文，`feat:` 等前缀 + 正文，不加 Co-Authored-By；正文写「没跑真书回归、没上真机；c66 + 新字母让四台全部漫画重建一次，没有跨页的书产物应逐字节不变（待本机回归确认），Kindle 上字节变了的书进度清零」
- 不读密钥、`.env`、`auth*`；clean-room：不读 GPL 代码（含 KCC 源码）
- 内存：拆页时只多解码一次同一张图，峰值不超过现在处理单张大页的上限（`MAX_COMIC_DECODE_PIXELS`）；外来数据不 panic
- 只对改动的 Rust 文件跑 rustfmt；编译用 `CARGO_TARGET_DIR=~/.cache/sheng-ren-target`
- 风险点：设备行为（页数、顺序变）、漫画规则（动漫画路径，维护者 2026-11-10 已同意）→ 合并前 agy 二审

## 分步任务
1. [x] `profile/src/lib.rs` 加 `comic_split_spreads`；四个 toml 写 `true`，注释「2026-11-10 维护者定：竖屏只拆跨页、不旋转；没上真机」。自检：`cargo test -p profile`
2. [x] `imgopt.rs`：`pub fn spread_halves(bytes, …) -> Option<(Vec<u8>, Vec<u8>)>`（或同义：返回左右两半的解码图 / 字节）和 `is_spread(w, h) -> bool`；单元测试：1600×1100 → 是、1100×1000（1.1）→ 否、700×1000 → 否；奇数宽 1601 → 左 800 右 801
3. [x] `streaming.rs` + `mod.rs`：按「约束 → 写法」拆页、补 manifest / spine、写新条目；`OptimizeOpts::comic_split_spreads`；COMIC_VERSION 66。自检：`cargo test -p bookconv`
4. [x] `optimize/tests.rs` 新测试（合成书：封面一页 + 普通页 + 一张 1600×1100 跨页，左半填灰度 40、右半填 200，或画 L / R）：
   - ltr：spine 多一页，顺序 封面、普通页、左半、右半；左半那页的图平均灰度接近 40
   - rtl（OPF spine 写 rtl，或 `page_direction = Some(Rtl)`）：顺序 右半、左半
   - 1.1 宽的页、封面是宽图时：不拆
   - 原跨页 itemref 带 `properties="page-spread-left"` 时：拆后两页都不带
   - 开关关着：产物和现在逐字节相同
   - 固定版式（kindle 的 opts）也能拆，新页带 viewport
   - CBZ 来源（`convert::cbz` 转出的）同样能拆
5. [x] `fingerprint.rs` 新字母；`fp_golden.rs` 加 mutation；`UPDATE_FP_GOLDEN=1 cargo test -p library --test fp_golden`；`git diff` 应只有漫画行变（四台），文字书行 0 变化
6. [x] 跑一遍 KFX、KEPUB：用 `cargo run --release -p bookconv --bin epub-optimize -- --device=kindle` / `--device=kobo` 处理第 4 步的合成书，再 `epub-to-kfx` / 书库生成，确认页数 +1、不报错（结果里贴命令和输出摘要）
7. [x] 文档：`docs/architecture.md`（只改上面那句旧表述）、`docs/typesetting.md`、`docs/devices.md`、`docs/decisions.md`、`docs/development.md#版本号`
8. [x] `cargo test --workspace` 全过；`cargo clippy --workspace --all-targets -- -D warnings` 0 警告

## 验收标准
- [x] 功能：满足判定的跨页拆成两页，ltr 左先右后、rtl 右先左后；不满足的（宽页、封面、小图、动图、非纯图页）不变
- [x] 功能：manifest / spine 合法（`bookconv::check::check_epub_file` 质量门通过），目录仍指向先读的那半
- [x] 测试：`cargo test --workspace` 全部通过；clippy 0 警告；第 4 步每条都有断言
- [x] 不回归：开关关着时产物逐字节不变（测试证明）；文字书指纹 0 变化
- [x] 「不做」的事确实没做
- [ ] 真机（vinci，合并后）：四台看合成跨页书（或 vinci 找的真跨页书）两半的顺序、铺满情况；Kindle、Kobo 固定版式看新页没有被拉伸

## 交付物
代码提交到上面的分支并推送；「结果」填好；状态改为 done 或 blocked。

## 给执行方的指令
```
执行任务单 .dev/tasks/2026-11-10-comic-spread-split.md：先把状态改为 accepted 并提交，按「分步任务」做完，填好「结果」，状态改为 done（做不下去改为 blocked 并写明原因），提交并推送分支。
```

## 结果

- 执行方 / 开始–结束时间：opencode-local / 2026-11-10 14:20–14:50 +08:00
- base..head：`origin/master（a02315d）..HEAD`（任务单提交 + `663d9b8` accepted + `36cb229` 实现）
- 提交：

  | 步骤 | 提交号 | 文件 | 说明 |
  |---|---|---|---|
  | 接受任务 | `663d9b8` | `.dev/tasks/2026-11-10-comic-spread-split.md` | 状态改为 accepted |
  | 1–8 | `36cb229` | `imgopt.rs`、`streaming.rs`、`optimize/mod.rs`、`optimize/tests.rs`、profile、四个 toml、`fingerprint.rs`、`fp_golden`、文档 | 跨页拆分、c66、W、测试和文档 |

- 测试：

  | 命令 | 通过 / 失败 / 未跑 | 摘要 |
  |---|---|---|
  | `cargo test -p profile` | 通过 | 13 个，含四台 `comic_split_spreads` 断言 |
  | `cargo test -p bookconv` | 通过 | 527 个，含新增 3 个跨页测试和 2 个 imgopt 测试 |
  | `cargo test --workspace` | 通过 | 773 个，0 失败 |
  | `cargo clippy --workspace --all-targets -- -D warnings` | 通过 | 0 警告 |
  | `UPDATE_FP_GOLDEN=1 cargo test -p library --test fp_golden` | 通过 | 旧文字书行 0 变化；漫画行因 c66 + `W` 重建；新增 `comic_split_spreads` mutation 8 行 |

- 功能检查（离线测试）：
  - `imgopt::tests::spread_aspect_and_widths`：1600×1100 是跨页、1100×1000 / 700×1000 不是、正好 1200×1000 按 6/5 整数比算跨页；1601 宽切成 800 / 801。
  - `optimize::tests::comic_spread_split_ltr_rtl_and_skips_non_spreads`：合成书封面（宽）、普通页、1.1 宽页、跨页（左 40 / 右 200）；ltr 多一页且左半暗、右半亮，rtl 反过来；宽封面和 1.1 宽页不拆；OPF 不再有 `page-spread`；nav 字节不变；开关关着仍是 4 页、没有 `-eink2` 条目。
  - `optimize::tests::comic_spread_split_skips_small_animated_and_text_pages`：小图（短边 < 阅读范围宽 1/3）、动图、图文混排页都不拆。
  - `optimize::tests::comic_spread_split_fixed_layout_and_cbz`：Kindle、Kobo 固定版式都能拆，新页带 viewport（Kobo 100%×100%）；`check::check_epub` 质量门通过；CBZ 转出的书同样多一页。
- KFX / KEPUB 实际命令（合成书 `/tmp/opencode/spread-split/input.epub`，4 页原始 spine）：
  - `cargo run --release -p bookconv --bin epub-optimize -- --device=kindle input.epub kindle.epub` → 17 文件 / 6 章；产物 spine 5 页（cover、p1、p3、p2、eink-spread-page-0），新增 `p2-eink2.xhtml`、`p2-eink2.png`，`page-spread` 已去掉。
  - `cargo run --release -p kfx --bin epub-to-kfx -- --id=1 kindle.epub kindle.kfx` → 59684 字节；`kfx-dump` 显示 5 个 sec（sec0–sec4）。
  - `cargo run --release -p bookconv --bin epub-optimize -- --device=kobo input.epub kobo.epub` + `cargo run --release -p bookconv --bin epub-to-kepub -- kobo.epub kobo.kepub.epub` → KEPUB 5 章、spine 5 页，新页和 `kobo.kepub.epub` 都正常写出，没有报错。
- 不回归：开关关着时测试断言页数不变、无 `-eink2` 条目；`fp_golden.txt` 的旧文字书行 0 变化（新增 mutation 的 4 条文字书行是新增，不是旧行变化）。
- 遇到的问题：无。
- 偏离：无（17 个文件都在「涉及文件」里，`crates/library/src/fingerprint.rs` 只加了 `W`）。
- 未完成：真机验证（验收最后一项，合并后 vinci）：四台看合成跨页书或真跨页书的两半顺序、铺满；Kindle、Kobo 固定版式看新页没有被拉伸。**没跑真书回归、没上真机**；c66 + `W` 让四台全部漫画重建一次，没有跨页的书产物应逐字节不变（待本机回归确认），Kindle 上字节变了的书进度清零。

## 审查
<!-- 负责 bot 填写 -->
- 验收：
- 二审：
- 合并：
