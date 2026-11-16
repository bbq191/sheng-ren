//! 够写 KFX 用的 CSS：解析样式表、按选择器优先级层叠、算出每个元素的计算值。KFX 写出器（`kfx::css` 就是这个模块）和
//! 掌阅、Move 的 `kindle_rules`（算全书正文字号，[`crate::wash`]）共用，两边的口径一样。
//!
//! 只认 KFX 能表达的属性（字体、字号、字重、斜体、对齐、缩进、行高、边距、内边距、颜色、背景色、上下标、边框、
//! 列表符号、表格边框合并与间距、文字装饰、字间距、`pre`）；
//! 别的属性忽略。选择器匹配用 scraper（html5ever DOM），优先级自己算。
//! `@media` 按阅读模式的阅读范围、屏幕求值（见 [`media_ok`]），`@font-face`、`@page` 等跳过（嵌入字体另由 [`font_faces`] 读）。
//!
//! 2026-10-15 从 `bookconv` 拆成单独的 crate（`bookconv::cascade` 照旧能用）：KFX 写出器和优化器共用这一个层叠引擎。

pub mod decl;
pub mod ua;

mod computed;
mod html;
mod media;
mod rules;
mod sheet;
mod shorthand;
mod text;
mod value;

#[cfg(test)]
mod tests;

use std::collections::HashMap;

pub use bookbase::color::{contrast, ensure_contrast, luminance, over_white, parse_color};
pub use computed::{
    body_font_size, parse_border_width, parse_shadow, weighted_mode, Border, BorderWidth, Computed,
    Shadow,
};
pub use html::{font_faces, is_block, presentational_hints, FontFace, BLOCK_TAGS};
pub use media::{media_ok, MediaEnv};
pub use rules::Rules;
pub use sheet::{Origin, Sheet};
pub use shorthand::{
    box_sides, parse_bg_position, parse_bg_size, parse_font_shorthand, BoxSides, FontShorthand,
};
pub use text::{
    background_tokens, compound_tag_classes, font_face_rules, is_font_face, last_compound,
    parse_decls, rule_selector, rule_spans, specificity, split_leading_statements, split_top,
    strip_comments, tokens, RuleSpan,
};
pub use value::{
    device_line, device_px_em, parse_box_len, parse_len, set_device_line, set_device_px_em,
    DeviceLine, Len, PT_PER_EM,
};

/// 一条声明。
#[derive(Clone, Debug, PartialEq)]
pub struct Decl {
    pub prop: String,
    pub value: String,
    pub important: bool,
}

/// 一个元素层叠出来的声明：属性名（小写）→ 值。用快速哈希（[`bookbase::util::FastHasher`]）：每个元素建一张、换算计算值时
/// 查几十次，SipHash 是层叠里最大的开销。
pub type Decls = HashMap<String, String, bookbase::util::FastState>;

/// 生效的声明不是外链样式表里的普通声明的：属性名 → (来源, 带没带 `!important`)（[`Sheet::cascade_split`]）。
pub type Origins = HashMap<String, (Origin, bool), bookbase::util::FastState>;

/// 声明里的 `url(…)` 按 `base`（样式表或行内 `style` 所在文件的书内路径）换成书内路径；`base` 空时不换。
pub(crate) fn resolve_urls(decls: &mut [Decl], base: &str) {
    if base.is_empty() {
        return;
    }
    for d in decls.iter_mut() {
        if let Some(inner) = leading_url(&d.value) {
            d.value = format!("url({})", bookbase::epubpath::resolve_link(base, inner).0);
        }
    }
}

/// 值是 `url(…)` 开头时括号里的地址（去掉引号；按 [`bookbase::html::css_urls`] 认）。
pub(crate) fn leading_url(value: &str) -> Option<&str> {
    bookbase::html::css_urls(value)
        .into_iter()
        .next()
        .filter(|u| u.start == 0)
        .map(|u| u.value)
}
