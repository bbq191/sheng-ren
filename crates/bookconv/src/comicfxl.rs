//! 漫画写成固定版式（profile 的 `comic_fixed_layout`，Kindle、Kobo 用）。
//!
//! Kindle 自带阅读器排流式版式时强制留页边距（最小档左右还有 101px），整页图离屏幕边缘做不到 1px。固定版式下每页按
//! `original-resolution` 的画布整页显示：画布取屏幕实际分辨率（截图坐标 1272×1696），图按 1:1 铺满整屏
//! （2026-09-30 真机：截图和页面图逐像素对齐，缩放 1.000、四边偏差 0）。
//!
//! 声明照 KindleGen 的约定写在 OPF 的 `<meta name content>` 里（`fixed-layout`、`original-resolution`、`book-type`、
//! `orientation-lock`、`zero-gutter`、`zero-margin`），再加 EPUB 3 的 `rendition:layout = pre-paginated`；AZW3 写出器把前面
//! 那组原样写成同名的 EXTH 记录。原书自带的同名声明（比如按 1440×1920 设计的固定版式漫画）先去掉，换成新画布的。
//! 每页 `<head>` 加同尺寸的 `viewport`；只有图、没有字的页，图按画布大小显示。这份 EPUB 只是中间产物：KFX 写出器不读这里的
//! 样式，按图的实际尺寸保持比例缩进画布（见 `kfx::write::fixed_image_size`），所以比例不一致的图在 Kindle 上不会被拉伸。
//! 只动版式声明和样式，文字、图片不变。
//!
//! Kobo（2026-11-05 Clara BW 真机，方案 D）不一样：整页图只在比例和画布一致（[`fills_canvas`]）时写
//! `width:100%;height:100%`，比例不符的图只写 `max-width:100%;max-height:100%`，免得 Nickel 拉伸；html/body 和 KEPUB
//! 包装的高度链都写 100%（profile `comic_fixed_layout_percent`）。纯图页的 `<img>` 在 kepub 阶段也不再包 koboSpan
//! （`kepub.rs`）。A 包同时改了像素画布、纯图页也包 koboSpan 等多处，B/C/E 各测不同变量，真机只证明 D 满屏，不能归因到单一差异。
use crate::html;
use crate::wash::opf::is_local;

/// OPF `<meta name=…>` 里固定版式那一组（和 AZW3 写出器认的一样）。原书的 `RegionMagnification` 也去掉（整页图不需要分格放大）。
const NAMED: [&str; 8] = [
    "fixed-layout",
    "original-resolution",
    "book-type",
    "orientation-lock",
    "zero-gutter",
    "zero-margin",
    "RegionMagnification",
    "primary-writing-mode",
];
/// EPUB 3 的 `<meta property=…>` 版式声明。
const PROPERTIES: [&str; 3] = [
    "rendition:layout",
    "rendition:spread",
    "rendition:orientation",
];

/// 加到 `eink-wash.css` 的规则：页面不留外边距（固定版式下画布就是整页）。
pub const CSS_RULES: &str = "html{margin:0;padding:0;}\nbody{margin:0;padding:0;}\n";

/// Kobo（方案 D）加到 `eink-wash.css` 的规则：除了不留外边距，html/body 和 KEPUB 的两层包装都撑满高度。
/// 图片是 `height:100%`，高度链断在哪一层，整页图就只占那一层的内容区、四周留边（2026-11-05 Clara BW 真机）。
pub const CSS_RULES_PERCENT: &str = "html{margin:0;padding:0;height:100%;}\nbody{margin:0;padding:0;height:100%;}\n#book-columns,#book-inner{height:100%;}\n";

/// 固定版式的 CSS：`percent` 走 Kobo 的百分比写法，否则是 Kindle 的像素画布写法。
pub fn css_rules(percent: bool) -> &'static str {
    if percent {
        CSS_RULES_PERCENT
    } else {
        CSS_RULES
    }
}

/// 去掉 OPF 里原有的版式声明，写上 `w×h` 的固定版式声明。`primary-writing-mode` 是 Kindle 的竖排声明，漫画用不上，一并去掉。
pub fn opf(opf: &str, w: u32, h: u32) -> String {
    let mut edits: Vec<(usize, usize, String)> = Vec::new();
    for t in html::tags(opf).filter(|t| t.is_start() && is_local(t.name, "meta")) {
        let tag = &opf[t.start..t.end];
        let named = html::attr_value(tag, "name").is_some_and(|n| NAMED.contains(&n));
        let prop = html::attr_value(tag, "property").is_some_and(|p| PROPERTIES.contains(&p));
        if !named && !prop {
            continue;
        }
        let end = if t.kind == html::TagKind::Open {
            html::find_close(opf, t.end, t.name).map_or(t.end, |c| c.end)
        } else {
            t.end
        };
        // 连同前面的空白一起去掉，不留空行
        let start = opf[..t.start]
            .trim_end_matches([' ', '\t', '\r', '\n'])
            .len();
        edits.push((start, end, String::new()));
    }
    let cleaned = html::apply_edits(opf, edits);
    let decl = format!(
        "<meta property=\"rendition:layout\">pre-paginated</meta><meta name=\"fixed-layout\" content=\"true\"/><meta name=\"original-resolution\" content=\"{w}x{h}\"/>\
<meta name=\"book-type\" content=\"comic\"/><meta name=\"orientation-lock\" content=\"portrait\"/><meta name=\"zero-gutter\" content=\"true\"/><meta name=\"zero-margin\" content=\"true\"/>"
    );
    crate::wash::opf::insert_metadata(&cleaned, &decl).unwrap_or(cleaned)
}

