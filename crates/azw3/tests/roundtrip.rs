//! 往返校验：组一本小 EPUB → AZW3 → 用 bookconv 的 KF8 读取器读回来。

use bookconv::convert::palm;
use bookconv::epub::{assemble, Book, BookMeta, Chapter, NavEntry, Resource};

fn png(w: u32, h: u32) -> Vec<u8> {
    let img = image::DynamicImage::ImageRgb8(image::RgbImage::from_pixel(w, h, image::Rgb([200, 30, 30])));
    let mut out = Vec::new();
    img.write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png).unwrap();
    out
}

fn sample_epub() -> Vec<u8> {
    let long = "正文段落，含中文与 English mixed text。".repeat(300); // 跨多条 4096 字节记录，也会有多字节字符跨记录
    let mut book = Book {
        meta: BookMeta {
            book_id: "t".into(),
            title: "测试书".into(),
            author: "作者甲".into(),
            language: "zh".into(),
            publisher: "出版社".into(),
            cover: Some(png(600, 800)),
            cover_ext: "png".into(),
            cover_media_type: "image/png".into(),
        },
        chapters: vec![
            Chapter { title: "第一章".into(), html_body: format!(r##"<h1>第一章</h1><p>{long}<a href="chap_0002.xhtml#s2">跳到第二节</a></p><img src="images/a.png" alt=""/>"##), level: 1 },
            Chapter { title: "第二章".into(), html_body: format!(r#"<h1>第二章</h1><p>{long}</p><h2 id="s2">第二节</h2><p>节正文。</p>"#), level: 1 },
        ],
        resources: vec![Resource { path: "images/a.png".into(), media_type: "image/png".into(), bytes: png(100, 50) }],
        nav: vec![
            NavEntry { title: "第一章".into(), level: 1, href: "chap_0001.xhtml".into() },
            NavEntry { title: "第二章".into(), level: 1, href: "chap_0002.xhtml".into() },
            NavEntry { title: "第二节".into(), level: 2, href: "chap_0002.xhtml#s2".into() },
        ],
    };
    assemble(&mut book).unwrap()
}

#[test]
fn azw3_reads_back_with_kf8_reader() {
    let epub = sample_epub();
    let azw3 = azw3::epub_to_azw3(&epub, &azw3::Opts { fixed_id: Some((0x1234, 0x5678)), ..Default::default() }).unwrap();
    assert_eq!(&azw3[60..68], b"BOOKMOBI");
    let records = palm::parse_palmdb(&azw3).unwrap();
    let h = palm::parse_header(records[0]).unwrap();
    assert_eq!(h.compression, 2);
    assert_eq!(h.extra_flags, 1);
    let raw = palm::decompress_text(&records, &h);
    let raw = String::from_utf8(raw).expect("解压后是合法 UTF-8（多字节尾随字节剥得对）");
    assert!(raw.contains("<body aid=\"0\">") && raw.contains("跳到第二节"), "骨架 + 片段");
    assert!(raw.contains("kindle:embed:0002?mime=image/png"), "图片改成 kindle:embed（资源 1 是封面）");
    // 片段索引：键 = 插入位置，插入位置就是骨架里 </body> 的位置。spine 里第一个是 bookconv 自动加的封面页。
    let starts = palm::parse_fragment_starts(&records, &h);
    assert_eq!(starts.len(), 3);
    for &s in &starts {
        assert!(raw[s..].starts_with("</body>"), "插入位置应在骨架的 </body> 前");
    }
    // 链接回填：fid=2（第二章的片段），off = id="s2" 那个标签在片段里的偏移
    let link = raw.find("kindle:pos:fid:0002:off:").expect("书内链接改成 kindle:pos");
    let off = u32::from_str_radix(&raw[link + 24..link + 34], 32).unwrap() as usize;
    let tail = "</body>\n</html>\n"; // 骨架在 </body> 之后的部分；rawML 里片段紧跟骨架存放
    assert!(raw[starts[2]..].starts_with(tail));
    let frag2 = starts[2] + tail.len();
    assert!(raw[frag2 + off..].starts_with(r#"<h2 id="s2">"#), "偏移要正好落在目标标签上: {}", &raw[frag2 + off..frag2 + off + 30]);
    let ncx = palm::parse_ncx(&records, &h);
    let labels: Vec<(&str, u8)> = ncx.iter().map(|e| (e.label.as_str(), e.level)).collect();
    assert!(labels.contains(&("第一章", 0)) && labels.contains(&("第二章", 0)) && labels.contains(&("第二节", 1)), "{labels:?}");
    // 整本读回成 EPUB
    let (back, _) = bookconv::convert::kf8::azw3_to_epub(&azw3).unwrap();
    let entries = bookconv::epubzip::read_entries(&back).unwrap();
    let text: String = entries.iter().filter(|e| e.name.ends_with(".xhtml")).map(|e| String::from_utf8_lossy(&e.data).into_owned()).collect();
    assert!(text.contains("节正文。") && text.contains("正文段落"), "内容读得回来");
}
