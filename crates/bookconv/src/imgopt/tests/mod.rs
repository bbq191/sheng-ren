//! `imgopt` 各段的小测试（拆分自 `imgopt.rs`）：共用造图与解码辅助。

include!("comic.rs");
include!("dark.rs");
include!("formats.rs");
include!("text.rs");
include!("trim.rs");

use super::*;
use image::codecs::jpeg::JpegEncoder;
use image::imageops::FilterType;
use image::{DynamicImage, GenericImageView, ImageFormat, RgbImage};
use profile::CoverThumbFit;
use std::io::Cursor;

    fn jpeg_of(w: u32, h: u32) -> Vec<u8> {
        let img = DynamicImage::ImageRgb8(RgbImage::from_fn(w, h, |x, y| {
            image::Rgb([(x % 256) as u8, (y % 256) as u8, 128])
        }));
        let mut buf = Vec::new();
        JpegEncoder::new_with_quality(&mut buf, 90)
            .encode_image(&img)
            .unwrap();
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
        profile::fixture("xochitl")
            .unwrap()
            .readable(profile::Format::Epub)
    }

    /// 生产入口，白边取缺省 1px。
    fn prep(bytes: &[u8], area: Screen, grayscale: bool) -> Option<Vec<u8>> {
        prepare_comic_page_for_epub(bytes, area, 1, grayscale)
    }

    /// 解码 + 裁边（生产的前两步）：返回 (裁后的图, 产物格式, 是否已经和原图不同——裁了边、转了灰度或换了格式)。
    fn decode_trim_comic(
        bytes: &[u8],
        grayscale: bool,
    ) -> Option<(DynamicImage, ImageFormat, bool)> {
        let src = decode_comic(bytes, grayscale, MAX_COMIC_DECODE_PIXELS, true, false)?;
        let orig = src.img.dimensions();
        let (img, _, _) = trim_page(src.img, TrimMode::WhiteOnly);
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
        let px: Vec<u8> = (0..h)
            .flat_map(|y| (0..w).map(move |x| (((x / 5 + y / 5) % 2) * 60) as u8))
            .collect();
        let mut buf = Vec::new();
        image::ImageEncoder::write_image(
            JpegEncoder::new_with_quality(&mut buf, 95),
            &px,
            w,
            h,
            image::ExtendedColorType::L8,
        )
        .unwrap();
        buf
    }

    fn rgb_jpeg(w: u32, h: u32, f: impl Fn(u32, u32) -> [u8; 3]) -> Vec<u8> {
        let mut buf = Vec::new();
        JpegEncoder::new_with_quality(&mut buf, 95)
            .encode_image(&DynamicImage::ImageRgb8(RgbImage::from_fn(w, h, |x, y| {
                image::Rgb(f(x, y))
            })))
            .unwrap();
        buf
    }

    fn mean_luma(bytes: &[u8]) -> f64 {
        let g = image::load_from_memory(bytes).unwrap().to_luma8();
        g.as_raw().iter().map(|&v| f64::from(v)).sum::<f64>() / g.as_raw().len() as f64
    }
