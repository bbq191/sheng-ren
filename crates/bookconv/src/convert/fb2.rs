//! FB2 → EPUB。quick-xml 逐事件解析成一棵小树，再按 FB2 结构映射到 `epub::Book`。
//!
//! 不用 `fb2` crate 的 serde 模型：它解析时把文字首尾空白裁掉，再按西文习惯在行内元素两侧**猜着补空格**
//! （`正文<a>[1]</a>` 会变成 `正文 [1]`），正文字符因此多出或少掉；它也只保留第一个 `<body>`。这里文字节点
//! 原样照搬，XML 里的空白交给阅读器按 HTML 规则处理，和 FB2 阅读器看到的一致。
//!
//! 映射：
//! - 主 `<body>`（第一个）：每个 `<section>` 一章（章名 = `<title>` 纯文本，正文开头渲成 `<hN>`，N = 嵌套深度），
//!   子 `<section>` 成后续更深一级的章；`<body>` 自带的 `<title>`（整书大标题）、题图、题词单独成首章。
//! - 其余 `<body>`（`name="notes"` 脚注、`comments` 评注等）：各成一章接在正文后面，里面的 `<section>` 不再拆章，
//!   整段放进 `<div id=原 id>`，节标题渲成加粗段落（脚注一条一页没法读）。
//! - `<a l:href="#id">`（脚注标号等）改成指向 id 所在章文件的真链接；找不到目标的只留链接文字。
//! - 二进制图片按序号命名（`images/binN.扩展名`），另建 id → 路径表：id 里有非 ASCII 字符时安全化会撞名。

use crate::epub::{Book, BookMeta, Chapter, Resource};
use base64::Engine;
use quick_xml::events::Event;
use regex::Regex;
use std::collections::HashMap;

/// FB2 字节 → (母版 EPUB 字节, 书名)。
pub fn fb2_to_epub(data: &[u8]) -> Result<(Vec<u8>, String), String> {
    let xml = decode_xml(data);
    let root = parse(&xml)?;
    let fb = root.child("FictionBook").ok_or("FB2 解析：没有 <FictionBook> 根元素")?;
    let ti = fb.child("description").and_then(|d| d.child("title-info"));
    let title = {
        let t = ti.and_then(|t| t.child("book-title")).map(|t| t.text()).unwrap_or_default();
        let t = t.trim();
        if t.is_empty() { "未命名".to_string() } else { t.to_string() }
    };

    // 二进制资源：base64 解码一次，按序号命名，建 id → (路径, 资源下标) 映射供图片引用与封面解析。
    let mut id_to_res: HashMap<String, (String, usize)> = HashMap::new();
    let mut resources: Vec<Resource> = Vec::new();
    for b in fb.children("binary") {
        let id = b.attr("id").unwrap_or_default().to_string();
        let bytes = match decode_b64(&b.text()) {
            Ok(v) => v,
            Err(e) => {
                eprintln!("警告：FB2 二进制 {id} 跳过：{e}");
                continue;
            }
        };
        let declared = b.attr("content-type").unwrap_or("").to_string();
        let (ext, media_type) = match super::common::image_ext_mime(&bytes) {
            Some((ext, mime)) => (ext, mime.to_string()),
            None => (ext_for_media(&declared), declared),
        };
        let path = format!("images/bin{}.{ext}", resources.len() + 1);
        id_to_res.insert(id, (path.clone(), resources.len()));
        resources.push(Resource { path, media_type, bytes });
    }
    let ctx = Ctx { images: &id_to_res };

    // 封面：coverpage 首图 href="#id" → 对应资源
    let cover_res = ti
        .and_then(|t| t.child("coverpage"))
        .and_then(|c| c.child("image"))
        .and_then(|im| im.attr("href"))
        .and_then(|h| id_to_res.get(h.trim_start_matches('#')))
        .map(|(_, i)| &resources[*i]);
    let (cover, cover_ext, cover_media_type) = match cover_res {
        Some(r) => (Some(r.bytes.clone()), r.path.rsplit('.').next().unwrap_or("jpg").to_string(), r.media_type.clone()),
        None => (None, "jpg".to_string(), "image/jpeg".to_string()),
    };

    let mut chapters: Vec<Chapter> = Vec::new();
    for (i, body) in fb.children("body").enumerate() {
        if i == 0 {
            render_main_body(body, &ctx, &mut chapters);
        } else {
            render_extra_body(body, &ctx, &mut chapters);
        }
    }
    if chapters.iter().all(|c| c.html_body.trim().is_empty()) {
        return Err("FB2 无可读正文".into());
    }
    resolve_links(&mut chapters);

    let author = ti.map(authors_to_string).unwrap_or_default();
    let lang = ti.and_then(|t| t.child("lang")).map(|l| l.text().trim().to_string()).unwrap_or_default();
    let mut book = Book {
        meta: BookMeta {
            book_id: format!("fb2:{}", super::common::sanitize_id(&title)),
            title: title.clone(),
            author,
            language: if lang.is_empty() { "zh".into() } else { lang },
            publisher: String::new(),
            cover,
            cover_ext,
            cover_media_type,
        },
        chapters,
        resources,
        nav: Vec::new(),
    };
    let epub = crate::epub::assemble_master(&mut book)?;
    Ok((epub, title))
}

