//! 解除"锁死阅读器设置"的 CSS 声明，别的样式（颜色、粗斜体、对齐……）一律不动（用户 2026-09-29：解除字体和字号锁定，
//! 但不改动颜色等其他样式）。清洗层（`wash::css`，样式表、`<style>`、`style=""`）和优化器（`htmlproc::strip_font_locks`，
//! 不清洗时的内联样式）共用这一套判断。
//!
//! | 属性 | 处理 |
//! |---|---|
//! | `font-family` | 去掉（字体交给阅读器）；书里嵌了文件、又不是正文字体的保留（用户 2026-10-05，见 `wash::fonts`） |
//! | `font-size` | 绝对单位（px/pt/cm…、`medium` 这类关键字）去掉；相对单位（em/%/rem/ex/ch、`smaller`/`larger`）保留——标题、小字、上标的层次是原书的样式，随阅读器字号一起缩放，不算锁；但正文整体那一层（`body`/`html`、不带类的 `p`/`div`）上的相对字号也去掉，否则全书正文被缩放一遍，阅读器里设的字号就不是实际字号 |
//! | `font` 简写 | 拆开：字体、字号、行高去掉，`font-style`/`font-weight`/`font-variant` 保留成分项 |
//! | `line-height` | 去掉（2026-09-27 用户定：写死的行高让阅读器的"行距"不起作用） |
//! | `background` 简写 | 只留颜色（写成 `background-color`），背景图去掉（xochitl 不认 no-repeat，把背景图平铺满页盖住正文，真机《飘》） |
//! | `background-image` | 去掉（同上） |
//!
//! 这些属性要在调用方的过滤清单里（`WashOpts::filter_props`）才处理；不在清单里的原样保留。

use std::collections::HashSet;

/// 一条声明怎么处理。
#[derive(Debug, PartialEq, Eq)]
pub enum Unlock {
    Keep,
    Drop,
    /// 换成这些声明（`属性:值`，不带分号）。
    Replace(Vec<String>),
}

/// 判断一条（已在过滤清单里的）声明。`base_text` = 这条声明作用在正文整体那一层（`body`/`html`、不带类的 `p`/`div`）。
/// `keep_fonts`：保留的字体（规范化的名字，见 `wash::fonts::norm_family`）；`font-family` 的第一个字体在里面就保留。
pub fn unlock(prop: &str, value: &str, base_text: bool, keep_fonts: &HashSet<String>) -> Unlock {
    match prop.to_ascii_lowercase().as_str() {
        "font-family" if crate::wash::fonts::norm_family(value).is_some_and(|f| keep_fonts.contains(&f)) => Unlock::Keep,
        "font-family" | "line-height" | "background-image" => Unlock::Drop,
        "font-size" => {
            if !base_text && is_relative_size(value) {
                Unlock::Keep
            } else {
                Unlock::Drop
            }
        }
        "font" => {
            let kept = font_shorthand_style(value);
            if kept.is_empty() {
                Unlock::Drop
            } else {
                Unlock::Replace(kept)
            }
        }
        "background" => match background_color(value) {
            Some(c) => Unlock::Replace(vec![format!("background-color:{c}")]),
            None => Unlock::Drop,
        },
        // 清单里别的属性（调用方额外加的，如注释容器的 `font-weight`）：整条去掉
        _ => Unlock::Drop,
    }
}

/// `!important` 拆出来：(值, " !important" 或 "")。
pub(crate) fn split_important(value: &str) -> (&str, &'static str) {
    let v = value.trim();
    match v.rfind('!') {
        Some(i) if v[i + 1..].trim().eq_ignore_ascii_case("important") => (v[..i].trim_end(), " !important"),
        _ => (v, ""),
    }
}

/// 字号是不是相对的：`smaller`/`larger`/`inherit` 之类，或者里面每个带单位的数都是 em/%/rem/ex/ch。
/// `calc()` 里混了绝对单位、`12px`、`medium`、`x-large` 这些都算绝对。
pub fn is_relative_size(value: &str) -> bool {
    let (v, _) = split_important(value);
    let l = v.to_ascii_lowercase();
    if matches!(l.as_str(), "smaller" | "larger" | "inherit" | "initial" | "unset" | "revert") {
        return true;
    }
    let mut saw_number = false;
    let b = l.as_bytes();
    let mut i = 0;
    while i < b.len() {
        if b[i].is_ascii_digit() || (b[i] == b'.' && b.get(i + 1).is_some_and(u8::is_ascii_digit)) {
            while i < b.len() && (b[i].is_ascii_digit() || b[i] == b'.') {
                i += 1;
            }
            let start = i;
            while i < b.len() && (b[i].is_ascii_alphabetic() || b[i] == b'%') {
                i += 1;
            }
            let unit = &l[start..i];
            if !matches!(unit, "em" | "%" | "rem" | "ex" | "ch") {
                return false; // px/pt/…；不带单位的数（只有 0 合法）也当绝对，拿不准就去掉
            }
            saw_number = true;
        } else {
            i += 1;
        }
    }
    saw_number
}

