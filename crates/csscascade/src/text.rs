//! CSS 文本：扫描、拆分、规则位置。

use crate::rules::MAX_MEDIA_NESTING;
use crate::shorthand::expand_shorthand;
use crate::Decl;

// ---------------------------------------------------------------- CSS 文本：扫描、拆分、规则位置
//
// 层叠（上面的 `Rules`）和清洗层、优化器（`wash::css`、`cssunlock`、`bgfit`、`capfit`、`wash::fonts`……）共用的 CSS 文本小工具，
// 每样只有这一份（2026-10-10 审计以前成对重复、健壮程度不一：有的认引号有的不认，有的会切坏 `calc()`）。

/// [`lex`] 给出的一个字符。
#[derive(Clone, Copy, Debug, PartialEq)]
enum Lex {
    /// 字符串、注释之外、没被转义的字符：括号、分隔符这些结构只看它。
    Code(usize, char),
    /// 字符串里的字符（含两边的引号）、反斜杠和被它转义的字符：算词的一部分，但不当结构。
    Quoted(usize),
    /// 一段注释 `/* … */`：(起点, 最后一个字符的位置)；没闭合的到末尾。
    Comment(usize, usize),
}

/// 逐字符扫 CSS 文本（`it` 给出 (位置, 字符)），按 CSS Syntax 规范分出字符串、注释、转义：反斜杠转义下一个字符，字符串遇到
/// 没转义的换行就结束（坏字符串），注释只在字符串外面算。以前落单的引号一直吞到下一个同样的引号，`{}` 配不上、后面的规则全废。
/// 找配对的括号、按分隔符拆（选择器列表、媒体查询、属性值）、切词、去注释、找规则、算优先级都用它。
fn lex<I: Iterator<Item = (usize, char)>>(it: I) -> Lexer<I> {
    Lexer {
        it: it.peekable(),
        quote: None,
    }
}

struct Lexer<I: Iterator<Item = (usize, char)>> {
    it: std::iter::Peekable<I>,
    quote: Option<char>,
}

impl<I: Iterator<Item = (usize, char)>> Iterator for Lexer<I> {
    type Item = Lex;

    fn next(&mut self) -> Option<Lex> {
        let (i, c) = self.it.next()?;
        if c == '\\' {
            self.it.next();
            return Some(Lex::Quoted(i));
        }
        match self.quote {
            Some(q) => {
                if c == q || matches!(c, '\n' | '\r' | '\x0c') {
                    self.quote = None;
                }
                Some(Lex::Quoted(i))
            }
            None if c == '"' || c == '\'' => {
                self.quote = Some(c);
                Some(Lex::Quoted(i))
            }
            None if c == '/' && self.it.peek().is_some_and(|&(_, n)| n == '*') => {
                let (mut last, _) = self.it.next().unwrap_or((i, '*'));
                let mut prev = '\0';
                for (j, ch) in self.it.by_ref() {
                    last = j;
                    if prev == '*' && ch == '/' {
                        break;
                    }
                    prev = ch;
                }
                Some(Lex::Comment(i, last))
            }
            None => Some(Lex::Code(i, c)),
        }
    }
}

/// 字符串、注释之外的字符依次交给 `f`；`f` 返回 `Some` 就停下返回它。
fn scan_css<R>(
    it: impl Iterator<Item = (usize, char)>,
    mut f: impl FnMut(usize, char) -> Option<R>,
) -> Option<R> {
    for l in lex(it) {
        if let Lex::Code(i, c) = l {
            if let Some(r) = f(i, c) {
                return Some(r);
            }
        }
    }
    None
}

/// 去掉 `/* … */` 注释，每段换成一个空格（注释在 CSS 里起分隔作用：`1px/**/solid` 是两个词）；字符串里的 `/*` 不算，
/// 没闭合的注释去到末尾。没有注释时原样借用。层叠解析样式表、清洗层判断选择器（[`rule_selector`]）共用
/// （2026-09-30 审计：`/* p 的边距 */ .note{…}` 被当成 p 规则改了边距，`/* fonts */ @font-face{…}` 没认出是 @font-face）。
pub fn strip_comments(css: &str) -> std::borrow::Cow<'_, str> {
    if !css.contains("/*") {
        return std::borrow::Cow::Borrowed(css);
    }
    let mut out = String::with_capacity(css.len());
    let mut from = 0;
    for l in lex(css.char_indices()) {
        if let Lex::Comment(s, last) = l {
            out.push_str(&css[from..s]);
            out.push(' ');
            from = last + css[last..].chars().next().map_or(0, char::len_utf8);
        }
    }
    out.push_str(&css[from..]);
    std::borrow::Cow::Owned(out)
}

