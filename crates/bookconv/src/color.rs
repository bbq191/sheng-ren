//! 颜色：CSS 颜色解析、WCAG 对比度和 Send to Kindle 的对比度规则。KFX 写出器（`kfx::css`）和掌阅、Move 的
//! Send to Kindle 规则（`wash::kindle_rules`）共用。

/// `#rgb`、`#rgba`、`#rrggbb`、`#rrggbbaa`、`rgb()`/`rgba()`、常见色名 → ARGB（透明度和 `rgba()` 一样写进最高字节）。
pub fn parse_color(v: &str) -> Option<u32> {
    let v = v.trim().to_ascii_lowercase();
    if let Some(h) = v.strip_prefix('#') {
        if !h.bytes().all(|c| c.is_ascii_hexdigit()) {
            return None;
        }
        let n = u32::from_str_radix(h, 16).ok()?;
        let nib = |k: u32| ((n >> (4 * k)) & 0xF) * 17;
        return match h.len() {
            3 => Some(0xFF00_0000 | nib(2) << 16 | nib(1) << 8 | nib(0)),
            4 => Some(nib(0) << 24 | nib(3) << 16 | nib(2) << 8 | nib(1)),
            6 => Some(0xFF00_0000 | n),
            8 => Some(n.rotate_right(8)),
            _ => None,
        };
    }
    if let Some(inner) = v.strip_prefix("rgba(").or_else(|| v.strip_prefix("rgb(")).and_then(|s| s.strip_suffix(')')) {
        // 每一项可以是数（颜色 0–255、透明度 0–1）或百分比（100% = 255 / 1）。以前把 `%` 直接去掉，
        // `rgb(100%, 0%, 0%)` 成了 (100, 0, 0)，`rgba(…, 50%)` 成了不透明。
        // 任何一项不是数（`var(--r)`、`calc(…)`、空项）就整条不认：以前按 0 算，`rgb(var(--r), 0, 0)` 成了黑色（拿不准就不处理）。
        let p: Vec<(f64, bool)> = inner
            .split(',')
            .map(|x| {
                let x = x.trim();
                let pct = x.ends_with('%');
                x.trim_end_matches('%').trim().parse::<f64>().ok().filter(|v| v.is_finite()).map(|v| (v, pct))
            })
            .collect::<Option<_>>()?;
        if p.len() < 3 {
            return None;
        }
        // 透明度照 Send to Kindle 舍去小数（《绍宋》`rgba(128,0,0,0.7)` → 0xB2、0.5 → 0x7F；以前四舍五入成 0xB3、0x80）。
        // 加一点余量：0.6 × 255 在浮点里是 152.99999…
        let a = p.get(3).map(|&(a, pct)| ((if pct { a / 100.0 } else { a }).clamp(0.0, 1.0) * 255.0 + 1e-6).floor() as u32).unwrap_or(255);
        let c = |(x, pct): (f64, bool)| (if pct { x / 100.0 * 255.0 } else { x }).clamp(0.0, 255.0).round() as u32;
        return Some(a << 24 | c(p[0]) << 16 | c(p[1]) << 8 | c(p[2]));
    }
    if v == "transparent" {
        return Some(0);
    }
    // CSS 的 148 个颜色名（以前只认 9 个：《狼厅》`color:brown` 的诗句颜色丢了，Send to Kindle 照写 #a52a2a）
    NAMED.binary_search_by(|(n, _)| n.cmp(&v.as_str())).ok().map(|i| 0xFF00_0000 | NAMED[i].1)
}

