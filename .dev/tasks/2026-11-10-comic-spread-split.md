# 漫画跨页拆分：横向跨页拆成两张竖页，顺序随翻页方向

- Task ID: 2026-11-10-comic-spread-split
- 负责 bot: sheng-ren
- 状态: done
- 执行方: opencode-local
- 分支: feat/comic-spread-split（从最新 master 切出；须在 2026-11-10-manga-rtl-bangumi 合并之后）
- 更新时间: 2026-11-10T13:45+08:00
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

### 返工（2026-11-10）

- 接受返工：`19fd603`；返工代码：`69c91a6`（`streaming.rs`、`optimize/tests.rs`、`docs/typesetting.md`）。
- **阻断 1（EPUB3 `properties` 里的 page-spread 值）**：`add_spread_spine_items` 现在会把原页 itemref 的 `properties` 拆成词，去掉本地名（去掉 `rendition:` 等前缀后）以 `page-spread-` 开头的词，其余词保留、空格规整；剩空就整个属性删掉。原来的普通属性 `page-spread-*` 删除逻辑保留。测试书改成规范写法 `properties="rendition:layout-pre-paginated page-spread-left"` 且非自闭合，断言原页剩 `properties="rendition:layout-pre-paginated"`；另加单元测试覆盖只有 `properties="page-spread-right"` 时整个属性删掉。
- **阻断 2（Kobo 半张被拉伸）**：判跨页时同时要求 `(iw / 2).min(ih) >= 阅读范围宽 / 3`，半张会变成装饰小图就不拆；`docs/typesetting.md` 补了半句。测试：Kobo profile、480×400 跨页不拆，spine 页数不变。
- **阻断 3（非自闭合 itemref）**：原页 itemref 不是自闭合时，新 itemref 插在配对的 `</itemref>` 之后（带前缀的按原前缀）；找不到结束标签就不拆（返回 `None`，整书照旧）。测试书原页写成 `<itemref …></itemref>`，断言新 itemref 是 spine 的直接子元素、紧跟在 `</itemref>` 后；单元测试也覆盖自闭合和非自闭合两种。
- **阻断 4（共用图被换成半张）**：拆之前先扫所有 `ish` 条目里的 `<img src>` / `<image href>` / `xlink:href`，统计每张图被几个 xhtml 引用；被不止一页引用的不拆。测试：两页引用同一张 1600×1100 跨页图 → spine 仍 2 页、两页是同一张整图（平均约 203，不是左半约 89），没有 `-eink2`。
- **建议（清洗层改名）**：照 `prep.rep.wash.renamed` 建「新名 → 原名」表，`read_image_head` / `read_by_name` 用原名读原图字节，manifest / 新条目仍用清洗后的新名。
- 测试：`cargo test --workspace` 776 个全过、0 失败；`cargo clippy --workspace --all-targets -- -D warnings` 0 警告。
- KFX / KEPUB 复跑（`input-props.epub` 用规范 `properties` + 非自闭合 itemref）：kindle / kobo 产物 spine 都是 5 页，原页剩 `properties="rendition:layout-pre-paginated"`、新 itemref 在 `</itemref>` 后、没有 `page-spread`；`epub-to-kfx` 59684 字节、`kfx-dump` 有 sec0–sec4 五个 section；`epub-to-kepub` 改写 5 个章节、正常写出。
- 偏离：无（只改 `streaming.rs`、`optimize/tests.rs`、`docs/typesetting.md`）。

