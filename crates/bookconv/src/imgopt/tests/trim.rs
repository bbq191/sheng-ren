// 裁边、透明合成与解码上限相关测试。

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
        JpegEncoder::new_with_quality(&mut buf, 100)
            .encode_image(&img)
            .unwrap();
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
        for c in [[0u8, 0, 0], [128, 128, 128], [200, 30, 30], [230, 230, 230]] {
            assert!(trim_dims(&bordered(c)).is_none(), "{c:?} 的边不该裁");
        }
        for c in [[255u8, 255, 255], [240, 242, 238]] {
            assert_eq!(
                trim_dims(&bordered(c)),
                Some((160, 260)),
                "{c:?} 是纸白，要裁"
            );
        }
        // 端到端：黑边页已经是阅读范围大小 → 原样；大一圈的黑边页缩放后黑边还在（画面到边，四周只有 1px 白边）
        let area = test_area();
        let black_frame = |w: u32, h: u32| {
            let px: Vec<u8> = (0..h)
                .flat_map(|y| {
                    (0..w).map(move |x| {
                        if x < 40 || y < 40 || x + 40 >= w || y + 40 >= h {
                            0
                        } else {
                            (128 + (x + y) % 100) as u8
                        }
                    })
                })
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
        };
        assert!(
            prep(&black_frame(area.width, area.height), area, false).is_none(),
            "黑底页不裁不补"
        );
        let out =
            image::load_from_memory(&prep(&black_frame(1091, 1592), area, false).unwrap()).unwrap();
        let (l, t, r, b) = dark_margins(&out);
        assert!(
            (l, r) == (1, 1) || (t, b) == (1, 1),
            "黑边保留、整页缩放：左{l} 上{t} 右{r} 下{b}"
        );
    }

    #[test]
    fn trim_none_when_no_uniform_border() {
        let img = DynamicImage::ImageRgb8(RgbImage::from_fn(200, 300, |x, y| {
            image::Rgb([(x % 256) as u8, (y % 256) as u8, 128])
        }));
        let mut buf = Vec::new();
        JpegEncoder::new_with_quality(&mut buf, 95)
            .encode_image(&img)
            .unwrap();
        assert!(
            trim_dims(&buf).is_none(),
            "画面一直到边缘、没有留白，不该裁"
        );
    }

    #[test]
    fn trim_capped_by_max_fraction_for_near_solid_image() {
        // 几乎整张纯色(只有中心一小块不同)——裁边不能把整张图裁没，单边应被 TRIM_MAX_FRACTION 卡住。
        let img = DynamicImage::ImageRgb8(RgbImage::from_fn(200, 200, |x, y| {
            if (90..110).contains(&x) && (90..110).contains(&y) {
                image::Rgb([0, 0, 0])
            } else {
                image::Rgb([255, 255, 255])
            }
        }));
        let mut buf = Vec::new();
        JpegEncoder::new_with_quality(&mut buf, 100)
            .encode_image(&img)
            .unwrap();
        let (w, h) = trim_dims(&buf).expect("大片留白应触发裁边");
        let cap = (200.0 * TRIM_MAX_FRACTION) as u32;
        assert!(
            w >= 200 - 2 * cap && h >= 200 - 2 * cap,
            "单边最多裁 TRIM_MAX_FRACTION，不能把画面裁没: got {w}x{h}"
        );
    }

    #[test]
    fn trim_handles_margin_beyond_old_cap() {
        // 真机回归（2026-09-19，《镖人》母版库反馈"优化没把大量留白裁切完"）：中文漫画常见的
        // 版权页（CIP 页）实测单边留白能到 22%-29%，旧的 15% 上限在这里会
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
        JpegEncoder::new_with_quality(&mut buf, 100)
            .encode_image(&img)
            .unwrap();
        let (got_w, got_h) = trim_dims(&buf).expect("留白应触发裁边");
        let old_cap = (w as f32 * 0.15) as u32;
        assert!(
            got_w < w - 2 * old_cap,
            "25% 留白不该被旧的 15% 上限卡住: got {got_w}"
        );
        assert_eq!(
            (got_w, got_h),
            (w - 2 * margin, h - 2 * margin),
            "留白在新上限内应该精确裁掉: got {got_w}x{got_h}"
        );
    }

    #[test]
    fn skips_already_small_image() {
        let small = jpeg_of(800, 600);
        assert!(
            downscale_for_epub(&small, test_screen()).is_none(),
            "已达标图不动（幂等、免二次损失）"
        );
    }

    #[test]
    fn ignores_non_image_bytes() {
        assert!(downscale_for_epub(b"not an image at all", test_screen()).is_none());
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
        DynamicImage::ImageRgba8(rgba)
            .write_to(&mut Cursor::new(&mut png), ImageFormat::Png)
            .unwrap();
        let la = GrayAlphaImage::from_fn(w, h, |x, y| {
            if x < w / 2 {
                image::LumaA([(y % 9 * 20) as u8, 255])
            } else {
                image::LumaA([0, 0])
            }
        });
        let mut la_png = Vec::new();
        DynamicImage::ImageLumaA8(la)
            .write_to(&mut Cursor::new(&mut la_png), ImageFormat::Png)
            .unwrap();
        for (src, grayscale) in [
            (&png, false),
            (&png, true),
            (&la_png, false),
            (&la_png, true),
        ] {
            let (img, _, _) = decode_trim_comic(src, grayscale).expect("能解码");
            let rgb = img.to_rgb8();
            // 裁边会把右侧的纯白透明区当留白裁掉一部分，取裁后最右一列检查：必须是白，不能是黑。
            let right = rgb.get_pixel(rgb.width() - 1, rgb.height() / 2).0;
            assert_eq!(
                right,
                [255, 255, 255],
                "透明区应合成成白色 (grayscale={grayscale})"
            );
            let out = prep(src, test_area(), grayscale).expect("要补白");
            let back = image::load_from_memory(&out).unwrap().to_rgb8();
            let p = back.get_pixel(back.width() - 1, back.height() / 2).0;
            assert!(
                p.iter().all(|&c| c >= 250),
                "产物右侧不该发黑: {p:?} (grayscale={grayscale})"
            );
        }
        let half = flatten_alpha_on_white(DynamicImage::ImageRgba8(RgbaImage::from_pixel(
            1,
            1,
            image::Rgba([0, 0, 0, 128]),
        )))
        .to_rgb8();
        assert_eq!(
            half.get_pixel(0, 0).0,
            [127, 127, 127],
            "半透明黑合成白底 = 中灰"
        );
    }

    /// 带透明的 PNG 插图缩小时按预乘 alpha 缩（v56，同背景图）：透明处存的是黑色时，不透明的白色部分贴着透明区的那几列不再被染灰
    /// （以前 `image` 自带的缩放不预乘，出黑边）。
    #[test]
    fn transparent_png_downscale_has_no_dark_fringe() {
        let (w, h) = (2400u32, 3000u32);
        // 左半不透明白、右半全透明（透明处存的颜色是杂的，有黑有彩，PNG 也没那么好压）
        let img = image::RgbaImage::from_fn(w, h, |x, y| {
            if x < w / 2 {
                image::Rgba([255, 255, 255, 255])
            } else if (x + y) % 7 == 0 {
                image::Rgba([0, 0, 0, 0])
            } else {
                image::Rgba([(x % 256) as u8, (y % 256) as u8, 0, 0])
            }
        });
        let mut png = Vec::new();
        image::DynamicImage::ImageRgba8(img)
            .write_to(&mut Cursor::new(&mut png), ImageFormat::Png)
            .unwrap();
        let out = downscale_for_epub(&png, test_screen()).expect("超框要缩");
        let small = image::load_from_memory(&out).unwrap().into_rgba8();
        let (sw, sh) = small.dimensions();
        assert!(sw <= test_screen().width && sh <= test_screen().height);
        for x in 0..sw {
            let p = small.get_pixel(x, sh / 2).0;
            // 贴着透明区、半透明的那几列也算：颜色该还是白的
            if p[3] >= 8 {
                assert!(
                    p[0] >= 250 && p[1] >= 250 && p[2] >= 250,
                    "白色部分边上被染色（x={x}）：{p:?}"
                );
            }
        }
    }

    #[test]
    fn within_decode_budget_boundary() {
        let fits = |w: u64, h: u64, max_px: u64| w * h <= text_decode_limit(max_px);
        assert!(
            fits(3000, 3000, MAX_COMIC_DECODE_PIXELS),
            "900 万像素，等于上限，应允许"
        );
        assert!(
            !fits(3001, 3000, MAX_COMIC_DECODE_PIXELS),
            "超一点点也该拒绝"
        );
        assert!(!fits(2000, 2000, 3_000_000), "调用方给的上限更小时按它");
    }

    /// 超限图（见 `MAX_DECODE_PIXELS` 文档：阈值按实测峰值内存定）：各解码入口都该直接放弃处理、原样保留，
    /// 不整张解出来。5001×5000 远超 900 万像素，足够验证调用链路。
    #[test]
    fn oversized_image_skipped_by_text_paths_but_comic_pages_are_processed() {
        let huge = gray_jpeg_of(5001, 5000, 0);
        assert!(
            downscale_for_epub(&huge, test_screen()).is_none(),
            "文字书插图：超限图应跳过降采样"
        );
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
        assert!(
            prepare_comic_page_limited(
                &page,
                test_area(),
                1,
                false,
                ComicPage {
                    gamma: None,
                    ..Default::default()
                },
                5_000_000
            )
            .is_none(),
            "超过调用方上限的漫画页原样保留"
        );
        assert_eq!(
            prepare_comic_page_limited(
                &page,
                test_area(),
                1,
                false,
                ComicPage {
                    gamma: None,
                    ..Default::default()
                },
                6_000_000
            ),
            prep(&page, test_area(), false),
            "没超过的和缺省一样"
        );
        assert!(
            downscale_for_epub_limited(&page, test_screen(), 5_000_000).is_none(),
            "插图同理"
        );
        assert_eq!(
            downscale_for_epub_limited(&page, test_screen(), 9_000_000),
            downscale_for_epub(&page, test_screen())
        );
    }

    /// 文件头声明超过 [`MAX_COMIC_DECODE_PIXELS`] 的图（解压炸弹）：不解码，原样保留。
    #[test]
    fn comic_decode_refuses_decompression_bomb_by_header() {
        let mut bomb = black_jpeg(64, 64);
        // 把 SOF0 里的高、宽改成 9000×9000（8100 万像素），数据不变：只读文件头就该拒绝
        let sof = bomb
            .windows(2)
            .position(|w| w == [0xFF, 0xC0])
            .expect("基线 JPEG 有 SOF0");
        bomb[sof + 5..sof + 9].copy_from_slice(&[0x23, 0x28, 0x23, 0x28]);
        assert_eq!(comic_header_dims(&bomb).map(|d| d.1), Some((9000, 9000)));
        assert!(decode_comic(&bomb, false, MAX_COMIC_DECODE_PIXELS, true, false).is_none());
        assert!(prep(&bomb, test_area(), true).is_none());
    }

