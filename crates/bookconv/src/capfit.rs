//! 带图注的竖长图写宽度百分比，图和图注排在同一页（profile `caption_fit`，掌阅、Kindle）。
//!
//! 掌阅、Kindle 不认多看图集（`duokan-image-gallery`），图一张张排开：1200×2200 的人物图按满宽显示撑满一页，图注（人名）
//! 掉到下一页顶上单独一行（《绍宋》简介页）。限高的写法掌阅都不认（`max-height` 的 %、vh、px、em，`page-break-inside:avoid`），
//! `height:85%` 有效，宽度 `60%`/`55%`/`50%` 有效、`70%` 不够（2026-10-06 掌阅真机 adb 截屏）；KFX 写出器只认图片宽度、
//! 不认高度，所以统一写**宽度百分比**。
//!
//! 只处理认得出是图注的结构（拿不准就不处理）：
//! - `<figure>` 里只有一张 `<img>`，文字都在 `<figcaption>` 里；
//! - 多看图集的一格（`duokan-image-gallery-cell`）里只有一张 `<img>`，文字都在 `duokan-image-maintitle`/`subtitle` 里；
//! - 一个块里只有一张 `<img>`（或 `<img>` 本身），紧跟着一个短文字段落（`<p>`/`<div>`，可见文字不超过 [`MAX_CAPTION_CHARS`]
//!   个字、里面没有图）。
//!
//! 只处理**按原尺寸显示会超出一页**的图（图宽超过阅读范围宽时按满宽算）：显示高度超过 阅读范围高 × [`IMAGE_SHARE`] 时，`<img>` 写行内
//! `width:P%`，P = ⌊[`IMAGE_SHARE`] × 阅读范围高 × 图宽 / 图高 / 阅读范围宽 × 100⌋，不小于 [`MIN_PERCENT`]。**只缩不放**：本来就放得下的小图不动，写的宽度也不超过图按原尺寸显示的宽度。原书已经定的宽度
//! （行内样式、`width` 属性、样式表——样式表按真的选择器匹配，取匹配到的最小百分比）不比 P 大的照旧（阿加莎全集的 `image-60`、
//! 《深夜小狗》的 `…-alone40` 本来就不超页）；宽度不是百分比、写了高度的不动（拿不准）。图片字节不动。

use std::collections::HashMap;

use crate::html;

/// 图最多占阅读范围高的比例，剩下的留给图注和页边距。依据：掌阅真机（2026-10-06）宽 60% 有效、70% 不够；按掌阅的阅读范围
/// 1264×1680 和《绍宋》1200×2200 的人物图算，0.8 对应约 58%，落在有效的一边。
pub const IMAGE_SHARE: f64 = 0.8;
/// 宽度百分比的下限：极细长的图也不缩到比这更窄（再窄就看不清了，宁可超页）。
pub const MIN_PERCENT: u32 = 20;
/// 算作图注的段落最多几个字（不计空白）。
pub const MAX_CAPTION_CHARS: usize = 40;

/// 一处带图注的 `<img>`：开标签在 html 里的范围、`src` 原文。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Cand {
    pub start: usize,
    pub end: usize,
    pub src: String,
}

/// `w × h` 的图在 `area` 里按原尺寸显示（宽过阅读范围时按满宽）会超出一页（留出图注）时，图该写的宽度百分比；不超页 → `None`。
/// 只缩不放：结果不超过图按原尺寸显示占阅读范围宽的比例。
pub fn percent(w: u32, h: u32, area: crate::imgopt::Screen) -> Option<u32> {
    if w == 0 || h == 0 || area.width == 0 {
        return None;
    }
    let (w, h, aw, ah) = (w as f64, h as f64, area.width as f64, area.height as f64);
    let shown_w = w.min(aw);
    if shown_w * h / w <= IMAGE_SHARE * ah {
        return None; // 原尺寸显示放得下（含小图）：不动
    }
    let natural = shown_w / aw * 100.0;
    let p = (IMAGE_SHARE * ah * w / h / aw * 100.0).min(natural);
    Some((p.floor() as u32).max(MIN_PERCENT).min(natural.floor() as u32))
}

