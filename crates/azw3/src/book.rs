//! 读 EPUB：元数据、spine 里的 XHTML、CSS、图片、封面、目录。

use bookconv::epubzip::{dir_of, percent_decode, posix_norm, read_entries, resolve};
use bookconv::wash::{manifest_items, parse_opf, tag_attr};
use regex::Regex;
use std::collections::HashMap;
use std::sync::OnceLock;

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

fn text_of(re: &Regex, opf: &str) -> Vec<String> {
    static TAG: OnceLock<Regex> = OnceLock::new();
    let tag = TAG.get_or_init(|| Regex::new(r#"(?s)<[^>]*>"#).unwrap());
    re.captures_iter(opf).map(|c| bookconv::util::xml_unescape(tag.replace_all(&c[1], "").trim()).into_owned()).filter(|s| !s.is_empty()).collect()
}

fn dc(name: &str) -> Regex {
    Regex::new(&format!(r#"(?s)<dc:{name}\b[^>]*>(.*?)</dc:{name}>"#)).unwrap()
}

fn image_mime(bytes: &[u8]) -> Option<&'static str> {
    match bytes {
        [0xFF, 0xD8, ..] => Some("image/jpeg"),
        [0x89, b'P', b'N', b'G', ..] => Some("image/png"),
        [b'G', b'I', b'F', b'8', ..] => Some("image/gif"),
        _ => None,
    }
}

/// EPUB3 nav 文档里的目录：`<ol>` 嵌套深度即层级。
fn nav_toc(html: &str, nav_path: &str) -> Vec<TocItem> {
    static TOK: OnceLock<Regex> = OnceLock::new();
    static NAV: OnceLock<Regex> = OnceLock::new();
    let nav = NAV.get_or_init(|| Regex::new(r#"(?is)<nav\b[^>]*toc[^>]*>(.*?)</nav>"#).unwrap());
    let Some(body) = nav.captures(html).map(|c| c.get(1).unwrap().as_str()) else { return Vec::new() };
    let tok = TOK.get_or_init(|| Regex::new(r#"(?is)<ol\b[^>]*>|</ol>|<a\b[^>]*\bhref="([^"]*)"[^>]*>(.*?)</a>"#).unwrap());
    let mut depth = 0u32;
    let mut out = Vec::new();
    for c in tok.captures_iter(body) {
        let t = c.get(0).unwrap().as_str();
        if t.len() >= 3 && t[..3].eq_ignore_ascii_case("<ol") {
            depth += 1;
        } else if t.eq_ignore_ascii_case("</ol>") {
            depth = depth.saturating_sub(1);
        } else if let (Some(h), Some(label)) = (c.get(1), c.get(2)) {
            let label = bookconv::wash::plain_text(label.as_str());
            let (p, f) = h.as_str().split_once('#').unwrap_or((h.as_str(), ""));
            out.push(TocItem { label, level: depth.saturating_sub(1), path: posix_norm(&resolve(dir_of(nav_path), &percent_decode(p))), frag: f.to_string() });
        }
    }
    out
}

pub fn load(epub: &[u8]) -> Result<Loaded, String> {
    let entries = read_entries(epub)?;
    let opf = parse_opf(&entries).ok_or("找不到 OPF")?;
    let opf_text = String::from_utf8_lossy(&entries[opf.index].data).into_owned();
    let get = |p: &str| entries.iter().find(|e| e.name == p);

    let mut meta = Meta {
        title: text_of(&dc("title"), &opf_text).into_iter().next().unwrap_or_default(),
        authors: text_of(&dc("creator"), &opf_text),
        publisher: text_of(&dc("publisher"), &opf_text).into_iter().next().unwrap_or_default(),
        language: text_of(&dc("language"), &opf_text).into_iter().next().unwrap_or_default(),
        date: text_of(&dc("date"), &opf_text).into_iter().next().unwrap_or_default(),
        description: text_of(&dc("description"), &opf_text).into_iter().next().unwrap_or_default(),
        rtl: false,
    };
    static SPINE: OnceLock<Regex> = OnceLock::new();
    if let Some(m) = SPINE.get_or_init(|| Regex::new(r#"<spine\b[^>]*>"#).unwrap()).find(&opf_text) {
        meta.rtl = tag_attr(m.as_str(), "page-progression-direction") == Some("rtl");
    }

    let mut media: HashMap<String, String> = HashMap::new();
    let mut cover: Option<String> = None;
    let mut css = Vec::new();
    let mut images = Vec::new();
    for it in manifest_items(&opf_text) {
        let path = posix_norm(&resolve(&opf.dir, &percent_decode(it.href)));
        media.insert(path.clone(), it.media_type.to_string());
        if it.properties.split_whitespace().any(|p| p == "cover-image") {
            cover = Some(path.clone());
        }
        let Some(e) = get(&path) else { continue };
        if it.media_type == "text/css" {
            css.push((path, String::from_utf8_lossy(&e.data).into_owned()));
        } else if let Some(mime) = image_mime(&e.data) {
            images.push(Image { path, bytes: e.data.clone(), mime });
        }
    }
    static COVER_META: OnceLock<Regex> = OnceLock::new();
    if cover.is_none() {
        if let Some(m) = COVER_META.get_or_init(|| Regex::new(r#"(?s)<meta\b[^>]*\bname="cover"[^>]*>"#).unwrap()).find(&opf_text) {
            cover = tag_attr(m.as_str(), "content").and_then(|id| opf.items.get(id)).cloned();
        }
    }
    cover = cover.filter(|c| images.iter().any(|i| &i.path == c));

    let mut docs = Vec::new();
    for p in &opf.spine {
        let is_html = media.get(p).is_some_and(|m| m.contains("html"));
        if !is_html {
            continue;
        }
        if let Some(e) = get(p) {
            docs.push(Doc { path: p.clone(), html: String::from_utf8_lossy(&e.data).into_owned() });
        }
    }
    if docs.is_empty() {
        return Err("spine 里没有 XHTML 文档".into());
    }

    let mut toc = Vec::new();
    if let Some(ncx) = opf.ncx.as_ref().and_then(|p| get(p).map(|e| (p, e))) {
        for (depth, label, target) in bookconv::ncx::parse_ncx_flat(&String::from_utf8_lossy(&ncx.1.data)) {
            let (p, f) = target.split_once('#').unwrap_or((target.as_str(), ""));
            toc.push(TocItem { label, level: depth.saturating_sub(1) as u32, path: posix_norm(&resolve(dir_of(ncx.0), &percent_decode(p))), frag: f.to_string() });
        }
    }
    if toc.is_empty() {
        if let Some(nav) = opf.nav_doc.as_ref().and_then(|p| get(p).map(|e| (p, e))) {
            toc = nav_toc(&String::from_utf8_lossy(&nav.1.data), nav.0);
        }
    }
    toc.retain(|t| !t.label.trim().is_empty() && docs.iter().any(|d| d.path == t.path));
    Ok(Loaded { meta, docs, css, images, cover, toc })
}
