//! 清洗层（规则最初对照上游 Calibre 清洗脚本移植）。作用于**解包后的条目表**，由优化器在各遍处理之前调用
//! （`OptimizeOpts::wash`），书库生成与命令行 `epub-optimize` 走同一份。
//!
//! 规则（与 Calibre 参数一一对应）：
//! 1. 伪 DRM 剥离（= `strip_pseudo_drm.py`）：`META-INF/encryption.xml` 只列样式/字体/脚本 → 丢弃这些文件 +
//!    encryption.xml + OPF manifest 项；列了正文/图片/导航 = 真 DRM → **报错停下**。
//! 2. 字体字号解锁（独立 .css、`<style>`、`style=""` 三处，规则见 `crate::cssunlock`）：`font-family`、`line-height`、
//!    绝对字号去掉，相对字号保留（正文整体那一层的除外），`font` 简写只留粗斜体，`background` 简写只留颜色
//!    （背景图去掉：xochitl 把背景图平铺满页盖住正文，真机《飘》）。颜色、对齐等别的样式不动（用户 2026-09-29）。
//! 3. 边距归零（= `--margin-* 0`）：body/html/@page 的 margin/padding 删掉（不另注入规则）。
//! 4. 段距归零 + 首行缩进（= `--remove-paragraph-spacing --remove-paragraph-spacing-indent-size 2`）：p/div 的
//!    上下 margin/padding 归零（左右保留：blockquote/列表缩进不伤），`p{text-indent:2em}`；`keep_para_spacing` 时
//!    只注缩进。类规则须带元素名才压得过书自带类规则（见书架白皮书 §03y 的七条规则；xochitl 不认 `!important`）。
//! 5. 空页清理：正文无文字无图（Calibre MOBI 转出的 `mbppagebreak` 独占页）→ 从 spine/manifest/zip 删除，
//!    目录里指向它的条目改指下一篇。
//! 6. 自动目录（= `--use-auto-toc --level1-toc //h:h1 --level2-toc //h:h2`）：缺省**仅在书无目录时**从 h1/h2 生成
//!    `toc.ncx` + `nav.xhtml`（xochitl 两者都认）；`AutoToc::Always` 强制重建（原目录坏掉的书）。
//!    定章节（`chapters.rs`，文字书）：按目录层级定书/卷、章、节，漏掉的节补进目录、目录改指到文件中间的标题。
//!    **不拆文件**（2026-10-06 用户定：章节不强制分页，原书的文件结构原样保留）。
//! 7. 单标签重复 `id=` 折叠（`collapse_dup_id_attrs`）：非法 XHTML 会让 xochitl 整章白屏，这里先修、质量门再拦。
//! 8. 全书 id 去重（`ids.rs`）：跨文件重复的 id 改名，全书指向它的链接一起改。
//! 9. 规范整理（`normalize.rs`，最后一步）：XHTML 修成合法 XML（DOCTYPE、命名实体、裸 `&`/`<`、控制字符、空元素、多余闭合标签），
//!    OPF 升级到 EPUB 3（`dcterms:modified` 固定值、唯一标识符、`opf:` 属性改 `refines`），缺导航文档的按 NCX 生成，guide 写成 landmarks；
//!    NCX 与 `<spine toc>` 保留（xochitl 靠它）。
//!
//! 标签、属性、纯文本、可见性一律走 `crate::html`（完整属性名、两种引号、注释不当标签）。
//! 全部规则幂等：注入块带 `class="eink-wash"` 标记，重复过不再叠加。
use crate::html::{self, Edit};
use crate::htmlproc::collapse_dup_id_attrs;
// zip 条目与 zip 内 posix 路径工具已迁到 `epubzip`；这里 re-export，保住 `crate::wash::Entry`/`wash::resolve` 等旧路径。
pub use crate::epubzip::{dir_of, is_html, is_html_entry, percent_decode, posix_norm, relative_to, resolve, Entry};
use crate::util::{is_image_ext, xml_escape};
use regex::Regex;
use std::borrow::Cow;
use std::collections::{HashMap, HashSet};
use std::sync::OnceLock;

// 按职责拆成子模块（原 `wash.rs` 一个文件 2000+ 行）；兄弟模块之间的互相调用走这层（各子模块 `use super::*`）。
mod chapters;
mod cover;
mod css;
mod dead_refs;
mod drm;
mod empty_pages;
mod encoding;
pub mod fonts;
mod html5fix;
mod ids;
mod kindle_images;
mod kindle_rules;
mod top_spacer;
mod hidden;
mod break_margin;
mod blank_fold;
mod block_style;
mod box_fix;
mod heading_div;
mod root_em;
mod inline_css;
mod css_compat;
mod sup_sub;
pub use self::kindle_rules::VScale;

/// 测试用：照竖直换算后的 `eink-ua.css`（同 `add_ua_css`）。
#[cfg(test)]
pub(crate) fn kindle_rules_scale_ua_for_test(vs: Option<VScale>) -> String {
    let css = crate::uastyle::ua_css(None);
    vs.map_or(css.clone(), |vs| kindle_rules::scale_vertical_css(&css, &vs))
}
pub use self::box_fix::BoxGeometry;
pub use self::top_spacer::SpacerGeometry;
mod layout;
mod ncx_fix;
pub mod normalize;
pub mod opf;
mod safe_names;
mod toc;
mod typeset;

// 对外（优化器、质量门、书库、统计）用到的项；其余只在清洗层内部用。
pub use self::cover::{ensure_cover_declared, prepend_cover_page};
pub use self::encoding::transcode_entries;

