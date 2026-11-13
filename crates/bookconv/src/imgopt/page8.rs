//! 归一后的 8 位整页（[`Page8`]）与灰度转换（拆分自 `imgopt.rs`）。

use image::imageops::FilterType;
use image::ImageFormat;

use super::decode::fir_lanczos3;

/// 转成 8 位灰度，结果和 `image` 0.25 的 `DynamicImage::into_luma8` 逐字节相同。8 位 RGB（彩色页在黑白屏上转灰度，最常见的情形）
/// 走 [`rgb8_into_luma8`]，其余类型交给 `image`。
pub(crate) fn into_luma8(img: image::DynamicImage) -> image::GrayImage {
    match img {
        image::DynamicImage::ImageRgb8(c) => rgb8_into_luma8(c),
        other => other.into_luma8(),
    }
}

/// `image` 0.25 把 sRGB 原色的 RGB 转灰度时用的亮度系数（`moxcms` 由 BT.709 原色、D65 白点算出的 XYZ 矩阵的 Y 行，f32 的位）。
const SRGB_LUMA: [f32; 3] = [
    f32::from_bits(0x3e59_c05b),
    f32::from_bits(0x3f37_15fc),
    f32::from_bits(0x3d93_cf75),
];

/// 8 位 RGB → 8 位灰度，和 `image` 0.25 的 `into_luma8` 逐字节相同，只是快：它按 256 像素一批展开成 f32、算亮度、每个像素调一次
/// `roundf`（x86-64 基线没有取整指令，是函数调用），2026-10-07 量《死亡筆記》彩色扫描页转灰度占整页处理的约两成。这里照它的
/// 运算顺序逐像素算：分量乘 1/255 → 按 [`SRGB_LUMA`] 乘加（编译目标有 FMA 时是融合乘加，同 `image` 的 `multiply_accumulate`）→
/// 乘 255 → 四舍五入（远离零）→ 饱和转 u8。色彩空间不是 sRGB 原色的（系数不同）交给 `image` 自己转。测试里和 `image` 逐个对照。
pub(crate) fn rgb8_into_luma8(img: image::RgbImage) -> image::GrayImage {
    let cs = img.color_space();
    if cs.primaries != image::metadata::CicpColorPrimaries::SRgb {
        return image::DynamicImage::ImageRgb8(img).into_luma8();
    }
    // 和 `image` 的 `multiply_accumulate` 同一个条件：编译目标有 FMA（aarch64 一律有）时是融合乘加，否则先乘（舍入）再加
    const FUSED: bool = cfg!(any(
        all(
            any(target_arch = "x86", target_arch = "x86_64"),
            target_feature = "fma"
        ),
        all(target_arch = "aarch64", target_feature = "neon")
    ));
    // `image` 的算法（逐项照搬，查不准时用它）：分量乘 1/255 → 按系数乘加（第一项 `0 + r'·kr` 就是乘积舍入一次）→ 乘 255 → 取整
    const R: f32 = 1.0 / 255.0;
    let k = SRGB_LUMA;
    let exact = |p: &[u8]| -> u8 {
        let (r, g, b) = (
            f32::from(p[0]) * R,
            f32::from(p[1]) * R,
            f32::from(p[2]) * R,
        );
        let v = if FUSED {
            b.mul_add(k[2], g.mul_add(k[1], r * k[0]))
        } else {
            r * k[0] + g * k[1] + b * k[2]
        } * 255.0;
        // `v.round() as u8`：v 不小于 0；`v - 整数部分` 在这个范围里是精确的，≥ 0.5 进一，和 `round` 的远离零一致；超过 255 的饱和
        let i = v as i32;
        (i + i32::from(v - i as f32 >= 0.5)).clamp(0, 255) as u8
    };
    // 快路：同一个亮度 `Σ kᵢ·cᵢ` 用 2^-20 定点查表相加（每项误差不到 2^-21），加 0.5 取整数部分。f32 那条路的误差不到 1e-4，
    // 定点值的小数部分离 .5 超过 2^-10（约 0.001）时两边一定舍到同一个整数；离得更近的（约千分之二的像素）走 `exact`。
    const SHIFT: u32 = 20;
    const NEAR: u32 = 1 << 10;
    let fixed = |k: f32| -> [u32; 256] {
        std::array::from_fn(|c| (c as f64 * f64::from(k) * f64::from(1u32 << SHIFT)).round() as u32)
    };
    let (fr, fg, fb) = (fixed(k[0]), fixed(k[1]), fixed(k[2]));
    let (w, h) = img.dimensions();
    let raw = img.as_raw();
    let mut px = vec![0u8; raw.len() / 3];
    for (o, p) in px.iter_mut().zip(raw.as_chunks::<3>().0) {
        let s = fr[p[0] as usize] + fg[p[1] as usize] + fb[p[2] as usize] + (1 << (SHIFT - 1));
        let t = s & ((1 << SHIFT) - 1);
        *o = if (NEAR..(1 << SHIFT) - NEAR).contains(&t) {
            (s >> SHIFT).min(255) as u8
        } else {
            exact(p)
        };
    }
    let mut out = image::GrayImage::from_raw(w, h, px).expect("逐像素映射，长度正好是宽×高");
    // 同 `image`：输出沿用原图的色彩空间
    let _ = out.set_color_space(cs);
    out
}

