//! 页首的上边距垫成空白图（不认 `margin-top` 的阅读器：Move，profile `margin_top = false`；只修复、照 Send to Kindle 统一的文字书）。
//!
//! xochitl 不认 `margin-top`（`padding-top`、`border-top`、相对定位也不认），页首的空元素也不占地方，只有真内容能把下面的东西推下去
//! （2026-10-12 测试书「测试-标题位置」1–7，读排出的 PDF 量）。原书靠上外边距把标题压到页面中下部的（《飘》「第一卷」
//! `h1{margin-top:47%;padding-top:1.2em}`），Kindle、掌阅在约三分之一处，Move 顶在页首。所以每章开头那块写了上边距的，
//! 在 `<body>` 一开头垫一张透明的空白图（不加任何字）：
//!
//! - **Kindle 上的距离**照 KFX 写出器算：从 `<body>` 往里，第一个有内容（字、图）的元素和它外面套的块，上外边距相邻的合并取大、
//!   上内边距照加；`%` 按 32em 页宽（`kindle_model::PAGE_WIDTH_EM`）、`pt` 按 12pt 一 em。写出器把竖直距离写成行高的倍数，Kindle 按行距
//!   换成像素，所以一 rem 是 Kindle 正文一行的像素 / 1.2（profile `line_px`，63 → 52.5；2026-10-12 截图「第一卷」上外边距落在 779px）；
//! - 按两台内容区的高折到 Move（同样占页高的比例），减去 xochitl 在图后面自己加的一段空（量出来的数写在 profile `[page_top]`：标题约 91px × 字号 em，
//!   别的约 46px；测试书量的），就是垫图的高；不到内容区高 5% 的不垫（那段空是约数）；
//! - 图是一张 100×200 的透明 PNG（`eink-spacer.png`），宽度写百分比（Move 只认百分比宽度），高按比例出来；规则写在
//!   `eink-spacer.css`（选择器 `html body img.类`，同 `kindle_images`）。
//! - 要垫的比 xochitl 自己加的那段空还小（《三体》获奖感言 2em 标题只要 130px，那段空约 182px）、或者差不到 5% 垫不了的：
//!   第一块开头是字时，改在**这一块里面的最前面**放一张极窄的透明图（`eink-strut.png`，1×400），把第一行撑高，字就往下挪
//!   「图高 − 44px × 字号 em」（2026-10-13 测试书「测试-行内垫图」：2em 标题图高 130/300 挪 42/212，段落 130 挪 86；图高不到这个数
//!   不挪；横向挪不到 1px；标题前不会再多那段空）。只有一行内起作用，不加字。块在 Move 上的行距 L 不是 1 时（`<p>` 不认行距，
//!   别的块认；行距先由 `block_style.rs` 照 Kindle 归一过），字照样挪 Δ、后面的内容挪 L × Δ（测试书「测试-行内垫图5」：L 为 0.74、
//!   1.618 都是），取 Δ = 2 × 要挪的 / (1 + L)，字和后面的内容差得一样多。
use super::*;
use crate::cascade::{Computed, Len, MediaEnv, Rules};
use crate::kindle_model::{self as km, Chapter};
use crate::reader_model::ReaderModel;
use profile::PageTop;
use scraper::Node;
use std::collections::BTreeMap;
use std::rc::Rc;

const CSS_NAME: &str = "eink-spacer.css";
const IMG_NAME: &str = "eink-spacer.png";
/// 垫图的原始宽高：高是宽的 2 倍，宽 100% 时高是内容区宽的 2 倍，够垫满一页。
const IMG_W: u32 = 100;
const IMG_H: u32 = 200;
/// 垫图至少占内容区高的多少才垫：上面那段空是量出来的约数（误差几十像素），差得不多的（阿加莎、《春雪》每章标题上方 2%–4%）
/// 垫了未必更准，不垫。
const MIN_SHARE: f64 = 0.05;
const STRUT_NAME: &str = "eink-strut.png";
const STRUT_W: u32 = 1;
const STRUT_H: u32 = 400;

/// 一章怎么垫：`<body>` 开头一块空白图（高），或者第一块里面最前面一张行内的窄图（高、第一块是 `<body>` 里第几个开始标签）。
#[derive(Clone, Copy, Debug, PartialEq)]
enum Pad {
    Block(f64),
    Strut(f64, usize),
}

