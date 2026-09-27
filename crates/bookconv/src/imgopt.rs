//! 优化器图片降采样——按目标设备屏幕（[`Screen`]，来自 `profile` crate）做设备级优化。
//!
//! 书里常带 2000–4000px 的高清原图，超出屏幕的像素**纯属浪费**：拖慢加载、吃内存、还逼阅读器在渲染期
//! 临时缩放（慢且质量不可控）。优化器在组包/优化阶段把超大图 Lanczos3 预缩进屏幕框，缩放质量我们控。
//!
//! 纪律：**只缩不放、保宽高比、保原格式、达标即跳过（幂等 + 免二次编码损失）、任何失败原样保留**
//! （绝不因优化损坏原书）。只碰 JPEG/PNG（书内图几乎都是；GIF 可能动图，跳过不冒险）。

use image::codecs::jpeg::JpegEncoder;
use image::imageops::FilterType;
use image::ImageFormat;
use std::io::Cursor;

pub use profile::Screen;

/// 单测用的设备屏幕：rMPP Move（954×1696）。历史用例与真机数据都是在 Move 上量的，断言里的数字以它为准。
#[cfg(test)]
pub(crate) fn test_screen() -> Screen {
    profile::get("rmpp-move").expect("内置 rmpp-move profile").screen
}

/// 单张图片允许解码的像素数上限（w×h，跟格式/用途无关）——防极端高分辨率原图解码成未压缩位图
/// 把内存顶爆。2026-09-19 真机坐实：用户真实投递一套漫画（《乱马1/2》8 卷）触发超限按卷拆分
/// 落库，book-serve `VmHWM` 冲到 271MB——定位到 `downscale_into`/`decode_trim_comic`/`dither_bilevel`
/// 三处解码前只用 `header_dims` 读了宽高判断"要不要处理"，没有对"这张图本身大到不该整个解出来"
/// 设硬上限。
///
/// **阈值取值不是"3 字节/像素 RGB8"这种理论估算**——第一版按这个估算给了 2500 万像素（估算峰值
/// ~75MB），结果真机又撞上一次《火影忍者》多卷投递，`VmHWM` 又冲到 262MB，跟没修之前几乎一个
/// 量级。本地测量当时的真实调用链（裁边 → 缩放两道串联，现已被 `prepare_comic_page_for_epub` 单趟取代，忠实
/// 复刻 `optimize.rs` 的真实用法）在不同像素数下的实测 `VmHWM`（`/proc/<pid>/status`，release
/// 编译）：400万像素→62MB、870万像素（A4 300dpi）→97-109MB、1600万像素→164MB、2500万像素→
/// 230-236MB——**理论估算的单缓冲区大小完全没抓住真实开销**（`image` 库内部解码+`to_rgb8()`+
/// resize 中间缓冲多份同时存活，实测开销约 9-16MB/百万像素，远高于 3 字节/像素≈3MB/百万像素的
/// naive 估算）。改用实测数据定阈值：900 万像素（约 3000×3000，覆盖 A4 300dpi 及绝大多数真实
/// 漫画/书籍扫描页）在真机上峰值约 100-110MB——比 262MB 危机低一个数量级，设备实测可用内存
/// 通常有几百 MB 余量，这个量级的单张图瞬时峰值不构成风险。超限的图直接放弃处理、原样保留原图
/// 字节——调用方对这三个函数返回 `None` 本来就是"原样保留"语义，天然兜底，不是新错误路径。
const MAX_DECODE_PIXELS: u64 = 9_000_000;

fn within_decode_budget(w: u32, h: u32) -> bool {
    (w as u64) * (h as u64) <= MAX_DECODE_PIXELS
}
/// 重编码 JPEG 质量（0–100）。85 = 视觉无损级，体积/画质平衡；e-ink 上更看不出差异。
const JPEG_QUALITY: u8 = 85;
/// 漫画页专用重编码质量——EPUB 线原则④"漫画不允许压画质"：超限时仍必须缩到屏幕框内（否则设备渲染
/// 异常），但不该像普通插图那样再吃一道 85 质量的有损重编码，95 更接近视觉无损。
const JPEG_QUALITY_COMIC: u8 = 95;
/// EPUB 漫画→PDF 里**预放大**后的页面所用 JPEG 质量：放大产生的像素本就平滑，q95 会体积暴涨
/// （镖人卷02 实测 21MB→113MB），q85 约 71MB 且真机对照仍明显比阅读器自己放大清晰。
const JPEG_QUALITY_UPSCALED: u8 = 85;
/// 预放大的倍数上限：超过视为缩略图/装饰小图，不值得放大到整页宽。
const MAX_PDF_UPSCALE: f32 = 3.0;

/// 按 `fmt` 编码回同一格式：JPEG 用 `quality`，PNG 无损；其余格式 `None`。各处理函数共用（此前每处各抄一份 match）。
fn encode_as(fmt: ImageFormat, img: &image::DynamicImage, quality: u8) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    match fmt {
        ImageFormat::Jpeg => JpegEncoder::new_with_quality(&mut out, quality).encode_image(img).ok()?,
        ImageFormat::Png => img.write_to(&mut Cursor::new(&mut out), ImageFormat::Png).ok()?,
        _ => return None,
    }
    Some(out)
}

