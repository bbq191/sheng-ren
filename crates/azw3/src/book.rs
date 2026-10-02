//! 读 EPUB：元数据、spine 里的 XHTML、CSS、图片、封面、目录。

use bookconv::epubzip::{dir_of, percent_decode, posix_norm, read_entries, resolve};
use bookconv::convert::common::{image_ext_mime, is_webp};
use bookconv::html;
use bookconv::util::{fnv64, xml_unescape};
use bookconv::wash::opf::is_local;
use bookconv::wash::normalize::nav_toc_items;
use bookconv::wash::{manifest_items, opf_dc, parse_opf};
use std::collections::HashMap;

pub struct Doc {
    pub path: String,
    pub html: String,
}

/// 目录项：`level` 从 0 起。
pub struct TocItem {
    pub label: String,
    pub level: u32,
    pub path: String,
    pub frag: String,
}

#[derive(Default)]
pub struct Meta {
    pub title: String,
    pub authors: Vec<String>,
    pub publisher: String,
    pub language: String,
    pub date: String,
    pub description: String,
    /// spine `page-progression-direction="rtl"`（日漫）。
    pub rtl: bool,
    /// 这本书的确定性指纹：OPF `unique-identifier` 指向的标识符的 FNV-1a 64 位哈希，没有标识符时取整个 OPF 的哈希。
    /// 调用方没给固定 ID 时用它派生唯一 ID（同一本书每次转出来一样）。
    pub stable_id: u64,
    /// `dcterms:modified`（`CCYY-MM-DDThh:mm:ssZ`）换算成的 Unix 秒；没有或格式不对为 `None`。
    pub modified: Option<u32>,
    /// 固定版式的声明（EXTH 编号, 值）：OPF 里 KindleGen 约定的 `<meta name="fixed-layout" content="true"/>` 等几项，
    /// 原样写成同名的 EXTH 记录（编号见 MobileRead Wiki 的 MOBI 页）。没有 `fixed-layout = true` 时为空（流式排版）。
    pub fixed_layout: Vec<(u32, String)>,
}

/// OPF `<meta name=…>` 的名字 → EXTH 编号（固定版式那一组，MobileRead Wiki「MOBI」页的 EXTH 表）。
const FIXED_LAYOUT_EXTH: [(&str, u32); 6] = [
    ("fixed-layout", 122),
    ("book-type", 123),
    ("orientation-lock", 124),
    ("original-resolution", 126),
    ("zero-gutter", 127),
    ("zero-margin", 128),
];

/// OPF 里固定版式那一组 `<meta name content>`；`fixed-layout` 不是 `true` 时一律不要（流式排版的书写了别的几项也没用）。
fn fixed_layout_metas(opf: &str) -> Vec<(u32, String)> {
    let mut out = Vec::new();
    for t in bookconv::html::tags(opf).filter(|t| t.is_start() && bookconv::wash::opf::is_local(t.name, "meta")) {
        let tag = &opf[t.start..t.end];
        let (Some(name), Some(content)) = (bookconv::html::attr_value(tag, "name"), bookconv::html::attr_value(tag, "content")) else { continue };
        if let Some((_, n)) = FIXED_LAYOUT_EXTH.iter().find(|(k, _)| *k == name) {
            if !out.iter().any(|(m, _)| m == n) {
                out.push((*n, content.trim().to_string()));
            }
        }
    }
    if !out.iter().any(|(n, v)| *n == 122 && v == "true") {
        return Vec::new();
    }
    out.sort_by_key(|(n, _)| *n);
    out
}

/// 第一个 `<spine>`（认带前缀的 `<opf:spine>`）的 `page-progression-direction` 是不是 `rtl`。
fn spine_rtl(opf: &str) -> bool {
    html::tags(opf)
        .find(|t| t.is_start() && is_local(t.name, "spine"))
        .is_some_and(|t| html::attr_value(&opf[t.start..t.end], "page-progression-direction") == Some("rtl"))
}

