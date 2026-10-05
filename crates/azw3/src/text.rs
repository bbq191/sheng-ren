//! 正文排版：每个 XHTML 文档拆成**骨架**（`<html><head>…<body aid="X"></body></html>`）和**片段**（body 里的内容），
//! 在 rawML 里依次存"骨架、片段、骨架、片段…"；片段在阅读器里插回骨架的 `</body>` 之前（KF8 样本黑盒分析得出）。
//!
//! 引用改写：图片 → `kindle:embed:XXXX?mime=…`（资源序号，1 起，base32 四位）；CSS → `kindle:flow:XXXX?mime=text/css`；
//! 书内链接 → `kindle:pos:fid:XXXX:off:YYYYYYYYYY`（fid = 片段序号，off = 目标在片段里的字节偏移，base32）。
//! 链接先写成等长占位串，所有文档排完、偏移定下来之后再回填，回填不改变任何偏移。

use bookconv::epubbook::Loaded;
use bookconv::epubzip::{dir_of, percent_decode, posix_norm, resolve};
use bookconv::html;
use bookconv::util::xml_unescape;
use regex::Regex;
use std::borrow::Cow;
use std::collections::HashMap;
use std::sync::OnceLock;

/// base32 编码（数字 0-9A-V）与读取侧共用一份。
pub use crate::read::palm::base32;

const POS_PLACEHOLDER: &str = "kindle:pos:fid:####:off:##########";

pub struct Fragment {
    /// 插入位置（组装后文本里的绝对偏移）= 骨架起点 + 骨架里 `</body>` 的偏移。
    pub insert_pos: u32,
    pub len: u32,
    pub aid: String,
}

pub struct NcxItem {
    pub label: String,
    pub level: u32,
    pub pos: u32,
    pub fid: u32,
    pub off: u32,
}

pub struct Layout {
    /// 第 0 条流：全部骨架与片段。
    pub flow0: Vec<u8>,
    /// 各骨架 (起点, 长度)。
    pub skeletons: Vec<(u32, u32)>,
    pub fragments: Vec<Fragment>,
    /// CSS 流（第 1 条起）。
    pub css_flows: Vec<Vec<u8>>,
    pub ncx: Vec<NcxItem>,
}

fn url_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r#"(?i)url\(\s*['"]?([^'")]+)['"]?\s*\)"#).unwrap())
}

