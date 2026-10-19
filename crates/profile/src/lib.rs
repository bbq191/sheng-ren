//! 设备 profile（阅读模式）：目标阅读器的屏幕、真实可阅读范围、黑白彩色。现在有三份，都按设备自带的阅读器量（见 docs/devices.md）：
//! `kindle`（Kindle PW12 签名版，KFX）、`ireader`（掌阅 Ocean 5 Pro，EPUB）、`xochitl`（reMarkable Move，EPUB）。
//!
//! 算法里不写死屏幕数字，一律从 profile 读。`[screen]` 是设备**标称**分辨率；优化时用的是
//! [`Profile::readable`]：阅读器实际能用来显示内容的范围（按产物格式分，阅读器页边距、状态栏等都已扣掉），
//! 有实测值就内置在 `[readable.<格式>]`，没有就退回标称尺寸。每台设备一份 `profiles/<id>.toml`（文件名即 id），
//! 构建时全部嵌入（见 `build.rs`）——**增删设备 = 增删文件**，不改代码。运行期还可以用
//! [`Registry::with_dir`] 从外部目录加载，同 id 覆盖内置项。

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::Path;
use std::sync::OnceLock;

include!(concat!(env!("OUT_DIR"), "/builtin.rs"));

/// profile 没写 `comic_margin` 时漫画页的白边（像素）。
pub const DEFAULT_COMIC_MARGIN: u32 = 1;

/// `image_gamma` 只加深短边至少这么多像素的插图：分隔线、横线图、图标、页首垫的空白图不算插图（拿不准就不处理）。
pub const MIN_GAMMA_IMAGE_SIDE: u32 = 200;

/// 产物格式。KFX 是先按同一套规则优化出 EPUB、再转换（给 Kindle 自带阅读器，2026-10-05 起，见 docs/kfx.md）；
/// PDF、AZW3 已删（AZW3 2026-10-09 用户定：KFX 取代），写了按未知值报错。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Format {
    Epub,
    Kfx,
}

impl Format {
    /// 文件扩展名（也是 TOML 里的写法）。
    pub fn ext(self) -> &'static str {
        match self {
            Format::Epub => "epub",
            Format::Kfx => "kfx",
        }
    }
}

/// 竖屏像素尺寸（`width <= height`）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Screen {
    pub width: u32,
    pub height: u32,
}

impl Screen {
    pub const fn long_edge(self) -> u32 {
        self.height
    }
    pub const fn short_edge(self) -> u32 {
        self.width
    }
    /// 宽/高。
    pub fn aspect(self) -> f32 {
        self.width as f32 / self.height as f32
    }
}

/// 注释在这个阅读器里怎么看（用户 2026-09-29 定）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Notes {
    /// 点标号弹窗显示：标号标 `epub:type="noteref"`、注释正文是 `<aside epub:type="footnote">`，阅读器据此认出注释。
    /// 内置的三个模式都不用（自带阅读器上没验证过弹窗），留给书库 `profiles/` 里的自定义模式。
    Popup,
    /// 点标号跳到注释、再返回（xochitl：没有弹窗，只认同文件 `#锚点`，不认 `epub:type`）。
    Jump,
}

/// 注释标号只有一个小图标（多看等书）时怎么办。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum NoteIcons {
    /// 保留原图标，限成一个字高（要阅读器认 CSS 限高、图标链接也能点）。内置的三个模式都换数字（见 `Number`），
    /// 留给自定义模式。
    #[default]
    Keep,
    /// 换成上标数字（xochitl：只有图的链接点了没反应，CSS 也限不住图标大小）。
    Number,
}

/// 阅读器怎么定图片的显示尺寸（只修复、照 Send to Kindle 统一的文字书用：`kindle_rules` 照它把图片宽度改写成显示出来和
/// Kindle 一样大的写法，见 `bookconv::wash::kindle_images`）。2026-10-12 三台测试书（`测试-插图`）量的。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ImageSizing {
    /// 照 CSS（缺省；Kindle 由 KFX 写出器写，不经 `kindle_rules`）。
    #[default]
    Css,
    /// px 是屏幕像素（掌阅：300px 的图只占四分之一页宽，Kindle 把 px 换成 em、跟着字号走，占三分之二）。
    DevicePx,
    /// 只认百分比宽度（xochitl：没写宽度、写 px/em、只写高的图一律拉满一行，小图也放大到整行）。
    Percent,
}

/// 产物怎么送到设备上（`[deliver]`，2026-10-07 用户定：`booklib sync` 按设备接没接上直接传，不在电脑上留产物）。
/// 不写这一节的模式（书库 `profiles/` 里的自定义模式）产物照旧放在电脑上（跟踪目录旁的 `<模式 id>/`）。
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "lowercase", deny_unknown_fields)]
pub enum Deliver {
    /// MTP 设备（Kindle、掌阅）：jmtpfs 自动挂到 `$XDG_RUNTIME_DIR/mtp/<mount>`，下面是存储（如「Internal Storage」），
    /// 产物放进存储根目录的 `dir`（如 `documents`），子目录照跟踪目录镜像。挂载点在、里面有存储就算接上了。
    Mtp { mount: String, dir: String },
    /// Move（xochitl）：经 SSH 端口转发调设备上书架服务的导入接口（`port` 是它在设备本机监听的端口），直接加入 xochitl、
    /// 原地替换保留 UUID。`hosts` 依次试（`用户@地址`，USB、Wi-Fi），第一个连得上的算接上了。文件夹按层建：
    /// 跟踪目录里的子目录路径 `a/b` 是 xochitl 里「a」文件夹里的「b」文件夹（书架服务逐级找、没有就建）。
    Xochitl { hosts: Vec<String>, port: u16 },
}

/// 标题在阅读器上的字号（`heading_font_keywords` 的阅读器要写，测试书量的；正文 1em＝1）。
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct HeadingSizes {
    /// 认的关键字 → 大小（绝对大小，不随外层字号变）。
    pub keywords: BTreeMap<String, f64>,
    /// h1–h6 没写字号（或写了不认的）时按标签固定的大小。
    pub tags: [f64; 6],
}

/// 页首垫图要的阅读器尺寸（`margin_top = false` 的阅读器要写，屏幕像素，测试书量的）。
#[derive(Debug, Clone, Copy, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PageTop {
    /// 垫图后面阅读器自己加的空：标题每 em 字号多少。
    pub gap_heading_per_em: f64,
    /// 同上，别的块（和字号无关）。
    pub gap_other: f64,
    /// 第一块里最前面放行内窄图撑高第一行时，图高要比想挪的多出多少（每 em 字号）字才开始挪。
    pub strut_base_per_em: f64,
}

