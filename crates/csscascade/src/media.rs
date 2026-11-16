//! `@media` 求值。

use crate::text::{matching_close, split_top};
use crate::value::split_number;

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
        MediaEnv {
            width: r.width.into(),
            height: r.height.into(),
            device_width: p.screen.width.into(),
            device_height: p.screen.height.into(),
            color: p.color,
        }
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
    q.is_empty()
        || split_top(q, ',')
            .into_iter()
            .any(|one| media_query(one, env) == Some(true))
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
            i = matching_close(
                q[start..].char_indices().map(|(k, c)| (start + k, c)),
                '(',
                ')',
            )?;
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
        "device-aspect-ratio" => Some(cmp_num(
            env.device_width / env.device_height,
            ratio(value?)?,
        )),
        "orientation" if cmp == std::cmp::Ordering::Equal => match value? {
            "portrait" => Some(env.height >= env.width),
            "landscape" => Some(env.width > env.height),
            _ => None,
        },
        // 颜色位数只知道是不是 0：黑白屏 color 是 0，彩色屏 monochrome 是 0；别的位数不知道 → 求值不了
        "color" | "monochrome" => {
            let zero = if base == "color" {
                !env.color
            } else {
                env.color
            };
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
