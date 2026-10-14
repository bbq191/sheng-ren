//! em 一律按根字号算的阅读器（Move，profile `em_root`）：字号、外边距、内边距、首行缩进写 em 的，换算成按根的值写进行内
//! （只修复、照 Send to Kindle 统一的文字书；2026-10-14 用户要修）。
//!
//! xochitl 外链样式表里的 em（字号的 % 也是）不按父元素、自己的字号，按阅读器字号算（2026-10-14 探针「测试-div探针」4–6：
//! 外层 2em 里 `font-size:0.5em` 排成 0.5 倍；1.75em 的块 `margin-left:2em` 只挪 76px、`margin-top:1em` 只空 38px）。前面
//! 照 Kindle 改写的 pass 都按 CSS 的意思写（[`crate::kindle_model::Chapter`] 层叠出来的值），这里最后逐个元素比：
//! - 字号写了 em、%（不是 rem）、按 CSS 算出来的（根 em）和 Move 读出来的（写的数当根 em）不一样的，接 `font-size:算出来的em`；
//!   写 px 的同样（Move 的 px 是屏幕像素：《疯探》正文 `14.9333px` 只有正常字号的四成，怪癖表 Q21；Kindle 按 0.75pt）；
//! - 外边距、内边距、首行缩进写了 em、元素字号不是 1 的，接 `属性:数×字号em`。
//!
//! 接在行内 `style` 末尾（同一个 `style` 里后写的胜），之后 `inline_css` 把行内的 em 挪进外链样式表（Move 行内的 em 另有
//! 一套怪癖）。标题（h1–h6，Move 只认字号关键字、不认上下外边距，`block_style`、`box_fix` 管）和标题里面的元素不动。
use super::*;
use crate::cascade::MediaEnv;
use crate::kindle_model::{self as km, Chapter};

/// 差多少算一样（根 em）。
const EPS: f64 = 0.005;

pub(super) fn apply(entries: &mut [Entry], rep: &mut WashReport, media: Option<&MediaEnv>, em_root: bool, em_px: Option<f64>) {
    if !em_root {
        return;
    }
    let css = km::book_css(entries, media);
    let chapters: Vec<usize> = (0..entries.len()).filter(|&i| is_chapter_entry(&entries[i])).collect();
    let fixed: Vec<Option<(Vec<u8>, usize)>> = crate::util::par_map(&chapters, |&i| fix(&entries[i], media, &css, em_px));
    drop(css);
    for (i, f) in chapters.into_iter().zip(fixed) {
        if let Some((data, n)) = f {
            entries[i].data = data;
            rep.kindle_rule_edits += n;
        }
    }
}

/// 写的数和单位：`1.5em` → (1.5, "em")；不是一个长度的 `None`。
fn number_unit(v: &str) -> Option<(f64, String)> {
    let v = v.trim().trim_end_matches("!important").trim().to_ascii_lowercase();
    let split = v.find(|c: char| !(c.is_ascii_digit() || matches!(c, '.' | '-' | '+')))?;
    let n: f64 = v[..split].parse().ok()?;
    Some((n, v[split..].to_string()))
}

fn fmt(v: f64) -> String {
    super::kindle_rules::fmt_num(v)
}

/// 字号的写法在 Move 上是多少（根 em）：em、%、ex 写的数当根 em，px 按屏幕像素；别的（rem、pt、关键字）`None`＝照 CSS。
fn font_on_move(v: &str, em_px: Option<f64>) -> Option<f64> {
    let (n, unit) = number_unit(v)?;
    match unit.as_str() {
        "em" => Some(n),
        "%" => Some(n / 100.0),
        "ex" => Some(n * 0.5),
        "px" => em_px.map(|e| n / e),
        _ => None,
    }
}

/// `<body>` 上要接的字号（`<body>` 不在元素表里；写 px 的书：《狼厅》`body{font-size:16px}`，Move 上正文只有四成大）。
fn body_font(ch: &Chapter, em_px: Option<f64>) -> Option<String> {
    let fs = ch.body_comp.font_size;
    let m = font_on_move(ch.body_decls.get("font-size")?, em_px)?;
    ((m - fs).abs() > EPS).then(|| format!("font-size:{}em", fmt(fs)))
}

