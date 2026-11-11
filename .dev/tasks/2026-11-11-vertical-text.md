# 竖排第一期：Kobo、汉王的中文文字书竖排（引号转直角、西文数字转全角竖立）

- Task ID: 2026-11-11-vertical-text
- 负责 bot: sheng-ren
- 状态: verified
- 执行方: opencode-local
- 分支: vertical-text（从最新 master `10ce515` 或更新的切出）
- 更新时间: 2026-11-11T17:40+08:00
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
- 执行方 / 开始–结束时间：opencode-local / 2026-11-11 14:20–17:05 +08:00
- base..head：`15abbea..HEAD`（`15abbea` 是分支切出的 master）
- 提交：

  | 步骤 | 提交号 | 文件 | 说明 |
  | --- | --- | --- | --- |
  | 接受 | `74bc0ed` | 任务单 | 状态改 accepted |
  | 1 | `454b8cb` | `crates/profile/src/lib.rs` | `vertical_text`、`vertical_ascii` 两个字段、校验、测试 |
  | 2–9、11 | `cb12f7e` | 见 `git show --stat cb12f7e`（31 个文件） | 判定、引号、全角、竖排样式、优化器 / 书库接入、版本、金样、文档 |
  | 10 | `b19a963` | `kobo.toml`、`profile/src/lib.rs`、`optimize/tests.rs`、`fp_golden.txt` | Kobo 注释弹窗（规则 7，单独提交） |
  | 格式 | `e83635c` | 10 个改过的文件 | `cargo fmt` |
  | 结果 | 本提交 | 任务单 | 填结果、状态改为 done |

- 实现（对应分步任务）：
  1. **profile**：`vertical_text: bool`（缺省 false）、`vertical_ascii`（`keep` / `fullwidth`，留了 `tcy` 枚举位但 `validate` 报「未实现」）。`vertical_ascii != keep` 必须开 `vertical_text`。`--print-profile` 直接输出。
  2. **判定**（`crates/bookconv/src/vertical.rs`）：`is_zh_text` 按 spine 抽前 20000 个非空白字；`zh*` 时汉字 ≥ 30%，否则汉字 ≥ 60% 且假名 < 2%，假名 ≥ 2% 一律不是；固定版式不算。`is_fixed_layout` 认 `rendition:layout=pre-paginated` 和 `fixed-layout=true`。`is_natively_vertical` 认外链 CSS、`<style>`、`html`/`body` 行内样式里的 `writing-mode: vertical-*`（含 `-webkit-`/`-epub-` 前缀），表格规则不算。
  3. **引号**：按段落成对、嵌套合法才转；弯引号 “”→『』、‘’→「」；引号里全是西文不转；ASCII 直引号先认撇号（前后 ASCII 字母/数字），余下按出现顺序成对；落单的留给全角；`code`/`pre`/`script`/`style`/属性不转；`&quot;`/`&apos;`/`&#34;`/`&#39;` 先还原再配对；目录 nav、`toc.ncx` 的 `<text>` 同正文。
  4. **全角**：正文文字节点 U+0021–U+007E → +0xFEE0；空格不转；`head`/`code`/`pre`/`kbd`/`samp`/`script`/`style`/`svg`/`math`/`rt`/`rp`、注释、属性不动；字符引用整体认出来，`&amp;`→＆、`&lt;`→＜、`&#x41;`→Ａ，`&#x4E2D;` 原样。
  5. **竖排样式**：`uastyle::ua_css_vertical`（段距写 margin-left/right、块缩进 margin-top、列表 padding-top）；`eink-vertical.css`（root writing-mode、`p` 左右段距、表格横排）；书自己的 CSS 盒子横转竖（top→right…、width↔height、float 去掉；绝对定位 / 负边距整条不动、计数）。`WashOpts.vertical` 让 `kindle_images`、`blank_fold`、`block_style`、`top_spacer`、`break_margin`、`box_fix`、`scale_vertical`、`scale_horizontal`、`root_em` 跳过。
  6. **优化器**：`OptimizeOpts.vertical` / `vertical_ascii`，`set_vertical()`；只修复的 `html_prefix` 两个出口都接「注释 → 引号 → 全角」；`transform_ncx`；OPF 写 spine rtl + `primary-writing-mode`（按书指定方向时方向以书为准，只补 meta）；`epub-optimize --vertical`。
  7. **KEPUB**：只加了一个测试（全角串、`rt`/`rp`、`eink-vertical.css` link），`kepub.rs` 产物逻辑没改，`VERSION` 仍是 2。
  8. **书库**：`Meta.zh_text`、`Meta.layout.vertical`；`vertical.toml`（`on`/`off`，最长前缀胜）；`vertical_for` 按「书 → 目录清单 → 自动判定」，漫画 / 固定版式 / 设备没开 `vertical_text` 一律不竖排；`booklib vertical --on|--off|--auto`、补全、`list` 每台显示 `竖排` / `横排`、`devices` 标出竖排能力。
  9. **开台 + 版本 + 金样**：`kobo.toml`、`hanvon.toml` 写 `vertical_text = true`、`vertical_ascii = "fullwidth"`；`bigme.toml` 显式 `vertical_text = false`（xReader 不竖排、KOReader 也不会）。`OPTIMIZE_VERSION` 67→68，历史说明；`COMIC_VERSION` 按单子不动。`fp_golden.rs` 加 `vertical_text`、`vertical_ascii` 变异组。
  10. **注释弹窗**：`kobo.toml` 的 `notes = "popup"`、`repair_note_links = true`；单测覆盖 Kobo 横排/竖排都是 `<aside epub:type="footnote">` + `noteref`，汉王仍不写 aside。