/// 样式表里的一条规则在原文里的位置（[`rule_spans`]）。
#[derive(Clone, Debug, PartialEq)]
pub struct RuleSpan<'a> {
    /// `{` 前面的原文：从上一个 `{`/`}`（或开头）之后起，可能带着前面的注释和 `@charset …;`/`@import …;` 这类语句——
    /// 判断选择器时过 [`rule_selector`]，写回照原文。
    pub prelude: &'a str,
    /// `prelude` 的起点。
    pub start: usize,
    /// `{` 和 `}` 之间的原文（嵌套的花括号也在里面）。
    pub body: &'a str,
    /// `body` 的起点。
    pub body_start: usize,
    /// 规则结束处（`}` 之后）；到文末还没配上 `}` 的是文末（`closed` 为假）。
    pub end: usize,
    pub closed: bool,
    /// 外面套着的 at 规则（`@media …`、`@supports …`）的 `prelude`，由外向内；最多记 [`MAX_MEDIA_NESTING`] 层，实际层数看 `depth`。
    pub parents: Vec<&'a str>,
    pub depth: usize,
}

/// 样式表里的规则，按出现顺序，位置都是原文的（改写时只替换 `body`，别的原样留）。层叠（[`Rules`]）、清洗层和优化器读写
/// 样式表都用它（2026-10-10 审计以前清洗层用一条只认最内层 `{…}` 的正则，字符串、注释里的花括号会切错，和层叠是两套解析）：
/// - 引号、注释、转义按 CSS 规范认（[`lex`]），里面的 `{`、`}`、`;` 不算；
/// - `{` 前面（去掉注释和 `@charset …;` 这类语句以后）以 `@` 开头的是 at 规则：里面有块的（`@media`、`@supports`、`@page`
///   的页边距框……）往里找、自己不给，没有块的（`@font-face`、`@page`）当一条规则给出；外面的 at 规则记进 `parents`，
///   要不要（`@media` 条件成不成立）由调用方定；
/// - 别的是普通规则，整个 `{…}` 给出（CSS 嵌套写法里面的块不再拆）；
/// - `{` 前面一个字都没有的块不给（同以前的正则）；到文末没配上 `}` 的块按 CSS 规范在文末收尾；多余的 `}` 跳过。
///
/// 不递归（套几万层的 `@media` 也不会栈溢出）。
pub fn rule_spans(css: &str) -> Vec<RuleSpan<'_>> {
    struct At<'a> {
        prelude: &'a str,
        start: usize,
        open: usize,
        has_child: bool,
    }
    fn push<'a>(
        out: &mut Vec<RuleSpan<'a>>,
        css: &'a str,
        start: usize,
        open: usize,
        close: Option<usize>,
        ats: &[At<'a>],
    ) {
        if start == open {
            return;
        }
        let end = close.unwrap_or(css.len());
        out.push(RuleSpan {
            prelude: &css[start..open],
            start,
            body: &css[open + 1..end],
            body_start: open + 1,
            end: close.map_or(end, |c| c + 1),
            closed: close.is_some(),
            parents: ats
                .iter()
                .take(MAX_MEDIA_NESTING)
                .map(|a| a.prelude)
                .collect(),
            depth: ats.len(),
        });
    }
    let mut out = Vec::new();
    let mut ats: Vec<At> = Vec::new();
    // 正在读的普通规则：(prelude 起点, `{` 的位置, 花括号层数)
    let mut qual: Option<(usize, usize, usize)> = None;
    let mut last = 0;
    for l in lex(css.char_indices()) {
        let Lex::Code(i, c) = l else { continue };
        match (c, qual.as_mut()) {
            ('{', Some(q)) => q.2 += 1,
            ('}', Some(q)) => {
                q.2 -= 1;
                if q.2 == 0 {
                    push(&mut out, css, q.0, q.1, Some(i), &ats);
                    qual = None;
                    last = i + 1;
                }
            }
            ('{', None) => {
                if let Some(a) = ats.last_mut() {
                    a.has_child = true;
                }
                let prelude = &css[last..i];
                if rule_head(prelude).starts_with('@') {
                    ats.push(At {
                        prelude,
                        start: last,
                        open: i,
                        has_child: false,
                    });
                } else {
                    qual = Some((last, i, 1));
                }
                last = i + 1;
            }
            ('}', None) => {
                if let Some(a) = ats.pop() {
                    if !a.has_child {
                        push(&mut out, css, a.start, a.open, Some(i), &ats);
                    }
                }
                last = i + 1;
            }
            _ => {}
        }
    }
    // 到文末没收尾的：最里面那一块（普通规则，或里面没有块的 at 规则）照收，外面的 at 规则里都有块、本来就不给
    if let Some((start, open, _)) = qual {
        push(&mut out, css, start, open, None, &ats);
    } else if let Some((a, outer)) = ats.split_last() {
        if !a.has_child {
            push(&mut out, css, a.start, a.open, None, outer);
        }
    }
    out
}

