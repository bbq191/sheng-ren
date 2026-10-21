//! 大段上外边距在 Kindle 上排不下时换页、丢掉边距，掌阅、Move 照样做（只修复、照 Send to Kindle 统一的文字书；2026-10-12 定）。
//!
//! Kindle（Send to Kindle 同样）：一块内容写了上外边距，本页剩下的地方放不下「边距 + 这块」时，这块挪到下一页的页首，那段边距丢掉。
//! 掌阅、Move 不丢：《飘》版权页后强制分页，那一页先是一张 20×20 的小图，接着 `<div style="margin-top:55%">` 的二维码——Kindle 上
//! 二维码排到下一页页首、「阿獴读书·阅读经典」跟在下面；掌阅把二维码推到页底、那行字掉到下一页。
//!
//! 这里照 Kindle 的尺寸估算：从文件开头、强制分页起算，按块累加（一 rem 竖直 = Kindle 正文一行 / 1.2，文字按字数估行数、一行
//! = 字号 × Kindle 行距，图片按 Kindle 上显示的宽、原图比例），某块「已排的 + 上外边距 + 自己」超过一页时，在它前面强制分页、
//! 上外边距写成 0（类 `eink-pb`，规则在 `eink-pb.css`，选择器 `html body 标签.eink-pb` 压过书里的；行内 `style` 里另接一份，
//! 掌阅认行内的）。只看上外边距至少页高 1/4 的块（往下推版面用的），估算误差顾不到的小边距不动。
use super::*;
use crate::cascade::{Computed, Len, MediaEnv, Rules};
use crate::kindle_model::{self as km, Chapter};
use scraper::ElementRef;
use std::rc::Rc;

const CSS_NAME: &str = "eink-pb.css";
const CLASS: &str = "eink-pb";
/// 上外边距至少占 Kindle 内容区高的多少才看。
const MIN_SHARE: f64 = 0.25;
/// 估出来的高超过内容区高的这么多就算放不下：估算没算图片行框、行距的零头（《飘》二维码页估 1496px，Kindle 上实际放不下 1546）。
const FIT_SHARE: f64 = 0.9;
/// 一页里的图高不到内容区高的这么多，就是只有一个小点的页（`cand`，见 [`breaks_in`]）。
const TINY_SHARE: f64 = 0.05;

