//! 优化器图片降采样——按目标设备屏幕（[`Screen`]，来自 `profile` crate）做设备级优化。
//!
//! 书里常带 2000–4000px 的高清原图，超出屏幕的像素**纯属浪费**：拖慢加载、吃内存、还逼阅读器在渲染期
//! 临时缩放（慢且质量不可控）。优化器在组包/优化阶段把超大图 Lanczos3 预缩进屏幕框，缩放质量我们控。
//!
//! 纪律：**只缩不放、保宽高比、保原格式、达标即跳过（幂等 + 免二次编码损失）、任何失败原样保留**
//! （绝不因优化损坏原书）。文字书插图只碰 JPEG/PNG（书内图几乎都是；GIF 可能动图，跳过不冒险）。
//! 漫画页另有一套（[`prepare_comic_page_for_epub`]）：按阅读范围放大或缩小、四边留 `comic_margin` 像素白边；
//! 静态的 GIF/WebP 页也处理（转成 PNG/JPEG）。

use image::codecs::jpeg::JpegEncoder;
use image::imageops::FilterType;
use image::ImageFormat;
use std::io::Cursor;

pub use profile::Screen;

/// 单测用的设备屏幕：rMPP Move（954×1696）。历史用例与真机数据都是在 Move 上量的，断言里的数字以它为准。
#[cfg(test)]
pub(crate) fn test_screen() -> Screen {
    profile::get("xochitl").expect("内置 xochitl profile").screen
}

/// 单张图片允许解码的像素数上限（w×h，跟格式/用途无关）——防极端高分辨率原图解码成未压缩位图把内存顶爆。
///
/// **阈值按实测定，不按"3 字节/像素 RGB8"估算**：release 构建下量整页处理（解码 → 裁边 → 缩放 → 编码）的
/// 峰值常驻内存（`/proc/<pid>/status` 的 `VmHWM`）：400 万像素→62MB、870 万像素（A4 300dpi）→97–109MB、
/// 1600 万像素→164MB、2500 万像素→230–236MB，约 9–16MB/百万像素，远高于理论的 3MB/百万像素
/// （`image` 库解码、类型转换、缩放的中间缓冲同时存活）。900 万像素（约 3000×3000，覆盖 A4 300dpi 与绝大多数
/// 真实漫画/书籍扫描页）单张峰值约 100–110MB。超限的图直接放弃处理、原样保留原图字节——调用方对返回 `None`
/// 本来就是"原样保留"语义。并行时的总量另由 [`crate::imgpool::PIXEL_BUDGET`] 约束。
/// 只管文字书插图（[`downscale_for_epub`] 等）；漫画页的上限是 [`MAX_COMIC_DECODE_PIXELS`]。
pub const MAX_DECODE_PIXELS: u64 = 9_000_000;

/// 文字书插图能不能解码：不超过 [`MAX_DECODE_PIXELS`]，也不超过调用方给的解码上限 `max_px`
/// （`OptimizeOpts::limits.max_decode_pixels`，缺省 [`MAX_COMIC_DECODE_PIXELS`]，比 900 万大，等于不起作用）。
fn within_decode_budget(w: u32, h: u32, max_px: u64) -> bool {
    (w as u64) * (h as u64) <= MAX_DECODE_PIXELS.min(max_px)
}

/// 漫画页单张允许解码的像素数上限：只防解压炸弹（几 KB 的文件声明几亿像素），真实的高分辨率扫描页照常处理。
/// 6400 万像素约是 A4 600dpi 双页跨页（约 7000×9900 的一半再大一些）；按实测约 9–16MB/百万像素，最坏单张峰值约 1GB。
/// 超过 [`crate::imgpool::PIXEL_BUDGET`] 的大页开工时独占全部额度，不与别的图同时处理，所以总内存仍有上界。
/// 2026-09-29 之前漫画页也用 [`MAX_DECODE_PIXELS`]，超过 900 万像素的页原样保留（没缩放、没补白、没转灰度）。
pub const MAX_COMIC_DECODE_PIXELS: u64 = 64_000_000;

/// 重编码 JPEG 质量（0–100）。85 = 视觉无损级，体积/画质平衡；e-ink 上更看不出差异。
const JPEG_QUALITY: u8 = 85;
/// 漫画页重编码质量（缩小、放大、补白都用它）——漫画不压画质：95 接近视觉无损。
/// 2026-09-29 之前预放大的页用 85（怕体积暴涨），用户定：漫画一律 95。
const JPEG_QUALITY_COMIC: u8 = 95;
/// 预放大的倍数上限：超过就不放大，按图自己的比例尺补白（见 [`comic_layout`]）。
const MAX_UPSCALE: f64 = 3.0;

/// 保比缩进 `max_w × max_h` 框（宽高比保持、保原格式，JPEG 质量 [`JPEG_QUALITY`]），只在超框时动；返回新字节或 `None`
/// （已达标 / 非 JPEG·PNG / 解码失败 / 重编码没变小 → 调用方原样保留）。
/// 带 EXIF 方向的图按**摆正后**的宽高判断超不超框；要重编码时先摆正（重编码不带 EXIF，不摆正就转歪了，见 [`decode_oriented`]）。
fn downscale_into(bytes: &[u8], max_w: u32, max_h: u32, max_px: u64) -> Option<Vec<u8>> {
    let (fmt, (w, h)) = header_dims(bytes)?;
    let orientation = orientation_of(bytes, fmt);
    let (w, h) = if swaps_axes(orientation) { (h, w) } else { (w, h) };
    if w <= max_w && h <= max_h {
        return None; // 已达标：不解码不重编码（避免无谓的二次有损压缩；2473 页漫画只读头是秒级、全解是分钟级）
    }
    if !within_decode_budget(w, h, max_px) {
        return None; // 极端高分辨率原图：不整个解出来，原样保留（见 MAX_DECODE_PIXELS 文档）
    }
    let mut img = image::load_from_memory_with_format(bytes, fmt).ok()?;
    img.apply_orientation(orientation);
    // 缩放走 SIMD 版 Lanczos3（`resize_lanczos3`，同漫画页；尺寸算法与 `DynamicImage::resize` 相同），编码时灰度保持单分量。
    // 此前 `img.resize` 是 `image` 自带的标量实现（实测慢约 20 倍，且中间缓冲是 f32，内存大），`encode_image` 还会把
    // 灰度图写成 3 分量 JPEG（体积白涨）——文字书插图每张都走这里（2026-09-25 审计）。非 L8/RGB8 类型（带透明 PNG 等）照旧。
    let (nw, nh) = fit_within(w, h, max_w, max_h);
    let out = match Page8::try_from_dynamic(img) {
        Ok(page) => page.resize_lanczos3(nw, nh).encode(fmt, JPEG_QUALITY)?,
        // 带透明、16 位等其它类型（只有 PNG 会到这里）：`image` 自带缩放，类型不变、原样写 PNG；JPEG 不收这种类型
        Err(img) => {
            let resized = img.resize_exact(nw, nh, FilterType::Lanczos3);
            drop(img);
            if fmt != ImageFormat::Png {
                return None;
            }
            let mut out = Vec::new();
            resized.write_to(&mut Cursor::new(&mut out), ImageFormat::Png).ok()?;
            out
        }
    };
    // 只有确实变小才采用（极端下重编码可能变大 → 保留原图，不倒退体积）。
    (out.len() < bytes.len()).then_some(out)
}

/// 保比放进 `max_w × max_h` 框后的尺寸（与 `image` 库 `DynamicImage::resize` 的 `resize_dimensions` 同一算法：取宽高两个比例的
/// 较小者、四舍五入、至少 1）。只用于缩小，结果不会超过原尺寸。
fn fit_within(w: u32, h: u32, max_w: u32, max_h: u32) -> (u32, u32) {
    let ratio = f64::min(max_w as f64 / w as f64, max_h as f64 / h as f64);
    (((w as f64 * ratio).round() as u32).max(1), ((h as f64 * ratio).round() as u32).max(1))
}

/// **CBZ/漫画整页**降采样：按朝向选盒（竖 短边×长边 / 横 长边×短边），页整张填屏、横页横读可用长边宽。
/// 真机探针（2026-09-02，Move，5 张 400–2400px 宽图上机看渲染的 `<uuid>.pdf`）：只卡长边会让方图多留 1.8× 无用像素。
/// 横竖按**摆正后**（EXIF 方向）的宽高判断：存成横的、靠 EXIF 转 90° 显示的竖图要进竖框。
pub fn downscale_for_device(bytes: &[u8], screen: Screen) -> Option<Vec<u8>> {
    let (fmt, (w, h)) = header_dims(bytes)?;
    let (w, h) = if swaps_axes(orientation_of(bytes, fmt)) { (h, w) } else { (w, h) };
    let (long, short) = (screen.long_edge(), screen.short_edge());
    let (max_w, max_h) = if w >= h { (long, short) } else { (short, long) };
    downscale_into(bytes, max_w, max_h, MAX_COMIC_DECODE_PIXELS)
}

/// 图片里 EXIF 的方向标签（JPEG、PNG 的 eXIf、WebP 的 EXIF 块）；没有或读不出 → 不用转。只读文件头，不解码像素。
fn orientation_of(bytes: &[u8], fmt: ImageFormat) -> image::metadata::Orientation {
    use image::ImageDecoder;
    image::ImageReader::with_format(Cursor::new(bytes), fmt)
        .into_decoder()
        .and_then(|mut d| d.orientation())
        .unwrap_or(image::metadata::Orientation::NoTransforms)
}

/// 这个方向摆正时宽高互换（转 90°/270° 的四种）。
fn swaps_axes(o: image::metadata::Orientation) -> bool {
    use image::metadata::Orientation::*;
    matches!(o, Rotate90 | Rotate270 | Rotate90FlipH | Rotate270FlipH)
}

/// 跑一段处理外来图片的代码，兜住解码器的 panic，按处理失败（`None`）算。书里、网上的图都是外部输入，第三方解码器遇到畸形
/// 图片偶尔会 panic：不兜住的话一张坏图会摔掉整本书的优化（图片 worker 的 panic 经 `thread::scope` 重新抛给调用方；
/// 抓远程图、换封面在主线程，直接 panic）。优化器处理图片、读图片头、抓远程图、解封面共用。
pub(crate) fn guard<T>(f: impl FnOnce() -> Option<T>) -> Option<T> {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)).unwrap_or(None)
}

/// 解码并按 EXIF 方向摆正（格式按魔数认）。重编码写不回 EXIF：像素不摆正的话，在按 EXIF 显示的阅读器上
/// 原来正的图就转歪了；摆正后像素本身就是该显示的样子，认不认 EXIF 的阅读器看到的都一样。缩略图等只读用途也用它。
/// 解不开（含解码器 panic，见 [`guard`]）→ `None`。
pub fn decode_oriented(bytes: &[u8]) -> Option<image::DynamicImage> {
    guard(|| {
        let fmt = crate::util::image_kind(bytes)?.format;
        let mut img = image::load_from_memory_with_format(bytes, fmt).ok()?;
        img.apply_orientation(orientation_of(bytes, fmt));
        Some(img)
    })
}