/// 「照 Send to Kindle 的规则统一」（[`WashMode::Repair`] 的 `kindle_rules`，掌阅、Move 的文字书）这一路自己的版本，只进书库指纹（`u` 段），
/// 不写进书（书里的优化标记 2026-10-09 起也不写版本号了）：这一路改了只让开了它的书过期。
/// - 1（2026-10-08）：标签缺省样式、正文字体、body 左右边距、文字对比度（指纹里写 `u`）。
/// - 2（2026-10-09）：原书没有封面页时补一页（[`prepend_cover_page`]；《绍宋》《狼厅》）。
/// - 3（2026-10-09）：spine 里标了 `linear="no"` 的目录页拿掉（`drop_nonlinear_nav`；《绍宋》）。
/// - 4（2026-10-09 审计）：行内 `style` 里的字符引用先还原再改（`font-family:&quot;宋体&quot;` 以前认不出是正文字体、声明被 `;` 切碎）。
/// - 5（2026-10-10）：字号按全书正文字号归一（`kindle_rules::font_scale`；《消失的爱人》《啸风山庄》《疯探》，别的书逐字节不变）。
/// - 6（2026-10-10 审计）：统计正文字号时 `@media`、`<link media>` 按阅读模式的阅读范围、屏幕求（`WashOpts::media`，同 KFX 写出器；
///   以前特性条件一律不成立；只修复时 `OptimizeOpts::repair_only` 要把 `media` 带过去，否则还是不成立），`font` 简写里的字号也算
///   （层叠展开简写）；正文字体、负外边距、body 上的类改在同一趟全书层叠里算（`kindle_rules::book_facts`，同 KFX 写出器的口径：
///   以前另按原始 HTML 和只认类、裸标签的字体表算）；改写样式表按 `cascade::rule_spans` 逐条规则（以前是只认最内层花括号的正则，
///   字符串、注释里的花括号会切错）；指纹 `u` 段带上屏幕；CSS 文本工具收成一份的边角变化（见 `OPTIMIZE_VERSION` 55）。测试书掌阅、Move 逐字节不变。
/// - 7（2026-10-12）：改完自查正文字体（`kindle_rules::exposed_fonts`）：删掉胜出的正文字体声明后，层叠里被它盖住的字体会露出来，
///   接管的字数比它本来就生效的多就一并去掉（《嘯風山莊》两份样式表都给正文写了字体，21.9 万字的正文以前显示成先链的那一种）。
///   测试书只有《嘯風山莊》变，可见文字不变。
/// - 8（2026-10-12）：阅读器的两个怪癖（用户定）：不认 `rgba()` 的（掌阅）把规则里的 `rgba()` 换成按白底合成的不透明色
///   （《飘》「第一卷」标题的半透明白条作废后字看不见）；不认背景图 `no-repeat` 的（Move，profile `background_no_repeat`）
///   去掉写了不平铺的背景图、背景色照留（大图偏到一边被裁、小图平铺满页）。
/// - 9（2026-10-12）：图片照 Kindle 上的大小显示（`kindle_images.rs`，profile `image_sizing`）：Move 只认百分比宽度、不写宽度和
///   px/em 写的图一律拉满一行，折成 Kindle 上占内容区的百分比；掌阅的 px 是屏幕像素，px 写的、没写尺寸的折成 Kindle 的大小。
///   规则写在 `eink-img.css`，`<img>` 上加类（2026-10-12 三台测试书「测试-插图」量的）。
/// - 10（2026-10-12）：Move 上整页背景图不再去掉，做成整屏大小的一块（`bgfit::plan_tiles`、`imgopt::page_tile`）：xochitl 从内容区
///   左上角起按原始像素平铺、铺满整页，图块左上角内容区那么大的地方照 Kindle 画（尺寸、位置相对内容区），其余白色，每页正好一份
///   （《飘》「第一卷」的插图、版权页底纹，《雪国》标题页）。
/// - 11（2026-10-12）：Move 不认 `margin-top`、页首的空元素不占地方：每章开头写了上边距的，在 `<body>` 开头垫一张透明空白图，
///   推到 Kindle 上同样的位置（`top_spacer.rs`，profile `margin_top = false`；《飘》「第一卷」`margin-top:47%`，差不到内容区高 5% 的不垫）。
/// - 12（2026-10-12）：Kindle 的竖直距离按行距算（profile kindle `line_px` 63，一 rem ≈ 52.5px，以前按 1em = 39px，「第一卷」
///   在 Move 上偏高）；只有一个块的页面（卷首、标题页）背景图相对 body 盒子定位（Kindle 上 body 只到标题的下外边距，`bottom` 的
///   插图贴着它、和标题叠在一起，以前按内容区底边画）。三台截图对过。
/// - 13（2026-10-12）：掌阅背景尺寸的高写 `auto` 会把图拉长到整页高（测试书「测试-掌阅背景」）：整页背景图照 Kindle 上的大小、
///   位置写成屏幕像素的 `background-size`、`background-position`，接在规则末尾（profile `background_auto_height = false`；
///   《飘》「第一卷」插图、版权页底纹，掌阅真机 ✓）。
/// - 14（2026-10-12）：竖直间距照 Kindle 换算（用户定，方案 B）：Kindle 按行距排上下外边距、内边距（一 rem ≈ 52.5px），掌阅、Move
///   按自己的字号（≈ 37.5px），同样 1em 在 Kindle 上占页高多一半；上下外边距、内边距（含 `eink-ua.css`）改写成占内容区高的比例和
///   Kindle 一样（`kindle_rules::VScale`，profile `em_px`）。`%`、`px`、`pt` 折成屏幕像素，顺带修了掌阅把 `pt` 当 px、Move 的 `%`
///   只有十来像素。测试书三台量过，掌阅、Move 每种写法差 1–4px。
/// - 15（2026-10-12）：竖直间距换算挪到最后（`kindle_rules::scale_vertical_entries`）：14 先换算再算页首垫图，垫图读到放大过的
///   边距、垫高了，《飘》「第一卷」在 Move 上标题挤到下一页（真机）。
/// - 16（2026-10-12）：Move 上去掉 `display:none` 的元素（`hidden.rs`，profile `display_none = false`，用户定）：xochitl 行内、外链的
///   `display:none` 都不认，《飘》《春雪》版权页顶上给目录用的「版权信息」标题露出来、把版权页挤成两页。`id` 留成空 `<span>`。
/// - 17（2026-10-12）：15 把竖直换算挪到最后时漏了不是 UTF-8 的样式表（《嘯風山莊》Big5 的 CSS.css 没换算），补上（同 `rewrite_entries` 按单字节读写）。
/// - 18（2026-10-12）：大段上外边距在 Kindle 上排不下时换页、丢掉边距，掌阅、Move 照样做（`break_margin.rs`，用户定）：按 Kindle
///   的尺寸估算，从文件开头、强制分页起累加，上外边距至少页高 1/4 的块「已排 + 边距 + 自己」超过一页的 90% 就在它前面分页、边距写 0。
///   《飘》上下册、《春雪》版权页后的二维码换到下一页页首（掌阅以前推到页底、那行字掉到下一页）；25 本只有这 3 本变。
/// - 19（2026-10-13，用户定）：只有空白的段落（`<p>&nbsp;</p>`、`<p>　</p>`、`<p><br/></p>`、块之间单独的 `<br/>`）照 Kindle 折成下一块的
///   上外边距（`wash/blank_fold.rs`）：空段掌阅 `display:none` 藏起、Move 去掉（不认 `display:none`，用户定），下一块上外边距写成 Kindle 上
///   折叠后 + 0.6 × 空段行高。《三体》585 处，掌阅、Move 每个空段以前比 Kindle 多空 0.6–2 行。
/// - 20（2026-10-13）：Move 上块末尾的 `<br/>` 去掉（Kindle、掌阅不排，Move 多排一行；`blank_fold.rs`）；页首要垫的比 xochitl 标题前自己加的
///   空还少、或者多得不到 5% 的，第一块开头是字、块在 Move 上行距是 1 时改在块里最前面放行内窄图撑高第一行（`top_spacer.rs`，
///   `eink-strut.png`）；Move 上 `<p>` 以外的块行距照 Kindle 按全书正文行距归一（`line_height.rs`，profile `line_height_except_p`，用户定）。
/// - 21（2026-10-13）：Move 上标题字号照 Kindle 写成最接近的关键字（xochitl 的 h1–h6 只认 `small`…`xx-large`，`heading_size.rs`，
///   profile `heading_font_keywords`）；行内垫图按标题在 Move 上的字号算。
/// - 22（2026-10-13）：Move 上写了 em 的 `<style>` 挪进外链样式表、行内 `style` 里写 em 的长度搬进类规则、外链规则最后一条紧挨 `}` 的 em 声明补分号
///   （xochitl 的行内 style、`<style>`、不带分号又紧挨 `}` 的声明 em 按 23px、字号和首行缩进的 em 不认；`inline_css.rs`，profile
///   `inline_css_lengths`）。
/// - 23（2026-10-13）：Move 上左右内边距并进外边距（悬挂缩进排对）、上下内边距和标题的上下外边距照 Kindle 写到相邻块认的一侧、
///   上下边框插同色同粗的横线图（xochitl 不认内边距、不画边框、不认标题外边距；`box_fix.rs`，profile `padding`、`borders`、
///   `heading_margins`，边框用户选横线图）。
/// - 24（2026-10-13）：块上 px 写的左右外边距照 Kindle 折成 em（掌阅、Move 的 px 是屏幕像素，Kindle 按 0.45pt），Move 上 % 写的按
///   内容区宽折成 em（Move 当成 px；`box_fix.rs`，profile `px_margins`、`percent_margins`）。
/// - 25（2026-10-13）：掌阅上 rem 改成 em（字号的折成 px）、去掉 `!important`（靠它胜出的值写进行内 `style`）、行距和 Kindle 差 5% 以上的
///   块写成 em（掌阅不认 rem、带 `!important` 的声明、不带单位的行距；`css_compat.rs`、`block_style.rs`，profile `rem`、`important`、
///   `line_height_numbers`、`line_px`），块上 px 写的左右内边距、首行缩进照 Kindle 折成 em（掌阅的 px 是屏幕像素；`box_fix.rs`，profile
///   `px_margins`）。
/// - 26（2026-10-13）：Move 上 `<p>` 的行距也照 Kindle 归一，写在行内 `style`（xochitl 的 `<p>` 只认行内写的行距，按缺省行距的倍数；
///   `block_style.rs`）。
/// - 27（2026-10-13）：Move 上左右有框的块（全框、单边）包进单格表格，框写成单元格行内的边框（颜色、粗细、点线照书），块的左外边距
///   挪到表格、内边距写到单元格；书里表格、单元格的边框写进行内、加 `cellspacing="0"`（xochitl 普通块的边框不画，只画表格、单元格
///   行内写的；`box_fix.rs`，用户选）。
/// - 28（2026-10-13）：Move 上块的宽度折成左右外边距、页中间负的上下外边距照 Kindle 写成非负的、列表项上下内边距并进外边距；两台单元格上
///   px 的内边距、掌阅行内元素和图上 px 的左右内外边距照 Kindle 折成 em（探针 Q88–Q99；`box_fix.rs`，profile `block_width`、`negative_margins`）。
/// - 29（2026-10-14）：Move 上没有项目符号的列表项左右内边距并进外边距（《海葵》目录页）；单元格缺省顶端对齐，一行两格以上、Kindle 上居中的
///   写 `vertical-align:middle`（探针 Q105、Q106；`box_fix.rs`，profile `cell_middle`；掌阅同样）；掌阅上写了 `vertical-align:super/sub`
///   的行内元素里面包 `<sup>`/`<sub>`（Q100–Q123；`sup_sub.rs`，profile `inline_valign`）。
/// - 30（2026-10-14）：Move 上单元格 CSS 写的宽度照 Kindle 写成 `width` 属性（探针 Q127–Q129；`box_fix.rs`，profile `cell_width`）。
/// - 31（2026-10-14，Kindle 对照）：撤掉 30 的 Move 单元格 `width` 属性（Kindle 不按单元格宽度压窄，Move 不认 CSS 宽度本来就同
///   Kindle）；单元格的宽度两台照 Send to Kindle 写成列宽百分比（掌阅行内 `width:P%`、Move 属性，profile `cell_width`）；定宽块 `margin:auto` 不居中（Kindle 不认）：Move 折外边距时多出来的
///   都放右边、掌阅左边的 auto 写成 0（profile `margin_auto`）；首行缩进 px 按 0.45pt（掌阅折 em 跟着对）；`font-size:large` 1.125。
///   Move 上左右外边距、内边距的 em 照 Kindle 按页宽换算（乘一行的 em 数 ÷ 32，约 0.70；profile `horizontal_scale`，用户选）；
///   body 的左右边距逐文件照 Kindle（和全书多数不一样的文件 body 加类，`eink-body.css`，用户选）；图片加了按 Kindle 折的类时
///   `<img>` 行内 style 里的宽高去掉（两台认行内样式、压过类规则：《克莱因壶》Move、《狼厅》掌阅的图和 Kindle 不一样大）。
/// - 32（2026-10-14，P0 验收核查）：字号归一时外边距、内边距、首行缩进、行高里的 px、pt 这些绝对长度也乘系数（Kindle 写出器按
///   本元素原来的字号换算，等于跟着归一缩放；《嘯風山莊》段间距掌阅、Move 大 16.7%）；掌阅行距差 3% 以上就接（以前 5%）；
///   `br{display:block}` 的空段 `<p><br/></p>` 照样折掉（《人生复本》112 处）；Move 上字号和最近的关键字差 3% 以上的标题改成
///   `<div>`（`heading_div.rs`，用户定：《人生海海》小节标题 1.75em，xx-large 是 2.018）；Move 上 em 一律按根字号算、字号的 px
///   是屏幕像素：字号、边距、缩进换成按根的 em（`root_em.rs`，profile `em_root`，用户定）；字号 `1em` 也挪出行内；空段自己的
///   上下外边距照 Kindle 留着、逐对折叠（`blank_fold.rs`，同写出器 23）；掌阅上标题自己没写对齐的照 Kindle 继承来的写明（掌阅
///   缺省居中、`inherit` 不认；`block_style.rs`，profile `heading_center`）。
/// - 33（2026-10-14，三本武侠全集体检、探针 Q132–Q134 三台真机）：Kindle 不浮动——掌阅标题上的 `float` 写成 `none`（profile
///   `float`）；定宽块只有左边是 auto 的 Kindle 靠右（写出器写块的对齐）：掌阅不再把这种 auto 写成 0，Move 折外边距时多出来的放
///   左边；页首块的负上外边距配上内边距，掌阅上照 Kindle 能看见的写（上外边距 0、上内边距减去推出去的）。《梁羽生》卷名竖排窄条
///   靠右、一页放下（掌阅以前拆到三页、Move 以前横排）。
/// - 34（2026-10-15，标题前后的距离三台抽样 + 探针 Q135–Q142 Kindle 真机）：行高照 Kindle **显示**的值写——KFX 里 0.6、0.667
///   的行 Kindle 排成正常行高的 0.744 倍（`kindle_model::SHOWN_MIN_LINE_HEIGHT`），以前照写的 0.6 写给两台，《人生海海》标题前后
///   在 Move 上紧了三成（掌阅不认这种倍数，多半没变化）。同时修《人生海海》版权页、目录、「第一部」在 Move 上的问题：
///   插了下边框横线图的块行距不低于 1、接在行内（Move 行距少的从行下面扣，横线压字，profile `line_height_floor`；外链里写不带
///   单位的 `line-height` 会让后面的规则失效）；文件开头的块上的强制分页行内接 `page-break-before:auto`（Move 多出一张空白页）；
///   往行内 `style` 接声明时末尾的 `&quot;` 不再被当成分号去掉（`html::style_base`，「第一部」的字号、居中丢了）。
/// - 35（2026-10-15，三台版式检查 `tools/kfx/layout.py` 查出来、用户定修）：Kindle 上不出页的空页（刻意留的空白页、只有隐藏元素的页，
///   写出器不出版面）两台也去掉、链接改指下一页（`empty_pages::remove_kindle_empty_pages`，《疯探》`blank.html`）；Move 上左右有框的
///   列表也包进单格表格（《雪国》注释列表的虚线框）；页首的块也插上横线图，垫图让出它的高（`box_fix::page_top_rule_px`，《人生海海》
///   「第一部」、《平凡的世界》卷名）；页首垫图前面什么都没有的空元素照 Kindle 不算（写出器不出节点；以前整章不垫，「第一部」
///   在 Move 上偏上一截）；包框的列表单元格左内边距补到 Kindle 的缩进（`box_fix::list_indent_extra_px`，《雪国》注释框序号紧贴框线）；包框的单元格照外层写回字号、行距、对齐、字体（书里 `td{font-size:0.6em}` 落在我们的表格上）。
/// - 36（2026-10-15，用户在 Move 上看《梁羽生》）：页首块上内边距很大（拿来推字的，卷名 `padding-top:6em` 配 3 倍字号，18 rem）时，
///   上横线图只画线、不连着内边距，距离只由垫图出（以前算两遍，垫图 + 标题前自带的空 + 横线图的透明段超过一页，卷名被推到下一页，
///   前面多出一页空背景，33 版就有）；Move 上 HTML 属性 `width="80%"` 的图写成 CSS 类（Move 不认属性、拉满一行，《梁羽生》作者介绍页的
///   文字被挤到下一页；`kindle_images` 的 `attr_percent`）。
pub const KINDLE_RULES_VERSION: &str = "36";
pub use self::css::filter_css;
#[cfg(test)]
use self::css::wash_html;
pub use self::drm::{encrypted_targets, real_drm_items, PSEUDO_DRM_SAFE_EXTS};
pub use self::opf::{manifest_items, opf_dc, parse_opf, tag_attr, ManifestItem, Opf, OpfDc};
pub use self::toc::{is_toc_file, toc_entry_count, TocItem};
pub use self::typeset::{count_dup_id_tags, wash_css, NOTEICON_RULE};
pub use crate::html::plain_text;
pub use crate::reader_model::ReaderModel;