/// CSS 颜色名（按名字排序，二分查找）。
const NAMED: [(&str, u32); 148] = [
        ("aliceblue", 0xF0F8FF),
        ("antiquewhite", 0xFAEBD7),
        ("aqua", 0x00FFFF),
        ("aquamarine", 0x7FFFD4),
        ("azure", 0xF0FFFF),
        ("beige", 0xF5F5DC),
        ("bisque", 0xFFE4C4),
        ("black", 0x000000),
        ("blanchedalmond", 0xFFEBCD),
        ("blue", 0x0000FF),
        ("blueviolet", 0x8A2BE2),
        ("brown", 0xA52A2A),
        ("burlywood", 0xDEB887),
        ("cadetblue", 0x5F9EA0),
        ("chartreuse", 0x7FFF00),
        ("chocolate", 0xD2691E),
        ("coral", 0xFF7F50),
        ("cornflowerblue", 0x6495ED),
        ("cornsilk", 0xFFF8DC),
        ("crimson", 0xDC143C),
        ("cyan", 0x00FFFF),
        ("darkblue", 0x00008B),
        ("darkcyan", 0x008B8B),
        ("darkgoldenrod", 0xB8860B),
        ("darkgray", 0xA9A9A9),
        ("darkgreen", 0x006400),
        ("darkgrey", 0xA9A9A9),
        ("darkkhaki", 0xBDB76B),
        ("darkmagenta", 0x8B008B),
        ("darkolivegreen", 0x556B2F),
        ("darkorange", 0xFF8C00),
        ("darkorchid", 0x9932CC),
        ("darkred", 0x8B0000),
        ("darksalmon", 0xE9967A),
        ("darkseagreen", 0x8FBC8F),
        ("darkslateblue", 0x483D8B),
        ("darkslategray", 0x2F4F4F),
        ("darkslategrey", 0x2F4F4F),
        ("darkturquoise", 0x00CED1),
        ("darkviolet", 0x9400D3),
        ("deeppink", 0xFF1493),
        ("deepskyblue", 0x00BFFF),
        ("dimgray", 0x696969),
        ("dimgrey", 0x696969),
        ("dodgerblue", 0x1E90FF),
        ("firebrick", 0xB22222),
        ("floralwhite", 0xFFFAF0),
        ("forestgreen", 0x228B22),
        ("fuchsia", 0xFF00FF),
        ("gainsboro", 0xDCDCDC),
        ("ghostwhite", 0xF8F8FF),
        ("gold", 0xFFD700),
        ("goldenrod", 0xDAA520),
        ("gray", 0x808080),
        ("green", 0x008000),
        ("greenyellow", 0xADFF2F),
        ("grey", 0x808080),
        ("honeydew", 0xF0FFF0),
        ("hotpink", 0xFF69B4),
        ("indianred", 0xCD5C5C),
        ("indigo", 0x4B0082),
        ("ivory", 0xFFFFF0),
        ("khaki", 0xF0E68C),
        ("lavender", 0xE6E6FA),
        ("lavenderblush", 0xFFF0F5),
        ("lawngreen", 0x7CFC00),
        ("lemonchiffon", 0xFFFACD),
        ("lightblue", 0xADD8E6),
        ("lightcoral", 0xF08080),
        ("lightcyan", 0xE0FFFF),
        ("lightgoldenrodyellow", 0xFAFAD2),
        ("lightgray", 0xD3D3D3),
        ("lightgreen", 0x90EE90),
        ("lightgrey", 0xD3D3D3),
        ("lightpink", 0xFFB6C1),
        ("lightsalmon", 0xFFA07A),
        ("lightseagreen", 0x20B2AA),
        ("lightskyblue", 0x87CEFA),
        ("lightslategray", 0x778899),
        ("lightslategrey", 0x778899),
        ("lightsteelblue", 0xB0C4DE),
        ("lightyellow", 0xFFFFE0),
        ("lime", 0x00FF00),
        ("limegreen", 0x32CD32),
        ("linen", 0xFAF0E6),
        ("magenta", 0xFF00FF),
        ("maroon", 0x800000),
        ("mediumaquamarine", 0x66CDAA),
        ("mediumblue", 0x0000CD),
        ("mediumorchid", 0xBA55D3),
        ("mediumpurple", 0x9370DB),
        ("mediumseagreen", 0x3CB371),
        ("mediumslateblue", 0x7B68EE),
        ("mediumspringgreen", 0x00FA9A),
        ("mediumturquoise", 0x48D1CC),
        ("mediumvioletred", 0xC71585),
        ("midnightblue", 0x191970),
        ("mintcream", 0xF5FFFA),
        ("mistyrose", 0xFFE4E1),
        ("moccasin", 0xFFE4B5),
        ("navajowhite", 0xFFDEAD),
        ("navy", 0x000080),
        ("oldlace", 0xFDF5E6),
        ("olive", 0x808000),
        ("olivedrab", 0x6B8E23),
        ("orange", 0xFFA500),
        ("orangered", 0xFF4500),
        ("orchid", 0xDA70D6),
        ("palegoldenrod", 0xEEE8AA),
        ("palegreen", 0x98FB98),
        ("paleturquoise", 0xAFEEEE),
        ("palevioletred", 0xDB7093),
        ("papayawhip", 0xFFEFD5),
        ("peachpuff", 0xFFDAB9),
        ("peru", 0xCD853F),
        ("pink", 0xFFC0CB),
        ("plum", 0xDDA0DD),
        ("powderblue", 0xB0E0E6),
        ("purple", 0x800080),
        ("rebeccapurple", 0x663399),
        ("red", 0xFF0000),
        ("rosybrown", 0xBC8F8F),
        ("royalblue", 0x4169E1),
        ("saddlebrown", 0x8B4513),
        ("salmon", 0xFA8072),
        ("sandybrown", 0xF4A460),
        ("seagreen", 0x2E8B57),
        ("seashell", 0xFFF5EE),
        ("sienna", 0xA0522D),
        ("silver", 0xC0C0C0),
        ("skyblue", 0x87CEEB),
        ("slateblue", 0x6A5ACD),
        ("slategray", 0x708090),
        ("slategrey", 0x708090),
        ("snow", 0xFFFAFA),
        ("springgreen", 0x00FF7F),
        ("steelblue", 0x4682B4),
        ("tan", 0xD2B48C),
        ("teal", 0x008080),
        ("thistle", 0xD8BFD8),
        ("tomato", 0xFF6347),
        ("turquoise", 0x40E0D0),
        ("violet", 0xEE82EE),
        ("wheat", 0xF5DEB3),
        ("white", 0xFFFFFF),
        ("whitesmoke", 0xF5F5F5),
        ("yellow", 0xFFFF00),
        ("yellowgreen", 0x9ACD32),
];

