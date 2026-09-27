//! 正文排版：每个 XHTML 文档拆成**骨架**（`<html><head>…<body aid="X"></body></html>`）和**片段**（body 里的内容），
//! 在 rawML 里依次存"骨架、片段、骨架、片段…"；片段在阅读器里插回骨架的 `</body>` 之前（KF8 样本黑盒分析得出）。
//!
//! 引用改写：图片 → `kindle:embed:XXXX?mime=…`（资源序号，1 起，base32 四位）；CSS → `kindle:flow:XXXX?mime=text/css`；
//! 书内链接 → `kindle:pos:fid:XXXX:off:YYYYYYYYYY`（fid = 片段序号，off = 目标在片段里的字节偏移，base32）。
//! 链接先写成等长占位串，所有文档排完、偏移定下来之后再回填，回填不改变任何偏移。

use crate::book::Loaded;
use bookconv::epubzip::{dir_of, percent_decode, posix_norm, resolve};
use regex::Regex;
use std::collections::HashMap;
use std::sync::OnceLock;

const B32: &[u8] = b"0123456789ABCDEFGHIJKLMNOPQRSTUV";

pub fn base32(mut v: u32, width: usize) -> String {
    let mut s = Vec::new();
    loop {
        s.push(B32[(v % 32) as usize]);
        v /= 32;
        if v == 0 {
            break;
        }
    }
    while s.len() < width {
        s.push(b'0');
    }
    s.reverse();
    String::from_utf8(s).unwrap()
}

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

/// 标签里的 href/src 属性（单双引号都认）。
fn attr_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r#"(?i)(\s)(href|src|xlink:href)(\s*=\s*)(?:"([^"]*)"|'([^']*)')"#).unwrap())
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
fn rewrite_tag(tag: &str, name: &str, cx: &DocCtx) -> (String, Vec<(usize, usize, String)>) {
    let attr = |a: &regex::Captures| a.get(4).or(a.get(5)).map_or("", |m| m.as_str()).to_string();
    let is_css_link = name == "link" && tag.to_ascii_lowercase().contains("stylesheet");
    // 非样式表的 <link>（Adobe 的 page-template.xpgt 等）和指向书里不存在的样式表的 <link>，Kindle 用不上，去掉。
    if name == "link" {
        let href = attr_re().captures_iter(tag).find(|a| a[2].eq_ignore_ascii_case("href")).map(|a| attr(&a)).unwrap_or_default();
        let target = posix_norm(&resolve(dir_of(cx.path), &percent_decode(href.split('#').next().unwrap_or(""))));
        if !is_css_link || !cx.flows.contains_key(&target) {
            return (String::new(), Vec::new());
        }
    }
    let mut out = String::with_capacity(tag.len());
    let mut links = Vec::new();
    let mut last = 0;
    for a in attr_re().captures_iter(tag) {
        let m = a.get(0).unwrap();
        let v = attr(&a);
        if v.contains(':') && !v.starts_with('#') {
            continue; // http:、mailto:、data: 等外部地址
        }
        let (p, frag) = v.split_once('#').unwrap_or((&v, ""));
        let target = if p.is_empty() { cx.path.to_string() } else { posix_norm(&resolve(dir_of(cx.path), &percent_decode(p))) };
        let new = if is_css_link {
            match cx.flows.get(&target) {
                Some(n) => format!("kindle:flow:{}?mime=text/css", base32(*n, 4)),
                None => continue,
            }
        } else if let Some((n, mime)) = cx.res.get(&target).filter(|_| name != "a") {
            format!("kindle:embed:{}?mime={mime}", base32(*n, 4))
        } else if let Some(&j) = cx.doc_index.get(&target).filter(|_| name == "a" || name == "area") {
            out.push_str(&tag[last..m.start()]);
            let prefix = format!("{}{}{}\"", &a[1], &a[2], &a[3]);
            links.push((out.len() + prefix.len(), j, percent_decode(frag)));
            out.push_str(&prefix);
            out.push_str(POS_PLACEHOLDER);
            out.push('"');
            last = m.end();
            continue;
        } else {
            continue;
        };
        out.push_str(&tag[last..m.start()]);
        out.push_str(&format!("{}{}{}\"{new}\"", &a[1], &a[2], &a[3]));
        last = m.end();
    }
    out.push_str(&tag[last..]);
    (out, links)
}

