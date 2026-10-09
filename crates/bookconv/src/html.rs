//! XHTML 文本工具：清洗层（`wash`）与 XHTML 规则（`htmlproc`）共用的一套容错解析（2026-09-27 审计后统一）。
//!
//! 此前各处各写一条正则，毛病是同一类：只认双引号属性、`\bid="` 会命中 `data-id="`、注释或属性值里的 `>`
//! 会把标签提前截断、找不到属性就再追加一个（产生重复属性，xochitl 严格 XML 整章白屏）。这里只认"标签 + 属性"
//! 这一层，不建 DOM：
//! - [`tags`]：按文档序扫出每个标签。注释、CDATA、`<!DOCTYPE>`、`<?xml?>` 整体算一个（注释里的 `<p>` 不算标签）；
//!   属性值里引号内的 `>` 不会截断标签；认不出来的 `<`（`a < b`）当普通文字。
//! - [`attrs`]/[`attr`]：按**完整属性名**找（不分大小写；`id` 不会命中 `data-id`/`aid`），双引号、单引号、
//!   无引号的值都认，带值和字节范围，调用方就地改值（[`set_attr`]/[`edit_attrs`]），不追加重复属性。
//! - [`plain_text`]/[`has_visible`]/[`last_visible_end`]：纯文本与"读者看得见的内容"，全书一套口径（见 [`has_visible`]）。
//! - [`parse_spans`]：元素的开闭位置（容错：多余的闭合标签忽略，没闭合的在祖先闭合处隐式闭合）。
//! - [`split_href`]/[`frag_id`]：链接拆成路径与锚点；锚点先百分号解码再拿去对 id。
//!
//! 属性值一律是**原文**（字符引用未还原）；id、锚点比较都按原文比。
use crate::util::xml_unescape;
use regex::Regex;
use std::borrow::Cow;
use std::sync::OnceLock;

/// 空元素（没有闭合标签）。
pub const VOID_ELEMENTS: &[&str] = &["area", "base", "br", "col", "embed", "hr", "img", "input", "link", "meta", "param", "source", "track", "wbr"];

/// 块级元素：它们的闭合标签标志着"一块结束"（注释环判定、假段落切分等用）。
pub const BLOCK_ELEMENTS: &[&str] = &[
    "p", "div", "h1", "h2", "h3", "h4", "h5", "h6", "li", "ul", "ol", "dl", "dt", "dd", "blockquote", "section", "article", "aside", "nav", "header", "footer",
    "table", "tr", "td", "th", "pre", "figure", "figcaption",
];

/// 读者看得见的非文字元素（图片、分隔线、表格、公式、音视频）。有它们就算"有内容"，哪怕没有文字。
pub const MEDIA_ELEMENTS: &[&str] = &["img", "svg", "image", "hr", "table", "video", "audio", "math", "object"];

pub fn is_void(name: &str) -> bool {
    VOID_ELEMENTS.iter().any(|v| v.eq_ignore_ascii_case(name))
}

pub fn is_block(name: &str) -> bool {
    BLOCK_ELEMENTS.iter().any(|v| v.eq_ignore_ascii_case(name))
}

fn is_media(name: &str) -> bool {
    MEDIA_ELEMENTS.iter().any(|v| v.eq_ignore_ascii_case(name))
}

/// 内容不是正文的元素（样式、脚本、标题栏）：纯文本、可见性判定跳过它们的内容。
fn is_non_text(name: &str) -> bool {
    name.eq_ignore_ascii_case("style") || name.eq_ignore_ascii_case("script") || name.eq_ignore_ascii_case("title")
}

// ───────────────────────── 标签扫描 ─────────────────────────

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TagKind {
    /// `<p …>`
    Open,
    /// `<br/>`、`<p …/>`
    SelfClosing,
    /// `</p>`
    Close,
    /// 注释、CDATA、`<!DOCTYPE>`、`<?xml?>`（`name` 为空）。
    Other,
}

#[derive(Clone, Copy, Debug)]
pub struct Tag<'a> {
    pub kind: TagKind,
    /// 元素名原文（大小写照原样；比较用 [`Tag::is`]）。
    pub name: &'a str,
    pub start: usize,
    pub end: usize,
}

impl Tag<'_> {
    pub fn is(&self, name: &str) -> bool {
        self.name.eq_ignore_ascii_case(name)
    }
    /// 开标签或自闭合标签。
    pub fn is_start(&self) -> bool {
        matches!(self.kind, TagKind::Open | TagKind::SelfClosing)
    }
    /// `<h1>`–`<h6>` 的级别。
    pub fn heading_level(&self) -> Option<u8> {
        heading_level_of(self.name)
    }
}