- 测试：

  | 命令 | 结果 | 摘要 |
  | --- | --- | --- |
  | `cargo test --workspace` | 通过 | 全部通过（bookconv 552、library 71+7+14+47+1、profile 19、kfx 53、csscascade 16） |
  | `cargo clippy --workspace --all-targets -- -D warnings` | 通过 | 0 警告 |
  | `cargo fmt --all -- --check` | 通过 | 干净 |
  | `cargo test -p bookconv vertical` | 通过 | 判定、引号、全角、NCX、CSS 横转竖、竖排 OPF/link、原书竖排不写 root、`--vertical` 开关、Kobo 弹窗 |
  | `cargo test -p library vertical_books` | 通过 | Kobo / 汉王自动竖排、Bigme / Kindle 不竖排、单本 `--off/--auto`、目录 `--off` 和继承、设备没开时强制 `--on` 也不竖排 |
  | `booklib vertical --off/--auto`（临时书库） | 通过 | 写出 / 删掉这本书的覆盖，`list` 能读 |
  | `UPDATE_FP_GOLDEN=1` + 复跑 | 通过 | 见下面 |
- `fp_golden`：
  - 新增 `vertical_text`、`vertical_ascii` 两组变异，每台 4 行，共 20 行（放在最后，旧行不挪）。
  - Kobo、汉王的中文书（样例文字书是中文）旧行 `info` 段多 `Vf`；Kobo 文字书注释段 `jump#`→`popup#`、段末多 `n`；Kobo 漫画行注释段也跟着变（漫画产物本身逐字节不变）。
  - 其余设备（Bigme、Kindle、掌阅）旧行除版本号 `67→68` 外一字不变。
- 合成回归（`/tmp/opencode/vertical/reg/`，不提交）：三本最小 EPUB（中文书含“”‘’、直引号、撇号、`2026年`、`KOReader`、`US$6`、表格、一条注释、`<code>`、带引号和西文的目录；英文书；3 页漫画）。
  - 中文书 `epub-optimize --device=kobo --vertical`：OPF 有 `<meta name="primary-writing-mode" content="vertical-rl"/>`、`<spine page-progression-direction="rtl">`；每章 link 顺序 `eink-ua.css` → 书 CSS → `eink-vertical.css`（最后）；正文 `他说『好』，２０２６年与ＫＯＲｅａｄｅｒ，ＵＳ＄６，ｄｏｎ＇ｔ。`、`她说『直引号』和「单引号」，１２＂ ５＇。`，`<code>code 123</code>` 不动；nav / ncx 的 `<text>` 为 `第１章 『标题』 ＯＫ`，属性不动；Kobo 注释是 `<aside epub:type="footnote">` + `noteref`。汉王中文书同样竖排，但注释仍是跳转。
  - 与 master `15abbea` 的 `epub-optimize` 二进制对比：**Bigme、Kindle、掌阅三本全部逐字节相同；汉王上的英文书和漫画逐字节相同；Kobo 上的英文书（无注释）和漫画逐字节相同**。中文书 Kobo / 汉王按 `--vertical` 才竖排（`epub-optimize` 不自动判定，书库才判定），和 master 不同是预期。
  - 字符账：中文书产物把全角 ASCII 还原、『』「」按引号还原后，字符多重集合和原书**相同**（105 == 105）；差异只有引号、全角，以及注释块搬到了章末。
- 偏离：
  - 新增 `crates/library/src/vertical.rs`（书库侧判定、`vertical.toml`、`booklib vertical` 的实现）和 `crates/library/Cargo.toml` 的 `toml` 依赖；任务单允许「按需新增书库配置读写」，但没在「涉及文件」里点名这个模块。
  - `crates/library/src/lib.rs` 的 `Meta` 加了 `zh_text`、`layout` 两个字段（`layout.vertical`），`sources.rs` 加了 `root_dir()`；`flow.rs` 加了一个综合用例。
  - 规则 7（注释弹窗）的配置、单测、金样在 `b19a963`，但 `docs/decisions.md`、`docs/devices.md`、`docs/typesetting.md`、`docs/kepub.md` 里关于弹窗的说明在 `cb12f7e` 里一起写了；要完全单独回退 `b19a963` 时还得手工去掉这几处文档。原因：先写文档再拆提交，漏拆了文档。
  - `epub-optimize --vertical` 不会自动判定「是不是中文书」，只按 flag 强制处理（任务单规则 9 就是这么写的）；书库生成时由 `vertical_for` 判定。
