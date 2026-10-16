//! 行内 `style`、`<style>` 和规则最后一条不带分号又紧挨 `}` 的声明里的 em 照 CSS 排（不这样排的阅读器：Move，profile
//! `inline_css_lengths = false`；只修复、照 Send to Kindle 统一的文字书；2026-10-13 用户要修）。
//!
//! xochitl 有两套 CSS 解析（2026-10-13 怪癖测试书 `tools/quirks/probe.py`）：外链样式表里带分号的声明照 CSS 排（1em 37.8px）；
//! 行内 `style`、`<style>` 里的、规则最后一条不带分号且紧挨着 `}` 的声明走另一套——em、rem 的长度按 1em＝23px（约 0.61 倍），`font-size`、
//! `text-indent` 写 em、% 不认（px 认）。《狼厅》每章 `<style>` 里的 `text-indent:2em`（17299 段）在 Move 上没有首行缩进，
//! 这里最后一步（竖直间距换算之后）：
//!
//! - 写了 em 的 `<style>` 的内容挪进和这一章同目录的外链样式表 `eink-style-<哈希>.css`（内容一样的合用一份），原位置换成 `<link>`，
//!   层叠顺序不变；`media` 照带；
//! - 行内 `style` 里写 em（`font-size` 还有 %）的外边距、内边距、首行缩进、字号、字距，搬进 `eink-inline.css` 的类规则
//!   `html body 标签.eink-iN.eink-iN{… !important}`（行内本来压过样式表；Move 不按 `!important` 提优先级、优先级一样时比规则在各自
//!   样式表里的序号，所以类名重复到比全书哪个选择器的类都多，见 [`class_repeats`]；KINDLE_RULES 41），
//!   同一属性行内写了好几次的按最后一次；标题的字号不搬（Move 的标题只认关键字，`block_style` 写的关键字在行内）；
//! - 全书外链样式表（含上面挪出来的）里最后一条声明写 em、不带分号、值紧挨着 `}` 的补上分号（只有这种走另一套解析，探针 Q45、Q61–Q64）。
use super::*;
use crate::util::{fnv64, xml_escape, xml_unescape};
use std::collections::BTreeMap;

const INLINE_CSS: &str = "eink-inline.css";
const CLASS: &str = "eink-i";

/// 行内声明里要搬进样式表的属性（值里有 em 长度时）。
const MOVED: &[&str] = &[
    "margin", "margin-top", "margin-bottom", "margin-left", "margin-right", "padding", "padding-top", "padding-bottom", "padding-left", "padding-right", "text-indent", "font-size", "font",
    "letter-spacing", "word-spacing",
];