/// 元素名是 `h1`–`h6`（不分大小写）时的级别；`h0`、`h7` 之类不算。
pub fn heading_level_of(name: &str) -> Option<u8> {
    let b = name.as_bytes();
    (b.len() == 2 && b[0].eq_ignore_ascii_case(&b'h') && (b'1'..=b'6').contains(&b[1])).then(|| b[1] - b'0')
}

/// [`tags`] 的迭代器。
pub struct Tags<'a> {
    html: &'a str,
    pos: usize,
    end: usize,
}

fn is_name_byte(c: u8) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, b':' | b'_' | b'-' | b'.')
}

/// 从 `i`（元素名之后）扫属性，返回标签结束位置（`>` 之后）与是否自闭合；每个属性回调一次。
/// 引号没闭合、到 `limit` 都没有 `>` 时返回 `None`（调用方退回"到下一个 `>`"）。
fn scan_attrs<'a>(s: &'a str, mut i: usize, limit: usize, mut visit: impl FnMut(Attr<'a>)) -> Option<(usize, bool)> {
    let b = s.as_bytes();
    loop {
        while i < limit && b[i].is_ascii_whitespace() {
            i += 1;
        }
        if i >= limit {
            return None;
        }
        match b[i] {
            b'>' => return Some((i + 1, false)),
            b'/' if i + 1 < limit && b[i + 1] == b'>' => return Some((i + 2, true)),
            b'/' => {
                i += 1;
                continue;
            }
            _ => {}
        }
        let name_start = i;
        while i < limit && !b[i].is_ascii_whitespace() && !matches!(b[i], b'=' | b'>' | b'/') {
            i += 1;
        }
        let name_end = i;
        let mut j = i;
        while j < limit && b[j].is_ascii_whitespace() {
            j += 1;
        }
        if j < limit && b[j] == b'=' {
            j += 1;
            while j < limit && b[j].is_ascii_whitespace() {
                j += 1;
            }
            if j >= limit {
                return None;
            }
            let (vs, ve, next) = if b[j] == b'"' || b[j] == b'\'' {
                let q = b[j];
                let close = s[j + 1..limit].bytes().position(|c| c == q)? + j + 1;
                (j + 1, close, close + 1)
            } else {
                let mut k = j;
                while k < limit && !b[k].is_ascii_whitespace() && b[k] != b'>' {
                    k += 1;
                }
                (j, k, k)
            };
            visit(Attr { name: &s[name_start..name_end], value: &s[vs..ve], start: name_start, end: next, value_start: vs, value_end: ve, quote: (next > ve).then(|| b[ve] as char) });
            i = next;
        } else {
            if name_end > name_start {
                visit(Attr { name: &s[name_start..name_end], value: "", start: name_start, end: name_end, value_start: name_end, value_end: name_end, quote: None });
            } else {
                i += 1; // 杂散字符（引号等），跳过
            }
            i = i.max(name_end);
        }
    }
}

impl<'a> Iterator for Tags<'a> {
    type Item = Tag<'a>;
    fn next(&mut self) -> Option<Tag<'a>> {
        let s = self.html;
        let b = s.as_bytes();
        loop {
            let st = self.pos + s[self.pos..self.end].find('<')?;
            let rest = &s[st..self.end];
            // 没结束的注释/CDATA 吞到扫描范围末尾。
            let (kind, name, end) = if let Some(r) = rest.strip_prefix("<!--") {
                (TagKind::Other, "", r.find("-->").map_or(self.end, |i| st + i + 7))
            } else if let Some(r) = rest.strip_prefix("<![CDATA[") {
                (TagKind::Other, "", r.find("]]>").map_or(self.end, |i| st + i + 12))
            } else if rest.starts_with("<!") || rest.starts_with("<?") {
                let e = st + rest.find('>')? + 1;
                (TagKind::Other, "", e)
            } else if rest.len() > 2 && b[st + 1] == b'/' && b[st + 2].is_ascii_alphabetic() {
                let mut i = st + 2;
                while i < self.end && is_name_byte(b[i]) {
                    i += 1;
                }
                let e = st + rest.find('>')? + 1;
                (TagKind::Close, &s[st + 2..i], e)
            } else if rest.len() > 1 && b[st + 1].is_ascii_alphabetic() {
                let mut i = st + 1;
                while i < self.end && is_name_byte(b[i]) {
                    i += 1;
                }
                let name = &s[st + 1..i];
                match scan_attrs(s, i, self.end, |_| {}) {
                    Some((e, selfc)) => (if selfc { TagKind::SelfClosing } else { TagKind::Open }, name, e),
                    None => {
                        // 引号没配对：退回到下一个 `>`（旧正则的口径）。
                        let e = st + rest.find('>')? + 1;
                        (if s[..e - 1].ends_with('/') { TagKind::SelfClosing } else { TagKind::Open }, name, e)
                    }
                }
            } else {
                self.pos = st + 1; // 不是标签的 `<`
                continue;
            };
            self.pos = end;
            return Some(Tag { kind, name, start: st, end });
        }
    }
}