- 未完成 / 待真机：
  - **没跑真书回归、没上真机**。真机由 vinci 验：Kobo、汉王各 sync 一本真书（对话多、有注释、有西文），看竖排、点左侧下一页、引号『』「」、数字西文立着不叠字、注释能点开（Kobo 弹窗）、目录能跳且目录里的西文是全角；Kobo 上一本有注释的横排书也弹窗；同一本在 KOReader 里能读（横排退化、引号/段距可接受）；Bigme、Kindle 上同一本仍横排。
  - KEPUB 只在单测里验证了全角串 / `rt`/`rp` / 样式表链接不被 koboSpan 拆坏，没有在真机 Nickel 上验竖排书。
  - 汉王「1–11 正常」是任务单给的实机结论；本单没在汉王上逐项复验。

### 返工（2026-11-11，一审退回后）

- 状态先改回 accepted（`398ad5f`），返工提交 `f127d66`。
- **阻断 1（原书竖排的书 CSS 也被横转竖）**：`wash::apply_vertical` 里 `native` 为真时跳过整段书 CSS 改写；补测试 `natively_vertical_book_does_not_get_root_writing_mode_rule` 断言 `s.css` 逐字节不变、`eink-vertical.css` 不写根元素 writing-mode。
- **阻断 2（读不到原件时把「不是」永久写进 meta.json）**：`zh_text_of` 读失败时这次按不竖排算，但不进缓存、不写 meta.json；补测试 `unreadable_source_does_not_cache_not_chinese`（模拟早期条目 `zh_text=None`、删掉原件、读一次、再读 meta.json 仍为 `None`）。
- **建议 1（入库时判 zh_text）**：`EpubInfo::read` 里判一次，EPUB 写进 `Meta.zh_text`；CBZ 记 `false`。`add_file` 的测试断言 `m.zh_text == Some(true)`。
- **建议 2（弯撇号）**：前后都是 ASCII 字母/数字的 `’` 按撇号算，不进配对；补测试 `他说“don’t worry”就走了，“这里是中文”。` → 第一个全西文弯引号不转、第二个转成『这里是中文』。
- **建议 3（vertical 逐本写只看 kobo/hanvon）**：改成遍历所有 `vertical_text = true` 的设备。
- **建议 4（vertical 补全）**：新增 `__complete vertical`，按 `paths` 的规则只列已跟踪条目、它下面的子目录和书；fish 脚本改用它，补测试。
- **建议 5（三段式 border / 圆角）**：书 CSS 横转竖补 `border-top-width`、`border-left-style`、`border-bottom-color`、`border-top-left-radius` 等；补测试。
- **建议 6（`0c55ca7` 没有正文、规则 7 文档混在 `cb12f7e`）**：`0c55ca7` 已在远端、没有改写历史；规则 7 的文档仍在 `cb12f7e`。可以回退 `b19a963` 的配置 / 单测 / 金样，但文档要手工改回。要彻底一刀回退得重写分支历史，**没做**，留给维护者决定。
- **线索 1–6 不要求本次改，未动**（`p` 左右段距是否会盖过书里 `p{margin:0}`、表格里的全角数字、Kobo 空注释页、`--no-wash --vertical` 不挂样式、`@media` 内规则、Kobo 漫画指纹）。
- 返工后测试：`cargo test --workspace` 全过（bookconv 552、library 71+7+14+48+1、profile 19、kfx 53、csscascade 16），`cargo clippy --workspace --all-targets -- -D warnings` 0 警告，`cargo fmt --all -- --check` 干净。
- **仍然没跑真书回归、没上真机**；真机验证项同上面「未完成」。

### 二审后返工（2026-11-11，agy 二审无阻断，三条建议 + 一条线索）

