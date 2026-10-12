//! 大段上外边距在 Kindle 上排不下时换页、丢掉边距，掌阅、Move 照样做（只修复、照 Send to Kindle 统一的文字书；2026-10-12 用户定）。
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
use crate::cascade::{Computed, Len, MediaEnv, Rules, Sheet};
use scraper::{ElementRef, Html, Node};
use std::rc::Rc;

const CSS_NAME: &str = "eink-pb.css";
const CLASS: &str = "eink-pb";
/// 写出器把百分比折成多少 em 的页宽（同 `kfx::write` 的 `PAGE_WIDTH_EM`）。
const PAGE_WIDTH_EM: f64 = 32.0;
/// 上外边距至少占 Kindle 内容区高的多少才看。
const MIN_SHARE: f64 = 0.25;
/// 估出来的高超过内容区高的这么多就算放不下：估算没算图片行框、行距的零头（《飘》二维码页估 1496px，Kindle 上实际放不下 1546）。
const FIT_SHARE: f64 = 0.9;

pub(super) fn apply(entries: &mut Vec<Entry>, rep: &mut WashReport, media: Option<&MediaEnv>, known: Option<&HashMap<String, (u32, u32)>>, k: KindleImageRef) {
    let css = super::top_spacer::parse_css(entries, media);
    let dims = super::image_dims_of(entries, rep, known);
    let chapters: Vec<usize> = (0..entries.len()).filter(|&i| is_chapter_entry(&entries[i])).collect();
    let marks: Vec<Vec<usize>> = crate::util::par_map(&chapters, |&i| breaks_in(&entries[i], media, &css, &dims, k));
    if marks.iter().all(Vec::is_empty) {
        return;
    }
    drop(css);
    drop(dims);
    let opf_idx = find_opf(entries);
    let css_path = match opf_idx.map(|i| dir_of(&entries[i].name)) {
        Some(d) if !d.is_empty() => format!("{d}/{CSS_NAME}"),
        _ => CSS_NAME.to_string(),
    };
    let mut tags_used: std::collections::BTreeSet<String> = Default::default();
    for (&i, m) in chapters.iter().zip(&marks) {
        if m.is_empty() {
            continue;
        }
        let e = &mut entries[i];
        let Ok(text) = std::str::from_utf8(&e.data) else { continue };
        let Some((bs, be)) = html::body_range(text) else { continue };
        let starts: Vec<html::Tag> = html::tags_in(text, bs, be).filter(|t| t.is_start()).collect();
        let mut edits = Vec::new();
        for &n in m {
            let Some(t) = starts.get(n) else { continue };
            let raw = &text[t.start..t.end];
            let mut tag = html::add_class(raw, CLASS);
            // 行内写的上外边距（掌阅认行内样式）：后面接一份 0、强制分页
            if let Some(s) = html::attr_value(&tag, "style") {
                let s = format!("{};margin-top:0;page-break-before:always;", s.trim_end().trim_end_matches(';'));
                tag = html::set_attr(&tag, "style", &s);
            }
            tags_used.insert(t.name.to_ascii_lowercase());
            edits.push((t.start, t.end, tag));
        }
        let n = edits.len();
        let out = html::apply_edits(text, edits);
        let href = crate::epubzip::href_to(dir_of(&e.name), &css_path, "");
        e.data = typeset::inject_css_link(&out, &href).into_bytes();
        rep.kindle_rule_edits += n;
    }
    // Move 把写 0 的值当没写（xochitl 的毛病），写 0.01em
    let mut sheet = String::from("/* 大段上外边距在 Kindle 上排不下时换页（sheng-ren kindle_rules，见 wash/break_margin.rs） */\n");
    for t in &tags_used {
        sheet.push_str(&format!("html body {t}.{CLASS}{{page-break-before:always;margin-top:0.01em;}}\n"));
    }
    add_css_entry(entries, opf_idx, &css_path, &sheet, "eink-pb-css");
}