use self::css::*;
use self::dead_refs::*;
use self::drm::strip_pseudo_drm;
use self::empty_pages::*;
use self::ids::dedup_ids_across_book;
use self::layout::*;
use self::ncx_fix::*;
use self::opf::{find_opf, opf_book_title, opf_unique_identifier};
use self::chapters::chapters_into_toc;
pub(crate) use self::chapters::is_toc_like_page;
pub(crate) use self::toc::name_index;
pub(crate) use self::kindle_rules::fmt_num;
use self::toc::*;
use self::typeset::*;

#[cfg(test)]
mod tests;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AutoToc {
    Off,
    /// 书无目录（nav/ncx 缺失或零条目）时生成。
    IfMissing,
    Always,
}

/// 正文排版语言（决定首行缩进/段落习惯）。`Auto` 由 `wash_entries` 按全书 CJK/拉丁字符占比判定。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LangMode {
    Auto,
    /// 中文习惯：首行缩进 2em（两个全角字）、段间无空。
    Cjk,
    /// 拉丁习惯：首行缩进 1.2em、标题后首段不缩进。
    Latin,
}

/// 清洗层走哪一路（[`WashOpts::mode`]）。以前是两个布尔开关 `repair_only`、`kindle_rules`，后一个只在前一个为真时才看，
/// `repair_only` 为真时别的排版选项也都不看——这个约定写成枚举。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum WashMode {
    /// 完整清洗（缺省）：解锁、排版、删空白页、目录……[`WashOpts`] 的排版选项（`keep_para_spacing`、`filter_props`、`lang`、
    /// `keep_fonts`、`css_rgba`）只有这一路看。
    #[default]
    Full,
    /// 只修复（文字书，profile `text_repair_only`）：只做 EPUB 3 修复和目录（[`repair_entries`]），不解锁、不排版、不删空白页，
    /// 排版选项不看（`auto_toc` 照看）。调用方已判定不是漫画。
    Repair {
        /// 照 Send to Kindle 的规则统一（profile `kindle_rules`）：标签缺省样式 `eink-ua.css`（[`crate::uastyle::ua_css`]），
        /// 正文字体、body 左右边距、文字对比度（`kindle_rules.rs`）。统计正文字号时按 [`WashOpts::media`] 求 `@media`。
        kindle_rules: bool,
    },
}

