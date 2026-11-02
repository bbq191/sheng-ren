//! EPUB → KEPUB（Kobo 自带阅读器 Nickel 的 Kobo WebKit 引擎），clean-room：只照公开资料（Kobo 官方 epub-spec、kepubify 的
//! 包文档、epub.studio 的格式说明、Kobo 官方转换样本的公开对比），不读 kepubify 源码，不碰 Calibre。设计与来源见 docs/kepub.md。
//!
//! 只在原文上**插入**标签，原文一个字节都不改（剥掉插入的标签后与输入逐字节相等，测试守着）：
//! - 章节 `<body>` 里面包两层 `<div id="book-columns"><div id="book-inner">…</div></div>`；
//! - 每句文字包 `<span class="koboSpan" id="kobo.段.句">`（段、句从 1 起，每个文件重新数；span 只包文字、不跨元素边界）；
//! - 图片自己占一段：`<span class="koboSpan" id="kobo.段.1"><img …/></span>`。
//!
//! OPF、CSS、图片等其余条目原样拷（压缩数据不动）；导航文档（nav、ncx）不改；已经是 KEPUB 的章节（有包装或 koboSpan）整章不动。
//! 不加 Kobo 的样式补丁（`kobostylehacks`）：会改书的版式（维护者 2026-11-01 定）。

use crate::epubzip::{self, EpubWriter};
use crate::error::BookError;
use crate::html::{self, TagKind};

/// 写出器版本：改了产物的写法就加一（进书库指纹，旧产物按需重建）。
pub const VERSION: u32 = 1;

/// 包装的开头、结尾。
const WRAP_OPEN: &str = r#"<div id="book-columns"><div id="book-inner">"#;
const WRAP_CLOSE: &str = "</div></div>";

/// 里面的文字不包 span 的元素（脚本、样式、矢量图、公式、注音读法、多媒体回退文字等）。
const SKIP: &[&str] = &[
    "script", "style", "svg", "math", "head", "title", "rt", "rp", "textarea", "audio", "video",
    "canvas", "noscript", "template", "iframe", "object",
];

/// 句末标点。ASCII `.` 另算（后面是空白、收尾引号或结尾才算句末，避免 3.14 之类）。
fn is_terminal(c: char) -> bool {
    matches!(c, '。' | '！' | '？' | '!' | '?' | '…' | '．' | '｡')
}

/// 句末之后归前一句的收尾引号、括号。
fn is_closer(c: char) -> bool {
    matches!(
        c,
        '」' | '』' | '”' | '’' | '）' | '》' | '】' | '〉' | '〕' | '"' | '\'' | ')' | ']'
    )
}

#[derive(Clone, Copy, Debug)]
pub struct Opts {
    /// 没有可见文字的页（封面、漫画页）也加两层包装。缺省开（kepubify 文档说包装是必需的）；关掉只给真机 A/B 用
    /// （`epub-to-kepub --no-wrap-image-pages`）。
    pub wrap_image_pages: bool,
}

impl Default for Opts {
    fn default() -> Self {
        Opts {
            wrap_image_pages: true,
        }
    }
}

#[derive(Debug, Default)]
pub struct Report {
    /// 改写了的章节数。
    pub files: usize,
    /// 加的 koboSpan 数。
    pub spans: usize,
    pub warnings: Vec<String>,
}

/// 一个原文字符：在文本里的字节范围（字符引用 `&…;` 整个算一个）和解码后的字符。
struct Unit {
    start: usize,
    end: usize,
    ch: char,
}

/// 把文本切成 [`Unit`]：普通 UTF-8 字符一个一个来，`&…;` 能解成单个字符的整个当一个（`&#160;` 当空白、`&#8230;` 当省略号），
/// 解不了的 `&` 当普通字符。切分只会落在 Unit 边界上，所以绝不切破字符引用。
fn units(s: &str) -> Vec<Unit> {
    let mut v = Vec::with_capacity(s.len());
    let mut i = 0;
    while i < s.len() {
        let c = s[i..].chars().next().unwrap_or(' ');
        if c == '&' {
            if let Some(j) = s.as_bytes()[i..].iter().take(40).position(|&b| b == b';') {
                let ent = &s[i..=i + j];
                let d = html::unescape_entities(ent);
                let mut cs = d.chars();
                if let (Some(dc), None) = (cs.next(), cs.next()) {
                    if d != ent {
                        v.push(Unit {
                            start: i,
                            end: i + j + 1,
                            ch: dc,
                        });
                        i += j + 1;
                        continue;
                    }
                }
            }
        }
        v.push(Unit {
            start: i,
            end: i + c.len_utf8(),
            ch: c,
        });
        i += c.len_utf8();
    }
    v
}