/// 页面里带图注的 `<img>`（见模块文档的三种结构）。
pub fn candidates(text: &str) -> Vec<Cand> {
    if !html::contains_ci(text, "<img") {
        return Vec::new();
    }
    let (lo, hi) = html::body_range(text).unwrap_or((0, text.len()));
    let spans = html::parse_spans(text, lo, hi);
    let ends = subtree_ends(&spans);
    let mut out = Vec::new();
    for (i, s) in spans.iter().enumerate().filter(|(_, s)| s.name == "img") {
        let Some(src) = html::attr_value(&text[s.open_start..s.open_end], "src").filter(|v| !v.trim().is_empty()) else { continue };
        if has_caption(text, &spans, &ends, i) {
            out.push(Cand { start: s.open_start, end: s.open_end, src: src.to_string() });
        }
    }
    out
}

/// 每个元素子树的终点（不含）：`parse_spans` 按开标签的先后排，`spans[a]` 的后代正好是 `a + 1 .. ends[a]` 这一段
/// （以前每问一次"j 在不在 a 里面"都顺着 parent 往上走、每数一次都扫全部元素，图多的页是平方级）。
fn subtree_ends(spans: &[html::Span]) -> Vec<usize> {
    let mut ends: Vec<usize> = (1..=spans.len()).collect();
    for j in (0..spans.len()).rev() {
        if let Some(p) = spans[j].parent {
            ends[p] = ends[p].max(ends[j]);
        }
    }
    ends
}

/// `spans[a]` 的全部后代的下标。
fn descendants(ends: &[usize], a: usize) -> std::ops::Range<usize> {
    a + 1..ends[a]
}

/// 元素里的 `<img>` 个数。
fn img_count(spans: &[html::Span], ends: &[usize], a: usize) -> usize {
    descendants(ends, a).filter(|&j| spans[j].name == "img").count()
}

/// 元素里的媒体元素（图片、svg、表格……）个数，口径同 [`html::MEDIA_ELEMENTS`]。
fn media_count(spans: &[html::Span], ends: &[usize], a: usize) -> usize {
    descendants(ends, a).filter(|&j| html::MEDIA_ELEMENTS.contains(&spans[j].name.as_str())).count()
}

/// 元素内部、除去 `caps` 这些元素以外，没有可见文字。
fn text_only_in(text: &str, spans: &[html::Span], a: usize, caps: &[usize]) -> bool {
    let mut ranges: Vec<(usize, usize)> = caps.iter().map(|&c| (spans[c].open_start, spans[c].close_end)).collect();
    ranges.sort_unstable();
    let mut pos = spans[a].open_end;
    for (s, e) in ranges {
        if s < pos {
            continue;
        }
        if !html::plain_text(&text[pos..s]).is_empty() {
            return false;
        }
        pos = e;
    }
    html::plain_text(&text[pos..spans[a].close_start.max(pos)]).is_empty()
}

/// 带这几个类之一的后代元素。
fn with_class(text: &str, spans: &[html::Span], ends: &[usize], a: usize, classes: &[&str]) -> Vec<usize> {
    descendants(ends, a)
        .filter(|&j| {
            let tag = &text[spans[j].open_start..spans[j].open_end];
            classes.iter().any(|c| html::has_class(tag, c))
        })
        .collect()
}

