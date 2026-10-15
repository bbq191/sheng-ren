//! 只有空白的段落照 Kindle 折成下一块的上边距，掌阅、Move 照样做（只修复、照 Send to Kindle 统一的文字书；2026-10-13 用户定）。
//!
//! Kindle（Send to Kindle 同样，KFX 写出器 12 照做，见 `kindle_model::BLANK_LINE_FOLD`）：`<p>&nbsp;</p>`、`<p>　</p>`、
//! `<p><br/></p>`、块之间单独的 `<br/>` 不出节点，自己的上下外边距跟着没了；下一块的上外边距照常和前一块的下外边距折叠以后，再加
//! 0.6 × 空段自己的行高（`line-height` 倍数 × 字号，没写按 1.2）。掌阅、Move 照 CSS 排出一整行（2026-10-13 测试书「测试-空段」：
//! 上下 1em 的两段之间夹一个 `&nbsp;` 空段，掌阅多 38px、Move 多 88px；`<p><br/></p>` 掌阅多 97px、Move 排成两行多 138px；
//! 全角空格的空段 Move 当空白吞掉、一点不空）。《三体》的 `<p><br/></p>` 有 585 处。
//!
//! 这里把空段藏起来：认 `display:none` 的（掌阅）加类 `eink-blank`；不认的（Move：`display`、`height`、`max-height`、`font-size`、
//! `float` 都试过不管用）把元素去掉（用户定：里面只有看不见的空白，Kindle 上也没有）。接住的那一块加类 `eink-gN`，上外边距写成
//! Kindle 上折叠后的总距离（`em`，之后照竖直间距换算）；两台都折叠外边距（实测），总距离不小于前一块的下外边距，折叠后就是它。
//! 文件末尾的空段 Kindle 上没有下一块（不折），只藏起来。拿不准的不动：空段里有 `id`、负外边距、接住的块有上内边距或上边框又
//! 套着块、空段后面同一层没有块而文件后面还有内容。
//!
//! 块末尾的 `<br/>`（`<h1>标题<br/></h1>`，后面只剩空格、换行）Kindle 也不排（写出器去掉块文字末尾的换行），掌阅不排（《三体》
//! 获奖感言标题下三台对过），Move 每个多排一行（2026-10-13 测试书「测试-末尾br」：段落多 50px、1.5em 标题多 75px，套在 `<span>`
//! 里的一样）：Move 上一并去掉。
use super::*;
use crate::cascade::{Len, MediaEnv, Rules};
use crate::kindle_model::{self as km, Chapter, BLANK_LINE_FOLD, LH_EM};
use scraper::{ElementRef, Node};

const CSS_NAME: &str = "eink-blank.css";
const BLANK: &str = "eink-blank";
/// 藏起空段的写法（掌阅认；Move 不认，元素直接去掉）。
const HIDE: &str = "display:none;";

/// 一章里要改的：空段（`<body>` 里第几个开始标签），接住的块（第几个开始标签, 上外边距 em）。
#[derive(Default, Debug, PartialEq)]
struct Plan {
    blanks: Vec<usize>,
    gaps: Vec<(usize, f64)>,
    /// 块末尾的 `<br/>`（只在去掉空段的阅读器上去掉）。
    trailing: Vec<usize>,
}