/// 图（`iw × ih`）按 `w × h` 的画布大小显示不会变形：宽高比一致，误差在图自己的比例尺上不超过 1 像素
/// （`|ih·w − iw·h| ≤ max(w, h)`）。漫画页排版（`imgopt::prepare_comic_page_for_epub`）产出的整页都满足——画布大小的页，
/// 以及不放大、按阅读范围比例补白的页（补白时另一条边四舍五入，差不到 1px）。
pub fn fills_canvas((iw, ih): (u32, u32), w: u32, h: u32) -> bool {
    iw > 0 && ih > 0 && (ih as u64 * w as u64).abs_diff(iw as u64 * h as u64) <= w.max(h) as u64
}

/// body 里只有一张 `<img>`（没有别的 `<img>`/`<image>`/`<svg>`），也没有可见文字。
fn image_only_body(page: &str, b0: usize, b1: usize) -> bool {
    let mut media = 0usize;
    let mut imgs = 0usize;
    for t in html::tags_in(page, b0, b1) {
        if !t.is_start() {
            continue;
        }
        if is_local(t.name, "img") {
            imgs += 1;
        }
        if is_local(t.name, "img") || is_local(t.name, "image") || is_local(t.name, "svg") {
            media += 1;
        }
    }
    media == 1 && imgs == 1 && html::plain_text(&page[b0..b1]).trim().is_empty()
}

/// 只有图、没有字的页：`<body>` 里只有一张 `<img>`，也没有可见文字。和 [`page`] 里给整页图写尺寸的条件、`kepub` 里
/// 不包 koboSpan 的条件是同一个判定。
pub fn image_only(page: &str) -> bool {
    html::body_range(page).is_some_and(|(b0, b1)| image_only_body(page, b0, b1))
}