/// 阅读器的 CSS 怪癖（[`Profile::css`]）：同一份书在 Kindle 上怎么显示，这台就要写成什么样才一样。字段名同 TOML 里的键。
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CssQuirks {
    /// 阅读器认 CSS 的 `rgba()` 颜色（TOML 里不写是 `true`）。掌阅不认：`rgba()` 写的颜色连同那条声明作废（《绍宋》深红底色
    /// 显示成白底），ireader 写 `false`，优化器换成 `#rrggbb`（2026-10-06 真机测试书）。
    pub css_rgba: bool,
    /// 阅读器认背景图的 `background-repeat: no-repeat`（TOML 里不写是 `true`）。xochitl 不认：背景图一律从某个原点按原始像素
    /// 平铺，`no-repeat`、`background-size`、`background-position` 都不管——大图一块就超出整页、显得偏到一边被裁掉，
    /// 小图铺满一页（2026-10-12 Move 测试书：12 种写法画得一模一样，预先缩小的四种宽度都平铺）。xochitl 写 `false`：
    /// 只修复、照 Send to Kindle 统一的文字书（`kindle_rules`）里，写了不平铺（`no-repeat`、`repeat-x`、`repeat-y`……）的
    /// 背景图去掉、背景色照留；本来就平铺的照留。整页背景图做成整屏一块（KINDLE_RULES 10，`bookconv::bgfit::PageTile`）。
    pub background_no_repeat: bool,
    /// 阅读器认 `margin-top`（TOML 里不写是 `true`）。xochitl 不认（`padding-top`、`border-top` 也不认），页首的空元素也不占地方
    /// （2026-10-12 Move 测试书）：写 `false` 时只修复、照 Send to Kindle 统一的文字书里，每章开头写了上边距的在 `<body>` 开头
    /// 垫一张透明空白图，推到 Kindle 上同样的位置（`bookconv::wash::top_spacer`，《飘》「第一卷」`margin-top:47%`）。
    pub margin_top: bool,
    /// 阅读器认背景尺寸里的 `auto` 高（TOML 里不写是 `true`）。掌阅不认：`background-size:100% auto`、`1264px auto` 都把图拉长到
    /// 整页高（2026-10-12 测试书「测试-掌阅背景」：写成像素宽高、`contain` 才按比例；定位范围是整屏）。写 `false` 时只修复、照
    /// Send to Kindle 统一的文字书里，整页背景图照 Kindle 上的大小、位置写成屏幕像素（`bookconv::wash::kindle_rules`）。
    pub background_auto_height: bool,
    /// 阅读器认 `display:none`（TOML 里不写是 `true`）。xochitl 不认（行内、外链都不认，2026-10-12 测试书「测试-隐藏」）：写 `false`
    /// 时只修复、照 Send to Kindle 统一的文字书里去掉 `display:none` 的元素（在 Kindle 上本来就看不见；用户定），`id` 留成空 `<span>`。
    pub display_none: bool,
    /// `<p>` 不认外链、`<style>` 里的 `line-height`（行内 `style` 的认，2026-10-13 怪癖测试书 Q83）、别的块（标题、div、li、blockquote、td）
    /// 按阅读器自己的缺省行距的倍数认（TOML 里不写是
    /// `false`）。xochitl 是这样（2026-10-13 测试书「测试-行距」：body `line-height:1.618` 时 `<p>` 一行 56px、div 90px、2em 标题
    /// 179px；行内 `style` 也认）：写 `true` 时只修复、照 Send to Kindle 统一的文字书里，这些块的行距照 Kindle 按全书正文行距归一
    /// （`bookconv::wash::block_style`）。
    pub line_height_except_p: bool,
    /// [`Profile::line_height_except_p`] 的块上行距倍数的下限（TOML 里不写是没有）。xochitl 行距少出来的空白全从行的下面扣（多出来的
    /// 也全加在下面），倍数低于 1 时字的下沿伸出行盒、和下面的内容叠在一起（2026-10-15 探针 Q143：h1 写 0.744，末尾的横线图压在
    /// 字上，写 1 和不写一样；《人生海海》版权页、目录的大字标题写 0.85 还压着字下沿）；Kindle 上下平分，压到 0.744 也不叠。插了下边框横线图（`bookconv::wash::box_fix`）的块行距不低于它；
    /// 别的块照 Kindle 压（段落里行与行之间不叠）。
    pub line_height_floor: Option<f64>,
    /// em 一律按根字号（阅读器字号）算（TOML 里不写是 `false`）：字号、外边距、首行缩进的 em（字号的 % 也是）不按父元素、自己的字号，
    /// 按根。xochitl 是这样（2026-10-14 探针「测试-div探针」4–6：外层 2em 里 `font-size:0.5em` 排成 0.5 倍，1.75em 的块
    /// `margin-left:2em` 只挪 76px）：写 `true` 时只修复、照 Send to Kindle 统一的文字书里，字号、边距、缩进的 em 换算成按根的值
    /// （`bookconv::wash::root_em`）。
    pub em_root: bool,
    /// 标题（h1–h6）自己没写 `text-align` 时居中（TOML 里不写是 `false`）。掌阅是这样（2026-10-14 探针「测试-标题对齐」：`h2` 不写居中，
    /// `text-align:inherit` 也不认、照样居中，`left` 认；Kindle 继承外层的）：写 `true` 时只修复、照 Send to Kindle 统一的文字书里，
    /// 标题照 Kindle 继承来的对齐写明（`bookconv::wash::block_style`）。
    pub heading_center: bool,
    /// 标题（h1–h6）的 `font-size` 只认关键字（TOML 里不写是 `false`）。xochitl 是这样：em、px、pt、% 一概不认，固定按标签排，只认
    /// `small`/`medium`/`large`/`x-large`/`xx-large`（绝对大小；2026-10-13 测试书「测试-标题字号」）：写 `true` 时只修复、照 Send to
    /// Kindle 统一的文字书里，标题字号照 Kindle 写成最接近的关键字（`bookconv::wash::block_style`）。
    pub heading_font_keywords: bool,
    /// 标题在这台上的字号（`heading_font_keywords` 时必须写，见 [`HeadingSizes`]；TOML 表 `[heading_sizes]`）。
    pub heading_sizes: Option<HeadingSizes>,
    /// 页首垫图要的尺寸（`margin_top = false` 时必须写，见 [`PageTop`]；TOML 表 `[page_top]`）。
    pub page_top: Option<PageTop>,
    /// 块末尾的 `<br/>`（`<h1>标题<br/></h1>`）多排一行（TOML 里不写是 `false`）。Kindle、掌阅不排，xochitl 多排一行
    /// （2026-10-13 测试书「测试-末尾br」：段落多 50px、1.5em 标题多 75px）：写 `true` 时只修复、照 Send to Kindle 统一的文字书里
    /// 去掉（`bookconv::wash::blank_fold`）。
    pub trailing_br_line: bool,
    /// 行内 `style`、`<style>`、规则最后一条不带分号又紧挨着 `}` 的声明里 em 的长度照 CSS 排（TOML 里不写是 `true`）。xochitl 不：它们走另一套
    /// 解析，em 按 23px（外链 37.8px），`font-size`、`text-indent` 的 em、% 不认（2026-10-13 怪癖测试书 Q26–Q29、Q45–Q52、Q61–Q64）：写
    /// `false` 时只修复、照 Send to Kindle 统一的文字书里 `<style>` 挪进外链样式表、行内写 em 的长度搬进类规则、外链规则末尾紧挨 `}` 的 em 声明补分号
    /// （`bookconv::wash::inline_css`）。
    pub inline_css_lengths: bool,
    /// 阅读器认内边距（TOML 里不写是 `true`）。xochitl 不认（上下左右都不认，2026-10-13 怪癖测试书 Q06、Q12、Q39），但块的背景色照画
    /// （Q68）：写 `false` 时只修复、照 Send to Kindle 统一的文字书里，左右内边距并进左右外边距，上下内边距折进和相邻块之间的间距
    /// （`bookconv::wash::box_fix`）；带背景色的块、表格、列表不动。
    pub padding: bool,
    /// 阅读器画边框（TOML 里不写是 `true`）。xochitl 一概不画（Q07、Q40），能画图（Q69、Q70）：写 `false` 时上、下边框在块里最前、最后
    /// 插一张同色同粗的横线图（连着这一侧的内边距），左右边框、表格线做不了（`bookconv::wash::box_fix`，用户定）。
    pub borders: bool,
    /// 阅读器认 h1–h6 的上下外边距（TOML 里不写是 `true`）。xochitl 都不认（页中间的也不认，Q05、Q65、Q67），标题前后自带一段空、
    /// 和相邻块的外边距折叠（前一块的下外边距、后一块的上外边距认，Q66、Q74）：写 `false` 时标题前后的间距照 Kindle 写到相邻的块上
    /// （`bookconv::wash::box_fix`）。
    pub heading_margins: bool,
    /// 左右外边距写 px 时和 Kindle 一样大（TOML 里不写是 `true`）。掌阅、xochitl 的 px 是屏幕像素（Move 50px 挪 50，探针 Q10、Q48），
    /// Kindle 写出器按 1px＝0.45pt（约 1.46 个 Kindle 屏幕像素）：写 `false` 时只修复、照 Send to Kindle 统一的文字书里块上 px 的左右
    /// 外边距折成 em（`bookconv::wash::box_fix`）；认内边距的阅读器（掌阅：`text-indent:30px` 挪 30，Q16、Q60）px 写的左右内边距、
    /// 首行缩进一并折成 em。
    pub px_margins: bool,
    /// 左右外边距写 % 时按包含块宽（TOML 里不写是 `true`）。xochitl 当成 px（10% 挪 10px，Q11、Q54）：写 `false` 时块上 % 的左右外边距按
    /// 内容区宽折成 em（`bookconv::wash::box_fix`）。
    pub percent_margins: bool,
    /// 阅读器认块的 `width`（TOML 里不写是 `true`）。xochitl 不认（Q30，块一律满宽）、左右外边距认：写 `false` 时只修复、照 Send to Kindle
    /// 统一的文字书里块的宽度照 Kindle 折成左右外边距（`margin:auto` 居中的两边各一半；`bookconv::wash::box_fix`）。
    pub block_width: bool,
    /// 阅读器认负的外边距（TOML 里不写是 `true`）。xochitl 不认（Q08、Q75–Q79，上下左右都不认）：写 `false` 时只修复、照 Send to Kindle
    /// 统一的文字书里，页中间相邻两块之间带负外边距的照 Kindle 折叠后的间距写成两边的非负外边距（`bookconv::wash::box_fix`；左右的不管，用户定）。
    pub negative_margins: bool,
    /// 阅读器单元格缺省竖直居中（TOML 里不写是 `true`，同 Kindle：Send to Kindle 没写 `vertical-align` 的单元格居中）。xochitl 缺省顶端对齐
    /// （2026-10-14 探针 Q105、Q106，`middle` 写了认）：写 `false` 时只修复、照 Send to Kindle 统一的文字书里，一行两格以上、Kindle 上居中的
    /// 单元格行内写 `vertical-align:middle`（`bookconv::wash::box_fix`）。
    pub cell_middle: bool,
    /// 阅读器认行内元素上 `vertical-align` 写的上标、下标（TOML 里不写是 `true`）。掌阅不认（2026-10-14 探针 Q100、Q101、Q122、Q123），
    /// `<sup>`/`<sub>` 标签认（Q120、Q121）：写 `false` 时只修复、照 Send to Kindle 统一的文字书里，写了 super/sub 的行内元素里面包一层
    /// `<sup>`/`<sub>`（Kindle 写出器两种写法写成同一个值；`bookconv::wash::sup_sub`）。
    pub inline_valign: bool,
    /// 阅读器按单元格的宽度（CSS、`width` 属性）把格子压窄（TOML 里不写是 `true`，同 CSS）。Kindle 不压：Send to Kindle 把宽度写成
    /// 表格列宽的百分比，`td{width:1em}` 的字照样横排（2026-10-14 怪癖测试书 Q127–Q129 真机）。掌阅压窄（字竖着排）。只修复、照
    /// Send to Kindle 统一的文字书里单元格的宽度照 Kindle 写成列宽百分比：`true` 写行内 `width:P%`（掌阅 `width:auto` 不认）、
    /// `false`（xochitl：CSS 写的不认、属性认）写 `width` 属性（`bookconv::wash::box_fix`）。
    pub cell_width: bool,
    /// 阅读器按左右外边距 `auto` 让定了宽度的块居中、靠右（TOML 里不写是 `true`，同 CSS）。Kindle 不（Send to Kindle 写了块对齐
    /// `$580`，Kindle 照样靠左，2026-10-14 怪癖测试书 Q124 真机）：只修复、照 Send to Kindle 统一的文字书里定了宽度的块上 auto 的
    /// 外边距写成 0（认块宽度的阅读器；不认的在宽度折成外边距时多出来的都放右边，`bookconv::wash::box_fix`）。
    pub margin_auto: bool,
    /// 认 `float`（TOML 里不写是 `true`，同 CSS）。Kindle 不浮动（KFX 写出器不写 float，Send to Kindle 写的 Kindle 也不按它排：标题只写
    /// `float:right` 的照旧自己占一行，2026-10-14 怪癖测试书 Q133 真机）；掌阅认，标题浮到右边、后面的字排在旁边：写 `true` 时只修复、
    /// 照 Send to Kindle 统一的文字书里标题、段落上的 float 写成 `none`（`bookconv::wash::box_fix`；段落是 2026-10-16 探针 Q148 补的）。Move 不认，写 `false`。
    pub float: bool,
    /// 左右外边距、内边距的 `em` 照 Kindle 按页宽换算（TOML 里不写是 `false`）。Kindle 的左右长度一律按页宽的百分比显示（em 按一页
    /// 32 个 em 折，写出器同 Send to Kindle），xochitl 一行只有约 22 个 em（842px ÷ 37.8），同样的 em 宽出约 1.4 倍：写 `true` 时只修复、
    /// 照 Send to Kindle 统一的文字书里左右外边距、内边距的 em、pt 乘「这台一行的 em 数 ÷ 32」（`bookconv::wash::kindle_rules`，用户
    /// 2026-10-14 选；正的首行缩进不动，Kindle 在整页宽的块里也按 em；负的悬挂缩进跟着乘）。
    pub horizontal_scale: bool,
    /// 阅读器认 `rem`（TOML 里不写是 `true`）。掌阅不认，整条声明当没写（2026-10-13 怪癖测试书 Q53：`margin-left:2rem` 不挪）：写 `false`
    /// 时只修复、照 Send to Kindle 统一的文字书里字号以外的 rem 改成 em（Kindle 写出器同样按本元素的 em 算），字号的 rem 折成 px
    /// （`bookconv::wash::css_compat`）。
    pub rem: bool,
    /// 阅读器认 `!important`（TOML 里不写是 `true`）。掌阅带 `!important` 的声明一概不起作用（只有这一条、没有冲突的也不起作用；行内、
    /// 外链都一样，Q28、Q85、Q86）：写 `false` 时只修复、照 Send to Kindle 统一的文字书里去掉 `!important`，靠它胜出的值写进元素的
    /// 行内 `style`（`bookconv::wash::css_compat`）。
    pub important: bool,
    /// 阅读器认不带单位的 `line-height`（TOML 里不写是 `true`）。掌阅不认（`<p>`、`<div>`、标题、行内 `style` 都不认，Q80–Q83），
    /// 写成长度的认（`3em` 一行 114px，Q84），缺省一行 `line_px`：写 `false` 时只修复、照 Send to Kindle 统一的文字书里，行距和
    /// Kindle 不一样的块照 Kindle 写成 em（`bookconv::wash::block_style`；要写 `line_px`、`em_px`）。
    pub line_height_numbers: bool,
}