/// 按文档序扫出 `html` 的全部标签（见模块说明）。
pub fn tags(html: &str) -> Tags<'_> {
    Tags { html, pos: 0, end: html.len() }
}

/// 只扫 `html[lo..hi]`（偏移仍相对整个 `html`）。
pub fn tags_in(html: &str, lo: usize, hi: usize) -> Tags<'_> {
    Tags { html, pos: lo, end: hi }
}

/// 从 `from` 起第一个名为 `name` 的闭合标签。
pub fn find_close<'a>(html: &'a str, from: usize, name: &str) -> Option<Tag<'a>> {
    tags_in(html, from, html.len()).find(|t| t.kind == TagKind::Close && t.is(name))
}

// ───────────────────────── 属性 ─────────────────────────

/// 一个属性。偏移相对传给 [`attrs`] 的字符串。无值属性（`<td nowrap>`）`value` 为空、`quote` 为 `None`。
#[derive(Clone, Copy, Debug)]
pub struct Attr<'a> {
    pub name: &'a str,
    /// 原文（字符引用未还原）。
    pub value: &'a str,
    /// 属性名起点（不含前导空白）。
    pub start: usize,
    /// 整个属性（含结束引号）的终点。
    pub end: usize,
    pub value_start: usize,
    pub value_end: usize,
    pub quote: Option<char>,
}

impl Attr<'_> {
    pub fn is(&self, name: &str) -> bool {
        self.name.eq_ignore_ascii_case(name)
    }
}

/// 开标签（`<p …>`，也可以只传属性部分）里的全部属性，文档序。
pub fn attrs(tag: &str) -> Vec<Attr<'_>> {
    let b = tag.as_bytes();
    let mut i = 0;
    if b.first() == Some(&b'<') {
        i = 1;
        while i < b.len() && is_name_byte(b[i]) {
            i += 1;
        }
    }
    let mut out = Vec::new();
    // 没有 `>`（只传了属性部分）时 scan_attrs 在末尾返回 None，已收集的属性照用。
    let _ = scan_attrs(tag, i, tag.len(), |a| out.push(a));
    out
}

/// 第一个名为 `name`（不分大小写、完整匹配）的属性。
pub fn attr<'a>(tag: &'a str, name: &str) -> Option<Attr<'a>> {
    attrs(tag).into_iter().find(|a| a.is(name))
}

/// 第一个名为 `name` 的属性值（原文）。
pub fn attr_value<'a>(tag: &'a str, name: &str) -> Option<&'a str> {
    attr(tag, name).map(|a| a.value)
}

/// 把值写进指定引号里：值里的同种引号转成字符引用。
fn quoted(value: &str, q: char) -> String {
    match q {
        '\'' => value.replace('\'', "&#39;"),
        _ => value.replace('"', "&quot;"),
    }
}

/// 设置属性：已有就**就地改值**（保留原引号），没有就加在最后一个属性后面（`<br/>` 的 `/` 之前）。
pub fn set_attr(tag: &str, name: &str, value: &str) -> String {
    let all = attrs(tag);
    if let Some(a) = all.iter().find(|a| a.is(name)) {
        return match a.quote {
            Some(q) => format!("{}{}{}", &tag[..a.value_start], quoted(value, q), &tag[a.value_end..]),
            None => format!("{}{}=\"{}\"{}", &tag[..a.start], a.name, quoted(value, '"'), &tag[a.end..]),
        };
    }
    let at = match all.last() {
        Some(a) => a.end,
        None => {
            let b = tag.as_bytes();
            let mut i = usize::from(b.first() == Some(&b'<'));
            while i < b.len() && is_name_byte(b[i]) {
                i += 1;
            }
            i
        }
    };
    format!("{} {}=\"{}\"{}", &tag[..at], name, quoted(value, '"'), &tag[at..])
}

/// 去掉所有名为 `name` 的属性（连同前导空白）。
pub fn remove_attr(tag: &str, name: &str) -> String {
    let mut out = String::with_capacity(tag.len());
    let mut last = 0;
    for a in attrs(tag).into_iter().filter(|a| a.is(name)) {
        let start = tag[..a.start].trim_end().len();
        out.push_str(&tag[last..start.max(last)]);
        last = a.end;
    }
    out.push_str(&tag[last..]);
    out
}

/// [`edit_attrs`] 对一个属性的处置。
pub enum Edit {
    Keep,
    /// 换成新值（原文写入，保留原引号；值里的同种引号会转成字符引用）。
    Set(String),
    /// 连同前导空白删掉。
    Remove,
}

