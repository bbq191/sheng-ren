//! 够写 KFX 用的 CSS：解析样式表、按选择器优先级层叠、算出每个元素的计算值。KFX 写出器（`kfx::css` 就是这个模块）和
//! 掌阅、Move 的 `kindle_rules`（算全书正文字号，[`crate::wash`]）共用，两边的口径一样。
//!
//! 只认 KFX 能表达的属性（字体、字号、字重、斜体、对齐、缩进、行高、边距、内边距、颜色、背景色、上下标、边框、
//! 列表符号、表格边框合并与间距、文字装饰、字间距、`pre`）；
//! 别的属性忽略。选择器匹配用 scraper（html5ever DOM），优先级自己算。
//! `@media` 按阅读模式的阅读范围、屏幕求值（见 [`media_ok`]），`@font-face`、`@page` 等跳过（嵌入字体另由 [`font_faces`] 读）。

use scraper::{ElementRef, Selector};
use std::collections::HashMap;
use std::sync::Arc;

/// 一条声明。
#[derive(Clone, Debug, PartialEq)]
pub struct Decl {
    pub prop: String,
    pub value: String,
    pub important: bool,
}

struct Rule {
    sel: Selector,
    /// 快速排除：元素不满足它就一定匹配不上（见 [`Need::of`]）；`None` 时只能完整匹配。
    need: Option<Need>,
    spec: (u32, u32, u32),
    decls: Vec<Decl>,
}

/// 选择器最右边那一节（`p.a.b#x`）要求元素有的标签名、类、id。只是**必要条件**：比较一律不分 ASCII 大小写
/// （怪异模式下类名、id 不分大小写，宽一点不会把能匹配的排除掉），满足了还要完整匹配。
/// 大合集几十万个元素、每份样式表几十条规则，逐条完整匹配是写出器最慢的一步（2026-10-06：阿加莎全集层叠 1.1 秒）。
#[derive(Debug, PartialEq)]
struct Need {
    tag: Option<String>,
    classes: Vec<String>,
    ids: Vec<String>,
}

impl Need {
    /// 只认由标识符、`.`、`#`、空白和 `>`/`+`/`~` 组成的选择器；带属性、伪类、`*`、命名空间、转义的不做（返回 `None`）。
    fn of(sel: &str) -> Option<Need> {
        let ident = |c: char| c.is_alphanumeric() || c == '-' || c == '_';
        if !sel.chars().all(|c| ident(c) || matches!(c, '.' | '#' | '>' | '+' | '~') || c.is_whitespace()) {
            return None;
        }
        let last = sel.rsplit(|c: char| c.is_whitespace() || matches!(c, '>' | '+' | '~')).next()?;
        if last.is_empty() {
            return None;
        }
        let mut need = Need { tag: None, classes: Vec::new(), ids: Vec::new() };
        let mut rest = last;
        let tag_end = rest.find(['.', '#']).unwrap_or(rest.len());
        if tag_end > 0 {
            need.tag = Some(rest[..tag_end].to_string());
        }
        rest = &rest[tag_end..];
        while let Some(kind) = rest.chars().next() {
            let body = &rest[1..];
            let end = body.find(['.', '#']).unwrap_or(body.len());
            if end == 0 {
                return None;
            }
            let name = body[..end].to_string();
            if kind == '.' { need.classes.push(name) } else { need.ids.push(name) }
            rest = &body[end..];
        }
        Some(need)
    }

    fn may_match(&self, el: &ElementRef) -> bool {
        let v = el.value();
        self.tag.as_deref().is_none_or(|t| v.name().eq_ignore_ascii_case(t))
            && self.ids.iter().all(|i| v.id().is_some_and(|x| x.eq_ignore_ascii_case(i)))
            && self.classes.iter().all(|c| v.classes().any(|x| x.eq_ignore_ascii_case(c)))
    }
}

/// 解析好的一份样式表（规则按出现顺序）。同一份样式表被很多文档引用时只解析一次，各文档的 [`Sheet`] 共用。
#[derive(Default)]
pub struct Rules(Vec<Rule>);

/// `@media` 最多套几层（真书里最多两层；更深的整块不收）。
const MAX_MEDIA_NESTING: usize = 32;

impl Rules {
    /// 解析一份样式表；`url(…)` 按样式表自己的路径 `base`（书内路径）换成书内路径（`base` 空时不换）。
    /// `@media` 块按 `media` 求值（见 [`media_ok`]）。
    pub fn parse(css: &str, base: &str, media: Option<&MediaEnv>) -> Arc<Rules> {
        let mut r = Rules::default();
        r.add(&strip_comments(css), base, media);
        Arc::new(r)
    }

    /// `css` 已去掉注释。规则按 [`rule_spans`] 找：外面套的 at 规则都得是条件成立的 `@media`（别的 at 规则里面的不收），
    /// 套超过 [`MAX_MEDIA_NESTING`] 层的不收。
    fn add(&mut self, css: &str, base: &str, media: Option<&MediaEnv>) {
        let media_block_ok = |prelude: &str| {
            let head = rule_head(prelude).to_ascii_lowercase();
            head.strip_prefix("@media").is_some_and(|q| media_ok(q, media))
        };
        for r in rule_spans(css) {
            if r.depth > MAX_MEDIA_NESTING || !r.parents.iter().all(|p| media_block_ok(p)) {
                continue;
            }
            let head = rule_head(r.prelude);
            if head.is_empty() || head.starts_with('@') {
                continue;
            }
            let mut decls = parse_decls(r.body);
            resolve_urls(&mut decls, base);
            if decls.is_empty() {
                continue;
            }
            for one in split_top(&head, ',') {
                let one = one.trim();
                if let Ok(sel) = Selector::parse(one) {
                    self.0.push(Rule { sel, need: Need::of(one), spec: specificity(one), decls: decls.clone() });
                }
            }
        }
    }
}

/// 声明里的 `url(…)` 按 `base`（样式表或行内 `style` 所在文件的书内路径）换成书内路径；`base` 空时不换。
fn resolve_urls(decls: &mut [Decl], base: &str) {
    if base.is_empty() {
        return;
    }
    for d in decls.iter_mut() {
        if let Some(inner) = leading_url(&d.value) {
            d.value = format!("url({})", crate::epubzip::resolve_link(base, inner).0);
        }
    }
}

/// 值是 `url(…)` 开头时括号里的地址（去掉引号；按 [`crate::html::css_urls`] 认）。
fn leading_url(value: &str) -> Option<&str> {
    crate::html::css_urls(value).into_iter().next().filter(|u| u.start == 0).map(|u| u.value)
}

