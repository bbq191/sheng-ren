# 竖排第一期：Kobo、汉王的中文文字书竖排（引号转直角、西文数字转全角竖立）

- Task ID: 2026-11-11-vertical-text
- 负责 bot: sheng-ren
- 状态: pending
- 执行方: opencode-local
- 分支: vertical-text（从最新 master `10ce515` 或更新的切出）
- 更新时间: 2026-11-11T15:45+08:00
- 引用: 无（竖排 PRD：vault `Projects/sheng-ren/prd/2026-11-11-vertical-text.md`；Kindle 翻页方向已由 `2026-11-11-kindle-text-rtl` 单独修好）

## 需求原文
> 以文字为主的书籍是否可以加入竖排功能（包括标点和注释转换）

vinci 2026-11-11 的决定（原话或口述结论）：
> 1. 哪些书竖排：中文文字书全部竖排
> 2. 引号：按 GB/T 15834：“”换『』、‘’换「」
> 3. 同意竖排书例外改版式和引号
> 4. 自带阅读器和 KOReader 都用
> 5. Kindle 翻页方向问题先单独修
>
> 目录引号跟着转；能弹窗的设备改弹窗；表格保持横排；开竖排，以自带阅读器为主；我不会有日文韩文书籍的（阈值照原案）。
>
> 实机（Kobo Clara BW、汉王 Clear 6 Turbo、Bigme B7 Pro，测试书 01–13）：Kobo「在 01 翻页时点左边，段距：a 感觉距离更宽」；13 号「U1 不错，其次 U3」；「先说汉王，1-11 正常，13 U1；bigme，没有竖排」。

## 输入
- 背景：vault `Projects/sheng-ren/context.md`；PRD 见上（调研、出处、实机记录都在那里，本单只写要实现的行为）。仓库 `docs/decisions.md`（2026-10-08「文字书只修复」）、`docs/typesetting.md`、`docs/devices.md`、`docs/kepub.md`。
- 实机结论（2026-11-11，本单据此定规则）：
  - Kobo（KEPUB，Nickel）：`html` 或 `body` 上写 `writing-mode: vertical-rl` 就竖排，只写 `primary-writing-mode` 不竖排；spine rtl 时点左侧是下一页；段距用物理左右外边距（测试书 10 的 a：`p{margin:0 1em}`）最宽；ruby 在右侧；注释（`<aside epub:type="footnote">` + `noteref`）点开是「预览注释」弹窗。
  - Kobo 的坑：半角数字、西文躺倒时会叠到前一个汉字上，插空格、`margin`、`text-orientation:upright`、縦中横都治不好（测试书 11、12）；**全部转全角竖立**（13 号 U1）干净。
  - 汉王（EPUB，自带阅读器）：01–11 正常（竖排、翻页、引号、縦中横、注释、西文不叠），13 号也选 U1。
- vinci 2026-11-11 另定四处细节（见「已定细节」）：Kobo 全部文字书注释改弹窗；半角空格不转；ASCII 直引号也转直角；目录（ncx、nav）同正文转全角和引号。
  - Bigme（xReader）不竖排，KOReader（上游）也不会竖排 → Bigme 不开。
