//! 漫画页：裁边、解码、排版、缩放补白与跨页拆分（拆分自 `imgopt.rs`）。

use image::ImageFormat;
use std::io::Cursor;

use profile::Screen;

use super::decode::*;
use super::page8::*;

/// 预放大的倍数上限：超过就不放大，按图自己的比例尺补白（见 [`comic_layout`]）。
const MAX_UPSCALE: f64 = 3.0;

/// 裁边判定容差：一行/列里像素两两 RGB 通道极差都 ≤ 这个值才算"纯色留白"。留够松（8）容 JPEG 压缩
/// 噪声，但不到能吃掉真实画面渐变的地步。
const TRIM_TOLERANCE: u8 = 8;
/// 单边最多裁掉原图这个比例——防止极端图（比如整页近乎纯色）被误判成"全是留白"裁没内容。
/// 真机《镖人》母版库实测坐实过 0.15 太保守（2026-09-19 反馈"优化没把大量留白裁切完"）：
/// 每卷开头的版权页（CIP 页，中文漫画常见排版）实际留白单边能到 22%-29%，旧阈值在 15% 就强行
/// 停手，裁不干净。抽样 43 张真实页量出的最大值约 28.6%，0.35 留出约 6 个百分点余量；两边独立
/// 累加最多到 0.7×边长，仍留 30% 给内容，不会把整页裁没。
pub(crate) const TRIM_MAX_FRACTION: f32 = 0.35;

/// 留白的亮度下限：一行/列的首像素 R、G、B 都 ≥ 这个值、其余像素与它的差都在 [`TRIM_TOLERANCE`] 以内，才算留白。
///
/// 只裁接近白的边：裁掉的边最后补的是白边（[`Page8::paste_on`] 底色 255；`comic_dark_bg` 的黑底页不裁、只把补边换成底色，见 [`dark_fill`]），黑色、灰色、彩色的边（出血到页边的黑底页、
/// 深色满版画面）裁了再补白就改了原画（2026-10-03 审计：此前只看"纯色"，黑底页的黑边被裁掉换成白边）。
/// 235 的依据：BT.601 有限范围（16–235）里 235 就是标称白，按这个范围出的扫描、视频截图里纸白落在 235 附近。
/// 只卡首像素、其余仍按容差比：贴着画面的那一两行留白有 JPEG 振铃（实测 q90 纸白 241 的边上有 233），这样判定与以前对
/// 白边的结果完全相同，只是多排除了不白的边。泛黄的纸、浅灰底低于它的不裁——拿不准就不处理。
const TRIM_WHITE_MIN: u8 = 235;

/// 裁边认哪种边算留白（[`trim_page`]）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrimMode {
    /// 只裁接近白的纯色边（首像素各通道 ≥ [`TRIM_WHITE_MIN`]）。本 crate 的漫画页都用它：裁掉的边之后补的是白边，
    /// 黑色、彩色的边裁了再补白就改了原画。
    WhiteOnly,
    /// 任何颜色的纯色边都裁（黑、灰、彩色）。给要去掉扫描件黑框之类、且不会再补白边的调用方用；
    /// 出血到页边的黑底页、深色满版画面的边也会被裁掉，用之前想清楚。
    AnyUniform,
}

impl TrimMode {
    /// 首像素各通道的亮度下限。
    fn white_min(self) -> u8 {
        match self {
            TrimMode::WhiteOnly => TRIM_WHITE_MIN,
            TrimMode::AnyUniform => 0,
        }
    }
}

/// 一行/一列像素是否"留白"：首像素各通道都 ≥ `white_min`（[`TrimMode::white_min`]），其余像素与首像素的通道差都 ≤ [`TRIM_TOLERANCE`]。
/// `px(i)` 取该行/列第 i 个像素。
fn line_is_blank(len: u32, white_min: u8, px: impl Fn(u32) -> [u8; 3]) -> bool {
    if len == 0 {
        return true;
    }
    let first = px(0);
    first.iter().all(|&c| c >= white_min)
        && (1..len).all(|i| {
            let p = px(i);
            (0..3).all(|c| p[c].abs_diff(first[c]) <= TRIM_TOLERANCE)
        })
}