/// 一章里要在前面分页的元素：`<body>` 里第几个开始标签（和 [`html::tags_in`] 数的一致；对不上的章不动）。
fn breaks_in(e: &Entry, media: Option<&MediaEnv>, css: &HashMap<&str, std::sync::Arc<Rules>>, dims: &HashMap<&str, (u32, u32)>, k: KindleImageRef) -> Vec<usize> {
    let Ok(text) = std::str::from_utf8(&e.data) else { return Vec::new() };
    let Some((bs, be)) = html::body_range(text) else { return Vec::new() };
    let tag_count = html::tags_in(text, bs, be).filter(|t| t.is_start()).count();
    let html = Html::parse_document(text);
    let sheet = Sheet::for_doc(&html, &e.name, media, |path| css.get(path).cloned());
    let comp = |el: &ElementRef, parent: &Computed| {
        let mut d = sheet.cascade(el, &e.name);
        crate::cascade::presentational_hints(el, &mut d);
        Computed::derive(parent, &d, el.value().name())
    };
    let root = html.root_element();
    let Some(body) = root.children().filter_map(ElementRef::wrap).find(|el| el.value().name() == "body") else { return Vec::new() };
    let rc = Rc::new(comp(&root, &Computed::root()));
    let bc = Rc::new(comp(&body, &rc));
    // body 里的元素按文档序：(元素, 计算值)；数目要和标签扫描一致才知道改哪个标签
    let mut els: Vec<(ElementRef, Rc<Computed>)> = Vec::new();
    let mut stack: Vec<(ego_tree::NodeRef<Node>, Rc<Computed>)> = body.children().rev().map(|c| (c, bc.clone())).collect();
    while let Some((n, parent)) = stack.pop() {
        let Some(el) = ElementRef::wrap(n) else { continue };
        let c = Rc::new(comp(&el, &parent));
        els.push((el, c.clone()));
        for ch in n.children().rev() {
            stack.push((ch, c.clone()));
        }
    }
    if els.len() != tag_count {
        return Vec::new();
    }
    let rem = k.v_rem_px;
    let line = rem * 1.2;
    let px = |l: &Option<Len>, fs: f64| match l {
        Some(Len::Em(n)) => n * fs * rem,
        Some(Len::Percent(n)) => n / 100.0 * PAGE_WIDTH_EM * rem,
        Some(Len::Pt(n)) => n / crate::cascade::PT_PER_EM * rem,
        None => 0.0,
    };
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
        let imgs: f64 = el.descendants().filter_map(ElementRef::wrap).filter(|d| d.value().name() == "img").map(|d| {
            let dc = els.iter().find(|(x, _)| x.id() == d.id()).map(|(_, c)| c.clone());
            dc.map_or(0.0, |dc| img_h(&d, &dc))
        }).sum();
        h += imgs + leaf_text(el, c);
        h
    };
    let mut out = Vec::new();
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
        if el.value().classes().any(|c| c == "eink-sp") {
            skip_until = Some(el.id());
            continue;
        }
        if c.break_before {
            acc = 0.0;
        }
        if !is_blockish(el, c) {
            continue;
        }
        let has_block_child = el.children().filter_map(ElementRef::wrap).any(|ch| els.iter().find(|(x, _)| x.id() == ch.id()).is_some_and(|(x, cc)| is_blockish(x, cc)));
        let m = px(&c.margin[0], c.font_size);
        if m >= MIN_SHARE * k.content_h {
            let h = height_of(i);
            if acc > 0.0 && acc + m + h > FIT_SHARE * k.content_h {
                out.push(i);
                acc = h;
            } else {
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
    out
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
        apply(&mut book, &mut WashReport::default(), None, None, kref());
        let out = String::from_utf8(book[3].data.clone()).unwrap();
        assert!(out.contains(r#"<div class="image eink-pb" style="margin-top:55%;margin-top:0;page-break-before:always;">"#), "{out}");
        assert!(out.contains("eink-pb.css"));
        let sheet = String::from_utf8(book.iter().find(|e| e.name == "O/eink-pb.css").unwrap().data.clone()).unwrap();
        assert!(sheet.contains("html body div.eink-pb{page-break-before:always;margin-top:0.01em;}"), "{sheet}");
        // 放得下的（《飘》「第一卷」：文件开头上外边距 47% 的标题）不动
        let title = r#"<html xmlns="http://www.w3.org/1999/xhtml"><head><title>t</title></head><body><div><h1 style="margin-top:47%">第一卷</h1></div></body></html>"#;
        let mut b2 = vec![book[0].clone(), Entry { name: "O/c.xhtml".into(), data: title.as_bytes().to_vec() }];
        let before = b2.clone();
        apply(&mut b2, &mut WashReport::default(), None, None, kref());
        assert_eq!(b2, before);
    }
}
