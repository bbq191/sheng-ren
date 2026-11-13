//! 文字书插图、背景图与条目元数据（拆分自 `imgopt.rs`）。

use image::ImageFormat;

use profile::Screen;

use super::comic::gamma_lut;
use super::decode::*;
use super::page8::Page8;

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
    ImgHead::read(bytes, is_comic_format, true).map(|h| (h.w, h.h))
}

/// **整页背景图**按原书的尺寸意图缩进阅读范围（见 [`crate::bgfit`]）：只缩不放、保原格式、带透明的保持透明（不铺白底：
/// 阅读器在透明处画背景色）。和 [`downscale_into`] 不同，缩了就用，不管重编码后是不是变小（调色板 PNG 解成 RGBA 再写回常常
/// 更大）——这类图显示对比体积重要。不用缩、不是 JPEG/PNG、超过解码上限（[`MAX_DECODE_PIXELS`] 和 `max_px` 中小的那个）、
/// 解码失败 → `None`（原样保留）。
pub fn downscale_background(
    bytes: &[u8],
    fit: crate::bgfit::BgFit,
    area: Screen,
    max_px: u64,
) -> Option<Vec<u8>> {
    let head = ImgHead::read(bytes, is_jpeg_or_png, true)?;
    let (nw, nh) = fit.target(head.w, head.h, area)?;
    let img = head.decode(bytes, text_decode_limit(max_px))?;
    match Page8::try_from_dynamic(img) {
        Ok(page) => page.resize_lanczos3(nw, nh).encode(head.fmt, JPEG_QUALITY),
        Err(img) if head.fmt == ImageFormat::Png => resize_alpha_png(img, nw, nh),
        Err(_) => None,
    }
}

/// 整页背景图做成整屏大小的一块（平铺背景的阅读器用，见 [`crate::bgfit::PageTile`]）：左上角内容区那么大的地方铺背景色
/// （没写是白的），图按尺寸、位置画上去（超出内容区的裁掉，同 Kindle），其余白色。带透明的按透明度叠在背景色上。
/// `jpeg`：条目是 JPEG（写回 JPEG），否则写成不透明的 PNG。规则在清洗时已经照留了，所以图解不开（格式不对、超过解码上限 `max_px`、
/// 放得太大）时也写一块只有背景色的（等于以前去掉背景图），不留原图让阅读器乱铺。
pub fn page_tile(
    bytes: &[u8],
    tile: &crate::bgfit::PageTile,
    max_px: u64,
    jpeg: bool,
) -> Option<Vec<u8>> {
    use image::ImageEncoder;
    let (pw, ph) = tile.page;
    let (aw, ah) = (tile.area.0.min(pw), tile.area.1.min(ph));
    // 一张图只这一份、产物也不大，解码上限按调用方给的（不用正文插图的 900 万：《飘》版权页的底纹 2631×3508）
    let placed = ImgHead::read(bytes, is_jpeg_or_png, true).and_then(|head| {
        let img = head.decode(bytes, max_px)?.into_rgba8();
        let (x, y, dw, dh) = tile.placement(img.width(), img.height());
        let (dw, dh) = ((dw.round() as u32).max(1), (dh.round() as u32).max(1));
        // 放得特别大的（窄条图 cover）：缩放的结果太占内存，不做
        if (dw as u64) * (dh as u64) > 4 * (pw as u64) * (ph as u64) {
            return None;
        }
        let scaled = if (dw, dh) == img.dimensions() {
            img.into_raw()
        } else {
            fir_lanczos3(
                &img.into_raw(),
                head.w,
                head.h,
                fast_image_resize::PixelType::U8x4,
                dw,
                dh,
            )?
        };
        Some((x, y, dw, dh, scaled))
    });
    let (x, y, dw, dh, scaled) = placed.unwrap_or((0.0, 0.0, 0, 0, Vec::new()));
    let bg = crate::color::over_white(tile.color.unwrap_or(0xFFFF_FFFF));
    let bg = [(bg >> 16) as u8, (bg >> 8) as u8, bg as u8];
    let mut out = vec![255u8; (pw as usize) * (ph as usize) * 3];
    let (x0, y0) = (x.round() as i64, y.round() as i64);
    for py in 0..ah as i64 {
        for px in 0..aw as i64 {
            let o = ((py as usize) * (pw as usize) + px as usize) * 3;
            let (sx, sy) = (px - x0, py - y0);
            let mut c = bg;
            if sx >= 0 && sy >= 0 && (sx as u32) < dw && (sy as u32) < dh {
                let i = ((sy as usize) * (dw as usize) + sx as usize) * 4;
                let a = scaled[i + 3] as u32;
                for k in 0..3 {
                    c[k] =
                        ((scaled[i + k] as u32 * a + bg[k] as u32 * (255 - a) + 127) / 255) as u8;
                }
            }
            out[o..o + 3].copy_from_slice(&c);
        }
    }
    let mut enc = Vec::new();
    if jpeg {
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut enc, JPEG_QUALITY)
            .write_image(&out, pw, ph, image::ExtendedColorType::Rgb8)
            .ok()?;
    } else {
        image::codecs::png::PngEncoder::new(&mut enc)
            .write_image(&out, pw, ph, image::ExtendedColorType::Rgb8)
            .ok()?;
    }
    Some(enc)
}