/// 一个文档用到的样式表：各份 [`Rules`] 和它们第一条规则的全局序号（后出现的规则优先）。
#[derive(Default)]
pub struct Sheet {
    parts: Vec<(Arc<Rules>, usize)>,
}

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
    Lexer { it: it.peekable(), quote: None }
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
fn scan_css<R>(it: impl Iterator<Item = (usize, char)>, mut f: impl FnMut(usize, char) -> Option<R>) -> Option<R> {
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
    fn push<'a>(out: &mut Vec<RuleSpan<'a>>, css: &'a str, start: usize, open: usize, close: Option<usize>, ats: &[At<'a>]) {
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
            parents: ats.iter().take(MAX_MEDIA_NESTING).map(|a| a.prelude).collect(),
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
                    ats.push(At { prelude, start: last, open: i, has_child: false });
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
fn rule_head(prelude: &str) -> String {
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
    sel.trim().rsplit(|c: char| c.is_whitespace() || matches!(c, '>' | '+' | '~')).next().unwrap_or("")
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
    rule_spans(css).into_iter().filter(|r| is_font_face(&rule_selector(r.prelude)))
}

/// 选择器（已过 [`rule_selector`]）是不是 `@font-face`。
pub fn is_font_face(sel: &str) -> bool {
    sel.trim().eq_ignore_ascii_case("@font-face")
}

/// 找和 `it` 第一个字符（左括号 `l`）配对的右括号 `r` 的位置，引号里的不算。
fn matching_close(it: impl Iterator<Item = (usize, char)>, l: char, r: char) -> Option<usize> {
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

/// 解析 `a: b; c: d !important`。切声明用 [`crate::html::css_decls`]（和优化器同一个：引号、括号里的 `;` 不切，
/// 引号里的转义、坏引号退回按 `;` 切），这里只管 `!important` 和展开简写。
pub fn parse_decls(s: &str) -> Vec<Decl> {
    let mut out = Vec::new();
    for d in crate::html::css_decls(s) {
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
        out.extend(expand_shorthand(&prop, &value).into_iter().map(|(prop, value)| Decl { prop, value, important }));
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
/// （[`crate::cssunlock::background_longhands`]）共用。
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
            Lex::Code(i, c) if c.is_whitespace() && depth == 0 => end_at(v, &mut toks, &mut start, i),
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

/// `margin`/`padding` 这类四值简写拆成的四边。
#[derive(Debug, PartialEq, Eq)]
pub enum BoxSides<'a> {
    /// 上、右、下、左，和 `" !important"`（没有就是空串）。
    Sides([&'a str; 4], &'static str),
    /// 整体一个全局关键字（`inherit`/`initial`/`unset`/`revert`），不能跟别的边混写在一个简写里。
    Keyword(&'a str, &'static str),
    /// 拆不清（超过 4 个值、括号不配对、关键字混在别的值里）。
    Unknown,
}

/// 拆四值简写（`margin`、`padding`、`border-style`/`-width`/`-color`、`border-radius`）的值：1–4 个值按 CSS 规则展开成四边
/// （1 个四边一样，2 个是上下、左右，3 个是上、左右、下）；括号里的空格不算分隔（`calc(1em + 2px)`）。层叠、清洗层段距归零
/// （`wash::css`）、章尾去下边距（`wash::layout`）、`kindle_rules` 共用（以前层叠另有一份按空白切的，会把 `calc()` 切坏、
/// 超过 4 个值也照取前 4 个）。
pub fn box_sides(val: &str) -> BoxSides<'_> {
    let (v, important) = crate::cssunlock::split_important(val);
    let (parts, balanced) = tokens(v);
    if !balanced {
        return BoxSides::Unknown;
    }
    let keyword = |p: &str| matches!(p.to_ascii_lowercase().as_str(), "inherit" | "initial" | "unset" | "revert" | "revert-layer");
    match parts[..] {
        [k] if keyword(k) => BoxSides::Keyword(k, important),
        _ if parts.iter().any(|p| keyword(p)) => BoxSides::Unknown,
        [a] => BoxSides::Sides([a, a, a, a], important),
        [a, b] => BoxSides::Sides([a, b, a, b], important),
        [a, b, c] => BoxSides::Sides([a, b, c, b], important),
        [a, b, c, d] => BoxSides::Sides([a, b, c, d], important),
        _ => BoxSides::Unknown,
    }
}

/// 层叠用：四值简写展开成上、右、下、左（全局关键字四边都是它）；拆不清的整条不认（`None`）。
fn four_values(value: &str) -> Option<[&str; 4]> {
    match box_sides(value) {
        BoxSides::Sides(v, _) => Some(v),
        BoxSides::Keyword(k, _) => Some([k; 4]),
        BoxSides::Unknown => None,
    }
}

fn expand_shorthand(prop: &str, value: &str) -> Vec<(String, String)> {
    let four = |base: &str| -> Vec<(String, String)> {
        let Some(v) = four_values(value) else { return Vec::new() };
        ["top", "right", "bottom", "left"].iter().zip(v).map(|(side, v)| (format!("{base}-{side}"), v.to_string())).collect()
    };
    // `border-top: 1px solid red` 这类：拆成样式、宽度、颜色（没写的按 CSS 缺省：无、medium、当前颜色）。
    let border_side = |side: &str| -> Vec<(String, String)> {
        let (mut style, mut width, mut color) = ("none".to_string(), "medium".to_string(), String::new());
        for t in tokens(value).0 {
            let l = t.to_ascii_lowercase();
            if BORDER_STYLES.contains(&l.as_str()) {
                style = l;
            } else if parse_color(t).is_some() || l == "currentcolor" || l == "transparent" {
                color = t.to_string();
            } else {
                width = t.to_string();
            }
        }
        let mut v = vec![(format!("border-{side}-style"), style), (format!("border-{side}-width"), width)];
        v.push((format!("border-{side}-color"), color));
        v
    };
    let sides = ["top", "right", "bottom", "left"];
    match prop {
        "margin" => four("margin"),
        "padding" => four("padding"),
        "border" => sides.iter().flat_map(|s| border_side(s)).collect(),
        "border-top" | "border-right" | "border-bottom" | "border-left" => border_side(&prop[7..]),
        "border-style" | "border-width" | "border-color" => {
            let what = &prop[7..];
            four("border").into_iter().map(|(k, v)| (format!("{k}-{what}"), v)).collect()
        }
        "border-radius" => {
            // 只取「/」前面的（水平半径）；顺序是左上、右上、右下、左下。
            let Some(v) = four_values(value.split('/').next().unwrap_or("")) else { return Vec::new() };
            ["top-left", "top-right", "bottom-right", "bottom-left"].iter().zip(v).map(|(c, v)| (format!("border-{c}-radius"), v.to_string())).collect()
        }
        "list-style" => {
            let mut out = Vec::new();
            for t in value.split_whitespace() {
                let l = t.to_ascii_lowercase();
                if l == "inside" || l == "outside" {
                    out.push(("list-style-position".to_string(), l));
                } else if !l.starts_with("url(") {
                    out.push(("list-style-type".to_string(), l));
                }
            }
            out
        }
        // `text-decoration: underline solid red` 只留线的种类。
        "text-decoration" | "text-decoration-line" => vec![("text-decoration".to_string(), value.to_ascii_lowercase())],
        "background" => background_shorthand(value),
        "font" => font_shorthand(value),
        _ => vec![(prop.to_string(), value.to_string())],
    }
}

/// `font` 简写拆开的样子（CSS 2.1 / CSS Fonts 3 的语法：`[style || variant || weight || stretch]? size[/line-height]? family`）。
#[derive(Debug, PartialEq)]
pub(crate) struct FontShorthand<'a> {
    pub(crate) style: Option<&'a str>,
    pub(crate) variant: Option<&'a str>,
    pub(crate) weight: Option<&'a str>,
    pub(crate) stretch: Option<&'a str>,
    pub(crate) size: &'a str,
    pub(crate) line_height: Option<&'a str>,
    pub(crate) family: &'a str,
}

/// 按 CSS 规范拆 `font` 简写（值里已去掉 `!important`）。拿不准的（系统字体关键字 `caption`、全局关键字 `inherit`、
/// 字号前的词认不出、字号或行高是 `calc()` 之类带空格的写法、没有字体族）返回 `None`，不处理。
pub(crate) fn parse_font_shorthand(v: &str) -> Option<FontShorthand<'_>> {
    fn is_size(t: &str) -> bool {
        let l = t.to_ascii_lowercase();
        matches!(l.as_str(), "xx-small" | "x-small" | "small" | "medium" | "large" | "x-large" | "xx-large" | "xxx-large" | "larger" | "smaller")
            || (l.starts_with(|c: char| c.is_ascii_digit() || c == '.') && l.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'%'))
    }
    fn is_line_height(t: &str) -> bool {
        t.eq_ignore_ascii_case("normal") || is_size(t)
    }
    const STRETCH: [&str; 8] = ["ultra-condensed", "extra-condensed", "condensed", "semi-condensed", "semi-expanded", "expanded", "extra-expanded", "ultra-expanded"];
    let mut f = FontShorthand { style: None, variant: None, weight: None, stretch: None, size: "", line_height: None, family: "" };
    let mut rest = v.trim();
    // 字号之前的词：各最多一个，`normal` 可以出现几次（分不清是哪一项，都等于缺省）
    let mut normals = 0;
    loop {
        let end = rest.find(char::is_whitespace)?;
        let tok = &rest[..end];
        let l = tok.to_ascii_lowercase();
        let slot = match l.as_str() {
            "normal" => {
                normals += 1;
                None
            }
            "italic" | "oblique" => Some(&mut f.style),
            "small-caps" => Some(&mut f.variant),
            "bold" | "bolder" | "lighter" => Some(&mut f.weight),
            _ if l.len() == 3 && l.ends_with("00") && (b'1'..=b'9').contains(&l.as_bytes()[0]) => Some(&mut f.weight),
            _ if STRETCH.contains(&l.as_str()) => Some(&mut f.stretch),
            _ => break,
        };
        if let Some(slot) = slot {
            if slot.is_some() {
                return None;
            }
            *slot = Some(tok);
        }
        if normals + [f.style, f.variant, f.weight, f.stretch].iter().filter(|x| x.is_some()).count() > 4 {
            return None;
        }
        rest = rest[end..].trim_start();
    }
    let end = rest.find(|c: char| c.is_whitespace() || c == '/')?;
    f.size = &rest[..end];
    if !is_size(f.size) {
        return None;
    }
    rest = rest[end..].trim_start();
    if let Some(r) = rest.strip_prefix('/') {
        let r = r.trim_start();
        let end = r.find(char::is_whitespace)?;
        let lh = &r[..end];
        if !is_line_height(lh) {
            return None;
        }
        f.line_height = Some(lh);
        rest = r[end..].trim_start();
    }
    f.family = rest.trim();
    (!f.family.is_empty()).then_some(f)
}

/// `font` 简写展开成分项。简写会把没写的项重置成缺省值（CSS 规范），所以没写的字体样式、小型大写、粗细、行高都是 `normal`；
/// `font-stretch` 不收（KFX 没有对应的样式）。拆不清的（系统字体关键字、全局关键字、`calc()` 字号……，见 [`parse_font_shorthand`]）
/// 整条不认（和以前一样：以前 `font` 简写一律不认，2026-10-10 审计补上——掌阅、Move 统计正文字号看不到写在简写里的字号，
/// 改写时却照样缩放它）。
fn font_shorthand(value: &str) -> Vec<(String, String)> {
    let Some(f) = parse_font_shorthand(value) else { return Vec::new() };
    let or_normal = |v: Option<&str>| v.unwrap_or("normal").to_string();
    vec![
        ("font-style".to_string(), or_normal(f.style)),
        ("font-variant".to_string(), or_normal(f.variant)),
        ("font-weight".to_string(), or_normal(f.weight)),
        ("font-size".to_string(), f.size.to_string()),
        ("line-height".to_string(), or_normal(f.line_height)),
        ("font-family".to_string(), f.family.to_string()),
    ]
}

/// `background` 简写里没写的位置、尺寸记成这个值：按初始值算（不写出来）。
const BG_INITIAL: &str = "initial";

/// `background: url(…) bottom / 100% no-repeat fixed rgba(…)` 拆成各项。没写的项按 CSS 回到初始值（无图、重复、滚动、透明、
/// 位置尺寸不写）：`background:none`、只写颜色的简写会盖掉前面规则的背景图（以前没写的不出现、不重置，背景图还留着）。
/// `inherit`/`initial`/`unset` 这类整个值是全局关键字的不展开（不认）。
fn background_shorthand(value: &str) -> Vec<(String, String)> {
    if matches!(value.trim().to_ascii_lowercase().as_str(), "inherit" | "initial" | "unset" | "revert" | "revert-layer") {
        return Vec::new();
    }
    let (mut pos, mut size, mut after_slash) = (Vec::new(), Vec::new(), false);
    let mut out = Vec::new();
    for t in background_tokens(value) {
        let l = t.to_ascii_lowercase();
        if l.starts_with("url(") || l == "none" {
            out.push(("background-image".to_string(), t.to_string()));
        } else if l == "/" {
            after_slash = true;
        } else if ["repeat", "no-repeat", "repeat-x", "repeat-y", "space", "round"].contains(&l.as_str()) {
            out.push(("background-repeat".to_string(), l));
        } else if ["fixed", "scroll", "local"].contains(&l.as_str()) {
            out.push(("background-attachment".to_string(), l));
        } else if parse_color(t).is_some() || l == "transparent" {
            out.push(("background-color".to_string(), t.to_string()));
        } else if after_slash {
            size.push(l);
        } else {
            pos.push(l);
        }
    }
    for (k, v) in [("background-position", pos), ("background-size", size)] {
        out.push((k.to_string(), if v.is_empty() { BG_INITIAL.to_string() } else { v.join(" ") }));
    }
    for (k, init) in [("background-image", "none"), ("background-repeat", "repeat"), ("background-attachment", "scroll"), ("background-color", "transparent")] {
        if !out.iter().any(|(p, _)| p == k) {
            out.push((k.to_string(), init.to_string()));
        }
    }
    out
}

/// `background-position` → (x, y) 百分比或长度；关键字按 CSS（只写一个时另一维居中）。
pub fn parse_bg_position(v: &str) -> [Option<Len>; 2] {
    let kw = |t: &str| match t {
        "left" | "top" => Some(Len::Percent(0.0)),
        "center" => Some(Len::Percent(50.0)),
        "right" | "bottom" => Some(Len::Percent(100.0)),
        _ => parse_len(t),
    };
    let t: Vec<&str> = v.split_whitespace().collect();
    match t.as_slice() {
        [a] if *a == BG_INITIAL => [None, None],
        [a] if matches!(*a, "top" | "bottom") => [Some(Len::Percent(50.0)), kw(a)],
        [a] => [kw(a), Some(Len::Percent(50.0))],
        [a, b] if matches!(*a, "top" | "bottom") || matches!(*b, "left" | "right") => [kw(b), kw(a)],
        [a, b, ..] => [kw(a), kw(b)],
        [] => [None, None],
    }
}