/// OPF `<package unique-identifier="X">` 指向的 `<dc:identifier id="X">` 的文本（字符引用已还原、去掉首尾空白）。
fn unique_identifier(opf: &str) -> Option<String> {
    let pkg = html::tags(opf).find(|t| t.is_start() && is_local(t.name, "package"))?;
    let want = html::attr_value(&opf[pkg.start..pkg.end], "unique-identifier")?;
    let t = html::tags(opf).find(|t| t.kind == html::TagKind::Open && is_local(t.name, "identifier") && html::attr_value(&opf[t.start..t.end], "id") == Some(want))?;
    let close = html::find_close(opf, t.end, t.name)?;
    let v = xml_unescape(opf[t.end..close.start].trim()).into_owned();
    (!v.is_empty()).then_some(v)
}

/// 第一个 `<meta property="dcterms:modified">` 的值换算成 Unix 秒（只认 `CCYY-MM-DDThh:mm:ssZ`，1970–2105 年）。
fn modified_secs(opf: &str) -> Option<u32> {
    let t = html::tags(opf).find(|t| t.kind == html::TagKind::Open && is_local(t.name, "meta") && html::attr_value(&opf[t.start..t.end], "property") == Some("dcterms:modified"))?;
    let close = html::find_close(opf, t.end, t.name)?;
    let v = opf[t.end..close.start].trim().as_bytes();
    if v.len() != 20 || v[4] != b'-' || v[7] != b'-' || v[10] != b'T' || v[13] != b':' || v[16] != b':' || v[19] != b'Z' {
        return None;
    }
    let num = |a: usize, b: usize| -> Option<i64> { std::str::from_utf8(&v[a..b]).ok()?.parse::<i64>().ok() };
    let (y, mo, d, h, mi, s) = (num(0, 4)?, num(5, 7)?, num(8, 10)?, num(11, 13)?, num(14, 16)?, num(17, 19)?);
    if !(1..=12).contains(&mo) || !(1..=31).contains(&d) || h > 23 || mi > 59 || s > 60 {
        return None;
    }
    // 公历日期 → 1970-01-01 起的天数（按 3 月起算的年，闰日落在年末）
    let (yy, mm) = if mo <= 2 { (y - 1, mo + 9) } else { (y, mo - 3) };
    let era = yy.div_euclid(400);
    let yoe = yy - era * 400;
    let doy = (153 * mm + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146097 + doe - 719468;
    u32::try_from(days * 86400 + h * 3600 + mi * 60 + s).ok()
}

pub struct Image {
    pub path: String,
    pub bytes: Vec<u8>,
    pub mime: &'static str,
}

pub struct Loaded {
    pub meta: Meta,
    pub docs: Vec<Doc>,
    pub css: Vec<(String, String)>,
    pub images: Vec<Image>,
    pub cover: Option<String>,
    pub toc: Vec<TocItem>,
}

/// EPUB3 nav 文档里的目录（`epub:type` 含 `toc` 的那个 `<nav>`）：和优化器补 NCX 用同一套解析（[`nav_toc_items`]，
/// `<ol>` 嵌套深度即层级，从 1 起），这里换成从 0 起、锚点百分号解码。
fn nav_toc(html: &str, nav_path: &str) -> Vec<TocItem> {
    nav_toc_items(html, nav_path)
        .into_iter()
        .map(|t| TocItem { label: t.title, level: u32::from(t.level.saturating_sub(1)), path: t.path, frag: percent_decode(&t.frag) })
        .collect()
}

/// 静态 WebP → PNG（KF8 不认 WebP）：解码后无损编码，像素不变（有损 WebP 的像素就是它解出来的样子）。动画 WebP 只取一帧会丢内容，
/// 返回 `None`（当作不支持的图片，给出警告）。
fn webp_to_png(b: &[u8]) -> Option<Vec<u8>> {
    let dec = image::codecs::webp::WebPDecoder::new(std::io::Cursor::new(b)).ok()?;
    if dec.has_animation() {
        return None;
    }
    let img = image::DynamicImage::from_decoder(dec).ok()?;
    let mut out = Vec::new();
    img.write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png).ok()?;
    Some(out)
}