pub(super) fn apply(entries: &mut Vec<Entry>, rep: &mut WashReport) {
    let opf_idx = find_opf(entries);
    let opf_dir = opf_idx.map(|i| dir_of(&entries[i].name).to_string()).unwrap_or_default();
    let at = |dir: &str, n: &str| if dir.is_empty() { n.to_string() } else { format!("{dir}/{n}") };
    let inline_path = at(&opf_dir, INLINE_CSS);
    // 挪出来的 <style>：书内路径 → 内容；行内搬出来的类：(标签, 声明) → 序号
    // 三段（2026-10-15 改，以前整步串行）：① 各章并行看要挪哪些 `<style>`、哪些行内 `style` 要拆（[`plan_chapter`]，只读）；
    // ② 串行按「章的顺序、章里标签的顺序」给搬出来的声明编类号——和以前逐章做时编号的先后一样，产物逐字节不变；
    // ③ 各章并行改写（[`rewrite_chapter`]）。
    let chapters = super::chapter_indices(entries);
    let plans: Vec<ChapterPlan> = crate::util::par_map(&chapters, |&i| plan_chapter(&entries[i], &at));
    let mut extracted: BTreeMap<String, String> = BTreeMap::new();
    let mut classes: Vec<(String, String)> = Vec::new();
    let mut numbered: HashMap<usize, (ChapterPlan, Vec<usize>)> = HashMap::new();
    for (&i, plan) in chapters.iter().zip(plans) {
        if plan.styles.is_empty() && plan.inline.is_empty() {
            continue;
        }
        for (_, _, _, path, css) in &plan.styles {
            extracted.insert(path.clone(), css.clone());
        }
        let ks: Vec<usize> = plan
            .inline
            .iter()
            .map(|(_, _, tag, _, moved)| {
                classes.iter().position(|c| c.0 == *tag && c.1 == *moved).unwrap_or_else(|| {
                    classes.push((tag.clone(), moved.clone()));
                    classes.len() - 1
                })
            })
            .collect();
        numbered.insert(i, (plan, ks));
    }
    let mut work: Vec<(&mut Entry, (ChapterPlan, Vec<usize>))> = entries.iter_mut().enumerate().filter_map(|(i, e)| numbered.remove(&i).map(|p| (e, p))).collect();
    rep.kindle_rule_edits += crate::util::par_map_mut(&mut work, |(e, (plan, ks))| rewrite_chapter(e, plan, ks, &inline_path)).into_iter().sum::<usize>();
    for (n, (path, css)) in extracted.iter().enumerate() {
        add_css_entry(entries, opf_idx, path, css, &format!("eink-style-{}", n + 1));
    }
    if !classes.is_empty() {
        let mut sheet = String::from("/* 行内 style 里写 em 的长度（sheng-ren kindle_rules，见 wash/inline_css.rs） */\n");
        let reps = class_repeats(entries.iter().filter(|e| is_css_name(&e.name)).map(|e| e.data.as_slice()).chain(extracted.values().map(|c| c.as_bytes())));
        for (k, (tag, decls)) in classes.iter().enumerate() {
            let class = format!(".{CLASS}{}", k + 1).repeat(reps);
            let sel = if tag == "body" { format!("html body{class}") } else { format!("html body {tag}{class}") };
            sheet.push_str(&format!("{sel}{{{decls}}}\n"));
        }
        add_css_entry(entries, opf_idx, &inline_path, &sheet, "eink-inline-css");
    }
    // 全书样式表：每条规则最后一条声明补分号
    for e in entries.iter_mut().filter(|e| is_css_name(&e.name)) {
        let (text, latin1) = match std::str::from_utf8(&e.data) {
            Ok(t) => (Cow::Borrowed(t), false),
            Err(_) => (Cow::Owned(crate::util::latin1_decode(&e.data)), true),
        };
        let (out, n) = close_last_decls(&text);
        if n > 0 {
            rep.kindle_rule_edits += n;
            e.data = if latin1 { crate::util::latin1_encode(&out) } else { out.into_bytes() };
        }
    }
}

/// 一章要改的（[`plan_chapter`]）。
#[derive(Default)]
struct ChapterPlan {
    /// 挪出去的 `<style>`：(块的起止, 换成的 `<link>`, 挪成的样式表的书内路径, 内容)。
    styles: Vec<(usize, usize, String, String, String)>,
    /// 要拆的行内 `style`（文档序，`<body>` 自己在最前）：(标签的起止, 按哪个标签名搬, 留在行内的, 搬出去的)。
    inline: Vec<(usize, usize, String, String, String)>,
}

