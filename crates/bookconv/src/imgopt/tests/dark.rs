// 黑底页、16 位图与跨页拆分相关测试。

    /// 黑底页测试图：`w × h` 灰度，底色 `bg`，中间 `(x0..x1, y0..y1)` 是亮的画（白底上横竖的细黑线，不均匀、不会被当底色裁掉）。
    fn framed_png(w: u32, h: u32, bg: u8, inner: (u32, u32, u32, u32)) -> Vec<u8> {
        let (x0, y0, x1, y1) = inner;
        let img = image::GrayImage::from_fn(w, h, |x, y| {
            let v = if (x0..x1).contains(&x) && (y0..y1).contains(&y) {
                if (x + y) % 9 == 0 {
                    30
                } else {
                    230
                }
            } else {
                bg
            };
            image::Luma([v])
        });
        let mut png = Vec::new();
        image::DynamicImage::ImageLuma8(img)
            .write_to(&mut std::io::Cursor::new(&mut png), ImageFormat::Png)
            .unwrap();
        png
    }

    fn dark_page(bytes: &[u8], dark_bg: bool) -> Option<Vec<u8>> {
        prepare_comic_page_limited(
            bytes,
            test_area(),
            1,
            true,
            ComicPage {
                gamma: Some(1.1),
                autocontrast: true,
                dark_bg,
                ..Default::default()
            },
            MAX_COMIC_DECODE_PIXELS,
        )
    }

    #[test]
    fn dark_background_jpeg_with_stray_outer_line_still_gets_dark_fill() {
        // 黑底 22、最外 1px 一圈浅灰杂边（200，不到白边阈值、裁白边裁不掉），JPEG 编码：c62 每条边一圈 12px 里 1px 亮就判不上
        let img = image::GrayImage::from_fn(1200, 1300, |x, y| {
            let d = x.min(y).min(1199 - x).min(1299 - y);
            let v = if d == 0 {
                200
            } else if (150..1050).contains(&x) && (150..1150).contains(&y) {
                if (x + y) % 9 == 0 {
                    30
                } else {
                    230
                }
            } else {
                22
            };
            image::Luma([v])
        });
        let mut jpg = Vec::new();
        image::DynamicImage::ImageLuma8(img)
            .write_to(&mut std::io::Cursor::new(&mut jpg), ImageFormat::Jpeg)
            .unwrap();
        let on = image::load_from_memory(&dark_page(&jpg, true).unwrap())
            .unwrap()
            .to_luma8();
        let (w, h) = on.dimensions();
        for (x, y) in [(0, 0), (w - 1, h - 1)] {
            assert!(
                on.get_pixel(x, y).0[0] < 40,
                "补的是黑边（JPEG 底色略浮），不是白边：({x},{y})={}",
                on.get_pixel(x, y).0[0]
            );
        }
    }

    #[test]
    fn dark_background_page_keeps_layout_and_only_fill_changes() {
        // 扫描的黑是深灰 22，四周一圈 150px 黑边；页比屏幕扁，上下要补边
        let png = framed_png(1200, 1300, 22, (150, 150, 1050, 1150));
        let on = image::load_from_memory(&dark_page(&png, true).unwrap())
            .unwrap()
            .to_luma8();
        let off = image::load_from_memory(&dark_page(&png, false).unwrap())
            .unwrap()
            .to_luma8();
        assert_eq!(on.dimensions(), off.dimensions(), "输出尺寸一样");
        let (w, h) = on.dimensions();
        // 内容（不是补边的部分）位置一样：关着时补的是白，找第一行 / 最后一行不全白的
        let off_rows: Vec<u32> = (0..h)
            .filter(|&y| !(0..w).all(|x| off.get_pixel(x, y).0[0] > 250))
            .collect();
        let (top, bot) = (off_rows[0], *off_rows.last().unwrap());
        assert!(top > 0, "上下有补边：{top}");
        let corner = on.get_pixel(0, 0).0[0];
        assert!(
            corner < 16,
            "补的是黑边（底色过加深表），不是白边：{corner}"
        );
        assert_eq!(on.get_pixel(w - 1, h - 1).0[0], corner);
        // 关着时补白的那几行，开着时整行是底色；画面那几行和关着时逐像素接近（JPEG、加深有误差）：位置、缩放都一样，只是补边颜色不同
        for y in (0..h)
            .step_by(7)
            .chain([top.saturating_sub(3), top, bot, (bot + 3).min(h - 1)])
        {
            if y < top || y > bot {
                assert!(
                    (0..w).all(|x| on.get_pixel(x, y).0[0].abs_diff(corner) <= 4),
                    "第 {y} 行应是补边"
                );
            } else {
                // 左右的补边（关着白、开着底色）除外，其余逐像素接近
                let bad = (0..w).find(|&x| {
                    let (a, b) = (on.get_pixel(x, y).0[0], off.get_pixel(x, y).0[0]);
                    a.abs_diff(b) > 24 && !(b > 250 && a.abs_diff(corner) <= 4)
                });
                assert!(bad.is_none(), "画面没裁没放大：第 {y} 行 x={bad:?} 对不上");
            }
        }
        let mid = (top + bot) / 2;
        // 黑边留着：中间一行左边还有 >60px 的黑
        let dark_run = (0..w)
            .take_while(|&x| on.get_pixel(x, mid).0[0] < 64)
            .count();
        assert!(dark_run > 60, "黑边不裁：{dark_run}px");
    }

    #[test]
    fn white_page_bytes_unchanged_with_dark_bg_on() {
        let png = framed_png(1200, 1500, 255, (150, 150, 1050, 1350));
        assert_eq!(
            dark_page(&png, true),
            dark_page(&png, false),
            "白底页开不开逐字节一样"
        );
        let jpg = gray_jpeg_of(1000, 1400, 255);
        assert_eq!(dark_page(&jpg, true), dark_page(&jpg, false));
    }

    #[test]
    fn bleeding_dark_art_and_dark_corners_are_not_dark_pages() {
        // 出血画：上、左、右暗，下边是亮的画（三条边暗）
        let three = framed_png(1200, 1500, 22, (150, 150, 1050, 1500));
        assert_eq!(
            dark_page(&three, true),
            dark_page(&three, false),
            "三边暗的出血画不判"
        );
        // 白底页上一个大暗角
        let corner = {
            let img = image::GrayImage::from_fn(1200, 1500, |x, y| {
                image::Luma([if x < 500 && y < 600 {
                    15
                } else if (x + y) % 9 == 0 {
                    30
                } else {
                    250
                }])
            });
            let mut png = Vec::new();
            image::DynamicImage::ImageLuma8(img)
                .write_to(&mut std::io::Cursor::new(&mut png), ImageFormat::Png)
                .unwrap();
            png
        };
        assert_eq!(
            dark_page(&corner, true),
            dark_page(&corner, false),
            "局部暗角不判"
        );
        // 满版深色画（棋盘格 0/60）：四边暗，但底色中位数落在 60（> 48），JPEG 振铃还有亮于 64 的，不当黑底页
        let art = black_jpeg(1200, 1500);
        assert_eq!(
            dark_page(&art, true),
            dark_page(&art, false),
            "满版深色画不判"
        );
    }


    /// 2026-11-10 复验阻断 2：16 位 RGB PNG 转灰度必须和改前逐字节相同（不能先截成 8 位再转）；
    /// 16 位 R=G=B 开格间空白照常收，16 位彩色不收。
    #[test]
    fn sixteen_bit_rgb_keeps_original_grayscale_conversion() {
        let area = Screen {
            width: 1072,
            height: 1448,
        };
        let png16 = |color: bool| {
            let mut buf = Vec::new();
            let img =
                image::ImageBuffer::<image::Rgb<u16>, Vec<u16>>::from_fn(800, 2000, |_, y| {
                    let gap = (600..640).contains(&y) || (1240..1280).contains(&y);
                    image::Rgb(if gap {
                        [65535, 65535, 65535]
                    } else if color {
                        [50000, 100, 100]
                    } else {
                        [10000, 10000, 10000]
                    })
                });
            image::DynamicImage::ImageRgb16(img)
                .write_to(&mut std::io::Cursor::new(&mut buf), ImageFormat::Png)
                .unwrap();
            buf
        };
        let expected = |png: &[u8]| {
            image::load_from_memory(png)
                .unwrap()
                .into_luma8()
                .into_raw()
        };
        let gray = |p: Page8| match p {
            Page8::Gray(g) => g.into_raw(),
            Page8::Rgb(c) => c.into_raw(),
        };
        for color in [true, false] {
            let png = png16(color);
            let off = decode_comic(&png, true, MAX_COMIC_DECODE_PIXELS, true, false)
                .unwrap()
                .img;
            let on = decode_comic(&png, true, MAX_COMIC_DECODE_PIXELS, true, true)
                .unwrap()
                .img;
            assert_eq!(gray(off), expected(&png), "关着：16 位转灰度和改前一致");
            assert_eq!(gray(on), expected(&png), "开着：转灰度也一样");
        }
        let color = png16(true);
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
        assert_eq!(off, on, "16 位彩色页不裁格间空白");
        let gray16 = png16(false);
        let off = prepare_comic_page_limited(
            &gray16,
            area,
            0,
            true,
            ComicPage::default(),
            MAX_COMIC_DECODE_PIXELS,
        )
        .unwrap();
        let on = prepare_comic_page_limited(
            &gray16,
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
        assert_ne!(off, on, "16 位 R=G=B 页照常裁");
    }

    /// 跨页拆开后两半各自处理：左半 40、右半 200，最终图（带白边）左半仍明显暗于右半；1.1 宽的页不拆。
    #[test]
    fn spread_halves_are_split_and_processed() {
        let png = |w: u32, h: u32, split: u32| {
            let img = image::GrayImage::from_fn(w, h, |x, _| {
                image::Luma([if x < split { 40 } else { 200 }])
            });
            let mut out = Vec::new();
            image::DynamicImage::ImageLuma8(img)
                .write_to(&mut std::io::Cursor::new(&mut out), ImageFormat::Png)
                .unwrap();
            out
        };
        let area = test_screen();
        let (left, right) = prepare_comic_spread(
            &png(1601, 1100, 800),
            area,
            1,
            true,
            ComicPage::default(),
            MAX_COMIC_DECODE_PIXELS,
        )
        .unwrap();
        let avg = |b: &[u8]| {
            let img = image::load_from_memory(b).unwrap().to_luma8();
            let sum: u64 = img.as_raw().iter().map(|&v| u64::from(v)).sum();
            sum as f64 / f64::from(img.width() * img.height())
        };
        assert!(avg(&left) < 120.0, "左半平均 {}", avg(&left));
        assert!(avg(&right) > 180.0, "右半平均 {}", avg(&right));
        assert!(
            prepare_comic_spread(
                &png(1100, 1000, 550),
                area,
                1,
                true,
                ComicPage::default(),
                MAX_COMIC_DECODE_PIXELS
            )
            .is_none(),
            "1.1 宽的页不拆"
        );
    }
