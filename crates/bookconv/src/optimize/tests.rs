//! 优化器单测。全部走生产路径 [`optimize_epub_file_streaming`]（经临时文件），不另设内存版实现。
    use super::*;
    use std::io::{Cursor, Read};
    use zip::write::SimpleFileOptions;
    use zip::CompressionMethod;

    /// 测试便利封装：字节进字节出，内部写临时文件走流式路径。
    fn optimize_epub_with(epub: &[u8], opts: &OptimizeOpts) -> Result<(Vec<u8>, Report), String> {
        let t = tempfile::tempdir().unwrap();
        let (input, output) = (t.path().join("in.epub"), t.path().join("out.epub"));
        std::fs::write(&input, epub).unwrap();
        let rep = optimize_epub_file_streaming(&input, &output, opts, |_, _| {})?;
        Ok((std::fs::read(&output).unwrap(), rep))
    }

    fn optimize_epub(epub: &[u8], screen: crate::imgopt::Screen) -> Result<(Vec<u8>, Report), String> {
        optimize_epub_with(epub, &OptimizeOpts::new(screen))
    }

    /// 产物里的幂等标记内容；没有标记 / 不是 zip → `None`。
    fn optimized_version(epub: &[u8]) -> Option<String> {
        let mut ar = ZipArchive::new(Cursor::new(epub)).ok()?;
        let mut f = ar.by_name(OPTIMIZE_MARKER).ok()?;
        let mut s = String::new();
        f.read_to_string(&mut s).ok()?;
        Some(s.trim().to_string())
    }

    fn entry_bytes(epub: &[u8], name: &str) -> Vec<u8> {
        let mut v = Vec::new();
        ZipArchive::new(Cursor::new(epub)).unwrap().by_name(name).unwrap().read_to_end(&mut v).unwrap();
        v
    }

    #[test]
    fn inline_remote_images_fetches_and_keeps_local_and_failed() {
        // 本地图不动；远程抓到→内联改本地名+进资源；远程抓不到→<img> 原样保留（不改书的内容）
        let html = r#"<p><img src="local.png"/><img class="c" src="https://x.com/a.png"/><img src="//y.com/b.png"/><img alt='x>y' src='https://x.com/c.png?a=1&amp;b=2'/></p>"#;
        let mut n = 0usize;
        // 已经优化过的书再跑：书里已有 remote_img_0.png，新抓的图不能重名
        let mut taken: HashSet<String> = ["OEBPS/remote_img_0.png".to_string()].into_iter().collect();
        let (out, res) = inline_remote_images(html, "OEBPS", &mut n, &mut taken, |src: &str| {
            if src.contains("a.png") || src == "https://x.com/c.png?a=1&b=2" { Some((vec![1, 2, 3], "png")) } else { None } // b 抓不到
        });
        assert!(out.contains(r#"src="local.png""#), "本地图应原样: {out}");
        assert!(out.contains(r#"class="c" src="remote_img_1.png""#), "远程抓到应改本地名、避开已有的名字: {out}");
        assert!(!out.contains("x.com"), "抓到的远程 URL 应换成本地名: {out}");
        assert!(out.contains(r#"<img src="//y.com/b.png"/>"#), "抓不到的远程 img 应原样保留: {out}");
        assert!(out.contains(r#"<img alt='x>y' src='remote_img_2.png'/>"#), "单引号、属性值里有 > 也认，字符引用先还原再抓: {out}");
        assert_eq!(res.iter().map(|r| r.0.as_str()).collect::<Vec<_>>(), ["OEBPS/remote_img_1.png", "OEBPS/remote_img_2.png"], "资源落本章目录");
        assert_eq!(res[0].1, vec![1, 2, 3]);
        assert!(has_remote_img(r#"<img alt='a>b' src='https://a/b.jpg'/>"#));
    }

    #[test]
    fn svg_cover_to_img_only_replaces_a_lone_cover_svg() {
        // 审计复现：前一个 <svg> 没有 <image>，旧正则从它一路跨到后面那个 </svg>，把中间的正文吞了
        let page = r#"<html><body><svg width="10" height="10"><text x="0" y="5">图中文字</text></svg><p>这一段正文会不会丢？</p><svg><image xlink:href="a.jpg"/></svg></body></html>"#;
        assert_eq!(svg_cover_to_img(page), page, "页面还有别的可见内容：不是封面页，不动");
        // 真封面页：只有一个 svg、里面只有一张图
        let cover = r#"<html><body><div><svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 600 800"><image width="600" height="800" xlink:href='../Images/cover.jpg'/></svg></div></body></html>"#;
        assert_eq!(
            svg_cover_to_img(cover),
            r#"<html><body><div><img src="../Images/cover.jpg" alt="cover" style="display:block;margin:0 auto;max-width:100%;height:auto;"/></div></body></html>"#
        );
        // svg 里有文字、或有两张图：不动
        let with_text = r#"<html><body><svg><image href="c.jpg"/><text>书名</text></svg></body></html>"#;
        assert_eq!(svg_cover_to_img(with_text), with_text);
        let two = r#"<html><body><svg><image href="a.jpg"/><image href="b.jpg"/></svg></body></html>"#;
        assert_eq!(svg_cover_to_img(two), two);
    }

    /// 端到端设备优化：EPUB 含超大 JPEG + 灰字 CSS + 灰字/细体内联 style，过优化器后
    /// ① 图片缩到 ≤1696px、② 灰字→纯黑、细字重→400。
    #[test]
    fn device_tuning_downscales_image_and_blackens_text() {
        use image::{codecs::jpeg::JpegEncoder, DynamicImage, GenericImageView, RgbImage};
        // 超大 JPEG（3392×1908 = 2× 屏）
        let big = DynamicImage::ImageRgb8(RgbImage::from_fn(3392, 1908, |x, _| {
            image::Rgb([(x % 256) as u8, 100, 150])
        }));
        let mut jpg = Vec::new();
        JpegEncoder::new_with_quality(&mut jpg, 90).encode_image(&big).unwrap();

        let mut buf = Vec::new();
        {
            let mut zw = ZipWriter::new(Cursor::new(&mut buf));
            let stored = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
            zw.start_file("mimetype", stored).unwrap();
            zw.write_all(b"application/epub+zip").unwrap();
            zw.start_file("OEBPS/style.css", stored).unwrap();
            zw.write_all(b"body{color:#333}a{color:blue}").unwrap();
            zw.start_file("OEBPS/img/big.jpg", stored).unwrap();
            zw.write_all(&jpg).unwrap();
            zw.start_file("OEBPS/c1.xhtml", stored).unwrap();
            zw.write_all(r#"<html><body><p style="color:#666;font-weight:300">灰字</p></body></html>"#.as_bytes()).unwrap();
            zw.finish().unwrap();
        }
        let (out, _rep) = optimize_epub(&buf, crate::imgopt::test_screen()).unwrap();
        let mut ar = ZipArchive::new(Cursor::new(&out)).unwrap();
        // ① 图片缩小
        let mut ib = Vec::new();
        ar.by_name("OEBPS/img/big.jpg").unwrap().read_to_end(&mut ib).unwrap();
        let (w, h) = image::load_from_memory(&ib).unwrap().dimensions();
        assert!(w <= 1696 && h <= 1696, "图应缩到 ≤1696，实为 {w}x{h}");
        assert!(ib.len() < jpg.len(), "缩后体积应变小");
        // ② css 灰字→黑、彩色不动
        let mut css = String::new();
        ar.by_name("OEBPS/style.css").unwrap().read_to_string(&mut css).unwrap();
        assert_eq!(css, "body{color:#000000}a{color:blue}", "css 灰字→黑、彩色不动: {css}");
        // ② 内联 style 灰字→黑、细体→400
        let mut x = String::new();
        ar.by_name("OEBPS/c1.xhtml").unwrap().read_to_string(&mut x).unwrap();
        assert!(x.contains(r#"style="color:#000000;font-weight:400""#), "内联提对比: {x}");
    }

    #[test]
    fn comic_epub_gets_higher_quality_reencode_than_text_book() {
        use image::{codecs::jpeg::JpegEncoder, DynamicImage, GenericImageView, RgbImage};
        // x、y 都要变化——否则整张图任意一列（或行）颜色恒定，会被 trim_margins 的"纯色留白"判据
        // 误判成可裁的边框，让漫画路径意外比对照组多裁一刀，干扰这条测试本身要验证的"质量差异"。
        let big = DynamicImage::ImageRgb8(RgbImage::from_fn(2000, 3000, |x, y| image::Rgb([(x % 256) as u8, (y % 256) as u8, 150])));
        let mut jpg = Vec::new();
        JpegEncoder::new_with_quality(&mut jpg, 90).encode_image(&big).unwrap();

        // 漫画书：25 张纯图片页（其中一张是超框大图）
        let mut comic_buf = Vec::new();
        {
            let mut zw = ZipWriter::new(Cursor::new(&mut comic_buf));
            let stored = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
            zw.start_file("mimetype", stored).unwrap();
            zw.write_all(b"application/epub+zip").unwrap();
            let items: String = (1..=25).map(|i| format!(r#"<item id="c{i}" href="c{i}.xhtml" media-type="application/xhtml+xml"/>"#)).collect();
            let spine: String = (1..=25).map(|i| format!(r#"<itemref idref="c{i}"/>"#)).collect();
            zw.start_file("content.opf", stored).unwrap();
            zw.write_all(format!(r#"<package version="3.0"><metadata><dc:title>漫画</dc:title></metadata><manifest>{items}</manifest><spine>{spine}</spine></package>"#).as_bytes()).unwrap();
            for i in 1..=25 {
                zw.start_file(format!("c{i}.xhtml"), stored).unwrap();
                zw.write_all(format!(r#"<html><body><img src="p{i}.jpg"/></body></html>"#).as_bytes()).unwrap();
            }
            zw.start_file("p1.jpg", stored).unwrap();
            zw.write_all(&jpg).unwrap();
            zw.finish().unwrap();
        }
        // 文字书：同一张超框大图，但正文是长文字（不判漫画）
        let long_text = "正".repeat(500);
        let mut text_buf = Vec::new();
        {
            let mut zw = ZipWriter::new(Cursor::new(&mut text_buf));
            let stored = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
            zw.start_file("mimetype", stored).unwrap();
            zw.write_all(b"application/epub+zip").unwrap();
            zw.start_file("content.opf", stored).unwrap();
            zw.write_all(r#"<package version="3.0"><metadata><dc:title>文字书</dc:title></metadata><manifest><item id="c1" href="c1.xhtml" media-type="application/xhtml+xml"/></manifest><spine><itemref idref="c1"/></spine></package>"#.as_bytes()).unwrap();
            zw.start_file("c1.xhtml", stored).unwrap();
            zw.write_all(format!("<html><body><p>{long_text}</p><img src=\"p1.jpg\"/></body></html>").as_bytes()).unwrap();
            zw.start_file("p1.jpg", stored).unwrap();
            zw.write_all(&jpg).unwrap();
            zw.finish().unwrap();
        }

        let (comic_out, _) = optimize_epub(&comic_buf, crate::imgopt::test_screen()).unwrap();
        let (text_out, _) = optimize_epub(&text_buf, crate::imgopt::test_screen()).unwrap();
        // 漫画的 OPF 打上漫画标签（阅读器按它套漫画设置），文字书不打
        let comic_opf = String::from_utf8(entry_bytes(&comic_out, "content.opf")).unwrap();
        assert!(comic_opf.contains("<dc:subject>漫画</dc:subject>"), "{comic_opf}");
        assert!(!String::from_utf8(entry_bytes(&text_out, "content.opf")).unwrap().contains("<dc:subject>"));
        let mut comic_img = Vec::new();
        ZipArchive::new(Cursor::new(&comic_out)).unwrap().by_name("p1.jpg").unwrap().read_to_end(&mut comic_img).unwrap();
        let mut text_img = Vec::new();
        ZipArchive::new(Cursor::new(&text_out)).unwrap().by_name("p1.jpg").unwrap().read_to_end(&mut text_img).unwrap();

        // 2026-09-19 起两边不再要求尺寸完全一致：漫画路径多了 `pad_to_device_aspect` 这一步
        // （真机反馈"底部留白太多"，CSS 治不了；用户明确目标是"上下留白尽量等比、左右留白尽可能
        // 接近 0"，见该函数文档），文字书路径的内嵌图不是整页漫画，不需要补白。这里只保留还站得
        // 住脚的部分：两边都应该被缩进屏幕框内（不超限），漫画路径的高度应该补到刚好等于设备
        // 页面长宽比对应的高度（宽度不变，左右留白全程是 0）。
        let (cw, ch) = image::load_from_memory(&comic_img).unwrap().dimensions();
        let (tw, th) = image::load_from_memory(&text_img).unwrap().dimensions();
        assert!(cw <= crate::imgopt::test_screen().width && ch <= crate::imgopt::test_screen().height, "漫画路径应该缩进屏幕框: {cw}x{ch}");
        assert!(tw <= crate::imgopt::test_screen().width && th <= crate::imgopt::test_screen().height, "文字书内嵌图也应该缩进屏幕框: {tw}x{th}");
        assert_eq!((cw, ch), (crate::imgopt::test_screen().width, crate::imgopt::test_screen().height), "缺省（开关关）漫画整页补白到屏幕比例 954×1696: {cw}x{ch}");
        // 传真实可阅读范围（Move EPUB 842×1455）：漫画页补白到阅读范围，文字书内嵌图只缩不补白
        let area = OptimizeOpts::new(profile::get("xochitl").unwrap().readable(profile::Format::Epub));
        let (a_out, _) = optimize_epub_with(&comic_buf, &area).unwrap();
        let mut a_img = Vec::new();
        ZipArchive::new(Cursor::new(&a_out)).unwrap().by_name("p1.jpg").unwrap().read_to_end(&mut a_img).unwrap();
        assert_eq!(image::load_from_memory(&a_img).unwrap().dimensions(), (842, 1455), "漫画页补白到阅读范围");
        let (t_out, _) = optimize_epub_with(&text_buf, &area).unwrap();
        let mut t_img = Vec::new();
        ZipArchive::new(Cursor::new(&t_out)).unwrap().by_name("p1.jpg").unwrap().read_to_end(&mut t_img).unwrap();
        let (w, h) = image::load_from_memory(&t_img).unwrap().dimensions();
        assert!(w <= 842 && h <= 1455, "文字书内嵌图缩进阅读范围: {w}x{h}");
        assert!(comic_img.len() > text_img.len(), "漫画书判定应触发更高质量重编码，体积应更大: comic={} text={}", comic_img.len(), text_img.len());
    }

    #[test]
    fn streaming_reports_progress_per_entry() {
        let epub = make_crossfile_endnote_epub();
        let t = tempfile::tempdir().unwrap();
        let input_path = t.path().join("in.epub");
        let output_path = t.path().join("out.epub");
        std::fs::write(&input_path, &epub).unwrap();
        let mut progresses = Vec::new();
        let rep = optimize_epub_file_streaming(&input_path, &output_path, &OptimizeOpts::new(crate::imgopt::test_screen()), |done, total| progresses.push((done, total))).unwrap();
        assert_eq!((rep.total_files, rep.html_files), (4, 3), "mimetype + 3 章");
        assert!(!progresses.is_empty(), "阶段二应该至少回调一次进度");
        assert!(progresses.iter().all(|(_, total)| *total == progresses[0].1), "total 全程不变");
        assert_eq!(progresses.last().unwrap().0, progresses[0].1, "最后一次回调 done 应该等于 total（全部写完）");
        assert!(progresses.windows(2).all(|w| w[0].0 < w[1].0), "done 应该严格递增，不重复不倒退");
        assert_eq!(rep.bytes_after as u64, std::fs::metadata(&output_path).unwrap().len());
    }

    #[test]
    fn streaming_comic_image_equals_direct_transform() {
        // 流式版的差别只在"什么时候、从哪读图片字节"、在哪个线程处理，结果应与直接调 transform_image_bytes 逐字节一致。
        use image::{codecs::jpeg::JpegEncoder, DynamicImage, GenericImageView, RgbImage};
        let big = DynamicImage::ImageRgb8(RgbImage::from_fn(2000, 3000, |x, y| image::Rgb([(x % 256) as u8, (y % 256) as u8, 150])));
        let mut jpg = Vec::new();
        JpegEncoder::new_with_quality(&mut jpg, 90).encode_image(&big).unwrap();

        let mut comic_buf = Vec::new();
        {
            let mut zw = ZipWriter::new(Cursor::new(&mut comic_buf));
            let stored = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
            zw.start_file("mimetype", stored).unwrap();
            zw.write_all(b"application/epub+zip").unwrap();
            let items: String = (1..=25).map(|i| format!(r#"<item id="c{i}" href="c{i}.xhtml" media-type="application/xhtml+xml"/>"#)).collect();
            let spine: String = (1..=25).map(|i| format!(r#"<itemref idref="c{i}"/>"#)).collect();
            zw.start_file("content.opf", stored).unwrap();
            zw.write_all(format!(r#"<package version="3.0"><metadata><dc:title>漫画</dc:title></metadata><manifest>{items}</manifest><spine>{spine}</spine></package>"#).as_bytes()).unwrap();
            for i in 1..=25 {
                zw.start_file(format!("c{i}.xhtml"), stored).unwrap();
                zw.write_all(format!(r#"<html><body><img src="p{i}.jpg"/></body></html>"#).as_bytes()).unwrap();
            }
            zw.start_file("p1.jpg", stored).unwrap();
            zw.write_all(&jpg).unwrap();
            zw.finish().unwrap();
        }

        let (stream_out, _) = optimize_epub(&comic_buf, crate::imgopt::test_screen()).unwrap();
        let stream_img = entry_bytes(&stream_out, "p1.jpg");
        let direct = transform_image_bytes(&jpg, true, crate::imgopt::test_screen(), false).unwrap();
        assert_eq!(stream_img, direct, "流式并行处理结果应与直接处理逐字节一致");
        let mut ar = ZipArchive::new(Cursor::new(&stream_out)).unwrap();
        assert_eq!(ar.by_name("p1.jpg").unwrap().compression(), CompressionMethod::Stored, "已压缩的图片 STORED");
        assert!(image::load_from_memory(&stream_img).unwrap().dimensions().0 <= 954, "流式版也该按漫画框约束缩放");
    }

    #[test]
    fn streaming_parallel_many_images_match_sequential_and_keep_order() {
        // 并行（worker + 提前量）不许乱序、不许改任何一张图的处理结果：20 张互不相同的图（尺寸/内容都不同，
        // 顺序错位或串图必然被发现），流式并行版逐张、逐字节对照顺序直接处理的结果；条目顺序也必须与原书一致。
        use image::{DynamicImage, RgbImage};
        let n = 20usize; // ≥20 张才判漫画，走漫画单趟管线
        let mut comic_buf = Vec::new();
        {
            let mut zw = ZipWriter::new(Cursor::new(&mut comic_buf));
            let stored = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
            zw.start_file("mimetype", stored).unwrap();
            zw.write_all(b"application/epub+zip").unwrap();
            let items: String = (1..=n).map(|i| format!(r#"<item id="c{i}" href="c{i}.xhtml" media-type="application/xhtml+xml"/>"#)).collect();
            let spine: String = (1..=n).map(|i| format!(r#"<itemref idref="c{i}"/>"#)).collect();
            zw.start_file("content.opf", stored).unwrap();
            zw.write_all(format!(r#"<package version="3.0"><metadata><dc:title>漫画</dc:title></metadata><manifest>{items}</manifest><spine>{spine}</spine></package>"#).as_bytes()).unwrap();
            for i in 1..=n {
                zw.start_file(format!("c{i}.xhtml"), stored).unwrap();
                zw.write_all(format!(r#"<html><body><img src="p{i}.png"/></body></html>"#).as_bytes()).unwrap();
                let (w, h) = (320 + (i as u32 * 37) % 30, 480 + (i as u32 * 91) % 40); // 短边≥318 才走补白路径；debug 构建下 SIMD 缩放很慢，图要小
                let img = DynamicImage::ImageRgb8(RgbImage::from_fn(w, h, |x, y| image::Rgb([((x + i as u32 * 13) % 256) as u8, ((y * 3) % 256) as u8, (i * 8 % 256) as u8])));
                // PNG：不预放大（无损放大体积暴涨），只补白到设备长宽比——路径真实、debug 构建下也够快。
                let mut jpg = Vec::new();
                img.write_to(&mut Cursor::new(&mut jpg), image::ImageFormat::Png).unwrap();
                zw.start_file(format!("p{i}.png"), stored).unwrap();
                zw.write_all(&jpg).unwrap();
            }
            zw.finish().unwrap();
        }
        let (stream_out, _) = optimize_epub(&comic_buf, crate::imgopt::test_screen()).unwrap();
        let (mut a, mut b) = (ZipArchive::new(Cursor::new(&comic_buf)).unwrap(), ZipArchive::new(Cursor::new(&stream_out)).unwrap());
        for i in 1..=n {
            let name = format!("p{i}.png");
            let (mut x, mut y) = (Vec::new(), Vec::new());
            a.by_name(&name).unwrap().read_to_end(&mut x).unwrap();
            b.by_name(&name).unwrap().read_to_end(&mut y).unwrap();
            let want = transform_image_bytes(&x, true, crate::imgopt::test_screen(), false).unwrap_or(x);
            assert_eq!(want, y, "第 {i} 张图并行结果与顺序结果不一致（乱序或串图）");
        }
        let mut y1 = Vec::new();
        b.by_name("p1.png").unwrap().read_to_end(&mut y1).unwrap();
        let (w1, h1) = image::ImageReader::new(Cursor::new(&y1)).with_guessed_format().unwrap().into_dimensions().unwrap();
        assert!(h1 > 491 && w1 == 327, "应真的补白到设备长宽比（管线确实跑过）: {w1}x{h1}");
        let order = |z: &mut ZipArchive<Cursor<&Vec<u8>>>| -> Vec<String> { (0..z.len()).map(|i| z.by_index(i).unwrap().name().to_string()).filter(|n| n != OPTIMIZE_MARKER).collect() };
        assert_eq!(order(&mut a), order(&mut b), "条目顺序必须与原书一致");
    }

    #[test]
    fn streaming_rejects_missing_input_and_leaves_no_partial_output() {
        let t = tempfile::tempdir().unwrap();
        let output_path = t.path().join("out.epub");
        let err = optimize_epub_file_streaming(&t.path().join("does-not-exist.epub"), &output_path, &OptimizeOpts::new(crate::imgopt::test_screen()), |_, _| {}).unwrap_err();
        assert!(err.contains("打开输入失败"), "{err}");
        assert!(!output_path.exists(), "输入都打不开，不该产生任何输出文件");
    }

    /// 造一个最小 EPUB(mimetype + 一章带锁字体的 xhtml)，过优化器后字体锁应被剥掉、结构保留。
    fn make_epub() -> Vec<u8> {
        let mut buf = Vec::new();
        {
            let mut zw = ZipWriter::new(Cursor::new(&mut buf));
            let stored = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
            zw.start_file("mimetype", stored).unwrap();
            zw.write_all(b"application/epub+zip").unwrap();
            zw.start_file("EPUB/css/x.css", stored).unwrap();
            zw.write_all(b"body{margin:0}").unwrap();
            zw.start_file("EPUB/xhtml/Section01.xhtml", stored).unwrap();
            zw.write_all(
                r#"<html><body><p style="font-size:16px;font-family:'PingFang SC';">正文</p></body></html>"#.as_bytes(),
            )
            .unwrap();
            zw.finish().unwrap();
        }
        buf
    }

    #[test]
    fn strips_font_keeps_structure() {
        let (out, rep) = optimize_epub(&make_epub(), crate::imgopt::test_screen()).unwrap();
        assert_eq!(rep.html_files, 1);
        // 重新解开验证
        let mut ar = ZipArchive::new(Cursor::new(&out)).unwrap();
        // mimetype 首个
        assert_eq!(ar.by_index(0).unwrap().name(), "mimetype");
        // css 原样保留
        let mut css = String::new();
        ar.by_name("EPUB/css/x.css").unwrap().read_to_string(&mut css).unwrap();
        assert_eq!(css, "body{margin:0}");
        // xhtml 字体锁被剥
        let mut x = String::new();
        ar.by_name("EPUB/xhtml/Section01.xhtml").unwrap().read_to_string(&mut x).unwrap();
        assert!(!x.contains("font-family"), "字体锁未剥: {x}");
        assert!(x.contains("<p>正文</p>"), "正文结构被破坏: {x}");
    }

    /// 真机《甲午：摇摆的战争》坐实的真实形态：章节文件**没有扩展名**（`Chapter_2`/`Chapter_7_1`
    /// 这种命名），只按扩展名判断的 `is_html` 会把它整个漏过、字体锁/脚注图标全都没清洗——用户反馈
    /// "字体锁死改不了"。这条端到端跑一遍优化器，断言无扩展名的章节也真的被处理了。
    #[test]
    fn strips_font_lock_on_extensionless_chapter_file() {
        let mut buf = Vec::new();
        {
            let mut zw = ZipWriter::new(Cursor::new(&mut buf));
            let stored = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
            zw.start_file("mimetype", stored).unwrap();
            zw.write_all(b"application/epub+zip").unwrap();
            zw.start_file("EPUB/xhtml/Chapter_2", stored).unwrap();
            zw.write_all(r#"<?xml version="1.0"?><html><body><p style="font-size:16px;font-family:'PingFang SC';">正文</p></body></html>"#.as_bytes()).unwrap();
            zw.finish().unwrap();
        }
        let (out, rep) = optimize_epub(&buf, crate::imgopt::test_screen()).unwrap();
        assert_eq!(rep.html_files, 1, "无扩展名的章节也该被数进 html_files: {:?}", rep);
        let mut ar = ZipArchive::new(Cursor::new(&out)).unwrap();
        let mut x = String::new();
        ar.by_name("EPUB/xhtml/Chapter_2").unwrap().read_to_string(&mut x).unwrap();
        assert!(!x.contains("font-family"), "无扩展名章节的字体锁未剥: {x}");
        assert!(x.contains("<p>正文</p>"), "正文结构被破坏: {x}");
    }

    /// Calibre 转出的 duokan 脚注（《人骨拼图》AZW3→EPUB 真实形态）：同文件 href 带文件名、
    /// 标记是真 `<img>` + `<a id="c_2_1">`、注释块 `<li id="a_2_1">` 内回链 `href="part0004.html#c_2_1"`
    /// 构成真 2-环。优化后：href 归一裸锚、标记换上标且 id 保留、回链去链、注释留在原 li 里不被搬成
    /// 无效嵌套（v4 前两条红线全踩：id 丢=回链悬空、`<p id><p>` 嵌套）。
    #[test]
    fn calibre_washed_duokan_footnote_survives() {
        let mut buf = Vec::new();
        {
            let mut zw = ZipWriter::new(Cursor::new(&mut buf));
            let stored = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
            zw.start_file("mimetype", stored).unwrap();
            zw.write_all(b"application/epub+zip").unwrap();
            zw.start_file("text/part0004.html", stored).unwrap();
            zw.write_all(r##"<html><body><p class="a">退休储蓄基金<sup class="calibre4"><a class="duokan-footnote" href="part0004.html#a_2_1" id="c_2_1"><img alt="注释1" class="duokan-footnote1" src="../images/00003.png"/></a></sup>，她可以</p>
<ol class="duokan-footnote-content">
<li class="duokan-footnote-item" id="a_2_1">
<p class="pfootnotetext"><a class="calibre6" href="part0004.html#c_2_1">美国一项延税储蓄计划。</a></p>
</li>
</ol></body></html>"##.as_bytes()).unwrap();
            zw.finish().unwrap();
        }
        let (out, _) = optimize_epub(&buf, crate::imgopt::test_screen()).unwrap();
        let mut ar = ZipArchive::new(Cursor::new(&out)).unwrap();
        let mut x = String::new();
        ar.by_name("text/part0004.html").unwrap().read_to_string(&mut x).unwrap();
        assert!(!x.contains("part0004.html#"), "同文件 href 未归一裸锚: {x}");
        assert!(x.contains(r##"<a href="#a_2_1" id="c_2_1"><sup>1</sup></a>"##), "标记未换上标/丢 id: {x}");
        assert!(!x.contains("<img"), "标记死图未清: {x}");
        assert!(!x.contains(r##"href="#c_2_1""##), "回链未去链(互指对整对丢弃): {x}");
        assert!(x.contains("延税储蓄计划"), "注释文本丢失: {x}");
        assert!(x.contains(r##"<li class="duokan-footnote-item" id="a_2_1">"##), "注释块应原地留在 li 里: {x}");
        assert!(!x.contains("<div class=\"footnotes\">"), "同章注释不该被当跨文件搬走: {x}");
        assert!(!x.contains("<p id=\"a_2_1\">\n"), "不该产出无效嵌套 <p id><p>: {x}");
    }

    /// 造一本"正文章引用书末尾注文件"的 EPUB（bug 复现形态）：两章各引用 notes.xhtml 里的一条
    /// 普通尾注（<a href="notes.xhtml#nX">，非 noteref；注释为 <p id="nX">）。优化后每章 marker 应变
    /// 同章锚点、注释搬进对应章，且两章共用形态不撞 id。
    fn make_crossfile_endnote_epub() -> Vec<u8> {
        let mut buf = Vec::new();
        {
            let mut zw = ZipWriter::new(Cursor::new(&mut buf));
            let stored = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
            zw.start_file("mimetype", stored).unwrap();
            zw.write_all(b"application/epub+zip").unwrap();
            zw.start_file("ch1.xhtml", stored).unwrap();
            zw.write_all(r#"<html><body><p>第一章正文<a href="notes.xhtml#n1">1</a>结束</p></body></html>"#.as_bytes()).unwrap();
            zw.start_file("ch2.xhtml", stored).unwrap();
            zw.write_all(r#"<html><body><p>第二章正文<a href="notes.xhtml#n2">1</a>结束</p></body></html>"#.as_bytes()).unwrap();
            zw.start_file("notes.xhtml", stored).unwrap();
            zw.write_all(r#"<html><body><p class="footnote" id="n1">第一章的注释</p><p class="footnote" id="n2">第二章的注释</p></body></html>"#.as_bytes()).unwrap();
            zw.finish().unwrap();
        }
        buf
    }

    /// 带真实 content.opf 的最小书：spine 里第一页是一份「目录页」（链到一堆不同章节文件，形状
    /// 跟 Calibre 常见排版一致），后面跟几章正文。
    fn make_epub_with_html_toc_page(n_chapters: usize) -> Vec<u8> {
        let mut buf = Vec::new();
        {
            let mut zw = ZipWriter::new(Cursor::new(&mut buf));
            let stored = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
            zw.start_file("mimetype", stored).unwrap();
            zw.write_all(b"application/epub+zip").unwrap();
            let links: String = (1..=n_chapters).map(|i| format!(r#"<li><a href="c{i}.xhtml">第{i}章</a></li>"#)).collect();
            zw.start_file("OEBPS/toc.html", stored).unwrap();
            zw.write_all(format!(r#"<html><body><h1>目录</h1><ul>{links}</ul></body></html>"#).as_bytes()).unwrap();
            for i in 1..=n_chapters {
                zw.start_file(format!("OEBPS/c{i}.xhtml"), stored).unwrap();
                zw.write_all(format!("<html><body><p>第{i}章正文</p></body></html>").as_bytes()).unwrap();
            }
            let manifest_chapters: String = (1..=n_chapters).map(|i| format!(r#"<item id="c{i}" href="c{i}.xhtml" media-type="application/xhtml+xml"/>"#)).collect();
            let spine_chapters: String = (1..=n_chapters).map(|i| format!(r#"<itemref idref="c{i}"/>"#)).collect();
            zw.start_file("OEBPS/content.opf", stored).unwrap();
            zw.write_all(
                format!(
                    r#"<package version="3.0"><metadata><dc:title>书</dc:title></metadata><manifest><item id="toc-html" href="toc.html" media-type="application/xhtml+xml"/>{manifest_chapters}</manifest><spine><itemref idref="toc-html"/>{spine_chapters}</spine></package>"#
                )
                .as_bytes(),
            )
            .unwrap();
            zw.finish().unwrap();
        }
        buf
    }

    /// 母版库按书指定翻页方向（2026-09-25）：`page_direction=Some` 只改 OPF 的 spine 属性，其余条目与不指定时逐字节相同；
    /// 不指定＝保留原书（原书没写就还是没写）。
    #[test]
    fn page_direction_touches_only_opf_spine() {
        use crate::direction::{spine_direction, PageDirection};
        let epub = make_epub_with_html_toc_page(3);
        let entries = |bytes: &[u8]| {
            let mut ar = ZipArchive::new(Cursor::new(bytes)).unwrap();
            (0..ar.len())
                .map(|i| {
                    let mut f = ar.by_index(i).unwrap();
                    let mut v = Vec::new();
                    f.read_to_end(&mut v).unwrap();
                    (f.name().to_string(), v)
                })
                .collect::<Vec<_>>()
        };
        let base = OptimizeOpts { wash: Some(crate::wash::WashOpts::default()), ..OptimizeOpts::new(crate::imgopt::test_screen()) };
        let rtl = OptimizeOpts { page_direction: Some(PageDirection::Rtl), ..base.clone() };
        let (plain, _) = optimize_epub_with(&epub, &base).unwrap();
        let (flipped, _) = optimize_epub_with(&epub, &rtl).unwrap();
        let (a, b) = (entries(&plain), entries(&flipped));
        assert_eq!(a.len(), b.len());
        for ((na, da), (nb, db)) in a.iter().zip(&b) {
            assert_eq!(na, nb, "条目顺序不变");
            let (sa, sb) = (String::from_utf8_lossy(da), String::from_utf8_lossy(db));
            if na.ends_with(".opf") {
                assert_eq!(spine_direction(&sa), None, "不指定＝保留原书（原书没写）: {sa}");
                assert_eq!(spine_direction(&sb), Some(PageDirection::Rtl), "{sb}");
                assert_eq!(sb.replacen(r#" page-progression-direction="rtl""#, "", 1), sa, "OPF 只多这一个属性");
            } else {
                assert_eq!(da, db, "{na} 不该受方向设置影响");
            }
        }
        let opf_of = |v: &[(String, Vec<u8>)]| v.iter().find(|(n, _)| n.ends_with(".opf")).map(|(_, d)| d.clone()).unwrap();
        // 从左往右：原书的 rtl 被改掉
        let ltr = OptimizeOpts { page_direction: Some(PageDirection::Ltr), ..base.clone() };
        let (back, _) = optimize_epub_with(&flipped, &ltr).unwrap();
        let opf = String::from_utf8(opf_of(&entries(&back))).unwrap();
        assert_eq!(spine_direction(&opf), Some(PageDirection::Ltr), "{opf}");
    }

    #[test]
    fn html_toc_page_kept_in_spine_not_stripped_as_redundant() {
        // 真机回归（2026-09-19，《疯探》）：书自带的 HTML 目录页（链到几十个章节文件）曾被旧逻辑当
        // "跟原生 TOC 冗余"从 spine 删掉，翻页再也看不到目录——违背 EPUB 线原则①"保留目录页"。
        let (out, _) = optimize_epub(&make_epub_with_html_toc_page(20), crate::imgopt::test_screen()).unwrap();
        let mut ar = ZipArchive::new(Cursor::new(&out)).unwrap();
        let mut opf = String::new();
        ar.by_name("OEBPS/content.opf").unwrap().read_to_string(&mut opf).unwrap();
        assert!(opf.contains(r#"idref="toc-html""#), "目录页的 itemref 不该从 spine 被删: {opf}");
    }

    /// 只有 mimetype 与给定文件的最小 EPUB（没有 OPF）。
    fn zip_book(files: &[(&str, &str)]) -> Vec<u8> {
        let mut buf = Vec::new();
        {
            let mut zw = ZipWriter::new(Cursor::new(&mut buf));
            let stored = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
            zw.start_file("mimetype", stored).unwrap();
            zw.write_all(b"application/epub+zip").unwrap();
            for (n, d) in files {
                zw.start_file(*n, stored).unwrap();
                zw.write_all(d.as_bytes()).unwrap();
            }
            zw.finish().unwrap();
        }
        buf
    }

    fn text_of(epub: &[u8], name: &str) -> String {
        String::from_utf8(entry_bytes(epub, name)).unwrap()
    }

    /// M1（2026-09-28 审计）：不清洗时两个注释文件里都有 `id="fn1"`，各章拿到的是自己那一条（此前索引只按 id，一条被另一条覆盖丢了）。
    #[test]
    fn audit_m1_same_note_id_in_two_files_without_wash() {
        let book = zip_book(&[
            ("ch1.xhtml", r#"<html><body><p>甲章<a href="n1.xhtml#fn1">1</a>。</p></body></html>"#),
            ("ch2.xhtml", r#"<html><body><p>乙章<a href="n2.xhtml#fn1">1</a>。</p></body></html>"#),
            ("n1.xhtml", r#"<html><body><p class="footnote" id="fn1">甲章的注释</p></body></html>"#),
            ("n2.xhtml", r#"<html><body><p class="footnote" id="fn1">乙章的注释</p></body></html>"#),
        ]);
        let (out, _) = optimize_epub(&book, crate::imgopt::test_screen()).unwrap();
        let (c1, c2) = (text_of(&out, "ch1.xhtml"), text_of(&out, "ch2.xhtml"));
        assert!(c1.contains("甲章的注释") && !c1.contains("乙章的注释"), "{c1}");
        assert!(c2.contains("乙章的注释") && !c2.contains("甲章的注释"), "{c2}");
    }

    /// M2：目录页（nav）里的链接不算注释引用、也不往目录页里搬注释；收集了却没有哪章会接的注释放回原处；
    /// 属性值里有 `>` 的 marker 也认得。
    #[test]
    fn audit_m2_nav_refs_ignored_and_unclaimed_notes_put_back() {
        let book = zip_book(&[
            ("nav.xhtml", r#"<html><body><nav><ol><li><a href="notes.xhtml#n1">注一</a></li></ol></nav></body></html>"#),
            // duokan 形态的 noteref：换标记后 href 变成本章 `#n2`，第二遍不会再接 n2
            ("ch1.xhtml", r#"<html><body><p>正文<sup><a epub:type="noteref" href="notes.xhtml#n2">&lt;img class="duokan-footnote" alt="注释2"/&gt;</a></sup>，又<a title="a>b" href="notes.xhtml#n3">3</a>。</p></body></html>"#),
            ("notes.xhtml", r#"<html><body><p class="footnote" id="n1">第一条注释</p><p class="footnote" id="n2">第二条注释</p><p class="footnote" id="n3">第三条注释</p></body></html>"#),
        ]);
        let (out, _) = optimize_epub(&book, crate::imgopt::test_screen()).unwrap();
        let (nav, ch1, notes) = (text_of(&out, "nav.xhtml"), text_of(&out, "ch1.xhtml"), text_of(&out, "notes.xhtml"));
        assert!(!nav.contains("第一条注释") && notes.contains("第一条注释"), "只有目录引用的注释留在原处: {nav} {notes}");
        assert!(notes.contains("第二条注释") && !ch1.contains("第二条注释"), "没人接的注释放回原处: {notes}");
        assert!(ch1.contains("第三条注释") && !notes.contains("第三条注释") && ch1.contains(r##"<a href="#n3">3</a>"##), "{ch1}");
        let all = [&nav, &ch1, &notes].iter().map(|t| t.matches("条注释").count()).sum::<usize>();
        assert_eq!(all, 3, "每条注释恰好出现一次");
    }

    #[test]
    fn crossfile_endnotes_relinked_per_chapter() {
        let (out, _) = optimize_epub(&make_crossfile_endnote_epub(), crate::imgopt::test_screen()).unwrap();
        let mut ar = ZipArchive::new(Cursor::new(&out)).unwrap();
        let read = |ar: &mut ZipArchive<Cursor<&Vec<u8>>>, n: &str| {
            let mut s = String::new();
            ar.by_name(n).unwrap().read_to_string(&mut s).unwrap();
            s
        };
        let ch1 = read(&mut ar, "ch1.xhtml");
        assert!(ch1.contains(r##"<a href="#n1">1</a>"##), "ch1 marker 未改同章锚点: {ch1}");
        assert!(ch1.contains("第一章的注释"), "ch1 未搬入其注释: {ch1}");
        assert!(!ch1.contains("notes.xhtml"), "ch1 仍残留跨文件 href: {ch1}");
        let ch2 = read(&mut ar, "ch2.xhtml");
        assert!(ch2.contains("第二章的注释"), "ch2(中间章)未搬入其注释: {ch2}");
        assert!(ch2.contains(r##"<a href="#n2">1</a>"##), "ch2 marker 未改同章锚点: {ch2}");
        // 注释已从 notes.xhtml 移走（不重复渲染）
        let notes = read(&mut ar, "notes.xhtml");
        assert!(!notes.contains("第一章的注释") && !notes.contains("第二章的注释"), "注释未从源文件移除: {notes}");
    }

    #[test]
    fn double_optimize_inline_footnote_no_dup() {
        // 版本升级会重优化已优化过的旧书——重优化不得把已内联的注释再翻倍。
        let opts = OptimizeOpts { wash: Some(crate::wash::WashOpts::default()), footnote: FootnoteMode::Inline, ..OptimizeOpts::new(crate::imgopt::test_screen()) };
        let (out, _) = optimize_epub_with(&make_crossfile_endnote_epub(), &opts).unwrap();
        let (out2, _) = optimize_epub_with(&out, &opts).unwrap();
        let mut ar = ZipArchive::new(Cursor::new(&out2)).unwrap();
        let mut ch1 = String::new();
        ar.by_name("ch1.xhtml").unwrap().read_to_string(&mut ch1).unwrap();
        let n = ch1.matches("第一章的注释").count();
        assert_eq!(n, 1, "重优化后注释重复 {n} 次: {ch1}");
    }

    #[test]
    fn reoptimize_relinked_footnote_no_dup() {
        // 模拟旧版本(v6/v7)产物：注释已移同章末尾 <div class="footnotes"> + marker 已是同章锚点。
        // 版本升级重优化这类书时，不得把注释再翻倍。
        let mut buf = Vec::new();
        {
            let mut zw = ZipWriter::new(Cursor::new(&mut buf));
            let stored = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
            zw.start_file("mimetype", stored).unwrap();
            zw.write_all(b"application/epub+zip").unwrap();
            zw.start_file("ch1.xhtml", stored).unwrap();
            zw.write_all(r##"<html><body><p>正文<a href="#n1">1</a>结束</p><div class="footnotes"><p id="n1">第一章的注释</p></div></body></html>"##.as_bytes()).unwrap();
            zw.finish().unwrap();
        }
        let opts = OptimizeOpts { wash: Some(crate::wash::WashOpts::default()), footnote: FootnoteMode::Inline, ..OptimizeOpts::new(crate::imgopt::test_screen()) };
        let (out, _) = optimize_epub_with(&buf, &opts).unwrap();
        let mut ar = ZipArchive::new(Cursor::new(&out)).unwrap();
        let mut ch1 = String::new();
        ar.by_name("ch1.xhtml").unwrap().read_to_string(&mut ch1).unwrap();
        let n = ch1.matches("第一章的注释").count();
        assert_eq!(n, 1, "重优化旧版脚注结构翻倍 {n} 次: {ch1}");
    }

    #[test]
    fn marks_and_detects_optimized() {
        let raw = make_epub();
        assert!(optimized_version(&raw).is_none(), "原始 EPUB 不该带标记");
        // 默认 optimize_epub 无清洗层 → 只算"核心遍"标记，不能冒充完整优化
        let (out, _) = optimize_epub(&raw, crate::imgopt::test_screen()).unwrap();
        assert_eq!(optimized_version(&out).as_deref(), Some(format!("{OPTIMIZE_VERSION}-core").as_str()), "无 wash 应标 -core");
        assert!(optimized_version(&out).as_deref() != Some(OPTIMIZE_VERSION), "有标记但不算当前完整优化");
        // 带清洗层 → 完整标记
        let (full, _) = optimize_epub_with(&raw, &OptimizeOpts { wash: Some(crate::wash::WashOpts::default()), footnote: FootnoteMode::Anchor, ..OptimizeOpts::new(crate::imgopt::test_screen()) }).unwrap();
        assert_eq!(optimized_version(&full).as_deref(), Some(OPTIMIZE_VERSION), "含 wash 应标完整版本");
        // 重优化幂等：标记只有一条(不残留旧标记)、版本仍正确
        let (out2, _) = optimize_epub(&out, crate::imgopt::test_screen()).unwrap();
        assert_eq!(optimized_version(&out2).as_deref(), Some(format!("{OPTIMIZE_VERSION}-core").as_str()));
        let mut ar = ZipArchive::new(Cursor::new(&out2)).unwrap();
        let marker_count = (0..ar.len())
            .filter(|&i| ar.by_index(i).unwrap().name() == OPTIMIZE_MARKER)
            .count();
        assert_eq!(marker_count, 1, "重优化不应残留重复标记条目");
    }

    /// 源书没有 `mimetype`（或内容不规范）：产物必须补上规范的 `mimetype`，排第一且 STORED，过质量门。
    #[test]
    fn missing_mimetype_is_written_first_and_stored() {
        let mut buf = Vec::new();
        {
            let mut zw = ZipWriter::new(Cursor::new(&mut buf));
            let deflated = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
            zw.start_file("OEBPS/toc.ncx", deflated).unwrap();
            zw.write_all(br#"<ncx><navMap><navPoint><navLabel><text>one</text></navLabel><content src="c1.xhtml"/></navPoint></navMap></ncx>"#).unwrap();
            zw.start_file("OEBPS/c1.xhtml", deflated).unwrap();
            zw.write_all("<html><body><p>正文</p></body></html>".as_bytes()).unwrap();
            zw.finish().unwrap();
        }
        let (out, _) = optimize_epub(&buf, crate::imgopt::test_screen()).unwrap();
        {
            let mut ar = ZipArchive::new(Cursor::new(&out)).unwrap();
            let first = ar.by_index(0).unwrap();
            assert_eq!((first.name(), first.compression()), ("mimetype", CompressionMethod::Stored));
        }
        assert_eq!(entry_bytes(&out, "mimetype"), b"application/epub+zip");
        let rep = crate::check::check_epub(&out, false).unwrap();
        assert!(rep.ok, "{:?}", rep.errors);
        // 源书 mimetype 内容带换行、还被压缩：改写成规范内容、STORED
        let mut buf2 = Vec::new();
        {
            let mut zw = ZipWriter::new(Cursor::new(&mut buf2));
            zw.start_file("mimetype", SimpleFileOptions::default()).unwrap();
            zw.write_all(b"application/epub+zip\n").unwrap();
            zw.start_file("c1.xhtml", SimpleFileOptions::default()).unwrap();
            zw.write_all(b"<html><body><p>x</p></body></html>").unwrap();
            zw.finish().unwrap();
        }
        let (out2, _) = optimize_epub(&buf2, crate::imgopt::test_screen()).unwrap();
        assert_eq!(entry_bytes(&out2, "mimetype"), b"application/epub+zip");
        assert_eq!(ZipArchive::new(Cursor::new(&out2)).unwrap().by_index(0).unwrap().compression(), CompressionMethod::Stored);
    }

    /// 起一个只回一张 PNG 的本地 HTTP 服务（一次连接一次响应，服务 `n` 次后退出），返回端口。
    fn serve_png(png: Vec<u8>, n: usize) -> u16 {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        std::thread::spawn(move || {
            for stream in listener.incoming().take(n) {
                let mut s = stream.unwrap();
                let mut req = [0u8; 4096];
                let _ = s.read(&mut req);
                let head = format!("HTTP/1.1 200 OK\r\nContent-Type: image/png\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", png.len());
                let _ = s.write_all(head.as_bytes());
                let _ = s.write_all(&png);
            }
        });
        port
    }

    /// 远程图端到端：抓到的图写进 zip、src 改本地名、**补进 OPF manifest**（manifest 里没有的资源不算书的一部分）；抓不到的
    /// `<img>` 原样保留。OPF 推迟到最后写，其它条目顺序不变。
    #[test]
    fn remote_images_are_added_to_manifest_and_failed_ones_kept() {
        let mut png = Vec::new();
        image::DynamicImage::ImageRgb8(image::RgbImage::from_pixel(20, 10, image::Rgb([200, 10, 10]))).write_to(&mut Cursor::new(&mut png), image::ImageFormat::Png).unwrap();
        let port = serve_png(png.clone(), 1);
        // 拿一个肯定没人监听的端口：绑定后立刻释放。
        let dead = std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
        let chapter = format!(r#"<html><body><p>正文<img src="http://127.0.0.1:{port}/a.png"/></p><p><img alt="x" src="http://127.0.0.1:{dead}/b.png"/></p></body></html>"#);
        let mut buf = Vec::new();
        {
            let mut zw = ZipWriter::new(Cursor::new(&mut buf));
            let stored = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
            zw.start_file("mimetype", stored).unwrap();
            zw.write_all(b"application/epub+zip").unwrap();
            zw.start_file("META-INF/container.xml", stored).unwrap();
            zw.write_all(br#"<container><rootfiles><rootfile full-path="OEBPS/content.opf"/></rootfiles></container>"#).unwrap();
            zw.start_file("OEBPS/content.opf", stored).unwrap();
            zw.write_all(br#"<package version="3.0"><metadata><dc:title>t</dc:title></metadata><manifest><item id="c1" href="text/c1.xhtml" media-type="application/xhtml+xml"/></manifest><spine><itemref idref="c1"/></spine></package>"#).unwrap();
            zw.start_file("OEBPS/text/c1.xhtml", stored).unwrap();
            zw.write_all(chapter.as_bytes()).unwrap();
            zw.finish().unwrap();
        }
        let (out, _) = optimize_epub(&buf, crate::imgopt::test_screen()).unwrap();
        let ch = String::from_utf8(entry_bytes(&out, "OEBPS/text/c1.xhtml")).unwrap();
        assert!(ch.contains(r#"src="remote_img_0.png""#), "抓到的图改本地名: {ch}");
        assert!(ch.contains(&format!(r#"<img alt="x" src="http://127.0.0.1:{dead}/b.png"/>"#)), "抓不到的原样保留: {ch}");
        assert_eq!(entry_bytes(&out, "OEBPS/text/remote_img_0.png"), png, "小图不缩放，原样写入");
        let opf = String::from_utf8(entry_bytes(&out, "OEBPS/content.opf")).unwrap();
        assert!(opf.contains(r#"<item id="eink-remote-img-0" href="text/remote_img_0.png" media-type="image/png"/></manifest>"#), "{opf}");
        let names: Vec<String> = {
            let mut ar = ZipArchive::new(Cursor::new(&out)).unwrap();
            (0..ar.len()).map(|i| ar.by_index(i).unwrap().name().to_string()).collect()
        };
        assert_eq!(names, ["mimetype", "META-INF/container.xml", "OEBPS/text/c1.xhtml", "OEBPS/text/remote_img_0.png", "OEBPS/content.opf", OPTIMIZE_MARKER]);
        let rep = crate::check::check_epub(&out, false).unwrap();
        assert!(rep.ok, "{:?}", rep.errors);
    }

    #[test]
    fn add_manifest_items_handles_prefix_and_relative_paths() {
        let imgs = vec![("OEBPS/text/remote_img_0.jpg".to_string(), vec![]), ("remote_img_1.gif".to_string(), vec![])];
        let out = add_manifest_items(r#"<opf:package><opf:manifest><opf:item id="a"/></opf:manifest></opf:package>"#, "OEBPS/content.opf", &imgs);
        // 元素名跟着 manifest 的前缀；已有的 eink-remote-img-0（上次优化抓的）不重复用
        assert_eq!(out, r#"<opf:package><opf:manifest><opf:item id="a"/><opf:item id="eink-remote-img-0" href="text/remote_img_0.jpg" media-type="image/jpeg"/><opf:item id="eink-remote-img-1" href="../remote_img_1.gif" media-type="image/gif"/></opf:manifest></opf:package>"#);
        let again = add_manifest_items(r#"<package><manifest><item id="eink-remote-img-0" href="old.png"/></manifest></package>"#, "a.opf", &imgs[..1]);
        assert!(again.contains(r#"<item id="eink-remote-img-1" href="OEBPS/text/remote_img_0.jpg""#), "{again}");
        assert_eq!(add_manifest_items("<package/>", "a.opf", &imgs), "<package/>", "没有 manifest 原样");
        assert!(has_remote_img(r#"<IMG class="x" src="//cdn/a.png">"#) && has_remote_img(r#"<img src="https://a/b.jpg"/>"#));
        assert!(!has_remote_img(r#"<img data-src="https://a/b.jpg" src="b.jpg"/>"#));
    }

    /// 规范整理端到端（v27）：EPUB 2 书过完整优化 → OPF 3.0、有 nav、NCX 与 `spine toc` 保留且 dtb:uid 对上；
    /// manifest 的 `properties` 按最终内容标（SVG 封面换成 img 后不再标 svg，正文里的内嵌 svg 标上）；两次产物逐字节相同。
    #[test]
    fn epub3_upgrade_is_deterministic_and_marks_final_properties() {
        let opf = r#"<?xml version="1.0"?><package xmlns="http://www.idpf.org/2007/opf" version="2.0" unique-identifier="id"><metadata xmlns:dc="http://purl.org/dc/elements/1.1/" xmlns:opf="http://www.idpf.org/2007/opf"><dc:title>书</dc:title><dc:creator opf:role="aut">某</dc:creator><dc:language>zh</dc:language><dc:identifier id="id">urn:uuid:1</dc:identifier></metadata><manifest><item id="cover" href="cover.xhtml" media-type="application/xhtml+xml"/><item id="c1" href="c1.xhtml" media-type="application/xhtml+xml"/><item id="img" href="c.jpg" media-type="image/jpeg"/><item id="ncx" href="toc.ncx" media-type="application/x-dtbncx+xml"/></manifest><spine toc="ncx"><itemref idref="cover"/><itemref idref="c1"/></spine></package>"#;
        let book = zip_book(&[
            ("META-INF/container.xml", r#"<container><rootfiles><rootfile full-path="OEBPS/content.opf"/></rootfiles></container>"#),
            ("OEBPS/content.opf", opf),
            ("OEBPS/cover.xhtml", r#"<html><body><div><svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 6 8"><image width="6" height="8" xlink:href="c.jpg"/></svg></div></body></html>"#),
            ("OEBPS/c1.xhtml", "<html><body><h1>第一章</h1><p>甲&nbsp;乙</p><svg xmlns=\"http://www.w3.org/2000/svg\"><text>图</text></svg></body></html>"),
            ("OEBPS/toc.ncx", r#"<ncx><head><meta name="dtb:uid" content="x"/></head><navMap><navPoint><navLabel><text>第一章</text></navLabel><content src="c1.xhtml"/></navPoint></navMap></ncx>"#),
        ]);
        let opts = OptimizeOpts { wash: Some(crate::wash::WashOpts::default()), ..OptimizeOpts::new(crate::imgopt::test_screen()) };
        let (a, _) = optimize_epub_with(&book, &opts).unwrap();
        let (b, _) = optimize_epub_with(&book, &opts).unwrap();
        assert_eq!(a, b, "产物逐字节确定（dcterms:modified 是固定值）");
        let opf = text_of(&a, "OEBPS/content.opf");
        assert!(opf.contains(r#"version="3.0""#) && opf.contains(&format!(r#"<meta property="dcterms:modified">{}</meta>"#, crate::wash::normalize::EPUB3_MODIFIED)), "{opf}");
        assert!(opf.contains(r#"properties="nav""#) && opf.contains(r#"<spine toc="ncx">"#), "{opf}");
        assert!(opf.contains(r#"<item id="cover" href="cover.xhtml" media-type="application/xhtml+xml"/>"#), "封面 svg 已换成 img，不标 svg: {opf}");
        assert!(opf.contains(r#"<item id="c1" href="c1.xhtml" media-type="application/xhtml+xml" properties="svg"/>"#), "{opf}");
        assert!(opf.contains(r#"properties="cover-image""#), "{opf}");
        assert!(text_of(&a, "OEBPS/toc.ncx").contains(r#"<meta name="dtb:uid" content="urn:uuid:1"/>"#));
        assert!(text_of(&a, "OEBPS/nav.xhtml").contains(r#"<a href="c1.xhtml">第一章</a>"#));
        let c1 = text_of(&a, "OEBPS/c1.xhtml");
        assert!(c1.contains(r#"xmlns:epub="http://www.idpf.org/2007/ops""#) && c1.contains("甲&#160;乙"), "{c1}");
    }
