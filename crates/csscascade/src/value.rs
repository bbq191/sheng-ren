//! 长度与设备静态量。

// ---------------------------------------------------------------- 值

/// 长度。`Em` 相对元素自己的字号。
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Len {
    Em(f64),
    Percent(f64),
    Pt(f64),
}

impl Len {
    /// 数值是 0。
    pub fn is_zero(&self) -> bool {
        matches!(self, Len::Em(n) | Len::Percent(n) | Len::Pt(n) if *n == 0.0)
    }

    /// 数值乘 `k`（单位不变）。
    pub fn scaled(self, k: f64) -> Len {
        match self {
            Len::Em(n) => Len::Em(n * k),
            Len::Percent(n) => Len::Percent(n * k),
            Len::Pt(n) => Len::Pt(n * k),
        }
    }
}

/// 切出开头的数字和后面的单位（`1.5em` → (1.5, "em")）；数字解析不了返回 `None`。
pub(crate) fn split_number(v: &str) -> Option<(f64, &str)> {
    let end = v
        .find(|c: char| !(c.is_ascii_digit() || c == '.' || c == '-' || c == '+'))
        .unwrap_or(v.len());
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
pub(crate) const PT_PER_BOX_PX: f64 = 0.45;

/// 按阅读器排出来的样子算（`epub-to-kfx --styles --device=掌阅或 Move`，`tools/kfx/unify.py` 用）时，这台 1em 的屏幕像素：
/// 掌阅、Move 上盒子长度的 px 是屏幕像素（怪癖测试书 Q10、Q48），[`parse_box_len`] 按它折。`None`（缺省）照 Send to Kindle 的
/// 0.45pt 一 px。进程级的开关，只给分析命令用。
static DEVICE_PX_EM: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

pub fn set_device_px_em(em_px: Option<f64>) {
    DEVICE_PX_EM.store(
        em_px.map_or(0, f64::to_bits),
        std::sync::atomic::Ordering::Relaxed,
    );
}

/// 按阅读器排出来的样子算时，这台的行距怎么排（进程级的开关，只给分析命令用；写出器按 Send to Kindle，不看它）：
/// - `except_p`：`<p>` 不认外链、`<style>` 里的 `line-height` 的阅读器（Move，profile `line_height_except_p`）：`<p>` 只认行内
///   `style` 写的、没写是阅读器缺省行距，别的块按「缺省行距 × 倍数」排，`normal` 和没写都是 1 倍（怪癖测试书 Q83、「测试-行距」）；
/// - `length_em`：只认长度的阅读器（掌阅，profile `line_height_numbers = false`）缺省行距是 em 的几倍：不带单位的数、`normal`
///   整条当没写（跟父元素），全书缺省是这个倍数、随字号（怪癖测试书 Q80–Q84，同 `wash::block_style` 的模型）。
/// - `em_root`：em 一律按根字号算的阅读器（Move，profile `em_root`）：字号的 em、% 当 rem，边距、首行缩进的 em 当根 em。
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct DeviceLine {
    pub except_p: bool,
    pub length_em: Option<f64>,
    pub em_root: bool,
}

static DEVICE_LINE: std::sync::Mutex<DeviceLine> = std::sync::Mutex::new(DeviceLine {
    except_p: false,
    length_em: None,
    em_root: false,
});

pub fn set_device_line(m: DeviceLine) {
    *DEVICE_LINE.lock().unwrap_or_else(|e| e.into_inner()) = m;
}

pub fn device_line() -> DeviceLine {
    *DEVICE_LINE.lock().unwrap_or_else(|e| e.into_inner())
}

pub fn device_px_em() -> Option<f64> {
    let b = DEVICE_PX_EM.load(std::sync::atomic::Ordering::Relaxed);
    (b != 0).then(|| f64::from_bits(b))
}

/// 外边距、内边距的长度：同 [`parse_len`]，但 px 按 1px＝0.45pt 换算（Send to Kindle 的口径，和边框宽度一样：《绍宋》
/// `margin:-10px` 写成 -0.3125lh、`margin-top:-12.5px` 在半号字上写成 -1.172lh；以前按 CSS 的 1px＝0.75pt，大了 2/3）。字号不走这里。
pub fn parse_box_len(v: &str) -> Option<Len> {
    let t = v.trim().to_ascii_lowercase();
    match t
        .strip_suffix("px")
        .and_then(|n| n.trim().parse::<f64>().ok())
    {
        Some(n) => Some(match device_px_em() {
            Some(em) => Len::Pt(n / em * PT_PER_EM),
            None => Len::Pt(n * PT_PER_BOX_PX),
        }),
        None => parse_len(&t),
    }
}