/// `push`：照 Kindle 估算在放不下的大上边距前分页（掌阅、Move）；不管开不开，Kindle 上不出页的小图页（见 [`breaks_in`]）三台都去掉。
pub(super) fn apply(entries: &mut Vec<Entry>, rep: &mut WashReport, media: Option<&MediaEnv>, known: Option<&HashMap<String, (u32, u32)>>, k: KindleImageRef, push: bool) {
    let dims = super::image_dims_of(entries, rep, known);
    let (chapters, found) = super::scan_chapters(entries, media, |e, css| breaks_in(e, media, css, &dims, k));
    let (marks, drops): (Vec<Vec<usize>>, Vec<Vec<usize>>) = found.into_iter().map(|(m, d)| (if push { m } else { Vec::new() }, d)).unzip();
    if marks.iter().chain(&drops).all(Vec::is_empty) {
        return;
    }
    drop(dims);
    let css_path = super::beside_opf(entries, CSS_NAME);
    let mut tags_used: std::collections::BTreeSet<String> = Default::default();
    for ((&i, m), d) in chapters.iter().zip(&marks).zip(&drops) {
        if m.is_empty() && d.is_empty() {
            continue;
        }
        let e = &mut entries[i];
        let Ok(text) = std::str::from_utf8(&e.data) else { continue };
        let Some((bs, be)) = html::body_range(text) else { continue };
        let starts: Vec<html::Tag> = html::tags_in(text, bs, be).filter(|t| t.is_start()).collect();
        let mut edits = Vec::new();
        // 只有 Kindle 上小到几乎看不见的图的页（《飘》《春雪》版权页后 20×20 的小图）：去掉前面的强制分页，图留在上一页的底部
        // （行内末尾接 auto，压过类规则、行内写的分页）
        for &n in d {
            let Some(t) = starts.get(n) else { continue };
            let raw = &text[t.start..t.end];
            let old = html::attr_value(raw, "style").map(|s| html::style_base(s).to_string()).unwrap_or_default();
            let style = if old.is_empty() { "page-break-before:auto;".to_string() } else { format!("{old};page-break-before:auto;") };
            edits.push((t.start, t.end, html::set_attr(raw, "style", &style)));
        }
        for &n in m {
            let Some(t) = starts.get(n) else { continue };
            let raw = &text[t.start..t.end];
            let mut tag = html::add_class(raw, CLASS);
            // 行内写的上外边距（掌阅认行内样式）：后面接一份 0、强制分页
            if let Some(s) = html::attr_value(&tag, "style") {
                let s = format!("{};margin-top:0;page-break-before:always;", html::style_base(s));
                tag = html::set_attr(&tag, "style", &s);
            }
            tags_used.insert(t.name.to_ascii_lowercase());
            edits.push((t.start, t.end, tag));
        }
        let n = edits.len();
        let out = html::apply_edits(text, edits);
        e.data = if m.is_empty() {
            out.into_bytes()
        } else {
            typeset::inject_css_link(&out, &crate::epubzip::href_to(dir_of(&e.name), &css_path, "")).into_bytes()
        };
        rep.kindle_rule_edits += n;
    }
    if tags_used.is_empty() {
        return;
    }
    // Move 把写 0 的值当没写（xochitl 的毛病），写 0.01em
    let mut sheet = String::from("/* 大段上外边距在 Kindle 上排不下时换页（sheng-ren kindle_rules，见 wash/break_margin.rs） */\n");
    for t in &tags_used {
        sheet.push_str(&format!("html body {t}.{CLASS}{{page-break-before:always;margin-top:0.01em;}}\n"));
    }
    add_css_entry(entries, find_opf(entries), &css_path, &sheet, "eink-pb-css");
}

