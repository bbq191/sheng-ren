//! 够写 KFX 用的 CSS：解析样式表、按选择器优先级层叠、算出每个元素的计算值。
//!
//! 只认 KFX 能表达的属性（字体、字号、字重、斜体、对齐、缩进、行高、边距、内边距、颜色、背景色、上下标）；
//! 别的属性忽略。选择器匹配用 scraper（html5ever DOM），优先级自己算。
//! `@media` 只收 `all`/`screen`/`amzn-kf8`，`@font-face`、`@page` 等跳过（字体先交给阅读器）。

use scraper::{ElementRef, Selector};
use std::collections::HashMap;

/// 一条声明。
#[derive(Clone, Debug, PartialEq)]
pub struct Decl {
    pub prop: String,
    pub value: String,
    pub important: bool,
}

struct Rule {
    sel: Selector,
    spec: (u32, u32, u32),
    order: usize,
    decls: Vec<Decl>,
}

#[derive(Default)]
pub struct Sheet {
    rules: Vec<Rule>,
}

fn strip_comments(css: &str) -> String {
    let mut out = String::with_capacity(css.len());
    let mut rest = css;
    while let Some(p) = rest.find("/*") {
        out.push_str(&rest[..p]);
        match rest[p + 2..].find("*/") {
            Some(e) => rest = &rest[p + 2 + e + 2..],
            None => return out,
        }
    }
    out.push_str(rest);
    out
}

/// 找和 `open`（`{` 的位置）配对的 `}`。
fn matching_brace(s: &str, open: usize) -> Option<usize> {
    let mut depth = 0usize;
    let mut quote: Option<char> = None;
    for (i, c) in s[open..].char_indices() {
        match (quote, c) {
            (Some(q), c) if c == q => quote = None,
            (Some(_), _) => {}
            (None, '"' | '\'') => quote = Some(c),
            (None, '{') => depth += 1,
            (None, '}') => {
                depth -= 1;
                if depth == 0 {
                    return Some(open + i);
                }
            }
            _ => {}
        }
    }
    None
}

