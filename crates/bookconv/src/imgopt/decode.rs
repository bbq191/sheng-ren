//! 图片解码、文件头、缩放与透明合成（拆分自 `imgopt.rs`）。

use image::ImageFormat;
use std::io::Cursor;

use super::page8::Page8;

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

/// 漫画页单张允许解码的像素数上限：只防解压炸弹（几 KB 的文件声明几亿像素），真实的高分辨率扫描页照常处理。
/// 6400 万像素约是 A4 600dpi 双页跨页（约 7000×9900 的一半再大一些）；按实测约 9–16MB/百万像素，最坏单张峰值约 1GB。
/// 超过 [`crate::imgpool::PIXEL_BUDGET`] 的大页开工时独占全部额度，不与别的图同时处理，所以总内存仍有上界。
/// 2026-09-29 之前漫画页也用 [`MAX_DECODE_PIXELS`]，超过 900 万像素的页原样保留（没缩放、没补白、没转灰度）。
pub const MAX_COMIC_DECODE_PIXELS: u64 = 64_000_000;

/// 重编码 JPEG 质量（0–100）。85 = 视觉无损级，体积/画质平衡；e-ink 上更看不出差异。
pub(crate) const JPEG_QUALITY: u8 = 85;
/// 漫画页重编码质量（缩小、放大、补白都用它）——漫画不压画质：95 接近视觉无损。
/// 2026-09-29 之前预放大的页用 85（怕体积暴涨），维护者定：漫画一律 95。
/// 公开给自己排版整页图的调用方（[`decode_page`] → [`trim_page`] → [`Page8::encode`]）用同一个质量。
pub const JPEG_QUALITY_COMIC: u8 = 95;

/// 保比缩进 `max_w × max_h` 框（宽高比保持、保原格式，JPEG 质量 [`JPEG_QUALITY`]），只在超框时动；返回新字节或 `None`
/// （已达标 / 非 JPEG·PNG / 解码失败 / 重编码没变小 → 调用方原样保留）。
/// 带 EXIF 方向的图按**摆正后**的宽高判断超不超框；要重编码时先摆正（重编码不带 EXIF，不摆正就转歪了，见 [`ImgHead`]）。
pub(crate) fn downscale_into(bytes: &[u8], max_w: u32, max_h: u32, max_px: u64) -> Option<Vec<u8>> {
    let head = ImgHead::read(bytes, is_jpeg_or_png, true)?;
    let (w, h) = (head.w, head.h);
    if w <= max_w && h <= max_h {
        return None; // 已达标：不解码不重编码（避免无谓的二次有损压缩；2473 页漫画只读头是秒级、全解是分钟级）
    }
    // 极端高分辨率原图：不整个解出来，原样保留（见 MAX_DECODE_PIXELS 文档）
    let img = head.decode(bytes, text_decode_limit(max_px))?;
    // 缩放走 SIMD 版 Lanczos3（`resize_lanczos3`，同漫画页；尺寸算法与 `DynamicImage::resize` 相同），编码时灰度保持单分量。
    // 此前 `img.resize` 是 `image` 自带的标量实现（实测慢约 20 倍，且中间缓冲是 f32，内存大），`encode_image` 还会把
    // 灰度图写成 3 分量 JPEG（体积白涨）——文字书插图每张都走这里（2026-09-25 审计）。
    let (nw, nh) = fit_within(w, h, max_w, max_h);
    let out = match Page8::try_from_dynamic(img) {
        Ok(page) => page
            .resize_lanczos3(nw, nh)
            .encode(head.fmt, JPEG_QUALITY)?,
        // 带透明、16 位等其它类型（只有 PNG 会到这里；JPEG 不收这种类型）：按预乘 alpha 缩、写回 PNG，同背景图
        // （[`resize_alpha_png`]；v56 以前用 `image` 自带的不预乘缩放，透明处存的是黑色的图边上出黑边）
        Err(img) if head.fmt == ImageFormat::Png => resize_alpha_png(img, nw, nh)?,
        Err(_) => return None,
    };
    // 只有确实变小才采用（极端下重编码可能变大 → 保留原图，不倒退体积）。
    (out.len() < bytes.len()).then_some(out)
}

