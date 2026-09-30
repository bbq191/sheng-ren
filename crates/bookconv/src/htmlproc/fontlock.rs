//! 字体锁：`strip_font_locks` 解除内联样式里的字体、字号锁（规则同清洗层，见 `crate::cssunlock`）。
//! 2026-09-29 起不再把灰字改黑、细字重提到 400：用户要求不改颜色等其他样式（只解字体、字号的锁）。
use super::*;

// ===== 字体解锁 =====

/// 解除内联 style 里的字体、字号锁——第三方 EPUB 常内联写死字体/字号，覆盖掉阅读器的设置。`font-family` 去掉、
/// 绝对字号去掉（相对字号保留，`body`/`html` 上的也去掉）、`font` 简写只留粗斜体，判断同清洗层（[`crate::cssunlock::unlock`]）。
/// style 因此清空则连整个 style 属性一并删掉；其他声明(颜色/对齐/缩进等)原样保留。
/// 只认名字正好是 `style` 的属性（2026-09-27 审计：旧正则 `\s*style="` 没有左边界，`data-style=""`、
/// SVG 的 `font-style=""` 也被当成 style 改写，产出非法 XML）；值按声明切（引号/括号/字符引用里的 `;` 不算）。
pub fn strip_font_locks(html: &str) -> String {
    use crate::cssunlock::{unlock, Unlock};
    html::edit_attrs(html, &["style"], |t, a| {
        let base_text = matches!(t.name.to_ascii_lowercase().as_str(), "body" | "html");
        let decls = html::css_decls(a.value);
        let font = |d: &html::CssDecl| ["font", "font-family", "font-size"].iter().any(|p| d.prop.eq_ignore_ascii_case(p));
        let edits: Vec<(&html::CssDecl, Unlock)> =
            decls.iter().filter(|d| font(d)).map(|d| (d, unlock(d.prop, d.value, base_text))).filter(|(_, u)| *u != Unlock::Keep).collect();
        if edits.is_empty() {
            return Edit::Keep;
        }
        let mut kept = String::with_capacity(a.value.len());
        let mut last = 0;
        for (d, u) in &edits {
            kept.push_str(&a.value[last..d.start]);
            if let Unlock::Replace(v) = u {
                kept.push_str(&v.join(";"));
                if d.raw.trim_end().ends_with(';') {
                    kept.push(';');
                }
            }
            last = d.start + d.raw.len();
        }
        kept.push_str(&a.value[last..]);
        let kept = kept.trim().trim_start_matches(';').trim();
        if kept.is_empty() {
            Edit::Remove // 整个 style 属性删掉(连前导空格)
        } else {
            Edit::Set(kept.to_string())
        }
    })
    .into_owned()
}

#[cfg(test)]
mod font_lock_tests {
    use super::strip_font_locks;
    #[test]
    fn strips_full_font_style() {
        // 导入赎罪真实形态：整个 style 都是 font → 连 style 属性一起删
        let html = r#"<p style="font-size:16px;font-family:&#39;PingFang SC&#39;;">正文</p>"#;
        assert_eq!(strip_font_locks(html), "<p>正文</p>");
    }
    #[test]
    fn keeps_non_font_decls() {
        let html = r#"<p style="color:red;font-size:16px;text-align:center;">x</p>"#;
        let out = strip_font_locks(html);
        assert!(!out.contains("font-size"), "font-size 未删: {out}");
        assert!(out.contains("color:red"), "color 被误删: {out}");
        assert!(out.contains("text-align:center"), "text-align 被误删: {out}");
    }
    #[test]
    fn relative_size_and_font_style_kept() {
        assert_eq!(strip_font_locks(r#"<span style="font-size:0.8em">x</span>"#), r#"<span style="font-size:0.8em">x</span>"#);
        assert_eq!(strip_font_locks(r#"<body style="font-size:0.8em">x</body>"#), "<body>x</body>", "正文整体的相对字号也去掉");
        assert_eq!(strip_font_locks(r#"<span style="font:italic bold 12px serif;color:#333">x</span>"#), r#"<span style="font-style:italic;font-weight:bold;color:#333">x</span>"#);
    }
    #[test]
    fn no_style_untouched() {
        assert_eq!(strip_font_locks("<p>纯文本</p>"), "<p>纯文本</p>");
    }
}