/// 一页：`<head>` 里的 `viewport` 换成 `w×h`；只有图、没有字的页，图按画布大小显示。`percent`（Kobo 方案 D）时，图的比例
/// 和画布一致（[`fills_canvas`]，`dims` 给图尺寸）才写 `width:100%;height:100%`；比例不符或读不到尺寸的写
/// `max-width:100%;max-height:100%` 居中，不拉伸装饰小图、坏图。否则写 `w`/`h` 像素（Kindle）。没有 `<head>` 的不动。
pub fn page(
    page: &str,
    w: u32,
    h: u32,
    percent: bool,
    dims: impl Fn(&str) -> Option<(u32, u32)>,
) -> Option<String> {
    let head = html::tags(page).find(|t| t.is_start() && t.is("head"))?;
    let head_close = html::find_close(page, head.end, "head")?;
    let viewport = format!("<meta name=\"viewport\" content=\"width={w}, height={h}\"/>");
    let mut edits: Vec<(usize, usize, String)> = Vec::new();
    for t in
        html::tags_in(page, head.end, head_close.start).filter(|t| t.is_start() && t.is("meta"))
    {
        if html::attr_value(&page[t.start..t.end], "name")
            .is_some_and(|n| n.eq_ignore_ascii_case("viewport"))
        {
            edits.push((t.start, t.end, String::new()));
        }
    }
    edits.push((head_close.start, head_close.start, viewport));
    if let Some((b0, b1)) = html::body_range(page) {
        if image_only_body(page, b0, b1) {
            if let Some(t) = html::tags_in(page, b0, b1).find(|t| t.is_start() && t.is("img")) {
                let tag = &page[t.start..t.end];
                let style = if percent {
                    let full = html::attr_value(tag, "src")
                        .and_then(dims)
                        .is_some_and(|d| fills_canvas(d, w, h));
                    if full {
                        "width:100%;height:100%".to_string()
                    } else {
                        // 比例不符（装饰小图、处理不了的原图）或读不到尺寸：只限制不超过页面，保持原比例，
                        // 不学 Kindle 的 KFX 写出器替我们等比缩放（Nickel 直接按这份样式排）。
                        "display:block;max-width:100%;max-height:100%;margin:0 auto".to_string()
                    }
                } else {
                    format!("width:{w}px;height:{h}px")
                };
                edits.push((t.start, t.end, html::set_attr(tag, "style", &style)));
            }
        }
    }
    let out = html::apply_edits(page, edits);
    (out != page).then_some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opf_replaces_existing_layout_declarations() {
        let o = r#"<package><metadata><dc:title>x</dc:title>
    <meta property="rendition:layout">pre-paginated</meta>
    <meta name="original-resolution" content="1440x1920" />
    <meta name="fixed-layout" content="true" />
    <meta name="cover" content="c"/>
</metadata></package>"#;
        let out = opf(o, 1272, 1696);
        assert!(!out.contains("1440x1920"), "{out}");
        assert_eq!(out.matches("original-resolution").count(), 1);
        assert!(
            out.contains(r#"content="1272x1696""#)
                && out.contains("pre-paginated")
                && out.contains(r#"<meta name="cover" content="c"/>"#),
            "{out}"
        );
        assert_eq!(out.matches("fixed-layout").count(), 1);
    }

    #[test]
    fn image_page_gets_viewport_and_full_canvas_text_page_only_viewport() {
        let p = r#"<html><head><title>1</title><meta name="viewport" content="width=1440,height=1920" /></head><body><center><div><img src="a.jpg" alt="" style="height:100%"/></div></center></body></html>"#;
        let out = page(p, 1272, 1696, false, |_| None).unwrap();
        assert!(
            out.contains(r#"<meta name="viewport" content="width=1272, height=1696"/></head>"#)
                && !out.contains("1440"),
            "{out}"
        );
        assert!(
            out.contains(r#"style="width:1272px;height:1696px""#),
            "{out}"
        );
        let t = r#"<html><head><title>1</title></head><body><p>版权页</p><img src="a.jpg"/></body></html>"#;
        let out = page(t, 1272, 1696, false, |_| None).unwrap();
        assert!(
            out.contains("viewport") && !out.contains("1272px"),
            "有字的页图不动: {out}"
        );
        assert_eq!(
            page(
                &page(p, 1272, 1696, false, |_| None).unwrap(),
                1272,
                1696,
                false,
                |_| None
            ),
            None,
            "幂等"
        );
    }

    #[test]
    fn kobo_percent_page_uses_viewport_canvas_and_full_height_chain() {
        let p = r#"<html><head><title>1</title><meta name="viewport" content="width=1440,height=1920" /></head><body><center><div><img src="a.jpg" alt="" style="height:100%"/></div></center></body></html>"#;
        let out = page(p, 1072, 1448, true, |_| Some((1072, 1448))).unwrap();
        assert!(
            out.contains(r#"<meta name="viewport" content="width=1072, height=1448"/></head>"#)
                && !out.contains("1440"),
            "{out}"
        );
        assert!(out.contains(r#"style="width:100%;height:100%""#), "{out}");
        assert!(
            css_rules(true).contains("html{margin:0;padding:0;height:100%;}")
                && css_rules(true).contains("#book-columns,#book-inner{height:100%;}")
                && !css_rules(false).contains("height:100%"),
            "Kobo 高度链只在百分比写法里"
        );
        assert_eq!(
            page(
                &page(p, 1072, 1448, true, |_| Some((1072, 1448))).unwrap(),
                1072,
                1448,
                true,
                |_| Some((1072, 1448))
            ),
            None,
            "幂等"
        );
        // 装饰小图/处理不了的原图：比例和画布不一致，不能宽高都写 100% 让 Nickel 拉伸
        let small = page(p, 1072, 1448, true, |_| Some((200, 800))).unwrap();
        assert!(
            small.contains(r#"style="display:block;max-width:100%;max-height:100%;margin:0 auto""#)
                && !small.contains("width:100%;height:100%"),
            "{small}"
        );
        // 读不到图尺寸也走不拉伸的写法
        let unknown = page(p, 1072, 1448, true, |_| None).unwrap();
        assert!(
            unknown.contains("max-width:100%;max-height:100%"),
            "{unknown}"
        );
    }

    #[test]
    fn image_only_accepts_one_img_and_no_text() {
        assert!(image_only(
            r#"<html><body><div><img src="a.jpg"/></div></body></html>"#
        ));
        assert!(!image_only(
            r#"<html><body><p>字</p><img src="a.jpg"/></body></html>"#
        ));
        assert!(!image_only(
            r#"<html><body><img src="a.jpg"/><img src="b.jpg"/></body></html>"#
        ));
        assert!(!image_only(
            r#"<html><body><svg><image href="a.jpg"/></svg></body></html>"#
        ));
        assert!(!image_only("<html><p>无 body</p></html>"));
    }

    #[test]
    fn canvas_shape_tolerates_one_pixel() {
        assert!(fills_canvas((1272, 1696), 1272, 1696) && fills_canvas((701, 935), 1272, 1696));
        assert!(
            !fills_canvas((300, 200), 1272, 1696)
                && !fills_canvas((700, 1000), 1272, 1696)
                && !fills_canvas((0, 0), 1272, 1696)
        );
    }
}
