//! 阅读器不认的 CSS 写法换成认的、排出来一样的写法（只修复、照 Send to Kindle 统一的文字书；2026-10-13 要修）。都是掌阅
//! （2026-10-13 怪癖测试书 `tools/quirks/probe.py ireader`）：
//!
//! - **`rem`**（profile `rem = false`）：掌阅不认，整条声明当没写（Q53：`margin-left:2rem` 不挪）。Kindle 写出器的层叠把字号以外的
//!   rem 当本元素的 em 算（[`crate::cascade::parse_len`]），这里照样改成 em；字号的 rem 按 16px 一 rem 折成 px（掌阅字号的 px 同
//!   Kindle，Q21）。《克莱因壶》的段落首行缩进写 `2rem`，掌阅上整本没有缩进。
//! - **`!important`**（profile `important = false`）：掌阅带它的声明一概不起作用，只有这一条、没有冲突的也一样（Q28、Q85、Q86；
//!   《克莱因壶》56 个标题的居中、《春雪》《飘》行内的下划线写法都失效）。去掉 `!important` 会改层叠：以前靠它压过更具体的选择器、
//!   行内 `style` 的，去掉以后就输了。所以先按 Kindle 的层叠（认 `!important`）和去掉以后的层叠（[`crate::cascade::Sheet::cascade_flat`]）
//!   逐个元素比，不一样的属性把 Kindle 的值接在元素的行内 `style` 末尾（行内压过样式表），再把全书的 `!important` 去掉。值里有
//!   `url()` 的（层叠里已经解析成书内路径）不接。
//!
//! 在 `kindle_rules` 改写之后、别的 pass 之前做：后面的 pass 按 Kindle 的层叠算，改完以后算出来的和改之前一样（rem 当 em、
//! `!important` 胜出的值在行内），不受影响。
use super::*;
use crate::cascade::{MediaEnv, Sheet};
use crate::util::xml_escape;
use scraper::{ElementRef, Html};

pub(super) fn apply(entries: &mut [Entry], rep: &mut WashReport, media: Option<&MediaEnv>, rem: bool, important: bool) {
    if rem && important {
        return;
    }
    // 先按改之前的样式表算 Kindle 上的层叠，接好行内的值，再改样式表
    if !important && entries.iter().any(|e| (is_css_name(&e.name) || is_chapter_entry(e)) && has_important(&String::from_utf8_lossy(&e.data))) {
        let (chapters, fixed) = super::scan_chapters(entries, media, |e, css| pin_important(e, media, css));
        for (i, f) in chapters.into_iter().zip(fixed) {
            if let Some((data, n)) = f {
                entries[i].data = data;
                rep.kindle_rule_edits += n;
            }
        }
    }
    rep.kindle_rule_edits += crate::util::par_map_mut(entries, |e| {
        let mut n = 0;
        if is_css_name(&e.name) {
            let (text, latin1) = match std::str::from_utf8(&e.data) {
                Ok(t) => (std::borrow::Cow::Borrowed(t), false),
                Err(_) => (std::borrow::Cow::Owned(crate::util::latin1_decode(&e.data)), true),
            };
            let out = fix_css(&text, rem, important, &mut n);
            if n > 0 {
                e.data = if latin1 { crate::util::latin1_encode(&out) } else { out.into_bytes() };
            }
            return n;
        }
        if !is_chapter_entry(e) {
            return 0;
        }
        let Ok(text) = std::str::from_utf8(&e.data) else { return 0 };
        let s = html::edit_style_attrs(text, |_, v| match fix_decls(v, rem, important) {
            Some((v, k)) => {
                n += k;
                if v.trim().is_empty() { html::Edit::Remove } else { html::Edit::Set(v) }
            }
            None => html::Edit::Keep,
        })
        .into_owned();
        let s = html::edit_style_blocks(&s, |_, css| {
            let mut k = 0;
            let out = fix_css(css, rem, important, &mut k);
            n += k;
            if k > 0 { html::Edit::Set(out) } else { html::Edit::Keep }
        })
        .into_owned();
        if n > 0 {
            e.data = s.into_bytes();
        }
        n
    })
    .into_iter()
    .sum::<usize>();
}

fn has_important(t: &str) -> bool {
    t.to_ascii_lowercase().contains("important")
}

