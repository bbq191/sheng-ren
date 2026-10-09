//! 照 Send to Kindle 的规则排（只修复的文字书，profile `kindle_rules`：掌阅、Move；2026-10-08 用户定「三台统一是一套规则」）。
//! Kindle 那边由 KFX 写出器按这些规则写（`kfx::write`，规则表见 docs/kfx.md#send-to-kindle-的样式规则）；这里把其中会让
//! Kindle 和按 CSS 排的阅读器看起来不一样的几条，就地改进书自己的样式表（只改值、删声明，不加选择器——xochitl 的 CSS 解析器很脆）：
//!
//! - 标签的缺省样式（`<p>` 上下 1em 等）：`eink-ua.css`，见 [`crate::uastyle`]（`repair_entries` 里挂）；
//! - **正文字体用阅读器的字体**：`font-family` 第一个是正文字体（按继承逐元素算，[`super::fonts::pick_body_font`]）的整条去掉
//!   （Kindle 写 `default,…`，`default` 排第一、后面的备选不起作用；EPUB 里留着备选，阅读器会去找「宋体」这些系统字体）；
//!   写在 `font` 简写里的拆成分项、不写字体族（字号、行高、粗斜体照原值；拆不清的不动）；
//! - **body 的左右外边距、内边距不要**（Kindle 不写）；用到负的左（右）外边距的文件的字数占多数时，这一侧照留（Kindle 按文件定）；
//! - **文字和背景的对比度至少 4.5:1**（[`crate::color::ensure_contrast`]）：同一条规则里写了背景色的按它；没写的按白页面，
//!   但只调亮度不超过 0.5 的颜色（白字、浅灰多半压在背景图或外层的深色块上，看不到，不动）；规则写了深背景、没写文字颜色、
//!   缺省的黑字不够看清时补一条文字颜色。按规则算（Kindle 按元素算，背景来自祖先时这里看不到）。
//!
//! 文字一个不动；行内 `style` 同样处理（xochitl 不认行内样式，掌阅认）。
use super::css::{box_sides, compound_tag_classes, css_rule_re, last_compound, split_leading_statements, strip_css_comments, BoxSides};
use super::*;
use crate::color::{contrast, ensure_contrast, luminance, over_white, parse_color};

/// 照 Send to Kindle 的规则改书的样式表（独立的 `.css`、`<style>`）和行内 `style`；改了的声明数记进 `rep.kindle_rule_edits`。
pub(super) fn apply(entries: &mut [Entry], rep: &mut WashReport) {
    // 只用到类、标签的字体表（不投票、不找批注，见 `fonts::collect_rules`）
    let plan = super::fonts::collect_rules(entries);
    let (body_font, body_classes, neg) = scan(entries, &plan);
    let ctx = Ctx { body_font, body_classes, neg_left: neg.0, neg_right: neg.1 };
    let edits = crate::util::par_map_mut(entries, |e| {
        let mut n = 0;
        if is_css_name(&e.name) {
            let out = match std::str::from_utf8(&e.data) {
                Ok(text) => rewrite_css(text, &ctx, &mut n).into_bytes(),
                // 不是 UTF-8 的样式表（《啸风山庄》的 CSS.css 是 Big5）：按单字节原样读写。CSS 的语法都是 ASCII，要改的值（颜色、边距）
                // 也是，别的字节（Big5 写的字体名）原样留着
                Err(_) => crate::util::latin1_encode(&rewrite_css(&crate::util::latin1_decode(&e.data), &ctx, &mut n)),
            };
            if n > 0 {
                e.data = out;
            }
            return n;
        }
        if !is_chapter_entry(e) {
            return 0;
        }
        let Ok(text) = std::str::from_utf8(&e.data) else { return 0 };
        let s = html::edit_style_attrs(text, |t, value| {
            let body = t.name.eq_ignore_ascii_case("body");
            let (v, k) = rewrite_decls(value, body, &ctx);
            n += k;
            if k == 0 {
                html::Edit::Keep
            } else if v.trim().is_empty() {
                html::Edit::Remove
            } else {
                html::Edit::Set(v)
            }
        })
        .into_owned();
        let s = html::style_block_re().replace_all(&s, |c: &regex::Captures| format!("{}{}{}", &c[1], rewrite_css(&c[2], &ctx, &mut n), &c[3])).into_owned();
        if n > 0 {
            e.data = s.into_bytes();
        }
        n
    });
    rep.kindle_rule_edits += edits.into_iter().sum::<usize>();
}