/// 文字书正文插图中间调加深（profile `image_gamma`）：短边不到 [`profile::MIN_GAMMA_IMAGE_SIDE`]、带透明通道、不是 8 位灰度/RGB、
/// 内容不是灰度（[`Page8::is_gray_content`]）的 → `None`（原样保留）；其余按 [`gamma_lut`] 加深、原格式重编码（灰度保持单分量）。
/// JPEG 按原图的质量编（[`crate::jpegopt::estimate_quality`]，最高 [`JPEG_QUALITY_COMIC`]）：原书插图常是 40–55 的质量，按 95 编
/// 《福尔摩斯》《金庸》大 45%–66%；按原质量体积基本不变，和加深后的图比误差 42–45dB（2026-10-16 量的），墨水屏上看不出。
/// 超过解码上限、解不开 → `None`。带 EXIF 方向的先摆正。
pub fn darken_gray_image(bytes: &[u8], gamma: f64, max_px: u64) -> Option<Vec<u8>> {
    let head = ImgHead::read(bytes, is_jpeg_or_png, true)?;
    if head.w.min(head.h) < profile::MIN_GAMMA_IMAGE_SIDE {
        return None;
    }
    let img = head.decode(bytes, text_decode_limit(max_px))?;
    if img.color().has_alpha() {
        return None; // 透明处露出的是页面底色，加深后边缘和底色怎么接拿不准，不动
    }
    let mut page = Page8::try_from_dynamic(img).ok()?;
    if !page.is_gray_content() {
        return None;
    }
    page.apply_lut(&gamma_lut(gamma));
    let quality = crate::jpegopt::estimate_quality(bytes)
        .map_or(JPEG_QUALITY_COMIC, |q| q.min(JPEG_QUALITY_COMIC));
    page.encode(head.fmt, quality)
}


/// 优化器交给图片处理的条目（按扩展名：jpg/jpeg/png/gif/webp）。GIF/WebP 只有漫画页会处理（[`prepare_comic_page_for_epub`]），
/// 文字书里的原样保留（[`downscale_for_epub`] 只认 JPEG/PNG）。流式优化阶段一这些条目只占位、不读字节。
pub fn is_page_image(name: &str) -> bool {
    crate::util::is_image_ext(name)
}

/// 图片条目处理后换了格式（漫画里的 GIF/WebP 转成 PNG/JPEG，条目名不变）时，OPF manifest 该写的新 media-type；
/// 没换格式 → `None`。按产物字节的魔数判断，不看处理过程。
pub fn converted_media_type(name: &str, out: &[u8]) -> Option<&'static str> {
    let mt = crate::util::image_kind(out)
        .filter(|k| matches!(k.format, ImageFormat::Jpeg | ImageFormat::Png))?
        .mime;
    (crate::util::image_media_type_of_ext(&crate::util::image_ext_of(name)) != mt).then_some(mt)
}
