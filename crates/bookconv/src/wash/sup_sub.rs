//! 行内元素上 `vertical-align` 写的上标、下标照 Kindle（不认它的阅读器：掌阅，profile `inline_valign = false`；只修复、照 Send to Kindle
//! 统一的文字书；2026-10-14 用户要修）。
//!
//! 掌阅（2026-10-14 怪癖测试书 Q100、Q101、Q120–Q123）：`<span>` 上的 `vertical-align:super/sub` 不认（写长度、`position:relative`
//! 也不认），`<sup>`/`<sub>` 标签认（抬高或压低、字缩小）。Kindle 写出器的层叠里两种写法一样（[`crate::cascade::Computed`] 的
//! `superscript`/`subscript`，`top`/`text-top` 也算上标），写成同一个上标、下标值。所以把写了这些值的行内元素里面整个包一层
//! `<sup>`/`<sub>`：元素本身（`<a>` 注释链接、类、行内样式）和字都不动，掌阅上显示同它的 `<sup>`（元素自己写了字号的，包的这层写
//! `font-size:100%`，同 `<sup class="…">` 作者字号盖过标签缺省）。外面已经是上标、下标的（写在
//! 外层元素上的、在 `<sup>` 里的）不再包，同写出器只在和外层不一样时写。
use super::*;
use crate::cascade::MediaEnv;
use crate::kindle_model::Chapter;

pub(super) fn apply(entries: &mut [Entry], rep: &mut WashReport, media: Option<&MediaEnv>, inline_valign: bool) {
    if inline_valign {
        return;
    }
    // 先粗筛：书里没写 vertical-align 的不用层叠
    if !entries.iter().any(|e| (is_css_name(&e.name) || is_chapter_entry(e)) && String::from_utf8_lossy(&e.data).to_ascii_lowercase().contains("vertical-align")) {
        return;
    }
    let (chapters, fixed) = super::scan_chapters(entries, media, |e, css| wrap(e, media, css));
    for (i, f) in chapters.into_iter().zip(fixed) {
        if let Some((data, n)) = f {
            entries[i].data = data;
            rep.kindle_rule_edits += n;
        }
    }
}

fn wrap(e: &Entry, media: Option<&MediaEnv>, css: &HashMap<&str, std::sync::Arc<crate::cascade::Rules>>) -> Option<(Vec<u8>, usize)> {
    let text = std::str::from_utf8(&e.data).ok()?;
    let ch = Chapter::parse(&e.name, text, media, css).filter(|c| c.matches_tags)?;
    let (bs, be) = html::body_range(text)?;
    let starts: Vec<html::Tag> = html::tags_in(text, bs, be).filter(|t| t.is_start()).collect();
    let mut edits: Vec<(usize, usize, String)> = Vec::new();
    for (i, el) in ch.els.iter().enumerate() {
        let name = ch.el(i).value().name().to_ascii_lowercase();
        if el.is_block || el.hidden || matches!(name.as_str(), "img" | "svg" | "image" | "sup" | "sub" | "br") {
            continue;
        }
        let tag = match el.decls.get("vertical-align").map(|v| v.trim().to_ascii_lowercase()).as_deref() {
            Some("super" | "top" | "text-top") => "sup",
            Some("sub") => "sub",
            _ => continue,
        };
        // 外层已经是上标、下标的不再包
        let outer = el.parent.map_or(&ch.body_comp, |p| &ch.els[p].comp);
        if outer.superscript || outer.subscript {
            continue;
        }
        let Some(t) = starts.get(i) else { continue };
        if t.end >= 2 && text[..t.end].ends_with("/>") {
            continue;
        }
        let Some(close) = html::find_matching_close(text, t.end, &name) else { continue };
        if text[t.end..close.start].trim().is_empty() {
            continue;
        }
        // 元素自己写了字号的，包的这层不再按标签缺省缩小（掌阅上同 `<sup class="…">`：作者的字号盖过标签缺省）
        let open = if el.decls.contains_key("font-size") { format!(r#"<{tag} style="font-size:100%">"#) } else { format!("<{tag}>") };
        edits.push((t.end, t.end, open));
        edits.push((close.start, close.start, format!("</{tag}>")));
    }
    if edits.is_empty() {
        return None;
    }
    let n = edits.len() / 2;
    edits.sort_by_key(|x| (x.0, x.1));
    Some((html::apply_edits(text, edits).into_bytes(), n))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn book(body: &str, css: &str) -> Vec<Entry> {
        vec![
            Entry { name: "O/content.opf".into(), data: br#"<package><manifest><item id="c" href="c.xhtml" media-type="application/xhtml+xml"/></manifest><spine><itemref idref="c"/></spine></package>"#.to_vec() },
            Entry { name: "O/s.css".into(), data: css.as_bytes().to_vec() },
            Entry { name: "O/c.xhtml".into(), data: format!(r#"<html xmlns="http://www.w3.org/1999/xhtml"><head><title>t</title><link rel="stylesheet" href="s.css"/></head><body>{body}</body></html>"#).into_bytes() },
        ]
    }

    /// 类规则、行内写的上标下标包一层标签；外层已经是上标的、本来就是 `<sup>` 的、块、空的不动；认的阅读器不改。
    #[test]
    fn wraps_super_and_sub() {
        let body = r##"<p>甲<a class="n" href="#z">1</a>乙<span style="vertical-align:sub">2</span><span class="n">外<span class="n">内</span></span><sup class="n">3</sup><span class="n"></span></p><p class="n">块</p>"##;
        let mut b = book(body, ".n{vertical-align:super;font-size:0.75em}");
        apply(&mut b, &mut WashReport::default(), None, false);
        let page = String::from_utf8(b[2].data.clone()).unwrap();
        assert!(page.contains(r##"<a class="n" href="#z"><sup style="font-size:100%">1</sup></a>乙<span style="vertical-align:sub"><sub>2</sub></span><span class="n"><sup style="font-size:100%">外<span class="n">内</span></sup></span><sup class="n">3</sup><span class="n"></span></p><p class="n">块</p>"##), "{page}");
        let mut b = book(body, ".n{vertical-align:super}");
        apply(&mut b, &mut WashReport::default(), None, true);
        assert!(String::from_utf8(b[2].data.clone()).unwrap().contains(body));
    }
}
