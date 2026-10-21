//! 标签的缺省样式（HTML 规范的 UA 样式表里影响版面的那几条）。Send to Kindle 照它排没写样式的书（2026-10-08 对照《绍宋》《绝叫》：
//! 没写外边距的 `<p>` 上下各 1em、`<h2>` 1.5em），各家阅读器自己的缺省却不一样——掌阅给 `<p>` 的外边距是 0，原书丢了样式表的
//! 《绝叫》在掌阅上段与段挤在一起（2026-10-08 真机截屏），Kindle 上有段距。
//!
//! 两处共用这一张表：KFX 写出器按它给标签缺省值（`kfx::css`），只修复的文字书按它写一份 `eink-ua.css` 挂在书自带样式之前
//! （profile `kindle_rules`，掌阅、Move；[`ua_css`]），书里写了的照样盖过它——三台按同一套缺省排（2026-10-08 定：照 Send to Kindle 统一）。

// 标签缺省值的表在 `csscascade::ua`（层叠引擎要用，2026-10-15 挪过去），这里照旧能用
pub use csscascade::ua::{BLOCK_MARGIN_TAGS, HEADINGS, INDENTED_BLOCK_TAGS, INDENT_PX};

/// 样式表的文件名（放 OPF 同目录）。
pub const UA_CSS_NAME: &str = "eink-ua.css";

/// 写进书里的缺省样式表。⚠ xochitl 的 CSS 解析器很脆（见 `wash::WASH_CSS_NAME` 的注）：只用裸元素选择器、一个选择器一条规则、
/// 每条声明以 `;` 收尾、不用 `!important`。挂在书自带样式之前：书里写了的同特异性靠源序后者胜，类选择器、行内样式本来就更高。
///
/// `font_scale`：全书正文字号不是 1em 时要乘的系数（`wash::kindle_rules`），写成 `body` 的字号百分比——书里 `body` 没写字号时
/// 由它把正文拉回阅读器的字号；写了的书里那条已经乘过，盖过这条。
pub fn ua_css(font_scale: Option<f64>) -> String {
    let mut s = String::new();
    for t in BLOCK_MARGIN_TAGS {
        s.push_str(&format!("{t}{{margin-top:1em;margin-bottom:1em;}}\n"));
    }
    for t in INDENTED_BLOCK_TAGS {
        s.push_str(&format!("{t}{{margin-top:1em;margin-bottom:1em;margin-left:{INDENT_PX}px;margin-right:{INDENT_PX}px;}}\n"));
    }
    for (t, size, margin) in HEADINGS {
        s.push_str(&format!("{t}{{font-size:{size}em;font-weight:bold;margin-top:{margin}em;margin-bottom:{margin}em;}}\n"));
    }
    if let Some(k) = font_scale {
        s.push_str(&format!("body{{font-size:{}%;}}\n", crate::wash::fmt_num(k * 100.0)));
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ua_css_is_bare_element_rules_ending_with_semicolons() {
        let css = ua_css(None);
        assert!(css.starts_with("p{margin-top:1em;margin-bottom:1em;}\n"), "{css}");
        assert!(css.contains("h2{font-size:1.5em;font-weight:bold;margin-top:0.83em;margin-bottom:0.83em;}"), "{css}");
        assert!(css.contains("blockquote{margin-top:1em;margin-bottom:1em;margin-left:40px;margin-right:40px;}"), "{css}");
        for line in css.lines() {
            let (sel, body) = line.split_once('{').unwrap();
            assert!(sel.chars().all(|c| c.is_ascii_alphanumeric()), "只用裸元素选择器：{line}");
            assert!(body.ends_with(";}") && !body.contains("!important"), "{line}");
        }
        assert!(!css.contains("body"));
        assert!(ua_css(Some(0.8)).ends_with("body{font-size:80%;}\n"));
        assert!(ua_css(Some(1.0 / 1.15)).ends_with("body{font-size:86.9565%;}\n"));
    }
}