/// 四边白色留白的检测：返回 `(left, top, 裁后宽, 裁后高)`；没有可裁的留白 / 图太小 / 会裁成空 → `None`。
/// 只在"确实是留白"时裁——边缘整行/整列像素接近白（[`TRIM_WHITE_MIN`]）且高度一致（[`TRIM_TOLERANCE`]）才算留白，一遇到不满足就停，
/// 不会裁进真实画面。单边最多裁 [`TRIM_MAX_FRACTION`]，兜底极端误判。
/// `px(x, y)` 取像素 RGB（[`Page8::rgb_at`]）。
/// `mode` 为 [`TrimMode::AnyUniform`] 时不看亮度，任何纯色边都算留白。
fn trim_bounds_with(
    w: u32,
    h: u32,
    mode: TrimMode,
    px: impl Fn(u32, u32) -> [u8; 3],
) -> Option<(u32, u32, u32, u32)> {
    if w < 4 || h < 4 {
        return None;
    }
    let max_v = ((h as f32) * TRIM_MAX_FRACTION) as u32;
    let max_h = ((w as f32) * TRIM_MAX_FRACTION) as u32;
    let white_min = mode.white_min();
    let row = |y: u32| line_is_blank(w, white_min, |x| px(x, y));
    let col = |x: u32| line_is_blank(h, white_min, |y| px(x, y));
    let mut top = 0u32;
    while top < max_v && top + 1 < h && row(top) {
        top += 1;
    }
    let mut bottom = 0u32;
    while bottom < max_v && bottom + 1 < h && row(h - 1 - bottom) {
        bottom += 1;
    }
    let mut left = 0u32;
    while left < max_h && left + 1 < w && col(left) {
        left += 1;
    }
    let mut right = 0u32;
    while right < max_h && right + 1 < w && col(w - 1 - right) {
        right += 1;
    }
    if top == 0 && bottom == 0 && left == 0 && right == 0 {
        return None; // 没有可裁的留白
    }
    let (new_w, new_h) = (w - left - right, h - top - bottom);
    if new_w == 0 || new_h == 0 {
        return None;
    }
    Some((left, top, new_w, new_h))
}

/// 解码后的漫画页（[`decode_comic`]）。
pub(crate) struct ComicSrc {
    /// 归一到 8 位灰度/RGB 的整页。
    pub(crate) img: Page8,
    /// 要重新编码时用的格式（[`comic_output_format`]）。GIF/WebP 只在本来就要改像素（裁边、缩放、补白、转灰度）时才换格式，
    /// 光是格式不同不算改动：不需要动的 GIF/WebP 原样保留。
    pub(crate) out_fmt: ImageFormat,
    /// 彩色转成了灰度（黑白屏）：像素已经和原图不同，原字节不能原样沿用。
    pub(crate) to_gray: bool,
    /// 按 EXIF 方向摆正过：原字节靠 EXIF 才显示正，重编码（不带 EXIF）时必须用摆正后的像素；也不原样沿用原字节
    /// （阅读器认不认 EXIF 没验证过，摆正后哪台都一样）。
    pub(crate) rotated: bool,
    /// 原图（转灰度之前）的内容是不是灰度：存成 RGB 的 R=G=B 页算灰度，真正的彩页不算。裁格间空白只动灰度内容；
    /// 黑白屏上彩页先转成了 `Page8::Gray`，不能到那时再判（那一律是灰度）。不需要判的调用方给 `true`（开关关着不用）。
    pub(crate) gray_content: bool,
}

/// 漫画页产物的编码格式：JPEG、PNG 保持原格式；GIF 转 PNG（调色板图，无损）；WebP 看编码方式——有损的转 JPEG，无损的转 PNG。
/// 动图（多帧 GIF、动画 WebP）返回 `None`：只取第一帧会丢内容，原样保留。
fn comic_output_format(fmt: ImageFormat, bytes: &[u8]) -> Option<ImageFormat> {
    match fmt {
        ImageFormat::Jpeg | ImageFormat::Png => Some(fmt),
        ImageFormat::Gif => (gif_frame_count(bytes, 2)? == 1).then_some(ImageFormat::Png),
        ImageFormat::WebP => {
            if image::codecs::webp::WebPDecoder::new(Cursor::new(bytes))
                .ok()?
                .has_animation()
            {
                return None;
            }
            Some(if webp_is_lossless(bytes)? {
                ImageFormat::Png
            } else {
                ImageFormat::Jpeg
            })
        }
        _ => None,
    }
}

/// GIF 里有几帧（数到 `limit` 为止）：按 GIF89a 的块结构跳过去数图像描述符（`0x2C`），不解码像素。
/// 以前用解码器逐帧解出来数，静态页随后还要再整张解一遍。结构不对、截断 → `None`。
pub(crate) fn gif_frame_count(b: &[u8], limit: usize) -> Option<usize> {
    if b.len() < 13 || !(b.starts_with(b"GIF87a") || b.starts_with(b"GIF89a")) {
        return None;
    }
    let table = |packed: u8| {
        if packed & 0x80 != 0 {
            3usize << ((packed & 7) + 1)
        } else {
            0
        }
    };
    // 子块序列：长度字节 + 数据，长度 0 结束
    let skip_sub_blocks = |mut i: usize| -> Option<usize> {
        loop {
            let n = *b.get(i)? as usize;
            i += 1 + n;
            if n == 0 {
                return Some(i);
            }
        }
    };
    let mut i = 13 + table(b[10]);
    let mut frames = 0;
    loop {
        match *b.get(i)? {
            0x21 => i = skip_sub_blocks(i + 2)?,
            0x2C => {
                frames += 1;
                if frames >= limit {
                    return Some(frames);
                }
                let packed = *b.get(i + 9)?;
                i = skip_sub_blocks(i + 10 + table(packed) + 1)?; // 局部色表之后是 LZW 最小码长（1 字节）
            }
            0x3B => return Some(frames),
            _ => return None,
        }
    }
}