- 代码现状（master `10ce515`，行号以此为准）：
  - 四台 profile 都是 `text_repair_only = true` + `kindle_rules = true`（`crates/profile/profiles/kobo.toml:11-12`、`hanvon.toml:9-10`、`bigme.toml:11-12`），文字书走**只修复**：`crates/bookconv/src/optimize/streaming.rs:86-97` 判漫画后换成 `repair_only()`（`optimize/mod.rs:366`）；章节变换 `EntryXform::html_prefix`（`optimize/mod.rs:1145`）在只修复时开头就返回（`:1146-1171`），**正文现在一个字不动**——竖排的文字处理要在这条路上另开口子。
  - 只修复的清洗：`crates/bookconv/src/wash/mod.rs:692` `repair_entries`，`kindle_rules` 时跑 `KINDLE_RULE_PASSES`（`:771` 起，各 pass 的 `enabled` 看 `WashOpts`）；`eink-ua.css` 由 `add_ua_css`（`:1018`）写，内容 `crates/bookconv/src/uastyle.rs:23` `ua_css`（物理上下边距）；我们写的样式表、manifest 条目用 `add_css_entry`（`:1046`）、`beside_opf`。
  - 优化选项：`OptimizeOpts`（`optimize/mod.rs:254`），`new`（`:409`）、`for_profile_against`（`:454`）；清洗选项 `WashOpts`（`wash/mod.rs:299`），`wash_opts_for`（`optimize/mod.rs:495`）。
  - OPF 改写：第二遍只在 `opf_name` 有值时动 OPF（`optimize/mod.rs:714` 的条件、`:1277` 的分支）；spine 方向 `crates/bookconv/src/direction.rs:57` `set_spine_direction`（日漫 rtl 已在用）。
  - 注释：`htmlproc/footnote.rs`；弹窗写法 `:398-405`（`<aside epub:type="footnote" … class="eink-note">`），模式枚举 `FootnoteMode`（`optimize/mod.rs:210`），只修复下的「保证注释能点」分支在 `html_prefix:1146-1171`（profile `repair_note_links`，现在只有 xochitl 用）。
  - KEPUB：`crates/bookconv/src/kepub.rs`，koboSpan 跳过 `rt/rp` 等（`:26` `SKIP`），写出器版本 `:19` `VERSION = 2`。
  - 漫画判定：`crates/bookconv/src/comic_detect.rs:68` `is_comic`；去噪 `:16` `strip_noise_tags`。
  - 版本：`OPTIMIZE_VERSION = "67"`（`optimize/mod.rs:167`，上面是历史说明）；`COMIC_VERSION = "68"`（`:204`）。
  - profile：`Profile`（`crates/profile/src/lib.rs:379`，`cover_image_thumb` 在 `:446`）、TOML 读入结构（`:578` 一带）、转换（`:678` 一带）、`validate`（`:697`，同类校验见 `:774`）、`every_field_has_a_fingerprint_decision`（`:1054`）。`--print-profile` 输出 JSON 给 tools 读。
  - 书库：`crates/library/src/generate.rs` `plan`（`:98`，指纹拼法 `:160`；日漫 rtl 的按书字母 `r` 在 `:142-148`，照这个口子加竖排字母）、`produce`（`:912`，`OptimizeOpts::for_profile` 在 `:923`）、`comic_of`（`:202`，判一次存进 `Meta` 的写法照抄）；`Meta`（`crates/library/src/lib.rs:67`，`comic` 在 `:103`）；设备段指纹 `crates/library/src/fingerprint.rs`（`text_seg:148`）；金样 `crates/library/tests/fp_golden.rs`（`mutations:19`）+ `fp_golden.txt`。
  - CLI：`crates/library/src/bin/booklib/main.rs` 的 `COMMANDS`（`:38`，`track` 在 `:77`，`list` `:127`，`devices` `:180`；`cmd_devices:881`、`cmd_list:929`）；补全 `bin/booklib/complete.rs`（`paths:28`、fish 脚本 `:171`，`track` 的写法 `:205`，测试 `fish_script_covers_every_command:298`）。
  - `epub-optimize`：`crates/bookconv/src/bin/epub_optimize.rs`（选项表 `:44-56`）。
  - 字符账：`tools/regress/compare.py`。
- 涉及文件（预计）：
  - 新增 `crates/bookconv/src/vertical.rs`（判定、引号、全角、横转竖 CSS、竖排样式表、OPF；按需拆子模块）
  - 修改 `crates/bookconv/src/{lib.rs,optimize/mod.rs,optimize/streaming.rs,uastyle.rs,wash/mod.rs,bin/epub_optimize.rs}`，按需 `htmlproc/footnote.rs`、`kepub.rs`
  - 修改 `crates/profile/src/lib.rs`、`crates/profile/profiles/{kobo,hanvon,bigme}.toml`
  - 修改 `crates/library/src/{generate.rs,fingerprint.rs,lib.rs}`、`crates/library/src/bin/booklib/{main.rs,complete.rs}`，按需新增书库配置读写（`vertical.toml`）
  - 测试：`crates/bookconv/src/optimize/tests.rs`、`vertical.rs` 内单测、`crates/library/tests/{flow.rs,cli.rs,fp_golden.txt}`
  - 文档：`docs/decisions.md`、`docs/typesetting.md`、`docs/devices.md`、`docs/kepub.md`、`docs/usage.md`、`docs/architecture.md`（指纹字母）、`docs/development.md`（版本号现值）