/// 注释怎么呈现（[`Profile::notes`]）。
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct NotePolicy {
    /// 注释的呈现方式。
    #[serde(rename = "notes")]
    pub mode: Notes,
    /// 只有图标的注释标号怎么办（TOML 里不写是 `keep`）。
    #[serde(rename = "note_icons")]
    pub icons: NoteIcons,
    /// 保留原书注释里"跳回正文"的回链（TOML 里不写是 `true`）。xochitl 写 `false`：它遇到标号↔注释互相链接的一对，
    /// 两条链接都丢掉（正向也点不动），只能去掉回链、返回靠阅读器自己的"返回"。Kindle 自带阅读器没有可靠的"返回"，要靠回链。
    #[serde(rename = "note_backlinks")]
    pub backlinks: bool,
}

/// 漫画怎么排（[`Profile::comic`]）。字段名去掉了 TOML 里的 `comic_` 前缀。
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ComicPolicy {
    /// 漫画页图到可阅读范围四边的白边（像素）：图保比缩放进"阅读范围 − 2×白边"的框，受限的那条边两侧正好是这么宽。
    /// TOML 里不写是 [`DEFAULT_COMIC_MARGIN`]；须小于阅读范围短边的 1/4。
    #[serde(rename = "comic_margin")]
    pub margin: u32,
    /// 漫画在阅读器里要设成的页边距（xochitl 设置里的"页边距"，单位同 `.content` 的 `margins`）。`Some` 时：优化器给漫画写
    /// `META-INF/eink-reader-margins`（值就是它），`xochitl/comic-margins.sh` 凭它登记、Move 上的页边距代理在第一次开书时设好；
    /// 漫画的文字页、混排页的字补回默认留白，有图的页去掉 `<body>` 的类（见 `bookconv::comicpad`）。
    #[serde(rename = "comic_reader_margins")]
    pub reader_margins: Option<u32>,
    /// 漫画的真实可阅读范围（设成 `comic_reader_margins` 后实测）；没写就和 EPUB 的一样。
    #[serde(rename = "comic_readable")]
    readable: Option<Screen>,
    /// 漫画的翻页方向改成这个（`"ltr"` 或 `"rtl"`，写进 OPF 的 `page-progression-direction`）；不写就照原书。
    /// ireader 写 `"ltr"`：掌阅遇到从右往左翻的书会四周留边、整页图铺不满（2026-09-30 真机），用户选了铺满。
    #[serde(rename = "comic_page_direction")]
    pub page_direction: Option<String>,
    /// 漫画写成固定版式，画布是 `comic_readable`（Kindle：流式版式下阅读器强制留页边距，固定版式才能整页铺满，
    /// 2026-09-30 真机）。见 `bookconv::comicfxl`。
    #[serde(rename = "comic_fixed_layout")]
    pub fixed_layout: bool,
    /// 漫画用的产物格式（须在 `formats` 里）；不写就和文字书一样用 `formats` 的第一个。内置模式都不写（kindle 2026-10-05
    /// 曾经写 `azw3`，KFX 固定版式真机通过后去掉；AZW3 格式 2026-10-09 删了），留给自定义模式。
    #[serde(rename = "comic_format")]
    pub format: Option<Format>,
    /// 漫画页中间调加深的伽马（`255·(v/255)^γ`，黑、白两端不动；TOML 里不写＝不加深）。只加深内容是灰度的页：黑白屏上转成灰度的页
    /// 全算，彩色屏上只算本来就没有颜色的页（彩页原样）。2026-10-16 用户在 Move、掌阅上比过原样 / 1.2 / 1.3 / 1.45，选 1.45
    /// （第一轮 Move 上对比度、锐化、饱和度都看不出区别）。见 `bookconv::imgopt::prepare_comic_page_limited`。
    #[serde(rename = "comic_gamma")]
    pub gamma: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Profile {
    /// 文件名（不含 `.toml`），命令行与配置里用的标识。
    pub id: String,
    pub name: String,
    pub screen: Screen,
    pub color: bool,
    /// 首选产物格式在前。
    pub formats: Vec<Format>,
    /// 注释怎么呈现（[`NotePolicy`]）。TOML 里仍是 `notes`、`note_icons`、`note_backlinks`。
    #[serde(flatten)]
    pub notes: NotePolicy,
    /// 各格式在阅读器里的真实可阅读范围（像素，竖屏）；没有的格式用 `screen`。
    readable: BTreeMap<Format, Screen>,
    /// 漫画怎么排（[`ComicPolicy`]）。TOML 里仍是 `comic_` 开头的平铺字段。
    #[serde(flatten)]
    pub comic: ComicPolicy,
    /// 阅读器的 CSS 怪癖（认不认某种写法、怎么折算）：照 Kindle 改写的各项开关，见 [`CssQuirks`]。TOML 里仍是平铺的字段。
    #[serde(flatten)]
    pub css: CssQuirks,
    /// 保留 CSS 背景图（TOML 里不写是 `false`：清洗层去掉背景图，只留背景色——xochitl 不认 `no-repeat`，把背景图
    /// 平铺满页盖住正文）。kindle 写 `true`：KFX 写出器照 Amazon 的写法写背景图（2026-10-05）。
    pub background_images: bool,
    /// 保留背景图时也保留 `background-size`、`background-attachment`（TOML 里不写是 `true`）。Kindle 要靠 `fixed` 和尺寸
    /// 按整页铺背景图（去掉后只铺在内容那一块）；掌阅写了尺寸会把图挤变形，ireader 写 `false`（2026-10-05 真机）。
    pub background_sizing: bool,
    /// 带图注、按满宽显示会超出一页的图（竖长的人物图之类）给 `<img>` 写行内宽度百分比，图和图注排在同一页（TOML 里不写是
    /// `false`）。掌阅、Kindle 不认多看图集，图一张张排开撑满一页、图注掉到下一页；限高的写法掌阅都不认（`max-height`、
    /// `page-break-inside`），宽度百分比认（2026-10-06 掌阅真机 adb 截屏），KFX 写出器也只认宽度。见 `bookconv::capfit`。
    pub caption_fit: bool,
    /// 阅读器能正确显示图片的透明通道（TOML 里不写是 `true`）。Kindle（KFX）把正文图片透明的地方显示成黑色（《绍宋》章标题图，
    /// 2026-10-06 真机），写 `false`：正文 `<img>`/SVG `<image>` 用到的带透明像素的图先合成到白底（格式不变）。
    pub image_alpha: bool,
    /// 阅读器认 WebP 图片（TOML 里不写是 `true`）。Move 不认（2026-10-19 测试书「测试-WebP」：有损、无损、透明、动图、CSS 背景图、
    /// 行内小图、封面一律空白，同样的图存 PNG 正常；掌阅都认），写 `false`：文字书里的静态 WebP 无损转成 PNG（改名 `.png`、引用和
    /// `media-type` 一起改），动图不转（只取一帧会丢内容）。Kindle 不用：KFX 写出器读书时本来就把静态 WebP 转成 PNG。
    pub webp: bool,
    /// 文字书只做修复（TOML 里不写是 `false`）：升级成合规的 EPUB 3、修不规范的写法、目录到节，**书里的文字、图片、样式一概不动**
    /// （不解锁字体字号、不排版、不搬注释、不删空白页、不缩图）。Kindle、掌阅写 `true`（2026-10-08 用户定）；漫画不归它管。
    pub text_repair_only: bool,
    /// 只修复时仍处理注释链接，保证注释能点（TOML 里不写是 `false`）：注释搬进引用它的那一章、链接改成同文件锚点，按 `note_backlinks`
    /// 去掉回链，按 `note_icons` 把只有图标的标号换成数字。xochitl 写 `true`（2026-10-08 用户定：xochitl 只保障注释可跳）。
    pub repair_note_links: bool,
    /// 只修复的文字书照 Send to Kindle 的规则统一（TOML 里不写是 `false`）：标签缺省样式（`<p>` 上下 1em、标题字号……，
    /// `bookconv::uastyle`）、正文字体用阅读器字体、body 左右边距不要、文字对比度 4.5（`bookconv::wash::kindle_rules`）。
    /// ireader、xochitl 写 `true`（2026-10-08 用户定三台一套规则）。Kindle 不用：KFX 写出器照这套规则写。
    pub kindle_rules: bool,
    /// 文字书正文插图中间调加深的伽马（同 `comic_gamma` 的算法；TOML 里不写＝不加深）。只动正文 `<img>`/SVG `<image>` 用到、
    /// 没在 CSS 里当背景、内容是灰度、短边至少 [`MIN_GAMMA_IMAGE_SIDE`] 像素、没有透明通道的 JPEG/PNG；只修复的书也做（「图片不动」
    /// 的例外，2026-10-16 用户定）。用户在 Move、掌阅上比过，选 1.3。见 `bookconv::imgopt::darken_gray_image`。
    pub image_gamma: Option<f64>,
    /// 阅读器怎么定图片尺寸（TOML 里不写是 `css`），见 [`ImageSizing`]。ireader 写 `device-px`，xochitl 写 `percent`。
    pub image_sizing: ImageSizing,
    /// 阅读器缺省字号下 1em 是几个屏幕像素（只 kindle 写：掌阅、Move 照它把 em 写的图片宽度折成 Kindle 上的实际大小，
    /// 2026-10-12 测试书宽 10em 的图在 Kindle 上 392px）。
    pub font_px: Option<u32>,
    /// 阅读器缺省字号、行距下正文一行几个屏幕像素（kindle 写；掌阅也写，`line_height_numbers = false` 时按它算缺省行距）。Kindle：KFX 的竖直距离写成行高的倍数，Kindle 按行距换成像素，
    /// 一 rem 的竖直距离＝这个数 / 1.2（写出器按 1.2 倍行高折算）。掌阅、Move 照它把页首的上边距、背景图的位置折成 Kindle 上的
    /// 位置（2026-10-12 Kindle 截图：《飘》第一章正文行距 63px，「第一卷」上外边距 9.28 lh 落在 779px）。
    pub line_px: Option<u32>,
    /// 这个阅读器缺省字号下 CSS 的 1em 是几个屏幕像素（掌阅、Move 写）：`kindle_rules` 照它把竖直方向的边距换算成和 Kindle 上占页高
    /// 一样的比例（Kindle 按行距排边距，一 rem ≈ 52.5px；2026-10-12 测试书「测试-竖直间距」：掌阅 1em 37.5px、Move 37.8px）。
    pub em_px: Option<f64>,
    /// 产物送到哪台设备、怎么送；`None` 时产物留在电脑上。不影响产物内容（不进指纹）。
    pub deliver: Option<Deliver>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ProfileFile {
    name: String,
    screen: Screen,
    /// 以前的字段（屏幕像素密度），没有地方用，已去掉；写了也照收，旧的自定义 profile 不会因此报错。
    #[serde(default, rename = "ppi")]
    _ppi: Option<serde::de::IgnoredAny>,
    color: bool,
    formats: Vec<Format>,
    notes: Notes,
    #[serde(default)]
    note_icons: NoteIcons,
    #[serde(default = "yes")]
    note_backlinks: bool,
    #[serde(default)]
    readable: BTreeMap<Format, Screen>,
    comic_margin: Option<u32>,
    comic_reader_margins: Option<u32>,
    comic_readable: Option<Screen>,
    comic_page_direction: Option<String>,
    #[serde(default)]
    comic_fixed_layout: bool,
    comic_format: Option<Format>,
    comic_gamma: Option<f64>,
    #[serde(default)]
    background_images: bool,
    #[serde(default = "yes")]
    background_sizing: bool,
    #[serde(default = "yes")]
    css_rgba: bool,
    #[serde(default = "yes")]
    background_no_repeat: bool,
    #[serde(default = "yes")]
    margin_top: bool,
    #[serde(default = "yes")]
    background_auto_height: bool,
    #[serde(default = "yes")]
    display_none: bool,
    #[serde(default)]
    line_height_except_p: bool,
    #[serde(default)]
    line_height_floor: Option<f64>,
    #[serde(default)]
    em_root: bool,
    #[serde(default)]
    heading_center: bool,
    #[serde(default)]
    heading_font_keywords: bool,
    heading_sizes: Option<HeadingSizes>,
    page_top: Option<PageTop>,
    #[serde(default)]
    trailing_br_line: bool,
    #[serde(default = "yes")]
    inline_css_lengths: bool,
    #[serde(default = "yes")]
    padding: bool,
    #[serde(default = "yes")]
    borders: bool,
    #[serde(default = "yes")]
    heading_margins: bool,
    #[serde(default = "yes")]
    px_margins: bool,
    #[serde(default = "yes")]
    percent_margins: bool,
    #[serde(default = "yes")]
    block_width: bool,
    #[serde(default = "yes")]
    negative_margins: bool,
    #[serde(default = "yes")]
    cell_middle: bool,
    #[serde(default = "yes")]
    inline_valign: bool,
    #[serde(default = "yes")]
    cell_width: bool,
    #[serde(default = "yes")]
    margin_auto: bool,
    #[serde(default = "yes")]
    float: bool,
    #[serde(default)]
    horizontal_scale: bool,
    #[serde(default = "yes")]
    rem: bool,
    #[serde(default = "yes")]
    important: bool,
    #[serde(default = "yes")]
    line_height_numbers: bool,
    #[serde(default)]
    caption_fit: bool,
    #[serde(default = "yes")]
    image_alpha: bool,
    #[serde(default = "yes")]
    webp: bool,
    #[serde(default)]
    text_repair_only: bool,
    #[serde(default)]
    repair_note_links: bool,
    #[serde(default)]
    kindle_rules: bool,
    image_gamma: Option<f64>,
    #[serde(default)]
    image_sizing: ImageSizing,
    font_px: Option<u32>,
    #[serde(default)]
    line_px: Option<u32>,
    #[serde(default)]
    em_px: Option<f64>,
    deliver: Option<Deliver>,
}

fn yes() -> bool {
    true
}

impl Profile {
    /// 解析一份 profile TOML；`id` 由调用方给（通常是文件名）。
    pub fn parse(id: &str, toml_text: &str) -> Result<Profile, String> {
        let f: ProfileFile = toml::from_str(toml_text).map_err(|e| format!("profile {id}: {e}"))?;
        let p = Profile { id: id.to_string(), name: f.name, screen: f.screen, color: f.color, formats: f.formats, notes: NotePolicy { mode: f.notes, icons: f.note_icons, backlinks: f.note_backlinks }, readable: f.readable, comic: ComicPolicy { margin: f.comic_margin.unwrap_or(DEFAULT_COMIC_MARGIN), reader_margins: f.comic_reader_margins, readable: f.comic_readable, page_direction: f.comic_page_direction, fixed_layout: f.comic_fixed_layout, format: f.comic_format, gamma: f.comic_gamma }, css: CssQuirks { margin_top: f.margin_top, display_none: f.display_none, line_height_except_p: f.line_height_except_p, line_height_floor: f.line_height_floor, em_root: f.em_root, heading_center: f.heading_center, heading_font_keywords: f.heading_font_keywords, heading_sizes: f.heading_sizes, page_top: f.page_top, trailing_br_line: f.trailing_br_line, inline_css_lengths: f.inline_css_lengths, padding: f.padding, borders: f.borders, heading_margins: f.heading_margins, px_margins: f.px_margins, percent_margins: f.percent_margins, block_width: f.block_width, negative_margins: f.negative_margins, cell_middle: f.cell_middle, inline_valign: f.inline_valign, cell_width: f.cell_width, margin_auto: f.margin_auto, float: f.float, horizontal_scale: f.horizontal_scale, rem: f.rem, important: f.important, line_height_numbers: f.line_height_numbers, css_rgba: f.css_rgba, background_no_repeat: f.background_no_repeat, background_auto_height: f.background_auto_height }, background_images: f.background_images, background_sizing: f.background_sizing, caption_fit: f.caption_fit, image_alpha: f.image_alpha, webp: f.webp, text_repair_only: f.text_repair_only, repair_note_links: f.repair_note_links, kindle_rules: f.kindle_rules, image_gamma: f.image_gamma, image_sizing: f.image_sizing, font_px: f.font_px, line_px: f.line_px, em_px: f.em_px, deliver: f.deliver };
        p.validate()?;
        Ok(p)
    }

    fn validate(&self) -> Result<(), String> {
        let positive = |v: f64| v.is_finite() && v > 0.0;
        match &self.css.heading_sizes {
            None if self.css.heading_font_keywords => return Err(format!("profile {}: heading_font_keywords = true 要写 [heading_sizes]", self.id)),
            Some(h) if h.keywords.is_empty() || !h.keywords.values().chain(&h.tags).all(|&v| positive(v)) => {
                return Err(format!("profile {}: [heading_sizes] 的 keywords 须非空，大小都须是正数", self.id));
            }
            _ => {}
        }
        match &self.css.page_top {
            None if !self.css.margin_top => return Err(format!("profile {}: margin_top = false 要写 [page_top]", self.id)),
            Some(t) if !(positive(t.gap_heading_per_em) && t.gap_other >= 0.0 && t.gap_other.is_finite() && positive(t.strut_base_per_em)) => {
                return Err(format!("profile {}: [page_top] 的尺寸须是正数", self.id));
            }
            _ => {}
        }
        if !self.css.line_height_numbers && !(self.line_px.is_some_and(|l| l > 0) && self.em_px.is_some_and(positive)) {
            return Err(format!("profile {}: line_height_numbers = false 要写 line_px、em_px", self.id));
        }
        for (key, g) in [("comic_gamma", self.comic.gamma), ("image_gamma", self.image_gamma)] {
            if g.is_some_and(|g| !(0.2..=5.0).contains(&g)) {
                return Err(format!("profile {}: {key} 须在 0.2–5 之间（1 是不变，大于 1 加深中间调）", self.id));
            }
        }
        if let Some(d) = self.comic.page_direction.as_deref().filter(|d| !matches!(*d, "ltr" | "rtl")) {
            return Err(format!("profile {}: comic_page_direction 只能是 ltr 或 rtl，不是 {d}", self.id));
        }
        match &self.deliver {
            Some(Deliver::Mtp { mount, dir }) => {
                let bad = |v: &str| v.is_empty() || v.starts_with('.') || v.contains(['/', '\\']);
                if bad(mount) || bad(dir) {
                    return Err(format!("profile {}: [deliver] 的 mount、dir 须是一级目录名（非空、不以 . 开头、不含斜杠）", self.id));
                }
            }
            Some(Deliver::Xochitl { hosts, port }) if hosts.is_empty() || hosts.iter().any(|h| h.is_empty() || h.starts_with('-') || h.contains(char::is_whitespace)) || *port == 0 => {
                return Err(format!("profile {}: [deliver] 的 hosts 须非空（每项是 用户@地址）、port 非零", self.id));
            }
            Some(Deliver::Xochitl { .. }) => {}
            None => {}
        }
        let Screen { width, height } = self.screen;
        if self.id.is_empty() {
            return Err("profile id 为空".into());
        }
        // id 是产物目录名（`<跟踪目录旁>/<id>/`）和生成记录的文件名（`output-state/<id>.json`）：`.` 开头的记录文件书库不认
        // （删书时删不到这个模式的产物），`.tmp-` 开头的还会被当成残留临时文件清掉；`--device=` 按逗号分开
        if self.id.starts_with('.') || self.id.contains([',', '/', '\\']) {
            return Err(format!("profile id {:?} 不能以 . 开头、不能含逗号或斜杠（改文件名）", self.id));
        }
        if width == 0 || height == 0 || width > height {
            return Err(format!("profile {}: screen 须为竖屏且非零（width <= height），实际 {width}x{height}", self.id));
        }
        if self.formats.is_empty() {
            return Err(format!("profile {}: formats 为空", self.id));
        }
        if let Some((i, f)) = self.formats.iter().enumerate().find(|(i, f)| self.formats[..*i].contains(f)) {
            return Err(format!("profile {}: formats 第 {} 项 {f:?} 重复", self.id, i + 1));
        }
        if let Some(f) = self.comic.format.filter(|f| !self.formats.contains(f)) {
            return Err(format!("profile {}: comic_format {f:?} 不在 formats 里", self.id));
        }
        for (fmt, r) in &self.readable {
            if !self.formats.contains(fmt) {
                return Err(format!("profile {}: readable 里的 {fmt:?} 不在 formats 里", self.id));
            }
            // 竖屏（宽 ≤ 高）：长边、短边（`Screen::long_edge`/`short_edge`）和漫画排版都按竖屏算，写反了会把横竖框选反
            if r.width == 0 || r.height == 0 || r.width > width || r.height > height || r.width > r.height {
                return Err(format!("profile {}: readable {fmt:?} 须为竖屏（宽 ≤ 高）、非零且不超过屏幕 {width}x{height}，实际 {}x{}", self.id, r.width, r.height));
            }
        }
        if let Some(r) = self.comic.readable {
            if r.width == 0 || r.height == 0 || r.width > width || r.height > height || r.width > r.height {
                return Err(format!("profile {}: comic_readable 须为竖屏（宽 ≤ 高）、非零且不超过屏幕 {width}x{height}，实际 {}x{}", self.id, r.width, r.height));
            }
            if self.comic.margin >= r.width.min(r.height) / 4 {
                return Err(format!("profile {}: comic_margin {} 须小于漫画阅读范围短边的 1/4（{}x{}）", self.id, self.comic.margin, r.width, r.height));
            }
        }
        // 漫画白边按每种格式的阅读范围（没有实测值的格式用屏幕）都要够小：留给图的框至少是阅读范围的一半
        for fmt in &self.formats {
            let r = self.readable(*fmt);
            if self.comic.margin >= r.width.min(r.height) / 4 {
                return Err(format!("profile {}: comic_margin {} 须小于 {fmt:?} 阅读范围短边的 1/4（{}x{}）", self.id, self.comic.margin, r.width, r.height));
            }
        }
        Ok(())
    }

    /// 产物为 `format` 时的真实可阅读范围：有内置实测值用实测值，否则用标称屏幕。
    pub fn readable(&self, format: Format) -> Screen {
        self.readable.get(&format).copied().unwrap_or(self.screen)
    }

    /// 产物格式：`formats` 的第一个。
    pub fn format(&self) -> Format {
        self.formats[0]
    }

    /// 一本书的产物格式：漫画用 `comic_format`（有的话），别的用 [`Profile::format`]。
    pub fn format_for(&self, comic: bool) -> Format {
        match self.comic.format {
            Some(f) if comic => f,
            _ => self.format(),
        }
    }

    /// 产物格式的阅读范围（[`Profile::readable`]`(self.format())`）。
    pub fn output_readable(&self) -> Screen {
        self.readable(self.format())
    }

    /// 漫画页排版用的阅读范围：`comic_readable`，没写就是产物格式的阅读范围。
    pub fn comic_readable(&self) -> Screen {
        self.comic.readable.unwrap_or_else(|| self.output_readable())
    }
}

/// 一组 profile，按 id 排序。
#[derive(Debug, Clone, Default)]
pub struct Registry {
    profiles: BTreeMap<String, Profile>,
}

impl Registry {
    /// 内置 profile（`profiles/*.toml`）。
    pub fn builtin() -> &'static Registry {
        static REG: OnceLock<Registry> = OnceLock::new();
        REG.get_or_init(|| {
            let mut r = Registry::default();
            for (id, text) in BUILTIN {
                let p = Profile::parse(id, text).unwrap_or_else(|e| panic!("内置 {e}"));
                r.profiles.insert(p.id.clone(), p);
            }
            r
        })
    }

    /// 内置 profile 加上 `dir` 下的 `*.toml`，同 id 以 `dir` 里的为准。
    pub fn with_dir(dir: &Path) -> Result<Registry, String> {
        let mut r = Registry::builtin().clone();
        let entries = std::fs::read_dir(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        for e in entries {
            let path = e.map_err(|e| e.to_string())?.path();
            // 隐藏文件（编辑器的锁文件、备份等）不算 profile
            if path.extension().is_none_or(|x| x != "toml") || path.file_name().is_some_and(|n| n.as_encoded_bytes().starts_with(b".")) {
                continue;
            }
            let id = path.file_stem().and_then(|s| s.to_str()).ok_or_else(|| format!("{}: 文件名不是 UTF-8", path.display()))?;
            let text = std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
            let p = Profile::parse(id, &text)?;
            r.profiles.insert(p.id.clone(), p);
        }
        Ok(r)
    }

    pub fn get(&self, id: &str) -> Option<&Profile> {
        self.profiles.get(id)
    }

    pub fn iter(&self) -> impl Iterator<Item = &Profile> {
        self.profiles.values()
    }
}

/// 三台统一照着排的那台设备（内置 kindle，2026-10-12 用户定「三台以 Kindle 为准」）：掌阅、Move 的图片大小、竖直间距按它的
/// 阅读范围和字号换算。「以谁为准」只在这里定；用到它的地方（`bookconv::optimize::OptimizeOpts::for_profile`、书库的指纹）都从这里取，
/// 单测、自定义调用可以用 `for_profile_against` 另给。
pub fn reference() -> Option<&'static Profile> {
    get("kindle")
}

/// 内置 profile 按 id 取。
pub fn get(id: &str) -> Option<&'static Profile> {
    Registry::builtin().get(id)
}