/// 灰度整页的像素（RGB 的按三个分量里最暗的取：比 [`crate::comiccrop`] 的门槛暗的分量就算内容）。
pub(crate) fn darkness_plane(img: &Page8) -> std::borrow::Cow<'_, [u8]> {
    match img {
        Page8::Gray(g) => std::borrow::Cow::Borrowed(g.as_raw()),
        Page8::Rgb(c) => std::borrow::Cow::Owned(
            c.as_raw()
                .as_chunks::<3>()
                .0
                .iter()
                .map(|p| p[0].min(p[1]).min(p[2]))
                .collect(),
        ),
    }
}

/// [`Page8::is_gray_content`]：三个分量相差超过这么多算"有颜色"的像素。JPEG 存成 RGB 的黑白页分量差一般在个位数。
const GRAY_SPREAD: u8 = 16;
/// [`Page8::is_gray_content`]：有颜色的像素不超过总数的这个比例（千分之二）仍算灰度内容（小块彩色水印、扫描的杂点）。
const GRAY_OUTLIERS_PER_MILLE: usize = 2;

/// 归一后的 8 位整页：单通道灰度或 RGB。漫画页解码时一次归一（[`decode_comic`]），文字书插图解出来正好是这两种时也走它
/// （[`downscale_into`]）；裁边、缩放、补白、编码都只对这两种写，不再各自留一个"其它类型"的分支。
/// 公开给 [`decode_page`] 的调用方：取尺寸、缩放、编码用下面的公开方法。
pub enum Page8 {
    Gray(image::GrayImage),
    Rgb(image::RgbImage),
}

impl Page8 {
    /// 本来就是 8 位灰度/RGB 的图直接接过来（不拷贝）；其它类型原样退回。
    pub(crate) fn try_from_dynamic(img: image::DynamicImage) -> Result<Page8, image::DynamicImage> {
        match img {
            image::DynamicImage::ImageLuma8(g) => Ok(Page8::Gray(g)),
            image::DynamicImage::ImageRgb8(c) => Ok(Page8::Rgb(c)),
            other => Err(other),
        }
    }

    pub(crate) fn into_dynamic(self) -> image::DynamicImage {
        match self {
            Page8::Gray(g) => image::DynamicImage::ImageLuma8(g),
            Page8::Rgb(c) => image::DynamicImage::ImageRgb8(c),
        }
    }

    /// `(宽, 高)`。
    pub fn dimensions(&self) -> (u32, u32) {
        match self {
            Page8::Gray(g) => g.dimensions(),
            Page8::Rgb(c) => c.dimensions(),
        }
    }