struct Ctx {
    /// 正文字体（规范化：去引号、小写）。
    body_font: Option<String>,
    /// 只用在 `<body>` 上的类（别的元素也用的不算：改它的边距会连带改别处）。
    body_classes: HashSet<String>,
    neg_left: bool,
    neg_right: bool,
}

/// 正文字体（[`super::fonts::pick_body_font`] 的口径）；只用在 `<body>` 上的类；body 的左、右边距要不要照留。每个文件只扫一趟。
///
/// Kindle 按文件定（文件里有块的左/右外边距是负的，这一侧 body 的边距照加）；样式表是全书共用的，这里按字数取多数：
/// 用到负外边距的文件的字数多过没用到的，这一侧才留（《平凡的世界》183 个文件里 5 个用了负的左边距，丢掉；《罗杰疑案》每章都用，留）。
fn scan(entries: &[Entry], plan: &super::fonts::FontPlan) -> (Option<String>, HashSet<String>, (bool, bool)) {
    // 写了负的左、右外边距的规则：选择器最后一段的类名和裸标签名
    let (mut neg_cls, mut neg_tag): ([HashSet<String>; 2], [HashSet<String>; 2]) = Default::default();
    let mut take = |css: &str| {
        for c in css_rule_re().captures_iter(css) {
            let (l, r) = html::css_decls(&c[2]).iter().fold((false, false), |(l, r), d| {
                let (a, b) = negative_sides(d.prop, d.value);
                (l || a, r || b)
            });
            if !(l || r) {
                continue;
            }
            for part in strip_css_comments(split_leading_statements(&c[1]).1).split(',') {
                let (tag, classes) = compound_tag_classes(last_compound(part));
                for (side, on) in [(0, l), (1, r)] {
                    if on {
                        if classes.is_empty() {
                            if !tag.is_empty() {
                                neg_tag[side].insert(tag.clone());
                            }
                        } else {
                            neg_cls[side].extend(classes.iter().map(|c| c.to_string()));
                        }
                    }
                }
            }
        }
    };
    for e in entries.iter().filter(|e| is_css_name(&e.name)) {
        if let Ok(t) = std::str::from_utf8(&e.data) {
            take(t);
        }
    }
    for e in entries.iter().filter(|e| is_html_entry(&e.name, &e.data)) {
        if let Ok(t) = std::str::from_utf8(&e.data) {
            for c in html::style_block_re().captures_iter(t) {
                take(&c[2]);
            }
        }
    }
    struct FileScan {
        votes: super::fonts::FontVotes,
        on_body: Vec<String>,
        elsewhere: Vec<String>,
        used: [bool; 2],
        chars: usize,
    }
    let files: Vec<&Entry> = entries.iter().filter(|e| is_chapter_entry(e)).collect();
    let scans = crate::util::par_map(&files, |e| {
        let Ok(t) = std::str::from_utf8(&e.data) else { return None };
        let (mut on_body, mut elsewhere) = (Vec::new(), Vec::new());
        let mut used = [false; 2];
        for tag in html::tags(t).filter(|g| g.is_start()) {
            let raw = &t[tag.start..tag.end];
            let name = tag.name.to_ascii_lowercase();
            let cls: Vec<&str> = html::attr_value(raw, "class").map(|c| c.split_whitespace().collect()).unwrap_or_default();
            let set = if tag.is("body") { &mut on_body } else { &mut elsewhere };
            set.extend(cls.iter().map(|c| c.to_string()));
            for side in 0..2 {
                used[side] |= neg_tag[side].contains(&name) || cls.iter().any(|c| neg_cls[side].contains(*c));
            }
            if let Some(st) = html::attr_value(raw, "style") {
                for d in html::css_decls(st) {
                    let (a, b) = negative_sides(d.prop, d.value);
                    used[0] |= a;
                    used[1] |= b;
                }
            }
        }
        let chars = html::plain_text(t).chars().filter(|c| !c.is_whitespace()).count();
        Some(FileScan { votes: super::fonts::font_votes(t, plan), on_body, elsewhere, used, chars })
    });
    let (mut on_body, mut elsewhere) = (HashSet::new(), HashSet::new());
    // 每一侧：(用到负外边距的文件的字数, 没用到的)
    let mut chars = [(0usize, 0usize); 2];
    let mut votes = Vec::with_capacity(scans.len());
    for f in scans.into_iter().flatten() {
        on_body.extend(f.on_body);
        elsewhere.extend(f.elsewhere);
        for (c, used) in chars.iter_mut().zip(f.used) {
            if used {
                c.0 += f.chars;
            } else {
                c.1 += f.chars;
            }
        }
        votes.push(f.votes);
    }
    let keep = |s: usize| chars[s].0 > chars[s].1;
    (super::fonts::pick_body_font(votes), on_body.difference(&elsewhere).cloned().collect(), (keep(0), keep(1)))
}