/// `remove`：阅读器不认 `display:none`（profile `display_none = false`），空段元素去掉；`trailing`：阅读器把块末尾的 `<br/>`
/// 多排一行（profile `trailing_br_line`），去掉。
pub(super) fn apply(entries: &mut Vec<Entry>, rep: &mut WashReport, media: Option<&MediaEnv>, remove: bool, trailing: bool) {
    let (chapters, plans) = super::scan_chapters(entries, media, |e, css| plan(e, media, css));
    if plans.iter().all(|p| p.blanks.is_empty() && (!trailing || p.trailing.is_empty())) {
        return;
    }
    let css_path = super::beside_opf(entries, CSS_NAME);
    // 三段（2026-10-15 改，以前写回整段串行）：① 各章并行算好编辑（[`resolve`]，只读；折成上边距的那几处等编号）；
    // ② 串行按「章的顺序、章里的顺序」给 (标签, 上外边距) 编类号——和以前逐章做时的先后一样，产物逐字节不变；③ 各章并行改写。
    let resolved: Vec<Option<Resolved>> = crate::util::par_map(&chapters.iter().zip(&plans).collect::<Vec<_>>(), |&(&i, p)| {
        (!(p.blanks.is_empty() && (!trailing || p.trailing.is_empty()))).then(|| resolve(&entries[i], p, remove, trailing)).flatten()
    });
    // 同样的 (标签, 上外边距) 共用一个类
    let mut classes: Vec<(String, String)> = Vec::new();
    let mut blank_tags: std::collections::BTreeSet<String> = Default::default();
    let mut jobs: HashMap<usize, (Resolved, Vec<usize>)> = HashMap::new();
    for (&i, r) in chapters.iter().zip(resolved) {
        let Some(r) = r else { continue };
        blank_tags.extend(r.blank_tags.iter().cloned());
        let ks: Vec<usize> = r
            .edits
            .iter()
            .filter_map(|(_, _, ed)| match ed {
                Pending::Gap(key) => Some(classes.iter().position(|c| c == key).unwrap_or_else(|| {
                    classes.push(key.clone());
                    classes.len() - 1
                })),
                Pending::Done(_) => None,
            })
            .collect();
        jobs.insert(i, (r, ks));
    }
    let mut work: Vec<(&mut Entry, (Resolved, Vec<usize>))> = entries.iter_mut().enumerate().filter_map(|(i, e)| jobs.remove(&i).map(|j| (e, j))).collect();
    rep.kindle_rule_edits += crate::util::par_map_mut(&mut work, |(e, (r, ks))| rewrite(e, r, ks, &css_path)).into_iter().sum::<usize>();
    let mut sheet = String::from("/* 只有空白的段落照 Kindle 折成下一块的上边距（sheng-ren kindle_rules，见 wash/blank_fold.rs） */\n");
    for t in blank_tags.iter().filter(|_| !remove) {
        sheet.push_str(&format!("html body {t}.{BLANK}{{{HIDE}}}\n"));
    }
    for (k, (t, v)) in classes.iter().enumerate() {
        sheet.push_str(&format!("html body {t}.eink-g{}{{margin-top:{v};}}\n", k + 1));
    }
    add_css_entry(entries, find_opf(entries), &css_path, &sheet, "eink-blank-css");
}

/// 一处编辑的新内容：定好了的，或者折成上边距、要等类号的（键是 (标签, 上外边距)，原标签就是 `text[起..止]`）。
enum Pending {
    Done(String),
    Gap((String, String)),
}

/// 一章算好的编辑（[`resolve`]）：(起, 止, 新内容)，按以前的先后（空段、块末尾的 `<br/>`、折成的上边距）；用到的空段标签。
struct Resolved {
    edits: Vec<(usize, usize, Pending)>,
    blank_tags: Vec<String>,
}

/// 照 `p` 算一章的编辑（只读）。不是 UTF-8、没有 `<body>` 的章不动（`None`）。
fn resolve(e: &Entry, p: &Plan, remove: bool, trailing: bool) -> Option<Resolved> {
    let text = std::str::from_utf8(&e.data).ok()?;
    let (bs, be) = html::body_range(text)?;
    let starts: Vec<html::Tag> = html::tags_in(text, bs, be).filter(|t| t.is_start()).collect();
    let mut edits = Vec::new();
    let mut blank_tags = Vec::new();
    for &n in &p.blanks {
        let Some(t) = starts.get(n) else { continue };
        let raw = &text[t.start..t.end];
        if remove {
            let name = t.name.to_ascii_lowercase();
            let end = if raw.ends_with("/>") || html::is_void(&name) { Some(t.end) } else { html::find_close(text, t.end, &name).map(|c| c.end) };
            if let Some(end) = end {
                edits.push((t.start, end, Pending::Done(String::new())));
            }
            continue;
        }
        let mut tag = html::add_class(raw, BLANK);
        // 行内写的高、边距（《消失的爱人》`height:1em`；掌阅认行内样式）：后面接一份
        if let Some(s) = html::attr_value(&tag, "style") {
            let s = format!("{};{HIDE}", html::style_base(s));
            tag = html::set_attr(&tag, "style", &s);
        }
        blank_tags.push(t.name.to_ascii_lowercase());
        edits.push((t.start, t.end, Pending::Done(tag)));
    }
    for &n in p.trailing.iter().filter(|_| trailing) {
        let Some(t) = starts.get(n) else { continue };
        let end = if text[t.start..t.end].ends_with("/>") { Some(t.end) } else { html::find_close(text, t.end, "br").map(|c| c.end).or(Some(t.end)) };
        if let Some(end) = end {
            edits.push((t.start, end, Pending::Done(String::new())));
        }
    }
    for &(n, em) in &p.gaps {
        let Some(t) = starts.get(n) else { continue };
        edits.push((t.start, t.end, Pending::Gap((t.name.to_ascii_lowercase(), format!("{}em", fmt_em(em))))));
    }
    Some(Resolved { edits, blank_tags })
}