/// `font` 简写里字号之前的样式词：`italic bold small-caps 12px/1.5 "Foo"` → `font-style:italic`、`font-weight:bold`、
/// `font-variant:small-caps`。`normal` 分不清是哪一项，不留；系统字体关键字（`caption`、`menu`……）整条去掉。
fn font_shorthand_style(value: &str) -> Vec<String> {
    let (v, important) = split_important(value);
    let mut out = Vec::new();
    for tok in v.split_whitespace() {
        let t = tok.to_ascii_lowercase();
        let decl = match t.as_str() {
            "italic" | "oblique" => ("font-style", tok),
            "bold" | "bolder" | "lighter" => ("font-weight", tok),
            "small-caps" => ("font-variant", tok),
            "normal" => continue,
            _ if t.len() == 3 && t.ends_with("00") && t.as_bytes()[0].is_ascii_digit() => ("font-weight", tok),
            _ => break, // 到字号（或系统字体关键字）了：后面是字号、行高、字体
        };
        out.push(format!("{}:{}{important}", decl.0, decl.1));
    }
    out
}

/// 保留背景图的模式（profile 的 `background_images`）里 `background` 简写拆成分项：背景色、背景图、重复、位置，
/// `sizing` 时再加尺寸（`/ cover`）和附着（`fixed`）。掌阅不认 `background` 简写（整条不生效），`background-size`
/// 会把图挤变形；Kindle 要 `fixed` 和尺寸才按整页铺（2026-10-05 真机）。KFX 写出器两种写法都认。
pub fn background_longhands(value: &str, sizing: bool) -> Vec<String> {
    let (v, important) = split_important(value);
    // 括号外的 `/`（位置 / 尺寸）隔开成单独的词，`url(../a.png)` 里的不动
    let mut spaced = String::new();
    let mut depth = 0i32;
    for c in v.chars() {
        match c {
            '(' => depth += 1,
            ')' => depth -= 1,
            _ => {}
        }
        if c == '/' && depth == 0 {
            spaced.push_str(" / ");
        } else {
            spaced.push(c);
        }
    }
    let mut out = Vec::new();
    let mut pos = Vec::new();
    let mut size = Vec::new();
    let mut after_slash = false;
    for t in top_level_tokens(&spaced).0 {
        let l = t.to_ascii_lowercase();
        if after_slash && !(["cover", "contain", "auto"].contains(&l.as_str()) || l.starts_with(|c: char| c.is_ascii_digit() || c == '.')) {
            after_slash = false; // 尺寸写完了
        }
        if l.starts_with("url(") {
            out.push(format!("background-image:{t}{important}"));
        } else if l == "/" {
            after_slash = true;
        } else if ["repeat", "no-repeat", "repeat-x", "repeat-y", "space", "round"].contains(&l.as_str()) {
            out.push(format!("background-repeat:{l}{important}"));
        } else if after_slash {
            size.push(t);
        } else if ["fixed", "scroll", "local"].contains(&l.as_str()) {
            if sizing {
                out.push(format!("background-attachment:{l}{important}"));
            }
        } else if ["border-box", "padding-box", "content-box", "none"].contains(&l.as_str()) {
            // 盒子：不要
        } else if ["left", "right", "top", "bottom", "center"].contains(&l.as_str()) || l.starts_with(|c: char| c.is_ascii_digit() || c == '-' || c == '.') {
            pos.push(t);
        } else {
            out.push(format!("background-color:{t}{important}"));
        }
    }
    if !pos.is_empty() {
        out.push(format!("background-position:{}{important}", pos.join(" ")));
    }
    if sizing && !size.is_empty() {
        out.push(format!("background-size:{}{important}", size.join(" ")));
    }
    out
}

/// 按顶层空白切词，括号里的空格不算（`rgb(0, 0, 0)`、`url(a b.png)`）。另返回括号配不配对（`(` 与 `)` 个数相同）。
/// `background` 简写（这里）和 `margin`/`padding` 简写（`wash::css::box_sides`）共用。
pub(crate) fn top_level_tokens(v: &str) -> (Vec<&str>, bool) {
    let mut toks = Vec::new();
    let (mut depth, mut start) = (0i32, None::<usize>);
    for (i, ch) in v.char_indices() {
        match ch {
            '(' => depth += 1,
            ')' => depth -= 1,
            c if c.is_whitespace() && depth == 0 => {
                if let Some(s) = start.take() {
                    toks.push(&v[s..i]);
                }
                continue;
            }
            _ => {}
        }
        start.get_or_insert(i);
    }
    if let Some(s) = start {
        toks.push(&v[s..]);
    }
    (toks, depth == 0)
}