## 约束
- 不做：
  - Kindle / KFX（`$192` 仍写死横排，第二期另开单）；`kindle.toml` 不开竖排。
  - 日文、韩文书的处理（假名 ≥ 2% 的书按判定规则不竖排即可）。
  - 縦中横（`text-combine-upright`）、西文躺倒的修补、`text-orientation`：本期两台都走全角竖立，**只给 `vertical_ascii` 留 `tcy` 之类的枚举位，不实现**。
  - 竖排专用码位（U+FE10–FE19、U+FE30–FE4F）一个都不写；省略号、破折号、书名号、括号、标点**不改字**。
  - 阿拉伯数字转汉字数字；页内横竖混排（只有表格整张横排）；割注；Kobo 底边裁字的边距修正（实机没复现，不做）。
  - 不改书库里的原件；不改漫画、固定版式书；不改 Bigme、Kindle、掌阅（ireader）产物（逐字节不变）；汉王的横排书不变。
  - 不读、不参考任何 GPL/AGPL 转换器源码（KCC、calibre、KFX Input），只看公开规范和本仓库代码。
- 只在上面的分支工作；不合并；不改「涉及文件」以外的文件，确需改时写进「结果 → 偏离」。
- 提交信息用中文，`feat:` / `test:` / `docs:` 等前缀 + 正文，不加 Co-Authored-By；不读密钥、`.env`、`auth*`。
- 风险点：**设备行为**（Kobo、汉王全部中文文字书改成竖排、改引号和西文字形；Kobo **全部文字书**（横排的也算）注释改弹窗；第一次 sync 全部重建、这些书重传）、**改文字**（「文字书只修复」的两个例外）、跨很多文件 → 必须 agy 二审。

## 规则（要实现的行为）

### 1. profile 新字段
- `vertical_text: bool`，缺省 `false`：这台的中文文字书竖排。`kobo.toml`、`hanvon.toml` 写 `true`（注释：2026-11-11 测试书 01–13 实机通过，vinci 定）；`bigme.toml` 显式写 `false` 并注明「xReader 不竖排、KOReader 也不会，2026-11-11 实机」；`kindle.toml`、`ireader.toml` 不写。
- `vertical_ascii`：竖排书里半角 ASCII 怎么办，枚举，缺省 `"keep"`（原样，交给阅读器）；本期只实现 `"fullwidth"`（规则 5）。Kobo、汉王写 `"fullwidth"`（注释：测试书 11–13，半角躺倒会叠字，全角竖立干净）。枚举里可以留 `tcy` 的位置但**不实现、不接受**（写了报错「未实现」）。
- 注释弹窗**不加新字段**，用现成的 `notes = "popup"` + `repair_note_links = true`（规则 7）。
- 校验（`validate`）：`vertical_ascii != keep` 而 `vertical_text = false` → 报错（照 `:774` 的写法）。
- 两个字段在 `every_field_has_a_fingerprint_decision` 里都要有决定：**不进设备段**，只在竖排的书上出按书字母（规则 9），这样不竖排的书指纹除版本号外一字不变。

### 2. 哪些书竖排（判定）
全部满足才竖排，判定函数放 `bookconv`（输入是书的条目），书库调用并缓存：
1. 不是漫画（`Meta::comic != Some(true)` / `is_comic` 为假），也不是固定版式（OPF `rendition:layout` 为 `pre-paginated` 或有 fixed-layout 声明）。
2. 是中文：
   - 抽样：按 spine 顺序取正文文字（`strip_noise_tags` 去掉 head/script/style 后去标签），数前 20000 个非空白字符里汉字（CJK 统一汉字 + 扩展 A）、假名（平假名、片假名）的比例。
   - `dc:language` 以 `zh` 开头（不分大小写）且汉字 ≥ 30% → 是；
   - `dc:language` 缺失或是别的值（`und`、`en` 常见误标）时，汉字 ≥ 60% 且假名 < 2% → 是；
   - 假名 ≥ 2% → 不是（日文）；`zh*` 但汉字 < 30% → 不是。
   - 阈值写成常量，注释写来历（PRD 原案，vinci 2026-11-11 同意）。