/// 一章里 Kindle 上靠 `!important` 胜出、去掉以后会输的属性：接到元素的行内 `style`。返回 (新内容, 接了几条)；不用改的 `None`。
/// 元素数和开始标签对不上（解析器补了标签）的整章不管。
fn pin_important(e: &Entry, media: Option<&MediaEnv>, css: &HashMap<&str, std::sync::Arc<crate::cascade::Rules>>) -> Option<(Vec<u8>, usize)> {
    let text = std::str::from_utf8(&e.data).ok()?;
    let doc = Html::parse_document(text);
    let sheet = Sheet::for_doc(&doc, &e.name, media, |p| css.get(p).cloned());
    let body = doc.root_element().children().filter_map(ElementRef::wrap).find(|el| el.value().name() == "body")?;
    let els: Vec<ElementRef> = std::iter::once(body).chain(body.descendants().skip(1).filter_map(ElementRef::wrap)).collect();
    let (bs, be) = html::body_range(text)?;
    let body_tag = html::tags(text).find(|t| t.is_start() && t.is("body"))?;
    let tags: Vec<html::Tag> = std::iter::once(body_tag).chain(html::tags_in(text, bs, be).filter(|t| t.is_start())).collect();
    if tags.len() != els.len() {
        return None;
    }
    let mut edits = Vec::new();
    let mut n = 0;
    for (el, t) in els.iter().zip(&tags) {
        let kindle = sheet.cascade(el, &e.name);
        let flat = sheet.cascade_flat(el, &e.name);
        let mut add: Vec<(&String, &String)> = kindle.iter().filter(|(p, v)| flat.get(*p) != Some(*v) && !v.to_ascii_lowercase().contains("url(")).collect();
        if add.is_empty() {
            continue;
        }
        add.sort();
        let raw = &text[t.start..t.end];
        let mut style = html::attr_value(raw, "style").map(|s| html::style_base(s).to_string()).unwrap_or_default();
        for (p, v) in &add {
            if !style.trim().is_empty() {
                style.push(';');
            }
            style.push_str(&xml_escape(&format!("{p}:{v}")));
        }
        style.push(';');
        n += add.len();
        edits.push((t.start, t.end, html::set_attr(raw, "style", &style)));
    }
    if edits.is_empty() {
        return None;
    }
    Some((html::apply_edits(text, edits).into_bytes(), n))
}

/// 整份样式表的规则体照 [`fix_decls`] 改（`@font-face` 这类 at 规则不动）。
fn fix_css(css: &str, rem: bool, important: bool, n: &mut usize) -> String {
    let edits: Vec<(usize, usize, String)> = crate::cascade::rule_spans(css)
        .into_iter()
        .filter(|r| r.closed && !crate::cascade::rule_selector(r.prelude).trim().starts_with('@'))
        .filter_map(|r| {
            let (t, k) = fix_decls(r.body, rem, important)?;
            *n += k;
            Some((r.body_start, r.body_start + r.body.len(), t))
        })
        .collect();
    html::apply_edits(css, edits)
}

/// 一段声明：`important` 为假时去掉 `!important`，`rem` 为假时 rem 改写（见模块说明）。返回 (新文本, 改了几条)；没改的 `None`。
fn fix_decls(text: &str, rem: bool, important: bool) -> Option<(String, usize)> {
    let mut edits = Vec::new();
    for d in html::css_decls(text) {
        let (value, imp) = crate::cssunlock::split_important(d.value);
        let imp = if important { imp } else { "" };
        let prop = d.prop.to_ascii_lowercase();
        let new_value = if rem { None } else { rem_free(&prop, value) };
        if new_value.is_none() && imp.len() == crate::cssunlock::split_important(d.value).1.len() {
            continue;
        }
        let start = d.start + d.raw.find(d.value).unwrap_or(0);
        edits.push((start, start + d.value.len(), format!("{}{imp}", new_value.as_deref().unwrap_or(value))));
    }
    let k = edits.len();
    (k > 0).then(|| (html::apply_edits(text, edits), k))
}

