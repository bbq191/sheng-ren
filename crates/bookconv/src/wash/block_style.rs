//! 块的行距、标题字号照 Kindle（只修复、照 Send to Kindle 统一的文字书；2026-10-13 用户定）：Kindle 上的值按
//! [`crate::kindle_model`] 算，这台阅读器拿到原来的 CSS 排成多少按 [`ReaderModel`] 算，不一样的在行内 `style` 末尾接上这台认的写法
//! （行内的优先级最高）。一章只解析一遍，两样一起改。2026-10-13 由 `line_height.rs`、`heading_size.rs` 合成。
//!
//! **行距**（只有 `<p>` 不认 `line-height` 的阅读器：Move，profile `line_height_except_p`）。Kindle（KFX 写出器同 Send to Kindle，
//! `kfx::write::style` 的行高）：行高写成「本元素行高 ÷ 全书正文行高」（没写按 1.2，下限 0.6），正文就是阅读器的行距设置。
//! xochitl：`<p>` 的 `line-height` 一概不认（总是阅读器的行距），别的块（标题、div、li、blockquote、td，行内 `style` 也算）按
//! 「阅读器缺省行距 × 倍数」排（2026-10-13 测试书「测试-行距」：body `line-height:1.618` 时 `<p>` 一行 56px、div 90px、2em 标题
//! 179px；`line-height:2`、`2em` 的 2em 标题 222px，`normal` 111px）。所以正文行距写了 1.618 的书，Move 上标题、div 的行距比 Kindle
//! 大六成（《三体》标题两行隔 179px，Kindle 上约 123）。这里给 `<p>` 以外、自己排字的块（直接有字，或者字在行内元素里）接
//! `line-height:倍数`：倍数照 Kindle 算，和这台上本来的（自己写的照写的、继承的照继承来的，`normal` 是 1）差不到 0.01 的不写。
//! 全书正文行距和写出器一样（[`km::base_line_height`]）。
//!
//! **标题字号**（标题的 `font-size` 只认关键字的阅读器：Move，profile `heading_font_keywords` + `[heading_sizes]`）。xochitl 的
//! h1–h6 不认 em、px、pt、%、`font` 简写（类规则、行内、`!important`、标题里的 `<span>`/`<div>`/`<p>` 写的都不认），固定按标签排；
//! 只认几个关键字，而且是绝对大小（不随外层字号变；2026-10-13 测试书「测试-标题字号」1–4，量出来的大小在 profile 里）。Kindle 照
//! CSS 排（字号按正文归一）：《平凡的世界》章名写 2em 的 h2，Kindle 上 2em、Move 上 1.5em。这里给每个标题取它在 Kindle 上的字号
//! （标题里的字按所在元素的计算字号计字数，取最多的；`kindle_rules` 已经照正文归一过，根是阅读器的正文字号），挑比例最接近的
//! 关键字，和这台上本来的（写了认的关键字照写的，没写照标签的）不同时接 `font-size:关键字`。
//!
//! 同一个标签两样都要改时先接行距、再接字号。
use super::*;
use crate::cascade::MediaEnv;
use crate::kindle_model::{self as km, Chapter, LH_EM};
use crate::reader_model::{HeadingSizes, ReaderModel};
use std::collections::BTreeMap;