// ── 解析 ────────────────────────────────────────────────────────────────────

/// 按 BOM / XML 声明里的 `encoding` 解码成 UTF-8（FB2 常见 windows-1251）。认不出的编码按 UTF-8。
fn decode_xml(data: &[u8]) -> String {
    if let Some((enc, bom_len)) = encoding_rs::Encoding::for_bom(data) {
        return enc.decode_without_bom_handling(&data[bom_len..]).0.into_owned();
    }
    let head = String::from_utf8_lossy(&data[..data.len().min(200)]);
    let label = head
        .find("?>")
        .map(|end| &head[..end])
        .and_then(|decl| decl.find("encoding=").map(|p| &decl[p + 9..]))
        .and_then(|rest| rest.split(['"', '\'']).nth(1));
    let enc = label.and_then(|l| encoding_rs::Encoding::for_label(l.trim().as_bytes())).unwrap_or(encoding_rs::UTF_8);
    enc.decode_without_bom_handling(data).0.into_owned()
}

enum Node {
    El(El),
    Text(String),
}

/// 元素：本地名（去掉命名空间前缀）+ 属性（本地名 → 值）+ 子节点。
struct El {
    name: String,
    attrs: Vec<(String, String)>,
    kids: Vec<Node>,
}

impl El {
    fn attr(&self, name: &str) -> Option<&str> {
        self.attrs.iter().find(|(k, _)| k == name).map(|(_, v)| v.as_str())
    }
    fn elements(&self) -> impl Iterator<Item = &El> {
        self.kids.iter().filter_map(|n| match n {
            Node::El(e) => Some(e),
            Node::Text(_) => None,
        })
    }
    fn children<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a El> {
        self.elements().filter(move |e| e.name == name)
    }
    fn child(&self, name: &str) -> Option<&El> {
        self.elements().find(|e| e.name == name)
    }
    /// 全部后代文字拼起来（不含标签）。
    fn text(&self) -> String {
        let mut s = String::new();
        self.collect_text(&mut s);
        s
    }
    fn collect_text(&self, out: &mut String) {
        for k in &self.kids {
            match k {
                Node::Text(t) => out.push_str(t),
                Node::El(e) => e.collect_text(out),
            }
        }
    }
}

fn open_el(e: &quick_xml::events::BytesStart) -> El {
    let name = String::from_utf8_lossy(e.local_name().as_ref()).into_owned();
    let attrs = e
        .attributes()
        .flatten()
        .map(|a| {
            let k = String::from_utf8_lossy(a.key.local_name().as_ref()).into_owned();
            let raw = String::from_utf8_lossy(&a.value);
            let v = quick_xml::escape::unescape(&raw).map(|v| v.into_owned()).unwrap_or_else(|_| raw.to_string());
            (k, v)
        })
        .collect();
    El { name, attrs, kids: Vec::new() }
}