/// 图片头部声明的像素数（不解码，JPEG/PNG/GIF/WebP）；读不出来按 100 万像素估，给并行内存预算用（[`crate::imgpool`]）。
pub fn pixel_count(bytes: &[u8]) -> u64 {
    comic_header_dims(bytes).map(|(_, (w, h))| (w as u64) * (h as u64)).unwrap_or(1_000_000)
}

/// 只读文件头取 (格式, 宽, 高)，不解码像素（不按 EXIF 方向换宽高）。非 JPEG/PNG → None。
pub fn header_dims(bytes: &[u8]) -> Option<(ImageFormat, (u32, u32))> {
    header_dims_if(bytes, |f| matches!(f, ImageFormat::Jpeg | ImageFormat::Png))
}

/// 漫画页能处理的格式（JPEG/PNG/GIF/WebP）的 (格式, 宽, 高)，只读文件头。
fn comic_header_dims(bytes: &[u8]) -> Option<(ImageFormat, (u32, u32))> {
    header_dims_if(bytes, |f| matches!(f, ImageFormat::Jpeg | ImageFormat::Png | ImageFormat::Gif | ImageFormat::WebP))
}

fn header_dims_if(bytes: &[u8], ok: impl Fn(ImageFormat) -> bool) -> Option<(ImageFormat, (u32, u32))> {
    let fmt = crate::util::image_kind(bytes)?.format;
    if !ok(fmt) {
        return None;
    }
    let dims = image::ImageReader::with_format(Cursor::new(bytes), fmt).into_dimensions().ok()?;
    Some((fmt, dims))
}

/// **EPUB 内嵌图**降采样：一律竖向屏幕框（**宽绝不超屏幕短边**）。EPUB 图可能**行内**（xochitl 按固有
/// 尺寸渲染、不认 CSS），横图容许长边宽会让行内横幅溢出竖屏——2026-09-04 Move 真机《飘》1696×630 的
/// `class="logo"` 内联横幅溢出坐实。竖向框下：块级图仍适配列宽（显示无变化）、行内图不再超宽。
pub fn downscale_for_epub(bytes: &[u8], screen: Screen) -> Option<Vec<u8>> {
    downscale_for_epub_limited(bytes, screen, MAX_COMIC_DECODE_PIXELS)
}

/// 同 [`downscale_for_epub`]，另给解码上限 `max_px`（像素，`OptimizeOpts::limits.max_decode_pixels`）：超过它或
/// [`MAX_DECODE_PIXELS`] 的图不解码、原样保留。
pub fn downscale_for_epub_limited(bytes: &[u8], screen: Screen, max_px: u64) -> Option<Vec<u8>> {
    downscale_into(bytes, screen.width, screen.height, max_px)
}

/// 图片按 EXIF 方向摆正后的宽高（JPEG/PNG/GIF/WebP，只读文件头）。给按宽高比排版的地方用（[`crate::capfit`]）。
pub fn display_dims(bytes: &[u8]) -> Option<(u32, u32)> {
    let (fmt, (w, h)) = comic_header_dims(bytes)?;
    Some(if swaps_axes(orientation_of(bytes, fmt)) { (h, w) } else { (w, h) })
}

/// 带透明像素的 PNG 合成到白底，仍写成 PNG（灰度+透明 → 灰度，其余 → RGB；按 EXIF 摆正）。不是 PNG、没有透明通道、
/// alpha 全是不透明、超过解码上限（[`MAX_DECODE_PIXELS`] 和 `max_px` 中小的那个）、解不开 → `None`（原样保留）。
/// 给不认透明的阅读器用（[`crate::imgalpha`]）。
pub fn flatten_transparent_png(bytes: &[u8], max_px: u64) -> Option<Vec<u8>> {
    let (fmt, (w, h)) = header_dims(bytes)?;
    if fmt != ImageFormat::Png || !within_decode_budget(w, h, max_px) {
        return None;
    }
    let mut img = image::load_from_memory_with_format(bytes, fmt).ok()?;
    if !img.color().has_alpha() {
        return None;
    }
    let transparent = match &img {
        image::DynamicImage::ImageLumaA8(i) => i.pixels().any(|p| p.0[1] < 255),
        image::DynamicImage::ImageRgba8(i) => i.pixels().any(|p| p.0[3] < 255),
        other => other.to_rgba8().pixels().any(|p| p.0[3] < 255),
    };
    if !transparent {
        return None;
    }
    img.apply_orientation(orientation_of(bytes, fmt));
    let flat = flatten_alpha_on_white(img);
    let mut out = Vec::new();
    flat.write_to(&mut Cursor::new(&mut out), ImageFormat::Png).ok()?;
    Some(out)
}

/// **整页背景图**按原书的尺寸意图缩进阅读范围（见 [`crate::bgfit`]）：只缩不放、保原格式、带透明的保持透明（不铺白底：
/// 阅读器在透明处画背景色）。和 [`downscale_into`] 不同，缩了就用，不管重编码后是不是变小（调色板 PNG 解成 RGBA 再写回常常
/// 更大）——这类图显示对比体积重要。不用缩、不是 JPEG/PNG、超过解码上限（[`MAX_DECODE_PIXELS`] 和 `max_px` 中小的那个）、
/// 解码失败 → `None`（原样保留）。
pub fn downscale_background(bytes: &[u8], fit: crate::bgfit::BgFit, area: Screen, max_px: u64) -> Option<Vec<u8>> {
    let (fmt, (w, h)) = header_dims(bytes)?;
    let orientation = orientation_of(bytes, fmt);
    let (w, h) = if swaps_axes(orientation) { (h, w) } else { (w, h) };
    let (nw, nh) = fit.target(w, h, area)?;
    if !within_decode_budget(w, h, max_px) {
        return None;
    }
    let mut img = image::load_from_memory_with_format(bytes, fmt).ok()?;
    img.apply_orientation(orientation);
    match Page8::try_from_dynamic(img) {
        Ok(page) => page.resize_lanczos3(nw, nh).encode(fmt, JPEG_QUALITY),
        Err(img) if fmt == ImageFormat::Png => resize_alpha_png(img, nw, nh),
        Err(_) => None,
    }
}

/// 带透明（或 16 位等）的 PNG 缩放后写回 PNG。透明的按预乘 alpha 缩（`fast_image_resize` 缺省如此），透明像素的颜色
/// 不会渗进边缘（`image` 自带的缩放不预乘，调色板图透明处常是黑色，边上会出黑边）。16 位的降成 8 位。
fn resize_alpha_png(img: image::DynamicImage, nw: u32, nh: u32) -> Option<Vec<u8>> {
    use fast_image_resize::images::{Image, ImageRef};
    use fast_image_resize::{FilterType as FirFilter, PixelType, ResizeAlg, ResizeOptions, Resizer};
    use image::{ExtendedColorType, ImageEncoder};
    let (w, h) = (img.width(), img.height());
    let (raw, pt, ct) = match img {
        image::DynamicImage::ImageLumaA8(i) => (i.into_raw(), PixelType::U8x2, ExtendedColorType::La8),
        image::DynamicImage::ImageLuma16(_) => (img.into_luma8().into_raw(), PixelType::U8, ExtendedColorType::L8),
        i if !i.color().has_alpha() => (i.into_rgb8().into_raw(), PixelType::U8x3, ExtendedColorType::Rgb8),
        i if !i.color().has_color() => (i.into_luma_alpha8().into_raw(), PixelType::U8x2, ExtendedColorType::La8),
        i => (i.into_rgba8().into_raw(), PixelType::U8x4, ExtendedColorType::Rgba8),
    };
    let src = ImageRef::new(w, h, &raw, pt).ok()?;
    let mut dst = Image::new(nw, nh, pt);
    let opts = ResizeOptions::new().resize_alg(ResizeAlg::Convolution(FirFilter::Lanczos3));
    Resizer::new().resize(&src, &mut dst, &opts).ok()?;
    let mut out = Vec::new();
    image::codecs::png::PngEncoder::new(&mut out).write_image(dst.buffer(), nw, nh, ct).ok()?;
    Some(out)
}

/// 裁边判定容差：一行/列里像素两两 RGB 通道极差都 ≤ 这个值才算"纯色留白"。留够松（8）容 JPEG 压缩
/// 噪声，但不到能吃掉真实画面渐变的地步。
const TRIM_TOLERANCE: u8 = 8;
/// 单边最多裁掉原图这个比例——防止极端图（比如整页近乎纯色）被误判成"全是留白"裁没内容。
/// 真机《镖人》母版库实测坐实过 0.15 太保守（2026-09-19 用户反馈"优化没把大量留白裁切完"）：
/// 每卷开头的版权页（CIP 页，中文漫画常见排版）实际留白单边能到 22%-29%，旧阈值在 15% 就强行
/// 停手，裁不干净。抽样 43 张真实页量出的最大值约 28.6%，0.35 留出约 6 个百分点余量；两边独立
/// 累加最多到 0.7×边长，仍留 30% 给内容，不会把整页裁没。
const TRIM_MAX_FRACTION: f32 = 0.35;

/// 留白的亮度下限：一行/列的首像素 R、G、B 都 ≥ 这个值、其余像素与它的差都在 [`TRIM_TOLERANCE`] 以内，才算留白。
///
/// 只裁接近白的边：裁掉的边最后补的是白边（[`Page8::paste_on_white`]），黑色、灰色、彩色的边（出血到页边的黑底页、
/// 深色满版画面）裁了再补白就改了原画（2026-10-03 审计：此前只看"纯色"，黑底页的黑边被裁掉换成白边）。
/// 235 的依据：BT.601 有限范围（16–235）里 235 就是标称白，按这个范围出的扫描、视频截图里纸白落在 235 附近。
/// 只卡首像素、其余仍按容差比：贴着画面的那一两行留白有 JPEG 振铃（实测 q90 纸白 241 的边上有 233），这样判定与以前对
/// 白边的结果完全相同，只是多排除了不白的边。泛黄的纸、浅灰底低于它的不裁——拿不准就不处理。
const TRIM_WHITE_MIN: u8 = 235;

/// 一行/一列像素是否"白色留白"：首像素各通道都 ≥ [`TRIM_WHITE_MIN`]，其余像素与首像素的通道差都 ≤ [`TRIM_TOLERANCE`]。
/// `px(i)` 取该行/列第 i 个像素。
fn line_is_blank(len: u32, px: impl Fn(u32) -> [u8; 3]) -> bool {
    if len == 0 {
        return true;
    }
    let first = px(0);
    first.iter().all(|&c| c >= TRIM_WHITE_MIN)
        && (1..len).all(|i| {
            let p = px(i);
            (0..3).all(|c| p[c].abs_diff(first[c]) <= TRIM_TOLERANCE)
        })
}