/// 半透明颜色叠在白底上的不透明颜色。
pub fn over_white(c: u32) -> u32 {
    over(c, 0xFFFF_FFFF)
}

/// WCAG 相对亮度（0–1）。
pub fn luminance(c: u32) -> f64 {
    let lin = |s: u32| {
        let v = f64::from((c >> s) & 0xFF) / 255.0;
        if v <= 0.04045 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) }
    };
    0.2126 * lin(16) + 0.7152 * lin(8) + 0.0722 * lin(0)
}

/// WCAG 对比度。
pub fn contrast(a: u32, b: u32) -> f64 {
    let (la, lb) = (luminance(a), luminance(b));
    (la.max(lb) + 0.05) / (la.min(lb) + 0.05)
}

/// Send to Kindle 的文字对比度规则（2026-10-08 逐个对出来，都是刚到 4.5:1 的那个值）：和背景（没有背景按白页面）的对比度不到 4.5
/// 时，比背景暗的按比例压暗（`#f5ac00` → `#9d6e00`、`#c87860` → `#a86551`，三个通道乘同一个数），比背景亮的往白里调；这个方向
/// 到头也不够就反过来（橙底 `#f0a200` 上的白字 → `#454545`、`#0097e0` 上的白字 → `#292929`、深蓝底 `#0168b7` 上的黑字 → `#e4e4e4`）。
///
/// `bg` 是不透明的背景。`fg` 半透明时按叠在 `bg` 上显示出来的颜色算（以前按叠在白底上算，深背景上的半透明字算错）；要调的话结果是
/// 调好的**不透明**颜色（再带原来的透明度，显示出来又会被背景冲淡，不够 4.5）。不用调的原样返回。
pub fn ensure_contrast(fg: u32, bg: u32) -> u32 {
    const MIN: f64 = 4.5;
    let solid = over(fg, bg);
    if contrast(solid, bg) >= MIN {
        return fg;
    }
    let ch = |c: u32, s: u32| f64::from((c >> s) & 0xFF);
    let make = |f: &dyn Fn(f64) -> f64| 0xFF00_0000 | [16, 8, 0].iter().fold(0, |acc, &s| acc | ((f(ch(solid, s)).round().clamp(0.0, 255.0) as u32) << s));
    let search = |dark: bool| {
        (0..=1000).map(|k| f64::from(k) / 1000.0).map(|t| if dark { make(&|v| v * (1.0 - t)) } else { make(&|v| v + (255.0 - v) * t) }).find(|&c| contrast(c, bg) >= MIN)
    };
    let darker = luminance(solid) <= luminance(bg);
    search(darker).or_else(|| search(!darker)).unwrap_or(fg)
}