/// 读 EPUB。`warnings` 收集放不进 AZW3 的内容（不认识的图片格式）。
pub fn load(epub: &[u8], warnings: &mut Vec<String>) -> Result<Loaded, String> {
    let mut entries = read_entries(epub)?;
    let opf = parse_opf(&entries).ok_or("找不到 OPF")?;
    let opf_text = String::from_utf8_lossy(&entries[opf.index].data).into_owned();
    let index: HashMap<String, usize> = entries.iter().enumerate().map(|(i, e)| (e.name.clone(), i)).collect();

    let dc = opf_dc(&opf_text);
    let stable_id = unique_identifier(&opf_text).map_or_else(|| fnv64(&entries[opf.index].data), |id| fnv64(id.as_bytes()));
    let modified = modified_secs(&opf_text);
    let mut meta = Meta { title: dc.title, authors: dc.creators, publisher: dc.publisher, language: dc.language, date: dc.date, description: dc.description, rtl: false, stable_id, modified, fixed_layout: fixed_layout_metas(&opf_text) };
    meta.rtl = spine_rtl(&opf_text);

    let mut media: HashMap<String, String> = HashMap::new();
    let mut cover: Option<String> = None;
    let mut css = Vec::new();
    let mut images = Vec::new();
    let mut unsupported = Vec::new();
    for it in manifest_items(&opf_text) {
        let path = posix_norm(&resolve(&opf.dir, &percent_decode(it.href)));
        media.insert(path.clone(), it.media_type.to_string());
        if it.properties.split_whitespace().any(|p| p == "cover-image") {
            cover = Some(path.clone());
        }
        let Some(&i) = index.get(&path) else { continue };
        if it.media_type == "text/css" {
            css.push((path, String::from_utf8_lossy(&entries[i].data).into_owned()));
        } else if let Some((_, mime)) = image_ext_mime(&entries[i].data) {
            // 图片字节直接移走，不复制（大漫画省一份内存）
            images.push(Image { path, bytes: std::mem::take(&mut entries[i].data), mime });
        } else if let Some(png) = is_webp(&entries[i].data).then(|| webp_to_png(&entries[i].data)).flatten() {
            images.push(Image { path, bytes: png, mime: "image/png" });
        } else if it.media_type.starts_with("image/") {
            unsupported.push(path);
        }
    }
    if !unsupported.is_empty() {
        warnings.push(format!("{} 张图片格式不支持（只支持 JPEG/PNG/GIF 和静态 WebP），在 Kindle 上不显示：{}", unsupported.len(), unsupported[0]));
    }
    if cover.is_none() {
        if let Some(&(_, _, id)) = bookconv::wash::opf::cover_meta_tags(&opf_text).first() {
            cover = opf.items.get(id).cloned();
        }
    }
    cover = cover.filter(|c| images.iter().any(|i| &i.path == c));

    let mut docs = Vec::new();
    for p in &opf.spine {
        let is_html = media.get(p).is_some_and(|m| m.contains("html"));
        if let (true, Some(&i)) = (is_html, index.get(p)) {
            docs.push(Doc { path: p.clone(), html: String::from_utf8_lossy(&entries[i].data).into_owned() });
        }
    }
    if docs.is_empty() {
        return Err("spine 里没有 XHTML 文档".into());
    }

    let get = |p: &str| index.get(p).map(|&i| &entries[i]);
    let mut toc = Vec::new();
    if let Some(ncx) = opf.ncx.as_ref().and_then(|p| get(p).map(|e| (p, e))) {
        for (depth, label, target) in bookconv::ncx::parse_ncx_flat(&String::from_utf8_lossy(&ncx.1.data)) {
            let (p, f) = target.split_once('#').unwrap_or((target.as_str(), ""));
            toc.push(TocItem { label, level: depth.saturating_sub(1) as u32, path: posix_norm(&resolve(dir_of(ncx.0), &percent_decode(p))), frag: percent_decode(f) });
        }
    }
    if toc.is_empty() {
        if let Some(nav) = opf.nav_doc.as_ref().and_then(|p| get(p).map(|e| (p, e))) {
            toc = nav_toc(&String::from_utf8_lossy(&nav.1.data), nav.0);
        }
    }
    let doc_paths: std::collections::HashSet<&str> = docs.iter().map(|d| d.path.as_str()).collect();
    toc.retain(|t| !t.label.trim().is_empty() && doc_paths.contains(t.path.as_str()));
    clamp_levels(&mut toc);
    Ok(Loaded { meta, docs, css, images, cover, toc })
}