- 状态改回 accepted（`5b3da84`），返工提交 `4aacf74`。
- **`!important` 简写**：`rotate_box_values` 先剥掉末尾 `!important`（大小写、空白都容忍），旋转后接回；补 1/2/3/4 值各带 `!important` 的测试，不再出现 `margin: !important 1em` 这种非法写法。
- **原生竖排判定**：`is_natively_vertical` 只认作用在 `html` / `body` / `:root` 上的 `writing-mode`（可带类、属性、伪类，如 `body.v`、`html[lang]`）；选择器按最右一节**整词**比元素名，不再用 `contains("rt")` 子串，`.article`、`.part` 之类局部类不会被误排除或误判全书竖排，`.signature` 这类局部竖排也不算。补 `.article` / `body.v` / `html table` 测试；台版合成书（`html{writing-mode:vertical-rl}`）仍判原生竖排、CSS 逐字节不变。
- **实体识别**：全角里 `&` 后只有 `[A-Za-z0-9#]` 开头、其后全 `[A-Za-z0-9]` 直到 `;`（≤32 字符）才当实体；遇空白、标点立即当普通 `&` 转 `＆`。`Tom & Jerry; 2026` → `Ｔｏｍ ＆ Ｊｅｒｒｙ； ２０２６`，`&amp;`、`&#x4E2D;` 仍整体保留。
- **英寸号**：数字后的 `"` 后面不是字母/汉字时当英寸号，不参与直引号配对；`12" x 14"` → `１２＂ ｘ １４＂`，`他说"好"` 仍是『好』。
- `border-radius` 多值简写、规则 7 文档拆提交、Kobo 空注释页、`eink-vertical.css` 段距：按二审「不修」或后续单处理，未动。
- 返工后测试：`cargo test --workspace` 全过，`cargo clippy --workspace --all-targets -- -D warnings` 0 警告，`cargo fmt --all -- --check` 干净。
- **仍然没跑真书回归、没上真机。**

