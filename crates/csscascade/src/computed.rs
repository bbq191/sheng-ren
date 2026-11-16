//! 计算值：`Computed` 与各属性组，以及正文字号的统计。

use crate::shorthand::{parse_bg_position, parse_bg_size, BORDER_STYLES};
use crate::text::{split_top, tokens};
use crate::value::{parse_box_len, parse_len, Len, PT_PER_BOX_PX, PT_PER_EM};
use crate::{leading_url, parse_color, Decls};
use bookbase::color::over_white;
use std::collections::HashMap;

/// 元素的计算值（只留 KFX 用得上的）。字号 `font_size` 是相对根字号的倍数。
#[derive(Clone, Debug, PartialEq)]
pub struct Computed {
    pub font_family: Option<String>,
    /// `font-family` 第一个以外的备选（小写、逗号连接，Send to Kindle 照写整串：`default,st,宋体,zw,sans-serif`）。
    pub font_fallbacks: Option<String>,
    pub font_size: f64,
    pub bold: bool,
    /// 字重 600（Send to Kindle 写成半粗 `$360`，《绍宋》`p.ganyan1`）。`bold` 同时为真（挑字体文件时当粗体）。
    pub semibold: bool,
    /// `<b>`/`<strong>` 自己、样式表没写字重：HTML 缺省的 `bolder`（Send to Kindle 写 `$362`，不继承）。
    pub bolder: bool,
    /// 自己或祖先的样式表写了 `font-weight`（Send to Kindle 照写，normal 也写 `$350`：《罗杰疑案》`font-weight:normal` 的 h1）。
    pub weight_declared: bool,
    pub italic: bool,
    pub text_align: Option<String>,
    pub text_indent: Option<Len>,
    /// 行高，单位：元素字号的倍数；`None`＝normal。
    pub line_height: Option<f64>,
    /// 写成长度（`%`、em、px、pt）的行高按 CSS 算成绝对值往下继承（根 em）；没有数字倍数那样跟着子元素字号变
    /// （《疯探》body `line-height:130%`、作者行 `font-size:1.2em`，Send to Kindle 写 0.833lh）。
    pub line_height_abs: Option<f64>,
    pub color: Option<u32>,
    pub lang: Option<String>,
    pub superscript: bool,
    /// 行内元素的基线偏移（往上为正，本元素字号的长度）：`vertical-align` 写的长度、`position:relative` 的 `top`（取负）/`bottom`
    /// （Send to Kindle 写成 `$31`：2026-10-14 怪癖测试书 Q122 `vertical-align:0.5em`、Q123 `position:relative;top:-0.5em` 都写 0.5em）。
    /// 不继承。
    pub baseline_shift: Option<Len>,
    pub subscript: bool,
    /// 下划线、删除线、上划线（CSS 里不继承，但画在子元素上，按继承处理）。
    pub decoration: [bool; 3],
    pub small_caps: bool,
    /// 字间距，单位：元素字号的倍数。
    pub letter_spacing: Option<f64>,
    /// `white-space: pre`（`<pre>`）：空格、换行原样保留。
    pub pre: bool,
    /// `white-space: nowrap`：不自动换行（Amazon 写成 `$45: true`，《绍宋》卷首语、信件里居中的诗句）。
    pub nowrap: bool,
    /// `word-break: break-all`（Send to Kindle 写成 `$569: $570`，《绍宋》全书 `p{word-break:break-all}`）。
    pub break_all: bool,
    /// 自己或祖先有背景色、背景图（Send to Kindle 只在没有背景时省掉近黑的文字颜色，见 KFX 写出器 `crates/kfx/src/write/style.rs` 的 `text_props`）。
    pub on_background: bool,
    /// 自己或祖先有背景图：只有背景图、没有背景色的地方不按对比度调文字颜色（Send to Kindle 同样：《雪国》扉页背景图上的白字照写）。
    pub on_image: bool,
    /// 自己或祖先（含 body）有左右外边距、内边距或边框：Send to Kindle 把首行缩进写成百分比（见 KFX 写出器 `crates/kfx/src/write/style.rs` 的 `block_props`）。
    pub in_hbox: bool,
    /// `list-style-type`（`None`＝按标签缺省）、`list-style-position: inside`。
    pub list_style: Option<String>,
    pub list_inside: bool,
    /// 表格：`border-collapse: collapse`、`border-spacing`（这两个 CSS 里是继承的）。
    pub border_collapse: bool,
    pub border_spacing: Option<Len>,
    // 不继承的
    pub display: Option<String>,
    pub margin: [Option<Len>; 4],
    /// 外边距写的是 `auto`（上、右、下、左）：有宽度的块按左右 auto 对齐（Send to Kindle 写 `$580`）。
    pub margin_auto: [bool; 4],
    pub padding: [Option<Len>; 4],
    pub background: Option<u32>,
    /// 文字底下实际的背景色（自己或最近的祖先的，不透明；`None`＝白页面）：Send to Kindle 按它保证文字对比度。
    pub backdrop: Option<u32>,
    /// `box-shadow`、`text-shadow`（第一个；x、y、模糊、颜色）。Send to Kindle 写 `$496`、`$497`。
    pub box_shadow: Option<Shadow>,
    pub text_shadow: Option<Shadow>,
    /// 四边边框（上、右、下、左）。
    pub border: [Option<Border>; 4],
    /// 圆角：左上、右上、右下、左下。
    pub radius: [Option<Len>; 4],
    pub width: Option<Len>,
    /// `min-height`（Send to Kindle 写 `$62`，《恶女的告白》`min-height:2em`）。
    pub min_height: Option<Len>,
    /// 单元格的 `vertical-align`。
    pub valign: Option<String>,
    /// 背景图（书内路径）、不重复、固定、位置 (x, y)、尺寸 (宽, 高)。
    pub bg_image: Option<String>,
    pub bg_no_repeat: bool,
    pub bg_fixed: bool,
    pub bg_position: [Option<Len>; 2],
    pub bg_size: [Option<Len>; 2],
    /// `background-size: cover`（`bg_size` 里和 `100% 100%` 写法一样，这里另记）：页面一级的背景，Send to Kindle 让它铺满一页。
    pub bg_cover: bool,
    /// 强制分页：`page-break-before`/`break-before` 写了 `always`、`page`、`left`、`right`（Send to Kindle 在这里另起一节，
    /// 2026-10-12《飘》版权页后的二维码页）；`after` 同样。不继承。
    pub break_before: bool,
    pub break_after: bool,
}