/// 命令行参数里的 `--device=<id>`（必填）→ 内置 profile。缺失或未知 id 时 `Err` 带上可选 id 列表，供调用方报用法错。
pub fn device_from_args<S: AsRef<str>>(args: &[S]) -> Result<&'static Profile, String> {
    let id = args.iter().find_map(|a| a.as_ref().strip_prefix("--device="));
    id.and_then(get).ok_or_else(|| {
        let ids: Vec<_> = Registry::builtin().iter().map(|p| p.id.as_str()).collect();
        format!("需要 --device=<设备>，可选: {}", ids.join(" / "))
    })
}

#[cfg(test)]
mod tests {
    /// 给 `Profile` 加字段会让这里编译不过：逼着决定新字段进不进产物指纹（`library` 的 `fingerprint.rs`）。
    /// 指纹没变就不重传，所以漏了＝改了规则书不重建，多了＝白白重传全部书。分类：
    /// - 直接：`library/src/fingerprint.rs` 里有它自己的一段；
    /// - 阅读器模型：进 `bookconv::reader_model::ReaderModel`，由 `m` 段的签名带上；
    /// - 否：不影响产物内容。
    ///
    /// （2026-10-15 对拍发现 `background_auto_height`、`margin_top` 改值时产物会变但指纹不变，已补进 `kindle_rules` 段的 `h`、`p`。）
    #[test]
    fn every_field_has_a_fingerprint_decision() {
        let p = get("kindle").unwrap();
        let Profile {
            // 直接
            id: _,
            screen: _,
            color: _,
            formats: _,
            readable: _,
            notes: NotePolicy { mode: _, icons: _, backlinks: _ },
            comic: ComicPolicy { margin: _, reader_margins: _, readable: _, page_direction: _, fixed_layout: _, format: _, gamma: _ },
            background_images: _,
            background_sizing: _,
            caption_fit: _,
            image_alpha: _,
            webp: _,
            text_repair_only: _,
            repair_note_links: _,
            kindle_rules: _,
            image_gamma: _,
            image_sizing: _,
            font_px: _,
            // 阅读器模型（也有的同时直接进）
            em_px: _,
            line_px: _,
            // 否
            name: _,
            deliver: _,
            css,
        } = p.clone();
        let CssQuirks {
            css_rgba: _,
            background_no_repeat: _,
            display_none: _,
            line_height_except_p: _,
            line_height_floor: _,
            line_height_numbers: _,
            heading_font_keywords: _,
            heading_sizes: _,
            heading_center: _,
            margin_top: _,
            page_top: _,
            trailing_br_line: _,
            inline_css_lengths: _,
            padding: _,
            borders: _,
            heading_margins: _,
            px_margins: _,
            percent_margins: _,
            block_width: _,
            negative_margins: _,
            cell_middle: _,
            inline_valign: _,
            cell_width: _,
            margin_auto: _,
            float: _,
            horizontal_scale: _,
            em_root: _,
            rem: _,
            important: _,
            background_auto_height: _,
        } = css;
    }

