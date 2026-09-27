//! EPUB 的 NCX 目录与页内图片引用解析（漫画 EPUB→PDF 用：书签来自 NCX，页面图片来自每页引用的 `<img>`）。

use crate::epubzip::{dir_of, posix_norm, resolve, Entry};
use regex::Regex;
use std::collections::HashMap;
use std::sync::OnceLock;

fn navpoint_event_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r#"(?s)(<navPoint\b)|(</navPoint>)|<navLabel>\s*<text>([^<]*)</text>\s*</navLabel>|<content\s+src="([^"]*)""#).unwrap()
    })
}

/// 线性扫 `toc.ncx`，展平成 `(depth, title, target)` 序列（不建真正的树——跟本 crate 一贯的
/// "扁平+depth"写法一致，如 `wash.rs::dense_ranks`）。NCX 规范保证 `<navLabel>` 和 `<content>`
/// 总是先于自己的子 `<navPoint>` 出现，扫描时按"刚看到 content 就用当前 depth/title 落地一条"
/// 处理即可，不用等子节点扫完。
fn parse_ncx_flat(ncx_text: &str) -> Vec<(usize, String, String)> {
    let mut depth = 0usize;
    let mut cur_title = String::new();
    let mut out = Vec::new();
    for c in navpoint_event_re().captures_iter(ncx_text) {
        if c.get(1).is_some() {
            depth += 1;
        } else if c.get(2).is_some() {
            depth = depth.saturating_sub(1);
        } else if let Some(t) = c.get(3) {
            // NCX 里是转义过的 XML 文本；标题之后会当书签/目录项再转义一次，先还原字符引用，
            // 否则 `卷一 &amp; 卷二` 会以字面 `&amp;` 出现在书签里。
            cur_title = crate::util::xml_unescape(t.as_str().trim()).into_owned();
        } else if let Some(s) = c.get(4) {
            out.push((depth, std::mem::take(&mut cur_title), s.as_str().to_string()));
        }
    }
    out
}

/// 一页 (x)html 里引用的图片，解析成 zip 内绝对路径（相对该页自身目录解析，去重按出现顺序）。
pub(crate) fn imgs_referenced(html: &str, page_dir: &str) -> Vec<String> {
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| Regex::new(r#"(?i)<img\b[^>]*\bsrc="([^"]+)""#).unwrap());
    let mut seen = std::collections::HashSet::new();
    re.captures_iter(html)
        // src 按 URL 百分号编码解码（中文/空格文件名常写成 `%E5%9B%BE.jpg`），否则对不上 zip 条目名，这张图取不到、整页被丢。
        .map(|c| posix_norm(&resolve(page_dir, &crate::epubzip::percent_decode(&c[1]))))
        .filter(|p| seen.insert(p.clone()))
        .collect()
}

/// NCX 展平并解析成 `(depth, 标题, spine 下标)` 的完整列表，不按深度过滤。没有 `toc.ncx`、entry 缺失、
/// 或全部目标都对不上 spine（拿不到任何有效节点）时返回 `None`。
fn ncx_resolved(entries: &[Entry], opf: &crate::wash::Opf) -> Option<Vec<(usize, String, usize)>> {
    let ncx_path = opf.ncx.as_ref()?;
    let ncx_entry = entries.iter().find(|e| &e.name == ncx_path)?;
    let ncx_text = String::from_utf8_lossy(&ncx_entry.data);
    let ncx_dir = dir_of(ncx_path);
    let flat = parse_ncx_flat(&ncx_text);
    let resolved: Vec<(usize, String, usize)> = flat
        .into_iter()
        .filter_map(|(depth, title, target)| {
            let no_frag = target.split('#').next().unwrap_or(&target);
            let abs = posix_norm(&resolve(ncx_dir, no_frag));
            opf.spine.iter().position(|p| p == &abs).map(|idx| (depth, title, idx))
        })
        .collect();
    if resolved.is_empty() {
        None
    } else {
        Some(resolved)
    }
}

/// `[start,end)` 范围内，spine 绝对下标 → NCX 标题的映射，不拘深度（"卷"下面的"话"级子目录也照收）；
/// 同一页多条取先出现的，空标题不收。
pub(crate) fn ncx_titles_in_range(entries: &[Entry], opf: &crate::wash::Opf, start: usize, end: usize) -> HashMap<usize, String> {
    let mut map = HashMap::new();
    let Some(resolved) = ncx_resolved(entries, opf) else { return map };
    for (_, title, idx) in resolved {
        if idx >= start && idx < end && !title.trim().is_empty() {
            map.entry(idx).or_insert(title);
        }
    }
    map
}

/// 没有可用目录结构时的退路：贪心按页累加体积，快超预算（加上这一页会超）就切一刀开新的一份；

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percent_encoded_img_src_resolves_to_zip_entry() {
        assert_eq!(imgs_referenced(r#"<img src="../images/%E5%9B%BE%201.jpg"/>"#, "text"), ["images/图 1.jpg"]);
        assert_eq!(imgs_referenced(r#"<img src="a.jpg"/><img src="a.jpg"/>"#, ""), ["a.jpg"], "同页重复引用去重");
    }

    #[test]
    fn ncx_flat_depth_and_titles_decoded_once() {
        let ncx = r#"<navMap><navPoint><navLabel><text>猫 &amp; 鼠</text></navLabel><content src="t/a.html"/>
<navPoint><navLabel><text>卷&#20108;</text></navLabel><content src="t/b.html#x"/></navPoint></navPoint></navMap>"#;
        assert_eq!(parse_ncx_flat(ncx), [(1, "猫 & 鼠".to_string(), "t/a.html".to_string()), (2, "卷二".to_string(), "t/b.html#x".to_string())]);
    }
}
