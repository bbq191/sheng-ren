// GIF/WebP/EXIF 与灰度转换相关测试。

    fn gif_of(frames: &[image::RgbaImage]) -> Vec<u8> {
        let mut buf = Vec::new();
        {
            let mut enc = image::codecs::gif::GifEncoder::new(&mut buf);
            enc.encode_frames(frames.iter().map(|f| image::Frame::new(f.clone())))
                .unwrap();
        }
        buf
    }

    /// 静态 GIF 页转 PNG 并照常排版；动图原样保留（只取第一帧会丢内容）。
    #[test]
    fn static_gif_page_becomes_png_and_animated_gif_is_kept() {
        let area = Screen {
            width: 300,
            height: 400,
        };
        let page = image::RgbaImage::from_fn(200, 280, |x, y| {
            if (x / 20 + y / 20) % 2 == 0 {
                image::Rgba([0, 0, 0, 255])
            } else {
                image::Rgba([200, 60, 60, 255])
            }
        });
        let gif = gif_of(std::slice::from_ref(&page));
        assert_eq!(pixel_count(&gif), 200 * 280, "GIF 也按文件头算像素额度");
        let out = prep(&gif, area, false).expect("静态 GIF 要处理");
        assert_eq!(image::guess_format(&out).unwrap(), ImageFormat::Png);
        assert_eq!(
            image::load_from_memory(&out).unwrap().dimensions(),
            (212, 282),
            "GIF→PNG 不放大：高受限，白边 1px，宽按 300:400 算"
        );
        assert_eq!(
            converted_media_type("OEBPS/p1.gif", &out),
            Some("image/png")
        );
        let gray = prep(&gif, area, true).unwrap();
        assert_eq!(
            image::load_from_memory(&gray).unwrap().color(),
            image::ColorType::L8,
            "黑白屏转灰度"
        );
        let other = image::RgbaImage::from_pixel(200, 280, image::Rgba([90, 90, 200, 255]));
        assert!(
            prep(&gif_of(&[page, other]), area, false).is_none(),
            "动图原样保留"
        );
        // 装饰小图的 GIF（短边不到阅读范围宽 1/3）没有白边：原样
        let tiny = gif_of(&[image::RgbaImage::from_fn(60, 60, |x, y| {
            image::Rgba([((x + y) % 2 * 200) as u8, 0, 0, 255])
        })]);
        assert!(prep(&tiny, area, false).is_none());
        // 文字书的插图路径不碰 GIF
        assert!(downscale_for_epub(
            &gif,
            Screen {
                width: 100,
                height: 100
            }
        )
        .is_none());
    }

    /// WebP：无损的转 PNG；有损、无损按 RIFF 块识别。
    #[test]
    fn webp_page_is_converted_by_its_encoding() {
        let area = Screen {
            width: 300,
            height: 400,
        };
        let img = image::RgbImage::from_fn(240, 380, |x, y| {
            image::Rgb([(x % 256) as u8, (y % 256) as u8, 30])
        });
        let mut webp = Vec::new();
        image::codecs::webp::WebPEncoder::new_lossless(&mut webp)
            .encode(img.as_raw(), 240, 380, image::ExtendedColorType::Rgb8)
            .unwrap();
        assert_eq!(webp_is_lossless(&webp), Some(true));
        let out = prep(&webp, area, false).expect("WebP 要处理");
        assert_eq!(
            image::guess_format(&out).unwrap(),
            ImageFormat::Png,
            "无损 WebP → PNG"
        );
        assert_eq!(converted_media_type("a/p.webp", &out), Some("image/png"));
        // 有损（VP8）与扩展格式（VP8X 在前）的块识别；认不出的返回 None
        let chunk = |fourcc: &[u8], len: u32| {
            [fourcc, &len.to_le_bytes()[..], &vec![0u8; len as usize]].concat()
        };
        let riff = |body: Vec<u8>| {
            [
                &b"RIFF"[..],
                &((body.len() + 4) as u32).to_le_bytes()[..],
                b"WEBP",
                &body,
            ]
            .concat()
        };
        assert_eq!(webp_is_lossless(&riff(chunk(b"VP8 ", 10))), Some(false));
        assert_eq!(
            webp_is_lossless(&riff([chunk(b"VP8X", 10), chunk(b"VP8 ", 3)].concat())),
            Some(false),
            "奇数长度的块按偶数对齐"
        );
        assert_eq!(
            webp_is_lossless(&riff([chunk(b"VP8X", 10), chunk(b"VP8L", 4)].concat())),
            Some(true)
        );
        assert_eq!(webp_is_lossless(b"RIFF\0\0\0\0WEBP"), None);
        assert_eq!(
            converted_media_type("a/p.jpg", &black_jpeg(8, 8)),
            None,
            "没换格式"
        );
        assert_eq!(
            converted_media_type("a/p.webp", &black_jpeg(8, 8)),
            Some("image/jpeg"),
            "有损 WebP 转成 JPEG"
        );
    }

    /// 横放存储、EXIF 方向 6（顺时针转 90° 显示）的 JPEG：原图左半黑、右半白，摆正后上半黑、下半白。
    fn exif_rotated_jpeg(w: u32, h: u32) -> Vec<u8> {
        let img =
            image::GrayImage::from_fn(w, h, |x, _| image::Luma([if x < w / 2 { 0 } else { 255 }]));
        // TIFF 头（小端）+ 一个 IFD：0x0112 Orientation，SHORT，值 6
        let exif = vec![
            0x49, 0x49, 42, 0, 8, 0, 0, 0, 1, 0, 0x12, 0x01, 3, 0, 1, 0, 0, 0, 6, 0, 0, 0, 0, 0, 0,
            0,
        ];
        let mut buf = Vec::new();
        let mut enc = JpegEncoder::new_with_quality(&mut buf, 95);
        image::ImageEncoder::set_exif_metadata(&mut enc, exif).unwrap();
        image::ImageEncoder::write_image(enc, img.as_raw(), w, h, image::ExtendedColorType::L8)
            .unwrap();
        buf
    }

    /// 上下两半的平均灰度。
    fn top_bottom_luma(img: &DynamicImage) -> (f64, f64) {
        let g = img.to_luma8();
        let (w, h) = g.dimensions();
        let avg = |y0: u32, y1: u32| {
            (y0..y1)
                .flat_map(|y| (0..w).map(move |x| (x, y)))
                .map(|(x, y)| g.get_pixel(x, y)[0] as f64)
                .sum::<f64>()
                / ((y1 - y0) * w) as f64
        };
        (avg(0, h / 3), avg(h - h / 3, h))
    }

    /// 公开的整页接口：`AnyUniform` 连黑边、彩边也裁，`WhiteOnly` 只裁白边；`apply_exif: false` 不按 EXIF 摆正。
    #[test]
    fn public_page_api_trim_modes_and_exif_switch() {
        let bordered = |c: [u8; 3]| {
            let img = DynamicImage::ImageRgb8(RgbImage::from_fn(200, 300, |x, y| {
                if !(20..180).contains(&x) || !(20..280).contains(&y) {
                    image::Rgb(c)
                } else {
                    image::Rgb([(x % 256) as u8, (y % 256) as u8, 128])
                }
            }));
            let mut buf = Vec::new();
            JpegEncoder::new_with_quality(&mut buf, 100)
                .encode_image(&img)
                .unwrap();
            buf
        };
        let opts = PageDecode {
            grayscale: false,
            max_px: MAX_DECODE_PIXELS,
            apply_exif: true,
        };
        for c in [[0u8, 0, 0], [128, 128, 128], [200, 30, 30], [255, 255, 255]] {
            let (img, fmt) = decode_page(&bordered(c), opts).unwrap();
            assert_eq!(fmt, ImageFormat::Jpeg);
            let (img, l, t) = trim_page(img, TrimMode::AnyUniform);
            assert_eq!(
                (img.dimensions(), l, t),
                ((160, 260), 20, 20),
                "{c:?} 的纯色边在 AnyUniform 下要裁"
            );
        }
        let (img, _) = decode_page(&bordered([0, 0, 0]), opts).unwrap();
        assert_eq!(
            trim_page(img, TrimMode::WhiteOnly).0.dimensions(),
            (200, 300),
            "WhiteOnly 不裁黑边"
        );
        // 编码保持格式与尺寸
        let (img, fmt) = decode_page(&bordered([255, 255, 255]), opts).unwrap();
        let out = img
            .resize_lanczos3(100, 150)
            .encode(fmt, JPEG_QUALITY_COMIC)
            .unwrap();
        assert_eq!(
            image::load_from_memory(&out).unwrap().dimensions(),
            (100, 150)
        );
        // EXIF：摆正后宽高互换；不摆正保持存储的宽高
        let src = exif_rotated_jpeg(300, 200);
        assert_eq!(decode_page(&src, opts).unwrap().0.dimensions(), (200, 300));
        assert_eq!(
            decode_page(
                &src,
                PageDecode {
                    apply_exif: false,
                    ..opts
                }
            )
            .unwrap()
            .0
            .dimensions(),
            (300, 200)
        );
        // 解码上限
        assert!(decode_page(
            &src,
            PageDecode {
                max_px: 300 * 200 - 1,
                ..opts
            }
        )
        .is_none());
    }

    #[test]
    fn exif_orientation_is_applied_before_reencoding() {
        let src = exif_rotated_jpeg(3000, 2000);
        assert_eq!(decode_oriented(&src).unwrap().dimensions(), (2000, 3000));
        // 文字书插图：按摆正后的竖图缩进竖框，像素也摆正（重编码不带 EXIF）
        let out =
            image::load_from_memory(&downscale_for_epub(&src, test_screen()).unwrap()).unwrap();
        assert_eq!(
            out.dimensions(),
            (954, 1431),
            "2000×3000 摆正后按竖框缩，而不是按存储的横向宽高"
        );
        let (top, bottom) = top_bottom_luma(&out);
        assert!(top < 30.0 && bottom > 225.0, "上黑下白: {top} {bottom}");
        // 摆正后本来就在框里：不动（原字节连同 EXIF 保留）
        assert!(downscale_for_epub(&exif_rotated_jpeg(1200, 900), test_screen()).is_none());
        // 漫画页：同样先摆正再排版——和直接存成竖图（不带 EXIF）的同一张图结果逐字节相同（PNG 无损，比得了字节）
        let raw = image::RgbImage::from_fn(1200, 800, |x, y| {
            image::Rgb([
                (x * 7 % 256) as u8,
                (y * 5 % 256) as u8,
                ((x * y) % 256) as u8,
            ])
        });
        let png = |img: &image::RgbImage, exif: Option<Vec<u8>>| {
            let mut buf = Vec::new();
            let mut enc = image::codecs::png::PngEncoder::new(&mut buf);
            if let Some(e) = exif {
                image::ImageEncoder::set_exif_metadata(&mut enc, e).unwrap();
            }
            image::ImageEncoder::write_image(
                enc,
                img.as_raw(),
                img.width(),
                img.height(),
                image::ExtendedColorType::Rgb8,
            )
            .unwrap();
            buf
        };
        let exif6 = vec![
            0x49, 0x49, 42, 0, 8, 0, 0, 0, 1, 0, 0x12, 0x01, 3, 0, 1, 0, 0, 0, 6, 0, 0, 0, 0, 0, 0,
            0,
        ];
        let tagged = png(&raw, Some(exif6));
        let upright = png(&image::imageops::rotate90(&raw), None);
        assert_eq!(
            decode_oriented(&tagged).unwrap().to_rgb8(),
            image::imageops::rotate90(&raw)
        );
        assert_eq!(
            prep(&tagged, test_area(), false).unwrap(),
            prep(&upright, test_area(), false).unwrap()
        );
        assert_eq!(
            prep(&tagged, test_area(), true).unwrap(),
            prep(&upright, test_area(), true).unwrap()
        );
        // 已经是阅读范围大小的页，带 EXIF 方向也要重编码（不能原样沿用靠 EXIF 才正的字节）
        let exact = exif_rotated_jpeg(test_area().height, test_area().width);
        assert!(prep(&exact, test_area(), false).is_some());
    }

    #[test]
    fn gif_frames_counted_without_decoding() {
        let f = image::RgbaImage::from_pixel(4, 4, image::Rgba([1, 2, 3, 255]));
        assert_eq!(
            gif_frame_count(&gif_of(std::slice::from_ref(&f)), 2),
            Some(1)
        );
        assert_eq!(
            gif_frame_count(&gif_of(&[f.clone(), f.clone(), f.clone()]), 2),
            Some(2)
        );
        let one = gif_of(&[f]);
        assert_eq!(gif_frame_count(&one[..one.len() - 3], 2), None, "截断");
        assert_eq!(gif_frame_count(b"GIF89a", 2), None);
    }

    /// 快速 RGB → 灰度和 `image` 的 `into_luma8` 逐像素相同：全部 2^24 种颜色排成 4096×4096 一张图比（debug 下约 2.5 秒；
    /// 2026-10-16 以前标 `#[ignore]`、平时只跑抽样的 B 取 12 个值的那版）。
    #[test]
    fn rgb_to_luma_matches_image_exhaustive() {
        let raw: Vec<u8> = (0..1u32 << 24)
            .flat_map(|c| [(c >> 16) as u8, (c >> 8) as u8, c as u8])
            .collect();
        let img = RgbImage::from_raw(4096, 4096, raw).unwrap();
        assert_eq!(
            rgb8_into_luma8(img.clone()).as_raw(),
            DynamicImage::ImageRgb8(img).into_luma8().as_raw()
        );
    }