/// WebP 的图像数据是不是无损编码（`VP8L` 块）；有损是 `VP8 ` 块。按 RIFF 块顺序找第一个图像块（扩展格式 `VP8X` 在它前面）。
/// 认不出 → `None`。
pub(crate) fn webp_is_lossless(bytes: &[u8]) -> Option<bool> {
    if bytes.len() < 12 || &bytes[..4] != b"RIFF" || &bytes[8..12] != b"WEBP" {
        return None;
    }
    let mut i = 12usize;
    while i + 8 <= bytes.len() {
        let size = u32::from_le_bytes(bytes[i + 4..i + 8].try_into().ok()?) as usize;
        match &bytes[i..i + 4] {
            b"VP8L" => return Some(true),
            b"VP8 " => return Some(false),
            _ => {}
        }
        i = i.checked_add(8)?.checked_add(size)?.checked_add(size & 1)?;
    }
    None
}

/// 解码漫画页并归一到 `Luma8`/`Rgb8`（灰度保持灰度）。[`prepare_comic_page_for_epub`] 的第一步。
/// 用 `into_luma8`/`into_rgb8`：解码结果本来就是 8 位对应类型（几乎所有漫画页）时**不再拷贝整图**，
/// 且不再让"原始解码图 + 归一副本"同时占内存。带透明通道的图（RGBA/LA 的 PNG、带透明色的 GIF）先合成到白底再归一
/// （[`flatten_alpha_on_white`]）——直接丢掉 alpha 会把透明区域变成它底下存的颜色，通常是纯黑。
/// `grayscale`（黑白屏设备）时彩色图顺手转成单通道 8 位灰度（256 级，不抖动）。带 EXIF 方向的先摆正（[`ImgHead`]）。
/// 只合成了白底、别的都不用做时不算改动：原图照旧原样保留，透明区域交给阅读器按页面底色显示。
/// 超过解码上限 `max_px`（缺省 [`MAX_COMIC_DECODE_PIXELS`]）、动图、解不开 → `None`（原样保留）。
/// `apply_exif = false` 时不看 EXIF 方向（图来自不认 EXIF 的容器，见 [`PageDecode::apply_exif`]）。
pub(crate) fn decode_comic(
    bytes: &[u8],
    grayscale: bool,
    max_px: u64,
    apply_exif: bool,
    need_gray_content: bool,
) -> Option<ComicSrc> {
    let head = ImgHead::read(bytes, is_comic_format, apply_exif)?;
    if head.pixels() > max_px {
        return None; // 解压炸弹或离谱的大图：不整个解出来，原样保留（在判动图之前先拦，不为它读整个文件）
    }
    let out_fmt = comic_output_format(head.fmt, bytes)?;
    let decoded = head.decode(bytes, max_px)?;
    let rotated = head.rotated();
    let gray = matches!(
        decoded.color(),
        image::ColorType::L8
            | image::ColorType::L16
            | image::ColorType::La8
            | image::ColorType::La16
    );
    let to_gray = grayscale && !gray;
    let decoded = if decoded.color().has_alpha() {
        flatten_alpha_on_white(decoded)
    } else {
        decoded
    };
    // 彩色要在转灰度之前判（黑白屏转完一律是 Gray，`is_gray_content` 恒真）；只在开了格间空白时才多拷一份判，
    // 转灰度这一步保持原来的写法（16 位图走 `image` 自己的 `into_luma8`，不能先截成 8 位再转，否则关着也变）
    let gray_content = if gray || !need_gray_content {
        true
    } else {
        Page8::Rgb(decoded.to_rgb8()).is_gray_content()
    };
    let img = if gray || to_gray {
        Page8::Gray(into_luma8(decoded))
    } else {
        Page8::Rgb(decoded.into_rgb8())
    };
    Some(ComicSrc {
        img,
        out_fmt,
        to_gray,
        rotated,
        gray_content,
    })
}

/// [`decode_page`] 的选项。
#[derive(Debug, Clone, Copy)]
pub struct PageDecode {
    /// 彩色图转成单通道 8 位灰度（256 级，不抖动）。
    pub grayscale: bool,
    /// 解码上限（像素数，w×h）：超过的图不解码，返回 `None`。漫画页缺省 [`MAX_COMIC_DECODE_PIXELS`]，内存紧的调用方给更小的值
    /// （比如 [`MAX_DECODE_PIXELS`]）。
    pub max_px: u64,
    /// 按 EXIF 方向摆正。图来自不认 EXIF 的容器时（比如 PDF 里嵌的 JPEG：PDF 阅读器按原始像素画，不看 EXIF）传 `false`，
    /// 否则摆正后反而和原来显示的方向不一样。
    pub apply_exif: bool,
}