impl WashMode {
    /// 只修复（[`WashMode::Repair`]）。
    pub fn is_repair(self) -> bool {
        matches!(self, WashMode::Repair { .. })
    }

    /// 只修复且照 Send to Kindle 的规则统一。
    pub fn kindle_rules(self) -> bool {
        matches!(self, WashMode::Repair { kindle_rules: true })
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct WashOpts {
    /// 完整清洗还是只修复（[`WashMode`]）。
    pub mode: WashMode,
    /// 保留原书段间距（诗集/剧本靠空行分节）。
    pub keep_para_spacing: bool,
    pub auto_toc: AutoToc,
    /// 剥掉的 CSS 属性（小写）。缺省 [`DEFAULT_FILTER_PROPS`]。
    pub filter_props: Vec<String>,
    /// 正文排版语言（`Auto`=自动探测）。
    pub lang: LangMode,
    /// 保留的字体（规范化的名字）。`wash_entries` 开头按全书分析填上（见 `fonts`），调用方不用给。
    pub keep_fonts: HashSet<String>,
    /// 阅读器认 `rgba()` 颜色（profile 的 `css_rgba`，缺省 `true`）；`false` 时换成不透明写法（见 `cssunlock::rgba_to_opaque`）。
    pub css_rgba: bool,
    /// 阅读器认背景图的 `no-repeat`（profile 的 `background_no_repeat`，缺省 `true`）；`false` 时 `kindle_rules` 把写了不平铺的
    /// 背景图去掉（`kindle_rules::rewrite_decls`）。
    pub background_no_repeat: bool,
    /// `kindle_rules` 统计正文字号时求 `@media`、`<link media>` 用的环境（阅读模式的产物阅读范围、屏幕、黑白彩色，
    /// [`crate::cascade::MediaEnv::for_format`]）；`None`＝求值不了的特性条件一律不成立。
    pub media: Option<crate::cascade::MediaEnv>,
    /// 阅读器怎么定图片尺寸（profile 的 `image_sizing`）和 Kindle 上图片的大小（[`KindleImageRef`]）：不是照 CSS 的阅读器，
    /// `kindle_rules` 把图片宽度改写成显示出来和 Kindle 一样大的写法（`kindle_images.rs`）。`None` 不改。
    pub image_sizing: Option<(profile::ImageSizing, KindleImageRef)>,
    /// 图片的原始宽高（路径 → 像素）：流式优化时清洗层拿到的图片条目是空占位，由调用方读文件头填上（`kindle_images` 用）。
    pub image_dims: Option<std::sync::Arc<HashMap<String, (u32, u32)>>>,
    /// 平铺背景的阅读器（`background_no_repeat = false`）上整页背景图做成整页一块用的尺寸（[`crate::bgfit::plan_tiles`]）；
    /// `None` 时这类背景图照旧去掉。
    pub page_tiles: Option<crate::bgfit::TileGeometry>,
    /// 不认 `margin-top` 的阅读器（Move，profile `margin_top = false`）：每章开头写了上边距的，在 `<body>` 开头垫空白图推到
    /// Kindle 上同样的位置（`top_spacer.rs`）。`None` 不垫。
    pub top_spacer: Option<SpacerGeometry>,
    /// 背景尺寸的高写 `auto` 会拉长图的阅读器（掌阅，profile `background_auto_height = false`）：整页背景图照 Kindle 上的大小、
    /// 位置写成屏幕像素（`kindle_rules`）。整屏和阅读范围（[`crate::bgfit::TileGeometry`]）；`None` 不改。
    pub bg_place: Option<crate::bgfit::TileGeometry>,
    /// 竖直间距照 Kindle 换算（掌阅、Move，profile `em_px`；`kindle_rules::VScale`）。`None` 不换。
    pub v_scale: Option<VScale>,
    /// 左右边距的 em 照 Kindle 按页宽换算的系数（Move，profile `horizontal_scale`；`kindle_rules::scale_horizontal_entries`）。`None` 不换。
    pub h_scale: Option<f64>,
    /// 阅读器和 CSS 不一样的地方（[`ReaderModel`]，从 profile 来）：不认 `display:none` 的去掉隐藏的元素（`hidden.rs`）、空段去掉
    /// 而不是藏起来、块末尾多排一行的 `<br/>` 去掉（`blank_fold.rs`）；块的行距、标题字号照 Kindle 改写（`block_style.rs`）；页首垫图
    /// 的尺寸（`top_spacer.rs`）。缺省是完全照 CSS 排的阅读器，什么都不改。
    pub reader: ReaderModel,
    /// 按怪癖表体检哪台（profile id，[`crate::audit::covers`] 的才有）；`None` 不体检。只报告、不改书。
    pub audit: Option<String>,
    /// 内边距、边框、标题外边距照 Kindle 改写要的这台尺寸（`box_fix.rs`；不认内边距、不画边框、不认标题外边距的阅读器才有）。
    pub box_geometry: Option<BoxGeometry>,
}

/// Kindle 上图片显示多大的两个量（kindle profile 的 KFX 阅读范围宽、缺省字号下 1em 的像素）。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct KindleImageRef {
    /// 内容区宽、高（屏幕像素）：不写宽度的图按原图像素显示，宽过、高过内容区的保持比例缩进来。
    pub content_px: f64,
    pub content_h: f64,
    /// 1em（正文字号）的屏幕像素。
    pub em_px: f64,
    /// 竖直方向一 rem 的屏幕像素（Kindle 正文一行的像素 / 1.2，profile `line_px`；没写时同 `em_px`）。
    pub v_rem_px: f64,
}

impl Default for WashOpts {
    fn default() -> Self {
        WashOpts { keep_para_spacing: false, auto_toc: AutoToc::IfMissing, filter_props: DEFAULT_FILTER_PROPS.iter().map(|s| s.to_string()).collect(), lang: LangMode::Auto, keep_fonts: HashSet::new(), css_rgba: true, background_no_repeat: true, mode: WashMode::Full, media: None, image_sizing: None, image_dims: None, page_tiles: None, top_spacer: None, bg_place: None, v_scale: None, h_scale: None, reader: ReaderModel::default(), audit: None, box_geometry: None }
    }
}

/// 注入排版规则的外链 css 文件名（放 OPF 同目录）。真机坐实（2026-09-04《缩进诊断6》/《飘》）：
/// **xochitl 只认外链 `.css` 文件里的规则，完全无视内联 `<style>` 块和元素 `style=` 属性**——所以
/// 排版规则（首行缩进/边距）必须写成外链 css 才在 xochitl 生效（按标准 CSS 渲染的阅读器外链、内联都认）。
/// ⚠ xochitl 的 css 解析器很脆：**只用裸元素选择器**（`p`/`body`），一条类/复杂选择器就可能让整表失效
/// （《缩进诊断5》带 `.big` 类规则时整表不生效，diag6 纯 `p{}` 生效）。
const WASH_CSS_NAME: &str = "eink-wash.css";

/// zip 条目是不是清洗层写的样式表（任意目录下的 `eink-wash.css`）。
pub fn is_wash_css_name(name: &str) -> bool {
    name.rsplit('/').next() == Some(WASH_CSS_NAME)
}

// 要解锁的属性。怎么解（整条去掉，还是只去掉字体、背景图、绝对字号）见 `crate::cssunlock`。
// background / background-image：书常在 body/分卷页用 CSS 背景图（装饰纹样、分卷插画）。xochitl **无视
// no-repeat / background-size** → 把背景图**平铺**满页盖住正文（真机《飘》body.fen 的 `background:url() no-repeat`
// 被铺成多幅）；`background` 简写里的颜色保留成 `background-color`。
// line-height（2026-09-27 用户定）：与字号字体同理，书写死行高会让设备的"行距"设置不起作用。
pub const DEFAULT_FILTER_PROPS: &[&str] = &["font-family", "font-size", "font", "line-height", "background-image", "background"];
const WASH_MARK: &str = "eink-wash";

#[derive(Clone, Debug, Default, PartialEq, serde::Serialize)]
pub struct WashReport {
    pub pseudo_drm_stripped: Vec<String>,
    /// 体检（[`crate::audit`]，`kindle_rules` 改写之前的书）：这台上和 Kindle 不一样又没处理的、没验证过的写法。
    #[serde(skip)]
    pub audit: Vec<crate::audit::Finding>,
    /// 文件名里有安卓存储不能用的字符（`*:?` 等）而改了名的条目：(原名, 新名)。流式优化器按原名回原书读图片字节。
    pub renamed: Vec<(String, String)>,
    /// 要做成整页图块的背景图（清洗后的 zip 路径 → 画法），由优化器改图（[`crate::imgopt::page_tile`]）。
    #[serde(skip)]
    pub page_tiles: HashMap<String, crate::bgfit::PageTile>,
    pub css_files: usize,
    /// 保留下来的嵌入字体（规范化的名字，排好序）。
    pub kept_fonts: Vec<String>,
    /// 标成批注（`eink-annot`）的元素个数。
    pub annotations_marked: usize,
    pub html_files: usize,
    pub empty_pages_removed: Vec<String>,
    pub toc_generated: usize,
    pub dup_id_tags_collapsed: usize,
    /// 挂上标签缺省样式表 `eink-ua.css` 的章节数（[`WashMode::Repair`] 的 `kindle_rules`）。
    pub ua_css_linked: usize,
    /// 照 Send to Kindle 的规则改了的声明数（正文字体、body 左右边距、文字对比度，见 `kindle_rules.rs`）。
    pub kindle_rule_edits: usize,
    /// `toc.ncx` 的 `dtb:uid` 跟 OPF 标识符不一致、被改到一致（0 或 1——一本书只有一个 ncx）。
    pub ncx_uid_fixed: usize,
    /// 书自带的扁平目录（"第X部　编号　章名"排版惯例）被重建成两级后的条目数；0＝没检测到这种
    /// 惯例、原样没动。
    pub toc_parts_restructured: usize,
    /// `toc.ncx` 里指向外部 DTD 的 `<!DOCTYPE>` 声明被剥掉（0 或 1）。
    pub ncx_doctype_stripped: usize,
    /// manifest 里 NCX 条目的 `id` 被改成 `"ncx"`（0 或 1）。见 `fix_ncx_manifest_id`。
    pub ncx_manifest_id_fixed: usize,
    /// 指向书内不存在文件的 `<img>` / 字体全缺的 `@font-face` 被去掉的个数。见 `drop_dead_refs`。
    pub dead_refs_removed: usize,
    /// 书自带目录漏掉、补进目录的节数。见 `chapters.rs`。
    pub toc_sections_added: usize,
    /// 书自带目录（NCX）指错位置、按书里的目录页或下一个文件核实后改指的条目数。见 `toc::repair_ncx_targets`。
    pub ncx_targets_repaired: usize,
    /// 跨文件重复、被改名的 id 数（全书指向它们的链接一起改）。见 `ids.rs`。
    pub dup_ids_renamed: usize,
    /// 文件末尾删掉的空元素/换行数（章尾空白页）。
    pub trailing_blanks_removed: usize,
    /// 去掉了下边距/之后分页的样式表数（包住章节结尾的容器）。
    pub tail_spacing_rules_fixed: usize,
    /// 规范整理的 XML 修复计数。见 `normalize.rs`。
    pub xml_fixes: normalize::XmlFixes,
    /// OPF 被改写（升级到 EPUB 3 或补必需项）：0 或 1。
    pub epub3_upgraded: usize,
    /// 按 NCX 新生成的导航文档条目数（0＝书本来就有 nav）。
    pub nav_generated: usize,
    /// 从 `<guide>` 写进 nav 的 landmarks 条数。
    pub landmarks_added: usize,
    /// 没有 NCX 的书按 nav 生成的 NCX 条目数（xochitl 读目录靠 NCX）。
    pub ncx_generated: usize,
    /// 不是 UTF-8、转成了 UTF-8 的 XHTML/OPF/NCX 个数。见 `encoding.rs`。
    pub transcoded_to_utf8: usize,
}


// ───────────────────────── 入口 ─────────────────────────

/// 全书 CJK vs 拉丁字符占比 → 主语言（Han 字数 ≥ 拉丁字母数 = Cjk）。扫全部 html 正文，早停够量即定。
fn detect_dominant_script(entries: &[Entry]) -> LangMode {
    let (mut han, mut latin) = (0u64, 0u64);
    for e in entries.iter().filter(|e| is_chapter_entry(e)) {
        let Ok(t) = std::str::from_utf8(&e.data) else { continue };
        for ch in plain_text(t).chars() {
            if matches!(ch, '\u{4E00}'..='\u{9FFF}' | '\u{3400}'..='\u{4DBF}' | '\u{F900}'..='\u{FAFF}') {
                han += 1;
            } else if ch.is_ascii_alphabetic() {
                latin += 1;
            }
        }
        if han + latin > 20_000 {
            break; // 够量即判，不必扫全书
        }
    }
    if han >= latin {
        LangMode::Cjk
    } else {
        LangMode::Latin
    }
}

/// 对条目表就地清洗。真 DRM 返回 Err（调用方应整体失败、原样不动）。漫画识别（`comic_detect::is_comic`）在定章节前按清洗到那一步的条目判。
pub fn wash_entries(entries: &mut Vec<Entry>, opts: &WashOpts) -> Result<WashReport, crate::BookError> {
    wash_with(entries, opts, None)
}

/// 同 [`wash_entries`]，漫画识别用调用方判好的结果（优化器按原书判一次，清洗层和图片处理用同一个结果，不再判第二遍）。
pub(crate) fn wash_entries_as(entries: &mut Vec<Entry>, opts: &WashOpts, comic: bool) -> Result<WashReport, crate::BookError> {
    wash_with(entries, opts, Some(comic))
}

fn wash_with(entries: &mut Vec<Entry>, opts: &WashOpts, comic: Option<bool>) -> Result<WashReport, crate::BookError> {
    let mut rep = WashReport::default();
    // 先把 GBK、Big5、UTF-16 的文件转成 UTF-8：后面各步都按 UTF-8 读写
    encoding::transcode_to_utf8(entries, &mut rep);
    strip_pseudo_drm(entries, &mut rep)?;
    // 文件名有安卓存储不能用的字符的先改名：后面各步按条目名找文件
    safe_names::rename_unsafe_entries(entries, &mut rep);
    if opts.mode.is_repair() {
        repair_entries(entries, opts, &mut rep);
        return Ok(rep);
    }
    remove_empty_pages(entries, &mut rep);
    drop_dead_refs(entries, &mut rep);
    // Auto → 探测主语言，解析成具体 Cjk/Latin 再逐文件注排版（探测在剥空页之后、注样式之前）。
    let font_plan = fonts::analyze(entries);
    let opts = {
        let mut o = opts.clone();
        if o.lang == LangMode::Auto {
            o.lang = detect_dominant_script(entries);
        }
        o.keep_fonts = font_plan.keep.clone();
        o
    };
    rep.kept_fonts = font_plan.keep.iter().cloned().collect();
    rep.kept_fonts.sort();
    let opts = &opts;
    // 书的语言标签：OPF `dc:language` 优先，没有就按探测到的主语言。补给缺 lang 的 <html>。
    let opf_idx = find_opf(entries);
    let lang_tag = book_lang_tag(entries, opf_idx, opts.lang);
    // 新建目录时的标题按书的语言（中文"目录"、其它"Contents"）。
    let heading = toc_title(opts.lang);
    // 外链 wash css 的 zip 路径（放 OPF 同目录；无 OPF 兜底放根）。排版规则写这里、逐 html 加 <link>——
    // xochitl 只认外链 css（内联 <style> 无视），见 WASH_CSS_NAME 注。
    let css_path = match opf_idx {
        Some(i) => {
            let d = dir_of(&entries[i].name);
            if d.is_empty() { WASH_CSS_NAME.to_string() } else { format!("{d}/{WASH_CSS_NAME}") }
        }
        None => WASH_CSS_NAME.to_string(),
    };
    // 书的样式表里写了首行缩进的类（英文首段顶格时不留在 eink-flush 上，见 `typeset::flush_first_para_after_heading`）
    let mut indent_classes: HashSet<String> = HashSet::new();
    for e in entries.iter_mut().filter(|e| is_css_name(&e.name)) {
        if let Ok(t) = std::str::from_utf8(&e.data) {
            let css = filter_css(t, opts);
            if opts.lang == LangMode::Latin {
                indent_classes.extend(indent_classes_of(&css));
            }
            e.data = css.into_bytes();
            rep.css_files += 1;
        }
    }
    // 逐文件独立，多线程做（`util::par_map_mut`，结果与逐个做相同）
    let counts = crate::util::par_map_mut(entries, |e| {
        if !is_chapter_entry(e) {
            return None;
        }
        let t = std::str::from_utf8(&e.data).ok()?;
        let (marked, n) = fonts::mark_annotations(t, &font_plan);
        let (out, dups) = wash_html_with(&marked, opts, &indent_classes);
        let href = crate::epubzip::href_to(dir_of(&e.name), &css_path, "");
        let out = inject_css_link(&out, &href);
        let out = ensure_html_lang(&align_classes(&out), &lang_tag);
        e.data = out.into_bytes();
        Some((n, dups))
    });
    for (n, dups) in counts.into_iter().flatten() {
        rep.annotations_marked += n;
        rep.dup_id_tags_collapsed += dups;
        rep.html_files += 1;
    }
    add_wash_css_entry(entries, opf_idx, &css_path, &wash_css(opts));
    fix_ncx_manifest_id(entries, &mut rep);
    // 目录指错位置的先修好：后面的分部重建、定章节都按目录找标题
    repair_ncx_targets(entries, &mut rep);
    restructure_existing_toc_parts(entries, opts.auto_toc, heading, &mut rep);
    nest_parts_among_siblings(entries, opts.auto_toc, heading, &mut rep);
    auto_toc(entries, opts.auto_toc, heading, &mut rep);
    // 定章节、补节进目录放在自动目录之后：自动目录给标题补的 id 已经在，改目录链接时能对上。漫画不做。
    // 不拆文件（2026-10-06 用户定：章节不强制分页，原书的文件结构原样保留）。
    let comic = comic.unwrap_or_else(|| crate::comic_detect::is_comic(entries));
    if !comic {
        chapters_into_toc(entries, heading, &mut rep);
    }
    // 全书 id 去重放在补 id 之后：补的 `eink-sec-N` 只避开本文件已有的锚点，跨文件撞名在这里改。
    dedup_ids_across_book(entries, &mut rep);
    remove_chapter_end_blanks(entries, &mut rep);
    strip_ncx_doctype(entries, &mut rep);
    // 规范整理：XML 修复、升级 EPUB 3、导航文档（见 `normalize.rs`）。放最后：前面各步新写的文件也要过一遍；
    // 升级可能补了标识符，NCX 的 dtb:uid 在它之后对齐。
    normalize::normalize_book(entries, &lang_tag, heading, &mut rep);
    fix_ncx_uid(entries, &mut rep);
    Ok(rep)
}

/// 只修复（[`WashMode::Repair`]，文字书；2026-10-08 用户定三台（Kindle、掌阅、Move）都这样做）：书里的文字、图片、样式一概不动，只做
/// - 合规：指向不存在文件的引用去掉（`drop_dead_refs`）、一个标签上重复的 `id` 合并、跨文件重复的 id 改名（链接跟着改）、
///   NCX 的 DOCTYPE 和 manifest id、`dtb:uid` 对齐，最后规范整理成 EPUB 3（`normalize.rs`：合法 XML、OPF 3.0、nav 与 NCX 互补）；
/// - 目录：指错位置的改指、分部重建、没有目录的按标题生成、按目录层级定章节并把漏掉的节补进目录（补的 id 不改显示）；
/// - 照 Send to Kindle 的规则统一（[`WashMode::Repair`] 的 `kindle_rules`，掌阅、Move）：标签缺省样式表挂在书自带样式之前，正文字体、
///   body 左右边距、文字对比度改进书的样式表（`kindle_rules.rs`）。
///
/// 伪 DRM 剥离、文件名改安全字符在调用方（[`wash_with`]）已经做了。
fn repair_entries(entries: &mut Vec<Entry>, opts: &WashOpts, rep: &mut WashReport) {
    let auto_toc_mode = opts.auto_toc;
    drop_dead_refs(entries, rep);
    let lang = detect_dominant_script(entries);
    let lang_tag = book_lang_tag(entries, find_opf(entries), lang);
    let heading = toc_title(lang);
    let dups = crate::util::par_map_mut(entries, |e| {
        if !is_chapter_entry(e) {
            return 0;
        }
        let Ok(t) = std::str::from_utf8(&e.data) else { return 0 };
        let n = count_dup_id_tags(t);
        if n > 0 {
            e.data = collapse_dup_id_attrs(t).into_bytes();
        }
        n
    });
    rep.dup_id_tags_collapsed += dups.into_iter().sum::<usize>();
    if opts.mode.kindle_rules() {
        // 体检看书本来的写法：先于下面所有改写
        if let Some(reader) = &opts.audit {
            rep.audit = crate::audit::audit(entries, opts.media.as_ref(), reader);
        }
        // 先去掉 Move 上会露出来的隐藏元素：后面的页首垫图、背景图位置都按看得见的内容算
        if !opts.reader.display_none {
            rep.kindle_rule_edits += hidden::apply(entries);
        }
        // Kindle 上不出页的空页（刻意留的空白页、只有隐藏元素的页）去掉：后面各步不用再管它们
        remove_kindle_empty_pages(entries, rep, opts.media.as_ref());
        let font_scale = kindle_rules::apply(entries, rep, opts);
        add_ua_css(entries, rep, font_scale);
        // 不认 rem、`!important` 的阅读器（掌阅）：换成认的写法。别的 pass 之前：它们按 Kindle 的层叠算，换完算出来一样
        css_compat::apply(entries, rep, opts.media.as_ref(), opts.reader.rem, opts.reader.important);
        // 不认行内元素上 vertical-align 上标下标的阅读器（掌阅）：里面包 <sup>/<sub>
        sup_sub::apply(entries, rep, opts.media.as_ref(), opts.reader.inline_valign);
        if let Some((sizing, kref)) = opts.image_sizing {
            kindle_images::apply(entries, rep, opts.media.as_ref(), opts.image_dims.as_deref(), sizing, kref);
        }
        // 只有空白的段落照 Kindle 折成下一块的上边距（照 Kindle 换算竖直间距的两台都做；按原来的 CSS 算，垫图、分页估算之前）
        if opts.v_scale.is_some() {
            blank_fold::apply(entries, rep, opts.media.as_ref(), !opts.reader.display_none, opts.reader.trailing_br_line);
        }
        // 行距、标题字号照 Kindle 在垫图之前：行内垫图要看块在这台上的行距、字号
        block_style::apply(entries, rep, opts.media.as_ref(), &opts.reader);
        if let Some(g) = opts.top_spacer {
            top_spacer::apply(entries, rep, opts.media.as_ref(), g, &opts.reader);
        }
        // 大段上外边距在 Kindle 上排不下时换页、丢掉边距（掌阅、Move 都照 Kindle 换算竖直间距时做，按原来的 CSS 估）
        if let (Some(_), Some((_, k))) = (opts.v_scale, opts.image_sizing) {
            break_margin::apply(entries, rep, opts.media.as_ref(), opts.image_dims.as_deref(), k);
        }
        // 内边距、边框、标题外边距：垫图、分页估算之后（它们按原来的 CSS 算 Kindle 上的样子），竖直换算之前（写的 em 跟着换算）
        if let Some(g) = opts.box_geometry {
            box_fix::apply(entries, rep, opts.media.as_ref(), &opts.reader, g);
        }
        // 竖直间距照 Kindle 换算放在最后：上面几步都按原来的 CSS 算 Kindle 上的样子
        if let Some(vs) = opts.v_scale {
            rep.kindle_rule_edits += kindle_rules::scale_vertical_entries(entries, &vs);
        }
        // 左右边距的 em 照 Kindle 按页宽换算（Move）：同样最后做，box_fix 写的按 Kindle 的口径，一起乘
        if let Some(h) = opts.h_scale {
            rep.kindle_rule_edits += kindle_rules::scale_horizontal_entries(entries, h);
        }
        // em 一律按根字号算的阅读器（Move）：字号、边距、缩进的 em 换成按根的值写进行内。上面几步写完、换算完以后，挪行内样式之前
        root_em::apply(entries, rep, opts.media.as_ref(), opts.reader.em_root, opts.reader.em_root_px);
        // 行内 style、<style>、不带分号又紧挨 } 的声明里 em 不照 CSS 排的阅读器（Move）：挪进外链样式表。最后做：上面几步写进行内的、换算过的值一起挪
        if !opts.reader.inline_css_lengths {
            inline_css::apply(entries, rep);
        }
        drop_nonlinear_nav(entries);
    }
    fix_ncx_manifest_id(entries, rep);
    repair_ncx_targets(entries, rep);
    restructure_existing_toc_parts(entries, auto_toc_mode, heading, rep);
    nest_parts_among_siblings(entries, auto_toc_mode, heading, rep);
    auto_toc(entries, auto_toc_mode, heading, rep);
    chapters_into_toc(entries, heading, rep);
    dedup_ids_across_book(entries, rep);
    strip_ncx_doctype(entries, rep);
    normalize::normalize_book(entries, &lang_tag, heading, rep);
    fix_ncx_uid(entries, rep);
    // Move 上字号对不上关键字的标题改成 div：目录（补节、改指、生成 nav）都按 h1–h6 认标题，放在最后
    if opts.mode.kindle_rules() {
        rep.kindle_rule_edits += heading_div::finish(entries);
    }
}

/// spine 里标了 `linear="no"` 的目录页从 spine 拿掉（manifest 里留着，仍是导航文档），见 [`opf::nonlinear_nav_itemrefs`]。
fn drop_nonlinear_nav(entries: &mut [Entry]) {
    let Some(i) = find_opf(entries) else { return };
    let Ok(text) = std::str::from_utf8(&entries[i].data) else { return };
    let spans = opf::nonlinear_nav_itemrefs(text);
    if spans.is_empty() {
        return;
    }
    let out = html::apply_edits(text, spans.into_iter().map(|(s, e)| (s, e, String::new())).collect());
    entries[i].data = out.into_bytes();
}

/// 书的语言标签：OPF `dc:language` 优先，没有就按主语言（`Latin` → en，其余 zh）。
fn book_lang_tag(entries: &[Entry], opf_idx: Option<usize>, lang: LangMode) -> String {
    opf_idx
        .and_then(|i| {
            static DC_LANG: OnceLock<Regex> = OnceLock::new();
            let t = String::from_utf8_lossy(&entries[i].data);
            DC_LANG.get_or_init(|| Regex::new(r#"(?s)<dc:language\b[^>]*>\s*([A-Za-z]{2,3}(?:-[A-Za-z0-9]+)*)\s*</dc:language>"#).unwrap()).captures(&t).map(|c| c[1].to_string())
        })
        .unwrap_or_else(|| if lang == LangMode::Latin { "en".into() } else { "zh".into() })
}

/// **只做规范整理**（EPUB 3），和清洗层最后一步是同一套：NCX 去 DOCTYPE → XML 修复 → OPF 升级 3.0 → 导航文档与 landmarks → NCX
/// `dtb:uid` 对齐 OPF。给 `booklib meta --edit` 用（2026-09-30 用户：改元数据写出的书也要和 booklib 一样符合 EPUB 3）。不做排版、目录等清洗。
pub fn normalize_epub3(entries: &mut Vec<Entry>) -> WashReport {
    let mut rep = WashReport::default();
    let lang = detect_dominant_script(entries);
    let lang_tag = book_lang_tag(entries, find_opf(entries), lang);
    strip_ncx_doctype(entries, &mut rep);
    normalize::normalize_book(entries, &lang_tag, toc_title(lang), &mut rep);
    fix_ncx_uid(entries, &mut rep);
    rep
}

/// 新增（或重优化时更新）外链 wash css 文件，并往 OPF manifest 补一条 `<item>`（幂等）。
fn add_wash_css_entry(entries: &mut Vec<Entry>, opf_idx: Option<usize>, css_path: &str, content: &str) {
    add_css_entry(entries, opf_idx, css_path, content, "eink-wash-css");
}

/// 只修复时挂上标签的缺省样式（[`WashMode::Repair`] 的 `kindle_rules`）：OPF 同目录写 `eink-ua.css`，每章 `<head>` 里**第一个**放指向它的 `<link>`
/// （在书自带的样式表、`<style>` 之前，书里写了的照样盖过它）。目录页（nav）不挂。
fn add_ua_css(entries: &mut Vec<Entry>, rep: &mut WashReport, font_scale: Option<f64>) {
    let opf_idx = find_opf(entries);
    let name = crate::uastyle::UA_CSS_NAME;
    let css_path = match opf_idx.map(|i| dir_of(&entries[i].name)) {
        Some(d) if !d.is_empty() => format!("{d}/{name}"),
        _ => name.to_string(),
    };
    let linked = crate::util::par_map_mut(entries, |e| {
        if !is_chapter_entry(e) {
            return false;
        }
        let Ok(t) = std::str::from_utf8(&e.data) else { return false };
        let out = typeset::inject_css_link_first(t, &crate::epubzip::href_to(dir_of(&e.name), &css_path, ""));
        let changed = out != t;
        e.data = out.into_bytes();
        changed
    });
    rep.ua_css_linked += linked.into_iter().filter(|c| *c).count();
    add_css_entry(entries, opf_idx, &css_path, &crate::uastyle::ua_css(font_scale), "eink-ua-css");
}

/// 新增（或更新）一份我们写的样式表，并往 OPF manifest 补一条 `<item>`（幂等）。
fn add_css_entry(entries: &mut Vec<Entry>, opf_idx: Option<usize>, css_path: &str, content: &str, id: &str) {
    add_entry(entries, opf_idx, css_path, content.as_bytes().to_vec(), id, "text/css");
}

/// 新增（或更新）一个我们写的文件（样式表、图），并往 OPF manifest 补一条 `<item>`（幂等）。
fn add_entry(entries: &mut Vec<Entry>, opf_idx: Option<usize>, path: &str, data: Vec<u8>, id: &str, media_type: &str) {
    if let Some(e) = entries.iter_mut().find(|e| e.name == path) {
        e.data = data;
    } else {
        entries.push(Entry { name: path.to_string(), data });
    }
    if let Some(oi) = opf_idx {
        let opf_dir = dir_of(&entries[oi].name).to_string();
        let text = String::from_utf8_lossy(&entries[oi].data);
        if manifest_items(&text).iter().any(|it| it.path(&opf_dir) == path) {
            return;
        }
        let href = crate::epubzip::href_to(&opf_dir, path, "");
        if let Some(t) = opf::insert_manifest_items(&text, &[opf::NewItem { id, href: &href, media_type, properties: "" }]) {
            entries[oi].data = t.into_bytes();
        }
    }
}

/// 图片的原始宽高（清洗后的 zip 路径 → 像素）：调用方读好的（流式优化时图片条目是空占位；清洗层改过名的按改名记录换过来），
/// 没有的从条目字节读。
pub(crate) fn image_dims_of<'e>(entries: &'e [Entry], rep: &WashReport, known: Option<&HashMap<String, (u32, u32)>>) -> HashMap<&'e str, (u32, u32)> {
    let mut dims: HashMap<&str, (u32, u32)> = HashMap::new();
    if let Some(k) = known {
        let back: HashMap<&str, &str> = rep.renamed.iter().map(|(old, new)| (new.as_str(), old.as_str())).collect();
        for e in entries.iter().filter(|e| crate::util::is_image_ext(&e.name)) {
            if let Some(d) = k.get(back.get(e.name.as_str()).copied().unwrap_or(&e.name)) {
                dims.insert(e.name.as_str(), *d);
            }
        }
    }
    for e in entries.iter().filter(|e| crate::util::is_image_ext(&e.name) && !e.data.is_empty()) {
        if let Some(d) = crate::imgopt::guard(|| image::ImageReader::new(std::io::Cursor::new(&e.data)).with_guessed_format().ok()?.into_dimensions().ok()) {
            dims.entry(e.name.as_str()).or_insert(d);
        }
    }
    dims
}

/// 条目名是不是样式表（`.css`，不分大小写）。
pub(crate) fn is_css_name(name: &str) -> bool {
    name.len() >= 4 && name.as_bytes()[name.len() - 4..].eq_ignore_ascii_case(b".css")
}

/// 正文章节：是 html、又不是目录文件（[`is_toc_file`]）。
pub(crate) fn is_chapter_entry(e: &Entry) -> bool {
    is_html_entry(&e.name, &e.data) && !is_toc_file(&e.name)
}

/// 比较标题文字用：去掉所有空白（含全角空格）。
pub(super) fn squash_ws(t: &str) -> String {
    t.chars().filter(|c| !c.is_whitespace()).collect()
}

/// 全书链接改写的一处链接：所在文件、链接值原文、解析出的目标文件与锚点（锚点原文，未解码）。
pub(super) struct Link<'a> {
    pub file: &'a str,
    pub value: &'a str,
    pub target: String,
    pub frag: Option<&'a str>,
}