## 审查
<!-- 负责 bot 填写 -->
- 验收：**退回**（sheng-ren，2026-11-10，box 上 `e4d51de` 重跑；状态退回 pending，同一执行方 opencode-local 返工）
  - 做到的（附证据）：
    - 判定：`split_spread_pages`（`streaming.rs` 约 790–960 行）只看 spine 里的纯图页（`comicfxl::image_only`：一张 `<img>`、没有字），跳过 spine 第一页和 OPF 封面图；`display_dims` 读文件头（含 EXIF）后 `is_spread`（整数比 6/5，正好 1.2 算）；短边 < 阅读范围宽 1/3 不拆；`prepare_comic_spread` 解码后再判一次。box 上实测 EXIF：存成 2000×1400、方向 6（显示竖）不拆，存成 1400×2801、方向 6（显示横）拆
    - 拆法：`spread_widths` 左 `w/2`、右 `w-w/2`（1601 → 800 / 801 有单元测试）；两半各走 `process_comic_page`（`force_encode`，`cut_bottom: None`）；拆好的图字节非空，不进 `image_positions`，所以不参加页码投票、worker 不再处理
    - 方向：`opts.page_direction`（B 单 `generate.rs:925` 按书写的）> `comic_page_direction`（profile，只对漫画）> 原书 spine，和 `optimize/mod.rs:1066` 写 OPF 方向的优先级同一个式子；测试用 `page_direction = Some(Rtl)` 断言右半在前（就是 B 单那个字段）。box 上 spread.epub（OPF rtl，左半顶上画了黑条）四台产物都是右半（无黑条）在原页、左半在 `-eink2`
    - 不保留整张、manifest / spine：box 上四台产物 spine 都多一页、新页紧跟原页，`epub-optimize --check` 四台都过；目录：四台 nav 和 master 的产物逐字节相同
    - 固定版式 / 流式 / CBZ：Kindle viewport 1272×1696、Kobo 1072×1448 + 100%×100% 有测试；box 上 `epub-to-kfx` → `kfx-dump` 6 个 section（master 产物 5 个），`epub-to-kepub` → KEPUB spine 多一页、新页带 viewport；hanvon / ireader 流式产物新页正常；CBZ（`convert::cbz` 转出的）多一页有测试（ltr）
    - profile：四个 toml `comic_split_spreads = true`，`builtin_profiles_split_comic_spreads` 断言；COMIC_VERSION 66 + 历史一行；指纹 `W` 只在开了的模式出
    - fp_golden：240 行删、248 行加；删掉的全是四台的漫画行（每台 60 行，`c65`→`c66`、末尾加 `W`），文字书旧行 0 变化；多出的 8 行是 `comic_split_spreads` mutation（漫画行没有 `W`、文字书行同 `none`）
    - 开关关着 / 没有跨页：box 上 master（`a02315d`）和本分支的 `epub-optimize` 比：没有跨页的书四台产物逐字节相同；把四个 toml 临时改成 `false` 重编，有跨页的书四台产物和 master 逐字节相同
    - 文档：architecture.md、typesetting.md 两处「只 Kindle 开」/「掌阅、Move 不开」已改；typesetting、devices、decisions、development（c66）都写了
    - 测试：`cargo test --workspace` 23 组 773 个通过、0 失败；`cargo clippy --workspace --all-targets -- -D warnings` exit 0；`cargo fmt --all -- --check` exit 0
    - 提交合规：中文前缀加正文、无 Co-Authored-By、没动 master、没合并；正文写了没跑真书回归、没上真机、Kindle 进度清零
  - **阻断 1（验收标准没做到）：EPUB 3 正规写法的 `properties="page-spread-left"` 没去掉**。`add_spread_spine_items`（`streaming.rs:1067`）只删名字以 `page-spread-` 开头的**属性**；EPUB 3 里 `page-spread-left` / `page-spread-right` / `rendition:page-spread-*` 是 `properties` 属性里的**值**。测试书（`tests.rs:565`）写的 `page-spread-left="yes"` 不是规范写法，所以测试过了。box 上把测试书改成 `<itemref idref="p2" properties="page-spread-left"/>`，产物原页 itemref 仍带 `properties="page-spread-left"`（四台都是；日本固定版式漫画 EPUB 常见每个 itemref 都带）。要改：原页 itemref 的 `properties` 里去掉本地名（去掉 `rendition:` 等前缀后）以 `page-spread-` 开头的词，其余词保留、空格规整，剩空就整个属性删掉；现有删属性的逻辑可以留着。测试：测试书的跨页 itemref 改成 `properties="rendition:layout-pre-paginated page-spread-left"`，断言原页 itemref 剩 `properties="rendition:layout-pre-paginated"`、新 itemref 不带 `properties`；再加一条只有 `properties="page-spread-right"` 的，断言属性整个没了
  - **阻断 2（Kobo 新页被拉伸）：两半是装饰小图时仍按画布尺寸写 100%×100%**。`streaming.rs:890` 只判整张短边；拆出来的半张短边是 `min(w/2, h)`，可能 < 阅读范围宽 1/3，这时 `process_comic_page` 走装饰小图分支（只裁边、不缩放补白），但 `streaming.rs:259` 一律按画布尺寸记 `image_dims` → Kobo 写 `width:100%;height:100%`。box 上实测：Kobo profile、480×400 跨页（阈值 357），两半各 240×400，产物两页都是 `width:100%;height:100%`（0.6 的图拉到 0.74 的画布）。要改：判跨页时同时要求两半不是装饰小图——`(iw / 2).min(ih) < screen.width / 3` 时不拆（整张照旧处理：整张不是小图，会正常缩放补白）；`docs/typesetting.md` 那条补半句。测试：Kobo opts、480×400 跨页不拆、spine 页数不变
  - **阻断 3（OPF 结构错）：原页 itemref 不是自闭合时新 itemref 插到它里面**。`streaming.rs:1078` 插在开始标签的 `t.end` 后。box 上实测 `<itemref idref="p"></itemref>` → `<itemref idref="p"><itemref idref="eink-spread-page-0"/></itemref>`（`check_epub` 没查出来）。要改：不是自闭合的，插到对应 `</itemref>`（带前缀的照前缀）之后；找不到结束标签就不拆这一页（整书照旧，同 `matched != adds.len()` 的处理）。测试：原页写成 `<itemref idref="p2"></itemref>`，断言产物里新 itemref 是 spine 的直接子元素、紧跟在原页 `</itemref>` 后
  - **阻断 4（别的页图被换成半张）：同一张跨页图被两页引用时，另一页显示成半张**。`streaming.rs:874` `seen_images` 只防重复拆，但 `streaming.rs:994` 按图条目名把字节整个换成先读的一半，所有引用这张图的页都变。box 上实测：p002、p003 两页都引用 `img/p002.jpg`，产物 p002 拆成两页正常，p003 也只剩一半。要改：先统计每张图被几个 xhtml 引用（所有 `ish` 条目里 `<img>` / `<image>` 的 src / href 解析后），被不止一页引用的不拆。测试：两页引用同一张跨页图 → 不拆、两页图字节一样且是整张处理后的结果
  - 建议（不阻断）：
    - 清洗层改过名的图（`safe_names`：文件名里有 `*:?"<>|\` 的）：`split_spread_pages` 拿新名去原书 zip 读（`read_image_head` / `read_by_name`），读不到就静默不拆。要拆的话照 `WritePlan::src` 用 `prep.rep.wash` 的改名表换回原名
    - 「开关关着逐字节不变」测试只断言页数和没有 `-eink2`，没比字节；box 上已用 master 二进制证明（见上），不强求补
    - CBZ 来源只测了 ltr；rtl 与 EPUB 同一条代码路径，不强求
- 返工后：sheng-ren 重新验收，通过后再请 vinci 跑 agy 二审（回执 `.dev/tasks/2026-11-10-comic-spread-split.agy.md`）
- 复验：**通过**（sheng-ren，2026-11-10，box 上 `ccc9bcf` 重跑；状态保持 done，等 agy 二审，二审无阻断后合并、改 verified）
  - 返工记录：「结果 → 返工（2026-11-10）」已写（`19fd603` 接受、`69c91a6` 代码、`ccc9bcf` done）；返工只改 `streaming.rs`、`optimize/tests.rs`、`docs/typesetting.md`，没出「涉及文件」。注：返工记录里写的时间（15:40）和提交时间（13:29）对不上，不影响结论
  - 阻断 1 已修：`add_spread_spine_items`（`streaming.rs:1128-1152`）把 `properties` 拆成词，去掉本地名以 `page-spread-` 开头的，其余保留，剩空删属性。box 上合成书 b1（rtl，三张跨页 itemref 分别带 `rendition:layout-pre-paginated page-spread-left`、`page-spread-right`、`rendition:page-spread-center  rendition:align-x-center`）：四台产物依次剩 `properties="rendition:layout-pre-paginated"`、属性整个没了、`properties="rendition:align-x-center"`（双空格规整成一个）；新 itemref 都不带 `properties`。上一轮的 spread.epub（`properties="page-spread-left"`）四台产物和上一轮比只有 OPF 变（原页 itemref 的 `properties` 没了），没拆的 1.07 宽页照旧保留 `page-spread-left`
  - 阻断 2 已修：`streaming.rs:938-944` 多判 `(iw / 2).min(ih) < screen.width / 3`。box 上 b2（Kobo 阈值 1072/3=357）：480×400、712×400（半张 356）不拆、716×400（半张 358）拆；Kobo 产物所有页图都是 1072×1448 画布，`width:100%;height:100%` 不再拉伸；Kindle、掌阅（阈值更高）都不拆；`docs/typesetting.md` 已补半句
  - 阻断 3 已修：`streaming.rs:1154-1160` 非自闭合时用 `html::find_close` 插到 `</itemref>` 后，找不到返回 `None`（整书不拆）。box 上 b3：`<itemref idref="x2" properties="page-spread-left"></itemref>` → 新 itemref 紧跟 `</itemref>`、原页 `properties` 删掉；带前缀、内有换行的 `<opf:itemref idref="x4" linear="yes">\n  </opf:itemref>` → 新的是 `<opf:itemref idref="eink-spread-page-2" linear="yes"/>`，紧跟结束标签。四台都对，Kindle 产物 `epub-to-kfx` → `kfx-dump` 8 个 section（spine 8 页）
  - 阻断 4 已修：`streaming.rs:858-884` 先扫所有 `ish` 条目的 `<img src>` / `<image href|xlink:href>` 统计引用页数，>1 不拆（`streaming.rs:913-918`）。box 上 b4：p002/p003 共用一张跨页图（`<img>`）、p004 的图另被不在 spine 里的 `sub/extra.xhtml` 用 SVG `<image xlink:href="../img/p004.jpg">` 引用 → 两张都不拆，四台产物这两张图和 master 产物逐字节相同（整张处理）；只被一页引用的 p006 照常拆。上一轮的 dup.epub 也不再拆
  - 不阻断建议「清洗层改名」也做了：`streaming.rs:846-857` 按 `wash.renamed` 换回原名读。box 上 b5（`img/p:002.jpg`）四台都拆出 `img/p_002.jpg` + `img/p_002-eink2.jpg`
  - 质量门：b1–b5 四台 20 个产物 `epub-optimize --check` 全过，两次运行逐字节相同；Kobo b1 → `epub-to-kepub` 11 章正常
  - 方向：spread.epub（OPF rtl）四台原页仍是右半、`-eink2` 是左半（左半顶上黑条只在 `-eink2`）；`page_direction`（Bangumi 日漫）> profile > 原书 spine 的代码和测试没动
  - 不回归：没有跨页的 nospread.epub 四台产物和 master（`a02315d` 重编）逐字节相同；四个 toml 临时改 `false` 重编后，b1–b5 和上一轮 4 本（spread / exif / dup / nospread）共 36 个产物全部和 master 逐字节相同（改动已还原）
  - fp_golden：`a02315d..HEAD` 仍是删 240 行、加 248 行，删掉的 240 行全是四台漫画行（`c65`），文字书旧行 0 变化；返工没动 `crates/library`
  - 测试：`cargo test --workspace` 23 组 776 个通过、0 失败（比上一轮多 3 个返工测试）；`cargo clippy --workspace --all-targets -- -D warnings` exit 0；`cargo fmt --all -- --check` exit 0
  - 提交合规：中文前缀加正文、无 Co-Authored-By、没动 master、没合并
  - 建议（不阻断，可留给二审 / 以后）：
    - 「找不到 `</itemref>` 就不拆」这条路径没有测试（`add_spread_spine_items` 返回 `None`）；另外 `find_close` 取的是后面第一个 `</itemref>`，原页开始标签没闭合、后面又有非自闭合 itemref 的坏 OPF 会插错位置（OPF 本身已不合法，不强求）
    - 共用图只统计 xhtml 里的 `<img>` / `<image>`，CSS `background-image` 引用同一张图的不算（漫画书里少见）
    - 新 itemref 不带原页 itemref 上 `page-spread-*` 以外的 `properties`（如单页 `rendition:layout-pre-paginated`）；四台输出整书版式一致，目前没影响
    - 上一轮的「开关关着测试只比页数」「CBZ 只测 ltr」没补测试；box 上已用 master 二进制证明开关关着逐字节相同
- 二审：待 vinci 跑 agy（范围 `a02315d..<本次复验提交>`）
- 合并：