/// `{` 前面那段去掉注释、取最后一个顶层 `;` 之后的部分（前面的是 `@charset …;` 这类以分号结束的语句），去掉两边空白。
/// 层叠拿它当选择器、[`rule_spans`] 拿它判断是不是 at 规则。
pub(crate) fn rule_head(prelude: &str) -> String {
    let stmt = split_top(prelude, ';').pop().unwrap_or("");
    strip_comments(stmt).trim().to_string()
}

/// [`RuleSpan::prelude`] 拿来**判断**时的样子：去掉开头的语句式 at 规则（`@charset "utf-8";`、`@import …;`）和注释。清洗层、
/// 优化器判断选择器的地方一律用它，写回仍用原文——以前有的地方只去注释，样式表开头有 `@charset` 时第一条规则被当成 at 规则
/// 跳过（2026-10-10 审计，`capfit`、`bgfit` 等）。
pub fn rule_selector(raw: &str) -> std::borrow::Cow<'_, str> {
    strip_comments(split_leading_statements(raw).1)
}

/// 选择器文本开头的语句式 at 规则（以 `@` 开头、到括号、引号和注释之外的 `;` 为止，可以有好几条，前后可以夹注释）
/// 拆成 (这些语句, 其余)。没有就是 `("", 原文)`。
pub fn split_leading_statements(sel: &str) -> (&str, &str) {
    let mut cut = 0;
    while strip_comments(&sel[cut..]).trim_start().starts_with('@') {
        let rest = &sel[cut..];
        let mut depth = 0usize;
        let end = scan_css(rest.char_indices(), |i, c| {
            match c {
                '(' => depth += 1,
                ')' => depth = depth.saturating_sub(1),
                ';' if depth == 0 => return Some(i + 1),
                _ => {}
            }
            None
        });
        match end {
            Some(e) => cut += e,
            None => break,
        }
    }
    (&sel[..cut], &sel[cut..])
}

/// 选择器（一个逗号分项）的最后一个复合选择器：`div.a > p.b:first-child` → `p.b:first-child`。按空白和组合符 `>`、`+`、`~` 切。
/// 字体分析、Send to Kindle 规则、正文层判断、章尾容器类、整页背景共用。
pub fn last_compound(sel: &str) -> &str {
    sel.trim()
        .rsplit(|c: char| c.is_whitespace() || matches!(c, '>' | '+' | '~'))
        .next()
        .unwrap_or("")
}

/// 复合选择器的标签名（小写，没写是空串）和类名：伪类、属性选择器（`:`、`[` 起）不算，空的类名不算，`p.a.b` → ("p", ["a", "b"])。
pub fn compound_tag_classes(compound: &str) -> (String, Vec<&str>) {
    let c = compound.split([':', '[']).next().unwrap_or("");
    let mut parts = c.split('.');
    let tag = parts.next().unwrap_or("").to_ascii_lowercase();
    (tag, parts.filter(|p| !p.is_empty()).collect())
}

