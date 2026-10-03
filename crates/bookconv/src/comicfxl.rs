//! 漫画写成固定版式（profile 的 `comic_fixed_layout`，Kindle 用）。
//!
//! Kindle 自带阅读器排流式版式时强制留页边距（最小档左右还有 101px），整页图离屏幕边缘做不到 1px。固定版式下每页按
//! `original-resolution` 的画布整页显示：画布取屏幕实际分辨率（截图坐标 1272×1696），图按 1:1 铺满整屏
//! （2026-09-30 真机：截图和页面图逐像素对齐，缩放 1.000、四边偏差 0）。
//!
//! 声明照 KindleGen 的约定写在 OPF 的 `<meta name content>` 里（`fixed-layout`、`original-resolution`、`book-type`、
//! `orientation-lock`、`zero-gutter`、`zero-margin`），再加 EPUB 3 的 `rendition:layout = pre-paginated`；AZW3 写出器把前面
//! 那组原样写成同名的 EXTH 记录。原书自带的同名声明（比如按 1440×1920 设计的固定版式漫画）先去掉，换成新画布的。
//! 每页 `<head>` 加同尺寸的 `viewport`；只有图、没有字的页，图的宽高比和画布一致时按画布大小显示（[`fills_canvas`]），
//! 不一致的（装饰小图、动图、超大或解不开没排版的图）只限制不超出画布、保持比例，不拉伸变形。只动版式声明和样式，文字、图片不变。
use crate::html;
use crate::wash::opf::is_local;

/// OPF `<meta name=…>` 里固定版式那一组（和 AZW3 写出器认的一样）。原书的 `RegionMagnification` 也去掉（整页图不需要分格放大）。
const NAMED: [&str; 8] = ["fixed-layout", "original-resolution", "book-type", "orientation-lock", "zero-gutter", "zero-margin", "RegionMagnification", "primary-writing-mode"];
/// EPUB 3 的 `<meta property=…>` 版式声明。
const PROPERTIES: [&str; 3] = ["rendition:layout", "rendition:spread", "rendition:orientation"];

/// 加到 `eink-wash.css` 的规则：页面不留外边距（固定版式下画布就是整页）。
pub const CSS_RULES: &str = "html{margin:0;padding:0;}\nbody{margin:0;padding:0;}\n";

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
        let end = if t.kind == html::TagKind::Open { html::find_close(opf, t.end, t.name).map_or(t.end, |c| c.end) } else { t.end };
        // 连同前面的空白一起去掉，不留空行
        let start = opf[..t.start].trim_end_matches([' ', '\t', '\r', '\n']).len();
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

/// 一页：`<head>` 里的 `viewport` 换成 `w×h`；只有图、没有字的页，图按画布大小显示（`style` 换成 `width`/`height` 像素）。
/// 没有 `<head>` 的不动。
///
/// 默认这一页的图已经排成画布比例（[`page_sized`] 的尺寸一律当成画布）。拿得到图的实际尺寸时用 [`page_sized`]：
/// 比例和画布不一致的图在这里会被拉伸变形。
pub fn page(page: &str, w: u32, h: u32) -> Option<String> {
    page_sized(page, w, h, |_| Some((w, h)))
}

