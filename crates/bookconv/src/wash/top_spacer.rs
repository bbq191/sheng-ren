//! 页首的上边距垫成空白图（不认 `margin-top` 的阅读器：Move，profile `margin_top = false`；只修复、照 Send to Kindle 统一的文字书）。
//!
//! xochitl 不认 `margin-top`（`padding-top`、`border-top`、相对定位也不认），页首的空元素也不占地方，只有真内容能把下面的东西推下去
//! （2026-10-12 测试书「测试-标题位置」1–7，读排出的 PDF 量）。原书靠上外边距把标题压到页面中下部的（《飘》「第一卷」
//! `h1{margin-top:47%;padding-top:1.2em}`），Kindle、掌阅在约三分之一处，Move 顶在页首。所以每章开头那块写了上边距的，
//! 在 `<body>` 一开头垫一张透明的空白图（不加任何字）：
//!
//! - **Kindle 上的距离**照 KFX 写出器算：从 `<body>` 往里，第一个有内容（字、图）的元素和它外面套的块，上外边距相邻的合并取大、
//!   上内边距照加；`%` 按 32em 页宽（写出器 `PAGE_WIDTH_EM`）、`pt` 按 12pt 一 em，一 em 是 Kindle 正文字号的像素（39）；
//! - 按两台内容区的高折到 Move（同样占页高的比例），减去 xochitl 在图后面自己加的一段空（标题约 91px × 字号 em，
//!   别的约 46px；测试书量的），就是垫图的高；不到内容区高 5% 的不垫（那段空是约数）；
//! - 图是一张 100×200 的透明 PNG（`eink-spacer.png`），宽度写百分比（Move 只认百分比宽度），高按比例出来；规则写在
//!   `eink-spacer.css`（选择器 `html body img.类`，同 `kindle_images`）。
use super::*;
use crate::cascade::{Computed, Len, MediaEnv, Rules, Sheet};
use scraper::{ElementRef, Html, Node};
use std::collections::BTreeMap;
use std::rc::Rc;

const CSS_NAME: &str = "eink-spacer.css";
const IMG_NAME: &str = "eink-spacer.png";
/// 垫图的原始宽高：高是宽的 2 倍，宽 100% 时高是内容区宽的 2 倍，够垫满一页。
const IMG_W: u32 = 100;
const IMG_H: u32 = 200;
/// 写出器把百分比折成多少 em 的页宽（同 `kfx::write` 的 `PAGE_WIDTH_EM`）。
const PAGE_WIDTH_EM: f64 = 32.0;
/// xochitl 在垫图后面自己加的空（屏幕像素）：标题每 em 字号多少、别的块多少（2026-10-12 测试书：h1 字号 1/1.35/2em 时
/// 91/122/181，段落、div 46，和字号无关）。
const GAP_HEADING_PER_EM: f64 = 91.0;
const GAP_OTHER: f64 = 46.0;
/// 垫图至少占内容区高的多少才垫：上面那段空是量出来的约数（误差几十像素），差得不多的（阿加莎、《春雪》每章标题上方 2%–4%）
/// 垫了未必更准，不垫。
const MIN_SHARE: f64 = 0.05;

/// 垫图要的尺寸：Kindle 内容区宽、高、一 em 的像素（[`KindleImageRef`]），这个阅读器内容区的宽、高（像素）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SpacerGeometry {
    pub kindle: KindleImageRef,
    pub area: (u32, u32),
}

