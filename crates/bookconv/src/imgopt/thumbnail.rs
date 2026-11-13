//! 书架封面缩略图（拆分自 `imgopt.rs`）。

use image::ImageFormat;

use profile::CoverThumbFit;

use super::comic::{decode_page, PageDecode};
use super::page8::darkness_plane;

/// 书架缩略图（profile `cover_thumb_aspect`）：凑成宽高比 `aspect`，长边超过 [`THUMB_MAX_SIDE`] 的缩下来，JPEG 质量
/// [`THUMB_QUALITY`]（黑白屏转灰度）。
///
/// `fit`（profile `cover_thumb_fit`）：
/// - [`CoverThumbFit::Crop`]（缺省，Kindle、掌阅、汉王）：先去掉扫描留下的窄白边（[`thumb_scan_margins`]），再居中裁成比例
///   （宽的裁左右、窄的裁上下）。
/// - [`CoverThumbFit::Pad`]（Kobo 2026-11-11）：**不裁不拉伸**，按比例在缺的那一方向两边各补一半白（`#FFFFFF`），
///   不去扫描窄白边（白边本来就要补）。长边仍以 [`THUMB_MAX_SIDE`] 为上限（补完的整张画布算）。
///
/// 比例只差 [`THUMB_TOLERANCE`] 以内、解不开、超过解码上限 → `None`。只给书架用，书里的封面页照旧用原图。
pub fn cover_thumbnail(
    bytes: &[u8],
    aspect: f32,
    fit: CoverThumbFit,
    grayscale: bool,
    max_px: u64,
) -> Option<Vec<u8>> {
    let (img, _) = decode_page(
        bytes,
        PageDecode {
            grayscale,
            max_px,
            apply_exif: true,
        },
    )?;
    let (w0, h0) = img.dimensions();
    match fit {
        CoverThumbFit::Crop => {
            let (l, t, r, b) = thumb_scan_margins(&darkness_plane(&img), w0, h0);
            let img = if (l, t, r, b) != (0, 0, 0, 0) {
                img.crop(l, t, w0 - l - r, h0 - t - b)
            } else {
                img
            };
            let (w, h) = img.dimensions();
            let have = w as f32 / h as f32;
            if (w, h) == (w0, h0) && (have - aspect).abs() <= THUMB_TOLERANCE {
                return None;
            }
            let (cw, ch) = if have > aspect {
                (((h as f32 * aspect).round() as u32).clamp(1, w), h)
            } else {
                (w, ((w as f32 / aspect).round() as u32).clamp(1, h))
            };
            let img = img.crop((w - cw) / 2, (h - ch) / 2, cw, ch);
            let scale = (THUMB_MAX_SIDE as f32 / cw.max(ch) as f32).min(1.0);
            let img = if scale < 1.0 {
                img.resize_lanczos3(
                    ((cw as f32 * scale).round() as u32).max(1),
                    ((ch as f32 * scale).round() as u32).max(1),
                )
            } else {
                img
            };
            img.encode(ImageFormat::Jpeg, THUMB_QUALITY)
        }
        CoverThumbFit::Pad => {
            let have = w0 as f32 / h0 as f32;
            if (have - aspect).abs() <= THUMB_TOLERANCE {
                return None;
            }
            // 补完的画布（原尺寸）：宽了就补上下、窄了就补左右，两边各一半（奇数多的一像素留在下/右）
            let (cw, ch) = if have > aspect {
                (w0, ((w0 as f32 / aspect).round() as u32).max(h0))
            } else {
                (((h0 as f32 * aspect).round() as u32).max(w0), h0)
            };
            // 长边上限按补完的画布算：内容也照同一个比例缩，补的白边不单独缩放
            let scale = (THUMB_MAX_SIDE as f32 / cw.max(ch) as f32).min(1.0);
            let sw = ((w0 as f32 * scale).round() as u32).max(1);
            let sh = ((h0 as f32 * scale).round() as u32).max(1);
            let fw = ((cw as f32 * scale).round() as u32).max(sw);
            let fh = ((ch as f32 * scale).round() as u32).max(sh);
            let img = if scale < 1.0 {
                img.resize_lanczos3(sw, sh)
            } else {
                img
            };
            img.paste_on(fw, fh, (fw - sw) / 2, (fh - sh) / 2, 255)
                .encode(ImageFormat::Jpeg, THUMB_QUALITY)
        }
    }
}