/// 一段文字里的句子（字节范围，相对 `s`）。前导空白不进句子；句末之后的收尾引号和空白归这一句；最后一句的尾部空白不进句子。
/// `split = false`（`pre` 里）整段当一句。
fn sentences(s: &str, split: bool) -> Vec<(usize, usize)> {
    let u = units(s);
    let n = u.len();
    let ws = |k: usize| u[k].ch.is_whitespace();
    let mut out = Vec::new();
    let mut k = 0;
    let push = |out: &mut Vec<(usize, usize)>, a: usize, mut b: usize| {
        let mut a = a;
        while a < b && ws(a) {
            a += 1;
        }
        if b == n {
            while b > a && ws(b - 1) {
                b -= 1;
            }
        }
        if a < b {
            out.push((u[a].start, u[b - 1].end));
        }
    };
    let mut start = 0;
    if split {
        while k < n {
            let c = u[k].ch;
            let ends =
                is_terminal(c) || (c == '.' && (k + 1 == n || ws(k + 1) || is_closer(u[k + 1].ch)));
            if !ends {
                k += 1;
                continue;
            }
            let mut j = k + 1;
            while j < n && (is_terminal(u[j].ch) || u[j].ch == '.') {
                j += 1;
            }
            while j < n && is_closer(u[j].ch) {
                j += 1;
            }
            while j < n && ws(j) {
                j += 1;
            }
            push(&mut out, start, j);
            start = j;
            k = j;
        }
    }
    if start < n {
        push(&mut out, start, n);
    }
    out
}

/// 元素名去掉命名空间前缀（`svg:svg` → `svg`）。
fn local(name: &str) -> &str {
    name.rsplit(':').next().unwrap_or(name)
}

/// 里面是原样文字的元素（HTML 规范的 raw text）：到第一个 `</名字` 为止，里面的 `<`、`<!--` 都不当标签。
const RAW_TEXT: &[&str] = &["script", "style"];

/// `from` 起第一个 `</name`（后面紧跟 `>`、`/`、空白或文末）的闭标签之后；没有就是 `None`。
fn raw_text_end(doc: &str, from: usize, name: &str) -> Option<usize> {
    let pat = format!("</{name}");
    let mut at = from;
    loop {
        let k = at + html::find_ci(&doc[at..], &pat)?;
        match doc.as_bytes().get(k + pat.len()) {
            Some(b'>' | b'/') | None => {
                return Some(k + doc[k..].find('>').map_or(doc.len() - k, |g| g + 1))
            }
            Some(c) if c.is_ascii_whitespace() => {
                return Some(k + doc[k..].find('>').map_or(doc.len() - k, |g| g + 1))
            }
            _ => at = k + pat.len(),
        }
    }
}

/// 有没有已经是 KEPUB 的痕迹：`id="book-columns"`/`book-inner` 或 class 里有 `koboSpan`（按标签属性看，不搜正文）。
fn already_kepub(doc: &str) -> bool {
    html::tags(doc).filter(|t| t.is_start()).any(|t| {
        let tag = &doc[t.start..t.end];
        html::attr_value(tag, "id").is_some_and(|v| v == "book-columns" || v == "book-inner")
            || html::has_class(tag, "koboSpan")
    })
}