/// 保比缩进 `max_w × max_h` 框（宽高比保持、保原格式，JPEG 质量 [`JPEG_QUALITY`]），只在超框时动；返回新字节或 `None`
/// （已达标 / 非 JPEG·PNG / 解码失败 / 重编码没变小 → 调用方原样保留）。
fn downscale_into(bytes: &[u8], max_w: u32, max_h: u32) -> Option<Vec<u8>> {
    let (fmt, (w, h)) = header_dims(bytes)?;
    if w <= max_w && h <= max_h {
        return None; // 已达标：不解码不重编码（避免无谓的二次有损压缩；2473 页漫画只读头是秒级、全解是分钟级）
    }
    if !within_decode_budget(w, h) {
        return None; // 极端高分辨率原图：不整个解出来，原样保留（见 MAX_DECODE_PIXELS 文档）
    }
    let img = image::load_from_memory_with_format(bytes, fmt).ok()?;
    // 缩放走 SIMD 版 Lanczos3（`resize_lanczos3`，同漫画页；尺寸算法与 `DynamicImage::resize` 相同），编码时灰度保持单分量。
    // 此前 `img.resize` 是 `image` 自带的标量实现（实测慢约 20 倍，且中间缓冲是 f32，内存大），`encode_image` 还会把
    // 灰度图写成 3 分量 JPEG（体积白涨）——文字书插图每张都走这里（2026-09-25 审计）。非 L8/RGB8 类型（带透明 PNG 等）照旧。
    let (nw, nh) = fit_within(w, h, max_w, max_h);
    let resized = resize_lanczos3(&img, nw, nh);
    drop(img);
    let out = encode_keep_gray(fmt, &resized, JPEG_QUALITY).or_else(|| encode_as(fmt, &resized, JPEG_QUALITY))?;
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
pub fn downscale_for_device(bytes: &[u8], screen: Screen) -> Option<Vec<u8>> {
    let (_, (w, h)) = header_dims(bytes)?;
    let (long, short) = (screen.long_edge(), screen.short_edge());
    let (max_w, max_h) = if w >= h { (long, short) } else { (short, long) };
    downscale_into(bytes, max_w, max_h)
}

/// 图片头部声明的像素数（不解码）；读不出来按 100 万像素估，给并行内存预算用（[`crate::imgpool`]）。
pub fn pixel_count(bytes: &[u8]) -> u64 {
    header_dims(bytes).map(|(_, (w, h))| (w as u64) * (h as u64)).unwrap_or(1_000_000)
}

/// 只读文件头取 (格式, 宽, 高)，不解码像素。非 JPEG/PNG → None。
pub(crate) fn header_dims(bytes: &[u8]) -> Option<(ImageFormat, (u32, u32))> {
    let fmt = image::guess_format(bytes).ok()?;
    if !matches!(fmt, ImageFormat::Jpeg | ImageFormat::Png) {
        return None;
    }
    let dims = image::ImageReader::with_format(Cursor::new(bytes), fmt).into_dimensions().ok()?;
    Some((fmt, dims))
}

/// **EPUB 内嵌图**降采样：一律竖向屏幕框（**宽绝不超屏幕短边**）。EPUB 图可能**行内**（xochitl 按固有
/// 尺寸渲染、不认 CSS），横图容许长边宽会让行内横幅溢出竖屏——2026-09-04 Move 真机《飘》1696×630 的
/// `class="logo"` 内联横幅溢出坐实。竖向框下：块级图仍适配列宽（显示无变化）、行内图不再超宽。
pub fn downscale_for_epub(bytes: &[u8], screen: Screen) -> Option<Vec<u8>> {
    downscale_into(bytes, screen.width, screen.height)
}

/// 漫画页补白容差：页面长宽比与阅读范围比例的相对误差在这个范围内不补。要严：Move 真机上一张偏差 1.6% 的页没补白，
/// 图片就少 4.5pt 宽并出现左右不对称（上游白皮书 §20）。
///
/// 补白目标是 profile 里的**真实可阅读范围**（不是屏幕标称尺寸）。xochitl 的 EPUB 图片框：宽撑满栏宽（页宽 − 2×页边距）、
/// 高度上限固定（Move 上 462.1pt），任何 `height` 声明都不生效；把漫画页补到"栏宽:高度上限"的比例，图片正好填满图片框。
/// 旧做法补到屏幕比例 954:1696，图片先顶到高度上限，只有 260pt 宽（页宽 303pt）、左右留白 20.0/22.9pt。
const PAD_ASPECT_TOLERANCE: f32 = 0.003;

/// 裁边判定容差：一行/列里像素两两 RGB 通道极差都 ≤ 这个值才算"纯色留白"。留够松（8）容 JPEG 压缩
/// 噪声，但不到能吃掉真实画面渐变的地步。
const TRIM_TOLERANCE: u8 = 8;
/// 单边最多裁掉原图这个比例——防止极端图（比如整页近乎纯色）被误判成"全是留白"裁没内容。
/// 真机《镖人》母版库实测坐实过 0.15 太保守（2026-09-19 用户反馈"优化没把大量留白裁切完"）：
/// 每卷开头的版权页（CIP 页，中文漫画常见排版）实际留白单边能到 22%-29%，旧阈值在 15% 就强行
/// 停手，裁不干净。抽样 43 张真实页量出的最大值约 28.6%，0.35 留出约 6 个百分点余量；两边独立
/// 累加最多到 0.7×边长，仍留 30% 给内容，不会把整页裁没。
const TRIM_MAX_FRACTION: f32 = 0.35;

/// 一行/一列像素是否"纯色"（每个像素与首像素的 RGB 通道极差都 ≤ [`TRIM_TOLERANCE`]）。`px(i)` 取该行/列第 i 个像素。
fn line_is_uniform(len: u32, px: impl Fn(u32) -> [u8; 3]) -> bool {
    if len <= 1 {
        return true;
    }
    let first = px(0);
    (1..len).all(|i| {
        let p = px(i);
        (0..3).all(|c| (p[c] as i16 - first[c] as i16).unsigned_abs() as u8 <= TRIM_TOLERANCE)
    })
}

/// 四边纯色留白的检测：返回 `(left, top, 裁后宽, 裁后高)`；没有可裁的留白 / 图太小 / 会裁成空 → `None`。
/// 只在"确实是留白"时裁——边缘整行/整列像素高度一致（[`TRIM_TOLERANCE`]）才算留白，一遇到不满足就停，
/// 不会裁进真实画面。单边最多裁 [`TRIM_MAX_FRACTION`]，兜底极端误判。
/// `px(x, y)` 取像素 RGB——灰度图直接返回 `[v; 3]`，不必先造一份 3 倍大的 RGB 副本（此前灰度页为探测边缘整图 `to_rgb8()`）。
fn trim_bounds_with(w: u32, h: u32, px: impl Fn(u32, u32) -> [u8; 3]) -> Option<(u32, u32, u32, u32)> {
    if w < 4 || h < 4 {
        return None;
    }
    let max_v = ((h as f32) * TRIM_MAX_FRACTION) as u32;
    let max_h = ((w as f32) * TRIM_MAX_FRACTION) as u32;
    let row = |y: u32| line_is_uniform(w, |x| px(x, y));
    let col = |x: u32| line_is_uniform(h, |y| px(x, y));
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

/// [`trim_bounds_with`] 的 `DynamicImage` 入口：`Luma8`/`Rgb8` 直接读像素，其它类型（调用方已归一，实际不会到）退化成 RGB 副本。
fn trim_bounds(img: &image::DynamicImage) -> Option<(u32, u32, u32, u32)> {
    use image::DynamicImage;
    let (w, h) = (img.width(), img.height());
    match img {
        DynamicImage::ImageLuma8(g) => trim_bounds_with(w, h, |x, y| {
            let v = g.get_pixel(x, y)[0];
            [v, v, v]
        }),
        DynamicImage::ImageRgb8(c) => trim_bounds_with(w, h, |x, y| c.get_pixel(x, y).0),
        other => {
            let c = other.to_rgb8();
            trim_bounds_with(w, h, |x, y| c.get_pixel(x, y).0)
        }
    }
}

/// 解码并归一到 `Luma8`/`Rgb8`（灰度保持灰度），并做四边留白裁边。返回 `(图, 格式, 是否裁过)`。
/// [`prepare_comic_page_for_pdf`] / [`prepare_comic_page_for_epub`] 共用的前半段（此前 PDF 版整段抄了一份）。
/// 用 `into_luma8`/`into_rgb8`：解码结果本来就是 8 位对应类型（几乎所有漫画页）时**不再拷贝整图**，
/// 且不再让"原始解码图 + 归一副本"同时占内存。
/// 解码 + 裁白边。`grayscale`（黑白屏设备）时彩色图顺手转成单通道 8 位灰度（256 级，不抖动）。
/// 返回 (图, 原格式, 是否改动过像素——裁了边或转了灰度)。
fn decode_trim_comic(bytes: &[u8], grayscale: bool) -> Option<(image::DynamicImage, ImageFormat, bool)> {
    use image::DynamicImage;
    let (fmt, (w, h)) = header_dims(bytes)?;
    if !within_decode_budget(w, h) {
        return None; // 极端高分辨率原图：不整个解出来，原样保留（见 MAX_DECODE_PIXELS 文档）
    }
    let decoded = image::load_from_memory_with_format(bytes, fmt).ok()?;
    let gray = matches!(decoded.color(), image::ColorType::L8 | image::ColorType::L16 | image::ColorType::La8 | image::ColorType::La16);
    let to_gray = grayscale && !gray;
    let img = if gray || to_gray { DynamicImage::ImageLuma8(decoded.into_luma8()) } else { DynamicImage::ImageRgb8(decoded.into_rgb8()) };
    Some(match trim_bounds(&img) {
        Some((l, t, cw, ch)) => (img.crop_imm(l, t, cw, ch), fmt, true),
        None => (img, fmt, to_gray),
    })
}

/// **EPUB 漫画 → PDF 专用的单趟页面处理**：解码一次 → 裁边 → 按 PDF 里实际绘制的整数像素尺寸
/// （[`crate::convert::pdfwrite::place_image`]）重采样一次 → 编码一次。**恰好没有可裁的留白、
/// 也不需要缩小时返回 `None`，调用方直接嵌原图字节（零损失）。**
///
/// 取代此前的 `trim_margins` → `downscale_for_epub_comic` 两道串联，它们各自 decode+encode 一遍，
/// 叠加以下三处画质损失（2026-09-20 用户反馈"EPUB 漫画优化成 PDF 会降画质"，拿真机同款乱马/镖人
/// 样本离线量化：1091px 宽网点漫画相对"一次理想重采样"只有 26-31dB）：
///
/// 1. **重采样两遍**：先 Lanczos 缩到 954 框，PDF 里又按 98% 页宽（934.92 非整数）+ 非整数偏移摆放，
///    阅读器等于再缩+亚像素平移一遍；这里直接一次缩到 `place_image` 的整数绘制尺寸，阅读器 1:1 贴。
/// 2. **JPEG 有损代际两代**（裁边一代、缩放一代，各 q95）：合成一趟只剩一代。
/// 3. **灰度图被 `to_rgb8()` 转成 RGB 再编码**：这里保持灰度（单分量 JPEG / 灰度 PNG），不引入
///    多余的色度通道噪声，体积也更小。
///
/// **JPEG 低分辨率源图会由我们预放大**（2026-09-20 真机 A/B 坐实）：镖人卷02 源图仅 566×800，PDF 里
/// 按 934 宽摆放要放大 1.65 倍。让 xochitl 放大 vs 我们先 Lanczos 放大到整数绘制宽、设备 1:1 显示，
/// 用户对照后判定**后者明显更清晰**（xochitl 的 PDF 放大滤镜偏糊）。代价是体积：q95 会 21MB→113MB，
/// 放大后内容本就平滑，改用 [`JPEG_QUALITY_UPSCALED`]=85 压到约 71MB。边界：放大倍数超过
/// [`MAX_PDF_UPSCALE`]（缩略图/装饰小图，放大只是白涨体积）不放大；PNG 不放大（无损放大体积暴涨）。
pub fn prepare_comic_page_for_pdf(bytes: &[u8], page_w: u32, page_h: u32) -> Option<Vec<u8>> {
    let (img, fmt, trimmed) = decode_trim_comic(bytes, false)?;
    let (cw, ch) = (img.width(), img.height());
    let (dw, dh, _, _) = crate::convert::pdfwrite::place_image(cw, ch, page_w, page_h);
    // 高度撑满分支的绘制宽可能是奇数——取偶保证左右边距整数（页宽偶数时）。
    let dw = ((dw.round() as u32) & !1).max(2);
    let dh = (dh.round() as u32).max(1);
    let shrink = dw < cw && dh < ch;
    let upscale = fmt == ImageFormat::Jpeg && dw > cw && dh > ch && (dw as f32 / cw as f32) <= MAX_PDF_UPSCALE;
    if !trimmed && !shrink && !upscale {
        return None; // 既没裁又不缩放：原图字节零损失直接嵌
    }
    let img = if shrink || upscale { resize_lanczos3(&img, dw, dh) } else { img };
    encode_keep_gray(fmt, &img, if upscale { JPEG_QUALITY_UPSCALED } else { JPEG_QUALITY_COMIC })
}

/// 白底画布上放置 `img`（保持 `Luma8`/`Rgb8` 类型，偏移 `off_x/off_y`）。
fn paste_on_white(img: &image::DynamicImage, cw: u32, ch: u32, off_x: u32, off_y: u32) -> image::DynamicImage {
    use image::DynamicImage;
    match img {
        DynamicImage::ImageLuma8(g) => {
            let mut canvas = image::GrayImage::from_pixel(cw, ch, image::Luma([255]));
            image::imageops::overlay(&mut canvas, g, off_x as i64, off_y as i64);
            DynamicImage::ImageLuma8(canvas)
        }
        DynamicImage::ImageRgb8(c) => {
            let mut canvas = image::RgbImage::from_pixel(cw, ch, image::Rgb([255, 255, 255]));
            image::imageops::overlay(&mut canvas, c, off_x as i64, off_y as i64);
            DynamicImage::ImageRgb8(canvas)
        }
        other => {
            let mut canvas = image::RgbImage::from_pixel(cw, ch, image::Rgb([255, 255, 255]));
            image::imageops::overlay(&mut canvas, &other.to_rgb8(), off_x as i64, off_y as i64);
            DynamicImage::ImageRgb8(canvas)
        }
    }
}

/// **EPUB 漫画整页的单趟处理**（取代 `trim_margins` → `downscale_for_epub_comic` → `pad_to_device_aspect`
/// 三道串联：每道各自解码+编码一遍，三代 JPEG 有损、灰度被转 RGB、三次整图缩放/合成）。
///
/// 解码一次 → 裁边 → 等比放进阅读范围 `area`（profile 的真实可阅读范围）**一次**缩放（缩小，或 JPEG 小图放大，见
/// [`prepare_comic_page_for_pdf`] 的 A/B 结论：让 xochitl 自己放大偏糊，我们预放大更清晰）→ 白底补到
/// `area` 的比例（`width:100%` 渲染正好填满阅读器的图片框，见 [`PAD_ASPECT_TOLERANCE`]）→ 编码一次，
/// 灰度保持单分量；`grayscale`（黑白屏设备）时彩色页也转成单分量 8 位灰度（256 级，不抖动，2026-09-27 用户定）。
/// 小于设备短边 1/3 的装饰小图只裁边，不缩放/补白（同 `pad_to_device_aspect`）。
/// 什么都不需要做时返回 `None`（原字节零损失）。
pub fn prepare_comic_page_for_epub(bytes: &[u8], area: Screen, grayscale: bool) -> Option<Vec<u8>> {
    let (img, fmt, trimmed) = decode_trim_comic(bytes, grayscale)?;
    let (cw, ch) = (img.width(), img.height());
    let quality_default = JPEG_QUALITY_COMIC;
    let (page_w, page_h) = (area.width, area.height);
    let (out_img, quality) = if cw.min(ch) < page_w / 3 {
        if !trimmed {
            return None; // 装饰小图且没白边：原样
        }
        (img, quality_default)
    } else {
        let frame_aspect = area.aspect();
        let s = (page_w as f32 / cw as f32).min(page_h as f32 / ch as f32);
        let shrink = s < 1.0;
        let upscale = fmt == ImageFormat::Jpeg && s > 1.0 && s <= MAX_PDF_UPSCALE;
        let (img, quality) = if shrink || upscale {
            let nw = ((cw as f32 * s).round() as u32).clamp(1, page_w);
            let nh = ((ch as f32 * s).round() as u32).clamp(1, page_h);
            (resize_lanczos3(&img, nw, nh), if upscale { JPEG_QUALITY_UPSCALED } else { quality_default })
        } else {
            (img, quality_default)
        };
        let (w, h) = (img.width(), img.height());
        let cur_aspect = w as f32 / h as f32;
        if ((cur_aspect - frame_aspect) / frame_aspect).abs() <= PAD_ASPECT_TOLERANCE {
            if !trimmed && !shrink && !upscale {
                return None;
            }
            (img, quality)
        } else if cur_aspect > frame_aspect {
            let new_h = (w as f32 / frame_aspect).round() as u32;
            (paste_on_white(&img, w, new_h, 0, (new_h - h) / 2), quality)
        } else {
            let new_w = (h as f32 * frame_aspect).round() as u32;
            (paste_on_white(&img, new_w, h, (new_w - w) / 2, 0), quality)
        }
    };
    encode_keep_gray(fmt, &out_img, quality)
}

/// Lanczos3 重采样，SIMD 实现（`fast_image_resize`，x86 SSE4/AVX2、aarch64 NEON 运行期自动选）。
///
/// 替换 `DynamicImage::resize_exact(.., Lanczos3)` 的原因：2026-09-20 分阶段计时（乱马/镖人，
/// release、每页 ~1000×1500）显示**缩放占整页处理时间的 74–79%**（145–218ms/页），编码 15–22%，
/// 解码/裁边探测可忽略；`fast_image_resize` 同一算法（Lanczos3 卷积）快约 **20 倍**（7–11ms/页）。
/// **不是逐位一致**：与 `image` 库实现的像素差均值 0.1–0.2 灰阶、最大 ~30（仅高对比边缘），二者对
/// 浮点参照（PIL）都是 53–55dB——远低于随后 JPEG q95 编码本身的误差（约 45dB），没有可见差别。
/// 仅处理 `Luma8`/`Rgb8`（调用方已归一到这两种）；其它类型或库报错时退回 `image` 自带实现。
fn resize_lanczos3(img: &image::DynamicImage, dw: u32, dh: u32) -> image::DynamicImage {
    use fast_image_resize::images::{Image, ImageRef};
    use fast_image_resize::{FilterType as FirFilter, PixelType, ResizeAlg, ResizeOptions, Resizer};
    use image::DynamicImage;
    let opts = ResizeOptions::new().resize_alg(ResizeAlg::Convolution(FirFilter::Lanczos3));
    let fast = || -> Option<DynamicImage> {
        let mut resizer = Resizer::new();
        match img {
            DynamicImage::ImageLuma8(g) => {
                let src = ImageRef::new(g.width(), g.height(), g.as_raw(), PixelType::U8).ok()?;
                let mut dst = Image::new(dw, dh, PixelType::U8);
                resizer.resize(&src, &mut dst, &opts).ok()?;
                image::GrayImage::from_raw(dw, dh, dst.into_vec()).map(DynamicImage::ImageLuma8)
            }
            DynamicImage::ImageRgb8(c) => {
                let src = ImageRef::new(c.width(), c.height(), c.as_raw(), PixelType::U8x3).ok()?;
                let mut dst = Image::new(dw, dh, PixelType::U8x3);
                resizer.resize(&src, &mut dst, &opts).ok()?;
                image::RgbImage::from_raw(dw, dh, dst.into_vec()).map(DynamicImage::ImageRgb8)
            }
            _ => None,
        }
    };
    fast().unwrap_or_else(|| img.resize_exact(dw, dh, FilterType::Lanczos3))
}

/// 按 `fmt` 编码，JPEG **保持灰度图为单分量**。`image` 0.25 的 `JpegEncoder::encode_image(&DynamicImage)` 对
/// `ImageLuma8` 也会转成 3 分量 RGB 输出（2026-09-20 实测 SOF 分量数=3、回读 `Rgb8`），必须走
/// `ImageEncoder::write_image(.., ExtendedColorType::L8)` 才是真灰度 JPEG。仅接受 `Luma8`/`Rgb8`
/// （调用方已归一到这两种），JPEG 遇其它类型返回 `None`；PNG 走 `image` 自带无损编码；其余格式 `None`。
fn encode_keep_gray(fmt: ImageFormat, img: &image::DynamicImage, jpeg_quality: u8) -> Option<Vec<u8>> {
    use image::{DynamicImage, ExtendedColorType, ImageEncoder};
    let mut out = Vec::new();
    match fmt {
        ImageFormat::Jpeg => {
            let enc = JpegEncoder::new_with_quality(&mut out, jpeg_quality);
            match img {
                DynamicImage::ImageLuma8(g) => enc.write_image(g.as_raw(), g.width(), g.height(), ExtendedColorType::L8).ok()?,
                DynamicImage::ImageRgb8(c) => enc.write_image(c.as_raw(), c.width(), c.height(), ExtendedColorType::Rgb8).ok()?,
                _ => return None,
            }
        }
        ImageFormat::Png => img.write_to(&mut Cursor::new(&mut out), ImageFormat::Png).ok()?,
        _ => return None,
    }
    Some(out)
}

/// 「漫画省刷新」色彩保留阈值：页面平均色度（RGB 通道极差 /255 的均值）低于此值视作**黑白/偏色扫描**、
/// 转 1-bit；高于此值视作**真彩页**（漫画彩封/彩插）→ 保留彩色不动。真机实测火影正文=0（灰度 JPEG）、
/// 彩封≈0.5(HSL 饱和度)，0.06 能干净分开：清洗偏色扫描、保住真彩。
const COLOR_KEEP_CHROMA: f32 = 0.06;

/// 采样估计页面平均色度（避免逐像素遍历大图）：每隔若干像素取样，取 RGB 极差均值 /255。
/// 灰度图（r=g=b）色度恒 0，直接返回——此前不分类型一律 `to_rgb8()`，灰度漫画页（最常见）白白复制出一份
/// 3 倍大的 RGB 副本（900 万像素上限的图就是 27MB）只为算出 0；RGB 图直接读原像素，也不再先克隆一份（2026-09-25 审计）。
fn mean_chroma(img: &image::DynamicImage) -> f32 {
    use image::DynamicImage;
    match img {
        DynamicImage::ImageLuma8(_) | DynamicImage::ImageLumaA8(_) | DynamicImage::ImageLuma16(_) | DynamicImage::ImageLumaA16(_) => 0.0,
        DynamicImage::ImageRgb8(c) => mean_chroma_of(c.width(), c.height(), c.pixels().map(|p| p.0)),
        other => {
            let rgb = other.to_rgb8();
            mean_chroma_of(rgb.width(), rgb.height(), rgb.pixels().map(|p| p.0))
        }
    }
}

fn mean_chroma_of(w: u32, h: u32, pixels: impl Iterator<Item = [u8; 3]>) -> f32 {
    let total = (w as u64) * (h as u64);
    if total == 0 {
        return 0.0;
    }
    let step = ((total / 40_000).max(1)) as usize; // 约取 ~4 万样本封顶
    let (mut sum, mut n) = (0f32, 0u32);
    for [r, g, b] in pixels.step_by(step) {
        let spread = r.max(g).max(b) - r.min(g).min(b);
        sum += spread as f32;
        n += 1;
    }
    if n == 0 {
        0.0
    } else {
        (sum / n as f32) / 255.0
    }
}

/// 「漫画省刷新」核心：解码一页图 → 若为真彩页返回 `None`（调用方保留彩色）；否则转灰度 + Floyd–Steinberg
/// 抖动成双色（0/255）返回 `GrayImage`。抖动保住网点/灰面观感，双色触发面板更轻的 mono 波形（真机坐实：
/// 1-bit 翻页显著更快更轻），且比 8-bit 灰度 FlateDecode 体积小得多。只碰 JPEG/PNG，其余/解码失败=`None`。
pub fn dither_bilevel(bytes: &[u8]) -> Option<image::GrayImage> {
    let (fmt, (w, h)) = header_dims(bytes)?;
    if !within_decode_budget(w, h) {
        return None; // 极端高分辨率原图：不整个解出来，原样保留（见 MAX_DECODE_PIXELS 文档）
    }
    let img = image::load_from_memory_with_format(bytes, fmt).ok()?;
    if mean_chroma(&img) >= COLOR_KEEP_CHROMA {
        return None; // 真彩页：保留彩色（Move 是彩屏，别无脑丢色）
    }
    let mut luma = img.into_luma8(); // 本来就是 8 位灰度时不再复制
    image::imageops::colorops::dither(&mut luma, &image::imageops::colorops::BiLevel);
    Some(luma)
}

/// 条目是否是可降采样图片（按扩展名快筛，真正的格式判定在 `downscale_for_device` 里用魔数）。
pub fn is_downscalable(name: &str) -> bool {
    let l = name.to_lowercase();
    l.ends_with(".jpg") || l.ends_with(".jpeg") || l.ends_with(".png")
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
        profile::get("rmpp-move").unwrap().readable(profile::Format::Epub)
    }

    #[test]
    fn prepare_epub_page_upscales_low_res_and_pads_to_exact_frame() {
        // 镖人同款 566×800 灰度：等比放大到 842 宽（1190 高）→ 白底补到阅读范围 842×1455，灰度保持。
        let out = prepare_comic_page_for_epub(&gray_jpeg_of(566, 800, 0), test_area(), false).expect("低分辨率必须预放大");
        let img = image::load_from_memory(&out).unwrap();
        assert_eq!((img.width(), img.height()), (842, 1455));
        assert_eq!(img.color(), image::ColorType::L8);
    }

    #[test]
    fn prepare_epub_page_shrinks_large_page_once_and_pads() {
        // 乱马同款 1091×1592：缩到 842×1229，补白到 842×1455。
        let out = prepare_comic_page_for_epub(&gray_jpeg_of(1091, 1592, 0), test_area(), false).expect("超框必须缩");
        assert_eq!(image::load_from_memory(&out).unwrap().dimensions(), (842, 1455));
    }

    #[test]
    fn prepare_epub_page_trim_then_fit_in_one_pass() {
        // 带 60px 白边：先裁再适配，仍是阅读范围尺寸，且只编码一次（尺寸即证明一趟到位）。
        let out = prepare_comic_page_for_epub(&gray_jpeg_of(800, 1200, 60), test_area(), false).unwrap();
        assert_eq!(image::load_from_memory(&out).unwrap().dimensions(), (842, 1455));
    }

    #[test]
    fn prepare_epub_page_pads_tall_narrow_page_left_right_without_exceeding_width() {
        // 比阅读范围"窄"的高瘦页（如 700×1600）：高度顶到 1455，宽度 < 842，左右对称补白到 842——宽绝不超阅读范围。
        let out = prepare_comic_page_for_epub(&gray_jpeg_of(700, 1600, 0), test_area(), false).expect("高瘦页必须缩+补白");
        assert_eq!(image::load_from_memory(&out).unwrap().dimensions(), (842, 1455));
    }

    #[test]
    fn epub_pad_tolerance_catches_page_one_point_six_percent_off_frame() {
        // 真机 e2e：一张比框窄 1.6% 的页被 2% 容差放过，图片少 4.5pt 宽且左右不对称——补白容差必须更严。
        let w = (842.0_f32 * 0.984).round() as u32;
        let out = prepare_comic_page_for_epub(&gray_jpeg_of(w, 1455, 0), test_area(), false).expect("偏差 1.6% 必须补白");
        assert_eq!(image::load_from_memory(&out).unwrap().dimensions(), (842, 1455));
    }

    #[test]
    fn nominal_screen_is_used_as_is_when_no_readable_area() {
        // 没有内置阅读范围的设备按标称屏幕补白。
        let out = prepare_comic_page_for_epub(&gray_jpeg_of(566, 800, 0), test_screen(), false).expect("低分辨率必须预放大");
        assert_eq!(image::load_from_memory(&out).unwrap().dimensions(), (954, 1696));
        assert!(prepare_comic_page_for_epub(&gray_jpeg_of(954, 1696, 0), test_screen(), false).is_none(), "已是屏幕页、无白边：原字节");
    }

    #[test]
    fn prepare_epub_page_leaves_untouched_when_already_device_page_or_tiny_icon() {
        assert!(prepare_comic_page_for_epub(&gray_jpeg_of(842, 1455, 0), test_area(), false).is_none(), "已是阅读范围尺寸、无白边：原字节零损失");
        assert!(prepare_comic_page_for_epub(&gray_jpeg_of(200, 300, 0), test_area(), false).is_none(), "装饰小图且无白边：原样");
    }

    #[test]
    fn grayscale_devices_get_single_channel_comic_pages() {
        let color = jpeg_of(842, 1455); // 彩色、尺寸正好是阅读范围、没有白边：彩色屏原样零损失
        assert!(prepare_comic_page_for_epub(&color, test_area(), false).is_none());
        let out = prepare_comic_page_for_epub(&color, test_area(), true).expect("黑白屏要转灰度");
        let img = image::load_from_memory(&out).unwrap();
        assert_eq!(img.color(), image::ColorType::L8, "单通道 8 位灰度（256 级）");
        assert_eq!(img.dimensions(), (842, 1455));
        // 需要缩放的彩色页：彩色屏保持 RGB，黑白屏出灰度
        let big = jpeg_of(1600, 2400);
        assert_eq!(image::load_from_memory(&prepare_comic_page_for_epub(&big, test_area(), false).unwrap()).unwrap().color(), image::ColorType::Rgb8);
        assert_eq!(image::load_from_memory(&prepare_comic_page_for_epub(&big, test_area(), true).unwrap()).unwrap().color(), image::ColorType::L8);
        // 不抖动：纯中灰（128）转完仍是均匀中灰，不会变成黑白点
        let mut mid = Vec::new();
        JpegEncoder::new_with_quality(&mut mid, 95).encode_image(&DynamicImage::ImageRgb8(RgbImage::from_pixel(842, 1455, image::Rgb([128, 128, 128])))).unwrap();
        let g = image::load_from_memory(&prepare_comic_page_for_epub(&mid, test_area(), true).unwrap()).unwrap().to_luma8();
        assert!(g.pixels().all(|p| (120..=136).contains(&p.0[0])), "中灰保持中灰，没有抖动成黑白点");
    }

    #[test]
    fn prepare_epub_page_does_not_upscale_png() {
        // PNG 不放大（无损放大体积暴涨）：700×1000 只补白到设备长宽比，宽仍 700。
        let img = DynamicImage::ImageRgb8(RgbImage::from_fn(700, 1000, |x, y| image::Rgb([(x % 256) as u8, (y % 256) as u8, 9])));
        let mut png = Vec::new();
        img.write_to(&mut Cursor::new(&mut png), ImageFormat::Png).unwrap();
        let out = prepare_comic_page_for_epub(&png, test_area(), false).unwrap();
        let (w, h) = image::load_from_memory(&out).unwrap().dimensions();
        assert_eq!(w, 700, "PNG 不放大");
        assert!(h > 1000, "补白后应更高: {h}");
    }

    #[test]
    fn prepare_pdf_page_returns_none_when_no_work_needed() {
        // PNG 不放大：700×1000 无白边、比绘制宽小 → 原字节零损失直接嵌。
        let img = DynamicImage::ImageRgb8(RgbImage::from_fn(700, 1000, |x, y| image::Rgb([(x % 256) as u8, (y % 256) as u8, 128])));
        let mut png = Vec::new();
        img.write_to(&mut Cursor::new(&mut png), ImageFormat::Png).unwrap();
        assert!(prepare_comic_page_for_pdf(&png, 954, 1696).is_none());
        // 放大倍数超上限（缩略图）的 JPEG 同样不放大。
        assert!(prepare_comic_page_for_pdf(&jpeg_of(200, 300), 954, 1696).is_none());
    }

    #[test]
    fn prepare_pdf_page_upscales_low_res_jpeg_to_exact_draw_width() {
        // 镖人同款：566×800 → 按 934 宽摆放；预放大到整数绘制宽，阅读器 1:1。
        let out = prepare_comic_page_for_pdf(&jpeg_of(566, 800), 954, 1696).expect("低分辨率 JPEG 必须预放大");
        let img = image::load_from_memory(&out).unwrap();
        assert_eq!(img.width(), 934);
        let (dw, dh, x, _) = crate::convert::pdfwrite::place_image(img.width(), img.height(), 954, 1696);
        assert_eq!((dw, dh), (img.width() as f32, img.height() as f32));
        assert_eq!(x.fract(), 0.0);
    }

    #[test]
    fn prepare_pdf_page_shrinks_once_to_exact_integer_draw_size() {
        // 1091×1592 灰度页（乱马同款尺寸）：一次缩到 934 宽，与 place_image 的绘制尺寸精确吻合 → 阅读器 1:1。
        let src = gray_jpeg_of(1091, 1592, 0);
        let out = prepare_comic_page_for_pdf(&src, 954, 1696).expect("超过绘制宽必须缩");
        let img = image::load_from_memory(&out).unwrap();
        assert_eq!(img.width(), 934, "缩后宽必须等于 place_image 的整数绘制宽");
        let (dw, dh, x, y) = crate::convert::pdfwrite::place_image(img.width(), img.height(), 954, 1696);
        assert_eq!((dw, dh), (img.width() as f32, img.height() as f32), "阅读器里应 1:1 无二次缩放");
        assert_eq!(x.fract(), 0.0);
        assert_eq!(y.fract(), 0.0);
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
            let fast = resize_lanczos3(&img, dw, dh);
            let slow = img.resize_exact(dw, dh, FilterType::Lanczos3);
            assert_eq!(fast.color(), slow.color());
            assert_eq!(fast.dimensions(), (dw, dh));
            let (a, b) = (fast.as_bytes(), slow.as_bytes());
            let mean = a.iter().zip(b).map(|(x, y)| x.abs_diff(*y) as f64).sum::<f64>() / a.len() as f64;
            assert!(mean < 0.5, "SIMD 与 image 库 Lanczos3 差距过大: 均值 {mean}");
        }
    }

    #[test]
    fn prepare_pdf_page_keeps_grayscale_grayscale() {
        let src = gray_jpeg_of(1091, 1592, 0);
        assert_eq!(image::load_from_memory(&src).unwrap().color(), image::ColorType::L8, "夹具本身必须是真灰度");
        let out = prepare_comic_page_for_pdf(&src, 954, 1696).unwrap();
        assert_eq!(image::load_from_memory(&out).unwrap().color(), image::ColorType::L8, "灰度页不该被转成 RGB");
    }

    #[test]
    fn prepare_pdf_page_trims_border_then_upscales_once() {
        // 700×1000 带 40px 白边：先裁成 ~620×920，再一次放大到 934 宽（不是先裁编一代、再放大编一代）。
        let src = gray_jpeg_of(700, 1000, 40);
        let out = prepare_comic_page_for_pdf(&src, 954, 1696).expect("有白边必须裁");
        let img = image::load_from_memory(&out).unwrap();
        assert_eq!(img.width(), 934);
        assert_eq!(img.color(), image::ColorType::L8);
    }

    #[test]
    fn prepare_pdf_page_trim_and_shrink_in_one_pass() {
        let src = gray_jpeg_of(1400, 2000, 60);
        let out = prepare_comic_page_for_pdf(&src, 954, 1696).unwrap();
        let (w, h) = image::load_from_memory(&out).unwrap().dimensions();
        assert_eq!(w, 934, "裁边后仍 >934 宽 → 缩到绘制宽: {w}x{h}");
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
        // 非 Move 设备（Kindle PW12 / 掌阅 Ocean 5 Pro：1264×1680）：缩放框与漫画页框都按传入的屏幕算，不再是 954×1696。
        let kindle = profile::get("kindle-pw12-sig").unwrap().screen;
        let (w, h) = image::load_from_memory(&downscale_for_epub(&jpeg_of(2400, 3200), kindle).unwrap()).unwrap().dimensions();
        assert_eq!((w, h), (1260, 1680), "竖图按 1264×1680 框等比缩");
        let (w, h) = image::load_from_memory(&downscale_for_device(&jpeg_of(3200, 1600), kindle).unwrap()).unwrap().dimensions();
        assert_eq!((w, h), (1680, 840), "横页按横向框 1680×1264");
        let out = prepare_comic_page_for_epub(&gray_jpeg_of(1091, 1592, 0), kindle, false).expect("要补白到屏幕比例");
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

    #[test]
    fn within_decode_budget_boundary() {
        assert!(within_decode_budget(3000, 3000), "900 万像素，等于上限，应允许");
        assert!(!within_decode_budget(3001, 3000), "超一点点也该拒绝");
    }

    /// 2026-09-19 真机事故回归测试：用户真实投递一套漫画，某张扫描页解码成未压缩位图把
    /// book-serve `VmHWM` 顶到 271MB（第一版阈值按理论估算定的 2500 万像素，真机又撞了一次
    /// 262MB，说明理论估算不可靠，改用实测数据重新定阈值，见 `MAX_DECODE_PIXELS` 文档）。三个
    /// 解码入口都该对超限图直接放弃处理、原样保留，不再整张解出来。5001×5000（远超新阈值
    /// 900 万像素）足够验证真实调用链路，不需要造更大的图。
    #[test]
    fn oversized_image_skipped_by_all_decode_entries() {
        let huge = jpeg_of(5001, 5000);
        assert!(downscale_for_device(&huge, test_screen()).is_none(), "超限图应跳过降采样");
        assert!(downscale_for_epub(&huge, test_screen()).is_none(), "超限图应跳过降采样");
        assert!(decode_trim_comic(&huge, false).is_none(), "超限图应跳过裁边");
        assert!(dither_bilevel(&huge).is_none(), "超限图应跳过省刷新转换");
    }

    /// 省内存改写前后色度数值一致：灰度恒 0、RGB 直接读、其它类型（RGBA）照旧转 RGB 算。
    #[test]
    fn mean_chroma_matches_rgb_conversion_for_every_color_type() {
        let reference = |img: &DynamicImage| {
            let rgb = img.to_rgb8();
            mean_chroma_of(rgb.width(), rgb.height(), rgb.pixels().map(|p| p.0))
        };
        let rgb = RgbImage::from_fn(300, 200, |x, y| image::Rgb([(x % 256) as u8, (y % 256) as u8, ((x + y) % 256) as u8]));
        let cases = [
            DynamicImage::ImageRgb8(rgb.clone()),
            DynamicImage::ImageRgba8(DynamicImage::ImageRgb8(rgb.clone()).to_rgba8()),
            DynamicImage::ImageLuma8(DynamicImage::ImageRgb8(rgb.clone()).to_luma8()),
            DynamicImage::ImageLumaA8(DynamicImage::ImageRgb8(rgb).to_luma_alpha8()),
        ];
        for img in &cases {
            assert_eq!(mean_chroma(img), reference(img), "{:?}", img.color());
        }
        assert!(mean_chroma(&cases[0]) > COLOR_KEEP_CHROMA, "彩图样本要真有色度");
        assert_eq!(mean_chroma(&cases[2]), 0.0);
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

    #[test]
    fn is_downscalable_by_ext() {
        assert!(is_downscalable("OEBPS/images/p1.JPG"));
        assert!(is_downscalable("a/b.png"));
        assert!(!is_downscalable("style.css"));
        assert!(!is_downscalable("cover.gif"));
    }
}