/// 一章要接的声明：第几个开始标签 → 声明。
fn plan(ch: &Chapter, em_px: Option<f64>) -> Vec<(usize, Vec<String>)> {
    let mut out = Vec::new();
    // 在标题里（含标题自己）
    let mut in_heading: Vec<bool> = Vec::with_capacity(ch.els.len());
    for (i, el) in ch.els.iter().enumerate() {
        let tag = km::reader_tag(&ch.el(i));
        let h = matches!(tag.as_str(), "h1" | "h2" | "h3" | "h4" | "h5" | "h6") || el.parent.is_some_and(|p| in_heading[p]);
        in_heading.push(h);
        if h || el.hidden {
            continue;
        }
        let fs = el.comp.font_size;
        let mut ds = Vec::new();
        if el.decls.get("font-size").and_then(|v| font_on_move(v, em_px)).is_some_and(|m| (m - fs).abs() > EPS) {
            ds.push(format!("font-size:{}em", fmt(fs)));
        }
        if (fs - 1.0).abs() > EPS {
            for prop in ["margin-top", "margin-right", "margin-bottom", "margin-left", "padding-top", "padding-right", "padding-bottom", "padding-left", "text-indent"] {
                if let Some((n, unit)) = el.decls.get(prop).and_then(|v| number_unit(v)) {
                    if unit == "em" && n != 0.0 {
                        ds.push(format!("{prop}:{}em", fmt(n * fs)));
                    }
                }
            }
        }
        if !ds.is_empty() {
            out.push((i, ds));
        }
    }
    out
}

fn fix(e: &Entry, media: Option<&MediaEnv>, css: &HashMap<&str, std::sync::Arc<crate::cascade::Rules>>, em_px: Option<f64>) -> Option<(Vec<u8>, usize)> {
    let text = std::str::from_utf8(&e.data).ok()?;
    let ch = Chapter::parse(&e.name, text, media, css).filter(|c| c.matches_tags)?;
    let todo = plan(&ch, em_px);
    let body = body_font(&ch, em_px);
    if todo.is_empty() && body.is_none() {
        return None;
    }
    let (bs, be) = html::body_range(text)?;
    let starts: Vec<html::Tag> = html::tags_in(text, bs, be).filter(|t| t.is_start()).collect();
    let mut edits = Vec::new();
    let mut n = 0;
    let body_tag = html::tags(text).find(|t| t.is_start() && t.is("body"));
    let todo = body_tag.iter().zip(body).map(|(t, d)| (*t, vec![d])).chain(todo.into_iter().filter_map(|(i, ds)| Some((*starts.get(i)?, ds)))).collect::<Vec<_>>();
    for (t, ds) in todo {
        let tag = &text[t.start..t.end];
        let add: String = ds.iter().map(|d| format!("{d};")).collect();
        let style = match html::attr_value(tag, "style") {
            Some(s) if !s.trim().is_empty() => format!("{};{add}", s.trim_end().trim_end_matches(';')),
            _ => add,
        };
        n += ds.len();
        edits.push((t.start, t.end, html::set_attr(tag, "style", &style)));
    }
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

    /// 嵌套的 em 字号写成按根的；字号不是 1 的块上 em 边距、缩进乘字号；不写字号继承的、字号 1 的、px、标题和标题里的不动；不按根的阅读器不改。
    #[test]
    fn em_rebased_to_root() {
        let body = r#"<div class="a"><p class="b">甲</p><p>乙</p><p class="m">丙</p></div><p class="m">丁</p><p class="big m">戊</p><h2 class="m">标题<span class="b">字</span></h2><p class="px">己</p>"#;
        let css = "div.a{font-size:2em;} .b{font-size:0.5em;} .m{margin-top:1em;text-indent:2em;padding-left:3px;} .big{font-size:150%;} .px{font-size:16px;}";
        let mut b = book(body, css);
        apply(&mut b, &mut WashReport::default(), None, true, Some(37.8));
        let page = String::from_utf8(b[2].data.clone()).unwrap();
        assert!(
            page.contains(r#"<div class="a"><p class="b" style="font-size:1em;">甲</p><p>乙</p><p class="m" style="margin-top:2em;text-indent:4em;">丙</p></div><p class="m">丁</p><p class="big m" style="margin-top:1.5em;text-indent:3em;">戊</p><h2 class="m">标题<span class="b">字</span></h2><p class="px" style="font-size:1em;">己</p>"#),
            "{page}"
        );
        let mut b = book(body, css);
        apply(&mut b, &mut WashReport::default(), None, false, Some(37.8));
        assert!(String::from_utf8(b[2].data.clone()).unwrap().contains(body));
        // `<body>` 上写 px 的（《狼厅》）
        let mut b = book("<p>甲</p>", "body{font-size:16px;}");
        apply(&mut b, &mut WashReport::default(), None, true, Some(37.8));
        assert!(String::from_utf8(b[2].data.clone()).unwrap().contains(r#"<body style="font-size:1em;"><p>甲</p>"#));
    }
}