/// XML → 小树（根是一个无名元素）。文字节点原样保留（包括空白）。
fn parse(xml: &str) -> Result<El, String> {
    let mut reader = quick_xml::Reader::from_str(xml);
    reader.check_end_names(false);
    let mut stack: Vec<El> = vec![El { name: String::new(), attrs: Vec::new(), kids: Vec::new() }];
    loop {
        let ev = reader.read_event().map_err(|e| format!("FB2 解析: {e}（位置 {}）", reader.buffer_position()))?;
        match ev {
            Event::Start(e) => stack.push(open_el(&e)),
            Event::Empty(e) => {
                let el = open_el(&e);
                stack.last_mut().unwrap().kids.push(Node::El(el));
            }
            Event::End(_) => {
                if stack.len() > 1 {
                    let el = stack.pop().unwrap();
                    stack.last_mut().unwrap().kids.push(Node::El(el));
                }
            }
            Event::Text(t) => {
                // 认不出的实体（如 `&nbsp;`）不让整本失败：原样当文字。
                let s = t.unescape().map(|s| s.into_owned()).unwrap_or_else(|_| String::from_utf8_lossy(&t).into_owned());
                stack.last_mut().unwrap().kids.push(Node::Text(s));
            }
            Event::CData(t) => stack.last_mut().unwrap().kids.push(Node::Text(String::from_utf8_lossy(&t).into_owned())),
            Event::Eof => break,
            _ => {}
        }
    }
    // 没闭合的元素依次收回父级（截断的文件尽量保住已读到的内容）。
    while stack.len() > 1 {
        let el = stack.pop().unwrap();
        stack.last_mut().unwrap().kids.push(Node::El(el));
    }
    Ok(stack.pop().unwrap())
}

// ── 渲染 ────────────────────────────────────────────────────────────────────

/// 图片 href（`#binid`）→ OEBPS 相对路径解析上下文。
struct Ctx<'a> {
    images: &'a HashMap<String, (String, usize)>,
}
impl Ctx<'_> {
    fn image(&self, el: &El) -> Option<&str> {
        el.attr("href").and_then(|h| self.images.get(h.trim_start_matches('#'))).map(|(p, _)| p.as_str())
    }
}

/// 书内链接先写成这个占位 scheme，全书渲染完、知道每个 id 落在哪章之后由 [`resolve_links`] 改写。
const LINK_MARK: &str = "fb2link:";

/// 文字与属性值都用它转义（含双引号，并丢掉 XML 不允许的控制字符）。
use crate::util::xml_escape as xesc;

fn id_attr(el: &El) -> String {
    el.attr("id").map(|id| format!(" id=\"{}\"", xesc(id))).unwrap_or_default()
}

/// 主 body：大标题/题图/题词成首章，各 section 递归成章。
fn render_main_body(body: &El, ctx: &Ctx, out: &mut Vec<Chapter>) {
    let mut html = String::new();
    let mut title = String::new();
    for el in body.elements().filter(|e| e.name != "section") {
        if el.name == "title" {
            title = title_plain(el);
            render_heading(el, 1, "", ctx, &mut html);
        } else {
            render_block(el, 2, ctx, &mut html);
        }
    }
    if !html.trim().is_empty() {
        out.push(Chapter { title, html_body: html, level: 1 });
    }
    for sec in body.children("section") {
        walk_section(sec, 1, ctx, out);
    }
}