/// `background-size` → (宽, 高)；`cover` 照 Amazon 写成 100% 100%（《绍宋》样本），`auto` 不写。
pub fn parse_bg_size(v: &str) -> [Option<Len>; 2] {
    let one = |t: &str| (t != "auto").then(|| parse_len(t)).flatten();
    let t: Vec<&str> = v.split_whitespace().collect();
    match t.as_slice() {
        ["cover"] => [Some(Len::Percent(100.0)), Some(Len::Percent(100.0))],
        ["contain"] => [Some(Len::Percent(100.0)), None],
        [a] => [one(a), None],
        [a, b, ..] => [one(a), one(b)],
        [] => [None, None],
    }
}

const BORDER_STYLES: &[&str] = &["none", "hidden", "solid", "dashed", "dotted", "double", "groove", "ridge", "inset", "outset"];

/// 一个选择器的优先级 (id, 类/属性/伪类, 标签与伪元素)，按 Selectors 规范：`:not()`/`:is()`/`:has()` 取括号里最高的那个，
/// `:where()` 算 0，别的带括号的伪类（`:nth-child(2n+1)`、`:lang(zh)`）算一个伪类，括号里的字不计（2026-10-06 以前计成标签）。
/// 单冒号的 `:before`/`:after`/`:first-line`/`:first-letter` 是伪元素。
fn specificity(sel: &str) -> (u32, u32, u32) {
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
    let close_of = |open: usize, l: char, r: char| -> usize { matching_close(ch.iter().copied().enumerate().skip(open), l, r).unwrap_or(ch.len()) };
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
                let name = ch[start.min(i)..i].iter().collect::<String>().to_ascii_lowercase();
                let mut args: Option<String> = None;
                if ch.get(i) == Some(&'(') {
                    let end = close_of(i, '(', ')');
                    args = Some(ch[i + 1..end.min(ch.len())].iter().collect());
                    i = end + 1;
                }
                if element || matches!(name.as_str(), "before" | "after" | "first-line" | "first-letter") {
                    c += 1;
                } else {
                    match (name.as_str(), args) {
                        ("not" | "is" | "matches" | "-webkit-any" | "-moz-any" | "has", Some(inner)) => {
                            let m = split_top(&inner, ',').into_iter().map(specificity).max().unwrap_or_default();
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

/// 全书正文字号（根 em）：`count` 是字号（千分之一取整）→ 字数（不算空白），取字数最多的，字数一样多的取小的（结果和遍历顺序
/// 无关）。不在 0.5–3 之间的不信，用 1。KFX 写出器（按解析好的块数）和掌阅、Move 的 `kindle_rules`（按 DOM 文字数）共用。
pub fn body_font_size(count: &HashMap<i64, usize>) -> f64 {
    weighted_mode(count).filter(|v| (0.5..=3.0).contains(v)).unwrap_or(1.0)
}

/// 按字数加权的众数（键是千分之一取整的值），字数一样多的取小的。
pub fn weighted_mode(count: &HashMap<i64, usize>) -> Option<f64> {
    count.iter().max_by_key(|&(&k, &n)| (n, std::cmp::Reverse(k))).map(|(&k, _)| k as f64 / 1000.0)
}

/// 求值 `@media` 特性条件用的环境（像素）：`width`/`height` 是阅读范围，`device-width`/`device-height` 是屏幕，
/// 都由调用方从阅读模式传入（[`MediaEnv::for_profile`]），这里不写死数字。CSS 的 1px 按设备 1 像素算（Kindle 实际怎么算没核实）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MediaEnv {
    pub width: f64,
    pub height: f64,
    pub device_width: f64,
    pub device_height: f64,
    /// 彩色屏（`false`＝黑白：`(color)` 不成立、`(monochrome)` 成立）。
    pub color: bool,
}

impl MediaEnv {
    /// 阅读模式的 KFX 阅读范围、屏幕、黑白彩色（KFX 写出器用）。
    pub fn for_profile(p: &profile::Profile) -> MediaEnv {
        MediaEnv::for_format(p, profile::Format::Kfx)
    }

    /// 阅读模式在格式 `format` 下的阅读范围、屏幕、黑白彩色。掌阅、Move 的 `kindle_rules` 统计正文字号时按 EPUB 的阅读范围求
    /// `@media`（阅读器自己就是这样求的），口径同 KFX 写出器按 KFX 阅读范围求。
    pub fn for_format(p: &profile::Profile, format: profile::Format) -> MediaEnv {
        let r = p.readable(format);
        MediaEnv { width: r.width.into(), height: r.height.into(), device_width: p.screen.width.into(), device_height: p.screen.height.into(), color: p.color }
    }
}

/// 收的媒体类型：`all`、`screen` 和 Kindle 的 `amzn-kf8`（`amzn-mobi`、`print` 等都不是）。
const MEDIA_TYPES: &[&str] = &["all", "screen", "amzn-kf8"];

/// `@media`（或 `<link>`/`<style>` 的 `media` 属性）的条件成不成立。按 Media Queries 规范：逗号列表任一条成立即成立；
/// 每条是 `[not|only] 媒体类型 [and (特性)]*` 或 `(特性) [and (特性)]*`，`not` 把整条取反；媒体类型不认识的算不成立
/// （所以 `not amzn-mobi` 成立）。特性按 `env` 求值；**求值不了的**（没有 `env`、`em` 之类相对单位、`resolution`、
/// 范围写法 `(width >= 600px)`、`or` 等）和写错的那一条**不收**，带 `not` 也不收。空条件成立。
pub fn media_ok(query: &str, env: Option<&MediaEnv>) -> bool {
    let q = query.trim();
    q.is_empty() || split_top(q, ',').into_iter().any(|one| media_query(one, env) == Some(true))
}

/// 一条媒体查询：`Some(成立与否)`；写错或求值不了 → `None`。
fn media_query(q: &str, env: Option<&MediaEnv>) -> Option<bool> {
    let q = q.trim().to_ascii_lowercase();
    // 切成词和括号组
    let mut toks: Vec<&str> = Vec::new();
    let mut i = 0;
    let b = q.as_bytes();
    while i < b.len() {
        if b[i].is_ascii_whitespace() {
            i += 1;
        } else if b[i] == b'(' {
            let start = i;
            i = matching_close(q[start..].char_indices().map(|(k, c)| (start + k, c)), '(', ')')?;
            toks.push(&q[start..=i]);
            i += 1;
        } else {
            let start = i;
            while i < b.len() && !b[i].is_ascii_whitespace() && b[i] != b'(' {
                i += 1;
            }
            toks.push(&q[start..i]);
        }
    }
    let mut rest = toks.as_slice();
    let negate = match rest.first() {
        Some(&"not") => {
            rest = &rest[1..];
            true
        }
        Some(&"only") => {
            rest = &rest[1..];
            false
        }
        _ => false,
    };
    let mut ok = true;
    match rest.first() {
        Some(t) if t.starts_with('(') => {}
        Some(t) => {
            ok = MEDIA_TYPES.contains(t);
            rest = &rest[1..];
            if let Some((&"and", r)) = rest.split_first() {
                rest = r;
                if rest.is_empty() {
                    return None;
                }
            } else if !rest.is_empty() {
                return None;
            }
        }
        None => return None,
    }
    // 特性：(特性) [and (特性)]*
    let mut expect_feature = !rest.is_empty();
    for t in rest {
        if expect_feature {
            let inner = t.strip_prefix('(')?.strip_suffix(')')?;
            ok &= media_feature(inner, env?)?;
        } else if *t != "and" {
            return None;
        }
        expect_feature = !expect_feature;
    }
    if !rest.is_empty() && expect_feature {
        return None; // 以 `and` 结尾
    }
    Some(ok != negate)
}

/// 一个特性（括号里面的部分）：`Some(成立与否)`；不认识、求值不了 → `None`。
fn media_feature(f: &str, env: &MediaEnv) -> Option<bool> {
    let (name, value) = match f.split_once(':') {
        Some((n, v)) => (n.trim(), Some(v.trim())),
        None => (f.trim(), None),
    };
    if name.is_empty() || !name.bytes().all(|c| c.is_ascii_lowercase() || c == b'-') {
        return None; // 范围写法、嵌套、`or` 等
    }
    let (cmp, base) = match name.strip_prefix("min-") {
        Some(b) => (std::cmp::Ordering::Greater, b),
        None => match name.strip_prefix("max-") {
            Some(b) => (std::cmp::Ordering::Less, b),
            None => (std::cmp::Ordering::Equal, name),
        },
    };
    let cmp_num = |actual: f64, want: f64| -> bool {
        let eps = 1e-6;
        match cmp {
            std::cmp::Ordering::Greater => actual >= want - eps,
            std::cmp::Ordering::Less => actual <= want + eps,
            std::cmp::Ordering::Equal => (actual - want).abs() <= eps,
        }
    };
    let ratio = |v: &str| -> Option<f64> {
        let (a, b) = v.split_once('/').unwrap_or((v, "1"));
        let (a, b): (f64, f64) = (a.trim().parse().ok()?, b.trim().parse().ok()?);
        (b > 0.0 && a.is_finite()).then(|| a / b)
    };
    let length = |actual: f64| -> Option<bool> { Some(cmp_num(actual, media_px(value?)?)) };
    match base {
        "width" => length(env.width),
        "height" => length(env.height),
        "device-width" => length(env.device_width),
        "device-height" => length(env.device_height),
        "aspect-ratio" => Some(cmp_num(env.width / env.height, ratio(value?)?)),
        "device-aspect-ratio" => Some(cmp_num(env.device_width / env.device_height, ratio(value?)?)),
        "orientation" if cmp == std::cmp::Ordering::Equal => match value? {
            "portrait" => Some(env.height >= env.width),
            "landscape" => Some(env.width > env.height),
            _ => None,
        },
        // 颜色位数只知道是不是 0：黑白屏 color 是 0，彩色屏 monochrome 是 0；别的位数不知道 → 求值不了
        "color" | "monochrome" => {
            let zero = if base == "color" { !env.color } else { env.color };
            match value {
                None if cmp == std::cmp::Ordering::Equal => Some(!zero),
                None => None,
                Some(v) => {
                    let want: f64 = v.parse().ok()?;
                    zero.then(|| cmp_num(0.0, want))
                }
            }
        }
        _ => None,
    }
}

/// 媒体查询里的长度 → 像素：`px` 和绝对单位（1in = 96px）；`em`/`rem`/`vw` 等相对单位求值不了。
fn media_px(v: &str) -> Option<f64> {
    let (n, unit) = split_number(v.trim())?;
    let per = match unit {
        "px" => 1.0,
        "" if n == 0.0 => 1.0,
        "in" => 96.0,
        "cm" => 96.0 / 2.54,
        "mm" => 96.0 / 25.4,
        "q" => 96.0 / 101.6,
        "pt" => 96.0 / 72.0,
        "pc" => 16.0,
        _ => return None,
    };
    Some(n * per)
}

impl Sheet {
    /// 追加一份样式表。`order` 是全局序号起点（后出现的规则优先），返回下一个序号。
    pub fn add(&mut self, css: &str, order: usize) -> usize {
        self.add_at(css, order, "", None)
    }

    /// 同 [`Sheet::add`]，`url(…)` 按样式表自己的路径 `base`（书内路径）换成书内路径。
    pub fn add_at(&mut self, css: &str, order: usize, base: &str, media: Option<&MediaEnv>) -> usize {
        self.add_rules(Rules::parse(css, base, media), order)
    }

    /// 追加一份解析好的样式表（见 [`Rules::parse`]），返回下一个序号。
    pub fn add_rules(&mut self, rules: Arc<Rules>, order: usize) -> usize {
        let next = order + rules.0.len();
        self.parts.push((rules, order));
        next
    }

    /// 一篇 HTML 文档（书内路径 `doc_path`）生效的样式表：按文档顺序收 `<style>` 和 `<link rel="stylesheet">`，`media` 属性
    /// 和 `@media` 同一口径（按 `media` 求，不成立的整份跳过）。外部样式表由 `rules_for(书内路径)` 给解析好的规则——
    /// 怎么解析、怎么缓存是调用方的事（KFX 写出器跨线程共享缓存，`kindle_rules` 事先全解析好），找不到的跳过。
    /// KFX 写出器和掌阅、Move 的 `kindle_rules` 共用，两边统计正文字号的口径因此一样（2026-10-10 审计以前各写一份）。
    pub fn for_doc(html: &scraper::Html, doc_path: &str, media: Option<&MediaEnv>, mut rules_for: impl FnMut(&str) -> Option<Arc<Rules>>) -> Sheet {
        static LINK_STYLE: std::sync::OnceLock<Selector> = std::sync::OnceLock::new();
        let sel = LINK_STYLE.get_or_init(|| Selector::parse("link, style").unwrap_or_else(|_| unreachable!()));
        let mut sheet = Sheet::default();
        let mut order = 0;
        for el in html.select(sel) {
            if el.value().attr("media").is_some_and(|m| !media_ok(m, media)) {
                continue;
            }
            if el.value().name() == "style" {
                order = sheet.add_at(&el.text().collect::<String>(), order, doc_path, media);
            } else if el.value().attr("rel").is_some_and(|r| r.to_ascii_lowercase().contains("stylesheet")) {
                if let Some(rules) = el.value().attr("href").and_then(|h| rules_for(&crate::epubzip::resolve_link(doc_path, h).0)) {
                    order = sheet.add_rules(rules, order);
                }
            }
        }
        sheet
    }

    /// 一个元素上生效的声明（按层叠排好，后面的覆盖前面的），含行内 `style`。
    /// 行内 `style` 里的 `url(…)` 按文档路径 `base` 解析（同样式表按自己的路径；以前不解析，子目录里文档的行内背景图找不到）。
    pub fn cascade(&self, el: &ElementRef, base: &str) -> HashMap<String, String> {
        // (!important, 优先级, 出现顺序, 声明)
        type Hit<'a> = (bool, (u32, u32, u32), usize, &'a Decl);
        let mut hits: Vec<Hit> = Vec::new();
        for (rules, base) in &self.parts {
            for (i, r) in rules.0.iter().enumerate() {
                if r.need.as_ref().is_none_or(|n| n.may_match(el)) && r.sel.matches(el) {
                    for d in &r.decls {
                        hits.push((d.important, r.spec, base + i, d));
                    }
                }
            }
        }
        let mut inline: Vec<Decl> = el.value().attr("style").map(parse_decls).unwrap_or_default();
        resolve_urls(&mut inline, base);
        for d in &inline {
            hits.push((d.important, (1000, 0, 0), usize::MAX, d));
        }
        hits.sort_by_key(|h| (h.0, h.1, h.2));
        let mut out = HashMap::new();
        for (_, _, _, d) in hits {
            out.insert(d.prop.clone(), d.value.clone());
        }
        out
    }
}

/// 缺省是块级的标签（`display` 没写时）。
pub const BLOCK_TAGS: &[&str] = &[
    "address", "article", "aside", "blockquote", "body", "center", "dd", "details", "dialog", "dir", "div", "dl", "dt", "fieldset", "figcaption",
    "figure", "footer", "form", "h1", "h2", "h3", "h4", "h5", "h6", "header", "hgroup", "hr", "li", "main", "menu", "nav", "ol", "p", "pre",
    "section", "summary", "table", "tbody", "td", "tfoot", "th", "thead", "tr", "ul", "caption",
];

/// 元素是不是块级：先看 `display`，没写按标签。
pub fn is_block(el: &ElementRef, comp: &Computed) -> bool {
    match comp.display.as_deref() {
        Some("block" | "list-item" | "table" | "table-row" | "table-cell" | "flex") => true,
        Some("inline" | "inline-block") => false,
        _ => BLOCK_TAGS.contains(&el.value().name()),
    }
}

/// HTML 的表现属性当成优先级最低的样式（样式表写了的不动）：块的 `align`、`<center>`、`<font size/color/face>`
/// （Send to Kindle 同样：《福尔摩斯》`<p align="justify">` 两端对齐、`<font size="1">` 字号 0.625、`size="7"` 3.0）。
pub fn presentational_hints(el: &ElementRef, decls: &mut HashMap<String, String>) {
    let e = el.value();
    let mut hint = |k: &str, v: String| {
        decls.entry(k.to_string()).or_insert(v);
    };
    match e.name() {
        "p" | "div" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6" | "td" | "th" | "tr" | "caption" | "blockquote" => {
            if let Some(a) = e.attr("align").map(|a| a.trim().to_ascii_lowercase()).filter(|a| ["left", "right", "center", "justify"].contains(&a.as_str())) {
                hint("text-align", a);
            }
        }
        "center" => hint("text-align", "center".into()),
        "font" => {
            // HTML 字号 1–7（同浏览器：10、13、16、18、24、32、48px，绝对字号），`+n`/`-n` 相对 3（饱和加减：`+2147483647` 以前溢出）
            if let Some(s) = e.attr("size").map(str::trim) {
                let n = match s.strip_prefix('+') {
                    Some(r) => r.parse::<i32>().ok().map(|r| 3i32.saturating_add(r)),
                    None => match s.strip_prefix('-') {
                        Some(r) => r.parse::<i32>().ok().map(|r| 3i32.saturating_sub(r)),
                        None => s.parse::<i32>().ok(),
                    },
                };
                if let Some(n) = n {
                    let em = [0.625, 0.8125, 1.0, 1.125, 1.5, 2.0, 3.0][(n.clamp(1, 7) - 1) as usize];
                    hint("font-size", format!("{em}rem"));
                }
            }
            if let Some(c) = e.attr("color") {
                hint("color", c.trim().to_string());
            }
            if let Some(f) = e.attr("face") {
                hint("font-family", f.trim().to_string());
            }
        }
        _ => {}
    }
}

/// 一条 `@font-face`：字体名、字体文件（相对书根的路径）、是不是粗体、斜体。
#[derive(Clone, Debug, PartialEq)]
pub struct FontFace {
    pub family: String,
    pub path: String,
    pub bold: bool,
    pub italic: bool,
}

/// 样式表里的 `@font-face`（只收 `src` 里有 `url()` 指到书内文件的）。`base` 是样式表所在文件的路径。
pub fn font_faces(css: &str, base: &str) -> Vec<FontFace> {
    let css = strip_comments(css);
    let mut out = Vec::new();
    for r in font_face_rules(&css) {
        let decls = parse_decls(r.body);
        let get = |k: &str| decls.iter().rev().find(|d| d.prop == k).map(|d| d.value.as_str());
        let family = get("font-family").map(|f| f.trim().trim_matches(['"', '\'']).to_string()).filter(|f| !f.is_empty());
        let url = get("src").and_then(|src| {
            let raw = crate::html::css_urls(src).into_iter().next()?.value.trim();
            (!raw.is_empty() && !crate::html::is_external(raw)).then(|| crate::epubzip::resolve_link(base, raw).0)
        });
        if let (Some(family), Some(path)) = (family, url) {
            let bold = get("font-weight").is_some_and(|w| matches!(w.trim(), "bold" | "bolder") || w.trim().parse::<u32>().is_ok_and(|n| n >= 600));
            let italic = get("font-style").is_some_and(|v| matches!(v.trim(), "italic" | "oblique"));
            out.push(FontFace { family, path, bold, italic });
        }
    }
    out
}

// ---------------------------------------------------------------- 值

/// 长度。`Em` 相对元素自己的字号。
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Len {
    Em(f64),
    Percent(f64),
    Pt(f64),
}

/// 切出开头的数字和后面的单位（`1.5em` → (1.5, "em")）；数字解析不了返回 `None`。
fn split_number(v: &str) -> Option<(f64, &str)> {
    let end = v.find(|c: char| !(c.is_ascii_digit() || c == '.' || c == '-' || c == '+')).unwrap_or(v.len());
    Some((v[..end].parse().ok()?, &v[end..]))
}

pub fn parse_len(v: &str) -> Option<Len> {
    let v = v.trim().to_ascii_lowercase();
    if v == "0" || v == "auto" {
        return Some(Len::Em(0.0));
    }
    let (n, unit) = split_number(&v)?;
    match unit {
        "em" | "rem" => Some(Len::Em(n)),
        "%" => Some(Len::Percent(n)),
        "pt" => Some(Len::Pt(n)),
        "px" => Some(Len::Pt(n * 0.75)),
        "ex" => Some(Len::Em(n * 0.5)),
        "" => Some(Len::Em(n)),
        _ => None,
    }
}

/// 1em 折合多少 pt（根字号按 12pt 算：pt 写的字号、行高、字间距换成 em 用）。
pub const PT_PER_EM: f64 = 12.0;

/// 外边距、内边距、边框的 1px 折合多少 pt（Send to Kindle 的口径，见 [`parse_box_len`]）。
const PT_PER_BOX_PX: f64 = 0.45;

/// 外边距、内边距的长度：同 [`parse_len`]，但 px 按 1px＝0.45pt 换算（Send to Kindle 的口径，和边框宽度一样：《绍宋》
/// `margin:-10px` 写成 -0.3125lh、`margin-top:-12.5px` 在半号字上写成 -1.172lh；以前按 CSS 的 1px＝0.75pt，大了 2/3）。字号不走这里。
pub fn parse_box_len(v: &str) -> Option<Len> {
    let t = v.trim().to_ascii_lowercase();
    match t.strip_suffix("px").and_then(|n| n.trim().parse::<f64>().ok()) {
        Some(n) => Some(Len::Pt(n * PT_PER_BOX_PX)),
        None => parse_len(&t),
    }
}

pub use crate::color::{contrast, ensure_contrast, luminance, over_white, parse_color};

/// 元素的计算值（只留 KFX 用得上的）。字号 `font_size` 是相对根字号的倍数。
#[derive(Clone, Debug, PartialEq)]
pub struct Computed {
    pub font_family: Option<String>,
    /// `font-family` 第一个以外的备选（小写、逗号连接，Send to Kindle 照写整串：`default,st,宋体,zw,sans-serif`）。
    pub font_fallbacks: Option<String>,
    pub font_size: f64,
    pub bold: bool,
    /// 字重 600（Send to Kindle 写成半粗 `$360`，《绍宋》`p.ganyan1`）。`bold` 同时为真（挑字体文件时当粗体）。
    pub semibold: bool,
    /// `<b>`/`<strong>` 自己、样式表没写字重：HTML 缺省的 `bolder`（Send to Kindle 写 `$362`，不继承）。
    pub bolder: bool,
    /// 自己或祖先的样式表写了 `font-weight`（Send to Kindle 照写，normal 也写 `$350`：《罗杰疑案》`font-weight:normal` 的 h1）。
    pub weight_declared: bool,
    pub italic: bool,
    pub text_align: Option<String>,
    pub text_indent: Option<Len>,
    /// 行高，单位：元素字号的倍数；`None`＝normal。
    pub line_height: Option<f64>,
    /// 写成长度（`%`、em、px、pt）的行高按 CSS 算成绝对值往下继承（根 em）；没有数字倍数那样跟着子元素字号变
    /// （《疯探》body `line-height:130%`、作者行 `font-size:1.2em`，Send to Kindle 写 0.833lh）。
    pub line_height_abs: Option<f64>,
    pub color: Option<u32>,
    pub lang: Option<String>,
    pub superscript: bool,
    pub subscript: bool,
    /// 下划线、删除线、上划线（CSS 里不继承，但画在子元素上，按继承处理）。
    pub decoration: [bool; 3],
    pub small_caps: bool,
    /// 字间距，单位：元素字号的倍数。
    pub letter_spacing: Option<f64>,
    /// `white-space: pre`（`<pre>`）：空格、换行原样保留。
    pub pre: bool,
    /// `white-space: nowrap`：不自动换行（Amazon 写成 `$45: true`，《绍宋》卷首语、信件里居中的诗句）。
    pub nowrap: bool,
    /// `word-break: break-all`（Send to Kindle 写成 `$569: $570`，《绍宋》全书 `p{word-break:break-all}`）。
    pub break_all: bool,
    /// 自己或祖先有背景色、背景图（Send to Kindle 只在没有背景时省掉近黑的文字颜色，见 KFX 写出器 `crates/kfx/src/write/style.rs` 的 `text_props`）。
    pub on_background: bool,
    /// 自己或祖先有背景图：只有背景图、没有背景色的地方不按对比度调文字颜色（Send to Kindle 同样：《雪国》扉页背景图上的白字照写）。
    pub on_image: bool,
    /// 自己或祖先（含 body）有左右外边距、内边距或边框：Send to Kindle 把首行缩进写成百分比（见 KFX 写出器 `crates/kfx/src/write/style.rs` 的 `block_props`）。
    pub in_hbox: bool,
    /// `list-style-type`（`None`＝按标签缺省）、`list-style-position: inside`。
    pub list_style: Option<String>,
    pub list_inside: bool,
    /// 表格：`border-collapse: collapse`、`border-spacing`（这两个 CSS 里是继承的）。
    pub border_collapse: bool,
    pub border_spacing: Option<Len>,
    // 不继承的
    pub display: Option<String>,
    pub margin: [Option<Len>; 4],
    /// 外边距写的是 `auto`（上、右、下、左）：有宽度的块按左右 auto 对齐（Send to Kindle 写 `$580`）。
    pub margin_auto: [bool; 4],
    pub padding: [Option<Len>; 4],
    pub background: Option<u32>,
    /// 文字底下实际的背景色（自己或最近的祖先的，不透明；`None`＝白页面）：Send to Kindle 按它保证文字对比度。
    pub backdrop: Option<u32>,
    /// `box-shadow`、`text-shadow`（第一个；x、y、模糊、颜色）。Send to Kindle 写 `$496`、`$497`。
    pub box_shadow: Option<Shadow>,
    pub text_shadow: Option<Shadow>,
    /// 四边边框（上、右、下、左）。
    pub border: [Option<Border>; 4],
    /// 圆角：左上、右上、右下、左下。
    pub radius: [Option<Len>; 4],
    pub width: Option<Len>,
    /// `min-height`（Send to Kindle 写 `$62`，《恶女的告白》`min-height:2em`）。
    pub min_height: Option<Len>,
    /// 单元格的 `vertical-align`。
    pub valign: Option<String>,
    /// 背景图（书内路径）、不重复、固定、位置 (x, y)、尺寸 (宽, 高)。
    pub bg_image: Option<String>,
    pub bg_no_repeat: bool,
    pub bg_fixed: bool,
    pub bg_position: [Option<Len>; 2],
    pub bg_size: [Option<Len>; 2],
    /// `background-size: cover`（`bg_size` 里和 `100% 100%` 写法一样，这里另记）：页面一级的背景，Send to Kindle 让它铺满一页。
    pub bg_cover: bool,
}

/// 阴影：x、y 偏移，模糊半径，颜色（没写＝`None`）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Shadow {
    pub x: Len,
    pub y: Len,
    pub blur: Len,
    pub color: Option<u32>,
}

