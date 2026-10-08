//! 够写 KFX 用的 CSS：解析样式表、按选择器优先级层叠、算出每个元素的计算值。
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

impl Rules {
    /// 解析一份样式表；`url(…)` 按样式表自己的路径 `base`（书内路径）换成书内路径（`base` 空时不换）。
    /// `@media` 块按 `media` 求值（见 [`media_ok`]）。
    pub fn parse(css: &str, base: &str, media: Option<&MediaEnv>) -> Arc<Rules> {
        let mut r = Rules::default();
        r.add(css, base, media);
        Arc::new(r)
    }

    fn add(&mut self, css: &str, base: &str, media: Option<&MediaEnv>) {
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
                    if media_ok(q, media) {
                        self.add(body, base, media);
                    }
                }
            } else if !head.is_empty() {
                let mut decls = parse_decls(body);
                if !base.is_empty() {
                    for d in decls.iter_mut().filter(|d| d.value.to_ascii_lowercase().starts_with("url(")) {
                        let inner = d.value[4..].trim_end_matches(')').trim().trim_matches(['"', '\'']);
                        d.value = format!("url({})", bookconv::epubzip::resolve_link(base, inner).0);
                    }
                }
                if !decls.is_empty() {
                    for one in split_top(head, ',') {
                        let one = one.trim();
                        if let Ok(sel) = Selector::parse(one) {
                            self.0.push(Rule { sel, need: Need::of(one), spec: specificity(one), decls: decls.clone() });
                        }
                    }
                }
            }
            rest = &rest[close + 1..];
        }
    }
}