/// 附属 body（脚注/评注）：整个 body 一章，section 不拆章、原样按顺序放进 `<div id>`。
fn render_extra_body(body: &El, ctx: &Ctx, out: &mut Vec<Chapter>) {
    let mut html = String::new();
    let mut title = String::new();
    for el in body.elements() {
        match el.name.as_str() {
            "title" => {
                title = title_plain(el);
                render_heading(el, 1, "", ctx, &mut html);
            }
            "section" => render_inline_section(el, ctx, &mut html),
            _ => render_block(el, 2, ctx, &mut html),
        }
    }
    if html.trim().is_empty() {
        return;
    }
    if title.is_empty() {
        // 目录里要有个名字（只进目录，不进正文）。
        title = match body.attr("name") {
            Some("notes") => "注释".into(),
            Some("comments") => "评注".into(),
            Some(n) if !n.trim().is_empty() => n.trim().to_string(),
            _ => String::new(),
        };
    }
    out.push(Chapter { title, html_body: html, level: 1 });
}

fn render_inline_section(sec: &El, ctx: &Ctx, out: &mut String) {
    out.push_str(&format!("<div{}>\n", id_attr(sec)));
    for el in sec.elements() {
        match el.name.as_str() {
            "title" => {
                for line in title_lines(el) {
                    out.push_str(&format!("<p{}><strong>", id_attr(line)));
                    render_inline_kids(line, ctx, out);
                    out.push_str("</strong></p>\n");
                }
            }
            "section" => render_inline_section(el, ctx, out),
            _ => render_block(el, 2, ctx, out),
        }
    }
    // section 里直接放文字（不合规但常见）：包成段落，不丢
    push_loose_text(sec, out);
    out.push_str("</div>\n");
}

/// 递归展开一个 section：本节内容成一章，子节成后续更深一级的章。子节之后还有内容时另起一个无标题章接上，
/// 保持原文顺序、不丢内容。
fn walk_section(sec: &El, depth: i64, ctx: &Ctx, out: &mut Vec<Chapter>) {
    let mut title = String::new();
    let mut html = String::new();
    let mut id_pending = sec.attr("id").is_some();
    let flush = |title: &mut String, html: &mut String, level: i64, out: &mut Vec<Chapter>| {
        if !html.trim().is_empty() || !title.is_empty() {
            out.push(Chapter { title: std::mem::take(title), html_body: std::mem::take(html), level });
        }
    };
    for k in &sec.kids {
        let el = match k {
            Node::El(e) => e,
            Node::Text(t) => {
                if !t.trim().is_empty() {
                    html.push_str(&format!("<p>{}</p>\n", xesc(t.trim())));
                }
                continue;
            }
        };
        match el.name.as_str() {
            "title" => {
                title = title_plain(el);
                let id = if id_pending { id_attr(sec) } else { String::new() };
                id_pending = false;
                render_heading(el, depth, &id, ctx, &mut html);
            }
            "section" => {
                flush(&mut title, &mut html, depth, out);
                walk_section(el, depth + 1, ctx, out);
            }
            _ => {
                if id_pending {
                    // 没有标题的节：id 放在一个空锚点上，链接照样能跳到节首。
                    html.push_str(&format!("<a{}></a>", id_attr(sec)));
                    id_pending = false;
                }
                render_block(el, depth + 1, ctx, &mut html);
            }
        }
    }
    flush(&mut title, &mut html, depth, out);
}

/// `<title>` 渲成 `<hN>`，多行之间用 `<br/>`。
fn render_heading(title: &El, depth: i64, id: &str, ctx: &Ctx, out: &mut String) {
    let n = depth.clamp(1, 6);
    out.push_str(&format!("<h{n}{id}>"));
    for (i, line) in title_lines(title).into_iter().enumerate() {
        if i > 0 {
            out.push_str("<br/>");
        }
        render_inline_kids(line, ctx, out);
    }
    out.push_str(&format!("</h{n}>\n"));
}

/// `<title>` 里的行：各个 `<p>`（`<empty-line/>` 跳过）。没有 `<p>`、文字直接写在 `<title>` 里（不合规但常见，
/// `<title>第一章 文字</title>`）时整个 `<title>` 算一行，不然章标题渲成空的 `<hN>`、目录里也没有名字。
fn title_lines(title: &El) -> Vec<&El> {
    let lines: Vec<&El> = title.children("p").collect();
    if lines.is_empty() && !title.text().trim().is_empty() {
        return vec![title];
    }
    lines
}

