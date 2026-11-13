// 文字书插图、封面缩略图、缩放与排版相关测试。

    #[test]
    fn cover_thumbnail_center_crops_to_aspect() {
        let jpg = |w: u32, h: u32| {
            let mut b = Vec::new();
            image::DynamicImage::ImageRgb8(RgbImage::from_fn(w, h, |x, _| {
                image::Rgb([(x % 256) as u8, 0, 0])
            }))
            .write_to(&mut std::io::Cursor::new(&mut b), image::ImageFormat::Jpeg)
            .unwrap();
            b
        };
        let dims = |b: Vec<u8>| image::load_from_memory(&b).unwrap().dimensions();
        // 宽的裁左右、窄的裁上下；长边超过 1200 的缩下来；黑白屏转灰度
        assert_eq!(
            dims(
                cover_thumbnail(
                    &jpg(750, 1000),
                    0.65,
                    CoverThumbFit::Crop,
                    false,
                    MAX_DECODE_PIXELS
                )
                .unwrap()
            ),
            (650, 1000)
        );
        assert_eq!(
            dims(
                cover_thumbnail(
                    &jpg(610, 1000),
                    0.65,
                    CoverThumbFit::Crop,
                    false,
                    MAX_DECODE_PIXELS
                )
                .unwrap()
            ),
            (610, 938)
        );
        assert_eq!(
            dims(
                cover_thumbnail(
                    &jpg(1800, 2400),
                    0.65,
                    CoverThumbFit::Crop,
                    false,
                    MAX_DECODE_PIXELS
                )
                .unwrap()
            ),
            (780, 1200)
        );
        let g = cover_thumbnail(
            &jpg(750, 1000),
            0.65,
            CoverThumbFit::Crop,
            true,
            MAX_DECODE_PIXELS,
        )
        .unwrap();
        assert_eq!(
            image::load_from_memory(&g).unwrap().color(),
            image::ColorType::L8
        );
        // 比例差不到 0.005：不做
        assert_eq!(
            cover_thumbnail(
                &jpg(652, 1000),
                0.65,
                CoverThumbFit::Crop,
                false,
                MAX_DECODE_PIXELS
            ),
            None
        );
        // 扫描的窄白边（上 20、下 30、右 20，右边最外一列灰线）去掉再裁；白超过 5% 的那一边不动（封面本身的白底）
        let png = |w: u32, h: u32, f: &dyn Fn(u32, u32) -> u8| {
            let mut b = Vec::new();
            image::DynamicImage::ImageLuma8(image::GrayImage::from_fn(w, h, |x, y| {
                image::Luma([f(x, y)])
            }))
            .write_to(&mut std::io::Cursor::new(&mut b), image::ImageFormat::Png)
            .unwrap();
            b
        };
        let scan = png(700, 1000, &|x, y| {
            if x == 699 {
                200
            } else if !(20..970).contains(&y) || x >= 680 {
                255
            } else {
                60
            }
        });
        assert_eq!(
            thumb_scan_margins(
                image::load_from_memory(&scan).unwrap().to_luma8().as_raw(),
                700,
                1000
            ),
            (0, 20, 20, 30)
        );
        assert_eq!(
            dims(
                cover_thumbnail(&scan, 0.65, CoverThumbFit::Crop, true, MAX_DECODE_PIXELS).unwrap()
            ),
            (618, 950)
        );
        let poster = png(600, 1000, &|_, y| {
            if (110..900).contains(&y) {
                60
            } else {
                255
            }
        });
        assert_eq!(
            thumb_scan_margins(
                image::load_from_memory(&poster)
                    .unwrap()
                    .to_luma8()
                    .as_raw(),
                600,
                1000
            ),
            (0, 0, 0, 0),
            "白占 11%、10%：不动"
        );
    }

    #[test]
    fn cover_thumbnail_pads_white_without_cropping() {
        // 四块纯色，验证补白/缩放没有裁切或拉伸内容
        let jpg = |w: u32, h: u32| {
            let mut b = Vec::new();
            image::DynamicImage::ImageRgb8(RgbImage::from_fn(w, h, |x, y| {
                let left = x < w / 2;
                let top = y < h / 2;
                match (left, top) {
                    (true, true) => image::Rgb([200, 30, 30]),
                    (false, true) => image::Rgb([30, 200, 30]),
                    (true, false) => image::Rgb([30, 30, 200]),
                    (false, false) => image::Rgb([220, 220, 30]),
                }
            }))
            .write_to(&mut std::io::Cursor::new(&mut b), image::ImageFormat::Jpeg)
            .unwrap();
            b
        };
        let aspect = 0.7403;
        let decode = |b: &[u8]| image::load_from_memory(b).unwrap().to_rgb8();
        let ratio_ok = |w: u32, h: u32| (w as f32 - aspect * h as f32).abs() <= 1.0;
        // 宽封面（0.8 > 0.7403）：补上下
        let src = jpg(800, 1000);
        let out =
            cover_thumbnail(&src, aspect, CoverThumbFit::Pad, false, MAX_DECODE_PIXELS).unwrap();
        let img = decode(&out);
        assert_eq!(img.dimensions(), (800, 1081));
        assert!(ratio_ok(800, 1081), "比例在 ±1px 内");
        // 上下白边
        for y in [3, 1077] {
            for x in [3, 400, 797] {
                let p = img.get_pixel(x, y).0;
                assert!(p.iter().all(|&v| v >= 240), "({x},{y}) 是白边：{p:?}");
            }
        }
        // 内容没裁：四块的中心都在，且位置没被拉伸（y 偏移 40）
        let orig = decode(&src);
        for (x, y) in [(200, 250), (600, 250), (200, 750), (600, 750)] {
            let a = orig.get_pixel(x, y).0;
            let b = img.get_pixel(x, y + 40).0;
            let diff = (0..3).map(|i| a[i].abs_diff(b[i])).max().unwrap();
            assert!(diff <= 16, "({x},{y}) 内容变了：{a:?} → {b:?}");
        }
        // 窄封面（0.6 < 0.7403）：补左右
        let out = cover_thumbnail(
            &jpg(600, 1000),
            aspect,
            CoverThumbFit::Pad,
            false,
            MAX_DECODE_PIXELS,
        )
        .unwrap();
        let img = decode(&out);
        assert_eq!(img.dimensions(), (740, 1000));
        assert!(ratio_ok(740, 1000));
        for x in [3, 736] {
            let p = img.get_pixel(x, 500).0;
            assert!(p.iter().all(|&v| v >= 240), "x={x} 是白边：{p:?}");
        }
        // 长边上限按补完的画布算
        let out = cover_thumbnail(
            &jpg(1800, 2400),
            aspect,
            CoverThumbFit::Pad,
            false,
            MAX_DECODE_PIXELS,
        )
        .unwrap();
        let (w, h) = decode(&out).dimensions();
        assert_eq!((w, h), (889, 1200));
        assert!(ratio_ok(w, h));
        // 比例已合：不做；黑白屏转灰度
        assert_eq!(
            cover_thumbnail(
                &jpg(740, 1000),
                aspect,
                CoverThumbFit::Pad,
                false,
                MAX_DECODE_PIXELS
            ),
            None
        );
        let g = cover_thumbnail(
            &jpg(800, 1000),
            aspect,
            CoverThumbFit::Pad,
            true,
            MAX_DECODE_PIXELS,
        )
        .unwrap();
        assert_eq!(
            image::load_from_memory(&g).unwrap().color(),
            image::ColorType::L8
        );
    }


    #[test]
    fn darken_gray_image_only_touches_opaque_gray_illustrations() {
        let gray = gray_jpeg_of(600, 400, 0);
        let out = darken_gray_image(&gray, 1.3, MAX_DECODE_PIXELS).expect("灰度插图加深");
        let img = image::load_from_memory(&out).unwrap();
        assert_eq!(
            (img.color(), img.dimensions()),
            (image::ColorType::L8, (600, 400)),
            "尺寸不变、灰度保持单分量"
        );
        assert!(mean_luma(&out) < mean_luma(&gray) - 5.0);
        let rgb_gray = rgb_jpeg(600, 400, |x, _| [(x % 200) as u8; 3]);
        assert!(
            darken_gray_image(&rgb_gray, 1.3, MAX_DECODE_PIXELS).is_some(),
            "存成 RGB 的灰度插图也算"
        );
        assert!(
            darken_gray_image(&jpeg_of(600, 400), 1.3, MAX_DECODE_PIXELS).is_none(),
            "彩色插图不动"
        );
        assert!(
            darken_gray_image(&gray_jpeg_of(800, 150, 0), 1.3, MAX_DECODE_PIXELS).is_none(),
            "短边不到 200：分隔线、图标之类不动"
        );
        let mut png = Vec::new();
        DynamicImage::ImageLumaA8(image::GrayAlphaImage::from_pixel(
            300,
            300,
            image::LumaA([100, 128]),
        ))
        .write_to(&mut Cursor::new(&mut png), ImageFormat::Png)
        .unwrap();
        assert!(
            darken_gray_image(&png, 1.3, MAX_DECODE_PIXELS).is_none(),
            "带透明通道的不动"
        );
        let mut opaque = Vec::new();
        DynamicImage::ImageLuma8(image::GrayImage::from_pixel(300, 300, image::Luma([128])))
            .write_to(&mut Cursor::new(&mut opaque), ImageFormat::Png)
            .unwrap();
        let out = darken_gray_image(&opaque, 1.3, MAX_DECODE_PIXELS).unwrap();
        assert_eq!(
            image::guess_format(&out).unwrap(),
            ImageFormat::Png,
            "PNG 仍是 PNG"
        );
        assert_eq!(
            image::load_from_memory(&out)
                .unwrap()
                .to_luma8()
                .get_pixel(5, 5)
                .0[0],
            gamma_lut(1.3)[128]
        );
    }

    #[test]
    fn prepare_epub_page_does_not_upscale_png() {
        // PNG 不放大（无损放大体积暴涨）：700×1000 按自己的比例尺补白到阅读范围的宽高比，图不缩放。
        // 受限边（宽）的白边按比例：1px × 700/840 ≈ 0.83 → 1px（至少 1px）；画布 702 宽、高按 842:1455 算。
        let img = DynamicImage::ImageRgb8(RgbImage::from_fn(700, 1000, |x, y| {
            image::Rgb([(x % 256) as u8, (y % 256) as u8, 9])
        }));
        let mut png = Vec::new();
        img.write_to(&mut Cursor::new(&mut png), ImageFormat::Png)
            .unwrap();
        let out = prep(&png, test_area(), false).unwrap();
        assert_eq!(
            image::guess_format(&out).unwrap(),
            ImageFormat::Png,
            "PNG 仍是 PNG"
        );
        let page = image::load_from_memory(&out).unwrap();
        assert_eq!(page.dimensions(), (702, 1213), "702 × round(702×1455/842)");
        // 图原样（没缩放）贴在中间：左上角 (1, 106)，像素逐个相同（PNG 无损）
        let back = page.to_rgb8();
        let src = img.to_rgb8();
        assert!(
            (0..700).step_by(37).all(|x| (0..1000)
                .step_by(41)
                .all(|y| back.get_pixel(x + 1, y + 106) == src.get_pixel(x, y))),
            "图不缩放、居中"
        );
        assert_eq!(
            *back.get_pixel(0, 500),
            image::Rgb([255, 255, 255]),
            "左边 1px 白边"
        );
    }

    #[test]
    fn resize_lanczos3_simd_matches_image_crate_closely() {
        // 合成带高对比边缘+渐变的 RGB 与灰度图，SIMD 结果与 image 库实现的像素差必须很小（均值 <0.5 灰阶）。
        let rgb = DynamicImage::ImageRgb8(RgbImage::from_fn(1091, 1592, |x, y| {
            let edge = if (x / 37 + y / 41) % 2 == 0 { 20 } else { 235 };
            image::Rgb([edge, ((x + y) % 256) as u8, (x % 256) as u8])
        }));
        let gray = DynamicImage::ImageLuma8(image::GrayImage::from_fn(700, 1000, |x, y| {
            image::Luma([((x * 3 + y * 5) % 256) as u8])
        }));
        for (img, (dw, dh)) in [(rgb, (934u32, 1363u32)), (gray, (934, 1334))] {
            let fast = Page8::try_from_dynamic(img.clone())
                .ok()
                .unwrap()
                .resize_lanczos3(dw, dh)
                .into_dynamic();
            let slow = img.resize_exact(dw, dh, FilterType::Lanczos3);
            assert_eq!(fast.color(), slow.color());
            assert_eq!(fast.dimensions(), (dw, dh));
            let (a, b) = (fast.as_bytes(), slow.as_bytes());
            let mean = a
                .iter()
                .zip(b)
                .map(|(x, y)| x.abs_diff(*y) as f64)
                .sum::<f64>()
                / a.len() as f64;
            assert!(
                mean < 0.5,
                "SIMD 与 image 库 Lanczos3 差距过大: 均值 {mean}"
            );
        }
    }

    #[test]
    fn frames_follow_the_given_screen() {
        // 非 Move 的屏幕（掌阅 Air 3 Pro，1440×1920）：缩放框与漫画页框都按传入的屏幕算，不再是 954×1696。
        let ireader = profile::get("ireader").unwrap().screen;
        let (w, h) =
            image::load_from_memory(&downscale_for_epub(&jpeg_of(2400, 3200), ireader).unwrap())
                .unwrap()
                .dimensions();
        assert_eq!((w, h), (1440, 1920), "竖图按 1440×1920 框等比缩");
        let out = prep(&gray_jpeg_of(1091, 1592, 0), ireader, false).expect("要补白到屏幕比例");
        assert_eq!(
            image::load_from_memory(&out).unwrap().dimensions(),
            (1440, 1920)
        );
    }

    #[test]
    fn epub_portrait_box_caps_width_954() {
        // EPUB 内嵌图一律卡宽 ≤954（防行内横幅溢出竖屏）
        // 横图 1696×630 的内联横幅（《飘》真机溢出源）→ 954×~355
        let banner = jpeg_of(1696, 630);
        let (w, h) = image::load_from_memory(&downscale_for_epub(&banner, test_screen()).unwrap())
            .unwrap()
            .dimensions();
        assert_eq!(w, 954, "横幅宽必须卡到 954");
        assert!(h < 400, "保比 h={h}");
        // 方图 → 954×954；竖图 1000×3000 → 565×1696
        let sq = jpeg_of(2000, 2000);
        assert_eq!(
            image::load_from_memory(&downscale_for_epub(&sq, test_screen()).unwrap())
                .unwrap()
                .dimensions(),
            (954, 954)
        );
        let tall = jpeg_of(1000, 3000);
        let (w, h) = image::load_from_memory(&downscale_for_epub(&tall, test_screen()).unwrap())
            .unwrap()
            .dimensions();
        assert!(w <= 954 && h == 1696, "竖图 {w}x{h}");
    }

    /// 造一张带纯白边框的图：中心是彩色渐变，四边留白。
    /// 文字书插图缩放改走 SIMD 后：尺寸与 `DynamicImage::resize` 完全相同、像素差很小；灰度 JPEG 仍是单分量。
    #[test]
    fn downscale_for_epub_keeps_resize_dimensions_and_gray_jpeg() {
        for &(w, h) in &[(2000u32, 1500u32), (1200, 3000), (955, 10), (3001, 2999)] {
            let src = jpeg_of(w, h);
            let want = image::load_from_memory(&src).unwrap().resize(
                test_screen().width,
                test_screen().height,
                FilterType::Lanczos3,
            );
            let got = image::load_from_memory(
                &downscale_for_epub(&src, test_screen()).expect("超框要缩"),
            )
            .unwrap();
            assert_eq!(got.dimensions(), want.dimensions(), "{w}x{h}");
            let (a, b) = (got.to_rgb8(), want.to_rgb8());
            let mean_diff = a
                .as_raw()
                .iter()
                .zip(b.as_raw())
                .map(|(x, y)| x.abs_diff(*y) as u64)
                .sum::<u64>() as f64
                / a.as_raw().len() as f64;
            assert!(mean_diff < 2.0, "{w}x{h} 像素均差 {mean_diff}");
        }
        let gray = gray_jpeg_of(1800, 2400, 0);
        let out = downscale_for_epub(&gray, test_screen()).unwrap();
        assert!(
            matches!(
                image::load_from_memory(&out).unwrap(),
                DynamicImage::ImageLuma8(_)
            ),
            "灰度 JPEG 不该被写成 3 分量"
        );
    }

    /// 排版算术：受限边两侧正好 `m` 像素，另一条边两侧不少于 `m`（差奇数时右、下多 1），画布就是阅读范围，宽高比不变
    /// （另一条边是按比例四舍五入的结果，误差不超过半像素），不拉伸不压扁。
    #[test]
    fn layout_puts_exact_margin_on_constrained_axis_and_keeps_aspect() {
        let areas = [
            profile::get("ireader")
                .unwrap()
                .readable(profile::Format::Epub),
            test_area(),
            Screen {
                width: 300,
                height: 400,
            },
        ];
        let sizes = [
            (1091u32, 1592u32),
            (1687, 2480),
            (566, 800),
            (700, 1600),
            (1600, 1000),
            (2000, 2000),
            (1264, 1680),
            (842, 1455),
            (3001, 4999),
            (301, 1999),
        ];
        for area in areas {
            for m in [0u32, 1, 3] {
                for (w, h) in sizes {
                    let l = comic_layout(w, h, area, m, true);
                    let ctx = format!("{w}x{h} → {area:?} m={m}: {l:?}");
                    assert_eq!((l.canvas_w, l.canvas_h), (area.width, area.height), "{ctx}");
                    let (left, right) = (l.x, l.canvas_w - l.x - l.nw);
                    let (top, bottom) = (l.y, l.canvas_h - l.y - l.nh);
                    let width_bound = (left, right) == (m, m);
                    assert!(
                        width_bound || (top, bottom) == (m, m),
                        "受限边两侧正好 m: {ctx}"
                    );
                    assert!(
                        left >= m && right >= m && top >= m && bottom >= m,
                        "四边都不少于 m: {ctx}"
                    );
                    assert!(right - left <= 1 && bottom - top <= 1, "居中: {ctx}");
                    // 宽高比：另一条边 = 受限边 × 原比例，四舍五入
                    let err = if width_bound {
                        l.nh as f64 - h as f64 * l.nw as f64 / w as f64
                    } else {
                        l.nw as f64 - w as f64 * l.nh as f64 / h as f64
                    };
                    assert!(err.abs() <= 0.5, "宽高比误差 {err}: {ctx}");
                }
            }
        }
    }

    /// 端到端量产物：整张纯黑、画面到边的页，放大（掌阅 1264×1680，乱马同款 1091×1592）和缩小（死亡笔记同款
    /// 1687×2480 到 Move 842×1455）后，解码量深色内容到四边的距离：受限边两侧 1px，另一条边两侧相差不超过 1px。
    #[test]
    fn output_pages_measure_one_pixel_on_constrained_axis() {
        let ireader = profile::get("ireader")
            .unwrap()
            .readable(profile::Format::Epub);
        for (w, h, area) in [
            (1091u32, 1592u32, ireader),
            (1687, 2480, test_area()),
            (1300, 900, test_area()),
        ] {
            let out = prep(&black_jpeg(w, h), area, false).expect("要处理");
            let page = image::load_from_memory(&out).unwrap();
            assert_eq!(page.dimensions(), (area.width, area.height), "{w}x{h}");
            let (l, t, r, b) = dark_margins(&page);
            assert!(
                (l, r) == (1, 1) || (t, b) == (1, 1),
                "{w}x{h}: 受限边两侧 1px，实际 左{l} 上{t} 右{r} 下{b}"
            );
            assert!(
                l.abs_diff(r) <= 1 && t.abs_diff(b) <= 1,
                "{w}x{h}: 居中，实际 左{l} 上{t} 右{r} 下{b}"
            );
        }
        // 白边可配：3px
        let out = prepare_comic_page_for_epub(&black_jpeg(1091, 1592), ireader, 3, false).unwrap();
        let (l, t, r, b) = dark_margins(&image::load_from_memory(&out).unwrap());
        assert!(
            (t, b) == (3, 3) && l.abs_diff(r) <= 1,
            "左{l} 上{t} 右{r} 下{b}"
        );
    }

    /// 不放大的排版（PNG 等、或放大倍数超过上限）：图不缩放，画布是阅读范围的宽高比，白边按比例缩小但至少 1px。
    #[test]
    fn native_scale_layout_pads_to_area_aspect_without_scaling() {
        let area = test_area();
        for (w, h) in [(700u32, 1000u32), (400, 1200), (600, 400)] {
            let l = comic_layout(w, h, area, 1, false);
            assert_eq!((l.nw, l.nh), (w, h), "不缩放");
            let err = l.canvas_w as f64 / l.canvas_h as f64 - area.aspect() as f64;
            assert!(
                err.abs() < 2.0 / l.canvas_h.min(l.canvas_w) as f64,
                "{w}x{h}: 画布比例 {}x{}",
                l.canvas_w,
                l.canvas_h
            );
            let (left, right, top, bottom) = (l.x, l.canvas_w - l.x - w, l.y, l.canvas_h - l.y - h);
            assert!(
                (left, right) == (1, 1) || (top, bottom) == (1, 1),
                "{w}x{h}: {l:?}"
            );
            assert!(left >= 1 && right >= 1 && top >= 1 && bottom >= 1);
        }
        // 白边按比例：300×400 的阅读范围、8px 白边，150×100 的图（宽受限，一半比例尺）→ 白边 4px
        let l = comic_layout(
            150,
            100,
            Screen {
                width: 300,
                height: 400,
            },
            8,
            false,
        );
        assert_eq!((l.x, l.canvas_w), (4, 158));
        assert_eq!(
            comic_layout(
                150,
                100,
                Screen {
                    width: 300,
                    height: 400
                },
                0,
                false
            )
            .x,
            0,
            "白边 0 时不补"
        );
    }

    /// 已经排好的页再处理一遍原样保留（原字节），不一代代重编码；正好是阅读范围大小、画面顶到边的页也不为 1px 重编码。
    #[test]
    fn already_laid_out_pages_are_left_untouched() {
        let area = test_area();
        let once = prep(&gray_jpeg_of(1091, 1592, 0), area, false).unwrap();
        assert!(prep(&once, area, false).is_none(), "产物再跑一遍：原样");
        assert!(
            prep(&black_jpeg(842, 1455), area, false).is_none(),
            "阅读范围大小、画面到边：原样"
        );
        assert!(
            prep(&black_jpeg(842, 1455), area, true).is_none(),
            "本来就是灰度：黑白屏也原样"
        );
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
            DynamicImage::ImageRgb8(RgbImage::from_fn(w, h, |x, y| {
                image::Rgb([(x % 256) as u8, (y % 256) as u8, 9])
            }))
            .write_to(&mut Cursor::new(&mut b), ImageFormat::Png)
            .unwrap();
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
            let out =
                prepare_comic_page_for_epub(&src, kindle, 1, true).unwrap_or_else(|| src.clone());
            let dims = image::load_from_memory(&out).unwrap().dimensions();
            assert_eq!(
                crate::comicfxl::fills_canvas(dims, kindle.width, kindle.height),
                want,
                "{dims:?}"
            );
        }
    }