fn has_caption(text: &str, spans: &[html::Span], ends: &[usize], img: usize) -> bool {
    // 最近的 figure / 图集格子：在里面就只按它判，不再看后面那一种
    let mut p = spans[img].parent;
    while let Some(k) = p {
        let tag = &text[spans[k].open_start..spans[k].open_end];
        if spans[k].name == "figure" {
            let caps: Vec<usize> = descendants(ends, k).filter(|&j| spans[j].name == "figcaption").collect();
            return spans[k].closed() && img_count(spans, ends, k) == 1 && media_count(spans, ends, k) == 1 && caps_visible(text, spans, &caps) && text_only_in(text, spans, k, &caps);
        }
        if html::has_class(tag, "duokan-image-gallery-cell") {
            let caps = with_class(text, spans, ends, k, &["duokan-image-maintitle", "duokan-image-subtitle"]);
            return spans[k].closed() && img_count(spans, ends, k) == 1 && media_count(spans, ends, k) == 1 && caps_visible(text, spans, &caps) && text_only_in(text, spans, k, &caps);
        }
        p = spans[k].parent;
    }
    // 块里只有这张图：从 img 往上走到只包着它的最外层块
    let mut unit = img;
    while let Some(k) = spans[unit].parent {
        if !spans[k].closed() || !matches!(spans[k].name.as_str(), "div" | "p" | "span" | "a" | "center") {
            break;
        }
        if media_count(spans, ends, k) != 1 || !html::plain_text(&text[spans[k].open_end..spans[k].close_start]).is_empty() {
            break;
        }
        unit = k;
    }
    // 紧跟的兄弟元素：中间没有可见内容
    let parent = spans[unit].parent;
    let after = spans[unit].close_end;
    // 兄弟只可能在 unit 的子树之后（之前的元素都开在 unit 前面，子树里的父元素都不是 `parent`）
    let Some(next) = (ends[unit]..spans.len()).find(|&j| spans[j].parent == parent && spans[j].open_start >= after) else { return false };
    if html::has_visible(&text[after..spans[next].open_start]) {
        return false;
    }
    let s = &spans[next];
    if !s.closed() || !matches!(s.name.as_str(), "p" | "div") || media_count(spans, ends, next) != 0 {
        return false;
    }
    let cap = html::plain_text(&text[s.open_end..s.close_start]);
    let n = cap.chars().filter(|c| !c.is_whitespace()).count();
    (1..=MAX_CAPTION_CHARS).contains(&n)
}

/// 图注元素至少有一个，里面有可见文字。
fn caps_visible(text: &str, spans: &[html::Span], caps: &[usize]) -> bool {
    caps.iter().any(|&c| !html::plain_text(&text[spans[c].open_end..spans[c].close_start]).is_empty())
}

/// 写宽度要用到的全书信息：图的显示宽高、样式表里给图片定宽度或高度的规则。
#[derive(Default)]
pub struct Ctx {
    /// zip 路径 → 图的显示宽高（按 EXIF 摆正后）。
    pub dims: HashMap<String, (u32, u32)>,
    /// 写了 `width`/`height` 的规则：选择器（解析不了的不收——阅读器多半也不认）、它定的尺寸。
    rules: Vec<(scraper::Selector, Size)>,
}

/// 规则（或属性）给图定的尺寸。
#[derive(Clone, Copy, Debug, PartialEq)]
enum Size {
    /// 宽度百分比。
    Pct(f64),
    /// 宽度不是百分比、写了高度：拿不准。
    Unsure,
}

impl Ctx {
    /// 收一份样式表（或 `<style>` 的内容）里写了宽度、高度的规则。
    pub fn add_css(&mut self, css: &str) {
        for c in crate::wash::css_rule_re().captures_iter(css) {
            let sel = crate::wash::rule_selector(&c[1]);
            if sel.trim_start().starts_with('@') {
                continue;
            }
            let mut size = None;
            for d in html::css_decls(&c[2]) {
                let v = crate::cssunlock::split_important(d.value).0.trim().to_ascii_lowercase();
                match d.prop.to_ascii_lowercase().as_str() {
                    "width" if v == "auto" => {}
                    "width" => {
                        let pct = v.strip_suffix('%').and_then(|n| n.trim().parse::<f64>().ok());
                        size = Some(match (size, pct) {
                            (Some(Size::Unsure), _) | (_, None) => Size::Unsure,
                            (_, Some(p)) => Size::Pct(p),
                        });
                    }
                    "height" | "min-height" | "min-width" if v != "auto" && v != "0" => size = Some(Size::Unsure),
                    _ => {}
                }
            }
            let Some(size) = size else { continue };
            let parsed = scraper::Selector::parse(sel.trim()).ok();
            if let Some(s) = parsed {
                self.rules.push((s, size));
            }
        }
    }