    /// 内容是不是灰度：单通道的一律是；RGB 的看三个分量相差超过 [`GRAY_SPREAD`] 的像素是否不超过千分之 [`GRAY_OUTLIERS_PER_MILLE`]
    /// （存成 RGB 的黑白漫画、扫描插图算灰度，彩页、彩色插图不算）。
    pub fn is_gray_content(&self) -> bool {
        let Page8::Rgb(c) = self else { return true };
        let px = c.as_raw().as_chunks::<3>().0;
        let limit = px.len() * GRAY_OUTLIERS_PER_MILLE / 1000;
        let mut colored = 0usize;
        for p in px {
            if p.iter().max().unwrap_or(&0) - p.iter().min().unwrap_or(&0) > GRAY_SPREAD {
                colored += 1;
                if colored > limit {
                    return false;
                }
            }
        }
        true
    }

    /// 所有分量里的 (最小, 最大) 值（[`autocontrast_lut`] 的黑点、白点；内容是灰度的页三个分量差不多，同 PIL 按亮度取）。
    pub(crate) fn extrema(&self) -> (u8, u8) {
        let raw: &[u8] = match self {
            Page8::Gray(g) => g,
            Page8::Rgb(c) => c,
        };
        raw.iter()
            .fold((u8::MAX, u8::MIN), |(lo, hi), &v| (lo.min(v), hi.max(v)))
    }

    /// 每个分量按查找表换值（[`gamma_lut`]）。
    pub fn apply_lut(&mut self, lut: &[u8; 256]) {
        let raw: &mut [u8] = match self {
            Page8::Gray(g) => g,
            Page8::Rgb(c) => c,
        };
        for v in raw {
            *v = lut[usize::from(*v)];
        }
    }

    /// 像素的 RGB（灰度给 `[v; 3]`，不必先造一份 3 倍大的 RGB 副本）。
    pub(crate) fn rgb_at(&self, x: u32, y: u32) -> [u8; 3] {
        match self {
            Page8::Gray(g) => [g.get_pixel(x, y)[0]; 3],
            Page8::Rgb(c) => c.get_pixel(x, y).0,
        }
    }

    /// 裁出 `(x, y)` 起 `w × h` 的一块。
    pub(crate) fn crop(self, x: u32, y: u32, w: u32, h: u32) -> Page8 {
        self.crop_ref(x, y, w, h)
    }

    /// 同 [`Page8::crop`]，但保留原图（跨页要先后裁左右两半）。
    pub(crate) fn crop_ref(&self, x: u32, y: u32, w: u32, h: u32) -> Page8 {
        match self {
            Page8::Gray(g) => Page8::Gray(image::imageops::crop_imm(g, x, y, w, h).to_image()),
            Page8::Rgb(c) => Page8::Rgb(image::imageops::crop_imm(c, x, y, w, h).to_image()),
        }
    }

    /// 删掉 `cuts` 里的行区间（升序、不重叠），其余行按原顺序拼起来（裁格间空白用）。
    pub(crate) fn remove_rows(&self, cuts: &[(u32, u32)]) -> Page8 {
        if cuts.is_empty() {
            return match self {
                Page8::Gray(g) => Page8::Gray(g.clone()),
                Page8::Rgb(c) => Page8::Rgb(c.clone()),
            };
        }
        let keep = |y: u32| !cuts.iter().any(|&(a, b)| a <= y && y < b);
        match self {
            Page8::Gray(g) => {
                let (w, h) = g.dimensions();
                let mut raw = Vec::with_capacity((w * h) as usize);
                for y in 0..h {
                    if keep(y) {
                        let s = (y * w) as usize;
                        raw.extend_from_slice(&g.as_raw()[s..s + w as usize]);
                    }
                }
                let nh = raw.len() as u32 / w.max(1);
                Page8::Gray(image::GrayImage::from_raw(w, nh, raw).expect("按行拼接，长度正好"))
            }
            Page8::Rgb(c) => {
                let (w, h) = c.dimensions();
                let stride = w as usize * 3;
                let mut raw = Vec::with_capacity(stride * h as usize);
                for y in 0..h {
                    if keep(y) {
                        let s = y as usize * stride;
                        raw.extend_from_slice(&c.as_raw()[s..s + stride]);
                    }
                }
                let nh = raw.len() as u32 / stride.max(1) as u32;
                Page8::Rgb(image::RgbImage::from_raw(w, nh, raw).expect("按行拼接，长度正好"))
            }
        }
    }