## 审查
<!-- 负责 bot 填写 -->
- 验收：**退回**（sheng-ren，2026-11-11，box 复跑 `0c55ca7`），状态改回 pending，同一执行方返工；修完重新验收，再交 agy 二审。
  - 复跑：`cargo test --workspace` 全过，共 780 个（bookconv 552、csscascade 16、kfx 53、library 71+7+14+47+1、profile 19），0 failed；`cargo clippy --workspace --all-targets -- -D warnings` 0 警告；`cargo fmt --all -- --check` 干净。
  - 金样（按 设备/变异/类别 对键比，`|67|→|68|` 抹平）：旧行 640 行一行不少，新增 20 行（`vertical_text`、`vertical_ascii` 各 10）。Bigme、Kindle、掌阅 384 行一字不变；汉王漫画 64 行不变，汉王文字书 64 行只有 info 段 `-`→`-Vf`（样例书是中文，预期）；Kobo 漫画 64 行只有注释段 `jump`→`popup`；Kobo 文字书 64 行是 `-Vf` + 注释段 + 文字书段多 `n`（`text_repair_only`、`repair_note_links` 两个变异行按各自的值少 `n`）。另：新增行不在文件末尾，而是接在各设备那一段后面（第 129 行起），「结果」写的「放在最后」不准确，不影响对比。
  - 逐字节（同一输入，master `15abbea` 与本分支的 `epub-optimize` 各跑一遍，5 本合成书 × 5 台＝25 组）：Bigme、Kindle、掌阅、汉王 20 组全部 `cmp` 相同（汉王不带 `--vertical`，即横排）；Kobo 5 组里漫画、无注释英文书、原书竖排的书（没注释）相同，中文书和有注释的英文书不同，`diff -r` 看只差在注释改弹窗（`noteref` + 章末 `<aside epub:type="footnote">`）——符合规则 7。
  - 竖排产物（合成中文书，`--device=kobo|hanvon --vertical`）：OPF `primary-writing-mode=vertical-rl`、`<spine page-progression-direction="rtl">`；每章 link 顺序 `eink-ua.css` → 书 CSS → `eink-vertical.css`（nav 也挂）；`eink-vertical.css` 根元素 writing-mode（三种写法）、表格 `horizontal-tb`；正文 `他说『你好，「朋友」。』`、`他说『好』，又说「甲」和「乙」，『外「内」外』`、`Ｉ ｄｏｎ＇ｔ`、`９０＇ｓ`、`ｒｏｃｋ＇ｎ＇ｒｏｌｌ`、`１２＂ 宽`、`５＇`、`２０２６年`、`ＫＯＲｅａｄｅｒ`、`ＵＳ＄６`、`Ｈｅｌｌｏ ｗｏｒｌｄ`（空格没转）、`Ａ＆Ｂ ＜ｔａｇ＞ Ａ&#x4E2D;`；`<code>`、`<rt>`、`href`、`alt`、`id` 都没动；`toc.ncx`（含 `docTitle`）和 nav 都是 `第１章 『标题』 ＯＫ`、`第２章 『Ｑ＆Ａ』 ｄｏｎ＇ｔ`；Kobo 注释是弹窗写法，汉王仍是跳转；书 CSS 横转竖 `p{margin:1em 0}`→`margin:0 1em`、`img{max-width}`→`max-height`、`.box` 的 margin/padding/border/width 按规则转、`float` 去掉、`position:absolute` 的规则原样。转出的 KEPUB（`epub-to-kepub`）koboSpan 不拆全角串、不进 `rt`，全部 XHTML/NCX/OPF 能被 XML 解析。
  - 提交：中文、前缀齐全、无 Co-Authored-By、没动 master、没合并；没碰 `install.sh`、密钥；`Meta` 两个新字段都是可选（`serde(default)`），旧 meta.json 照读，不算数据迁移。clean-room：diff 里没有 GPL 转换器来源的代码或引用（只在注释里提到 KindleGen 的公开元数据约定 `fixed-layout`）。
  - **阻断**
    1. 原书本来竖排的书，书自己的 CSS 也被「横转竖」了，违反规则 2、4（「原书本来竖排的不转」）。`crates/bookconv/src/wash/mod.rs:1072` 算了 `native`，但 `:1075` 起的改写循环不看它，只有 `vertical_css(native)` 用到。实测（合成台版书，`html{writing-mode:vertical-rl}` + `p{margin:0 1em}` + `.box{margin-top:2em;width:30%}`，Kobo、汉王都一样）：产物变成 `p{margin:1em 0}`、`.box{margin-right:2em;height:30%}`，本来对的竖排段距被转成每列首尾空白。要改：`native` 时跳过整段改写；补一个测试（原书竖排的书 CSS 逐字节不变）。
    2. 中文判定读不到原件时，把「不是」**永久**写进 meta.json。`crates/library/src/vertical.rs:94` `self.is_zh_text(meta).unwrap_or(false)` 之后照样 `note_zh_text` 并在持锁时 `save_meta`（`:95-104`）：原件暂时不在（移动硬盘没插、原件改过名）时跑一次 `sync`，这本中文书以后就一直不竖排，只能手工 `--on`。`comic_of` 遇到这种情况是返回错误、不存。要改：读不出来时这次按不竖排算，但不进缓存、不写 meta.json（或照 `comic_of` 返回错误，二选一，写清楚理由）；补测试。
  - **建议**
    1. 规则 2 要求「入库时判一次存进 `Meta`」，现在只在第一次用到时判（`crates/library/src/catalog.rs:212` 只存了 `comic`）。和阻断 2 一起改：入库时顺手判 `zh_text`。
    2. 弯引号段里有弯撇号（`don’t`、`it’s`，中文书里夹英文很常见）就整段配不上，同段别的“”也不转。实测 `他说“don’t worry”就走了，“这里是中文”。` 整段原样（`crates/bookconv/src/vertical.rs:365-378`）。建议：前后都是字母或数字的 `’` 照直引号的规则当撇号，不进配对。这条不算改规则，只是把 vinci 对直引号定的撇号口径用到弯引号上；拿不准就先列给我。
    3. `booklib vertical <目录>` 逐本写书时只看 `kobo`、`hanvon` 两台的跟踪（`crates/library/src/vertical.rs:174-175` 写死设备 id）：书库 `profiles/` 里的自定义模式、或只跟踪给别台的目录会漏。建议改成遍历开了 `vertical_text` 的设备。
    4. 补全：`vertical` 用的是 `__complete tracked`（`crates/library/src/bin/booklib/complete.rs:206`），只列跟踪条目本身，不列它下面的子目录和书；任务单要的是「`paths` 的规则、但只列已跟踪的」。
    5. 书 CSS 横转竖漏了 `border-top-width`、`border-left-style` 这类三段式属性和圆角（`crates/bookconv/src/vertical.rs:608` 按最后一个 `-` 拆，`width`/`style`/`color` 不是方向），补上并加测试。
    6. `0c55ca7`（改了代码）没有正文，也没写进「结果」的提交表；规则 7 的文档混在 `cb12f7e` 里（执行方已在「偏离」里说明）。返工时把弹窗的文档挪进单独的提交，保证规则 7 能一刀回退。
  - **线索**（不要求本次改，留给二审和真机）
    1. `eink-vertical.css` 的 `p{margin-left:1em;margin-right:1em}`（`crates/bookconv/src/vertical.rs:526`）挂在最后，会盖过书里写的 `p{margin:0}`：多数中文书本来没有段距、只缩进两格，竖排后都会多出一格段距。这是任务单「方法 a」照字面做的结果，真机看是否太松。
    2. 表格整张横排，但里面的西文、数字也转了全角（`项目 Ｉｔｅｍ`、`数值 ４２`），横排里看着稍宽，不影响阅读。
    3. Kobo 开 `repair_note_links` 后，只放注释的那一章被搬空，剩一个空页（合成英文书的 `c1.xhtml` 只剩空 `<body>`），这是 xochitl 一直以来的行为，Kobo 上第一次出现，真机留意。
    4. `epub-optimize --no-wash --vertical` 只改文字、不挂竖排样式表（`set_vertical` 只在有 `wash` 时同步），出测试书时别这样组合。
    5. `rewrite_css` 跳过 `@` 开头的规则，`@media` 块里的规则有没有转：未核实。
    6. 规则 7 让 Kobo 漫画的指纹也变（注释段），全部 Kobo 漫画会重建；产物逐字节不变，不会重传。