/// 保比放进 `max_w × max_h` 框后的尺寸（与 `image` 库 `DynamicImage::resize` 的 `resize_dimensions` 同一算法：取宽高两个比例的
/// 较小者、四舍五入、至少 1）。只用于缩小，结果不会超过原尺寸。
fn fit_within(w: u32, h: u32, max_w: u32, max_h: u32) -> (u32, u32) {
    let ratio = f64::min(max_w as f64 / w as f64, max_h as f64 / h as f64);
    (
        ((w as f64 * ratio).round() as u32).max(1),
        ((h as f64 * ratio).round() as u32).max(1),
    )
}

/// 图片里 EXIF 的方向标签（JPEG、PNG 的 eXIf、WebP 的 EXIF 块）；没有或读不出 → 不用转。只读文件头，不解码像素。
fn orientation_of(bytes: &[u8], fmt: ImageFormat) -> image::metadata::Orientation {
    use image::ImageDecoder;
    image::ImageReader::with_format(Cursor::new(bytes), fmt)
        .into_decoder()
        .and_then(|mut d| d.orientation())
        .unwrap_or(image::metadata::Orientation::NoTransforms)
}

/// 外来图片的文件头：格式、EXIF 方向、**摆正后**的宽高（只读文件头，不解码像素）。"读头 → EXIF 方向 → 解码上限 → 解码 → 摆正"
/// 这一套由它统一：缩插图、透明图合成白底、背景图、漫画页、缩略图都是先 [`ImgHead::read`]（按头里的宽高决定要不要动），
/// 要动再 [`ImgHead::decode`]。重编码写不回 EXIF：像素不摆正的话，在按 EXIF 显示的阅读器上原来正的图就转歪了；摆正后像素本身
/// 就是该显示的样子，认不认 EXIF 的阅读器看到的都一样。
#[derive(Debug, Clone, Copy)]
pub(crate) struct ImgHead {
    pub(crate) fmt: ImageFormat,
    pub(crate) orientation: image::metadata::Orientation,
    /// 摆正后的宽高（转 90°/270° 的已互换）。
    pub(crate) w: u32,
    pub(crate) h: u32,
}

impl ImgHead {
    /// 读文件头：格式按魔数认、`accept` 不收的格式 → `None`；`apply_exif = false` 时不看 EXIF 方向（图来自不认 EXIF 的容器，
    /// 见 [`PageDecode::apply_exif`]）。
    pub(crate) fn read(
        bytes: &[u8],
        accept: impl Fn(ImageFormat) -> bool,
        apply_exif: bool,
    ) -> Option<ImgHead> {
        let (fmt, dims) = header_dims_if(bytes, accept)?;
        let orientation = if apply_exif {
            orientation_of(bytes, fmt)
        } else {
            image::metadata::Orientation::NoTransforms
        };
        let (w, h) = upright_dims(dims, orientation);
        Some(ImgHead {
            fmt,
            orientation,
            w,
            h,
        })
    }

    /// 像素数（w×h，和转不转无关）。
    pub(crate) fn pixels(&self) -> u64 {
        (self.w as u64) * (self.h as u64)
    }

    /// 要按 EXIF 方向转（像素和原图不同了）。
    pub(crate) fn rotated(&self) -> bool {
        self.orientation != image::metadata::Orientation::NoTransforms
    }

    /// 解码并摆正。像素数超过 `max_px`（解码上限：防解压炸弹和极端大图把内存顶爆）的不解，解不开的也是 `None`——调用方原样保留原图。
    /// 不兜 panic（[`guard`] 由调用方套在外面）。
    pub(crate) fn decode(&self, bytes: &[u8], max_px: u64) -> Option<image::DynamicImage> {
        if self.pixels() > max_px {
            return None;
        }
        let mut img = image::load_from_memory_with_format(bytes, self.fmt).ok()?;
        img.apply_orientation(self.orientation);
        Some(img)
    }
}

/// 文字书插图（含背景图、透明图合成白底）的解码上限：[`MAX_DECODE_PIXELS`] 和调用方给的 `max_px`（`OptimizeOpts::limits.max_decode_pixels`，
/// 缺省 [`MAX_COMIC_DECODE_PIXELS`]，比 900 万大，等于不起作用）中小的那个。
pub(crate) fn text_decode_limit(max_px: u64) -> u64 {
    MAX_DECODE_PIXELS.min(max_px)
}

