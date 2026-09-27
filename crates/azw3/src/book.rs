//! 读 EPUB：元数据、spine 里的 XHTML、CSS、图片、封面、目录。

use bookconv::epubzip::{dir_of, percent_decode, posix_norm, read_entries, resolve};
use bookconv::convert::common::image_ext_mime;
use bookconv::wash::{cover_meta_re, manifest_items, opf_dc, parse_opf, tag_attr};
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
            out.push(TocItem { label, level: depth.saturating_sub(1), path: posix_norm(&resolve(dir_of(nav_path), &percent_decode(p))), frag: percent_decode(f) });
        }
    }
    out
}

/// 读 EPUB。`warnings` 收集放不进 AZW3 的内容（不认识的图片格式）。
pub fn load(epub: &[u8], warnings: &mut Vec<String>) -> Result<Loaded, String> {
    let mut entries = read_entries(epub)?;
    let opf = parse_opf(&entries).ok_or("找不到 OPF")?;
    let opf_text = String::from_utf8_lossy(&entries[opf.index].data).into_owned();
    let index: HashMap<String, usize> = entries.iter().enumerate().map(|(i, e)| (e.name.clone(), i)).collect();

    let dc = opf_dc(&opf_text);
    let mut meta = Meta { title: dc.title, authors: dc.creators, publisher: dc.publisher, language: dc.language, date: dc.date, description: dc.description, rtl: false };
    static SPINE: OnceLock<Regex> = OnceLock::new();
    if let Some(m) = SPINE.get_or_init(|| Regex::new(r#"<spine\b[^>]*>"#).unwrap()).find(&opf_text) {
        meta.rtl = tag_attr(m.as_str(), "page-progression-direction") == Some("rtl");
    }

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
        } else if it.media_type.starts_with("image/") {
            unsupported.push(path);
        }
    }
    if !unsupported.is_empty() {
        warnings.push(format!("{} 张图片格式不支持（只支持 JPEG/PNG/GIF），在 Kindle 上不显示：{}", unsupported.len(), unsupported[0]));
    }
    if cover.is_none() {
        if let Some(m) = cover_meta_re().find(&opf_text) {
            cover = tag_attr(m.as_str(), "content").and_then(|id| opf.items.get(id)).cloned();
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
    fn toc_levels_never_skip() {
        let item = |level| TocItem { label: "x".into(), level, path: String::new(), frag: String::new() };
        let mut toc = vec![item(1), item(3), item(1), item(0), item(2)];
        clamp_levels(&mut toc);
        assert_eq!(toc.iter().map(|t| t.level).collect::<Vec<_>>(), [0, 1, 1, 0, 1]);
    }
}