/// 样式表里的 `@font-face` 规则（[`rule_spans`] 里选择器正好是 `@font-face` 的，不分大小写；`@media` 里的也算）。
/// 层叠读嵌入字体（[`font_faces`]）、清洗层剔除死字体（`wash::dead_refs`）、字体分析（`wash::fonts`）共用
/// （以前三处各找各的：两处用一条遇到字符串里的 `}` 就截断的正则）。
pub fn font_face_rules(css: &str) -> impl Iterator<Item = RuleSpan<'_>> {
    rule_spans(css)
        .into_iter()
        .filter(|r| is_font_face(&rule_selector(r.prelude)))
}

/// 选择器（已过 [`rule_selector`]）是不是 `@font-face`。
pub fn is_font_face(sel: &str) -> bool {
    sel.trim().eq_ignore_ascii_case("@font-face")
}

/// 找和 `it` 第一个字符（左括号 `l`）配对的右括号 `r` 的位置，引号里的不算。
pub(crate) fn matching_close(
    it: impl Iterator<Item = (usize, char)>,
    l: char,
    r: char,
) -> Option<usize> {
    let mut depth = 0usize;
    scan_css(it, |i, c| {
        if c == l {
            depth += 1;
        } else if c == r {
            depth = depth.saturating_sub(1);
            if depth == 0 {
                return Some(i);
            }
        }
        None
    })
}

/// 解析 `a: b; c: d !important`。切声明用 [`bookbase::html::css_decls`]（和优化器同一个：引号、括号里的 `;` 不切，
/// 引号里的转义、坏引号退回按 `;` 切），这里只管 `!important` 和展开简写。
pub fn parse_decls(s: &str) -> Vec<Decl> {
    let mut out = Vec::new();
    for d in bookbase::html::css_decls(s) {
        // `css_decls` 会跳过属性名前面的杂字符（`*zoom` → `zoom`）；IE 的 `*color:red` 这类在浏览器里是无效声明，照旧不收。
        let lead = (d.prop.as_ptr() as usize).saturating_sub(d.raw.as_ptr() as usize);
        if !d.raw.get(..lead).unwrap_or("").trim().is_empty() {
            continue;
        }
        let prop = d.prop.to_ascii_lowercase();
        let mut value = d.value.to_string();
        let mut important = false;
        if let Some(i) = value.to_ascii_lowercase().rfind("!important") {
            value.truncate(i);
            value = value.trim().to_string();
            important = true;
        }
        if prop.is_empty() || value.is_empty() {
            continue;
        }
        out.extend(
            expand_shorthand(&prop, &value)
                .into_iter()
                .map(|(prop, value)| Decl {
                    prop,
                    value,
                    important,
                }),
        );
    }
    out
}

/// 按分隔符拆，括号、引号、注释里的不拆。
pub fn split_top(s: &str, sep: char) -> Vec<&str> {
    let mut out = Vec::new();
    let (mut depth, mut start) = (0i32, 0);
    scan_css(s.char_indices(), |i, c| {
        match c {
            '(' => depth += 1,
            ')' => depth -= 1,
            c if c == sep && depth == 0 => {
                out.push(&s[start..i]);
                start = i + c.len_utf8();
            }
            _ => {}
        }
        None::<()>
    });
    out.push(&s[start..]);
    out
}

/// 按顶层空白切词：括号、字符串里的空白不算（`rgb(0, 0, 0)`、`url(a b.png)`、`"A B"`），顶层的注释也算分隔、不进词。
/// 另返回括号配不配对。`margin`/`padding` 等四值简写（[`box_sides`]）、`background` 简写（[`background_tokens`]）、阴影、边框共用。
pub fn tokens(v: &str) -> (Vec<&str>, bool) {
    tokens_with(v, false)
}

/// 同 [`tokens`]，另把括号外的 `/`（`background` 简写的「位置 / 尺寸」）切成单独的词。层叠（`background` 展开）和清洗层
/// （[`crate::decl::background_longhands`]）共用。
pub fn background_tokens(v: &str) -> Vec<&str> {
    tokens_with(v, true).0
}