/// 去掉目录项后层级可能断档（0 → 2）：每一项最多比前一项深一级。KF8 目录索引的父子区间依赖这一点。
fn clamp_levels(toc: &mut [TocItem]) {
    let mut prev: Option<u32> = None;
    for t in toc {
        t.level = match prev {
            None => 0,
            Some(p) => t.level.min(p + 1),
        };
        prev = Some(t.level);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nav_toc_picks_toc_nav_and_reads_any_quote() {
        let html = r#"<body><nav epub:type="landmarks"><ol><li><a href="toc.xhtml">Table of Contents</a></li></ol></nav>
<nav epub:type='toc' id="toc"><ol><li><a class='x' href='Text/c1.xhtml#s%201'>第一章</a><ol><li><a href="Text/c2.xhtml?a=1&amp;b=2">第二节</a></li></ol></li></ol></nav></body>"#;
        let toc = nav_toc(html, "OEBPS/nav.xhtml");
        let got: Vec<_> = toc.iter().map(|t| (t.label.as_str(), t.level, t.path.as_str(), t.frag.as_str())).collect();
        assert_eq!(got, [("第一章", 0, "OEBPS/Text/c1.xhtml", "s 1"), ("第二节", 1, "OEBPS/Text/c2.xhtml?a=1&b=2", "")]);
    }

    #[test]
    fn stable_id_from_unique_identifier_and_modified_time() {
        let opf = r#"<package unique-identifier="bid"><metadata><dc:identifier id="other">x</dc:identifier><dc:identifier id='bid'> urn:uuid:1&amp;2 </dc:identifier>
<meta property="dcterms:modified">2024-02-29T23:59:59Z</meta></metadata></package>"#;
        assert_eq!(unique_identifier(opf).as_deref(), Some("urn:uuid:1&2"));
        assert_eq!(modified_secs(opf), Some(1_709_251_199));
        assert_eq!(modified_secs(r#"<meta property="dcterms:modified">2000-01-01T00:00:00Z</meta>"#), Some(946_684_800));
        assert_eq!(modified_secs(r#"<meta property="dcterms:modified">2000-13-01T00:00:00Z</meta>"#), None);
        assert_eq!(unique_identifier(r#"<package><dc:identifier id="bid">x</dc:identifier></package>"#), None);
    }

    #[test]
    fn spine_rtl_accepts_prefixed_opf_and_any_quote() {
        assert!(spine_rtl(r#"<package><spine toc="ncx" page-progression-direction="rtl"></spine></package>"#));
        assert!(spine_rtl(r#"<opf:package xmlns:opf="http://www.idpf.org/2007/opf"><opf:spine page-progression-direction='rtl'><opf:itemref idref="a"/></opf:spine></opf:package>"#));
        assert!(!spine_rtl(r#"<opf:spine page-progression-direction="ltr"/>"#));
        assert!(!spine_rtl(r#"<spinex page-progression-direction="rtl"/><spine/>"#));
        assert!(!spine_rtl(r#"<!-- <spine page-progression-direction="rtl"> --><spine/>"#));
    }

    #[test]
    fn toc_levels_never_skip() {
        let item = |level| TocItem { label: "x".into(), level, path: String::new(), frag: String::new() };
        let mut toc = vec![item(1), item(3), item(1), item(0), item(2)];
        clamp_levels(&mut toc);
        assert_eq!(toc.iter().map(|t| t.level).collect::<Vec<_>>(), [0, 1, 1, 0, 1]);
    }
}
