//! 块级标签、表现属性与嵌入字体。

use crate::computed::Computed;
use crate::text::{font_face_rules, parse_decls, strip_comments};
use crate::Decls;
use scraper::ElementRef;

/// 缺省是块级的标签（`display` 没写时）。
pub const BLOCK_TAGS: &[&str] = &[
    "address",
    "article",
    "aside",
    "blockquote",
    "body",
    "center",
    "dd",
    "details",
    "dialog",
    "dir",
    "div",
    "dl",
    "dt",
    "fieldset",
    "figcaption",
    "figure",
    "footer",
    "form",
    "h1",
    "h2",
    "h3",
    "h4",
    "h5",
    "h6",
    "header",
    "hgroup",
    "hr",
    "li",
    "main",
    "menu",
    "nav",
    "ol",
    "p",
    "pre",
    "section",
    "summary",
    "table",
    "tbody",
    "td",
    "tfoot",
    "th",
    "thead",
    "tr",
    "ul",
    "caption",
];

/// 元素是不是块级：先看 `display`，没写按标签。
pub fn is_block(el: &ElementRef, comp: &Computed) -> bool {
    match comp.display.as_deref() {
        Some("block" | "list-item" | "table" | "table-row" | "table-cell" | "flex") => true,
        Some("inline" | "inline-block") => false,
        _ => BLOCK_TAGS.contains(&el.value().name()),
    }
}

/// HTML 的表现属性当成优先级最低的样式（样式表写了的不动）：块的 `align`、`<center>`、`<font size/color/face>`、`<img width="N%">`
/// （Send to Kindle 同样：《福尔摩斯》`<p align="justify">` 两端对齐、`<font size="1">` 字号 0.625、`size="7"` 3.0）。
pub fn presentational_hints(el: &ElementRef, decls: &mut Decls) {
    let e = el.value();
    let mut hint = |k: &str, v: String| {
        decls.entry(k.to_string()).or_insert(v);
    };
    match e.name() {
        "p" | "div" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6" | "td" | "th" | "tr" | "caption"
        | "blockquote" => {
            if let Some(a) = e
                .attr("align")
                .map(|a| a.trim().to_ascii_lowercase())
                .filter(|a| ["left", "right", "center", "justify"].contains(&a.as_str()))
            {
                hint("text-align", a);
            }
        }
        "center" => hint("text-align", "center".into()),
        // `<img width="60%">`：百分比的宽度属性当 CSS 宽度（HTML 规范的表现属性；掌阅、Move 照这个显示，KFX 写出器以前不认、
        // Kindle 按原图像素显示——《金庸》639 张 60% 的插图、《飘》《春雪》，2026-10-12 `tools/kfx/unify.py`）。像素的
        // `width="566"` 多半就是原图宽度，没对过阅读器怎么算 px，不收
        "img" => {
            if let Some(w) = e.attr("width").map(str::trim).filter(|w| {
                w.strip_suffix('%')
                    .is_some_and(|n| n.trim().parse::<f64>().is_ok_and(|v| v > 0.0))
            }) {
                hint("width", w.to_string());
            }
        }
        "font" => {
            // HTML 字号 1–7（同浏览器：10、13、16、18、24、32、48px，绝对字号），`+n`/`-n` 相对 3（饱和加减：`+2147483647` 以前溢出）
            if let Some(s) = e.attr("size").map(str::trim) {
                let n = match s.strip_prefix('+') {
                    Some(r) => r.parse::<i32>().ok().map(|r| 3i32.saturating_add(r)),
                    None => match s.strip_prefix('-') {
                        Some(r) => r.parse::<i32>().ok().map(|r| 3i32.saturating_sub(r)),
                        None => s.parse::<i32>().ok(),
                    },
                };
                if let Some(n) = n {
                    let em =
                        [0.625, 0.8125, 1.0, 1.125, 1.5, 2.0, 3.0][(n.clamp(1, 7) - 1) as usize];
                    hint("font-size", format!("{em}rem"));
                }
            }
            if let Some(c) = e.attr("color") {
                hint("color", c.trim().to_string());
            }
            if let Some(f) = e.attr("face") {
                hint("font-family", f.trim().to_string());
            }
        }
        _ => {}
    }
}

/// 一条 `@font-face`：字体名、字体文件（相对书根的路径）、是不是粗体、斜体。
#[derive(Clone, Debug, PartialEq)]
pub struct FontFace {
    pub family: String,
    pub path: String,
    pub bold: bool,
    pub italic: bool,
}

/// 样式表里的 `@font-face`（只收 `src` 里有 `url()` 指到书内文件的）。`base` 是样式表所在文件的路径。
pub fn font_faces(css: &str, base: &str) -> Vec<FontFace> {
    let css = strip_comments(css);
    let mut out = Vec::new();
    for r in font_face_rules(&css) {
        let decls = parse_decls(r.body);
        let get = |k: &str| {
            decls
                .iter()
                .rev()
                .find(|d| d.prop == k)
                .map(|d| d.value.as_str())
        };
        let family = get("font-family")
            .map(|f| f.trim().trim_matches(['"', '\'']).to_string())
            .filter(|f| !f.is_empty());
        let url = get("src").and_then(|src| {
            let raw = bookbase::html::css_urls(src)
                .into_iter()
                .next()?
                .value
                .trim();
            (!raw.is_empty() && !bookbase::html::is_external(raw))
                .then(|| bookbase::epubpath::resolve_link(base, raw).0)
        });
        if let (Some(family), Some(path)) = (family, url) {
            let bold = get("font-weight").is_some_and(|w| {
                matches!(w.trim(), "bold" | "bolder")
                    || w.trim().parse::<u32>().is_ok_and(|n| n >= 600)
            });
            let italic =
                get("font-style").is_some_and(|v| matches!(v.trim(), "italic" | "oblique"));
            out.push(FontFace {
                family,
                path,
                bold,
                italic,
            });
        }
    }
    out
}