    use super::*;

    /// 漫画阅读范围没写时和产物格式的一样；改了的克隆不影响内置的。
    #[test]
    fn comic_readable_falls_back_to_output_readable() {
        let mut p = get("xochitl").unwrap().clone();
        assert_eq!((p.comic.reader_margins, p.comic_readable()), (Some(1), Screen { width: 952, height: 1457 }));
        p.comic.reader_margins = None;
        p.comic.readable = None;
        assert_eq!(p.comic_readable(), Screen { width: 842, height: 1455 });
        p.readable.insert(Format::Epub, Screen { width: 800, height: 1400 });
        assert_eq!((p.output_readable(), p.comic_readable()), (Screen { width: 800, height: 1400 }, Screen { width: 800, height: 1400 }));
        assert_eq!(get("xochitl").unwrap().comic.reader_margins, Some(1));
    }

    /// 去掉了的 `ppi` 键：写了照收（不论值），不写也行。
    #[test]
    fn dropped_ppi_key_is_still_accepted() {
        let rest = "name = \"x\"\ncolor = false\nformats = [\"epub\"]\nnotes = \"jump\"\n[screen]\nwidth = 1000\nheight = 1500\n";
        assert!(Profile::parse("x", rest).is_ok());
        assert!(Profile::parse("x", &format!("ppi = 300\n{rest}")).is_ok());
        assert!(Profile::parse("x", &format!("ppi = 0\n{rest}")).is_ok());
        assert!(Profile::parse("x", &format!("dpi = 300\n{rest}")).is_err(), "别的不认识的键照样报错");
    }