/// `box-shadow`/`text-shadow` 的第一个阴影（`inset`、`none` 不收）。长度的 px 按 1px＝0.45pt。
pub fn parse_shadow(v: &str) -> Option<Shadow> {
    let first = split_top(v, ',').into_iter().next()?.trim().to_ascii_lowercase();
    if first.is_empty() || first == "none" || first.contains("inset") {
        return None;
    }
    let mut lens = Vec::new();
    let mut color = None;
    for tok in tokens(&first).0 {
        match parse_box_len(tok) {
            Some(l) if tok.starts_with(|c: char| c.is_ascii_digit() || c == '-' || c == '.' || c == '+') => lens.push(l),
            _ => color = parse_color(tok).or(color),
        }
    }
    (lens.len() >= 2).then(|| Shadow { x: lens[0], y: lens[1], blur: lens.get(2).copied().unwrap_or(Len::Pt(0.0)), color })
}

/// 一条边：样式（`solid` 等，不含 none）、宽度、颜色（没写＝当前颜色）。
#[derive(Clone, Debug, PartialEq)]
pub struct Border {
    pub style: String,
    pub width: BorderWidth,
    pub color: Option<u32>,
}

/// 边框宽度：pt 或 em（样本里 px 一律按 1px＝0.45pt 换算，thin/medium/thick＝1/3/5px）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum BorderWidth {
    Pt(f64),
    Em(f64),
}

