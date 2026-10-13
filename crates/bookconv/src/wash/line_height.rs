//! 行距照 Kindle 按全书正文行距归一（只有 `<p>` 不认 `line-height` 的阅读器：Move，profile `line_height_except_p`；只修复、照
//! Send to Kindle 统一的文字书；2026-10-13 用户定）。
//!
//! Kindle（KFX 写出器同 Send to Kindle，`kfx::write::style` 的行高）：行高写成「本元素行高 ÷ 全书正文行高」（没写按 1.2，下限 0.6），
//! 正文就是阅读器的行距设置。xochitl：`<p>` 的 `line-height` 一概不认（总是阅读器的行距），别的块（标题、div、li、blockquote、td，
//! 行内 `style` 也算）按「阅读器缺省行距 × 倍数」排（2026-10-13 测试书「测试-行距」：body `line-height:1.618` 时 `<p>` 一行 56px、
//! div 90px、2em 标题 179px；`line-height:2`、`2em` 的 2em 标题 222px，`normal` 111px）。所以正文行距写了 1.618 的书，Move 上标题、
//! div 的行距比 Kindle 大六成（《三体》标题两行隔 179px，Kindle 上约 123）。
//!
//! 这里给 `<p>` 以外、自己排字的块（直接有字，或者字在行内元素里）在行内 `style` 末尾接 `line-height:倍数`（行内的优先级最高，Move 认）：
//! 倍数照 Kindle 算，和 Move 上本来的（自己写的照写的、继承的照继承来的，`normal` 是 1）差不到 0.01 的不写。
//! 全书正文行距和写出器一样：每段字按所在块的行高（没写 1.2）计字数，取最多的，不在 1–3 之间的用 1.2。
use super::*;
use crate::cascade::MediaEnv;
use crate::kindle_model::{self as km, Chapter, LH_EM};

pub(super) fn apply(entries: &mut [Entry], rep: &mut WashReport, media: Option<&MediaEnv>) {
    let css = km::book_css(entries, media);
    let chapters: Vec<usize> = (0..entries.len()).filter(|&i| is_chapter_entry(&entries[i])).collect();
    let docs: Vec<Vec<Item>> = crate::util::par_map(&chapters, |&i| items(&entries[i], media, &css));
    drop(css);
    let mut count: HashMap<i64, usize> = HashMap::new();
    for it in docs.iter().flatten() {
        if it.chars > 0 {
            *count.entry(km::key(it.lh.unwrap_or(LH_EM))).or_default() += it.chars;
        }
    }
    let base = km::base_line_height(&count);
    for (&i, items) in chapters.iter().zip(&docs) {
        let plan = plan(items, base);
        if plan.is_empty() {
            continue;
        }
        let e = &mut entries[i];
        let Ok(text) = std::str::from_utf8(&e.data) else { continue };
        let Some((bs, be)) = html::body_range(text) else { continue };
        let starts: Vec<html::Tag> = html::tags_in(text, bs, be).filter(|t| t.is_start()).collect();
        let mut edits = Vec::new();
        for (n, v) in plan {
            let Some(t) = starts.get(n) else { continue };
            let raw = &text[t.start..t.end];
            let style = match html::attr_value(raw, "style") {
                Some(s) if !s.trim().is_empty() => format!("{};line-height:{};", s.trim_end().trim_end_matches(';'), fmt(v)),
                _ => format!("line-height:{};", fmt(v)),
            };
            edits.push((t.start, t.end, html::set_attr(raw, "style", &style)));
        }
        rep.kindle_rule_edits += edits.len();
        e.data = html::apply_edits(text, edits).into_bytes();
    }
}

fn fmt(v: f64) -> String {
    let s = format!("{:.4}", v);
    s.trim_end_matches('0').trim_end_matches('.').to_string()
}

/// `<body>` 里的一个元素（文档序，和标签扫描的开始标签一一对应）。
struct Item {
    /// 父元素在列表里的下标（父元素是 `<body>` 的 `None`）。
    parent: Option<usize>,
    /// 计算出的行高（元素字号的倍数，`None`＝没写/normal）。
    lh: Option<f64>,
    /// 自己（含行内 `style`）写了 `line-height`。
    own: bool,
    /// 归一要管的块：块级、不是 `<p>`、自己排字。
    target: bool,
    /// 自己排的字数（不算空白；行内元素里的算给所在的块），统计正文行距用。
    chars: usize,
}

/// 一章的元素表；元素数和标签扫描对不上（解析器补了标签）的整章不管，也不计字数。
fn items(e: &Entry, media: Option<&MediaEnv>, css: &HashMap<&str, std::sync::Arc<crate::cascade::Rules>>) -> Vec<Item> {
    let Ok(text) = std::str::from_utf8(&e.data) else { return Vec::new() };
    let Some(ch) = Chapter::parse(&e.name, text, media, css).filter(|c| c.matches_tags) else { return Vec::new() };
    let mut out: Vec<Item> = (0..ch.els.len())
        .map(|i| {
            let el = &ch.els[i];
            Item { parent: el.parent, lh: el.comp.line_height, own: el.decls.contains_key("line-height"), target: el.is_block && ch.el(i).value().name() != "p", chars: 0 }
        })
        .collect();
    // 字算给所在的块；直接在 `<body>` 里的、隐藏的不算
    for (i, el) in ch.els.iter().enumerate() {
        if let (Some(b), false) = (el.block, el.hidden) {
            out[b].chars += ch.own_chars(i);
        }
    }
    out
}