3. 没有被退出（规则 3）。
- 原书**本来就竖排**（任一样式表或 `<style>` 里根元素 `html`/`body` 或正文块写了 `writing-mode: vertical-*`，含 `-webkit-`/`-epub-` 前缀）：在开了 `vertical_text` 的台上照竖排书处理，但**不再加根元素的 writing-mode**；引号（原书是“”的才转）、西文全角、spine rtl、跳过横排 pass 照做。在没开的台上照现在（原样带过去）。
- 判定结果（是不是中文文字书）入库时判一次存进 `Meta`（字段名自定，如 `zh_text: Option<bool>`），早期条目第一次用到时判、持锁时存下——照 `comic_of`（`generate.rs:202`）的写法。

### 3. 按书 / 按目录覆盖
- `meta.json` 新增 `layout.vertical`：缺省没有（自动判定）；`true`（强制竖排，判定没认出的中文书）；`false`（不竖排）。强制开也不能让漫画、固定版式竖排。
- `booklib vertical --on|--off|--auto <书或目录>…`：书 → 写 / 删这本的字段；目录 → 对目录下现有的书逐本写，并记进书库根的 `vertical.toml`：`off = ["技术/", "外文/"]`（相对书根目录，前缀匹配；`--on` 同理记 `on = [...]`，`--auto` 从清单里删）。以后入库到清单目录的书继承。
- 优先级：书自己的字段 → 目录清单（最长前缀胜）→ 自动判定。只在设备开了 `vertical_text` 时才有意义；不做「某本只在某台竖排」。
- Tab 补全：`vertical` 的路径复用 `__complete paths` 的规则（当前目录 + 书根目录相对路径），但**只列已跟踪的**目录和书（与 `track` 相反）；`--on/--off/--auto` 进 `COMMANDS` 表的选项；`fish_script_covers_every_command` 照覆盖。
- `booklib devices` / `booklib list` 能看到每本书在每台是横是竖（排查用，格式自定，写进 `docs/usage.md`）。

### 4. 竖排书的版式（CSS、OPF）
- 新样式表 `eink-vertical.css`（OPF 同目录，manifest 条目 id 自定，每章 `<head>` 里**最后一个**挂上，盖过书自己的）：
  - 根元素：`html{writing-mode:vertical-rl;-webkit-writing-mode:vertical-rl;-epub-writing-mode:vertical-rl;}`（原书本来竖排的不写这条）；
  - 段距用方法 a：物理左右外边距（实机选定）；
  - `table{writing-mode:horizontal-tb;-webkit-writing-mode:horizontal-tb;-epub-writing-mode:horizontal-tb;}`（表格整张横排）；
  - 只用简单选择器、每条声明以 `;` 收尾（同 `uastyle` 的约定）。
- `eink-ua.css` 竖排版（`uastyle` 加一个函数）：段距、标题间距写 `margin-left/right`，块缩进写 `margin-top`，列表缩进写 `padding-top`，没有物理上下段距。
- `KINDLE_RULE_PASSES` 里按横排算几何的 pass 对竖排书**跳过**：`kindle_images`、`blank_fold`、`block_style`、`top_spacer`、`break_margin`、`box_fix`、`scale_vertical`、`scale_horizontal`、`root_em`（实现方式：`WashOpts` 加 `vertical: bool`，这些 pass 的 `enabled` 加 `!o.vertical`；保留与方向无关的 `hidden`、`empty_pages`、`kindle_rules`（字体、对比度）、`css_compat`、`sup_sub`、`inline_css`、`drop_nonlinear_nav`）。`kindle_rules` 里去 body 左右边距那一条若和竖排冲突，照实处理并写进「结果」。
- 书自己的 CSS 横转竖（只对块级规则的盒子属性）：`margin/padding/border` 的 `top→right、right→bottom、bottom→left、left→top`；`width↔height`、`max-width↔max-height`、`min-*` 同理；`float` 去掉；`text-align`、`text-indent` 不动。图片 `max-width:100%` 变 `max-height:100%`；整页图、封面页不动。原书本来竖排的不转。复杂块（`position:absolute/fixed`、负边距）不转、原样留，记进报告计数。
- OPF：spine 写 `page-progression-direction="rtl"`（`set_spine_direction`，与日漫共用；按书指定了方向的以书为准）；`<meta name="primary-writing-mode" content="vertical-rl"/>` 补上（已有就不重复）。
- 封面页、书架缩略图不变；nav/ncx 结构不变。

