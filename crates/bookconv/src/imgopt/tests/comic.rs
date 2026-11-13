// 漫画页处理、缩略图与跨页/格间空白相关测试。

    /// 解码器 panic 按处理失败算（抓远程图、解封面以前没兜住，一张坏图摔掉整本书的优化）。
    #[test]
    fn webp_to_png_takes_first_frame_of_animation() {
        assert!(is_convertible_webp(TEST_ANIM_WEBP));
        let png = webp_to_png(TEST_ANIM_WEBP).expect("动图取第一帧");
        let img = image::load_from_memory(&png).unwrap();
        assert!(
            matches!(img, image::DynamicImage::ImageRgb8(_)),
            "全不透明的存 RGB"
        );
        assert_eq!(img.to_rgb8().get_pixel(1, 1).0, [200, 30, 30], "第一帧");
    }

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
        assert_eq!(
            image::load_from_memory(&out).unwrap().dimensions(),
            (842, 1455)
        );
    }

    #[test]
    fn prepare_epub_page_trim_then_fit_in_one_pass() {
        // 带 60px 白边：先裁再适配，仍是阅读范围尺寸，且只编码一次（尺寸即证明一趟到位）。
        let out = prep(&gray_jpeg_of(800, 1200, 60), test_area(), false).unwrap();
        assert_eq!(
            image::load_from_memory(&out).unwrap().dimensions(),
            (842, 1455)
        );
    }

    #[test]
    fn prepare_epub_page_pads_tall_narrow_page_left_right_without_exceeding_width() {
        // 比阅读范围"窄"的高瘦页（如 700×1600）：高度顶到 1455，宽度 < 842，左右对称补白到 842——宽绝不超阅读范围。
        let out = prep(&gray_jpeg_of(700, 1600, 0), test_area(), false).expect("高瘦页必须缩+补白");
        assert_eq!(
            image::load_from_memory(&out).unwrap().dimensions(),
            (842, 1455)
        );
    }

    #[test]
    fn epub_pad_tolerance_catches_page_one_point_six_percent_off_frame() {
        // 真机 e2e：一张比框窄 1.6% 的页被 2% 容差放过，图片少 4.5pt 宽且左右不对称——补白容差必须更严。
        let w = (842.0_f32 * 0.984).round() as u32;
        let out = prep(&gray_jpeg_of(w, 1455, 0), test_area(), false).expect("偏差 1.6% 必须补白");
        assert_eq!(
            image::load_from_memory(&out).unwrap().dimensions(),
            (842, 1455)
        );
    }

    #[test]
    fn nominal_screen_is_used_as_is_when_no_readable_area() {
        // 没有内置阅读范围的设备按标称屏幕补白。
        let out =
            prep(&gray_jpeg_of(566, 800, 0), test_screen(), false).expect("低分辨率必须预放大");
        assert_eq!(
            image::load_from_memory(&out).unwrap().dimensions(),
            (954, 1696)
        );
        assert!(
            prep(&gray_jpeg_of(954, 1696, 0), test_screen(), false).is_none(),
            "已是屏幕页、无白边：原字节"
        );
    }

    #[test]
    fn prepare_epub_page_leaves_untouched_when_already_device_page_or_tiny_icon() {
        assert!(
            prep(&gray_jpeg_of(842, 1455, 0), test_area(), false).is_none(),
            "已是阅读范围尺寸、无白边：原字节零损失"
        );
        assert!(
            prep(&gray_jpeg_of(200, 300, 0), test_area(), false).is_none(),
            "装饰小图且无白边：原样"
        );
    }

    #[test]
    fn grayscale_devices_get_single_channel_comic_pages() {
        let color = jpeg_of(842, 1455); // 彩色、尺寸正好是阅读范围、没有白边：彩色屏原样零损失
        assert!(prep(&color, test_area(), false).is_none());
        let out = prep(&color, test_area(), true).expect("黑白屏要转灰度");
        let img = image::load_from_memory(&out).unwrap();
        assert_eq!(
            img.color(),
            image::ColorType::L8,
            "单通道 8 位灰度（256 级）"
        );
        assert_eq!(img.dimensions(), (842, 1455));
        // 需要缩放的彩色页：彩色屏保持 RGB，黑白屏出灰度
        let big = jpeg_of(1600, 2400);
        assert_eq!(
            image::load_from_memory(&prep(&big, test_area(), false).unwrap())
                .unwrap()
                .color(),
            image::ColorType::Rgb8
        );
        assert_eq!(
            image::load_from_memory(&prep(&big, test_area(), true).unwrap())
                .unwrap()
                .color(),
            image::ColorType::L8
        );
        // 不抖动：纯中灰（128）转完仍是均匀中灰，不会变成黑白点
        let mut mid = Vec::new();
        JpegEncoder::new_with_quality(&mut mid, 95)
            .encode_image(&DynamicImage::ImageRgb8(RgbImage::from_pixel(
                842,
                1455,
                image::Rgb([128, 128, 128]),
            )))
            .unwrap();
        let g = image::load_from_memory(&prep(&mid, test_area(), true).unwrap())
            .unwrap()
            .to_luma8();
        // 灰色的边不是留白、不裁：整页按 1px 白边重排，白边以内的像素都是中灰
        let (w, h) = g.dimensions();
        assert!(
            (3..w - 3).all(|x| (3..h - 3).all(|y| (120..=136).contains(&g.get_pixel(x, y).0[0]))),
            "中灰保持中灰，没有抖动成黑白点"
        );
    }

    #[test]
    fn gamma_lut_keeps_black_and_white_and_darkens_midtones() {
        let l = gamma_lut(1.45);
        assert_eq!((l[0], l[255]), (0, 255));
        assert_eq!(l[128], 94, "255·(128/255)^1.45");
        assert!(l.windows(2).all(|w| w[0] <= w[1]), "单调");
        assert!((1..255).all(|v| l[v] <= v as u8));
        assert_eq!(gamma_lut(1.0), std::array::from_fn(|v| v as u8));
    }

    #[test]
    fn gray_content_is_judged_by_channel_spread() {
        let gray = Page8::Rgb(RgbImage::from_fn(100, 100, |x, _| {
            image::Rgb([x as u8, x as u8 + 3, x as u8])
        }));
        assert!(gray.is_gray_content(), "存成 RGB 的灰度（分量差个位数）");
        assert!(Page8::Gray(image::GrayImage::new(5, 5)).is_gray_content());
        let colored = Page8::Rgb(RgbImage::from_fn(100, 100, |x, y| {
            if x < 10 && y < 10 {
                image::Rgb([255, 0, 0])
            } else {
                image::Rgb([200, 200, 200])
            }
        }));
        assert!(!colored.is_gray_content(), "1% 的像素是红的：算彩色");
        let speck = Page8::Rgb(RgbImage::from_fn(100, 100, |x, y| {
            if x == 0 && y < 2 {
                image::Rgb([255, 0, 0])
            } else {
                image::Rgb([200, 200, 200])
            }
        }));
        assert!(speck.is_gray_content(), "千分之二以内的杂色不算");
    }

    #[test]
    fn comic_gamma_darkens_gray_pages_only() {
        let area = test_area();
        let gray = gray_jpeg_of(1600, 2400, 0);
        let plain = prepare_comic_page_limited(
            &gray,
            area,
            1,
            false,
            ComicPage {
                gamma: None,
                ..Default::default()
            },
            MAX_COMIC_DECODE_PIXELS,
        )
        .unwrap();
        assert_eq!(
            Some(plain.clone()),
            prep(&gray, area, false),
            "不加深时和以前一样"
        );
        let dark = prepare_comic_page_limited(
            &gray,
            area,
            1,
            false,
            ComicPage {
                gamma: Some(1.45),
                ..Default::default()
            },
            MAX_COMIC_DECODE_PIXELS,
        )
        .unwrap();
        assert_eq!(
            image::load_from_memory(&dark).unwrap().dimensions(),
            image::load_from_memory(&plain).unwrap().dimensions()
        );
        assert!(mean_luma(&dark) < mean_luma(&plain) - 10.0, "灰度页加深");
        // 彩页在彩色屏上原样；本来不用动的灰度页要加深就得重编码
        let color = jpeg_of(1600, 2400);
        assert_eq!(
            prepare_comic_page_limited(
                &color,
                area,
                1,
                false,
                ComicPage {
                    gamma: Some(1.45),
                    ..Default::default()
                },
                MAX_COMIC_DECODE_PIXELS
            ),
            prep(&color, area, false)
        );
        let fitted = gray_jpeg_of(842, 1455, 0);
        assert!(prep(&fitted, area, false).is_none());
        assert!(prepare_comic_page_limited(
            &fitted,
            area,
            1,
            false,
            ComicPage {
                gamma: Some(1.45),
                ..Default::default()
            },
            MAX_COMIC_DECODE_PIXELS
        )
        .is_some());
        // 黑白屏：彩页转成灰度后也算灰度页
        let on_gray = prepare_comic_page_limited(
            &color,
            area,
            1,
            true,
            ComicPage {
                gamma: Some(1.45),
                ..Default::default()
            },
            MAX_COMIC_DECODE_PIXELS,
        )
        .unwrap();
        assert!(mean_luma(&on_gray) < mean_luma(&prep(&color, area, true).unwrap()) - 5.0);
    }

    #[test]
    fn autocontrast_lut_stretches_like_kcc() {
        let l = autocontrast_lut((22, 255)).unwrap();
        assert_eq!(
            (l[0], l[22], l[255]),
            (0, 0, 255),
            "最黑 → 0、最白 → 255，两端外钳住"
        );
        assert_eq!(l[100], 85, "(100−22)·255/233 = 85.4");
        assert_eq!(
            autocontrast_lut((0, 182)).unwrap()[182],
            255,
            "PIL 截断在这里落成 254"
        );
        assert!(l.windows(2).all(|w| w[0] <= w[1]), "单调");
        assert_eq!(autocontrast_lut((0, 255)), None, "已经拉满");
        assert_eq!(
            autocontrast_lut((60, 200)),
            None,
            "范围不到 159：有意的低对比度，不拉"
        );
        assert!(autocontrast_lut((40, 199)).is_some());
    }

    #[test]
    fn comic_autocontrast_uses_pre_resize_extrema_and_composes_with_gamma() {
        let area = test_area();
        // 30–230 的横向渐变（PNG 无损）：缩放会在边缘振出更黑的点，黑点、白点要在缩放前取
        let mut png = Vec::new();
        image::DynamicImage::ImageLuma8(image::GrayImage::from_fn(1600, 2400, |x, _| {
            image::Luma([(30 + x * 200 / 1599) as u8])
        }))
        .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
        .unwrap();
        let luma = |b: &[u8]| image::load_from_memory(b).unwrap().to_luma8();
        let plain = luma(
            &prepare_comic_page_limited(
                &png,
                area,
                1,
                false,
                ComicPage {
                    gamma: None,
                    ..Default::default()
                },
                MAX_COMIC_DECODE_PIXELS,
            )
            .unwrap(),
        );
        let stretched = luma(
            &prepare_comic_page_limited(
                &png,
                area,
                1,
                false,
                ComicPage {
                    gamma: None,
                    autocontrast: true,
                    ..Default::default()
                },
                MAX_COMIC_DECODE_PIXELS,
            )
            .unwrap(),
        );
        let both = luma(
            &prepare_comic_page_limited(
                &png,
                area,
                1,
                false,
                ComicPage {
                    gamma: Some(1.2),
                    autocontrast: true,
                    ..Default::default()
                },
                MAX_COMIC_DECODE_PIXELS,
            )
            .unwrap(),
        );
        let s = autocontrast_lut((30, 230)).unwrap();
        let g = gamma_lut(1.2);
        let (x, y) = (plain.width() / 2, plain.height() / 2);
        let v = plain.get_pixel(x, y).0[0];
        assert_eq!(
            stretched.get_pixel(x, y).0[0],
            s[v as usize],
            "按原图的 30–230 拉"
        );
        assert_eq!(
            both.get_pixel(x, y).0[0],
            g[s[v as usize] as usize],
            "先拉满、再伽马，合成一张表"
        );
        assert_eq!(stretched.get_pixel(0, 0).0[0], 255, "补的白边不变");
    }

    /// 跨页阈值（2026-11-10 vinci 确认 1.2）和奇数宽的正中切法。
    #[test]
    fn spread_aspect_and_widths() {
        assert!(is_spread(1600, 1100), "1600/1100 ≈ 1.45");
        assert!(!is_spread(1100, 1000), "1.1 不拆");
        assert!(!is_spread(700, 1000), "竖页不拆");
        assert!(is_spread(1200, 1000), "正好 1.2 算跨页");
        assert_eq!(spread_widths(1600), (800, 800));
        assert_eq!(spread_widths(1601), (800, 801), "奇数宽右半多 1px");
    }

    /// 2026-11-10 审查阻断 1：彩色按转灰度之前的原图判——黑白屏上彩页不裁格间空白，R=G=B 的 RGB 页照常裁。
    #[test]
    fn gutter_crop_checks_color_before_grayscale() {
        let area = Screen {
            width: 1072,
            height: 1448,
        };
        let panels = |rgb: bool| {
            image::RgbImage::from_fn(800, 2000, |_, y| {
                let v = if (600..640).contains(&y) || (1240..1280).contains(&y) {
                    [255, 255, 255]
                } else if rgb {
                    [200, 20, 20]
                } else {
                    [40, 40, 40]
                };
                image::Rgb(v)
            })
        };
        let mut color = Vec::new();
        image::DynamicImage::ImageRgb8(panels(true))
            .write_to(&mut std::io::Cursor::new(&mut color), ImageFormat::Png)
            .unwrap();
        let run = |b: &[u8]| {
            let img = image::load_from_memory(b).unwrap().to_luma8();
            let (w, h) = img.dimensions();
            let dark = |y: u32| (0..w).any(|x| img.get_pixel(x, y).0[0] < 128);
            let mut y = 0;
            while y < h && !dark(y) {
                y += 1;
            }
            let start = y;
            while y < h && dark(y) {
                y += 1;
            }
            y - start
        };
        let off = prepare_comic_page_limited(
            &color,
            area,
            0,
            true,
            ComicPage::default(),
            MAX_COMIC_DECODE_PIXELS,
        )
        .unwrap();
        let on = prepare_comic_page_limited(
            &color,
            area,
            0,
            true,
            ComicPage {
                gutter_crop: true,
                ..Default::default()
            },
            MAX_COMIC_DECODE_PIXELS,
        )
        .unwrap();
        assert_eq!(off, on, "彩色页转灰度前是彩色：不裁格间空白");

        let mut gray_rgb = Vec::new();
        let mut enc = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut gray_rgb, 100);
        enc.encode_image(&panels(false)).unwrap();
        let off = prepare_comic_page_limited(
            &gray_rgb,
            area,
            0,
            true,
            ComicPage::default(),
            MAX_COMIC_DECODE_PIXELS,
        )
        .unwrap();
        let on = prepare_comic_page_limited(
            &gray_rgb,
            area,
            0,
            true,
            ComicPage {
                gutter_crop: true,
                ..Default::default()
            },
            MAX_COMIC_DECODE_PIXELS,
        )
        .unwrap();
        assert_ne!(off, on, "R=G=B 的 RGB 页照常裁");
        assert!(run(&on) > run(&off), "格变高");
    }