/// 封面四边扫描留下的窄白边 `(左, 上, 右, 下)`：由外向内一行（列）里至少 99% 的像素比 [`THUMB_WHITE`] 亮算白，最外面
/// [`THUMB_EDGE_LINE`] 行以内不白的（扫描的一道灰线）跳过不算；一边的白超过边长的 [`THUMB_MAX_MARGIN`] 就当作封面本身的白底、那一边
/// 不动（《三体全集》立体书效果图上下的白占 11%、8%：先去掉再裁，书脊、封面边被切，维护者看了「不好看」）。
/// 《竹光侍》卷01 的封面上 17、下 32、右 20 像素（右边最外一列灰线），书架上看得出一圈白（2026-10-30 维护者问）。
pub(crate) fn thumb_scan_margins(px: &[u8], w: u32, h: u32) -> (u32, u32, u32, u32) {
    let white_row = |y: u32| {
        let row = &px[(y * w) as usize..((y + 1) * w) as usize];
        row.iter().filter(|&&v| v >= THUMB_WHITE).count() * 100 >= row.len() * 99
    };
    let white_col = |x: u32| {
        (0..h)
            .filter(|&y| px[(y * w + x) as usize] >= THUMB_WHITE)
            .count()
            * 100
            >= h as usize * 99
    };
    // 由外向内：最外 THUMB_EDGE_LINE 行里不白的跳过，之后碰到不白的就停；返回白到哪（不含）
    let side = |len: u32, white: &dyn Fn(u32) -> bool| {
        let mut end = 0;
        for i in 0..len {
            if white(i) {
                end = i + 1;
            } else if i >= THUMB_EDGE_LINE {
                break;
            }
        }
        if end as f32 > len as f32 * THUMB_MAX_MARGIN {
            0
        } else {
            end
        }
    };
    let top = side(h, &|i| white_row(i));
    let bottom = side(h, &|i| white_row(h - 1 - i));
    let left = side(w, &|i| white_col(i));
    let right = side(w, &|i| white_col(w - 1 - i));
    if left + right >= w || top + bottom >= h {
        return (0, 0, 0, 0);
    }
    (left, top, right, bottom)
}

/// [`thumb_scan_margins`]：比这个亮算白（扫描纸白，JPEG 噪点在 240 以上）。
const THUMB_WHITE: u8 = 240;
/// [`thumb_scan_margins`]：最外面这么多行（列）里不白的不算（扫描边上的一道灰线）。
const THUMB_EDGE_LINE: u32 = 3;
/// [`thumb_scan_margins`]：一边的白超过边长的这个比例就不动（当作封面本身的白底）。
const THUMB_MAX_MARGIN: f32 = 0.05;

/// [`cover_thumbnail`]：长边最多这么多像素（Kindle 书架网格里封面框 344×531，留足余量）。
const THUMB_MAX_SIDE: u32 = 1200;
/// [`cover_thumbnail`]：JPEG 质量。
const THUMB_QUALITY: u8 = 90;
/// [`cover_thumbnail`]：和要的宽高比差这么多以内不裁（裁掉不到一个像素宽的边没意义）。
const THUMB_TOLERANCE: f32 = 0.005;

/// 已有的书架缩略图（宽 `w`、高 `h`）合不合 `aspect`：差在 [`THUMB_TOLERANCE`] 以内。再优化时用来判断留着还是重做。
pub fn thumb_aspect_fits(w: u32, h: u32, aspect: f32) -> bool {
    h > 0 && (w as f32 / h as f32 - aspect).abs() <= THUMB_TOLERANCE
}