pub(super) fn apply(entries: &mut Vec<Entry>, rep: &mut WashReport, media: Option<&MediaEnv>, g: SpacerGeometry) {
    let css: HashMap<&str, std::sync::Arc<Rules>> = entries
        .iter()
        .filter(|e| is_css_name(&e.name))
        .map(|e| {
            let text = match std::str::from_utf8(&e.data) {
                Ok(t) => Cow::Borrowed(t),
                Err(_) => Cow::Owned(crate::util::latin1_decode(&e.data)),
            };
            (e.name.as_str(), Rules::parse(&text, &e.name, media))
        })
        .collect();
    let chapters: Vec<usize> = (0..entries.len()).filter(|&i| is_chapter_entry(&entries[i])).collect();
    let heights: Vec<Option<f64>> = crate::util::par_map(&chapters, |&i| lead(&entries[i], media, &css, g).map(|(k, heading_fs)| spacer_height(k, heading_fs, g)).filter(|&h| h >= MIN_SHARE * f64::from(g.area.1)));
    if heights.iter().all(Option::is_none) {
        return;
    }
    let opf_idx = find_opf(entries);
    let dir = opf_idx.map(|i| dir_of(&entries[i].name).to_string()).unwrap_or_default();
    let at = |n: &str| if dir.is_empty() { n.to_string() } else { format!("{dir}/{n}") };
    let (css_path, img_path) = (at(CSS_NAME), at(IMG_NAME));
    let mut rules: BTreeMap<String, String> = BTreeMap::new();
    for (&i, h) in chapters.iter().zip(&heights) {
        let Some(h) = *h else { continue };
        // 宽度百分比：高 = 内容区宽 × 百分比 × 高宽比
        let pct = h / (f64::from(g.area.0) * f64::from(IMG_H) / f64::from(IMG_W)) * 100.0;
        let pct = super::kindle_rules::fmt_num((pct * 100.0).round() / 100.0);
        let class = format!("eink-sp-{}", pct.replace('.', "_"));
        rules.entry(class.clone()).or_insert_with(|| format!("html body img.{class}{{width:{pct}%;height:auto;}}"));
        let e = &mut entries[i];
        let Ok(text) = std::str::from_utf8(&e.data) else { continue };
        let Some(body) = html::tags(text).find(|t| t.is_start() && t.is("body")) else { continue };
        let src = crate::epubzip::href_to(dir_of(&e.name), &img_path, "");
        let out = format!(r#"{}<div class="eink-sp"><img class="{class}" src="{src}" alt=""/></div>{}"#, &text[..body.end], &text[body.end..]);
        let href = crate::epubzip::href_to(dir_of(&e.name), &css_path, "");
        e.data = typeset::inject_css_link(&out, &href).into_bytes();
        rep.kindle_rule_edits += 1;
    }
    let mut sheet = String::from("/* 页首的上边距垫成空白图（sheng-ren kindle_rules，见 wash/top_spacer.rs） */\n");
    for r in rules.values() {
        sheet.push_str(r);
        sheet.push('\n');
    }
    add_css_entry(entries, opf_idx, &css_path, &sheet, "eink-spacer-css");
    if !entries.iter().any(|e| e.name == img_path) {
        entries.push(Entry { name: img_path.clone(), data: spacer_png() });
        if let Some(oi) = opf_idx {
            let text = String::from_utf8_lossy(&entries[oi].data);
            let href = crate::epubzip::href_to(&dir, &img_path, "");
            if let Some(t) = opf::insert_manifest_items(&text, &[opf::NewItem { id: "eink-spacer-png", href: &href, media_type: "image/png", properties: "" }]) {
                entries[oi].data = t.into_bytes();
            }
        }
    }
}

/// 垫图的高（这个阅读器的屏幕像素）：Kindle 上的距离按内容区高的比例折过来，减去 xochitl 在图后面自己加的空。
fn spacer_height(kindle_px: f64, heading_fs: Option<f64>, g: SpacerGeometry) -> f64 {
    let target = kindle_px * f64::from(g.area.1) / g.kindle.content_h;
    target - heading_fs.map_or(GAP_OTHER, |fs| GAP_HEADING_PER_EM * fs)
}

/// 透明的 100×200 PNG。
fn spacer_png() -> Vec<u8> {
    use image::ImageEncoder;
    let mut out = Vec::new();
    let px = vec![0u8; (IMG_W * IMG_H * 2) as usize];
    image::codecs::png::PngEncoder::new(&mut out).write_image(&px, IMG_W, IMG_H, image::ExtendedColorType::La8).expect("PNG 编码");
    out
}

/// 一章开头第一个有内容的元素在 Kindle 上离内容区顶多远（像素），和它所在的块是不是标题（是的话给标题字号，根 em）。
/// 没有上边距、没有内容的 `None`。
fn lead(e: &Entry, media: Option<&MediaEnv>, css: &HashMap<&str, std::sync::Arc<Rules>>, g: SpacerGeometry) -> Option<(f64, Option<f64>)> {
    let text = std::str::from_utf8(&e.data).ok()?;
    let html = Html::parse_document(text);
    let sheet = Sheet::for_doc(&html, &e.name, media, |path| css.get(path).cloned());
    let comp = |el: &ElementRef, parent: &Computed| {
        let mut d = sheet.cascade(el, &e.name);
        crate::cascade::presentational_hints(el, &mut d);
        Computed::derive(parent, &d, el.value().name())
    };
    let root = html.root_element();
    let body = root.children().filter_map(ElementRef::wrap).find(|el| el.value().name() == "body")?;
    let mut parent = Rc::new(comp(&root, &Computed::root()));
    parent = Rc::new(comp(&body, &parent));
    // 从 body 往里一路走第一个有内容的孩子，记下沿途的块
    let has_content = |n: ego_tree::NodeRef<Node>| {
        n.descendants().any(|d| match d.value() {
            Node::Text(t) => !t.trim().is_empty(),
            Node::Element(v) => v.name() == "img" || v.name() == "svg",
            _ => false,
        })
    };
    let mut chain: Vec<(String, Rc<Computed>)> = Vec::new();
    let mut node = *body;
    while let Some(next) = node.children().find(|c| has_content(*c)) {
        // 前面有没内容的兄弟元素（空段、`<br/>`）：Kindle 上也占地方或另算，拿不准就不垫
        if node.children().take_while(|c| c.id() != next.id()).any(|c| matches!(c.value(), Node::Element(_))) {
            return None;
        }
        let Some(el) = ElementRef::wrap(next) else { break };
        let c = Rc::new(comp(&el, &parent));
        if !crate::cascade::is_block(&el, &c) {
            break;
        }
        chain.push((el.value().name().to_ascii_lowercase(), c.clone()));
        parent = c;
        node = next;
    }
    let (tag, last) = chain.last()?;
    let kem = g.kindle.em_px;
    let px = |l: &Option<Len>, fs: f64| match l {
        Some(Len::Em(n)) => n * fs * kem,
        Some(Len::Percent(n)) => n / 100.0 * PAGE_WIDTH_EM * kem,
        Some(Len::Pt(n)) => n / crate::cascade::PT_PER_EM * kem,
        None => 0.0,
    };
    // 上外边距相邻的合并（中间隔着上内边距、上边框的不合并），上内边距照加
    let (mut offset, mut pending) = (0.0f64, 0.0f64);
    for (_, c) in &chain {
        pending = pending.max(px(&c.margin[0], c.font_size));
        let pad = px(&c.padding[0], c.font_size);
        if pad > 0.0 || c.border[0].is_some() {
            offset += pending + pad;
            pending = 0.0;
        }
    }
    offset += pending;
    let heading = matches!(tag.as_str(), "h1" | "h2" | "h3" | "h4" | "h5" | "h6").then_some(last.font_size);
    (offset > 0.0).then_some((offset, heading))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn geom() -> SpacerGeometry {
        SpacerGeometry { kindle: KindleImageRef { content_px: 1104.0, content_h: 1546.0, em_px: 39.0 }, area: (842, 1455) }
    }

    fn book(body: &str, css: &str) -> Vec<Entry> {
        let e = |n: &str, d: String| Entry { name: n.into(), data: d.into_bytes() };
        vec![
            e("O/content.opf", r#"<package><manifest><item id="c" href="c.xhtml" media-type="application/xhtml+xml"/></manifest><spine><itemref idref="c"/></spine></package>"#.into()),
            e("O/s.css", css.into()),
            e("O/c.xhtml", format!(r#"<html xmlns="http://www.w3.org/1999/xhtml"><head><link rel="stylesheet" href="s.css"/></head>{body}</html>"#)),
        ]
    }

    /// 《飘》「第一卷」：套两层 div 的 h1，上外边距 47%、上内边距 1.2em。
    #[test]
    fn heading_pushed_down_like_kindle() {
        let mut b = book(r#"<body class="fen"><div id="a"><div id="b"><h1 class="h2"><span>第一卷</span></h1></div></div></body>"#, "h1.h2{font-size:1.35em;margin-top:47%;padding-top:1.2em}");
        let mut rep = WashReport::default();
        apply(&mut b, &mut rep, None, geom());
        let c = String::from_utf8(b.iter().find(|e| e.name == "O/c.xhtml").unwrap().data.clone()).unwrap();
        assert!(c.contains(r#"<body class="fen"><div class="eink-sp"><img class="eink-sp-"#), "{c}");
        assert!(c.contains("eink-spacer.css"));
        let sheet = String::from_utf8(b.iter().find(|e| e.name == "O/eink-spacer.css").unwrap().data.clone()).unwrap();
        // Kindle：47% × 32em × 39 = 586.6，加 1.2 × 1.35 × 39 = 63.2 → 649.8；折到 Move 611.5，减标题的 91 × 1.35 → 488.7；
        // 宽 488.7 / 1684 = 29.02%
        assert!(sheet.contains("html body img.eink-sp-29_02{width:29.02%;height:auto;}"), "{sheet}");
        assert!(b.iter().any(|e| e.name == "O/eink-spacer.png" && !e.data.is_empty()));
        assert!(String::from_utf8_lossy(&b[0].data).contains("eink-spacer.png"));
    }

    #[test]
    fn nothing_to_push_or_unsure_is_left_alone() {
        for (body, css) in [
            (r#"<body><p>正文</p></body>"#, "p{margin-top:1em}"),
            (r#"<body><p class="e"></p><h1 class="t">标题</h1></body>"#, "h1.t{margin-top:40%}"),
            (r#"<body><h1>标题</h1></body>"#, "h1{margin-top:0}"),
        ] {
            let mut b = book(body, css);
            let before = b.clone();
            apply(&mut b, &mut WashReport::default(), None, geom());
            assert_eq!(b, before, "{body} {css}");
        }
    }
}