/// 值里的 rem 改写：字号的 `Nrem` 折成 `16N px`，别的属性里的每个 `Nrem` 改成 `Nem`；没有 rem、有 `url()`/引号的 `None`。
fn rem_free(prop: &str, value: &str) -> Option<String> {
    let lower = value.to_ascii_lowercase();
    if !lower.contains("rem") || lower.contains("url(") || value.contains(['"', '\'']) {
        return None;
    }
    if prop == "font-size" {
        let n: f64 = lower.trim().strip_suffix("rem")?.parse().ok()?;
        return Some(format!("{}px", super::kindle_rules::fmt_num(n * 16.0)));
    }
    if prop == "font" {
        return None;
    }
    let b = value.as_bytes();
    let mut out = String::with_capacity(value.len());
    let mut last = 0;
    let mut i = 0;
    let mut changed = false;
    while i + 3 <= b.len() {
        // 数字紧跟 rem、后面不是名字字符
        if lower.as_bytes()[i..].starts_with(b"rem") && i > 0 && (b[i - 1].is_ascii_digit() || b[i - 1] == b'.') && !b.get(i + 3).is_some_and(|c| c.is_ascii_alphanumeric() || *c == b'-' || *c == b'_') {
            out.push_str(&value[last..i]);
            out.push_str("em");
            last = i + 3;
            i += 3;
            changed = true;
        } else {
            i += 1;
        }
    }
    out.push_str(&value[last..]);
    changed.then_some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn book(page: &str, css: &str) -> Vec<Entry> {
        vec![
            Entry { name: "O/content.opf".into(), data: br#"<package><manifest><item id="c" href="c.xhtml" media-type="application/xhtml+xml"/></manifest><spine><itemref idref="c"/></spine></package>"#.to_vec() },
            Entry { name: "O/s.css".into(), data: css.as_bytes().to_vec() },
            Entry { name: "O/c.xhtml".into(), data: format!(r#"<html xmlns="http://www.w3.org/1999/xhtml"><head><title>t</title><link rel="stylesheet" href="s.css"/><style>p{{margin:1rem !important}}</style></head><body>{page}</body></html>"#).into_bytes() },
        ]
    }

    fn run(page: &str, css: &str, rem: bool, important: bool) -> (String, String) {
        let mut b = book(page, css);
        apply(&mut b, &mut WashReport::default(), None, rem, important);
        (String::from_utf8(b[1].data.clone()).unwrap(), String::from_utf8(b[2].data.clone()).unwrap())
    }

    #[test]
    fn rem_to_em_and_px() {
        assert_eq!(rem_free("margin", "1rem 0.5rem 2REM"), Some("1em 0.5em 2em".into()));
        assert_eq!(rem_free("font-size", "1.5rem"), Some("24px".into()));
        assert_eq!(rem_free("margin", "1em"), None);
        assert_eq!(rem_free("background", "url(lorem1rem.png)"), None);
        assert_eq!(rem_free("font-family", "remington"), None);
        let (css, page) = run(r#"<p style="text-indent:2rem">字</p>"#, "p{text-indent:2rem !important;font-size:0.75rem}", false, true);
        assert_eq!(css, "p{text-indent:2em !important;font-size:12px}");
        assert!(page.contains("<style>p{margin:1em !important}</style>") && page.contains(r#"<p style="text-indent:2em">字</p>"#), "{page}");
    }

    /// 去掉 `!important`：靠它压过更具体的选择器、行内的，Kindle 上的值接进行内；本来就胜的不接；只认 rem 的阅读器不动 rem。
    #[test]
    fn important_pinned_inline() {
        let css = "h2{text-align:center !important} h2.t{text-align:left} .u{text-decoration:underline !important} p{color:red !important}";
        let page = r#"<h2 class="t">章</h2><h2>节</h2><p class="u" style="text-decoration:none;color:blue !important">字</p>"#;
        let (out_css, out) = run(page, css, true, false);
        assert_eq!(out_css, "h2{text-align:center} h2.t{text-align:left} .u{text-decoration:underline} p{color:red}");
        assert!(out.contains(r#"<h2 class="t" style="text-align:center;">章</h2><h2>节</h2>"#), "{out}");
        // 行内的 `!important` 压过样式表的：照行内；样式表的 underline !important 压过行内的 none：接上
        assert!(out.contains(r#"<p class="u" style="text-decoration:none;color:blue;text-decoration:underline;">字</p>"#), "{out}");
        assert!(out.contains("<style>p{margin:1rem}</style>"), "{out}");
    }

    #[test]
    fn css_reader_untouched() {
        let css = "h2{text-align:center !important;margin:1rem}";
        let (out_css, _) = run("<h2>章</h2>", css, true, true);
        assert_eq!(out_css, css);
    }
}