/// 解码一张整页图（JPEG/PNG/静态 GIF/静态 WebP）并归一到 8 位灰度或 RGB（[`Page8`]）：带透明通道的先合成到白底，
/// 灰度图保持灰度。返回 `(图, 要重新编码时用的格式)`：JPEG、PNG 保持原格式，GIF 转 PNG，WebP 有损转 JPEG、无损转 PNG。
/// 动图、超过 `opts.max_px`、解不开 → `None`。
///
/// 给自己排版整页图的调用方用（本 crate 的漫画页走 [`prepare_comic_page_for_epub`]，内部是同一套解码）：之后接
/// [`trim_page`] 裁边、[`Page8::resize_lanczos3`] 缩放、[`Page8::encode`] 编码。不兜 panic，需要时调用方自己兜。
pub fn decode_page(bytes: &[u8], opts: PageDecode) -> Option<(Page8, ImageFormat)> {
    decode_comic(bytes, opts.grayscale, opts.max_px, opts.apply_exif, false)
        .map(|s| (s.img, s.out_fmt))
}

/// 裁掉四边纯色留白（哪种边算留白见 [`TrimMode`]）：返回 (裁后的图, 左偏移, 上偏移)。没得裁时原图原样返回、偏移 0。
/// 一行/列里像素两两通道差都在 8 以内才算留白，一遇到不满足就停；单边最多裁边长的 35%，兜底极端误判。
/// 按值接收：裁了边时原图在这里就释放，不和裁后的副本同时占内存。
pub fn trim_page(img: Page8, mode: TrimMode) -> (Page8, u32, u32) {
    let (w, h) = img.dimensions();
    match trim_bounds_with(w, h, mode, |x, y| img.rgb_at(x, y)) {
        Some((l, t, cw, ch)) => (img.crop(l, t, cw, ch), l, t),
        None => (img, 0, 0),
    }
}

/// 漫画页在画布上的排版：图缩放成 `nw × nh`，放在 `canvas_w × canvas_h` 的白底画布上，左上角在 (`x`, `y`)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PageLayout {
    pub(crate) nw: u32,
    pub(crate) nh: u32,
    pub(crate) canvas_w: u32,
    pub(crate) canvas_h: u32,
    pub(crate) x: u32,
    pub(crate) y: u32,
}

impl PageLayout {
    /// 原图（裁边前 `ow × oh`，裁掉的左、上留白 `tl`、`tt`，裁后 `cw × ch`）已经是这个排版、只差白边那一两个像素：
    /// 画布就是原图尺寸，图的位置差不超过 `margin + 1` 像素、边长差不超过白边在该方向上对应的量（受限边 2×白边，另一条按
    /// 阅读范围的长短边比例放大）再加 2。这时原字节就是结果——正好是阅读范围大小、画面顶到边的页不为 1px 白边重编码整页；
    /// 已经优化过的书再跑一遍也不会一代代重编码（JPEG 噪声会让 1px 白边裁不干净，所以要有容差）。
    fn matches_original(
        &self,
        (ow, oh): (u32, u32),
        (tl, tt): (u32, u32),
        (cw, ch): (u32, u32),
        area: Screen,
        margin: u32,
    ) -> bool {
        let (long, short) = (
            area.width.max(area.height) as u64,
            area.width.min(area.height).max(1) as u64,
        );
        let size_tol = ((2 * margin as u64 * long).div_ceil(short) + 2) as u32;
        (self.canvas_w, self.canvas_h) == (ow, oh)
            && self.x.abs_diff(tl) <= margin + 1
            && self.y.abs_diff(tt) <= margin + 1
            && self.nw.abs_diff(cw) <= size_tol
            && self.nh.abs_diff(ch) <= size_tol
    }
}

/// `w × h` 保比放进 `bw × bh` 框（放大或缩小）：受限的那条边**正好等于**框，另一条按比例四舍五入（不超过框、至少 1）。
/// 用整数比较宽高比，受限边不受浮点误差影响。
fn fit_box(w: u32, h: u32, bw: u32, bh: u32) -> (u32, u32) {
    let (w, h, bw, bh) = (w as u64, h as u64, bw as u64, bh as u64);
    if w * bh >= h * bw {
        (bw as u32, ((h * bw + w / 2) / w).clamp(1, bh) as u32)
    } else {
        (((w * bh + h / 2) / h).clamp(1, bw) as u32, bh as u32)
    }
}