/// 照算好的编辑改写一章（`ks` 是折成上边距那几处按顺序编好的类号，从 0 起），返回改动数。
fn rewrite(e: &mut Entry, r: &Resolved, ks: &[usize], css_path: &str) -> usize {
    let Ok(text) = std::str::from_utf8(&e.data) else { return 0 };
    let mut ks = ks.iter();
    let mut edits: Vec<(usize, usize, String)> = r
        .edits
        .iter()
        .map(|(s, t, ed)| match ed {
            Pending::Done(new) => (*s, *t, new.clone()),
            Pending::Gap((_, value)) => {
                let k = ks.next().expect("每处折成的上边距都编了号");
                let mut tag = html::add_class(&text[*s..*t], &format!("eink-g{}", k + 1));
                if let Some(st) = html::attr_value(&tag, "style") {
                    let st = format!("{};margin-top:{value};", html::style_base(st));
                    tag = html::set_attr(&tag, "style", &st);
                }
                (*s, *t, tag)
            }
        })
        .collect();
    edits.sort_by_key(|e| e.0);
    let n = edits.len();
    let out = html::apply_edits(text, edits);
    let href = crate::epubzip::href_to(dir_of(&e.name), css_path, "");
    e.data = typeset::inject_css_link(&out, &href).into_bytes();
    n
}

fn fmt_em(v: f64) -> String {
    let s = format!("{:.4}", (v * 10000.0).round() / 10000.0);
    s.trim_end_matches('0').trim_end_matches('.').to_string()
}

/// 只有空白、排得出一行的内容（Kindle 折掉的）：去掉 HTML 空白（空格、制表、换行）以后还剩字符、而且全是空白（`&nbsp;`、
/// 全角空格），或者有 `<br/>`。只有普通空格的按 CSS 排不出行，不算。
fn blank_text(el: &ElementRef) -> bool {
    let mut any = false;
    for d in el.descendants() {
        match d.value() {
            Node::Text(t) => {
                for c in t.chars().filter(|c| !matches!(c, ' ' | '\t' | '\n' | '\r' | '\x0c')) {
                    if !c.is_whitespace() {
                        return false;
                    }
                    any = true;
                }
            }
            Node::Element(v) => match v.name() {
                "br" => any = true,
                "span" | "b" | "i" | "em" | "strong" | "font" | "u" | "small" | "big" | "sup" | "sub" | "a" => {}
                _ if d.id() == el.id() => {}
                _ => return false,
            },
            _ => {}
        }
    }
    any
}

