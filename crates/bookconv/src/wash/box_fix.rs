//! 内边距、边框、标题的上下外边距照 Kindle（不认内边距、不画边框、不认标题外边距的阅读器：Move，profile `padding`、`borders`、
//! `heading_margins` 写 `false`；只修复、照 Send to Kindle 统一的文字书；2026-10-13 用户要修，边框用户选「上/下边框插横线图」）。
//!
//! xochitl（2026-10-13 怪癖测试书 `tools/quirks/probe.py`）：内边距上下左右都不认（Q06、Q12、Q39），块的背景色照画（Q68）；边框一概
//! 不画（Q07、Q40），图照画（横线图放在段落后、标题里 `display:block` 的都画满宽，Q69、Q70）；h1–h6 的上下外边距都不认（Q05、Q65、
//! Q67），标题前后自带一段空，和前一块的下外边距、后一块的上外边距折叠（那两个认，Q66、Q74）；`margin-left` 加负的首行缩进排得出
//! 悬挂缩进（Q71）。所以（Kindle 上的值按 [`crate::kindle_model`] 从现在的 CSS 算，写成 `eink-box.css` 里的类规则（`html body 标签.类`，不用 `!important`：掌阅带了反而输；改到的元素行内的外边距删掉、并进类规则，行内的压过样式表）；
//! 竖直的写 em，之后照竖直间距换算）：
//!
//! - **px、% 写的左右外边距**（profile `px_margins`、`percent_margins` 写 `false`：掌阅、Move 的 px 是屏幕像素，Kindle 写出器按
//!   0.45pt 一 px；Move 的 % 当成 px）折成 em：px 按 0.45pt、12pt 一根 em，% 按这台的内容区宽（KINDLE_RULES 24）；认内边距的（掌阅）
//!   px 写的左右内边距（0.45pt）、首行缩进（同写出器 0.75pt）也折成 em（KINDLE_RULES 25；Move 首行缩进的 px 没量过，不动）；
//! - **左右内边距**并进同侧的外边距（悬挂缩进 `text-indent:-2em;padding-left:2em` 跟着对了）；带背景色、背景图的块（并了色块就
//!   缩到字上）、表格、列表、`margin:auto` 那一侧不动；
//! - **上下边框**（看得见的）：块里最前（上边框）、最后（下边框）插一张横线图 `eink-rule-N.png`（同色、同粗、实线 / 点线 / 虚线照
//!   画，连着这一侧的内边距画成透明的一截，宽 100%、高按比例）；文件里前面还没有内容的块不插上边框（页首的位置由垫图管，再插会多
//!   出来）；
//! - **左右有框的块**（全框、单边竖线）包进单格表格，框写成单元格行内的边框（xochitl 只画表格、单元格行内写的边框，见
//!   [`frame_open`]；KINDLE_RULES 27，用户选）；定了宽度、居中、有背景图的照插横线图；**书里的表格**单元格的边框写进行内；
//! - **上下内边距、标题的上下外边距**：同一层相邻的两块，Kindle 上两块之间的距离 = 外边距折叠 + 前一块的下内边距 + 后一块的上内
//!   边距（横线图里画了的不算），写到 Move 认的那一侧：后一块不是标题就写它的上外边距（不小于前一块的下外边距，折叠以后就是它），
//!   后一块是标题就写前一块的下外边距；两块都是标题的、有子块又有内边距的、空块（只有 id 的分页 div）、我们垫的页首空白图、
//!   页首的标题（文件里前面还没有内容，位置由垫图管）不动。
use super::*;
use crate::cascade::{BorderWidth, Len, MediaEnv};
use crate::kindle_model::{self as km, Chapter};
use crate::reader_model::ReaderModel;
use scraper::{ElementRef, Node};
use std::collections::BTreeMap;

const CSS_NAME: &str = "eink-box.css";
/// 横线图的宽（像素）：显示时宽 100%，高按比例。
const RULE_W: u32 = 1000;

/// 这台阅读器的尺寸。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BoxGeometry {
    /// 内容区宽（屏幕像素）。
    pub area_w: f64,
    /// 缺省字号下 1em 的屏幕像素。
    pub em_px: f64,
    /// Kindle 上竖直一 rem 在这台上的屏幕像素（同竖直间距换算：Kindle 一 rem 的像素 × 两台内容区高之比）。
    pub v_rem_px: f64,
    /// 左右边距的 em 最后要乘的系数（profile `horizontal_scale`：这台一行的 em 数 ÷ 32，Move 约 0.7；不换算的 1）。这里折出来的左右
    /// 长度先按 Kindle 的口径写（一 em 在页宽里占 1/32），乘完正好是这台上占页宽的比例。
    pub h_scale: f64,
}

/// 横线怎么画。
#[derive(Clone, Copy, Debug, PartialEq)]
enum LineStyle {
    Solid,
    Dotted,
    Dashed,
}

/// 一张横线图：颜色（ARGB）、样式、线粗、连着的内边距、显示宽（都是屏幕像素）、线在上还是在下。
#[derive(Clone, Copy, Debug, PartialEq)]
struct Line {
    color: u32,
    style: LineStyle,
    border_px: f64,
    pad_px: f64,
    width_px: f64,
    top: bool,
}

impl Line {
    /// 图的高（像素，宽 [`RULE_W`]）和线占的行数。
    fn rows(&self) -> (u32, u32) {
        let scale = f64::from(RULE_W) / self.width_px;
        let line = (self.border_px * scale).round().max(1.0) as u32;
        let pad = (self.pad_px * scale).round().max(0.0) as u32;
        (line + pad, line)
    }

    /// 同样的图合用一个文件：按画出来的像素认。
    fn key(&self) -> LineKey {
        let (h, l) = self.rows();
        (self.color, self.style as u8, h, l, self.top)
    }

    fn png(&self) -> Vec<u8> {
        use image::ImageEncoder;
        let (h, line) = self.rows();
        let (a, r, g, b) = ((self.color >> 24) as u8, (self.color >> 16) as u8, (self.color >> 8) as u8, self.color as u8);
        let mut px = vec![0u8; (RULE_W * h * 4) as usize];
        let rows = if self.top { 0..line } else { h - line..h };
        // 点线、虚线：按线粗算一段多长（点：粗 1 段、空 1 段；虚线：3 段实、2 段空）
        let unit = line.max(2);
        for y in rows {
            for x in 0..RULE_W {
                let on = match self.style {
                    LineStyle::Solid => true,
                    LineStyle::Dotted => (x / unit) % 2 == 0,
                    LineStyle::Dashed => (x / unit) % 5 < 3,
                };
                if on {
                    let i = ((y * RULE_W + x) * 4) as usize;
                    px[i..i + 4].copy_from_slice(&[r, g, b, a]);
                }
            }
        }
        let mut out = Vec::new();
        image::codecs::png::PngEncoder::new(&mut out).write_image(&px, RULE_W, h, image::ExtendedColorType::Rgba8).expect("PNG 编码");
        out
    }
}

/// 横线图的认法：(颜色, 样式, 图高, 线占的行数, 线在上)。
type LineKey = (u32, u8, u32, u32, bool);

/// 一章要改的：元素（`<body>` 里第几个开始标签）→ 要加的声明；横线图 (第几个开始标签, 图)。
#[derive(Default)]
struct Plan {
    decls: BTreeMap<usize, Vec<String>>,
    lines: Vec<(usize, Line)>,
    /// 行内 `style` 里要删掉的属性（前缀：`margin`、`padding`）：行内的压过样式表（Move 连 `!important` 都压过，2026-10-13 探针
    /// Q48 修后），改了外边距的元素把行内的外边距（不认内边距的阅读器连内边距）删掉、值并进类规则。
    strip: BTreeMap<usize, Vec<&'static str>>,
    /// 左右有看得见的边框的块：包进单格表格画框（元素, 外面那层 div 的类规则（左右外边距）, 单元格的行内 `style`），见 [`frame_open`]。
    frames: Vec<(usize, String, String)>,
    /// 表格、单元格：(元素, 行内 `style` 末尾接的边框)；表格另加 `cellspacing="0"`。
    cells: Vec<(usize, String)>,
    /// 单元格的 `width` 属性：(元素, 新值；`None`＝删掉)，见 [`ReaderModel::cell_width`]。
    cell_widths: Vec<(usize, Option<String>)>,
    /// 接在行内 `style` 末尾的声明（不能写进外链类规则的：xochitl 的外链样式表里一条不带单位的 `line-height` 会让排在它后面的规则
    /// 全部失效，2026-10-15《人生海海》实测）。
    inline: BTreeMap<usize, Vec<String>>,
}