/// 全文逐个开标签，对名字在 `names` 里的属性调 `f(标签, 属性)` 决定改不改。没有改动时原样借用返回。
pub fn edit_attrs<'a>(html: &'a str, names: &[&str], mut f: impl FnMut(&Tag, &Attr) -> Edit) -> Cow<'a, str> {
    let mut edits: Vec<(usize, usize, String)> = Vec::new();
    for t in tags(html).filter(|t| t.is_start()) {
        let tag = &html[t.start..t.end];
        for a in attrs(tag) {
            if !names.iter().any(|n| a.is(n)) {
                continue;
            }
            match f(&t, &a) {
                Edit::Keep => {}
                Edit::Set(v) => match a.quote {
                    Some(q) => edits.push((t.start + a.value_start, t.start + a.value_end, quoted(&v, q))),
                    None => edits.push((t.start + a.start, t.start + a.end, format!("{}=\"{}\"", a.name, quoted(&v, '"')))),
                },
                Edit::Remove => edits.push((t.start + tag[..a.start].trim_end().len(), t.start + a.end, String::new())),
            }
        }
    }
    if edits.is_empty() {
        return Cow::Borrowed(html);
    }
    Cow::Owned(apply_edits(html, edits))
}

/// 按 `(起, 止, 替换)` 重建全文（区间按起点升序、互不重叠）。
pub fn apply_edits(html: &str, edits: Vec<(usize, usize, String)>) -> String {
    let mut out = String::with_capacity(html.len() + 64);
    let mut last = 0;
    for (s, e, r) in edits {
        if s < last {
            continue;
        }
        out.push_str(&html[last..s]);
        out.push_str(&r);
        last = e;
    }
    out.push_str(&html[last..]);
    out
}

/// 文档里的锚点：任何元素的 `id`，以及 `<a name>`（老书的注释落点常这样写）。(值原文, 所在标签起点)，文档序。
pub fn anchors(html: &str) -> Vec<(&str, usize)> {
    let mut out = Vec::new();
    for t in tags(html).filter(|t| t.is_start()) {
        for a in attrs(&html[t.start..t.end]) {
            if (a.is("id") || (a.is("name") && t.is("a"))) && !a.value.is_empty() {
                out.push((a.value, t.start));
            }
        }
    }
    out
}

/// 全文里每个 `href`/`src`/`xlink:href` 的值（原文）。
pub fn link_values(html: &str) -> Vec<&str> {
    let mut out = Vec::new();
    for t in tags(html).filter(|t| t.is_start()) {
        for a in attrs(&html[t.start..t.end]) {
            if a.is("href") || a.is("src") || a.is("xlink:href") {
                out.push(a.value);
            }
        }
    }
    out
}

/// 开标签的 `class` 里有没有 `class` 这个类（按空白分词，区分大小写）。
pub fn has_class(tag: &str, class: &str) -> bool {
    attrs(tag).iter().any(|a| a.is("class") && a.value.split_ascii_whitespace().any(|c| c == class))
}

/// 开标签加一个类：已经有这个类就原样返回（重复处理不会越加越多）；没有 `class` 或它是空的就写成这个类，否则追加在
/// 原有的类后面。其它属性、原引号都保留；新加的 `class` 属性放在最后一个属性后面（见 [`set_attr`]）。
pub fn add_class(tag: &str, class: &str) -> String {
    if has_class(tag, class) {
        return tag.to_string();
    }
    match attr_value(tag, "class").map(str::trim) {
        Some(v) if !v.is_empty() => set_attr(tag, "class", &format!("{v} {class}")),
        _ => set_attr(tag, "class", class),
    }
}

// ───────────────────────── 链接 ─────────────────────────

/// 链接值拆成 (路径, 锚点)；锚点是原文（可能百分号编码），没有 `#` 时为 `None`。
pub fn split_href(v: &str) -> (&str, Option<&str>) {
    match v.split_once('#') {
        Some((p, f)) => (p, Some(f)),
        None => (v, None),
    }
}

/// 锚点原文 → 拿去对 id 的值（百分号解码：NCX 里常把中文锚点写成 `%E6%B3%A8`）。
pub fn frag_id(frag: &str) -> Cow<'_, str> {
    if frag.contains('%') {
        Cow::Owned(crate::epubzip::percent_decode(frag))
    } else {
        Cow::Borrowed(frag)
    }
}