/// 全书（html、NCX、OPF）的 `href`/`src`/`xlink:href` 改写：`f` 返回新值就替换（保留原引号）。书外链接不回调；
/// `skip(条目名)` 为真的条目不动；OPF 里只改 `<guide>` 这类引用，manifest 的 `<item href>` 是文件本身的声明、从不改
/// （2026-09-28 审计：空页删除没删掉单引号 OPF 的 item 时，把它的 href 改成了邻页，spine 就重复了一章）。
/// 空页清理、全书 id 去重共用。返回改了的条目数。
pub(super) fn rewrite_book_links(entries: &mut [Entry], skip: impl Fn(&str) -> bool + Sync, f: impl Fn(&Link) -> Option<String> + Sync) -> usize {
    // 各文件独立，多线程做（`util::par_map_mut`）
    let changed = crate::util::par_map_mut(entries, |e| {
        let l = e.name.to_ascii_lowercase();
        let is_opf = l.ends_with(".opf");
        if skip(&e.name) || !(is_opf || l.ends_with(".ncx") || is_html_entry(&e.name, &e.data)) {
            return false;
        }
        let Ok(text) = std::str::from_utf8(&e.data) else { return false };
        let name = e.name.as_str();
        let new = html::edit_attrs(text, &["href", "src", "xlink:href"], |t, a| {
            if is_opf && opf::is_local(t.name, "item") {
                return Edit::Keep;
            }
            let (p, _) = html::split_href(a.value);
            if html::is_external(p) {
                return Edit::Keep;
            }
            // 目标文件按 `epubzip::resolve_link` 解析；锚点给原文（改链时原样写回，对 id 时再 `html::frag_id`）
            let target = crate::epubzip::resolve_link(name, a.value).0;
            match f(&Link { file: name, value: a.value, target, frag: html::split_href(a.value).1 }) {
                Some(v) if v != a.value => Edit::Set(v),
                _ => Edit::Keep,
            }
        });
        match new {
            Cow::Owned(new) => {
                e.data = new.into_bytes();
                true
            }
            Cow::Borrowed(_) => false,
        }
    });
    changed.into_iter().filter(|&c| c).count()
}