fn tokens_with(v: &str, slash: bool) -> (Vec<&str>, bool) {
    fn end_at<'a>(v: &'a str, toks: &mut Vec<&'a str>, start: &mut Option<usize>, at: usize) {
        if let Some(s) = start.take() {
            toks.push(&v[s..at]);
        }
    }
    let (mut toks, mut depth, mut start) = (Vec::new(), 0i32, None::<usize>);
    for l in lex(v.char_indices()) {
        match l {
            Lex::Comment(i, _) if depth == 0 => end_at(v, &mut toks, &mut start, i),
            Lex::Comment(i, _) | Lex::Quoted(i) => {
                start.get_or_insert(i);
            }
            Lex::Code(i, c) if c.is_whitespace() && depth == 0 => {
                end_at(v, &mut toks, &mut start, i)
            }
            Lex::Code(i, '/') if slash && depth == 0 => {
                end_at(v, &mut toks, &mut start, i);
                toks.push(&v[i..i + 1]);
            }
            Lex::Code(i, c) => {
                match c {
                    '(' => depth += 1,
                    ')' => depth -= 1,
                    _ => {}
                }
                start.get_or_insert(i);
            }
        }
    }
    end_at(v, &mut toks, &mut start, v.len());
    (toks, depth == 0)
}

/// 一个选择器的优先级 (id, 类/属性/伪类, 标签与伪元素)，按 Selectors 规范：`:not()`/`:is()`/`:has()` 取括号里最高的那个，
/// `:where()` 算 0，别的带括号的伪类（`:nth-child(2n+1)`、`:lang(zh)`）算一个伪类，括号里的字不计（2026-10-06 以前计成标签）。
/// 单冒号的 `:before`/`:after`/`:first-line`/`:first-letter` 是伪元素。
pub fn specificity(sel: &str) -> (u32, u32, u32) {
    let ch: Vec<char> = sel.chars().collect();
    let (mut a, mut b, mut c) = (0u32, 0u32, 0u32);
    let is_ident = |x: char| x.is_alphanumeric() || x == '-' || x == '_' || !x.is_ascii();
    // 从 i 起跳过一个标识符（含 `\` 转义），返回结束位置
    let skip_ident = |mut i: usize| -> usize {
        while i < ch.len() {
            if ch[i] == '\\' {
                i += 2;
            } else if is_ident(ch[i]) {
                i += 1;
            } else {
                break;
            }
        }
        i.min(ch.len())
    };
    // 从 open（`(` 或 `[`）起找配对的右括号，引号里的不算；返回右括号位置（没配上返回末尾）
    let close_of = |open: usize, l: char, r: char| -> usize {
        matching_close(ch.iter().copied().enumerate().skip(open), l, r).unwrap_or(ch.len())
    };
    let mut i = 0;
    while i < ch.len() {
        match ch[i] {
            '#' => {
                a += 1;
                i = skip_ident(i + 1);
            }
            '.' => {
                b += 1;
                i = skip_ident(i + 1);
            }
            '[' => {
                b += 1;
                i = close_of(i, '[', ']') + 1;
            }
            ':' => {
                let element = ch.get(i + 1) == Some(&':');
                let start = i + if element { 2 } else { 1 };
                i = skip_ident(start);
                let name = ch[start.min(i)..i]
                    .iter()
                    .collect::<String>()
                    .to_ascii_lowercase();
                let mut args: Option<String> = None;
                if ch.get(i) == Some(&'(') {
                    let end = close_of(i, '(', ')');
                    args = Some(ch[i + 1..end.min(ch.len())].iter().collect());
                    i = end + 1;
                }
                if element
                    || matches!(
                        name.as_str(),
                        "before" | "after" | "first-line" | "first-letter"
                    )
                {
                    c += 1;
                } else {
                    match (name.as_str(), args) {
                        (
                            "not" | "is" | "matches" | "-webkit-any" | "-moz-any" | "has",
                            Some(inner),
                        ) => {
                            let m = split_top(&inner, ',')
                                .into_iter()
                                .map(specificity)
                                .max()
                                .unwrap_or_default();
                            (a, b, c) = (a + m.0, b + m.1, c + m.2);
                        }
                        ("where", Some(_)) => {}
                        _ => b += 1,
                    }
                }
            }
            x if is_ident(x) || x == '\\' => {
                c += 1;
                i = skip_ident(i);
            }
            _ => i += 1,
        }
    }
    (a, b, c)
}