/// 书外链接（带协议 `http:`/`mailto:`/`data:`…，或 `//` 开头）。协议按 RFC 3986：字母开头、只有字母数字和 `+-.`、然后 `:`。
/// 以前见到 `:` 就算：清洗改安全文件名之前，原书文件名里的 `:`（《春雪》《飘》的 `../Images/**::**…jpg`）被当成了外链。
pub fn is_external(path: &str) -> bool {
    if path.starts_with("//") {
        return true;
    }
    let Some(i) = path.find(':') else { return false };
    let scheme = &path[..i];
    scheme.starts_with(|c: char| c.is_ascii_alphabetic()) && scheme.chars().all(|c| c.is_ascii_alphanumeric() || "+-.".contains(c))
}

// ───────────────────────── 元素 ─────────────────────────

/// 一个元素在 html 里的位置（字节偏移）。空元素/没有闭合标签的元素 `close_start == close_end`。
#[derive(Clone, Debug)]
pub struct Span {
    /// 小写元素名。
    pub name: String,
    pub open_start: usize,
    pub open_end: usize,
    pub close_start: usize,
    pub close_end: usize,
    pub parent: Option<usize>,
    pub void: bool,
}

impl Span {
    /// 有真正的闭合标签（不是空元素、也不是隐式闭合）。
    pub fn closed(&self) -> bool {
        !self.void && self.close_end > self.close_start
    }
    /// `<h1>`–`<h6>` 的级别（口径同 [`Tag::heading_level`]）。
    pub fn heading_level(&self) -> Option<u8> {
        heading_level_of(&self.name)
    }
}

/// 解析 `html[lo..hi]` 里的元素（容错：闭合标签找不到对应开标签就忽略，中间没闭合的元素视为在此处隐式闭合，
/// 到 `hi` 都没闭合的 `close_start == close_end == hi`）。
pub fn parse_spans(html: &str, lo: usize, hi: usize) -> Vec<Span> {
    let mut spans: Vec<Span> = Vec::new();
    let mut stack: Vec<usize> = Vec::new();
    for t in tags_in(html, lo, hi) {
        match t.kind {
            TagKind::Other => {}
            TagKind::Close => {
                let name = t.name.to_ascii_lowercase();
                if let Some(pos) = stack.iter().rposition(|&i| spans[i].name == name) {
                    while stack.len() > pos + 1 {
                        let i = stack.pop().unwrap();
                        spans[i].close_start = t.start;
                        spans[i].close_end = t.start;
                    }
                    let i = stack.pop().unwrap();
                    spans[i].close_start = t.start;
                    spans[i].close_end = t.end;
                }
            }
            TagKind::Open | TagKind::SelfClosing => {
                let name = t.name.to_ascii_lowercase();
                let void = t.kind == TagKind::SelfClosing || is_void(&name);
                let parent = stack.last().copied();
                let close = if void { t.end } else { hi };
                spans.push(Span { name, open_start: t.start, open_end: t.end, close_start: close, close_end: close, parent, void });
                if !void {
                    stack.push(spans.len() - 1);
                }
            }
        }
    }
    spans
}

/// 第一个 `<body …>` 之后到最后一个 `</body>` 之前的范围（不分大小写）；没有成对的 body → `None`。
pub fn body_range(html: &str) -> Option<(usize, usize)> {
    let open = tags(html).find(|t| t.is_start() && t.is("body"))?;
    let close = rfind_ci(html, "</body>")?;
    (close >= open.end).then_some((open.end, close))
}

/// 第一个 `<body …>` 与其后**第一个** `</body>` 之间的原文；没有成对的 body → `None`。
pub fn first_body_inner(html: &str) -> Option<&str> {
    let open = tags(html).find(|t| t.is_start() && t.is("body"))?;
    let rest = &html[open.end..];
    let end = find_ci(rest, "</body>")?;
    Some(&rest[..end])
}

// ───────────────────────── 不分大小写查找 ─────────────────────────

/// ASCII 不分大小写地找 `needle` 第一次出现的起点（`needle` 为空时是 0）。
pub fn find_ci(hay: &str, needle: &str) -> Option<usize> {
    let n = needle.as_bytes();
    if n.is_empty() {
        return Some(0);
    }
    hay.as_bytes().windows(n.len()).position(|w| w.eq_ignore_ascii_case(n))
}

/// ASCII 不分大小写地找 `needle` 最后一次出现的起点（`needle` 为空时是 `hay.len()`）。
pub fn rfind_ci(hay: &str, needle: &str) -> Option<usize> {
    let n = needle.as_bytes();
    if n.is_empty() {
        return Some(hay.len());
    }
    hay.as_bytes().windows(n.len()).rposition(|w| w.eq_ignore_ascii_case(n))
}

/// ASCII 不分大小写地包含。
pub fn contains_ci(hay: &str, needle: &str) -> bool {
    find_ci(hay, needle).is_some()
}

// ───────────────────────── 文字 ─────────────────────────