pub(crate) fn is_jpeg_or_png(f: ImageFormat) -> bool {
    matches!(f, ImageFormat::Jpeg | ImageFormat::Png)
}

/// 漫画页能处理的格式（JPEG/PNG/GIF/WebP）。
pub(crate) fn is_comic_format(f: ImageFormat) -> bool {
    matches!(
        f,
        ImageFormat::Jpeg | ImageFormat::Png | ImageFormat::Gif | ImageFormat::WebP
    )
}

/// 这个方向摆正时宽高互换（转 90°/270° 的四种）。
fn swaps_axes(o: image::metadata::Orientation) -> bool {
    use image::metadata::Orientation::*;
    matches!(o, Rotate90 | Rotate270 | Rotate90FlipH | Rotate270FlipH)
}

/// 存储的宽高 `(w, h)` 按方向 `o` 摆正后的宽高（转 90°/270° 的互换）。
fn upright_dims((w, h): (u32, u32), o: image::metadata::Orientation) -> (u32, u32) {
    if swaps_axes(o) {
        (h, w)
    } else {
        (w, h)
    }
}

/// 跑一段处理外来图片的代码，兜住解码器的 panic，按处理失败（`None`）算。书里、网上的图都是外部输入，第三方解码器遇到畸形
/// 图片偶尔会 panic：不兜住的话一张坏图会摔掉整本书的优化（图片 worker 的 panic 经 `thread::scope` 重新抛给调用方；
/// 抓远程图、换封面在主线程，直接 panic）。优化器处理图片、读图片头、抓远程图、解封面共用。
pub(crate) fn guard<T>(f: impl FnOnce() -> Option<T>) -> Option<T> {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)).unwrap_or(None)
}

/// 测试用的 4×4 两帧动图 WebP（无损，PIL 生成）：第一帧红 (200,30,30)、第二帧蓝。
#[cfg(test)]
pub(crate) const TEST_ANIM_WEBP: &[u8] = &[
    82, 73, 70, 70, 136, 0, 0, 0, 87, 69, 66, 80, 86, 80, 56, 88, 10, 0, 0, 0, 2, 0, 0, 0, 3, 0, 0,
    3, 0, 0, 65, 78, 73, 77, 6, 0, 0, 0, 0, 0, 0, 0, 0, 0, 65, 78, 77, 70, 42, 0, 0, 0, 0, 0, 0, 0,
    0, 0, 3, 0, 0, 3, 0, 0, 100, 0, 0, 2, 86, 80, 56, 76, 17, 0, 0, 0, 47, 3, 192, 0, 0, 7, 80,
    143, 34, 215, 163, 255, 129, 136, 232, 127, 0, 0, 65, 78, 77, 70, 42, 0, 0, 0, 0, 0, 0, 0, 0,
    0, 3, 0, 0, 3, 0, 0, 100, 0, 0, 0, 86, 80, 56, 76, 17, 0, 0, 0, 47, 3, 192, 0, 0, 7, 80, 143,
    122, 20, 185, 255, 129, 136, 232, 127, 0, 0,
];

/// WebP → PNG（KFX 不认 WebP；Move 也不认，profile `webp = false`）：解码后无损编码，像素不变（有损 WebP 的像素就是它解出来的样子）。
/// 动图取第一帧（2026-10-19 定：掌阅自己就显示第一帧，比 Kindle、Move 上整张不显示强）。解出来带透明通道、其实全不透明的
/// 存成 RGB（动图一律解成 RGBA）：像素不变，插图加深、Kindle 的白底合成照不透明的图算。
/// 书里的图是外部输入：解码前按文件头的宽高核对解码上限（同漫画页 [`MAX_COMIC_DECODE_PIXELS`]，防几 KB 的文件声明几亿像素），
/// 解码器的 panic 由 [`guard`] 兜住——KFX 转换不在 `catch_unwind` 里，不兜的话一张坏图摔掉整个 sync。
pub fn webp_to_png(b: &[u8]) -> Option<Vec<u8>> {
    use image::ImageDecoder;
    guard(|| {
        let dec = image::codecs::webp::WebPDecoder::new(std::io::Cursor::new(b)).ok()?;
        let (w, h) = dec.dimensions();
        if u64::from(w) * u64::from(h) > MAX_COMIC_DECODE_PIXELS {
            return None;
        }
        let img = image::DynamicImage::from_decoder(dec).ok()?;
        let img = match img {
            image::DynamicImage::ImageRgba8(rgba) if rgba.pixels().all(|p| p.0[3] == 255) => {
                image::DynamicImage::ImageRgb8(image::DynamicImage::ImageRgba8(rgba).to_rgb8())
            }
            other => other,
        };
        let mut out = Vec::new();
        img.write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png)
            .ok()?;
        Some(out)
    })
}