pub fn parse_border_width(v: &str) -> Option<BorderWidth> {
    let v = v.trim().to_ascii_lowercase();
    match v.as_str() {
        "thin" => return Some(BorderWidth::Pt(0.45)),
        "medium" => return Some(BorderWidth::Pt(1.35)),
        "thick" => return Some(BorderWidth::Pt(2.25)),
        _ => {}
    }
    if let Some(n) = v.strip_suffix("px") {
        return n.trim().parse::<f64>().ok().map(|n| BorderWidth::Pt(n * PT_PER_BOX_PX));
    }
    match parse_len(&v)? {
        Len::Em(n) => Some(BorderWidth::Em(n)),
        Len::Pt(n) => Some(BorderWidth::Pt(n)),
        Len::Percent(_) => None,
    }
}

impl Computed {
    pub fn has_border(&self) -> bool {
        self.border.iter().any(|b| b.as_ref().is_some_and(|b| b.width != BorderWidth::Pt(0.0) && b.width != BorderWidth::Em(0.0)))
    }
}

impl Computed {
    pub fn root() -> Self {
        Computed {
            font_family: None,
            font_fallbacks: None,
            font_size: 1.0,
            bold: false,
            semibold: false,
            bolder: false,
            weight_declared: false,
            italic: false,
            text_align: None,
            text_indent: None,
            line_height: None,
            line_height_abs: None,
            color: None,
            lang: None,
            superscript: false,
            subscript: false,
            decoration: [false; 3],
            small_caps: false,
            letter_spacing: None,
            pre: false,
            nowrap: false,
            break_all: false,
            on_background: false,
            on_image: false,
            in_hbox: false,
            list_style: None,
            list_inside: false,
            border_collapse: false,
            border_spacing: None,
            display: None,
            margin: [None; 4],
            margin_auto: [false; 4],
            padding: [None; 4],
            background: None,
            backdrop: None,
            box_shadow: None,
            text_shadow: None,
            border: [None, None, None, None],
            radius: [None; 4],
            width: None,
            min_height: None,
            valign: None,
            bg_image: None,
            bg_no_repeat: false,
            bg_fixed: false,
            bg_position: [None; 2],
            bg_size: [None; 2],
            bg_cover: false,
        }
    }

    fn reset_box(&self) -> Computed {
        Computed {
            display: None,
            margin: [None; 4],
            margin_auto: [false; 4],
            padding: [None; 4],
            background: None,
            box_shadow: None,
            border: [None, None, None, None],
            radius: [None; 4],
            width: None,
            min_height: None,
            valign: None,
            bg_image: None,
            bg_no_repeat: false,
            bg_fixed: false,
            bg_position: [None; 2],
            bg_size: [None; 2],
            bg_cover: false,
            bolder: false,
            ..self.clone()
        }
    }

    /// 只留继承的属性（匿名块用：外边距、内边距、背景属于包着它的元素）。
    pub fn inherited(&self) -> Computed {
        self.reset_box()
    }

    /// 由父元素的计算值和本元素的声明算出本元素的计算值。
    pub fn derive(parent: &Computed, decls: &HashMap<String, String>, tag: &str) -> Computed {
        let mut c = parent.reset_box();
        c.on_background = parent.on_background || parent.background.is_some() || parent.bg_image.is_some();
        c.on_image = parent.on_image || parent.bg_image.is_some();
        // 各组属性按固定顺序算（后面的组要用前面算好的值：字号先于行高、字间距的换算，外边距在标签缺省值之后覆盖）。
        Computed::tag_defaults(&mut c, parent, tag);
        Computed::font_props(&mut c, parent, decls);
        Computed::text_layout_props(&mut c, parent, decls);
        Computed::text_style_props(&mut c, decls, tag);
        Computed::list_table_props(&mut c, decls);
        Computed::box_props(&mut c, parent, decls);
        Computed::background_props(&mut c, parent, decls);
        c
    }