/// [`walk_text`] 回调的一段：标签之间的文字（起点, 原文），或一个媒体元素标签。
enum Piece<'a> {
    Text(usize, &'a str),
    Media(Tag<'a>),
}

/// 逐段回调标签之间的文字（跳过 `<style>`/`<script>`/`<title>` 的内容与注释）与媒体元素；回调返回 `false` 时提前结束。
fn walk_text<'a>(html: &'a str, mut f: impl FnMut(Piece<'a>) -> bool) {
    let mut pos = 0;
    let mut skip_until: Option<&str> = None;
    for t in tags(html) {
        if skip_until.is_none() && t.start > pos && !f(Piece::Text(pos, &html[pos..t.start])) {
            return;
        }
        pos = t.end;
        match skip_until {
            Some(n) => {
                if t.kind == TagKind::Close && t.is(n) {
                    skip_until = None;
                }
            }
            None => {
                if t.kind == TagKind::Open && is_non_text(t.name) {
                    skip_until = Some(t.name);
                } else if t.is_start() && is_media(t.name) && !f(Piece::Media(t)) {
                    return;
                }
            }
        }
    }
    if skip_until.is_none() && pos < html.len() {
        f(Piece::Text(pos, &html[pos..]));
    }
}

/// 一段文字（已去标签）里有没有看得见的字：去掉空白（含 U+00A0、U+3000）和不换行空格实体后还有字符。
fn text_visible(t: &str) -> bool {
    if !t.contains('&') {
        return t.chars().any(|c| !c.is_whitespace());
    }
    let t = t.replace("&nbsp;", " ");
    xml_unescape(&t).chars().any(|c| !c.is_whitespace())
}

/// 片段里有没有读者看得见的内容。全书一套口径（定章节、空页清理、章尾空白共用）：
/// - 非空白文字：空白含 U+00A0、U+3000；`&nbsp;`、`&#160;`、`&#xa0;`、`&#12288;` 这类只表示空白的字符引用也算空白；
/// - 或媒体元素 [`MEDIA_ELEMENTS`]（图片 `img`/`svg`/`image`、分隔线 `hr`、表格、公式、音视频、`object`）。
///
/// 注释、`<style>`/`<script>`/`<title>` 的内容不算。
pub fn has_visible(fragment: &str) -> bool {
    let mut found = false;
    walk_text(fragment, |p| {
        found = match p {
            Piece::Text(_, t) => text_visible(t),
            Piece::Media(_) => true,
        };
        !found
    });
    found
}

/// 片段里最后一处可见内容结束的偏移（没有可见内容时 `None`）。口径同 [`has_visible`]。
pub fn last_visible_end(fragment: &str) -> Option<usize> {
    let mut last = None;
    walk_text(fragment, |p| {
        match p {
            Piece::Text(pos, t) if text_visible(t) => last = Some(pos + t.len()),
            Piece::Media(tag) => last = Some(tag.end),
            _ => {}
        }
        true
    });
    last
}

/// 纯文本：去标签（注释与 `<style>`/`<script>`/`<title>` 的内容也去掉）、字符引用还原成字符、空白折叠成单个空格。
/// 结果是"纯文本"，写回 XML 时调用方要再 `xml_escape`。
pub fn plain_text(html: &str) -> String {
    let mut raw = String::with_capacity(html.len().min(4096));
    walk_text(html, |p| {
        if let Piece::Text(_, t) = p {
            raw.push_str(t);
        }
        true
    });
    // 字符引用还原（2026-09-25 审计）：此前不还原，标题里的 `&amp;`/`&#12288;` 被再转义成 `&amp;amp;`，目录显示出字面的 `&amp;`。
    let raw = raw.replace("&nbsp;", " ");
    xml_unescape(&raw).split_whitespace().collect::<Vec<_>>().join(" ")
}

// ───────────────────────── CSS 声明 ─────────────────────────

/// 一条 CSS 声明（`style=""` 属性或规则体里的一段）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CssDecl<'a> {
    /// 属性名（原样大小写，已去掉前后空白与注释）。
    pub prop: &'a str,
    /// 值（去掉前后空白，`!important` 保留）。
    pub value: &'a str,
    /// 这一段原文（含前导空白与结尾 `;`），只删不改时原样拼回用。
    pub raw: &'a str,
    /// `raw` 在原文里的起点（终点 = `start + raw.len()`）。
    pub start: usize,
}

/// 把 CSS 声明文本切成一条条声明：按 `;` 切，但**引号里、括号里**（`url(data:image/png;base64,…)`）的 `;` 不算，
/// HTML 字符引用（`style` 属性里常见的 `&#39;`、`&quot;`）整体算值的一部分，`/* */` 注释跳过。
/// 没有冒号或属性名不合法的片段不算声明（[`CssDecl`] 不收，拼回时调用方可按需保留）。
/// 引号或括号不配对（书里写坏的 CSS）时退回只按 `;` 切，免得一个坏引号吞掉后面所有声明。
pub fn css_decls(text: &str) -> Vec<CssDecl<'_>> {
    css_decls_with(text, true).unwrap_or_else(|| css_decls_with(text, false).unwrap_or_default())
}