- 第二轮验收：**通过**（sheng-ren，2026-11-11，box 复跑 `727b4b6`，返工提交 `f127d66`）。两条阻断都已修好且各有复现；状态保持 done，交 agy 二审。
  - 复跑：`cargo test --workspace` 全过，共 781 个（与一审同口径：bookconv 552、csscascade 16、kfx 53、library 71+7+14+48+1、profile 19；另 bookbase 30），0 failed；`cargo clippy --workspace --all-targets -- -D warnings` 0 警告；`cargo fmt --all -- --check` 干净。
  - 金样（同一审方法，对键比、`|67|→|68|` 抹平）：旧行 640 行一行不少，新增 20 行；Bigme、Kindle、掌阅 384 行一字不变；汉王漫画 64 行不变、汉王文字书 64 行只有 `-`→`-Vf`；Kobo 漫画 64 行只有注释段；Kobo 文字书 63 行 `-Vf` + 注释段 + `n`、1 行 `-Vf` + 注释段。与一审完全一致，返工没有动到指纹。
  - 逐字节（master `15abbea` 与 `727b4b6` 的 `epub-optimize`，5 本合成书 × 5 台＝25 组）：23 组 `cmp` 相同；只有 Kobo 的中文书、有注释的英文书不同，`diff -r` 仍只差在注释改弹窗。与一审一致。
  - **阻断 1 复现（已修）**：合成台版书（`html{writing-mode:vertical-rl}` + `p{margin:0 1em}` + `.box{margin-top:2em;width:30%}`）`--device=kobo|hanvon --vertical`：两台产物的 `OEBPS/style.css` 与原件逐字节相同（一审时是 `p{margin:1em 0}`、`.box{margin-right:2em;height:30%}`）；`eink-vertical.css` 不写根元素 writing-mode，只有表格 `horizontal-tb` 和 `p` 段距。修在 `crates/bookconv/src/wash/mod.rs:1075`（`if !native`），测试 `crates/bookconv/src/optimize/tests.rs:3606` 断言 `s.css` 不变。
  - **阻断 2 复现（已修）**：临时加一个持锁测试（入库后把 meta.json 的 `zh_text` 改成没有、删掉原件、`lib.lock()` 后 `zh_text_of`）：在一审时的 `18f2435` 上失败（meta.json 被写成 `Some(false)`），在 `727b4b6` 上通过；同一个 `Library` 把原件放回来再判一次得 `true` 并写进 meta.json（进程内也没缓存成「不是」）。修在 `crates/library/src/vertical.rs:96`。执行方补的测试 `crates/library/tests/flow.rs:3126` 原来**不持锁**，而只有持锁时才写 meta.json，测不到要防的路径；验收时顺手改为持锁（`be85920`，测试 2 行，不动代码）。
  - 竖排产物复查（合成中文书，Kobo、汉王 `--vertical`）：与一审产物 `diff -r` 只差弯撇号那一段，其余（OPF、样式表顺序、全角、引号、ncx/nav、注释写法、`code`/`rt`/属性不动）都和一审一致。弯撇号段现在是 `他说“ｄｏｎ’ｔ ｗｏｒｒｙ”就走了，『这里是中文』。`（全西文的弯引号不转，第二对转『』），两台一样。
  - 一审建议逐条：
    1. 入库时判 `zh_text`：已改，`crates/library/src/lib.rs:329` 在 `EpubInfo::read` 判，`crates/library/src/catalog.rs:214` 存进 `Meta`，CBZ 记 `false`（`:184`）。
    2. 弯撇号：已改（`crates/bookconv/src/vertical.rs:358`），实测见上。
    3. `vertical` 逐本写不再写死 kobo/hanvon：已改为遍历 `vertical_text = true` 的设备（`crates/library/src/vertical.rs:175`）。
    4. 补全：已加 `__complete vertical`（`crates/library/src/bin/booklib/complete.rs:46`、`:146`），fish 脚本改用它，有测试。
    5. 三段式 border、圆角：已改（`crates/bookconv/src/vertical.rs:612`），角的映射核对过（左上→右上……）；顺带实测 `@media` 块里的规则也会转，一审线索 5 可以关。
    6. 规则 7 一刀回退：没改，理由是 `0c55ca7` 已推送、不改写历史；回退要 revert `b19a963` 再手工改回 `cb12f7e` 里的文档。理由成立，接受；合并时用 merge 提交保留这几个提交，别 squash。
  - **阻断**：无。
  - **建议**（不挡二审）
    1. `border-radius` 的多值简写不转：`border-radius:1px 2px 3px 4px` 原样输出，竖排后应为 `4px 1px 2px 3px`（`crates/bookconv/src/vertical.rs:612` 只处理四个角的长写法；`border-width/style/color` 的四值简写已经会转）。中文书里少见，可以和下一轮竖排一起补。
  - **线索**
    1. `crates/library/src/vertical.rs:95` 注释写「agy 一审阻断 2」，实际是负责 bot 验收的阻断 2；`crates/library/src/lib.rs:113` 的 `zh_text` 文档仍写「入库后第一次用到时判」，现在新入库时就判，只有早期条目才是第一次用到时判。只是注释。
    2. `vertical` 补全也会列出已跟踪条目的上级目录（`crates/library/src/bin/booklib/complete.rs:149` 的 `t.starts_with(&cand)`），方便一路补下去；对上级目录跑 `booklib vertical` 会记进 vertical.toml，作用到下面所有已跟踪的书，符合规则 3。
    3. 读不到原件时 `zh_text_of` 每次都会再试着打开一次（不缓存是故意的），代价可以忽略。
    4. 一审线索 1–4、6 不变，留给二审和真机。