/// 颜色 `c`（可以半透明）叠在不透明的 `bg` 上显示出来的不透明颜色。
pub fn over(c: u32, bg: u32) -> u32 {
    let a = f64::from(c >> 24) / 255.0;
    let ch = |s: u32| ((f64::from((c >> s) & 0xFF) * a + f64::from((bg >> s) & 0xFF) * (1.0 - a)).round() as u32).min(255);
    0xFF00_0000 | ch(16) << 16 | ch(8) << 8 | ch(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn semi_transparent_text_judged_over_its_background() {
        // 不透明的照旧（Send to Kindle 对出来的值）
        assert_eq!(ensure_contrast(0xFFF5_AC00, 0xFFFF_FFFF), 0xFF9D_6E00);
        // 深蓝底上的半透明白字：叠上去是浅蓝、不够 4.5，要调；以前按叠在白底上算（纯白），当成够亮不调
        let fg = 0x80FF_FFFF;
        let bg = 0xFF01_68B7;
        assert!(contrast(over(fg, bg), bg) < 4.5);
        let adj = ensure_contrast(fg, bg);
        assert_eq!(adj >> 24, 0xFF, "调出来的是不透明颜色");
        assert!(contrast(adj, bg) >= 4.5);
        // 白底上够深的半透明黑字不动
        assert_eq!(ensure_contrast(0xE600_0000, 0xFFFF_FFFF), 0xE600_0000);
        assert_eq!(over_white(0x8000_0000), over(0x8000_0000, 0xFFFF_FFFF));
    }

    #[test]
    fn rgb_with_unparsable_component_is_not_a_color() {
        assert_eq!(parse_color("rgb(255, 0, 0)"), Some(0xFFFF_0000));
        assert_eq!(parse_color("rgba(0, 0, 0, 0.5)"), Some(0x7F00_0000));
        assert_eq!(parse_color("rgb(100%, 0%, 0%)"), Some(0xFFFF_0000));
        // 以前不认的分量按 0 算：var() 成了黑色、空的透明度成了全透明
        assert_eq!(parse_color("rgb(var(--r), 0, 0)"), None);
        assert_eq!(parse_color("rgba(0, 0, 0, var(--a))"), None);
        assert_eq!(parse_color("rgb(calc(10 + 5), 0, 0)"), None);
        assert_eq!(parse_color("rgba(1, 2, 3, )"), None);
        assert_eq!(parse_color("rgb(inf, 0, 0)"), None);
    }
}