### 5. 半角 ASCII 转全角（`vertical_ascii = "fullwidth"`，Kobo、汉王）
- 竖排书里，正文**文字节点**中 U+0021–U+007E 一律换成 U+FF01–FF5E（码位 +0xFEE0），竖立显示；不躺倒、不包縦中横、不加 span。
- 不动：标签、属性值（含 `href`、`src`、`id`、`class`、`style`）、`<code>`、`<pre>`、`<kbd>`、`<samp>`、`<script>`、`<style>`、`<svg>`、`<math>`、`<rt>`、`<rp>`（拼音、注音不转）、`<head>` 里的一切、注释（`<!-- -->`）、CDATA；字符引用要先整体认出来，**不能拆坏**：代表的字符在 U+0021–U+007E 里的（`&amp;` `&lt;` `&gt;` `&quot;` `&apos;`、`&#65;`、`&#x41;`）换成对应全角字的字面字符（`＆` `＜` `＞` `＂` `＇` `Ａ`），别的引用原样留；输出必须仍是合法 XHTML（测试覆盖）。
- U+0020 半角空格**不转**（vinci 2026-11-11 定）。别的空白（换行、制表、U+00A0 等）也不转。
- 注释号（`<sup>`、`[1]`）里的数字跟着转。
- 目录同正文（vinci 2026-11-11 定）：nav 文档的文字、`toc.ncx` 的 `<docTitle>`/`<navLabel>` 里 `<text>` 的文字都转全角、转引号；`href`、`src`、`id`、`playOrder` 等属性不动。
- 引号判定（规则 6 的「全是西文」）要在全角之前、按原始 ASCII 判。

### 6. 引号（GB/T 15834）
- 外层 “ ” → 『 』，内层 ‘ ’ → 「 」。只在**一个段落里成对**（开闭配得上、嵌套合法）时转；配不上整段不转。
- 引号里全是西文（没有汉字）的不转。
- 原书已是「」『』的不动。
- **ASCII 直引号也转**（vinci 2026-11-11 定），在全角之前做，按段落（同上的段落口径）：
  - 先认撇号：前后都是 ASCII 字母或数字的 `'`（`don't`、`90's`、`rock'n'roll`）不是引号，留给全角规则转成 `＇`；
  - 余下的 `"` 在段内按出现顺序开、闭交替配对，成对的 → 『 』；余下的 `'` 同样交替配对 → 「 」；
  - 配不上的（段内单数个，最后剩下的那个）不当引号，留给全角规则转成 `＂` / `＇`；
  - 直引号和弯引号各配各的，不混配；「引号里全是西文不转」只管弯引号，直引号照本条转。
- 不转：`<code>`、`<pre>`、`<script>`、`<style>`、属性值。
- 目录 nav、`toc.ncx` 的文字也转（vinci 定），弯、直引号同正文。
- 除引号（和规则 5 的全角）外字符账一个字不差。

### 7. 注释弹窗（Kobo 全部文字书，vinci 2026-11-11 定）
- Kobo 的**全部文字书**（竖排、横排都算）走弹窗：标号标 `epub:type="noteref"`、注释块写 `<aside epub:type="footnote">`。05 号实机弹「预览注释」。
- 做法：只改 `kobo.toml`：`notes = "popup"`、`repair_note_links = true`（注释原样，标明推翻原来「推断：最稳」的 jump，2026-11-11 实机）。现成路径是 `html_prefix` 的只修复分支（注释搬进本章、改同章锚点，`FootnoteMode::Popup`），原来只有 xochitl 用；按代码要改的地方照实改，写进「结果」。
- 连带效果（照实写进「结果」和文档）：`repair_note_links` 打开后，`note_icons = "number"`（Kobo 已写）也生效，图标注释号换成上标数字；`note_backlinks` 缺省保留，回链不去。指纹的注释段从 `jump#…` 变 `popup#…`、文字书段多 `n`，Kobo 全部书（漫画也带注释段）重建；漫画产物应逐字节不变，要验。
- 汉王不变（`notes = "jump"`，不开 `repair_note_links`）；Bigme、Kindle、掌阅不变。
- 这一步**单独一个提交**（只含 `kobo.toml`、需要的代码改动、测试、金样、文档），可以单独回退。