    /// 放到 `cw × ch`、底色 `fill`（灰度值，彩色三个分量相同）的画布上，左上角在 `(x, y)`。
    pub(crate) fn paste_on(&self, cw: u32, ch: u32, x: u32, y: u32, fill: u8) -> Page8 {
        match self {
            Page8::Gray(g) => {
                let mut canvas = image::GrayImage::from_pixel(cw, ch, image::Luma([fill]));
                image::imageops::overlay(&mut canvas, g, x as i64, y as i64);
                Page8::Gray(canvas)
            }
            Page8::Rgb(c) => {
                let mut canvas = image::RgbImage::from_pixel(cw, ch, image::Rgb([fill; 3]));
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
    pub fn resize_lanczos3(self, dw: u32, dh: u32) -> Page8 {
        use fast_image_resize::PixelType;
        let (w, h) = self.dimensions();
        let (raw, pt) = match &self {
            Page8::Gray(g) => (g.as_raw(), PixelType::U8),
            Page8::Rgb(c) => (c.as_raw(), PixelType::U8x3),
        };
        let out = fir_lanczos3(raw, w, h, pt, dw, dh).and_then(|v| match self {
            Page8::Gray(_) => image::GrayImage::from_raw(dw, dh, v).map(Page8::Gray),
            Page8::Rgb(_) => image::RgbImage::from_raw(dw, dh, v).map(Page8::Rgb),
        });
        out.unwrap_or_else(|| {
            match self
                .into_dynamic()
                .resize_exact(dw, dh, FilterType::Lanczos3)
            {
                image::DynamicImage::ImageLuma8(g) => Page8::Gray(g),
                other => Page8::Rgb(other.into_rgb8()),
            }
        })
    }

    /// 按 `fmt` 编码（JPEG 质量 `jpeg_quality`）；JPEG、PNG 以外 → `None`。JPEG **灰度保持单分量**（`image` 0.25 的
    /// `JpegEncoder::encode_image(&DynamicImage)` 对 `ImageLuma8` 也会转成 3 分量 RGB 输出，2026-09-20 实测 SOF 分量数=3、回读 `Rgb8`）。
    /// JPEG 编码时哈夫曼表按这张图重做（无损，见 [`crate::jpegopt`]）：[`crate::jpegopt::encode_optimized`] 直接从像素编，
    /// 和"`image` 的 `JpegEncoder` 编一遍再 [`crate::jpegopt::optimize_verified`]"逐字节相同，只是快（不按通用表编一遍再解回来）。
    /// 那种写法会 panic 的极端情形（系数超出通用哈夫曼表的范围）这里返回 `None`。
    pub fn encode(self, fmt: ImageFormat, jpeg_quality: u8) -> Option<Vec<u8>> {
        use image::{ExtendedColorType, ImageEncoder};
        let (w, h) = self.dimensions();
        let (raw, ct) = match &self {
            Page8::Gray(g) => (g.as_raw(), ExtendedColorType::L8),
            Page8::Rgb(c) => (c.as_raw(), ExtendedColorType::Rgb8),
        };
        let mut out = Vec::new();
        match fmt {
            // 同样画质小约 7%–16%
            ImageFormat::Jpeg => crate::jpegopt::encode_optimized(
                raw,
                w,
                h,
                ct == ExtendedColorType::L8,
                jpeg_quality,
            ),
            ImageFormat::Png => {
                image::codecs::png::PngEncoder::new(&mut out)
                    .write_image(raw, w, h, ct)
                    .ok()?;
                Some(out)
            }
            _ => None,
        }
    }
}