/// 同 [`page`]，只有图的页按图的实际尺寸决定样式：`img_size(src)`（`src` 是 `<img>` 的 `src` 属性原文，调用方按章节路径
/// 解析成条目、取**处理后**图片的宽高）满足 [`fills_canvas`] → 按画布大小显示；比例不一致或取不到尺寸 → 只写
/// `max-width`/`max-height` 为画布大小（比画布大的保持比例缩进画布，小的按原尺寸，不放大不变形）。
pub fn page_sized(page: &str, w: u32, h: u32, img_size: impl Fn(&str) -> Option<(u32, u32)>) -> Option<String> {
    let head = html::tags(page).find(|t| t.is_start() && t.is("head"))?;
    let head_close = html::find_close(page, head.end, "head")?;
    let viewport = format!("<meta name=\"viewport\" content=\"width={w}, height={h}\"/>");
    let mut edits: Vec<(usize, usize, String)> = Vec::new();
    for t in html::tags_in(page, head.end, head_close.start).filter(|t| t.is_start() && t.is("meta")) {
        if html::attr_value(&page[t.start..t.end], "name").is_some_and(|n| n.eq_ignore_ascii_case("viewport")) {
            edits.push((t.start, t.end, String::new()));
        }
    }
    edits.push((head_close.start, head_close.start, viewport));
    if let Some((b0, b1)) = html::body_range(page) {
        let body = &page[b0..b1];
        let media = html::tags_in(page, b0, b1).filter(|t| t.is_start() && (is_local(t.name, "img") || is_local(t.name, "image") || is_local(t.name, "svg"))).count();
        let imgs: Vec<html::Tag> = html::tags_in(page, b0, b1).filter(|t| t.is_start() && t.is("img")).collect();
        if media == 1 && imgs.len() == 1 && html::plain_text(body).trim().is_empty() {
            let t = &imgs[0];
            let tag = &page[t.start..t.end];
            let fills = html::attr_value(tag, "src").and_then(img_size).is_some_and(|d| fills_canvas(d, w, h));
            let style = if fills { format!("width:{w}px;height:{h}px") } else { format!("max-width:{w}px;max-height:{h}px") };
            edits.push((t.start, t.end, html::set_attr(tag, "style", &style)));
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
        assert!(out.contains(r#"content="1272x1696""#) && out.contains("pre-paginated") && out.contains(r#"<meta name="cover" content="c"/>"#), "{out}");
        assert_eq!(out.matches("fixed-layout").count(), 1);
    }

    #[test]
    fn image_page_gets_viewport_and_full_canvas_text_page_only_viewport() {
        let p = r#"<html><head><title>1</title><meta name="viewport" content="width=1440,height=1920" /></head><body><center><div><img src="a.jpg" alt="" style="height:100%"/></div></center></body></html>"#;
        let out = page(p, 1272, 1696).unwrap();
        assert!(out.contains(r#"<meta name="viewport" content="width=1272, height=1696"/></head>"#) && !out.contains("1440"), "{out}");
        assert!(out.contains(r#"style="width:1272px;height:1696px""#), "{out}");
        let t = r#"<html><head><title>1</title></head><body><p>版权页</p><img src="a.jpg"/></body></html>"#;
        let out = page(t, 1272, 1696).unwrap();
        assert!(out.contains("viewport") && !out.contains("1272px"), "有字的页图不动: {out}");
        assert_eq!(page(&page(p, 1272, 1696).unwrap(), 1272, 1696), None, "幂等");
    }

    /// 只给比例和画布一致的图写画布宽高；装饰小图（300×200 的 logo）、取不到尺寸的图保持比例，不拉伸。
    #[test]
    fn only_canvas_shaped_images_are_sized_to_canvas() {
        let p = |src: &str| format!(r#"<html><head><title>1</title></head><body><div><img src="{src}" alt=""/></div></body></html>"#);
        let size = |src: &str| match src {
            "full.jpg" => Some((1272, 1696)),
            "native.png" => Some((702, 936)), // 不放大的 PNG：按 1272:1696 补白，高 = round(702×1696/1272) = 936
            "logo.png" => Some((300, 200)),
            _ => None,
        };
        let style = |src: &str| {
            let out = page_sized(&p(src), 1272, 1696, size).unwrap();
            html::attr_value(&out[out.find("<img").unwrap()..], "style").unwrap().to_string()
        };
        assert_eq!(style("full.jpg"), "width:1272px;height:1696px");
        assert_eq!(style("native.png"), "width:1272px;height:1696px");
        assert_eq!(style("logo.png"), "max-width:1272px;max-height:1696px", "装饰小图不拉伸");
        assert_eq!(style("missing.jpg"), "max-width:1272px;max-height:1696px", "取不到尺寸的不写死宽高");
        assert!(fills_canvas((1272, 1696), 1272, 1696) && fills_canvas((701, 935), 1272, 1696));
        assert!(!fills_canvas((300, 200), 1272, 1696) && !fills_canvas((700, 1000), 1272, 1696) && !fills_canvas((0, 0), 1272, 1696));
    }
}