/// 改写一个章节 XHTML。返回 `None`：没有 body、已经是 KEPUB（原样用）。否则返回新文本和加的 span 数。
pub fn transform_xhtml(doc: &str, opts: &Opts) -> Option<(String, usize)> {
    let (bs, be) = html::body_range(doc)?;
    if already_kepub(doc) {
        return None;
    }
    let mut edits: Vec<(usize, usize, String)> = Vec::new();
    let (mut para, mut sent, mut spans, mut text_spans) = (0usize, 0usize, 0usize, 0usize);
    let mut need_new_para = true;
    // 正在跳过的元素名与嵌套深度（同名嵌套才算）
    let mut skip: Option<(&str, usize)> = None;
    let mut pre_depth = 0usize;
    let mut pos = bs;
    let mut tags: Vec<html::Tag> = html::tags_in(doc, bs, be).collect();
    let mut i = 0;
    while i < tags.len() {
        let t = tags[i];
        if skip.is_none() && t.start > pos {
            let seg = &doc[pos..t.start];
            let ss = sentences(seg, pre_depth == 0);
            if !ss.is_empty() {
                if need_new_para {
                    para += 1;
                    sent = 0;
                    need_new_para = false;
                }
                let mut r = String::with_capacity(seg.len() + ss.len() * 40);
                let mut last = 0;
                for (a, b) in ss {
                    sent += 1;
                    r.push_str(&seg[last..a]);
                    r.push_str(&format!(
                        r#"<span class="koboSpan" id="kobo.{para}.{sent}">"#
                    ));
                    r.push_str(&seg[a..b]);
                    r.push_str("</span>");
                    last = b;
                    spans += 1;
                    text_spans += 1;
                }
                r.push_str(&seg[last..]);
                edits.push((pos, t.start, r));
            }
        }
        pos = t.end;
        i += 1;
        if t.kind == TagKind::Open
            && RAW_TEXT
                .iter()
                .any(|r| local(t.name).eq_ignore_ascii_case(r))
        {
            // 直接跳到闭标签之后（tags 扫描不把它当原样文字）；没闭标签就到 body 末尾都不包
            pos = raw_text_end(doc, t.end, t.name)
                .unwrap_or(be)
                .min(be)
                .max(t.end);
            // 从闭标签之后重新扫（里面的 `<!--` 会让原来的扫描吞掉后文）
            tags = html::tags_in(doc, pos, be).collect();
            i = 0;
            continue;
        }
        if let Some((name, depth)) = skip {
            if t.name.eq_ignore_ascii_case(name) {
                match t.kind {
                    TagKind::Open => skip = Some((name, depth + 1)),
                    TagKind::Close if depth <= 1 => skip = None,
                    TagKind::Close => skip = Some((name, depth - 1)),
                    _ => {}
                }
            }
            continue;
        }
        if t.kind == TagKind::Open && SKIP.iter().any(|s| local(t.name).eq_ignore_ascii_case(s)) {
            skip = Some((t.name, 1));
            continue;
        }
        if t.is("pre") {
            match t.kind {
                TagKind::Open => pre_depth += 1,
                TagKind::Close => pre_depth = pre_depth.saturating_sub(1),
                _ => {}
            }
        }
        if t.is("img") && t.is_start() {
            // `<img …></img>` 写法连闭合标签一起包（中间只许空白和注释）
            let mut end = t.end;
            if t.kind == TagKind::Open {
                let mut j = i;
                while j < tags.len()
                    && tags[j].kind == TagKind::Other
                    && doc[end..tags[j].start].trim().is_empty()
                {
                    end = tags[j].end;
                    j += 1;
                }
                match tags.get(j) {
                    Some(n)
                        if n.kind == TagKind::Close
                            && n.is("img")
                            && doc[end..n.start].trim().is_empty() =>
                    {
                        end = n.end;
                        pos = n.end;
                        i = j + 1;
                    }
                    _ => end = t.end,
                }
            }
            para += 1;
            spans += 1;
            edits.push((
                t.start,
                t.start,
                format!(r#"<span class="koboSpan" id="kobo.{para}.1">"#),
            ));
            edits.push((end, end, "</span>".into()));
            need_new_para = true;
            continue;
        }
        if t.kind != TagKind::Other
            && (html::is_block(t.name)
                || ["caption", "dt", "dd", "address"].iter().any(|b| t.is(b)))
        {
            need_new_para = true;
        }
    }
    if skip.is_none() && be > pos {
        let seg = &doc[pos..be];
        let ss = sentences(seg, pre_depth == 0);
        if !ss.is_empty() {
            if need_new_para {
                para += 1;
                sent = 0;
            }
            let mut r = String::new();
            let mut last = 0;
            for (a, b) in ss {
                sent += 1;
                r.push_str(&seg[last..a]);
                r.push_str(&format!(
                    r#"<span class="koboSpan" id="kobo.{para}.{sent}">"#
                ));
                r.push_str(&seg[a..b]);
                r.push_str("</span>");
                last = b;
                spans += 1;
                text_spans += 1;
            }
            r.push_str(&seg[last..]);
            edits.push((pos, be, r));
        }
    }
    let wrap = opts.wrap_image_pages || text_spans > 0;
    if !wrap && edits.is_empty() {
        return None;
    }
    let mut all = Vec::with_capacity(edits.len() + 2);
    if wrap {
        all.push((bs, bs, WRAP_OPEN.to_string()));
    }
    all.extend(edits);
    if wrap {
        all.push((be, be, WRAP_CLOSE.to_string()));
    }
    Some((html::apply_edits(doc, all), spans))
}

/// 书里的 CSS 有没有裸 `div`/`span` 类型选择器或 `body >` 子代选择器（会匹配到新加的包装、koboSpan，Kobo 官方 epub-spec
/// 也这样提醒）。按 csscascade 切规则、选择器（at 规则里的也看）；只提醒，不改。
fn risky_selectors(css: &str) -> bool {
    use crate::cascade::{rule_selector, rule_spans, split_top};
    let is_type = |simple: &str, ty: &str| {
        let base = simple.split(['.', '#', ':', '[']).next().unwrap_or("");
        local(base.trim_start_matches('|')).eq_ignore_ascii_case(ty)
    };
    rule_spans(css).iter().any(|r| {
        let sel = rule_selector(r.prelude);
        if sel.trim_start().starts_with('@') {
            return false;
        }
        split_top(&sel, ',').into_iter().any(|one| {
            // 复合选择器序列：按组合符切开，组合符单独成项
            let spaced = one
                .replace('>', " > ")
                .replace('+', " + ")
                .replace('~', " ~ ");
            let parts: Vec<&str> = spaced.split_whitespace().collect();
            parts.iter().enumerate().any(|(k, p)| {
                let bare = |ty| {
                    is_type(p, ty) && p.len() == ty.len()
                        || is_type(p, ty) && p[ty.len()..].starts_with(':')
                };
                bare("div")
                    || bare("span")
                    || (is_type(p, "body") && parts.get(k + 1) == Some(&">"))
            })
        })
    })
}

/// `input`（优化过的 EPUB）→ `output`（KEPUB）。章节按 [`transform_xhtml`] 改写，其余条目原样拷压缩数据。
pub fn epub_to_kepub(
    input: &std::path::Path,
    output: &std::path::Path,
    opts: &Opts,
) -> Result<Report, BookError> {
    let (mut zip, opf_path, opf) = epubzip::open_opf(input)?;
    let dir = epubzip::dir_of(&opf_path).to_string();
    let nav: Vec<String> = crate::wash::opf::manifest_items(&opf)
        .iter()
        .filter(|it| it.properties.split_whitespace().any(|x| x == "nav"))
        .map(|it| it.path(&dir))
        .collect();
    let mut w = EpubWriter::create(output)?;
    let mut rep = Report::default();
    let mut risky = false;
    for k in 0..zip.len() {
        let mut f = zip.by_index(k).map_err(|e| BookError::zip("读条目", e))?;
        let name = f.name().to_string();
        if name == "mimetype" || f.is_dir() {
            continue;
        }
        let base = name.rsplit('/').next().unwrap_or(&name);
        let maybe_html = epubzip::is_html(&name) || !base.contains('.');
        let is_css = name.to_ascii_lowercase().ends_with(".css");
        if !(maybe_html || is_css) || nav.contains(&name) || crate::wash::is_toc_file(&name) {
            w.raw_copy(f)?;
            continue;
        }
        let size = f.size();
        let data = epubzip::read_all(&mut f, size, &name)?;
        drop(f);
        // 不改的条目重新打开、原样拷压缩数据
        let mut copy = |w: &mut EpubWriter<_>| -> Result<(), BookError> {
            w.raw_copy(zip.by_index(k).map_err(|e| BookError::zip("读条目", e))?)
        };
        if is_css {
            risky |= risky_selectors(&String::from_utf8_lossy(&data));
            copy(&mut w)?;
            continue;
        }
        let text = match std::str::from_utf8(&data) {
            Ok(t) if epubzip::is_html_entry(&name, &data) => t,
            _ => {
                copy(&mut w)?;
                continue;
            }
        };
        // 章内 <style> 也查
        risky |= html::style_blocks(text)
            .iter()
            .any(|b| risky_selectors(&text[b.content_start..b.content_end]));
        match transform_xhtml(text, opts) {
            Some((out, n)) => {
                rep.files += 1;
                rep.spans += n;
                w.put(&name, out.as_bytes())?;
            }
            None => copy(&mut w)?,
        }
    }
    w.finish()?;
    if risky {
        rep.warnings.push("KEPUB：书里的 CSS 有裸 div/span 或 body > 选择器，可能匹配到新加的包装或 koboSpan（只提醒，没改 CSS）".into());
    }
    Ok(rep)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc(body: &str) -> String {
        format!(
            r#"<?xml version="1.0" encoding="utf-8"?><html xmlns="http://www.w3.org/1999/xhtml"><head><title>t。</title><style>p{{x:1}}</style></head><body class="b">{body}</body></html>"#
        )
    }
    fn body_of(d: &str) -> String {
        let (a, b) = html::body_range(d).unwrap();
        d[a..b].to_string()
    }
    fn conv(body: &str) -> String {
        let (out, _) = transform_xhtml(&doc(body), &Opts::default()).unwrap();
        let b = body_of(&out);
        let b = b
            .strip_prefix(WRAP_OPEN)
            .and_then(|x| x.strip_suffix(WRAP_CLOSE))
            .expect("包装");
        b.to_string()
    }
    /// 剥掉插入的 koboSpan 和包装，应当和原文逐字节相等。
    fn strip(s: &str) -> String {
        let mut out = s.replacen(WRAP_OPEN, "", 1);
        if let Some(i) = out.rfind(WRAP_CLOSE) {
            out.replace_range(i..i + WRAP_CLOSE.len(), "");
        }
        let mut r = String::new();
        let mut rest = out.as_str();
        let mut depth = Vec::new();
        while let Some(i) = rest.find('<') {
            r.push_str(&rest[..i]);
            rest = &rest[i..];
            let e = rest.find('>').unwrap() + 1;
            let tag = &rest[..e];
            if tag.starts_with(r#"<span class="koboSpan""#) {
                depth.push(true);
            } else if tag == "</span>" && depth.pop() == Some(true) {
            } else {
                if tag.starts_with("<span") {
                    depth.push(false);
                }
                r.push_str(tag);
            }
            rest = &rest[e..];
        }
        r.push_str(rest);
        r
    }

    #[test]
    fn sentences_split_cjk_and_closers() {
        assert_eq!(
            conv("<p>一。二！</p>"),
            r#"<p><span class="koboSpan" id="kobo.1.1">一。</span><span class="koboSpan" id="kobo.1.2">二！</span></p>"#
        );
        assert_eq!(
            conv("<p>「好。」他说……对吗？！嗯</p>"),
            r#"<p><span class="koboSpan" id="kobo.1.1">「好。」</span><span class="koboSpan" id="kobo.1.2">他说……</span><span class="koboSpan" id="kobo.1.3">对吗？！</span><span class="koboSpan" id="kobo.1.4">嗯</span></p>"#
        );
    }

    #[test]
    fn english_period_rules_and_trailing_space() {
        assert_eq!(
            conv("<p>Pi is 3.14 ok. Next one.\n</p>"),
            "<p><span class=\"koboSpan\" id=\"kobo.1.1\">Pi is 3.14 ok. </span><span class=\"koboSpan\" id=\"kobo.1.2\">Next one.</span>\n</p>"
        );
    }

    #[test]
    fn entities_are_never_split_and_decoded_for_rules() {
        // &#8230; 是省略号、&#8221; 是收尾引号、&#160; 是空白
        let b = conv("<p>甲&#8230;&#8221;乙&amp;丙。&#160;</p><p>&#160;</p>");
        assert_eq!(
            b,
            r#"<p><span class="koboSpan" id="kobo.1.1">甲&#8230;&#8221;</span><span class="koboSpan" id="kobo.1.2">乙&amp;丙。</span>&#160;</p><p>&#160;</p>"#
        );
    }

    #[test]
    fn nested_inline_spans_stay_inside_elements() {
        assert_eq!(
            conv(r#"<p>a<em class="x">b。c</em>d<a href="n.xhtml#1" id="r1">1</a></p>"#),
            r#"<p><span class="koboSpan" id="kobo.1.1">a</span><em class="x"><span class="koboSpan" id="kobo.1.2">b。</span><span class="koboSpan" id="kobo.1.3">c</span></em><span class="koboSpan" id="kobo.1.4">d</span><a href="n.xhtml#1" id="r1"><span class="koboSpan" id="kobo.1.5">1</span></a></p>"#
        );
    }

    #[test]
    fn paragraph_numbers_advance_on_blocks_and_images() {
        assert_eq!(
            conv(
                r#"<div>一<p>二</p>三</div><p>前<img src="a.png"/>后<br/>续</p><img src="b.jpg"></img>"#
            ),
            concat!(
                r#"<div><span class="koboSpan" id="kobo.1.1">一</span><p><span class="koboSpan" id="kobo.2.1">二</span></p><span class="koboSpan" id="kobo.3.1">三</span></div>"#,
                r#"<p><span class="koboSpan" id="kobo.4.1">前</span><span class="koboSpan" id="kobo.5.1"><img src="a.png"/></span><span class="koboSpan" id="kobo.6.1">后</span><br/><span class="koboSpan" id="kobo.6.2">续</span></p>"#,
                r#"<span class="koboSpan" id="kobo.7.1"><img src="b.jpg"></img></span>"#
            )
        );
    }

    #[test]
    fn skipped_elements_and_ruby() {
        let b = conv(
            r#"<p><ruby>漢<rp>(</rp><rt>かん</rt><rp>)</rp></ruby>字</p><svg><text>不包</text><svg><text>x</text></svg>y</svg><script type="text/javascript">a</script><pre>x. y.</pre>"#,
        );
        assert_eq!(
            b,
            r#"<p><ruby><span class="koboSpan" id="kobo.1.1">漢</span><rp>(</rp><rt>かん</rt><rp>)</rp></ruby><span class="koboSpan" id="kobo.1.2">字</span></p><svg><text>不包</text><svg><text>x</text></svg>y</svg><script type="text/javascript">a</script><pre><span class="koboSpan" id="kobo.2.1">x. y.</span></pre>"#
        );
    }

    #[test]
    fn wrapper_keeps_body_attrs_and_head_untouched_and_is_idempotent() {
        let d = doc("<p>一</p>");
        let (out, n) = transform_xhtml(&d, &Opts::default()).unwrap();
        assert_eq!(n, 1);
        assert!(
            out.contains(r#"<body class="b"><div id="book-columns"><div id="book-inner"><p>"#),
            "{out}"
        );
        assert!(
            out.contains("<title>t。</title>"),
            "head 里不加 span：{out}"
        );
        assert!(
            transform_xhtml(&out, &Opts::default()).is_none(),
            "已是 KEPUB 不再改"
        );
        assert!(transform_xhtml(
            &doc(r#"<p><span class="koboSpan" id="kobo.1.1">x</span></p>"#),
            &Opts::default()
        )
        .is_none());
        assert!(transform_xhtml("<html><p>无 body</p></html>", &Opts::default()).is_none());
    }

    #[test]
    fn image_only_pages_wrap_by_default_and_ab_switch_turns_it_off() {
        let d = doc(r#"<img src="p1.jpg" width="10" height="10"/>"#);
        let (on, _) = transform_xhtml(&d, &Opts::default()).unwrap();
        assert!(on.contains(WRAP_OPEN));
        let (off, n) = transform_xhtml(
            &d,
            &Opts {
                wrap_image_pages: false,
            },
        )
        .unwrap();
        assert!(!off.contains(WRAP_OPEN) && n == 1, "{off}");
        // 有文字的页不受开关影响
        let (t, _) = transform_xhtml(
            &doc("<p>字</p>"),
            &Opts {
                wrap_image_pages: false,
            },
        )
        .unwrap();
        assert!(t.contains(WRAP_OPEN));
    }

    #[test]
    fn stripping_inserted_markup_gives_back_the_input() {
        for body in [
            "<p>「好。」他说……对吗？！嗯</p>",
            r#"<div>一<p>二</p>三</div><p>前<img src="a.png"/>后<br/>续</p><span class="x">s</span>"#,
            "<p>甲&#8230;&#8221;乙&amp;丙。&#160;</p>\n<p>Dr. Who. Ok?</p>",
            "<!-- 注释。 --><p><![CDATA[x]]>文。</p>",
        ] {
            let d = doc(body);
            let (out, _) = transform_xhtml(&d, &Opts::default()).unwrap();
            assert_eq!(strip(&out), d);
        }
    }

    #[test]
    fn review_fixes_trailing_ws_raw_text_prefixes_img_comment() {
        // 尾部多个空白都不进 span
        assert_eq!(
            conv("<p>一。\r\n  </p>"),
            "<p><span class=\"koboSpan\" id=\"kobo.1.1\">一。</span>\r\n  </p>"
        );
        // body 里的 script 有裸 < 也不吞后文
        let b = conv(
            "<script>for(i=0;i<n;i++){}</script><p>后</p><style>/* <!-- */ p{}</style><p>再</p>",
        );
        assert!(
            b.contains(r#"<p><span class="koboSpan" id="kobo.1.1">后</span></p>"#)
                && b.contains(">再</span>"),
            "{b}"
        );
        assert!(
            !b[..b.find("</script>").unwrap()].contains("koboSpan"),
            "{b}"
        );
        // 带前缀的 svg 不插 span
        let b = conv(
            r#"<svg:svg xmlns:svg="http://www.w3.org/2000/svg"><svg:text>字</svg:text></svg:svg><p>x</p>"#,
        );
        assert!(
            !b[..b.find("</svg:svg>").unwrap()].contains("koboSpan"),
            "{b}"
        );
        // <img></img> 中间夹注释
        assert_eq!(
            conv(r#"<img src="a.png"><!-- 注 --></img>"#),
            r#"<span class="koboSpan" id="kobo.1.1"><img src="a.png"><!-- 注 --></img></span>"#
        );
        // 不包装模式下什么都没改的页返回 None
        assert!(transform_xhtml(
            &doc(r#"<svg><image href="c.jpg"/></svg>"#),
            &Opts {
                wrap_image_pages: false
            }
        )
        .is_none());
    }

    #[test]
    fn risky_selector_detection() {
        assert!(risky_selectors("div { margin: 1em }"));
        assert!(risky_selectors("p, span:first-child { x:1 }"));
        assert!(risky_selectors("body > p { x:1 }"));
        assert!(risky_selectors("@media screen { div:first-child { x:1 } }"));
        assert!(!risky_selectors(
            "div.x, p span.y, #a { x:1 } /* div { } */"
        ));
        assert!(!risky_selectors(
            "tbody > tr, .body > p, #body > div.x, p::after { content: \"{ div }\" }"
        ));
    }

    #[test]
    fn whole_book_mimetype_first_nav_untouched() {
        let d = tempfile::tempdir().unwrap();
        let src = d.path().join("a.epub");
        let mut w = EpubWriter::create(&src).unwrap();
        w.put("META-INF/container.xml", br#"<?xml version="1.0"?><container><rootfiles><rootfile full-path="OEBPS/c.opf" media-type="application/oebps-package+xml"/></rootfiles></container>"#).unwrap();
        w.put("OEBPS/c.opf", br#"<package><manifest><item id="n" href="nav.xhtml" properties="nav" media-type="application/xhtml+xml"/><item id="c" href="c1.xhtml" media-type="application/xhtml+xml"/></manifest><spine><itemref idref="c"/></spine></package>"#).unwrap();
        w.put(
            "OEBPS/nav.xhtml",
            doc("<nav><ol><li><a href=\"c1.xhtml\">一</a></li></ol></nav>").as_bytes(),
        )
        .unwrap();
        w.put("OEBPS/c1.xhtml", doc("<p>一。二。</p>").as_bytes())
            .unwrap();
        w.put("OEBPS/s.css", b"span { color: red }").unwrap();
        w.finish().unwrap();
        let out = d.path().join("a.kepub.epub");
        let rep = epub_to_kepub(&src, &out, &Opts::default()).unwrap();
        assert_eq!((rep.files, rep.spans, rep.warnings.len()), (1, 2, 1));
        let mut z = epubzip::open_file_zip(&out).unwrap();
        let first = z.by_index(0).unwrap();
        assert_eq!(
            (first.name(), first.compression()),
            ("mimetype", zip::CompressionMethod::Stored)
        );
        drop(first);
        let nav =
            String::from_utf8(epubzip::read_by_name(&mut z, "OEBPS/nav.xhtml").unwrap()).unwrap();
        assert!(!nav.contains("koboSpan") && !nav.contains("book-columns"));
        let c1 =
            String::from_utf8(epubzip::read_by_name(&mut z, "OEBPS/c1.xhtml").unwrap()).unwrap();
        assert!(c1.contains(r#"id="kobo.1.2""#));
    }
}