/// 实际用的白边：profile 已校验小于阅读范围短边的 1/4，这里只防调用方直接传进离谱的值。
fn effective_margin(margin: u32, area: Screen) -> u32 {
    margin.min(area.width.min(area.height) / 4)
}

/// 漫画页的排版（**只算尺寸，不碰像素**）：裁边后的图 `w × h`、阅读范围 `area`、白边 `margin` 像素。
///
/// - **常规**：图保比放进 `(W − 2m) × (H − 2m)` 的框（[`fit_box`]），居中放在 `W × H` 白底画布上。受限的那条边两侧正好
///   各 `m` 像素，另一条边两侧更多（差奇数时右、下多 1px）；宽高比不变，不拉伸不压扁。比框大的缩小；比框小的 JPEG
///   **放大**（`may_upscale`，倍数不超过 [`MAX_UPSCALE`]）：画布是阅读范围大小，图按原尺寸放就只占中间一小块；
///   预先放大后铺满画布，不依赖阅读器会不会、怎么放大图片（各阅读器放不放大没逐一验证）。
/// - **不放大**（PNG 等无损格式、或要放大超过 [`MAX_UPSCALE`] 倍）：图保持原尺寸，画布按图自己的比例尺补到阅读范围的
///   宽高比，白边按比例缩小（至少 1px，`margin = 0` 时为 0）——阅读器把整页放大到屏幕后，受限边的白边仍约 `m` 像素。
pub(crate) fn comic_layout(w: u32, h: u32, area: Screen, margin: u32, may_upscale: bool) -> PageLayout {
    let (aw, ah) = (area.width, area.height);
    let m = effective_margin(margin, area);
    let (bw, bh) = (aw - 2 * m, ah - 2 * m);
    let (nw, nh) = fit_box(w, h, bw, bh);
    let upscale = nw > w || nh > h;
    if !upscale || (may_upscale && nw as f64 / w as f64 <= MAX_UPSCALE) {
        return PageLayout {
            nw,
            nh,
            canvas_w: aw,
            canvas_h: ah,
            x: (aw - nw) / 2,
            y: (ah - nh) / 2,
        };
    }
    // 不放大：受限边（放大到框时先顶满的那条）按比例缩小白边，另一条按阅读范围的宽高比算，至少容得下图和白边。
    let width_bound = (w as u64) * (bh as u64) >= (h as u64) * (bw as u64);
    let (along, box_along) = if width_bound { (w, bw) } else { (h, bh) };
    let mm = if m == 0 {
        0
    } else {
        (((m as u64) * (along as u64) + box_along as u64 / 2) / box_along as u64).max(1) as u32
    };
    let (canvas_w, canvas_h) = if width_bound {
        let cw = w + 2 * mm;
        (
            cw,
            ((((cw as u64) * (ah as u64) + aw as u64 / 2) / aw as u64) as u32).max(h + 2 * mm),
        )
    } else {
        let ch = h + 2 * mm;
        (
            ((((ch as u64) * (aw as u64) + ah as u64 / 2) / ah as u64) as u32).max(w + 2 * mm),
            ch,
        )
    };
    PageLayout {
        nw: w,
        nh: h,
        canvas_w,
        canvas_h,
        x: (canvas_w - w) / 2,
        y: (canvas_h - h) / 2,
    }
}

/// **EPUB 漫画整页的单趟处理**：解码一次 → 裁白边 → 按 [`comic_layout`] 缩放（缩小，或 JPEG 小图放大）→ 居中放上白底画布 →
/// 编码一次（JPEG 质量 [`JPEG_QUALITY_COMIC`]，灰度保持单分量）。
///
/// - `area` 是 profile 的真实可阅读范围，`margin` 是 profile 的 `comic_margin`：产物画布正好是 `area` 大小，
///   图到四边的距离在受限的那条边上正好是 `margin` 像素（另一条边更多），宽高比不变。
/// - `grayscale`（黑白屏设备）时彩色页转成单分量 8 位灰度（256 级，不抖动，2026-09-27 定）。
/// - 静态 GIF 转 PNG、WebP 转 JPEG/PNG（[`comic_output_format`]）；动图原样保留。
/// - 短边不到阅读范围宽度 1/3 的装饰小图只裁边（和换格式、转灰度），不缩放、不补白。
/// - 已经排好的页（和目标排版相差不超过 1px，见 [`PageLayout::matches_original`]）、没有别的要改时返回 `None`（原字节零损失）。
///   带 EXIF 方向（非"不用转"）的页一律摆正后重编码。
/// - 超过 [`MAX_COMIC_DECODE_PIXELS`] 的图、解不开的图返回 `None`。
pub fn prepare_comic_page_for_epub(
    bytes: &[u8],
    area: Screen,
    margin: u32,
    grayscale: bool,
) -> Option<Vec<u8>> {
    prepare_comic_page_limited(
        bytes,
        area,
        margin,
        grayscale,
        ComicPage::default(),
        MAX_COMIC_DECODE_PIXELS,
    )
}

