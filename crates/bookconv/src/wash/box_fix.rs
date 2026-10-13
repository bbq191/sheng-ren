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
//!   出来）；左右边框、表格线做不了；
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
}

pub(super) fn apply(entries: &mut Vec<Entry>, rep: &mut WashReport, media: Option<&MediaEnv>, reader: &ReaderModel, g: BoxGeometry) {
    if reader.padding && reader.borders && reader.heading_margins && reader.px_margins && reader.percent_margins {
        return;
    }
    let css = km::book_css(entries, media);
    let chapters: Vec<usize> = (0..entries.len()).filter(|&i| is_chapter_entry(&entries[i])).collect();
    let plans: Vec<Plan> = crate::util::par_map(&chapters, |&i| plan(&entries[i], media, &css, reader, g));
    drop(css);
    if plans.iter().all(|p| p.decls.is_empty() && p.lines.is_empty()) {
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
        if p.decls.is_empty() && p.lines.is_empty() {
            continue;
        }
        let e = &mut entries[i];
        let Ok(text) = std::str::from_utf8(&e.data) else { continue };
        let Some((bs, be)) = html::body_range(text) else { continue };
        let starts: Vec<html::Tag> = html::tags_in(text, bs, be).filter(|t| t.is_start()).collect();
        let dir = dir_of(&e.name).to_string();
        let mut edits: Vec<(usize, usize, String)> = Vec::new();
        for (&n, ds) in &p.decls {
            let Some(t) = starts.get(n) else { continue };
            let tag = t.name.to_ascii_lowercase();
            let body = ds.iter().map(|d| format!("{d};")).collect::<String>();
            let k = classes.iter().position(|c| *c == (tag.clone(), body.clone())).unwrap_or_else(|| {
                classes.push((tag.clone(), body));
                classes.len() - 1
            });
            let mut raw = text[t.start..t.end].to_string();
            if let (Some(pre), Some(style)) = (p.strip.get(&n), html::attr_value(&raw, "style").map(|s| crate::util::xml_unescape(s).into_owned())) {
                let kept: String = html::css_decls(&style).iter().filter(|d| !pre.iter().any(|p| d.prop.to_ascii_lowercase().starts_with(p))).map(|d| d.raw).collect();
                raw = if kept.trim().is_empty() { html::remove_attr(&raw, "style") } else { html::set_attr(&raw, "style", &crate::util::xml_escape(kept.trim())) };
            }
            edits.push((t.start, t.end, html::add_class(&raw, &format!("eink-b{}", k + 1))));
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
                let Some(close) = html::find_close(text, t.end, &name) else { continue };
                close.start
            };
            edits.push((pos, pos, img));
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
    let tag = |i: usize| ch.el(i).value().name().to_ascii_lowercase();
    let comp = |i: usize| &ch.els[i].comp;
    let has_bg = |i: usize| comp(i).background.is_some_and(|c| c >> 24 != 0) || comp(i).bg_image.is_some();
    // 水平长度换成本元素的 em：pt 按 12pt 一根 em，% 按这台的内容区宽（Move 的 % 外边距是坏的，折成 em 写）
    let to_em = |l: Option<Len>, fs: f64| match l {
        Some(Len::Em(n)) => n,
        Some(Len::Pt(n)) => n / km::PT_PER_EM / fs,
        Some(Len::Percent(p)) => p / 100.0 * g.area_w / (g.em_px * fs),
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
    // 水平：左右内边距并进外边距；px、% 写的左右外边距折成 em（元素 → 改后的左右外边距 em，给横线图算宽度用）
    let mut h_margin: HashMap<usize, [f64; 2]> = HashMap::new();
    if !(reader.padding && reader.px_margins && reader.percent_margins) {
        // 声明的单位不对的（这台上 px、% 和 Kindle 不一样大）
        let wrong_unit = |i: usize, name: &str| {
            let v = ch.els[i].decls.get(name).map(|v| v.trim().to_ascii_lowercase()).unwrap_or_default();
            let nonzero = v.trim_end_matches(|c: char| c.is_ascii_alphabetic() || c == '%').trim().parse::<f64>().is_ok_and(|n| n != 0.0);
            nonzero && ((v.ends_with("px") && !reader.px_margins) || (v.ends_with('%') && !reader.percent_margins))
        };
        for i in 0..ch.els.len() {
            if !usable(i) {
                continue;
            }
            let c = comp(i);
            let mut m = [to_em(c.margin[3], c.font_size), to_em(c.margin[1], c.font_size)];
            let mut changed = false;
            for (k, side, name) in [(0, 3, "margin-left"), (1, 1, "margin-right")] {
                if c.margin_auto[side] {
                    continue;
                }
                // 带背景色的块不并内边距（并了色块就缩到字上）
                let p = if reader.padding || has_bg(i) { 0.0 } else { to_em(c.padding[side], c.font_size) };
                if p.abs() < 1e-6 && !wrong_unit(i, name) {
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
    // 横线图：看得见的上、下边框
    let mut drawn = vec![[false; 2]; ch.els.len()];
    if !reader.borders {
        for i in 0..ch.els.len() {
            if !usable(i) {
                continue;
            }
            let c = comp(i);
            for (k, side, top) in [(0, 0, true), (1, 2, false)] {
                let Some(b) = visible(i, side) else { continue };
                if top && !content_before[i] {
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
                let width_px = (g.area_w - (m[0] + m[1]).max(0.0) * c.font_size * g.em_px).clamp(g.area_w * 0.1, g.area_w);
                let color = b.color.or(c.color).unwrap_or(0xFF00_0000) | 0xFF00_0000;
                out.lines.push((i, Line { color, style, border_px: (border_rem * g.v_rem_px).max(1.0), pad_px, width_px, top }));
                drawn[i][k] = true;
            }
        }
    }
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
            if skipped(&tag(a)) || skipped(&tag(b)) {
                continue;
            }
            let (ha, hb) = (heading(&tag(a)), heading(&tag(b)));
            // 两块都是标题、有空块的不动；后一块是标题而文件里它前面还没有内容的（页首）由垫图管
            if (ha && hb) || !has_content(a) || !has_content(b) || (hb && !content_before[b]) {
                continue;
            }
            let (Some(a_mb), Some(b_mt)) = (ch.margin_edge(a, false), ch.margin_edge(b, true)) else { continue };
            let a_pb = if reader.padding || drawn[a][1] { 0.0 } else { pad_rem(a, 2) };
            let b_pt = if reader.padding || drawn[b][0] { 0.0 } else { pad_rem(b, 0) };
            // 有子块又有内边距的：子块的外边距和它折不折叠说不准，不动
            if (a_pb > 0.0 && !ch.block_children(a).is_empty()) || (b_pt > 0.0 && !ch.block_children(b).is_empty()) {
                continue;
            }
            let gap = km::collapse(a_mb, b_mt) + a_pb + b_pt;
            let heading_involved = !reader.heading_margins && (ha || hb);
            if a_pb + b_pt < 1e-6 && !heading_involved {
                continue;
            }
            if !hb {
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
        out.strip.insert(i, pre);
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
        apply(&mut b, &mut WashReport::default(), None, &reader, BoxGeometry { area_w: 842.0, em_px: 37.8, v_rem_px: 49.4 });
        let page = String::from_utf8(b[2].data.clone()).unwrap();
        let sheet = b.iter().find(|e| e.name == "O/eink-box.css").map(|e| String::from_utf8(e.data.clone()).unwrap()).unwrap_or_default();
        (page, sheet, b)
    }

    /// 悬挂缩进：左内边距并进左外边距；带背景色的、列表不动。
    /// 认内边距、px 是屏幕像素的阅读器（掌阅）：px 的左右内边距（0.45pt 一 px）、首行缩进（0.75pt 一 px）折成 em，em 写的不动；
    /// 行内写的删掉、没改的那几边并进类规则。
    #[test]
    fn px_padding_and_indent_on_padding_reader() {
        let reader = ReaderModel { px_margins: false, ..ReaderModel::default() };
        let mut b = book(r#"<p class="a">一</p><p class="e">二</p><p style="padding:8px 16px;text-indent:32px">三</p>"#, "p.a{padding-left:20px;text-indent:24px} p.e{padding-left:2em;text-indent:2em}");
        apply(&mut b, &mut WashReport::default(), None, &reader, BoxGeometry { area_w: 1264.0, em_px: 37.5, v_rem_px: 49.4 });
        let page = String::from_utf8(b[2].data.clone()).unwrap();
        let sheet = String::from_utf8(b.iter().find(|e| e.name.ends_with(CSS_NAME)).unwrap().data.clone()).unwrap();
        assert!(page.contains(r#"<p class="a eink-b1">一</p><p class="e">二</p><p class="eink-b2">三</p>"#), "{page}");
        assert!(sheet.contains("p.eink-b1{padding-left:0.75em;text-indent:1.5em;}"), "{sheet}");
        assert!(sheet.contains("p.eink-b2{padding-left:0.6em;padding-right:0.6em;text-indent:2em;padding-top:0.3em;padding-bottom:0.3em;}"), "{sheet}");
    }

    #[test]
    fn horizontal_padding_joins_margin() {
        let (page, sheet, _) = run(r#"<p class="h">注一</p><p class="bg">色块</p><ul><li class="h">项</li></ul>"#, "p.h,li.h{text-indent:-2em;padding-left:2em;margin-left:1em} p.bg{padding-left:1em;background-color:#ccc}");
        assert!(page.contains(r#"<p class="h eink-b1">注一</p><p class="bg">色块</p><ul><li class="h">项</li></ul>"#), "{page}");
        assert!(sheet.contains("html body p.eink-b1{margin-left:3em;}"), "{sheet}");
    }

    /// px、% 的左右外边距折成 em：px 按 0.45pt、12pt 一根 em（50px → 1.875em），% 按内容区宽（10% of 842 → 84.2px ÷ 37.8 → 2.2275em）；
    /// 掌阅只折 px；`margin:auto` 那一侧、em 写的不动；带背景色的块也折（不并内边距）。
    #[test]
    fn px_and_percent_side_margins_become_em() {
        let body = r#"<p class="px">甲</p><p class="pc">乙</p><p class="auto">丙</p><p class="em">丁</p><p class="bg">戊</p>"#;
        let css = "p.px{margin-left:50px} p.pc{margin-right:10%} p.auto{margin:0 auto;width:50%} p.em{margin-left:2em} p.bg{margin-left:50px;padding-left:1em;background-color:#ccc}";
        let (page, sheet, _) = run(body, css);
        assert!(page.contains(r#"<p class="px eink-b1">甲</p><p class="pc eink-b2">乙</p><p class="auto">丙</p><p class="em">丁</p><p class="bg eink-b1">戊</p>"#), "{page}");
        assert!(sheet.contains("html body p.eink-b1{margin-left:1.875em;}") && sheet.contains("html body p.eink-b2{margin-right:2.2275em;}"), "{sheet}");
        // 行内写了外边距的：行内的删掉（行内的压过样式表），没改的那几边照现在的值并进类规则
        let (page, sheet, _) = run(r#"<p style="margin:1em 0 0 50px;color:red">己</p>"#, "");
        assert!(page.contains(r#"<p style="color:red" class="eink-b1">己</p>"#), "{page}");
        assert!(sheet.contains("html body p.eink-b1{margin-left:1.875em;margin-top:1em;margin-right:0em;margin-bottom:0em;}"), "{sheet}");
        let mut b = book(body, css);
        let reader = ReaderModel::for_profile(profile::get("ireader").unwrap());
        apply(&mut b, &mut WashReport::default(), None, &reader, BoxGeometry { area_w: 1264.0, em_px: 37.5, v_rem_px: 49.4 });
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

    /// 上下边框插横线图（块里最前、最后），连着的内边距画进图里、不再算进间距；文件开头的块不插上边框；左右边框不管。
    #[test]
    fn top_and_bottom_borders_become_rule_images() {
        let body = r#"<h2 class="u">标题</h2><p>正文</p><div class="box">框</div>"#;
        let (page, sheet, b) = run(body, "h2.u{border-top:1px solid #000;border-bottom:2px dotted #f00;padding-bottom:0.5em} div.box{border:1px solid #000}");
        assert!(page.contains(r#"<h2 class="u">标题<img class="eink-sp eink-r1" src="eink-rule-1.png" alt=""/></h2>"#), "{page}");
        assert!(page.contains(r#"<div class="box"><img class="eink-sp eink-r2" src="eink-rule-2.png" alt=""/>框<img class="eink-sp eink-r3" src="eink-rule-3.png" alt=""/></div>"#), "{page}");
        assert!(sheet.contains("html body img.eink-r1{display:block;width:100%;height:auto;margin:0;padding:0;}"), "{sheet}");
        let png = image::load_from_memory(&b.iter().find(|e| e.name == "O/eink-rule-1.png").unwrap().data).unwrap().to_rgba8();
        // 下边框：上面透明（0.5em 内边距），最下面一行红色点线
        let h = png.height();
        assert!(h > 20, "{h}");
        assert_eq!(png.get_pixel(0, 0).0[3], 0);
        assert_eq!(png.get_pixel(0, h - 1).0, [255, 0, 0, 255]);
        let opf = String::from_utf8(b[0].data.clone()).unwrap();
        assert!(opf.contains(r#"href="eink-rule-1.png" media-type="image/png""#), "{opf}");
    }
}
