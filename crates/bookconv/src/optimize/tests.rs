//! 优化器单测。全部走生产路径 [`optimize_epub_file_streaming`]（经临时文件），不另设内存版实现。
    use super::*;
    use std::io::{Cursor, Read, Write};
    use zip::ZipWriter;
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

    /// 产物里的幂等标记内容（走正式入口 [`optimized_version_file`]）；没有标记 / 不是 zip → `None`。
    fn optimized_version(epub: &[u8]) -> Option<String> {
        let t = tempfile::tempdir().unwrap();
        let p = t.path().join("b.epub");
        std::fs::write(&p, epub).unwrap();
        optimized_version_file(&p)
    }

    fn entry_bytes(epub: &[u8], name: &str) -> Vec<u8> {
        let mut v = Vec::new();
        ZipArchive::new(Cursor::new(epub)).unwrap().by_name(name).unwrap().read_to_end(&mut v).unwrap();
        v
    }

    #[test]
    fn inline_remote_images_fetches_and_keeps_local_and_drops_failed() {
        // 本地图不动；远程抓到→内联改本地名+进资源；远程抓不到→删掉这个 <img>（设备不联网，留着是断图）
        let html = r#"<p><img src="local.png"/><img class="c" src="https://x.com/a.png"/><img src="//y.com/b.png"/>字<img src="http://z/d.png"></img><img alt='x>y' src='https://x.com/c.png?a=1&amp;b=2'/></p>"#;
        let mut n = 0usize;
        // 已经优化过的书再跑：书里已有 remote_img_0.png，新抓的图不能重名
        let mut taken: HashSet<String> = ["OEBPS/remote_img_0.png".to_string()].into_iter().collect();
        let (out, res) = inline_remote_images(html, "OEBPS", &mut n, &mut taken, true, |src: &str| {
            if src.contains("a.png") || src == "https://x.com/c.png?a=1&b=2" { Some((vec![1, 2, 3], "png")) } else { None } // b 抓不到
        });
        assert!(out.contains(r#"src="local.png""#), "本地图应原样: {out}");
        assert!(out.contains(r#"class="c" src="remote_img_1.png""#), "远程抓到应改本地名、避开已有的名字: {out}");
        assert!(!out.contains("x.com"), "抓到的远程 URL 应换成本地名: {out}");
        assert!(!out.contains("y.com") && !out.contains("z/d.png") && !out.contains("</img>"), "抓不到的远程 img 删掉（连闭合标签）: {out}");
        assert!(out.contains(r#"remote_img_1.png"/>字<img alt="#), "旁边的字不动: {out}");
        assert!(out.contains(r#"<img alt='x>y' src='remote_img_2.png'/>"#), "单引号、属性值里有 > 也认，字符引用先还原再抓: {out}");
        assert_eq!(res.iter().map(|r| r.0.as_str()).collect::<Vec<_>>(), ["OEBPS/remote_img_1.png", "OEBPS/remote_img_2.png"], "资源落本章目录");
        assert_eq!(res[0].1, vec![1, 2, 3]);
        assert!(has_remote_img(r#"<img alt='a>b' src='https://a/b.jpg'/>"#));
    }

    /// 一章的远程图同时抓（同一个地址只抓一次），起名、改写照出现顺序，和逐张抓一样（2026-10-09）。
    #[test]
    fn remote_images_fetched_once_each_in_parallel_named_in_order() {
        let html: String = (0..6).map(|i| format!(r#"<img src="https://x.com/{}.png"/>"#, i % 3)).collect();
        let calls = std::sync::atomic::AtomicUsize::new(0);
        let (mut n, mut taken) = (0usize, HashSet::new());
        let (out, res) = inline_remote_images(&html, "", &mut n, &mut taken, true, |src: &str| {
            calls.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            std::thread::sleep(std::time::Duration::from_millis(50));
            Some((src.as_bytes().to_vec(), "png"))
        });
        assert_eq!(calls.load(std::sync::atomic::Ordering::Relaxed), 3, "三个不同地址各抓一次");
        let names: Vec<&str> = res.iter().map(|r| r.0.as_str()).collect();
        assert_eq!(names, ["remote_img_0.png", "remote_img_1.png", "remote_img_2.png", "remote_img_3.png", "remote_img_4.png", "remote_img_5.png"]);
        assert_eq!(res[3].1, b"https://x.com/0.png", "第四张和第一张同一地址、内容相同");
        assert!(out.starts_with(r#"<img src="remote_img_0.png"/><img src="remote_img_1.png"/>"#), "{out}");
    }

    #[test]
    fn noteicon_rule_dropped_only_when_icons_become_numbers() {
        let mut epub = Vec::new();
        {
            let mut zw = ZipWriter::new(Cursor::new(&mut epub));
            let stored = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
            for (name, body) in [
                ("mimetype", "application/epub+zip"),
                ("META-INF/container.xml", r#"<container><rootfiles><rootfile full-path="OEBPS/content.opf"/></rootfiles></container>"#),
                ("OEBPS/content.opf", r#"<package version="3.0"><metadata><dc:title>t</dc:title></metadata><manifest><item id="c1" href="text/c1.xhtml" media-type="application/xhtml+xml"/></manifest><spine><itemref idref="c1"/></spine></package>"#),
                ("OEBPS/text/c1.xhtml", r#"<html><head><title>t</title></head><body><p>正文</p></body></html>"#),
            ] {
                zw.start_file(name, stored).unwrap();
                zw.write_all(body.as_bytes()).unwrap();
            }
            zw.finish().unwrap();
        }
        let css_of = |number: bool| {
            let opts = OptimizeOpts { wash: Some(Default::default()), number_note_icons: number, ..OptimizeOpts::new(crate::imgopt::test_screen()) };
            let (out, _) = optimize_epub_with(&epub, &opts).unwrap();
            let name = crate::epubzip::read_entries(&out).unwrap().into_iter().find(|e| crate::wash::is_wash_css_name(&e.name)).unwrap();
            String::from_utf8(name.data).unwrap()
        };
        assert!(css_of(false).contains(".eink-noteicon{"), "保留图标的模式要这条");
        let css = css_of(true);
        assert!(!css.contains("noteicon") && css.contains(".eink-note{"), "换成数字的模式去掉，别的规则不动: {css}");
    }

    /// 只改标签上的 `preserveAspectRatio="none"`，正文里写着这串字的不动（此前对全文做字符串替换）。
    #[test]
    fn fix_cover_aspect_only_touches_the_attribute() {
        let page = r#"<svg preserveaspectratio="none" viewBox="0 0 1 1"><image PreserveAspectRatio='none'/></svg><p>属性写法 preserveAspectRatio="none" 会拉伸</p><svg preserveAspectRatio="xMinYMin"/>"#;
        assert_eq!(
            fix_cover_aspect(page),
            r#"<svg preserveAspectRatio="xMidYMid meet" viewBox="0 0 1 1"><image preserveAspectRatio='xMidYMid meet'/></svg><p>属性写法 preserveAspectRatio="none" 会拉伸</p><svg preserveAspectRatio="xMinYMin"/>"#
        );
        let text_only = r#"<p>preserveAspectRatio="none"</p>"#;
        assert_eq!(fix_cover_aspect(text_only), text_only);
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
        // ② 颜色、字重原样保留（2026-09-29 起不再把灰字改黑、细字重提到 400）
        let mut css = String::new();
        ar.by_name("OEBPS/style.css").unwrap().read_to_string(&mut css).unwrap();
        assert!(!css.contains("#000000"), "css 颜色不改: {css}");
        let mut x = String::new();
        ar.by_name("OEBPS/c1.xhtml").unwrap().read_to_string(&mut x).unwrap();
        assert!(!x.contains("#000000") && !x.contains("font-weight:400"), "内联颜色、字重不改: {x}");
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

    /// 漫画里的静态 GIF 页转成 PNG（条目名不变），OPF manifest 的 media-type 跟着改；动图、文字书里的 GIF 原样，media-type 不动。
    #[test]
    fn comic_gif_pages_become_png_and_manifest_media_type_follows() {
        let gif = |frames: Vec<image::RgbaImage>| {
            let mut buf = Vec::new();
            image::codecs::gif::GifEncoder::new(&mut buf).encode_frames(frames.into_iter().map(image::Frame::new)).unwrap();
            buf
        };
        let page = |i: u32| image::RgbaImage::from_fn(200, 280, move |x, y| if (x / 10 + y / 10 + i).is_multiple_of(2) { image::Rgba([0, 0, 0, 255]) } else { image::Rgba([230, 230, 230, 255]) });
        let n = 20u32;
        let mut buf = Vec::new();
        {
            let mut zw = ZipWriter::new(Cursor::new(&mut buf));
            let stored = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
            zw.start_file("mimetype", stored).unwrap();
            zw.write_all(b"application/epub+zip").unwrap();
            let items: String = (1..=n).map(|i| format!(r#"<item id="c{i}" href="t/c{i}.xhtml" media-type="application/xhtml+xml"/><item id="p{i}" href="i/p%20{i}.gif" media-type="image/gif"/>"#)).collect();
            let spine: String = (1..=n).map(|i| format!(r#"<itemref idref="c{i}"/>"#)).collect();
            zw.start_file("O/content.opf", stored).unwrap();
            zw.write_all(format!(r#"<package version="3.0"><metadata><dc:title>漫画</dc:title></metadata><manifest>{items}</manifest><spine>{spine}</spine></package>"#).as_bytes()).unwrap();
            for i in 1..=n {
                zw.start_file(format!("O/t/c{i}.xhtml"), stored).unwrap();
                zw.write_all(format!(r#"<html><body><img src="../i/p%20{i}.gif"/></body></html>"#).as_bytes()).unwrap();
                // 第 1 张是两帧动图：原样保留
                let frames = if i == 1 { vec![page(0), page(1)] } else { vec![page(i)] };
                zw.start_file(format!("O/i/p {i}.gif"), stored).unwrap();
                zw.write_all(&gif(frames)).unwrap();
            }
            zw.finish().unwrap();
        }
        let (out, _) = optimize_epub(&buf, crate::imgopt::Screen { width: 300, height: 400 }).unwrap();
        let opf = String::from_utf8(entry_bytes(&out, "O/content.opf")).unwrap();
        assert!(opf.contains(r#"href="i/p%201.gif" media-type="image/gif"/>"#), "动图不动: {opf}");
        for i in 2..=n {
            assert!(opf.contains(&format!(r#"<item id="p{i}" href="i/p%20{i}.gif" media-type="image/png"/>"#)), "第 {i} 张 media-type 改成 PNG: {opf}");
            let img = entry_bytes(&out, &format!("O/i/p {i}.gif"));
            assert_eq!(image::guess_format(&img).unwrap(), image::ImageFormat::Png);
            assert_eq!({ let d = image::load_from_memory(&img).unwrap(); (d.width(), d.height()) }, (212, 282));
        }
        assert_eq!(image::guess_format(&entry_bytes(&out, "O/i/p 1.gif")).unwrap(), image::ImageFormat::Gif);
        assert!(opf.contains("<dc:subject>漫画</dc:subject>"));
    }

    #[test]
    fn set_manifest_media_types_matches_resolved_href_only() {
        let opf = r#"<package><manifest><item id="a" href="img/a%20b.gif" media-type="image/gif"/><item media-type='image/webp' href="../x/c.webp" id="c"/><item id="d" href="img/d.gif"/><item id="e" href="img/e.gif" media-type="image/gif"/></manifest></package>"#;
        let got = set_manifest_media_types(opf, "OEBPS/content.opf", &[("OEBPS/img/a b.gif".into(), "image/png"), ("x/c.webp".into(), "image/jpeg"), ("OEBPS/img/d.gif".into(), "image/png")]);
        assert_eq!(
            got,
            r#"<package><manifest><item id="a" href="img/a%20b.gif" media-type="image/png"/><item media-type='image/jpeg' href="../x/c.webp" id="c"/><item id="d" href="img/d.gif"/><item id="e" href="img/e.gif" media-type="image/gif"/></manifest></package>"#
        );
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
        let direct = transform_image_bytes(&jpg, true, crate::imgopt::test_screen(), 1, false, None, false, Limits::default().max_decode_pixels).unwrap();
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
            let want = transform_image_bytes(&x, true, crate::imgopt::test_screen(), 1, false, None, false, Limits::default().max_decode_pixels).unwrap_or(x);
            assert_eq!(want, y, "第 {i} 张图并行结果与顺序结果不一致（乱序或串图）");
        }
        let mut y1 = Vec::new();
        b.by_name("p1.png").unwrap().read_to_end(&mut y1).unwrap();
        let (w1, h1) = image::ImageReader::new(Cursor::new(&y1)).with_guessed_format().unwrap().into_dimensions().unwrap();
        assert!(h1 > 491 && w1 == 329, "应真的补白到设备长宽比（管线确实跑过；PNG 不缩放，宽 327 + 两侧各 1px 白边）: {w1}x{h1}");
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
        assert!(x.contains(r##"<sup><a href="#a_2_1" id="c_2_1"><img alt="注释1" class="duokan-footnote1 eink-noteicon" src="../images/00003.png"/></a></sup>"##), "图标标号应原样保留（加限高的类）、id 不丢: {x}");
        assert!(!x.contains(r##"href="#c_2_1""##), "回链未去链(互指对整对丢弃): {x}");
        assert!(x.contains("延税储蓄计划"), "注释文本丢失: {x}");
        assert!(x.contains(r##"<li class="duokan-footnote-item" id="a_2_1">"##), "注释块应原地留在 li 里: {x}");
        assert!(!x.contains("<div class=\"footnotes\">"), "同章注释不该被当跨文件搬走: {x}");
        assert!(!x.contains("<p id=\"a_2_1\">\n"), "不该产出无效嵌套 <p id><p>: {x}");
    }

    fn one_entry_epub(files: &[(&str, &str)]) -> Vec<u8> {
        let mut buf = Vec::new();
        {
            let mut zw = ZipWriter::new(Cursor::new(&mut buf));
            let stored = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
            zw.start_file("mimetype", stored).unwrap();
            zw.write_all(b"application/epub+zip").unwrap();
            for (n, body) in files {
                zw.start_file(*n, stored).unwrap();
                zw.write_all(body.as_bytes()).unwrap();
            }
            zw.finish().unwrap();
        }
        buf
    }

    /// 注释识别统一成"互相链接的一对"（2026-10-05）：注释段落没有 note 语义（《罗杰疑案》`class="fncontent"`、锚点在段首的
    /// `<a id>`）、和正文同一个文件。弹窗模式搬到章末写成弹窗；跳转模式同文件的留在原处。
    #[test]
    fn same_file_backlink_pair_becomes_popup_only_in_popup_mode() {
        let ch = r##"<html><body><p>对付希巴女王<a id="zw1" href="#zhu1"><sup>[1]</sup></a>那样</p><p>别的段落<a href="#x">交叉引用</a></p><p id="x">目标段落</p><p class="fncontent"><a id="zhu1" href="#zw1">[1]</a>Queen of Sheba。</p></body></html>"##;
        let raw = one_entry_epub(&[("c.xhtml", ch)]);
        let base = OptimizeOpts { wash: None, ..OptimizeOpts::new(crate::imgopt::test_screen()) };
        let (pop, _) = optimize_epub_with(&raw, &OptimizeOpts { footnote: FootnoteMode::Popup, ..base.clone() }).unwrap();
        let x = String::from_utf8(entry_bytes(&pop, "c.xhtml")).unwrap();
        assert_eq!(x.matches(r#"epub:type="noteref""#).count(), 1, "{x}");
        assert!(x.contains(r#"<aside epub:type="footnote" id="zhu1""#) && x.contains("Queen of Sheba"), "{x}");
        assert!(x.contains(r#"<p id="x">目标段落</p>"#), "交叉引用的目标不是注释：{x}");
        let (jump, _) = optimize_epub_with(&raw, &OptimizeOpts { footnote: FootnoteMode::Anchor, ..base }).unwrap();
        let y = String::from_utf8(entry_bytes(&jump, "c.xhtml")).unwrap();
        // 测试缺省选项去掉回链（回链换成 `<span>`，同 xochitl），注释段落本身原地不动
        assert!(!y.contains("<aside") && y.contains(r#"<p class="fncontent">"#) && y.contains("Queen of Sheba。</p></body>"), "跳转模式同文件注释原地不动：{y}");
    }

    /// 跨文件、没有 note 语义、靠回链配对的注释：跳转模式也搬进引用它的那一章。
    #[test]
    fn crossfile_backlink_pair_without_semantic_is_moved() {
        let raw = one_entry_epub(&[
            ("ch1.xhtml", r#"<html><body><p>正文<a id="r1" href="notes.xhtml#n1"><sup>1</sup></a>结束</p></body></html>"#),
            ("notes.xhtml", r#"<html><body><p class="x" id="n1"><a href="ch1.xhtml#r1">1</a>注释内容</p></body></html>"#),
        ]);
        let (out, _) = optimize_epub_with(&raw, &OptimizeOpts { wash: None, footnote: FootnoteMode::Anchor, ..OptimizeOpts::new(crate::imgopt::test_screen()) }).unwrap();
        let ch1 = String::from_utf8(entry_bytes(&out, "ch1.xhtml")).unwrap();
        assert!(ch1.contains("注释内容") && ch1.contains(r##"href="#n1""##), "{ch1}");
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
        zip_book_bytes(&files.iter().map(|&(n, d)| (n, d.as_bytes())).collect::<Vec<_>>())
    }

    fn zip_book_bytes(files: &[(&str, &[u8])]) -> Vec<u8> {
        let mut buf = Vec::new();
        {
            let mut zw = ZipWriter::new(Cursor::new(&mut buf));
            let stored = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
            zw.start_file("mimetype", stored).unwrap();
            zw.write_all(b"application/epub+zip").unwrap();
            for (n, d) in files {
                zw.start_file(*n, stored).unwrap();
                zw.write_all(d).unwrap();
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
        assert!(ch1.contains("第三条注释") && !notes.contains("第三条注释") && ch1.contains(r##"<a title="a>b" href="#n3">3</a>"##), "标号只改 href、别的属性留着: {ch1}");
        let all = [&nav, &ch1, &notes].iter().map(|t| t.matches("条注释").count()).sum::<usize>();
        assert_eq!(all, 3, "每条注释恰好出现一次");
    }

    /// 2026-09-30 审计：两章都引用同一条注释时，此前每章章末各放一份（注释文字重复）。现在不止一章引用的注释留在原处、
    /// 链接照原书是跨文件的；只有一章引用的照常搬进那一章。
    #[test]
    fn note_referenced_from_two_chapters_stays_in_place() {
        let book = zip_book(&[
            ("a.xhtml", r#"<html><body><p>甲章<a href="n.xhtml#fn1">1</a>，又<a href="n.xhtml#fn2">2</a>。</p></body></html>"#),
            ("b.xhtml", r#"<html><body><p>乙章<a href="n.xhtml#fn1">1</a>。</p></body></html>"#),
            ("n.xhtml", r#"<html><body><p class="footnote" id="fn1">共用的注释</p><p class="footnote" id="fn2">甲章自己的注释</p></body></html>"#),
        ]);
        let (out, _) = optimize_epub(&book, crate::imgopt::test_screen()).unwrap();
        let (a, b, n) = (text_of(&out, "a.xhtml"), text_of(&out, "b.xhtml"), text_of(&out, "n.xhtml"));
        let all = [&a, &b, &n].iter().map(|t| t.matches("共用的注释").count()).sum::<usize>();
        assert_eq!(all, 1, "注释文字只能出现一次: {a}\n{b}\n{n}");
        assert!(n.contains(r#"id="fn1""#) && a.contains(r##"href="n.xhtml#fn1""##) && b.contains(r##"href="n.xhtml#fn1""##), "留在原处、链接不动: {a}\n{b}\n{n}");
        assert!(a.contains("甲章自己的注释") && a.contains(r##"<a href="#fn2">2</a>"##) && !n.contains("甲章自己的注释"), "只有一章引用的照常搬: {a}\n{n}");
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
        let opts = OptimizeOpts { wash: Some(crate::wash::WashOpts::default()), footnote: FootnoteMode::Anchor, ..OptimizeOpts::new(crate::imgopt::test_screen()) };
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
        let opts = OptimizeOpts { wash: Some(crate::wash::WashOpts::default()), footnote: FootnoteMode::Anchor, ..OptimizeOpts::new(crate::imgopt::test_screen()) };
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
        assert_eq!(optimized_version(&out).as_deref(), Some("core"), "无 wash 应标 core");
        // 带清洗层 → 完整标记
        let (full, _) = optimize_epub_with(&raw, &OptimizeOpts { wash: Some(crate::wash::WashOpts::default()), footnote: FootnoteMode::Anchor, ..OptimizeOpts::new(crate::imgopt::test_screen()) }).unwrap();
        assert_eq!(optimized_version(&full).as_deref(), Some("full"), "含 wash 应标 full（不写版本号）");
        // 重优化幂等：标记只有一条(不残留旧标记)、版本仍正确
        let (out2, _) = optimize_epub(&out, crate::imgopt::test_screen()).unwrap();
        assert_eq!(optimized_version(&out2).as_deref(), Some("core"));
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
    fn remote_images_are_added_to_manifest_and_failed_ones_dropped() {
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
        assert!(!ch.contains(&format!("127.0.0.1:{dead}")) && ch.contains("<p></p>"), "抓不到的删掉: {ch}");
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

    /// 带整页背景图的书：`body.c` cover、`body.n` 没写尺寸（带透明的 PNG）、`body.s` cover 但同一张图还被 `<img>` 用。
    fn bg_book() -> (Vec<u8>, Vec<(&'static str, Vec<u8>)>) {
        let png = |w: u32, h: u32, alpha: bool| {
            let img = if alpha {
                image::DynamicImage::ImageRgba8(image::RgbaImage::from_fn(w, h, |x, _| if x < w / 2 { image::Rgba([0, 0, 0, 0]) } else { image::Rgba([200, 30, 30, 255]) }))
            } else {
                image::DynamicImage::ImageRgb8(image::RgbImage::from_fn(w, h, |x, y| image::Rgb([(x % 256) as u8, (y % 256) as u8, 90])))
            };
            let mut b = Vec::new();
            img.write_to(&mut Cursor::new(&mut b), image::ImageFormat::Png).unwrap();
            b
        };
        let imgs = vec![("OEBPS/i/a.png", png(1200, 1600, false)), ("OEBPS/i/b.png", png(1080, 1560, true)), ("OEBPS/i/d.png", png(1500, 1600, false))];
        let mut epub = Vec::new();
        {
            let mut zw = ZipWriter::new(Cursor::new(&mut epub));
            let stored = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
            let files: Vec<(&str, &[u8])> = vec![
                ("mimetype", b"application/epub+zip"),
                ("META-INF/container.xml", br#"<container><rootfiles><rootfile full-path="OEBPS/content.opf"/></rootfiles></container>"#),
                ("OEBPS/content.opf", br#"<package version="3.0"><metadata><dc:title>t</dc:title></metadata><manifest><item id="c1" href="c1.xhtml" media-type="application/xhtml+xml"/><item id="s" href="s.css" media-type="text/css"/><item id="a" href="i/a.png" media-type="image/png"/><item id="b" href="i/b.png" media-type="image/png"/><item id="d" href="i/d.png" media-type="image/png"/></manifest><spine><itemref idref="c1"/></spine></package>"#),
                ("OEBPS/s.css", b"body.c{background:url(i/a.png) bottom / cover no-repeat fixed #700} body.n{background:url(\"i/b.png\") no-repeat fixed #111} body.s{background:url(i/d.png) / cover no-repeat}"),
                ("OEBPS/c1.xhtml", r#"<html><head><title>t</title><link rel="stylesheet" href="s.css"/></head><body class="c"><p>正文</p><img src="i/d.png"/></body></html>"#.as_bytes()),
            ];
            for (name, body) in files.into_iter().chain(imgs.iter().map(|(n, b)| (*n, b.as_slice()))) {
                zw.start_file(name, stored).unwrap();
                zw.write_all(body).unwrap();
            }
            zw.finish().unwrap();
        }
        (epub, imgs)
    }

    #[test]
    fn page_backgrounds_prescaled_by_intent_when_sizes_are_dropped() {
        let (epub, imgs) = bg_book();
        let screen = crate::imgopt::Screen { width: 600, height: 800 };
        let opts = OptimizeOpts { screen, text_repair_only: false, ..OptimizeOpts::for_profile(profile::get("ireader").unwrap()) };
        assert!(opts.fit_backgrounds, "ireader 去掉 background-size，要预先缩");
        let (out, _) = optimize_epub_with(&epub, &opts).unwrap();
        let img = |n: &str| image::load_from_memory(&entry_bytes(&out, n)).unwrap();
        // cover：宽高两个比例取大者（600/1200、800/1600 都是 0.5）
        assert_eq!((img("OEBPS/i/a.png").width(), img("OEBPS/i/a.png").height()), (600, 800));
        // 没写尺寸：缩进阅读范围（800/1560 小于 600/1080）；透明保持透明，不铺白底
        let b = img("OEBPS/i/b.png").to_rgba8();
        assert_eq!(b.dimensions(), (554, 800));
        assert_eq!((b.get_pixel(10, 10)[3], b.get_pixel(500, 10)[3]), (0, 255));
        // 透明处是黑色：按预乘 alpha 缩，交界处不透明的像素不被染黑（`image` 自带的缩放不预乘，会出黑边）
        let edge = (277..290).find(|&x| b.get_pixel(x, 10)[3] > 128).unwrap();
        assert!(b.get_pixel(edge, 10)[0] > 180, "交界处 {:?}", b.get_pixel(edge, 10));
        // 同一张图也被 <img> 用：不按 cover 缩，照普通插图处理
        let d = &imgs[2].1;
        assert_eq!(entry_bytes(&out, "OEBPS/i/d.png"), crate::imgopt::downscale_for_epub(d, screen).unwrap_or(d.clone()));
        assert!(!String::from_utf8(entry_bytes(&out, "OEBPS/s.css")).unwrap().contains("background-size"));
    }

    #[test]
    fn page_backgrounds_untouched_when_sizes_are_kept() {
        // kindle 保留 background-size：背景图照普通插图处理（和以前一样）
        let (epub, imgs) = bg_book();
        let screen = crate::imgopt::Screen { width: 600, height: 800 };
        let opts = OptimizeOpts { screen, text_repair_only: false, ..OptimizeOpts::for_profile(profile::get("kindle").unwrap()) };
        assert!(!opts.fit_backgrounds);
        let (out, _) = optimize_epub_with(&epub, &opts).unwrap();
        for (n, b) in &imgs {
            assert_eq!(&entry_bytes(&out, n), &crate::imgopt::downscale_for_epub(b, screen).unwrap_or(b.clone()), "{n}");
        }
    }


    /// 透明图书：`logo.png`（左半透明、透明处存黑色）只当 `<img>`；`bg.png`（同样带透明）只当 CSS 背景；`both.png` 两样都用；
    /// `opaque.png` 是 RGBA 但全不透明；`tall.jpg` 900×1650（放得进掌阅阅读范围、原尺寸显示又超出 0.8 屏高）带多看图注。
    fn alpha_book() -> (Vec<u8>, Vec<(&'static str, Vec<u8>)>) {
        let png = |alpha: u8| {
            let img = image::DynamicImage::ImageRgba8(image::RgbaImage::from_fn(40, 20, |x, _| if x < 20 { image::Rgba([0, 0, 0, alpha]) } else { image::Rgba([200, 30, 30, 255]) }));
            let mut b = Vec::new();
            img.write_to(&mut Cursor::new(&mut b), image::ImageFormat::Png).unwrap();
            b
        };
        let jpg = {
            let img = image::DynamicImage::ImageRgb8(image::RgbImage::from_fn(900, 1650, |x, y| image::Rgb([(x % 256) as u8, (y % 256) as u8, 90])));
            let mut b = Vec::new();
            img.write_to(&mut Cursor::new(&mut b), image::ImageFormat::Jpeg).unwrap();
            b
        };
        let imgs = vec![("OEBPS/i/logo.png", png(0)), ("OEBPS/i/bg.png", png(0)), ("OEBPS/i/both.png", png(0)), ("OEBPS/i/opaque.png", png(255)), ("OEBPS/i/tall.jpg", jpg)];
        let mut epub = Vec::new();
        {
            let mut zw = ZipWriter::new(Cursor::new(&mut epub));
            let stored = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
            let files: Vec<(&str, &[u8])> = vec![
                ("mimetype", b"application/epub+zip"),
                ("META-INF/container.xml", br#"<container><rootfiles><rootfile full-path="OEBPS/content.opf"/></rootfiles></container>"#),
                ("OEBPS/content.opf", br#"<package version="3.0"><metadata><dc:title>t</dc:title></metadata><manifest><item id="c1" href="c1.xhtml" media-type="application/xhtml+xml"/><item id="s" href="s.css" media-type="text/css"/><item id="a" href="i/logo.png" media-type="image/png"/><item id="b" href="i/bg.png" media-type="image/png"/><item id="c" href="i/both.png" media-type="image/png"/><item id="d" href="i/opaque.png" media-type="image/png"/><item id="e" href="i/tall.jpg" media-type="image/jpeg"/></manifest><spine><itemref idref="c1"/></spine></package>"#),
                ("OEBPS/s.css", b"body.x{background:url(i/bg.png) no-repeat #700} div.y{background-image:url('i/both.png')}"),
                ("OEBPS/c1.xhtml", r#"<html><head><title>t</title><link rel="stylesheet" href="s.css"/></head><body class="x"><div class="logo"><img class="logo" src="i/logo.png"/></div><p>正文</p><img src="i/both.png"/><img src="i/opaque.png"/><div class="duokan-image-gallery"><div class="duokan-image-gallery-cell"><img src="i/tall.jpg" alt=""/><p class="duokan-image-maintitle">沧州赵玖</p></div></div></body></html>"#.as_bytes()),
            ];
            for (name, body) in files.into_iter().chain(imgs.iter().map(|(n, b)| (*n, b.as_slice()))) {
                zw.start_file(name, stored).unwrap();
                zw.write_all(body).unwrap();
            }
            zw.finish().unwrap();
        }
        (epub, imgs)
    }

    #[test]
    fn kindle_flattens_transparent_img_on_white_only() {
        let (epub, imgs) = alpha_book();
        let opts = OptimizeOpts { text_repair_only: false, ..OptimizeOpts::for_profile(profile::get("kindle").unwrap()) };
        assert!(opts.flatten_alpha);
        let (out, _) = optimize_epub_with(&epub, &opts).unwrap();
        let logo = entry_bytes(&out, "OEBPS/i/logo.png");
        assert_eq!(crate::util::image_kind(&logo).unwrap().format, image::ImageFormat::Png, "仍是 PNG");
        let img = image::load_from_memory(&logo).unwrap();
        assert!(!img.color().has_alpha(), "没有透明通道了");
        let rgb = img.to_rgb8();
        assert_eq!((rgb.get_pixel(5, 5).0, rgb.get_pixel(30, 5).0), ([255, 255, 255], [200, 30, 30]), "透明处是白的，不透明处不变");
        let orig = |n: &str| imgs.iter().find(|(m, _)| *m == n).unwrap().1.clone();
        for n in ["OEBPS/i/bg.png", "OEBPS/i/both.png", "OEBPS/i/opaque.png"] {
            assert_eq!(entry_bytes(&out, n), orig(n), "{n}：背景图、两用的、全不透明的都不动");
        }
    }

    #[test]
    fn ireader_keeps_transparency() {
        let (epub, imgs) = alpha_book();
        let opts = OptimizeOpts { text_repair_only: false, ..OptimizeOpts::for_profile(profile::get("ireader").unwrap()) };
        assert!(!opts.flatten_alpha);
        let (out, _) = optimize_epub_with(&epub, &opts).unwrap();
        assert_eq!(entry_bytes(&out, "OEBPS/i/logo.png"), imgs[0].1);
    }

    #[test]
    fn caption_fit_follows_profile_and_readable_area() {
        let (epub, imgs) = alpha_book();
        let html = |p: &str| {
            let (out, _) = optimize_epub_with(&epub, &OptimizeOpts { text_repair_only: false, ..OptimizeOpts::for_profile(profile::get(p).unwrap()) }).unwrap();
            (text_of(&out, "OEBPS/c1.xhtml"), out)
        };
        // 掌阅 1264×1680：0.8 × 1680 × 900/1650 / 1264 = 0.57998 → 57%；Kindle 1104×1546 → 61%
        let (ir, ir_out) = html("ireader");
        assert!(ir.contains(r#"<img src="i/tall.jpg" alt="" style="width:57%"/>"#), "{ir}");
        assert_eq!(entry_bytes(&ir_out, "OEBPS/i/tall.jpg"), imgs[4].1, "图片字节不动");
        let (k, _) = html("kindle");
        assert!(k.contains(r#"style="width:61%""#), "{k}");
        let (x, _) = html("xochitl");
        assert!(!x.contains("width:"), "xochitl 不开：{x}");
        // 别的图不动
        assert!(ir.contains(r#"<img class="logo" src="i/logo.png"/>"#), "{ir}");
    }

    /// `title`：OPF 的 `dc:title` 换成新书名（转义、挂在旧书名上的 refines 一起删）；缺省 `None` 和空白串都不动，产物逐字节相同。
    #[test]
    fn title_option_rewrites_dc_title_only_when_given() {
        let (epub, _) = bg_book();
        let base = OptimizeOpts { text_repair_only: false, ..OptimizeOpts::for_profile(profile::get("xochitl").unwrap()) };
        let (plain, _) = optimize_epub_with(&epub, &base).unwrap();
        let (blank, _) = optimize_epub_with(&epub, &OptimizeOpts { title: Some("  ".into()), ..base.clone() }).unwrap();
        assert_eq!(plain, blank, "空白书名当没给");
        let (out, _) = optimize_epub_with(&epub, &OptimizeOpts { title: Some(" 新书名 & <副题> ".into()), ..base.clone() }).unwrap();
        let opf = String::from_utf8(entry_bytes(&out, "OEBPS/content.opf")).unwrap();
        assert!(opf.contains("<dc:title>新书名 &amp; &lt;副题&gt;</dc:title>"), "{opf}");
        assert_eq!(opf.matches("<dc:title").count(), 1, "{opf}");
        let rep = crate::check::check_epub(&out, false).unwrap();
        assert!(rep.ok, "改了书名的书照样过质量门：{:?}", rep.errors);
        // 除 OPF 外各条目不变
        let names = |b: &[u8]| { let ar = ZipArchive::new(Cursor::new(b)).unwrap(); ar.file_names().map(str::to_string).collect::<Vec<_>>() };
        assert_eq!(names(&out), names(&plain));
        for n in names(&plain).iter().filter(|n| *n != "OEBPS/content.opf") {
            assert_eq!(entry_bytes(&out, n), entry_bytes(&plain, n), "{n}");
        }
    }

    /// 取消：`cancel()` 为真时返回以 `CANCELLED_MSG` 开头的错误，已建出的输出文件删掉；第几次问到时取消都一样。
    #[test]
    fn cancel_stops_and_leaves_no_output() {
        let (epub, _) = bg_book();
        let t = tempfile::tempdir().unwrap();
        let (input, output) = (t.path().join("in.epub"), t.path().join("out.epub"));
        std::fs::write(&input, &epub).unwrap();
        let opts = OptimizeOpts::for_profile(profile::get("xochitl").unwrap());
        for after in 0..8 {
            let n = std::cell::Cell::new(0);
            let cancel = || { n.set(n.get() + 1); n.get() > after };
            let err = optimize_epub_file_streaming_with_cancel(&input, &output, &opts, |_, _| {}, &cancel).unwrap_err();
            assert!(err.starts_with(CANCELLED_MSG), "{err}");
            assert!(!output.exists(), "第 {after} 次之后取消：不留半成品");
        }
        // 还没开始写就取消：不碰调用方原有的文件
        std::fs::write(&output, b"old").unwrap();
        assert!(optimize_epub_file_streaming_with_cancel(&input, &output, &opts, |_, _| {}, &|| true).is_err());
        assert_eq!(std::fs::read(&output).unwrap(), b"old");
        // 不取消：和没有取消参数的入口逐字节相同
        optimize_epub_file_streaming_with_cancel(&input, &output, &opts, |_, _| {}, &|| false).unwrap();
        assert_eq!(std::fs::read(&output).unwrap(), optimize_epub_with(&epub, &opts).unwrap().0);
    }

    /// `limits`：缺省值和以前的常量一样（产物不变）；调小解码上限后超过的图原样保留（不删图），并行额度给 0 也不卡死。
    #[test]
    fn limits_default_matches_constants_and_smaller_limit_keeps_images() {
        assert_eq!(Limits::default(), Limits { max_decode_pixels: 64_000_000, pool_pixel_budget: 36_000_000 });
        let (epub, imgs) = bg_book();
        let base = OptimizeOpts { text_repair_only: false, ..OptimizeOpts::for_profile(profile::get("xochitl").unwrap()) };
        assert_eq!(base.limits, Limits::default());
        let small = OptimizeOpts { limits: Limits { max_decode_pixels: 1_000_000, pool_pixel_budget: 0 }, ..base.clone() };
        let (out, _) = optimize_epub_with(&epub, &small).unwrap();
        for (n, b) in &imgs {
            assert_eq!(&entry_bytes(&out, n), b, "超过解码上限的图原样保留：{n}");
        }
        let (full, _) = optimize_epub_with(&epub, &base).unwrap();
        assert_ne!(entry_bytes(&full, "OEBPS/i/d.png"), imgs[2].1, "缺省上限下照常缩");
    }

    #[test]
    fn optimized_version_file_reads_marker() {
        let t = tempfile::tempdir().unwrap();
        assert_eq!(optimized_version_file(&t.path().join("none.epub")), None, "没有文件");
        let p = t.path().join("x.epub");
        std::fs::write(&p, b"not a zip").unwrap();
        assert_eq!(optimized_version_file(&p), None, "不是 zip");
    }

    /// 文字书只修复（profile `text_repair_only`，Kindle、掌阅，2026-10-08 用户定）：样式、注释、空白页、图片一概不动，
    /// 只升级成 EPUB 3、修成合法 XML、目录补到节。
    #[test]
    fn text_repair_only_keeps_content() {
        use image::{codecs::jpeg::JpegEncoder, DynamicImage, RgbImage};
        let big = DynamicImage::ImageRgb8(RgbImage::from_fn(3000, 2000, |x, y| image::Rgb([(x % 256) as u8, (y % 256) as u8, 90])));
        let mut jpg = Vec::new();
        JpegEncoder::new_with_quality(&mut jpg, 90).encode_image(&big).unwrap();
        let css = "body{font-family:\"宋体\";line-height:1.8;margin:2em}p{text-indent:0;margin:0.5em 0}";
        let c1 = r#"<?xml version="1.0" encoding="utf-8"?><html xmlns="http://www.w3.org/1999/xhtml"><head><title>t</title><link rel="stylesheet" href="style.css"/></head><body><h1>第一章</h1><p style="font-size:14px">　　正文&nbsp;一段<a href="notes.xhtml#n1">[1]</a></p><h2>一节</h2><p>又一段<img src="big.jpg"/></p><p>&nbsp;</p></body></html>"#;
        let empty = r#"<html xmlns="http://www.w3.org/1999/xhtml"><head><title>t</title></head><body><div class="mbppagebreak"></div></body></html>"#;
        let notes = r#"<html xmlns="http://www.w3.org/1999/xhtml"><head><title>t</title></head><body><p class="footnote" id="n1"><a href="c1.xhtml">[1]</a>注释</p></body></html>"#;
        let opf = r#"<?xml version="1.0"?><package xmlns="http://www.idpf.org/2007/opf" version="2.0" unique-identifier="u"><metadata xmlns:dc="http://purl.org/dc/elements/1.1/"><dc:identifier id="u">x</dc:identifier><dc:title>书</dc:title><dc:language>zh</dc:language></metadata><manifest><item id="ncx" href="toc.ncx" media-type="application/x-dtbncx+xml"/><item id="css" href="style.css" media-type="text/css"/><item id="img" href="big.jpg" media-type="image/jpeg"/><item id="c1" href="c1.xhtml" media-type="application/xhtml+xml"/><item id="e" href="empty.xhtml" media-type="application/xhtml+xml"/><item id="n" href="notes.xhtml" media-type="application/xhtml+xml"/></manifest><spine toc="ncx"><itemref idref="c1"/><itemref idref="e"/><itemref idref="n"/></spine></package>"#;
        let ncx = r#"<ncx xmlns="http://www.daisy.org/z3986/2005/ncx/"><head><meta name="dtb:uid" content="x"/></head><docTitle><text>书</text></docTitle><navMap><navPoint id="p1" playOrder="1"><navLabel><text>第一章</text></navLabel><content src="c1.xhtml"/></navPoint><navPoint id="p2" playOrder="2"><navLabel><text>注释</text></navLabel><content src="notes.xhtml"/></navPoint></navMap></ncx>"#;
        let mut buf = Vec::new();
        {
            let mut zw = ZipWriter::new(Cursor::new(&mut buf));
            let stored = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
            zw.start_file("mimetype", stored).unwrap();
            zw.write_all(b"application/epub+zip").unwrap();
            zw.start_file("META-INF/container.xml", stored).unwrap();
            zw.write_all(br#"<container><rootfiles><rootfile full-path="OEBPS/content.opf" media-type="application/oebps-package+xml"/></rootfiles></container>"#).unwrap();
            for (n, d) in [("OEBPS/content.opf", opf), ("OEBPS/toc.ncx", ncx), ("OEBPS/style.css", css), ("OEBPS/c1.xhtml", c1), ("OEBPS/empty.xhtml", empty), ("OEBPS/notes.xhtml", notes)] {
                zw.start_file(n, stored).unwrap();
                zw.write_all(d.as_bytes()).unwrap();
            }
            zw.start_file("OEBPS/big.jpg", stored).unwrap();
            zw.write_all(&jpg).unwrap();
            zw.finish().unwrap();
        }
        let opts = OptimizeOpts::for_profile(profile::get("ireader").unwrap());
        assert!(opts.text_repair_only);
        let (out, _) = optimize_epub_with(&buf, &opts).unwrap();
        // 样式表只按 Send to Kindle 的规则改（profile `kindle_rules`）：正文字体「宋体」整条去掉、body 的左右外边距不要；别的一个字不改
        assert_eq!(text_of(&out, "OEBPS/style.css"), "body{line-height:1.8;margin-top:2em;margin-bottom:2em;}p{text-indent:0;margin:0.5em 0}");
        assert_eq!(entry_bytes(&out, "OEBPS/big.jpg"), jpg, "图片原样");
        let names: Vec<String> = ZipArchive::new(Cursor::new(&out)).unwrap().file_names().map(String::from).collect();
        assert!(!names.iter().any(|n| n.ends_with("eink-wash.css")), "不加排版样式表: {names:?}");
        let x = text_of(&out, "OEBPS/c1.xhtml");
        assert!(x.contains(r#"style="font-size:14px""#) && x.contains("　　正文") && x.contains(r#"href="notes.xhtml#n1""#), "行内样式、段首空格、跨文件注释链接原样: {x}");
        assert!(!x.contains("&nbsp;") && x.contains("&#160;") && x.contains(r#"xmlns:epub="#), "修成合法 XHTML（命名实体换数字引用）: {x}");
        assert_eq!(crate::html::plain_text(&x), crate::html::plain_text(c1), "可见文字一字不差");
        assert!(text_of(&out, "OEBPS/notes.xhtml").contains("注释"), "注释留在原处");
        let opf_out = text_of(&out, "OEBPS/content.opf");
        assert!(opf_out.contains(r#"version="3.0""#) && opf_out.contains("empty.xhtml"), "升级 EPUB 3；空白页不删: {opf_out}");
        assert!(names.iter().any(|n| n.ends_with("nav.xhtml")), "补 nav: {names:?}");
        let flat = crate::ncx::parse_ncx_flat(&text_of(&out, "OEBPS/toc.ncx"));
        let labels: Vec<(usize, &str)> = flat.iter().map(|(d, l, _)| (*d, l.as_str())).collect();
        assert_eq!(labels, [(1, "第一章"), (2, "一节"), (1, "注释")], "目录补到节");
        // 标签缺省样式（profile `kindle_rules`，2026-10-08 用户定照 Send to Kindle 统一）：挂在书自带样式之前，书里写了的盖过它
        assert_eq!(text_of(&out, "OEBPS/eink-ua.css"), crate::uastyle::ua_css(None));
        let (ua, own) = (x.find("eink-ua.css").unwrap(), x.find("style.css").unwrap());
        assert!(ua < own, "缺省样式在书自带样式之前: {x}");
        assert!(opf_out.contains(r#"href="eink-ua.css""#), "manifest 补上: {opf_out}");
        assert!(!text_of(&out, "OEBPS/nav.xhtml").contains("eink-ua.css"), "目录页不挂");
        // Kindle 不写（KFX 写出器自己按同一张表排）
        let (k, _) = optimize_epub_with(&buf, &OptimizeOpts::for_profile(profile::get("kindle").unwrap())).unwrap();
        let knames: Vec<String> = ZipArchive::new(Cursor::new(&k)).unwrap().file_names().map(String::from).collect();
        assert!(!knames.iter().any(|n| n.ends_with("eink-ua.css")), "{knames:?}");
        assert_eq!(text_of(&k, "OEBPS/style.css"), css, "Kindle 的 EPUB 样式表一个字不改（规则由 KFX 写出器照做）");
    }

    /// xochitl 只修复、另保证注释能点（profile `repair_note_links`，2026-10-08 用户定）：注释搬进引用它的那一章、改同文件锚点（和原来的完整优化同一套），
    /// 别的（样式表、行内样式、段首空格、空白页）一概不动，也不加排版样式表。
    #[test]
    fn xochitl_repair_keeps_content_but_notes_jump() {
        let css = "p{font-family:\"宋体\";line-height:1.8}";
        let opf = r#"<?xml version="1.0"?><package xmlns="http://www.idpf.org/2007/opf" version="3.0" unique-identifier="u"><metadata xmlns:dc="http://purl.org/dc/elements/1.1/"><dc:identifier id="u">x</dc:identifier><dc:title>书</dc:title><dc:language>zh</dc:language></metadata><manifest><item id="nav" href="nav.xhtml" media-type="application/xhtml+xml" properties="nav"/><item id="css" href="s.css" media-type="text/css"/><item id="c1" href="c1.xhtml" media-type="application/xhtml+xml"/><item id="n" href="notes.xhtml" media-type="application/xhtml+xml"/></manifest><spine><itemref idref="c1"/><itemref idref="n"/></spine></package>"#;
        let nav = r#"<html xmlns="http://www.w3.org/1999/xhtml" xmlns:epub="http://www.idpf.org/2007/ops"><head><title>t</title></head><body><nav epub:type="toc"><ol><li><a href="c1.xhtml">第一章</a></li></ol></nav></body></html>"#;
        let c1 = r#"<html xmlns="http://www.w3.org/1999/xhtml"><head><title>t</title><link rel="stylesheet" href="s.css"/></head><body><h1>第一章</h1><p style="font-size:14px">　　正文<a id="r1" href="notes.xhtml#n1">[1]</a>结束</p></body></html>"#;
        let notes = r#"<html xmlns="http://www.w3.org/1999/xhtml"><head><title>t</title></head><body><p class="footnote" id="n1"><a href="c1.xhtml#r1">[1]</a>注释正文</p></body></html>"#;
        let mut buf = Vec::new();
        {
            let mut zw = ZipWriter::new(Cursor::new(&mut buf));
            let stored = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
            zw.start_file("mimetype", stored).unwrap();
            zw.write_all(b"application/epub+zip").unwrap();
            zw.start_file("META-INF/container.xml", stored).unwrap();
            zw.write_all(br#"<container><rootfiles><rootfile full-path="OEBPS/content.opf" media-type="application/oebps-package+xml"/></rootfiles></container>"#).unwrap();
            for (n, d) in [("OEBPS/content.opf", opf), ("OEBPS/nav.xhtml", nav), ("OEBPS/s.css", css), ("OEBPS/c1.xhtml", c1), ("OEBPS/notes.xhtml", notes)] {
                zw.start_file(n, stored).unwrap();
                zw.write_all(d.as_bytes()).unwrap();
            }
            zw.finish().unwrap();
        }
        let opts = OptimizeOpts::for_profile(profile::get("xochitl").unwrap());
        assert!(opts.text_repair_only && opts.repair_note_links);
        let (out, _) = optimize_epub_with(&buf, &opts).unwrap();
        assert_eq!(text_of(&out, "OEBPS/s.css"), "p{line-height:1.8}", "样式表只按 Send to Kindle 的规则改：正文字体整条去掉");
        let names: Vec<String> = ZipArchive::new(Cursor::new(&out)).unwrap().file_names().map(String::from).collect();
        assert!(!names.iter().any(|n| n.ends_with("eink-wash.css")), "不加排版样式表: {names:?}");
        let x = text_of(&out, "OEBPS/c1.xhtml");
        // 正文是行内的 14px：字号按正文归一成 16px（阅读器的字号），段首空格原样
        assert!(x.contains(r#"style="font-size:16px;""#) && x.contains("　　正文"), "字号归一、段首空格原样: {x}");
        assert!(x.contains(r##"href="#n1""##) && x.contains("注释正文"), "注释搬进本章、链接改同文件锚点: {x}");
    }

    /// 标签没关这类语法错误的书走完整流程（2026-10-09：html5fix 的样本，测试真书里一本都没触发过）：三个模式产物里每个 XHTML
    /// 都是合法 XML（xochitl 遇到不合法的整章空白），可见文字一个不差；配对修复修不好的那章按 HTML5 重新解析过。
    #[test]
    fn broken_markup_book_repaired_in_all_modes() {
        const OPF: &str = r#"<?xml version="1.0" encoding="utf-8"?><package xmlns="http://www.idpf.org/2007/opf" version="3.0" unique-identifier="id"><metadata xmlns:dc="http://purl.org/dc/elements/1.1/"><dc:identifier id="id">broken-markup</dc:identifier><dc:title>标签修复样本</dc:title><dc:language>zh</dc:language></metadata><manifest><item id="nav" href="nav.xhtml" media-type="application/xhtml+xml" properties="nav"/><item id="c1" href="c1.xhtml" media-type="application/xhtml+xml"/><item id="c2" href="c2.xhtml" media-type="application/xhtml+xml"/></manifest><spine><itemref idref="c1"/><itemref idref="c2"/></spine></package>"#;
        const NAV: &str = r#"<?xml version="1.0" encoding="utf-8"?><html xmlns="http://www.w3.org/1999/xhtml" xmlns:epub="http://www.idpf.org/2007/ops"><head><title>目录</title></head><body><nav epub:type="toc"><ol><li><a href="c1.xhtml">第一章</a></li><li><a href="c2.xhtml">第二章</a></li></ol></nav></body></html>"#;
        // 交叉嵌套、没关的 <p>/<li>、没闭合的空元素、HTML5 才有的实体、谁都没有的实体
        const C1: &str = r#"<?xml version="1.0" encoding="utf-8"?><html xmlns="http://www.w3.org/1999/xhtml"><head><title>第一章</title></head><body><h1>第一章</h1><p><i>交叉嵌套</p></i><p>没关的段落一<p>没关的段落二<br>换行<p><b>粗<i>粗斜</b>斜</i>&bigstar;&foo;</p><ul><li>列表一<li>列表二</ul><img src="nope.png"></body></html>"#;
        const C2: &str = r#"<?xml version="1.0" encoding="utf-8"?><html xmlns="http://www.w3.org/1999/xhtml"><head><title>第二章</title></head><body><h1>第二章</h1><p>本来就合法的一章。</p></body></html>"#;
        let epub = zip_book(&[
            ("META-INF/container.xml", r#"<?xml version="1.0"?><container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container"><rootfiles><rootfile full-path="OEBPS/content.opf" media-type="application/oebps-package+xml"/></rootfiles></container>"#),
            ("OEBPS/content.opf", OPF),
            ("OEBPS/nav.xhtml", NAV),
            ("OEBPS/c1.xhtml", C1),
            ("OEBPS/c2.xhtml", C2),
        ]);
        let want1 = crate::html::plain_text(C1).replace("&bigstar;", "★");
        for id in ["kindle", "ireader", "xochitl"] {
            let (out, rep) = optimize_epub_with(&epub, &OptimizeOpts::for_profile(profile::get(id).unwrap())).unwrap();
            let wash = rep.wash.as_ref().expect("文字书走清洗层");
            assert!(wash.xml_fixes.html5_reparsed >= 1, "{id}：第一章按 HTML5 重新解析: {:?}", wash.xml_fixes);
            let mut ar = ZipArchive::new(Cursor::new(&out)).unwrap();
            let names: Vec<String> = ar.file_names().map(str::to_string).collect();
            for n in names.iter().filter(|n| n.ends_with(".xhtml")) {
                let mut t = String::new();
                ar.by_name(n).unwrap().read_to_string(&mut t).unwrap();
                assert!(crate::wash::normalize::well_formed_xml(&t), "{id}：{n} 不是合法 XML:\n{t}");
            }
            let c1 = text_of(&out, "OEBPS/c1.xhtml");
            assert_eq!(crate::html::plain_text(&c1), want1, "{id}：可见文字一个不差:\n{c1}");
            assert!(c1.contains("<i>交叉嵌套</i></p>") && c1.contains("&amp;foo;"), "{id}：{c1}");
            assert_eq!(crate::html::plain_text(&text_of(&out, "OEBPS/c2.xhtml")), crate::html::plain_text(C2), "{id}：合法的一章文字不变");
        }
    }

    /// GBK 编码的 OPF（EPUB 2 写法的封面声明，要补 `cover-image`）：先转 UTF-8 再改，书名不坏（2026-10-09 审计：以前先按
    /// UTF-8 读成替换字符再写回）。
    #[test]
    fn gbk_opf_transcoded_before_cover_fix() {
        let opf = r#"<?xml version="1.0" encoding="gbk"?><package xmlns="http://www.idpf.org/2007/opf" version="2.0" unique-identifier="id"><metadata xmlns:dc="http://purl.org/dc/elements/1.1/"><dc:identifier id="id">gbk-book</dc:identifier><dc:title>中文书名</dc:title><dc:language>zh</dc:language><meta name="cover" content="cov"/></metadata><manifest><item id="cov" href="cover.jpg" media-type="image/jpeg"/><item id="c1" href="c1.xhtml" media-type="application/xhtml+xml"/><item id="ncx" href="toc.ncx" media-type="application/x-dtbncx+xml"/></manifest><spine toc="ncx"><itemref idref="c1"/></spine></package>"#;
        let (opf_gbk, _, _) = encoding_rs::GBK.encode(opf);
        let mut jpg = Vec::new();
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpg, 90).encode_image(&image::DynamicImage::ImageRgb8(image::RgbImage::new(60, 80))).unwrap();
        let c1 = r#"<?xml version="1.0" encoding="utf-8"?><html xmlns="http://www.w3.org/1999/xhtml"><head><title>一</title></head><body><h1>第一章</h1><p>正文。</p></body></html>"#;
        let ncx = r#"<?xml version="1.0" encoding="utf-8"?><ncx xmlns="http://www.daisy.org/z3986/2005/ncx/" version="2005-1"><head><meta name="dtb:uid" content="gbk-book"/></head><docTitle><text>书</text></docTitle><navMap><navPoint id="n1" playOrder="1"><navLabel><text>第一章</text></navLabel><content src="c1.xhtml"/></navPoint></navMap></ncx>"#;
        let epub = zip_book_bytes(&[
            ("META-INF/container.xml", br#"<?xml version="1.0"?><container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container"><rootfiles><rootfile full-path="OEBPS/content.opf" media-type="application/oebps-package+xml"/></rootfiles></container>"#),
            ("OEBPS/content.opf", &opf_gbk),
            ("OEBPS/c1.xhtml", c1.as_bytes()),
            ("OEBPS/toc.ncx", ncx.as_bytes()),
            ("OEBPS/cover.jpg", &jpg),
        ]);
        for id in ["kindle", "ireader", "xochitl"] {
            let (out, rep) = optimize_epub_with(&epub, &OptimizeOpts::for_profile(profile::get(id).unwrap())).unwrap();
            let opf = text_of(&out, "OEBPS/content.opf");
            assert!(opf.contains("中文书名") && !opf.contains('\u{FFFD}'), "{id}：{opf}");
            assert!(opf.contains("cover-image"), "{id}：封面声明照补: {opf}");
            assert_eq!(rep.wash.as_ref().unwrap().transcoded_to_utf8, 1, "{id}");
        }
    }

    /// 和原书逐字节相同的大字体原样拷原书的压缩数据（不解压再重压），改过名的（文件名里有 `:`）用新名字（2026-10-09）。
    #[test]
    fn unchanged_fonts_copied_raw_under_new_name() {
        let opf = r#"<?xml version="1.0" encoding="utf-8"?><package xmlns="http://www.idpf.org/2007/opf" version="3.0" unique-identifier="id"><metadata xmlns:dc="http://purl.org/dc/elements/1.1/"><dc:identifier id="id">font-book</dc:identifier><dc:title>字体</dc:title><dc:language>zh</dc:language></metadata><manifest><item id="nav" href="nav.xhtml" media-type="application/xhtml+xml" properties="nav"/><item id="c1" href="c1.xhtml" media-type="application/xhtml+xml"/><item id="f" href="Fonts/a:b.ttf" media-type="font/ttf"/></manifest><spine><itemref idref="c1"/></spine></package>"#;
        let nav = r#"<?xml version="1.0" encoding="utf-8"?><html xmlns="http://www.w3.org/1999/xhtml" xmlns:epub="http://www.idpf.org/2007/ops"><head><title>目录</title></head><body><nav epub:type="toc"><ol><li><a href="c1.xhtml">一</a></li></ol></nav></body></html>"#;
        let c1 = r#"<?xml version="1.0" encoding="utf-8"?><html xmlns="http://www.w3.org/1999/xhtml"><head><title>一</title><style>@font-face{font-family:"甲";src:url("Fonts/a:b.ttf")}</style></head><body><h1>一</h1><p style="font-family:甲">正文。</p></body></html>"#;
        // 压得动、又不是一串相同字节的"字体"
        let font: Vec<u8> = (0..200_000u32).map(|i| ((i * 7919) % 251) as u8 ^ (i / 1000) as u8).collect();
        let mut epub = Vec::new();
        {
            let mut zw = ZipWriter::new(Cursor::new(&mut epub));
            let stored = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
            let deflated = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated).compression_level(Some(9));
            zw.start_file("mimetype", stored).unwrap();
            zw.write_all(b"application/epub+zip").unwrap();
            for (n, d) in [
                ("META-INF/container.xml", br#"<?xml version="1.0"?><container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container"><rootfiles><rootfile full-path="OEBPS/content.opf" media-type="application/oebps-package+xml"/></rootfiles></container>"#.as_slice()),
                ("OEBPS/content.opf", opf.as_bytes()),
                ("OEBPS/nav.xhtml", nav.as_bytes()),
                ("OEBPS/c1.xhtml", c1.as_bytes()),
                ("OEBPS/Fonts/a:b.ttf", font.as_slice()),
            ] {
                zw.start_file(n, deflated).unwrap();
                zw.write_all(d).unwrap();
            }
            zw.finish().unwrap();
        }
        let raw_of = |z: &[u8], name: &str| {
            let mut ar = ZipArchive::new(Cursor::new(z)).unwrap();
            let i = ar.index_for_name(name).unwrap();
            let mut v = Vec::new();
            ar.by_index_raw(i).unwrap().read_to_end(&mut v).unwrap();
            v
        };
        let (out, _) = optimize_epub_with(&epub, &OptimizeOpts::for_profile(profile::get("ireader").unwrap())).unwrap();
        let names: Vec<String> = ZipArchive::new(Cursor::new(&out)).unwrap().file_names().map(str::to_string).collect();
        let new_name = names.iter().find(|n| n.starts_with("OEBPS/Fonts/")).expect("字体还在").clone();
        assert!(!new_name.contains(':'), "改成安全的名字：{new_name}");
        assert_eq!(entry_bytes(&out, &new_name), font, "内容不变");
        assert_eq!(raw_of(&out, &new_name), raw_of(&epub, "OEBPS/Fonts/a:b.ttf"), "压缩数据原样拷过来（原书是 9 级压的，重压的话不一样）");
    }