    /// 样式表给这个 `<img>` 定的尺寸：宽度取匹配的规则里最小的百分比（不管优先级，宁可少改）；有一条拿不准就拿不准。
    fn sheet_size(&self, img: scraper::ElementRef) -> Option<Size> {
        let mut out: Option<Size> = None;
        for (sel, size) in &self.rules {
            if !sel.matches(&img) {
                continue;
            }
            out = Some(match (out, *size) {
                (Some(Size::Unsure), _) | (_, Size::Unsure) => Size::Unsure,
                (Some(Size::Pct(a)), Size::Pct(b)) => Size::Pct(a.min(b)),
                (None, s) => s,
            });
        }
        out
    }
}

/// 给页面里带图注、会超页的 `<img>` 写宽度百分比。`name` 是页面的 zip 路径，`ctx` 是全书的图片宽高和样式表规则，
/// `area` 是阅读范围。没改动 → `None`。
pub fn apply(text: &str, name: &str, ctx: &Ctx, area: crate::imgopt::Screen) -> Option<String> {
    let todo: Vec<(Cand, u32)> = candidates(text)
        .into_iter()
        .filter_map(|c| {
            let &(w, h) = ctx.dims.get(&crate::epubzip::resolve_link(name, &c.src).0)?;
            Some((c, percent(w, h, area)?))
        })
        .collect();
    if todo.is_empty() {
        return None;
    }
    // 样式表规则按真的选择器匹配：整页用 HTML 解析器解析一遍，`<img>` 按文档顺序和标签扫描对上（个数对不上就整页不动）
    let doc = scraper::Html::parse_document(text);
    let img_sel = scraper::Selector::parse("img").ok()?;
    let parsed: Vec<scraper::ElementRef> = doc.select(&img_sel).collect();
    let starts: Vec<usize> = html::tags(text).filter(|t| t.is_start() && t.is("img")).map(|t| t.start).collect();
    if parsed.len() != starts.len() {
        return None;
    }
    let mut edits: Vec<(usize, usize, String)> = Vec::new();
    for (c, p) in todo {
        let Some(k) = starts.iter().position(|&s| s == c.start) else { continue };
        let tag = &text[c.start..c.end];
        let sheet = ctx.sheet_size(parsed[k]);
        // 原书已经定了的宽度（行内样式优先，其次 `width` 属性、样式表）：不比 P 大就不用改，拿不准的不改
        let has_inline_width = html::attr_value(tag, "style").is_some_and(|st| html::css_decls(&crate::util::xml_unescape(st)).iter().any(|d| d.prop.eq_ignore_ascii_case("width")));
        if has_inline_width {
            if sheet == Some(Size::Unsure) {
                continue;
            }
        } else {
            if html::attr(tag, "height").is_some() {
                continue;
            }
            let attr = html::attr_value(tag, "width").map(|v| match v.trim().strip_suffix('%').and_then(|n| n.trim().parse::<f64>().ok()) {
                Some(p) => Size::Pct(p),
                None => Size::Unsure,
            });
            match (attr, sheet) {
                (_, Some(Size::Unsure)) | (Some(Size::Unsure), _) => continue,
                (Some(Size::Pct(cur)), _) | (None, Some(Size::Pct(cur))) if cur <= p as f64 => continue,
                _ => {}
            }
        }
        if let Some(new) = with_width(tag, p) {
            edits.push((c.start, c.end, new));
        }
    }
    (!edits.is_empty()).then(|| html::apply_edits(text, edits))
}