fn plan(e: &Entry, media: Option<&MediaEnv>, css: &HashMap<&str, std::sync::Arc<Rules>>) -> Plan {
    let Ok(text) = std::str::from_utf8(&e.data) else { return Plan::default() };
    let Some(ch) = Chapter::parse(&e.name, text, media, css).filter(|c| c.matches_tags) else { return Plan::default() };
    let body = ch.body();
    let bc = ch.body_comp.clone();
    // body 里的元素按文档序编号（和标签扫描一致才知道改哪个标签）；`<body>` 自己不在里面
    let n_of = |el: &ElementRef| ch.index_of(el.id());
    let c_of = |el: &ElementRef| n_of(el).map(|i| ch.els[i].comp.clone());
    let is_blockish = |el: &ElementRef| n_of(el).is_some_and(|i| ch.els[i].is_block);
    // `<br>` 不算块（`br{display:block}` 的书：《人生复本》的空段 `<p><br class="calibre3"/></p>`，Kindle 照样折掉）
    let has_block_child = |el: &ElementRef| el.children().filter_map(ElementRef::wrap).any(|ch| ch.value().name() != "br" && is_blockish(&ch));
    // 竖直长度换成根 em
    let rem = |l: &Option<Len>, fs: f64| km::vertical_rem(*l, fs);
    let collapse = km::collapse;
    // 一块（连同第一个、最后一个子块，中间没有内边距、边框隔着的）在 Kindle 上的上 / 下外边距
    let edge = |el: &ElementRef, top: bool| -> Option<f64> { ch.margin_edge(n_of(el)?, top) };
    let blank_el = |el: &ElementRef| -> bool {
        if el.value().name() == "br" {
            return true;
        }
        is_blockish(el) && !has_block_child(el) && el.descendants().filter_map(ElementRef::wrap).all(|d| d.value().attr("id").is_none()) && blank_text(el)
    };
    // 这一层是块的序列（直接的文字只有空白，元素除了单独的 `<br/>` 都是块）才看
    let block_level = |parent: &ElementRef| {
        parent.children().all(|c| match c.value() {
            Node::Text(t) => t.trim().is_empty() && !t.contains(['\u{a0}', '\u{3000}']),
            Node::Element(v) => v.name() == "br" || ElementRef::wrap(c).is_some_and(|el| is_blockish(&el)),
            _ => true,
        })
    };
    let mut out = Plan::default();
    let mut parents = vec![body];
    parents.extend(body.descendants().filter_map(ElementRef::wrap).filter(|el| is_blockish(el) && has_block_child(el)));
    for parent in parents {
        if !block_level(&parent) {
            continue;
        }
        let Some(pc) = c_of(&parent).or_else(|| (parent.id() == body.id()).then(|| bc.clone())) else { continue };
        let kids: Vec<ElementRef> = parent.children().filter_map(ElementRef::wrap).collect();
        let mut i = 0;
        while i < kids.len() {
            if !blank_el(&kids[i]) {
                i += 1;
                continue;
            }
            let start = i;
            let mut gap = 0.0;
            // 空段自己的 (上, 下) 外边距（单独的 `<br/>` 没有）：Kindle 照留、逐对折叠（同写出器的 `Gap`，2026-10-14）
            let mut margins: Vec<(f64, f64)> = Vec::new();
            let mut ok = true;
            while i < kids.len() && blank_el(&kids[i]) {
                let el = &kids[i];
                // 单独的 `<br/>` 是匿名块，行高字号照父元素
                let c = if el.value().name() == "br" { pc.clone() } else { c_of(el).unwrap_or_else(|| pc.clone()) };
                ok &= (0..4).step_by(2).all(|s| rem(&c.margin[s], c.font_size) >= 0.0);
                gap += BLANK_LINE_FOLD * c.line_height.unwrap_or(LH_EM) * c.font_size;
                if el.value().name() != "br" {
                    margins.push((rem(&c.margin[0], c.font_size), rem(&c.margin[2], c.font_size)));
                }
                i += 1;
            }
            let run = &kids[start..i];
            // 只有 `<br/>`、夹在文字中间的不算（`block_level` 已经排除）；一串都是 `<br/>` 而前后没有块的也不算
            if !ok {
                continue;
            }
            let ids: Vec<usize> = run.iter().filter_map(n_of).collect();
            match kids.get(i) {
                Some(next) => {
                    let (Some(nt), prev_b) = (edge(next, true), if start > 0 { edge(&kids[start - 1], false) } else { Some(0.0) }) else { continue };
                    let Some(pb) = prev_b else { continue };
                    let nc = c_of(next).unwrap();
                    // 接住的块有上内边距或上边框又套着块：Kindle 上间距给谁拿不准
                    if (rem(&nc.padding[0], nc.font_size) > 0.0 || nc.border[0].is_some()) && has_block_child(next) {
                        continue;
                    }
                    // Kindle：前一块的下外边距、各空段的上下外边距、后一块的上外边距逐对折叠，空段的高另加；没有空段外边距的（单独的
                    // `<br/>`）后一块没有上外边距时不折叠，前一块的下外边距留着
                    let total = match margins.split_first() {
                        Some((first, rest)) => {
                            let mut t = collapse(pb, first.0);
                            let mut bottom = first.1;
                            for &(top, b) in rest {
                                t += collapse(bottom, top);
                                bottom = b;
                            }
                            t + collapse(bottom, nt) + gap
                        }
                        None if nt == 0.0 => pb + gap,
                        None => collapse(pb, nt) + gap,
                    };
                    out.blanks.extend(ids);
                    out.gaps.push((n_of(next).unwrap(), total / nc.font_size));
                }
                None => {
                    // 同一层后面没有块：文件后面也没有内容（文件末尾的空段，Kindle 上没有）才只藏起来
                    let last = run.last().unwrap();
                    let later = last.next_siblings().chain(last.ancestors().flat_map(|a| a.next_siblings())).any(|n| {
                        n.descendants().any(|d| match d.value() {
                            Node::Text(t) => !t.trim().is_empty(),
                            Node::Element(v) => matches!(v.name(), "img" | "svg" | "image" | "hr" | "table"),
                            _ => false,
                        })
                    });
                    if !later {
                        out.blanks.extend(ids);
                    }
                }
            }
        }
    }
    // 块末尾的 `<br/>`：最近的块里没有子块、`<br/>` 前面有字或图，后面到块结束只有空格换行、`<br/>` 和空的行内元素
    let content = |n: ego_tree::NodeRef<Node>| match n.value() {
        Node::Text(t) => t.chars().any(|c| !matches!(c, ' ' | '\t' | '\n' | '\r' | '\x0c')),
        Node::Element(v) => matches!(v.name(), "img" | "svg" | "image" | "hr" | "table" | "input" | "object" | "video" | "audio" | "math"),
        _ => false,
    };
    for br in body.descendants().filter_map(ElementRef::wrap).filter(|el| el.value().name() == "br" && el.value().attr("id").is_none()) {
        let Some(block) = br.ancestors().filter_map(ElementRef::wrap).find(|a| a.id() == body.id() || is_blockish(a)) else { continue };
        if block.id() == body.id() || has_block_child(&block) || n_of(&block).is_some_and(|n| out.blanks.contains(&n)) || c_of(&block).is_some_and(|c| c.pre) {
            continue;
        }
        let mut before = false;
        let mut after = false;
        let mut seen = false;
        for d in block.descendants() {
            if d.id() == br.id() {
                seen = true;
            } else if content(d) {
                if seen {
                    after = true;
                    break;
                }
                before = true;
            }
        }
        if before && !after {
            out.trailing.extend(n_of(&br));
        }
    }
    out.blanks.sort_unstable();
    out.gaps.sort_by_key(|g| g.0);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn book(page: &str) -> Vec<Entry> {
        vec![
            Entry { name: "O/content.opf".into(), data: br#"<package><manifest><item id="c" href="c.xhtml" media-type="application/xhtml+xml"/></manifest><spine><itemref idref="c"/></spine></package>"#.to_vec() },
            Entry { name: "O/s.css".into(), data: b"p{margin-top:1em;margin-bottom:1em;} p.big{font-size:2em;} p.lh2{line-height:2;}".to_vec() },
            Entry { name: "O/c.xhtml".into(), data: page.as_bytes().to_vec() },
        ]
    }

    fn page(body: &str) -> String {
        format!(r#"<html xmlns="http://www.w3.org/1999/xhtml"><head><title>t</title><link rel="stylesheet" href="s.css"/></head><body>{body}</body></html>"#)
    }

    /// 同写出器的测试：`<p>` 上下 1em 折叠成 1em，再加折掉的空段：一个 0.72em（缺省行高 1.2），两个 1.44em，行高 2 的 1.2em；
    /// 文件末尾的只藏起来；有 id 的不动。
    #[test]
    fn blank_paragraphs_fold_into_next_margin_like_kindle() {
        let mut b = book(&page(r#"<p>甲</p><p>&#160;</p><p>乙</p><p><br/></p><p>　</p><p class="big">丙</p><p class="lh2">&#160;</p><p>丁</p><br/><p>戊</p><p id="k">&#160;</p><p>己</p><p> </p><p>庚</p><p><br/></p>"#));
        let mut moved = b.clone();
        apply(&mut b, &mut WashReport::default(), None, false, false);
        let out = String::from_utf8(b[2].data.clone()).unwrap();
        assert!(out.contains(r#"<p>甲</p><p class="eink-blank">&#160;</p><p class="eink-g1">乙</p><p class="eink-blank"><br/></p><p class="eink-blank">　</p><p class="big eink-g1">丙</p><p class="lh2 eink-blank">&#160;</p><p class="eink-g2">丁</p><br class="eink-blank"/><p class="eink-g3">戊</p><p id="k">&#160;</p><p>己</p><p> </p><p>庚</p><p class="eink-blank"><br/></p>"#), "{out}");
        assert!(out.contains("eink-blank.css"));
        let sheet = String::from_utf8(b.iter().find(|e| e.name == "O/eink-blank.css").unwrap().data.clone()).unwrap();
        // 空段自己的上下外边距（1em）照留、逐对折叠（2026-10-14，同写出器的 `Gap`）：乙 1 + 1 + 0.72 = 2.72；丙（字号 2em，上外边距 2rem）：
        // 1 + 1 + 2 + 1.44 = 5.44rem = 2.72em；丁：前一段（丙）下外边距 2rem 和空段 1em 折成 2，再 1 + 1.2 = 4.2；戊：单独的 `<br/>` 没有外边距，
        // 1 + 0.72（按 body）；同标签同值共用一个类
        assert!(sheet.contains("html body p.eink-g1{margin-top:2.72em;}"), "{sheet}");
        assert!(sheet.contains("html body p.eink-g2{margin-top:4.2em;}") && sheet.contains("html body p.eink-g3{margin-top:1.72em;}"), "{sheet}");
        assert!(sheet.contains("html body p.eink-blank{display:none;}") && sheet.contains("html body br.eink-blank{display:none;}"), "{sheet}");
        // 不认 display:none 的（Move）：空段去掉，接住的块一样
        apply(&mut moved, &mut WashReport::default(), None, true, true);
        let out = String::from_utf8(moved[2].data.clone()).unwrap();
        assert!(out.contains(r#"<body><p>甲</p><p class="eink-g1">乙</p><p class="big eink-g1">丙</p><p class="eink-g2">丁</p><p class="eink-g3">戊</p><p id="k">&#160;</p><p>己</p><p> </p><p>庚</p></body>"#), "{out}");
        let sheet = String::from_utf8(moved.iter().find(|e| e.name == "O/eink-blank.css").unwrap().data.clone()).unwrap();
        assert!(!sheet.contains("eink-blank{"), "{sheet}");
    }

    /// `br{display:block}` 的书：`<p><br class="b"/></p>` 照样是空段（《人生复本》）。
    #[test]
    fn block_br_still_blank() {
        let mut b = book(&page(r#"<p>甲</p><p><br class="b"/></p><p>乙</p>"#));
        b[1].data.extend_from_slice(b" .b{display:block}");
        apply(&mut b, &mut WashReport::default(), None, false, false);
        let out = String::from_utf8(b[2].data.clone()).unwrap();
        assert!(out.contains(r#"<p class="eink-blank"><br class="b"/></p><p class="eink-g1">乙</p>"#), "{out}");
    }

    /// 后一块没有上外边距：前一块的下外边距和空段的外边距折叠、空段的下外边距留着、空段的高另加（1 + 1 + 0.72 = 2.72rem，h2 字号 1.5em）；空段后面同一层没块、文件后面还有字的不动。
    #[test]
    fn no_top_margin_and_unsure_cases() {
        let mut b = book(&page(r#"<p>甲</p><p>&#160;</p><h2 style="margin-top:0">乙</h2><div><p>丙</p><p>&#160;</p></div><p>丁</p>"#));
        apply(&mut b, &mut WashReport::default(), None, false, false);
        let out = String::from_utf8(b[2].data.clone()).unwrap();
        assert!(out.contains(r#"<p class="eink-blank">&#160;</p><h2 style="margin-top:0;margin-top:1.8133em;" class="eink-g1">乙</h2><div><p>丙</p><p>&#160;</p></div>"#), "{out}");
    }

    /// 块末尾的 `<br/>`：多排一行的阅读器（Move）上去掉（套在行内元素里的、连着两个的也去），中间的、开头的、有 id 的、块里套着块的
    /// 不动；不排的（掌阅）不动。和认不认 `display:none` 无关。
    #[test]
    fn trailing_br_removed_only_where_reader_draws_it() {
        let body = r#"<h2>甲<br/></h2><p><span>乙<br/></span> <br/>
</p><p>丙<br/>丁</p><p><br/>戊</p><p>己<br id="x"/></p><div>庚<br/><p>辛</p></div>"#;
        let mut b = book(&page(body));
        let before = b[2].data.clone();
        apply(&mut b, &mut WashReport::default(), None, false, false);
        assert_eq!(b[2].data, before);
        apply(&mut b, &mut WashReport::default(), None, false, true);
        let out = String::from_utf8(b[2].data.clone()).unwrap();
        assert!(out.contains(r#"<h2>甲</h2><p><span>乙</span> 
</p><p>丙<br/>丁</p><p><br/>戊</p><p>己<br id="x"/></p><div>庚<br/><p>辛</p></div>"#), "{out}");
    }
}