/// 阴影：x、y 偏移，模糊半径，颜色（没写＝`None`）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Shadow {
    pub x: Len,
    pub y: Len,
    pub blur: Len,
    pub color: Option<u32>,
}

/// `box-shadow`/`text-shadow` 的第一个阴影（`inset`、`none` 不收）。长度的 px 按 1px＝0.45pt。
pub fn parse_shadow(v: &str) -> Option<Shadow> {
    let first = split_top(v, ',')
        .into_iter()
        .next()?
        .trim()
        .to_ascii_lowercase();
    if first.is_empty() || first == "none" || first.contains("inset") {
        return None;
    }
    let mut lens = Vec::new();
    let mut color = None;
    for tok in tokens(&first).0 {
        match parse_box_len(tok) {
            Some(l)
                if tok.starts_with(|c: char| {
                    c.is_ascii_digit() || c == '-' || c == '.' || c == '+'
                }) =>
            {
                lens.push(l)
            }
            _ => color = parse_color(tok).or(color),
        }
    }
    (lens.len() >= 2).then(|| Shadow {
        x: lens[0],
        y: lens[1],
        blur: lens.get(2).copied().unwrap_or(Len::Pt(0.0)),
        color,
    })
}

/// 一条边：样式（`solid` 等，不含 none）、宽度、颜色（没写＝当前颜色）。
#[derive(Clone, Debug, PartialEq)]
pub struct Border {
    pub style: String,
    pub width: BorderWidth,
    pub color: Option<u32>,
}

/// 边框宽度：pt 或 em（样本里 px 一律按 1px＝0.45pt 换算，thin/medium/thick＝1/3/5px）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum BorderWidth {
    Pt(f64),
    Em(f64),
}