pub(super) fn apply(entries: &mut [Entry], rep: &mut WashReport, media: Option<&MediaEnv>, reader: &ReaderModel) {
    let heads = reader.heading_sizes.as_ref();
    if !reader.line_height_except_p && heads.is_none() {
        return;
    }
    let css = km::book_css(entries, media);
    let chapters: Vec<usize> = (0..entries.len()).filter(|&i| is_chapter_entry(&entries[i])).collect();
    let docs: Vec<Doc> = crate::util::par_map(&chapters, |&i| scan(&entries[i], media, &css, heads));
    drop(css);
    let base = {
        let mut count: HashMap<i64, usize> = HashMap::new();
        for it in docs.iter().flat_map(|d| &d.items) {
            if it.chars > 0 {
                *count.entry(km::key(it.lh.unwrap_or(LH_EM))).or_default() += it.chars;
            }
        }
        km::base_line_height(&count)
    };
    for (&i, doc) in chapters.iter().zip(&docs) {
        // 第几个开始标签 → 要接的声明（先行距、后字号）
        let mut decls: BTreeMap<usize, Vec<String>> = BTreeMap::new();
        if reader.line_height_except_p {
            for (n, v) in line_heights(&doc.items, base) {
                decls.entry(n).or_default().push(format!("line-height:{}", fmt(v)));
            }
        }
        for (n, kw) in &doc.heading_sizes {
            decls.entry(*n).or_default().push(format!("font-size:{kw}"));
        }
        if decls.is_empty() {
            continue;
        }
        let e = &mut entries[i];
        let Ok(text) = std::str::from_utf8(&e.data) else { continue };
        let Some((bs, be)) = html::body_range(text) else { continue };
        let starts: Vec<html::Tag> = html::tags_in(text, bs, be).filter(|t| t.is_start()).collect();
        let mut edits = Vec::new();
        for (n, ds) in decls {
            let Some(t) = starts.get(n) else { continue };
            let mut tag = text[t.start..t.end].to_string();
            for d in &ds {
                let style = match html::attr_value(&tag, "style") {
                    Some(s) if !s.trim().is_empty() => format!("{};{d};", s.trim_end().trim_end_matches(';')),
                    _ => format!("{d};"),
                };
                tag = html::set_attr(&tag, "style", &style);
            }
            rep.kindle_rule_edits += ds.len();
            edits.push((t.start, t.end, tag));
        }
        e.data = html::apply_edits(text, edits).into_bytes();
    }
}

fn fmt(v: f64) -> String {
    let s = format!("{:.4}", v);
    s.trim_end_matches('0').trim_end_matches('.').to_string()
}

/// 一章扫出来的：元素表（行距用）、要改字号的标题。
#[derive(Default)]
struct Doc {
    items: Vec<Item>,
    /// (第几个开始标签, 关键字)。
    heading_sizes: Vec<(usize, String)>,
}

/// `<body>` 里的一个元素（文档序，和标签扫描的开始标签一一对应）。
struct Item {
    /// 父元素在列表里的下标（父元素是 `<body>` 的 `None`）。
    parent: Option<usize>,
    /// 计算出的行高（元素字号的倍数，`None`＝没写/normal）。
    lh: Option<f64>,
    /// 自己（含行内 `style`）写了 `line-height`。
    own: bool,
    /// 行距要管的块：块级、不是 `<p>`、自己排字。
    target: bool,
    /// 自己排的字数（不算空白；行内元素里的算给所在的块），统计正文行距用。
    chars: usize,
}

/// 扫一章；元素数和标签扫描对不上（解析器补了标签）的整章不管，也不计字数。
fn scan(e: &Entry, media: Option<&MediaEnv>, css: &HashMap<&str, std::sync::Arc<crate::cascade::Rules>>, heads: Option<&HeadingSizes>) -> Doc {
    let Ok(text) = std::str::from_utf8(&e.data) else { return Doc::default() };
    let Some(ch) = Chapter::parse(&e.name, text, media, css).filter(|c| c.matches_tags) else { return Doc::default() };
    let mut items: Vec<Item> = (0..ch.els.len())
        .map(|i| {
            let el = &ch.els[i];
            Item { parent: el.parent, lh: el.comp.line_height, own: el.decls.contains_key("line-height"), target: el.is_block && ch.el(i).value().name() != "p", chars: 0 }
        })
        .collect();
    // 字算给所在的块；直接在 `<body>` 里的、隐藏的不算
    for (i, el) in ch.els.iter().enumerate() {
        if let (Some(b), false) = (el.block, el.hidden) {
            items[b].chars += ch.own_chars(i);
        }
    }
    let heading_sizes = heads.map(|h| heading_sizes(&ch, h)).unwrap_or_default();
    Doc { items, heading_sizes }
}

/// 要改行距的：(第几个开始标签, 行距倍数)。文档序走一遍，记下每个元素在这台上的行距（改过的往下继承改后的值）。
fn line_heights(items: &[Item], base: f64) -> Vec<(usize, f64)> {
    let mut on_reader: Vec<f64> = Vec::with_capacity(items.len());
    let mut out = Vec::new();
    for (i, it) in items.iter().enumerate() {
        // 没自己写的跟父元素：父元素改过就是改后的值，没改过就是原来算出的（数字倍数照继承、长度按绝对值继承，同 CSS）
        let inherited = it.parent.map(|p| (on_reader[p], items[p].lh));
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
        on_reader.push(cur);
    }
    out
}