/// 一个文档用到的样式表：各份 [`Rules`] 和它们第一条规则的全局序号（后出现的规则优先）。
#[derive(Default)]
pub struct Sheet {
    parts: Vec<(Arc<Rules>, usize)>,
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

/// 解析 `a: b; c: d !important`。切声明用 [`bookconv::html::css_decls`]（和优化器同一个：引号、括号里的 `;` 不切，
/// 引号里的转义、坏引号退回按 `;` 切），这里只管 `!important` 和展开简写。
pub fn parse_decls(s: &str) -> Vec<Decl> {
    let mut out = Vec::new();
    for d in bookconv::html::css_decls(s) {
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
    // `border-top: 1px solid red` 这类：拆成样式、宽度、颜色（没写的按 CSS 缺省：无、medium、当前颜色）。
    let border_side = |side: &str| -> Vec<(String, String)> {
        let (mut style, mut width, mut color) = ("none".to_string(), "medium".to_string(), String::new());
        for t in split_top(value, ' ').into_iter().map(str::trim).filter(|t| !t.is_empty()) {
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
            let h = value.split('/').next().unwrap_or("");
            let v: Vec<&str> = h.split_whitespace().collect();
            let (tl, tr, br, bl) = match v.as_slice() {
                [a] => (*a, *a, *a, *a),
                [a, b] => (*a, *b, *a, *b),
                [a, b, c] => (*a, *b, *c, *b),
                [a, b, c, d, ..] => (*a, *b, *c, *d),
                [] => return Vec::new(),
            };
            ["top-left", "top-right", "bottom-right", "bottom-left"].iter().zip([tl, tr, br, bl]).map(|(c, v)| (format!("border-{c}-radius"), v.to_string())).collect()
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
        // background 简写只取颜色。
        "background" => background_shorthand(value),
        _ => vec![(prop.to_string(), value.to_string())],
    }
}

/// `background: url(…) bottom / 100% no-repeat fixed rgba(…)` 拆成各项（没写的不出现，不重置）。
fn background_shorthand(value: &str) -> Vec<(String, String)> {
    let spaced = {
        // 把括号外的 `/` 隔开当单独的词
        let mut out = String::new();
        let mut depth = 0i32;
        for c in value.chars() {
            match c {
                '(' => depth += 1,
                ')' => depth -= 1,
                _ => {}
            }
            if c == '/' && depth == 0 {
                out.push_str(" / ");
            } else {
                out.push(c);
            }
        }
        out
    };
    let (mut pos, mut size, mut after_slash) = (Vec::new(), Vec::new(), false);
    let mut out = Vec::new();
    for t in split_top(&spaced, ' ').into_iter().map(str::trim).filter(|t| !t.is_empty()) {
        let l = t.to_ascii_lowercase();
        if l.starts_with("url(") {
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
    if !pos.is_empty() {
        out.push(("background-position".to_string(), pos.join(" ")));
    }
    if !size.is_empty() {
        out.push(("background-size".to_string(), size.join(" ")));
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
    let close_of = |open: usize, l: char, r: char| -> usize {
        let (mut depth, mut quote, mut i) = (0i32, None::<char>, open);
        while i < ch.len() {
            let x = ch[i];
            match quote {
                Some(q) if x == q => quote = None,
                Some(_) => {}
                None if x == '"' || x == '\'' => quote = Some(x),
                None if x == l => depth += 1,
                None if x == r => {
                    depth -= 1;
                    if depth == 0 {
                        return i;
                    }
                }
                None => {}
            }
            i += 1;
        }
        ch.len()
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
    /// 阅读模式的 KFX 阅读范围、屏幕、黑白彩色。
    pub fn for_profile(p: &profile::Profile) -> MediaEnv {
        let r = p.readable(profile::Format::Kfx);
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
            let mut depth = 0;
            let start = i;
            while i < b.len() {
                match b[i] {
                    b'(' => depth += 1,
                    b')' => {
                        depth -= 1;
                        if depth == 0 {
                            break;
                        }
                    }
                    _ => {}
                }
                i += 1;
            }
            if i >= b.len() {
                return None;
            }
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
    let v = v.trim();
    let end = v.find(|c: char| !(c.is_ascii_digit() || c == '.' || c == '-' || c == '+')).unwrap_or(v.len());
    let n: f64 = v[..end].parse().ok()?;
    let per = match &v[end..] {
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

    /// 一个元素上生效的声明（按层叠排好，后面的覆盖前面的），含行内 `style`。
    pub fn cascade(&self, el: &ElementRef) -> HashMap<String, String> {
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
    // 找 `@font-face {…}` 块用优化器同一个正则（`bookconv::wash::font_face_re`）。
    for m in bookconv::wash::font_face_re().find_iter(&css) {
        let block = m.as_str();
        let Some(open) = block.find('{') else { continue };
        let decls = parse_decls(&block[open + 1..block.len() - 1]);
        let get = |k: &str| decls.iter().rev().find(|d| d.prop == k).map(|d| d.value.as_str());
        let family = get("font-family").map(|f| f.trim().trim_matches(['"', '\'']).to_string()).filter(|f| !f.is_empty());
        let url = get("src").and_then(|src| {
            let i = src.to_ascii_lowercase().find("url(")?;
            let after = &src[i + 4..];
            let j = after.find(')')?;
            let raw = after[..j].trim().trim_matches(['"', '\'']).trim();
            (!raw.is_empty() && !raw.contains("://") && !raw.starts_with("data:")).then(|| bookconv::epubzip::resolve_link(base, raw).0)
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

/// 外边距、内边距的长度：同 [`parse_len`]，但 px 按 1px＝0.45pt 换算（Send to Kindle 的口径，和边框宽度一样：《绍宋》
/// `margin:-10px` 写成 -0.3125lh、`margin-top:-12.5px` 在半号字上写成 -1.172lh；以前按 CSS 的 1px＝0.75pt，大了 2/3）。字号不走这里。
pub fn parse_box_len(v: &str) -> Option<Len> {
    let t = v.trim().to_ascii_lowercase();
    match t.strip_suffix("px").and_then(|n| n.trim().parse::<f64>().ok()) {
        Some(n) => Some(Len::Pt(n * 0.45)),
        None => parse_len(&t),
    }
}

pub use bookconv::color::{contrast, ensure_contrast, luminance, over_white, parse_color};

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
    /// 自己或祖先有背景色、背景图（Send to Kindle 只在没有背景时省掉近黑的文字颜色，见 `write.rs` 的 `text_props`）。
    pub on_background: bool,
    /// 自己或祖先有背景图：只有背景图、没有背景色的地方不按对比度调文字颜色（Send to Kindle 同样：《雪国》扉页背景图上的白字照写）。
    pub on_image: bool,
    /// 自己或祖先（含 body）有左右外边距、内边距或边框：Send to Kindle 把首行缩进写成百分比（见 `write.rs` 的 `block_props`）。
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
    for tok in split_top(&first, ' ').into_iter().map(str::trim).filter(|s| !s.is_empty()) {
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
        return n.trim().parse::<f64>().ok().map(|n| BorderWidth::Pt(n * 0.45));
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
                let (_, size, margin) = bookconv::uastyle::HEADINGS.into_iter().find(|h| h.0 == tag).unwrap_or(bookconv::uastyle::HEADINGS[5]);
                c.font_size = parent.font_size * size;
                c.margin[0] = Some(Len::Em(margin));
                c.margin[2] = Some(Len::Em(margin));
            }
            // 段落、引文、图、预排版的上下外边距（UA 样式表）：Send to Kindle 给没写外边距的 `<p>` 上下各 1em
            // （相邻的折叠成一个，《绍宋》正文段与段之间 0.8333lh）；以前不给，段落挤在一起。哪些标签、多少和 `bookconv::uastyle` 的表一致。
            "p" | "dl" => {
                c.margin[0] = Some(Len::Em(1.0));
                c.margin[2] = Some(Len::Em(1.0));
            }
            "blockquote" | "figure" => {
                // 左右 40px，同样按 1px＝0.45pt（＝1.5em，和列表缩进一样）
                c.margin = [Some(Len::Em(1.0)), Some(Len::Pt(18.0)), Some(Len::Em(1.0)), Some(Len::Pt(18.0))];
            }
            "i" | "em" | "cite" | "var" | "dfn" => c.italic = true,
            "sup" => c.superscript = true,
            "sub" => c.subscript = true,
            "u" | "ins" => c.decoration[0] = true,
            "s" | "strike" | "del" => c.decoration[1] = true,
            "pre" => {
                c.pre = true;
                c.margin[0] = Some(Len::Em(1.0));
                c.margin[2] = Some(Len::Em(1.0));
                if c.font_family.is_none() {
                    c.font_family = Some("monospace".into());
                }
            }
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
                    Some(Len::Pt(n)) => Some(n / 12.0 / c.font_size),
                    None => c.line_height,
                }
            };
            // 数字倍数照倍数继承，长度（%、em、px、pt）算成绝对值继承（CSS 的规矩）
            c.line_height_abs = if unitless || v == "normal" { None } else { c.line_height.map(|l| l * c.font_size) };
        } else if let Some(a) = parent.line_height_abs {
            c.line_height = Some(a / c.font_size);
        }
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
                Some(Len::Pt(n)) => Some(n / 12.0 / c.font_size),
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
        // 不透明的纯白背景不算（Send to Kindle 同样：《疯探》`background-color:#ffffff` 的简介、目录页不出容器）
        c.background = get("background-color").and_then(parse_color).filter(|c| c >> 24 != 0 && *c != 0xFFFF_FFFF);
        c.backdrop = c.background.map(over_white).or(parent.backdrop);
        c.box_shadow = get("box-shadow").and_then(parse_shadow);
        if let Some(v) = get("text-shadow") {
            c.text_shadow = parse_shadow(v);
        }
        c.bg_image = get("background-image").filter(|v| v.to_ascii_lowercase().starts_with("url(")).map(|v| v[4..].trim_end_matches(')').trim().trim_matches(['"', '\'']).to_string());
        c.bg_no_repeat = get("background-repeat").is_some_and(|v| v.trim() == "no-repeat");
        c.bg_fixed = get("background-attachment").is_some_and(|v| v.trim() == "fixed");
        c.bg_position = get("background-position").map(|v| parse_bg_position(&v.to_ascii_lowercase())).unwrap_or([None; 2]);
        c.bg_size = get("background-size").map(|v| parse_bg_size(&v.to_ascii_lowercase())).unwrap_or([None; 2]);
        c.bg_cover = get("background-size").is_some_and(|v| v.trim().eq_ignore_ascii_case("cover"));
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
            assert_eq!(with.cascade(&el), without.cascade(&el), "{}", el.html());
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
        let d = s.cascade(&p);
        assert_eq!(d.get("color").map(String::as_str), Some("blue"));
        assert_eq!(d.get("text-indent").map(String::as_str), Some("2em"));
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
        let d = with.cascade(&p);
        assert_eq!((d.get("color").map(String::as_str), d.get("text-indent").map(String::as_str)), (Some("red"), Some("2em")));
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
