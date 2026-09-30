//! EPUB 漫画识别：给优化器用，决定该书走"漫画路"（裁白边、按阅读范围单趟缩放补白、画质优先）还是"文字书路"
//! （常规降采样）。阈值（2026-09-14 定案）：图 ≥20 张且平均每张图配的文字 <40 字。只看 html 文字与 `<img>`
//! 引用，不读图片字节（流式优化阶段一的图片条目是空占位）。

use crate::epubzip::Entry;
use crate::wash::parse_opf;
use regex::Regex;
use std::sync::OnceLock;

/// 判定漫画的最小图片数。
pub const MIN_IMAGES: usize = 20;
/// 判定漫画的"平均每张图配的文字数"上限。
pub const TEXT_PER_IMAGE: f64 = 40.0;

pub(crate) fn strip_noise_tags(html: &str) -> String {
    static RE: OnceLock<Regex> = OnceLock::new();
    // regex crate 不支持反向引用，三种标签各写一条 alternation。
    RE.get_or_init(|| Regex::new(r#"(?is)<script\b.*?</script>|<style\b.*?</style>|<head\b.*?</head>"#).unwrap()).replace_all(html, "").into_owned()
}

/// `<img>`/`<image>`（含 `svg:image`）个数；注释里的不算。
fn count_images(html: &str) -> usize {
    use crate::wash::opf::is_local;
    crate::html::tags(html).filter(|t| t.is_start() && (is_local(t.name, "img") || is_local(t.name, "image"))).count()
}

/// (spine 页里 `<img>`/`<image>` 总数, 可见文字总字数)。沿 OPF spine 遍历。
pub fn epub_image_stats(entries: &[Entry]) -> (usize, usize) {
    let Some(opf) = parse_opf(entries) else { return (0, 0) };
    // 条目名索引建一次（此前每个 spine 页 `entries.iter().find`，几千页漫画是"页数 × 条目数"次比较；同名取第一条）。
    let mut by_name: std::collections::HashMap<&str, &Entry> = std::collections::HashMap::with_capacity(entries.len());
    for e in entries {
        by_name.entry(e.name.as_str()).or_insert(e);
    }
    let mut images = 0usize;
    let mut text = 0usize;
    for p in &opf.spine {
        if crate::util::is_image_ext(p) {
            images += 1; // 少数畸形 EPUB 把图片文件直接列进 spine
            continue;
        }
        let Some(e) = by_name.get(p.as_str()) else { continue };
        let Ok(html) = std::str::from_utf8(&e.data) else { continue };
        images += count_images(html);
        let body = strip_noise_tags(html);
        text += crate::wash::plain_text(&body).chars().filter(|c| !c.is_whitespace()).count();
    }
    (images, text)
}

/// 判定：平均每张图配的文字 <[`TEXT_PER_IMAGE`] 字，而且图 ≥[`MIN_IMAGES`] 张——OPF 里已经标了 [`COMIC_SUBJECT`]
/// （CBZ 转出来的、优化过的漫画）时不看张数，薄薄一册也按漫画处理。
pub fn is_comic(entries: &[Entry]) -> bool {
    let (images, text) = epub_image_stats(entries);
    let enough = images >= MIN_IMAGES || (images > 0 && tagged_comic(entries));
    enough && (text as f64) < TEXT_PER_IMAGE * images as f64
}

/// OPF 里有没有 `<dc:subject>漫画</dc:subject>`。
fn tagged_comic(entries: &[Entry]) -> bool {
    parse_opf(entries).is_some_and(|o| has_comic_subject(&String::from_utf8_lossy(&entries[o.index].data)))
}

fn has_comic_subject(opf: &str) -> bool {
    use crate::html::{self, TagKind};
    html::tags(opf).filter(|t| t.kind == TagKind::Open && crate::wash::opf::is_local(t.name, "subject")).any(|t| {
        html::find_close(opf, t.end, t.name).is_some_and(|c| crate::util::xml_unescape(opf[t.end..c.start].trim()) == COMIC_SUBJECT)
    })
}

/// 漫画在 OPF 里打的标签（`<dc:subject>`）：书库、阅读器按标签分类时能认出漫画。
pub const COMIC_SUBJECT: &str = "漫画";

/// 给漫画的 OPF 加上 [`COMIC_SUBJECT`] 标签：已经有同名 `dc:subject` 的不动；插在 `</metadata>` 前面（`dc` 前缀，
/// EPUB 的 OPF 都声明了）。没有 `</metadata>` 的不动。返回 `None` 表示没改。
pub fn tag_opf_as_comic(opf: &str) -> Option<String> {
    if has_comic_subject(opf) {
        return None;
    }
    crate::wash::opf::insert_metadata(opf, &format!("<dc:subject>{COMIC_SUBJECT}</dc:subject>"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn e(name: &str, data: &str) -> Entry {
        Entry { name: name.into(), data: data.as_bytes().to_vec() }
    }

    fn opf(items: &str, spine: &str) -> Entry {
        e("content.opf", &format!(r#"<package version="3.0"><metadata><dc:title>书</dc:title></metadata><manifest>{items}</manifest><spine>{spine}</spine></package>"#))
    }

    #[test]
    fn text_book_with_scattered_illustrations_is_not_comic() {
        let mut v = vec![opf(
            r#"<item id="c1" href="c1.xhtml" media-type="application/xhtml+xml"/>"#,
            r#"<itemref idref="c1"/>"#,
        )];
        let long_text = "正".repeat(500);
        v.push(e("c1.xhtml", &format!("<html><body><p>{long_text}</p><img src=\"deco.png\"/></body></html>")));
        assert!(!is_comic(&v), "一张插图配几百字，不该判漫画");
    }

    #[test]
    fn image_dense_epub_with_almost_no_text_is_comic() {
        let items: String = (1..=25).map(|i| format!(r#"<item id="c{i}" href="c{i}.xhtml" media-type="application/xhtml+xml"/>"#)).collect();
        let spine: String = (1..=25).map(|i| format!(r#"<itemref idref="c{i}"/>"#)).collect();
        let mut v = vec![opf(&items, &spine)];
        for i in 1..=25 {
            v.push(e(&format!("c{i}.xhtml"), &format!(r#"<html><body><img src="p{i}.jpg"/></body></html>"#)));
        }
        assert!(is_comic(&v), "25 张纯图片页、几乎无字，应判漫画");
    }

    #[test]
    fn below_min_images_threshold_is_not_comic() {
        let items: String = (1..=10).map(|i| format!(r#"<item id="c{i}" href="c{i}.xhtml" media-type="application/xhtml+xml"/>"#)).collect();
        let spine: String = (1..=10).map(|i| format!(r#"<itemref idref="c{i}"/>"#)).collect();
        let mut v = vec![opf(&items, &spine)];
        for i in 1..=10 {
            v.push(e(&format!("c{i}.xhtml"), &format!(r#"<html><body><img src="p{i}.jpg"/></body></html>"#)));
        }
        assert!(!is_comic(&v), "只有 10 张图，没到 MIN_IMAGES 阈值");
    }

    #[test]
    fn tagged_comic_counts_even_below_threshold() {
        let items: String = (1..=10).map(|i| format!(r#"<item id="c{i}" href="c{i}.xhtml" media-type="application/xhtml+xml"/>"#)).collect();
        let spine: String = (1..=10).map(|i| format!(r#"<itemref idref="c{i}"/>"#)).collect();
        let tagged = format!(r#"<package version="3.0"><metadata><dc:subject>漫画</dc:subject></metadata><manifest>{items}</manifest><spine>{spine}</spine></package>"#);
        let mut v = vec![e("content.opf", &tagged)];
        for i in 1..=10 {
            v.push(e(&format!("c{i}.xhtml"), &format!(r#"<html><body><img src="p{i}.jpg"/></body></html>"#)));
        }
        assert!(is_comic(&v), "标了漫画（CBZ 转出来的）的，10 页也算");
        v[0] = e("content.opf", &tagged.replace("<dc:subject>漫画</dc:subject>", ""));
        assert!(!is_comic(&v), "没标的照旧要 20 张");
    }

    #[test]
    fn script_and_style_text_excluded_from_count() {
        let mut v = vec![opf(
            r#"<item id="c1" href="c1.xhtml" media-type="application/xhtml+xml"/>"#,
            r#"<itemref idref="c1"/>"#,
        )];
        v.push(e("c1.xhtml", r#"<html><head><style>body{color:red}</style><script>var x=1;</script></head><body><img src="p1.jpg"/></body></html>"#));
        let (images, text) = epub_image_stats(&v);
        assert_eq!(images, 1);
        assert_eq!(text, 0, "script/style 内容不该计入可见文字: got {text}");
    }

    #[test]
    fn tag_opf_as_comic_adds_subject_once() {
        let opf = r#"<package><metadata xmlns:dc="x"><dc:title>乱马</dc:title></metadata><manifest/></package>"#;
        let t = tag_opf_as_comic(opf).unwrap();
        assert!(t.contains("<dc:subject>漫画</dc:subject></metadata>"), "{t}");
        assert_eq!(tag_opf_as_comic(&t), None, "已有标签不重复加");
        assert_eq!(tag_opf_as_comic("<package><manifest/></package>"), None, "没有 metadata 不动");
    }
}