pub fn parse_border_width(v: &str) -> Option<BorderWidth> {
    let v = v.trim().to_ascii_lowercase();
    match v.as_str() {
        "thin" => return Some(BorderWidth::Pt(0.45)),
        "medium" => return Some(BorderWidth::Pt(1.35)),
        "thick" => return Some(BorderWidth::Pt(2.25)),
        _ => {}
    }
    if let Some(n) = v.strip_suffix("px") {
        return n
            .trim()
            .parse::<f64>()
            .ok()
            .map(|n| BorderWidth::Pt(n * PT_PER_BOX_PX));
    }
    match parse_len(&v)? {
        Len::Em(n) => Some(BorderWidth::Em(n)),
        Len::Pt(n) => Some(BorderWidth::Pt(n)),
        Len::Percent(_) => None,
    }
}

impl Computed {
    pub fn has_border(&self) -> bool {
        self.border.iter().any(|b| {
            b.as_ref()
                .is_some_and(|b| b.width != BorderWidth::Pt(0.0) && b.width != BorderWidth::Em(0.0))
        })
    }
}

impl Computed {
    pub fn root() -> Self {
        Computed {
            font_family: None,
            font_fallbacks: None,
            font_size: 1.0,
            bold: false,
            semibold: false,
            bolder: false,
            weight_declared: false,
            italic: false,
            text_align: None,
            text_indent: None,
            line_height: None,
            line_height_abs: None,
            color: None,
            lang: None,
            superscript: false,
            baseline_shift: None,
            subscript: false,
            decoration: [false; 3],
            small_caps: false,
            letter_spacing: None,
            pre: false,
            nowrap: false,
            break_all: false,
            on_background: false,
            on_image: false,
            in_hbox: false,
            list_style: None,
            list_inside: false,
            border_collapse: false,
            border_spacing: None,
            display: None,
            margin: [None; 4],
            margin_auto: [false; 4],
            padding: [None; 4],
            background: None,
            backdrop: None,
            box_shadow: None,
            text_shadow: None,
            border: [None, None, None, None],
            radius: [None; 4],
            width: None,
            min_height: None,
            valign: None,
            bg_image: None,
            bg_no_repeat: false,
            bg_fixed: false,
            bg_position: [None; 2],
            bg_size: [None; 2],
            bg_cover: false,
            break_before: false,
            break_after: false,
        }
    }

    fn reset_box(&self) -> Computed {
        Computed {
            display: None,
            margin: [None; 4],
            margin_auto: [false; 4],
            padding: [None; 4],
            background: None,
            box_shadow: None,
            border: [None, None, None, None],
            radius: [None; 4],
            width: None,
            min_height: None,
            valign: None,
            bg_image: None,
            bg_no_repeat: false,
            bg_fixed: false,
            bg_position: [None; 2],
            bg_size: [None; 2],
            break_before: false,
            break_after: false,
            bg_cover: false,
            bolder: false,
            baseline_shift: None,
            ..self.clone()
        }
    }

    /// 只留继承的属性（匿名块用：外边距、内边距、背景属于包着它的元素）。
    pub fn inherited(&self) -> Computed {
        self.reset_box()
    }

    /// 由父元素的计算值和本元素的声明算出本元素的计算值。
    pub fn derive(parent: &Computed, decls: &Decls, tag: &str) -> Computed {
        let mut c = parent.reset_box();
        c.on_background =
            parent.on_background || parent.background.is_some() || parent.bg_image.is_some();
        c.on_image = parent.on_image || parent.bg_image.is_some();
        // 各组属性按固定顺序算（后面的组要用前面算好的值：字号先于行高、字间距的换算，外边距在标签缺省值之后覆盖）。
        Computed::tag_defaults(&mut c, parent, tag);
        Computed::font_props(&mut c, parent, decls);
        Computed::text_layout_props(&mut c, parent, decls);
        Computed::text_style_props(&mut c, decls, tag);
        Computed::list_table_props(&mut c, decls);
        Computed::box_props(&mut c, parent, decls);
        Computed::background_props(&mut c, parent, decls);
        c
    }