    #[test]
    fn builtin_profiles_parse() {
        let ids: Vec<_> = Registry::builtin().iter().map(|p| p.id.as_str()).collect();
        assert_eq!(ids, ["ireader", "kindle", "xochitl"]);
        let k = get("kindle").unwrap();
        assert_eq!((k.format(), k.output_readable()), (Format::Kfx, Screen { width: 1104, height: 1546 }), "Kindle 自带阅读器真机实测");
        assert_eq!((k.format_for(false), k.format_for(true)), (Format::Kfx, Format::Kfx), "kindle：文字书、漫画都出 KFX");
        assert!(k.background_images && get("ireader").unwrap().background_images && !get("xochitl").unwrap().background_images, "背景图：kindle、ireader 保留，xochitl 去掉");
        assert!(k.background_sizing && !get("ireader").unwrap().background_sizing, "背景图尺寸：kindle 保留，ireader 去掉");
        assert!(k.css.css_rgba && !get("ireader").unwrap().css.css_rgba && get("xochitl").unwrap().css.css_rgba, "rgba()：只有掌阅不认");
        assert!(k.css.background_no_repeat && get("ireader").unwrap().css.background_no_repeat && !get("xochitl").unwrap().css.background_no_repeat, "背景图 no-repeat：只有 xochitl 不认");
        assert!(k.css.margin_top && get("ireader").unwrap().css.margin_top && !get("xochitl").unwrap().css.margin_top, "margin-top：只有 xochitl 不认");
        assert!(k.css.background_auto_height && !get("ireader").unwrap().css.background_auto_height && get("xochitl").unwrap().css.background_auto_height, "背景尺寸的 auto 高：只有掌阅不认");
        assert_eq!((k.image_sizing, get("ireader").unwrap().image_sizing, get("xochitl").unwrap().image_sizing), (ImageSizing::Css, ImageSizing::DevicePx, ImageSizing::Percent));
        assert!(k.font_px.is_some(), "掌阅、Move 折算图片宽度要 Kindle 的 1em 像素数");
        assert!(k.caption_fit && get("ireader").unwrap().caption_fit && !get("xochitl").unwrap().caption_fit, "图注同页：kindle、ireader 开");
        assert!(!k.image_alpha && get("ireader").unwrap().image_alpha && get("xochitl").unwrap().image_alpha, "透明图合成白底：只有 kindle");
        assert!(k.text_repair_only && get("ireader").unwrap().text_repair_only && get("xochitl").unwrap().text_repair_only, "文字书只修复：三个模式都是");
        assert!(!k.repair_note_links && !get("ireader").unwrap().repair_note_links && get("xochitl").unwrap().repair_note_links, "只修复时仍保证注释可跳：xochitl");
        assert!(!k.kindle_rules && get("ireader").unwrap().kindle_rules && get("xochitl").unwrap().kindle_rules, "标签缺省样式：掌阅、Move 写进书里，Kindle 由写出器排");
        let i = get("ireader").unwrap();
        assert_eq!((i.format(), i.output_readable()), (Format::Epub, Screen { width: 1264, height: 1680 }), "掌阅整页图铺满整屏");
        assert!(!k.color && !i.color);
        let x = get("xochitl").unwrap();
        assert_eq!(x.screen, Screen { width: 954, height: 1696 });
        assert_eq!(x.readable(Format::Epub), Screen { width: 842, height: 1455 }, "EPUB 用实测阅读范围");
        assert!(x.color);
        assert_eq!(x.formats, [Format::Epub]);
        assert_eq!((k.comic.margin, i.comic.margin, x.comic.margin), (1, 1, 0));
        assert_eq!(x.notes.icons, NoteIcons::Number);
        assert_eq!((k.comic.reader_margins, x.comic.reader_margins), (None, Some(1)));
        assert_eq!((k.comic_readable(), x.comic_readable()), (Screen { width: 1272, height: 1696 }, Screen { width: 952, height: 1457 }), "Kindle 漫画固定版式整屏");
        assert!(k.comic.fixed_layout && !i.comic.fixed_layout && !x.comic.fixed_layout);
        assert_eq!((k.notes.backlinks, i.notes.backlinks, x.notes.backlinks), (true, true, false), "只有 xochitl 去注释回链");
        assert_eq!((k.comic.page_direction.as_deref(), i.comic.page_direction.as_deref(), x.comic.page_direction.as_deref()), (None, None, None), "三台漫画都照书的翻页方向（掌阅往右翻的书要在阅读器里选「漫画」排版才铺满，2026-10-15）");
        assert!(get("nope").is_none());
    }

