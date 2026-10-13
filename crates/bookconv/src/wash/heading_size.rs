//! 标题字号照 Kindle 写成阅读器认的关键字（标题的 `font-size` 只认关键字的阅读器：Move，profile `heading_font_keywords`；只修复、照
//! Send to Kindle 统一的文字书；2026-10-13 用户要做）。
//!
//! xochitl 的 h1–h6 不认 em、px、pt、%、`font` 简写（类规则、行内、`!important`、标题里的 `<span>`/`<div>`/`<p>` 写的都不认），
//! 固定按标签排；只认几个关键字，而且是绝对大小（不随外层字号变）。2026-10-13 测试书「测试-标题字号」1–4 量的（以 `medium`＝正文
//! 1em 为 1）：`small` 0.784、`medium` 1、`large` 1.216、`x-large` 1.514、`xx-large` 2.018；`xx-small`、`x-small`、`xxx-large`、
//! `smaller`、`larger` 不认（退回标签的）；标签缺省 h1 2.018、h2 1.514、h3 1.216、h4 1、h5 0.784、h6 1。Kindle 照 CSS 排（字号按正文
//! 归一）：《平凡的世界》章名写 2em 的 h2，Kindle 上 2em、Move 上 1.5em。
//!
//! 这里给每个标题取它在 Kindle 上的字号（标题里的字按所在元素的计算字号计字数，取最多的；`kindle_rules` 已经照正文归一过，根是
//! 阅读器的正文字号），挑比例最接近的关键字，和 Move 上本来的（写了认的关键字照写的，没写照标签的）不同时，在行内 `style` 末尾接
//! `font-size:关键字`（行内的压过类规则，Move 认）。
use super::*;
use crate::cascade::MediaEnv;
use crate::kindle_model::{self as km, Chapter};

/// 标题认的关键字和量出来的大小（正文 1em＝1）。
const KEYWORDS: &[(&str, f64)] = &[("small", 0.784), ("medium", 1.0), ("large", 1.216), ("x-large", 1.514), ("xx-large", 2.018)];

/// 关键字在 Move 标题上的大小（不认的 `None`）。
pub(super) fn keyword_size(v: &str) -> Option<f64> {
    let v = v.trim().trim_end_matches("!important").trim().to_ascii_lowercase();
    KEYWORDS.iter().find(|(k, _)| *k == v).map(|(_, s)| *s)
}

/// 标签在 Move 上的缺省字号（不是标题的 `None`）。
pub(super) fn tag_size(tag: &str) -> Option<f64> {
    Some(match tag {
        "h1" => 2.018,
        "h2" => 1.514,
        "h3" => 1.216,
        "h4" | "h6" => 1.0,
        "h5" => 0.784,
        _ => return None,
    })
}

/// 和 `want` 比例最接近的关键字。
fn nearest(want: f64) -> (&'static str, f64) {
    *KEYWORDS.iter().min_by(|a, b| (a.1 / want).ln().abs().total_cmp(&(b.1 / want).ln().abs())).unwrap()
}