/// 漫画页除了排版以外的处理（profile 里 `comic_` 开头的几项；都不开时和以前逐字节相同）。
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct ComicPage {
    /// 中间调加深的伽马（`comic_gamma`）。
    pub gamma: Option<f64>,
    /// 拉满对比度（`comic_autocontrast`）。
    pub autocontrast: bool,
    /// 裁白边不让杂点挡住（`comic_trim_specks`，[`crate::comiccrop`]）。
    pub trim_specks: bool,
    /// 比框小的 JPEG 页不放大（profile `comic_upscale = false`）：和 PNG 一样保持原尺寸、按比例补白，交给阅读器放大到屏幕。
    pub no_upscale: bool,
    /// 页码：从这一行（解码后整页的坐标）往下切掉（`comic_page_numbers`，整卷投票定的，见 [`crate::comiccrop::page_number_cuts`]）。
    pub cut_bottom: Option<u32>,
    /// 黑底页补边补底色、不裁（`comic_dark_bg`，[`crate::comiccrop::dark_background`]）。
    pub dark_bg: bool,
    /// 裁掉格与格之间整行的空白带（profile `comic_gutter_crop`，[`crate::comiccrop::gutter_bands`]，2026-11-10，待真机比）。
    pub gutter_crop: bool,
}

/// 裁白边（[`trim_page`]，`specks` 时再按 [`crate::comiccrop`] 去掉杂点收一次）：返回 (裁后的图, 左偏移, 上偏移)。`full` 是整页宽高。
fn trim_comic(img: Page8, specks: bool, full: (u32, u32), off0: (u32, u32)) -> (Page8, u32, u32) {
    let (img, l, t) = trim_page(img, TrimMode::WhiteOnly);
    if !specks {
        return (img, l, t);
    }
    let (w, h) = img.dimensions();
    let b =
        crate::comiccrop::speck_bounds(&darkness_plane(&img), w, h, full, (off0.0 + l, off0.1 + t));
    match b {
        Some((x, y, cw, ch)) => (img.crop(x, y, cw, ch), l + x, t + y),
        None => (img, l, t),
    }
}

/// 整卷投票前的一页（`comic_page_numbers`）：解码（同 [`prepare_comic_page_limited`]，摆正 EXIF 方向、灰度），裁白边（`specks` 同处理时），
/// 在内容最底下找像页码的块（[`crate::comiccrop`]）。装饰小图（短边不到 `area` 宽 1/3）、解不开、超过上限的 → `None`。
pub fn comic_page_number(
    bytes: &[u8],
    area: Screen,
    specks: bool,
    max_px: u64,
) -> Option<crate::comiccrop::PageNumber> {
    let ComicSrc { img, .. } = decode_comic(bytes, true, max_px, true, false)?;
    let full = img.dimensions();
    let (img, l, t) = trim_comic(img, specks, full, (0, 0));
    let (w, h) = img.dimensions();
    if w.min(h) < area.width / 3 {
        return None;
    }
    crate::comiccrop::page_number_candidate(&darkness_plane(&img), w, h, (l, t), full)
}

/// 同 [`prepare_comic_page_for_epub`]，另给解码上限 `max_px`（像素，`OptimizeOpts::limits.max_decode_pixels`）：
/// 超过的页不解码、原样保留。`gamma`（profile `comic_gamma`）：内容是灰度的页（[`Page8::is_gray_content`]；黑白屏上转成灰度的页
/// 都算）缩放后按 [`gamma_lut`] 加深中间调；`autocontrast`（profile `comic_autocontrast`）：同样的页先按裁边后、缩放前整页最黑、最白的值
/// 拉满对比度（[`autocontrast_lut`]），和伽马合成一张表、缩放后查一次。装饰小图都不动。`page` 里另有页码裁切、杂点（[`ComicPage`]）。
/// 都不开时和以前逐字节相同。
pub fn prepare_comic_page_limited(
    bytes: &[u8],
    area: Screen,
    margin: u32,
    grayscale: bool,
    page: ComicPage,
    max_px: u64,
) -> Option<Vec<u8>> {
    let src = decode_comic(bytes, grayscale, max_px, true, page.gutter_crop)?;
    process_comic_page(src, area, margin, page, false)
}

/// 跨页判定：摆正 EXIF 后宽 / 高 ≥ 这个值（2026-11-10 vinci 确认）。单页约 0.65–0.75，跨页约 1.3–1.5；
/// 1.0–1.2 的宽页不拆，照旧整张缩放。
pub const SPREAD_MIN_ASPECT: f32 = 1.2;