/// 垫图要的尺寸：Kindle 内容区宽、高、一 em 的像素（[`KindleImageRef`]），这个阅读器内容区的宽、高（像素）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SpacerGeometry {
    pub kindle: KindleImageRef,
    pub area: (u32, u32),
}

/// `reader` 是阅读器模型：垫图后面阅读器自己加的空、行内垫图的基数（`page_top`，没有就不垫）、第一块在这台上的字号。
pub(super) fn apply(entries: &mut Vec<Entry>, rep: &mut WashReport, media: Option<&MediaEnv>, g: SpacerGeometry, reader: &ReaderModel) {
    let Some(t) = reader.page_top else { return };
    let css = km::book_css(entries, media);
    let chapters: Vec<usize> = (0..entries.len()).filter(|&i| is_chapter_entry(&entries[i])).collect();
    let pads: Vec<Option<Pad>> = crate::util::par_map(&chapters, |&i| page_box(&entries[i], media, &css, g, reader).and_then(|b| choose(&b, g, t)));
    if pads.iter().all(Option::is_none) {
        return;
    }
    let opf_idx = find_opf(entries);
    let dir = opf_idx.map(|i| dir_of(&entries[i].name).to_string()).unwrap_or_default();
    let at = |n: &str| if dir.is_empty() { n.to_string() } else { format!("{dir}/{n}") };
    let (css_path, img_path, strut_path) = (at(CSS_NAME), at(IMG_NAME), at(STRUT_NAME));
    let mut rules: BTreeMap<String, String> = BTreeMap::new();
    let mut used = (false, false);
    for (&i, pad) in chapters.iter().zip(&pads) {
        let Some(pad) = *pad else { continue };
        let e = &mut entries[i];
        let Ok(text) = std::str::from_utf8(&e.data) else { continue };
        let Some((bs, be)) = html::body_range(text) else { continue };
        let Some(body) = html::tags(text).find(|t| t.is_start() && t.is("body")) else { continue };
        let out = match pad {
            Pad::Block(h) => {
                // 宽度百分比：高 = 内容区宽 × 百分比 × 高宽比
                let pct = h / (f64::from(g.area.0) * f64::from(IMG_H) / f64::from(IMG_W)) * 100.0;
                let pct = super::kindle_rules::fmt_num((pct * 100.0).round() / 100.0);
                let class = format!("eink-sp-{}", pct.replace('.', "_"));
                rules.entry(class.clone()).or_insert_with(|| format!("html body img.{class}{{width:{pct}%;height:auto;}}"));
                used.0 = true;
                let src = crate::epubzip::href_to(dir_of(&e.name), &img_path, "");
                format!(r#"{}<div class="eink-sp"><img class="{class}" src="{src}" alt=""/></div>{}"#, &text[..body.end], &text[body.end..])
            }
            Pad::Strut(h, n) => {
                let Some(t) = html::tags_in(text, bs, be).filter(|t| t.is_start()).nth(n) else { continue };
                // 宽度百分比写到万分之一（图宽不到一像素，四舍五入差的高也不到一像素）
                let pct = h / (f64::from(g.area.0) * f64::from(STRUT_H) / f64::from(STRUT_W)) * 100.0;
                let pct = format!("{:.5}", (pct * 1e5).round() / 1e5);
                let pct = pct.trim_end_matches('0').trim_end_matches('.').to_string();
                let class = format!("eink-st-{}", pct.replace('.', "_"));
                rules.entry(class.clone()).or_insert_with(|| format!("html body img.{class}{{width:{pct}%;height:auto;}}"));
                used.1 = true;
                let src = crate::epubzip::href_to(dir_of(&e.name), &strut_path, "");
                format!(r#"{}<img class="eink-sp {class}" src="{src}" alt=""/>{}"#, &text[..t.end], &text[t.end..])
            }
        };
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
    for (on, path, id, w, h) in [(used.0, &img_path, "eink-spacer-png", IMG_W, IMG_H), (used.1, &strut_path, "eink-strut-png", STRUT_W, STRUT_H)] {
        if !on || entries.iter().any(|e| e.name == *path) {
            continue;
        }
        entries.push(Entry { name: path.clone(), data: clear_png(w, h) });
        if let Some(oi) = opf_idx {
            let text = String::from_utf8_lossy(&entries[oi].data);
            let href = crate::epubzip::href_to(&dir, path, "");
            if let Some(t) = opf::insert_manifest_items(&text, &[opf::NewItem { id, href: &href, media_type: "image/png", properties: "" }]) {
                entries[oi].data = t.into_bytes();
            }
        }
    }
}

/// 一章怎么垫（不垫的 `None`）：Kindle 上不到内容区高 5% 的不垫；`<body>` 开头垫一块够 5% 的照旧这样垫（《飘》真机对过）；
/// 垫不了的（要的比 xochitl 在垫图后自己加的空还少、或者多得不到 5%），第一块开头是字的改在块里放行内窄图，图高＝要挪的 + 44px × 字号。
fn choose(b: &PageBox, g: SpacerGeometry, t: PageTop) -> Option<Pad> {
    let min = MIN_SHARE * f64::from(g.area.1);
    let target = b.text_top * f64::from(g.area.1) / g.kindle.content_h;
    if target < min {
        return None;
    }
    let h = spacer_height(b.text_top, b.heading_fs, g, t);
    if h >= min {
        return Some(Pad::Block(h));
    }
    // 字挪 Δ、后面的内容挪 行距 × Δ：两边差得一样多，Δ = 2 × 要挪的 / (1 + 行距)
    b.strut_at.map(|(n, fs, lh)| Pad::Strut(2.0 * target / (1.0 + lh) + t.strut_base_per_em * fs, n))
}

/// 垫图的高（这个阅读器的屏幕像素）：Kindle 上的距离按内容区高的比例折过来，减去阅读器在图后面自己加的空（profile `[page_top]`）。
fn spacer_height(kindle_px: f64, heading_fs: Option<f64>, g: SpacerGeometry, t: PageTop) -> f64 {
    let target = kindle_px * f64::from(g.area.1) / g.kindle.content_h;
    target - heading_fs.map_or(t.gap_other, |fs| t.gap_heading_per_em * fs)
}

/// 透明的 PNG。
fn clear_png(w: u32, h: u32) -> Vec<u8> {
    use image::ImageEncoder;
    let mut out = Vec::new();
    let px = vec![0u8; (w * h * 2) as usize];
    image::codecs::png::PngEncoder::new(&mut out).write_image(&px, w, h, image::ExtendedColorType::La8).expect("PNG 编码");
    out
}

/// 一章开头在 Kindle 上的排法（像素，从内容区顶算）：第一个有内容的块的内容顶在哪、是不是标题；整章只有这一个块（卷首页、
/// 标题页）时 body 盒子的底在哪（背景图的 `bottom` 相对它：Kindle 上 body 只到这个块的下外边距为止，2026-10-12《飘》截图），
/// body 的背景图（书内路径）、是不是 `cover`。
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct PageBox {
    pub text_top: f64,
    pub heading_fs: Option<f64>,
    pub body_bottom: Option<f64>,
    pub bg: Option<String>,
    pub bg_cover: bool,
    /// 第一个有内容的块开头是字：它是 `<body>` 里第几个开始标签、字号（根 em）、在这台上的行距倍数；放行内垫图用。
    pub strut_at: Option<(usize, f64, f64)>,
}

/// 全书各章的 [`PageBox`]（条目下标 → 排法；拿不准的章没有）。给背景图定位用，只看 body 的底，行内垫图的字号用不到（按照 CSS 排算）。
pub(crate) fn page_boxes(entries: &[Entry], media: Option<&MediaEnv>, g: SpacerGeometry) -> HashMap<usize, PageBox> {
    let css = km::book_css(entries, media);
    let chapters: Vec<usize> = (0..entries.len()).filter(|&i| is_chapter_entry(&entries[i])).collect();
    let reader = ReaderModel::default();
    chapters.iter().zip(crate::util::par_map(&chapters, |&i| page_box(&entries[i], media, &css, g, &reader))).filter_map(|(&i, b)| b.map(|b| (i, b))).collect()
}

fn page_box(e: &Entry, media: Option<&MediaEnv>, css: &HashMap<&str, std::sync::Arc<Rules>>, g: SpacerGeometry, reader: &ReaderModel) -> Option<PageBox> {
    let text = std::str::from_utf8(&e.data).ok()?;
    let ch = Chapter::parse(&e.name, text, media, css)?;
    let body = ch.body();
    let body_comp = ch.body_comp.clone();
    // 从 body 往里一路走第一个有内容的孩子，记下沿途的块
    let has_content = |n: ego_tree::NodeRef<Node>| {
        n.descendants().any(|d| match d.value() {
            Node::Text(t) => !t.trim().is_empty(),
            Node::Element(v) => v.name() == "img" || v.name() == "svg",
            _ => false,
        })
    };
    let mut chain: Vec<(String, Rc<Computed>)> = Vec::new();
    let mut single = true;
    let mut node = *body;
    let mut last_node = node;
    while let Some(next) = node.children().find(|c| has_content(*c)) {
        // 前面有没内容的兄弟元素（空段、`<br/>`）：Kindle 上也占地方或另算，拿不准
        if node.children().take_while(|c| c.id() != next.id()).any(|c| matches!(c.value(), Node::Element(_))) {
            return None;
        }
        let Some(i) = ch.index_of(next.id()) else { break };
        if !ch.els[i].is_block {
            break;
        }
        let c = ch.els[i].comp.clone();
        single &= node.children().filter(|c| has_content(*c)).count() == 1;
        chain.push((ch.el(i).value().name().to_ascii_lowercase(), c));
        node = next;
        last_node = next;
    }
    let (tag, last) = chain.last()?;
    // Kindle 的竖直距离：写出器折成行高的倍数，Kindle 按行距换成像素（一 rem ≈ 正文一行 / 1.2）；`%` 按 32em 页宽
    let rem = g.kindle.v_rem_px;
    let px = |l: &Option<Len>, fs: f64| km::vertical_rem(*l, fs) * rem;
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
    // 只有这一个块、里面只有字：body 的底＝字的底 + 下内边距 + 下外边距（相邻的合并）。字按一行一行排，一行高＝字号 × 正文行距
    let body_bottom = (single && !last_node.descendants().any(|d| matches!(d.value(), Node::Element(v) if v.name() == "img" || v.name() == "svg"))).then(|| {
        let chars: usize = last_node.descendants().filter_map(|d| if let Node::Text(t) = d.value() { Some(t.chars().filter(|c| !c.is_whitespace()).count()) } else { None }).sum();
        let lines = ((chars as f64 * last.font_size * g.kindle.em_px) / g.kindle.content_px).ceil().max(1.0);
        let mut bottom = offset + lines * last.font_size * rem * km::LH_EM;
        let mut pending = 0.0f64;
        for (_, c) in chain.iter().rev() {
            let pad = px(&c.padding[2], c.font_size);
            if pad > 0.0 || c.border[2].is_some() {
                bottom += pending + pad;
                pending = 0.0;
            }
            pending = pending.max(px(&c.margin[2], c.font_size));
        }
        bottom + pending
    });
    let heading = matches!(tag.as_str(), "h1" | "h2" | "h3" | "h4" | "h5" | "h6").then_some(last.font_size);
    let starts_with_text = last_node.descendants().find_map(|d| match d.value() {
        Node::Text(t) if !t.trim().is_empty() => Some(true),
        Node::Element(v) if matches!(v.name(), "img" | "svg" | "br") => Some(false),
        _ => None,
    }) == Some(true);
    // 块在这台上的行距（Move 上 `<p>` 不认行距，算 1；别的块认）：xochitl 把行距倍数也乘到撑高的那一行上
    let lh = reader.line_height(tag, last.line_height);
    // 解析出的元素和标签扫描数得一样（没有补出来的 `tbody` 之类）才知道放在哪个标签后面
    let strut_at = (starts_with_text && ch.matches_tags).then(|| ch.index_of(last_node.id())).flatten().map(|n| {
        // 标题只认关键字的阅读器上标题按它的字号（profile `[heading_sizes]`），别的块照算出来的
        (n, reader.font_size(tag, ch.els[n].decls.get("font-size").map(String::as_str), last.font_size), lh)
    });
    Some(PageBox { text_top: offset, heading_fs: heading, body_bottom, bg: body_comp.bg_image.clone(), bg_cover: body_comp.bg_cover, strut_at })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reader() -> ReaderModel {
        ReaderModel::for_profile(profile::get("xochitl").unwrap())
    }

    fn geom() -> SpacerGeometry {
        SpacerGeometry { kindle: KindleImageRef { content_px: 1104.0, content_h: 1546.0, em_px: 39.0, v_rem_px: 52.5 }, area: (842, 1455) }
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
        let probe = book(r#"<body class="fen"><div id="a"><div id="b"><h1 class="h2"><span>第一卷</span></h1></div></div></body>"#, "body.fen{background:url(i.jpg) no-repeat bottom center} h1.h2{font-size:1.35em;margin-top:47%;padding-top:1.2em;padding-bottom:1em;margin-bottom:0.5em}");
        let pb = page_boxes(&probe, None, geom()).remove(&2).unwrap();
        // body 底：字顶 874.65 + 一行 1.35 × 63 + 下内边距 1.35 × 52.5 + 下外边距 0.675 × 52.5
        assert_eq!([pb.text_top, pb.body_bottom.unwrap()].map(|v| v.round()), [875.0, 1066.0]);
        assert_eq!((pb.bg.as_deref(), pb.bg_cover, pb.heading_fs), (Some("O/i.jpg"), false, Some(1.35)));
        let mut b = book(r#"<body class="fen"><div id="a"><div id="b"><h1 class="h2"><span>第一卷</span></h1></div></div></body>"#, "h1.h2{font-size:1.35em;margin-top:47%;padding-top:1.2em}");
        let mut rep = WashReport::default();
        apply(&mut b, &mut rep, None, geom(), &reader());
        let c = String::from_utf8(b.iter().find(|e| e.name == "O/c.xhtml").unwrap().data.clone()).unwrap();
        assert!(c.contains(r#"<body class="fen"><div class="eink-sp"><img class="eink-sp-"#), "{c}");
        assert!(c.contains("eink-spacer.css"));
        let sheet = String::from_utf8(b.iter().find(|e| e.name == "O/eink-spacer.css").unwrap().data.clone()).unwrap();
        // Kindle（一 rem 竖直 52.5px）：47% × 32 × 52.5 = 789.6，加 1.2 × 1.35 × 52.5 = 85.05 → 874.65；折到 Move 823.2，
        // 减标题的 91 × 1.35 → 700.3；宽 700.3 / 1684 = 41.59%
        assert!(sheet.contains("html body img.eink-sp-41_59{width:41.59%;height:auto;}"), "{sheet}");
        assert!(b.iter().any(|e| e.name == "O/eink-spacer.png" && !e.data.is_empty()));
        assert!(String::from_utf8_lossy(&b[0].data).contains("eink-spacer.png"));
    }

    /// 要垫的比 xochitl 标题前自己加的空还少：标题里最前面放行内窄图。Kindle 2.5rem × 52.5 = 131.25 → Move 123.52，
    /// 加 44 × 2.018（Move 上 h1 的固定字号）= 212.31；宽 212.31 / (842 × 400) = 0.06304%
    #[test]
    fn small_heading_margin_uses_inline_strut() {
        let mut b = book(r#"<body><h1 class="t"><span>标题</span></h1><p>正文</p></body>"#, "h1.t{font-size:2em;margin-top:1.25em}");
        apply(&mut b, &mut WashReport::default(), None, geom(), &reader());
        let c = String::from_utf8(b.iter().find(|e| e.name == "O/c.xhtml").unwrap().data.clone()).unwrap();
        assert!(c.contains(r#"<body><h1 class="t"><img class="eink-sp eink-st-0_06304" src="eink-strut.png" alt=""/><span>标题</span></h1>"#), "{c}");
        let sheet = String::from_utf8(b.iter().find(|e| e.name == "O/eink-spacer.css").unwrap().data.clone()).unwrap();
        assert!(sheet.contains("html body img.eink-st-0_06304{width:0.06304%;height:auto;}"), "{sheet}");
        assert!(b.iter().any(|e| e.name == "O/eink-strut.png") && !b.iter().any(|e| e.name == "O/eink-spacer.png"));
        // 标题在 Move 上的行距是 2：字挪 Δ、后面挪 2Δ，Δ = 2 × 123.52 / 3 = 82.35，加 88.79 → 171.14，宽 0.05081%
        let mut b = book(r#"<body><h1 class="t">标题</h1></body>"#, "h1.t{font-size:2em;margin-top:1.25em;line-height:2}");
        apply(&mut b, &mut WashReport::default(), None, geom(), &reader());
        let c = String::from_utf8(b.iter().find(|e| e.name == "O/c.xhtml").unwrap().data.clone()).unwrap();
        assert!(c.contains(r#"<h1 class="t"><img class="eink-sp eink-st-0_05081" "#), "{c}");
        // 第一块开头是图的不放
        let mut b = book(r#"<body><h1 class="t"><img src="i.png" alt=""/>标题</h1></body>"#, "h1.t{font-size:2em;margin-top:1.25em}");
        let before = b.clone();
        apply(&mut b, &mut WashReport::default(), None, geom(), &reader());
        assert_eq!(b, before);
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
            apply(&mut b, &mut WashReport::default(), None, geom(), &reader());
            assert_eq!(b, before, "{body} {css}");
        }
    }
}