pub(super) fn apply(entries: &mut [Entry], rep: &mut WashReport, media: Option<&MediaEnv>) {
    let css = km::book_css(entries, media);
    let chapters: Vec<usize> = (0..entries.len()).filter(|&i| is_chapter_entry(&entries[i])).collect();
    let plans: Vec<Vec<(usize, &'static str)>> = crate::util::par_map(&chapters, |&i| plan(&entries[i], media, &css));
    drop(css);
    for (&i, plan) in chapters.iter().zip(plans) {
        if plan.is_empty() {
            continue;
        }
        let e = &mut entries[i];
        let Ok(text) = std::str::from_utf8(&e.data) else { continue };
        let Some((bs, be)) = html::body_range(text) else { continue };
        let starts: Vec<html::Tag> = html::tags_in(text, bs, be).filter(|t| t.is_start()).collect();
        let mut edits = Vec::new();
        for (n, kw) in plan {
            let Some(t) = starts.get(n) else { continue };
            let raw = &text[t.start..t.end];
            let style = match html::attr_value(raw, "style") {
                Some(s) if !s.trim().is_empty() => format!("{};font-size:{kw};", s.trim_end().trim_end_matches(';')),
                _ => format!("font-size:{kw};"),
            };
            edits.push((t.start, t.end, html::set_attr(raw, "style", &style)));
        }
        rep.kindle_rule_edits += edits.len();
        e.data = html::apply_edits(text, edits).into_bytes();
    }
}

/// 一章里要改的标题：(`<body>` 里第几个开始标签, 关键字)。元素数和标签扫描对不上的整章不管。
fn plan(e: &Entry, media: Option<&MediaEnv>, css: &HashMap<&str, std::sync::Arc<crate::cascade::Rules>>) -> Vec<(usize, &'static str)> {
    let Ok(text) = std::str::from_utf8(&e.data) else { return Vec::new() };
    let Some(ch) = Chapter::parse(&e.name, text, media, css).filter(|c| c.matches_tags) else { return Vec::new() };
    // 标题：(第几个开始标签, Move 上本来的大小, 字号 → 字数)
    struct Heading {
        n: usize,
        on_move: f64,
        sizes: HashMap<i64, usize>,
    }
    let mut heads: Vec<Heading> = Vec::new();
    // 每个元素所在的标题（在 heads 里的下标）
    let mut head_of: Vec<Option<usize>> = Vec::with_capacity(ch.els.len());
    for (i, el) in ch.els.iter().enumerate() {
        // 标题里再套标题（不合法）按外面的算
        let head = match (el.parent.and_then(|p| head_of[p]), tag_size(ch.el(i).value().name())) {
            (None, Some(def)) => {
                let kw = el.decls.get("font-size").and_then(|v| keyword_size(v));
                heads.push(Heading { n: i, on_move: kw.unwrap_or(def), sizes: HashMap::new() });
                Some(heads.len() - 1)
            }
            (h, _) => h,
        };
        head_of.push(head);
        // 标题里的字按所在元素的计算字号计
        if let (Some(h), false) = (head, el.hidden) {
            let k = ch.own_chars(i);
            if k > 0 {
                *heads[h].sizes.entry(km::key(el.comp.font_size)).or_default() += k;
            }
        }
    }
    heads
        .into_iter()
        .filter_map(|h| {
            let want = km::weighted_mode(&h.sizes)?;
            let (kw, size) = nearest(want);
            ((size - h.on_move).abs() > 1e-6).then_some((h.n, kw))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(page: &str, css: &str) -> String {
        let mut b = vec![
            Entry { name: "O/content.opf".into(), data: br#"<package><manifest><item id="c" href="c.xhtml" media-type="application/xhtml+xml"/></manifest><spine><itemref idref="c"/></spine></package>"#.to_vec() },
            Entry { name: "O/s.css".into(), data: css.as_bytes().to_vec() },
            Entry { name: "O/c.xhtml".into(), data: format!(r#"<html xmlns="http://www.w3.org/1999/xhtml"><head><title>t</title><link rel="stylesheet" href="s.css"/></head><body>{page}</body></html>"#).into_bytes() },
        ];
        apply(&mut b, &mut WashReport::default(), None);
        String::from_utf8(b[2].data.clone()).unwrap()
    }

    /// 写 2em 的 h2 → xx-large；缺省的 h2（1.5em）不动；h3 写 1em → medium；字在 span 里按 span 的字号；写了认的关键字、和要的一样的不动；
    /// 行内 `style` 后面接；h6 缺省 0.67em（Move 上 1）→ small。
    #[test]
    fn headings_get_nearest_keyword() {
        let out = run(
            r#"<h2 class="big">章</h2><h2>节</h2><h3 class="one" style="color:red">小节</h3><h1><span class="s">标题</span></h1><h4 class="kw">四</h4><h6>六</h6><p class="big">正文</p>"#,
            "h1{font-size:2em} h2{font-size:1.5em} h3{font-size:1.17em} h4{font-size:1em} h6{font-size:0.67em} .big{font-size:2em} h3.one{font-size:1em} span.s{font-size:0.6em} h4.kw{font-size:large}",
        );
        assert!(out.contains(r#"<h2 class="big" style="font-size:xx-large;">章</h2><h2>节</h2><h3 class="one" style="color:red;font-size:medium;">小节</h3>"#), "{out}");
        // span 0.6em × 2em = 1.2em → large
        assert!(out.contains(r#"<h1 style="font-size:large;"><span class="s">标题</span></h1><h4 class="kw">四</h4><h6 style="font-size:small;">六</h6><p class="big">正文</p>"#), "{out}");
    }
}