/// 一条声明里左、右外边距有没有负值。
fn negative_sides(prop: &str, value: &str) -> (bool, bool) {
    let neg = |v: &str| v.trim_start().starts_with('-');
    match prop.trim().to_ascii_lowercase().as_str() {
        "margin-left" => (neg(value), false),
        "margin-right" => (false, neg(value)),
        "margin" => match box_sides(value) {
            BoxSides::Sides([_, r, _, l], _) => (neg(l), neg(r)),
            _ => (false, false),
        },
        _ => (false, false),
    }
}

fn rewrite_css(css: &str, ctx: &Ctx, n: &mut usize) -> String {
    css_rule_re()
        .replace_all(css, |c: &regex::Captures| {
            let (lead, sel) = split_leading_statements(&c[1]);
            let clean = strip_css_comments(sel);
            let trimmed = clean.trim();
            if trimmed.starts_with('@') {
                return c[0].to_string();
            }
            let body = trimmed.split(',').all(|s| is_body_selector(s, &ctx.body_classes));
            let (decls, k) = rewrite_decls(&c[2], body, ctx);
            *n += k;
            if k == 0 { c[0].to_string() } else { format!("{lead}{sel}{{{decls}}}") }
        })
        .into_owned()
}

/// 选择器是不是只选 `<body>`：`body`、`body.x`、`html body`、只用在 body 上的 `.x`。
fn is_body_selector(sel: &str, body_classes: &HashSet<String>) -> bool {
    let last = last_compound(sel);
    if last.contains([':', '[', '#']) {
        return false;
    }
    let mut parts = last.split('.');
    let tag = parts.next().unwrap_or("").to_ascii_lowercase();
    let classes: Vec<&str> = parts.collect();
    match tag.as_str() {
        "body" => true,
        "" => !classes.is_empty() && classes.iter().all(|c| body_classes.contains(*c)),
        _ => false,
    }
}

