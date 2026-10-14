//! Move 上字号对不上关键字的标题改成 `<div>`（2026-10-14 用户定）。
//!
//! xochitl 的 h1–h6 只认几个字号关键字（`wash/block_style.rs`），Kindle 上的字号和最近的关键字差 3% 以上的（《人生海海》小节标题
//! 1.75em，最近的 xx-large 是 2.018）照 CSS 写不出来；`<div>` 的字号、外边距 Move 都认。`block_style` 给这样的标题打上
//! [`km::DIV_ATTR`]、把层叠出来的声明全写进行内（改了标签选 `hN` 的规则就选不到了；复制规则会多一个类、特异性变高，
//! 《张居正》`.calibre6` 的字号输给了复制的 UA 规则），中间按这台排的 pass 把它当 `div` 算；目录处理完以后（补节、改指、
//! 生成 nav 都按 `<h1>`–`<h6>` 认标题）这里才真改：开、闭标签换成 `div`，去掉记号、加类 `eink-hN`；我们自己写的样式表
//! （`eink-*.css`）里选 `hN.eink-…` 的规则复制一条换成 `div`（类型换类型，特异性不变）。会被书里别的规则选到的、这一级
//! 出现在组合选择器前面的 `block_style` 不打记号（[`super::block_style::DivGuard`]）。
use super::*;
use crate::kindle_model as km;
use std::collections::BTreeSet;

/// 改标签、复制规则。返回改了几个标题。
pub(super) fn finish(entries: &mut [Entry]) -> usize {
    let mut levels: BTreeSet<u8> = BTreeSet::new();
    let mut n = 0;
    for e in entries.iter_mut().filter(|e| is_chapter_entry(e)) {
        let Ok(text) = std::str::from_utf8(&e.data) else { continue };
        if !text.contains(km::DIV_ATTR) {
            continue;
        }
        let mut edits: Vec<(usize, usize, String)> = Vec::new();
        for t in html::tags(text).filter(|t| t.kind == html::TagKind::Open) {
            let raw = &text[t.start..t.end];
            let Some(level) = t.heading_level().filter(|_| html::attr_value(raw, km::DIV_ATTR).is_some()) else { continue };
            let Some(close) = html::find_matching_close(text, t.end, t.name) else { continue };
            let tag = html::remove_attr(raw, km::DIV_ATTR);
            let tag = html::add_class(&format!("<div{}", &tag[1 + t.name.len()..]), &format!("eink-h{level}"));
            edits.push((t.start, t.end, tag));
            edits.push((close.start, close.end, "</div>".into()));
            levels.insert(level);
            n += 1;
        }
        edits.sort_by_key(|x| (x.0, x.1));
        e.data = html::apply_edits(text, edits).into_bytes();
    }
    if levels.is_empty() {
        return 0;
    }
    for e in entries.iter_mut().filter(|e| is_css_name(&e.name) && e.name.rsplit('/').next().is_some_and(|f| f.starts_with("eink-"))) {
        let Ok(text) = std::str::from_utf8(&e.data) else { continue };
        if let Some(o) = copy_rules(text, &levels) {
            e.data = o.into_bytes();
        }
    }
    n
}

/// 选到改了的标题级别的规则后面接一条复制的（`hN` 换成 `div`）；没有要接的 `None`。
fn copy_rules(css: &str, levels: &BTreeSet<u8>) -> Option<String> {
    let mut edits: Vec<(usize, usize, String)> = Vec::new();
    for r in crate::cascade::rule_spans(css) {
        if !r.closed || r.body.contains('{') {
            continue;
        }
        let sel = crate::cascade::rule_selector(r.prelude);
        if sel.trim_start().starts_with('@') {
            continue;
        }
        let copies: Vec<String> = split_selectors(&sel)
            .into_iter()
            .filter_map(|s| {
                let mut out = s.to_string();
                let mut hit = false;
                for &l in levels.iter().rev() {
                    let name = format!("h{l}");
                    // 从后往前换，前面的位置不变
                    // 只换带我们的类的（`h2.eink-b1`）：`eink-ua.css` 的 `h2{…}` 这类光写标签的换成 `div` 会选到全书的 div
                    for (at, _) in super::block_style::type_tokens(&out.clone(), &name).into_iter().rev().filter(|(_, rest)| rest.starts_with(".eink-")) {
                        out.replace_range(at..at + name.len(), "div");
                        hit = true;
                    }
                }
                hit.then(|| out.trim().to_string())
            })
            .collect();
        if copies.is_empty() {
            continue;
        }
        let body = r.body.trim_end();
        let semi = if body.is_empty() || body.ends_with(';') { "" } else { ";" };
        edits.push((r.end, r.end, format!("\n{}{{{body}{semi}}}", copies.join(", "))));
    }
    (!edits.is_empty()).then(|| html::apply_edits(css, edits))
}

/// 选择器列表按顶层的逗号拆开（括号里的不算：`:is(h1, h2)`）。
fn split_selectors(sel: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let (mut depth, mut from) = (0i32, 0);
    for (i, c) in sel.char_indices() {
        match c {
            '(' | '[' => depth += 1,
            ')' | ']' => depth -= 1,
            ',' if depth == 0 => {
                out.push(&sel[from..i]);
                from = i + 1;
            }
            _ => {}
        }
    }
    out.push(&sel[from..]);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 有记号的标题改成 `div.eink-hN`；我们写的 `eink-*.css` 里选 `hN` 的规则复制一条换成 `div`，书里的样式表不动；没记号的不动。
    #[test]
    fn renames_marked_headings_and_copies_own_rules() {
        let mut b = vec![
            Entry { name: "O/s.css".into(), data: b"h2{font-size:1.75em}".to_vec() },
            Entry { name: "O/eink-box.css".into(), data: b"html body h2.eink-b1{margin-top:2em} html body p.eink-b2{margin-top:1em} h2{font-size:1.5em}".to_vec() },
            Entry { name: "O/c.xhtml".into(), data: format!(r#"<html xmlns="http://www.w3.org/1999/xhtml"><head><title>t</title></head><body><h1>甲</h1><h2 class="t eink-b1" {}="1" style="font-size:1.75em">一<span>乙</span></h2><p>丙</p><h2>二</h2></body></html>"#, km::DIV_ATTR).into_bytes() },
        ];
        assert_eq!(finish(&mut b), 1);
        let page = String::from_utf8(b[2].data.clone()).unwrap();
        assert!(page.contains(r#"<h1>甲</h1><div class="t eink-b1 eink-h2" style="font-size:1.75em">一<span>乙</span></div><p>丙</p><h2>二</h2>"#), "{page}");
        assert_eq!(String::from_utf8(b[0].data.clone()).unwrap(), "h2{font-size:1.75em}");
        assert_eq!(String::from_utf8(b[1].data.clone()).unwrap(), "html body h2.eink-b1{margin-top:2em}\nhtml body div.eink-b1{margin-top:2em;} html body p.eink-b2{margin-top:1em} h2{font-size:1.5em}");
    }
}