/// 要改字号的标题：(第几个开始标签, 关键字)。
fn heading_sizes(ch: &Chapter, h: &HeadingSizes) -> Vec<(usize, String)> {
    // 标题：(第几个开始标签, 这台上本来的大小, Kindle 上的字号 → 字数)
    struct Heading {
        n: usize,
        on_reader: f64,
        sizes: HashMap<i64, usize>,
    }
    let mut heads: Vec<Heading> = Vec::new();
    // 每个元素所在的标题（在 heads 里的下标）
    let mut head_of: Vec<Option<usize>> = Vec::with_capacity(ch.els.len());
    for (i, el) in ch.els.iter().enumerate() {
        let tag = ch.el(i).value().name();
        // 标题里再套标题（不合法）按外面的算
        let head = match el.parent.and_then(|p| head_of[p]) {
            None if h.tag_size(tag).is_some() => {
                let on_reader = h.on_reader(tag, el.decls.get("font-size").map(String::as_str)).expect("是标题");
                heads.push(Heading { n: i, on_reader, sizes: HashMap::new() });
                Some(heads.len() - 1)
            }
            other => other,
        };
        head_of.push(head);
        // 标题里的字按所在元素的计算字号计
        if let (Some(hd), false) = (head, el.hidden) {
            let k = ch.own_chars(i);
            if k > 0 {
                *heads[hd].sizes.entry(km::key(el.comp.font_size)).or_default() += k;
            }
        }
    }
    heads
        .into_iter()
        .filter_map(|hd| {
            let want = km::weighted_mode(&hd.sizes)?;
            let (kw, size) = h.nearest(want);
            ((size - hd.on_reader).abs() > 1e-6).then(|| (hd.n, kw.to_string()))
        })
        .collect()
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

    fn move_reader() -> ReaderModel {
        ReaderModel::for_profile(profile::get("xochitl").unwrap())
    }

    fn run_with(page: &str, css: &str, reader: &ReaderModel) -> String {
        let mut b = book(page, css);
        apply(&mut b, &mut WashReport::default(), None, reader);
        String::from_utf8(b[2].data.clone()).unwrap()
    }

    /// 只看行距：标题字号不管的阅读器。
    fn run(page: &str, css: &str) -> String {
        run_with(page, css, &ReaderModel { heading_sizes: None, ..move_reader() })
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

    /// 写 2em 的 h2 → xx-large；缺省的 h2（1.5em）不动；h3 写 1em → medium；字在 span 里按 span 的字号；写了认的关键字、和要的一样的不动；
    /// 行内 `style` 后面接；h6 缺省 0.67em（Move 上 1）→ small。行距不管的阅读器只改字号。
    #[test]
    fn headings_get_nearest_keyword() {
        let reader = ReaderModel { line_height_except_p: false, ..move_reader() };
        let out = run_with(
            r#"<h2 class="big">章</h2><h2>节</h2><h3 class="one" style="color:red">小节</h3><h1><span class="s">标题</span></h1><h4 class="kw">四</h4><h6>六</h6><p class="big">正文</p>"#,
            "h1{font-size:2em} h2{font-size:1.5em} h3{font-size:1.17em} h4{font-size:1em} h6{font-size:0.67em} .big{font-size:2em} h3.one{font-size:1em} span.s{font-size:0.6em} h4.kw{font-size:large}",
            &reader,
        );
        assert!(out.contains(r#"<h2 class="big" style="font-size:xx-large;">章</h2><h2>节</h2><h3 class="one" style="color:red;font-size:medium;">小节</h3>"#), "{out}");
        // span 0.6em × 2em = 1.2em → large
        assert!(out.contains(r#"<h1 style="font-size:large;"><span class="s">标题</span></h1><h4 class="kw">四</h4><h6 style="font-size:small;">六</h6><p class="big">正文</p>"#), "{out}");
    }

    /// 两样都要改的标签：先行距、后字号；照 CSS 排的阅读器什么都不改。
    #[test]
    fn both_on_one_tag_and_css_reader_untouched() {
        let p = "<p>正文正文正文正文正文正文</p>".repeat(3);
        let page = format!(r#"<h2 style="color:red">章</h2>{p}"#);
        let css = "body{line-height:1.618} h2{font-size:2em}";
        assert!(run_with(&page, css, &move_reader()).contains(r#"<h2 style="color:red;line-height:1;font-size:xx-large;">章</h2>"#));
        assert_eq!(run_with(&page, css, &ReaderModel::default()), String::from_utf8(book(&page, css)[2].data.clone()).unwrap());
    }
}