/// `<img>` 开标签写上 `width:P%`：已有不比它大的百分比宽度、写了高度、宽度不是百分比的 → `None`（不动）。
fn with_width(tag: &str, p: u32) -> Option<String> {
    let style = html::attr_value(tag, "style").map(crate::util::xml_unescape).map(|s| s.into_owned()).unwrap_or_default();
    let decls = html::css_decls(&style);
    if decls.iter().any(|d| matches!(d.prop.to_ascii_lowercase().as_str(), "height" | "max-height" | "min-height" | "min-width")) {
        return None;
    }
    let new_decl = format!("width:{p}%");
    let new_style = match decls.iter().rfind(|d| d.prop.eq_ignore_ascii_case("width")) {
        Some(d) => {
            let v = crate::cssunlock::split_important(d.value).0.trim().to_ascii_lowercase();
            let cur: f64 = v.strip_suffix('%')?.trim().parse().ok()?;
            if cur <= p as f64 {
                return None;
            }
            // 换掉这一条（`raw` 含前导空白和结尾的 `;`）
            let end = d.start + d.raw.len();
            let semi = if d.raw.ends_with(';') { ";" } else { "" };
            let lead = &d.raw[..d.raw.len() - d.raw.trim_start().len()];
            format!("{}{lead}{new_decl}{semi}{}", &style[..d.start], &style[end..])
        }
        None => {
            let t = style.trim_end();
            if t.is_empty() {
                new_decl
            } else if t.ends_with(';') {
                format!("{t}{new_decl}")
            } else {
                format!("{t};{new_decl}")
            }
        }
    };
    Some(html::set_attr(tag, "style", &crate::util::xml_escape(&new_style)))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 子树区间和顺着 parent 往上找的结果一致（含没闭合、多余闭合标签的容错情形）。
    #[test]
    fn subtree_ends_match_parent_chain() {
        let h = "<div><p>a<img/></p><div><span>b</div><figure><img/><figcaption>c</figcaption></figure></p><p>d</p></div><p>e";
        let spans = html::parse_spans(h, 0, h.len());
        let ends = subtree_ends(&spans);
        let inside = |j: usize, a: usize| {
            let mut p = spans[j].parent;
            while let Some(k) = p {
                if k == a {
                    return true;
                }
                p = spans[k].parent;
            }
            false
        };
        for a in 0..spans.len() {
            for j in 0..spans.len() {
                assert_eq!(descendants(&ends, a).contains(&j), inside(j, a), "a={a} j={j}");
            }
        }
    }

    fn area() -> crate::imgopt::Screen {
        crate::imgopt::Screen { width: 1264, height: 1680 }
    }

    fn ctx(css: &str) -> Ctx {
        let mut c = Ctx { dims: [("OEBPS/Images/tall.jpg", (1200, 2200)), ("OEBPS/Images/wide.jpg", (1200, 800)), ("OEBPS/Images/sq.png", (1000, 1000))].into_iter().map(|(k, v)| (k.to_string(), v)).collect(), ..Default::default() };
        c.add_css(css);
        c
    }

    fn page(body: &str) -> String {
        format!(r#"<html xmlns="http://www.w3.org/1999/xhtml"><head><title>t</title></head><body>{body}</body></html>"#)
    }

    fn run(body: &str) -> Option<String> {
        run_css(body, "")
    }

    fn run_css(body: &str, css: &str) -> Option<String> {
        apply(&page(body), "OEBPS/Text/a.xhtml", &ctx(css), area())
    }

    #[test]
    fn percent_formula() {
        // 0.8 × 1680 × 1200 / 2200 / 1264 = 0.5799… → 57%
        assert_eq!(percent(1200, 2200, area()), Some(57));
        // Kindle 阅读范围 1104×1546：0.8 × 1546 × 1200 / 2200 / 1104 = 0.611 → 61%
        assert_eq!(percent(1200, 2200, crate::imgopt::Screen { width: 1104, height: 1546 }), Some(61));
        assert_eq!(percent(1200, 800, area()), None, "横图不超页");
        assert_eq!(percent(1000, 1000, area()), None, "方图按满宽 1264 高、不到 0.8×1680 → 不动");
        assert_eq!(percent(100, 2000, area()), Some(7), "极细长的小图：下限也不超过它原尺寸占的宽度（只缩不放）");
        assert_eq!(percent(400, 3000, area()), Some(MIN_PERCENT), "极细长的图有下限");
        assert_eq!(percent(300, 600, area()), None, "小图原尺寸放得下：不放大");
        assert_eq!(percent(600, 1500, area()), Some(42), "比阅读范围窄、但原尺寸显示超页（1500>0.8×1680）：缩到 42%（原尺寸占 47%）");
    }

    #[test]
    fn duokan_gallery_cell() {
        let body = r#"<div class="duokan-image-gallery"><div class="duokan-image-gallery-cell">
          <img src="../Images/tall.jpg" alt=""/>
          <p class="duokan-image-maintitle">沧州赵玖</p></div>
          <div class="duokan-image-gallery-cell"><img src="../Images/wide.jpg" alt=""/><p class="duokan-image-maintitle">韩世忠</p></div></div>"#;
        let out = run(body).expect("改了");
        assert!(out.contains(r#"<img src="../Images/tall.jpg" alt="" style="width:57%"/>"#), "{out}");
        assert!(out.contains(r#"<img src="../Images/wide.jpg" alt=""/>"#), "不超页的图不动");
        assert!(out.contains("沧州赵玖") && out.contains("韩世忠"));
    }

    #[test]
    fn figure_with_figcaption() {
        let out = run(r#"<figure><img src="../Images/tall.jpg"/><figcaption>图一　人物</figcaption></figure>"#).expect("改了");
        assert!(out.contains(r#"<img src="../Images/tall.jpg" style="width:57%"/>"#), "{out}");
        // figure 里两张图：拿不准
        assert!(run(r#"<figure><img src="../Images/tall.jpg"/><img src="../Images/tall.jpg"/><figcaption>两张</figcaption></figure>"#).is_none());
        // figure 里除了图注还有别的字：拿不准
        assert!(run(r#"<figure><img src="../Images/tall.jpg"/><p>别的说明</p><figcaption>图注</figcaption></figure>"#).is_none());
    }

    #[test]
    fn block_then_short_paragraph() {
        let out = run(r#"<div class="pic"><img src="../Images/tall.jpg"/></div>
            <p class="cap">图：赵玖</p><p>正文……</p>"#)
        .expect("改了");
        assert!(out.contains(r#"<img src="../Images/tall.jpg" style="width:57%"/>"#), "{out}");
        // 同一个块里：图后面紧跟短段落
        let out = run(r#"<div><img src="../Images/tall.jpg"/><p>赵玖</p></div>"#).expect("改了");
        assert!(out.contains("width:57%"), "{out}");
    }

    #[test]
    fn long_paragraph_is_not_caption() {
        let long = "这是一段很长的正文，".repeat(6);
        assert!(run(&format!(r#"<div><img src="../Images/tall.jpg"/></div><p>{long}</p>"#)).is_none());
        // 图和段落之间隔着别的内容、后面没有段落、块里还有字：都不算
        assert!(run(r#"<div><img src="../Images/tall.jpg"/></div><hr/><p>赵玖</p>"#).is_none());
        assert!(run(r#"<div><img src="../Images/tall.jpg"/></div>"#).is_none());
        assert!(run(r#"<p>正文<img src="../Images/tall.jpg"/></p><p>赵玖</p>"#).is_none());
        // 段落里有图：不是图注
        assert!(run(r#"<div><img src="../Images/tall.jpg"/></div><p><img src="../Images/sq.png"/>赵玖</p>"#).is_none());
    }

    #[test]
    fn existing_inline_width() {
        // 已有更小的宽度：保留
        assert!(run(r#"<figure><img src="../Images/tall.jpg" style="width:40%"/><figcaption>注</figcaption></figure>"#).is_none());
        // 更大的：换掉，别的声明不动
        let out = run(r#"<figure><img src="../Images/tall.jpg" style="border:0; width:100%;display:block"/><figcaption>注</figcaption></figure>"#).expect("改了");
        assert!(out.contains(r#"style="border:0; width:57%;display:block""#), "{out}");
        // 有别的声明、没有宽度：追加
        let out = run(r#"<figure><img src='../Images/tall.jpg' style='display:block'/><figcaption>注</figcaption></figure>"#).expect("改了");
        assert!(out.contains("style='display:block;width:57%'"), "{out}");
        // 宽度不是百分比、写了高度：拿不准
        assert!(run(r#"<figure><img src="../Images/tall.jpg" style="width:600px"/><figcaption>注</figcaption></figure>"#).is_none());
        assert!(run(r#"<figure><img src="../Images/tall.jpg" style="height:90%"/><figcaption>注</figcaption></figure>"#).is_none());
    }

    #[test]
    fn sheet_and_attribute_widths() {
        let fig = |img: &str| format!("<figure>{img}<figcaption>注</figcaption></figure>");
        // 样式表已经定了更小的百分比：不动（《深夜小狗》`image-alone40`、阿加莎 `image-60` 这类）
        let css = ".alone40{width:40%} .duokan-image-gallery img{width:100%;height:auto} img.px{width:300px} .tall{height:90%} div.pic{width:10%} sup img{height:1em} .duokan-footnote img{width:0.95em}";
        assert!(run_css(&fig(r#"<img class="x alone40" src="../Images/tall.jpg"/>"#), css).is_none());
        // 更大的（多看图集的 100%）：写
        let body = r#"<div class="duokan-image-gallery"><div class="duokan-image-gallery-cell"><img src="../Images/tall.jpg"/><p class="duokan-image-maintitle">赵玖</p></div></div>"#;
        assert!(run_css(body, css).expect("改了").contains("width:57%"));
        // 样式表的宽度不是百分比、写了高度：拿不准；别的标签上的宽度不管
        assert!(run_css(&fig(r#"<img class="px" src="../Images/tall.jpg"/>"#), css).is_none());
        assert!(run_css(&fig(r#"<img class="tall" src="../Images/tall.jpg"/>"#), css).is_none());
        assert!(run_css(&fig(r#"<img class="pic" src="../Images/tall.jpg"/>"#), css).is_some());
        // width 属性：百分比照样比，不是百分比的、有 height 属性的拿不准
        assert!(run(&fig(r#"<img src="../Images/tall.jpg" width="50%"/>"#)).is_none());
        assert!(run(&fig(r#"<img src="../Images/tall.jpg" width="95%"/>"#)).expect("改了").contains(r#"width="95%" style="width:57%""#));
        assert!(run(&fig(r#"<img src="../Images/tall.jpg" width="600"/>"#)).is_none());
        assert!(run(&fig(r#"<img src="../Images/tall.jpg" height="900"/>"#)).is_none());
        // 样式表开头的 `@charset` 和第一条规则被正则抓在一起：第一条规则照样收（以前被当成 at 规则跳过）
        assert!(run_css(&fig(r#"<img class="alone40" src="../Images/tall.jpg"/>"#), "@charset \"utf-8\";\n.alone40{width:40%}").is_none());
    }

    #[test]
    fn unknown_image_or_no_caption_untouched() {
        assert!(run(r#"<figure><img src="../Images/none.jpg"/><figcaption>注</figcaption></figure>"#).is_none(), "不知道尺寸的不动");
        assert!(run(r#"<figure><img src="../Images/tall.jpg"/></figure>"#).is_none(), "没有图注");
        assert!(run(r#"<p><img src="../Images/tall.jpg"/></p><p></p>"#).is_none());
    }
}