### 8. 只修复的例外
竖排书在只修复模式下额外做：规则 4 的 CSS/OPF、规则 5、6 的文字处理（规则 7 的注释弹窗不分横竖，走现成的 `repair_note_links` 路径）。在 `html_prefix` 的只修复分支（`optimize/mod.rs:1146`）和完整分支末尾都接上同一个 `vertical` 变换（顺序：注释处理 → 引号（弯、直）→ 全角）；`toc.ncx` 在 `transform_text`（`:1227`）里加一个分支（引号 + 全角，只动 `<text>`）；OPF 让 `opf_name`（`:714`）在竖排时也有值。

### 9. 选项传递、指纹、版本
- `OptimizeOpts` 加 `vertical: bool`（缺省 false）+ profile 值 `vertical_ascii`，`for_profile` 只带 profile 值，`vertical` 由书库按规则 2、3 定后设上（`generate.rs:923` 附近，照 `page_direction` 的写法）；`epub-optimize --vertical` 手动开（出测试书用，profile 没开 `vertical_text` 时也照开，西文按 profile 的值）。
- 指纹：最终竖排的书在 info 段后（照 `r` 的写法）加 `V`，`vertical_ascii = fullwidth` 再加 `f`（字母自定、不和现有冲突，写进 `docs/architecture.md`）。不竖排的书不出新字母。注释弹窗走现成的 `notes` 段和 `n`，不另加字母。
- `OPTIMIZE_VERSION` 67 → 68，历史说明写清：开了 `vertical_text` 的台（Kobo、汉王）上的中文文字书变；另外 Kobo 有注释的文字书因弹窗变（规则 7）；汉王的漫画、非中文书、退出的书，以及 Bigme、Kindle、掌阅逐字节不变。`COMIC_VERSION` 不动（漫画不走竖排）。`kepub::VERSION` 只有 kepub.rs 改了产物时才加。
- 版本号进每一行指纹，四台文字书下次 sync 都会重建；字节没变的不重传（传输层按产物字节判，kobo-cover-pad 二审已核实），Kindle 的 KFX 字节不变、进度不清。

## 分步任务
每步做完跑一次自检再提交；步骤可以合并提交，但规则 7（注释）单独一个提交。