/// 一章里要在前面分页的元素：`<body>` 里第几个开始标签（和 [`html::tags_in`] 数的一致；对不上的章不动）。
fn breaks_in(e: &Entry, media: Option<&MediaEnv>, css: &HashMap<&str, std::sync::Arc<Rules>>, dims: &HashMap<&str, (u32, u32)>, k: KindleImageRef) -> (Vec<usize>, Vec<usize>) {
    let Ok(text) = std::str::from_utf8(&e.data) else { return (Vec::new(), Vec::new()) };
    let Some(ch) = Chapter::parse(&e.name, text, media, css).filter(|c| c.matches_tags) else { return (Vec::new(), Vec::new()) };
    // body 里的元素按文档序：(元素, 计算值)；数目和标签扫描一致才知道改哪个标签
    let els: Vec<(ElementRef, Rc<Computed>)> = (0..ch.els.len()).map(|i| (ch.el(i), ch.els[i].comp.clone())).collect();
    let rem = k.v_rem_px;
    let line = rem * km::LH_EM;
    let px = |l: &Option<Len>, fs: f64| km::vertical_rem(*l, fs) * rem;
    // 图片在 Kindle 上的高：写了百分比宽的按内容区宽，em 的按字号，没写的按原图像素（宽过内容区的缩进来）
    let img_h = |el: &ElementRef, c: &Computed| -> f64 {
        let Some(src) = el.value().attr("src").or_else(|| el.value().attr("xlink:href")) else { return 0.0 };
        let Some(&(w, h)) = dims.get(crate::epubzip::resolve_link(&e.name, src).0.as_str()) else { return 0.0 };
        let shown = match c.width {
            Some(Len::Percent(p)) => p / 100.0 * k.content_px,
            Some(Len::Em(n)) => n * c.font_size * k.em_px,
            Some(Len::Pt(n)) => n / crate::cascade::PT_PER_EM * k.em_px,
            None => f64::from(w),
        }
        .min(k.content_px);
        shown * f64::from(h) / f64::from(w.max(1))
    };
    let leaf_text = |el: &ElementRef, c: &Computed| -> f64 {
        let chars: usize = el.text().map(|t| t.chars().filter(|c| !c.is_whitespace()).count()).sum();
        if chars == 0 {
            return 0.0;
        }
        let lines = (chars as f64 * c.font_size * k.em_px / k.content_px).ceil().max(1.0);
        lines * c.font_size * line
    };
    let is_blockish = |el: &ElementRef, c: &Computed| crate::cascade::is_block(el, c) || (el.value().name() == "img" && c.display.as_deref() == Some("block"));
    // 块的内容高（不含自己的上外边距）：没有块子孙的按文字、图片估；有的由子块累加（这里只要量级，不细算折叠）
    let height_of = |idx: usize| -> f64 {
        let (el, c) = &els[idx];
        let mut h = px(&c.padding[0], c.font_size) + px(&c.padding[2], c.font_size);
        let imgs: f64 = el.descendants().filter_map(ElementRef::wrap).filter(|d| d.value().name() == "img" && !d.value().classes().any(|c| c == "eink-sp")).map(|d| {
            ch.index_of(d.id()).map_or(0.0, |i| img_h(&d, &els[i].1))
        }).sum();
        h += imgs + leaf_text(el, c);
        h
    };
    let mut out = Vec::new();
    // 强制分页后的页里只有 Kindle 上小到几乎看不见的图（高不到内容区高的 5%），紧接着的块又被挪到下一页：这一页在 Kindle 上就是只有一个小点的
    // 空白页（《飘》《春雪》版权页后 `<div style="page-break-before:always"><img 20×20/></div>` 再接 `margin-top:55%` 的二维码）。
    // 2026-10-15 要去掉（三台一并）：返回要去掉分页的元素，小图并到上一页底部
    let mut drops: Vec<usize> = Vec::new();
    let mut cand: Option<usize> = None;
    let mut acc = 0.0f64;
    let mut skip_until: Option<ego_tree::NodeId> = None;
    for (i, (el, c)) in els.iter().enumerate() {
        if let Some(stop) = skip_until {
            if el.ancestors().any(|a| a.id() == stop) {
                continue;
            }
            skip_until = None;
        }
        // 我们垫的页首空白图（`top_spacer`，Move）本来就是在模拟 Kindle 上的上外边距，不算内容
        // 藏起来的（`display:none`，含 `blank_fold` 藏的空段）不占地方
        if el.value().classes().any(|c| c == "eink-sp") || c.display.as_deref() == Some("none") {
            skip_until = Some(el.id());
            continue;
        }
        if c.break_before {
            acc = 0.0;
            cand = None;
        }
        if !is_blockish(el, c) {
            continue;
        }
        let has_block_child = el.children().filter_map(ElementRef::wrap).any(|c| ch.index_of(c.id()).is_some_and(|j| is_blockish(&els[j].0, &els[j].1)));
        let m = px(&c.margin[0], c.font_size);
        if c.break_before && !has_block_child && el.text().all(|t| t.trim().is_empty()) {
            let imgs: f64 = el.descendants().filter_map(ElementRef::wrap).filter(|d| d.value().name() == "img").map(|d| ch.index_of(d.id()).map_or(0.0, |j| img_h(&d, &els[j].1))).sum();
            if imgs > 0.0 && imgs <= TINY_SHARE * k.content_h && m < MIN_SHARE * k.content_h {
                cand = Some(i);
            }
        } else if cand.is_some() && m < MIN_SHARE * k.content_h {
            // 小图页里还有别的内容
            cand = None;
        }
        if m >= MIN_SHARE * k.content_h {
            let h = height_of(i);
            if acc > 0.0 && acc + m + h > FIT_SHARE * k.content_h {
                out.push(i);
                drops.extend(cand.take());
                acc = h;
            } else {
                cand = None;
                acc += m;
                if !has_block_child {
                    acc += h;
                }
            }
            if !has_block_child {
                acc += px(&c.margin[2], c.font_size);
                skip_until = Some(el.id());
            }
            continue;
        }
        if has_block_child {
            acc += px(&c.margin[0], c.font_size) + px(&c.padding[0], c.font_size);
            continue;
        }
        // 叶子块：上下外边距 + 内容，子孙不再单独算
        acc += m + height_of(i) + px(&c.margin[2], c.font_size);
        skip_until = Some(el.id());
        if c.break_after {
            acc = 0.0;
        }
    }
    (out, drops)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kref() -> KindleImageRef {
        KindleImageRef { content_px: 1104.0, content_h: 1546.0, em_px: 39.0, v_rem_px: 52.5 }
    }

    fn png(w: u32, h: u32) -> Vec<u8> {
        let mut b = Vec::new();
        image::GrayImage::new(w, h).write_to(&mut std::io::Cursor::new(&mut b), image::ImageFormat::Png).unwrap();
        b
    }

    /// 《飘》版权页后的二维码页：分页后一张小图，接着上外边距 55% 的二维码（宽 50%）——Kindle 上二维码换到下一页页首。
    #[test]
    fn big_margin_that_does_not_fit_breaks_like_kindle() {
        let page = r#"<html xmlns="http://www.w3.org/1999/xhtml"><head><title>t</title></head><body><p>版权</p><div style="page-break-before:always"><img src="s.png"/></div><div class="image" style="margin-top:55%"><img src="q.png" width="50%"/></div><p>阿獴读书</p></body></html>"#;
        let mut book = vec![
            Entry { name: "O/content.opf".into(), data: br#"<package><manifest><item id="c" href="c.xhtml" media-type="application/xhtml+xml"/></manifest><spine><itemref idref="c"/></spine></package>"#.to_vec() },
            Entry { name: "O/s.png".into(), data: png(20, 20) },
            Entry { name: "O/q.png".into(), data: png(500, 500) },
            Entry { name: "O/c.xhtml".into(), data: page.as_bytes().to_vec() },
        ];
        apply(&mut book, &mut WashReport::default(), None, None, kref(), true);
        let out = String::from_utf8(book[3].data.clone()).unwrap();
        assert!(out.contains(r#"<div class="image eink-pb" style="margin-top:55%;margin-top:0;page-break-before:always;">"#), "{out}");
        assert!(out.contains("eink-pb.css"));
        // 小图单独成页（Kindle 上是只有一个小点的空白页）：前面的强制分页去掉，小图并到版权页底部（三台一并，KINDLE_RULES 36）
        assert!(out.contains(r#"<div style="page-break-before:always;page-break-before:auto;"><img src="s.png"/></div>"#), "{out}");
        // Kindle（不做分页估算，`push = false`）也去掉小图页的分页，二维码不加类
        let mut kd = book.clone();
        kd[3].data = page.as_bytes().to_vec();
        apply(&mut kd, &mut WashReport::default(), None, None, kref(), false);
        let kout = String::from_utf8(kd[3].data.clone()).unwrap();
        assert!(kout.contains("page-break-before:always;page-break-before:auto;") && !kout.contains("eink-pb") && !kout.contains("stylesheet"), "{kout}");
        // 小图页后面的块放得下（没有被挪走）：小图页就是正常的一页，不动
        let fits = r#"<html xmlns="http://www.w3.org/1999/xhtml"><head><title>t</title></head><body><p>版权</p><div style="page-break-before:always"><img src="s.png"/></div><p>正文</p></body></html>"#;
        let mut keep = book.clone();
        keep[3].data = fits.as_bytes().to_vec();
        let before = keep.clone();
        apply(&mut keep, &mut WashReport::default(), None, None, kref(), true);
        assert_eq!(keep, before);
        let sheet = String::from_utf8(book.iter().find(|e| e.name == "O/eink-pb.css").unwrap().data.clone()).unwrap();
        assert!(sheet.contains("html body div.eink-pb{page-break-before:always;margin-top:0.01em;}"), "{sheet}");
        // 放得下的（《飘》「第一卷」：文件开头上外边距 47% 的标题）不动
        let title = r#"<html xmlns="http://www.w3.org/1999/xhtml"><head><title>t</title></head><body><div><h1 style="margin-top:47%">第一卷</h1></div></body></html>"#;
        let mut b2 = vec![book[0].clone(), Entry { name: "O/c.xhtml".into(), data: title.as_bytes().to_vec() }];
        let before = b2.clone();
        apply(&mut b2, &mut WashReport::default(), None, None, kref(), true);
        assert_eq!(b2, before);
    }
}
