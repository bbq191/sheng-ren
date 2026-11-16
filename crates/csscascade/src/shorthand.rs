//! 简写展开：`margin`/`padding`/边框/`font`/`background` 等。

use crate::text::{background_tokens, tokens};
use crate::value::{parse_len, Len};
use bookbase::color::parse_color;

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
    let (v, important) = crate::decl::split_important(val);
    let (parts, balanced) = tokens(v);
    if !balanced {
        return BoxSides::Unknown;
    }
    let keyword = |p: &str| {
        matches!(
            p.to_ascii_lowercase().as_str(),
            "inherit" | "initial" | "unset" | "revert" | "revert-layer"
        )
    };
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
pub(crate) fn four_values(value: &str) -> Option<[&str; 4]> {
    match box_sides(value) {
        BoxSides::Sides(v, _) => Some(v),
        BoxSides::Keyword(k, _) => Some([k; 4]),
        BoxSides::Unknown => None,
    }
}

pub(crate) fn expand_shorthand(prop: &str, value: &str) -> Vec<(String, String)> {
    let four = |base: &str| -> Vec<(String, String)> {
        let Some(v) = four_values(value) else {
            return Vec::new();
        };
        ["top", "right", "bottom", "left"]
            .iter()
            .zip(v)
            .map(|(side, v)| (format!("{base}-{side}"), v.to_string()))
            .collect()
    };
    // `border-top: 1px solid red` 这类：拆成样式、宽度、颜色（没写的按 CSS 缺省：无、medium、当前颜色）。
    let border_side = |side: &str| -> Vec<(String, String)> {
        let (mut style, mut width, mut color) =
            ("none".to_string(), "medium".to_string(), String::new());
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
        let mut v = vec![
            (format!("border-{side}-style"), style),
            (format!("border-{side}-width"), width),
        ];
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
            four("border")
                .into_iter()
                .map(|(k, v)| (format!("{k}-{what}"), v))
                .collect()
        }
        "border-radius" => {
            // 只取「/」前面的（水平半径）；顺序是左上、右上、右下、左下。
            let Some(v) = four_values(value.split('/').next().unwrap_or("")) else {
                return Vec::new();
            };
            ["top-left", "top-right", "bottom-right", "bottom-left"]
                .iter()
                .zip(v)
                .map(|(c, v)| (format!("border-{c}-radius"), v.to_string()))
                .collect()
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
        "text-decoration" | "text-decoration-line" => {
            vec![("text-decoration".to_string(), value.to_ascii_lowercase())]
        }
        "background" => background_shorthand(value),
        "font" => font_shorthand(value),
        _ => vec![(prop.to_string(), value.to_string())],
    }
}

/// `font` 简写拆开的样子（CSS 2.1 / CSS Fonts 3 的语法：`[style || variant || weight || stretch]? size[/line-height]? family`）。
#[derive(Debug, PartialEq)]
pub struct FontShorthand<'a> {
    pub style: Option<&'a str>,
    pub variant: Option<&'a str>,
    pub weight: Option<&'a str>,
    pub stretch: Option<&'a str>,
    pub size: &'a str,
    pub line_height: Option<&'a str>,
    pub family: &'a str,
}

/// 按 CSS 规范拆 `font` 简写（值里已去掉 `!important`）。拿不准的（系统字体关键字 `caption`、全局关键字 `inherit`、
/// 字号前的词认不出、字号或行高是 `calc()` 之类带空格的写法、没有字体族）返回 `None`，不处理。
pub fn parse_font_shorthand(v: &str) -> Option<FontShorthand<'_>> {
    fn is_size(t: &str) -> bool {
        let l = t.to_ascii_lowercase();
        matches!(
            l.as_str(),
            "xx-small"
                | "x-small"
                | "small"
                | "medium"
                | "large"
                | "x-large"
                | "xx-large"
                | "xxx-large"
                | "larger"
                | "smaller"
        ) || (l.starts_with(|c: char| c.is_ascii_digit() || c == '.')
            && l.bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'%'))
    }
    fn is_line_height(t: &str) -> bool {
        t.eq_ignore_ascii_case("normal") || is_size(t)
    }
    const STRETCH: [&str; 8] = [
        "ultra-condensed",
        "extra-condensed",
        "condensed",
        "semi-condensed",
        "semi-expanded",
        "expanded",
        "extra-expanded",
        "ultra-expanded",
    ];
    let mut f = FontShorthand {
        style: None,
        variant: None,
        weight: None,
        stretch: None,
        size: "",
        line_height: None,
        family: "",
    };
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
            _ if l.len() == 3 && l.ends_with("00") && (b'1'..=b'9').contains(&l.as_bytes()[0]) => {
                Some(&mut f.weight)
            }
            _ if STRETCH.contains(&l.as_str()) => Some(&mut f.stretch),
            _ => break,
        };
        if let Some(slot) = slot {
            if slot.is_some() {
                return None;
            }
            *slot = Some(tok);
        }
        if normals
            + [f.style, f.variant, f.weight, f.stretch]
                .iter()
                .filter(|x| x.is_some())
                .count()
            > 4
        {
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
    let Some(f) = parse_font_shorthand(value) else {
        return Vec::new();
    };
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
    if matches!(
        value.trim().to_ascii_lowercase().as_str(),
        "inherit" | "initial" | "unset" | "revert" | "revert-layer"
    ) {
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
        } else if [
            "repeat",
            "no-repeat",
            "repeat-x",
            "repeat-y",
            "space",
            "round",
        ]
        .contains(&l.as_str())
        {
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
        out.push((
            k.to_string(),
            if v.is_empty() {
                BG_INITIAL.to_string()
            } else {
                v.join(" ")
            },
        ));
    }
    for (k, init) in [
        ("background-image", "none"),
        ("background-repeat", "repeat"),
        ("background-attachment", "scroll"),
        ("background-color", "transparent"),
    ] {
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
        [a, b] if matches!(*a, "top" | "bottom") || matches!(*b, "left" | "right") => {
            [kw(b), kw(a)]
        }
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

pub(crate) const BORDER_STYLES: &[&str] = &[
    "none", "hidden", "solid", "dashed", "dotted", "double", "groove", "ridge", "inset", "outset",
];