    #[test]
    fn device_from_args_finds_id_or_lists_choices() {
        assert_eq!(device_from_args(&["a.epub", "--device=xochitl"]).unwrap().id, "xochitl");
        let err = device_from_args(&["--device=nope"]).unwrap_err();
        assert!(err.contains("kindle") && err.contains("--device="), "{err}");
        assert!(device_from_args::<&str>(&[]).is_err());
    }

    #[test]
    fn rejects_landscape_and_unknown_keys() {
        let base = "name = \"x\"\nppi = 300\ncolor = false\nformats = [\"epub\"]\nnotes = \"jump\"\n";
        assert!(Profile::parse("x", &format!("{base}[screen]\nwidth = 1680\nheight = 1264\n")).is_err());
        assert!(Profile::parse("x", &format!("{base}extra = 1\n[screen]\nwidth = 10\nheight = 20\n")).is_err());
        assert!(Profile::parse("x", &format!("{base}[screen]\nwidth = 10\nheight = 20\n")).is_ok(), "comic_margin 缺省 1，要小于短边的 1/4");
        for bad in [".hidden", ".tmp-x", "a,b"] {
            assert!(Profile::parse(bad, &format!("{base}[screen]\nwidth = 10\nheight = 20\n")).is_err(), "id {bad} 不行");
        }
        assert!(Profile::parse("x", &format!("{base}comic_page_direction = \"up\"\n[screen]\nwidth = 100\nheight = 200\n")).is_err(), "翻页方向只能 ltr/rtl");
        let scr = "[screen]\nwidth = 100\nheight = 200\n";
        assert!(Profile::parse("x", &format!("{base}{scr}[readable.epub]\nwidth = 90\nheight = 180\n")).is_ok());
        assert!(Profile::parse("x", &format!("{base}{scr}[readable.epub]\nwidth = 101\nheight = 180\n")).is_err(), "阅读范围不能超过屏幕");
        // 阅读范围写反（横的）：屏幕是竖的，阅读范围也必须是竖的
        let wide = "[screen]\nwidth = 200\nheight = 200\n";
        assert!(Profile::parse("x", &format!("{base}{wide}[readable.epub]\nwidth = 200\nheight = 199\n")).is_err(), "readable 横的报错");
        assert!(Profile::parse("x", &format!("{base}{wide}[readable.epub]\nwidth = 199\nheight = 199\n")).is_ok(), "方的可以");
        assert!(Profile::parse("x", &format!("{base}{wide}[comic_readable]\nwidth = 200\nheight = 150\n")).is_err(), "comic_readable 横的报错");
        assert!(Profile::parse("x", &format!("{base}{wide}[comic_readable]\nwidth = 150\nheight = 200\n")).is_ok());
        assert!(Profile::parse("x", &format!("{base}{scr}[readable.pdf]\nwidth = 90\nheight = 180\n")).is_err(), "不再支持的格式（pdf）报错");
        assert!(Profile::parse("x", &format!("name = \"x\"\nppi = 300\ncolor = false\nformats = [\"kfx\"]\nnotes = \"jump\"\n{scr}")).is_ok(), "kfx 可以");
        for fmt in ["pdf", "mobi", "azw3"] {
            let old = format!("name = \"x\"\nppi = 300\ncolor = false\nformats = [\"{fmt}\"]\nnotes = \"jump\"\n{scr}");
            assert!(Profile::parse("x", &old).is_err(), "formats 里写 {fmt} 报错");
        }
        let dup = "name = \"x\"\nppi = 300\ncolor = false\nformats = [\"epub\", \"epub\"]\nnotes = \"jump\"\n";
        assert!(Profile::parse("x", &format!("{dup}{scr}")).is_err(), "formats 不能重复");
    }