/// CSS 里的 `url(图片)` 改成 `kindle:embed`；去掉 `@font-face`（不嵌字体，字体交给阅读器设置）。
fn rewrite_css(css: &str, css_path: &str, res: &HashMap<String, (u32, &'static str)>) -> String {
    static FACE: OnceLock<Regex> = OnceLock::new();
    let css = FACE.get_or_init(|| Regex::new(r#"(?is)@font-face\s*\{[^}]*\}"#).unwrap()).replace_all(css, "");
    url_re()
        .replace_all(&css, |c: &regex::Captures| {
            let p = posix_norm(&resolve(dir_of(css_path), &percent_decode(&c[1])));
            match res.get(&p) {
                Some((n, mime)) => format!("url(kindle:embed:{}?mime={mime})", base32(*n, 4)),
                None => c[0].to_string(),
            }
        })
        .into_owned()
}

/// 书内链接：占位串在片段里的字节偏移、目标文档序号、目标 id（已解码）。
struct Link {
    at: usize,
    doc: usize,
    frag: String,
}

/// 一个文档改写后的样子：骨架前半（到 body 开标签为止）、片段、骨架后半，外加片段里的链接。
struct Rewritten {
    head: String,
    frag: String,
    tail: String,
    links: Vec<Link>,
}

struct DocCtx<'a> {
    path: &'a str,
    res: &'a HashMap<String, (u32, &'static str)>,
    flows: &'a HashMap<String, u32>,
    doc_index: &'a HashMap<String, usize>,
}

/// 改写一个开标签里的引用。返回新标签，和其中链接占位串的 (标签内偏移, 目标文档, id)。标签整个去掉时返回空串。
/// 属性按 [`html::attrs`] 取（单双引号、无引号都认，属性值里的 `>` 不截断标签）；改写后的值一律写成双引号。
fn rewrite_tag(tag: &str, name: &str, cx: &DocCtx) -> (String, Vec<(usize, usize, String)>) {
    let all = html::attrs(tag);
    // 属性原文先还原字符引用（`&amp;` 等），再按路径/锚点分别百分号解码（与 id_offsets 对 id 的处理一致）。
    let val = |a: &html::Attr| xml_unescape(a.value).into_owned();
    // 非样式表的 <link>（Adobe 的 page-template.xpgt 等）和指向书里不存在的样式表的 <link>，Kindle 用不上，去掉。
    let is_css_link = name == "link" && all.iter().any(|a| a.is("rel") && a.value.split_ascii_whitespace().any(|w| w.eq_ignore_ascii_case("stylesheet")));
    if name == "link" {
        let href = all.iter().find(|a| a.is("href")).map(val).unwrap_or_default();
        let target = posix_norm(&resolve(dir_of(cx.path), &percent_decode(href.split('#').next().unwrap_or(""))));
        if !is_css_link || !cx.flows.contains_key(&target) {
            return (String::new(), Vec::new());
        }
    }
    let mut out = String::with_capacity(tag.len());
    let mut links = Vec::new();
    let mut last = 0;
    for a in all.iter().filter(|a| a.is("href") || a.is("src") || a.is("xlink:href")) {
        if a.quote.is_none() && a.value.is_empty() {
            continue; // `<a href>`：没有值
        }
        let v = val(a);
        let (p, frag) = v.split_once('#').unwrap_or((&v, ""));
        if html::is_external(p) {
            continue; // http:、mailto:、data: 等外部地址
        }
        let target = if p.is_empty() { cx.path.to_string() } else { posix_norm(&resolve(dir_of(cx.path), &percent_decode(p))) };
        // 换掉的是整个值（连同原来的引号）
        let (vs, ve) = if a.quote.is_some() { (a.value_start - 1, a.end) } else { (a.value_start, a.value_end) };
        let new = if is_css_link {
            match cx.flows.get(&target) {
                Some(n) => format!("kindle:flow:{}?mime=text/css", base32(*n, 4)),
                None => continue,
            }
        } else if let Some((n, mime)) = cx.res.get(&target).filter(|_| name != "a") {
            format!("kindle:embed:{}?mime={mime}", base32(*n, 4))
        } else if let Some(&j) = cx.doc_index.get(&target).filter(|_| name == "a" || name == "area") {
            out.push_str(&tag[last..vs]);
            out.push('"');
            links.push((out.len(), j, percent_decode(frag)));
            out.push_str(POS_PLACEHOLDER);
            out.push('"');
            last = ve;
            continue;
        } else {
            continue;
        };
        out.push_str(&tag[last..vs]);
        out.push('"');
        out.push_str(&new);
        out.push('"');
        last = ve;
    }
    out.push_str(&tag[last..]);
    (out, links)
}

/// `<head>` 里只留 `<title>`、`<meta>`、`<link>`、`<style>`、`<base>`：别的元素和散落的文字去掉。
/// EPUB 阅读器按 XHTML 处理，`<head>` 里的东西一概不显示；Kindle 会把它们当正文显示在章首——
/// 《绝叫》原书每章 `<head>` 里漏进一个 SVG 封面和一大段样式代码（多一个 `</div>*/` 把样式表截断了），
/// 在 Kindle 上满页代码（2026-09-30 真机）。只动 `<head>`，正文一个字不改（EPUB 阅读器本来就看不到这些）。
fn clean_head(html: &str) -> Cow<'_, str> {
    const KEEP: [&str; 5] = ["title", "meta", "link", "style", "base"];
    let Some(open) = html::tags(html).find(|t| t.is_start() && t.is("head")) else { return Cow::Borrowed(html) };
    let Some(close) = html::find_close(html, open.end, "head") else { return Cow::Borrowed(html) };
    let mut kept = String::new();
    let mut pos = open.end;
    for t in html::tags_in(html, open.end, close.start) {
        if t.start < pos || !t.is_start() || !KEEP.iter().any(|k| t.is(k)) {
            continue;
        }
        // title、style 连同内容和闭合标签一起留
        let end = if t.is("title") || t.is("style") { html::find_close(html, t.end, t.name).map_or(t.end, |c| c.end) } else { t.end };
        kept.push_str(&html[t.start..end]);
        pos = end;
    }
    if html[open.end..close.start] == kept {
        return Cow::Borrowed(html);
    }
    Cow::Owned(format!("{}{kept}{}", &html[..open.end], &html[close.start..]))
}