1. [ ] **profile 字段**（`crates/profile/src/lib.rs`）：加 `vertical_text`、`vertical_ascii` 两个字段（枚举、缺省、注释、TOML 读入、`--print-profile` 输出）、`validate` 校验、`every_field_has_a_fingerprint_decision` 的决定；先**不改**各台 toml。测试：缺省值、非法组合报错、`tcy` 报「未实现」。自检：`cargo test -p profile`。
2. [ ] **判定**（`vertical.rs`）：`is_fixed_layout`、`zh_text_sample`（抽样计数）、`is_zh_text(entries) -> bool`、`is_natively_vertical(entries) -> bool`。单测：`zh` 中文书、`zh-TW` 繁体书、误标 `en` 的中文书 → 是；日文书（假名多）、`zh` 但正文英文、中英对照（汉字 < 30%）、固定版式 → 不是；本来竖排的书认得出。自检：`cargo test -p bookconv vertical`。
3. [ ] **引号**（`vertical.rs`）：`convert_quotes(html) -> String`，只改文字节点。单测：成对的转；嵌套 “‘’” → 『「」』；不成对的段不转；全西文弯引文不转；`<code>`、属性值不转；已是「」的不动；直引号：`他说"好"` → `他说『好』`、`'甲'和'乙'` → `「甲」和「乙」`、`"外'内'外"` → `『外「内」外』`、`don't`、`90's`、`rock'n'roll` 里的 `'` 不当引号（全角后是 `ｄｏｎ＇ｔ`、`９０＇ｓ`）、段内单个 `"`（如 `12"`、`5'`）不配对（全角后 `＂` `＇`）、配对不跨段落；转后除引号外字符账相同（测试里自己比，去掉引号映射后逐字相等）。
4. [ ] **全角**（`vertical.rs`）：`fullwidth_ascii(html) -> String`。单测：`是2026年` → `是２０２６年`、`与KOReader` → `与ＫＯＲｅａｄｅｒ`、`US$6` → `ＵＳ＄６`；`<a href="x.html#n1">1</a>` 只改链接文字；`<code>`、`<pre>`、`<rt>`、`<style>`、属性、注释不改；`&amp;` → `＆`、`&lt;` → `＜`、`&#x41;` → `Ａ`、`&#x4E2D;` 原样，输出是合法 XHTML；空格不转（`Hello world` → `Ｈｅｌｌｏ ｗｏｒｌｄ`）。
5. [ ] **竖排样式**：`uastyle` 加竖排版 `eink-ua.css`；`vertical.rs` 写 `eink-vertical.css` 内容和书 CSS 横转竖（规则 4）。`WashOpts.vertical` + `repair_entries` 跳过规则 4 列的 pass、挂竖排样式表。单测：竖排 UA 没有 `margin-top/bottom` 段距；横转竖映射表逐项；`float` 去掉；`position:absolute` 的块不动；原书竖排的不加根元素 writing-mode；`kindle_rule_passes_are_ordered` 照过。
6. [ ] **接进优化器**：`OptimizeOpts.vertical` 等字段；`html_prefix` 两个分支接文字变换、`transform_text` 加 `toc.ncx` 分支、OPF 写 spine rtl + `primary-writing-mode`；`epub-optimize --vertical`。测试（`optimize/tests.rs`）：竖排书 OPF 有 rtl 和 meta、每章最后一个 link 是 `eink-vertical.css`、表格横排规则在、引号和全角生效、`toc.ncx` 的 `<text>` 和 nav 同样转全角和引号而属性不动；`vertical = false` 时产物和改前逐字节相同（同一输入新旧对比）。
7. [ ] **KEPUB**：竖排书过 `kepub.rs` 后 koboSpan 不拆坏全角串、不进 `rt/rp`、不碰 `eink-vertical.css` 链接；Kobo 竖排书过质量门（`check_epub_file`）。需要改 kepub.rs 时加 `VERSION`。测试照 `skipped_elements_and_ruby` 的写法。
8. [ ] **书库**：`Meta` 存判定结果和 `layout.vertical`；`vertical.toml` 读写；`generate.rs` 按规则 2、3 定 `opts.vertical`、指纹加字母；`booklib vertical` 命令、补全、`devices`/`list` 显示。测试（`flow.rs`/`cli.rs`）：Kobo、汉王上中文书竖排、Bigme/Kindle 不竖排；`--off` 单本、目录清单（含新入库到退出目录的书）、`--on` 强制都生效；漫画 `--on` 也不竖排；`fish_script_covers_every_command` 过。
9. [ ] **开台 + 版本 + 金样**：`kobo.toml`、`hanvon.toml` 写两个字段（规则 1 的注释），`bigme.toml` 写 `vertical_text = false` 和理由；`OPTIMIZE_VERSION` 67 → 68 写历史；`fp_golden.rs` 给两个新字段加变异组；`UPDATE_FP_GOLDEN=1` 重写后，把旧行的版本段 `|67|→|68|` 抹平，与 master 逐行对拍，**旧行应一字不变**（kobo-cover-pad 的做法），结果里写新增了几行。
10. [ ] **注释弹窗**（单独提交，在第 9 步之后）：规则 7。测试：Kobo 的竖排书和横排书注释块都是 `<aside epub:type="footnote">`、标号 `epub:type="noteref"`、链接都能点到；Kobo 漫画产物逐字节不变；汉王竖排书、横排书注释和改前一样。金样照实更新：结果里写 Kobo 行（注释段、`n`）变了几行，非 Kobo 旧行仍只有版本号变。
11. [ ] **文档**：`docs/decisions.md` 加「2026-11-11 文字书竖排第一期」——两个例外（竖排版式、引号改字）、Kobo/汉王 ASCII 全角（推翻 PRD 初稿「全角化不做」，只在这两台的竖排产物里做）、Bigme 不开及理由；`docs/typesetting.md` 加「竖排」一节；`docs/devices.md` 两个新字段和 Kobo 的 `notes`/`repair_note_links` 改动；`docs/kepub.md`（竖排、弹窗注释）；`docs/usage.md`（`booklib vertical`、`epub-optimize --vertical`）；`docs/architecture.md`（指纹字母）；`docs/development.md`（版本号现值）。
12. [ ] **合成回归**（不提交书，放 `/tmp/opencode/vertical/`）：自己合成三本最小 EPUB（中文文字书含“”‘’、`2026年`、`KOReader`、`US$6`、表格、一条注释、一段 `<code>`；英文书；漫画），各跑 `epub-optimize --device=kobo|hanvon|bigme|kindle`：贴出中文书 Kobo 产物（KEPUB）的 OPF spine、meta、一章 `<head>` 的 link 顺序、几行正文；对 bigme、kindle、ireader 上全部三本、汉王上的英文书和漫画、Kobo 上的漫画，用 master 二进制对比 `cmp` 逐字节相同（Kobo 文字书因注释弹窗会变，不在此列；另拿一本没有注释的英文书在 Kobo 上比，应相同）（版本标记若在产物里，照实说明）。再用字符账（`tools/regress/compare.py` 或测试里的同等比较）证明中文书除引号、全角外一字不差。合成中文书里加上直引号、撇号（`don't`）、单个 `"` 和一个带引号、西文的目录项。

