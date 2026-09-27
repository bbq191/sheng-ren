//! EPUB 漫画识别：给 `optimize_epub_with` 用，决定该书走"漫画路"（画质/裁边保留原图）还是"文字书路"
//! （常规降采样）。判定逻辑当初是从 host 侧 `shelf_cli/comic.py::epub_image_stats()` 移植过来的
//! （2026-09-14 定案的阈值：图 ≥20 张且平均每张图配的文字 <40 字），host 那份原来继续给 `shelf push`
//! 的路由分流（转 CBZ）用——**2026-09-18 host 整条线（含这份 Python 原版）已砍**，不再使用 PC 端，
//! 现在这份 Rust 实现是唯一在用的版本，不用再顾虑"改一边忘改另一边"。

use crate::epubzip::Entry;
use crate::wash::parse_opf;
use regex::Regex;
use std::sync::OnceLock;

/// 判定漫画的最小图片数（跟 comic.py 的 `MIN_PAGES` 同值——历史命名按"页"，这里语义是"图"）。
pub const MIN_IMAGES: usize = 20;
/// 判定漫画的"平均每张图配的文字数"上限。
pub const TEXT_PER_IMAGE: f64 = 40.0;

pub(crate) fn strip_noise_tags(html: &str) -> String {
    static RE: OnceLock<Regex> = OnceLock::new();
    // regex crate 不支持反向引用，三种标签各写一条 alternation。
    RE.get_or_init(|| Regex::new(r#"(?is)<script\b.*?</script>|<style\b.*?</style>|<head\b.*?</head>"#).unwrap()).replace_all(html, "").into_owned()
}

fn count_images(html: &str) -> usize {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r#"(?i)<(?:img|image)\b"#).unwrap()).find_iter(html).count()
}

/// (spine 页里 `<img>`/`<image>` 总数, 可见文字总字数)。沿 OPF spine 遍历——跟 comic.py 同一套算法。
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
        let low = p.to_ascii_lowercase();
        if low.ends_with(".jpg") || low.ends_with(".jpeg") || low.ends_with(".png") || low.ends_with(".gif") || low.ends_with(".webp") {
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

/// 判定：图 ≥[`MIN_IMAGES`] 张且平均每张图配的文字 <[`TEXT_PER_IMAGE`] 字。
pub fn is_comic(entries: &[Entry]) -> bool {
    let (images, text) = epub_image_stats(entries);
    images >= MIN_IMAGES && (text as f64) < TEXT_PER_IMAGE * images as f64
}

/// **能转 PDF 的漫画**：漫画且整本可见文字为 0。转 PDF 是"一图一页"，纯文字页（版权页、前情提要、
/// 章节标题页）和图片页里夹的文字（台词、旁白）**都没有对应物、会被丢掉**——用户明确要求"不允许变动
/// 书籍内容"，所以只要有一个可见字就不转，留在 EPUB 流程里（文字和目录原样保留）。2026-09-20 审计 33 卷
/// 只有《镖人(卷二)》简体版命中（14 个文字页 477 字 + 图片页内 26 字，12 条目录项指向文字页），其余
/// 32 卷文字量为 0。
pub fn is_text_free_comic(entries: &[Entry]) -> bool {
    let (images, text) = epub_image_stats(entries);
    images >= MIN_IMAGES && text == 0
}

/// 只读 html/opf 真实字节判断是不是漫画，图片条目留空占位（`is_comic`/`epub_image_stats` 从不读
/// 图片字节，只数 html 里 `<img>` 标签出现次数），不解码任何图片——给 `book-serve::Staging::
/// optimize()` 在决定"这本 EPUB 优化后走 PDF 还是 EPUB"之前用的轻量预判。打不开/解不了 zip 一律
/// 当"不是漫画"（安全默认——判不准就走现状 EPUB 老路径，不是新引入的失败模式）。
pub fn is_comic_epub_file(path: &std::path::Path) -> bool {
    read_entries_without_images(path).map(|e| is_comic(&e)).unwrap_or(false)
}

/// [`is_text_free_comic`] 的文件版：给 `book-serve::Staging::optimize()` 决定"走 PDF 还是走 EPUB 优化"。
/// 打不开/解不了 zip 一律 `false`（走现状 EPUB 路径，保内容优先）。
pub fn is_text_free_comic_epub_file(path: &std::path::Path) -> bool {
    read_entries_without_images(path).map(|e| is_text_free_comic(&e)).unwrap_or(false)
}

fn read_entries_without_images(path: &std::path::Path) -> Option<Vec<Entry>> {
    let file = std::fs::File::open(path).ok()?;
    let mut zip = zip::ZipArchive::new(std::io::BufReader::new(file)).ok()?;
    // 读不动的条目当整本"识别不了"（None，调用方按非漫画走）；此前是悄悄跳过坏条目继续判——坏 zip 后续优化本来就会报错。
    let entries = crate::epubzip::read_skeleton(&mut zip).ok()?.entries;
    Some(entries)
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
    fn is_comic_epub_file_reads_from_disk_without_decoding_images() {
        use std::io::Write;
        let items: String = (1..=25).map(|i| format!(r#"<item id="c{i}" href="c{i}.xhtml" media-type="application/xhtml+xml"/>"#)).collect();
        let spine: String = (1..=25).map(|i| format!(r#"<itemref idref="c{i}"/>"#)).collect();
        let opf_xml = format!(r#"<package version="3.0"><metadata><dc:title>书</dc:title></metadata><manifest>{items}</manifest><spine>{spine}</spine></package>"#);

        let mut buf = Vec::new();
        {
            let cursor = std::io::Cursor::new(&mut buf);
            let mut z = zip::ZipWriter::new(cursor);
            let opt = zip::write::SimpleFileOptions::default();
            z.start_file("content.opf", opt).unwrap();
            z.write_all(opf_xml.as_bytes()).unwrap();
            for i in 1..=25 {
                z.start_file(format!("c{i}.xhtml"), opt).unwrap();
                z.write_all(format!(r#"<html><body><img src="p{i}.jpg"/></body></html>"#).as_bytes()).unwrap();
                // 图片条目故意写非法/空字节——is_comic_epub_file 不该尝试解码它，判定应该照常通过。
                z.start_file(format!("p{i}.jpg"), opt).unwrap();
                z.write_all(b"not a real jpeg").unwrap();
            }
            z.finish().unwrap();
        }
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.epub");
        std::fs::write(&path, &buf).unwrap();
        assert!(is_comic_epub_file(&path), "25 张纯图片页应判定为漫画");

        let not_epub = dir.path().join("not.epub");
        std::fs::write(&not_epub, b"garbage").unwrap();
        assert!(!is_comic_epub_file(&not_epub), "解不了 zip 的文件应安全返回 false");
    }
}