    /// 标签的缺省样式（UA 样式表）：粗体、斜体、标题字号与外边距、段落等块的上下外边距、引文的左右缩进。
    /// 要在读声明之前：写了的声明覆盖这些缺省值。
    fn tag_defaults(c: &mut Computed, parent: &Computed, tag: &str) {
        // 标签的缺省样式（阅读器的 UA 样式表里有的）。
        match tag {
            "b" | "strong" => {
                c.bold = true;
                c.semibold = false;
                c.bolder = true;
            }
            "th" => {
                c.bold = true;
                c.semibold = false;
                c.text_align = Some("center".into());
            }
            "caption" => c.text_align = Some("center".into()),
            // 标题：粗体、字号、上下外边距（HTML 规范的 UA 样式表；Send to Kindle 同样照它排，2026-10-08 对照《绍宋》）。
            // 以前只给粗体：没写样式的 `<h2>1</h2>` 在 Kindle 上和正文一样大（《绝叫》的数字章名）。
            "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
                c.bold = true;
                c.semibold = false;
                // 字号、外边距和只修复的文字书写进书里的 `eink-ua.css` 是同一张表
                let (_, size, margin) = crate::ua::HEADINGS
                    .into_iter()
                    .find(|h| h.0 == tag)
                    .unwrap_or(crate::ua::HEADINGS[5]);
                c.font_size = parent.font_size * size;
                c.margin[0] = Some(Len::Em(margin));
                c.margin[2] = Some(Len::Em(margin));
            }
            "i" | "em" | "cite" | "var" | "dfn" => c.italic = true,
            "sup" => c.superscript = true,
            "sub" => c.subscript = true,
            "u" | "ins" => c.decoration[0] = true,
            "s" | "strike" | "del" => c.decoration[1] = true,
            "pre" => {
                c.pre = true;
                if c.font_family.is_none() {
                    c.font_family = Some("monospace".into());
                }
            }
            _ => {}
        }
        // 段落、引文、图、预排版的上下外边距（UA 样式表）：Send to Kindle 给没写外边距的 `<p>` 上下各 1em
        // （相邻的折叠成一个，《绍宋》正文段与段之间 0.8333lh）；以前不给，段落挤在一起。哪些标签、多少用 `crate::uastyle` 的表
        // （只修复的文字书写进书里的 `eink-ua.css` 是同一张）。
        use crate::ua::{BLOCK_MARGIN_TAGS, INDENTED_BLOCK_TAGS, INDENT_PX};
        let indented = INDENTED_BLOCK_TAGS.contains(&tag);
        if indented || BLOCK_MARGIN_TAGS.contains(&tag) {
            c.margin[0] = Some(Len::Em(1.0));
            c.margin[2] = Some(Len::Em(1.0));
        }
        if indented {
            // 左右 40px，同样按 1px＝0.45pt（＝1.5em，和列表缩进一样）
            let side = Some(Len::Pt(f64::from(INDENT_PX) * PT_PER_BOX_PX));
            c.margin[1] = side;
            c.margin[3] = side;
        }
    }

    /// 字体：字号、字体族、字重、斜体。字号最先算——后面行高、字间距的长度换算都按本元素字号。
    fn font_props(c: &mut Computed, parent: &Computed, decls: &Decls) {
        let get = |k: &str| decls.get(k).map(String::as_str);
        if let Some(v) = get("font-size") {
            let v = v.trim().to_ascii_lowercase();
            c.font_size = match v.as_str() {
                "xx-small" => 0.6,
                "x-small" => 0.75,
                "small" | "smaller" => {
                    0.89 * if v == "smaller" {
                        parent.font_size
                    } else {
                        1.0
                    }
                }
                "medium" => 1.0,
                // large 照 Send to Kindle 写 1.125（浏览器的 18px；2026-10-14 怪癖测试书 Q25，Amazon 写 1.125rem）；larger 没核过
                "large" => 1.125,
                "larger" => 1.2 * parent.font_size,
                "x-large" => 1.5,
                "xx-large" => 2.0,
                _ => match parse_len(&v) {
                    Some(Len::Em(n)) if v.ends_with("rem") => n,
                    Some(Len::Em(n)) => parent.font_size * n,
                    Some(Len::Percent(n)) => parent.font_size * n / 100.0,
                    Some(Len::Pt(n)) => n / PT_PER_EM,
                    None => parent.font_size,
                },
            };
            // 负的字号不合法（CSS 里这条声明作废、照父元素）；以前照算出负字号，换算出的长度跟着变号。
            if !(c.font_size >= 0.0 && c.font_size.is_finite()) {
                c.font_size = parent.font_size;
            }
        }
        if let Some(v) = get("font-family") {
            let names: Vec<String> = split_top(v, ',')
                .into_iter()
                .map(|n| n.trim().trim_matches(['"', '\'']).trim().to_string())
                .filter(|n| !n.is_empty())
                .collect();
            if names.first().is_some_and(|f| f != "inherit") {
                c.font_family = Some(names[0].clone());
                c.font_fallbacks = (names.len() > 1).then(|| names[1..].join(",").to_lowercase());
            }
        }
        if let Some(v) = get("font-weight") {
            let v = v.trim().to_ascii_lowercase();
            c.bold = match v.as_str() {
                "bold" | "bolder" => true,
                "normal" | "lighter" => false,
                n => n.parse::<u32>().map(|n| n >= 600).unwrap_or(c.bold),
            };
            c.semibold = v == "600";
            c.bolder = false;
            c.weight_declared = true;
        }
        if let Some(v) = get("font-style") {
            c.italic = matches!(v.trim().to_ascii_lowercase().as_str(), "italic" | "oblique");
        }
    }

    /// 段落排版：对齐、首行缩进、行高（长度按上面算好的本元素字号换算；没写行高的继承父元素的绝对行高）。
    fn text_layout_props(c: &mut Computed, parent: &Computed, decls: &Decls) {
        let get = |k: &str| decls.get(k).map(String::as_str);
        if let Some(v) = get("text-align") {
            let v = v.trim().to_ascii_lowercase();
            if ["left", "right", "center", "justify", "start", "end"].contains(&v.as_str()) {
                c.text_align = Some(v);
            }
        }
        // px 按 0.45pt 一 px（Send to Kindle 同边距：2026-10-14 怪癖测试书 Q16、Q60，`text-indent:30px` 写 1.125em；以前按 0.75pt）
        if let Some(v) = get("text-indent") {
            c.text_indent = parse_box_len(v);
        }
        if let Some(v) = get("line-height") {
            let v = v.trim().to_ascii_lowercase();
            let unitless = v.parse::<f64>().is_ok();
            c.line_height = if v == "normal" {
                None
            } else {
                match parse_len(&v) {
                    // 无单位的数字是倍数；em 相对本元素字号。
                    Some(Len::Em(n)) => Some(n),
                    Some(Len::Percent(n)) => Some(n / 100.0),
                    Some(Len::Pt(n)) => Some(n / PT_PER_EM / c.font_size),
                    None => c.line_height,
                }
            };
            // 数字倍数照倍数继承，长度（%、em、px、pt）算成绝对值继承（CSS 的规矩）
            c.line_height_abs = if unitless || v == "normal" {
                None
            } else {
                c.line_height.map(|l| l * c.font_size)
            };
        } else if let Some(a) = parent.line_height_abs {
            c.line_height = Some(a / c.font_size);
        }
    }

    /// 文字修饰：颜色、上下标（表格单元格里是纵向对齐）、装饰线、小型大写、字间距、空白与断行。
    fn text_style_props(c: &mut Computed, decls: &Decls, tag: &str) {
        let get = |k: &str| decls.get(k).map(String::as_str);
        if let Some(v) = get("color") {
            if let Some(col) = parse_color(v) {
                c.color = Some(col);
            }
        }
        if let Some(v) = get("vertical-align") {
            let v = v.trim().to_ascii_lowercase();
            // 图片的竖直对齐照原值留着（行内图片写 `$44`，KFX 写出器 20），不当上下标
            if matches!(tag, "td" | "th" | "tr" | "img") {
                c.valign = Some(v);
            } else {
                c.superscript = matches!(v.as_str(), "super" | "top" | "text-top");
                c.subscript = v == "sub";
                if v.starts_with(|ch: char| ch.is_ascii_digit() || ch == '-' || ch == '.') {
                    c.baseline_shift = parse_len(&v).filter(|l| !l.is_zero());
                }
            }
        }
        if !matches!(tag, "td" | "th" | "tr" | "img")
            && get("position").is_some_and(|v| v.trim().eq_ignore_ascii_case("relative"))
        {
            let shift = get("top")
                .and_then(parse_len)
                .map(|l| l.scaled(-1.0))
                .or_else(|| get("bottom").and_then(parse_len));
            if let Some(l) = shift.filter(|l| !l.is_zero()) {
                c.baseline_shift = Some(l);
            }
        }
        if let Some(v) = get("text-decoration") {
            if v.contains("none") {
                c.decoration = [false; 3];
            }
            for (i, k) in ["underline", "line-through", "overline"].iter().enumerate() {
                if v.contains(k) {
                    c.decoration[i] = true;
                }
            }
        }
        if let Some(v) = get("font-variant").or_else(|| get("font-variant-caps")) {
            c.small_caps = v.contains("small-caps");
        }
        if let Some(v) = get("letter-spacing") {
            c.letter_spacing = match parse_len(v) {
                Some(Len::Em(n)) if v.trim() != "normal" => Some(n),
                Some(Len::Pt(n)) => Some(n / PT_PER_EM / c.font_size),
                _ => None,
            };
        }
        if let Some(v) = get("white-space") {
            let v = v.trim().to_ascii_lowercase();
            c.pre = matches!(v.as_str(), "pre" | "pre-wrap" | "break-spaces");
            c.nowrap = matches!(v.as_str(), "nowrap" | "pre");
        }
        if let Some(v) = get("word-break") {
            c.break_all = v.trim().eq_ignore_ascii_case("break-all");
        }
    }

    /// 列表与表格：列表符号与位置、边框合并与间距。
    fn list_table_props(c: &mut Computed, decls: &Decls) {
        let get = |k: &str| decls.get(k).map(String::as_str);
        if let Some(v) = get("list-style-type") {
            c.list_style = Some(v.trim().to_ascii_lowercase());
        }
        if let Some(v) = get("list-style-position") {
            c.list_inside = v.trim().eq_ignore_ascii_case("inside");
        }
        if let Some(v) = get("border-collapse") {
            c.border_collapse = v.trim().eq_ignore_ascii_case("collapse");
        }
        if let Some(v) = get("border-spacing") {
            c.border_spacing = v.split_whitespace().next().and_then(parse_len);
        }
    }

    /// 盒模型：边框、圆角、宽高、display、外边距（写了的覆盖标签缺省值）、内边距，以及是否在左右有框的盒子里（`in_hbox`）。
    fn box_props(c: &mut Computed, parent: &Computed, decls: &Decls) {
        let get = |k: &str| decls.get(k).map(String::as_str);
        // 属性名写成常量（每个元素都要查，逐个 format! 很费）；四边顺序：上、右、下、左。
        const BORDER: [[&str; 3]; 4] = [
            ["border-top-style", "border-top-width", "border-top-color"],
            [
                "border-right-style",
                "border-right-width",
                "border-right-color",
            ],
            [
                "border-bottom-style",
                "border-bottom-width",
                "border-bottom-color",
            ],
            [
                "border-left-style",
                "border-left-width",
                "border-left-color",
            ],
        ];
        const RADIUS: [&str; 4] = [
            "border-top-left-radius",
            "border-top-right-radius",
            "border-bottom-right-radius",
            "border-bottom-left-radius",
        ];
        const MARGIN: [&str; 4] = ["margin-top", "margin-right", "margin-bottom", "margin-left"];
        const PADDING: [&str; 4] = [
            "padding-top",
            "padding-right",
            "padding-bottom",
            "padding-left",
        ];
        for (i, [style, width, color]) in BORDER.into_iter().enumerate() {
            let style = get(style).map(|v| v.trim().to_ascii_lowercase());
            c.border[i] = match style {
                Some(st)
                    if st != "none" && st != "hidden" && BORDER_STYLES.contains(&st.as_str()) =>
                {
                    Some(Border {
                        style: st,
                        width: get(width)
                            .and_then(parse_border_width)
                            .unwrap_or(BorderWidth::Pt(1.35)),
                        // 全透明的照原样（写出时写「透明」：《绍宋》信件框是 `border: 35px solid rgba(0,0,0,0)` 加 border-image，
                        // 以前强制不透明，Kindle 上画成 15.75pt 的黑框）；半透明的照旧按不透明写。
                        color: get(color).and_then(parse_color).map(|c| {
                            if c >> 24 == 0 {
                                c
                            } else {
                                c | 0xFF00_0000
                            }
                        }),
                    })
                }
                _ => None,
            };
        }
        for (i, corner) in RADIUS.into_iter().enumerate() {
            c.radius[i] = get(corner)
                .and_then(parse_len)
                .filter(|l| !matches!(l, Len::Em(n) if *n == 0.0));
        }
        c.width = get("width")
            .and_then(parse_len)
            .filter(|l| !matches!(l, Len::Em(n) if *n == 0.0));
        // `height` 也算（Send to Kindle 同样写成 `$62`：《消失的爱人》扉页 `div.shuming{height:6em}`），写了 `min-height` 的以它为准
        c.min_height = get("min-height")
            .or_else(|| get("height"))
            .and_then(parse_len)
            .filter(|l| !matches!(l, Len::Em(n) | Len::Pt(n) | Len::Percent(n) if *n == 0.0));
        c.display = get("display").map(|v| v.trim().to_ascii_lowercase());
        let forced = |k: &str| {
            get(&format!("page-break-{k}"))
                .or_else(|| get(&format!("break-{k}")))
                .is_some_and(|v| {
                    matches!(
                        v.trim().to_ascii_lowercase().as_str(),
                        "always" | "page" | "left" | "right" | "recto" | "verso"
                    )
                })
        };
        c.break_before = forced("before");
        c.break_after = forced("after");
        for i in 0..4 {
            // 写了的才覆盖标签的缺省外边距（上面的 UA 样式）；`auto` 这类算不出长度的当 0
            if let Some(v) = get(MARGIN[i]) {
                c.margin[i] = parse_box_len(v);
                c.margin_auto[i] = v.trim().eq_ignore_ascii_case("auto");
            }
            c.padding[i] = get(PADDING[i]).and_then(parse_box_len);
        }
        let nonzero = |l: &Option<Len>| {
            l.is_some_and(|l| !matches!(l, Len::Em(n) | Len::Pt(n) | Len::Percent(n) if n == 0.0))
        };
        c.in_hbox = parent.in_hbox
            || [1, 3]
                .iter()
                .any(|&i| nonzero(&c.margin[i]) || nonzero(&c.padding[i]) || c.border[i].is_some());
    }

    /// 背景与阴影：背景色（及它垫在白底上的颜色 `backdrop`，往下继承）、阴影、背景图及其各项属性。
    fn background_props(c: &mut Computed, parent: &Computed, decls: &Decls) {
        let get = |k: &str| decls.get(k).map(String::as_str);
        // 不透明的纯白背景不算（Send to Kindle 同样：《疯探》`background-color:#ffffff` 的简介、目录页不出容器）
        c.background = get("background-color")
            .and_then(parse_color)
            .filter(|c| c >> 24 != 0 && *c != 0xFFFF_FFFF);
        c.backdrop = c.background.map(over_white).or(parent.backdrop);
        c.box_shadow = get("box-shadow").and_then(parse_shadow);
        if let Some(v) = get("text-shadow") {
            c.text_shadow = parse_shadow(v);
        }
        c.bg_image = get("background-image")
            .and_then(leading_url)
            .map(str::to_string);
        c.bg_no_repeat = get("background-repeat").is_some_and(|v| v.trim() == "no-repeat");
        c.bg_fixed = get("background-attachment").is_some_and(|v| v.trim() == "fixed");
        c.bg_position = get("background-position")
            .map(|v| parse_bg_position(&v.to_ascii_lowercase()))
            .unwrap_or([None; 2]);
        c.bg_size = get("background-size")
            .map(|v| parse_bg_size(&v.to_ascii_lowercase()))
            .unwrap_or([None; 2]);
        c.bg_cover = get("background-size").is_some_and(|v| v.trim().eq_ignore_ascii_case("cover"));
    }
}

/// 全书正文字号（根 em）：`count` 是字号（千分之一取整）→ 字数（不算空白），取字数最多的，字数一样多的取小的（结果和遍历顺序
/// 无关）。不在 0.5–3 之间的不信，用 1。KFX 写出器（按解析好的块数）和掌阅、Move 的 `kindle_rules`（按 DOM 文字数）共用。
pub fn body_font_size(count: &HashMap<i64, usize>) -> f64 {
    weighted_mode(count)
        .filter(|v| (0.5..=3.0).contains(v))
        .unwrap_or(1.0)
}

/// 按字数加权的众数（键是千分之一取整的值），字数一样多的取小的。
pub fn weighted_mode(count: &HashMap<i64, usize>) -> Option<f64> {
    count
        .iter()
        .max_by_key(|&(&k, &n)| (n, std::cmp::Reverse(k)))
        .map(|(&k, _)| k as f64 / 1000.0)
}