## 验收标准
- [ ] 功能：Kobo、汉王上的中文文字书竖排（根元素 writing-mode、spine rtl、`primary-writing-mode`、`eink-vertical.css` 最后挂）；表格横排；“”‘’ → 『』「」（含目录）；直引号成对的转直角、撇号和落单的转全角；正文和目录（nav、ncx）ASCII 全角竖立、空格不转、标签/属性/code/ruby 不动；Kobo 全部文字书注释是弹窗写法；汉王注释不变
- [ ] 判定和覆盖：规则 2 的各类书判对；`--on/--off/--auto`、目录清单、补全都生效
- [ ] 不回归：Bigme、Kindle、掌阅产物逐字节不变；汉王上的横排书（漫画、固定版式、非中文书、退出的书）逐字节不变；Kobo **不在**逐字节对比里（有注释的文字书因弹窗变），但 Kobo 漫画、没有注释的横排书应逐字节不变；`fp_golden` 非 Kobo 旧行除版本号外一字不变，Kobo 旧行只多注释段和 `n` 的变化；现有注释、日漫 rtl、封面缩略图测试照过
- [ ] 测试：`cargo test --workspace` 全过；`cargo clippy --workspace --all-targets -- -D warnings` 0 警告；`cargo fmt --all -- --check` 干净
- [ ] 「不做」的事确实没做：没碰 KFX 写出器、没写縦中横、没写竖排专用码位、没改原件
- [ ] 结果里写明没跑真书回归、没上真机。真机由 vinci 验：Kobo、汉王各 sync 一本真书（建议对话多、有注释、有西文的书）——自带阅读器里竖排、点左侧下一页、引号『』「」、数字西文立着不叠字、注释能点开（Kobo 弹窗）、目录能跳且目录里的西文是全角；Kobo 上一本有注释的横排书注释也弹窗；同一本在 KOReader 里能读（横排退化：引号『』「」、段距变左右缩进可接受）；Bigme 和 Kindle 上同一本仍横排、和以前一样

## 交付物
代码提交到上面的分支并推送；「结果」填好；状态改为 done 或 blocked。

## 已定细节（vinci 2026-11-11 答复写单时的待确认）
1. 注释弹窗：Kobo **全部文字书**都开（横排也算），汉王不变；单独一个提交（规则 7）。
2. 半角空格 U+0020 不转（规则 5）。
3. ASCII 直引号：段内成对的 `"` → 『』、`'` → 「」；字母、数字之间的 `'` 是撇号，转全角 `＇`；落单的转全角 `＂` `＇`（规则 6）。
4. 目录（ncx、nav）同正文：转全角、转引号（规则 5、6）。
- 书库目录清单放书库根的 `vertical.toml`（照 PRD 设计；执行方觉得并进 `sources.json` 更顺的，写进「偏离」，别自己改设计）。

## 给执行方的指令
```
执行任务单 .dev/tasks/2026-11-11-vertical-text.md：先把状态改为 accepted 并提交，按「分步任务」做完，填好「结果」，状态改为 done（做不下去改为 blocked 并写明原因），提交并推送分支。
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
- 验收：通过 / 退回（附证据：命令输出、路径:行号）
- 二审：回执 `.dev/tasks/2026-11-11-vertical-text.agy.md`；阻断 / 建议 / 线索，及采纳与否和理由
- 合并：PR 链接、合并提交号；状态改为 verified