    /// 标签的缺省样式（UA 样式表）：粗体、斜体、标题字号与外边距、段落等块的上下外边距、引文的左右缩进。
    /// 要在读声明之前：写了的声明覆盖这些缺省值。
    fn tag_defaults(c: &mut Computed, parent: &Computed, tag: &str) {
        // 标签的缺省样式（阅读器的 UA 样式表里有的）。
        match tag {
            "b" | "strong" => {
                c.bold = true;
                c.semibold = false;
                c.bolder = true;
            }
            "th" => {
                c.bold = true;
                c.semibold = false;
                c.text_align = Some("center".into());
            }
            "caption" => c.text_align = Some("center".into()),
            // 标题：粗体、字号、上下外边距（HTML 规范的 UA 样式表；Send to Kindle 同样照它排，2026-10-08 对照《绍宋》）。
            // 以前只给粗体：没写样式的 `<h2>1</h2>` 在 Kindle 上和正文一样大（《绝叫》的数字章名）。
            "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
                c.bold = true;
                c.semibold = false;
                // 字号、外边距和只修复的文字书写进书里的 `eink-ua.css` 是同一张表
                let (_, size, margin) = crate::uastyle::HEADINGS.into_iter().find(|h| h.0 == tag).unwrap_or(crate::uastyle::HEADINGS[5]);
                c.font_size = parent.font_size * size;
                c.margin[0] = Some(Len::Em(margin));
                c.margin[2] = Some(Len::Em(margin));
            }
            "i" | "em" | "cite" | "var" | "dfn" => c.italic = true,
            "sup" => c.superscript = true,
            "sub" => c.subscript = true,
            "u" | "ins" => c.decoration[0] = true,
            "s" | "strike" | "del" => c.decoration[1] = true,
            "pre" => {
                c.pre = true;
                if c.font_family.is_none() {
                    c.font_family = Some("monospace".into());
                }
            }
            _ => {}
        }
        // 段落、引文、图、预排版的上下外边距（UA 样式表）：Send to Kindle 给没写外边距的 `<p>` 上下各 1em
        // （相邻的折叠成一个，《绍宋》正文段与段之间 0.8333lh）；以前不给，段落挤在一起。哪些标签、多少用 `crate::uastyle` 的表
        // （只修复的文字书写进书里的 `eink-ua.css` 是同一张）。
        use crate::uastyle::{BLOCK_MARGIN_TAGS, INDENTED_BLOCK_TAGS, INDENT_PX};
        let indented = INDENTED_BLOCK_TAGS.contains(&tag);
        if indented || BLOCK_MARGIN_TAGS.contains(&tag) {
            c.margin[0] = Some(Len::Em(1.0));
            c.margin[2] = Some(Len::Em(1.0));
        }
        if indented {
            // 左右 40px，同样按 1px＝0.45pt（＝1.5em，和列表缩进一样）
            let side = Some(Len::Pt(f64::from(INDENT_PX) * PT_PER_BOX_PX));
            c.margin[1] = side;
            c.margin[3] = side;
        }
    }

    /// 字体：字号、字体族、字重、斜体。字号最先算——后面行高、字间距的长度换算都按本元素字号。
    fn font_props(c: &mut Computed, parent: &Computed, decls: &HashMap<String, String>) {
        let get = |k: &str| decls.get(k).map(String::as_str);
        if let Some(v) = get("font-size") {
            let v = v.trim().to_ascii_lowercase();
            c.font_size = match v.as_str() {
                "xx-small" => 0.6,
                "x-small" => 0.75,
                "small" | "smaller" => 0.89 * if v == "smaller" { parent.font_size } else { 1.0 },
                "medium" => 1.0,
                "large" | "larger" => 1.2 * if v == "larger" { parent.font_size } else { 1.0 },
                "x-large" => 1.5,
                "xx-large" => 2.0,
                _ => match parse_len(&v) {
                    Some(Len::Em(n)) if v.ends_with("rem") => n,
                    Some(Len::Em(n)) => parent.font_size * n,
                    Some(Len::Percent(n)) => parent.font_size * n / 100.0,
                    Some(Len::Pt(n)) => n / PT_PER_EM,
                    None => parent.font_size,
                },
            };
            // 负的字号不合法（CSS 里这条声明作废、照父元素）；以前照算出负字号，换算出的长度跟着变号。
            if !(c.font_size >= 0.0 && c.font_size.is_finite()) {
                c.font_size = parent.font_size;
            }
        }
        if let Some(v) = get("font-family") {
            let names: Vec<String> = split_top(v, ',').into_iter().map(|n| n.trim().trim_matches(['"', '\'']).trim().to_string()).filter(|n| !n.is_empty()).collect();
            if names.first().is_some_and(|f| f != "inherit") {
                c.font_family = Some(names[0].clone());
                c.font_fallbacks = (names.len() > 1).then(|| names[1..].join(",").to_lowercase());
            }
        }
        if let Some(v) = get("font-weight") {
            let v = v.trim().to_ascii_lowercase();
            c.bold = match v.as_str() {
                "bold" | "bolder" => true,
                "normal" | "lighter" => false,
                n => n.parse::<u32>().map(|n| n >= 600).unwrap_or(c.bold),
            };
            c.semibold = v == "600";
            c.bolder = false;
            c.weight_declared = true;
        }
        if let Some(v) = get("font-style") {
            c.italic = matches!(v.trim().to_ascii_lowercase().as_str(), "italic" | "oblique");
        }
    }

    /// 段落排版：对齐、首行缩进、行高（长度按上面算好的本元素字号换算；没写行高的继承父元素的绝对行高）。
    fn text_layout_props(c: &mut Computed, parent: &Computed, decls: &HashMap<String, String>) {
        let get = |k: &str| decls.get(k).map(String::as_str);
        if let Some(v) = get("text-align") {
            let v = v.trim().to_ascii_lowercase();
            if ["left", "right", "center", "justify", "start", "end"].contains(&v.as_str()) {
                c.text_align = Some(v);
            }
        }
        if let Some(v) = get("text-indent") {
            c.text_indent = parse_len(v);
        }
        if let Some(v) = get("line-height") {
            let v = v.trim().to_ascii_lowercase();
            let unitless = v.parse::<f64>().is_ok();
            c.line_height = if v == "normal" {
                None
            } else {
                match parse_len(&v) {
                    // 无单位的数字是倍数；em 相对本元素字号。
                    Some(Len::Em(n)) => Some(n),
                    Some(Len::Percent(n)) => Some(n / 100.0),
                    Some(Len::Pt(n)) => Some(n / PT_PER_EM / c.font_size),
                    None => c.line_height,
                }
            };
            // 数字倍数照倍数继承，长度（%、em、px、pt）算成绝对值继承（CSS 的规矩）
            c.line_height_abs = if unitless || v == "normal" { None } else { c.line_height.map(|l| l * c.font_size) };
        } else if let Some(a) = parent.line_height_abs {
            c.line_height = Some(a / c.font_size);
        }
    }

    /// 文字修饰：颜色、上下标（表格单元格里是纵向对齐）、装饰线、小型大写、字间距、空白与断行。
    fn text_style_props(c: &mut Computed, decls: &HashMap<String, String>, tag: &str) {
        let get = |k: &str| decls.get(k).map(String::as_str);
        if let Some(v) = get("color") {
            if let Some(col) = parse_color(v) {
                c.color = Some(col);
            }
        }
        if let Some(v) = get("vertical-align") {
            let v = v.trim().to_ascii_lowercase();
            if matches!(tag, "td" | "th" | "tr") {
                c.valign = Some(v);
            } else {
                c.superscript = matches!(v.as_str(), "super" | "top" | "text-top");
                c.subscript = v == "sub";
            }
        }
        if let Some(v) = get("text-decoration") {
            if v.contains("none") {
                c.decoration = [false; 3];
            }
            for (i, k) in ["underline", "line-through", "overline"].iter().enumerate() {
                if v.contains(k) {
                    c.decoration[i] = true;
                }
            }
        }
        if let Some(v) = get("font-variant").or_else(|| get("font-variant-caps")) {
            c.small_caps = v.contains("small-caps");
        }
        if let Some(v) = get("letter-spacing") {
            c.letter_spacing = match parse_len(v) {
                Some(Len::Em(n)) if v.trim() != "normal" => Some(n),
                Some(Len::Pt(n)) => Some(n / PT_PER_EM / c.font_size),
                _ => None,
            };
        }
        if let Some(v) = get("white-space") {
            let v = v.trim().to_ascii_lowercase();
            c.pre = matches!(v.as_str(), "pre" | "pre-wrap" | "break-spaces");
            c.nowrap = matches!(v.as_str(), "nowrap" | "pre");
        }
        if let Some(v) = get("word-break") {
            c.break_all = v.trim().eq_ignore_ascii_case("break-all");
        }
    }

    /// 列表与表格：列表符号与位置、边框合并与间距。
    fn list_table_props(c: &mut Computed, decls: &HashMap<String, String>) {
        let get = |k: &str| decls.get(k).map(String::as_str);
        if let Some(v) = get("list-style-type") {
            c.list_style = Some(v.trim().to_ascii_lowercase());
        }
        if let Some(v) = get("list-style-position") {
            c.list_inside = v.trim().eq_ignore_ascii_case("inside");
        }
        if let Some(v) = get("border-collapse") {
            c.border_collapse = v.trim().eq_ignore_ascii_case("collapse");
        }
        if let Some(v) = get("border-spacing") {
            c.border_spacing = v.split_whitespace().next().and_then(parse_len);
        }
    }

    /// 盒模型：边框、圆角、宽高、display、外边距（写了的覆盖标签缺省值）、内边距，以及是否在左右有框的盒子里（`in_hbox`）。
    fn box_props(c: &mut Computed, parent: &Computed, decls: &HashMap<String, String>) {
        let get = |k: &str| decls.get(k).map(String::as_str);
        // 属性名写成常量（每个元素都要查，逐个 format! 很费）；四边顺序：上、右、下、左。
        const BORDER: [[&str; 3]; 4] = [
            ["border-top-style", "border-top-width", "border-top-color"],
            ["border-right-style", "border-right-width", "border-right-color"],
            ["border-bottom-style", "border-bottom-width", "border-bottom-color"],
            ["border-left-style", "border-left-width", "border-left-color"],
        ];
        const RADIUS: [&str; 4] = ["border-top-left-radius", "border-top-right-radius", "border-bottom-right-radius", "border-bottom-left-radius"];
        const MARGIN: [&str; 4] = ["margin-top", "margin-right", "margin-bottom", "margin-left"];
        const PADDING: [&str; 4] = ["padding-top", "padding-right", "padding-bottom", "padding-left"];
        for (i, [style, width, color]) in BORDER.into_iter().enumerate() {
            let style = get(style).map(|v| v.trim().to_ascii_lowercase());
            c.border[i] = match style {
                Some(st) if st != "none" && st != "hidden" && BORDER_STYLES.contains(&st.as_str()) => Some(Border {
                    style: st,
                    width: get(width).and_then(parse_border_width).unwrap_or(BorderWidth::Pt(1.35)),
                    // 全透明的照原样（写出时写「透明」：《绍宋》信件框是 `border: 35px solid rgba(0,0,0,0)` 加 border-image，
                    // 以前强制不透明，Kindle 上画成 15.75pt 的黑框）；半透明的照旧按不透明写。
                    color: get(color).and_then(parse_color).map(|c| if c >> 24 == 0 { c } else { c | 0xFF00_0000 }),
                }),
                _ => None,
            };
        }
        for (i, corner) in RADIUS.into_iter().enumerate() {
            c.radius[i] = get(corner).and_then(parse_len).filter(|l| !matches!(l, Len::Em(n) if *n == 0.0));
        }
        c.width = get("width").and_then(parse_len).filter(|l| !matches!(l, Len::Em(n) if *n == 0.0));
        // `height` 也算（Send to Kindle 同样写成 `$62`：《消失的爱人》扉页 `div.shuming{height:6em}`），写了 `min-height` 的以它为准
        c.min_height = get("min-height").or_else(|| get("height")).and_then(parse_len).filter(|l| !matches!(l, Len::Em(n) | Len::Pt(n) | Len::Percent(n) if *n == 0.0));
        c.display = get("display").map(|v| v.trim().to_ascii_lowercase());
        for i in 0..4 {
            // 写了的才覆盖标签的缺省外边距（上面的 UA 样式）；`auto` 这类算不出长度的当 0
            if let Some(v) = get(MARGIN[i]) {
                c.margin[i] = parse_box_len(v);
                c.margin_auto[i] = v.trim().eq_ignore_ascii_case("auto");
            }
            c.padding[i] = get(PADDING[i]).and_then(parse_box_len);
        }
        let nonzero = |l: &Option<Len>| l.is_some_and(|l| !matches!(l, Len::Em(n) | Len::Pt(n) | Len::Percent(n) if n == 0.0));
        c.in_hbox = parent.in_hbox || [1, 3].iter().any(|&i| nonzero(&c.margin[i]) || nonzero(&c.padding[i]) || c.border[i].is_some());
    }

    /// 背景与阴影：背景色（及它垫在白底上的颜色 `backdrop`，往下继承）、阴影、背景图及其各项属性。
    fn background_props(c: &mut Computed, parent: &Computed, decls: &HashMap<String, String>) {
        let get = |k: &str| decls.get(k).map(String::as_str);
        // 不透明的纯白背景不算（Send to Kindle 同样：《疯探》`background-color:#ffffff` 的简介、目录页不出容器）
        c.background = get("background-color").and_then(parse_color).filter(|c| c >> 24 != 0 && *c != 0xFFFF_FFFF);
        c.backdrop = c.background.map(over_white).or(parent.backdrop);
        c.box_shadow = get("box-shadow").and_then(parse_shadow);
        if let Some(v) = get("text-shadow") {
            c.text_shadow = parse_shadow(v);
        }
        c.bg_image = get("background-image").and_then(leading_url).map(str::to_string);
        c.bg_no_repeat = get("background-repeat").is_some_and(|v| v.trim() == "no-repeat");
        c.bg_fixed = get("background-attachment").is_some_and(|v| v.trim() == "fixed");
        c.bg_position = get("background-position").map(|v| parse_bg_position(&v.to_ascii_lowercase())).unwrap_or([None; 2]);
        c.bg_size = get("background-size").map(|v| parse_bg_size(&v.to_ascii_lowercase())).unwrap_or([None; 2]);
        c.bg_cover = get("background-size").is_some_and(|v| v.trim().eq_ignore_ascii_case("cover"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `@media` 套得再深也不栈溢出（以前两万层整个进程中止）；超过上限的那几层不收，浅的照收。
    #[test]
    fn deeply_nested_media_does_not_overflow() {
        let nest = |n: usize| format!("{}p{{color:red}}{}", "@media all{".repeat(n), "}".repeat(n));
        assert_eq!(Rules::parse(&nest(3), "", None).0.len(), 1);
        assert_eq!(Rules::parse(&nest(MAX_MEDIA_NESTING), "", None).0.len(), 1);
        assert_eq!(Rules::parse(&nest(MAX_MEDIA_NESTING + 1), "", None).0.len(), 0);
        let deep = format!("{}q{{color:red}}", nest(20000));
        let r = Rules::parse(&deep, "", None);
        assert_eq!(r.0.len(), 1, "后面的规则照收");
    }

    #[test]
    fn decls_and_shorthand() {
        let d = parse_decls("margin: 1em 0 2em; color: red !important; background: url(x.png) #fff no-repeat");
        let get = |p: &str| d.iter().find(|x| x.prop == p).map(|x| x.value.as_str());
        assert_eq!(get("margin-top"), Some("1em"));
        assert_eq!(get("margin-right"), Some("0"));
        assert_eq!(get("margin-bottom"), Some("2em"));
        assert_eq!(get("margin-left"), Some("0"));
        assert_eq!(get("background-color"), Some("#fff"));
        assert!(d.iter().any(|x| x.prop == "color" && x.important));
        // 和优化器同一个切法：引号里的 `;`、转义的引号、`url(data:…;base64,…)` 不切；坏引号不吞掉后面的声明；`*color` 不收
        let d = parse_decls(r#"font-family: "a;\"b"; background-image: url(data:image/png;base64,AA==); *color: blue; color: red"#);
        let got: Vec<(&str, &str)> = d.iter().map(|x| (x.prop.as_str(), x.value.as_str())).collect();
        assert_eq!(got, [("font-family", r#""a;\"b""#), ("background-image", "url(data:image/png;base64,AA==)"), ("color", "red")]);
        let d = parse_decls("font-family: Georgia'; color: red");
        assert!(d.iter().any(|x| x.prop == "color" && x.value == "red"), "{d:?}");
    }

    #[test]
    fn prefilter_is_only_a_necessary_condition() {
        assert_eq!(Need::of("div > p.a.B#x"), Some(Need { tag: Some("p".into()), classes: vec!["a".into(), "B".into()], ids: vec!["x".into()] }));
        assert_eq!(Need::of(".a"), Some(Need { tag: None, classes: vec!["a".into()], ids: vec![] }));
        for s in ["p:first-child", "a[href]", "*", "p *", "svg|a", r"p.a\:b", "p > ", "p.", "p..a"] {
            assert_eq!(Need::of(s), None, "{s}");
        }
        // 用预筛和不用预筛，每个元素层叠出来的结果一样
        let css = "p{color:red} p.a{color:blue} .A{text-indent:1em} div p.b{margin-top:1em} #X{font-size:2em} h1+p{color:green} span.c.d{font-weight:bold} li > span{color:gray} p:first-child{margin-left:1em}";
        let html = scraper::Html::parse_document(r#"<html><body><div><p class="a b" id="x">1</p><h1>t</h1><p class="A">2<span class="d c">3</span></p><ul><li><span>4</span></li></ul></div></body></html>"#);
        let mut with = Sheet::default();
        with.add(css, 0);
        let mut without = Sheet::default();
        without.add(css, 0);
        for part in &mut without.parts {
            for r in &mut Arc::get_mut(&mut part.0).unwrap().0 {
                r.need = None;
            }
        }
        let mut n = 0;
        for el in html.root_element().descendants().filter_map(ElementRef::wrap) {
            assert_eq!(with.cascade(&el, ""), without.cascade(&el, ""), "{}", el.html());
            n += 1;
        }
        assert!(n > 8);
    }

    #[test]
    fn specificity_counts() {
        assert_eq!(specificity("p"), (0, 0, 1));
        assert_eq!(specificity("p.a"), (0, 1, 1));
        assert_eq!(specificity("#x .a span"), (1, 1, 1));
        assert_eq!(specificity("div > p:first-child"), (0, 1, 2));
        assert_eq!(specificity("*"), (0, 0, 0));
        // 括号里的字不计成标签；:not/:is 取参数里最高的，:where 算 0
        assert_eq!(specificity("li:nth-child(2n+1)"), (0, 1, 1));
        assert_eq!(specificity("p:not(.a)"), (0, 1, 1));
        assert_eq!(specificity("p:not(#x, span)"), (1, 0, 1));
        assert_eq!(specificity("p:where(.a #b)"), (0, 0, 1));
        assert_eq!(specificity("p:is(div p, .a)"), (0, 1, 1));
        assert_eq!(specificity("p::first-line"), (0, 0, 2));
        assert_eq!(specificity("p:before"), (0, 0, 2));
        assert_eq!(specificity("a[href='x y']:lang(zh-Hant)"), (0, 2, 1));
        assert_eq!(specificity(r"p.a\.b"), (0, 1, 1));
    }

    #[test]
    fn cascade_order_and_media() {
        let mut s = Sheet::default();
        s.add("@charset \"utf-8\"; p { color: red } .a { color: blue } p { color: green } @media amzn-mobi { p.a { color: black } } @font-face { font-family: x }", 0);
        let html = scraper::Html::parse_document("<html><body><p class=\"a\" style=\"text-indent:2em\">x</p></body></html>");
        let p = html.select(&Selector::parse("p").unwrap()).next().unwrap();
        let d = s.cascade(&p, "");
        assert_eq!(d.get("color").map(String::as_str), Some("blue"));
        assert_eq!(d.get("text-indent").map(String::as_str), Some("2em"));
    }

    #[test]
    fn font_shorthand_expands_and_resets() {
        assert_eq!(
            parse_font_shorthand("normal small-caps 700 condensed larger / 2em  'A B', serif"),
            Some(FontShorthand { style: None, variant: Some("small-caps"), weight: Some("700"), stretch: Some("condensed"), size: "larger", line_height: Some("2em"), family: "'A B', serif" })
        );
        assert_eq!(parse_font_shorthand("bold bold 1em x"), None);
        // 层叠里展开成分项：没写的重置成 normal（盖掉前面规则的粗体、行高），后面的分项照常覆盖简写
        let mut s = Sheet::default();
        s.add("p { font-weight: bold; line-height: 2 } p { font: italic 15pt/1.5 \"宋体\", serif } .a { font-family: 楷体 } .b { font: caption }", 0);
        let html = scraper::Html::parse_document("<html><body><p class=\"a b\">x</p></body></html>");
        let p = html.select(&Selector::parse("p").unwrap()).next().unwrap();
        let d = s.cascade(&p, "");
        let get = |k: &str| d.get(k).map(String::as_str);
        assert_eq!((get("font-style"), get("font-weight"), get("font-size"), get("line-height")), (Some("italic"), Some("normal"), Some("15pt"), Some("1.5")));
        assert_eq!(get("font-family"), Some("楷体"), "后面的分项覆盖简写；认不出的简写（caption）整条不认");
        assert_eq!(Computed::derive(&Computed::root(), &d, "p").font_size, 1.25);
    }

    #[test]
    fn media_queries_by_env() {
        let env = MediaEnv { width: 1104.0, height: 1546.0, device_width: 1272.0, device_height: 1696.0, color: false };
        let ok = |q: &str| media_ok(q, Some(&env));
        for q in ["", "all", "screen", "amzn-kf8", "only screen", "not amzn-mobi", "not print", "print, screen", "SCREEN"] {
            assert!(ok(q), "{q}");
        }
        for q in ["print", "amzn-mobi", "speech", "not screen", "screen and", "screen (min-width: 1px)", "and (color)"] {
            assert!(!ok(q), "{q}");
        }
        // 特性按阅读范围、屏幕求值
        for q in ["screen and (min-width: 600px)", "(max-width: 1104px)", "(min-height: 1500px) and (orientation: portrait)", "(min-device-width: 1272px)", "(max-aspect-ratio: 3/4)", "(monochrome)", "not (color)", "(min-width: 10in)", "(color: 0)", "not screen and (max-width: 480px)"] {
            assert!(ok(q), "{q}");
        }
        for q in ["screen and (max-width: 480px)", "(min-width: 1105px)", "(orientation: landscape)", "(color)"] {
            assert!(!ok(q), "{q}");
        }
        // 求值不了的不收，带 not 也不收
        for q in ["(min-width: 30em)", "not screen and (min-width: 30em)", "(min-resolution: 2dppx)", "(width >= 600px)", "(min-monochrome: 4)", "(min-width: 600px) or (color)"] {
            assert!(!ok(q), "{q}");
        }
        // 没有环境：只看媒体类型
        assert!(media_ok("screen", None) && !media_ok("screen and (min-width: 1px)", None));
        // 样式表里的 @media
        let mut with = Sheet::default();
        with.add_rules(Rules::parse("p{color:red} @media screen and (max-width: 480px){p{color:blue}} @media (min-width: 600px){p{text-indent:2em}}", "", Some(&env)), 0);
        let html = scraper::Html::parse_document("<p>x</p>");
        let p = html.select(&Selector::parse("p").unwrap()).next().unwrap();
        let d = with.cascade(&p, "");
        assert_eq!((d.get("color").map(String::as_str), d.get("text-indent").map(String::as_str)), (Some("red"), Some("2em")));
    }

    /// 落单的引号按 CSS 规范在换行处结束（坏字符串），后面的规则照收；以前 `{}` 配不上、后面全废。到文末没配上的块在文末收尾。
    #[test]
    fn lone_quote_ends_at_newline() {
        let css = "p { font-family: \"Georgia;\n color: gray }\nh1 { color: red }\n.a { text-indent: 2em }\n.b { content: 'x\\'}'; color: blue }\n.c { color: green";
        let mut s = Sheet::default();
        s.add(css, 0);
        let html = scraper::Html::parse_document(r#"<html><body><h1 class="a">t</h1><p class="b c">x</p></body></html>"#);
        let get = |sel: &str, k: &str| {
            let el = html.select(&Selector::parse(sel).unwrap()).next().unwrap();
            s.cascade(&el, "").get(k).cloned()
        };
        assert_eq!(get("h1", "color").as_deref(), Some("red"));
        assert_eq!(get("h1", "text-indent").as_deref(), Some("2em"));
        // 引号里转义的引号、`}` 不算；没收尾的最后一块照收（后出现的 green 盖过 blue）
        assert_eq!(get("p", "color").as_deref(), Some("green"));
        // 拆分、配括号、优先级共用一个扫描器：引号里的逗号、括号不算
        assert_eq!(split_top(r#"a, "b,c", d(e,f), 'g\',h'"#, ','), ["a", r#" "b,c""#, " d(e,f)", r#" 'g\',h'"#]);
        assert_eq!(rule_spans("p{a{b}\"}\"c}x").iter().map(|r| (r.body, r.end)).collect::<Vec<_>>(), [("a{b}\"}\"c", 11)]);
        assert_eq!(specificity(r#"a[title=")"]:not(.x)"#), (0, 2, 1));
        assert!(media_ok("screen and (min-width: 1px)", Some(&MediaEnv { width: 10.0, height: 10.0, device_width: 10.0, device_height: 10.0, color: false })));
    }

    /// 规则位置：字符串、注释里的花括号不算；`@media` 里的给出并带上条件；开头的语句和注释留在 prelude 里（判断时 `rule_selector` 去掉）；
    /// `@font-face` 这类没有块的 at 规则照给；空 prelude 的块不给；到文末没收尾的在文末收尾。
    #[test]
    fn rule_spans_positions_and_parents() {
        let css = "@charset \"utf-8\"; /* { */ p { content: '}'; color: red }\n@media screen { .a { x: 1 } @supports (y) { .b { z: 2 } } }\n@font-face { font-family: f }\n}{orphan} .c { q: 1";
        let r = rule_spans(css);
        let got: Vec<(String, &str, Vec<&str>, bool)> = r.iter().map(|r| (rule_selector(r.prelude).trim().to_string(), r.body.trim(), r.parents.iter().map(|p| p.trim()).collect(), r.closed)).collect();
        assert_eq!(
            got,
            [
                ("p".to_string(), "content: '}'; color: red", vec![], true),
                (".a".to_string(), "x: 1", vec!["@media screen"], true),
                (".b".to_string(), "z: 2", vec!["@media screen", "@supports (y)"], true),
                ("@font-face".to_string(), "font-family: f", vec![], true),
                (".c".to_string(), "q: 1", vec![], false),
            ]
        );
        assert_eq!(&css[r[0].body_start..r[0].end], " content: '}'; color: red }");
        assert!(r[0].prelude.starts_with("@charset"));
        // 层叠只收条件成立的 `@media` 里的，`@supports` 里的不收；属性选择器里引号中的 `;` 不切选择器
        let mut s = Sheet::default();
        s.add("@media print { p { color: blue } } @supports (x) { p { text-indent: 3em } } [title=\"a;b\"] { text-indent: 1em }", 0);
        let html = scraper::Html::parse_document(r#"<p title="a;b">x</p>"#);
        let p = html.select(&Selector::parse("p").unwrap()).next().unwrap();
        let d = s.cascade(&p, "");
        assert_eq!((d.get("color"), d.get("text-indent").map(String::as_str)), (None, Some("1em")));
    }

    /// 去注释换成空格、字符串里的不算；切词认引号、注释当分隔；四值简写拆不清的不认。
    #[test]
    fn text_tools() {
        assert_eq!(strip_comments("a/* x */b 'c/*d*/' /* 没闭合"), "a b 'c/*d*/'  ");
        assert!(matches!(strip_comments("p{}"), std::borrow::Cow::Borrowed(_)));
        assert_eq!(tokens("1px/**/solid  \"A B\"\trgb(0, 0, 0)").0, ["1px", "solid", "\"A B\"", "rgb(0, 0, 0)"]);
        assert!(!tokens("calc(1px").1);
        assert_eq!(background_tokens("url(a/b.png) center/cover"), ["url(a/b.png)", "center", "/", "cover"]);
        assert_eq!(box_sides("calc(1em + 2px) 0"), BoxSides::Sides(["calc(1em + 2px)", "0", "calc(1em + 2px)", "0"], ""));
        assert_eq!(four_values("1px 2px 3px 4px 5px"), None);
        assert_eq!(four_values("inherit"), Some(["inherit"; 4]));
        assert_eq!(split_leading_statements("@import url('a;b.css'); /* c */ @charset \"x\";\n.x"), ("@import url('a;b.css'); /* c */ @charset \"x\";", "\n.x"));
        assert_eq!(compound_tag_classes("body..x"), ("body".to_string(), vec!["x"]));
    }

    /// `background:none`、只写颜色的 `background` 简写按 CSS 把没写的项（背景图等）重置成初始值；位置、尺寸回到不写。
    #[test]
    fn background_shorthand_resets_unset_parts() {
        let mut s = Sheet::default();
        s.add("div { background-image: url(a.png); background-repeat: no-repeat; background-position: center } div.n { background: none } div.c { background: #eee } div.u { background: url(b.png) }", 0);
        let html = scraper::Html::parse_document(r#"<html><body><div>0</div><div class="n">1</div><div class="c">2</div><div class="u">3</div></body></html>"#);
        let comps: Vec<Computed> = html
            .select(&Selector::parse("div").unwrap())
            .map(|el| Computed::derive(&Computed::root(), &s.cascade(&el, ""), "div"))
            .collect();
        assert_eq!(comps[0].bg_image.as_deref(), Some("a.png"));
        assert!(comps[0].bg_no_repeat && comps[0].bg_position[0].is_some());
        assert_eq!((comps[1].bg_image.as_deref(), comps[1].bg_no_repeat, comps[1].bg_position), (None, false, [None, None]));
        assert_eq!((comps[2].bg_image.as_deref(), comps[2].background), (None, Some(0xFFEE_EEEE)));
        assert_eq!((comps[3].bg_image.as_deref(), comps[3].bg_no_repeat, comps[3].bg_position), (Some("b.png"), false, [None, None]));
    }

    #[test]
    fn font_face_urls() {
        let f = font_faces(r#"@font-face{font-family:"宋体";src:local("st")}@font-face{font-family:"juan";src:url("../Fonts/juan.ttf")}@font-face{font-family:'b';font-weight:bold;src:local(x),url(../Fonts/b.ttf) format("truetype")}"#, "OEBPS/Styles/s.css");
        assert_eq!(f.len(), 2);
        assert_eq!(f[0], FontFace { family: "juan".into(), path: "OEBPS/Fonts/juan.ttf".into(), bold: false, italic: false });
        assert!(f[1].bold && f[1].path == "OEBPS/Fonts/b.ttf");
    }

    #[test]
    fn colors_and_lengths() {
        assert_eq!(parse_color("#01a0ea"), Some(0xFF01A0EA));
        assert_eq!(parse_color("#fff"), Some(0xFFFFFFFF));
        // #rgba、#rrggbbaa：透明度写进最高字节（同 rgba()）
        assert_eq!(parse_color("#f008"), Some(0x88FF0000));
        assert_eq!(parse_color("#01a0ea80"), Some(0x8001A0EA));
        assert_eq!(parse_color("#12345"), None);
        assert_eq!(parse_color("#+12"), None);
        // 透明度舍去小数（同 Send to Kindle：0.7 → 0xB2、0.5 → 0x7F）；0.6 × 255 的浮点误差不能舍成 152
        assert_eq!(parse_color("rgba(128, 0, 0, 0.7)"), Some(0xB2800000));
        assert_eq!(parse_color("rgba(245, 245, 220, 0.5)"), Some(0x7FF5F5DC));
        assert_eq!(parse_color("rgba(0, 0, 0, 0.6)"), Some(0x99000000));
        assert_eq!(parse_color("rgb(100%, 0%, 50%)"), Some(0xFFFF0080));
        assert_eq!(parse_color("rgba(0, 0, 0, 50%)"), Some(0x7F000000));
        assert_eq!(parse_len("1.5em"), Some(Len::Em(1.5)));
        assert_eq!(parse_len("30%"), Some(Len::Percent(30.0)));
        assert_eq!(parse_len("4px"), Some(Len::Pt(3.0)));
    }

    #[test]
    fn computed_inherits() {
        let mut d = HashMap::new();
        d.insert("font-size".to_string(), "2em".to_string());
        d.insert("margin-top".to_string(), "1em".to_string());
        let h = Computed::derive(&Computed::root(), &d, "h1");
        assert_eq!(h.font_size, 2.0);
        assert!(h.bold);
        let child = Computed::derive(&h, &HashMap::new(), "span");
        assert_eq!(child.font_size, 2.0);
        assert_eq!(child.margin[0], None);
        // 负字号作废，照父元素
        let neg = Computed::derive(&h, &HashMap::from([("font-size".to_string(), "-1em".to_string())]), "span");
        assert_eq!(neg.font_size, 2.0);
    }
}