/// `background` 简写里的颜色：`#hex`、`rgb()/rgba()/hsl()/hsla()`、`transparent`、命名色。没有就 `None`。
fn background_color(value: &str) -> Option<String> {
    let (v, important) = split_important(value);
    let (toks, _) = top_level_tokens(v);
    const NOT_COLOR: &[&str] = &[
        "none", "repeat", "no-repeat", "repeat-x", "repeat-y", "space", "round", "left", "right", "top", "bottom", "center", "fixed",
        "scroll", "local", "cover", "contain", "auto", "border-box", "padding-box", "content-box", "text", "inherit", "initial", "unset",
        "revert",
    ];
    toks.into_iter()
        .find(|t| {
            let l = t.to_ascii_lowercase();
            l.starts_with('#')
                || ["rgb(", "rgba(", "hsl(", "hsla("].iter().any(|p| l.starts_with(p))
                || (l.chars().all(|c| c.is_ascii_alphabetic()) && !NOT_COLOR.contains(&l.as_str()))
        })
        .map(|c| format!("{c}{important}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn background_shorthand_split_for_readers() {
        assert_eq!(
            background_longhands(r#"url("../Images/jsy.png") bottom / cover no-repeat fixed rgba(117, 0, 0, 1)"#, false),
            [r#"background-image:url("../Images/jsy.png")"#, "background-repeat:no-repeat", "background-color:rgba(117, 0, 0, 1)", "background-position:bottom"]
        );
        assert_eq!(background_longhands("url(a.png) no-repeat fixed #111", false), ["background-image:url(a.png)", "background-repeat:no-repeat", "background-color:#111"]);
        assert_eq!(
            background_longhands("url(a.png) bottom / 100% no-repeat fixed #111", true),
            ["background-image:url(a.png)", "background-repeat:no-repeat", "background-attachment:fixed", "background-color:#111", "background-position:bottom", "background-size:100%"]
        );
    }

    #[test]
    fn font_size_relative_kept_absolute_dropped() {
        for v in ["0.8em", "120%", "1.2rem", "smaller", "larger", "calc(1em + 10%)", "0.9em !important"] {
            assert_eq!(unlock("font-size", v, false, &HashSet::new()), Unlock::Keep, "{v}");
        }
        for v in ["12px", "10.5pt", "medium", "x-large", "calc(1em + 2px)", "0", "small"] {
            assert_eq!(unlock("font-size", v, false, &HashSet::new()), Unlock::Drop, "{v}");
        }
        assert_eq!(unlock("font-size", "0.9em", true, &HashSet::new()), Unlock::Drop, "正文整体那一层的相对字号也去掉");
    }

    #[test]
    fn font_family_kept_only_when_in_keep_set() {
        let keep: HashSet<String> = ["letter".to_string()].into();
        assert_eq!(unlock("font-family", "\"letter\", serif", false, &keep), Unlock::Keep);
        assert_eq!(unlock("font-family", "'Letter' !important", false, &keep), Unlock::Keep, "大小写、引号、!important 不影响");
        assert_eq!(unlock("font-family", "\"宋体\"", false, &keep), Unlock::Drop);
        assert_eq!(unlock("font-family", "serif, letter", false, &keep), Unlock::Drop, "只看第一个字体");
    }

    #[test]
    fn font_shorthand_keeps_style_and_weight() {
        assert_eq!(
            unlock("font", "italic bold 12px/30px Georgia, serif", false, &HashSet::new()),
            Unlock::Replace(vec!["font-style:italic".into(), "font-weight:bold".into()])
        );
        assert_eq!(unlock("font", "700 1em \"Foo\" !important", false, &HashSet::new()), Unlock::Replace(vec!["font-weight:700 !important".into()]));
        assert_eq!(unlock("font", "normal small-caps 10pt serif", false, &HashSet::new()), Unlock::Replace(vec!["font-variant:small-caps".into()]));
        assert_eq!(unlock("font", "12px serif", false, &HashSet::new()), Unlock::Drop);
        assert_eq!(unlock("font", "caption", false, &HashSet::new()), Unlock::Drop);
    }

    #[test]
    fn background_keeps_only_color() {
        assert_eq!(unlock("background", "url(a.png) no-repeat #eee", false, &HashSet::new()), Unlock::Replace(vec!["background-color:#eee".into()]));
        assert_eq!(unlock("background", "rgb(0, 0, 0) url(\"x y.png\")", false, &HashSet::new()), Unlock::Replace(vec!["background-color:rgb(0, 0, 0)".into()]));
        assert_eq!(unlock("background", "LightYellow", false, &HashSet::new()), Unlock::Replace(vec!["background-color:LightYellow".into()]));
        assert_eq!(unlock("background", "url(a.png) no-repeat center", false, &HashSet::new()), Unlock::Drop);
        assert_eq!(unlock("background", "none", false, &HashSet::new()), Unlock::Drop);
        assert_eq!(unlock("background-image", "url(a.png)", false, &HashSet::new()), Unlock::Drop);
    }

    #[test]
    fn other_listed_props_dropped() {
        assert_eq!(unlock("font-weight", "bold", false, &HashSet::new()), Unlock::Drop, "调用方额外加进清单的属性整条去掉");
        assert_eq!(unlock("font-family", "Foo", false, &HashSet::new()), Unlock::Drop);
        assert_eq!(unlock("line-height", "1.5", false, &HashSet::new()), Unlock::Drop);
    }
}