/// 标题纯文本（进 `<title>`/目录，不要标签）：各行去首尾空白后用空格连接。
fn title_plain(t: &El) -> String {
    let lines: Vec<String> = title_lines(t).into_iter().map(|p| p.text().trim().to_string()).filter(|s| !s.is_empty()).collect();
    lines.join(" ")
}

/// 块级容器：`<tag{attrs} id>…</tag>`。`attrs` 是写在开标签里的固定属性（带前导空格，如 ` class="poem"`），
/// 不进闭合标签。
fn wrap_block(tag: &str, attrs: &str, el: &El, depth: i64, ctx: &Ctx, out: &mut String) {
    out.push_str(&format!("<{tag}{attrs}{}>\n", id_attr(el)));
    for k in el.elements() {
        render_block(k, depth, ctx, out);
    }
    push_loose_text(el, out);
    out.push_str(&format!("</{tag}>\n"));
}

/// 块级容器里直接放的文字（不合规但常见）包成段落，不丢。
fn push_loose_text(el: &El, out: &mut String) {
    for k in &el.kids {
        if let Node::Text(t) = k {
            if !t.trim().is_empty() {
                out.push_str(&format!("<p>{}</p>\n", xesc(t.trim())));
            }
        }
    }
}

/// 块级元素 → XHTML。`depth` 决定 `<subtitle>` 等小标题的级别。
fn render_block(el: &El, depth: i64, ctx: &Ctx, out: &mut String) {
    let para = |tag: &str, out: &mut String| {
        out.push_str(&format!("<{tag}{}>", id_attr(el)));
        render_inline_kids(el, ctx, out);
        out.push_str(&format!("</{tag}>\n"));
    };
    match el.name.as_str() {
        "p" | "v" | "text-author" | "date" => para("p", out),
        "subtitle" => para(&format!("h{}", depth.clamp(3, 6)), out),
        "empty-line" => out.push_str("<br/>\n"),
        "image" => {
            if let Some(src) = ctx.image(el) {
                let alt = el.attr("alt").unwrap_or("");
                out.push_str(&format!("<p{}><img src=\"{}\" alt=\"{}\"/></p>\n", id_attr(el), xesc(src), xesc(alt)));
            }
        }
        "title" => render_heading(el, (depth + 1).max(4), &id_attr(el), ctx, out),
        "poem" => wrap_block("div", " class=\"poem\"", el, depth, ctx, out),
        "stanza" | "annotation" | "section" => wrap_block("div", "", el, depth, ctx, out),
        "cite" | "epigraph" => wrap_block("blockquote", "", el, depth, ctx, out),
        "table" => {
            out.push_str(&format!("<table{}>\n", id_attr(el)));
            for tr in el.children("tr") {
                out.push_str("<tr>");
                for cell in tr.elements().filter(|c| c.name == "th" || c.name == "td") {
                    let span: String = ["colspan", "rowspan"]
                        .iter()
                        .filter_map(|a| cell.attr(a).map(|v| format!(" {a}=\"{}\"", xesc(v))))
                        .collect();
                    out.push_str(&format!("<{}{span}>", cell.name));
                    render_inline_kids(cell, ctx, out);
                    out.push_str(&format!("</{}>", cell.name));
                }
                out.push_str("</tr>\n");
            }
            out.push_str("</table>\n");
        }
        // 认不出的块元素：有元素子节点就当容器，否则当段落（文字不丢）
        _ if el.elements().next().is_some() => wrap_block("div", "", el, depth, ctx, out),
        _ if !el.text().trim().is_empty() => para("p", out),
        _ => {}
    }
}