/// 改一段声明（规则体或行内 `style`）。返回 (新文本, 改了几条)。
fn rewrite_decls(text: &str, body: bool, ctx: &Ctx) -> (String, usize) {
    let decls = html::css_decls(text);
    let mut out = String::new();
    let mut changed = 0;
    let mut last = 0;
    // 同一条规则里的背景色
    let bg = decls.iter().rev().find_map(|d| match d.prop.to_ascii_lowercase().as_str() {
        "background-color" => parse_color(strip_important(d.value).0),
        "background" => crate::cssunlock::top_level_tokens(strip_important(d.value).0).0.into_iter().find_map(parse_color),
        _ => None,
    });
    let bg = bg.filter(|c| c >> 24 != 0).map(over_white).filter(|&c| c != 0xFFFF_FFFF);
    let has_bg = bg.is_some();
    let bg = bg.unwrap_or(0xFFFF_FFFF);
    let mut has_color = false;
    for d in &decls {
        out.push_str(&text[last..d.start]);
        last = d.start + d.raw.len();
        let prop = d.prop.to_ascii_lowercase();
        let (value, imp) = strip_important(d.value);
        let lead = &d.raw[..d.raw.find(|c: char| !c.is_whitespace()).unwrap_or(0)];
        let replaced: Option<String> = match prop.as_str() {
            // 整条去掉：留下备选（「st」「宋体」）的话阅读器会去找这些系统字体，就不是阅读器自己的字体了（Kindle 写 `default` 排第一）
            "font-family" => body_font_removed(value, ctx.body_font.as_deref()).map(|_| String::new()),
            // `font` 简写里的字体族是正文字体：拆成分项、不写字体族（字号、行高、粗斜体照原值）
            "font" => font_without_body_family(value, imp, lead, ctx.body_font.as_deref()),
            "color" => {
                has_color = true;
                parse_color(value).and_then(|col| {
                    // 规则里没写背景色时看不到祖先的背景（背景图、外层的背景色）：只调中等亮度以下的颜色，浅色（白字、浅灰）
                    // 多半压在图或深色块上，不动（Kindle 按元素算，背景图上的白字照写）
                    if !has_bg && luminance(over_white(col)) > 0.5 {
                        return None;
                    }
                    let adj = ensure_contrast(col, bg);
                    (adj != col).then(|| format!("{lead}color:{}{imp};", css_color(adj)))
                })
            }
            // body 的左右外边距、内边距（分项、简写一样）：这一侧有负外边距时照留（Kindle 把 body 这一侧的外边距加内边距照加）
            "margin-left" | "padding-left" if body && !ctx.neg_left => Some(String::new()),
            "margin-right" | "padding-right" if body && !ctx.neg_right => Some(String::new()),
            "margin" | "padding" if body => split_box(&prop, d.value, lead, ctx.neg_left, ctx.neg_right),
            _ => None,
        };
        match replaced {
            Some(r) => {
                changed += 1;
                out.push_str(&r);
            }
            None => out.push_str(d.raw),
        }
    }
    out.push_str(&text[last..]);
    // 写了深背景、没写文字颜色：缺省的黑字看不清时补上
    if !has_color && bg != 0xFFFF_FFFF && contrast(0xFF00_0000, bg) < 4.5 {
        let adj = ensure_contrast(0xFF00_0000, bg);
        if !out.trim_end().is_empty() && !out.trim_end().ends_with(';') {
            out.push(';');
        }
        out.push_str(&format!("color:{};", css_color(adj)));
        changed += 1;
    }
    (out, changed)
}

fn strip_important(v: &str) -> (&str, &'static str) {
    crate::cssunlock::split_important(v)
}

/// `font-family` 的第一个是正文字体时去掉它，返回剩下的备选（可能为空）；不是正文字体返回 `None`。
fn body_font_removed(value: &str, body: Option<&str>) -> Option<String> {
    let body = body?;
    let names: Vec<&str> = value.split(',').map(str::trim).filter(|s| !s.is_empty()).collect();
    let first = super::fonts::norm_family(names.first()?)?;
    (first == body).then(|| names[1..].join(","))
}

/// `font` 简写拆开的样子（CSS 2.1 / CSS Fonts 3 的语法：`[style || variant || weight || stretch]? size[/line-height]? family`）。
#[derive(Debug, PartialEq)]
struct FontShorthand<'a> {
    style: Option<&'a str>,
    variant: Option<&'a str>,
    weight: Option<&'a str>,
    stretch: Option<&'a str>,
    size: &'a str,
    line_height: Option<&'a str>,
    family: &'a str,
}

/// 按 CSS 规范拆 `font` 简写（值里已去掉 `!important`）。拿不准的（系统字体关键字 `caption`、全局关键字 `inherit`、
/// 字号前的词认不出、字号或行高是 `calc()` 之类带空格的写法、没有字体族）返回 `None`，不处理。
fn parse_font_shorthand(v: &str) -> Option<FontShorthand<'_>> {
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