fn rewrite_doc(html: &str, aid: &str, cx: &DocCtx) -> Result<Rewritten, String> {
    static SCRIPT: OnceLock<Regex> = OnceLock::new();
    static STYLE: OnceLock<Regex> = OnceLock::new();
    let path = cx.path;
    let html = clean_head(html);
    let html = SCRIPT.get_or_init(|| Regex::new(r#"(?is)<script\b.*?</script>"#).unwrap()).replace_all(&html, "");
    let html = STYLE
        .get_or_init(|| Regex::new(r#"(?is)(<style\b[^>]*>)(.*?)(</style>)"#).unwrap())
        .replace_all(&html, |c: &regex::Captures| format!("{}{}{}", &c[1], rewrite_css(&c[2], path, cx.res), &c[3]));
    // 只改开标签（含自闭合）；注释、CDATA、声明原样跳过（注释里的标签不改写）
    let mut out = String::with_capacity(html.len() + html.len() / 8);
    let mut links: Vec<(usize, usize, String)> = Vec::new();
    let mut body_open: Option<(usize, usize)> = None; // body 开标签在 out 里的 (起, 止)
    let mut last = 0;
    for t in html::tags(&html).filter(|t| t.is_start()) {
        out.push_str(&html[last..t.start]);
        last = t.end;
        let raw = &html[t.start..t.end];
        let name = t.name.to_ascii_lowercase();
        // 书里原有的 aid（单双引号、无引号都认）去掉，免得和骨架的 aid 冲突
        let tag = if html::attrs(raw).iter().any(|a| a.is("aid")) { Cow::Owned(html::remove_attr(raw, "aid")) } else { Cow::Borrowed(raw) };
        let (mut new, tag_links) = rewrite_tag(&tag, &name, cx);
        if name == "body" && body_open.is_none() {
            // body 开标签带上 aid（片段插回的位置由它标识）
            let trimmed = new.trim_end_matches('>').trim_end_matches('/').trim_end().to_string();
            new = format!("{trimmed} aid=\"{aid}\">");
            body_open = Some((out.len(), out.len() + new.len()));
        }
        let base = out.len();
        links.extend(tag_links.into_iter().map(|(at, j, f)| (base + at, j, f)));
        out.push_str(&new);
    }
    out.push_str(&html[last..]);
    let (_, b_end) = body_open.ok_or_else(|| format!("{path} 没有 <body>"))?;
    // 最后一个 </body>（body_range 找的第一个 body 开标签就是上面加了 aid 的那个）
    let close = html::body_range(&out).map(|(_, c)| c).filter(|&c| c >= b_end).ok_or_else(|| format!("{path} 没有 </body>"))?;
    let mut head = out[..b_end].to_string();
    let frag = out[b_end..close].to_string();
    let mut tail = out[close..].to_string();
    // 只有片段（body 内）里的链接要回填；body 外的（不合法，但可能有）改成 "#"，从后往前改免得偏移错位。
    let mut body_links = Vec::new();
    links.sort_unstable_by_key(|l| std::cmp::Reverse(l.0));
    for (at, doc, f) in links {
        if at >= close {
            tail.replace_range(at - close..at - close + POS_PLACEHOLDER.len(), "#");
        } else if at >= b_end {
            body_links.push(Link { at: at - b_end, doc, frag: f });
        } else {
            head.replace_range(at..at + POS_PLACEHOLDER.len(), "#");
        }
    }
    Ok(Rewritten { head, frag, tail, links: body_links })
}

/// 片段里各锚点的字节偏移（所在标签的起点）：任何元素的 `id`，外加 `<a name>`（老书的注释落点常这样写）。
/// 值先还原字符引用；同名只记第一个，`id` 优先于同名的 `<a name>`（HTML 找锚点也是 id 优先）。
fn id_offsets(frag: &str) -> HashMap<String, usize> {
    let mut m = HashMap::new();
    let mut names = Vec::new();
    for t in html::tags(frag).filter(|t| t.is_start()) {
        for a in html::attrs(&frag[t.start..t.end]).into_iter().filter(|a| !a.value.is_empty()) {
            if a.is("id") {
                m.entry(xml_unescape(a.value).into_owned()).or_insert(t.start);
            } else if a.is("name") && t.is("a") {
                names.push((a.value, t.start));
            }
        }
    }
    for (v, at) in names {
        m.entry(xml_unescape(v).into_owned()).or_insert(at);
    }
    m
}

/// `res`：图片路径 → (资源序号 1 起, mime)。`warnings` 收集找不到目标的链接/目录项（这些落到所在章节开头）。
pub fn layout(book: &Loaded, res: &HashMap<String, (u32, &'static str)>, warnings: &mut Vec<String>) -> Result<Layout, String> {
    let flows: HashMap<String, u32> = book.css.iter().enumerate().map(|(i, (p, _))| (p.clone(), i as u32 + 1)).collect();
    let css_flows: Vec<Vec<u8>> = book.css.iter().map(|(p, c)| rewrite_css(c, p, res).into_bytes()).collect();
    let doc_index: HashMap<String, usize> = book.docs.iter().enumerate().map(|(i, d)| (d.path.clone(), i)).collect();
    let mut docs = Vec::with_capacity(book.docs.len());
    for (i, d) in book.docs.iter().enumerate() {
        let cx = DocCtx { path: &d.path, res, flows: &flows, doc_index: &doc_index };
        docs.push(rewrite_doc(&d.html, &base32(i as u32, 1), &cx)?);
    }
    let ids: Vec<HashMap<String, usize>> = docs.iter().map(|d| id_offsets(&d.frag)).collect();
    let mut unresolved = 0usize;
    let mut resolve_off = |j: usize, frag: &str| -> u32 {
        if frag.is_empty() {
            return 0;
        }
        ids[j].get(frag).copied().unwrap_or_else(|| {
            unresolved += 1;
            0
        }) as u32
    };

    let mut flow0: Vec<u8> = Vec::new();
    let mut skeletons = Vec::new();
    let mut fragments = Vec::new();
    for (i, d) in docs.iter_mut().enumerate() {
        // 回填链接占位串（等长，偏移不变）
        for l in &d.links {
            let real = format!("kindle:pos:fid:{}:off:{}", base32(l.doc as u32, 4), base32(resolve_off(l.doc, &l.frag), 10));
            d.frag.replace_range(l.at..l.at + POS_PLACEHOLDER.len(), &real);
        }
        let start = flow0.len() as u32;
        skeletons.push((start, (d.head.len() + d.tail.len()) as u32));
        fragments.push(Fragment { insert_pos: start + d.head.len() as u32, len: d.frag.len() as u32, aid: base32(i as u32, 1) });
        flow0.extend_from_slice(d.head.as_bytes());
        flow0.extend_from_slice(d.tail.as_bytes());
        flow0.extend_from_slice(d.frag.as_bytes());
    }
    let ncx = book
        .toc
        .iter()
        .filter_map(|t| {
            let j = *doc_index.get(&t.path)?;
            let off = resolve_off(j, &t.frag);
            Some(NcxItem { label: t.label.clone(), level: t.level, pos: fragments[j].insert_pos + off, fid: j as u32, off })
        })
        .collect();
    if unresolved > 0 {
        warnings.push(format!("{unresolved} 个链接/目录项找不到目标 id，改指到所在章节开头"));
    }
    Ok(Layout { flow0, skeletons, fragments, css_flows, ncx })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc(path: &str, html: &str) -> bookconv::epubbook::Doc {
        bookconv::epubbook::Doc { path: path.into(), html: html.into() }
    }

    fn lay(docs: Vec<bookconv::epubbook::Doc>) -> (Layout, Vec<String>) {
        lay_with(docs, &HashMap::new())
    }

    fn lay_with(docs: Vec<bookconv::epubbook::Doc>, res: &HashMap<String, (u32, &'static str)>) -> (Layout, Vec<String>) {
        let book = Loaded { meta: Default::default(), docs, css: vec![], images: vec![], fonts: vec![], cover: None, toc: vec![] };
        let mut w = Vec::new();
        (layout(&book, res, &mut w).unwrap(), w)
    }

    #[test]
    fn stray_head_content_dropped_title_and_styles_kept() {
        let h = r#"<html><head><title>T</title><svg><image href="a.jpg"/></svg></div>*/ .x{a:b}<link rel="stylesheet" href="s.css"/><style>p{}</style></head><body><p>正文</p></body></html>"#;
        assert_eq!(clean_head(h), r#"<html><head><title>T</title><link rel="stylesheet" href="s.css"/><style>p{}</style></head><body><p>正文</p></body></html>"#);
        let ok = r#"<html><head><title>T</title></head><body/></html>"#;
        assert!(matches!(clean_head(ok), Cow::Borrowed(_)), "干净的不动");
    }

    #[test]
    fn gt_inside_quoted_attribute_does_not_cut_the_tag() {
        // 属性值里的 `>`：以前正则在这里把标签截断，图片丢、链接死
        let a = r#"<html><body><img title="x>y" src="images/a.png"/><a title='1>0' href="b.xhtml#s2">跳</a><!-- <img src="images/a.png"/> --></body></html>"#;
        let b = r#"<html><body><p>前</p><h2 id="s2">目标</h2></body></html>"#;
        let res = HashMap::from([("images/a.png".to_string(), (1u32, "image/png"))]);
        let (l, w) = lay_with(vec![doc("a.xhtml", a), doc("b.xhtml", b)], &res);
        let fa = frag_text(&l, 0);
        assert!(fa.starts_with(r#"<img title="x>y" src="kindle:embed:0001?mime=image/png"/>"#), "{fa}");
        let off = frag_text(&l, 1).find("<h2").unwrap() as u32;
        assert!(fa.contains(&format!(r#"<a title='1>0' href="kindle:pos:fid:0001:off:{}">"#, base32(off, 10))), "{fa}");
        assert!(fa.contains(r#"<!-- <img src="images/a.png"/> -->"#), "注释里的不改: {fa}");
        assert!(w.is_empty(), "{w:?}");
    }

    #[test]
    fn a_name_anchor_is_a_link_target_but_id_wins() {
        let a = r#"<html><body><a href="b.xhtml#n1">1</a><a href="b.xhtml#dup">2</a><a href="b.xhtml#x:y">3</a></body></html>"#;
        let b = r#"<html><body><p><a name="dup"></a>甲</p><p><a name="n1">乙</a></p><p id="dup">丙</p><p id="x:y">丁</p></body></html>"#;
        let (l, w) = lay(vec![doc("a.xhtml", a), doc("b.xhtml", b)]);
        let (fa, fb) = (frag_text(&l, 0), frag_text(&l, 1));
        let at = |needle: &str| base32(fb.find(needle).unwrap() as u32, 10);
        assert!(fa.contains(&format!(r#"off:{}">1<"#, at(r#"<a name="n1">"#))), "{fa}");
        assert!(fa.contains(&format!(r#"off:{}">2<"#, at(r#"<p id="dup">"#))), "id 优先于同名的 <a name>: {fa}");
        assert!(fa.contains(&format!(r#"off:{}">3<"#, at(r#"<p id="x:y">"#))), "锚点里有冒号不算外部链接: {fa}");
        assert!(w.is_empty(), "{w:?}");
    }

    /// 第 i 个片段的文字：流里每个文档是"骨架、片段"相连，片段紧跟在骨架后面。
    fn frag_text(l: &Layout, i: usize) -> String {
        let (start, len) = l.skeletons[i];
        let s = (start + len) as usize;
        String::from_utf8(l.flow0[s..s + l.fragments[i].len as usize].to_vec()).unwrap()
    }

    #[test]
    fn links_backfilled_by_offset_even_with_placeholder_text_quotes_and_encoding() {
        let a = r#"<html><head><link rel="x" href="b.xhtml"/></head><body><p>kindle:pos:fid:####:off:##########</p><a href='b.xhtml#caf%C3%A9'>1</a><a href="b.xhtml#nope">2</a></body></html>"#;
        let b = r#"<html><body><p>前文</p><p id='café'>目标</p></body></html>"#;
        let (l, w) = lay(vec![doc("a.xhtml", a), doc("b.xhtml", b)]);
        let fa = frag_text(&l, 0);
        assert!(fa.contains("<p>kindle:pos:fid:####:off:##########</p>"), "正文里的字面文字原样保留: {fa}");
        let fb = frag_text(&l, 1);
        let off = fb.find("<p id='café'>").unwrap() as u32;
        assert!(fa.contains(&format!("href=\"kindle:pos:fid:0001:off:{}\"", base32(off, 10))), "单引号 + 百分号编码的锚点要找到: {fa}");
        assert_eq!(w.len(), 1, "找不到的 #nope 要报出来: {w:?}");
    }

    #[test]
    fn stray_aid_removed_in_any_quote_and_href_entities_decoded() {
        let a = r#"<html><BODY aid='x'><p aid="y" data-aid='keep'>甲</p><a href="b%20c.xhtml#r&amp;d">1</a></Body></html>"#;
        let b = r#"<html><body><p>前</p><p id="r&amp;d">目标</p></body></html>"#;
        let (l, w) = lay(vec![doc("a.xhtml", a), doc("b c.xhtml", b)]);
        let head = String::from_utf8(l.flow0[..l.skeletons[0].1 as usize].to_vec()).unwrap();
        assert!(head.contains("<BODY aid=\"0\">") && !head.contains("'x'"), "body 上原有的单引号 aid 去掉、换成骨架的: {head}");
        let fa = frag_text(&l, 0);
        assert!(fa.starts_with("<p data-aid='keep'>甲</p>"), "data-aid 不误删: {fa}");
        let off = frag_text(&l, 1).find("<p id=").unwrap() as u32;
        assert!(fa.contains(&format!("kindle:pos:fid:0001:off:{}", base32(off, 10))), "href 里的 &amp; 先还原再对 id: {fa}");
        assert!(w.is_empty(), "{w:?}");
    }
}