/// 行内内容：文字原样（只做 XML 转义），样式元素映射成对应标签，命名样式透明穿透。
fn render_inline_kids(el: &El, ctx: &Ctx, out: &mut String) {
    for k in &el.kids {
        match k {
            Node::Text(t) => out.push_str(&xesc(t)),
            Node::El(e) => render_inline(e, ctx, out),
        }
    }
}

fn render_inline(el: &El, ctx: &Ctx, out: &mut String) {
    let tag = match el.name.as_str() {
        "strong" => "strong",
        "emphasis" => "em",
        "strikethrough" => "s",
        "sub" => "sub",
        "sup" => "sup",
        "code" => "code",
        "image" => {
            if let Some(src) = ctx.image(el) {
                let alt = el.attr("alt").unwrap_or("");
                out.push_str(&format!("<img src=\"{}\" alt=\"{}\"/>", xesc(src), xesc(alt)));
            }
            return;
        }
        "a" => {
            let href = el.attr("href").unwrap_or("").trim();
            let href = match href.strip_prefix('#') {
                Some(id) => format!("{LINK_MARK}{id}"),
                None => href.to_string(),
            };
            if href.is_empty() {
                render_inline_kids(el, ctx, out);
            } else {
                out.push_str(&format!("<a href=\"{}\">", xesc(&href)));
                render_inline_kids(el, ctx, out);
                out.push_str("</a>");
            }
            return;
        }
        // style（命名样式）及认不出的行内元素：拆壳保内容
        _ => {
            render_inline_kids(el, ctx, out);
            return;
        }
    };
    out.push_str(&format!("<{tag}{}>", id_attr(el)));
    render_inline_kids(el, ctx, out);
    out.push_str(&format!("</{tag}>"));
}