/// `font` 简写里的字体族第一个是正文字体：换成分项，不写字体族（交给阅读器的字体），别的照简写的意思写全——简写会把没写的项
/// 重置成缺省值，所以没写的粗细、斜体、小型大写、行高写 `normal`（`font-stretch` 只在写了时写：认它的阅读器少）。
/// 不是正文字体、拆不清的返回 `None`（不动）。
fn font_without_body_family(value: &str, imp: &str, lead: &str, body: Option<&str>) -> Option<String> {
    let body = body?;
    let f = parse_font_shorthand(value)?;
    if super::fonts::norm_family(f.family)? != body {
        return None;
    }
    let mut s = String::from(lead);
    let mut put = |prop: &str, v: &str| s.push_str(&format!("{prop}:{v}{imp};"));
    put("font-style", f.style.unwrap_or("normal"));
    put("font-variant", f.variant.unwrap_or("normal"));
    put("font-weight", f.weight.unwrap_or("normal"));
    if let Some(st) = f.stretch {
        put("font-stretch", st);
    }
    put("font-size", f.size);
    put("line-height", f.line_height.unwrap_or("normal"));
    Some(s)
}

/// body 的 `margin`/`padding` 简写拆开，只留上下（和要留的左右）。值按 [`box_sides`] 拆（括号里的空格不算分隔：`calc(1em + 2px)`），
/// 拆不清的不动。
fn split_box(prop: &str, value: &str, lead: &str, keep_left: bool, keep_right: bool) -> Option<String> {
    let ([top, right, bottom, left], imp) = match box_sides(value) {
        BoxSides::Sides(s, imp) => (s, imp),
        BoxSides::Keyword(k, imp) => ([k; 4], imp),
        BoxSides::Unknown => return None,
    };
    let zero = |v: &str| v.trim_start_matches(['0', '.']).chars().all(|c| c.is_ascii_alphabetic() || c == '%') && v.starts_with('0');
    if zero(right) && zero(left) {
        return None;
    }
    let mut s = format!("{lead}{prop}-top:{top}{imp};{prop}-bottom:{bottom}{imp};");
    if keep_left {
        s.push_str(&format!("{prop}-left:{left}{imp};"));
    }
    if keep_right {
        s.push_str(&format!("{prop}-right:{right}{imp};"));
    }
    Some(s)
}