fn css_decls_with<'a>(text: &'a str, nest: bool) -> Option<Vec<CssDecl<'a>>> {
    let b = text.as_bytes();
    let mut out = Vec::new();
    let (mut seg, mut i, mut depth) = (0usize, 0usize, 0usize);
    let mut quote: Option<u8> = None;
    let mut colon: Option<usize> = None;
    let push = |out: &mut Vec<CssDecl<'a>>, seg: usize, end: usize, raw_end: usize, colon: Option<usize>| {
        let Some(c) = colon else { return };
        let head = &text[seg..c];
        let head = head.rfind("*/").map_or(head, |k| &head[k + 2..]);
        let prop = head.trim().trim_start_matches(|ch: char| !(ch.is_ascii_alphabetic() || ch == '-' || ch == '_'));
        if prop.is_empty() || !prop.bytes().all(|ch| ch.is_ascii_alphanumeric() || ch == b'-' || ch == b'_') {
            return;
        }
        out.push(CssDecl { prop, value: text[c + 1..end].trim(), raw: &text[seg..raw_end], start: seg });
    };
    while i < b.len() {
        let c = b[i];
        if let Some(q) = quote {
            if c == b'\\' {
                i += 2;
                continue;
            }
            if c == q {
                quote = None;
            }
            i += 1;
            continue;
        }
        match c {
            b'"' | b'\'' if nest => quote = Some(c),
            b'(' if nest => depth += 1,
            b')' if nest => depth = depth.saturating_sub(1),
            b'/' if b.get(i + 1) == Some(&b'*') => {
                i = text[i + 2..].find("*/").map_or(b.len(), |k| i + 2 + k + 2);
                continue;
            }
            b'&' => {
                // 字符引用 `&#39;` / `&quot;`：整体跳过，它的 `;` 不是声明分隔符。
                let n = b[i + 1..].iter().take(10).position(|&x| x == b';');
                if let Some(n) = n.filter(|&n| n > 0 && b[i + 1..i + 1 + n].iter().enumerate().all(|(k, &x)| x.is_ascii_alphanumeric() || (k == 0 && x == b'#'))) {
                    i += n + 2;
                    continue;
                }
            }
            b':' if depth == 0 && colon.is_none() => colon = Some(i),
            b';' if depth == 0 => {
                push(&mut out, seg, i, i + 1, colon);
                seg = i + 1;
                colon = None;
            }
            _ => {}
        }
        i += 1;
    }
    if quote.is_some() || depth > 0 {
        return None;
    }
    push(&mut out, seg, b.len(), b.len(), colon);
    Some(out)
}