/// 书内链接占位 → `chap_N.xhtml#id`（id 所在的章）。目标 id 不存在的链接去掉 `<a>`、只留文字。
fn resolve_links(chapters: &mut [Chapter]) {
    // 占位链接是本模块自己写的（固定双引号、无其它属性），这条正则只认它；id 用 `crate::html` 扫（`data-id` 不算）。
    static RE_A: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
    let re_a = RE_A.get_or_init(|| Regex::new(&format!(r#"(?s)<a href="{LINK_MARK}([^"]*)">(.*?)</a>"#)).unwrap());
    let mut where_id: HashMap<String, usize> = HashMap::new();
    for (i, ch) in chapters.iter().enumerate() {
        for t in crate::html::tags(&ch.html_body).filter(|t| t.is_start()) {
            if let Some(id) = crate::html::attr_value(&ch.html_body[t.start..t.end], "id") {
                where_id.entry(id.to_string()).or_insert(i);
            }
        }
    }
    for ch in chapters.iter_mut() {
        if !ch.html_body.contains(LINK_MARK) {
            continue;
        }
        ch.html_body = re_a
            .replace_all(&ch.html_body, |c: &regex::Captures| match where_id.get(&c[1]) {
                Some(&i) => format!(r#"<a href="{}#{}">{}</a>"#, crate::epub::chapter_filename(i), &c[1], &c[2]),
                None => c[2].to_string(),
            })
            .into_owned();
    }
}

fn authors_to_string(ti: &El) -> String {
    let names: Vec<String> = ti
        .children("author")
        .filter_map(|a| {
            let part = |n: &str| a.child(n).map(|e| e.text().trim().to_string()).unwrap_or_default();
            let full: Vec<String> = ["first-name", "middle-name", "last-name"].iter().map(|n| part(n)).filter(|s| !s.is_empty()).collect();
            let name = if full.is_empty() { part("nickname") } else { full.join(" ") };
            (!name.is_empty()).then_some(name)
        })
        .collect();
    names.join("、")
}

fn decode_b64(content: &str) -> Result<Vec<u8>, String> {
    // FB2 base64 常含换行/空白，先剥净
    let cleaned: String = content.chars().filter(|c| !c.is_whitespace()).collect();
    base64::engine::general_purpose::STANDARD
        .decode(cleaned.as_bytes())
        .map_err(|e| format!("base64 解码: {e}"))
}

fn ext_for_media(mt: &str) -> &'static str {
    match mt.to_ascii_lowercase().as_str() {
        "image/jpeg" | "image/jpg" => "jpg",
        "image/png" => "png",
        "image/gif" => "gif",
        "image/svg+xml" => "svg",
        _ => "img",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // 最小 FB2：标题 + 作者 + 一张 1x1 PNG 封面(#cover) + 一节含一段带内联图。
    const RED_PNG_B64: &str = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAIAAACQd1PeAAAADElEQVQI12P4z8AAAAMBAQAY3Y2wAAAAAElFTkSuQmCC";

    fn sample_fb2() -> String {
        format!(
            r##"<?xml version="1.0" encoding="utf-8"?>
<FictionBook xmlns="http://www.gribuser.ru/xml/fictionbook/2.0" xmlns:l="http://www.w3.org/1999/xlink">
<description><title-info>
<book-title>测试书</book-title>
<author><first-name>张</first-name><last-name>三</last-name></author>
<coverpage><image l:href="#cover"/></coverpage>
<lang>zh</lang>
</title-info></description>
<body>
<section>
<title><p>第一章</p></title>
<p>正文<emphasis>强调</emphasis>结束<a l:href="#n1" type="note">[1]</a>。</p>
<p><image l:href="#pic1"/></p>
</section>
</body>
<body name="notes">
<section id="n1"><title><p>1</p></title><p>这是脚注正文。</p></section>
</body>
<binary id="cover" content-type="image/png">{png}</binary>
<binary id="pic1" content-type="image/png">{png}</binary>
</FictionBook>"##,
            png = RED_PNG_B64
        )
    }

    fn chapters_of(epub: &[u8]) -> Vec<(String, String)> {
        crate::epubzip::read_entries(epub)
            .unwrap()
            .into_iter()
            .filter(|e| e.name.contains("chap_"))
            .map(|e| (e.name.rsplit('/').next().unwrap().to_string(), String::from_utf8(e.data).unwrap()))
            .collect()
    }

    #[test]
    fn parses_and_builds_epub() {
        let (epub, title) = fb2_to_epub(sample_fb2().as_bytes()).unwrap();
        assert_eq!(title, "测试书");
        let names: Vec<String> = crate::epubzip::read_entries(&epub).unwrap().into_iter().map(|e| e.name).collect();
        assert!(names.iter().any(|n| n == "OEBPS/images/bin2.png"), "缺内联图资源: {names:?}");
        assert!(names.iter().any(|n| n.ends_with("cover.png")), "缺封面: {names:?}");
        let chs = chapters_of(&epub);
        let chap = &chs[0].1;
        assert!(chap.contains("<h1>第一章</h1>"), "章标题要进正文: {chap}");
        assert!(chap.contains("正文<em>强调</em>结束<a href="), "行内元素两侧不许多出空格: {chap}");
        assert!(chap.contains("src=\"images/bin2.png\""), "缺内联图: {chap}");
    }

    #[test]
    fn notes_body_kept_and_footnote_links_work() {
        let (epub, _) = fb2_to_epub(sample_fb2().as_bytes()).unwrap();
        let chs = chapters_of(&epub);
        assert_eq!(chs.len(), 2, "正文一章 + 脚注一章");
        let (notes_file, notes) = &chs[1];
        assert!(notes.contains("这是脚注正文。") && notes.contains(r#"<div id="n1">"#), "脚注 body 不能丢: {notes}");
        assert!(chs[0].1.contains(&format!(r#"<a href="{notes_file}#n1">[1]</a>"#)), "脚注标号要链到脚注: {}", chs[0].1);
    }

    #[test]
    fn non_ascii_binary_ids_do_not_collide() {
        let fb2 = format!(
            r##"<?xml version="1.0" encoding="utf-8"?>
<FictionBook xmlns="http://www.gribuser.ru/xml/fictionbook/2.0" xmlns:l="http://www.w3.org/1999/xlink">
<description><title-info><book-title>图</book-title></title-info></description>
<body><section><p><image l:href="#图一"/><image l:href="#图二"/></p></section></body>
<binary id="图一" content-type="image/png">{png}</binary>
<binary id="图二" content-type="image/png">{png}</binary>
</FictionBook>"##,
            png = RED_PNG_B64
        );
        let (epub, _) = fb2_to_epub(fb2.as_bytes()).unwrap();
        let chap = &chapters_of(&epub)[0].1;
        assert!(chap.contains("images/bin1.png") && chap.contains("images/bin2.png"), "{chap}");
    }

    #[test]
    fn poem_div_closes_with_bare_tag_name() {
        let fb2 = r#"<?xml version="1.0" encoding="utf-8"?><FictionBook><description><title-info><book-title>诗</book-title></title-info></description><body><section><poem><stanza><v>床前明月光</v></stanza></poem></section></body></FictionBook>"#;
        let (epub, _) = fb2_to_epub(fb2.as_bytes()).unwrap();
        let chap = &chapters_of(&epub)[0].1;
        assert!(chap.contains(r#"<div class="poem">"#), "{chap}");
        assert!(!chap.contains("</div class"), "闭合标签不能带属性: {chap}");
        assert_eq!(chap.matches("<div").count(), chap.matches("</div>").count(), "{chap}");
        let rep = crate::check::check_entries(&crate::epubzip::read_entries(&epub).unwrap(), false);
        assert!(rep.warnings.iter().all(|w| !w.contains("不是合法 XML")), "{:?}", rep.warnings);
    }

    #[test]
    fn title_without_p_renders_its_inline_text() {
        let fb2 = r##"<?xml version="1.0" encoding="utf-8"?><FictionBook><description><title-info><book-title>书</book-title></title-info></description><body><section><title>第一章 <emphasis>文字</emphasis></title><p>正文</p></section></body><body name="notes"><section id="n1"><title>1</title><p>注</p></section></body></FictionBook>"##;
        let (epub, _) = fb2_to_epub(fb2.as_bytes()).unwrap();
        let chs = chapters_of(&epub);
        assert!(chs[0].1.contains("<h1>第一章 <em>文字</em></h1>"), "{}", chs[0].1);
        assert!(chs[0].1.contains("<title>第一章 文字</title>"), "章名取标题纯文本: {}", chs[0].1);
        assert!(chs[1].1.contains(r#"<p><strong>1</strong></p>"#), "脚注节标题: {}", chs[1].1);
        let nav = String::from_utf8(crate::epubzip::read_entries(&epub).unwrap().into_iter().find(|e| e.name.ends_with("nav.xhtml")).unwrap().data).unwrap();
        assert!(nav.contains(">第一章 文字</a>"), "{nav}");
    }

    #[test]
    fn windows_1251_and_nested_sections_keep_order() {
        let head = "<?xml version=\"1.0\" encoding=\"windows-1251\"?><FictionBook><description><title-info><book-title>";
        let mut data = head.as_bytes().to_vec();
        data.extend_from_slice(&[0xCA, 0xED, 0xE8, 0xE3, 0xE0]); // «Книга»
        data.extend_from_slice(b"</book-title></title-info></description><body><section><title><p>A</p></title><p>a1</p><section><title><p>B</p></title><p>b1</p></section><p>a2</p></section></body></FictionBook>");
        let (epub, title) = fb2_to_epub(&data).unwrap();
        assert_eq!(title, "Книга");
        let all: String = chapters_of(&epub).into_iter().map(|(_, h)| h).collect();
        let (a1, b1, a2) = (all.find("a1").unwrap(), all.find("b1").unwrap(), all.find("a2").unwrap());
        assert!(a1 < b1 && b1 < a2, "子节之后的内容接在后面，不丢不乱序");
    }
}