/// 四边白色留白的检测：返回 `(left, top, 裁后宽, 裁后高)`；没有可裁的留白 / 图太小 / 会裁成空 → `None`。
/// 只在"确实是留白"时裁——边缘整行/整列像素接近白（[`TRIM_WHITE_MIN`]）且高度一致（[`TRIM_TOLERANCE`]）才算留白，一遇到不满足就停，
/// 不会裁进真实画面。单边最多裁 [`TRIM_MAX_FRACTION`]，兜底极端误判。
/// `px(x, y)` 取像素 RGB（[`Page8::rgb_at`]）。
fn trim_bounds_with(w: u32, h: u32, px: impl Fn(u32, u32) -> [u8; 3]) -> Option<(u32, u32, u32, u32)> {
    if w < 4 || h < 4 {
        return None;
    }
    let max_v = ((h as f32) * TRIM_MAX_FRACTION) as u32;
    let max_h = ((w as f32) * TRIM_MAX_FRACTION) as u32;
    let row = |y: u32| line_is_blank(w, |x| px(x, y));
    let col = |x: u32| line_is_blank(h, |y| px(x, y));
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
struct ComicSrc {
    /// 归一到 8 位灰度/RGB 的整页。
    img: Page8,
    /// 要重新编码时用的格式（[`comic_output_format`]）。GIF/WebP 只在本来就要改像素（裁边、缩放、补白、转灰度）时才换格式，
    /// 光是格式不同不算改动：不需要动的 GIF/WebP 原样保留。
    out_fmt: ImageFormat,
    /// 彩色转成了灰度（黑白屏）：像素已经和原图不同，原字节不能原样沿用。
    to_gray: bool,
    /// 按 EXIF 方向摆正过：原字节靠 EXIF 才显示正，重编码（不带 EXIF）时必须用摆正后的像素；也不原样沿用原字节
    /// （阅读器认不认 EXIF 没验证过，摆正后哪台都一样）。
    rotated: bool,
}

/// 漫画页产物的编码格式：JPEG、PNG 保持原格式；GIF 转 PNG（调色板图，无损）；WebP 看编码方式——有损的转 JPEG，无损的转 PNG。
/// 动图（多帧 GIF、动画 WebP）返回 `None`：只取第一帧会丢内容，原样保留。
fn comic_output_format(fmt: ImageFormat, bytes: &[u8]) -> Option<ImageFormat> {
    match fmt {
        ImageFormat::Jpeg | ImageFormat::Png => Some(fmt),
        ImageFormat::Gif => (gif_frame_count(bytes, 2)? == 1).then_some(ImageFormat::Png),
        ImageFormat::WebP => {
            if image::codecs::webp::WebPDecoder::new(Cursor::new(bytes)).ok()?.has_animation() {
                return None;
            }
            Some(if webp_is_lossless(bytes)? { ImageFormat::Png } else { ImageFormat::Jpeg })
        }
        _ => None,
    }
}

/// GIF 里有几帧（数到 `limit` 为止）：按 GIF89a 的块结构跳过去数图像描述符（`0x2C`），不解码像素。
/// 以前用解码器逐帧解出来数，静态页随后还要再整张解一遍。结构不对、截断 → `None`。
fn gif_frame_count(b: &[u8], limit: usize) -> Option<usize> {
    if b.len() < 13 || !(b.starts_with(b"GIF87a") || b.starts_with(b"GIF89a")) {
        return None;
    }
    let table = |packed: u8| if packed & 0x80 != 0 { 3usize << ((packed & 7) + 1) } else { 0 };
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
fn webp_is_lossless(bytes: &[u8]) -> Option<bool> {
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
/// `grayscale`（黑白屏设备）时彩色图顺手转成单通道 8 位灰度（256 级，不抖动）。带 EXIF 方向的先摆正（[`decode_oriented`]）。
/// 只合成了白底、别的都不用做时不算改动：原图照旧原样保留，透明区域交给阅读器按页面底色显示。
/// 超过解码上限 `max_px`（缺省 [`MAX_COMIC_DECODE_PIXELS`]）、动图、解不开 → `None`（原样保留）。
fn decode_comic(bytes: &[u8], grayscale: bool, max_px: u64) -> Option<ComicSrc> {
    let (fmt, (w, h)) = comic_header_dims(bytes)?;
    if (w as u64) * (h as u64) > max_px {
        return None; // 解压炸弹或离谱的大图：不整个解出来，原样保留
    }
    let out_fmt = comic_output_format(fmt, bytes)?;
    let orientation = orientation_of(bytes, fmt);
    let mut decoded = image::load_from_memory_with_format(bytes, fmt).ok()?;
    decoded.apply_orientation(orientation);
    let rotated = orientation != image::metadata::Orientation::NoTransforms;
    let gray = matches!(decoded.color(), image::ColorType::L8 | image::ColorType::L16 | image::ColorType::La8 | image::ColorType::La16);
    let to_gray = grayscale && !gray;
    let decoded = if decoded.color().has_alpha() { flatten_alpha_on_white(decoded) } else { decoded };
    let img = if gray || to_gray { Page8::Gray(decoded.into_luma8()) } else { Page8::Rgb(decoded.into_rgb8()) };
    Some(ComicSrc { img, out_fmt, to_gray, rotated })
}

/// 裁掉四边白色留白（[`trim_bounds_with`]）：返回 (裁后的图, 左偏移, 上偏移)。没得裁时原图原样返回、偏移 0。
/// 按值接收：裁了边时原图在这里就释放，不和裁后的副本同时占内存。
fn trim_comic(img: Page8) -> (Page8, u32, u32) {
    let (w, h) = img.dimensions();
    match trim_bounds_with(w, h, |x, y| img.rgb_at(x, y)) {
        Some((l, t, cw, ch)) => (img.crop(l, t, cw, ch), l, t),
        None => (img, 0, 0),
    }
}

/// 带透明通道的图合成到白底：灰度+alpha → `Luma8`，其余 → `Rgb8`（按 8 位合成，16 位先降到 8 位）。
/// 每个分量 `c·a/255 + 255·(1 − a/255)`，四舍五入。
fn flatten_alpha_on_white(img: image::DynamicImage) -> image::DynamicImage {
    use image::DynamicImage;
    let blend = |c: u8, a: u8| -> u8 { ((c as u32 * a as u32 + 255 * (255 - a as u32) + 127) / 255) as u8 };
    match img.color() {
        image::ColorType::La8 | image::ColorType::La16 => {
            let la = img.into_luma_alpha8();
            let (w, h) = la.dimensions();
            DynamicImage::ImageLuma8(image::GrayImage::from_fn(w, h, |x, y| {
                let p = la.get_pixel(x, y).0;
                image::Luma([blend(p[0], p[1])])
            }))
        }
        _ => {
            let rgba = img.into_rgba8();
            let (w, h) = rgba.dimensions();
            DynamicImage::ImageRgb8(image::RgbImage::from_fn(w, h, |x, y| {
                let p = rgba.get_pixel(x, y).0;
                image::Rgb([blend(p[0], p[3]), blend(p[1], p[3]), blend(p[2], p[3])])
            }))
        }
    }
}

/// 漫画页在画布上的排版：图缩放成 `nw × nh`，放在 `canvas_w × canvas_h` 的白底画布上，左上角在 (`x`, `y`)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct PageLayout {
    nw: u32,
    nh: u32,
    canvas_w: u32,
    canvas_h: u32,
    x: u32,
    y: u32,
}

impl PageLayout {
    /// 原图（裁边前 `ow × oh`，裁掉的左、上留白 `tl`、`tt`，裁后 `cw × ch`）已经是这个排版、只差白边那一两个像素：
    /// 画布就是原图尺寸，图的位置差不超过 `margin + 1` 像素、边长差不超过白边在该方向上对应的量（受限边 2×白边，另一条按
    /// 阅读范围的长短边比例放大）再加 2。这时原字节就是结果——正好是阅读范围大小、画面顶到边的页不为 1px 白边重编码整页；
    /// 已经优化过的书再跑一遍也不会一代代重编码（JPEG 噪声会让 1px 白边裁不干净，所以要有容差）。
    fn matches_original(&self, (ow, oh): (u32, u32), (tl, tt): (u32, u32), (cw, ch): (u32, u32), area: Screen, margin: u32) -> bool {
        let (long, short) = (area.width.max(area.height) as u64, area.width.min(area.height).max(1) as u64);
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
fn comic_layout(w: u32, h: u32, area: Screen, margin: u32, may_upscale: bool) -> PageLayout {
    let (aw, ah) = (area.width, area.height);
    let m = effective_margin(margin, area);
    let (bw, bh) = (aw - 2 * m, ah - 2 * m);
    let (nw, nh) = fit_box(w, h, bw, bh);
    let upscale = nw > w || nh > h;
    if !upscale || (may_upscale && nw as f64 / w as f64 <= MAX_UPSCALE) {
        return PageLayout { nw, nh, canvas_w: aw, canvas_h: ah, x: (aw - nw) / 2, y: (ah - nh) / 2 };
    }
    // 不放大：受限边（放大到框时先顶满的那条）按比例缩小白边，另一条按阅读范围的宽高比算，至少容得下图和白边。
    let width_bound = (w as u64) * (bh as u64) >= (h as u64) * (bw as u64);
    let (along, box_along) = if width_bound { (w, bw) } else { (h, bh) };
    let mm = if m == 0 { 0 } else { (((m as u64) * (along as u64) + box_along as u64 / 2) / box_along as u64).max(1) as u32 };
    let (canvas_w, canvas_h) = if width_bound {
        let cw = w + 2 * mm;
        (cw, ((((cw as u64) * (ah as u64) + aw as u64 / 2) / aw as u64) as u32).max(h + 2 * mm))
    } else {
        let ch = h + 2 * mm;
        (((((ch as u64) * (aw as u64) + ah as u64 / 2) / ah as u64) as u32).max(w + 2 * mm), ch)
    };
    PageLayout { nw: w, nh: h, canvas_w, canvas_h, x: (canvas_w - w) / 2, y: (canvas_h - h) / 2 }
}

/// **EPUB 漫画整页的单趟处理**：解码一次 → 裁白边 → 按 [`comic_layout`] 缩放（缩小，或 JPEG 小图放大）→ 居中放上白底画布 →
/// 编码一次（JPEG 质量 [`JPEG_QUALITY_COMIC`]，灰度保持单分量）。
///
/// - `area` 是 profile 的真实可阅读范围，`margin` 是 profile 的 `comic_margin`：产物画布正好是 `area` 大小，
///   图到四边的距离在受限的那条边上正好是 `margin` 像素（另一条边更多），宽高比不变。
/// - `grayscale`（黑白屏设备）时彩色页转成单分量 8 位灰度（256 级，不抖动，2026-09-27 用户定）。
/// - 静态 GIF 转 PNG、WebP 转 JPEG/PNG（[`comic_output_format`]）；动图原样保留。
/// - 短边不到阅读范围宽度 1/3 的装饰小图只裁边（和换格式、转灰度），不缩放、不补白。
/// - 已经排好的页（和目标排版相差不超过 1px，见 [`PageLayout::matches_original`]）、没有别的要改时返回 `None`（原字节零损失）。
///   带 EXIF 方向（非"不用转"）的页一律摆正后重编码。
/// - 超过 [`MAX_COMIC_DECODE_PIXELS`] 的图、解不开的图返回 `None`。
pub fn prepare_comic_page_for_epub(bytes: &[u8], area: Screen, margin: u32, grayscale: bool) -> Option<Vec<u8>> {
    prepare_comic_page_limited(bytes, area, margin, grayscale, MAX_COMIC_DECODE_PIXELS)
}

/// 同 [`prepare_comic_page_for_epub`]，另给解码上限 `max_px`（像素，`OptimizeOpts::limits.max_decode_pixels`）：
/// 超过的页不解码、原样保留。
pub fn prepare_comic_page_limited(bytes: &[u8], area: Screen, margin: u32, grayscale: bool, max_px: u64) -> Option<Vec<u8>> {
    let ComicSrc { img, out_fmt, to_gray, rotated } = decode_comic(bytes, grayscale, max_px)?;
    let orig = img.dimensions();
    let (img, tl, tt) = trim_comic(img);
    let (cw, ch) = img.dimensions();
    let trimmed = (cw, ch) != orig;
    if cw.min(ch) < area.width / 3 {
        // 装饰小图：只裁边
        return if trimmed || to_gray || rotated { img.encode(out_fmt, JPEG_QUALITY_COMIC) } else { None };
    }
    let lay = comic_layout(cw, ch, area, margin, out_fmt == ImageFormat::Jpeg);
    if !to_gray && !rotated && lay.matches_original(orig, (tl, tt), (cw, ch), area, effective_margin(margin, area)) {
        return None;
    }
    let img = if (lay.nw, lay.nh) != (cw, ch) { img.resize_lanczos3(lay.nw, lay.nh) } else { img };
    let page = if (lay.canvas_w, lay.canvas_h) != (lay.nw, lay.nh) { img.paste_on_white(lay.canvas_w, lay.canvas_h, lay.x, lay.y) } else { img };
    page.encode(out_fmt, JPEG_QUALITY_COMIC)
}

/// 归一后的 8 位整页：单通道灰度或 RGB。漫画页解码时一次归一（[`decode_comic`]），文字书插图解出来正好是这两种时也走它
/// （[`downscale_into`]）；裁边、缩放、补白、编码都只对这两种写，不再各自留一个"其它类型"的分支。
enum Page8 {
    Gray(image::GrayImage),
    Rgb(image::RgbImage),
}

impl Page8 {
    /// 本来就是 8 位灰度/RGB 的图直接接过来（不拷贝）；其它类型原样退回。
    fn try_from_dynamic(img: image::DynamicImage) -> Result<Page8, image::DynamicImage> {
        match img {
            image::DynamicImage::ImageLuma8(g) => Ok(Page8::Gray(g)),
            image::DynamicImage::ImageRgb8(c) => Ok(Page8::Rgb(c)),
            other => Err(other),
        }
    }

    fn into_dynamic(self) -> image::DynamicImage {
        match self {
            Page8::Gray(g) => image::DynamicImage::ImageLuma8(g),
            Page8::Rgb(c) => image::DynamicImage::ImageRgb8(c),
        }
    }

    fn dimensions(&self) -> (u32, u32) {
        match self {
            Page8::Gray(g) => g.dimensions(),
            Page8::Rgb(c) => c.dimensions(),
        }
    }

    /// 像素的 RGB（灰度给 `[v; 3]`，不必先造一份 3 倍大的 RGB 副本）。
    fn rgb_at(&self, x: u32, y: u32) -> [u8; 3] {
        match self {
            Page8::Gray(g) => [g.get_pixel(x, y)[0]; 3],
            Page8::Rgb(c) => c.get_pixel(x, y).0,
        }
    }

    /// 裁出 `(x, y)` 起 `w × h` 的一块。
    fn crop(self, x: u32, y: u32, w: u32, h: u32) -> Page8 {
        match self {
            Page8::Gray(g) => Page8::Gray(image::imageops::crop_imm(&g, x, y, w, h).to_image()),
            Page8::Rgb(c) => Page8::Rgb(image::imageops::crop_imm(&c, x, y, w, h).to_image()),
        }
    }

    /// 放到 `cw × ch` 的白底画布上，左上角在 (`x`, `y`)；类型不变。
    fn paste_on_white(&self, cw: u32, ch: u32, x: u32, y: u32) -> Page8 {
        match self {
            Page8::Gray(g) => {
                let mut canvas = image::GrayImage::from_pixel(cw, ch, image::Luma([255]));
                image::imageops::overlay(&mut canvas, g, x as i64, y as i64);
                Page8::Gray(canvas)
            }
            Page8::Rgb(c) => {
                let mut canvas = image::RgbImage::from_pixel(cw, ch, image::Rgb([255, 255, 255]));
                image::imageops::overlay(&mut canvas, c, x as i64, y as i64);
                Page8::Rgb(canvas)
            }
        }
    }

    /// Lanczos3 重采样到 `dw × dh`，SIMD 实现（`fast_image_resize`，x86 SSE4/AVX2、aarch64 NEON 运行期自动选）。
    ///
    /// 替换 `DynamicImage::resize_exact(.., Lanczos3)` 的原因：2026-09-20 分阶段计时（乱马/镖人，
    /// release、每页 ~1000×1500）显示**缩放占整页处理时间的 74–79%**（145–218ms/页），编码 15–22%，
    /// 解码/裁边探测可忽略；`fast_image_resize` 同一算法（Lanczos3 卷积）快约 **20 倍**（7–11ms/页）。
    /// **不是逐位一致**：与 `image` 库实现的像素差均值 0.1–0.2 灰阶、最大 ~30（仅高对比边缘），二者对
    /// 浮点参照（PIL）都是 53–55dB——远低于随后 JPEG q95 编码本身的误差（约 45dB），没有可见差别。
    /// 库报错时退回 `image` 自带实现（类型不变）。
    fn resize_lanczos3(self, dw: u32, dh: u32) -> Page8 {
        use fast_image_resize::images::{Image, ImageRef};
        use fast_image_resize::{FilterType as FirFilter, PixelType, ResizeAlg, ResizeOptions, Resizer};
        let opts = ResizeOptions::new().resize_alg(ResizeAlg::Convolution(FirFilter::Lanczos3));
        let (w, h) = self.dimensions();
        let (raw, pt) = match &self {
            Page8::Gray(g) => (g.as_raw(), PixelType::U8),
            Page8::Rgb(c) => (c.as_raw(), PixelType::U8x3),
        };
        let fast = || -> Option<Vec<u8>> {
            let src = ImageRef::new(w, h, raw, pt).ok()?;
            let mut dst = Image::new(dw, dh, pt);
            Resizer::new().resize(&src, &mut dst, &opts).ok()?;
            Some(dst.into_vec())
        };
        let out = fast().and_then(|v| match self {
            Page8::Gray(_) => image::GrayImage::from_raw(dw, dh, v).map(Page8::Gray),
            Page8::Rgb(_) => image::RgbImage::from_raw(dw, dh, v).map(Page8::Rgb),
        });
        out.unwrap_or_else(|| match self.into_dynamic().resize_exact(dw, dh, FilterType::Lanczos3) {
            image::DynamicImage::ImageLuma8(g) => Page8::Gray(g),
            other => Page8::Rgb(other.into_rgb8()),
        })
    }

    /// 按 `fmt` 编码（JPEG 质量 `jpeg_quality`）；JPEG、PNG 以外 → `None`。JPEG **灰度保持单分量**：`image` 0.25 的
    /// `JpegEncoder::encode_image(&DynamicImage)` 对 `ImageLuma8` 也会转成 3 分量 RGB 输出（2026-09-20 实测 SOF 分量数=3、
    /// 回读 `Rgb8`），必须走 `ImageEncoder::write_image(.., ExtendedColorType::L8)` 才是真灰度 JPEG。
    fn encode(self, fmt: ImageFormat, jpeg_quality: u8) -> Option<Vec<u8>> {
        use image::{ExtendedColorType, ImageEncoder};
        let (w, h) = self.dimensions();
        let (raw, ct) = match &self {
            Page8::Gray(g) => (g.as_raw(), ExtendedColorType::L8),
            Page8::Rgb(c) => (c.as_raw(), ExtendedColorType::Rgb8),
        };
        let mut out = Vec::new();
        match fmt {
            ImageFormat::Jpeg => {
                JpegEncoder::new_with_quality(&mut out, jpeg_quality).write_image(raw, w, h, ct).ok()?;
                // 哈夫曼表按这张图重做（无损：系数一个不动，见 `jpegopt`），同样画质小约 7%–16%
                Some(crate::jpegopt::optimize_verified(out))
            }
            ImageFormat::Png => {
                image::codecs::png::PngEncoder::new(&mut out).write_image(raw, w, h, ct).ok()?;
                Some(out)
            }
            _ => None,
        }
    }
}

/// 优化器交给图片处理的条目（按扩展名：jpg/jpeg/png/gif/webp）。GIF/WebP 只有漫画页会处理（[`prepare_comic_page_for_epub`]），
/// 文字书里的原样保留（[`downscale_for_epub`] 只认 JPEG/PNG）。流式优化阶段一这些条目只占位、不读字节。
pub fn is_page_image(name: &str) -> bool {
    crate::util::is_image_ext(name)
}

/// 图片条目处理后换了格式（漫画里的 GIF/WebP 转成 PNG/JPEG，条目名不变）时，OPF manifest 该写的新 media-type；
/// 没换格式 → `None`。按产物字节的魔数判断，不看处理过程。
pub fn converted_media_type(name: &str, out: &[u8]) -> Option<&'static str> {
    let mt = crate::util::image_kind(out).filter(|k| matches!(k.format, ImageFormat::Jpeg | ImageFormat::Png))?.mime;
    (crate::util::image_media_type_of_ext(&crate::util::image_ext_of(name)) != mt).then_some(mt)
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{DynamicImage, GenericImageView, RgbImage};


    fn jpeg_of(w: u32, h: u32) -> Vec<u8> {
        let img = DynamicImage::ImageRgb8(RgbImage::from_fn(w, h, |x, y| {
            image::Rgb([(x % 256) as u8, (y % 256) as u8, 128])
        }));
        let mut buf = Vec::new();
        JpegEncoder::new_with_quality(&mut buf, 90).encode_image(&img).unwrap();
        buf
    }

    fn gray_jpeg_of(w: u32, h: u32, border: u32) -> Vec<u8> {
        // 灰度渐变内容 + 四周 `border` 像素纯白留白。
        let img = image::GrayImage::from_fn(w, h, |x, y| {
            if x < border || y < border || x >= w - border || y >= h - border {
                image::Luma([255])
            } else {
                image::Luma([((x * 7 + y * 3) % 200) as u8])
            }
        });
        let mut buf = Vec::new();
        // 必须 write_image(L8)：encode_image(&DynamicImage) 会把灰度悄悄转成 3 分量 RGB。
        image::ImageEncoder::write_image(
            JpegEncoder::new_with_quality(&mut buf, 95),
            img.as_raw(),
            w,
            h,
            image::ExtendedColorType::L8,
        )
        .unwrap();
        buf
    }

    /// Move 在 xochitl 默认页边距下的 EPUB 真实可阅读范围（profile 里的 `readable.epub`，842×1455）。
    fn test_area() -> Screen {
        profile::get("xochitl").unwrap().readable(profile::Format::Epub)
    }

    /// 生产入口，白边取缺省 1px。
    fn prep(bytes: &[u8], area: Screen, grayscale: bool) -> Option<Vec<u8>> {
        prepare_comic_page_for_epub(bytes, area, 1, grayscale)
    }

    /// 解码 + 裁边（生产的前两步）：返回 (裁后的图, 产物格式, 是否已经和原图不同——裁了边、转了灰度或换了格式)。
    fn decode_trim_comic(bytes: &[u8], grayscale: bool) -> Option<(DynamicImage, ImageFormat, bool)> {
        let src = decode_comic(bytes, grayscale, MAX_COMIC_DECODE_PIXELS)?;
        let orig = src.img.dimensions();
        let (img, _, _) = trim_comic(src.img);
        let changed = img.dimensions() != orig || src.to_gray;
        Some((img.into_dynamic(), src.out_fmt, changed))
    }

    /// 页图里深色内容（< 128）的外框到四边的距离（左, 上, 右, 下）。
    fn dark_margins(img: &DynamicImage) -> (u32, u32, u32, u32) {
        let g = img.to_luma8();
        let (w, h) = g.dimensions();
        let dark = |x: u32, y: u32| g.get_pixel(x, y)[0] < 128;
        let col = |x: u32| (0..h).any(|y| dark(x, y));
        let row = |y: u32| (0..w).any(|x| dark(x, y));
        let l = (0..w).find(|&x| col(x)).expect("有内容");
        let r = (0..w).rev().find(|&x| col(x)).unwrap();
        let t = (0..h).find(|&y| row(y)).unwrap();
        let b = (0..h).rev().find(|&y| row(y)).unwrap();
        (l, t, w - 1 - r, h - 1 - b)
    }

    /// 深色（0 与 60 相间的 5px 棋盘格）、没有留白的灰度 JPEG：画面一直到边，任何一行一列都不是纯色，不会被当留白裁掉。
    fn black_jpeg(w: u32, h: u32) -> Vec<u8> {
        let px: Vec<u8> = (0..h).flat_map(|y| (0..w).map(move |x| (((x / 5 + y / 5) % 2) * 60) as u8)).collect();
        let mut buf = Vec::new();
        image::ImageEncoder::write_image(JpegEncoder::new_with_quality(&mut buf, 95), &px, w, h, image::ExtendedColorType::L8).unwrap();
        buf
    }

    /// 解码器 panic 按处理失败算（抓远程图、解封面以前没兜住，一张坏图摔掉整本书的优化）。
    #[test]
    fn guard_turns_decoder_panic_into_none() {
        assert_eq!(guard(|| -> Option<u8> { panic!("坏图") }), None);
        assert_eq!(guard(|| Some(3)), Some(3));
        assert!(decode_oriented(b"\xFF\xD8\xFF garbage").is_none());
    }

    #[test]
    fn prepare_epub_page_upscales_low_res_and_pads_to_exact_frame() {
        // 镖人同款 566×800 灰度：等比放大到 842 宽（1190 高）→ 白底补到阅读范围 842×1455，灰度保持。
        let out = prep(&gray_jpeg_of(566, 800, 0), test_area(), false).expect("低分辨率必须预放大");
        let img = image::load_from_memory(&out).unwrap();
        assert_eq!((img.width(), img.height()), (842, 1455));
        assert_eq!(img.color(), image::ColorType::L8);
    }

    #[test]
    fn prepare_epub_page_shrinks_large_page_once_and_pads() {
        // 乱马同款 1091×1592：缩到 842×1229，补白到 842×1455。
        let out = prep(&gray_jpeg_of(1091, 1592, 0), test_area(), false).expect("超框必须缩");
        assert_eq!(image::load_from_memory(&out).unwrap().dimensions(), (842, 1455));
    }

    #[test]
    fn prepare_epub_page_trim_then_fit_in_one_pass() {
        // 带 60px 白边：先裁再适配，仍是阅读范围尺寸，且只编码一次（尺寸即证明一趟到位）。
        let out = prep(&gray_jpeg_of(800, 1200, 60), test_area(), false).unwrap();
        assert_eq!(image::load_from_memory(&out).unwrap().dimensions(), (842, 1455));
    }

    #[test]
    fn prepare_epub_page_pads_tall_narrow_page_left_right_without_exceeding_width() {
        // 比阅读范围"窄"的高瘦页（如 700×1600）：高度顶到 1455，宽度 < 842，左右对称补白到 842——宽绝不超阅读范围。
        let out = prep(&gray_jpeg_of(700, 1600, 0), test_area(), false).expect("高瘦页必须缩+补白");
        assert_eq!(image::load_from_memory(&out).unwrap().dimensions(), (842, 1455));
    }

    #[test]
    fn epub_pad_tolerance_catches_page_one_point_six_percent_off_frame() {
        // 真机 e2e：一张比框窄 1.6% 的页被 2% 容差放过，图片少 4.5pt 宽且左右不对称——补白容差必须更严。
        let w = (842.0_f32 * 0.984).round() as u32;
        let out = prep(&gray_jpeg_of(w, 1455, 0), test_area(), false).expect("偏差 1.6% 必须补白");
        assert_eq!(image::load_from_memory(&out).unwrap().dimensions(), (842, 1455));
    }

    #[test]
    fn nominal_screen_is_used_as_is_when_no_readable_area() {
        // 没有内置阅读范围的设备按标称屏幕补白。
        let out = prep(&gray_jpeg_of(566, 800, 0), test_screen(), false).expect("低分辨率必须预放大");
        assert_eq!(image::load_from_memory(&out).unwrap().dimensions(), (954, 1696));
        assert!(prep(&gray_jpeg_of(954, 1696, 0), test_screen(), false).is_none(), "已是屏幕页、无白边：原字节");
    }

    #[test]
    fn prepare_epub_page_leaves_untouched_when_already_device_page_or_tiny_icon() {
        assert!(prep(&gray_jpeg_of(842, 1455, 0), test_area(), false).is_none(), "已是阅读范围尺寸、无白边：原字节零损失");
        assert!(prep(&gray_jpeg_of(200, 300, 0), test_area(), false).is_none(), "装饰小图且无白边：原样");
    }

    #[test]
    fn grayscale_devices_get_single_channel_comic_pages() {
        let color = jpeg_of(842, 1455); // 彩色、尺寸正好是阅读范围、没有白边：彩色屏原样零损失
        assert!(prep(&color, test_area(), false).is_none());
        let out = prep(&color, test_area(), true).expect("黑白屏要转灰度");
        let img = image::load_from_memory(&out).unwrap();
        assert_eq!(img.color(), image::ColorType::L8, "单通道 8 位灰度（256 级）");
        assert_eq!(img.dimensions(), (842, 1455));
        // 需要缩放的彩色页：彩色屏保持 RGB，黑白屏出灰度
        let big = jpeg_of(1600, 2400);
        assert_eq!(image::load_from_memory(&prep(&big, test_area(), false).unwrap()).unwrap().color(), image::ColorType::Rgb8);
        assert_eq!(image::load_from_memory(&prep(&big, test_area(), true).unwrap()).unwrap().color(), image::ColorType::L8);
        // 不抖动：纯中灰（128）转完仍是均匀中灰，不会变成黑白点
        let mut mid = Vec::new();
        JpegEncoder::new_with_quality(&mut mid, 95).encode_image(&DynamicImage::ImageRgb8(RgbImage::from_pixel(842, 1455, image::Rgb([128, 128, 128])))).unwrap();
        let g = image::load_from_memory(&prep(&mid, test_area(), true).unwrap()).unwrap().to_luma8();
        // 灰色的边不是留白、不裁：整页按 1px 白边重排，白边以内的像素都是中灰
        let (w, h) = g.dimensions();
        assert!((3..w - 3).all(|x| (3..h - 3).all(|y| (120..=136).contains(&g.get_pixel(x, y).0[0]))), "中灰保持中灰，没有抖动成黑白点");
    }

    #[test]
    fn prepare_epub_page_does_not_upscale_png() {
        // PNG 不放大（无损放大体积暴涨）：700×1000 按自己的比例尺补白到阅读范围的宽高比，图不缩放。
        // 受限边（宽）的白边按比例：1px × 700/840 ≈ 0.83 → 1px（至少 1px）；画布 702 宽、高按 842:1455 算。
        let img = DynamicImage::ImageRgb8(RgbImage::from_fn(700, 1000, |x, y| image::Rgb([(x % 256) as u8, (y % 256) as u8, 9])));
        let mut png = Vec::new();
        img.write_to(&mut Cursor::new(&mut png), ImageFormat::Png).unwrap();
        let out = prep(&png, test_area(), false).unwrap();
        assert_eq!(image::guess_format(&out).unwrap(), ImageFormat::Png, "PNG 仍是 PNG");
        let page = image::load_from_memory(&out).unwrap();
        assert_eq!(page.dimensions(), (702, 1213), "702 × round(702×1455/842)");
        // 图原样（没缩放）贴在中间：左上角 (1, 106)，像素逐个相同（PNG 无损）
        let back = page.to_rgb8();
        let src = img.to_rgb8();
        assert!((0..700).step_by(37).all(|x| (0..1000).step_by(41).all(|y| back.get_pixel(x + 1, y + 106) == src.get_pixel(x, y))), "图不缩放、居中");
        assert_eq!(*back.get_pixel(0, 500), image::Rgb([255, 255, 255]), "左边 1px 白边");
    }

    #[test]
    fn resize_lanczos3_simd_matches_image_crate_closely() {
        // 合成带高对比边缘+渐变的 RGB 与灰度图，SIMD 结果与 image 库实现的像素差必须很小（均值 <0.5 灰阶）。
        let rgb = DynamicImage::ImageRgb8(RgbImage::from_fn(1091, 1592, |x, y| {
            let edge = if (x / 37 + y / 41) % 2 == 0 { 20 } else { 235 };
            image::Rgb([edge, ((x + y) % 256) as u8, (x % 256) as u8])
        }));
        let gray = DynamicImage::ImageLuma8(image::GrayImage::from_fn(700, 1000, |x, y| image::Luma([((x * 3 + y * 5) % 256) as u8])));
        for (img, (dw, dh)) in [(rgb, (934u32, 1363u32)), (gray, (934, 1334))] {
            let fast = Page8::try_from_dynamic(img.clone()).ok().unwrap().resize_lanczos3(dw, dh).into_dynamic();
            let slow = img.resize_exact(dw, dh, FilterType::Lanczos3);
            assert_eq!(fast.color(), slow.color());
            assert_eq!(fast.dimensions(), (dw, dh));
            let (a, b) = (fast.as_bytes(), slow.as_bytes());
            let mean = a.iter().zip(b).map(|(x, y)| x.abs_diff(*y) as f64).sum::<f64>() / a.len() as f64;
            assert!(mean < 0.5, "SIMD 与 image 库 Lanczos3 差距过大: 均值 {mean}");
        }
    }

    #[test]
    fn device_orientation_box_for_comics() {
        // CBZ/漫画整页：按朝向选盒。横图 3392×1908 → 1696×954（横读可用满宽）
        let big = jpeg_of(3392, 1908);
        let (w, h) = image::load_from_memory(&downscale_for_device(&big, test_screen()).unwrap()).unwrap().dimensions();
        assert_eq!((w, h), (test_screen().height, 954), "横页应到 1696×954");
        // 方图 → 954×954
        let sq = jpeg_of(2000, 2000);
        let (w, h) = image::load_from_memory(&downscale_for_device(&sq, test_screen()).unwrap()).unwrap().dimensions();
        assert_eq!((w, h), (954, 954));
    }

    #[test]
    fn frames_follow_the_given_screen() {
        // 非 Move 的屏幕（掌阅 Ocean 5 Pro，1264×1680）：缩放框与漫画页框都按传入的屏幕算，不再是 954×1696。
        let ireader = profile::get("ireader").unwrap().screen;
        let (w, h) = image::load_from_memory(&downscale_for_epub(&jpeg_of(2400, 3200), ireader).unwrap()).unwrap().dimensions();
        assert_eq!((w, h), (1260, 1680), "竖图按 1264×1680 框等比缩");
        let (w, h) = image::load_from_memory(&downscale_for_device(&jpeg_of(3200, 1600), ireader).unwrap()).unwrap().dimensions();
        assert_eq!((w, h), (1680, 840), "横页按横向框 1680×1264");
        let out = prep(&gray_jpeg_of(1091, 1592, 0), ireader, false).expect("要补白到屏幕比例");
        assert_eq!(image::load_from_memory(&out).unwrap().dimensions(), (1264, 1680));
    }

    #[test]
    fn epub_portrait_box_caps_width_954() {
        // EPUB 内嵌图一律卡宽 ≤954（防行内横幅溢出竖屏）
        // 横图 1696×630 的内联横幅（《飘》真机溢出源）→ 954×~355
        let banner = jpeg_of(1696, 630);
        let (w, h) = image::load_from_memory(&downscale_for_epub(&banner, test_screen()).unwrap()).unwrap().dimensions();
        assert_eq!(w, 954, "横幅宽必须卡到 954");
        assert!(h < 400, "保比 h={h}");
        // 方图 → 954×954；竖图 1000×3000 → 565×1696
        let sq = jpeg_of(2000, 2000);
        assert_eq!(image::load_from_memory(&downscale_for_epub(&sq, test_screen()).unwrap()).unwrap().dimensions(), (954, 954));
        let tall = jpeg_of(1000, 3000);
        let (w, h) = image::load_from_memory(&downscale_for_epub(&tall, test_screen()).unwrap()).unwrap().dimensions();
        assert!(w <= 954 && h == 1696, "竖图 {w}x{h}");
    }

    /// 造一张带纯白边框的图：中心是彩色渐变，四边留白。
    fn framed_jpeg(w: u32, h: u32, margin: u32) -> Vec<u8> {
        let img = DynamicImage::ImageRgb8(RgbImage::from_fn(w, h, |x, y| {
            if x < margin || y < margin || x >= w - margin || y >= h - margin {
                image::Rgb([255, 255, 255])
            } else {
                image::Rgb([(x % 256) as u8, (y % 256) as u8, 128])
            }
        }));
        let mut buf = Vec::new();
        // 高质量无损级编码，避免 JPEG 压缩噪声把"纯色"判花（真实场景裁边容差本身留够松，这里只是
        // 让测试信号干净，不代表生产输入总是这么干净）。
        JpegEncoder::new_with_quality(&mut buf, 100).encode_image(&img).unwrap();
        buf
    }

    /// 走生产的解码+裁边探测（[`decode_trim_comic`]），返回裁后尺寸；没有可裁的留白 → `None`。
    fn trim_dims(bytes: &[u8]) -> Option<(u32, u32)> {
        let (img, _, trimmed) = decode_trim_comic(bytes, false)?;
        trimmed.then(|| img.dimensions())
    }

    #[test]
    fn trim_crops_uniform_white_border_only() {
        let framed = framed_jpeg(200, 300, 10);
        let (w, h) = trim_dims(&framed).expect("四边留白应触发裁边");
        assert_eq!((w, h), (180, 280), "应精确裁掉 10px 留白: got {w}x{h}");
    }

    /// 只裁接近白的边：黑底出血页、灰边、彩色边都不裁（裁了再补白就改了原画）；略带噪声的浅色纸白照裁。
    #[test]
    fn trim_only_near_white_borders() {
        let bordered = |c: [u8; 3]| {
            let img = DynamicImage::ImageRgb8(RgbImage::from_fn(200, 300, |x, y| {
                if !(20..180).contains(&x) || !(20..280).contains(&y) { image::Rgb(c) } else { image::Rgb([(x % 256) as u8, (y % 256) as u8, 128]) }
            }));
            let mut buf = Vec::new();
            JpegEncoder::new_with_quality(&mut buf, 100).encode_image(&img).unwrap();
            buf
        };
        for c in [[0u8, 0, 0], [128, 128, 128], [200, 30, 30], [230, 230, 230]] {
            assert!(trim_dims(&bordered(c)).is_none(), "{c:?} 的边不该裁");
        }
        for c in [[255u8, 255, 255], [240, 242, 238]] {
            assert_eq!(trim_dims(&bordered(c)), Some((160, 260)), "{c:?} 是纸白，要裁");
        }
        // 端到端：黑边页已经是阅读范围大小 → 原样；大一圈的黑边页缩放后黑边还在（画面到边，四周只有 1px 白边）
        let area = test_area();
        let black_frame = |w: u32, h: u32| {
            let px: Vec<u8> = (0..h).flat_map(|y| (0..w).map(move |x| if x < 40 || y < 40 || x + 40 >= w || y + 40 >= h { 0 } else { (128 + (x + y) % 100) as u8 })).collect();
            let mut buf = Vec::new();
            image::ImageEncoder::write_image(JpegEncoder::new_with_quality(&mut buf, 95), &px, w, h, image::ExtendedColorType::L8).unwrap();
            buf
        };
        assert!(prep(&black_frame(area.width, area.height), area, false).is_none(), "黑底页不裁不补");
        let out = image::load_from_memory(&prep(&black_frame(1091, 1592), area, false).unwrap()).unwrap();
        let (l, t, r, b) = dark_margins(&out);
        assert!((l, r) == (1, 1) || (t, b) == (1, 1), "黑边保留、整页缩放：左{l} 上{t} 右{r} 下{b}");
    }

    #[test]
    fn trim_none_when_no_uniform_border() {
        let img = DynamicImage::ImageRgb8(RgbImage::from_fn(200, 300, |x, y| image::Rgb([(x % 256) as u8, (y % 256) as u8, 128])));
        let mut buf = Vec::new();
        JpegEncoder::new_with_quality(&mut buf, 95).encode_image(&img).unwrap();
        assert!(trim_dims(&buf).is_none(), "画面一直到边缘、没有留白，不该裁");
    }

    #[test]
    fn trim_capped_by_max_fraction_for_near_solid_image() {
        // 几乎整张纯色(只有中心一小块不同)——裁边不能把整张图裁没，单边应被 TRIM_MAX_FRACTION 卡住。
        let img = DynamicImage::ImageRgb8(RgbImage::from_fn(200, 200, |x, y| {
            if (90..110).contains(&x) && (90..110).contains(&y) { image::Rgb([0, 0, 0]) } else { image::Rgb([255, 255, 255]) }
        }));
        let mut buf = Vec::new();
        JpegEncoder::new_with_quality(&mut buf, 100).encode_image(&img).unwrap();
        let (w, h) = trim_dims(&buf).expect("大片留白应触发裁边");
        let cap = (200.0 * TRIM_MAX_FRACTION) as u32;
        assert!(w >= 200 - 2 * cap && h >= 200 - 2 * cap, "单边最多裁 TRIM_MAX_FRACTION，不能把画面裁没: got {w}x{h}");
    }

    #[test]
    fn trim_handles_margin_beyond_old_cap() {
        // 真机回归（2026-09-19，《镖人》母版库反馈"优化没把大量留白裁切完"）：中文漫画常见的
        // 版权页（CIP 页）实测单边留白能到 22%-29%（抽样见会话记录），旧的 15% 上限在这里会
        // 强行停手、裁不干净。造一张留白比例超过旧上限、但仍在新上限内的图，确认新阈值下能
        // 裁到位（不是卡在旧的 15% 就停）。
        let (w, h, margin_frac) = (400u32, 600u32, 0.25f32);
        let margin = (w as f32 * margin_frac) as u32;
        let img = DynamicImage::ImageRgb8(RgbImage::from_fn(w, h, |x, y| {
            if x < margin || y < margin || x >= w - margin || y >= h - margin {
                image::Rgb([255, 255, 255])
            } else {
                image::Rgb([(x % 256) as u8, (y % 256) as u8, 128])
            }
        }));
        let mut buf = Vec::new();
        JpegEncoder::new_with_quality(&mut buf, 100).encode_image(&img).unwrap();
        let (got_w, got_h) = trim_dims(&buf).expect("留白应触发裁边");
        let old_cap = (w as f32 * 0.15) as u32;
        assert!(got_w < w - 2 * old_cap, "25% 留白不该被旧的 15% 上限卡住: got {got_w}");
        assert_eq!((got_w, got_h), (w - 2 * margin, h - 2 * margin), "留白在新上限内应该精确裁掉: got {got_w}x{got_h}");
    }

    #[test]
    fn skips_already_small_image() {
        let small = jpeg_of(800, 600);
        assert!(downscale_for_device(&small, test_screen()).is_none(), "已达标图不动（幂等、免二次损失）");
    }

    #[test]
    fn ignores_non_image_bytes() {
        assert!(downscale_for_device(b"not an image at all", test_screen()).is_none());
    }

    /// 透明 PNG 漫画页：透明区域要合成成白色，不能因为丢掉 alpha 变成黑色（彩色、黑白两条路径都查）。
    #[test]
    fn transparent_comic_page_flattens_onto_white_not_black() {
        use image::{GrayAlphaImage, RgbaImage};
        // 左半不透明黑色画线区，右半完全透明（底下存的颜色是 0 = 黑）；中间一列半透明灰。
        let (w, h) = (600u32, 1000u32);
        let rgba = RgbaImage::from_fn(w, h, |x, y| {
            if x < w / 2 {
                image::Rgba([(x % 7 * 30) as u8, (y % 5 * 40) as u8, 20, 255])
            } else if x == w / 2 {
                image::Rgba([0, 0, 0, 128])
            } else {
                image::Rgba([0, 0, 0, 0])
            }
        });
        let mut png = Vec::new();
        DynamicImage::ImageRgba8(rgba).write_to(&mut Cursor::new(&mut png), ImageFormat::Png).unwrap();
        let la = GrayAlphaImage::from_fn(w, h, |x, y| if x < w / 2 { image::LumaA([(y % 9 * 20) as u8, 255]) } else { image::LumaA([0, 0]) });
        let mut la_png = Vec::new();
        DynamicImage::ImageLumaA8(la).write_to(&mut Cursor::new(&mut la_png), ImageFormat::Png).unwrap();
        for (src, grayscale) in [(&png, false), (&png, true), (&la_png, false), (&la_png, true)] {
            let (img, _, _) = decode_trim_comic(src, grayscale).expect("能解码");
            let rgb = img.to_rgb8();
            // 裁边会把右侧的纯白透明区当留白裁掉一部分，取裁后最右一列检查：必须是白，不能是黑。
            let right = rgb.get_pixel(rgb.width() - 1, rgb.height() / 2).0;
            assert_eq!(right, [255, 255, 255], "透明区应合成成白色 (grayscale={grayscale})");
            let out = prep(src, test_area(), grayscale).expect("要补白");
            let back = image::load_from_memory(&out).unwrap().to_rgb8();
            let p = back.get_pixel(back.width() - 1, back.height() / 2).0;
            assert!(p.iter().all(|&c| c >= 250), "产物右侧不该发黑: {p:?} (grayscale={grayscale})");
        }
        let half = flatten_alpha_on_white(DynamicImage::ImageRgba8(RgbaImage::from_pixel(1, 1, image::Rgba([0, 0, 0, 128])))).to_rgb8();
        assert_eq!(half.get_pixel(0, 0).0, [127, 127, 127], "半透明黑合成白底 = 中灰");
    }

    #[test]
    fn within_decode_budget_boundary() {
        assert!(within_decode_budget(3000, 3000, MAX_COMIC_DECODE_PIXELS), "900 万像素，等于上限，应允许");
        assert!(!within_decode_budget(3001, 3000, MAX_COMIC_DECODE_PIXELS), "超一点点也该拒绝");
        assert!(!within_decode_budget(2000, 2000, 3_000_000), "调用方给的上限更小时按它");
    }

    /// 超限图（见 `MAX_DECODE_PIXELS` 文档：阈值按实测峰值内存定）：各解码入口都该直接放弃处理、原样保留，
    /// 不整张解出来。5001×5000 远超 900 万像素，足够验证调用链路。
    #[test]
    fn oversized_image_skipped_by_text_paths_but_comic_pages_are_processed() {
        let huge = gray_jpeg_of(5001, 5000, 0);
        assert!(downscale_for_device(&huge, test_screen()).is_none(), "文字书插图：超限图应跳过降采样");
        assert!(downscale_for_epub(&huge, test_screen()).is_none(), "文字书插图：超限图应跳过降采样");
        // 漫画页：2500 万像素照常处理（并行时由 imgpool 的像素额度独占，见 MAX_COMIC_DECODE_PIXELS）
        let out = prep(&huge, test_area(), false).expect("900 万像素以上的漫画页也要处理");
        let page = image::load_from_memory(&out).unwrap();
        assert_eq!(page.dimensions(), (842, 1455));
        assert_eq!(page.color(), image::ColorType::L8);
    }

    /// 调用方调小解码上限（设备上内存小）：超过的漫画页、插图不解码，原样保留；没超过的照常处理。
    #[test]
    fn caller_decode_limit_keeps_larger_images() {
        let page = gray_jpeg_of(2000, 3000, 0); // 600 万像素
        assert!(prepare_comic_page_limited(&page, test_area(), 1, false, 5_000_000).is_none(), "超过调用方上限的漫画页原样保留");
        assert_eq!(prepare_comic_page_limited(&page, test_area(), 1, false, 6_000_000), prep(&page, test_area(), false), "没超过的和缺省一样");
        assert!(downscale_for_epub_limited(&page, test_screen(), 5_000_000).is_none(), "插图同理");
        assert_eq!(downscale_for_epub_limited(&page, test_screen(), 9_000_000), downscale_for_epub(&page, test_screen()));
    }

    /// 文件头声明超过 [`MAX_COMIC_DECODE_PIXELS`] 的图（解压炸弹）：不解码，原样保留。
    #[test]
    fn comic_decode_refuses_decompression_bomb_by_header() {
        let mut bomb = black_jpeg(64, 64);
        // 把 SOF0 里的高、宽改成 9000×9000（8100 万像素），数据不变：只读文件头就该拒绝
        let sof = bomb.windows(2).position(|w| w == [0xFF, 0xC0]).expect("基线 JPEG 有 SOF0");
        bomb[sof + 5..sof + 9].copy_from_slice(&[0x23, 0x28, 0x23, 0x28]);
        assert_eq!(comic_header_dims(&bomb).map(|d| d.1), Some((9000, 9000)));
        assert!(decode_comic(&bomb, false, MAX_COMIC_DECODE_PIXELS).is_none());
        assert!(prep(&bomb, test_area(), true).is_none());
    }

    /// 文字书插图缩放改走 SIMD 后：尺寸与 `DynamicImage::resize` 完全相同、像素差很小；灰度 JPEG 仍是单分量。
    #[test]
    fn downscale_for_epub_keeps_resize_dimensions_and_gray_jpeg() {
        for &(w, h) in &[(2000u32, 1500u32), (1200, 3000), (955, 10), (3001, 2999)] {
            let src = jpeg_of(w, h);
            let want = image::load_from_memory(&src).unwrap().resize(test_screen().width, test_screen().height, FilterType::Lanczos3);
            let got = image::load_from_memory(&downscale_for_epub(&src, test_screen()).expect("超框要缩")).unwrap();
            assert_eq!(got.dimensions(), want.dimensions(), "{w}x{h}");
            let (a, b) = (got.to_rgb8(), want.to_rgb8());
            let mean_diff = a.as_raw().iter().zip(b.as_raw()).map(|(x, y)| x.abs_diff(*y) as u64).sum::<u64>() as f64 / a.as_raw().len() as f64;
            assert!(mean_diff < 2.0, "{w}x{h} 像素均差 {mean_diff}");
        }
        let gray = gray_jpeg_of(1800, 2400, 0);
        let out = downscale_for_epub(&gray, test_screen()).unwrap();
        assert!(matches!(image::load_from_memory(&out).unwrap(), DynamicImage::ImageLuma8(_)), "灰度 JPEG 不该被写成 3 分量");
    }

    /// 排版算术：受限边两侧正好 `m` 像素，另一条边两侧不少于 `m`（差奇数时右、下多 1），画布就是阅读范围，宽高比不变
    /// （另一条边是按比例四舍五入的结果，误差不超过半像素），不拉伸不压扁。
    #[test]
    fn layout_puts_exact_margin_on_constrained_axis_and_keeps_aspect() {
        let areas = [profile::get("ireader").unwrap().readable(profile::Format::Epub), test_area(), Screen { width: 300, height: 400 }];
        let sizes = [(1091u32, 1592u32), (1687, 2480), (566, 800), (700, 1600), (1600, 1000), (2000, 2000), (1264, 1680), (842, 1455), (3001, 4999), (301, 1999)];
        for area in areas {
            for m in [0u32, 1, 3] {
                for (w, h) in sizes {
                    let l = comic_layout(w, h, area, m, true);
                    let ctx = format!("{w}x{h} → {area:?} m={m}: {l:?}");
                    assert_eq!((l.canvas_w, l.canvas_h), (area.width, area.height), "{ctx}");
                    let (left, right) = (l.x, l.canvas_w - l.x - l.nw);
                    let (top, bottom) = (l.y, l.canvas_h - l.y - l.nh);
                    let width_bound = (left, right) == (m, m);
                    assert!(width_bound || (top, bottom) == (m, m), "受限边两侧正好 m: {ctx}");
                    assert!(left >= m && right >= m && top >= m && bottom >= m, "四边都不少于 m: {ctx}");
                    assert!(right - left <= 1 && bottom - top <= 1, "居中: {ctx}");
                    // 宽高比：另一条边 = 受限边 × 原比例，四舍五入
                    let err = if width_bound { l.nh as f64 - h as f64 * l.nw as f64 / w as f64 } else { l.nw as f64 - w as f64 * l.nh as f64 / h as f64 };
                    assert!(err.abs() <= 0.5, "宽高比误差 {err}: {ctx}");
                }
            }
        }
    }

    /// 端到端量产物：整张纯黑、画面到边的页，放大（掌阅 1264×1680，乱马同款 1091×1592）和缩小（死亡笔记同款
    /// 1687×2480 到 Move 842×1455）后，解码量深色内容到四边的距离：受限边两侧 1px，另一条边两侧相差不超过 1px。
    #[test]
    fn output_pages_measure_one_pixel_on_constrained_axis() {
        let ireader = profile::get("ireader").unwrap().readable(profile::Format::Epub);
        for (w, h, area) in [(1091u32, 1592u32, ireader), (1687, 2480, test_area()), (1300, 900, test_area())] {
            let out = prep(&black_jpeg(w, h), area, false).expect("要处理");
            let page = image::load_from_memory(&out).unwrap();
            assert_eq!(page.dimensions(), (area.width, area.height), "{w}x{h}");
            let (l, t, r, b) = dark_margins(&page);
            assert!((l, r) == (1, 1) || (t, b) == (1, 1), "{w}x{h}: 受限边两侧 1px，实际 左{l} 上{t} 右{r} 下{b}");
            assert!(l.abs_diff(r) <= 1 && t.abs_diff(b) <= 1, "{w}x{h}: 居中，实际 左{l} 上{t} 右{r} 下{b}");
        }
        // 白边可配：3px
        let out = prepare_comic_page_for_epub(&black_jpeg(1091, 1592), ireader, 3, false).unwrap();
        let (l, t, r, b) = dark_margins(&image::load_from_memory(&out).unwrap());
        assert!((t, b) == (3, 3) && l.abs_diff(r) <= 1, "左{l} 上{t} 右{r} 下{b}");
    }

    /// 不放大的排版（PNG 等、或放大倍数超过上限）：图不缩放，画布是阅读范围的宽高比，白边按比例缩小但至少 1px。
    #[test]
    fn native_scale_layout_pads_to_area_aspect_without_scaling() {
        let area = test_area();
        for (w, h) in [(700u32, 1000u32), (400, 1200), (600, 400)] {
            let l = comic_layout(w, h, area, 1, false);
            assert_eq!((l.nw, l.nh), (w, h), "不缩放");
            let err = l.canvas_w as f64 / l.canvas_h as f64 - area.aspect() as f64;
            assert!(err.abs() < 2.0 / l.canvas_h.min(l.canvas_w) as f64, "{w}x{h}: 画布比例 {}x{}", l.canvas_w, l.canvas_h);
            let (left, right, top, bottom) = (l.x, l.canvas_w - l.x - w, l.y, l.canvas_h - l.y - h);
            assert!((left, right) == (1, 1) || (top, bottom) == (1, 1), "{w}x{h}: {l:?}");
            assert!(left >= 1 && right >= 1 && top >= 1 && bottom >= 1);
        }
        // 白边按比例：300×400 的阅读范围、8px 白边，150×100 的图（宽受限，一半比例尺）→ 白边 4px
        let l = comic_layout(150, 100, Screen { width: 300, height: 400 }, 8, false);
        assert_eq!((l.x, l.canvas_w), (4, 158));
        assert_eq!(comic_layout(150, 100, Screen { width: 300, height: 400 }, 0, false).x, 0, "白边 0 时不补");
    }

    /// 已经排好的页再处理一遍原样保留（原字节），不一代代重编码；正好是阅读范围大小、画面顶到边的页也不为 1px 重编码。
    #[test]
    fn already_laid_out_pages_are_left_untouched() {
        let area = test_area();
        let once = prep(&gray_jpeg_of(1091, 1592, 0), area, false).unwrap();
        assert!(prep(&once, area, false).is_none(), "产物再跑一遍：原样");
        assert!(prep(&black_jpeg(842, 1455), area, false).is_none(), "阅读范围大小、画面到边：原样");
        assert!(prep(&black_jpeg(842, 1455), area, true).is_none(), "本来就是灰度：黑白屏也原样");
        // 差得多的不算：同尺寸但四周 60px 白边，要裁掉重排
        assert!(prep(&gray_jpeg_of(842, 1455, 60), area, false).is_some());
    }

    /// 固定版式按画布显示的判定（`comicfxl::fills_canvas`）和这里的排版一致：排过版的页（放大、缩小、不放大补白的 PNG、
    /// 原样保留的已排好的页）都是画布比例；装饰小图不是。
    #[test]
    fn laid_out_pages_fill_the_fixed_layout_canvas() {
        let kindle = profile::get("kindle").unwrap().comic_readable();
        let png = |w: u32, h: u32| {
            let mut b = Vec::new();
            DynamicImage::ImageRgb8(RgbImage::from_fn(w, h, |x, y| image::Rgb([(x % 256) as u8, (y % 256) as u8, 9]))).write_to(&mut Cursor::new(&mut b), ImageFormat::Png).unwrap();
            b
        };
        let cases: [(Vec<u8>, bool); 9] = [
            (black_jpeg(566, 800), true),
            (black_jpeg(1687, 2480), true),
            (black_jpeg(1300, 900), true),
            (black_jpeg(1272, 1696), true),
            (png(700, 1000), true),
            (png(450, 1203), true),
            (png(401, 1203), false), // 短边不到阅读范围宽的 1/3：按装饰小图只裁边
            (png(900, 600), true),
            (black_jpeg(300, 200), false),
        ];
        for (src, want) in cases {
            let out = prepare_comic_page_for_epub(&src, kindle, 1, true).unwrap_or_else(|| src.clone());
            let dims = image::load_from_memory(&out).unwrap().dimensions();
            assert_eq!(crate::comicfxl::fills_canvas(dims, kindle.width, kindle.height), want, "{dims:?}");
        }
    }

    fn gif_of(frames: &[image::RgbaImage]) -> Vec<u8> {
        let mut buf = Vec::new();
        {
            let mut enc = image::codecs::gif::GifEncoder::new(&mut buf);
            enc.encode_frames(frames.iter().map(|f| image::Frame::new(f.clone()))).unwrap();
        }
        buf
    }

    /// 静态 GIF 页转 PNG 并照常排版；动图原样保留（只取第一帧会丢内容）。
    #[test]
    fn static_gif_page_becomes_png_and_animated_gif_is_kept() {
        let area = Screen { width: 300, height: 400 };
        let page = image::RgbaImage::from_fn(200, 280, |x, y| if (x / 20 + y / 20) % 2 == 0 { image::Rgba([0, 0, 0, 255]) } else { image::Rgba([200, 60, 60, 255]) });
        let gif = gif_of(std::slice::from_ref(&page));
        assert_eq!(pixel_count(&gif), 200 * 280, "GIF 也按文件头算像素额度");
        let out = prep(&gif, area, false).expect("静态 GIF 要处理");
        assert_eq!(image::guess_format(&out).unwrap(), ImageFormat::Png);
        assert_eq!(image::load_from_memory(&out).unwrap().dimensions(), (212, 282), "GIF→PNG 不放大：高受限，白边 1px，宽按 300:400 算");
        assert_eq!(converted_media_type("OEBPS/p1.gif", &out), Some("image/png"));
        let gray = prep(&gif, area, true).unwrap();
        assert_eq!(image::load_from_memory(&gray).unwrap().color(), image::ColorType::L8, "黑白屏转灰度");
        let other = image::RgbaImage::from_pixel(200, 280, image::Rgba([90, 90, 200, 255]));
        assert!(prep(&gif_of(&[page, other]), area, false).is_none(), "动图原样保留");
        // 装饰小图的 GIF（短边不到阅读范围宽 1/3）没有白边：原样
        let tiny = gif_of(&[image::RgbaImage::from_fn(60, 60, |x, y| image::Rgba([((x + y) % 2 * 200) as u8, 0, 0, 255]))]);
        assert!(prep(&tiny, area, false).is_none());
        // 文字书的插图路径不碰 GIF
        assert!(downscale_for_epub(&gif, Screen { width: 100, height: 100 }).is_none());
    }

    /// WebP：无损的转 PNG；有损、无损按 RIFF 块识别。
    #[test]
    fn webp_page_is_converted_by_its_encoding() {
        let area = Screen { width: 300, height: 400 };
        let img = image::RgbImage::from_fn(240, 380, |x, y| image::Rgb([(x % 256) as u8, (y % 256) as u8, 30]));
        let mut webp = Vec::new();
        image::codecs::webp::WebPEncoder::new_lossless(&mut webp).encode(img.as_raw(), 240, 380, image::ExtendedColorType::Rgb8).unwrap();
        assert_eq!(webp_is_lossless(&webp), Some(true));
        let out = prep(&webp, area, false).expect("WebP 要处理");
        assert_eq!(image::guess_format(&out).unwrap(), ImageFormat::Png, "无损 WebP → PNG");
        assert_eq!(converted_media_type("a/p.webp", &out), Some("image/png"));
        // 有损（VP8）与扩展格式（VP8X 在前）的块识别；认不出的返回 None
        let chunk = |fourcc: &[u8], len: u32| [fourcc, &len.to_le_bytes()[..], &vec![0u8; len as usize]].concat();
        let riff = |body: Vec<u8>| [&b"RIFF"[..], &((body.len() + 4) as u32).to_le_bytes()[..], b"WEBP", &body].concat();
        assert_eq!(webp_is_lossless(&riff(chunk(b"VP8 ", 10))), Some(false));
        assert_eq!(webp_is_lossless(&riff([chunk(b"VP8X", 10), chunk(b"VP8 ", 3)].concat())), Some(false), "奇数长度的块按偶数对齐");
        assert_eq!(webp_is_lossless(&riff([chunk(b"VP8X", 10), chunk(b"VP8L", 4)].concat())), Some(true));
        assert_eq!(webp_is_lossless(b"RIFF\0\0\0\0WEBP"), None);
        assert_eq!(converted_media_type("a/p.jpg", &black_jpeg(8, 8)), None, "没换格式");
        assert_eq!(converted_media_type("a/p.webp", &black_jpeg(8, 8)), Some("image/jpeg"), "有损 WebP 转成 JPEG");
    }

    /// 横放存储、EXIF 方向 6（顺时针转 90° 显示）的 JPEG：原图左半黑、右半白，摆正后上半黑、下半白。
    fn exif_rotated_jpeg(w: u32, h: u32) -> Vec<u8> {
        let img = image::GrayImage::from_fn(w, h, |x, _| image::Luma([if x < w / 2 { 0 } else { 255 }]));
        // TIFF 头（小端）+ 一个 IFD：0x0112 Orientation，SHORT，值 6
        let exif = vec![0x49, 0x49, 42, 0, 8, 0, 0, 0, 1, 0, 0x12, 0x01, 3, 0, 1, 0, 0, 0, 6, 0, 0, 0, 0, 0, 0, 0];
        let mut buf = Vec::new();
        let mut enc = JpegEncoder::new_with_quality(&mut buf, 95);
        image::ImageEncoder::set_exif_metadata(&mut enc, exif).unwrap();
        image::ImageEncoder::write_image(enc, img.as_raw(), w, h, image::ExtendedColorType::L8).unwrap();
        buf
    }

    /// 上下两半的平均灰度。
    fn top_bottom_luma(img: &DynamicImage) -> (f64, f64) {
        let g = img.to_luma8();
        let (w, h) = g.dimensions();
        let avg = |y0: u32, y1: u32| (y0..y1).flat_map(|y| (0..w).map(move |x| (x, y))).map(|(x, y)| g.get_pixel(x, y)[0] as f64).sum::<f64>() / ((y1 - y0) * w) as f64;
        (avg(0, h / 3), avg(h - h / 3, h))
    }

    #[test]
    fn exif_orientation_is_applied_before_reencoding() {
        let src = exif_rotated_jpeg(3000, 2000);
        assert_eq!(decode_oriented(&src).unwrap().dimensions(), (2000, 3000));
        // 文字书插图：按摆正后的竖图缩进竖框，像素也摆正（重编码不带 EXIF）
        let out = image::load_from_memory(&downscale_for_epub(&src, test_screen()).unwrap()).unwrap();
        assert!(out.height() > out.width(), "{:?}", out.dimensions());
        let (top, bottom) = top_bottom_luma(&out);
        assert!(top < 30.0 && bottom > 225.0, "上黑下白: {top} {bottom}");
        // 网络图（按朝向选框）：摆正后是竖图，进竖框 954×1696，而不是按存储的横向宽高进横框
        let out = image::load_from_memory(&downscale_for_device(&src, test_screen()).unwrap()).unwrap();
        assert_eq!(out.dimensions(), (954, 1431), "2000×3000 摆正后按竖框缩");
        // 摆正后本来就在框里：不动（原字节连同 EXIF 保留）
        assert!(downscale_for_epub(&exif_rotated_jpeg(1200, 900), test_screen()).is_none());
        // 漫画页：同样先摆正再排版——和直接存成竖图（不带 EXIF）的同一张图结果逐字节相同（PNG 无损，比得了字节）
        let raw = image::RgbImage::from_fn(1200, 800, |x, y| image::Rgb([(x * 7 % 256) as u8, (y * 5 % 256) as u8, ((x * y) % 256) as u8]));
        let png = |img: &image::RgbImage, exif: Option<Vec<u8>>| {
            let mut buf = Vec::new();
            let mut enc = image::codecs::png::PngEncoder::new(&mut buf);
            if let Some(e) = exif {
                image::ImageEncoder::set_exif_metadata(&mut enc, e).unwrap();
            }
            image::ImageEncoder::write_image(enc, img.as_raw(), img.width(), img.height(), image::ExtendedColorType::Rgb8).unwrap();
            buf
        };
        let exif6 = vec![0x49, 0x49, 42, 0, 8, 0, 0, 0, 1, 0, 0x12, 0x01, 3, 0, 1, 0, 0, 0, 6, 0, 0, 0, 0, 0, 0, 0];
        let tagged = png(&raw, Some(exif6));
        let upright = png(&image::imageops::rotate90(&raw), None);
        assert_eq!(decode_oriented(&tagged).unwrap().to_rgb8(), image::imageops::rotate90(&raw));
        assert_eq!(prep(&tagged, test_area(), false).unwrap(), prep(&upright, test_area(), false).unwrap());
        assert_eq!(prep(&tagged, test_area(), true).unwrap(), prep(&upright, test_area(), true).unwrap());
        // 已经是阅读范围大小的页，带 EXIF 方向也要重编码（不能原样沿用靠 EXIF 才正的字节）
        let exact = exif_rotated_jpeg(test_area().height, test_area().width);
        assert!(prep(&exact, test_area(), false).is_some());
    }

    #[test]
    fn gif_frames_counted_without_decoding() {
        let f = image::RgbaImage::from_pixel(4, 4, image::Rgba([1, 2, 3, 255]));
        assert_eq!(gif_frame_count(&gif_of(std::slice::from_ref(&f)), 2), Some(1));
        assert_eq!(gif_frame_count(&gif_of(&[f.clone(), f.clone(), f.clone()]), 2), Some(2));
        let one = gif_of(&[f]);
        assert_eq!(gif_frame_count(&one[..one.len() - 3], 2), None, "截断");
        assert_eq!(gif_frame_count(b"GIF89a", 2), None);
    }
}
