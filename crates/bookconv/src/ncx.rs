//! EPUB 的 NCX 目录解析。

use regex::Regex;
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
pub fn parse_ncx_flat(ncx_text: &str) -> Vec<(usize, String, String)> {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ncx_flat_depth_and_titles_decoded_once() {
        let ncx = r#"<navMap><navPoint><navLabel><text>猫 &amp; 鼠</text></navLabel><content src="t/a.html"/>
<navPoint><navLabel><text>卷&#20108;</text></navLabel><content src="t/b.html#x"/></navPoint></navPoint></navMap>"#;
        assert_eq!(parse_ncx_flat(ncx), [(1, "猫 & 鼠".to_string(), "t/a.html".to_string()), (2, "卷二".to_string(), "t/b.html#x".to_string())]);
    }
}