/// 是 [`webp_to_png`] 会转的 WebP（认得出、像素数不超过上限；动图也算）。只读文件头，不解码像素。
pub fn is_convertible_webp(b: &[u8]) -> bool {
    use image::ImageDecoder;
    guard(|| {
        let dec = image::codecs::webp::WebPDecoder::new(std::io::Cursor::new(b)).ok()?;
        let (w, h) = dec.dimensions();
        Some(u64::from(w) * u64::from(h) <= MAX_COMIC_DECODE_PIXELS)
    })
    .unwrap_or(false)
}

/// 解码并按 EXIF 方向摆正（格式按魔数认，见 [`ImgHead`]）。缩略图等只读用途也用它；不设解码上限。
/// 解不开（含解码器 panic，见 [`guard`]）→ `None`。
pub fn decode_oriented(bytes: &[u8]) -> Option<image::DynamicImage> {
    decode_capped(bytes, u64::MAX, true)
}

/// 解码（格式按魔数认，JPEG/PNG/GIF/WebP）：文件头声明的像素数超过 `max_px` 的不解；`apply_exif` 时按 EXIF 方向摆正（见 [`ImgHead`]）。
/// 解不开（含解码器 panic，见 [`guard`]）→ `None`。
pub fn decode_capped(bytes: &[u8], max_px: u64, apply_exif: bool) -> Option<image::DynamicImage> {
    guard(|| ImgHead::read(bytes, |_| true, apply_exif)?.decode(bytes, max_px))
}

/// 图片头部声明的像素数（不解码，JPEG/PNG/GIF/WebP）；读不出来按 100 万像素估，给并行内存预算用（[`crate::imgpool`]）。
pub fn pixel_count(bytes: &[u8]) -> u64 {
    comic_header_dims(bytes)
        .map(|(_, (w, h))| (w as u64) * (h as u64))
        .unwrap_or(1_000_000)
}

/// 只读文件头取 (格式, 宽, 高)，不解码像素（不按 EXIF 方向换宽高）。非 JPEG/PNG → None。
pub fn header_dims(bytes: &[u8]) -> Option<(ImageFormat, (u32, u32))> {
    header_dims_if(bytes, is_jpeg_or_png)
}

/// 漫画页能处理的格式（JPEG/PNG/GIF/WebP）的 (格式, 宽, 高)，只读文件头。
pub(crate) fn comic_header_dims(bytes: &[u8]) -> Option<(ImageFormat, (u32, u32))> {
    header_dims_if(bytes, is_comic_format)
}

fn header_dims_if(
    bytes: &[u8],
    ok: impl Fn(ImageFormat) -> bool,
) -> Option<(ImageFormat, (u32, u32))> {
    let fmt = crate::util::image_kind(bytes)?.format;
    if !ok(fmt) {
        return None;
    }
    let dims = image::ImageReader::with_format(Cursor::new(bytes), fmt)
        .into_dimensions()
        .ok()?;
    Some((fmt, dims))
}