/// 解析 `a: b; c: d !important`。
pub fn parse_decls(s: &str) -> Vec<Decl> {
    let mut out = Vec::new();
    for part in split_top(s, ';') {
        let Some((p, v)) = part.split_once(':') else { continue };
        let prop = p.trim().to_ascii_lowercase();
        let mut value = v.trim().to_string();
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

/// 按分隔符拆，括号和引号里的不拆。
fn split_top(s: &str, sep: char) -> Vec<&str> {
    let mut out = Vec::new();
    let (mut depth, mut quote, mut start) = (0i32, None::<char>, 0);
    for (i, c) in s.char_indices() {
        match (quote, c) {
            (Some(q), c) if c == q => quote = None,
            (Some(_), _) => {}
            (None, '"' | '\'') => quote = Some(c),
            (None, '(') => depth += 1,
            (None, ')') => depth -= 1,
            (None, c) if c == sep && depth == 0 => {
                out.push(&s[start..i]);
                start = i + c.len_utf8();
            }
            _ => {}
        }
    }
    out.push(&s[start..]);
    out
}

fn expand_shorthand(prop: &str, value: &str) -> Vec<(String, String)> {
    let four = |base: &str| -> Vec<(String, String)> {
        let v: Vec<&str> = value.split_whitespace().collect();
        let (t, r, b, l) = match v.as_slice() {
            [a] => (*a, *a, *a, *a),
            [a, b] => (*a, *b, *a, *b),
            [a, b, c] => (*a, *b, *c, *b),
            [a, b, c, d, ..] => (*a, *b, *c, *d),
            [] => return Vec::new(),
        };
        ["top", "right", "bottom", "left"].iter().zip([t, r, b, l]).map(|(side, v)| (format!("{base}-{side}"), v.to_string())).collect()
    };
    match prop {
        "margin" => four("margin"),
        "padding" => four("padding"),
        // background 简写只取颜色。
        "background" => value
            .split_whitespace()
            .find(|t| parse_color(t).is_some())
            .map(|c| vec![("background-color".to_string(), c.to_string())])
            .unwrap_or_default(),
        _ => vec![(prop.to_string(), value.to_string())],
    }
}

/// 一个选择器的优先级 (id, 类/属性/伪类, 标签)。
fn specificity(sel: &str) -> (u32, u32, u32) {
    let (mut a, mut b, mut c) = (0, 0, 0);
    let mut prev_ident = false;
    let bytes: Vec<char> = sel.chars().collect();
    let mut i = 0;
    while i < bytes.len() {
        let ch = bytes[i];
        match ch {
            '#' => {
                a += 1;
                prev_ident = true;
            }
            '.' | '[' => {
                b += 1;
                prev_ident = true;
                if ch == '[' {
                    while i < bytes.len() && bytes[i] != ']' {
                        i += 1;
                    }
                }
            }
            ':' => {
                if bytes.get(i + 1) == Some(&':') {
                    c += 1;
                    i += 1;
                } else {
                    b += 1;
                }
                prev_ident = true;
            }
            c2 if c2.is_alphanumeric() || c2 == '-' || c2 == '_' || c2 == '*' => {
                if !prev_ident && c2 != '*' {
                    c += 1;
                }
                prev_ident = true;
            }
            _ => prev_ident = false,
        }
        i += 1;
    }
    (a, b, c)
}

fn media_ok(query: &str) -> bool {
    let q = query.to_ascii_lowercase();
    if q.contains("amzn-mobi") || q.contains("print") && !q.contains("screen") {
        return false;
    }
    q.trim().is_empty() || q.contains("all") || q.contains("screen") || q.contains("amzn-kf8")
}

impl Sheet {
    /// 追加一份样式表。`order` 是全局序号起点（后出现的规则优先），返回下一个序号。
    pub fn add(&mut self, css: &str, mut order: usize) -> usize {
        let css = strip_comments(css);
        let mut rest: &str = &css;
        while let Some(open) = rest.find('{') {
            let head = rest[..open].trim();
            let Some(close) = matching_brace(rest, open) else { break };
            let body = &rest[open + 1..close];
            // 前面可能残留 `@charset …;` 之类以分号结束的 at 规则。
            let head = head.rsplit(';').next().unwrap_or("").trim();
            if let Some(at) = head.strip_prefix('@') {
                let lower = at.to_ascii_lowercase();
                if let Some(q) = lower.strip_prefix("media") {
                    if media_ok(q) {
                        order = self.add(body, order);
                    }
                }
            } else if !head.is_empty() {
                let decls = parse_decls(body);
                if !decls.is_empty() {
                    for one in split_top(head, ',') {
                        let one = one.trim();
                        if let Ok(sel) = Selector::parse(one) {
                            self.rules.push(Rule { sel, spec: specificity(one), order, decls: decls.clone() });
                            order += 1;
                        }
                    }
                }
            }
            rest = &rest[close + 1..];
        }
        order
    }

    /// 一个元素上生效的声明（按层叠排好，后面的覆盖前面的），含行内 `style`。
    pub fn cascade(&self, el: &ElementRef) -> HashMap<String, String> {
        // (!important, 优先级, 出现顺序, 声明)
        type Hit<'a> = (bool, (u32, u32, u32), usize, &'a Decl);
        let mut hits: Vec<Hit> = Vec::new();
        for r in &self.rules {
            if r.sel.matches(el) {
                for d in &r.decls {
                    hits.push((d.important, r.spec, r.order, d));
                }
            }
        }
        let inline: Vec<Decl> = el.value().attr("style").map(parse_decls).unwrap_or_default();
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

// ---------------------------------------------------------------- 值

/// 长度。`Em` 相对元素自己的字号。
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Len {
    Em(f64),
    Percent(f64),
    Pt(f64),
}

pub fn parse_len(v: &str) -> Option<Len> {
    let v = v.trim().to_ascii_lowercase();
    if v == "0" || v == "auto" {
        return Some(Len::Em(0.0));
    }
    let num_end = v.find(|c: char| !(c.is_ascii_digit() || c == '.' || c == '-' || c == '+')).unwrap_or(v.len());
    let n: f64 = v[..num_end].parse().ok()?;
    match &v[num_end..] {
        "em" | "rem" => Some(Len::Em(n)),
        "%" => Some(Len::Percent(n)),
        "pt" => Some(Len::Pt(n)),
        "px" => Some(Len::Pt(n * 0.75)),
        "ex" => Some(Len::Em(n * 0.5)),
        "" => Some(Len::Em(n)),
        _ => None,
    }
}

/// `#rgb`、`#rrggbb`、`rgb()`/`rgba()`、常见色名 → ARGB。
pub fn parse_color(v: &str) -> Option<u32> {
    let v = v.trim().to_ascii_lowercase();
    if let Some(h) = v.strip_prefix('#') {
        let n = u32::from_str_radix(h, 16).ok()?;
        return match h.len() {
            3 => {
                let (r, g, b) = ((n >> 8) & 0xF, (n >> 4) & 0xF, n & 0xF);
                Some(0xFF00_0000 | ((r * 17) << 16) | ((g * 17) << 8) | (b * 17))
            }
            6 => Some(0xFF00_0000 | n),
            _ => None,
        };
    }
    if let Some(inner) = v.strip_prefix("rgba(").or_else(|| v.strip_prefix("rgb(")).and_then(|s| s.strip_suffix(')')) {
        let p: Vec<f64> = inner.split(',').map(|x| x.trim().trim_end_matches('%').parse().unwrap_or(0.0)).collect();
        if p.len() < 3 {
            return None;
        }
        let a = p.get(3).map(|a| (a.clamp(0.0, 1.0) * 255.0).round() as u32).unwrap_or(255);
        let c = |x: f64| x.clamp(0.0, 255.0).round() as u32;
        return Some(a << 24 | c(p[0]) << 16 | c(p[1]) << 8 | c(p[2]));
    }
    let named = match v.as_str() {
        "black" => 0x000000,
        "white" => 0xFFFFFF,
        "red" => 0xFF0000,
        "green" => 0x008000,
        "blue" => 0x0000FF,
        "gray" | "grey" => 0x808080,
        "silver" => 0xC0C0C0,
        "maroon" => 0x800000,
        "navy" => 0x000080,
        _ => return None,
    };
    Some(0xFF00_0000 | named)
}

/// 元素的计算值（只留 KFX 用得上的）。字号 `font_size` 是相对根字号的倍数。
#[derive(Clone, Debug, PartialEq)]
pub struct Computed {
    pub font_family: Option<String>,
    pub font_size: f64,
    pub bold: bool,
    pub italic: bool,
    pub text_align: Option<String>,
    pub text_indent: Option<Len>,
    /// 行高，单位：元素字号的倍数；`None`＝normal。
    pub line_height: Option<f64>,
    pub color: Option<u32>,
    pub lang: Option<String>,
    pub superscript: bool,
    // 不继承的
    pub display: Option<String>,
    pub margin: [Option<Len>; 4],
    pub padding: [Option<Len>; 4],
    pub background: Option<u32>,
}

impl Computed {
    pub fn root() -> Self {
        Computed {
            font_family: None,
            font_size: 1.0,
            bold: false,
            italic: false,
            text_align: None,
            text_indent: None,
            line_height: None,
            color: None,
            lang: None,
            superscript: false,
            display: None,
            margin: [None; 4],
            padding: [None; 4],
            background: None,
        }
    }

    /// 只留继承的属性（匿名块用：外边距、内边距、背景属于包着它的元素）。
    pub fn inherited(&self) -> Computed {
        Computed { display: None, margin: [None; 4], padding: [None; 4], background: None, ..self.clone() }
    }

    /// 由父元素的计算值和本元素的声明算出本元素的计算值。
    pub fn derive(parent: &Computed, decls: &HashMap<String, String>, tag: &str) -> Computed {
        let mut c = Computed { display: None, margin: [None; 4], padding: [None; 4], background: None, ..parent.clone() };
        // 标签的缺省样式（阅读器的 UA 样式表里有的）。
        match tag {
            "b" | "strong" | "th" => c.bold = true,
            "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => c.bold = true,
            "i" | "em" | "cite" | "var" | "dfn" => c.italic = true,
            "sup" => c.superscript = true,
            _ => {}
        }
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
                    Some(Len::Pt(n)) => n / 12.0,
                    None => parent.font_size,
                },
            };
        }
        if let Some(v) = get("font-family") {
            let first = split_top(v, ',').into_iter().next().unwrap_or("").trim().trim_matches(['"', '\'']).to_string();
            if !first.is_empty() && first != "inherit" {
                c.font_family = Some(first);
            }
        }
        if let Some(v) = get("font-weight") {
            let v = v.trim().to_ascii_lowercase();
            c.bold = match v.as_str() {
                "bold" | "bolder" => true,
                "normal" | "lighter" => false,
                n => n.parse::<u32>().map(|n| n >= 600).unwrap_or(c.bold),
            };
        }
        if let Some(v) = get("font-style") {
            c.italic = matches!(v.trim().to_ascii_lowercase().as_str(), "italic" | "oblique");
        }
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
            c.line_height = if v == "normal" {
                None
            } else {
                match parse_len(&v) {
                    // 无单位的数字是倍数；em 相对本元素字号。
                    Some(Len::Em(n)) => Some(n),
                    Some(Len::Percent(n)) => Some(n / 100.0),
                    Some(Len::Pt(n)) => Some(n / 12.0 / c.font_size),
                    None => c.line_height,
                }
            };
        }
        if let Some(v) = get("color") {
            if let Some(col) = parse_color(v) {
                c.color = Some(col);
            }
        }
        if let Some(v) = get("vertical-align") {
            c.superscript = matches!(v.trim(), "super" | "top" | "text-top");
        }
        c.display = get("display").map(|v| v.trim().to_ascii_lowercase());
        for (i, side) in ["top", "right", "bottom", "left"].iter().enumerate() {
            c.margin[i] = get(&format!("margin-{side}")).and_then(parse_len);
            c.padding[i] = get(&format!("padding-{side}")).and_then(parse_len);
        }
        c.background = get("background-color").and_then(parse_color).filter(|c| c >> 24 != 0);
        c
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
    }

    #[test]
    fn specificity_counts() {
        assert_eq!(specificity("p"), (0, 0, 1));
        assert_eq!(specificity("p.a"), (0, 1, 1));
        assert_eq!(specificity("#x .a span"), (1, 1, 1));
        assert_eq!(specificity("div > p:first-child"), (0, 1, 2));
        assert_eq!(specificity("*"), (0, 0, 0));
    }

    #[test]
    fn cascade_order_and_media() {
        let mut s = Sheet::default();
        s.add("@charset \"utf-8\"; p { color: red } .a { color: blue } p { color: green } @media amzn-mobi { p.a { color: black } } @font-face { font-family: x }", 0);
        let html = scraper::Html::parse_document("<html><body><p class=\"a\" style=\"text-indent:2em\">x</p></body></html>");
        let p = html.select(&Selector::parse("p").unwrap()).next().unwrap();
        let d = s.cascade(&p);
        assert_eq!(d.get("color").map(String::as_str), Some("blue"));
        assert_eq!(d.get("text-indent").map(String::as_str), Some("2em"));
    }

    #[test]
    fn colors_and_lengths() {
        assert_eq!(parse_color("#01a0ea"), Some(0xFF01A0EA));
        assert_eq!(parse_color("#fff"), Some(0xFFFFFFFF));
        assert_eq!(parse_color("rgba(128, 0, 0, 0.7)"), Some(0xB3800000));
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
    }
}