- 二审：回执 `.dev/tasks/2026-11-11-vertical-text.agy.md`；阻断 / 建议 / 线索，及采纳与否和理由（返工、重新验收通过后再跑）
- 合并：三次验收通过后，本地 `git merge --no-ff vertical-text` 合进 master（没走 PR），合并提交的说明为「merge: 竖排第一期（Kobo、汉王中文文字书竖排，Kobo 注释弹窗）」；状态 verified

### 二审后（sheng-ren，2026-11-11，据 `.dev/tasks/2026-11-11-vertical-text.agy.md`）

agy 二审无阻断。下面三条是 agy 的「建议」，但会让真书产物出错，合并前先修（返工第二轮）；其余留作后续。

**本轮要修**
1. **`!important` 简写旋转破坏语法**（agy 建议 1，`crates/bookconv/src/vertical.rs:668-679`）：`rotate_box_values` 前先剥掉末尾 `!important`（大小写、空白都容忍），旋转后再接回。补测试：1、2、3、4 值各带 `!important`。
2. **原书竖排判定**（agy 建议 2，`crates/bookconv/src/vertical.rs:181-198`）：
   - 排除 `table/img/svg/ruby/rt/rp` 改成按选择器里的**元素名**匹配（按组合符和 `.#[:` 切开后比整词），不再用子串；`.article`、`.part`、`.start` 不能被排除。
   - 只有作用在 `html`、`body`、`:root`（可带类/属性，如 `body.vrtl`、`html[lang]`）上的 `writing-mode: vertical-*`，或写在 `<html>`/`<body>` 的 `style` 属性上的，才算全书原生竖排；局部类（`.signature`、`.seal`）写了竖排不算。
   - 补测试：`.article{writing-mode:vertical-rl}` 单独出现不算原生；`body.v{writing-mode:vertical-rl}` 算；台版合成书仍算原生且 CSS 逐字节不变。
3. **实体识别**（agy 建议 3，`crates/bookconv/src/vertical.rs:458-477`）：`&` 后只有 `[A-Za-z0-9#]` 开头、其后全是 `[A-Za-z0-9]` 直到 `;`（≤32 字符）才当实体；遇空白等其他字符即不是实体，`&` 照普通字符转 `＆`。补测试：`Tom & Jerry; 2026` → `Ｔｏｍ ＆ Ｊｅｒｒｙ； ２０２６`，`&amp;`、`&#x4E2D;` 仍整体保留。
4. **英寸双引号**（agy 线索 1，`crates/bookconv/src/vertical.rs:410-428`）：`"` 紧跟在数字后、且后面不是字母汉字（是空白、标点、`x`/`×` 前的空白或段尾）时当英寸号，不参与配对，转 `＂`。补测试：`12" x 14"` → `１２＂ ｘ １４＂`；`他说"好"` 仍是『好』。

**不修，留给后续**
- agy 建议 4（弹窗文档混在 `cb12f7e`）：合并用 `--no-ff`，回退时手工修文档。
- agy 建议 5（`border-radius` 多值简写）：中文书少见，后续单。
- agy 线索 2、3（注释章搬空、`eink-vertical.css` 段距）：真机验收时看。

修完把「结果」的返工小节补上本轮提交，状态改 done。

### 三次验收（sheng-ren，2026-11-11，box 复跑 `1091195`，返工提交 `4aacf74`）

结论：**通过**，状态改 verified，`--no-ff` 合并进 master。验收中发现 `4aacf74` 的实体识别有一处死循环，属于小改动（一个文件，产物不变），已由负责 bot 自己修掉（`8eb0bc5`），见下文第 3 条。