/// `<style>…</style>` 块（三段捕获：开标签 / 内容 / 闭标签）。
pub fn style_block_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r#"(?is)(<style\b[^>]*>)(.*?)(</style>)"#).unwrap())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_external_by_scheme() {
        for ext in ["http://a/b", "https://a", "mailto:x@y", "data:image/png;base64,AA", "javascript:void(0)", "//cdn/x.png", "urn:isbn:1"] {
            assert!(is_external(ext), "{ext}");
        }
        for local in ["../Images/**::**.jpg", "a.xhtml#n:1", "Text/x:y.xhtml", "#frag", "", "1a:b", "*:x"] {
            assert!(!is_external(local), "{local}");
        }
    }

    #[test]
    fn tags_skip_comments_and_respect_quoted_gt() {
        let h = r#"<p title='a>b' class="x">t<!-- <div> --><br/></p><![CDATA[<i>]]>a < b"#;
        let got: Vec<(TagKind, &str)> = tags(h).map(|t| (t.kind, t.name)).collect();
        assert_eq!(got, [(TagKind::Open, "p"), (TagKind::Other, ""), (TagKind::SelfClosing, "br"), (TagKind::Close, "p"), (TagKind::Other, "")]);
        assert_eq!(&h[..tags(h).next().unwrap().end], r#"<p title='a>b' class="x">"#);
    }

    #[test]
    fn attr_exact_name_both_quotes_unquoted() {
        let t = r#"<span data-id="y" id='x' aid=3 hidden>"#;
        assert_eq!(attr_value(t, "id"), Some("x"));
        assert_eq!(attr_value(t, "aid"), Some("3"));
        assert_eq!(attr_value(t, "data-id"), Some("y"));
        assert_eq!(attr_value(t, "hidden"), Some(""));
        assert_eq!(attr_value(r#"<a data-id="1"/>"#, "id"), None);
        assert_eq!(attr_value(r#"id = "a b""#, "ID"), Some("a b"), "只传属性部分也行");
    }

    #[test]
    fn set_and_remove_attr_in_place() {
        assert_eq!(set_attr(r#"<h1 id='c1'>"#, "id", "n"), r#"<h1 id='n'>"#);
        assert_eq!(set_attr(r#"<p class="a"/>"#, "id", "n"), r#"<p class="a" id="n"/>"#);
        assert_eq!(set_attr("<br/>", "class", "x"), r#"<br class="x"/>"#);
        assert_eq!(set_attr("<td nowrap>", "nowrap", "1"), r#"<td nowrap="1">"#);
        assert_eq!(remove_attr(r#"<span id="x" data-id="y" id='z'>"#, "id"), r#"<span data-id="y">"#);
    }

    #[test]
    fn edit_attrs_only_exact_names() {
        let h = r#"<p data-style="font-size:1em" style='font-size:1em;color:red'>x</p><svg font-style="italic"/>"#;
        let out = edit_attrs(h, &["style"], |_, a| Edit::Set(a.value.replace("font-size:1em;", "")));
        assert_eq!(out, r#"<p data-style="font-size:1em" style='color:red'>x</p><svg font-style="italic"/>"#);
    }

    #[test]
    fn visible_union_of_media_and_text() {
        for v in ["x", "<hr/>", "<table></table>", "<p><img src='a'/></p>", "&amp;", "<svg/>", "<math/>"] {
            assert!(has_visible(v), "{v}");
        }
        for v in ["", " \n", "<p>&nbsp;&#160;&#xa0;\u{3000}&#12288;</p>", "<!-- 注 -->", "<style>p{}</style>", "<br/>"] {
            assert!(!has_visible(v), "{v}");
        }
        assert_eq!(last_visible_end("<p>字</p><p> </p>"), Some(6));
        assert_eq!(last_visible_end("<p></p><hr/>\n"), Some(12));
        assert_eq!(last_visible_end("<p> </p>"), None);
    }

    #[test]
    fn plain_text_decodes_and_drops_comments() {
        assert_eq!(plain_text("<h1>A &amp; B<!-- x > y --></h1>\n<p>c&#12288;d</p>"), "A & B c d");
        assert_eq!(plain_text("<style>p{}</style>文"), "文");
    }

    #[test]
    fn spans_implicit_close() {
        let h = "<div><p>a<b>b</p></div><p/>";
        let s = parse_spans(h, 0, h.len());
        let names: Vec<&str> = s.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names, ["div", "p", "b", "p"]);
        assert!(s[0].closed() && s[1].closed() && !s[2].closed() && s[3].void);
    }

    #[test]
    fn heading_levels_only_h1_to_h6() {
        let h = "<H2>a</H2><h0>b</h0><h7>c</h7><hr/><h6>d</h6>";
        let got: Vec<Option<u8>> = parse_spans(h, 0, h.len()).iter().map(Span::heading_level).collect();
        assert_eq!(got, [Some(2), None, None, None, Some(6)]);
    }

    #[test]
    fn anchors_include_a_name() {
        let h = r#"<p id='x'>a</p><a name="n1"></a><div data-id="no" name="no"></div>"#;
        let got: Vec<&str> = anchors(h).into_iter().map(|a| a.0).collect();
        assert_eq!(got, ["x", "n1"]);
    }

    #[test]
    fn css_decls_respect_quotes_parens_entities() {
        let d = css_decls(r#"background:url(data:image/png;base64,AAA=) no-repeat; content:"a;b"; font-family:&#39;A&#39;;/* x;y */color : red"#);
        let got: Vec<(&str, &str)> = d.iter().map(|d| (d.prop, d.value)).collect();
        assert_eq!(got, [("background", "url(data:image/png;base64,AAA=) no-repeat"), ("content", "\"a;b\""), ("font-family", "&#39;A&#39;"), ("color", "red")]);
        let bad: Vec<&str> = css_decls("font-family: Georgia';color:red").iter().map(|d| d.prop).collect();
        assert_eq!(bad, ["font-family", "color"], "坏引号退回按分号切");
        assert_eq!(css_decls("a:1;b:2").iter().map(|d| d.raw).collect::<String>(), "a:1;b:2");
    }

    #[test]
    fn body_ranges() {
        let h = "<html><body class='a>b'><p>x</p></body></html>";
        let (lo, hi) = body_range(h).unwrap();
        assert_eq!(&h[lo..hi], "<p>x</p>");
        assert_eq!(first_body_inner(h), Some("<p>x</p>"));
        assert_eq!(body_range("<html></html>"), None);
    }
}