/// 要改的：(第几个开始标签, 行距倍数)。文档序走一遍，记下每个元素在 Move 上的行距（改过的往下继承改后的值）。
fn plan(items: &[Item], base: f64) -> Vec<(usize, f64)> {
    let mut on_move: Vec<f64> = Vec::with_capacity(items.len());
    let mut out = Vec::new();
    for (i, it) in items.iter().enumerate() {
        // 没自己写的跟父元素：父元素改过就是改后的值，没改过就是原来算出的（数字倍数照继承、长度按绝对值继承，同 CSS）
        let inherited = it.parent.map(|p| (on_move[p], items[p].lh));
        let mut cur = match inherited {
            Some((pm, plh)) if !it.own && (pm - plh.unwrap_or(1.0)).abs() > 1e-9 => pm,
            _ => it.lh.unwrap_or(1.0),
        };
        if it.target && it.chars > 0 {
            let want = km::line_height_ratio(it.lh, base);
            let want = (want * 10000.0).round() / 10000.0;
            if (want - cur).abs() > 0.01 {
                out.push((i, want));
                cur = want;
            }
        }
        on_move.push(cur);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn book(page: &str, css: &str) -> Vec<Entry> {
        vec![
            Entry { name: "O/content.opf".into(), data: br#"<package><manifest><item id="c" href="c.xhtml" media-type="application/xhtml+xml"/></manifest><spine><itemref idref="c"/></spine></package>"#.to_vec() },
            Entry { name: "O/s.css".into(), data: css.as_bytes().to_vec() },
            Entry { name: "O/c.xhtml".into(), data: format!(r#"<html xmlns="http://www.w3.org/1999/xhtml"><head><title>t</title><link rel="stylesheet" href="s.css"/></head><body>{page}</body></html>"#).into_bytes() },
        ]
    }

    fn run(page: &str, css: &str) -> String {
        let mut b = book(page, css);
        apply(&mut b, &mut WashReport::default(), None);
        String::from_utf8(b[2].data.clone()).unwrap()
    }

    /// 正文 1.618（body 上写、段落继承）：标题、div 归一成 1（Move 上本来是 1.618），`<p>` 不动；标题自己写了 1em 的 → 1/1.618；
    /// 行内 `style` 后面接；套着块、自己不排字的 div 不动，里面的块照算；隐藏的不算正文。
    #[test]
    fn blocks_other_than_p_follow_kindle() {
        let p = "<p>正文正文正文正文正文正文</p>".repeat(3);
        let out = run(&format!(r#"<h1><span>标题</span></h1>{p}<div>说明</div><h2 class="t" style="color:red">小标题</h2><div class="w"><h3>节</h3><p>正文</p></div>"#), "body{line-height:1.618} h2.t{line-height:1em}");
        assert!(out.contains(r#"<h1 style="line-height:1;"><span>标题</span></h1>"#), "{out}");
        assert!(out.contains(r#"<p>正文正文正文正文正文正文</p><div style="line-height:1;">说明</div><h2 class="t" style="color:red;line-height:0.618;">小标题</h2><div class="w"><h3 style="line-height:1;">节</h3><p>正文</p></div>"#), "{out}");
    }

    /// 没写行距的书（正文按 1.2）：Move 上本来就是 1，不改；正文 1.5em、div 没写：Kindle 1.2/1.5 = 0.8；写成 1.5em 的 div 是 1。
    /// 改过的块里套着的块按改后的值继承。
    #[test]
    fn unwritten_and_inherited() {
        let p = "<p>正文正文正文正文正文正文</p>".repeat(3);
        let page = format!(r#"<div>说明</div>{p}<h1>标题</h1>"#);
        assert_eq!(run(&page, "p{margin:0}"), String::from_utf8(book(&page, "p{margin:0}")[2].data.clone()).unwrap());
        let out = run(&format!(r#"<div>说明</div>{p}<div class="e">一<div>二</div></div>"#), "p{line-height:1.5em} div.e{line-height:1.5em}");
        assert!(out.contains(r#"<div style="line-height:0.8;">说明</div>"#), "{out}");
        // div.e：Kindle 1.5/1.5 = 1，Move 上本来 1.5 → 写 1；里面的 div 继承 1.5em（长度按绝对值继承，Kindle 照样 1），Move 上继承改后的 1，不再写
        assert!(out.contains(r#"<div class="e" style="line-height:1;">一<div>二</div></div>"#), "{out}");
    }
}