/// 宽 / 高是不是达到跨页阈值（[`SPREAD_MIN_ASPECT`]）。`w`、`h` 是摆正 EXIF 后的尺寸。
pub fn is_spread(w: u32, h: u32) -> bool {
    // 用整数比 6/5 判 1.2（f32 的 1.2 是 1.20000004768，正好 1200×1000 会被判成不到）
    h > 0 && u64::from(w) * 5 >= u64::from(h) * 6
}

/// 跨页正中竖线切开：左半宽 `w/2`（向下取整），右半宽 `w - w/2`。
pub(crate) fn spread_widths(w: u32) -> (u32, u32) {
    (w / 2, w - w / 2)
}

/// 横向跨页拆成两张竖页，左右各自走完整的漫画页处理（[`prepare_comic_page_limited`] 同一套：裁边、杂点、拉对比度、
/// 伽马、黑底、缩放补白、格式），返回（左半、右半）的最终字节。正中竖线切：左半宽 `w/2`（向下取整），右半宽 `w - w/2`。
/// 不满足跨页、动图、超上限、解不开、编码失败 → `None`（原图照旧整张处理）。两半的页码裁切都是 `None`（不参加投票）。
pub fn prepare_comic_spread(
    bytes: &[u8],
    area: Screen,
    margin: u32,
    grayscale: bool,
    page: ComicPage,
    max_px: u64,
) -> Option<(Vec<u8>, Vec<u8>)> {
    let src = decode_comic(bytes, grayscale, max_px, true, page.gutter_crop)?;
    let (w, h) = src.img.dimensions();
    if !is_spread(w, h) {
        return None;
    }
    let (left_w, right_w) = spread_widths(w);
    let (out_fmt, to_gray, rotated, gray_content) =
        (src.out_fmt, src.to_gray, src.rotated, src.gray_content);
    // 一次只多留一张半页：左半处理完再裁右半，原图一直留到最后
    let left = src.img.crop_ref(0, 0, left_w, h);
    let left = process_comic_page(
        ComicSrc {
            img: left,
            out_fmt,
            to_gray,
            rotated,
            gray_content,
        },
        area,
        margin,
        ComicPage {
            cut_bottom: None,
            ..page
        },
        true,
    )?;
    let right = src.img.crop_ref(left_w, 0, right_w, h);
    let right = process_comic_page(
        ComicSrc {
            img: right,
            out_fmt,
            to_gray,
            rotated,
            gray_content,
        },
        area,
        margin,
        ComicPage {
            cut_bottom: None,
            ..page
        },
        true,
    )?;
    Some((left, right))
}

/// 已解码的一页 → 最终字节。`force_encode`：即使排版已经和原图一致也要编码（拆出来的半页必须拿出新字节）。
fn process_comic_page(
    src: ComicSrc,
    area: Screen,
    margin: u32,
    page: ComicPage,
    force_encode: bool,
) -> Option<Vec<u8>> {
    let ComicPage {
        gamma,
        autocontrast,
        trim_specks,
        no_upscale,
        cut_bottom,
        dark_bg,
        gutter_crop,
    } = page;
    let ComicSrc {
        img,
        out_fmt,
        to_gray,
        rotated,
        gray_content,
    } = src;
    let orig = img.dimensions();
    // 页码：从页码块的上沿往下切掉，再照常裁白边（切口上方的留白一起裁掉）
    let img = match cut_bottom.filter(|&c| c > 0 && c < orig.1) {
        Some(c) => img.crop(0, 0, orig.0, c),
        None => img,
    };
    let (img, tl, tt) = trim_comic(img, trim_specks, orig, (0, 0));
    // 黑底页只换补边颜色：不按底色裁、不重新放大，缩放和位置都和不开时一样（2026-11-01 真机看过，裁了的出血画构图坏了）
    let bg = if dark_bg { dark_fill(&img) } else { None };
    let (cw, ch) = img.dimensions();
    let trimmed = (cw, ch) != orig;
    if cw.min(ch) < area.width / 3 {
        // 装饰小图：只裁边
        return if trimmed || to_gray || rotated || force_encode {
            img.encode(out_fmt, JPEG_QUALITY_COMIC)
        } else {
            None
        };
    }
    // 裁格间空白（只在裁白边、黑底判定、装饰小图之后；黑底页、彩色页不动）。
    // 彩色按转灰度之前的原图判（`gray_content`）：黑白屏上彩页转成 Gray 后 `is_gray_content` 恒真。
    let mut gutter_changed = false;
    let img = if gutter_crop && bg.is_none() && gray_content {
        let (w, h) = img.dimensions();
        let plane = darkness_plane(&img);
        let bands = crate::comiccrop::gutter_bands(&plane, w, h);
        let cuts = crate::comiccrop::gutter_cuts(&bands, w, h, area);
        if cuts.is_empty() {
            img
        } else {
            gutter_changed = true;
            img.remove_rows(&cuts)
        }
    } else {
        img
    };
    let (cw, ch) = img.dimensions();
    // 黑点、白点要在缩放前取：Lanczos 在线条边上振出 0，缩放后取就拉不动（2026-10-30 测试书第一版这样错过）
    let darken = if (gamma.is_some() || autocontrast) && img.is_gray_content() {
        let stretch = autocontrast
            .then(|| autocontrast_lut(img.extrema()))
            .flatten();
        let curve = gamma.map(gamma_lut);
        match (stretch, curve) {
            (Some(s), Some(g)) => Some(std::array::from_fn(|v| g[s[v] as usize])),
            (s, g) => s.or(g),
        }
    } else {
        None
    };
    let lay = comic_layout(
        cw,
        ch,
        area,
        margin,
        out_fmt == ImageFormat::Jpeg && !no_upscale,
    );
    if !force_encode
        && !gutter_changed
        && !to_gray
        && !rotated
        && darken.is_none()
        && lay.matches_original(
            orig,
            (tl, tt),
            (cw, ch),
            area,
            effective_margin(margin, area),
        )
    {
        return None;
    }
    let mut img = if (lay.nw, lay.nh) != (cw, ch) {
        img.resize_lanczos3(lay.nw, lay.nh)
    } else {
        img
    };
    // 补白之前加深：白底 255 加深后还是 255，先后一样，补白前像素少
    if let Some(lut) = &darken {
        img.apply_lut(lut);
    }
    // 黑底页补底色（和画面里的底色同样过一遍加深表，接得上）；其余补白
    let fill = bg.map_or(255, |b| darken.as_ref().map_or(b, |lut| lut[b as usize]));
    let page = if (lay.canvas_w, lay.canvas_h) != (lay.nw, lay.nh) {
        img.paste_on(lay.canvas_w, lay.canvas_h, lay.x, lay.y, fill)
    } else {
        img
    };
    page.encode(out_fmt, JPEG_QUALITY_COMIC)
}