    #[test]
    fn comic_margin_defaults_to_one_and_is_validated() {
        let base = "name = \"x\"\nppi = 300\ncolor = false\nformats = [\"epub\"]\nnotes = \"jump\"\n";
        let scr = "[screen]\nwidth = 100\nheight = 200\n";
        assert_eq!(Profile::parse("x", &format!("{base}{scr}")).unwrap().comic.margin, DEFAULT_COMIC_MARGIN);
        assert_eq!(Profile::parse("x", &format!("{base}comic_margin = 0\n{scr}")).unwrap().comic.margin, 0);
        assert_eq!(Profile::parse("x", &format!("{base}comic_margin = 24\n{scr}")).unwrap().comic.margin, 24);
        assert!(Profile::parse("x", &format!("{base}comic_margin = 25\n{scr}")).is_err(), "短边 100 的 1/4 = 25，不能等于");
        // 按阅读范围算，不按屏幕：阅读范围 80 宽时上限是 20
        let rd = "[readable.epub]\nwidth = 80\nheight = 180\n";
        assert!(Profile::parse("x", &format!("{base}comic_margin = 19\n{scr}{rd}")).is_ok());
        assert!(Profile::parse("x", &format!("{base}comic_margin = 20\n{scr}{rd}")).is_err());
        assert!(Profile::parse("x", &format!("{base}comic_margin = -1\n{scr}")).is_err(), "负数报错");
    }

    #[test]
    fn dir_overrides_and_adds() {
        let dir = std::env::temp_dir().join(format!("profile-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let body = |w: u32| format!("name = \"t\"\nppi = 212\ncolor = false\nformats = [\"epub\"]\nnotes = \"jump\"\n[screen]\nwidth = {w}\nheight = 1448\n");
        std::fs::write(dir.join("xochitl.toml"), body(1072)).unwrap();
        std::fs::write(dir.join("new-dev.toml"), body(1000)).unwrap();
        std::fs::write(dir.join("README.md"), "ignored").unwrap();
        std::fs::write(dir.join(".#xochitl.toml"), "编辑器锁文件").unwrap();
        let r = Registry::with_dir(&dir).unwrap();
        std::fs::remove_dir_all(&dir).unwrap();
        assert_eq!(r.get("xochitl").unwrap().screen.width, 1072);
        assert_eq!(r.get("new-dev").unwrap().screen.width, 1000);
        assert_eq!(r.iter().count(), Registry::builtin().iter().count() + 1, "覆盖同 id、新增一台");
    }
}