/// ARGB → CSS `#rrggbb`。写的都是 [`ensure_contrast`] 调出来的不透明颜色（半透明的按合成后的颜色调，结果不透明），
/// 不写 `rgba()`（掌阅不认，整条声明作废）。
fn css_color(c: u32) -> String {
    debug_assert_eq!(c >> 24, 0xFF, "只写不透明颜色");
    format!("#{:06x}", c & 0xFF_FFFF)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx() -> Ctx {
        Ctx { body_font: Some("宋体".into()), body_classes: ["calibre".to_string()].into_iter().collect(), neg_left: false, neg_right: false }
    }

    #[test]
    fn rules_like_send_to_kindle() {
        let c = ctx();
        let mut n = 0;
        let css = r#"body{margin:0 5pt;font-family:"宋体",serif} .calibre{padding-left:1em;margin-top:1em} p{font-family:"宋体"} h1{color:#fff;background-color:#f0a200} .pale{color:#f5ac00} .dark{background-color:#0168b7} .ok{color:#333}"#;
        let out = rewrite_css(css, &c, &mut n);
        assert!(out.contains("body{margin-top:0;margin-bottom:0;}"), "{out}");
        assert!(out.contains(".calibre{margin-top:1em}"), "只用在 body 上的类去掉左右：{out}");
        assert!(out.contains("p{}"), "正文字体整条去掉：{out}");
        let white = rewrite_css(".t{color:#fff}", &c, &mut n);
        assert_eq!(white, ".t{color:#fff}", "没写背景色的白字不动（多半压在背景图上）");
        assert!(out.contains("h1{color:#454545;background-color:#f0a200}") && out.contains(".pale{color:#9d6e00;}"), "{out}");
        assert!(out.contains(".dark{background-color:#0168b7;color:#e4e4e4;}") && out.contains(".ok{color:#333}"), "{out}");
        // 有负的左外边距：body 的左边距照留
        let c2 = Ctx { neg_left: true, ..ctx() };
        let out2 = rewrite_css("body{margin:0 5pt}", &c2, &mut n);
        assert_eq!(out2, "body{margin-top:0;margin-bottom:0;margin-left:5pt;}");
        assert_eq!(negative_sides("margin", "-2em -2em 1.5em"), (true, true));
        assert_eq!(negative_sides("margin", "0 1em 0 -1em"), (true, false));
    }

    /// 负外边距那一侧：分项、简写的 margin 和 padding 都照留（以前简写的 padding 左右照删，和分项的 `padding-left` 不一致）。
    #[test]
    fn negative_side_kept_for_padding_shorthand_too() {
        let c = Ctx { neg_left: true, ..ctx() };
        let mut n = 0;
        assert_eq!(rewrite_css("body{padding-left:3em}", &c, &mut n), "body{padding-left:3em}");
        assert_eq!(rewrite_css("body{padding:0 3em}", &c, &mut n), "body{padding-top:0;padding-bottom:0;padding-left:3em;}");
        assert_eq!(rewrite_css("body{margin:1em 0 1em 2em}", &c, &mut n), "body{margin-top:1em;margin-bottom:1em;margin-left:2em;}");
    }

    /// 带空格的 `calc()` 不再被空格拆坏（以前 `calc(1em` `+` `2px)` 当成三个值，写出坏 CSS）。
    #[test]
    fn calc_in_box_shorthand_not_broken() {
        let c = ctx();
        let mut n = 0;
        assert_eq!(rewrite_css("body{margin:0 calc(1em + 2px)}", &c, &mut n), "body{margin-top:0;margin-bottom:0;}");
        let c2 = Ctx { neg_left: true, ..ctx() };
        assert_eq!(rewrite_css("body{margin:0 calc(1em + 2px) !important}", &c2, &mut n), "body{margin-top:0 !important;margin-bottom:0 !important;margin-left:calc(1em + 2px) !important;}");
        assert_eq!(negative_sides("margin", "0 calc(-1em + 2px)"), (false, false));
        assert_eq!(negative_sides("margin", "0 -1em !important"), (true, true));
    }

    /// `font` 简写里的正文字体：拆成分项、不写字体族；别的字体、拆不清的不动。
    #[test]
    fn body_font_in_font_shorthand() {
        let c = ctx();
        let mut n = 0;
        assert_eq!(
            rewrite_css(r#"p{font:italic bold 1.2em/1.5 "宋体", serif}"#, &c, &mut n),
            "p{font-style:italic;font-variant:normal;font-weight:bold;font-size:1.2em;line-height:1.5;}"
        );
        assert_eq!(rewrite_css(r#"p{font: 12pt 宋体 !important}"#, &c, &mut n), "p{font-style:normal !important;font-variant:normal !important;font-weight:normal !important;font-size:12pt !important;line-height:normal !important;}");
        for keep in [r#"p{font:1em "楷体"}"#, "p{font:caption}", "p{font:calc(1em + 1px) 宋体}", "p{font:inherit}", "p{font:bold 宋体}"] {
            assert_eq!(rewrite_css(keep, &c, &mut n), keep);
        }
        assert_eq!(
            parse_font_shorthand("normal small-caps 700 condensed larger / 2em  'A B', serif"),
            Some(FontShorthand { style: None, variant: Some("small-caps"), weight: Some("700"), stretch: Some("condensed"), size: "larger", line_height: Some("2em"), family: "'A B', serif" })
        );
        assert_eq!(parse_font_shorthand("bold bold 1em x"), None);
    }

    #[test]
    fn written_colors_are_opaque_hex() {
        assert_eq!(css_color(0xFF12_AB34), "#12ab34");
        // 半透明的浅色字：按合成到背景上的颜色调，写出来是不透明的 #hex（不写 rgba：掌阅不认）
        let c = ctx();
        let mut n = 0;
        let out = rewrite_css(".x{color:rgba(0,0,0,0.4)}", &c, &mut n);
        assert!(out.starts_with(".x{color:#") && !out.contains("rgba"), "{out}");
    }
}