/// 黑底页（`comic_dark_bg`）：内容是灰度、四边一圈是均匀的黑 / 深灰（[`crate::comiccrop::dark_background`]）的页返回底色，
/// 补边补它（不补白）；不是黑底页 → `None`。只判、不裁：按底色裁过一版（c61），竹光侍的出血画被裁坏了构图、迷宮飯效果也差。
fn dark_fill(img: &Page8) -> Option<u8> {
    if !img.is_gray_content() {
        return None;
    }
    let (w, h) = img.dimensions();
    let plane: std::borrow::Cow<'_, [u8]> = match img {
        Page8::Gray(g) => std::borrow::Cow::Borrowed(g.as_raw()),
        Page8::Rgb(c) => std::borrow::Cow::Owned(
            c.as_raw()
                .as_chunks::<3>()
                .0
                .iter()
                .map(|p| p[0].max(p[1]).max(p[2]))
                .collect(),
        ),
    };
    crate::comiccrop::dark_background(&plane, w, h)
}

/// 中间调加深的查找表：`v ↦ round(255·(v/255)^γ)`，0 和 255 不动，γ > 1 时中间灰度变深（profile `comic_gamma`、`image_gamma`；
/// 2026-10-16 Move、掌阅真机比过，黑白漫画、灰度插图上明显更好）。
pub fn gamma_lut(gamma: f64) -> [u8; 256] {
    std::array::from_fn(|v| (255.0 * (v as f64 / 255.0).powf(gamma)).round() as u8)
}

/// [`autocontrast_lut`]：整页最黑到最白不到这么多的页不拉（有意的低对比度，同 KCC：`255 − 32×3`）。
pub const MIN_AUTOCONTRAST_RANGE: u8 = 159;

/// 拉满对比度的查找表（照 KCC 的自动对比度，PIL `ImageOps.autocontrast` 不截尾）：`lo ↦ 0`、`hi ↦ 255`，中间线性、四舍五入，两端外的钳住
/// （PIL 截断取整，浮点误差下白 255 常落成 254，和补的白边接不上；2026-10-30 真机比过的测试书也是四舍五入）。`(lo, hi)` 是整页所有分量的最小、最大值（[`Page8::extrema`]）；范围不到 [`MIN_AUTOCONTRAST_RANGE`]、或已经是 0–255 的
/// 返回 `None`（不拉）。2026-10-30 Kindle 真机比过（《死亡筆記》卷07），维护者选拉满 + 伽马 1.1。
pub fn autocontrast_lut((lo, hi): (u8, u8)) -> Option<[u8; 256]> {
    if hi < lo || hi - lo < MIN_AUTOCONTRAST_RANGE || (lo, hi) == (0, 255) {
        return None;
    }
    let scale = 255.0 / f64::from(hi - lo);
    Some(std::array::from_fn(|v| {
        ((v as f64 - f64::from(lo)) * scale)
            .round()
            .clamp(0.0, 255.0) as u8
    }))
}