/// 看一章（只读）：挪哪些 `<style>`、拆哪些行内 `style`。`at(目录, 文件名)` 是书内路径。
fn plan_chapter(e: &Entry, at: &(impl Fn(&str, &str) -> String + Sync)) -> ChapterPlan {
    let mut plan = ChapterPlan::default();
    let Ok(text) = std::str::from_utf8(&e.data) else { return plan };
    let dir = dir_of(&e.name);
    for b in html::style_blocks(text) {
        let open = &text[b.start..b.content_start];
        let css = clean_style(&text[b.content_start..b.content_end]);
        // 没写 em 的不挪（封面页的 `@page{margin:0}` 这类）：Move 上排得一样，少动书
        if !css_has_em(&css) {
            continue;
        }
        let name = format!("eink-style-{:08x}.css", fnv64(css.as_bytes()) as u32);
        let media = html::attr_value(open, "media").map(|m| format!(r#" media="{m}""#)).unwrap_or_default();
        let link = format!(r#"<link rel="stylesheet" type="text/css" href="{name}"{media}/>"#);
        plan.styles.push((b.start, b.end, link, at(dir, &name), css));
    }
    if let Some((bs, be)) = html::body_range(text) {
        // `<body>` 自己也算（`root_em` 给写 px 字号的 `<body>` 接的 em：《狼厅》）
        let body = html::tags(text).find(|t| t.is_start() && t.is("body"));
        for t in body.into_iter().chain(html::tags_in(text, bs, be).filter(|t| t.is_start())) {
            let raw = &text[t.start..t.end];
            let Some(style) = html::attr_value(raw, "style") else { continue };
            // 要改成 `div` 的标题（`kindle_model::DIV_ATTR`）按 `div` 搬：字号也搬
            let tag = crate::kindle_model::reader_tag_raw(t.name, raw);
            let Some((kept, moved)) = split_inline(&xml_unescape(style), &tag) else { continue };
            plan.inline.push((t.start, t.end, tag, kept, moved));
        }
    }
    plan
}

/// 照 `plan` 改写一章（`ks` 是各处行内 `style` 编好的类号，从 0 起），返回改动数。
fn rewrite_chapter(e: &mut Entry, plan: &ChapterPlan, ks: &[usize], inline_path: &str) -> usize {
    let Ok(text) = std::str::from_utf8(&e.data) else { return 0 };
    let mut edits: Vec<(usize, usize, String)> = plan.styles.iter().map(|(s, t, link, _, _)| (*s, *t, link.clone())).collect();
    for ((s, t, _, kept, _), k) in plan.inline.iter().zip(ks) {
        let raw = &text[*s..*t];
        let mut new = if kept.trim().is_empty() { html::remove_attr(raw, "style") } else { html::set_attr(raw, "style", &xml_escape(kept)) };
        new = html::add_class(&new, &format!("{CLASS}{}", k + 1));
        edits.push((*s, *t, new));
    }
    edits.sort_by_key(|x| x.0);
    let n = edits.len();
    let mut out = html::apply_edits(text, edits);
    if !plan.inline.is_empty() {
        out = typeset::inject_css_link(&out, &crate::epubzip::href_to(dir_of(&e.name), inline_path, ""));
    }
    e.data = out.into_bytes();
    n
}

/// `<style>` 的内容当样式表：去掉包着的 `<![CDATA[ ]]>`、`<!-- -->`，字符引用还原（XHTML 里 `<style>` 的内容按 XML 解析）。
/// `eink-inline.css` 的类名要重复几遍：比全书样式表里任何选择器的类（含属性、伪类）都多一个，至少 2（压过 `eink-box.css` 这些
/// 一个类的规则）。xochitl 不按 `!important` 提优先级，优先级一样时比规则在**各自样式表里**的序号（2026-10-16 探针「测试-规则顺序」：
/// 《古龙》`eink-box.css` 靠后的 `text-indent:1.08em` 压过 `eink-inline.css` 第 8 条的 `1.8em !important`），只有优先级管用。
/// 带 id 的选择器类再多也压不过，不管（行内搬出来的属性上碰到的再说）。
fn class_repeats<'a>(sheets: impl Iterator<Item = &'a [u8]>) -> usize {
    let mut most = 2;
    for css in sheets {
        let css = String::from_utf8_lossy(css);
        for r in crate::cascade::rule_spans(&crate::cascade::strip_comments(&css)) {
            let sel = crate::cascade::rule_selector(r.prelude);
            if sel.trim_start().starts_with('@') {
                continue;
            }
            for one in crate::cascade::split_top(&sel, ',') {
                let (a, b, _) = crate::cascade::specificity(one.trim());
                if a == 0 {
                    most = most.max(b as usize + 1);
                }
            }
        }
    }
    most
}

fn clean_style(s: &str) -> String {
    let mut t = s.trim();
    for (a, b) in [("<![CDATA[", "]]>"), ("<!--", "-->")] {
        if let Some(inner) = t.strip_prefix(a).and_then(|x| x.strip_suffix(b)) {
            t = inner.trim();
        }
    }
    format!("{}\n", xml_unescape(t))
}

/// 样式表里有没有写 em 的声明（[`has_em`]）。
fn css_has_em(css: &str) -> bool {
    crate::cascade::rule_spans(css).iter().any(|r| html::css_decls(r.body).iter().any(|d| has_em(&d.prop.to_ascii_lowercase(), d.value)))
}

/// 值里有没有 em 长度（`font-size` 还算 %）。
fn has_em(prop: &str, v: &str) -> bool {
    v.split(|c: char| c.is_whitespace() || c == ',' || c == '/' || c == '(' || c == ')').any(|tok| {
        let tok = tok.trim().to_ascii_lowercase();
        let end = tok.find(|c: char| !(c.is_ascii_digit() || c == '.' || c == '-' || c == '+')).unwrap_or(tok.len());
        end > 0 && tok[..end].parse::<f64>().is_ok() && (matches!(&tok[end..], "em" | "rem" | "ex") || (prop.starts_with("font") && &tok[end..] == "%"))
    })
}

/// 行内 `style` 拆成 (留在行内的, 搬进样式表的声明)；没有要搬的 `None`。搬出去的每条带上 `!important`。
fn split_inline(style: &str, tag: &str) -> Option<(String, String)> {
    let decls = html::css_decls(style);
    let heading = matches!(tag, "h1" | "h2" | "h3" | "h4" | "h5" | "h6");
    // 行内元素上的首行缩进不起作用：不搬，少动书。（`font-size:1em` 以前也当没写不搬；Move 的 em 按根字号算，`root_em` 接的
    // `font-size:1em` 要压过 px 写的字号（《狼厅》），2026-10-14 起照搬）
    let inline = matches!(tag, "span" | "a" | "b" | "i" | "em" | "strong" | "sup" | "sub" | "font" | "small" | "big" | "u" | "s" | "code" | "cite" | "q" | "abbr");
    let noop = |p: &str, _: &str| inline && p == "text-indent";
    // 每个属性最后一次写的值
    let mut last: BTreeMap<String, &str> = BTreeMap::new();
    for d in &decls {
        last.insert(d.prop.to_ascii_lowercase(), d.value);
    }
    let moved_props: Vec<String> = last
        .iter()
        .filter(|(p, v)| MOVED.contains(&p.as_str()) && has_em(p, v) && !(heading && p.starts_with("font")) && !noop(p, v))
        .map(|(p, _)| p.clone())
        .collect();
    if moved_props.is_empty() {
        return None;
    }
    // 搬出去的按行内原来的顺序（同一属性只留最后一次）
    let mut moved = String::new();
    for d in &decls {
        let p = d.prop.to_ascii_lowercase();
        if moved_props.contains(&p) && last.get(&p) == Some(&d.value) && !moved.contains(&format!("{p}:")) {
            let v = d.value.trim();
            let imp = if v.to_ascii_lowercase().ends_with("!important") { "" } else { " !important" };
            moved.push_str(&format!("{p}:{v}{imp};"));
        }
    }
    let kept: String = decls.iter().filter(|d| !moved_props.contains(&d.prop.to_ascii_lowercase())).map(|d| d.raw).collect();
    Some((kept.trim().to_string(), moved))
}

/// 样式表里每条规则（`@media` 里的也算；套着块的不动）最后一条声明写 em、不带分号、值紧挨着 `}` 的补上分号
/// （xochitl 只有这种走另一套解析：`margin-left:2em}` 挪 46px，`2em }`、换行再 `}` 照常 76px，探针 Q45、Q61–Q64；中间有空白的
/// 不改，少动书）。返回 (新文本, 补了几条)。
fn close_last_decls(css: &str) -> (String, usize) {
    let edits: Vec<(usize, usize, String)> = crate::cascade::rule_spans(css)
        .into_iter()
        .filter(|r| r.closed && !r.body.contains('{') && !r.body.ends_with(char::is_whitespace))
        .filter_map(|r| {
            let code = crate::cascade::strip_comments(r.body);
            let code = code.trim_end();
            // 最后一条写的是 em 才补（另一套解析只错在 em 上，px、关键字照常：我们自己的 `height:auto}` 不动）
            let last_em = html::css_decls(code).last().is_some_and(|d| has_em(&d.prop.to_ascii_lowercase(), d.value));
            (last_em && !code.ends_with(';')).then(|| {
                let at = r.body_start + r.body.len();
                (at, at, ";".to_string())
            })
        })
        .collect();
    let n = edits.len();
    (html::apply_edits(css, edits), n)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn last_declarations_closed() {
        let (out, n) = close_last_decls("p{text-indent:2em} h1 { margin:1em 0 ; } @media x { .a{margin:0 1.5em/* c */} .b{margin:1em\n} .c{height:auto} } @font-face{font-family:a}\ndiv{}");
        assert_eq!(out, "p{text-indent:2em;} h1 { margin:1em 0 ; } @media x { .a{margin:0 1.5em/* c */;} .b{margin:1em\n} .c{height:auto} } @font-face{font-family:a}\ndiv{}");
        assert_eq!(n, 2);
    }

    #[test]
    fn inline_em_split() {
        assert_eq!(split_inline("color:red; margin-top:1.31em;text-indent:2em", "p"), Some(("color:red;".into(), "margin-top:1.31em !important;text-indent:2em !important;".into())));
        assert_eq!(split_inline("margin-left:20px;line-height:1;font-size:large", "p"), None);
        // 标题的字号不搬；同一属性按最后一次：后面写的 px 压过前面的 em，不搬
        assert_eq!(split_inline("font-size:2em;font-size:xx-large;margin-bottom:1em", "h2"), Some(("font-size:2em;font-size:xx-large;".into(), "margin-bottom:1em !important;".into())));
        assert_eq!(split_inline("text-indent:2em;text-indent:30px", "p"), None);
        assert_eq!(split_inline("font-size:120%;margin:0 0 1em", "span"), Some(("".into(), "font-size:120% !important;margin:0 0 1em !important;".into())));
        // 行内元素上的首行缩进不搬；1em 字号 2026-10-14 起照搬（Move 的 em 按根字号算，见 split_inline）
        assert_eq!(split_inline("font-size: 1em; text-indent: 2em", "span"), Some(("text-indent: 2em".into(), "font-size:1em !important;".into())));
        assert_eq!(split_inline("text-indent:0em", "p"), Some(("".into(), "text-indent:0em !important;".into())));
    }

    /// 类名重复的遍数：比全书最多类的选择器多一个、至少 2；带 id 的、at 规则头不算，`@media` 里面的算。
    #[test]
    fn class_repeats_beat_every_class_selector() {
        assert_eq!(class_repeats([b"p{x:1}".as_slice()].into_iter()), 2);
        assert_eq!(class_repeats([b"p.a{x:1} #i .a.b.c.d{x:1}".as_slice(), b"@media screen{div.a .b[title]{x:1}} /* .a.b.c.d.e{} */".as_slice()].into_iter()), 4);
    }

    fn book(chapter: &str) -> Vec<Entry> {
        vec![
            Entry { name: "O/content.opf".into(), data: br#"<package><manifest><item id="c" href="t/c.xhtml" media-type="application/xhtml+xml"/></manifest><spine><itemref idref="c"/></spine></package>"#.to_vec() },
            Entry { name: "O/s.css".into(), data: b"p{text-indent:2em}".to_vec() },
            Entry { name: "O/t/c.xhtml".into(), data: chapter.as_bytes().to_vec() },
        ]
    }

    /// 写了 em 的 `<style>` 挪成同目录的外链（media 照带、CDATA 去掉），没写的不动；行内 em 搬进类规则，外链补分号，章节链上 `eink-inline.css`。
    #[test]
    fn style_blocks_and_inline_em_moved_to_sheets() {
        let ch = r#"<html xmlns="http://www.w3.org/1999/xhtml"><head><link rel="stylesheet" href="../s.css"/><style media="screen"><![CDATA[p.a{text-indent:2em}]]></style><style>@page{margin:0}</style></head><body><p class="a" style="margin-top:1em;color:red">一</p><p style="margin-top:1em;color:red">二</p></body></html>"#;
        let mut b = book(ch);
        apply(&mut b, &mut WashReport::default());
        let out = String::from_utf8(b[2].data.clone()).unwrap();
        let css_name = b.iter().map(|e| e.name.as_str()).find(|n| n.starts_with("O/t/eink-style-")).unwrap().to_string();
        let file = css_name.rsplit('/').next().unwrap();
        assert!(out.contains(&format!(r#"<link rel="stylesheet" href="../s.css"/><link rel="stylesheet" type="text/css" href="{file}" media="screen"/><style>@page{{margin:0}}</style><link rel="stylesheet" type="text/css" href="../eink-inline.css"/></head>"#)), "{out}");
        assert!(out.contains(r#"<p class="a eink-i1" style="color:red">一</p><p style="color:red" class="eink-i1">二</p>"#), "{out}");
        let css = |n: &str| String::from_utf8(b.iter().find(|e| e.name == n).unwrap().data.clone()).unwrap();
        assert_eq!(css(&css_name), "p.a{text-indent:2em;}\n");
        assert_eq!(css("O/s.css"), "p{text-indent:2em;}");
        assert!(css("O/eink-inline.css").contains("html body p.eink-i1.eink-i1{margin-top:1em !important;}"), "{}", css("O/eink-inline.css"));
        let opf = String::from_utf8(b[0].data.clone()).unwrap();
        assert!(opf.contains("t/eink-style-") && opf.contains(r#"href="eink-inline.css""#), "{opf}");
    }
}