/// 带透明像素的 PNG 合成到白底，仍写成 PNG（灰度+透明 → 灰度，其余 → RGB；按 EXIF 摆正）。不是 PNG、没有透明通道、
/// alpha 全是不透明、超过解码上限（[`MAX_DECODE_PIXELS`] 和 `max_px` 中小的那个）、解不开 → `None`（原样保留）。
/// 给不认透明的阅读器用（[`crate::imgalpha`]）。
pub fn flatten_transparent_png(bytes: &[u8], max_px: u64) -> Option<Vec<u8>> {
    let img = ImgHead::read(bytes, |f| f == ImageFormat::Png, true)?
        .decode(bytes, text_decode_limit(max_px))?;
    if !img.color().has_alpha() {
        return None;
    }
    // 摆正不影响有没有透明像素，先摆正再看结果一样
    let transparent = match &img {
        image::DynamicImage::ImageLumaA8(i) => i.pixels().any(|p| p.0[1] < 255),
        image::DynamicImage::ImageRgba8(i) => i.pixels().any(|p| p.0[3] < 255),
        other => other.to_rgba8().pixels().any(|p| p.0[3] < 255),
    };
    if !transparent {
        return None;
    }
    let flat = flatten_alpha_on_white(img);
    let mut out = Vec::new();
    flat.write_to(&mut Cursor::new(&mut out), ImageFormat::Png)
        .ok()?;
    Some(out)
}

/// 带透明（或 16 位等）的 PNG 缩放后写回 PNG（插图 [`downscale_into`]、背景图 [`downscale_background`] 共用）。透明的按预乘 alpha 缩
/// （`fast_image_resize` 缺省如此），透明像素的颜色不会渗进边缘（`image` 自带的缩放不预乘，调色板图透明处常是黑色，边上会出黑边）。
/// 16 位的降成 8 位。
pub(crate) fn resize_alpha_png(img: image::DynamicImage, nw: u32, nh: u32) -> Option<Vec<u8>> {
    use fast_image_resize::PixelType;
    use image::{ExtendedColorType, ImageEncoder};
    let (w, h) = (img.width(), img.height());
    let (raw, pt, ct) = match img {
        image::DynamicImage::ImageLumaA8(i) => {
            (i.into_raw(), PixelType::U8x2, ExtendedColorType::La8)
        }
        image::DynamicImage::ImageLuma16(_) => (
            img.into_luma8().into_raw(),
            PixelType::U8,
            ExtendedColorType::L8,
        ),
        i if !i.color().has_alpha() => (
            i.into_rgb8().into_raw(),
            PixelType::U8x3,
            ExtendedColorType::Rgb8,
        ),
        i if !i.color().has_color() => (
            i.into_luma_alpha8().into_raw(),
            PixelType::U8x2,
            ExtendedColorType::La8,
        ),
        i => (
            i.into_rgba8().into_raw(),
            PixelType::U8x4,
            ExtendedColorType::Rgba8,
        ),
    };
    let dst = fir_lanczos3(&raw, w, h, pt, nw, nh)?;
    let mut out = Vec::new();
    image::codecs::png::PngEncoder::new(&mut out)
        .write_image(&dst, nw, nh, ct)
        .ok()?;
    Some(out)
}

/// `fast_image_resize` 的 Lanczos3 卷积缩放（[`resize_alpha_png`]、[`Page8::resize_lanczos3`] 共用同一套配置）：
/// `raw` 是 `w × h`、像素类型 `pt` 的原始像素，返回 `dw × dh` 的像素；库报错 → `None`。
pub(crate) fn fir_lanczos3(
    raw: &[u8],
    w: u32,
    h: u32,
    pt: fast_image_resize::PixelType,
    dw: u32,
    dh: u32,
) -> Option<Vec<u8>> {
    use fast_image_resize::images::{Image, ImageRef};
    use fast_image_resize::{FilterType as FirFilter, ResizeAlg, ResizeOptions, Resizer};
    let src = ImageRef::new(w, h, raw, pt).ok()?;
    let mut dst = Image::new(dw, dh, pt);
    let opts = ResizeOptions::new().resize_alg(ResizeAlg::Convolution(FirFilter::Lanczos3));
    Resizer::new().resize(&src, &mut dst, &opts).ok()?;
    Some(dst.into_vec())
}

/// 带透明通道的图合成到白底：灰度+alpha → `Luma8`，其余 → `Rgb8`（按 8 位合成，16 位先降到 8 位）。
/// 每个分量 `c·a/255 + 255·(1 − a/255)`，四舍五入。
pub(crate) fn flatten_alpha_on_white(img: image::DynamicImage) -> image::DynamicImage {
    use image::DynamicImage;
    let blend =
        |c: u8, a: u8| -> u8 { ((c as u32 * a as u32 + 255 * (255 - a as u32) + 127) / 255) as u8 };
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
