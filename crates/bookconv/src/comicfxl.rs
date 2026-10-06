//! 漫画写成固定版式（profile 的 `comic_fixed_layout`，Kindle 用）。
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
pub fn page(page: &str, w: u32, h: u32) -> Option<String> {
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
            edits.push((t.start, t.end, html::set_attr(tag, "style", &format!("width:{w}px;height:{h}px"))));
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

    #[test]
    fn canvas_shape_tolerates_one_pixel() {
        assert!(fills_canvas((1272, 1696), 1272, 1696) && fills_canvas((701, 935), 1272, 1696));
        assert!(!fills_canvas((300, 200), 1272, 1696) && !fills_canvas((700, 1000), 1272, 1696) && !fills_canvas((0, 0), 1272, 1696));
    }
}
