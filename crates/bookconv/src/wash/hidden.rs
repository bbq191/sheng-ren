//! 去掉 `display:none` 的元素（不认 `display:none` 的阅读器：Move，profile `display_none = false`；只修复、照 Send to Kindle 统一的文字书）。
//!
//! xochitl 不认 `display:none`：行内 `style` 不认（它根本不读行内样式），写在外链样式表里也不认（2026-10-12 测试书「测试-隐藏」）。
//! Kindle、掌阅上看不见的元素在 Move 上照样显示：《飘》《春雪》版权页顶上 `<h1 style="display:none;">版权信息</h1>`（给目录用的）
//! 成了一个大标题，竖直间距照 Kindle 换算后把版权页挤成两页加一页空白。2026-10-12 定：Move 上把这些元素去掉（在 Kindle 上本来
//! 就看不见）。元素上和里面的 `id` 留成空的 `<span id>`，指向它的目录、链接照样能跳。
//!
//! 认得出的：行内 `style` 写了 `display:none`；样式表、`<style>` 里只有一个简单选择器（`.类`、`标签.类`）的规则写了 `display:none`。
//! 别的（后代选择器、`:not()`……）拿不准，不动。
use super::*;

/// 去掉全书章节里 `display:none` 的元素，返回去掉了几个。
pub(super) fn apply(entries: &mut [Entry]) -> usize {
    let hidden = hidden_selectors(entries);
    crate::util::par_map_mut(entries, |e| {
        if !is_chapter_entry(e) {
            return 0;
        }
        let Ok(text) = std::str::from_utf8(&e.data) else { return 0 };
        let (out, n) = drop_hidden(text, &hidden);
        if n > 0 {
            e.data = out.into_bytes();
        }
        n
    })
    .into_iter()
    .sum()
}

/// 样式表、`<style>` 里写了 `display:none` 的简单选择器：(标签名或空, 类)。
fn hidden_selectors(entries: &[Entry]) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut scan = |css: &str| {
        for r in crate::cascade::rule_spans(css) {
            let none = html::css_decls(r.body).iter().any(|d| d.prop.eq_ignore_ascii_case("display") && crate::cssunlock::split_important(d.value).0.trim().eq_ignore_ascii_case("none"));
            if !none {
                continue;
            }
            for sel in crate::cascade::rule_selector(r.prelude).split(',') {
                let sel = sel.trim();
                if sel.contains([' ', '>', '+', '~', ':', '[', '#']) {
                    continue;
                }
                let (tag, classes) = crate::cascade::compound_tag_classes(sel);
                if let [c] = classes.as_slice() {
                    out.push((tag, c.to_string()));
                }
            }
        }
    };
    for e in entries {
        let Ok(text) = std::str::from_utf8(&e.data) else { continue };
        if is_css_name(&e.name) {
            scan(text);
        } else if is_chapter_entry(e) {
            for b in html::style_blocks(text) {
                scan(&text[b.content_start..b.content_end]);
            }
        }
    }
    out
}

/// 一章里去掉隐藏的元素（连同里面的东西），`id` 留成空 `<span>`。返回 (新文本, 去掉了几个)。
fn drop_hidden(text: &str, hidden: &[(String, String)]) -> (String, usize) {
    let Some((body_start, body_end)) = html::body_range(text) else { return (text.to_string(), 0) };
    let mut edits: Vec<(usize, usize, String)> = Vec::new();
    let mut skip_to = 0;
    for t in html::tags_in(text, body_start, body_end).filter(|t| t.is_start()) {
        if t.start < skip_to {
            continue;
        }
        let raw = &text[t.start..t.end];
        let inline_none = html::attr_value(raw, "style").is_some_and(|s| html::css_decls(s).iter().any(|d| d.prop.eq_ignore_ascii_case("display") && crate::cssunlock::split_important(d.value).0.trim().eq_ignore_ascii_case("none")));
        let name = t.name.to_ascii_lowercase();
        let class_none = hidden.iter().any(|(tag, c)| (tag.is_empty() || *tag == name) && html::has_class(raw, c));
        if !(inline_none || class_none) {
            continue;
        }
        let end = if raw.ends_with("/>") || html::is_void(&name) {
            t.end
        } else {
            match html::find_close(text, t.end, &name) {
                Some(c) => c.end,
                None => continue, // 没闭合：拿不准
            }
        };
        let ids: String = html::anchors(&text[t.start..end]).into_iter().map(|(id, _)| format!(r#"<span id="{id}"></span>"#)).collect();
        edits.push((t.start, end, ids));
        skip_to = end;
    }
    let n = edits.len();
    (html::apply_edits(text, edits), n)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hidden_elements_dropped_ids_kept() {
        let css = ".gone{display:none} p.x{display: none !important} div p.y{display:none} .z{display:block}";
        let page = r#"<html><head><style>.st{display:none}</style></head><body><h1 style="display:none;" id="h">版权信息</h1><p>看得见</p><p class="gone a">隐<b id="i">藏</b></p><div class="x">留</div><p class="x">去</p><p class="y">留</p><span class="st">去</span><br class="gone"/><p class="z">留</p></body></html>"#;
        let mut book = vec![Entry { name: "s.css".into(), data: css.as_bytes().to_vec() }, Entry { name: "c.xhtml".into(), data: page.as_bytes().to_vec() }];
        assert_eq!(apply(&mut book), 5);
        let out = String::from_utf8(book[1].data.clone()).unwrap();
        assert_eq!(out, r#"<html><head><style>.st{display:none}</style></head><body><span id="h"></span><p>看得见</p><span id="i"></span><div class="x">留</div><p class="y">留</p><p class="z">留</p></body></html>"#);
    }
}