- 复跑（含 `8eb0bc5`）：`cargo test --workspace` 全过，共 782 个（和前两轮同口径：bookconv 553、csscascade 16、kfx 53、library 71+7+14+48+1、profile 19；另 bookbase 30），0 failed；`cargo clippy --workspace --all-targets -- -D warnings` 0 警告；`cargo fmt --all -- --check` 干净。
- 金样：`fp_golden.txt` 这轮没改；按键对比 master，`|67|→|68|` 抹平后，结果和二审前完全一样：旧行 640 行一行不少，新增 20 行；Bigme、Kindle、掌阅 384 行不变；汉王文字书只有 `-Vf`；Kobo 只差注释段（文字书另有 `-Vf`、`n`）。
- 逐字节（master `15abbea` 对比本分支，5 本合成书 × 5 台＝25 组）：23 组相同，Kobo 的中文书、有注释的英文书两组只差注释弹窗；25 份产物和二次验收时（`727b4b6`）逐字节相同。合成中文书、台版书在 Kobo、汉王 `--vertical` 下的产物，和 `727b4b6` 的 `diff -r` 没有任何差异：这轮四处改动以外的行为都没变。
- 逐条：
  1. **`!important`**（`crates/bookconv/src/vertical.rs:727-750`）：`important_survives_box_shorthand_rotation` 通过。另外实测 `margin:1em 2em!important` → `2em 1em !important`，`padding:1px 2px 3px 4px !IMPORTANT` → `4px 1px 2px 3px !important`。
  2. **原书竖排判定**（`crates/bookconv/src/vertical.rs:181-221`，只看选择器最右一节的元素名是不是 `html`/`body`/`:root`；`style` 属性仍只认 `<html>`/`<body>`，见 `:168`）：`native_vertical_is_detected`（`.article`、`body.v`、`html table`）和 `natively_vertical_book_does_not_get_root_writing_mode_rule` 都通过。合成书 Kobo、汉王 `--vertical` 实测：
     - 只有 `.signature{writing-mode:vertical-rl}` 的横排书：不算原生竖排，挂 `eink-vertical.css`（根元素三种 writing-mode），书 CSS 横转竖（`p{margin:1em 0}`→`margin:0 1em`），OPF `primary-writing-mode=vertical-rl`、spine `rtl`；
     - 只有 `.article{writing-mode:vertical-rl}` 的书：同上；
     - `body.v{writing-mode:vertical-rl}`：算原生竖排，书 CSS 不动，`eink-vertical.css` 不写根元素；
     - 台版书（`html{writing-mode:vertical-rl}`）：两台的 `style.css` 都和原件 `cmp` 相同。
  3. **实体识别**（`crates/bookconv/src/vertical.rs:488-535`）：`fullwidth_text_only_and_entities` 通过。实测 `A&amp;B &lt;x&gt; &#x4E2D; &#65; &nbsp; Tom & Jerry; R&D a&b` → `Ａ＆Ｂ ＜ｘ＞ &#x4E2D; Ａ &nbsp; Ｔｏｍ ＆ Ｊｅｒｒｙ； Ｒ＆Ｄ ａ＆ｂ`。
     - **验收发现并已修**：`4aacf74` 遇到 `&` 后连续 32 个字母数字、又没有 `;`（如 `&abcdefghijklmnopqrstuvwxyz0123456789 x`）时，while 循环既不前进也不输出，`fullwidth_ascii` 卡死（临时测试 150 秒超时）。走 `epub-optimize` 时这种裸 `&` 会先被「规范整理」转成 `&amp;`（合成书正文、NCX、`--no-wash` 都试过，不会卡），所以现在还碰不到，但 `fullwidth_ascii` 是公开函数，一旦前面漏掉一处转义就会卡死。`8eb0bc5` 改成按「有没有吃掉实体」判断（`:496`、`:529`），并补了断言；修完后 20 个竖排产物（10 本合成书 × Kobo/汉王）和 `4aacf74` 逐字节相同。
  4. **英寸号**（`crates/bookconv/src/vertical.rs:434-449`）：`straight_quotes_and_apostrophes`（`12" x 14"` → `１２＂ ｘ １４＂`）通过。实测 `他说"好"` → 『好』，`屏幕 6"，重 200g，"好用"` → `６＂ … 『好用』`，`5'11" tall` → `５＇１１＂ ｔａｌｌ`。
- **阻断**：无（上面第 3 条的死循环已修）。
- **建议**（不挡合并，后续单）
  1. `! important`（`!` 和 `important` 之间有空格，CSS 允许）没剥出来：`border-width:1px 2px ! important` 会转成 `important 1px 2px !`（`crates/bookconv/src/vertical.rs:730` 只认紧挨着的 `!important`）。真书里很少见。
  2. 英寸号的判定按单子写的口径，会误伤「数字结尾的引语后面紧跟标点」：`他说"1984"。` → `他说＂１９８４＂。`（不再转『』）；后面是汉字时（`"1984"是本好书`）没问题。可以改成直引号配不上对时才把它当英寸号，要不要改等 vinci 定。
- **线索**
  1. 局部竖排的类（`.signature{writing-mode:vertical-rl;margin-top:2em}`）现在也会跟着横转竖，变成 `margin-right:2em`。它本来就是竖排元素，转了方向反而不对；很少见，真机留意。
  2. 前两轮的建议和线索仍按「二审后」小节的决定，留给后续：`border-radius` 多值简写、注释那一章被搬空、`eink-vertical.css` 段距、规则 7 的文档混在 `cb12f7e`。
