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
use crate::cascade::{Computed, MediaEnv, Sheet};
use scraper::{ElementRef, Html, Node};
use std::rc::Rc;

/// 没写 `line-height` 时 Kindle 当作的倍数（同写出器的 `LH_EM`）。
const LH_EM: f64 = 1.2;
/// 归一后的下限（同写出器的 `MIN_LINE_HEIGHT`）。
const MIN_LH: f64 = 0.6;

pub(super) fn apply(entries: &mut [Entry], rep: &mut WashReport, media: Option<&MediaEnv>) {
    let css = super::top_spacer::parse_css(entries, media);
    let chapters: Vec<usize> = (0..entries.len()).filter(|&i| is_chapter_entry(&entries[i])).collect();
    let docs: Vec<Vec<Item>> = crate::util::par_map(&chapters, |&i| items(&entries[i], media, &css));
    drop(css);
    let mut count: HashMap<i64, usize> = HashMap::new();
    for it in docs.iter().flatten() {
        if it.chars > 0 {
            *count.entry((it.lh.unwrap_or(LH_EM) * 1000.0).round() as i64).or_default() += it.chars;
        }
    }
    let base = crate::cascade::weighted_mode(&count).filter(|v| (1.0..=3.0).contains(v)).unwrap_or(LH_EM);
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
    let Some((bs, be)) = html::body_range(text) else { return Vec::new() };
    let tag_count = html::tags_in(text, bs, be).filter(|t| t.is_start()).count();
    let html = Html::parse_document(text);
    let sheet = Sheet::for_doc(&html, &e.name, media, |path| css.get(path).cloned());
    let root = html.root_element();
    let Some(body) = root.children().filter_map(ElementRef::wrap).find(|el| el.value().name() == "body") else { return Vec::new() };
    let derive = |el: &ElementRef, parent: &Computed| {
        let mut d = sheet.cascade(el, &e.name);
        crate::cascade::presentational_hints(el, &mut d);
        let own = d.contains_key("line-height");
        (Computed::derive(parent, &d, el.value().name()), own)
    };
    let rc = Rc::new(derive(&root, &Computed::root()).0);
    let bc = Rc::new(derive(&body, &rc).0);
    let mut out: Vec<Item> = Vec::new();
    // (节点, 父元素的计算值, 父元素下标, 所在块的下标, 在不在隐藏的元素里)
    type Frame<'a> = (ego_tree::NodeRef<'a, Node>, Rc<Computed>, Option<usize>, Option<usize>, bool);
    let mut stack: Vec<Frame> = body.children().rev().map(|c| (c, bc.clone(), None, None, false)).collect();
    while let Some((n, parent, pi, block, hidden)) = stack.pop() {
        match n.value() {
            Node::Text(t) => {
                let k = t.chars().filter(|c| !c.is_whitespace()).count();
                if let (Some(b), false) = (block, hidden || k == 0) {
                    out[b].chars += k;
                }
            }
            Node::Element(v) => {
                let Some(el) = ElementRef::wrap(n) else { continue };
                let (c, own) = derive(&el, &parent);
                let hidden = hidden || c.display.as_deref() == Some("none");
                let is_block = crate::cascade::is_block(&el, &c);
                let me = out.len();
                out.push(Item { parent: pi, lh: c.line_height, own, target: is_block && v.name() != "p", chars: 0 });
                let block = if is_block { Some(me) } else { block };
                let c = Rc::new(c);
                for ch in n.children().rev() {
                    stack.push((ch, c.clone(), Some(me), block, hidden));
                }
            }
            _ => {}
        }
    }
    if out.len() != tag_count {
        return Vec::new();
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
            let want = (it.lh.unwrap_or(LH_EM) / base).max(MIN_LH);
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
