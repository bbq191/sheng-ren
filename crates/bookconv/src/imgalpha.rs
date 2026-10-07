//! 正文图片的透明处合成白底（profile `image_alpha = false`，Kindle）。
//!
//! Kindle（KFX）把正文 `<img>` 里透明的地方显示成黑色：《绍宋》章标题图 logo.png（580×1062 RGBA，82% 像素全透明，透明像素
//! 存的颜色是黑色）在掌阅上透明正确，Kindle 上成了一大块黑底（2026-10-06 真机）。所以 kindle 模式把**正文 `<img>`、SVG
//! `<image>` 用到的、确实有透明像素的 PNG** 先合成到白底，格式不变（见 [`crate::imgopt::flatten_transparent_png`]）。
//!
//! - **CSS 背景图不动**：它下面有背景色，透明处该露出背景色；同一张图既当 `<img>` 又在 CSS（样式表、`<style>`、`style=""`）里
//!   被引用的也不动（拿不准）。
//! - 全不透明的 RGBA 图不改（解出来看一遍 alpha，没有不透明以外的像素就原样保留，不改无关产物）。
//! - 只管 PNG：GIF 可能是动图、重编码要量化颜色；WebP 在 KFX 里本来就不支持。漫画不归这里管（漫画页本来就合成白底）。

use std::collections::HashSet;

use crate::html;

/// 可能要合成白底的图（zip 路径；是不是 PNG、有没有透明像素，处理图片时再看）：正文 `<img src>`、SVG `<image href>` 引用、又没在任何 CSS 里被引用的。`entries` 是
/// (条目名, 字节, 是否 html)，路径都是清洗改名后的。
pub fn plan(entries: &[(String, Vec<u8>, bool)]) -> HashSet<String> {
    let css_urls = |text: &str, base: &str, css: &mut Vec<String>| {
        for u in crate::bgfit::urls(text) {
            if !html::is_external(&u) {
                css.push(crate::epubzip::resolve_link(base, &u).0);
            }
        }
    };
    // 各条目独立扫（多线程，`util::par_map`）：(正文用到的图, CSS 引用的图)，再并起来
    let per_entry = crate::util::par_map(entries, |(name, data, ish)| {
        let (mut imgs, mut css) = (Vec::new(), Vec::new());
        let Ok(text) = std::str::from_utf8(data) else { return (imgs, css) };
        if name.to_ascii_lowercase().ends_with(".css") {
            css_urls(text, name, &mut css);
        } else if *ish {
            for c in html::style_block_re().captures_iter(text) {
                css_urls(&c[2], name, &mut css);
            }
            for t in html::tags(text).filter(|t| t.is_start()) {
                let tag = &text[t.start..t.end];
                if let Some(s) = html::attr_value(tag, "style") {
                    css_urls(&crate::util::xml_unescape(s), name, &mut css);
                }
                let src = if t.is("img") {
                    html::attr_value(tag, "src")
                } else if t.name.rsplit(':').next().is_some_and(|n| n.eq_ignore_ascii_case("image")) {
                    html::attr_value(tag, "xlink:href").or_else(|| html::attr_value(tag, "href"))
                } else {
                    None
                };
                if let Some(v) = src.filter(|v| !v.is_empty() && !html::is_external(v)) {
                    imgs.push(crate::epubzip::resolve_link(name, v).0);
                }
            }
        }
        (imgs, css)
    });
    let mut imgs: HashSet<String> = HashSet::new();
    let mut css: HashSet<String> = HashSet::new();
    for (i, c) in per_entry {
        imgs.extend(i);
        css.extend(c);
    }
    imgs.retain(|p| !css.contains(p));
    imgs
}

#[cfg(test)]
mod tests {
    use super::*;

    fn e(name: &str, s: &str, ish: bool) -> (String, Vec<u8>, bool) {
        (name.into(), s.as_bytes().to_vec(), ish)
    }

    #[test]
    fn img_used_only_as_image() {
        let entries = vec![
            e("OEBPS/Styles/a.css", r#"body.x{background:url("../Images/bg.png")} .y{background-image:url(../Images/both.png)}"#, false),
            e(
                "OEBPS/Text/c.xhtml",
                r#"<html><head><style>p{background:url('../Images/st.png')}</style></head><body>
                <img src="../Images/logo.png"/><img src="../Images/both.png"/><img src="../Images/st.png"/>
                <div style="background:url(&quot;../Images/inl.png&quot;)"><img src="../Images/inl.png"/></div>
                <svg><image xlink:href="../Images/svg.png"/></svg><img src="../Images/p.jpg"/><img src="http://x/y.png"/></body></html>"#,
                true,
            ),
        ];
        let mut got: Vec<String> = plan(&entries).into_iter().collect();
        got.sort();
        // JPEG 也在里面：合成时按文件内容认格式，只处理 PNG
        assert_eq!(got, ["OEBPS/Images/logo.png", "OEBPS/Images/p.jpg", "OEBPS/Images/svg.png"]);
    }
}
