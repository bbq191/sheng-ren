use super::*;

/// 一本小 EPUB（封面、章节、粗体、换行、书内链接、两级目录）走一遍，核对真机定下来的几条规矩。
#[test]
fn end_to_end_rules() {
    use crate::container::Container;
    use crate::ion::SymbolTable;
    let mut jpeg = Vec::new();
    image::codecs::jpeg::JpegEncoder::new(&mut jpeg).encode_image(&image::GrayImage::from_pixel(4, 6, image::Luma([128]))).unwrap();
    let mut w = bookconv::epubzip::EpubWriter::new(std::io::Cursor::new(Vec::new())).unwrap();
    w.put("META-INF/container.xml", br#"<?xml version="1.0"?><container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container"><rootfiles><rootfile full-path="OEBPS/content.opf" media-type="application/oebps-package+xml"/></rootfiles></container>"#).unwrap();
    w.put("OEBPS/content.opf", r#"<?xml version="1.0"?><package xmlns="http://www.idpf.org/2007/opf" version="3.0" unique-identifier="id"><metadata xmlns:dc="http://purl.org/dc/elements/1.1/"><dc:identifier id="id">x</dc:identifier><dc:title>书</dc:title><dc:language>zh</dc:language></metadata><manifest><item id="nav" href="nav.xhtml" media-type="application/xhtml+xml" properties="nav"/><item id="img" href="c.jpg" media-type="image/jpeg" properties="cover-image"/><item id="cv" href="cover.xhtml" media-type="application/xhtml+xml"/><item id="c1" href="c1.xhtml" media-type="application/xhtml+xml"/><item id="c2" href="c2.xhtml" media-type="application/xhtml+xml"/></manifest><spine><itemref idref="cv"/><itemref idref="c1"/><itemref idref="c2"/></spine></package>"#.as_bytes()).unwrap();
    w.put("OEBPS/c.jpg", &jpeg).unwrap();
    w.put("OEBPS/nav.xhtml", r#"<html xmlns="http://www.w3.org/1999/xhtml" xmlns:epub="http://www.idpf.org/2007/ops"><body><nav epub:type="toc"><ol><li><a href="c1.xhtml">第一章</a><ol><li><a href="c2.xhtml#s">1</a></li></ol></li></ol></nav></body></html>"#.as_bytes()).unwrap();
    w.put("OEBPS/cover.xhtml", br#"<html xmlns="http://www.w3.org/1999/xhtml"><body><div><img src="c.jpg"/></div></body></html>"#).unwrap();
    w.put("OEBPS/c1.xhtml", r#"<html xmlns="http://www.w3.org/1999/xhtml"><head><style>p{text-indent:2em}</style></head><body><h1>第一章</h1><p>甲<b>乙</b>丙<br/>丁 <a href="c2.xhtml#s">跳</a></p></body></html>"#.as_bytes()).unwrap();
    w.put("OEBPS/c2.xhtml", r#"<html xmlns="http://www.w3.org/1999/xhtml"><body><h3 id="s">1</h3><p>戊</p></body></html>"#.as_bytes()).unwrap();
    let epub = w.finish().unwrap().into_inner();

    let (kfx, warnings) = epub_to_kfx(&epub, &Opts { fixed_id: Some(1), ..Default::default() }).unwrap();
    assert!(warnings.is_empty(), "{warnings:?}");
    let c = Container::parse(&kfx).unwrap();
    let syms: SymbolTable = c.symbols();
    let of = |ty: u32| c.entities.iter().filter(move |e| e.ty == ty);
    // 文字一个不多一个不少，`<br>` 是 `\n`。
    let texts: Vec<String> = of(T_TEXT_POOL).flat_map(|e| e.value().unwrap().field(CHILDREN).unwrap().as_list().unwrap().iter().map(|v| v.as_str().unwrap().to_string())).collect();
    assert_eq!(texts, ["第一章", "甲乙丙\n丁 跳", "1", "戊"]);
    // 图片字节：资源路径的符号编号 − 9 = 字节实体（且就是清单里的依赖）。
    let res = of(T_RESOURCE).next().unwrap().value().unwrap().clone();
    let loc = syms.sid(res.field(RES_LOCATION).unwrap().as_str().unwrap()).unwrap();
    let raw = of(T_RAW_MEDIA).next().unwrap();
    assert_eq!(raw.id, loc - SID_GAP);
    // 封面：`cover_image` 的符号编号 − 9 = 封面资源；字节实体叫 `$538.$597.$614` + `-ad`。
    let meta = format!("{:?}", of(T_METADATA).next().unwrap().value().unwrap());
    assert!(meta.contains(&format!("String({COVER_REF:?})")), "{meta}");
    // 中文书的元数据语言写 en（开间距设置），正文样式里仍是 zh。
    assert!(meta.contains(r#"String("language")), (307, String("en"))"#), "{meta}");
    let styles = format!("{:?}", of(T_STYLE).map(|e| e.value().unwrap().clone()).collect::<Vec<_>>());
    assert!(styles.contains(r#"(10, String("zh"))"#), "{styles}");
    assert_eq!(syms.sid(COVER_REF).unwrap() - SID_GAP, res.field(RESOURCE_REF).unwrap().as_symbol().unwrap());
    let aux = of(T_DOCUMENT_DATA).next().unwrap().value().unwrap().field(DOC_AUX).unwrap().field(DOC_AUX_NAME).unwrap().as_symbol().unwrap();
    assert_eq!(syms.display(raw.id), format!("{}-ad", syms.display(aux)));
    // 链接指向的锚点存在；目录两级。
    let anchors: Vec<u32> = of(T_ANCHOR).map(|e| e.id).collect();
    let story = format!("{:?}", of(T_STORYLINE).map(|e| e.value().unwrap().clone()).collect::<Vec<_>>());
    assert!(anchors.iter().any(|a| story.contains(&format!("({LINK_TO}, Symbol({a}))"))), "{story}");
    let toc = of(T_NAV_CONTAINER).find(|e| e.value().unwrap().field(NAV_TYPE) == Some(&Value::Symbol(NAV_TYPE_TOC))).unwrap();
    let top = toc.value().unwrap().field(NAV_ENTRIES).unwrap().as_list().unwrap();
    assert_eq!(top.len(), 1);
    assert_eq!(top[0].field(NAV_ENTRIES).unwrap().as_list().unwrap().len(), 1);
    // 位置：各版面长度之和 = `$609` 游程之和。
    let ranges = of(T_SECTION_RANGES).next().unwrap().value().unwrap().field(LIST).unwrap().as_list().unwrap().to_vec();
    for (r, m) in ranges.iter().zip(of(T_SECTION_PID_MAP)) {
        let sum: i64 = m.value().unwrap().field(LIST).unwrap().as_list().unwrap().iter().map(|x| x.as_list().map_or_else(|| x.as_int().unwrap(), |p| p[0].as_int().unwrap())).sum();
        assert_eq!(sum, r.field(LENGTH).unwrap().as_int().unwrap());
    }
}

/// 注释配对：正文链接 → 章末注释，注释开头的回链指回正文链接。只有正文那条算注释引用，回链不算。
#[test]
fn notes_paired_by_backlink() {
    let mut w = bookconv::epubzip::EpubWriter::new(std::io::Cursor::new(Vec::new())).unwrap();
    w.put("META-INF/container.xml", br#"<?xml version="1.0"?><container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container"><rootfiles><rootfile full-path="c.opf" media-type="application/oebps-package+xml"/></rootfiles></container>"#).unwrap();
    w.put("c.opf", br#"<?xml version="1.0"?><package xmlns="http://www.idpf.org/2007/opf" version="3.0" unique-identifier="id"><metadata xmlns:dc="http://purl.org/dc/elements/1.1/"><dc:identifier id="id">x</dc:identifier><dc:title>t</dc:title><dc:language>zh</dc:language></metadata><manifest><item id="c1" href="c1.xhtml" media-type="application/xhtml+xml"/></manifest><spine><itemref idref="c1"/></spine></package>"#).unwrap();
    w.put("c1.xhtml", r##"<html xmlns="http://www.w3.org/1999/xhtml"><body><p>正文<a id="r1" href="#n1"><sup>[1]</sup></a>接着</p><p>别的<a href="#x">链接</a></p><p id="x">目标</p><p class="fn"><a id="n1" href="#r1">[1]</a>注释正文</p></body></html>"##.as_bytes()).unwrap();
    let epub = w.finish().unwrap().into_inner();
    let (kfx, _) = epub_to_kfx(&epub, &Opts { fixed_id: Some(1), ..Default::default() }).unwrap();
    let c = crate::container::Container::parse(&kfx).unwrap();
    let story = format!("{:?}", c.entities.iter().filter(|e| e.ty == T_STORYLINE).map(|e| e.value().unwrap().clone()).collect::<Vec<_>>());
    assert_eq!(story.matches(&format!("({NOTE_REF}, Symbol({NOTE_REF_POPUP}))")).count(), 1, "{story}");
    assert_eq!(story.matches(&format!("({NOTE_CONTENT}, Symbol({NOTE_CONTENT_FOOTNOTE}))")).count(), 1, "{story}");
}

/// 注释引用前面隔着空格的锚点（《福尔摩斯》`<sup><span id="r"></span> <small><a href="#n">(1)</a></small></sup>`）：写出器 15 起空格不算进
/// 链接区间，配对照样认区间前紧挨着的空格处的 id。
#[test]
fn notes_paired_across_leading_space() {
    let mut w = bookconv::epubzip::EpubWriter::new(std::io::Cursor::new(Vec::new())).unwrap();
    w.put("META-INF/container.xml", br#"<?xml version="1.0"?><container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container"><rootfiles><rootfile full-path="c.opf" media-type="application/oebps-package+xml"/></rootfiles></container>"#).unwrap();
    w.put("c.opf", br#"<?xml version="1.0"?><package xmlns="http://www.idpf.org/2007/opf" version="3.0" unique-identifier="id"><metadata xmlns:dc="http://purl.org/dc/elements/1.1/"><dc:identifier id="id">x</dc:identifier><dc:title>t</dc:title><dc:language>zh</dc:language></metadata><manifest><item id="c1" href="c1.xhtml" media-type="application/xhtml+xml"/></manifest><spine><itemref idref="c1"/></spine></package>"#).unwrap();
    w.put("c1.xhtml", r##"<html xmlns="http://www.w3.org/1999/xhtml"><body><p>正文<sup><span id="r1"></span>
 <small>
<a href="#n1">(1)</a></small></sup> 接着，又 <a id="r2" href="#n2">[2]</a></p><p id="n1"><a href="#r1">(1)</a>注一</p><p><a id="n2" href="#r2">[2]</a>注二</p></body></html>"##.as_bytes()).unwrap();
    let epub = w.finish().unwrap().into_inner();
    let (kfx, _) = epub_to_kfx(&epub, &Opts { fixed_id: Some(1), ..Default::default() }).unwrap();
    let c = crate::container::Container::parse(&kfx).unwrap();
    let story = format!("{:?}", c.entities.iter().filter(|e| e.ty == T_STORYLINE).map(|e| e.value().unwrap().clone()).collect::<Vec<_>>());
    assert_eq!(story.matches(&format!("({NOTE_REF}, Symbol({NOTE_REF_POPUP}))")).count(), 2, "{story}");
    assert_eq!(story.matches(&format!("({NOTE_CONTENT}, Symbol({NOTE_CONTENT_FOOTNOTE}))")).count(), 2, "{story}");
}

/// 列表、表格、边框按 2026-10-05 测试书（Amazon 转出的 KFX）对照出来的写法。
#[test]
fn lists_tables_borders_like_amazon() {
    let mut w = bookconv::epubzip::EpubWriter::new(std::io::Cursor::new(Vec::new())).unwrap();
    w.put("META-INF/container.xml", br#"<?xml version="1.0"?><container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container"><rootfiles><rootfile full-path="c.opf" media-type="application/oebps-package+xml"/></rootfiles></container>"#).unwrap();
    w.put("c.opf", br#"<?xml version="1.0"?><package xmlns="http://www.idpf.org/2007/opf" version="3.0" unique-identifier="id"><metadata xmlns:dc="http://purl.org/dc/elements/1.1/"><dc:identifier id="id">x</dc:identifier><dc:title>t</dc:title><dc:language>zh</dc:language></metadata><manifest><item id="c1" href="c1.xhtml" media-type="application/xhtml+xml"/></manifest><spine><itemref idref="c1"/></spine></package>"#).unwrap();
    w.put("c1.xhtml", r#"<html xmlns="http://www.w3.org/1999/xhtml"><head><style>td{border:1px solid #000;vertical-align:top} .b{border-top:2px dashed red;padding-left:0.5em} li.x{list-style-type:lower-roman}</style></head><body>
        <ul><li>甲<ul><li>乙</li></ul></li></ul>
        <ol start="3"><li class="x">丙</li><li value="9"><p>丁</p><p>戊</p></li></ol>
        <ul style="list-style:none"><li>己</li></ul>
        <table style="border-collapse:collapse;width:80%"><caption>表题</caption><tr><td colspan="2" style="width:30%">庚</td></tr><tr><td rowspan="2">辛</td><td></td></tr></table>
        <div class="b"><p>壬</p></div><hr/><p>前<u>癸</u></p></body></html>"#.as_bytes()).unwrap();
    let epub = w.finish().unwrap().into_inner();
    let (kfx, warnings) = epub_to_kfx(&epub, &Opts { fixed_id: Some(1), ..Default::default() }).unwrap();
    assert!(warnings.is_empty(), "{warnings:?}");
    let c = crate::container::Container::parse(&kfx).unwrap();
    let story = format!("{:?}", c.entities.iter().filter(|e| e.ty == T_STORYLINE).map(|e| e.value().unwrap().clone()).collect::<Vec<_>>());
    let styles = format!("{:?}", c.entities.iter().filter(|e| e.ty == T_STYLE).map(|e| e.value().unwrap().clone()).collect::<Vec<_>>());
    let has = |k: u32, v: u32| story.contains(&format!("({k}, Symbol({v}))"));
    // 列表：嵌套的 ul 换空心圆；li 上写的符号优先；start/value；不显示符号的不是列表
    assert!(has(LIST_STYLE, LIST_DISC) && has(LIST_STYLE, LIST_CIRCLE) && has(LIST_STYLE, LIST_LOWER_ROMAN), "{story}");
    assert_eq!(story.matches(&format!("({NODE_TYPE}, Symbol({NODE_LIST}))")).count(), 3, "{story}");
    assert!(story.contains(&format!("({LIST_START}, Int(3))")) && story.contains(&format!("({LIST_START}, Int(9))")), "{story}");
    // 表格：表 → 表体 → 行 → 单元格，标题；跨列跨行、竖直对齐、列宽、边框合并
    for t in [NODE_TABLE, NODE_TBODY, NODE_ROW] {
        assert!(has(NODE_TYPE, t), "{t}: {story}");
    }
    assert!(has(NOTE_CONTENT, CAPTION) && story.contains(&format!("({TABLE_COLLAPSE}, Bool(true))")) && story.contains(&format!("({TABLE_COLUMNS}, ")), "{story}");
    assert!(styles.contains(&format!("({P_COLSPAN}, Int(2))")) && styles.contains(&format!("({P_ROWSPAN}, Int(2))")), "{styles}");
    assert!(styles.contains(&format!("({P_CELL_VALIGN}, Symbol({VALIGN_TOP}))")), "{styles}");
    // 边框：四边一样写「全部」（1px＝0.45pt），只有上边写上边；左内边距是 `$53`
    assert!(styles.contains(&format!("({}, Symbol({BORDER_SOLID}))", P_BORDER_STYLE[0])) && styles.contains("(307, F64(0.45))"), "{styles}");
    assert!(styles.contains(&format!("({}, Symbol({BORDER_DASHED}))", P_BORDER_STYLE[1])) && styles.contains(&format!("({}, Int({}))", P_BORDER_COLOR[1], 0xFFFF0000u32)), "{styles}");
    assert!(styles.contains(&format!("({P_PADDING_LEFT}, ")), "{styles}");
    assert!(has(NODE_TYPE, NODE_HR) && styles.contains(&format!("({P_UNDERLINE}, Symbol({BORDER_SOLID}))")), "{story}");
    // 文字一个不多一个不少
    let texts: Vec<String> = c.entities.iter().filter(|e| e.ty == T_TEXT_POOL).flat_map(|e| e.value().unwrap().field(CHILDREN).unwrap().as_list().unwrap().iter().map(|v| v.as_str().unwrap().to_string()).collect::<Vec<_>>()).collect();
    assert_eq!(texts.concat(), "甲乙丙丁戊己表题庚辛壬前癸");
}

/// 照 Send to Kindle 排《绍宋》的口径（2026-10-08 对照）：行高按正文行高归一、边距按本元素行高换算、`<p>` 和标题的 UA 外边距、
/// 标题 UA 字号、全透明边框写「透明」、左右百分比内边距照写、px 外边距按 0.45pt、`white-space:nowrap`。
#[test]
fn styles_like_send_to_kindle() {
    let mut w = bookconv::epubzip::EpubWriter::new(std::io::Cursor::new(Vec::new())).unwrap();
    w.put("META-INF/container.xml", br#"<?xml version="1.0"?><container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container"><rootfiles><rootfile full-path="c.opf" media-type="application/oebps-package+xml"/></rootfiles></container>"#).unwrap();
    w.put("c.opf", br#"<?xml version="1.0"?><package xmlns="http://www.idpf.org/2007/opf" version="3.0" unique-identifier="id"><metadata xmlns:dc="http://purl.org/dc/elements/1.1/"><dc:identifier id="id">x</dc:identifier><dc:title>t</dc:title><dc:language>zh</dc:language></metadata><manifest><item id="c1" href="c1.xhtml" media-type="application/xhtml+xml"/></manifest><spine><itemref idref="c1"/></spine></package>"#).unwrap();
    w.put("c1.xhtml", r#"<html xmlns="http://www.w3.org/1999/xhtml"><head><style>p{line-height:1.5em;text-indent:2em}
        h1.j{line-height:1em;padding:0.5em 33%;background-color:#700}
        blockquote.l{margin:7% 0;border:35px solid rgba(0,0,0,0)} p.c{margin:-10px;white-space:nowrap}</style></head><body>
        <h2>一</h2><p>甲甲甲甲甲甲甲甲</p><p>乙乙乙乙乙乙乙乙</p><h1 class="j">章</h1>
        <blockquote class="l"><p class="c">诗</p></blockquote></body></html>"#.as_bytes()).unwrap();
    let epub = w.finish().unwrap().into_inner();
    let (kfx, warnings) = epub_to_kfx(&epub, &Opts { fixed_id: Some(1), ..Default::default() }).unwrap();
    assert!(warnings.is_empty(), "{warnings:?}");
    let c = crate::container::Container::parse(&kfx).unwrap();
    let styles: Vec<Value> = c.entities.iter().filter(|e| e.ty == T_STYLE).map(|e| e.value().unwrap().clone()).collect();
    let all = format!("{styles:?}");
    let n = |v: f64, u: u32| format!("{:?}", num(v, u));
    let has = |k: u32, v: &str| all.contains(&format!("({k}, {v})"));
    // 正文 `line-height:1.5em` 是全书最多的行高 → 1.0；段间 UA 外边距 1em → 0.833333lh（相邻的折叠成一个）
    assert!(has(P_LINE_HEIGHT, &n(1.0, U_LH)) && has(P_MARGIN_TOP, &n(0.833333, U_LH)), "{all}");
    // 标题 UA：h2 1.5 倍字号
    assert!(has(P_FONT_SIZE, &n(1.5, U_FONT_EM)), "{all}");
    // 行高 1em 的 h1 → 0.666667；它的 0.5em 内边距 → 0.625lh；左右 33% 照写百分比
    assert!(has(P_LINE_HEIGHT, &n(0.666667, U_LH)) && has(P_PADDING_TOP, &n(0.625, U_LH)) && has(P_PADDING_LEFT, &n(33.0, U_PERCENT)), "{all}");
    // 没写行高的引文容器 1.2/1.5 = 0.8；7% 上边距（2.24em）→ 2.333333lh；35px 透明边框 → 15.75pt、颜色写「透明」
    assert!(has(P_LINE_HEIGHT, &n(0.8, U_LH)) && has(P_MARGIN_TOP, &n(2.333333, U_LH)), "{all}");
    assert!(has(P_BORDER_COLOR[0], &format!("Symbol({COLOR_TRANSPARENT})")) && has(P_BORDER_WIDTH[0], &n(15.75, U_PT)), "{all}");
    // px 外边距按 1px＝0.45pt：-10px → -0.3125lh；不换行
    assert!(has(P_MARGIN_TOP, &n(-0.3125, U_LH)) && has(P_NOWRAP, "Bool(true)"), "{all}");
}

/// 写出器 11 照 Send to Kindle（2026-10-08 对照《绍宋》《绝叫》）：不写 normal 字重、没有背景处不写近黑颜色、600 半粗、
/// `word-break:break-all`、标题提示、左右边距写包含块的百分比、窄容器里的首行缩进写百分比、缺字体文件的字体写 `default`、
/// 折叠后的外边距在后一块没有上边距时留在前一块。
#[test]
fn styles_rules_of_send_to_kindle() {
    let mut w = bookconv::epubzip::EpubWriter::new(std::io::Cursor::new(Vec::new())).unwrap();
    w.put("META-INF/container.xml", br#"<?xml version="1.0"?><container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container"><rootfiles><rootfile full-path="c.opf" media-type="application/oebps-package+xml"/></rootfiles></container>"#).unwrap();
    w.put("c.opf", br#"<?xml version="1.0"?><package xmlns="http://www.idpf.org/2007/opf" version="3.0" unique-identifier="id"><metadata xmlns:dc="http://purl.org/dc/elements/1.1/"><dc:identifier id="id">x</dc:identifier><dc:title>t</dc:title><dc:language>zh</dc:language></metadata><manifest><item id="c1" href="c1.xhtml" media-type="application/xhtml+xml"/></manifest><spine><itemref idref="c1"/></spine></package>"#).unwrap();
    w.put("c1.xhtml", r#"<html xmlns="http://www.w3.org/1999/xhtml"><head><style>@font-face{font-family:"宋体";src:url(none.ttf)}
        p{font-family:"宋体";color:#111;word-break:break-all;text-indent:2em;margin:0}
        p.g{font-weight:600;margin-left:0.5em} div.box{background-color:#eee;padding:1em} div.t{margin-bottom:1em}</style></head><body>
        <h2>标题</h2><p>正文一正文一</p><p class="g">半粗半粗</p><div class="box"><p>框里的字</p></div><div class="t">目录</div><p>下一段</p></body></html>"#.as_bytes()).unwrap();
    let epub = w.finish().unwrap().into_inner();
    let (kfx, _) = epub_to_kfx(&epub, &Opts { fixed_id: Some(1), ..Default::default() }).unwrap();
    let c = crate::container::Container::parse(&kfx).unwrap();
    let styles: Vec<Value> = c.entities.iter().filter(|e| e.ty == T_STYLE).map(|e| e.value().unwrap().clone()).collect();
    let all = format!("{styles:?}");
    let n = |v: f64, u: u32| format!("{:?}", num(v, u));
    let has = |k: u32, v: &str| all.contains(&format!("({k}, {v})"));
    assert!(!has(P_FONT_WEIGHT, &format!("Symbol({WEIGHT_NORMAL})")), "normal 字重不写: {all}");
    assert!(has(P_FONT_WEIGHT, &format!("Symbol({WEIGHT_SEMIBOLD})")) && has(P_FONT_WEIGHT, &format!("Symbol({WEIGHT_BOLD})")), "{all}");
    // #111 只在有背景色的框里写
    assert_eq!(all.matches(&format!("({P_COLOR}, Int({}))", 0xFF11_1111u32)).count(), 1, "{all}");
    assert!(has(P_WORD_BREAK, &format!("Symbol({WORD_BREAK_ALL})")) && has(P_LAYOUT_HINTS, &format!("List([Symbol({HINT_HEADING})])")), "{all}");
    assert!(has(P_FONT_FAMILY, "String(\"default\")") && !all.contains("宋体"), "{all}");
    // 0.5em 左边距 → 1.5625%；正文缩进 2em；框（左右各 1em 内边距）里的缩进 2/30 → 6.666667%
    assert!(has(P_MARGIN_LEFT, &n(1.5625, U_PERCENT)) && has(P_TEXT_INDENT, &n(2.0, U_EM)) && has(P_TEXT_INDENT, &n(6.666667, U_PERCENT)), "{all}");
    // 「目录」的下边距 1em（0.833333lh）留在自己身上：下一段没有上边距
    assert!(has(P_MARGIN_BOTTOM, &n(0.833333, U_LH)), "{all}");
}

/// 写出器 11 第二轮（2026-10-08，逐属性对照 21 本 Send to Kindle 的书）：字号按正文归一、相对根；行高写成长度的按绝对值继承、
/// 下限 0.6；body 左右边距不写（这一侧有负外边距时照加）；表现属性（`align`、`<font size>`）；整段的行内样式并进段落；
/// 正文字体写 `default`、备选整串照写；声明了的 normal 字重照写、`<b>` 写 bolder；对比度不到 4.5 的文字颜色调深；块宽度；
/// `min-height`；链接颜色写 `$576`/`$577`；`<li><p>` 里 `<p>` 自己的样式不丢。
#[test]
fn send_to_kindle_rules_round_two() {
    let mut w = bookconv::epubzip::EpubWriter::new(std::io::Cursor::new(Vec::new())).unwrap();
    w.put("META-INF/container.xml", br#"<?xml version="1.0"?><container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container"><rootfiles><rootfile full-path="c.opf" media-type="application/oebps-package+xml"/></rootfiles></container>"#).unwrap();
    w.put("c.opf", br#"<?xml version="1.0"?><package xmlns="http://www.idpf.org/2007/opf" version="3.0" unique-identifier="id"><metadata xmlns:dc="http://purl.org/dc/elements/1.1/"><dc:identifier id="id">x</dc:identifier><dc:title>t</dc:title><dc:language>zh</dc:language></metadata><manifest><item id="c1" href="c1.xhtml" media-type="application/xhtml+xml"/></manifest><spine><itemref idref="c1"/></spine></package>"#).unwrap();
    w.put("c1.xhtml", r#"<html xmlns="http://www.w3.org/1999/xhtml"><head><style>
        body{margin:0 5pt;line-height:130%;font-family:"Body Font",serif} p{font-size:1.25em;margin:0}
        p.big{font-size:1.5em} h2{font-weight:normal;color:#fff;background-color:#f0a200;width:3em;margin:1em auto 1em -2em}
        p.min{min-height:2em} p.pale{color:#f5ac00} a{color:#00c} li p.note{font-size:0.95em;text-indent:-1em}</style></head><body>
        <p>正文正文正文正文正文正文正文正文正文正文正文正文正文正文正文正文正文正文</p><p>正文正文正文正文正文正文正文正文正文正文正文正文</p>
        <p class="big">大字</p><h2>小标题</h2><p align="center"><font size="1">小字</font></p><p><span class="big"><b>整段</b></span></p>
        <p class="min">最小高度</p><p class="pale">浅色字</p><p><a href="c1.xhtml">链接链接</a>后面</p>
        <ul style="list-style:none;margin:0;padding:0"><li><p class="note">注释</p></li></ul></body></html>"#.as_bytes()).unwrap();
    let epub = w.finish().unwrap().into_inner();
    let (kfx, _) = epub_to_kfx(&epub, &Opts { fixed_id: Some(1), ..Default::default() }).unwrap();
    let c = crate::container::Container::parse(&kfx).unwrap();
    let styles: Vec<Value> = c.entities.iter().filter(|e| e.ty == T_STYLE).map(|e| e.value().unwrap().clone()).collect();
    let all = format!("{styles:?}");
    let n = |v: f64, u: u32| format!("{:?}", num(v, u));
    let has = |k: u32, v: &str| all.contains(&format!("({k}, {v})"));
    // 正文 1.25em 归一成 1.0；1.5em → 1.2；`<font size="1">` 0.625em → 0.5；整段的 span 并进段落（字号 1.2、bolder）
    assert!(has(P_FONT_SIZE, &n(1.2, U_FONT_EM)) && has(P_FONT_SIZE, &n(0.5, U_FONT_EM)) && has(P_FONT_WEIGHT, &format!("Symbol({WEIGHT_BOLDER})")), "{all}");
    // body 的 5pt 不写：没有 1.302% 的边距；h2 的 -2em（负的）让左边照加 → 左边距 (-2em*1.5 + 5pt)/32
    assert!(!has(P_MARGIN_RIGHT, &n(1.302083, U_PERCENT)), "{all}");
    assert!(has(P_MARGIN_LEFT, &n((-3.0 + 5.0 / 12.0) / 32.0 * 100.0, U_PERCENT)), "{all}");
    // 正文字体写 default + 备选；h2：normal 照写、白字在橙底上调成 #454545、宽度 3em + 靠左
    assert!(has(P_FONT_FAMILY, "String(\"default,serif\")") && has(P_FONT_WEIGHT, &format!("Symbol({WEIGHT_NORMAL})")), "{all}");
    assert!(has(P_COLOR, &format!("Int({})", 0xFF45_4545u32)) && has(P_WIDTH, &n(3.0, U_EM)) && has(P_BOX_ALIGN, &format!("Symbol({ALIGN_LEFT})")), "{all}");
    assert!(has(P_TEXT_ALIGN, &format!("Symbol({ALIGN_CENTER})")) && has(P_MIN_HEIGHT, &n(2.0, U_EM)), "{all}");
    // 浅色字压暗到 4.5:1；链接颜色写成链接的颜色
    assert!(has(P_COLOR, &format!("Int({})", 0xFF9D_6E00u32)) && has(P_LINK_UNVISITED, &format!("Struct([({P_COLOR}, Int({}))])", 0xFF00_00CCu32)), "{all}");
    // `<li><p class="note">`：p 自己的字号、缩进没丢（0.95/1.25 = 0.76）
    assert!(has(P_FONT_SIZE, &n(0.76, U_FONT_EM)), "{all}");
}

/// 固定版式（漫画）：照 Amazon 转的测试漫画写元数据、文档数据和每页的画布版面；从右往左翻写 `$559`。
#[test]
fn fixed_layout_odd_shaped_images_keep_aspect() {
    assert_eq!(fixed_image_size((1272, 1696), (1272, 1696)), (1272, 1696));
    assert_eq!(fixed_image_size((954, 1272), (1272, 1696)), (954, 1272), "同比例的小图不放大（Kindle 不放大、会空白）");
    assert_eq!(fixed_image_size((1440, 1920), (1272, 1696)), (1272, 1696), "同比例的大图缩到画布");
    assert_eq!(fixed_image_size((2544, 1696), (1272, 1696)), (1272, 848), "跨页按宽缩进画布");
    assert_eq!(fixed_image_size((300, 200), (1272, 1696)), (300, 200), "小图不放大");
    assert_eq!(fixed_image_size((400, 3392), (1272, 1696)), (200, 1696), "窄长图按高缩");
    assert_eq!(fixed_image_size((0, 0), (1272, 1696)), (1272, 1696));
    assert_eq!(fixed_image_size((300, 200), (0, 0)), (300, 200));
}

#[test]
fn fixed_layout_comic_like_amazon() {
    let mut jpeg = Vec::new();
    image::codecs::jpeg::JpegEncoder::new(&mut jpeg).encode_image(&image::GrayImage::from_pixel(1272, 1696, image::Luma([128]))).unwrap();
    let mut w = bookconv::epubzip::EpubWriter::new(std::io::Cursor::new(Vec::new())).unwrap();
    w.put("META-INF/container.xml", br#"<?xml version="1.0"?><container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container"><rootfiles><rootfile full-path="c.opf" media-type="application/oebps-package+xml"/></rootfiles></container>"#).unwrap();
    w.put("c.opf", br#"<?xml version="1.0"?><package xmlns="http://www.idpf.org/2007/opf" version="3.0" unique-identifier="id"><metadata xmlns:dc="http://purl.org/dc/elements/1.1/"><dc:identifier id="id">x</dc:identifier><dc:title>t</dc:title><dc:language>zh</dc:language><meta name="fixed-layout" content="true"/><meta name="original-resolution" content="1272x1696"/><meta name="orientation-lock" content="portrait"/></metadata><manifest><item id="i" href="i.jpg" media-type="image/jpeg"/><item id="p1" href="p1.xhtml" media-type="application/xhtml+xml"/><item id="p2" href="p2.xhtml" media-type="application/xhtml+xml"/></manifest><spine page-progression-direction="rtl"><itemref idref="p1"/><itemref idref="p2"/></spine></package>"#).unwrap();
    w.put("i.jpg", &jpeg).unwrap();
    for p in ["p1.xhtml", "p2.xhtml"] {
        w.put(p, br#"<html xmlns="http://www.w3.org/1999/xhtml"><body><div><img src="i.jpg" style="width:1272px;height:1696px"/></div></body></html>"#).unwrap();
    }
    let epub = w.finish().unwrap().into_inner();
    let (kfx, _) = epub_to_kfx(&epub, &Opts { fixed_id: Some(1), ..Default::default() }).unwrap();
    let c = crate::container::Container::parse(&kfx).unwrap();
    let dump = |ty: u32| format!("{:?}", c.entities.iter().filter(|e| e.ty == ty).map(|e| e.value().unwrap().clone()).collect::<Vec<_>>());
    let meta = dump(T_METADATA);
    assert!(meta.contains(r#"String("yj_fixed_layout")), (307, Int(1))"#) && meta.contains(r#"String("book_orientation_lock")), (307, String("portrait"))"#), "{meta}");
    let doc = dump(T_DOCUMENT_DATA);
    assert!(doc.contains(&format!("({DOC_FIXED}, Symbol({DOC_FIXED_VALUE}))")) && doc.contains(&format!("({DOC_DIRECTION}, Symbol({DIR_RTL}))")), "{doc}");
    let secs = dump(T_SECTION);
    assert_eq!(secs.matches(&format!("({TMPL_WIDTH}, Int(1272)), ({TMPL_HEIGHT}, Int(1696)), ({FIXED_PAGE_FIT}, Symbol({FIXED_PAGE_FIT_VALUE}))")).count(), 2, "{secs}");
    assert!(dump(T_STYLE).contains(&format!("({P_WIDTH}, F64(1272.0)), ({P_HEIGHT}, F64(1696.0))")), "{}", dump(T_STYLE));
    // 没有实体的 id 排在资源路径的符号后面（否则 Kindle 上比画布小的图有的页空白）
    let syms = c.symbols();
    let max_loc = c.entities.iter().filter(|e| e.ty == T_RESOURCE).filter_map(|e| e.value()?.field(RES_LOCATION)?.as_str().and_then(|l| syms.sid(l))).max().unwrap();
    let max_ent = c.entities.iter().map(|e| e.id).max().unwrap();
    assert!(max_ent < max_loc, "最大实体 id {max_ent} 不能排在资源路径符号 {max_loc} 后面");
}

/// 没嵌入的正文字体不写原名（Kindle 会换成别的字体，和表格、阅读器设置都对不上），写 `default`（写出器 11）；嵌入的装饰字体照写。
#[test]
fn unembedded_body_font_dropped() {
    let mut w = bookconv::epubzip::EpubWriter::new(std::io::Cursor::new(Vec::new())).unwrap();
    w.put("META-INF/container.xml", br#"<?xml version="1.0"?><container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container"><rootfiles><rootfile full-path="c.opf" media-type="application/oebps-package+xml"/></rootfiles></container>"#).unwrap();
    w.put("c.opf", br#"<?xml version="1.0"?><package xmlns="http://www.idpf.org/2007/opf" version="3.0" unique-identifier="id"><metadata xmlns:dc="http://purl.org/dc/elements/1.1/"><dc:identifier id="id">x</dc:identifier><dc:title>t</dc:title><dc:language>zh</dc:language></metadata><manifest><item id="c1" href="c1.xhtml" media-type="application/xhtml+xml"/></manifest><spine><itemref idref="c1"/></spine></package>"#).unwrap();
    w.put("c1.xhtml", r#"<html xmlns="http://www.w3.org/1999/xhtml"><head><style>p{font-family:"AR MingU30"} h1{font-family:"黑体"}</style></head><body><h1>标题</h1><p>很长很长的正文一</p><p>很长很长的正文二</p><table><tr><td>表格</td></tr></table></body></html>"#.as_bytes()).unwrap();
    let epub = w.finish().unwrap().into_inner();
    let (kfx, _) = epub_to_kfx(&epub, &Opts { fixed_id: Some(1), ..Default::default() }).unwrap();
    let c = crate::container::Container::parse(&kfx).unwrap();
    let styles = format!("{:?}", c.entities.iter().filter(|e| e.ty == T_STYLE).map(|e| e.value().unwrap().clone()).collect::<Vec<_>>());
    assert!(!styles.contains("AR MingU30") && styles.contains("黑体"), "{styles}");
}

/// 背景图：同《绍宋》样本，写在包住整页的容器上（`$479` 指向资源、不重复、固定、位置、尺寸）；`url()` 按样式表路径解析。
#[test]
fn background_image_like_amazon() {
    let mut png = Vec::new();
    image::GrayImage::from_pixel(4, 4, image::Luma([0])).write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png).unwrap();
    let mut w = bookconv::epubzip::EpubWriter::new(std::io::Cursor::new(Vec::new())).unwrap();
    w.put("META-INF/container.xml", br#"<?xml version="1.0"?><container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container"><rootfiles><rootfile full-path="O/c.opf" media-type="application/oebps-package+xml"/></rootfiles></container>"#).unwrap();
    w.put("O/c.opf", br#"<?xml version="1.0"?><package xmlns="http://www.idpf.org/2007/opf" version="3.0" unique-identifier="id"><metadata xmlns:dc="http://purl.org/dc/elements/1.1/"><dc:identifier id="id">x</dc:identifier><dc:title>t</dc:title><dc:language>zh</dc:language></metadata><manifest><item id="s" href="S/a.css" media-type="text/css"/><item id="b" href="I/bg.png" media-type="image/png"/><item id="c1" href="T/c1.xhtml" media-type="application/xhtml+xml"/></manifest><spine><itemref idref="c1"/></spine></package>"#).unwrap();
    w.put("O/S/a.css", br#"body.j{background:url("../I/bg.png") bottom / cover no-repeat fixed rgba(117, 0, 0, 1)}"#).unwrap();
    w.put("O/I/bg.png", &png).unwrap();
    w.put("O/T/c1.xhtml", br#"<html xmlns="http://www.w3.org/1999/xhtml"><head><link rel="stylesheet" href="../S/a.css"/></head><body class="j"><p>x</p></body></html>"#).unwrap();
    let epub = w.finish().unwrap().into_inner();
    let (kfx, warnings) = epub_to_kfx(&epub, &Opts { fixed_id: Some(1), ..Default::default() }).unwrap();
    assert!(warnings.is_empty(), "{warnings:?}");
    let c = crate::container::Container::parse(&kfx).unwrap();
    let styles = format!("{:?}", c.entities.iter().filter(|e| e.ty == T_STYLE).map(|e| e.value().unwrap().clone()).collect::<Vec<_>>());
    assert!(styles.contains(&format!("({P_BG_IMAGE}, Symbol(")) && styles.contains(&format!("({P_BG_REPEAT}, Symbol({BG_NO_REPEAT}))")) && styles.contains(&format!("({P_BG_ATTACHMENT}, Symbol({BG_FIXED}))")), "{styles}");
    assert!(styles.contains(&format!("({P_BG_SIZE_H}, ")) && styles.contains(&format!("({P_BACKGROUND}, Int({}))", 0xFF750000u32)), "{styles}");
    assert!(c.entities.iter().any(|e| e.ty == T_RESOURCE), "背景图登记成资源");
    // `cover` 的页面背景：容器节点写整页范围 `$645`（同 Send to Kindle《绍宋》卷首语）
    let story = format!("{:?}", c.entities.iter().filter(|e| e.ty == T_STORYLINE).map(|e| e.value().unwrap().clone()).collect::<Vec<_>>());
    assert!(story.contains(&format!("({BG_PAGE_BOUNDS}, Struct(")), "{story}");
}

/// SVG 包着的封面图（`<svg><image xlink:href>`，属性在 xlink 命名空间）写成整页图片版面（2026-10-08：以前找不到属性，封面页丢掉）。
#[test]
fn svg_wrapped_cover_kept() {
    let mut png = Vec::new();
    image::GrayImage::from_pixel(4, 4, image::Luma([0])).write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png).unwrap();
    let mut w = bookconv::epubzip::EpubWriter::new(std::io::Cursor::new(Vec::new())).unwrap();
    w.put("META-INF/container.xml", br#"<?xml version="1.0"?><container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container"><rootfiles><rootfile full-path="O/c.opf" media-type="application/oebps-package+xml"/></rootfiles></container>"#).unwrap();
    w.put("O/c.opf", br#"<?xml version="1.0"?><package xmlns="http://www.idpf.org/2007/opf" version="3.0" unique-identifier="id"><metadata xmlns:dc="http://purl.org/dc/elements/1.1/"><dc:identifier id="id">x</dc:identifier><dc:title>t</dc:title><dc:language>zh</dc:language></metadata><manifest><item id="b" href="I/c.png" media-type="image/png" properties="cover-image"/><item id="c0" href="T/cover.xhtml" media-type="application/xhtml+xml"/><item id="c1" href="T/c1.xhtml" media-type="application/xhtml+xml"/></manifest><spine><itemref idref="c0"/><itemref idref="c1"/></spine></package>"#).unwrap();
    w.put("O/I/c.png", &png).unwrap();
    w.put("O/T/cover.xhtml", br#"<?xml version="1.0" encoding="utf-8"?><html xmlns="http://www.w3.org/1999/xhtml"><head><title>Cover</title></head><body><div style="text-align:center"><svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink" viewBox="0 0 4 4" width="100%" height="100%"><image width="4" height="4" xlink:href="../I/c.png"/></svg></div></body></html>"#).unwrap();
    w.put("O/T/c1.xhtml", br#"<html xmlns="http://www.w3.org/1999/xhtml"><body><p>x</p></body></html>"#).unwrap();
    let epub = w.finish().unwrap().into_inner();
    let (kfx, warnings) = epub_to_kfx(&epub, &Opts { fixed_id: Some(1), ..Default::default() }).unwrap();
    assert!(warnings.is_empty(), "{warnings:?}");
    let c = crate::container::Container::parse(&kfx).unwrap();
    let story = format!("{:?}", c.entities.iter().filter(|e| e.ty == T_STORYLINE).map(|e| e.value().unwrap().clone()).collect::<Vec<_>>());
    assert!(story.contains(&format!("({NODE_TYPE}, Symbol({NODE_IMAGE}))")), "封面页的图片节点: {story}");
    assert_eq!(c.entities.iter().filter(|e| e.ty == T_SECTION).count(), 2, "封面页成了一个版面");
}

/// spine 里标了 `linear="no"` 的目录页（导航文档）不排（写出器 14，用户定三台都拿掉）。
#[test]
fn nonlinear_nav_page_skipped() {
    let mut w = bookconv::epubzip::EpubWriter::new(std::io::Cursor::new(Vec::new())).unwrap();
    w.put("META-INF/container.xml", br#"<?xml version="1.0"?><container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container"><rootfiles><rootfile full-path="c.opf" media-type="application/oebps-package+xml"/></rootfiles></container>"#).unwrap();
    w.put("c.opf", br#"<?xml version="1.0"?><package xmlns="http://www.idpf.org/2007/opf" version="3.0" unique-identifier="id"><metadata xmlns:dc="http://purl.org/dc/elements/1.1/"><dc:identifier id="id">x</dc:identifier><dc:title>t</dc:title><dc:language>zh</dc:language></metadata><manifest><item id="nav" href="nav.xhtml" media-type="application/xhtml+xml" properties="nav"/><item id="c1" href="c1.xhtml" media-type="application/xhtml+xml"/></manifest><spine><itemref idref="nav" linear="no"/><itemref idref="c1"/></spine></package>"#).unwrap();
    w.put("nav.xhtml", r#"<html xmlns="http://www.w3.org/1999/xhtml" xmlns:epub="http://www.idpf.org/2007/ops"><body><nav epub:type="toc"><h1>目录页</h1><ol><li><a href="c1.xhtml">第一章</a></li></ol></nav></body></html>"#.as_bytes()).unwrap();
    w.put("c1.xhtml", r#"<html xmlns="http://www.w3.org/1999/xhtml"><body><h1>第一章</h1><p>正文</p></body></html>"#.as_bytes()).unwrap();
    let epub = w.finish().unwrap().into_inner();
    let (kfx, _) = epub_to_kfx(&epub, &Opts { fixed_id: Some(1), ..Default::default() }).unwrap();
    let c = crate::container::Container::parse(&kfx).unwrap();
    let pools = format!("{:?}", c.entities.iter().filter(|e| e.ty == T_TEXT_POOL).map(|e| e.value().unwrap().clone()).collect::<Vec<_>>());
    assert!(!pools.contains("目录页") && pools.contains("正文"), "{pools}");
    assert_eq!(c.entities.iter().filter(|e| e.ty == T_SECTION).count(), 1);
    let nav = format!("{:?}", c.entities.iter().filter(|e| e.ty == T_NAV_CONTAINER).map(|e| e.value().unwrap().clone()).collect::<Vec<_>>());
    assert!(nav.contains("第一章"), "目录照常：{nav}");
}

/// spine 里没有封面页、正文没用到封面图：最前面补一页整页封面（写出器 13，照 Send to Kindle《绍宋》《狼厅》）。
#[test]
fn missing_cover_page_prepended() {
    let mut png = Vec::new();
    image::GrayImage::from_pixel(4, 4, image::Luma([0])).write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png).unwrap();
    let mut w = bookconv::epubzip::EpubWriter::new(std::io::Cursor::new(Vec::new())).unwrap();
    w.put("META-INF/container.xml", br#"<?xml version="1.0"?><container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container"><rootfiles><rootfile full-path="O/c.opf" media-type="application/oebps-package+xml"/></rootfiles></container>"#).unwrap();
    w.put("O/c.opf", br#"<?xml version="1.0"?><package xmlns="http://www.idpf.org/2007/opf" version="3.0" unique-identifier="id"><metadata xmlns:dc="http://purl.org/dc/elements/1.1/"><dc:identifier id="id">x</dc:identifier><dc:title>t</dc:title><dc:language>zh</dc:language></metadata><manifest><item id="b" href="I/c.png" media-type="image/png" properties="cover-image"/><item id="c1" href="T/c1.xhtml" media-type="application/xhtml+xml"/></manifest><spine><itemref idref="c1"/></spine></package>"#).unwrap();
    w.put("O/I/c.png", &png).unwrap();
    w.put("O/T/c1.xhtml", br#"<html xmlns="http://www.w3.org/1999/xhtml"><body><p>x</p></body></html>"#).unwrap();
    let epub = w.finish().unwrap().into_inner();
    let (kfx, warnings) = epub_to_kfx(&epub, &Opts { fixed_id: Some(1), ..Default::default() }).unwrap();
    assert!(warnings.is_empty(), "{warnings:?}");
    let c = crate::container::Container::parse(&kfx).unwrap();
    let sections: Vec<_> = c.entities.iter().filter(|e| e.ty == T_SECTION).map(|e| format!("{:?}", e.value().unwrap())).collect();
    assert_eq!(sections.len(), 2, "补了一个封面版面");
    let cover_sec = c.entities.iter().filter(|e| e.ty == T_SECTION).find(|e| format!("{:?}", e.value().unwrap()).contains(&format!("({TMPL_WIDTH}, Int(4))"))).expect("整页图片版面");
    // 阅读顺序 `{$169: [{$178, $170: [版面…]}]}` 的第一个版面
    let order = c.entities.iter().find(|e| e.ty == T_READING_ORDERS).unwrap().value().unwrap().clone();
    let field = |v: &Value, k: u32| match v {
        Value::Struct(f) => f.iter().find(|(n, _)| *n == k).map(|(_, v)| v.clone()),
        _ => None,
    };
    let Some(Value::List(orders)) = field(&order, READING_ORDERS) else { panic!("{order:?}") };
    let Some(Value::List(secs)) = field(&orders[0], SECTIONS) else { panic!("{order:?}") };
    assert!(matches!(secs[0], Value::Symbol(s) if s == cover_sec.id), "封面版面排第一：{order:?}");
    let nav = format!("{:?}", c.entities.iter().filter(|e| e.ty == T_NAV_CONTAINER).map(|e| e.value().unwrap().clone()).collect::<Vec<_>>());
    assert!(nav.contains("cover-nav-unit"), "封面地标：{nav}");
}

/// 图片的百分比宽度写进样式（`$56`，单位百分比，同表格宽度）；别的单位不写（写出器 9）。
#[test]
fn image_percent_width() {
    let mut png = Vec::new();
    image::GrayImage::from_pixel(4, 8, image::Luma([0])).write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png).unwrap();
let page = |body: &str| format!(r#"<html xmlns="http://www.w3.org/1999/xhtml"><head><style>img.l{{width:40%}} img.e{{width:10em}}</style></head><body><p>甲</p>{body}<p>乙</p></body></html>"#);
    let styles_of = |body: &str| {
        let mut w2 = bookconv::epubzip::EpubWriter::new(std::io::Cursor::new(Vec::new())).unwrap();
        w2.put("META-INF/container.xml", br#"<?xml version="1.0"?><container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container"><rootfiles><rootfile full-path="c.opf" media-type="application/oebps-package+xml"/></rootfiles></container>"#).unwrap();
        w2.put("c.opf", br#"<?xml version="1.0"?><package xmlns="http://www.idpf.org/2007/opf" version="3.0" unique-identifier="id"><metadata xmlns:dc="http://purl.org/dc/elements/1.1/"><dc:identifier id="id">x</dc:identifier><dc:title>t</dc:title><dc:language>zh</dc:language></metadata><manifest><item id="i" href="i.png" media-type="image/png"/><item id="c1" href="c1.xhtml" media-type="application/xhtml+xml"/></manifest><spine><itemref idref="c1"/></spine></package>"#).unwrap();
        w2.put("i.png", &png).unwrap();
        w2.put("c1.xhtml", page(body).as_bytes()).unwrap();
        let epub = w2.finish().unwrap().into_inner();
        let (kfx, warnings) = epub_to_kfx(&epub, &Opts { fixed_id: Some(1), ..Default::default() }).unwrap();
        assert!(warnings.is_empty(), "{warnings:?}");
        let c = crate::container::Container::parse(&kfx).unwrap();
        format!("{:?}", c.entities.iter().filter(|e| e.ty == T_STYLE).map(|e| e.value().unwrap().clone()).collect::<Vec<_>>())
    };
    let want = |p: f64| format!("({P_WIDTH}, {:?})", num(p, U_PERCENT));
    assert!(styles_of(r#"<div><img src="i.png" style="width:61%"/></div>"#).contains(&want(61.0)), "行内百分比");
    assert!(styles_of(r#"<div><img class="l" src="i.png"/></div>"#).contains(&want(40.0)), "样式表百分比");
    assert!(!styles_of(r#"<div><img class="e" src="i.png"/></div>"#).contains(&format!("({P_WIDTH}, ")), "em 宽度不写");
    assert!(!styles_of(r#"<div><img src="i.png"/></div>"#).contains(&format!("({P_WIDTH}, ")), "没写宽度");
}

#[test]
fn pid_map_matches_sample_shape() {
    // 样本《ABC谋杀案》第 2 个版面：[[0,3090],[1,325],[1,40],[4,333],9,14,6,9,15,[22,0]]
    let order = [(3090, 1), (325, 1), (40, 4), (333, 9), (334, 14), (335, 6), (336, 9), (337, 15), (338, 22)];
    let m = pid_map(&order);
    let expect = Value::List(vec![
        Value::List(vec![Value::Int(0), Value::Int(3090)]),
        Value::List(vec![Value::Int(1), Value::Int(325)]),
        Value::List(vec![Value::Int(1), Value::Int(40)]),
        Value::List(vec![Value::Int(4), Value::Int(333)]),
        Value::Int(9),
        Value::Int(14),
        Value::Int(6),
        Value::Int(9),
        Value::Int(15),
        Value::List(vec![Value::Int(22), Value::Int(0)]),
    ]);
    assert_eq!(Value::List(m), expect);
}

#[test]
fn compress_ids_matches_sample_shape() {
    let ids = vec![3088, 31, 267, 268, 269, 270, 271];
    assert_eq!(
        compress_ids(ids),
        Value::List(vec![Value::Int(31), Value::List(vec![Value::Int(267), Value::Int(5)]), Value::Int(3088)])
    );
}

/// 没有文字的元素的 id 也是锚点：挂到文档顺序里下一个块的开头，文末的挂到最后一个块的末尾；图片自己的 id 也算。
/// 以前丢掉，链接退回文件开头（《福尔摩斯探案全集》目录页「第一册」点了停在目录页开头）。
#[test]
fn empty_element_ids_anchor_to_next_block() {
    let mut png = Vec::new();
    image::GrayImage::from_pixel(4, 4, image::Luma([0])).write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png).unwrap();
    let mut w = bookconv::epubzip::EpubWriter::new(std::io::Cursor::new(Vec::new())).unwrap();
    w.put("META-INF/container.xml", br#"<?xml version="1.0"?><container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container"><rootfiles><rootfile full-path="c.opf" media-type="application/oebps-package+xml"/></rootfiles></container>"#).unwrap();
    w.put("c.opf", br#"<?xml version="1.0"?><package xmlns="http://www.idpf.org/2007/opf" version="3.0" unique-identifier="id"><metadata xmlns:dc="http://purl.org/dc/elements/1.1/"><dc:identifier id="id">x</dc:identifier><dc:title>t</dc:title><dc:language>zh</dc:language></metadata><manifest><item id="i" href="i.png" media-type="image/png"/><item id="c1" href="c1.xhtml" media-type="application/xhtml+xml"/></manifest><spine><itemref idref="c1"/></spine></package>"#).unwrap();
    w.put("i.png", &png).unwrap();
    w.put("c1.xhtml", r##"<html xmlns="http://www.w3.org/1999/xhtml"><body><p><a href="#a">1</a><a href="#b">2</a><a href="#c">3</a><a href="#d">4</a></p><p>甲</p><p><span id="a"></span></p><p>乙</p><div id="b"></div><p><img id="c" src="i.png"/></p><p>丙丁</p><span id="d"></span></body></html>"##.as_bytes()).unwrap();
    let epub = w.finish().unwrap().into_inner();
    let (kfx, warnings) = epub_to_kfx(&epub, &Opts { fixed_id: Some(1), ..Default::default() }).unwrap();
    assert!(warnings.is_empty(), "{warnings:?}");
    let c = crate::container::Container::parse(&kfx).unwrap();
    // 节点 id：按文字找文字节点，图片节点按类型找
    let story = c.entities.iter().find(|e| e.ty == T_STORYLINE).unwrap().value().unwrap().field(CHILDREN).unwrap().as_list().unwrap().to_vec();
    let pool = c.entities.iter().find(|e| e.ty == T_TEXT_POOL).unwrap().value().unwrap().field(CHILDREN).unwrap().as_list().unwrap().to_vec();
    let eid_of_text = |t: &str| {
        let idx = pool.iter().position(|v| v.as_str() == Some(t)).unwrap() as i64;
        story.iter().find(|n| n.field(TEXT_REF).and_then(|r| r.field(TEXT_INDEX)).and_then(Value::as_int) == Some(idx)).unwrap().field(EID).unwrap().as_int().unwrap()
    };
    let img = story.iter().find(|n| n.field(NODE_TYPE) == Some(&Value::Symbol(NODE_IMAGE))).unwrap().field(EID).unwrap().as_int().unwrap();
    // 链接按出现顺序配锚点：anchor0..3 → #a #b #c #d
    let syms = c.symbols();
    let target = |name: &str| {
        let sid = syms.sid(name).unwrap();
        let a = c.entities.iter().find(|e| e.ty == T_ANCHOR && e.id == sid).unwrap().value().unwrap().field(ANCHOR_POSITION).unwrap().clone();
        (a.field(EID).unwrap().as_int().unwrap(), a.field(OFFSET).unwrap().as_int().unwrap())
    };
    assert_eq!(target("anchor0"), (eid_of_text("乙"), 0), "空段落里的 span → 下一段开头");
    assert_eq!(target("anchor1"), (img, 0), "空 div → 下一块（图片）");
    assert_eq!(target("anchor2"), (img, 0), "图片自己的 id");
    assert_eq!(target("anchor3"), (eid_of_text("丙丁"), 2), "文末的 → 最后一块末尾");
}

/// 取不到的图片上的 id 挂到下一个节点开头，文档末尾的挂到最后一个节点末尾（写出器 15；以前随图片丢掉，链接退回文件开头）。
#[test]
fn missing_image_ids_anchor_to_next_node() {
    let mut w = bookconv::epubzip::EpubWriter::new(std::io::Cursor::new(Vec::new())).unwrap();
    w.put("META-INF/container.xml", br#"<?xml version="1.0"?><container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container"><rootfiles><rootfile full-path="c.opf" media-type="application/oebps-package+xml"/></rootfiles></container>"#).unwrap();
    w.put("c.opf", br#"<?xml version="1.0"?><package xmlns="http://www.idpf.org/2007/opf" version="3.0" unique-identifier="id"><metadata xmlns:dc="http://purl.org/dc/elements/1.1/"><dc:identifier id="id">x</dc:identifier><dc:title>t</dc:title><dc:language>zh</dc:language></metadata><manifest><item id="c1" href="c1.xhtml" media-type="application/xhtml+xml"/></manifest><spine><itemref idref="c1"/></spine></package>"#).unwrap();
    w.put("c1.xhtml", r##"<html xmlns="http://www.w3.org/1999/xhtml"><body><p><a href="#a">1</a><a href="#b">2</a></p><p>甲</p><p><img id="a" src="none.png"/></p><p>乙</p><p>丙丁</p><img id="b" src="none.png"/></body></html>"##.as_bytes()).unwrap();
    let epub = w.finish().unwrap().into_inner();
    let (kfx, warnings) = epub_to_kfx(&epub, &Opts { fixed_id: Some(1), ..Default::default() }).unwrap();
    assert_eq!(warnings.iter().filter(|w| w.contains("none.png")).count(), 1, "{warnings:?}");
    assert!(!warnings.iter().any(|w| w.contains("链接目标找不到")), "{warnings:?}");
    let c = crate::container::Container::parse(&kfx).unwrap();
    let story = c.entities.iter().find(|e| e.ty == T_STORYLINE).unwrap().value().unwrap().field(CHILDREN).unwrap().as_list().unwrap().to_vec();
    let pool = c.entities.iter().find(|e| e.ty == T_TEXT_POOL).unwrap().value().unwrap().field(CHILDREN).unwrap().as_list().unwrap().to_vec();
    let eid_of_text = |t: &str| {
        let idx = pool.iter().position(|v| v.as_str() == Some(t)).unwrap() as i64;
        story.iter().find(|n| n.field(TEXT_REF).and_then(|r| r.field(TEXT_INDEX)).and_then(Value::as_int) == Some(idx)).unwrap().field(EID).unwrap().as_int().unwrap()
    };
    let syms = c.symbols();
    let target = |name: &str| {
        let sid = syms.sid(name).unwrap();
        let a = c.entities.iter().find(|e| e.ty == T_ANCHOR && e.id == sid).unwrap().value().unwrap().field(ANCHOR_POSITION).unwrap().clone();
        (a.field(EID).unwrap().as_int().unwrap(), a.field(OFFSET).unwrap().as_int().unwrap())
    };
    assert_eq!(target("anchor0"), (eid_of_text("乙"), 0), "取不到的图 → 下一段开头");
    assert_eq!(target("anchor1"), (eid_of_text("丙丁"), 2), "文末取不到的图 → 最后一段末尾");
}

/// 一个文档的 body 子节点解析成块（不走整本书）。
fn body_blocks(body: &str) -> Vec<Block> {
    let html = Html::parse_document(&format!("<html><body>{body}</body></html>"));
    let d = Doc { path: "c.xhtml", sheet: Sheet::default(), lang: None, pending: Default::default(), gap: Default::default(), links: Default::default() };
    let body = html.select(&scraper::Selector::parse("body").unwrap()).next().unwrap();
    let comp = d.comp(&body, &Computed::root());
    let mut out = Vec::new();
    d.children(body, &comp, &mut out);
    out
}

/// 只有空白的段落不出节点，折成下一块上边距多出 0.6 × 本元素行高（Send to Kindle 同样）；只有普通空格的排不出行，不折。
#[test]
fn blank_paragraphs_fold_into_next_margin() {
    let mut blocks = body_blocks("<p>甲</p><p>&#160;</p><p>乙</p><p>　</p><p><br/></p><p>丙</p><p> </p><p>丁</p><p style=\"line-height:2\">&#160;</p><p>戊</p><p>&#160;</p>");
    collapse_siblings(&mut blocks);
    apply_gaps(&mut blocks);
    let tops: Vec<(String, f64)> = blocks
        .iter()
        .map(|b| match &b.kind {
            Kind::Text { text, .. } => (text.clone(), (b.margin_top * 1000.0).round() / 1000.0),
            _ => unreachable!(),
        })
        .collect();
    // <p> 上下 1em 折叠成 1em，再加折掉的空段：一个 0.72em（缺省行高 1.2），两个 1.44em，行高 2 的 1.2em；文末的不折
    assert_eq!(tops, [("甲".into(), 1.0), ("乙".into(), 1.72), ("丙".into(), 2.44), ("丁".into(), 1.0), ("戊".into(), 2.2)]);
}

type TextRuns = Vec<(String, Vec<(usize, usize, bool)>)>;

/// 文字块（深度优先）的 (文字, [(起点, 长度, 有没有链接)])；图片记成 `<img>`。
fn texts_and_runs(blocks: &[Block]) -> TextRuns {
    let mut out = Vec::new();
    for b in blocks {
        match &b.kind {
            Kind::Text { text, runs } => out.push((text.clone(), runs.iter().map(|r| (r.start, r.len, r.link.is_some())).collect())),
            Kind::Image { .. } => out.push(("<img>".to_string(), Vec::new())),
            Kind::Container(c) => out.extend(texts_and_runs(c)),
        }
    }
    out
}

/// 行内元素走到一半遇到块或图片（切开文字块）：区间在切开处截断，到新块从 0 接着开（以前按进来时的下标插进新缓冲：越界 panic、或区间错位）。
/// 链接里的块（`z`、`d`）写出器 15 起也带链接。
#[test]
fn inline_run_split_by_block_or_image() {
    let got = texts_and_runs(&body_blocks(r##"<div><b>x</b><a href="#n">y<div>z</div>wwwww</a></div>"##));
    assert_eq!(got, [("xy".into(), vec![(0, 1, false), (1, 1, true)]), ("z".into(), vec![(0, 1, true)]), ("wwwww".into(), vec![(0, 5, true)])]);
    let got = texts_and_runs(&body_blocks(r##"<p><b>x</b>t<a href="#n"><img src="i.png"/>see note one</a></p>"##));
    assert_eq!(got, [("xt".into(), vec![(0, 1, false)]), ("<img>".into(), vec![]), ("see note one".into(), vec![(0, 12, true)])]);
    // 不 panic 时以前区间错位到「www」上
    let got = texts_and_runs(&body_blocks(r##"<div>xx<a href="#n">y<div>z</div>wwwww</a></div>"##));
    assert_eq!(got, [("xxy".into(), vec![(2, 1, true)]), ("z".into(), vec![(0, 1, true)]), ("wwwww".into(), vec![(0, 5, true)])]);
    // 套着的两层都截断、都接着开，外层排在里层前面
    let got = texts_and_runs(&body_blocks(r##"<div>a<a href="#n">b<b>c<div>d</div>e</b>f</a></div>"##));
    assert_eq!(got, [("abc".into(), vec![(1, 2, true), (2, 1, false)]), ("d".into(), vec![(0, 1, true)]), ("ef".into(), vec![(0, 2, true), (0, 1, false)])]);
}

/// `<a href>` 里套着块级元素（`<a href><p>…</p></a>`）：块里的文字照样带链接（写出器 15；以前链接丢了）。
#[test]
fn link_around_block_kept() {
    let got = texts_and_runs(&body_blocks(r##"<div><a href="#n"><p>甲</p></a></div>"##));
    assert_eq!(got, [("甲".into(), vec![(0, 1, true)])]);
    let got = texts_and_runs(&body_blocks(r##"<div><a href="#n">前<div><p>中<b>粗</b></p></div>后</a>外</div>"##));
    assert_eq!(got, [("前".into(), vec![(0, 1, true)]), ("中粗".into(), vec![(0, 2, true), (1, 1, false)]), ("后外".into(), vec![(0, 1, true)])]);
    // 链接外面的块不受影响
    let got = texts_and_runs(&body_blocks(r##"<div><a href="#n">甲</a><p>乙</p></div>"##));
    assert_eq!(got, [("甲".into(), vec![(0, 1, true)]), ("乙".into(), vec![])]);
}

/// 行内元素前面待写出的空格不算进区间（写出器 15；`foo <a>bar</a>` 以前是 offset 3 len 4）；元素里自己开头的空格照旧算。
#[test]
fn run_starts_after_pending_space() {
    let got = texts_and_runs(&body_blocks(r##"<p>foo <a href="#n">bar</a></p>"##));
    assert_eq!(got, [("foo bar".into(), vec![(4, 3, true)])]);
    let got = texts_and_runs(&body_blocks(r##"<p>foo <b><a href="#n">bar</a></b> baz</p>"##));
    assert_eq!(got, [("foo bar baz".into(), vec![(4, 3, false), (4, 3, true)])]);
    let got = texts_and_runs(&body_blocks(r##"<p>foo<a href="#n"> bar</a></p>"##));
    assert_eq!(got, [("foo bar".into(), vec![(3, 4, true)])]);
    // `<br>` 吃掉了待写出的空格：区间从换行起
    let got = texts_and_runs(&body_blocks(r##"<p>foo <a href="#n"><br/>bar</a></p>"##));
    assert_eq!(got, [("foo\nbar".into(), vec![(3, 4, true)])]);
}

/// `<ul>`/`<ol>` 里直接放的文字、图片不丢（排成列表里的匿名块，不带符号）。
#[test]
fn list_direct_text_and_images_kept() {
    let blocks = body_blocks(r#"<ul>前言<li>甲</li>中间<li>乙</li><img src="i.png"/></ul>"#);
    assert_eq!(blocks.len(), 1);
    assert_eq!(blocks[0].ty, Some(NODE_LIST));
    let got: Vec<String> = texts_and_runs(&blocks).into_iter().map(|t| t.0).collect();
    assert_eq!(got, ["前言", "甲", "中间", "乙", "<img>"]);
    let Kind::Container(items) = &blocks[0].kind else { panic!() };
    let types: Vec<Option<u32>> = items.iter().map(|i| i.ty).collect();
    assert_eq!(types, [None, Some(NODE_LIST_ITEM), None, Some(NODE_LIST_ITEM), None]);
}

/// `<font size>` 的 `+n`/`-n` 饱和加减，不溢出（以前 `+2147483647` debug 下 panic、release 下回绕成最小字号）。
#[test]
fn font_size_attribute_saturates() {
    for (size, want) in [("+2147483647", "3rem"), ("-2147483647", "0.625rem"), ("+1", "1.125rem"), ("7", "3rem")] {
        let html = Html::parse_fragment(&format!(r#"<font size="{size}">x</font>"#));
        let el = html.select(&scraper::Selector::parse("font").unwrap()).next().unwrap();
        let mut decls = HashMap::new();
        crate::css::presentational_hints(&el, &mut decls);
        assert_eq!(decls.get("font-size").map(String::as_str), Some(want), "{size}");
    }
}

/// 行内 `style` 的 `url()` 按文档路径解析：子目录里文档的行内背景图找得到。
#[test]
fn inline_style_url_resolved_against_document() {
    let mut png = Vec::new();
    image::GrayImage::from_pixel(4, 4, image::Luma([0])).write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png).unwrap();
    let mut w = bookconv::epubzip::EpubWriter::new(std::io::Cursor::new(Vec::new())).unwrap();
    w.put("META-INF/container.xml", br#"<?xml version="1.0"?><container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container"><rootfiles><rootfile full-path="O/c.opf" media-type="application/oebps-package+xml"/></rootfiles></container>"#).unwrap();
    w.put("O/c.opf", br#"<?xml version="1.0"?><package xmlns="http://www.idpf.org/2007/opf" version="3.0" unique-identifier="id"><metadata xmlns:dc="http://purl.org/dc/elements/1.1/"><dc:identifier id="id">x</dc:identifier><dc:title>t</dc:title><dc:language>zh</dc:language><dc:date>2019</dc:date></metadata><manifest><item id="b" href="I/bg.png" media-type="image/png"/><item id="c1" href="T/c1.xhtml" media-type="application/xhtml+xml"/></manifest><spine><itemref idref="c1"/></spine></package>"#).unwrap();
    w.put("O/I/bg.png", &png).unwrap();
    w.put("O/T/c1.xhtml", br#"<html xmlns="http://www.w3.org/1999/xhtml"><body><div style="background-image:url('../I/bg.png')"><p>x</p></div></body></html>"#).unwrap();
    let epub = w.finish().unwrap().into_inner();
    let (kfx, warnings) = epub_to_kfx(&epub, &Opts { fixed_id: Some(1), ..Default::default() }).unwrap();
    assert!(warnings.is_empty(), "{warnings:?}");
    let c = crate::container::Container::parse(&kfx).unwrap();
    let styles = format!("{:?}", c.entities.iter().filter(|e| e.ty == T_STYLE).map(|e| e.value().unwrap().clone()).collect::<Vec<_>>());
    assert!(styles.contains(&format!("({P_BG_IMAGE}, Symbol(")) && c.entities.iter().any(|e| e.ty == T_RESOURCE), "{styles}");
    // 只写到年份的日期补成 1 月 1 日
    let meta = format!("{:?}", c.entities.iter().find(|e| e.ty == T_METADATA).unwrap().value().unwrap());
    assert!(meta.contains(r#"String("issue_date")), (307, String("2019-01-01"))"#), "{meta}");
}

/// 元数据 `issue_date`：能用的部分留下、按 `YYYY-MM-DD` 补齐（以前不足 10 个字符的整个换成 2000-01-01）。
#[test]
fn issue_date_keeps_usable_part() {
    for (d, want) in [
        ("2019-05-03T08:00:00Z", "2019-05-03"),
        ("2019-05-03", "2019-05-03"),
        ("2019-05", "2019-05-01"),
        ("2019", "2019-01-01"),
        (" 2019 ", "2019-01-01"),
        ("2019-5-3", "2019-01-01"),
        ("", "2000-01-01"),
        ("May 2019", "2000-01-01"),
        ("二〇一九年", "2000-01-01"),
    ] {
        assert_eq!(issue_date(d), want, "{d:?}");
    }
}

/// 固定版式的页图片取不到：同没有内容的文件，目录项改指下一页（以前没登记，目录项找不到位置）。
#[test]
fn fixed_layout_missing_image_page_registered_as_empty() {
    let mut jpeg = Vec::new();
    image::codecs::jpeg::JpegEncoder::new(&mut jpeg).encode_image(&image::GrayImage::from_pixel(8, 8, image::Luma([128]))).unwrap();
    let mut w = bookconv::epubzip::EpubWriter::new(std::io::Cursor::new(Vec::new())).unwrap();
    w.put("META-INF/container.xml", br#"<?xml version="1.0"?><container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container"><rootfiles><rootfile full-path="c.opf" media-type="application/oebps-package+xml"/></rootfiles></container>"#).unwrap();
    w.put("c.opf", br#"<?xml version="1.0"?><package xmlns="http://www.idpf.org/2007/opf" version="3.0" unique-identifier="id"><metadata xmlns:dc="http://purl.org/dc/elements/1.1/"><dc:identifier id="id">x</dc:identifier><dc:title>t</dc:title><dc:language>zh</dc:language><meta name="fixed-layout" content="true"/><meta name="original-resolution" content="8x8"/></metadata><manifest><item id="nav" href="nav.xhtml" media-type="application/xhtml+xml" properties="nav"/><item id="i" href="i.jpg" media-type="image/jpeg"/><item id="p1" href="p1.xhtml" media-type="application/xhtml+xml"/><item id="p2" href="p2.xhtml" media-type="application/xhtml+xml"/></manifest><spine><itemref idref="p1"/><itemref idref="p2"/></spine></package>"#).unwrap();
    w.put("nav.xhtml", br#"<html xmlns="http://www.w3.org/1999/xhtml" xmlns:epub="http://www.idpf.org/2007/ops"><body><nav epub:type="toc"><ol><li><a href="p1.xhtml">1</a></li><li><a href="p2.xhtml">2</a></li></ol></nav></body></html>"#).unwrap();
    w.put("i.jpg", &jpeg).unwrap();
    w.put("p1.xhtml", br#"<html xmlns="http://www.w3.org/1999/xhtml"><body><div><img src="missing.jpg"/></div></body></html>"#).unwrap();
    w.put("p2.xhtml", br#"<html xmlns="http://www.w3.org/1999/xhtml"><body><div><img src="i.jpg"/></div></body></html>"#).unwrap();
    let epub = w.finish().unwrap().into_inner();
    let (kfx, warnings) = epub_to_kfx(&epub, &Opts { fixed_id: Some(1), ..Default::default() }).unwrap();
    assert!(!warnings.iter().any(|w| w.contains("目录项找不到位置")), "{warnings:?}");
    let c = crate::container::Container::parse(&kfx).unwrap();
    let toc = c.entities.iter().find(|e| e.ty == T_NAV_CONTAINER && e.value().unwrap().field(NAV_TYPE) == Some(&Value::Symbol(NAV_TYPE_TOC))).unwrap();
    let targets: Vec<Value> = toc.value().unwrap().field(NAV_ENTRIES).unwrap().as_list().unwrap().iter().map(|e| e.field(NAV_TARGET).unwrap().clone()).collect();
    assert_eq!(targets.len(), 2);
    assert_eq!(targets[0], targets[1], "取不到图的第 1 页指到第 2 页");
}

/// 正文字体字数打平时结果确定（取字体名小的，没写字体的排最前），和 HashMap 的遍历顺序无关。
#[test]
fn body_font_tie_is_deterministic() {
    let block = |font: Option<&str>, text: &str| {
        let mut c = Computed::root();
        c.font_family = font.map(str::to_string);
        Block::anonymous(Kind::Text { text: text.into(), runs: Vec::new() }, c, Vec::new())
    };
    for _ in 0..20 {
        let docs: Vec<ParsedDoc> = vec![(0, "a", None, vec![block(Some("Zed"), "甲乙"), block(Some("Alpha"), "丙丁"), block(Some("Mid"), "戊")])];
        assert_eq!(TextCounts::of(&docs).body_font(), Some("alpha".to_string()));
        let docs: Vec<ParsedDoc> = vec![(0, "a", None, vec![block(Some("Zed"), "甲乙"), block(None, "丙丁")])];
        assert_eq!(TextCounts::of(&docs).body_font(), None);
    }
}

#[test]
fn non_finite_lengths_written_as_zero() {
    for v in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        assert_eq!(num(v, U_EM), num(0.0, U_EM));
    }
    assert_eq!(num(0.8333333333, U_LH), Value::Struct(vec![(VALUE, Value::F64(0.833333)), (UNIT, Value::Symbol(U_LH))]));
}

/// 好几个文档（走 `par_map` 的工作线程）、每个套 300 层没关的 `<div>`：不栈溢出、文字一个不少；套得超过上限的报错（不 panic、不中止）。
/// 同一张取不到的图引用几次只警告一次。
#[test]
fn deep_nesting_does_not_overflow() {
    let book = |n: usize, body: &str| {
        let mut w = bookconv::epubzip::EpubWriter::new(std::io::Cursor::new(Vec::new())).unwrap();
        w.put("META-INF/container.xml", br#"<?xml version="1.0"?><container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container"><rootfiles><rootfile full-path="c.opf" media-type="application/oebps-package+xml"/></rootfiles></container>"#).unwrap();
        let items: String = (0..n).map(|i| format!(r#"<item id="c{i}" href="c{i}.xhtml" media-type="application/xhtml+xml"/>"#)).collect();
        let spine: String = (0..n).map(|i| format!(r#"<itemref idref="c{i}"/>"#)).collect();
        w.put("c.opf", format!(r#"<?xml version="1.0"?><package xmlns="http://www.idpf.org/2007/opf" version="3.0" unique-identifier="id"><metadata xmlns:dc="http://purl.org/dc/elements/1.1/"><dc:identifier id="id">x</dc:identifier><dc:title>t</dc:title><dc:language>zh</dc:language></metadata><manifest>{items}</manifest><spine>{spine}</spine></package>"#).as_bytes()).unwrap();
        for i in 0..n {
            w.put(&format!("c{i}.xhtml"), format!(r#"<html xmlns="http://www.w3.org/1999/xhtml"><body>{body}</body></html>"#).replace("{i}", &i.to_string()).as_bytes()).unwrap();
        }
        w.finish().unwrap().into_inner()
    };
    // 带内边距的每层都是容器节点：块、版面、编码也套到将近上限（KFX 里套这么深，本仓库的解码器按上限 200 层不收，只核对不崩）
    let n = MAX_NESTING - 40;
    let boxed = format!("{}字{{i}}{}", "<div style=\"padding:1px\">".repeat(n), "</div>".repeat(n));
    assert!(!epub_to_kfx(&book(3, &boxed), &Opts::default()).unwrap().0.is_empty());
    assert!(epub_text_styles(std::io::Cursor::new(book(3, &boxed)), &Opts::default()).unwrap().contains("字2"));
    // 没有背景、边距的包裹层摊平，KFX 里不深
    let deep = format!("{}字{{i}}<p style=\"border:1px solid red\">框</p><img src=\"none.png\"/>{}", "<div>".repeat(300), "</div>".repeat(300));
    let (kfx, warnings) = epub_to_kfx(&book(20, &deep), &Opts { fixed_id: Some(1), ..Default::default() }).unwrap();
    assert_eq!(warnings.iter().filter(|w| w.contains("none.png")).count(), 1, "{warnings:?}");
    let c = crate::container::Container::parse(&kfx).unwrap();
    let texts: Vec<String> = c.entities.iter().filter(|e| e.ty == T_TEXT_POOL).flat_map(|e| e.value().unwrap().field(CHILDREN).unwrap().as_list().unwrap().iter().map(|v| v.as_str().unwrap().to_string()).collect::<Vec<_>>()).collect();
    assert_eq!(texts.len(), 40);
    for i in 0..20 {
        assert!(texts.contains(&format!("字{i}")), "{texts:?}");
    }
    // 文字样式也一样走得通
    assert!(epub_text_styles(std::io::Cursor::new(book(20, &deep)), &Opts::default()).unwrap().contains("字19"));
    let too_deep = format!("{}字", "<div>".repeat(MAX_NESTING + 10));
    let err = epub_to_kfx(&book(2, &too_deep), &Opts::default()).unwrap_err();
    assert!(err.contains("超过上限"), "{err}");
}

#[test]
fn margin_collapse() {
    assert_eq!(collapse(1.0, 1.5), 1.5);
    assert_eq!(collapse(-1.0, 2.0), 1.0);
    assert_eq!(collapse(-1.0, -2.0), -2.0);
}