/// 框住块的单格表格。xochitl 普通块上的边框不画（外链、行内都不画），表格、单元格**行内** `style` 写的边框画（分边、点线虚线、颜色
/// 照写），外链、类规则里给表格写的不认；单元格里块的外边距、背景色不认；表格行内的外边距、`width` 属性会和书里 `table{margin:0 auto}`
/// 这类规则搅在一起（2026-10-13 测试书「测试-竖线」1–10、《雪国》制作说明页）。所以框画在单元格的行内，表格满宽，外面再包一层 div
/// 管左右外边距（`eink-box.css` 的类规则，普通块的外边距认），块的内边距写成单元格的。
fn frame_open(class: usize, td_style: &str) -> String {
    format!(r#"<div class="eink-frame eink-b{class}"><table class="eink-frame" cellspacing="0" width="100%"><tbody><tr><td style="{td_style}">"#)
}
const FRAME_CLOSE: &str = "</td></tr></tbody></table></div>";

pub(super) fn apply(entries: &mut Vec<Entry>, rep: &mut WashReport, media: Option<&MediaEnv>, reader: &ReaderModel, g: BoxGeometry) {
    let css = km::book_css(entries, media);
    let chapters: Vec<usize> = (0..entries.len()).filter(|&i| is_chapter_entry(&entries[i])).collect();
    let plans: Vec<Plan> = crate::util::par_map(&chapters, |&i| plan(&entries[i], media, &css, reader, g));
    drop(css);
    if plans.iter().all(|p| p.decls.is_empty() && p.lines.is_empty() && p.frames.is_empty() && p.cells.is_empty() && p.cell_widths.is_empty() && p.inline.is_empty()) {
        return;
    }
    let opf_idx = find_opf(entries);
    let opf_dir = opf_idx.map(|i| dir_of(&entries[i].name).to_string()).unwrap_or_default();
    let at = |n: &str| if opf_dir.is_empty() { n.to_string() } else { format!("{opf_dir}/{n}") };
    let css_path = at(CSS_NAME);
    // (标签, 声明) → 类序号；横线图 → 序号
    let mut classes: Vec<(String, String)> = Vec::new();
    let mut images: Vec<(LineKey, Line)> = Vec::new();
    for (&i, p) in chapters.iter().zip(&plans) {
        if p.decls.is_empty() && p.lines.is_empty() && p.frames.is_empty() && p.cells.is_empty() && p.cell_widths.is_empty() && p.inline.is_empty() {
            continue;
        }
        let e = &mut entries[i];
        let Ok(text) = std::str::from_utf8(&e.data) else { continue };
        let Some((bs, be)) = html::body_range(text) else { continue };
        let starts: Vec<html::Tag> = html::tags_in(text, bs, be).filter(|t| t.is_start()).collect();
        let dir = dir_of(&e.name).to_string();
        let mut edits: Vec<(usize, usize, String)> = Vec::new();
        let empty = Vec::new();
        let tagged: std::collections::BTreeSet<usize> = p.decls.keys().chain(p.inline.keys()).copied().collect();
        for n in tagged {
            let ds = p.decls.get(&n).unwrap_or(&empty);
            let Some(t) = starts.get(n) else { continue };
            let tag = t.name.to_ascii_lowercase();
            let mut raw = text[t.start..t.end].to_string();
            if let (Some(pre), Some(style)) = (p.strip.get(&n), html::attr_value(&raw, "style").map(|s| crate::util::xml_unescape(s).into_owned())) {
                let kept: String = html::css_decls(&style).iter().filter(|d| !pre.iter().any(|p| d.prop.to_ascii_lowercase().starts_with(p))).map(|d| d.raw).collect();
                raw = if kept.trim().is_empty() { html::remove_attr(&raw, "style") } else { html::set_attr(&raw, "style", &crate::util::xml_escape(kept.trim())) };
            }
            if let Some(adds) = p.inline.get(&n) {
                let old = html::attr_value(&raw, "style").map(|s| crate::util::xml_unescape(s).into_owned()).unwrap_or_default();
                let old = old.trim_end();
                let old = old.strip_suffix(';').unwrap_or(old);
                let add: String = adds.iter().map(|d| format!("{d};")).collect();
                let style = if old.trim().is_empty() { add } else { format!("{old};{add}") };
                raw = html::set_attr(&raw, "style", &crate::util::xml_escape(&style));
            }
            if !ds.is_empty() {
                let body = ds.iter().map(|d| format!("{d};")).collect::<String>();
                let k = classes.iter().position(|c| *c == (tag.clone(), body.clone())).unwrap_or_else(|| {
                    classes.push((tag.clone(), body));
                    classes.len() - 1
                });
                raw = html::add_class(&raw, &format!("eink-b{}", k + 1));
            }
            edits.push((t.start, t.end, raw));
        }
        for &(n, line) in &p.lines {
            let Some(t) = starts.get(n) else { continue };
            let k = images.iter().position(|(key, _)| *key == line.key()).unwrap_or_else(|| {
                images.push((line.key(), line));
                images.len() - 1
            });
            let href = crate::epubzip::href_to(&dir, &at(&format!("eink-rule-{}.png", k + 1)), "");
            let img = format!(r#"<img class="eink-sp eink-r{}" src="{href}" alt=""/>"#, k + 1);
            let pos = if line.top {
                t.end
            } else {
                let name = t.name.to_ascii_lowercase();
                let Some(close) = html::find_matching_close(text, t.end, &name) else { continue };
                close.start
            };
            edits.push((pos, pos, img));
        }
        for (n, body, ds) in &p.frames {
            let Some(t) = starts.get(*n) else { continue };
            let Some(close) = html::find_matching_close(text, t.end, &t.name.to_ascii_lowercase()) else { continue };
            let key = ("div".to_string(), body.clone());
            let k = classes.iter().position(|c| *c == key).unwrap_or_else(|| {
                classes.push(key);
                classes.len() - 1
            });
            edits.push((t.start, t.start, frame_open(k + 1, ds)));
            edits.push((close.end, close.end, FRAME_CLOSE.to_string()));
        }
        for (n, add) in &p.cells {
            let Some(t) = starts.get(*n) else { continue };
            let mut raw = text[t.start..t.end].to_string();
            match p.cell_widths.iter().find(|(j, _)| j == n) {
                Some((_, Some(v))) => raw = html::set_attr(&raw, "width", v),
                Some((_, None)) => raw = html::remove_attr(&raw, "width"),
                None => {}
            }
            if t.name.eq_ignore_ascii_case("table") {
                raw = html::set_attr(&raw, "cellspacing", "0");
            }
            if !add.is_empty() {
                let style = match html::attr_value(&raw, "style") {
                    Some(s) if !s.trim().is_empty() => format!("{};{add}", html::style_base(s)),
                    _ => add.clone(),
                };
                raw = html::set_attr(&raw, "style", &style);
            }
            edits.push((t.start, t.end, raw));
        }
        edits.sort_by_key(|x| (x.0, x.1));
        rep.kindle_rule_edits += edits.len();
        let out = html::apply_edits(text, edits);
        let href = crate::epubzip::href_to(&dir, &css_path, "");
        e.data = typeset::inject_css_link(&out, &href).into_bytes();
    }
    // 不写 `!important`（掌阅带了反而输给前面的普通声明）：选择器 `html body 标签.类` 压过书里的规则，这份样式表链在最后
    let mut sheet = String::from("/* 内边距、边框、标题外边距、左右外边距照 Kindle（sheng-ren kindle_rules，见 wash/box_fix.rs） */\n");
    for (k, (tag, body)) in classes.iter().enumerate() {
        sheet.push_str(&format!("html body {tag}.eink-b{}{{{body}}}\n", k + 1));
    }
    for k in 0..images.len() {
        sheet.push_str(&format!("html body img.eink-r{}{{display:block;width:100%;height:auto;margin:0;padding:0;}}\n", k + 1));
    }
    add_css_entry(entries, opf_idx, &css_path, &sheet, "eink-box-css");
    for (k, (_, line)) in images.iter().enumerate() {
        add_entry(entries, opf_idx, &at(&format!("eink-rule-{}.png", k + 1)), line.png(), &format!("eink-rule-{}", k + 1), "image/png");
    }
}

/// 包进单格表格的列表（《雪国》注释列表 `ol{border:1px dashed}`）：Kindle 上文字离框左边约 10.3% 个内容区宽（2026-10-15 Kindle 弹窗
/// 截图：1104px 宽的内容区里 114px，序号在 60px 处），xochitl 自己的列表缩进只有约 25px（同日 Move 排出的 PDF，842px 的内容区里
/// 文字在 25px、序号在 7px 处）：单元格左内边距补上差的那截。
fn list_indent_extra_px(area_w: f64) -> f64 {
    (0.103 * area_w - 25.0).max(0.0).round()
}

/// 页首（文件里前面还没有内容）的块插的上横线图有多高（这台的屏幕像素：线粗，加上不认内边距时连着画的上内边距）；不插的 `None`。
/// 页首的块由页首垫图（`top_spacer.rs`）推到 Kindle 上字的位置，垫图要减去它，两边共用这一个判断（KINDLE_RULES 35：以前页首一律
/// 不插上横线，《人生海海》「第一部」、《平凡的世界》卷名上的线 Move 上没有，用户要修）。包进单格表格画框的块（左右有框）不在这里。
pub(super) fn page_top_rule_px(c: &crate::cascade::Computed, tag: &str, reader: &ReaderModel, v_rem_px: f64) -> Option<f64> {
    if reader.borders || skipped(tag) {
        return None;
    }
    let seen = |side: usize| c.border[side].as_ref().filter(|b| b.width != BorderWidth::Pt(0.0) && b.width != BorderWidth::Em(0.0) && b.color.is_none_or(|c| c >> 24 != 0));
    let b = seen(0)?;
    let framed = (seen(1).is_some() || seen(3).is_some()) && c.bg_image.is_none() && (c.width.is_some() || !(c.margin_auto[1] || c.margin_auto[3]));
    if framed {
        return None;
    }
    let border_rem = match b.width {
        BorderWidth::Pt(n) => n / km::PT_PER_EM,
        BorderWidth::Em(n) => n * c.font_size,
    };
    let pad = if reader.padding { 0.0 } else { km::vertical_rem(c.padding[0], c.font_size).max(0.0) * v_rem_px };
    Some((border_rem * v_rem_px).max(1.0) + pad)
}

/// 表格、列表、分隔线、图这些不动（外边距、内边距的意思不一样，横线也画不对）。
fn skipped(tag: &str) -> bool {
    matches!(tag, "td" | "th" | "tr" | "table" | "thead" | "tbody" | "tfoot" | "caption" | "li" | "ul" | "ol" | "dl" | "dt" | "dd" | "hr" | "img" | "svg" | "body" | "html")
}

fn heading(tag: &str) -> bool {
    matches!(tag, "h1" | "h2" | "h3" | "h4" | "h5" | "h6")
}

fn fmt(v: f64) -> String {
    let s = format!("{:.4}", v);
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s == "-0" { "0".to_string() } else { s.to_string() }
}

fn plan(e: &Entry, media: Option<&MediaEnv>, css: &HashMap<&str, std::sync::Arc<crate::cascade::Rules>>, reader: &ReaderModel, g: BoxGeometry) -> Plan {
    let mut out = Plan::default();
    let Ok(text) = std::str::from_utf8(&e.data) else { return out };
    let Some(ch) = Chapter::parse(&e.name, text, media, css).filter(|c| c.matches_tags) else { return out };
    let tag = |i: usize| km::reader_tag(&ch.el(i));
    let comp = |i: usize| &ch.els[i].comp;
    let has_bg = |i: usize| comp(i).background.is_some_and(|c| c >> 24 != 0) || comp(i).bg_image.is_some();
    // 水平长度换成本元素的 em：pt 按 12pt 一根 em，% 按这台的内容区宽（Move 的 % 外边距是坏的，折成 em 写）
    let to_em = |l: Option<Len>, fs: f64| match l {
        Some(Len::Em(n)) => n,
        Some(Len::Pt(n)) => n / km::PT_PER_EM / fs,
        Some(Len::Percent(p)) => p / 100.0 * g.area_w / (g.em_px * g.h_scale * fs),
        None => 0.0,
    };
    let pad_rem = |i: usize, side: usize| km::vertical_rem(comp(i).padding[side], comp(i).font_size).max(0.0);
    let visible = |i: usize, side: usize| {
        comp(i).border[side].as_ref().filter(|b| b.width != BorderWidth::Pt(0.0) && b.width != BorderWidth::Em(0.0) && b.color.is_none_or(|c| c >> 24 != 0))
    };
    // 文档序里每个元素前面有没有内容（字、图）：前面没有的块不插上边框（页首的位置由垫图管）
    let mut content_before = vec![false; ch.els.len()];
    {
        let mut seen = false;
        for d in ch.body().descendants() {
            match d.value() {
                Node::Text(t) if !t.trim().is_empty() => seen = true,
                Node::Element(v) => {
                    if let Some(j) = ch.index_of(d.id()) {
                        content_before[j] = seen;
                    }
                    if matches!(v.name(), "img" | "svg" | "image") && !v.classes().any(|c| c == "eink-sp") {
                        seen = true;
                    }
                }
                _ => {}
            }
        }
    }
    let usable = |i: usize| ch.els[i].is_block && !ch.els[i].hidden && !skipped(&tag(i));
    // 里面有没有内容（字、不是我们垫的图）：空块（只有 id 的分页 div）不配对
    let has_content = |i: usize| {
        ch.el(i).descendants().any(|d| match d.value() {
            Node::Text(t) => !t.trim().is_empty(),
            Node::Element(v) => matches!(v.name(), "img" | "svg" | "image") && !v.classes().any(|c| c == "eink-sp"),
            _ => false,
        })
    };
    let ours = |i: usize| ch.el(i).value().classes().any(|c| c == "eink-sp");
    // 一条看得见的边框写成单元格行内的 CSS（宽度照横线图折成屏幕像素）
    let border_css = |i: usize, side: usize| -> Option<String> {
        let b = visible(i, side)?;
        let c = comp(i);
        let rem = match b.width {
            BorderWidth::Pt(n) => n / km::PT_PER_EM,
            BorderWidth::Em(n) => n * c.font_size,
        };
        let style = if matches!(b.style.as_str(), "dotted" | "dashed" | "double") { b.style.as_str() } else { "solid" };
        let color = (b.color.or(c.color).unwrap_or(0xFF00_0000) | 0xFF00_0000) & 0xFF_FFFF;
        let name = ["top", "right", "bottom", "left"][side];
        Some(format!("border-{name}:{}px {style} #{color:06x}", fmt((rem * g.v_rem_px).max(1.0).round())))
    };
    // 块在 Kindle 上能用的宽（屏幕像素）：内容区宽减去祖先的左右外边距、内边距
    let avail_px = |i: usize| {
        let mut w = g.area_w;
        let mut p = ch.els[i].parent;
        while let Some(a) = p {
            let c = comp(a);
            w -= (to_em(c.margin[3], c.font_size) + to_em(c.margin[1], c.font_size) + to_em(c.padding[3], c.font_size) + to_em(c.padding[1], c.font_size)).max(0.0) * c.font_size * g.em_px * g.h_scale;
            p = ch.els[a].parent;
        }
        w.max(g.area_w * 0.1)
    };
    // 不认块宽度的阅读器（Move）：定了宽度的块照 Kindle 折成左右外边距（本元素的 em），多出来的放右边（Kindle 不按 auto 居中）
    let mut width_margins: HashMap<usize, [f64; 2]> = HashMap::new();
    if !reader.block_width {
        for i in 0..ch.els.len() {
            let c = comp(i);
            let Some(w) = c.width.filter(|_| usable(i)) else { continue };
            let unit = c.font_size * g.em_px * g.h_scale;
            let avail = avail_px(i);
            let w_px = match w {
                Len::Percent(p) => p / 100.0 * avail,
                l => to_em(Some(l), c.font_size) * unit,
            };
            let side = |s: usize| if c.margin_auto[s] { 0.0 } else { to_em(c.margin[s], c.font_size) * unit };
            let pad = (to_em(c.padding[3], c.font_size) + to_em(c.padding[1], c.font_size)) * unit;
            let extra = avail - w_px - pad - side(3) - side(1);
            if w_px <= 0.0 || extra <= 1.0 {
                continue;
            }
            // 多出来的放右边：Kindle 不按 margin:auto 居中（Q124 真机，Send to Kindle 写的块对齐不起作用）；只有左边 auto 的靠右
            // （写出器写成块的对齐 `$580` 靠右，Kindle 照排：《梁羽生》卷名 bigtitle 2026-10-14 真机），多出来的放左边
            let m = if c.margin_auto[3] && !c.margin_auto[1] { [(side(3) + extra) / unit, side(1) / unit] } else { [side(3) / unit, (side(1) + extra) / unit] };
            width_margins.insert(i, m);
        }
    }
    // 认块宽度、按 margin:auto 居中的阅读器（掌阅，Q124）：Kindle 不居中，定了宽度、两边都是 auto 的块左边的 auto 写成 0（只有左边
    // auto 的 Kindle 靠右，同掌阅，不动）
    if reader.margin_auto && reader.block_width {
        for i in 0..ch.els.len() {
            if usable(i) && comp(i).width.is_some_and(|w| !w.is_zero()) && comp(i).margin_auto[3] && comp(i).margin_auto[1] {
                out.decls.entry(i).or_default().push("margin-left:0".into());
            }
        }
    }
    // 文件里前面还没有内容的块上的「前面分页」：新文件本来就从新的一页开始，Kindle 不再多翻一页；Move 照排、多出一张空白页
    // （《人生海海》「第一部」`<div></div><h1 style="page-break-before:always">`，2026-10-15 真机）。行内末尾接 auto（同一个 style 里
    // 后写的胜；不走类规则：类规则会连带把行内的外边距搬进类规则，Move 上下一章的间距跟着乱了，同日实测）
    for (i, el) in ch.els.iter().enumerate() {
        let pb = |k: &str| el.decls.get(k).map(|v| v.trim().to_ascii_lowercase()).unwrap_or_default();
        let forced = matches!(pb("page-break-before").as_str(), "always" | "left" | "right") || matches!(pb("break-before").as_str(), "page" | "left" | "right" | "recto" | "verso");
        if forced && el.is_block && !el.hidden && !content_before[i] {
            out.inline.entry(i).or_default().push("page-break-before:auto".into());
        }
    }
    // 认 float 的阅读器（掌阅）：Kindle 不浮动（Q133、Q134 真机），标题上的写成 none（别的元素上的没量过，不动）
    if reader.float {
        for (i, el) in ch.els.iter().enumerate() {
            let f = el.decls.get("float").map(|v| v.trim().to_ascii_lowercase()).unwrap_or_default();
            if usable(i) && heading(&tag(i)) && matches!(f.as_str(), "left" | "right") {
                out.decls.entry(i).or_default().push("float:none".into());
            }
        }
    }
    // 页首（文件里前面还没有内容）的块，负的上外边距配上内边距（《梁羽生》卷名 `padding-top:6em;margin-top:-6em`）：Kindle 把盒子往上
    // 推出页面、看不见的那截不占地方，认负外边距的掌阅却按整个盒子分页，竖排的卷名拆到三页（2026-10-14 真机）。照 Kindle 能看见的写：
    // 上外边距 0，上内边距减去推出去的
    if reader.negative_margins && reader.padding {
        for (i, &before) in content_before.iter().enumerate() {
            let c = comp(i);
            let m = km::vertical_rem(c.margin[0], c.font_size);
            if !usable(i) || before || m >= 0.0 || c.margin_auto[0] {
                continue;
            }
            let p = (pad_rem(i, 0) + m).max(0.0);
            let d = out.decls.entry(i).or_default();
            d.push("margin-top:0".into());
            d.push(format!("padding-top:{}em", fmt(p / c.font_size)));
        }
    }
    // 左右有框的块（不画边框的阅读器）：包进单格表格；没背景图、宽度和居中不是 Kindle 的（折成外边距了的算）。列表（`ol`、`ul`）
    // 也包（《雪国》注释列表 `ol{border:1px dashed #366}`，Kindle 画框、Move 没有；KINDLE_RULES 35，用户要修）：列表自己的左右内边距
    // 是放项目符号的，留给列表，单元格左右不加
    let mut framed = vec![false; ch.els.len()];
    if !reader.borders {
        let list = |i: usize| matches!(tag(i).as_str(), "ol" | "ul");
        for (i, f) in framed.iter_mut().enumerate() {
            let c = comp(i);
            let sized = width_margins.contains_key(&i);
            let frameable = usable(i) || (list(i) && ch.els[i].is_block && !ch.els[i].hidden);
            if !frameable || !(visible(i, 1).is_some() || visible(i, 3).is_some()) || (!sized && (c.width.is_some() || c.margin_auto[1] || c.margin_auto[3])) || c.bg_image.is_some() {
                continue;
            }
            *f = true;
            let px = |l: Option<Len>| (to_em(l, c.font_size) * c.font_size * g.em_px * g.h_scale).max(0.0);
            // 外层 div 的左右外边距：块的外边距换成外层（父元素字号）的 em
            let parent_fs = ch.els[i].parent.map_or(ch.body_comp.font_size, |p| comp(p).font_size);
            let m = width_margins.get(&i).copied().unwrap_or([to_em(c.margin[3], c.font_size), to_em(c.margin[1], c.font_size)]);
            let pem = |v: f64| fmt((v * c.font_size / parent_fs).max(0.0));
            let wrap = format!("margin-left:{}em;margin-right:{}em;margin-top:0;margin-bottom:0;", pem(m[0]), pem(m[1]));
            let mut td: Vec<String> = (0..4).filter_map(|s| border_css(i, s)).collect();
            // 负的上外边距（不认负外边距的阅读器上没有）：Kindle 把框往上拉、上内边距再把字推下来，字还在原处（《梁羽生》卷名
            // `padding-top:6em;margin-top:-6em`，2026-10-14 探针 Q134），单元格的上内边距减去它
            let pull = if reader.negative_margins { 0.0 } else { km::vertical_rem(c.margin[0], c.font_size).min(0.0) };
            let pad = |side: usize| if side == 0 { (pad_rem(i, 0) + pull).max(0.0) * g.v_rem_px } else if side == 2 { pad_rem(i, 2) * g.v_rem_px } else if list(i) { if side == 3 { list_indent_extra_px(g.area_w) } else { 0.0 } } else { px(c.padding[side]) };
            td.push(format!("padding:{}px {}px {}px {}px", fmt(pad(0).round()), fmt(pad(1).round()), fmt(pad(2).round()), fmt(pad(3).round())));
            // 单元格里块的背景色 xochitl 不画，写到单元格上
            if let Some(bg) = c.background.filter(|b| b >> 24 != 0) {
                td.push(format!("background-color:#{:06x}", crate::color::over_white(bg) & 0xFF_FFFF));
            }
            td.push("vertical-align:top".into());
            // 书里给 `table`、`td` 写的规则也落在我们的表格上（《雪国》`td{font-size:0.6em;line-height:100%}`、`table{text-align:center}`：
            // 注释框里的字只剩六成大，2026-10-15 unify）：单元格照外面那层的字号、行距、对齐、字体写回去（字号 1em 相对外层 div，
            // 外层同框外；Move 按根算的由 `root_em` 换算）
            let pc = ch.els[i].parent.map_or(&*ch.body_comp, |p| comp(p));
            td.push("font-size:1em".into());
            td.push(match pc.line_height {
                Some(f) => format!("line-height:{}em", fmt(f)),
                None => "line-height:normal".into(),
            });
            td.push(format!("text-align:{}", pc.text_align.as_deref().unwrap_or("left")));
            if let Some(f) = &pc.font_family {
                td.push(format!("font-family:{}", if f.contains(' ') { format!("'{f}'") } else { f.clone() }));
            }
            out.frames.push((i, wrap, td.join(";") + ";"));
        }
    }
    // 水平：左右内边距并进外边距；px、% 写的左右外边距折成 em（元素 → 改后的左右外边距 em，给横线图算宽度用）
    let mut h_margin: HashMap<usize, [f64; 2]> = HashMap::new();
    if !(reader.padding && reader.px_margins && reader.percent_margins && reader.block_width) {
        // 声明的单位不对的（这台上 px、% 和 Kindle 不一样大）
        let wrong_unit = |i: usize, name: &str| {
            let v = ch.els[i].decls.get(name).map(|v| v.trim().to_ascii_lowercase()).unwrap_or_default();
            let nonzero = v.trim_end_matches(|c: char| c.is_ascii_alphabetic() || c == '%').trim().parse::<f64>().is_ok_and(|n| n != 0.0);
            nonzero && ((v.ends_with("px") && !reader.px_margins) || (v.ends_with('%') && !reader.percent_margins))
        };
        for (i, _) in framed.iter().enumerate().filter(|(_, f)| !**f) {
            if !usable(i) {
                continue;
            }
            let c = comp(i);
            let sized = width_margins.get(&i).copied();
            let mut m = sized.unwrap_or([to_em(c.margin[3], c.font_size), to_em(c.margin[1], c.font_size)]);
            let mut changed = false;
            for (k, side, name) in [(0, 3, "margin-left"), (1, 1, "margin-right")] {
                if c.margin_auto[side] && sized.is_none() {
                    continue;
                }
                // 带背景色的块不并内边距（并了色块就缩到字上）
                let p = if reader.padding || has_bg(i) { 0.0 } else { to_em(c.padding[side], c.font_size) };
                if p.abs() < 1e-6 && !wrong_unit(i, name) && sized.is_none() {
                    continue;
                }
                m[k] += p;
                changed = true;
                out.decls.entry(i).or_default().push(format!("{name}:{}em", fmt(m[k])));
            }
            if changed {
                h_margin.insert(i, m);
            }
            // 认内边距、px 又是屏幕像素的（掌阅）：px 写的左右内边距、首行缩进也照 Kindle 折成 em（Move 的内边距上面并进了外边距；
            // 首行缩进的 px Move 上没量过，不动）
            if reader.padding && !reader.px_margins {
                for (side, name) in [(3, "padding-left"), (1, "padding-right")] {
                    if wrong_unit(i, name) {
                        out.decls.entry(i).or_default().push(format!("{name}:{}em", fmt(to_em(c.padding[side], c.font_size))));
                    }
                }
                if wrong_unit(i, "text-indent") {
                    out.decls.entry(i).or_default().push(format!("text-indent:{}em", fmt(to_em(c.text_indent, c.font_size))));
                }
            }
        }
    }
    // 单元格、行内元素、图上 px 写的左右边距（px 是屏幕像素的阅读器）：照 Kindle（外边距、内边距 0.45pt 一 px）折成 em。单元格的内边距两台都认
    // （类规则、行内都认，2026-10-13 探针 Q97–Q99）；行内元素、图的内外边距掌阅认、Move 一概不认（Q88–Q94），Move 上不改
    if !reader.px_margins {
        let px_decl = |i: usize, name: &str| ch.els[i].decls.get(name).is_some_and(|v| {
            let v = v.trim().to_ascii_lowercase();
            v.ends_with("px") && v.trim_end_matches("px").trim().parse::<f64>().is_ok_and(|n| n != 0.0)
        });
        for (i, el) in ch.els.iter().enumerate() {
            let t = tag(i);
            let cell = matches!(t.as_str(), "td" | "th");
            if el.hidden || !(cell || ((!el.is_block || t == "img") && reader.padding)) {
                continue;
            }
            let c = comp(i);
            let sides = [("padding-top", 0), ("padding-right", 1), ("padding-bottom", 2), ("padding-left", 3)];
            if cell && sides.iter().any(|(n, _)| px_decl(i, n)) {
                // 四边一起写（行内的内边距要删掉，没改的边照现在的值）
                let d = out.decls.entry(i).or_default();
                for (name, side) in sides {
                    let v = if side % 2 == 0 { km::vertical_rem(c.padding[side], c.font_size) / c.font_size } else { to_em(c.padding[side], c.font_size) };
                    d.push(format!("{name}:{}em", fmt(v.max(0.0))));
                }
            }
            if !cell {
                for (name, l) in [("margin-left", c.margin[3]), ("margin-right", c.margin[1]), ("padding-left", c.padding[3]), ("padding-right", c.padding[1])] {
                    if px_decl(i, name) {
                        out.decls.entry(i).or_default().push(format!("{name}:{}em", fmt(to_em(l, c.font_size))));
                    }
                }
            }
        }
    }
    // 列表项的内边距（不认内边距的阅读器，Move：Q95 不认、Q96 上外边距认）：上下的并进同侧的外边距；左右的只在没有项目符号时并
    // （`list-style-type:none`、`display` 不是 list-item：有符号的并了会连符号一起挪）；带背景色的不动，有上下边框的上下不动
    if !reader.padding {
        for (i, el) in ch.els.iter().enumerate() {
            if tag(i) != "li" || el.hidden || has_bg(i) {
                continue;
            }
            let c = comp(i);
            let mut d = Vec::new();
            let (pt, pb) = (pad_rem(i, 0), pad_rem(i, 2));
            if pt + pb > 1e-6 && visible(i, 0).is_none() && visible(i, 2).is_none() {
                d.push(format!("margin-top:{}em", fmt((km::vertical_rem(c.margin[0], c.font_size) + pt) / c.font_size)));
                d.push(format!("margin-bottom:{}em", fmt((km::vertical_rem(c.margin[2], c.font_size) + pb) / c.font_size)));
            }
            let marker = c.list_style.as_deref() != Some("none") && c.display.as_deref().is_none_or(|v| v.eq_ignore_ascii_case("list-item"));
            if !marker {
                for (side, name) in [(3, "margin-left"), (1, "margin-right")] {
                    let p = to_em(c.padding[side], c.font_size);
                    if p.abs() > 1e-6 && !c.margin_auto[side] {
                        d.push(format!("{name}:{}em", fmt(to_em(c.margin[side], c.font_size) + p)));
                    }
                }
            }
            if !d.is_empty() {
                out.decls.entry(i).or_default().extend(d);
            }
        }
    }
    // 横线图：看得见的上、下边框
    let mut drawn = vec![[false; 2]; ch.els.len()];
    if !reader.borders {
        // 书里的表格、单元格：看得见的边框照写进行内（xochitl 只认表格、单元格行内的边框）
        for i in 0..ch.els.len() {
            if !matches!(tag(i).as_str(), "table" | "td" | "th") || ch.els[i].hidden {
                continue;
            }
            let add: Vec<String> = (0..4).filter_map(|s| border_css(i, s)).collect();
            // 单元格有线的表格也要 cellspacing="0"（不然格子之间隔开、线成双）
            let lined_cells = tag(i) == "table" && ch.el(i).descendants().filter_map(|d| ch.index_of(d.id())).any(|j| matches!(tag(j).as_str(), "td" | "th") && (0..4).any(|s| visible(j, s).is_some()));
            if !add.is_empty() || lined_cells {
                out.cells.push((i, if add.is_empty() { String::new() } else { add.join(";") + ";" }));
            }
        }
        for i in 0..ch.els.len() {
            if !usable(i) || framed[i] {
                continue;
            }
            let c = comp(i);
            for (k, side, top) in [(0, 0, true), (1, 2, false)] {
                let Some(b) = visible(i, side) else { continue };
                // 页首的块：垫图已经让出了横线图的高（`top_spacer.rs`，[`page_top_rule_px`]）；块里有行内垫图的，横线图插在它前面
                // （块开始标签后面），窄图撑高的是横线下面那一行，要挪的也减去了横线图
                if top && !content_before[i] && page_top_rule_px(c, &tag(i), reader, g.v_rem_px).is_none() {
                    continue;
                }
                let style = match b.style.as_str() {
                    "dotted" => LineStyle::Dotted,
                    "dashed" => LineStyle::Dashed,
                    _ => LineStyle::Solid,
                };
                let border_rem = match b.width {
                    BorderWidth::Pt(n) => n / km::PT_PER_EM,
                    BorderWidth::Em(n) => n * c.font_size,
                };
                let pad_px = if reader.padding { 0.0 } else { pad_rem(i, side) * g.v_rem_px };
                let m = h_margin.get(&i).copied().unwrap_or([to_em(c.margin[3], c.font_size), to_em(c.margin[1], c.font_size)]);
                let width_px = (g.area_w - (m[0] + m[1]).max(0.0) * c.font_size * g.em_px * g.h_scale).clamp(g.area_w * 0.1, g.area_w);
                let color = b.color.or(c.color).unwrap_or(0xFF00_0000) | 0xFF00_0000;
                out.lines.push((i, Line { color, style, border_px: (border_rem * g.v_rem_px).max(1.0), pad_px, width_px, top }));
                drawn[i][k] = true;
                // 行距少出来的空白全从下面扣的阅读器（Move）：行距倍数低于下限时字伸出行盒，末尾的横线图压在字上（profile
                // `line_height_floor`，2026-10-15 探针 Q143、《人生海海》版权页）。插了下边框的块行距不低于它（行内写的删掉、并进类规则）
                if let Some(floor) = reader.line_height_floor.filter(|_| !top) {
                    let cur = ch.els[i].decls.get("line-height").and_then(|v| v.trim().parse::<f64>().ok());
                    if cur.is_some_and(|v| v < floor) {
                        out.inline.entry(i).or_default().push(format!("line-height:{}", fmt(floor)));
                    }
                }
            }
        }
    }
    // 单元格缺省不居中的阅读器（Move 顶端对齐，Q105、Q106）：Kindle 只看单元格自己的 `vertical-align`（top/text-top 顶、bottom/text-bottom
    // 底、别的和没写的居中，同 KFX 写出器），一行两格以上、Kindle 上居中而自己没写 middle 的写进行内（行内的压过书里 `tr`、类规则的）
    if !reader.cell_middle {
        for i in 0..ch.els.len() {
            if !matches!(tag(i).as_str(), "td" | "th") || ch.els[i].hidden {
                continue;
            }
            let v = comp(i).valign.as_deref().unwrap_or("");
            if matches!(v, "middle" | "top" | "text-top" | "bottom" | "text-bottom") {
                continue;
            }
            let row = ch.el(i).parent().and_then(ElementRef::wrap);
            if row.is_none_or(|r| r.children().filter_map(ElementRef::wrap).filter(|c| matches!(c.value().name(), "td" | "th")).count() < 2) {
                continue;
            }
            const MIDDLE: &str = "vertical-align:middle;";
            match out.cells.iter_mut().find(|(j, _)| *j == i) {
                Some((_, add)) => add.push_str(MIDDLE),
                None => out.cells.push((i, MIDDLE.to_string())),
            }
        }
    }
    // 单元格的宽度照 Kindle：Send to Kindle 把它写成表格列宽的百分比（写了的宽度 ÷（写了的 + 没写的单元格的自然宽度），
    // [`km::column_percents`]），Kindle 上字照样横排（Q127–Q129 真机）。按宽度压窄格子的阅读器（掌阅：CSS、属性都认，`width:auto`
    // 不认）写成行内 `width:P%`、属性删掉；只认 `width` 属性的（Move）写成属性 `P%`
    for i in 0..ch.els.len() {
        if tag(i) != "tr" || ch.els[i].hidden {
            continue;
        }
        let cells: Vec<usize> = ch.el(i).children().filter_map(|c| ch.index_of(c.id())).filter(|&j| matches!(tag(j).as_str(), "td" | "th")).collect();
        let width_of = |j: usize| {
            comp(j).width.or_else(|| ch.el(j).value().attr("width").and_then(|w| match w.trim().parse::<f64>() {
                Ok(n) => Some(Len::Pt(n * 0.75)),
                Err(_) => crate::cascade::parse_len(w),
            }))
        };
        let widths: Vec<Option<Len>> = cells.iter().map(|&j| width_of(j)).collect();
        if widths.iter().all(Option::is_none) {
            continue;
        }
        let naturals: Vec<f64> = cells.iter().map(|&j| km::natural_px(&ch.el(j), comp(j).font_size)).collect();
        // 每一格各算（列宽只看第一行的是写出器的写法；这里每行一样的书居多）
        let total: f64 = widths.iter().zip(&naturals).map(|(w, n)| match w {
            Some(Len::Em(e)) => e * 16.0,
            Some(Len::Pt(p)) => p / 0.75,
            _ => *n,
        }).sum();
        for (k, &j) in cells.iter().enumerate() {
            let pct = match widths[k] {
                Some(Len::Percent(p)) => p,
                Some(Len::Em(e)) if total > 0.0 => e * 16.0 / total * 100.0,
                Some(Len::Pt(p)) if total > 0.0 => p / 0.75 / total * 100.0,
                _ => continue,
            };
            let v = format!("{}%", fmt((pct * 100.0).round() / 100.0));
            if reader.cell_width {
                out.cell_widths.push((j, None));
                let add = format!("width:{v};");
                match out.cells.iter_mut().find(|(x, _)| *x == j) {
                    Some((_, a)) => a.push_str(&add),
                    None => out.cells.push((j, add)),
                }
            } else {
                out.cell_widths.push((j, Some(v)));
                if !out.cells.iter().any(|(x, _)| *x == j) {
                    out.cells.push((j, String::new()));
                }
            }
        }
    }
    // 包进表格的块：单元格里块的外边距不认，同标题一样把前后的间距写到框外相邻的块上（框里的内边距写在单元格上，不算）
    // 竖直：同一层相邻两块之间的距离
    let body = ch.body();
    let mut parents: Vec<ElementRef> = vec![body];
    parents.extend((0..ch.els.len()).filter(|&i| ch.els[i].is_block && !ch.block_children(i).is_empty()).map(|i| ch.el(i)));
    for parent in parents {
        // 这一层是块的序列（直接的文字只有空白，元素都是块；我们垫的图不算）才看
        let ok = parent.children().all(|c| match c.value() {
            Node::Text(t) => t.trim().is_empty(),
            Node::Element(v) => v.classes().any(|k| k == "eink-sp") || ch.index_of(c.id()).is_some_and(|j| ch.els[j].is_block),
            _ => true,
        });
        if !ok {
            continue;
        }
        // 我们垫的页首空白图（`top_spacer`）不算块
        let kids: Vec<usize> = parent.children().filter_map(|c| ch.index_of(c.id())).filter(|&j| ch.els[j].is_block && !ch.els[j].hidden && !ours(j)).collect();
        for w in kids.windows(2) {
            let (a, b) = (w[0], w[1]);
            if (skipped(&tag(a)) && !framed[a]) || (skipped(&tag(b)) && !framed[b]) {
                continue;
            }
            // 带框的块同标题：间距写到另一块上
            let (ha, hb) = (heading(&tag(a)) || framed[a], heading(&tag(b)) || framed[b]);
            // 两块都是标题、有空块的不动；后一块是标题而文件里它前面还没有内容的（页首）由垫图管
            if (ha && hb) || !has_content(a) || !has_content(b) || (hb && !content_before[b]) {
                continue;
            }
            // 折叠链上有负外边距时 `margin_edge` 给不出；不认负外边距的阅读器改用这两块自己的（子块的不算）
            let own = |i: usize, side: usize| (!reader.negative_margins).then(|| km::vertical_rem(comp(i).margin[side], comp(i).font_size));
            let (Some(a_mb), Some(b_mt)) = (ch.margin_edge(a, false).or_else(|| own(a, 2)), ch.margin_edge(b, true).or_else(|| own(b, 0))) else { continue };
            let a_pb = if reader.padding || drawn[a][1] || framed[a] { 0.0 } else { pad_rem(a, 2) };
            let b_pt = if reader.padding || drawn[b][0] || framed[b] { 0.0 } else { pad_rem(b, 0) };
            // 有子块又有内边距的：子块的外边距和它折不折叠说不准，不动
            if (a_pb > 0.0 && !ch.block_children(a).is_empty()) || (b_pt > 0.0 && !ch.block_children(b).is_empty()) {
                continue;
            }
            let gap = km::collapse(a_mb, b_mt) + a_pb + b_pt;
            let heading_involved = (!reader.heading_margins && (ha || hb)) || framed[a] || framed[b];
            // 不认负外边距的阅读器（Move）：负的那条当 0，间距照 Kindle 折叠后的写成非负的（叠在一起的写 0）
            let neg = !reader.negative_margins && (a_mb < 0.0 || b_mt < 0.0);
            if a_pb + b_pt < 1e-6 && !heading_involved && !neg {
                continue;
            }
            let gap = if neg { gap.max(0.0) } else { gap };
            if neg && !ha && !hb {
                // 两边都写：只写一边的话另一边正的外边距和它折叠，取大的
                out.decls.entry(a).or_default().push(format!("margin-bottom:{}em", fmt(gap / comp(a).font_size)));
                out.decls.entry(b).or_default().push(format!("margin-top:{}em", fmt(gap / comp(b).font_size)));
            } else if !hb {
                if (gap - b_mt).abs() > 1e-6 || ha {
                    out.decls.entry(b).or_default().push(format!("margin-top:{}em", fmt(gap / comp(b).font_size)));
                }
            } else if (gap - a_mb).abs() > 1e-6 || hb {
                out.decls.entry(a).or_default().push(format!("margin-bottom:{}em", fmt(gap / comp(a).font_size)));
            }
        }
    }
    // 改了外边距、行内又写了外边距（或内边距）的：行内的删掉，没改的那几边照现在的值并进类规则
    let sides = [("margin-top", 0), ("margin-right", 1), ("margin-bottom", 2), ("margin-left", 3)];
    for (&i, ds) in out.decls.iter_mut() {
        let inline = |pre: &str| ch.els[i].origin.iter().any(|(k, (o, _))| *o == crate::cascade::Origin::Inline && k.starts_with(pre));
        let mut pre = Vec::new();
        if inline("margin") {
            pre.push("margin");
        }
        if (!reader.padding || ds.iter().any(|d| d.starts_with("padding-"))) && inline("padding") {
            pre.push("padding");
        }
        if ds.iter().any(|d| d.starts_with("text-indent:")) && inline("text-indent") {
            pre.push("text-indent");
        }
        if pre.is_empty() {
            continue;
        }
        if pre.contains(&"margin") {
            let c = comp(i);
            for (name, side) in sides {
                if ds.iter().any(|d| d.starts_with(&format!("{name}:"))) || c.margin_auto[side] {
                    continue;
                }
                let v = if side % 2 == 0 { km::vertical_rem(c.margin[side], c.font_size) / c.font_size } else { to_em(c.margin[side], c.font_size) };
                ds.push(format!("{name}:{}em", fmt(v)));
            }
        }
        // 认内边距的阅读器上删了行内的内边距：没改的那几边照现在的值并进类规则
        if reader.padding && pre.contains(&"padding") {
            let c = comp(i);
            for (name, side) in [("padding-top", 0), ("padding-right", 1), ("padding-bottom", 2), ("padding-left", 3)] {
                if ds.iter().any(|d| d.starts_with(&format!("{name}:"))) {
                    continue;
                }
                let v = if side % 2 == 0 { km::vertical_rem(c.padding[side], c.font_size) / c.font_size } else { to_em(c.padding[side], c.font_size) };
                ds.push(format!("{name}:{}em", fmt(v)));
            }
        }
        out.strip.entry(i).or_default().extend(pre);
    }
    // em 按根字号算的阅读器（Move）：上面写的 em 都是本元素的；`root_em` 换算别的元素，标题不管（见那里），标题上的这里换成根 em
    // （《梁羽生》卷名 3 倍字号，按本元素写的左外边距在 Move 上只挪三分之一，竖条没挤出来，2026-10-14 真机）
    if reader.em_root {
        for (&i, ds) in out.decls.iter_mut() {
            let fs = comp(i).font_size;
            if !heading(&tag(i)) || (fs - 1.0).abs() < 1e-6 {
                continue;
            }
            for d in ds.iter_mut() {
                if let Some((name, v)) = d.split_once(':') {
                    if let Some(n) = v.strip_suffix("em").and_then(|n| n.parse::<f64>().ok()) {
                        *d = format!("{name}:{}em", fmt(n * fs));
                    }
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn book(body: &str, css: &str) -> Vec<Entry> {
        vec![
            Entry { name: "O/content.opf".into(), data: br#"<package><manifest><item id="c" href="c.xhtml" media-type="application/xhtml+xml"/></manifest><spine><itemref idref="c"/></spine></package>"#.to_vec() },
            Entry { name: "O/s.css".into(), data: css.as_bytes().to_vec() },
            Entry { name: "O/c.xhtml".into(), data: format!(r#"<html xmlns="http://www.w3.org/1999/xhtml"><head><title>t</title><link rel="stylesheet" href="s.css"/></head><body>{body}</body></html>"#).into_bytes() },
        ]
    }

    fn run(body: &str, css: &str) -> (String, String, Vec<Entry>) {
        let mut b = book(body, css);
        let reader = ReaderModel::for_profile(profile::get("xochitl").unwrap());
        apply(&mut b, &mut WashReport::default(), None, &reader, BoxGeometry { area_w: 842.0, em_px: 37.8, v_rem_px: 49.4, h_scale: 1.0 });
        let page = String::from_utf8(b[2].data.clone()).unwrap();
        let sheet = b.iter().find(|e| e.name == "O/eink-box.css").map(|e| String::from_utf8(e.data.clone()).unwrap()).unwrap_or_default();
        (page, sheet, b)
    }

    /// 悬挂缩进：左内边距并进左外边距；带背景色的、列表不动。
    /// 认内边距、px 是屏幕像素的阅读器（掌阅）：px 的左右内边距、首行缩进（都是 0.45pt 一 px）折成 em，em 写的不动；
    /// 行内写的删掉、没改的那几边并进类规则。
    #[test]
    fn px_padding_and_indent_on_padding_reader() {
        let reader = ReaderModel { px_margins: false, ..ReaderModel::default() };
        let mut b = book(r#"<p class="a">一</p><p class="e">二</p><p style="padding:8px 16px;text-indent:32px">三</p>"#, "p.a{padding-left:20px;text-indent:24px} p.e{padding-left:2em;text-indent:2em}");
        apply(&mut b, &mut WashReport::default(), None, &reader, BoxGeometry { area_w: 1264.0, em_px: 37.5, v_rem_px: 49.4, h_scale: 1.0 });
        let page = String::from_utf8(b[2].data.clone()).unwrap();
        let sheet = String::from_utf8(b.iter().find(|e| e.name.ends_with(CSS_NAME)).unwrap().data.clone()).unwrap();
        assert!(page.contains(r#"<p class="a eink-b1">一</p><p class="e">二</p><p class="eink-b2">三</p>"#), "{page}");
        assert!(sheet.contains("p.eink-b1{padding-left:0.75em;text-indent:0.9em;}"), "{sheet}");
        assert!(sheet.contains("p.eink-b2{padding-left:0.6em;padding-right:0.6em;text-indent:1.2em;padding-top:0.3em;padding-bottom:0.3em;}"), "{sheet}");
    }

    /// 左右有框的块包进单格表格，框写在单元格行内（全框四边、单边只有那一边），块的左外边距挪到表格（屏幕像素）、内边距写成单元格的，
    /// 前后的间距写到相邻的块；定了宽度的（Move 不认宽度）折成外边距一起框；书里表格、单元格看得见的边框写进行内。
    #[test]
    fn boxes_framed_and_tables_lined() {
        let body = r#"<p>前</p><div class="box">框</div><p>后</p><p class="bar">引文</p><div class="w">定宽</div><table class="t"><tbody><tr><td>格</td></tr></tbody></table><table><tbody><tr><td>无</td></tr></tbody></table>"#;
        let css = "p{margin:1em 0} div.box{border:1px solid #333;padding:0.5em 0;margin:2em 0} p.bar{border-left:3pt dotted #f00;margin-left:1em} div.w{border:1px solid #000;width:50%} table.t td{border-bottom:1px solid #000}";
        let (page, sheet, _) = run(body, css);
        assert!(page.contains(r#"<p class="eink-b1">前</p><div class="eink-frame eink-b3"><table class="eink-frame" cellspacing="0" width="100%"><tbody><tr><td style="border-top:2px solid #333333;border-right:2px solid #333333;border-bottom:2px solid #333333;border-left:2px solid #333333;padding:25px 0px 25px 0px;vertical-align:top;font-size:1em;line-height:normal;text-align:left;"><div class="box">框</div></td></tr></tbody></table></div><p class="eink-b2">后</p>"#), "{page}");
        // 3pt 点线：0.25 rem × 49.4 ≈ 12px；左外边距 1em 挪到外层 div
        assert!(page.contains(r#"<div class="eink-frame eink-b4"><table class="eink-frame" cellspacing="0" width="100%"><tbody><tr><td style="border-left:12px dotted #ff0000;padding:0px 0px 0px 0px;vertical-align:top;font-size:1em;line-height:normal;text-align:left;"><p class="bar">引文</p></td></tr></tbody></table></div>"#), "{page}");
        assert!(sheet.contains("div.eink-b3{margin-left:0em;margin-right:0em;margin-top:0;margin-bottom:0;}") && sheet.contains("div.eink-b4{margin-left:1em;margin-right:0em;margin-top:0;margin-bottom:0;}"), "{sheet}");
        // 定宽 50% 的：宽度折成右外边距（421 ÷ 37.8），一起框
        assert!(page.contains(r#"<div class="eink-frame eink-b5"><table class="eink-frame" cellspacing="0" width="100%"><tbody><tr><td style="border-top:2px solid #000000;border-right:2px solid #000000;border-bottom:2px solid #000000;border-left:2px solid #000000;padding:0px 0px 0px 0px;vertical-align:top;font-size:1em;line-height:normal;text-align:left;"><div class="w">定宽</div>"#), "{page}");
        assert!(sheet.contains("div.eink-b5{margin-left:0em;margin-right:11.1376em;"), "{sheet}");
        assert!(page.contains(r#"<table class="t" cellspacing="0"><tbody><tr><td style="border-bottom:2px solid #000000;">格</td></tr></tbody></table><table><tbody><tr><td>无</td></tr></tbody></table>"#), "{page}");
    }

    /// Move：单元格 px 的内边距折成 em（四边一起，行内的删掉）；列表项的上下内边距并进外边距；页中间负的上外边距按 Kindle 折叠后写成
    /// 两边非负的。掌阅：行内元素 px 的左右外边距折成 em，Move 上不动（不认）。
    #[test]
    fn cells_lists_negative_and_inline() {
        let body = r#"<table><tbody><tr><td style="padding:8px 16px">格</td></tr></tbody></table><ul><li class="l">项</li></ul><p class="a">甲</p><p class="n">乙</p><p>丙<span class="s">注</span></p>"#;
        let css = "li.l{padding:10px 0;margin:0} p.a{margin:0 0 2em 0} p.n{margin-top:-0.5em} span.s{margin-left:10px}";
        let (page, sheet, _) = run(body, css);
        assert!(page.contains(r#"<td class="eink-b1">格</td>"#), "{page}");
        assert!(sheet.contains("td.eink-b1{padding-top:0.3em;padding-right:0.6em;padding-bottom:0.3em;padding-left:0.6em;}"), "{sheet}");
        // 10px × 0.45pt ÷ 12 = 0.375em
        assert!(page.contains(r#"<li class="l eink-b2">项</li>"#) && sheet.contains("li.eink-b2{margin-top:0.375em;margin-bottom:0.375em;}"), "{sheet}");
        // 2em 和 −0.5em 折叠成 1.5em，两边都写
        assert!(page.contains(r#"<p class="a eink-b3">甲</p><p class="n eink-b4">乙</p><p>丙<span class="s">注</span></p>"#), "{page}");
        assert!(sheet.contains("p.eink-b3{margin-bottom:1.5em;}") && sheet.contains("p.eink-b4{margin-top:1.5em;}"), "{sheet}");
        let mut b = book(body, css);
        let reader = ReaderModel::for_profile(profile::get("ireader").unwrap());
        apply(&mut b, &mut WashReport::default(), None, &reader, BoxGeometry { area_w: 1264.0, em_px: 37.5, v_rem_px: 49.4, h_scale: 1.0 });
        let page = String::from_utf8(b[2].data.clone()).unwrap();
        assert!(page.contains(r#"<span class="s eink-b"#) && page.contains(r#"<td class="eink-b1">格</td>"#), "{page}");
    }

    /// Move：插了下边框横线图的块，行内照 Kindle 写的行距低于下限 1 的，行内末尾接 1（不然字压在横线上；不写进外链）。
    #[test]
    fn bottom_rule_block_gets_line_floor() {
        let (page, sheet, _) = run(r#"<p>前</p><h1 class="t" style="line-height:0.744;text-align:center">版权信息</h1><p>后</p>"#, "h1.t{border-bottom:1px dotted #000}");
        assert!(page.contains(r#"<h1 class="t" style="line-height:0.744;text-align:center;line-height:1;">版权信息<img class="eink-sp eink-r1""#), "{page}");
        assert!(!sheet.contains("line-height"), "外链里不写不带单位的行高（Move 上会让后面的规则失效）：{sheet}");
    }

    /// 文件开头（前面没有内容）的块上的分页：行内末尾接 auto；页中间的不动。
    #[test]
    fn page_break_at_file_start_dropped() {
        let (page, sheet, _) = run(r#"<div></div><h1 style="page-break-before:always;text-align:center">第一部</h1><p>正文</p><h2 class="pb">二</h2>"#, "h2.pb{page-break-before:always}");
        assert!(page.contains(r#"<div></div><h1 style="page-break-before:always;text-align:center;page-break-before:auto;">第一部</h1>"#) && page.contains(r#"<h2 class="pb">二</h2>"#), "{page}");
        assert!(!sheet.contains("page-break"), "{sheet}");
    }

    /// 《梁羽生》卷名（`float:right`、定宽、只有左边 auto、页首负上外边距配上内边距）：Kindle 不浮动、只有左边 auto 的靠右、推出页面的
    /// 那截不占地方。掌阅标题的 float 写成 none、页首的上外边距 0、上内边距减去推出去的，左边的 auto 留着；Move 多出来的宽放左边。
    #[test]
    fn floated_title_follows_kindle() {
        let body = r#"<h1 class="bt">卷名</h1><p>正文</p><h2 class="fl">节</h2>"#;
        let css = "h1.bt{width:1.2em;float:right;padding:6em 0 0 0;margin:-6em 10px auto auto} h2.fl{float:left}";
        let mut b = book(body, css);
        let reader = ReaderModel::for_profile(profile::get("ireader").unwrap());
        apply(&mut b, &mut WashReport::default(), None, &reader, BoxGeometry { area_w: 1264.0, em_px: 37.5, v_rem_px: 49.4, h_scale: 1.0 });
        let page = String::from_utf8(b[2].data.clone()).unwrap();
        let sheet = String::from_utf8(b.iter().find(|e| e.name.ends_with(CSS_NAME)).unwrap().data.clone()).unwrap();
        assert!(page.contains(r#"<h1 class="bt eink-b1">卷名</h1><p>正文</p><h2 class="fl eink-b2">节</h2>"#), "{page}");
        assert!(sheet.contains("h1.eink-b1{float:none;margin-top:0;padding-top:0em;") && !sheet.contains("margin-left:0;") && sheet.contains("h2.eink-b2{float:none;}"), "{sheet}");
        let (_, sheet, _) = run(body, css);
        assert!(!sheet.contains("float") && sheet.contains("h1.eink-b1{margin-left:"), "{sheet}");
        let ml: f64 = sheet.split("h1.eink-b1{margin-left:").nth(1).unwrap().split("em").next().unwrap().parse().unwrap();
        // Move 的 em 按根字号：h1 2 倍字号，按本元素的数乘 2
        assert!(ml > 10.0 && sheet.contains("margin-right:0.375em"), "多出来的宽放左边：{sheet}");
    }

    /// Move 单元格缺省顶端对齐：一行两格以上、Kindle 上居中的（没写、写了 baseline、只写在 tr 上的）行内补 middle；写了 top 的、单格的不动。
    #[test]
    fn cells_get_kindle_vertical_middle() {
        let (page, _, _) = run(
            r#"<table><tbody><tr class="t"><td>甲</td><td class="b">乙</td><td class="p">丙</td></tr><tr><td>丁</td></tr></tbody></table>"#,
            "tr.t{vertical-align:top} td.b{vertical-align:baseline} td.p{vertical-align:top}",
        );
        assert!(page.contains(r#"<td style="vertical-align:middle;">甲</td><td class="b" style="vertical-align:middle;">乙</td><td class="p">丙</td></tr><tr><td>丁</td>"#), "{page}");
    }

    /// 单元格的宽度照 Send to Kindle 写成列宽百分比（1em＝16px、`width="30"`＝30px，÷（16 + 30 + 「丙」的自然宽度 26））：掌阅写行内
    /// `width:P%`、属性删掉，Move 写成属性；掌阅定宽块左边的 auto 写成 0。
    #[test]
    fn cell_width_dropped_and_auto_margin_zeroed() {
        let body = r#"<table><tbody><tr><td class="a">甲</td><td width="30">乙</td><td>丙</td></tr></tbody></table><p class="c">丁</p>"#;
        let css = "td{vertical-align:top} td.a{width:1em} p.c{width:50%;margin:0 auto}";
        let mut b = book(body, css);
        let reader = ReaderModel::for_profile(profile::get("ireader").unwrap());
        apply(&mut b, &mut WashReport::default(), None, &reader, BoxGeometry { area_w: 1264.0, em_px: 37.5, v_rem_px: 49.4, h_scale: 1.0 });
        let page = String::from_utf8(b[2].data.clone()).unwrap();
        let sheet = String::from_utf8(b.iter().find(|e| e.name.ends_with(CSS_NAME)).unwrap().data.clone()).unwrap();
        assert!(page.contains(r#"<td class="a" style="width:22.22%;">甲</td><td style="width:41.67%;">乙</td><td>丙</td>"#), "{page}");
        assert!(page.contains(r#"<p class="c eink-b1">丁</p>"#) && sheet.contains("p.eink-b1{margin-left:0;}"), "{sheet}");
        let (page, _, _) = run(body, css);
        assert!(page.contains(r#"<td class="a" width="22.22%">甲</td><td width="41.67%">乙</td><td>丙</td>"#), "{page}");
    }

    /// 包框、下边框横线图找的是配对的闭合标签：框里套着 div 的不插错位置。
    #[test]
    fn frame_around_nested_div() {
        let (page, _, _) = run(r#"<p>前</p><div class="f"><div>甲</div><div>乙</div></div><p>后</p>"#, "div.f{border:1px solid #000}");
        assert!(page.contains(r#"vertical-align:top;font-size:1em;line-height:normal;text-align:left;"><div class="f"><div>甲</div><div>乙</div></div></td></tr></tbody></table></div><p class="eink-b2">后</p>"#), "{page}");
    }

    #[test]
    fn horizontal_padding_joins_margin() {
        let (page, sheet, _) = run(r#"<p class="h">注一</p><p class="bg">色块</p><ul><li class="h">项</li></ul>"#, "p.h,li.h{text-indent:-2em;padding-left:2em;margin-left:1em} p.bg{padding-left:1em;background-color:#ccc}");
        assert!(page.contains(r#"<p class="h eink-b1">注一</p><p class="bg">色块</p><ul><li class="h">项</li></ul>"#), "{page}");
        assert!(sheet.contains("html body p.eink-b1{margin-left:3em;}"), "{sheet}");
        // 没有项目符号的列表项（自己写的、从 ul 继承的、display:block 的）左内边距并进外边距（《海葵》目录页）
        let (page, sheet, _) = run(
            r#"<ul><li class="n">甲</li></ul><ul class="u"><li>乙</li></ul><ul><li class="d">丙</li><li class="m">丁</li></ul>"#,
            "li{padding-left:2em;margin-left:0} li.n{list-style-type:none} ul.u{list-style:none} li.d{display:block}",
        );
        assert!(page.contains(r#"<li class="n eink-b1">甲</li>"#) && page.contains(r#"<li class="eink-b1">乙</li>"#) && page.contains(r#"<li class="d eink-b1">丙</li><li class="m">丁</li>"#), "{page}");
        assert!(sheet.contains("html body li.eink-b1{margin-left:2em;}"), "{sheet}");
    }

    /// px、% 的左右外边距折成 em：px 按 0.45pt、12pt 一根 em（50px → 1.875em），% 按内容区宽（10% of 842 → 84.2px ÷ 37.8 → 2.2275em）；
    /// 掌阅只折 px；`margin:auto` 那一侧、em 写的不动；带背景色的块也折（不并内边距）。
    #[test]
    fn px_and_percent_side_margins_become_em() {
        let body = r#"<p class="px">甲</p><p class="pc">乙</p><p class="auto">丙</p><p class="em">丁</p><p class="bg">戊</p>"#;
        let css = "p.px{margin-left:50px} p.pc{margin-right:10%} p.auto{margin:0 auto;width:50%} p.em{margin-left:2em} p.bg{margin-left:50px;padding-left:1em;background-color:#ccc}";
        let (page, sheet, _) = run(body, css);
        assert!(page.contains(r#"<p class="px eink-b1">甲</p><p class="pc eink-b2">乙</p><p class="auto eink-b3">丙</p><p class="em">丁</p><p class="bg eink-b1">戊</p>"#), "{page}");
        assert!(sheet.contains("html body p.eink-b1{margin-left:1.875em;}") && sheet.contains("html body p.eink-b2{margin-right:2.2275em;}"), "{sheet}");
        // Move 不认块宽度：50% 折成右边 (842 − 421) ÷ 37.8（Kindle 不按 margin:auto 居中）
        assert!(sheet.contains("html body p.eink-b3{margin-left:0em;margin-right:11.1376em;}"), "{sheet}");
        // 行内写了外边距的：行内的删掉（行内的压过样式表），没改的那几边照现在的值并进类规则
        let (page, sheet, _) = run(r#"<p style="margin:1em 0 0 50px;color:red">己</p>"#, "");
        assert!(page.contains(r#"<p style="color:red" class="eink-b1">己</p>"#), "{page}");
        assert!(sheet.contains("html body p.eink-b1{margin-left:1.875em;margin-top:1em;margin-right:0em;margin-bottom:0em;}"), "{sheet}");
        let mut b = book(body, css);
        let reader = ReaderModel::for_profile(profile::get("ireader").unwrap());
        apply(&mut b, &mut WashReport::default(), None, &reader, BoxGeometry { area_w: 1264.0, em_px: 37.5, v_rem_px: 49.4, h_scale: 1.0 });
        let page = String::from_utf8(b[2].data.clone()).unwrap();
        assert!(page.contains(r#"<p class="px eink-b1">甲</p><p class="pc">乙</p>"#), "掌阅的 % 照 CSS：{page}");
    }

    /// 上下内边距折进相邻块的间距：后一块不是标题写它的上外边距，是标题写前一块的下外边距；标题的外边距也照 Kindle 写到相邻的块上。
    /// 页首的标题前面的空块、垫图不配对。
    #[test]
    fn vertical_gaps_move_to_honored_side() {
        let (page, _, _) = run(r#"<div class="eink-sp"><img class="eink-sp-1" src="s.png" alt=""/></div><div id="pb"></div><h2 class="t">首</h2><p>文</p>"#, "h2.t{margin:2em 0 1.5em}");
        assert!(page.contains(r#"<div class="eink-sp"><img class="eink-sp-1" src="s.png" alt=""/></div><div id="pb"></div><h2 class="t">首</h2><p class="eink-b1">文</p>"#), "{page}");
        let body = r#"<p class="a">甲</p><p class="b">乙</p><h2 class="t">标题</h2><p>丙</p>"#;
        let (page, sheet, _) = run(body, "p{margin:1em 0} p.a{padding-bottom:0.5em} p.b{padding-top:1em} h2.t{margin:2em 0 1.5em}");
        // h2 字号 1.5em：上外边距 2em＝3 根 em、下 1.5em＝2.25。甲→乙：collapse(1,1)+0.5+1 = 2.5 写在乙上；乙→标题：collapse(1,3) = 3
        // 写在乙的下外边距；标题→丙：collapse(2.25,1) = 2.25 写在丙上
        assert!(page.contains(r#"<p class="a">甲</p><p class="b eink-b1">乙</p><h2 class="t">标题</h2><p class="eink-b2">丙</p>"#), "{page}");
        assert!(sheet.contains("html body p.eink-b1{margin-top:2.5em;margin-bottom:3em;}") && sheet.contains("html body p.eink-b2{margin-top:2.25em;}"), "{sheet}");
    }

    /// 上下边框插横线图（块里最前、最后），连着的内边距画进图里、不再算进间距；文件开头的块也插（KINDLE_RULES 35，垫图让出它的高）；
    /// 左右边框不管。
    #[test]
    fn top_and_bottom_borders_become_rule_images() {
        let body = r#"<h2 class="u">标题</h2><p>正文</p><div class="box">框</div>"#;
        let (page, sheet, b) = run(body, "h2.u{border-top:1px solid #000;border-bottom:2px dotted #f00;padding-bottom:0.5em} div.box{border-top:1px solid #000;border-bottom:1px solid #000}");
        assert!(page.contains(r#"<h2 class="u"><img class="eink-sp eink-r1" src="eink-rule-1.png" alt=""/>标题<img class="eink-sp eink-r2" src="eink-rule-2.png" alt=""/></h2>"#), "{page}");
        assert!(page.contains(r#"<div class="box"><img class="eink-sp eink-r1" src="eink-rule-1.png" alt=""/>框<img class="eink-sp eink-r3" src="eink-rule-3.png" alt=""/></div>"#), "{page}");
        assert!(sheet.contains("html body img.eink-r1{display:block;width:100%;height:auto;margin:0;padding:0;}"), "{sheet}");
        let png = image::load_from_memory(&b.iter().find(|e| e.name == "O/eink-rule-2.png").unwrap().data).unwrap().to_rgba8();
        // 下边框：上面透明（0.5em 内边距），最下面一行红色点线
        let h = png.height();
        assert!(h > 20, "{h}");
        assert_eq!(png.get_pixel(0, 0).0[3], 0);
        assert_eq!(png.get_pixel(0, h - 1).0, [255, 0, 0, 255]);
        let opf = String::from_utf8(b[0].data.clone()).unwrap();
        assert!(opf.contains(r#"href="eink-rule-1.png" media-type="image/png""#), "{opf}");
    }

    /// 左右有框的列表也包进单格表格（KINDLE_RULES 35，《雪国》注释列表）：列表自己的左右内边距留给项目符号，单元格只写上下的。
    #[test]
    fn framed_list_goes_into_table() {
        let (page, _, _) = run(r#"<p>前</p><ol class="n"><li>甲</li></ol><p>后</p>"#, "ol.n{border:1px dashed #336666;padding:10px 2em} td{font-size:0.6em;line-height:100%} table{text-align:center}");
        let at = page.find(r#"<ol class="n">"#).expect("列表还在");
        let td = &page[page[..at].rfind("<td").unwrap()..at];
        assert!(td.contains("dashed #336666") && td.contains("px 0px 19px 62px;vertical-align:top;font-size:1em;line-height:normal;text-align:left;"), "列表左内边距补到 Kindle 的缩进（842×0.103−25）: {page}");
        assert!(td.contains("padding:") && !td.contains("padding:0px 0px 0px 0px"), "上下内边距写到单元格: {page}");
        // 书里给 td、table 的规则落在我们的表格上：单元格照外层写回字号、行距、对齐
        assert!(td.contains("font-size:1em;line-height:normal;text-align:left;"), "{page}");
    }

    /// 页首块的上横线图有多高（垫图要让出来）：线粗 + 不认内边距时的上内边距；左右有框（包进表格）的、认边框的阅读器不算。
    #[test]
    fn page_top_rule_height() {
        let reader = ReaderModel::for_profile(profile::get("xochitl").unwrap());
        let comp = |css: &str| {
            let b = book(r#"<h1 class="t">第一部</h1>"#, css);
            let text = String::from_utf8(b[2].data.clone()).unwrap();
            let css = km::book_css(&b, None);
            let ch = Chapter::parse("O/c.xhtml", &text, None, &css).unwrap();
            ch.els[0].comp.as_ref().clone()
        };
        let h = page_top_rule_px(&comp("h1.t{border-top:2px dotted #000;padding-top:1em}"), "h1", &reader, 50.0).unwrap();
        // 2px = 0.9pt = 0.075 rem → 3.75px；上内边距 1em（h1 字号 2em）= 2 rem → 100px
        assert!((h - 103.75).abs() < 0.01, "{h}");
        assert_eq!(page_top_rule_px(&comp("h1.t{border:2px solid #000}"), "h1", &reader, 50.0), None);
        assert_eq!(page_top_rule_px(&comp("h1.t{border-bottom:2px solid #000}"), "h1", &reader, 50.0), None);
        let ir = ReaderModel::for_profile(profile::get("ireader").unwrap());
        assert_eq!(page_top_rule_px(&comp("h1.t{border-top:2px solid #000}"), "h1", &ir, 50.0), None);
    }
}