fn rewrite_doc(html: &str, aid: &str, cx: &DocCtx) -> Result<Rewritten, String> {
    static SCRIPT: OnceLock<Regex> = OnceLock::new();
    static TAG: OnceLock<Regex> = OnceLock::new();
    static AID: OnceLock<Regex> = OnceLock::new();
    static STYLE: OnceLock<Regex> = OnceLock::new();
    let path = cx.path;
    let html = SCRIPT.get_or_init(|| Regex::new(r#"(?is)<script\b.*?</script>"#).unwrap()).replace_all(html, "");
    let html = STYLE
        .get_or_init(|| Regex::new(r#"(?is)(<style\b[^>]*>)(.*?)(</style>)"#).unwrap())
        .replace_all(&html, |c: &regex::Captures| format!("{}{}{}", &c[1], rewrite_css(&c[2], path, cx.res), &c[3]));
    let aid_re = AID.get_or_init(|| Regex::new(r#"(?i)\s+aid\s*=\s*"[^"]*""#).unwrap());
    // 注释原样跳过（里面的标签不改写）；只认开标签
    let tag_re = TAG.get_or_init(|| Regex::new(r#"(?s)<!--.*?-->|<([A-Za-z][A-Za-z0-9:]*)\b[^>]*>"#).unwrap());
    let mut out = String::with_capacity(html.len() + html.len() / 8);
    let mut links: Vec<(usize, usize, String)> = Vec::new();
    let mut body_open: Option<(usize, usize)> = None; // body 开标签在 out 里的 (起, 止)
    let mut last = 0;
    for c in tag_re.captures_iter(&html) {
        let Some(name) = c.get(1) else { continue };
        let m = c.get(0).unwrap();
        out.push_str(&html[last..m.start()]);
        last = m.end();
        let name = name.as_str().to_ascii_lowercase();
        let tag = aid_re.replace_all(m.as_str(), "");
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
    let close = out.to_ascii_lowercase().rfind("</body>").filter(|&c| c >= b_end).ok_or_else(|| format!("{path} 没有 </body>"))?;
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

/// 片段里各 id 的字节偏移（单双引号都认；同名只记第一个）。
fn id_offsets(frag: &str) -> HashMap<String, usize> {
    static ID: OnceLock<Regex> = OnceLock::new();
    let re = ID.get_or_init(|| Regex::new(r#"(?i)<[A-Za-z][^>]*?\sid\s*=\s*(?:"([^"]+)"|'([^']+)')"#).unwrap());
    let mut m = HashMap::new();
    for c in re.captures_iter(frag) {
        let id = c.get(1).or(c.get(2)).unwrap().as_str();
        m.entry(bookconv::util::xml_unescape(id).into_owned()).or_insert(c.get(0).unwrap().start());
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

    #[test]
    fn base32_matches_kindle_digits() {
        assert_eq!(base32(9, 4), "0009");
        assert_eq!(base32(31, 4), "000V");
        assert_eq!(base32(32, 4), "0010");
        assert_eq!(base32(0, 10), "0000000000");
        assert_eq!(POS_PLACEHOLDER.len(), "kindle:pos:fid:0000:off:0000000000".len());
    }

    fn doc(path: &str, html: &str) -> crate::book::Doc {
        crate::book::Doc { path: path.into(), html: html.into() }
    }

    fn lay(docs: Vec<crate::book::Doc>) -> (Layout, Vec<String>) {
        let book = Loaded { meta: Default::default(), docs, css: vec![], images: vec![], cover: None, toc: vec![] };
        let mut w = Vec::new();
        (layout(&book, &HashMap::new(), &mut w).unwrap(), w)
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
}

