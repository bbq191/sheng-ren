//! 清洗层单测（原 `wash.rs` 内联的 `mod tests`；用例跨多个子模块，集中放这里）。
    use super::*;

    fn e(name: &str, data: &str) -> Entry {
        Entry { name: name.into(), data: data.as_bytes().to_vec() }
    }
    fn s(entries: &[Entry], name: &str) -> String {
        String::from_utf8(entries.iter().find(|e| e.name == name).unwrap().data.clone()).unwrap()
    }
    const OPF: &str = r#"<?xml version="1.0"?><package version="2.0"><metadata><dc:title>测试书</dc:title></metadata><manifest><item id="css" href="style.css" media-type="text/css"/><item id="dk" href="dkagent.css" media-type="text/css"/><item id="c1" href="c1.xhtml" media-type="application/xhtml+xml"/><item id="pb" href="pb.xhtml" media-type="application/xhtml+xml"/><item id="c2" href="c2.xhtml" media-type="application/xhtml+xml"/></manifest><spine><itemref idref="c1"/><itemref idref="pb"/><itemref idref="c2"/></spine></package>"#;

    #[test]
    fn decl_filter_and_spacing() {
        let f: Vec<String> = DEFAULT_FILTER_PROPS.iter().map(|s| s.to_string()).collect();
        assert_eq!(filter_decls("font-family:'A';color:#333;text-indent:1em;font:12px x", &f, Spacing::Keep), "color:#333;text-indent:1em;", "color 不再剥离（EPUB 线保留原书颜色）");
        assert_eq!(filter_decls("margin:1em 2em;padding-top:3px;padding-left:4px", &f, Spacing::Vertical), "margin:0 2em;padding-left:4px;");
        assert_eq!(filter_decls("margin:1em 2em 3em 4em", &f, Spacing::Vertical), "margin:0 2em 0 4em;");
        assert_eq!(filter_decls("margin:5pt", &f, Spacing::Vertical), "margin:0 5pt;");
        assert_eq!(filter_decls("margin:5pt;padding:2px;text-align:left", &f, Spacing::All), "text-align:left;");
        assert_eq!(filter_decls("line-height:1.5;height:100vh;min-height:90vh;height:2em", &f, Spacing::Keep), "height:2em;", "行高与 vh 高度剥掉");
        assert_eq!(filter_decls("font-family: &#39;A&#39;; text-indent:2em", &f, Spacing::Keep), "text-indent:2em;", "实体分号不截断");
        assert_eq!(selector_spacing("p.calibre1"), Spacing::Vertical);
        assert_eq!(selector_spacing("div > p"), Spacing::Vertical);
        assert_eq!(selector_spacing("body"), Spacing::All);
        assert_eq!(selector_spacing("@page"), Spacing::All);
        assert_eq!(selector_spacing(".calibre1"), Spacing::Keep);
        assert_eq!(selector_spacing("span, pre"), Spacing::Keep);
    }

    #[test]
    fn css_file_filter_keeps_font_face_and_media() {
        let o = WashOpts::default();
        let css = "@font-face{font-family:X;src:url(x.ttf)} body{margin:5pt;color:#333} p{margin:1em 0;text-align:justify;text-indent:0} @media print{ p{margin-top:2em} } .c{margin:1em}";
        let out = filter_css(css, &o);
        assert!(out.contains("@font-face{font-family:X;src:url(x.ttf)}"), "{out}");
        assert!(out.contains("body{color:#333;}"), "margin 因 Spacing::All 剥、color 保留(不再剥): {out}");
        assert!(out.contains(" p{margin:0 0;text-align:justify;text-indent:0;}"), "text-align 保留(不再剥): {out}");
        assert!(out.contains("p{}"), "media 内规则也处理(单独 margin-top 整条丢弃，与 color/text-align 剥离与否无关): {out}");
        assert!(out.contains(".c{margin:1em;}"), "类选择器不动: {out}");
        let k = WashOpts { keep_para_spacing: true, ..Default::default() };
        assert!(filter_css("p{margin:1em 0}", &k).contains("p{margin:1em 0;}"));
    }

    /// 真机《甲午：摇摆的战争》坐实：`.duokan-footnote-item{font-weight:bold}` 导致全书注释永远加粗，
    /// 用户报的是"跳转注释后字体不对"、追下去其实是加粗。2026-09-23 拍板：只剥**注释容器类**的字重
    /// （不碰正文语义加粗，§03av 原则②仍然有效），注释字号固定比正文小一档（相对单位）。
    #[test]
    fn footnote_container_selector_loses_bold_gains_smaller_relative_size() {
        let o = WashOpts::default();
        assert!(is_footnote_container_selector(".duokan-footnote-item"));
        assert!(is_footnote_container_selector(".footnotes"));
        assert!(is_footnote_container_selector(".eink-fnote"));
        assert!(!is_footnote_container_selector("p"), "普通正文选择器不该被当成注释容器");

        let css = ".duokan-footnote-item{margin:0 0.6em;font-weight:bold;text-align:justify}";
        let out = filter_css(css, &o);
        assert!(!out.contains("font-weight"), "注释容器类的字重必须剥: {out}");
        assert!(out.contains(&format!("font-size:{FOOTNOTE_FONT_SIZE}")), "注释容器类要补字号: {out}");
        assert!(out.contains("text-align:justify"), "其它声明不受影响: {out}");

        // 正文语义加粗不受影响（§03av 原则②）。
        let body = filter_css("p.emphasis{font-weight:bold;color:#333}", &o);
        assert!(body.contains("font-weight:bold"), "正文加粗必须保留: {body}");
    }

    #[test]
    fn strips_background_image_keeps_font_src() {
        let o = WashOpts::default();
        // 分卷页背景图（xochitl 平铺盖正文）应剥；@font-face 的 src:url 保留。
        let css = "@font-face{font-family:F;src:url(f.ttf)} body.fen{background:url(bg.png) no-repeat bottom center;background-size:100% auto;margin:0} .x{background-image:url(y.png);color:#333;text-indent:2em}";
        let out = filter_css(css, &o);
        assert!(!out.contains("bg.png") && !out.contains("y.png"), "背景图应剥: {out}");
        assert!(out.contains("f.ttf"), "@font-face src 保留: {out}");
        assert!(out.contains("text-indent:2em"), "非背景声明保留: {out}");
    }

    #[test]
    fn html_wash_filters_styles_and_collapses_dup_ids() {
        let o = WashOpts::default();
        let html = r#"<html><head><link rel="stylesheet" href="s.css"/></head><body style="margin:5pt"><p id="a" id="b" style="font-size:12px;margin-top:1em;margin-left:2em">x</p><div style="color:gray">y</div><style>p{color:#333;margin:1em}</style></body></html>"#;
        let (out, dups) = wash_html(html, &o);
        assert_eq!(dups, 1);
        assert!(out.contains(r#"<p id="a" style="margin-left:2em;">x</p>"#), "{out}");
        assert!(out.contains(r#"<div style="color:gray;">y</div>"#), "color 不再剥离、style 非空保留: {out}");
        assert!(out.contains("<body>"), "{out}");
        assert!(out.contains("<style>p{color:#333;margin:0 1em;}</style>"), "color 保留 + 1em 四边→上下归零左右保留: {out}");
        // wash_html 不再注入内联 <style>（排版规则改外链 css，由 wash_entries 注）——见 external_css_injected_and_linked。
        assert!(!out.contains(&format!(r#"class="{WASH_MARK}""#)), "不该再注入内联 eink-wash: {out}");
        // 旧版内联 eink-wash 块重洗时清掉
        let (cleaned, _) = wash_html(r#"<html><head><style class="eink-wash">p{text-indent:2em!important}</style></head><body><p>z</p></body></html>"#, &o);
        assert!(!cleaned.contains("eink-wash") && !cleaned.contains("text-indent"), "旧内联块未清: {cleaned}");
    }

    #[test]
    fn pseudo_drm_stripped_and_real_drm_rejected() {
        let mut v = vec![
            e("mimetype", "application/epub+zip"),
            e("META-INF/encryption.xml", r#"<encryption><EncryptedData><CipherData><CipherReference URI="dkagent.css"/></CipherData></EncryptedData></encryption>"#),
            e("content.opf", OPF),
            e("dkagent.css", "secret"),
            e("style.css", "p{}"),
            e("c1.xhtml", "<html><body><p>a</p></body></html>"),
            e("pb.xhtml", "<html><body><p>b</p></body></html>"),
            e("c2.xhtml", "<html><body><p>c</p></body></html>"),
        ];
        let rep = wash_entries(&mut v, &WashOpts::default()).unwrap();
        assert_eq!(rep.pseudo_drm_stripped, vec!["dkagent.css"]);
        assert!(!v.iter().any(|x| x.name == "dkagent.css" || x.name == "META-INF/encryption.xml"));
        assert!(!s(&v, "content.opf").contains("dkagent.css"), "manifest 项删除");
        let mut real = vec![e("META-INF/encryption.xml", r#"<CipherReference URI="OEBPS/c1.xhtml"/><CipherReference URI="a.ttf"/>"#), e("OEBPS/c1.xhtml", "")];
        let err = wash_entries(&mut real, &WashOpts::default()).unwrap_err();
        assert!(err.contains("真 DRM") && err.contains("OEBPS/c1.xhtml"), "{err}");
    }

    #[test]
    fn drop_opf_refs_removes_only_listed_ids_in_one_pass() {
        // 一遍扫描删多个页的 item/itemref：带空闭合标签/属性顺序不同/相邻空白都要处理干净，不误伤 id 相近的项（p1 vs p10、idref 别名）。
        let opf = concat!(
            "<manifest>\n<item id=\"p1\" href=\"p1.xhtml\"/>\n<item href=\"p10.xhtml\" id=\"p10\" media-type=\"x\"></item>\n",
            "<item id=\"p2\" href=\"p2.xhtml\"/>\n<item id=\"img\" href=\"a.png\"/></manifest>\n",
            "<spine>\n<itemref idref=\"p1\"/>\n<itemref linear=\"yes\" idref=\"p10\"></itemref>\n<itemref idref=\"p2\"/>\n</spine>"
        );
        let ids: HashSet<&str> = ["p1", "p10"].into_iter().collect();
        let out = opf::remove_items(opf, |it| ids.contains(it.id)).unwrap();
        assert!(!out.contains("p1.xhtml") && !out.contains("p10.xhtml") && !out.contains("idref=\"p1\"") && !out.contains("idref=\"p10\""), "{out}");
        assert!(out.contains("<item id=\"p2\" href=\"p2.xhtml\"/>") && out.contains("<itemref idref=\"p2\"/>") && out.contains("a.png"), "无关项原样: {out}");
        assert_eq!(opf::remove_items(opf, |_| false), None, "空集合 = 不改");
    }

    #[test]
    fn empty_page_removed_and_toc_retargeted() {
        let mut v = vec![
            e("content.opf", OPF),
            e("style.css", ""),
            e("dkagent.css", ""),
            e("toc.ncx", r#"<ncx><navMap><navPoint><content src="c1.xhtml"/></navPoint><navPoint><content src="pb.xhtml#x"/></navPoint></navMap></ncx>"#),
            e("c1.xhtml", "<html><body><p>a</p></body></html>"),
            e("pb.xhtml", r#"<html><body><div class="mbppagebreak"></div>&nbsp;</body></html>"#),
            e("c2.xhtml", "<html><body><p>c</p></body></html>"),
        ];
        let rep = wash_entries(&mut v, &WashOpts { auto_toc: AutoToc::Off, ..Default::default() }).unwrap();
        assert_eq!(rep.empty_pages_removed, vec!["pb.xhtml"]);
        assert!(!v.iter().any(|x| x.name == "pb.xhtml"));
        let opf = s(&v, "content.opf");
        assert!(!opf.contains(r#"idref="pb""#) && !opf.contains(r#"id="pb""#), "{opf}");
        assert!(s(&v, "toc.ncx").contains(r#"src="c2.xhtml""#), "指向空页的目录改指下一篇: {}", s(&v, "toc.ncx"));
        // 有图的页不算空
        assert!(!is_empty_page(r#"<html><body><img src="a.png"/></body></html>"#));
    }

    /// 有 `<body>` 没有 `</body>` 的截断页不是空页，整页留着（v51；以前判成空页、正文整页被删）。
    #[test]
    fn truncated_page_without_body_close_kept() {
        let mut v = vec![
            e("content.opf", OPF),
            e("style.css", ""),
            e("dkagent.css", ""),
            e("toc.ncx", r#"<ncx><navMap><navPoint><content src="c1.xhtml"/></navPoint><navPoint><content src="pb.xhtml"/></navPoint></navMap></ncx>"#),
            e("c1.xhtml", "<html><body><p>a</p></body></html>"),
            e("pb.xhtml", "<html><body><p>截断的正文"),
            e("c2.xhtml", "<html><body><p>c</p></body></html>"),
        ];
        let rep = wash_entries(&mut v, &WashOpts { auto_toc: AutoToc::Off, ..Default::default() }).unwrap();
        assert!(rep.empty_pages_removed.is_empty(), "{:?}", rep.empty_pages_removed);
        assert!(v.iter().any(|x| x.name == "pb.xhtml"));
        assert!(s(&v, "content.opf").contains(r#"idref="pb""#));
    }

    fn cover_book(meta: &str) -> Vec<Entry> {
        let opf = format!(r#"<package version="2.0"><metadata><dc:title>书</dc:title>{meta}</metadata><manifest><item id="p1" href="Text/p1.xhtml" media-type="application/xhtml+xml"/><item id="img1" href="Images/001.jpg" media-type="image/jpeg"/><item id="cover.txt" href="cover.txt" media-type="text/plain"/></manifest><spine><itemref idref="p1"/></spine></package>"#);
        vec![
            e("content.opf", &opf),
            e("Text/p1.xhtml", r#"<html><body><img src="../Images/001.jpg"/></body></html>"#),
            Entry { name: "Images/001.jpg".into(), data: Vec::new() },
            e("cover.txt", "not an image"),
        ]
    }

    #[test]
    fn ensure_cover_adds_declaration_from_first_spine_page_image() {
        let mut v = cover_book("");
        assert!(ensure_cover_declared(&mut v));
        let opf = s(&v, "content.opf");
        assert!(opf.contains(r#"<meta name="cover" content="img1"/>"#), "{opf}");
        assert!(opf.contains(r#"id="img1" href="Images/001.jpg" media-type="image/jpeg" properties="cover-image""#) || opf.contains("cover-image"), "{opf}");
        assert!(!ensure_cover_declared(&mut v), "已有有效声明，第二次不该再动（幂等）");
    }

    /// 原书只在 OPF 声明了封面图、哪一页都没用到：spine 最前面补一页（照 Send to Kindle，《绍宋》《狼厅》）；用到了的不补，
    /// 文件名里有 `:` 的页面引用也认得（《春雪》：清洗改名之前就要判断）。
    #[test]
    fn prepend_cover_page_only_when_no_page_uses_cover() {
        let book = |page: &str| {
            vec![
                e("O/content.opf", r#"<package version="3.0"><metadata><dc:title>书</dc:title></metadata><manifest><item id="nav" href="nav.xhtml" media-type="application/xhtml+xml" properties="nav"/><item id="c" href="I/**%3A%3Ac.png" media-type="image/png" properties="cover-image"/><item id="p1" href="T/p1.xhtml" media-type="application/xhtml+xml"/></manifest><spine><itemref idref="nav" linear="no"/><itemref idref="p1"/></spine></package>"#),
                e("O/T/p1.xhtml", page),
                e("O/nav.xhtml", "<html><body><nav/></body></html>"),
                Entry { name: "O/I/**::c.png".into(), data: Vec::new() },
            ]
        };
        let mut v = book("<html><body><p>正文</p></body></html>");
        let mut asked = None;
        assert!(prepend_cover_page(&mut v, |p| {
            asked = Some(p.to_string());
            Some((600, 800))
        }));
        assert_eq!(asked.as_deref(), Some("O/I/**::c.png"));
        let opf = s(&v, "O/content.opf");
        assert!(opf.contains(r#"<spine><itemref idref="eink-cover-page"/><itemref idref="nav" linear="no"/>"#), "补在 spine 最前：{opf}");
        assert!(opf.contains(r#"<item id="eink-cover-page" href="eink-cover.xhtml" media-type="application/xhtml+xml" properties="svg"/>"#), "{opf}");
        let page = s(&v, "O/eink-cover.xhtml");
        assert!(page.contains(r#"viewBox="0 0 600 800""#) && page.contains(r#"xlink:href="I/**%3A%3Ac.png""#), "{page}");
        assert!(normalize::well_formed_xml(&page), "{page}");
        assert!(!prepend_cover_page(&mut v, |_| Some((1, 1))), "补过了不再补");

        let mut v = book(r#"<html><body><p><img src="../I/**::c.png"/></p></body></html>"#);
        assert!(!prepend_cover_page(&mut v, |_| Some((600, 800))), "正文用到了封面图（文件名带冒号）不补");
        let mut v = book("<html><body><p>正文</p></body></html>");
        assert!(!prepend_cover_page(&mut v, |_| None), "读不出宽高不补");
    }

    #[test]
    fn ensure_cover_repairs_declaration_pointing_to_non_image() {
        // Calibre 产物：<meta name="cover" content="cover.txt"/> 指向 txt——xochitl 取不到封面（真机日志 null cover image）
        let mut v = cover_book(r#"<meta name="cover" content="cover.txt"/>"#);
        assert!(ensure_cover_declared(&mut v));
        let opf = s(&v, "content.opf");
        assert!(opf.contains(r#"<meta name="cover" content="img1"/>"#) && !opf.contains(r#"content="cover.txt""#), "{opf}");
    }

    #[test]
    fn ensure_cover_finds_svg_titlepage_image_before_page_referencing_bad_file() {
        // 《镖人(卷四)》：spine 首页是只含 SVG <image> 的 titlepage（真封面），第二页 cover.xhtml 的 <img> 指向坏文件 cover.txt。
        let opf = r#"<package version="2.0"><metadata><meta name="cover" content="cover.txt"/></metadata><manifest><item id="titlepage" href="titlepage.xhtml" media-type="application/xhtml+xml"/><item id="cover.xhtml" href="EPUB/xhtml/cover.xhtml" media-type="application/xhtml+xml"/><item id="cover.txt" href="EPUB/images/cover.txt" media-type="application/xhtml+xml"/><item id="image_000.jpg" href="EPUB/images/image_000.jpg" media-type="image/jpeg"/></manifest><spine><itemref idref="titlepage"/><itemref idref="cover.xhtml"/></spine></package>"#;
        let mut v = vec![
            e("content.opf", opf),
            e("titlepage.xhtml", r#"<html><body><svg xmlns:xlink="x"><image width="600" xlink:href="EPUB/images/image_000.jpg"/></svg></body></html>"#),
            e("EPUB/xhtml/cover.xhtml", r#"<html><body><img src="../images/cover.txt"/></body></html>"#),
            e("EPUB/images/cover.txt", "<?xml not an image"),
            Entry { name: "EPUB/images/image_000.jpg".into(), data: Vec::new() },
        ];
        assert!(ensure_cover_declared(&mut v));
        let o = s(&v, "content.opf");
        assert!(o.contains(r#"<meta name="cover" content="image_000.jpg"/>"#) && !o.contains(r#"content="cover.txt""#), "{o}");
    }

    #[test]
    fn ensure_cover_falls_back_to_first_real_image_when_cover_file_itself_is_broken() {
        // 封面页引用的图片文件本身是坏的（txt 残片）：跳过，兜底用后面页面里第一张真实图片。
        let opf = r#"<package version="2.0"><metadata><meta name="cover" content="cover.txt"/></metadata><manifest><item id="titlepage" href="titlepage.xhtml" media-type="application/xhtml+xml"/><item id="p2" href="p2.xhtml" media-type="application/xhtml+xml"/><item id="cover.txt" href="images/cover.txt" media-type="application/xhtml+xml"/><item id="image_000.jpg" href="images/image_000.jpg" media-type="image/jpeg"/></manifest><spine><itemref idref="titlepage"/><itemref idref="p2"/></spine></package>"#;
        let mut v = vec![
            e("content.opf", opf),
            e("titlepage.xhtml", r#"<html><body><svg><image xlink:href="images/cover.txt"/></svg></body></html>"#),
            e("p2.xhtml", r#"<html><body><img src="images/image_000.jpg"/></body></html>"#),
            e("images/cover.txt", "<?xml"),
            Entry { name: "images/image_000.jpg".into(), data: Vec::new() },
        ];
        assert!(ensure_cover_declared(&mut v));
        assert!(s(&v, "content.opf").contains(r#"<meta name="cover" content="image_000.jpg"/>"#));
    }

    #[test]
    fn ensure_cover_adds_property_when_meta_is_valid_but_item_lacks_cover_image() {
        // 火影 09：meta 有效、条目 id 带点（x00000001.jpg），但没有 properties="cover-image"——真机对照实验证明 xochitl 因此取不到封面。
        let mut v = cover_book(r#"<meta name="cover" content="img1"/>"#);
        assert!(ensure_cover_declared(&mut v), "meta 有效但缺属性也要补");
        assert!(s(&v, "content.opf").contains(r#"properties="cover-image""#));
        assert!(!ensure_cover_declared(&mut v), "补完幂等");
    }

    #[test]
    fn ensure_cover_leaves_fully_valid_declaration_and_unfindable_alone() {
        let mut ok = cover_book(r#"<meta name="cover" content="img1"/>"#);
        ok[0] = e("content.opf", &s(&ok, "content.opf").replace(r#"<item id="img1" href="Images/001.jpg" media-type="image/jpeg"/>"#, r#"<item id="img1" href="Images/001.jpg" media-type="image/jpeg" properties="cover-image"/>"#));
        let before = s(&ok, "content.opf");
        assert!(!ensure_cover_declared(&mut ok));
        assert_eq!(s(&ok, "content.opf"), before);
        // 第一页没有图：没有候选，不乱猜
        let mut none = cover_book("");
        none[1] = e("Text/p1.xhtml", "<html><body><p>纯文字</p></body></html>");
        assert!(!ensure_cover_declared(&mut none));
    }

    #[test]
    fn image_only_book_with_empty_ncx_gets_page_range_toc() {
        // 乱马/火影同款：25 个纯图片页、NCX 存在但 navMap 为空 → 生成"第 N–M 页"分段目录，且指向真实页面。
        let items: String = (1..=25).map(|i| format!(r#"<item id="p{i}" href="Text/p{i}.xhtml" media-type="application/xhtml+xml"/>"#)).collect();
        let spine: String = (1..=25).map(|i| format!(r#"<itemref idref="p{i}"/>"#)).collect();
        let opf = format!(r#"<package version="2.0" unique-identifier="id"><metadata><dc:title>漫画</dc:title><dc:identifier id="id">urn:x</dc:identifier></metadata><manifest><item id="ncx" href="toc.ncx" media-type="application/x-dtbncx+xml"/>{items}</manifest><spine toc="ncx">{spine}</spine></package>"#);
        let mut es = vec![
            e("content.opf", &opf),
            e("toc.ncx", r#"<ncx><head/><docTitle><text>Unknown</text></docTitle><navMap></navMap></ncx>"#),
        ];
        for i in 1..=25 {
            es.push(e(&format!("Text/p{i}.xhtml"), r#"<html><body><img src="../Images/x.jpg"/></body></html>"#));
        }
        assert_eq!(toc_entry_count(&es), 0);
        let mut rep = WashReport::default();
        auto_toc(&mut es, AutoToc::IfMissing, "目录", &mut rep);
        assert_eq!(rep.toc_generated, 2, "25 页 → 20+5 两段");
        let ncx = String::from_utf8_lossy(&es.iter().find(|x| x.name == "toc.ncx").unwrap().data).to_string();
        assert!(ncx.contains("第 1–20 页") && ncx.contains("第 21–25 页"), "{ncx}");
        assert!(ncx.contains("Text/p1.xhtml") && ncx.contains("Text/p21.xhtml"), "目录必须指向段首页: {ncx}");
        assert!(toc_entry_count(&es) > 0);
    }

    #[test]
    fn auto_toc_generated_only_when_missing() {
        let mk = || vec![
            e("OEBPS/content.opf", r#"<package version="3.0"><metadata><dc:title>书</dc:title></metadata><manifest><item id="c1" href="text/c1.xhtml" media-type="application/xhtml+xml"/><item id="c2" href="text/c2.xhtml" media-type="application/xhtml+xml"/></manifest><spine><itemref idref="c1"/><itemref idref="c2"/></spine></package>"#),
            e("OEBPS/text/c1.xhtml", "<html><body><h1>第一章</h1><p>a</p><h2 id=\"s1\">一节</h2></body></html>"),
            e("OEBPS/text/c2.xhtml", "<html><body><h1>第<i>二</i>章</h1><p>b</p></body></html>"),
        ];
        let mut v = mk();
        let rep = wash_entries(&mut v, &WashOpts::default()).unwrap();
        assert_eq!(rep.toc_generated, 3);
        let ncx = s(&v, "OEBPS/toc.ncx");
        assert!(ncx.contains(r#"src="text/c1.xhtml#eink-toc-1""#) && ncx.contains(r#"src="text/c1.xhtml#s1""#) && ncx.contains("第二章"), "{ncx}");
        let nav = s(&v, "OEBPS/nav.xhtml");
        assert!(nav.contains(r#"<li><a href="text/c1.xhtml#eink-toc-1">第一章</a><ol><li><a href="text/c1.xhtml#s1">一节</a></li></ol></li>"#), "{nav}");
        let opf = s(&v, "OEBPS/content.opf");
        assert!(opf.contains(r#"toc="ncx""#) && opf.contains(r#"properties="nav""#), "{opf}");
        assert!(s(&v, "OEBPS/text/c1.xhtml").contains(r#"<h1 id="eink-toc-1">"#));
        assert_eq!(toc_entry_count(&v), 6, "ncx 3 + nav 3");
        // 章节文件不是 .xhtml 也算；样式表、图片、书外链接不算
        let odd = vec![
            e("OEBPS/content.opf", r#"<package version="3.0"><manifest><item id="nav" href="nav.xhtml" media-type="application/xhtml+xml" properties="nav"/></manifest><spine/></package>"#),
            e("OEBPS/nav.xhtml", r#"<html><head><link href="a.css" rel="stylesheet"/></head><body><nav><ol><li><a href="c1.xml#x">一</a></li><li><a href="https://x.org/">外</a></li><li><img src="i.png"/></li></ol></nav></body></html>"#),
        ];
        assert_eq!(toc_entry_count(&odd), 1);
        // 已有目录 → IfMissing 不动
        let mut w = mk();
        w.push(e("OEBPS/toc.ncx", r#"<ncx><navMap><navPoint><content src="text/c1.xhtml"/></navPoint></navMap></ncx>"#));
        let rep = wash_entries(&mut w, &WashOpts::default()).unwrap();
        assert_eq!(rep.toc_generated, 0);
        // 自动目录不动 NCX；规范整理补 EPUB 3 必需的 nav（NCX 条目没有标签，退回一条指向第一章、标题用书名）
        assert_eq!(rep.nav_generated, 1);
        assert!(s(&w, "OEBPS/nav.xhtml").contains(r#"<li><a href="text/c1.xhtml">书</a></li>"#));
        assert!(s(&w, "OEBPS/toc.ncx").contains(r#"<content src="text/c1.xhtml"/>"#), "NCX 原样");
    }

    /// 标题/书名里的字符引用（`&amp;`、`&#12288;` 全角空格）：自动目录只转义一次，不再出现 `&amp;amp;`、`&amp;#12288;`。
    #[test]
    fn auto_toc_titles_are_not_double_escaped() {
        let mut v = vec![
            e("OEBPS/content.opf", r#"<package version="3.0"><metadata><dc:title>Tom &amp; Jerry</dc:title></metadata><manifest><item id="c1" href="c1.xhtml" media-type="application/xhtml+xml"/></manifest><spine><itemref idref="c1"/></spine></package>"#),
            e("OEBPS/c1.xhtml", "<html><body><h1>第一章&#12288;猫 &amp; 鼠 &lt;上&gt;</h1><p>a</p></body></html>"),
        ];
        wash_entries(&mut v, &WashOpts::default()).unwrap();
        for f in ["OEBPS/toc.ncx", "OEBPS/nav.xhtml"] {
            let t = s(&v, f);
            assert!(t.contains("第一章 猫 &amp; 鼠 &lt;上&gt;"), "{f}: {t}");
            assert!(!t.contains("&amp;amp;") && !t.contains("&amp;#") && !t.contains("&amp;lt;"), "{f}: {t}");
        }
        assert!(s(&v, "OEBPS/toc.ncx").contains("<docTitle><text>Tom &amp; Jerry</text></docTitle>"));
    }

    #[test]
    fn existing_ncx_dtb_uid_synced_to_opf_identifier() {
        // 真机回归（2026-09-19，《疯探》）：navMap 结构完全正确，但 dtb:uid 是第三方生成器随手写的
        // 另一个 uuid，跟 OPF 的 dc:identifier 对不上——reMarkable 原生目录面板遇到这种不匹配
        // 直接不显示目录入口（不是空列表），换一本 dtb:uid 匹配的书目录入口就在。
        let opf = r#"<package version="2.0" unique-identifier="bookid"><metadata><dc:title>书</dc:title><dc:identifier id="bookid">urn:uuid:real-book-id</dc:identifier></metadata><manifest><item id="toc" href="toc.ncx" media-type="application/x-dtbncx+xml"/><item id="c1" href="c1.xhtml" media-type="application/xhtml+xml"/></manifest><spine toc="toc"><itemref idref="c1"/></spine></package>"#;
        let mut v = vec![
            e("content.opf", opf),
            e("toc.ncx", r#"<ncx><head><meta name="dtb:uid" content="urn:uuid:stale-generator-id"/></head><navMap><navPoint><navLabel><text>章一</text></navLabel><content src="c1.xhtml"/></navPoint></navMap></ncx>"#),
            e("c1.xhtml", "<html><body><p>正文</p></body></html>"),
        ];
        let rep = wash_entries(&mut v, &WashOpts::default()).unwrap();
        assert_eq!(rep.ncx_uid_fixed, 1);
        let ncx = s(&v, "toc.ncx");
        assert!(ncx.contains(r#"content="urn:uuid:real-book-id""#), "dtb:uid 该改成跟 OPF 一致: {ncx}");
        assert!(!ncx.contains("stale-generator-id"), "旧的错误 uid 不该残留: {ncx}");
        // 已经一致时不误报、不改动字节（幂等）
        let mut w = vec![
            e("content.opf", opf),
            e("toc.ncx", r#"<ncx><head><meta name="dtb:uid" content="urn:uuid:real-book-id"/></head><navMap><navPoint><navLabel><text>章一</text></navLabel><content src="c1.xhtml"/></navPoint></navMap></ncx>"#),
            e("c1.xhtml", "<html><body><p>正文</p></body></html>"),
        ];
        let rep2 = wash_entries(&mut w, &WashOpts::default()).unwrap();
        assert_eq!(rep2.ncx_uid_fixed, 0, "已经一致不该误报修复过");
    }

    #[test]
    fn missing_ncx_dtb_uid_added_and_bad_depth_fixed() {
        // 《绝叫》原书（2026-10-06，掌阅自带阅读器建不出目录）：head 里只有 dtb:depth，内容却是 uid；没有 dtb:uid。
        let opf = r#"<package version="2.0" unique-identifier="pub-id"><metadata><dc:title>书</dc:title><dc:identifier id="pub-id">urn:uuid:99de</dc:identifier></metadata><manifest><item id="ncx" href="toc.ncx" media-type="application/x-dtbncx+xml"/><item id="c1" href="c1.xhtml" media-type="application/xhtml+xml"/></manifest><spine toc="ncx"><itemref idref="c1"/></spine></package>"#;
        let ncx = r#"<ncx xmlns="http://www.daisy.org/z3986/2005/ncx/" version="2005-1"><head><meta name="dtb:depth" content="urn:uuid:99de"></meta></head><navMap><navPoint id="a"><navLabel><text>一</text></navLabel><content src="c1.xhtml"/><navPoint id="b"><navLabel><text>甲</text></navLabel><content src="c1.xhtml#x"/></navPoint></navPoint></navMap></ncx>"#;
        let mut v = vec![e("content.opf", opf), e("toc.ncx", ncx), e("c1.xhtml", r#"<html><body><p>正文</p><p id="x">节</p></body></html>"#)];
        let rep = wash_entries(&mut v, &WashOpts::default()).unwrap();
        assert_eq!(rep.ncx_uid_fixed, 1);
        let out = s(&v, "toc.ncx");
        assert!(out.contains(r#"<meta name="dtb:uid" content="urn:uuid:99de"/>"#), "{out}");
        assert!(out.contains(r#"name="dtb:depth" content="2""#), "{out}");
        // 再跑一遍不动
        let before = out.clone();
        let rep2 = wash_entries(&mut v, &WashOpts::default()).unwrap();
        assert_eq!(rep2.ncx_uid_fixed, 0);
        assert_eq!(s(&v, "toc.ncx"), before);
        // 没有 head 的也补上
        let mut w = vec![e("content.opf", opf), e("toc.ncx", r#"<ncx><navMap><navPoint><navLabel><text>一</text></navLabel><content src="c1.xhtml"/></navPoint></navMap></ncx>"#), e("c1.xhtml", "<html><body><p>正文</p></body></html>")];
        wash_entries(&mut w, &WashOpts::default()).unwrap();
        assert!(s(&w, "toc.ncx").contains(r#"<ncx><head><meta name="dtb:uid" content="urn:uuid:99de"/></head><navMap>"#), "{}", s(&w, "toc.ncx"));
    }

    #[test]
    fn empty_or_prefixed_unique_identifier_still_matches_ncx() {
        // 唯一标识符是空值：规范整理改指向别的非空标识符，NCX 的 dtb:uid 跟着它（以前空值也当标识符，dtb:uid 会被改成空）。
        let opf = r#"<package version="2.0" unique-identifier="bookid"><metadata><dc:title>书</dc:title><dc:identifier id="bookid"> </dc:identifier><dc:identifier id="isbn">978-7</dc:identifier></metadata><manifest><item id="ncx" href="toc.ncx" media-type="application/x-dtbncx+xml"/><item id="c1" href="c1.xhtml" media-type="application/xhtml+xml"/></manifest><spine toc="ncx"><itemref idref="c1"/></spine></package>"#;
        // 带别的前缀的标识符（`dc11:`）：认得出来，不另补一个
        let opf2 = r#"<package version="2.0" unique-identifier="bookid" xmlns:dc11="http://purl.org/dc/elements/1.1/"><metadata><dc:title>书</dc:title><dc11:identifier id="bookid">urn:x</dc11:identifier></metadata><manifest><item id="ncx" href="toc.ncx" media-type="application/x-dtbncx+xml"/><item id="c1" href="c1.xhtml" media-type="application/xhtml+xml"/></manifest><spine toc="ncx"><itemref idref="c1"/></spine></package>"#;
        for (opf, want) in [(opf, "978-7"), (opf2, "urn:x")] {
            let mut v = vec![
                e("content.opf", opf),
                e("toc.ncx", r#"<ncx><head><meta name="dtb:uid" content="stale"/></head><navMap><navPoint><navLabel><text>章一</text></navLabel><content src="c1.xhtml"/></navPoint></navMap></ncx>"#),
                e("c1.xhtml", "<html><body><p>正文</p></body></html>"),
            ];
            wash_entries(&mut v, &WashOpts::default()).unwrap();
            let new_opf = s(&v, "content.opf");
            assert_eq!(opf::unique_identifier(&new_opf).as_deref(), Some(want), "{new_opf}");
            assert!(!new_opf.contains("urn:eink:"), "有非空标识符就不另补: {new_opf}");
            assert!(s(&v, "toc.ncx").contains(&format!(r#"content="{want}""#)), "{}", s(&v, "toc.ncx"));
        }
    }

    #[test]
    fn ncx_manifest_id_renamed_to_ncx_and_spine_toc_synced() {
        // 真机回归（2026-09-19，《疯探》，反编译 xochitl 二进制坐实）：dtb:uid、DOCTYPE 都修一致
        // 后原生目录入口依然不出现——根因是 xochitl 定位目录文件硬编码死查 manifest 里 id="ncx"，
        // 不是走 `<spine toc="IDREF">`；《疯探》完全合规的 `id="toc"` + `<spine toc="toc">`
        // 因此找不到。只改这一个 id 名字，真机验证 94 条章节标题全部恢复。
        let opf = r#"<package version="2.0"><metadata><dc:title>疯探</dc:title></metadata><manifest><item id="toc" href="toc.ncx" media-type="application/x-dtbncx+xml"/><item id="c1" href="text/c1.xhtml" media-type="application/xhtml+xml"/></manifest><spine toc="toc"><itemref idref="c1"/></spine></package>"#;
        let mut v = vec![
            e("content.opf", opf),
            e("toc.ncx", r#"<ncx><navMap><navPoint><navLabel><text>第一章</text></navLabel><content src="text/c1.xhtml"/></navPoint></navMap></ncx>"#),
            e("text/c1.xhtml", "<html><body><p>正文</p></body></html>"),
        ];
        let rep = wash_entries(&mut v, &WashOpts::default()).unwrap();
        assert_eq!(rep.ncx_manifest_id_fixed, 1);
        let opf_out = s(&v, "content.opf");
        assert!(opf_out.contains(r#"<item id="ncx" href="toc.ncx""#), "manifest 里 ncx 条目的 id 该改成 \"ncx\": {opf_out}");
        assert!(opf_out.contains(r#"<spine toc="ncx">"#), "spine 的 toc 属性该同步指向新 id: {opf_out}");
        assert!(!opf_out.contains(r#"id="toc""#), "旧 id 不该残留: {opf_out}");
        // 已经叫 "ncx" 时原样不动、不误报（幂等）
        let already_ok = r#"<package version="2.0"><metadata><dc:title>书</dc:title></metadata><manifest><item id="ncx" href="toc.ncx" media-type="application/x-dtbncx+xml"/><item id="c1" href="c1.xhtml" media-type="application/xhtml+xml"/></manifest><spine toc="ncx"><itemref idref="c1"/></spine></package>"#;
        let mut w = vec![
            e("content.opf", already_ok),
            e("toc.ncx", r#"<ncx><navMap><navPoint><navLabel><text>第一章</text></navLabel><content src="c1.xhtml"/></navPoint></navMap></ncx>"#),
            e("c1.xhtml", "<html><body><p>正文</p></body></html>"),
        ];
        let rep2 = wash_entries(&mut w, &WashOpts::default()).unwrap();
        assert_eq!(rep2.ncx_manifest_id_fixed, 0, "已经叫 ncx 不该误报修复过");
    }

    #[test]
    fn ncx_external_doctype_stripped() {
        // 真机回归（2026-09-19，dtb:uid 修一致后原生目录入口仍不出现）：《疯探》"番茄小说 EPUB
        // Generator" 产物的 toc.ncx 带外部 DTD 引用（daisy.org），《雪人》没有——这是两本书唯一
        // 剩下的结构性差异。剥掉不改变 NCX 语义，只去掉这个外部依赖。
        let ncx_with_doctype = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE ncx PUBLIC \"-//NISO//DTD ncx 2005-1//EN\" \"http://www.daisy.org/z3986/2005/ncx-2005-1.dtd\">\n<ncx xmlns=\"http://www.daisy.org/z3986/2005/ncx/\" version=\"2005-1\"><navMap><navPoint><navLabel><text>章一</text></navLabel><content src=\"c1.xhtml\"/></navPoint></navMap></ncx>";
        let mut v = vec![
            e("content.opf", r#"<package version="2.0"><metadata><dc:title>书</dc:title></metadata><manifest><item id="toc" href="toc.ncx" media-type="application/x-dtbncx+xml"/><item id="c1" href="c1.xhtml" media-type="application/xhtml+xml"/></manifest><spine toc="toc"><itemref idref="c1"/></spine></package>"#),
            e("toc.ncx", ncx_with_doctype),
            e("c1.xhtml", "<html><body><p>正文</p></body></html>"),
        ];
        let rep = wash_entries(&mut v, &WashOpts::default()).unwrap();
        assert_eq!(rep.ncx_doctype_stripped, 1);
        let ncx = s(&v, "toc.ncx");
        assert!(!ncx.to_ascii_uppercase().contains("DOCTYPE"), "DOCTYPE 该被剥掉: {ncx}");
        assert!(ncx.contains("<navPoint>") || ncx.contains(r#"<content src="c1.xhtml"/>"#), "navMap 内容不该被动: {ncx}");
        // 没有 DOCTYPE 的书原样不动、不误报
        let mut w = vec![
            e("content.opf", r#"<package version="2.0"><metadata><dc:title>书</dc:title></metadata><manifest><item id="toc" href="toc.ncx" media-type="application/x-dtbncx+xml"/><item id="c1" href="c1.xhtml" media-type="application/xhtml+xml"/></manifest><spine toc="toc"><itemref idref="c1"/></spine></package>"#),
            e("toc.ncx", r#"<ncx><navMap><navPoint><navLabel><text>章一</text></navLabel><content src="c1.xhtml"/></navPoint></navMap></ncx>"#),
            e("c1.xhtml", "<html><body><p>正文</p></body></html>"),
        ];
        let rep2 = wash_entries(&mut w, &WashOpts::default()).unwrap();
        assert_eq!(rep2.ncx_doctype_stripped, 0, "没有 DOCTYPE 不该误报剥过");
    }

    #[test]
    fn existing_flat_toc_with_part_prefix_restructured_into_two_levels() {
        // 真机回归（2026-09-19，《雪人》）：书自带扁平 toc.ncx，条目形如"第一部　01　雪人"
        // （首条，部+编号+章名）/"　02　卵石眼"（后续，只有编号+章名，隐式归属同一部）——原生
        // 目录面板显示的是完全扁平的列表，"01 雪人"没有嵌在"第一部"下面。
        let opf = r#"<package version="2.0" unique-identifier="BookId"><metadata><dc:title>雪人</dc:title><dc:identifier id="BookId">www.haodoo.net</dc:identifier></metadata><manifest><item id="ncx" href="toc.ncx" media-type="application/x-dtbncx+xml"/><item id="c1" href="1.xhtml" media-type="application/xhtml+xml"/><item id="c2" href="2.xhtml" media-type="application/xhtml+xml"/><item id="c10" href="10.xhtml" media-type="application/xhtml+xml"/><item id="c11" href="11.xhtml" media-type="application/xhtml+xml"/></manifest><spine toc="ncx"><itemref idref="c1"/><itemref idref="c2"/><itemref idref="c10"/><itemref idref="c11"/></spine></package>"#;
        let ncx = r#"<ncx><head><meta name="dtb:uid" content="www.haodoo.net"/></head><navMap>
<navPoint><navLabel><text>第一部　01　雪人</text></navLabel><content src="1.xhtml"/></navPoint>
<navPoint><navLabel><text>　02　卵石眼</text></navLabel><content src="2.xhtml"/></navPoint>
<navPoint><navLabel><text>第二部　10　粉筆</text></navLabel><content src="10.xhtml"/></navPoint>
<navPoint><navLabel><text>　11　死亡面具</text></navLabel><content src="11.xhtml"/></navPoint>
</navMap></ncx>"#;
        let mut v = vec![
            e("content.opf", opf),
            e("toc.ncx", ncx),
            e("1.xhtml", "<html><body><p>正文</p></body></html>"),
            e("2.xhtml", "<html><body><p>正文</p></body></html>"),
            e("10.xhtml", "<html><body><p>正文</p></body></html>"),
            e("11.xhtml", "<html><body><p>正文</p></body></html>"),
        ];
        let rep = wash_entries(&mut v, &WashOpts::default()).unwrap();
        assert_eq!(rep.toc_parts_restructured, 6, "2 个分部标题 + 4 条章节＝6 条");
        let out = s(&v, "toc.ncx");
        // 分部标题单独成父级、指向自己这条的目标（第一部开篇就是 1.xhtml）
        assert!(out.contains(r#"<text>第一部</text></navLabel><content src="1.xhtml"/>"#), "{out}");
        // "01 雪人" 降一级挂在"第一部"下面，同样指向 1.xhtml（分部标题那条自己也是这一章）；
        // plain_text 会把全角空格归一成半角（split_whitespace 统一处理，跟标题里其它空白一视同仁）
        assert!(out.contains(r#"<text>01 雪人</text></navLabel><content src="1.xhtml"/>"#), "{out}");
        // "02 卵石眼"（原来没有分部前缀）也降一级，挂在当前活跃的"第一部"下
        assert!(out.contains(r#"<text>02 卵石眼</text></navLabel><content src="2.xhtml"/>"#), "{out}");
        assert!(out.contains(r#"<text>第二部</text></navLabel><content src="10.xhtml"/>"#), "{out}");
        assert!(out.contains(r#"<text>10 粉筆</text></navLabel><content src="10.xhtml"/>"#), "{out}");
        // dtb:depth 该反映真的有两层
        assert!(out.contains(r#"<meta name="dtb:depth" content="2""#), "{out}");
        // 没有分部信号的书原样不动（不该被误伤）
        let opf2 = r#"<package version="2.0"><metadata><dc:title>书</dc:title></metadata><manifest><item id="ncx" href="toc.ncx" media-type="application/x-dtbncx+xml"/><item id="c1" href="c1.xhtml" media-type="application/xhtml+xml"/><item id="c2" href="c2.xhtml" media-type="application/xhtml+xml"/></manifest><spine toc="ncx"><itemref idref="c1"/><itemref idref="c2"/></spine></package>"#;
        let ncx2 = r#"<ncx><navMap><navPoint><navLabel><text>楔子</text></navLabel><content src="c1.xhtml"/></navPoint><navPoint><navLabel><text>尾声</text></navLabel><content src="c2.xhtml"/></navPoint></navMap></ncx>"#;
        let mut w = vec![e("content.opf", opf2), e("toc.ncx", ncx2), e("c1.xhtml", "<html><body><p>a</p></body></html>"), e("c2.xhtml", "<html><body><p>b</p></body></html>")];
        let rep2 = wash_entries(&mut w, &WashOpts::default()).unwrap();
        assert_eq!(rep2.toc_parts_restructured, 0, "没有'第X部'前缀不该重建");
    }

    #[test]
    fn lang_aware_indent() {
        // 只用裸 p{} 元素选择器（xochitl 解析器脆）、不带 !important（xochitl 吃不下）；中文 2em / 拉丁 1.2em。
        let cjk = wash_css(&WashOpts { lang: LangMode::Cjk, ..Default::default() });
        assert!(cjk.contains("text-indent:2em;") && !cjk.contains("1.2em") && !cjk.contains("!important"));
        assert!(cjk.starts_with("p{") && !cjk.contains(',') && !cjk.contains('+') && !cjk.contains('@'), "禁用复杂/逗号选择器: {cjk}");
        assert!(cjk.contains("padding-bottom:0;}"), "每条规则以分号收尾（xochitl 丢最后一个无分号声明）: {cjk}");
        let lat = wash_css(&WashOpts { lang: LangMode::Latin, ..Default::default() });
        assert!(lat.contains("text-indent:1.2em") && !lat.contains("!important"));
        // keep_para_spacing 时不归零段距
        let keep = wash_css(&WashOpts { keep_para_spacing: true, ..Default::default() });
        assert!(!keep.contains("margin-top:0") && keep.contains("p{text-indent:2em;text-align:justify;}"), "keep-spacing 也要尾分号: {keep}");
    }

    #[test]
    fn figure_and_figcaption_margin_zeroed_as_separate_bare_rules() {
        // 2026-09-10 真机 aeon.co 网文复现：figure/figcaption 默认边距没清零，图片夹在正文中间
        // 造成留白。修法＝跟 p 一样清零，但必须各自一条裸元素选择器规则——xochitl 解析器脆，
        // `figure,figcaption{}` 这种逗号选择器整条规则会失效（lang_aware_indent 测试断言过这条
        // 红线：不带逗号/复合选择器）。
        let css = wash_css(&WashOpts::default());
        assert!(css.contains("figure{margin:0;padding:0;}"), "{css}");
        assert!(css.contains("figcaption{margin:0;padding:0;}"), "{css}");
        assert!(!css.contains("figure,figcaption") && !css.contains("figcaption,figure"), "禁止逗号选择器: {css}");
        // keep_para_spacing 只管段落呼吸感，不该连带保留图片边距——不管这个档位开没开，figure/figcaption 都清零。
        let keep = wash_css(&WashOpts { keep_para_spacing: true, ..Default::default() });
        assert!(keep.contains("figure{margin:0;padding:0;}") && keep.contains("figcaption{margin:0;padding:0;}"), "{keep}");
    }

    /// 兜底：书压根没给注释块写过 CSS（纯靠我们自己生成的 `.footnotes`/`.eink-fnote`）时，"注释比正文
    /// 小一号"这条要求也得满足，不能只靠 `filter_css` 改书自带规则那条路（那条路对这种书压根碰不到）。
    #[test]
    fn wash_css_gives_footnote_classes_smaller_relative_size_as_bare_selectors() {
        let css = wash_css(&WashOpts::default());
        assert!(css.contains(&format!(".footnotes{{font-size:{FOOTNOTE_FONT_SIZE};}}")), "{css}");
        assert!(css.contains(&format!(".eink-fnote{{font-size:{FOOTNOTE_FONT_SIZE};}}")), "{css}");
        assert!(!css.contains(".footnotes,.eink-fnote") && !css.contains(".eink-fnote,.footnotes"), "禁止逗号选择器: {css}");
    }

    #[test]
    fn external_css_injected_and_linked() {
        // 端到端：英文书 wash 后——排版规则进外链 eink-wash.css、每章 <link> 指向它、OPF manifest 补 item。
        let mut v = vec![
            e("OEBPS/content.opf", r#"<package version="3.0"><metadata><dc:title>B</dc:title></metadata><manifest><item id="c1" href="Text/c1.xhtml" media-type="application/xhtml+xml"/></manifest><spine><itemref idref="c1"/></spine></package>"#),
            e("OEBPS/Text/c1.xhtml", "<html><head></head><body><h2>Chapter One</h2><p>English prose flowing across the page with many words indeed here</p></body></html>"),
        ];
        wash_entries(&mut v, &WashOpts::default()).unwrap();
        // 外链 css 存在且带拉丁缩进（英文书探测为 Latin）
        let css = s(&v, "OEBPS/eink-wash.css");
        assert!(css.contains("text-indent:1.2em") && !css.contains("!important"), "外链 css 应含拉丁缩进、无 !important: {css}");
        // 章节 <link> 相对路径正确（Text/ 下 → ../eink-wash.css），且不再有内联 text-indent
        let c1 = s(&v, "OEBPS/Text/c1.xhtml");
        assert!(c1.contains(r#"href="../eink-wash.css""#), "章节 link 路径错: {c1}");
        assert!(!c1.contains("text-indent:1.2em"), "通用缩进规则不该内联进 html: {c1}");
        assert!(c1.contains(r#"Chapter One</h2><div class="eink-flush">"#), "拉丁：标题后首段换 div 顶格: {c1}");
        assert!(css.contains(".eink-flush{text-indent:0.01em;margin-top:0;margin-bottom:0;}"), "外链 css 带 eink-flush 规则（0.01em 压继承）且尾分号: {css}");
        // OPF manifest 补了 item（相对 opf 目录 = eink-wash.css）
        let opf = s(&v, "OEBPS/content.opf");
        assert!(opf.contains(r#"href="eink-wash.css""#) && opf.contains("text/css"), "manifest 未补 item: {opf}");
        // 幂等：重洗不重复加 link / item
        wash_entries(&mut v, &WashOpts::default()).unwrap();
        assert_eq!(s(&v, "OEBPS/Text/c1.xhtml").matches("eink-wash.css").count(), 1, "link 重复");
        assert_eq!(s(&v, "OEBPS/content.opf").matches("eink-wash.css").count(), 1, "manifest item 重复");
    }

    #[test]
    fn book_css_indent_harmonized_and_first_para_flush() {
        // 书自带类规则的非零 text-indent 改成本书缩进；0 与负值保留；拉丁标题后首段内联不缩进且幂等
        let lat = WashOpts { lang: LangMode::Latin, ..Default::default() };
        let css = filter_css(".calibre_ {display:block;text-indent:2em;margin:0} .quote{text-indent:0;margin-left:2em} .hang{text-indent:-1.5em} p{text-indent:3%}", &lat);
        assert!(css.contains(".calibre_ {display:block;text-indent:1.2em;margin:0;}"), "{css}");
        assert!(css.contains(".quote{text-indent:0;margin-left:2em;}") && css.contains(".hang{text-indent:-1.5em;}"), "{css}");
        assert!(css.contains("p{text-indent:1.2em;}"), "百分比也算非零: {css}");
        let cjk = WashOpts { lang: LangMode::Cjk, ..Default::default() };
        assert!(filter_css(".calibre_ {text-indent:1.2em}", &cjk).contains("text-indent:2em"));
        let (h, _) = wash_html(r#"<html><body><h1 id="a">T</h1>
<div class="x"><p class="c" style="color:red;text-indent:2em">first</p><p>second</p></div><h2>U</h2><p style="text-indent:0">already</p></body></html>"#, &lat);
        assert!(h.contains(r#"<div class="eink-flush c" style="color:red;">first</div>"#), "书的类与颜色保留，行内 text-indent 去掉: {h}");
        assert!(h.contains(r#"<p>second</p>"#), "第二段不动: {h}");
        assert_eq!(h.matches("eink-flush").count(), 2, "h1 后与 h2 后各一段: {h}");
        let (h2, _) = wash_html(&h, &lat);
        assert_eq!(h2.matches("eink-flush").count(), 2, "幂等: {h2}");
        let (c, _) = wash_html("<html><body><h1>T</h1><p>x</p></body></html>", &cjk);
        assert!(!c.contains("text-indent:0"), "中文不做首段不缩进: {c}");
    }

    /// M4（2026-09-28 审计）：英文首段换成 eink-flush 的 div 时，作者用类写的强调（斜体、小型大写）保留；
    /// 写了 text-indent 的段落类不留在 div 上（xochitl 同为类规则先出现者胜，会压住 eink-flush）。
    #[test]
    fn latin_flush_keeps_emphasis_classes_drops_indent_classes() {
        let lat = WashOpts { lang: LangMode::Latin, ..Default::default() };
        let mut v = vec![
            e("content.opf", r#"<package version="2.0"><metadata><dc:title>B</dc:title><dc:language>en</dc:language></metadata><manifest><item id="css" href="s.css" media-type="text/css"/><item id="c1" href="c1.xhtml" media-type="application/xhtml+xml"/></manifest><spine><itemref idref="c1"/></spine></package>"#),
            e("s.css", ".body{display:block;text-indent:1.5em;margin:0} .ital{font-style:italic} .sc{font-variant:small-caps}"),
            e("c1.xhtml", r#"<html><head><link href="s.css" rel="stylesheet" type="text/css"/></head><body><h1>One</h1><p class="body ital sc" id="p1">It was a dark night, and the rain fell in torrents except at occasional intervals.</p><p class="body">Next.</p></body></html>"#),
        ];
        wash_entries(&mut v, &lat).unwrap();
        let h = s(&v, "c1.xhtml");
        assert!(h.contains(r#"<div class="eink-flush ital sc" id="p1">It was a dark night"#), "{h}");
        assert!(h.contains(r#"<p class="body">Next.</p>"#), "{h}");
        assert!(s(&v, "eink-wash.css").contains(".eink-flush{text-indent:0.01em;"), "首行不缩进的规则还在");
    }

    #[test]
    fn cjk_br_book_paragraphized_and_fullwidth_indent_stripped() {
        let cjk = WashOpts { lang: LangMode::Cjk, ..Default::default() };
        // 《人骨拼圖》形态：一章一个 div，h3 + 双 br + 每段全角空格开头、单 br 分段，无 <p>
        let src = "<html><head></head><body class=\"calibre\">\n<div class=\"calibre1\">\n<h3 class=\"calibre3\">14</h3><br class=\"calibre2\"/><br class=\"calibre2\"/>　　這間辦公室高居在曼哈頓下城高處。<br class=\"calibre2\"/>　　「對不起？長官？」<br class=\"calibre2\"/>　　嚴格說來，她不能算是。<br class=\"calibre2\"/></div></body></html>";
        let (h, _) = wash_html(src, &cjk);
        assert!(h.contains("<div class=\"calibre1\">\n<h3 class=\"calibre3\">14</h3>"), "块级标签原样: {h}");
        assert!(h.contains("<p>這間辦公室高居在曼哈頓下城高處。</p><p>「對不起？長官？」</p><p>嚴格說來，她不能算是。</p></div>"), "按 br 段落化且剥全角空格: {h}");
        assert!(!h.contains("<br") && !h.contains('　'), "br 与全角空格都不剩: {h}");
        // 有 <p> 的书：不动 br，但剥段首全角空格/nbsp
        let (h2, _) = wash_html("<html><body><p>　　第一段。</p><p>&#160;&#160;第二段。<br/>换行</p></body></html>", &cjk);
        assert!(h2.contains("<p>第一段。</p><p>第二段。<br/>换行</p>"), "{h2}");
        // 拉丁书不做段落化
        let lat = WashOpts { lang: LangMode::Latin, ..Default::default() };
        let (h3, _) = wash_html("<html><body><div>line one<br/>line two<br/>line three<br/>line four<br/>five</div></body></html>", &lat);
        assert!(h3.contains("line one<br/>line two"), "{h3}");
    }

    #[test]
    fn latin_flush_after_bold_heading_scene_break_and_chapter_start() {
        // 《Tell Me Your Dreams》形态：章名=加粗段落（非 <h>），场景切换=段末双 <br/>，无空段
        let lat = WashOpts { lang: LangMode::Latin, ..Default::default() };
        let src = r#"<html><body><div><p class="calibre_"><a href="x.html#1"><span class="bold"><span class="underline">Chapter Three</span></span></a></p><p class="calibre_"><span class="bold">I</span>N another place, at another time, Alette Peters could have been a successful artist.</p><p class="calibre_">Her father’s voice was blue.</p><p class="calibre_">The sound of running water was gray.<br class="calibre3"/><br class="calibre3"/></p><p class="calibre_">Alette Peters was twenty years old.</p><p class="calibre_">She could be plain-looking.</p><p class="calibre_">* * *</p><p class="calibre_">After the break.</p><p class="calibre_">Still after.</p></div></body></html>"#;
        // calibre_ 的规则写了 text-indent（真书里 `.calibre_ {display:block;text-indent:…}`）：不留在 eink-flush 上
        let (h, _) = wash_html_with(src, &lat, &["calibre_".to_string()].into_iter().collect());
        assert_eq!(h.matches("eink-flush").count(), 3, "章首正文 + 双br 后 + * * * 后各一段: {h}");
        assert!(h.contains(r#"<div class="eink-flush"><span class="bold">I</span>N another"#), "章首正文顶格＝换成只带 eink-flush 的 div（章名段本身不算）: {h}");
        assert!(h.contains(r#"<div class="eink-flush">Alette Peters was twenty"#), "双 br 后顶格: {h}");
        assert!(h.contains(r#"<div class="eink-flush">After the break.</div>"#), "* * * 后顶格: {h}");
        assert!(h.contains(r#"<p class="calibre_">Her father"#) && h.contains(r#"<p class="calibre_">Still after"#), "普通续段仍是 p: {h}");
        assert!(h.contains(r#"<p class="calibre_"><a href="x.html#1">"#), "章名段自己不动: {h}");
        // 拿空段当段距的书（空段 > 20%）：空段不算场景分隔
        let spaced = r#"<html><body><p>One.</p><p></p><p>Two.</p><p></p><p>Three.</p><p></p><p>Four.</p></body></html>"#;
        let (h3, _) = wash_html(spaced, &lat);
        assert_eq!(h3.matches("eink-flush").count(), 1, "只有章首一段顶格: {h3}");
        let (h4, _) = wash_html(&h, &lat);
        assert_eq!(h4.matches("eink-flush").count(), 3, "幂等（eink-flush div 当段落参与计数，后一段不被误顶格）: {h4}");
        assert!(h4.contains(r#"<p class="calibre_">Her father"#), "重洗后续段仍不顶格: {h4}");
    }

    #[test]
    fn detect_script() {
        let cjk = vec![e("c.xhtml", "<html><body><p>这是一本中文书籍需要两字缩进的测试内容足够多的汉字</p></body></html>")];
        assert_eq!(detect_dominant_script(&cjk), LangMode::Cjk);
        let en = vec![e("c.xhtml", "<html><body><p>This is an English book with plenty of latin letters here indeed</p></body></html>")];
        assert_eq!(detect_dominant_script(&en), LangMode::Latin);
    }

    #[test]
    fn auto_toc_from_h3_and_deep_nesting() {
        // 只用 h3 当章标题：旧 h1/h2 正则会漏，现在应生成目录
        let mut v = vec![
            e("content.opf", r#"<package version="3.0"><metadata><dc:title>书</dc:title></metadata><manifest><item id="c1" href="c1.xhtml" media-type="application/xhtml+xml"/></manifest><spine><itemref idref="c1"/></spine></package>"#),
            e("c1.xhtml", "<html><body><h3>章一</h3><p>a</p><h3>章二</h3></body></html>"),
        ];
        assert_eq!(wash_entries(&mut v, &WashOpts::default()).unwrap().toc_generated, 2, "h3 也进目录");
        // 多级嵌套 h1>h2>h3>h1
        let items = vec![TocItem::new(1, "A", "c.xhtml", "a"), TocItem::new(2, "B", "c.xhtml", "b"), TocItem::new(3, "C", "c.xhtml", "c"), TocItem::new(1, "D", "c.xhtml", "d")];
        let nav = build_nav(&items, "", "目录");
        assert!(nav.contains(r#"<li><a href="c.xhtml#a">A</a><ol><li><a href="c.xhtml#b">B</a><ol><li><a href="c.xhtml#c">C</a></li></ol></li></ol></li><li><a href="c.xhtml#d">D</a></li></ol>"#), "{nav}");
        let ncx = build_ncx(&items, "", "T", "eink-wash");
        assert!(ncx.contains(r#"<navPoint id="np1" playOrder="1"><navLabel><text>A</text></navLabel><content src="c.xhtml#a"/><navPoint id="np2""#), "{ncx}");
        assert!(ncx.contains(r#"</navPoint></navPoint></navPoint><navPoint id="np4""#), "C 收 3 层再开 D: {ncx}");
    }

    #[test]
    fn auto_toc_keeps_numbered_titles_whole() {
        // 2026-10-08 用户定：章节只按结构判，标题末尾的数字不拆成节（以前「第一章 1」拆成父子两级）
        let mut v = vec![
            e("content.opf", r#"<package version="3.0"><metadata><dc:title>书</dc:title></metadata><manifest><item id="c1" href="c1.xhtml" media-type="application/xhtml+xml"/></manifest><spine><itemref idref="c1"/></spine></package>"#),
            e("c1.xhtml", "<html><body><h1>第一章 1</h1><p>a</p><h1>后记</h1></body></html>"),
        ];
        wash_entries(&mut v, &WashOpts::default()).unwrap();
        let nav = s(&v, "nav.xhtml");
        assert!(nav.contains(r#"<li><a href="c1.xhtml#eink-toc-1">第一章 1</a></li><li><a href="c1.xhtml#eink-toc-2">后记</a></li>"#), "{nav}");
    }

    #[test]
    fn auto_toc_fallback_when_no_headings_at_all() {
        // 全书没有 h1–h6，退化到按 spine 文件生成目录（取正文首段文本当标题）
        let mut v = vec![
            e("content.opf", r#"<package version="3.0"><metadata><dc:title>书</dc:title></metadata><manifest><item id="c1" href="c1.xhtml" media-type="application/xhtml+xml"/><item id="c2" href="c2.xhtml" media-type="application/xhtml+xml"/></manifest><spine><itemref idref="c1"/><itemref idref="c2"/></spine></package>"#),
            e("c1.xhtml", "<html><body><p>从前有座山，山里有座庙。</p></body></html>"),
            e("c2.xhtml", "<html><body><p>庙里有个老和尚在讲故事。</p></body></html>"),
        ];
        let rep = wash_entries(&mut v, &WashOpts::default()).unwrap();
        assert_eq!(rep.toc_generated, 2, "无标题也要生成兜底目录");
        let nav = s(&v, "nav.xhtml");
        assert!(nav.contains(r#"<a href="c1.xhtml">从前有座山，山里有座庙。</a>"#), "无锚点、直接指文件本身: {nav}");
        assert!(nav.contains(r#"<a href="c2.xhtml">庙里有个老和尚在讲故事。</a>"#), "{nav}");
        let ncx = s(&v, "toc.ncx");
        assert!(ncx.contains(r#"content src="c1.xhtml"/"#), "ncx 同样不带 # : {ncx}");
    }

    #[test]
    fn auto_toc_fallback_for_mostly_imageonly_pages_is_page_ranges_not_body_n() {
        // 多数页是纯图片(无可提取文本)——疑似漫画/画册，不该灌一堆"正文 N"；但所有书都要有目录（2026-09-20 用户要求），
        // 所以改为按页分段的"第 N–M 页"（3 页 → 1 段）。
        let mut v = vec![
            e("content.opf", r#"<package version="3.0"><metadata><dc:title>书</dc:title></metadata><manifest><item id="c1" href="c1.xhtml" media-type="application/xhtml+xml"/><item id="c2" href="c2.xhtml" media-type="application/xhtml+xml"/><item id="c3" href="c3.xhtml" media-type="application/xhtml+xml"/></manifest><spine><itemref idref="c1"/><itemref idref="c2"/><itemref idref="c3"/></spine></package>"#),
            e("c1.xhtml", r#"<html><body><img src="p1.jpg"/></body></html>"#),
            e("c2.xhtml", r#"<html><body><img src="p2.jpg"/></body></html>"#),
            e("c3.xhtml", "<html><body><p>唯一一页有字。</p></body></html>"),
        ];
        let rep = wash_entries(&mut v, &WashOpts::default()).unwrap();
        assert_eq!(rep.toc_generated, 1, "多数纯图片页 → 按页分段目录");
        let nav = s(&v, "nav.xhtml");
        assert!(nav.contains("第 1–3 页") && !nav.contains("正文 "), "{nav}");
    }

    // ---- 无效引用清理 ----

    fn dead_refs(entries: &mut [Entry]) -> usize {
        let mut rep = WashReport::default();
        drop_dead_refs(entries, &mut rep);
        rep.dead_refs_removed
    }

    #[test]
    fn dead_img_removed_but_live_external_and_alt_kept() {
        let html = concat!(
            "<html><body><p>字</p>",
            r#"<img src="../Images/gone.jpg" alt="cover"/>"#,       // 缺失、alt=cover（我们自己写的）→ 删
            r#"<img src="../Images/ok.jpg"/>"#,                       // 存在 → 留
            r#"<img src="../Images/OK2.JPG"/>"#,                      // 仅大小写不同 → 留
            r#"<img src="../Images/%E5%9B%BE.png"/>"#,                // 百分号编码的存在文件 → 留
            r#"<img src="http://x/y.jpg"/><img src="data:image/png;base64,AA=="/>"#, // 外部 → 留
            r#"<img src="../Images/lost.jpg" alt="示意图：断桥"/>"#,   // 缺失但有真替代文字 → 留
            r#"<img src='../Images/gone2.png'>"#,                     // 单引号缺失无 alt → 删
            "</body></html>"
        );
        let mut v = vec![
            e("OEBPS/Text/a.xhtml", html),
            e("OEBPS/Images/ok.jpg", "x"),
            e("OEBPS/Images/ok2.jpg", "x"),
            e("OEBPS/Images/图.png", "x"),
        ];
        assert_eq!(dead_refs(&mut v), 2);
        let out = s(&v, "OEBPS/Text/a.xhtml");
        assert!(!out.contains("gone.jpg") && !out.contains("gone2.png"), "{out}");
        for keep in ["ok.jpg", "OK2.JPG", "%E5%9B%BE.png", "http://x/y.jpg", "data:image/png", "lost.jpg"] {
            assert!(out.contains(keep), "应保留 {keep}: {out}");
        }
        assert!(out.contains("<p>字</p>"), "文字不动");
        assert_eq!(dead_refs(&mut v), 0, "幂等");
    }

    #[test]
    fn dead_font_face_removed_in_css_and_style_block() {
        let css = concat!(
            r#"@font-face{font-family:"ht";src:url("../Fonts/ht.ttf")}"#,                    // 缺失 → 删
            r#"@font-face{font-family:"live";src:url(../Fonts/live.ttf)}"#,                    // 存在 → 留
            r#"@font-face{font-family:"duokan";src:url("res:/sdcard/DuoKan/Resource/Font/a.ttf")}"#, // 设备路径 → 删
            r#"@font-face{font-family:"mix";src:local("Songti"),url(../Fonts/none.ttf) format("truetype"),url(res:///sdcard/x.ttf)}"#, // 有 local → 留 local，剔死 url
            r#"@font-face{font-family:"web";src:url(https://f.example/x.woff2)}"#,             // 外部 → 留
            "p{color:#333}"
        );
        let html = format!(r#"<html><head><style type="text/css">{css}</style></head><body><p>字</p></body></html>"#);
        let mut v = vec![
            e("OEBPS/Styles/s.css", css),
            e("OEBPS/Text/a.xhtml", &html),
            e("OEBPS/Fonts/live.ttf", "x"),
        ];
        assert_eq!(dead_refs(&mut v), 6, "css 文件 3 条（2 整条删 + mix 剔 url）+ style 块 3 条");
        for name in ["OEBPS/Styles/s.css", "OEBPS/Text/a.xhtml"] {
            let out = s(&v, name);
            assert!(!out.contains("ht.ttf") && !out.contains("DuoKan") && !out.contains("none.ttf") && !out.contains("sdcard"), "{name}: {out}");
            assert!(out.contains(r#"src:local("Songti")}"#), "mix 只剩 local，逗号/format 清干净: {name}: {out}");
            assert!(out.contains("live.ttf") && out.contains("local(") && out.contains("f.example") && out.contains("p{color:#333}"), "{name}: {out}");
        }
    }

    #[test]
    fn wash_entries_reports_dead_refs_and_never_changes_text() {
        let text = "第一章 正文文字不能变。";
        let html = format!(r#"<html><head><title>t</title></head><body><h1>第一章</h1><p>{text}</p><img src="../Images/gone.jpg" alt="cover"/></body></html>"#);
        let mut v = vec![
            e("OEBPS/content.opf", r#"<package xmlns="http://www.idpf.org/2007/opf" version="3.0"><metadata xmlns:dc="http://purl.org/dc/elements/1.1/"><dc:title>t</dc:title></metadata><manifest><item id="a" href="Text/a.xhtml" media-type="application/xhtml+xml"/></manifest><spine><itemref idref="a"/></spine></package>"#),
            e("OEBPS/Text/a.xhtml", &html),
        ];
        let rep = wash_entries(&mut v, &WashOpts::default()).unwrap();
        assert_eq!(rep.dead_refs_removed, 1);
        let out = s(&v, "OEBPS/Text/a.xhtml");
        assert!(out.contains(text) && !out.contains("gone.jpg"), "{out}");
    }

    /// 空页判定与定章节共用 `html::has_visible`：只有分隔线/表格的页不算空页（此前这里只认图片/音视频，会被删掉）。
    #[test]
    fn is_empty_page_uses_shared_visibility() {
        let page = |b: &str| format!("<html><body>{b}</body></html>");
        for b in ["", " ", "<p></p>", "<p> &nbsp; </p>", "&#160;\u{a0}", "<div class=\"mbppagebreak\"></div>", "<!-- c -->", "<br/>\u{3000}", "<p>&#12288;</p>"] {
            assert!(is_empty_page(&page(b)), "{b:?}");
        }
        for b in ["x", "<p>字</p>", "&amp;", "<IMG src=a>", "<p><Svg/></p>", "<image/>", "<video>", "<hr/>", "<table><tr><td></td></tr></table>", "a < b"] {
            assert!(!is_empty_page(&page(b)), "{b:?}");
        }
        // 找不到完整 body 的页拿不准，不算空页（v51：以前按空 body 算，截断页的正文整页被删）
        assert!(!is_empty_page("<p>没有 body 的片段</p>"));
        assert!(!is_empty_page("<html><body><p>截断的页：有 body 没有 &lt;/body&gt;"));
        assert!(!is_empty_page("<html><body>"));
    }

    /// 全书没有 `<h>` 标题、目录里没有中文时，新建目录标题用"Contents"；重建已有 nav 时其它 `<nav>`（landmarks）与原标题保留。
    #[test]
    fn nav_rebuild_keeps_landmarks_and_title_language() {
        let items = vec![TocItem::new(1, "One", "c1.xhtml", "")];
        let old = r#"<html><head><title>x</title></head><body><nav epub:type="toc" id="toc"><h2>Table</h2><ol><li><a href="c0.xhtml">Zero</a></li></ol></nav><nav epub:type="landmarks"><ol><li><a epub:type="bodymatter" href="c1.xhtml">Start</a></li></ol></nav></body></html>"#;
        let out = write_nav(Some(old), &items, "", "Contents");
        assert_eq!(out, r#"<html><head><title>x</title></head><body><nav epub:type="toc" id="toc"><h2>Table</h2><ol><li><a href="c1.xhtml">One</a></li></ol></nav><nav epub:type="landmarks"><ol><li><a epub:type="bodymatter" href="c1.xhtml">Start</a></li></ol></nav></body></html>"#);
        assert!(write_nav(None, &items, "", "Contents").contains("<h1>Contents</h1>"));
        assert_eq!(toc_title(LangMode::Latin), "Contents");
        assert_eq!(toc_title(LangMode::Cjk), "目录");
    }

    /// manifest 项/属性解析（`parse_opf`、封面声明、占位封面探测共用）：属性顺序任意、`=` 两边带空格、
    /// 缺 id 的项不算、`data-id` 这类后缀同名的属性不能串。
    #[test]
    fn manifest_items_and_tag_attr() {
        let opf = r#"<manifest><item data-id="no" href="a%20b.xhtml" id="c1" media-type="application/xhtml+xml"/>
<item id = "img" properties="cover-image" href="i.jpg" media-type="image/jpeg"></item><item href="noid.css"/><itemref idref="c1"/></manifest>"#;
        let items = manifest_items(opf);
        assert_eq!(items.len(), 2, "缺 id 的项和 itemref 都不算");
        assert_eq!((items[0].id, items[0].href, items[0].media_type, items[0].properties), ("c1", "a%20b.xhtml", "application/xhtml+xml", ""));
        assert_eq!((items[1].id, items[1].properties), ("img", "cover-image"));
        assert!(items[1].tag.starts_with("<item id = ") && items[1].tag.ends_with('>'));
        assert_eq!(tag_attr(r#"<rootfile full-path="OEBPS/x.opf" media-type="y"/>"#, "FULL-PATH"), Some("OEBPS/x.opf"));
        assert_eq!(tag_attr(r#"<a data-id="1"/>"#, "id"), None);
    }

    // ───────────────────────── 章节与目录（不拆文件） ─────────────────────────

    /// 书：[(文件名, body)]，spine 按顺序。
    fn paged_book(chapters: &[(&str, &str)]) -> Vec<Entry> {
        let items: String = chapters.iter().enumerate().map(|(i, (n, _))| format!(r#"<item id="c{i}" href="Text/{n}" media-type="application/xhtml+xml"/>"#)).collect();
        let refs: String = (0..chapters.len()).map(|i| format!(r#"<itemref idref="c{i}"/>"#)).collect();
        let mut v = vec![e("OEBPS/content.opf", &format!(r#"<package version="3.0"><metadata><dc:title>书</dc:title></metadata><manifest>{items}</manifest><spine>{refs}</spine></package>"#))];
        for (n, body) in chapters {
            v.push(e(&format!("OEBPS/Text/{n}"), &format!(r#"<html><head><title>t</title></head><body id="b">{body}</body></html>"#)));
        }
        v
    }
    fn spine_files(v: &[Entry]) -> Vec<String> {
        parse_opf(v).unwrap().spine
    }
    fn body_of(v: &[Entry], name: &str) -> String {
        let h = s(v, name);
        h[h.find("<body").unwrap()..].to_string()
    }
    /// NCX 和 nav 的每条链接：目标文件在书里，带锚点的锚点在那个文件里（2026-10-06：不拆文件后，目录改指到文件中间的标题）。
    fn assert_toc_targets_exist(v: &[Entry]) {
        let opf = parse_opf(v).unwrap();
        let mut srcs: Vec<(String, String)> = Vec::new();
        if let Some(ncx) = &opf.ncx {
            for (_, _, t) in crate::ncx::parse_ncx_flat(&s(v, ncx)) {
                srcs.push((ncx.clone(), t));
            }
        }
        if let Some(nav) = &opf.nav_doc {
            for h in crate::html::link_values(&s(v, nav)) {
                srcs.push((nav.clone(), h.to_string()));
            }
        }
        assert!(!srcs.is_empty());
        for (from, t) in srcs {
            let (path, frag) = crate::epubzip::resolve_link(from.as_str(), &t);
            let e = v.iter().find(|e| e.name == path).unwrap_or_else(|| panic!("{from} 的目录指向不存在的文件 {t}"));
            if let Some(f) = frag.filter(|f| !f.is_empty()) {
                let html = String::from_utf8_lossy(&e.data);
                let id = crate::html::frag_id(&f).into_owned();
                assert!(crate::html::anchors(&html).iter().any(|(a, _)| crate::util::xml_unescape(a) == id.as_str()), "{from} 的目录 {t}：{path} 里没有锚点 {id}");
            }
        }
    }

    /// 2026-10-06 用户定：章节不再分页，原书的文件结构原样保留；目录照样有节（自动目录指到文件里的标题锚点）。
    #[test]
    fn chapters_not_split_sections_in_toc() {
        let mut v = paged_book(&[
            ("c1.xhtml", "<h1>第一章 风起</h1><p>引言段这是足够长的正文文字，确保标题页之后的内容超过三十个字这条门槛，不被当成书名页的作者行。</p><h2 id=\"s1\">第一节</h2><p>一节正文这是足够长的正文文字，确保标题页之后的内容超过三十个字这条门槛，不被当成书名页的作者行。</p><h2>第二节</h2><p>二节正文这是足够长的正文文字，确保标题页之后的内容超过三十个字这条门槛，不被当成书名页的作者行。</p>"),
            ("c2.xhtml", "<h1>第二章</h1><p>二章正文这是足够长的正文文字，确保标题页之后的内容超过三十个字这条门槛，不被当成书名页的作者行。</p>"),
        ]);
        wash_entries(&mut v, &WashOpts::default()).unwrap();
        assert_eq!(spine_files(&v), ["OEBPS/Text/c1.xhtml", "OEBPS/Text/c2.xhtml"], "不拆文件");
        assert!(!v.iter().any(|e| e.name.contains("-p")), "没有拆出来的份");
        let t = body_of(&v, "OEBPS/Text/c1.xhtml");
        assert!(t.contains("第一章 风起") && t.contains("引言段") && t.contains("第二节") && t.contains(r#"<body id="b">"#), "{t}");
        let nav = s(&v, "OEBPS/nav.xhtml");
        assert!(nav.contains(r#"href="Text/c1.xhtml#s1""#), "{nav}");
        assert_toc_targets_exist(&v);
        // 幂等
        let before: Vec<Entry> = v.clone();
        wash_entries(&mut v, &WashOpts::default()).unwrap();
        assert_eq!(spine_files(&v).len(), 2);
        assert_eq!(s(&v, "OEBPS/toc.ncx"), s(&before, "OEBPS/toc.ncx"));
    }

    /// 不拆文件：同文件的注释、别的文件指向章里锚点的链接都不用动。
    #[test]
    fn same_file_notes_and_cross_links_untouched() {
        let mut v = paged_book(&[
            ("c1.xhtml", r##"<h1>第一章</h1><h2>第一节</h2><p>正文<a href="#n1"><sup>1</sup></a>。</p><h2 id="s2">第二节</h2><p>另一节<a href="#n2">[2]</a>。</p><p id="n1"><a href="#r1">1</a> 注一。</p><p id="n2">注二。</p>"##),
            ("toc.xhtml", r#"<p><a href="c1.xhtml#s2">第二节</a></p>"#),
        ]);
        wash_entries(&mut v, &WashOpts::default()).unwrap();
        let c1 = body_of(&v, "OEBPS/Text/c1.xhtml");
        let (n1, n2, s2) = (c1.find("注一").unwrap(), c1.find("注二").unwrap(), c1.find("第二节").unwrap());
        assert!(s2 < n1 && n1 < n2, "注释留在原处（章末）：{c1}");
        assert!(s(&v, "OEBPS/Text/toc.xhtml").contains(r#"href="c1.xhtml#s2""#));
    }

    #[test]
    fn empty_anchor_before_heading_links_untouched() {
        let mut v = paged_book(&[
            ("c1.xhtml", r#"<h1>第一章</h1><p>正文这是足够长的正文文字，确保标题页之后的内容超过三十个字这条门槛，不被当成书名页的作者行。</p><a id="x"></a><h2>第一节</h2><p>节文。</p>"#),
            ("list.xhtml", r#"<h1>目录</h1><p><a href="c1.xhtml#x">一</a></p><p><a href="c1.xhtml">二</a></p><p><a href="c1.xhtml">三</a></p>"#),
        ]);
        wash_entries(&mut v, &WashOpts::default()).unwrap();
        assert!(s(&v, "OEBPS/Text/list.xhtml").contains(r#"href="c1.xhtml#x""#));
        assert_eq!(spine_files(&v).len(), 2);
    }

    #[test]
    fn comic_and_headingless_books_untouched() {
        let mut v = paged_book(&[("c1.xhtml", "<p>只有正文，没有标题。</p><p>第二段。</p>")]);
        let rep = wash_entries(&mut v, &WashOpts::default()).unwrap();
        assert_eq!(rep.toc_sections_added, 0);
        assert_eq!(spine_files(&v).len(), 1);
    }

    #[test]
    fn books_without_h_tags_use_toc_targets_as_titles() {
        let mut v = paged_book(&[
            ("c1.xhtml", r#"<p id="t1" class="block_7">緣起首回　開宗明義</p><p class="block_">正文第一段这是足够长的正文文字，确保标题页之后的内容超过三十个字这条门槛，不被当成书名页的作者行。</p><p>正文第二段。</p>"#),
            ("c2.xhtml", r#"<p id="t2">第二回</p><p>正文这是足够长的正文文字，确保标题页之后的内容超过三十个字这条门槛，不被当成书名页的作者行。</p>"#),
        ]);
        v.push(e("OEBPS/toc.ncx", r#"<ncx><navMap><navPoint><navLabel><text>一</text></navLabel><content src="Text/c1.xhtml#t1"/></navPoint><navPoint><navLabel><text>二</text></navLabel><content src="Text/c2.xhtml#t2"/></navPoint></navMap></ncx>"#));
        let opf = s(&v, "OEBPS/content.opf").replace("</manifest>", r#"<item id="ncx" href="toc.ncx" media-type="application/x-dtbncx+xml"/></manifest>"#);
        v[0].data = opf.into_bytes();
        wash_entries(&mut v, &WashOpts::default()).unwrap();
        assert_eq!(spine_files(&v).len(), 2);
        assert_eq!(crate::ncx::parse_ncx_flat(&s(&v, "OEBPS/toc.ncx")).len(), 2);
        assert_toc_targets_exist(&v);
    }

    #[test]
    fn toc_without_fragment_uses_first_paragraph_as_title() {
        let mut v = paged_book(&[("c1.xhtml", r#"<p class="block_7">第一回　楔子</p><p>正文一这是足够长的正文文字，确保标题页之后的内容超过三十个字这条门槛，不被当成书名页的作者行。</p>"#), ("c2.xhtml", r#"<div><p>第二回</p></div><p>正文二这是足够长的正文文字，确保标题页之后的内容超过三十个字这条门槛。</p>"#)]);
        v.push(e("OEBPS/toc.ncx", r#"<ncx><navMap><navPoint><navLabel><text>一</text></navLabel><content src="Text/c1.xhtml"/></navPoint><navPoint><navLabel><text>二</text></navLabel><content src="Text/c2.xhtml"/></navPoint></navMap></ncx>"#));
        let opf = s(&v, "OEBPS/content.opf").replace("</manifest>", r#"<item id="ncx" href="toc.ncx" media-type="application/x-dtbncx+xml"/></manifest>"#);
        v[0].data = opf.into_bytes();
        wash_entries(&mut v, &WashOpts::default()).unwrap();
        assert_eq!(spine_files(&v).len(), 2);
        assert!(body_of(&v, "OEBPS/Text/c2.xhtml").contains("正文二"));
    }

    #[test]
    fn sections_missing_from_own_toc_are_added_one_level_below_chapter() {
        let long = "这是足够长的正文文字，确保标题页之后的内容超过三十个字这条门槛，不被当成书名页的作者行。";
        let mut v = paged_book(&[
            ("c1.xhtml", &format!(r#"<h1 id="c1">第一章</h1><p>{long}</p><h2>第一节</h2><p>一节。</p><h2 id="s2">第二节</h2><p>二节。</p>"#)),
            ("c2.xhtml", &format!(r#"<h1 id="c2">第二章</h1><p>{long}</p>"#)),
        ]);
        v.push(e("OEBPS/toc.ncx", r#"<ncx><navMap><navPoint><navLabel><text>第一章</text></navLabel><content src="Text/c1.xhtml#c1"/></navPoint><navPoint><navLabel><text>第二章</text></navLabel><content src="Text/c2.xhtml#c2"/></navPoint></navMap></ncx>"#));
        let opf = s(&v, "OEBPS/content.opf").replace("</manifest>", r#"<item id="ncx" href="toc.ncx" media-type="application/x-dtbncx+xml"/></manifest>"#);
        v[0].data = opf.into_bytes();
        let rep = wash_entries(&mut v, &WashOpts::default()).unwrap();
        assert_eq!(rep.toc_sections_added, 2);
        let flat = crate::ncx::parse_ncx_flat(&s(&v, "OEBPS/toc.ncx"));
        let got: Vec<(usize, &str, &str)> = flat.iter().map(|(d, l, t)| (*d, l.as_str(), t.as_str())).collect();
        assert_eq!(
            got,
            [(1, "第一章", "Text/c1.xhtml#c1"), (2, "第一节", "Text/c1.xhtml#eink-sec-1"), (2, "第二节", "Text/c1.xhtml#s2"), (1, "第二章", "Text/c2.xhtml#c2")],
            "节挂在章下面一级，指向原文件里节标题的锚点"
        );
        assert!(s(&v, "OEBPS/Text/c1.xhtml").contains(r#"<h2 id="eink-sec-1">第一节</h2>"#));
        assert_toc_targets_exist(&v);
        assert_eq!(spine_files(&v).len(), 2);
        // 幂等：再跑一遍不重复补
        let rep2 = wash_entries(&mut v, &WashOpts::default()).unwrap();
        assert_eq!(rep2.toc_sections_added, 0);
        assert_eq!(crate::ncx::parse_ncx_flat(&s(&v, "OEBPS/toc.ncx")).len(), 4);
    }

    /// 嵌套目录的书：[(层级, 标签, 文件)]。
    fn nested_ncx_book(files: &[(&str, &str)], toc: &[(usize, &str, &str)]) -> Vec<Entry> {
        let mut v = paged_book(files);
        let mut xml = String::new();
        let mut depth = 0;
        for (d, l, f) in toc {
            while depth >= *d {
                xml.push_str("</navPoint>");
                depth -= 1;
            }
            xml.push_str(&format!("<navPoint><navLabel><text>{l}</text></navLabel><content src=\"Text/{f}\"/>"));
            depth = *d;
        }
        for _ in 0..depth {
            xml.push_str("</navPoint>");
        }
        v.push(e("OEBPS/toc.ncx", &format!("<ncx><navMap>{xml}</navMap></ncx>")));
        let opf = s(&v, "OEBPS/content.opf").replace("</manifest>", r#"<item id="ncx" href="toc.ncx" media-type="application/x-dtbncx+xml"/></manifest>"#);
        v[0].data = opf.into_bytes();
        v
    }

    /// 《绍宋》（卷 → 章）：章名是装饰图 + 「第一章」「明道宫」两段，前面一个隐藏的 h2。按目录层级：卷 → 章两层，隐藏的 h2 不当节、不补进目录。
    #[test]
    fn toc_driven_volume_and_paragraph_chapter_titles() {
        let long = "这是足够长的正文文字，确保标题页之后的内容超过三十个字这条门槛，不被当成书名页的作者行。";
        let ch = |n: &str, name: &str| format!(r#"<h2 style="display:none;">{n} {name}</h2><div class="j"><img src="../Images/j.png" alt="j"/></div><p class="j11">{n}</p><p class="j1">{name}</p><p>{long}</p><p>{long}</p>"#);
        let mut v = nested_ncx_book(
            &[("J01.xhtml", r#"<h1 class="juan">第一卷 靖康遗志</h1>"#), ("c1.xhtml", &ch("第一章", "明道宫")), ("c2.xhtml", &ch("第二章", "赤心队"))],
            &[(1, "第一卷 靖康遗志", "J01.xhtml"), (2, "第一章 明道宫", "c1.xhtml"), (2, "第二章 赤心队", "c2.xhtml")],
        );
        let rep = wash_entries(&mut v, &WashOpts::default()).unwrap();
        assert_eq!(rep.toc_sections_added, 0, "隐藏的 h2 不当节补进目录");
        let flat = crate::ncx::parse_ncx_flat(&s(&v, "OEBPS/toc.ncx"));
        let got: Vec<(usize, &str, &str)> = flat.iter().map(|(d, l, t)| (*d, l.as_str(), t.as_str())).collect();
        assert_eq!(got, [(1, "第一卷 靖康遗志", "Text/J01.xhtml"), (2, "第一章 明道宫", "Text/c1.xhtml"), (2, "第二章 赤心队", "Text/c2.xhtml")]);
        assert_eq!(spine_files(&v).len(), 3);
    }

    /// 合集（书 → 回）：书名下面一层像「第X回」，最上一层就是书一级；不拆文件，目录层级不变。
    #[test]
    fn toc_driven_collection_book_and_chapter_pages() {
        let long = "这是足够长的正文文字，确保标题页之后的内容超过三十个字这条门槛，不被当成书名页的作者行。";
        let mut v = nested_ncx_book(
            &[
                ("b1.xhtml", &format!(r#"<h1 id="b">书剑恩仇录</h1><h2 id="h1">第一回</h2><p>{long}</p>"#)),
                ("b1c2.xhtml", &format!(r#"<h2>第二回</h2><p>{long}</p>"#)),
                ("b2.xhtml", &format!(r#"<h1>碧血剑</h1><h2>第一回</h2><p>{long}</p>"#)),
            ],
            &[(1, "书剑恩仇录", "b1.xhtml#b"), (2, "第一回", "b1.xhtml#h1"), (2, "第二回", "b1c2.xhtml"), (1, "碧血剑", "b2.xhtml"), (2, "第一回", "b2.xhtml")],
        );
        wash_entries(&mut v, &WashOpts::default()).unwrap();
        assert_eq!(spine_files(&v).len(), 3, "不拆文件");
        let flat = crate::ncx::parse_ncx_flat(&s(&v, "OEBPS/toc.ncx"));
        let got: Vec<(usize, &str, &str)> = flat.iter().map(|(d, l, t)| (*d, l.as_str(), t.as_str())).collect();
        assert_eq!(
            got,
            [(1, "书剑恩仇录", "Text/b1.xhtml#b"), (2, "第一回", "Text/b1.xhtml#h1"), (2, "第二回", "Text/b1c2.xhtml"), (1, "碧血剑", "Text/b2.xhtml"), (2, "第一回", "Text/b2.xhtml#eink-ch-1")],
            "合集目录层级不变；书和第一回指到同一个文件时，第一回改指到文件中间的标题"
        );
        assert_toc_targets_exist(&v);
    }

    /// 合集里章名全是数字的书（《深夜小狗神秘事件》：2、3、5、7…）：数字章名按节处理，目录不重复补「2」下面的「2」。
    #[test]
    fn numeric_chapters_in_collection_not_duplicated_in_toc() {
        let long = "这是足够长的正文文字，确保标题页之后的内容超过三十个字这条门槛，不被当成书名页的作者行。";
        let mut v = nested_ncx_book(
            &[
                ("b1.xhtml", r#"<h1>恶意</h1>"#),
                ("b1c1.xhtml", &format!(r#"<h2>事件之章</h2><p>{long}</p>"#)),
                ("b2.xhtml", r#"<h1>深夜小狗神秘事件</h1>"#),
                ("b2p.xhtml", &format!(r#"<h2>前言</h2><p>{long}</p>"#)),
                ("b2c2.xhtml", &format!(r#"<div id="a2"></div><h2>2</h2><p>{long}</p>"#)),
                ("b2c3.xhtml", &format!(r#"<div id="a3"></div><h2>3</h2><p>{long}</p>"#)),
                ("b3.xhtml", r#"<h1>混凝土里的金发女郎</h1>"#),
                ("b3p.xhtml", &format!(r#"<h2>序幕</h2><p>{long}</p>"#)),
                ("b3c1.xhtml", &format!(r#"<h2>2</h2><p>{long}</p>"#)),
                ("b3c2.xhtml", &format!(r#"<h2>3</h2><p>{long}</p>"#)),
            ],
            &[
                (1, "恶意", "b1.xhtml"),
                (2, "事件之章", "b1c1.xhtml"),
                (1, "深夜小狗神秘事件", "b2.xhtml"),
                (2, "前言", "b2p.xhtml"),
                (2, "2", "b2c2.xhtml#a2"),
                (2, "3", "b2c3.xhtml#a3"),
                (1, "混凝土里的金发女郎", "b3.xhtml"),
                (2, "序幕", "b3p.xhtml"),
                (2, "2", "b3c1.xhtml"),
                (2, "3", "b3c2.xhtml"),
            ],
        );
        let rep = wash_entries(&mut v, &WashOpts::default()).unwrap();
        assert_eq!(rep.toc_sections_added, 0, "普通章名后面跟着数字章不是《13級階梯》那种节");
        assert_eq!(crate::ncx::parse_ncx_flat(&s(&v, "OEBPS/toc.ncx")).len(), 10);
    }

    /// 《克莱因壶》：「著作权使用契约书」后面一串「01」「02」…是章（数字章名），只有一串，不缩进到前一条下面。
    #[test]
    fn single_numeric_run_after_front_matter_not_indented() {
        let long = "这是足够长的正文文字，确保标题页之后的内容超过三十个字这条门槛，不被当成书名页的作者行。";
        let mut v = nested_ncx_book(
            &[("cp.xhtml", &format!(r#"<h2>著作权使用契约书</h2><p>{long}</p>"#)), ("c1.xhtml", &format!(r#"<h2>01</h2><p>{long}</p>"#)), ("c2.xhtml", &format!(r#"<h2>02</h2><p>{long}</p>"#))],
            &[(1, "著作权使用契约书", "cp.xhtml"), (1, "01", "c1.xhtml"), (1, "02", "c2.xhtml")],
        );
        wash_entries(&mut v, &WashOpts::default()).unwrap();
        let flat = crate::ncx::parse_ncx_flat(&s(&v, "OEBPS/toc.ncx"));
        assert!(flat.iter().all(|(d, _, _)| *d == 1), "{flat:?}");
    }

    /// 福尔摩斯全集里的《恐怖谷》：书下面「第一部」「第一章」…「第二部」「第一章」…平排。目录嵌成书 → 部 → 章，
    /// 部下面的章仍是章（不因为深了一层就当成节）。
    #[test]
    fn parts_among_siblings_nested_and_chapters_stay_chapters() {
        let long = "这是足够长的正文文字，确保标题页之后的内容超过三十个字这条门槛，不被当成书名页的作者行。";
        let mut v = nested_ncx_book(
            &[
                ("a.xhtml", r#"<h1>血字的研究</h1>"#),
                ("a1.xhtml", &format!(r#"<h2>第一章 歇洛克·福尔摩斯先生</h2><p>{long}</p>"#)),
                ("b.xhtml", r#"<h1>恐怖谷</h1>"#),
                ("bp1.xhtml", r#"<h2>第一部 伯尔斯通惨剧</h2>"#),
                ("b1.xhtml", &format!(r#"<h2>第一章 警讯</h2><p>{long}</p>"#)),
                ("bp2.xhtml", r#"<h2>第二部 死酷党人</h2>"#),
                ("b2.xhtml", &format!(r#"<h2>第一章 某人</h2><p>{long}</p>"#)),
            ],
            &[
                (1, "血字的研究", "a.xhtml"),
                (2, "第一章 歇洛克·福尔摩斯先生", "a1.xhtml"),
                (1, "恐怖谷", "b.xhtml"),
                (2, "第一部 伯尔斯通惨剧", "bp1.xhtml"),
                (2, "第一章 警讯", "b1.xhtml"),
                (2, "第二部 死酷党人", "bp2.xhtml"),
                (2, "第一章 某人", "b2.xhtml"),
            ],
        );
        wash_entries(&mut v, &WashOpts::default()).unwrap();
        let flat = crate::ncx::parse_ncx_flat(&s(&v, "OEBPS/toc.ncx"));
        let got: Vec<(usize, &str)> = flat.iter().map(|(d, l, _)| (*d, l.as_str())).collect();
        assert_eq!(got, [(1, "血字的研究"), (2, "第一章 歇洛克·福尔摩斯先生"), (1, "恐怖谷"), (2, "第一部 伯尔斯通惨剧"), (3, "第一章 警讯"), (2, "第二部 死酷党人"), (3, "第一章 某人")]);
        assert_eq!(spine_files(&v).len(), 7, "不拆文件");
        assert_toc_targets_exist(&v);
    }

    /// 《啸风山庄》：目录锚点是自闭合的空段落 `<p id="…"/>`，章名段落里有自闭合的 `<span/>`。产物仍是合法 XML，目录照指原处。
    #[test]
    fn toc_anchor_on_self_closed_paragraph() {
        let long = "这是足够长的正文文字，确保标题页之后的内容超过三十个字这条门槛，不被当成书名页的作者行。";
        let body = |n: &str, id: &str| format!(r#"<div class="x"><p class="chapter" id="{id}"/><p class="h1">{n}<span class="o"/></p><p class="p">{long}</p></div>"#);
        let mut v = nested_ncx_book(
            &[("c18.xhtml", &body("第十八章", "m18")), ("c19.xhtml", &body("第十九章", "m19"))],
            &[(1, "第十八章", "c18.xhtml#m18"), (1, "第十九章", "c19.xhtml#m19")],
        );
        wash_entries(&mut v, &WashOpts::default()).unwrap();
        for e in v.iter().filter(|e| e.name.ends_with(".xhtml")) {
            let t = std::str::from_utf8(&e.data).unwrap();
            assert!(quick_xml_ok(t), "{}: {t}", e.name);
        }
        assert_eq!(spine_files(&v).len(), 2);
        assert_toc_targets_exist(&v);
    }

    fn quick_xml_ok(t: &str) -> bool {
        let mut r = quick_xml::Reader::from_str(t);
        let mut stack: Vec<Vec<u8>> = Vec::new();
        loop {
            match r.read_event() {
                Ok(quick_xml::events::Event::Start(e)) => stack.push(e.name().as_ref().to_vec()),
                Ok(quick_xml::events::Event::End(e)) => {
                    if stack.pop().as_deref() != Some(e.name().as_ref()) {
                        return false;
                    }
                }
                Ok(quick_xml::events::Event::Eof) => return stack.is_empty(),
                Err(_) => return false,
                _ => {}
            }
        }
    }

    /// 《ABC谋杀案》：h2 只用在单独成页的版权页、目录页，真正的节是 h3（「1」「2」）。节一级不能被 h2 占掉。
    #[test]
    fn front_matter_only_level_is_not_section() {
        let long = "这是足够长的正文文字，确保标题页之后的内容超过三十个字这条门槛，不被当成书名页的作者行。";
        let mut v = paged_book(&[
            ("cp.xhtml", "<h2>版权信息</h2><p>书名：甲</p>"),
            ("c1.xhtml", &format!(r#"<h1 id="c1">第一章</h1><h3>1</h3><p>{long}</p><h3>2</h3><p>{long}</p>"#)),
            ("c2.xhtml", &format!(r#"<h1 id="c2">第二章</h1><p>{long}</p>"#)),
        ]);
        v.push(e("OEBPS/toc.ncx", r#"<ncx><navMap><navPoint><navLabel><text>第一章</text></navLabel><content src="Text/c1.xhtml#c1"/></navPoint><navPoint><navLabel><text>第二章</text></navLabel><content src="Text/c2.xhtml#c2"/></navPoint></navMap></ncx>"#));
        let opf = s(&v, "OEBPS/content.opf").replace("</manifest>", r#"<item id="ncx" href="toc.ncx" media-type="application/x-dtbncx+xml"/></manifest>"#);
        v[0].data = opf.into_bytes();
        let rep = wash_entries(&mut v, &WashOpts::default()).unwrap();
        assert_eq!(rep.toc_sections_added, 2);
        let flat = crate::ncx::parse_ncx_flat(&s(&v, "OEBPS/toc.ncx"));
        let got: Vec<(usize, &str)> = flat.iter().map(|(d, l, _)| (*d, l.as_str())).collect();
        assert_eq!(got, [(1, "第一章"), (2, "1"), (2, "2"), (1, "第二章")]);
    }

    // ───────────────────────── 2026-09-27 审计回归 ─────────────────────────

    /// A1/A5：`data-id` 不是 id；单引号的 id 也算已有，不再追加第二个 id。
    #[test]
    fn audit_ids_exact_name_and_single_quotes() {
        assert_eq!(crate::htmlproc::collapse_dup_id_attrs(r#"<span id="x" data-id="y">t</span>"#), r#"<span id="x" data-id="y">t</span>"#);
        assert_eq!(count_dup_id_tags(r#"<span id="x" data-id="y"/><p id='a' id="b"/>"#), 1);
        let mut v = vec![
            e("content.opf", r#"<package version="3.0"><metadata><dc:title>书</dc:title></metadata><manifest><item id="c1" href="c1.xhtml" media-type="application/xhtml+xml"/></manifest><spine><itemref idref="c1"/></spine></package>"#),
            e("c1.xhtml", r#"<html><body><h1 id='c1'>第一章</h1><p>a</p><h2 data-id="z">一节</h2><p>b</p></body></html>"#),
        ];
        wash_entries(&mut v, &WashOpts::default()).unwrap();
        let c = s(&v, "c1.xhtml");
        assert!(c.contains(r#"<h1 id='c1'>"#) && c.contains(r#"<h2 data-id="z" id="eink-toc-1">"#), "{c}");
        let ncx = s(&v, "toc.ncx");
        assert!(ncx.contains(r#"src="c1.xhtml#c1""#) && ncx.contains(r#"src="c1.xhtml#eink-toc-1""#) && !ncx.contains("#z"), "{ncx}");
    }

    /// A2：`data-style`、SVG `font-style` 不是 style 属性。
    #[test]
    fn audit_style_attr_exact_name() {
        let h = r#"<p data-style="font-size:2em">a</p><svg><text font-style="italic" style="font-size:3px;fill:red">x</text></svg>"#;
        assert_eq!(crate::htmlproc::strip_font_locks(h), r#"<p data-style="font-size:2em">a</p><svg><text font-style="italic" style="fill:red">x</text></svg>"#);
        let (w, _) = wash_html(&format!("<html><body>{h}</body></html>"), &WashOpts { lang: LangMode::Cjk, ..Default::default() });
        assert!(w.contains(r#"data-style="font-size:2em""#) && w.contains(r#"font-style="italic" style="fill:red;""#), "{w}");
    }

    /// A3：`<br>` 在标题、列表项里（不只是块容器里）时整个文件不做假段落切分。
    #[test]
    fn audit_cjk_paragraphize_only_container_brs() {
        let h = "<html><body><div><h2>第一章<br/>风起</h2><br/>甲<br/>乙<br/>丙<br/>丁</div></body></html>";
        assert_eq!(cjk_paragraphize(h), h);
        let li = "<html><body><div><ul><li>1981<br/></li></ul></div><div>《甲》<br/>《乙》<br/>《丙》<br/>《丁》</div></body></html>";
        assert_eq!(cjk_paragraphize(li), li);
        let span = "<html><body><div><span>甲<br/>乙</span><br/>丙<br/>丁<br/>戊</div></body></html>";
        assert_eq!(cjk_paragraphize(span), span);
        let ok = "<html><body><div>甲<br/>乙<br/>丙<br/>丁<br/>戊</div></body></html>";
        assert_eq!(cjk_paragraphize(ok), "<html><body><div><p>甲</p><p>乙</p><p>丙</p><p>丁</p><p>戊</p></div></body></html>");
    }

    /// A4：自闭合 `<p/>` 是空段，不会把下一段吞进来。
    #[test]
    fn audit_flush_self_closing_p() {
        let h = r#"<html><body><h1>T</h1><p class="a">One.</p><p class="a">1.</p><p class="a">2.</p><p/><p class="a">Two.</p><p class="a">Three.</p></body></html>"#;
        let out = flush_first_para_after_heading(h, &["a".to_string()].into_iter().collect());
        assert_eq!(out, r#"<html><body><h1>T</h1><div class="eink-flush">One.</div><p class="a">1.</p><p class="a">2.</p><p/><div class="eink-flush">Two.</div><p class="a">Three.</p></body></html>"#);
    }

    /// A6：值里带分号的 `url(data:…;base64,…)`、引号里的分号不截断声明。
    #[test]
    fn audit_css_decls_keep_data_urls() {
        let f: Vec<String> = DEFAULT_FILTER_PROPS.iter().map(|s| s.to_string()).collect();
        assert_eq!(filter_decls(r#"list-style-image:url(data:image/png;base64,AAAA);content:"a;b";font-size:20px"#, &f, Spacing::Keep), r#"list-style-image:url(data:image/png;base64,AAAA);content:"a;b";"#);
    }

    /// A7：单引号 id、NCX 里百分号编码的锚点都认；补进目录的节用原来的单引号 id，别的文件的链接不动。
    #[test]
    fn audit_toc_sees_single_quoted_and_encoded_anchors() {
        let long = "这是足够长的正文文字，确保标题页之后的内容超过三十个字这条门槛，不被当成书名页的作者行。";
        let mut v = paged_book(&[
            ("c1.xhtml", &format!(r##"<h1>第一章</h1><p>{long}</p><h2 id='s1'>第一节</h2><p>正文<a href="#n1"><sup>1</sup></a>。</p><h2 id="注二">第二节</h2><p>{long}</p><p><a name="n1"></a>注一。</p>"##)),
            ("list.xhtml", r#"<p>见<a href='c1.xhtml#s1'>第一节</a>。</p>"#),
        ]);
        v.push(e("OEBPS/toc.ncx", r#"<ncx><navMap><navPoint><navLabel><text>第一章</text></navLabel><content src="Text/c1.xhtml"/><navPoint><navLabel><text>第二节</text></navLabel><content src="Text/c1.xhtml#%E6%B3%A8%E4%BA%8C"/></navPoint></navPoint></navMap></ncx>"#));
        let opf = s(&v, "OEBPS/content.opf").replace("</manifest>", r#"<item id="ncx" href="toc.ncx" media-type="application/x-dtbncx+xml"/></manifest>"#);
        v[0].data = opf.into_bytes();
        let rep = wash_entries(&mut v, &WashOpts::default()).unwrap();
        assert_eq!(rep.toc_sections_added, 1, "第一节补进目录，第二节目录里已有");
        assert!(s(&v, "OEBPS/Text/list.xhtml").contains(r#"href='c1.xhtml#s1'"#));
        let flat = crate::ncx::parse_ncx_flat(&s(&v, "OEBPS/toc.ncx"));
        let got: Vec<(usize, &str)> = flat.iter().map(|(d, l, _)| (*d, l.as_str())).collect();
        assert_eq!(got, [(1, "第一章"), (2, "第一节"), (2, "第二节")]);
        assert_toc_targets_exist(&v);
    }

    /// B：空页删掉后，正文里的目录页与 OPF `<guide>` 指向它的链接也改指下一篇。
    #[test]
    fn audit_empty_page_links_in_body_and_guide_retargeted() {
        let opf = OPF.replace("</package>", r#"<guide><reference type="text" href="pb.xhtml"/></guide></package>"#);
        let mut v = vec![
            e("content.opf", &opf),
            e("c1.xhtml", r#"<html><body><p><a href='pb.xhtml#x'>空页</a></p></body></html>"#),
            e("pb.xhtml", r#"<html><body><div class="mbppagebreak"></div></body></html>"#),
            e("c2.xhtml", "<html><body><p>c</p></body></html>"),
        ];
        wash_entries(&mut v, &WashOpts { auto_toc: AutoToc::Off, ..Default::default() }).unwrap();
        assert!(s(&v, "c1.xhtml").contains(r#"href='c2.xhtml'"#), "{}", s(&v, "c1.xhtml"));
        assert!(s(&v, "content.opf").contains(r#"<reference type="text" href="c2.xhtml"/>"#), "{}", s(&v, "content.opf"));
    }

    /// B：`<pageList>`、`<content id=… src=…>` 的 NCX 也能补节；重建后 nav 里的 landmarks 保留。
    #[test]
    fn audit_merge_sections_keeps_landmarks() {
        let long = "这是足够长的正文文字，确保标题页之后的内容超过三十个字这条门槛，不被当成书名页的作者行。";
        let mut v = paged_book(&[("c1.xhtml", &format!(r#"<h1 id="c1">第一章</h1><p>{long}</p><h2>第一节</h2><p>一节。</p><h2>第二节</h2><p>二节。</p>"#))]);
        v.push(e("OEBPS/toc.ncx", r#"<ncx><navMap><navPoint id="a"><navLabel><text>第一章</text></navLabel><content id="x" src="Text/c1.xhtml#c1"/></navPoint></navMap></ncx>"#));
        v.push(e("OEBPS/nav.xhtml", r#"<html><head><title>t</title></head><body><nav epub:type="toc"><h1>目录</h1><ol><li><a href="Text/c1.xhtml#c1">第一章</a></li></ol></nav><nav epub:type="landmarks"><ol><li><a href="Text/c1.xhtml">正文</a></li></ol></nav></body></html>"#));
        let opf = s(&v, "OEBPS/content.opf").replace("</manifest>", r#"<item id="ncx" href="toc.ncx" media-type="application/x-dtbncx+xml"/><item id="nav" href="nav.xhtml" properties="nav" media-type="application/xhtml+xml"/></manifest>"#);
        v[0].data = opf.into_bytes();
        let rep = wash_entries(&mut v, &WashOpts::default()).unwrap();
        assert_eq!(rep.toc_sections_added, 2);
        let nav = s(&v, "OEBPS/nav.xhtml");
        assert!(nav.contains(r#"epub:type="landmarks""#) && nav.contains("第二节"), "{nav}");
    }

    // ───────────────────────── 好读式结构：段落章名、数字节号 ─────────────────────────

    /// 好读的书：第一个正文文件的 `<h3>` 是书名、"第一章"只是一段字（目录标签就是它），节号是独占一段的 `１`、`２`。
    fn haodoo_book(c1_body: &str, c2_body: &str) -> Vec<Entry> {
        let mut v = paged_book(&[("1.xhtml", c1_body), ("2.xhtml", c2_body)]);
        v.push(e("OEBPS/toc.ncx", r#"<ncx><navMap><navPoint><navLabel><text>第一章</text></navLabel><content src="Text/1.xhtml"/></navPoint><navPoint><navLabel><text>第二章</text></navLabel><content src="Text/2.xhtml"/></navPoint></navMap></ncx>"#));
        let opf = s(&v, "OEBPS/content.opf").replace("</manifest>", r#"<item id="ncx" href="toc.ncx" media-type="application/x-dtbncx+xml"/></manifest>"#);
        v[0].data = opf.into_bytes();
        v
    }
    const HAODOO_TEXT: &str = "出了近鐵布施站之後，沿著鐵路往西走。已經十月了，天氣仍然悶熱難當，地面卻是乾的。每當卡車疾馳而過，揚起的塵土極可能會飛進眼睛。";

    /// 段落章名指到书名页后面那一段；独占一段的「１」「２」不当节（2026-10-08 用户定：纯数字不能当节的依据）。
    #[test]
    fn haodoo_paragraph_chapter_numbers_not_sections() {
        let t = HAODOO_TEXT;
        let mut v = haodoo_book(
            &format!("<div><h3>《白夜行》東野圭吾</h3><p>《好讀書櫃》典藏版</p><p>第一章</p><p>　　１</p><p>{t}</p><p>　　２</p><p>{t}</p></div>"),
            &format!("<div><h3>第二章</h3><p>１</p><p>{t}</p><p>２</p><p>{t}</p><p>３</p><p>{t}</p></div>"),
        );
        let rep = wash_entries(&mut v, &WashOpts::default()).unwrap();
        assert_eq!(rep.toc_sections_added, 0);
        let flat = crate::ncx::parse_ncx_flat(&s(&v, "OEBPS/toc.ncx"));
        let got: Vec<(usize, &str, &str)> = flat.iter().map(|(d, l, t)| (*d, l.as_str(), t.as_str())).collect();
        assert_eq!(got, [(1, "第一章", "Text/1.xhtml#eink-ch-1"), (1, "第二章", "Text/2.xhtml")], "第一章指到章名段落（不是书名页）");
        assert!(s(&v, "OEBPS/Text/1.xhtml").contains(r#"<p id="eink-ch-1">第一章</p>"#));
        assert_toc_targets_exist(&v);
        assert_eq!(spine_files(&v).len(), 2, "不拆文件");
    }

    #[test]
    fn numbered_paragraphs_that_are_not_sections() {
        let t = HAODOO_TEXT;
        // 目录样的一串数字（中间没有正文）、不从 1 开始的楼层号、断号的、只有一个的，都不是节
        for body in [
            format!("<div><h3>第二章</h3><p>一</p><p>二</p><p>三</p><p>{t}</p></div>"),
            format!("<div><h3>第二章</h3><p class=\"lc\">308</p><p>{t}</p><p class=\"lc\">309</p><p>{t}</p></div>"),
            format!("<div><h3>第二章</h3><p>１</p><p>{t}</p><p>３</p><p>{t}</p></div>"),
            format!("<div><h3>第二章</h3><p>１</p><p>{t}</p></div>"),
        ] {
            let mut v = haodoo_book(&format!("<div><h3>第一章</h3><p>{t}</p></div>"), &body);
            let rep = wash_entries(&mut v, &WashOpts::default()).unwrap();
            assert_eq!(rep.toc_sections_added, 0, "{body}");
            assert_eq!(crate::ncx::parse_ncx_flat(&s(&v, "OEBPS/toc.ncx")).len(), 2, "{body}");
        }
    }

    /// 书自带的 NCX（平的）：[(标签, 文件)]。
    fn flat_ncx_book(files: &[(&str, &str)], toc: &[(&str, &str)]) -> Vec<Entry> {
        let mut v = paged_book(files);
        let points: String = toc.iter().map(|(l, f)| format!("<navPoint><navLabel><text>{l}</text></navLabel><content src=\"Text/{f}\"/></navPoint>")).collect();
        v.push(e("OEBPS/toc.ncx", &format!("<ncx><navMap>{points}</navMap></ncx>")));
        let opf = s(&v, "OEBPS/content.opf").replace("</manifest>", r#"<item id="ncx" href="toc.ncx" media-type="application/x-dtbncx+xml"/></manifest>"#);
        v[0].data = opf.into_bytes();
        v
    }

    /// 和章同级的数字标题（《13級階梯》`<h3>２</h3>` 单独成文件、目录是平的）不降成节，目录原样（2026-10-08 用户定：只按结构判）。
    #[test]
    fn numbered_headings_at_chapter_level_stay_chapters() {
        let t = HAODOO_TEXT;
        let (c1, c1b, c1c) = (format!("<div><h3>第一章　出獄</h3><p>１</p><p>{t}</p></div>"), format!("<div><h3>２</h3><p>{t}</p></div>"), format!("<div><h3>３</h3><p>{t}</p></div>"));
        let (c2, c2b) = (format!("<div><h3>第二章　事件</h3><p>１</p><p>{t}</p></div>"), format!("<div><h3>２</h3><p>{t}</p></div>"));
        let toc = [("第一章　出獄　　１", "c1.xhtml"), ("　　２", "c1b.xhtml"), ("　　３", "c1c.xhtml"), ("第二章　事件　　１", "c2.xhtml"), ("　　２", "c2b.xhtml")];
        let mut v = flat_ncx_book(&[("c1.xhtml", &c1), ("c1b.xhtml", &c1b), ("c1c.xhtml", &c1c), ("c2.xhtml", &c2), ("c2b.xhtml", &c2b)], &toc);
        let rep = wash_entries(&mut v, &WashOpts::default()).unwrap();
        assert_eq!(rep.toc_sections_added, 0);
        let flat = crate::ncx::parse_ncx_flat(&s(&v, "OEBPS/toc.ncx"));
        let got: Vec<(usize, &str, String)> = flat.iter().map(|(d, l, t)| (*d, l.as_str(), t.clone())).collect();
        let want: Vec<(usize, &str, String)> = toc.iter().map(|(l, f)| (1, l.trim(), format!("Text/{f}"))).collect();
        assert_eq!(got, want);
    }

    /// 全书只有一串同级数字标题（《月亮和六便士》`<h3>二</h3>`… 是章），目录不改成两级。
    #[test]
    fn single_run_of_numbered_headings_stays_chapters() {
        let t = HAODOO_TEXT;
        let (c1, c2, c3) = (format!("<div><h3>《月亮和六便士》毛姆</h3><p>一</p><p>{t}</p></div>"), format!("<div><h3>二</h3><p>{t}</p></div>"), format!("<div><h3>三</h3><p>{t}</p></div>"));
        let mut v = flat_ncx_book(&[("1.xhtml", &c1), ("2.xhtml", &c2), ("3.xhtml", &c3)], &[("一", "1.xhtml"), ("二", "2.xhtml"), ("三", "3.xhtml")]);
        wash_entries(&mut v, &WashOpts::default()).unwrap();
        let flat = crate::ncx::parse_ncx_flat(&s(&v, "OEBPS/toc.ncx"));
        assert!(flat.iter().all(|(d, _, _)| *d == 1), "{flat:?}");
    }

    /// 阿加莎全集：书名下面挂着「1」「2」——书名是书/卷级，数字是章；章里更深一级的 `<hN>` 才是节，补进目录。
    #[test]
    fn numbered_chapters_under_book_titles() {
        let t = HAODOO_TEXT;
        let ch = |n: &str| format!("<div><h2>{n}</h2><p>{t}</p><h3>小标题{n}</h3><p>{t}</p></div>");
        let (a1, a2, b1, b2) = (ch("1"), ch("2"), ch("1"), ch("2"));
        let mut v = paged_book(&[("a1.xhtml", &a1), ("a2.xhtml", &a2), ("b1.xhtml", &b1), ("b2.xhtml", &b2)]);
        let np = |l: &str, f: &str, kids: &str| format!("<navPoint><navLabel><text>{l}</text></navLabel><content src=\"Text/{f}\"/>{kids}</navPoint>");
        let ncx = format!(
            "<ncx><navMap>{}{}</navMap></ncx>",
            np("ABC谋杀案", "a1.xhtml", &(np("1", "a1.xhtml", "") + &np("2", "a2.xhtml", ""))),
            np("无人生还", "b1.xhtml", &(np("1", "b1.xhtml", "") + &np("2", "b2.xhtml", "")))
        );
        v.push(e("OEBPS/toc.ncx", &ncx));
        let opf = s(&v, "OEBPS/content.opf").replace("</manifest>", r#"<item id="ncx" href="toc.ncx" media-type="application/x-dtbncx+xml"/></manifest>"#);
        v[0].data = opf.into_bytes();
        let rep = wash_entries(&mut v, &WashOpts::default()).unwrap();
        assert_eq!(rep.toc_sections_added, 4, "每章的 <h3> 是节");
        let flat = crate::ncx::parse_ncx_flat(&s(&v, "OEBPS/toc.ncx"));
        let labels: Vec<(usize, &str)> = flat.iter().map(|(d, l, _)| (*d, l.as_str())).collect();
        assert_eq!(labels, [(1, "ABC谋杀案"), (2, "1"), (3, "小标题1"), (2, "2"), (3, "小标题2"), (1, "无人生还"), (2, "1"), (3, "小标题1"), (2, "2"), (3, "小标题2")]);
        assert_toc_targets_exist(&v);
    }

    /// 章标签末尾的数字不动（《鼠疫》"部　一"）；章里独占一段的「一」「二」不当节。
    #[test]
    fn part_number_in_label_is_kept() {
        let t = HAODOO_TEXT;
        let (c1, c2) = (format!("<div><h3>部　一</h3><p>一</p><p>{t}</p><p>二</p><p>{t}</p></div>"), format!("<div><h3>部　二</h3><p>一</p><p>{t}</p><p>二</p><p>{t}</p></div>"));
        let mut v = flat_ncx_book(&[("1.xhtml", &c1), ("2.xhtml", &c2)], &[("部　一", "1.xhtml"), ("部　二", "2.xhtml")]);
        let rep = wash_entries(&mut v, &WashOpts::default()).unwrap();
        assert_eq!(rep.toc_sections_added, 0);
        let flat = crate::ncx::parse_ncx_flat(&s(&v, "OEBPS/toc.ncx"));
        let labels: Vec<(usize, &str)> = flat.iter().map(|(d, l, _)| (*d, l.as_str())).collect();
        assert_eq!(labels, [(1, "部　一"), (1, "部　二")]);
    }

    /// MOBI 转来的书（《福尔摩斯探案全集》）：没有 `<hN>`，目录锚点是章名段落前面的空 `<span id>`——取紧跟着的段落当标题。
    #[test]
    fn toc_anchor_on_empty_span_before_title_paragraph() {
        let t = HAODOO_TEXT;
        let body = format!(
            r#"<p>{t}</p><span id="a1"></span><p><b>第一章</b> <b>歇洛克</b></p><p>{t}</p><span id="a2"></span><p><b>第二章</b> <b>演绎法</b></p><p>{t}</p>"#
        );
        let mut v = paged_book(&[("p.xhtml", &body)]);
        v.push(e("OEBPS/toc.ncx", r#"<ncx><navMap><navPoint><navLabel><text>第一章　歇洛克</text></navLabel><content src="Text/p.xhtml#a1"/></navPoint><navPoint><navLabel><text>第二章　演绎法</text></navLabel><content src="Text/p.xhtml#a2"/></navPoint></navMap></ncx>"#));
        let opf = s(&v, "OEBPS/content.opf").replace("</manifest>", r#"<item id="ncx" href="toc.ncx" media-type="application/x-dtbncx+xml"/></manifest>"#);
        v[0].data = opf.into_bytes();
        wash_entries(&mut v, &WashOpts::default()).unwrap();
        assert_eq!(spine_files(&v), ["OEBPS/Text/p.xhtml"], "不拆文件");
        assert_eq!(crate::ncx::parse_ncx_flat(&s(&v, "OEBPS/toc.ncx")).len(), 2);
        assert_toc_targets_exist(&v);
    }

    /// 书自带目录指错位置：核实得上的改指（书里同名链接 / 全书唯一同名标题 / 锚点在文件末尾时下一个文件），核实不上、有歧义的不动。
    #[test]
    fn ncx_targets_repaired_only_when_verified() {
        let t = HAODOO_TEXT;
        let toc_page = r##"<p><a href="c1.xhtml#x1">第一章</a></p><p><a href="c2.xhtml#x2">第二章</a></p><p><a href="c3.xhtml">附录</a></p>"##;
        let (c1, c2) = (format!(r#"<h3 id="x1">第一章</h3><p>{t}</p>"#), format!(r#"<h3 id="x2">第二章</h3><p>{t}</p><span id="end"></span>"#));
        let c3 = format!(r#"<h3>附录</h3><p>{t}</p>"#);
        let c4 = format!(r#"<h3>后记</h3><p>{t}</p>"#);
        let mut v = paged_book(&[("toc.xhtml", toc_page), ("c1.xhtml", &c1), ("c2.xhtml", &c2), ("c3.xhtml", &c3), ("c4.xhtml", &c4)]);
        // 第一章指到第二章、第二章指到登场人物（都错）；附录指在 c2 末尾；后记写成了"跋"（核实不了）
        v.push(e(
            "OEBPS/toc.ncx",
            r#"<ncx><navMap><navPoint><navLabel><text>第一章</text></navLabel><content src="Text/c2.xhtml#x2"/></navPoint><navPoint><navLabel><text>第二章</text></navLabel><content src="Text/c1.xhtml"/></navPoint><navPoint><navLabel><text>附录</text></navLabel><content src="Text/c2.xhtml#end"/></navPoint><navPoint><navLabel><text>跋</text></navLabel><content src="Text/c3.xhtml"/></navPoint></navMap></ncx>"#,
        ));
        let opf = s(&v, "OEBPS/content.opf").replace("</manifest>", r#"<item id="ncx" href="toc.ncx" media-type="application/x-dtbncx+xml"/></manifest>"#);
        v[0].data = opf.into_bytes();
        let mut rep = WashReport::default();
        super::toc::repair_ncx_targets(&mut v, &mut rep);
        let flat = crate::ncx::parse_ncx_flat(&s(&v, "OEBPS/toc.ncx"));
        let got: Vec<(&str, &str)> = flat.iter().map(|(_, l, t)| (l.as_str(), t.as_str())).collect();
        assert_eq!(got, [("第一章", "Text/c1.xhtml#x1"), ("第二章", "Text/c2.xhtml#x2"), ("附录", "Text/c3.xhtml"), ("跋", "Text/c3.xhtml")]);
        assert_eq!(rep.ncx_targets_repaired, 3);

        // 同一标题核实得上的地方不止一处：拿不准，不改
        let (d1, d2) = (format!(r#"<h3 id="y">第一章</h3><p>{t}</p>"#), format!(r#"<h3 id="z">第一章</h3><p>{t}</p>"#));
        let links = r##"<p><a href="d1.xhtml#y">第一章</a></p><p><a href="d2.xhtml#z">第一章</a></p><p><a href="d1.xhtml">第一章</a></p>"##;
        let mut w = paged_book(&[("toc.xhtml", links), ("d1.xhtml", &d1), ("d2.xhtml", &d2)]);
        w.push(e("OEBPS/toc.ncx", r#"<ncx><navMap><navPoint><navLabel><text>第一章</text></navLabel><content src="Text/toc.xhtml"/></navPoint></navMap></ncx>"#));
        let opf = s(&w, "OEBPS/content.opf").replace("</manifest>", r#"<item id="ncx" href="toc.ncx" media-type="application/x-dtbncx+xml"/></manifest>"#);
        w[0].data = opf.into_bytes();
        let mut rep = WashReport::default();
        super::toc::repair_ncx_targets(&mut w, &mut rep);
        assert_eq!(rep.ncx_targets_repaired, 0);
    }

    #[test]
    fn section_number_values() {
        use super::chapters::section_number;
        for (t, n) in [("１", 1), ("　　12", 12), ("一", 1), ("十", 10), ("十二", 12), ("二十", 20), ("九十九", 99)] {
            assert_eq!(section_number(t), Some(n), "{t}");
        }
        for t in ["", "一十", "十十", "1234", "第一", "１a", "百"] {
            assert_eq!(section_number(t), None, "{t}");
        }
    }

    // ───────────────────────── 2026-09-28 审计复现的问题 ─────────────────────────

    const LONG: &str = "这是足够长的正文文字，确保标题页之后的内容超过三十个字这条门槛，不被当成书名页的作者行。";

    /// H3：单引号 OPF 里的空页也删得掉，manifest 的 href 不被改成邻页（此前 spine 重复一章）。
    #[test]
    fn audit_h3_empty_page_in_single_quoted_opf() {
        let mut v = vec![
            e("META-INF/container.xml", r#"<container><rootfiles><rootfile full-path="OEBPS/content.opf"/></rootfiles></container>"#),
            e("OEBPS/content.opf", "<package version='3.0'><metadata><dc:title>B</dc:title></metadata><manifest><item id='c0' href='c0.xhtml' media-type='application/xhtml+xml'/><item id='c1' href='c1.xhtml' media-type='application/xhtml+xml'/></manifest><spine><itemref idref='c0'/><itemref idref='c1'/></spine><guide><reference type='text' href='c0.xhtml'/></guide></package>"),
            e("OEBPS/c0.xhtml", "<html><body><p> </p></body></html>"),
            e("OEBPS/c1.xhtml", &format!("<html><body><p>{LONG}</p></body></html>")),
        ];
        let rep = wash_entries(&mut v, &WashOpts::default()).unwrap();
        assert_eq!(rep.empty_pages_removed, ["OEBPS/c0.xhtml"]);
        let opf = s(&v, "OEBPS/content.opf");
        assert!(!opf.contains("c0.xhtml") || opf.contains("<reference type='text' href='c1.xhtml'/>"), "{opf}");
        assert!(opf.contains("<item id='c1' href='c1.xhtml'") && !opf.contains("id='c0'"), "{opf}");
        assert_eq!(spine_files(&v), ["OEBPS/c1.xhtml"]);
    }

    /// H4：`Chapter 1`、`Part 2`、`卷 一` 是这一条自己的编号，自动目录里不拆成两级。
    #[test]
    fn audit_h4_chapter_number_not_split() {
        let mut v = paged_book(&[("c1.xhtml", &format!("<h2>Chapter 1</h2><p>{LONG}</p><h2>Chapter 2</h2><p>{LONG}</p>"))]);
        wash_entries(&mut v, &WashOpts::default()).unwrap();
        let nav = s(&v, "OEBPS/nav.xhtml");
        assert!(nav.contains(">Chapter 1</a></li><li>") && !nav.contains("<ol><li><a href=\"Text/c1.xhtml#eink-toc-1\">1</a>"), "{nav}");
    }

    /// H5：已经分了层级的目录不压平；扁平目录重建时 navPoint 的 id 与 pageList 保留。
    #[test]
    fn audit_h5_nested_toc_untouched_flat_toc_keeps_ids_and_page_list() {
        let opf = r#"<package version="2.0" unique-identifier="u"><metadata><dc:identifier id="u">x</dc:identifier><dc:title>B</dc:title></metadata><manifest><item id="ncx" href="toc.ncx" media-type="application/x-dtbncx+xml"/><item id="c1" href="c1.xhtml" media-type="application/xhtml+xml"/></manifest><spine toc="ncx"><itemref idref="c1"/></spine></package>"#;
        let body = format!("<html><body><p id='a'>第一部</p><p id='b'>第一章</p><p id='c'>第一节</p><p>{LONG}</p></body></html>");
        let nested = r#"<ncx><head><meta name="dtb:uid" content="x"/></head><navMap><navPoint id="p1" playOrder="1"><navLabel><text>第一部</text></navLabel><content src="c1.xhtml#a"/><navPoint id="p2" playOrder="2"><navLabel><text>第一章</text></navLabel><content src="c1.xhtml#b"/><navPoint id="p3" playOrder="3"><navLabel><text>第一节</text></navLabel><content src="c1.xhtml#c"/></navPoint></navPoint></navPoint></navMap></ncx>"#;
        let mut v = vec![e("content.opf", opf), e("toc.ncx", nested), e("c1.xhtml", &body)];
        let rep = wash_entries(&mut v, &WashOpts::default()).unwrap();
        assert_eq!(rep.toc_parts_restructured, 0);
        assert_eq!(s(&v, "toc.ncx"), nested, "已嵌套的目录原样");
        let flat = r#"<ncx><head><meta name="dtb:uid" content="x"/><meta name="dtb:depth" content="1"/></head><docTitle><text>B</text></docTitle><navMap><navPoint id="n1" playOrder="1"><navLabel><text>第一部　第一章</text></navLabel><content src="c1.xhtml#a"/></navPoint><navPoint id="n2" playOrder="2"><navLabel><text>第一节</text></navLabel><content src="c1.xhtml#c"/></navPoint></navMap><pageList><pageTarget id="pg1" type="normal" value="1" playOrder="3"><navLabel><text>1</text></navLabel><content src="c1.xhtml#a"/></pageTarget></pageList></ncx>"#;
        let mut w = vec![e("content.opf", opf), e("toc.ncx", flat), e("c1.xhtml", &body)];
        let rep = wash_entries(&mut w, &WashOpts::default()).unwrap();
        assert_eq!(rep.toc_parts_restructured, 3);
        let out = s(&w, "toc.ncx");
        assert!(out.contains(r#"<navPoint id="n1" playOrder="1"><navLabel><text>第一部</text></navLabel><content src="c1.xhtml#a"/><navPoint id="eink-np-1" playOrder="2"><navLabel><text>第一章</text>"#), "{out}");
        assert!(out.contains(r#"<navPoint id="n2" playOrder="3"><navLabel><text>第一节</text>"#), "{out}");
        assert!(out.contains(r#"<pageList><pageTarget id="pg1""#) && out.contains("<docTitle><text>B</text></docTitle>"), "NCX 其余部分原样: {out}");
        assert!(out.contains(r#"<meta name="dtb:depth" content="2"/>"#), "{out}");
    }

    /// M3：样式表开头的 `@import` 不再让第一条规则整条跳过（字体锁照剥）。
    #[test]
    fn audit_m3_import_does_not_swallow_first_rule() {
        let out = filter_css("@charset \"utf-8\";\n@import url(a.css);\np{font-size:12pt;color:red}\nh1{font-size:2em}", &WashOpts::default());
        assert!(out.starts_with("@charset \"utf-8\";\n@import url(a.css);\np{"), "{out}");
        assert!(!out.contains("font-size:12pt") && out.contains("color:red;"), "{out}");
        assert_eq!(crate::cascade::split_leading_statements("@import url('a;b.css');\n.x"), ("@import url('a;b.css');", "\n.x"));
    }

    /// L：`margin:inherit` 不再写成非法的 `margin:0 inherit`；`!important` 与括号里的空格都认。
    #[test]
    fn audit_box_shorthand_keywords_and_important() {
        let f: Vec<String> = DEFAULT_FILTER_PROPS.iter().map(|s| s.to_string()).collect();
        assert_eq!(filter_decls("margin:inherit", &f, Spacing::Vertical), "margin-right:inherit;margin-left:inherit;");
        assert_eq!(filter_decls("margin:1em 2em !important", &f, Spacing::Vertical), "margin:0 2em !important;");
        assert_eq!(filter_decls("padding:calc(1em + 2px) 3px", &f, Spacing::Vertical), "padding:0 3px;");
        assert_eq!(box_sides("1px 2px 3px"), BoxSides::Sides(["1px", "2px", "3px", "2px"], ""));
        assert_eq!(box_sides("1px 2px 3px 4px 5px"), BoxSides::Unknown);
    }

    /// L：nav 文档不叫 nav.xhtml（OPF 里 `properties="nav"` 的 toc.xhtml）、又没有 NCX 时，也算书有目录，不被自动目录覆盖。
    #[test]
    fn audit_nav_named_toc_xhtml_counts_as_existing_toc() {
        let opf = r#"<package version="3.0"><metadata><dc:title>B</dc:title></metadata><manifest><item id="t" href="contents.xhtml" media-type="application/xhtml+xml" properties="nav"/><item id="c1" href="c1.xhtml" media-type="application/xhtml+xml"/></manifest><spine><itemref idref="c1"/></spine></package>"#;
        let nav = r#"<html><body><nav epub:type="toc"><ol><li><a href="c1.xhtml">作者的目录</a></li></ol></nav></body></html>"#;
        let mut v = vec![e("content.opf", opf), e("contents.xhtml", nav), e("c1.xhtml", &format!("<html><body><h1>一</h1><p>{LONG}</p></body></html>"))];
        let rep = wash_entries(&mut v, &WashOpts::default()).unwrap();
        assert_eq!(rep.toc_generated, 0);
        assert!(s(&v, "contents.xhtml").contains("作者的目录"));
    }

    /// L：封面声明认单引号的 manifest 项（此前 `replacen` 按双引号找 `id="…"`，补不上 `properties`）。
    #[test]
    fn audit_cover_declared_in_single_quoted_opf() {
        let mut v = vec![
            e("content.opf", "<package version='3.0'><metadata><dc:title>B</dc:title></metadata><manifest><item id='img' href='i/c.jpg' media-type='image/jpeg'/><item id='p' href='p.xhtml' media-type='application/xhtml+xml'/></manifest><spine><itemref idref='p'/></spine></package>"),
            e("p.xhtml", "<html><body><img src='i/c.jpg'/></body></html>"),
        ];
        assert!(ensure_cover_declared(&mut v));
        let opf = s(&v, "content.opf");
        assert!(opf.contains(r#"<item id='img' properties="cover-image" href='i/c.jpg'"#) && opf.contains(r#"<meta name="cover" content="img"/></metadata>"#), "{opf}");
        assert!(!ensure_cover_declared(&mut v), "幂等");
    }

    // ───────────────────────── 2026-09-30 审计 ─────────────────────────

    /// B2：整段只有空白的段落（作者留的空行）不剥成 `<p></p>`；有正文的段首空白照剥。
    #[test]
    fn audit_b2_blank_paragraphs_kept() {
        let h = "<html><body><p>　　正文一</p><p>&nbsp;</p><p>　</p><p> </p><p>&#12288;<span>乙</span></p><p>\u{3000}</p></body></html>";
        assert_eq!(cjk_paragraphize(h), "<html><body><p>正文一</p><p>&nbsp;</p><p>　</p><p> </p><p><span>乙</span></p><p>\u{3000}</p></body></html>");
        assert_eq!(cjk_paragraphize("<p>　<img src='a.png'/></p><p>　</p>"), "<p><img src='a.png'/></p><p>　</p>", "图片也算内容");
    }

    /// B3：选择器前面的 `/* … */` 注释不参与判断（注释原样留在输出里）。
    #[test]
    fn audit_b3_css_comments_not_part_of_selector() {
        let o = WashOpts::default();
        // 注释里提到 p：.note 不是 p 规则，上下边距不动
        let out = filter_css("/* p 的样式 */ .note{margin:1em 2em}", &o);
        assert_eq!(out, "/* p 的样式 */ .note{margin:1em 2em;}");
        // 注释里提到 footnote：普通规则不当注释容器（不加注释字号、不剥字重）
        let out = filter_css("/* footnote 在后面 */\n.big{font-weight:bold}", &o);
        assert_eq!(out, "/* footnote 在后面 */\n.big{font-weight:bold;}");
        // 注释后面的 @font-face 照样认出来，字体名不剥
        let face = "/* fonts */ @font-face{font-family:\"A\";src:url(a.ttf)}";
        assert_eq!(filter_css(face, &o), face);
        // 注释后面的 @import 也照样拆出来
        let out = filter_css("/* x; y */ @import url(a.css);\np{font-size:12pt;color:red}", &o);
        assert!(out.starts_with("/* x; y */ @import url(a.css);\np{") && !out.contains("12pt"), "{out}");
        // 章尾容器、首行缩进类、会画线的类也不受注释影响
        let mut cls = HashSet::new();
        cls.insert("tail".to_string());
        assert_eq!(strip_tail_spacing("/* 结尾 */ .tail{margin-bottom:1em;color:red}", &cls), "/* 结尾 */ .tail{color:red;}");
        assert!(indent_classes_of("/* .fake */ .real{text-indent:2em}").iter().eq(["real".to_string()].iter()));
    }

    /// B4：属性值里的 `<` 一律转义（标签之间的 `<字母` 照旧不动）。
    #[test]
    fn audit_b4_lt_in_attribute_escaped() {
        let mut fx = normalize::XmlFixes::default();
        let out = normalize::normalize_markup(r#"<html><body><img alt="<b>x</b>" src="a.png"/><p title=a<b>t</p></body></html>"#, true, &mut fx);
        assert!(out.contains(r#"<img alt="&lt;b>x&lt;/b>" src="a.png"/>"#) && out.contains(r#"<p title="a&lt;b">t</p>"#), "{out}");
        assert_eq!(fx.bare_lts, 3);
    }

    /// B5：目录链接里的字符引用只转义一次（重建目录、补节、按标题生成目录都不会把 `&amp;` 写成 `&amp;amp;`）。
    #[test]
    fn audit_b5_toc_srcs_not_double_escaped() {
        let opf = r#"<package version="2.0" unique-identifier="u"><metadata><dc:identifier id="u">x</dc:identifier><dc:title>B</dc:title></metadata><manifest><item id="ncx" href="toc.ncx" media-type="application/x-dtbncx+xml"/><item id="c1" href="a&amp;b.xhtml" media-type="application/xhtml+xml"/><item id="c2" href="c2.xhtml" media-type="application/xhtml+xml"/></manifest><spine toc="ncx"><itemref idref="c1"/><itemref idref="c2"/></spine></package>"#;
        let ncx = r#"<ncx><head><meta name="dtb:uid" content="x"/></head><navMap><navPoint id="n1"><navLabel><text>第一部　楔子</text></navLabel><content src="a&amp;b.xhtml#x&amp;y"/></navPoint><navPoint id="n2"><navLabel><text>尾声</text></navLabel><content src="c2.xhtml"/></navPoint></navMap></ncx>"#;
        let mut v = vec![e("content.opf", opf), e("toc.ncx", ncx), e("a&b.xhtml", "<html><body><p id='x&amp;y'>楔子</p></body></html>"), e("c2.xhtml", "<html><body><p>尾声</p></body></html>")];
        let rep = wash_entries(&mut v, &WashOpts::default()).unwrap();
        assert_eq!(rep.toc_parts_restructured, 3);
        let out = s(&v, "toc.ncx");
        assert!(out.contains(r#"<content src="a%26b.xhtml#x&amp;y"/>"#) && !out.contains("&amp;amp;"), "{out}");
        // 按标题生成目录：标题 id 里的 `&amp;` 进目录时不再转义第二次
        let mut w = paged_book(&[("c1.xhtml", &format!("<h1 id='a&amp;b'>第一章</h1><p>{LONG}</p><h1>第二章</h1><p>{LONG}</p>"))]);
        wash_entries(&mut w, &WashOpts::default()).unwrap();
        let ncx = s(&w, "OEBPS/toc.ncx");
        assert!(ncx.contains(r#"src="Text/c1.xhtml#a&amp;b""#) && !ncx.contains("&amp;amp;"), "{ncx}");
        let nav = s(&w, "OEBPS/nav.xhtml");
        assert!(nav.contains(r#"href="Text/c1.xhtml#a&amp;b""#) && !nav.contains("&amp;amp;"), "{nav}");
    }

    /// B6：只有文件名正好是 nav.xhtml/nav.html 的才按名字算目录文件。
    #[test]
    fn audit_b6_nav_prefixed_chapters_are_not_toc_files() {
        assert!(is_toc_file("OEBPS/nav.xhtml") && is_toc_file("NAV.html") && is_toc_file("x/toc.ncx"));
        assert!(!is_toc_file("Text/navarre.xhtml") && !is_toc_file("navy.html") && !is_toc_file("nav-1.xhtml"));
    }

    /// 带命名空间前缀的 OPF：不拆文件，OPF 不加项；读 DC 元数据不认注释里的。
    #[test]
    fn audit_prefixed_opf_not_split_and_dc_ignores_comments() {
        let opf = r#"<opf:package xmlns:opf="http://www.idpf.org/2007/opf" version="3.0"><opf:metadata><dc:title>书</dc:title></opf:metadata><opf:manifest><opf:item id="c0" href="Text/c1.xhtml" media-type="application/xhtml+xml"/></opf:manifest><opf:spine><opf:itemref idref="c0"/></opf:spine></opf:package>"#;
        let body = format!("<h1>第一章</h1><p>{LONG}</p><h1>第二章</h1><p>{LONG}</p>");
        let mut v = vec![e("OEBPS/content.opf", opf), e("OEBPS/Text/c1.xhtml", &format!("<html><head><title>t</title></head><body>{body}</body></html>"))];
        wash_entries(&mut v, &WashOpts::default()).unwrap();
        let o = s(&v, "OEBPS/content.opf");
        assert!(!o.contains("c0-p2"), "{o}");
        assert_eq!(spine_files(&v), ["OEBPS/Text/c1.xhtml"]);
        let dc = opf_dc(r#"<metadata><!-- <dc:title>旧</dc:title> --><dc:title/><dc:title id='t' data-x="a>b">新 &amp; 书</dc:title ><dc:creator>甲</dc:creator></metadata>"#);
        assert_eq!((dc.title.as_str(), dc.creators), ("新 & 书", vec!["甲".to_string()]));
    }

    /// 不可信的 NCX 嵌套超过 255 层：目录补节、重写 NCX/nav 时层级到 u8 为止，不溢出（此前 debug 构建 panic、release 回绕成 0 层）。
    #[test]
    fn audit_toc_deeper_than_u8_does_not_overflow() {
        let opf = r#"<package version="2.0"><metadata><dc:title>书</dc:title></metadata><manifest><item id="ncx" href="toc.ncx" media-type="application/x-dtbncx+xml"/><item id="a" href="a.xhtml" media-type="application/xhtml+xml"/></manifest><spine toc="ncx"><itemref idref="a"/></spine></package>"#;
        let mut ncx = String::from("<ncx><head></head><navMap>");
        for i in 0..300 {
            ncx.push_str(&format!("<navPoint id=\"n{i}\"><navLabel><text>第{i}层</text></navLabel><content src=\"a.xhtml\"/>"));
        }
        ncx.push_str(&"</navPoint>".repeat(300));
        ncx.push_str("</navMap></ncx>");
        let page = format!("<html><body><p>{LONG}</p><h2 id=\"s1\">一节</h2><p>{LONG}</p></body></html>");
        let mut v = vec![e("OEBPS/content.opf", opf), e("OEBPS/toc.ncx", &ncx), e("OEBPS/a.xhtml", &page)];
        let added = toc::merge_sections_into_toc(&mut v, &[toc::SectionRef { path: "OEBPS/a.xhtml".into(), id: "s1".into(), label: "一节".into() }], "目录");
        assert_eq!(added, 1);
        let out = s(&v, "OEBPS/toc.ncx");
        assert_eq!(out.matches("<navPoint ").count(), 301);
        assert_eq!(out.matches("</navPoint>").count(), 301, "{out}");
        let items: Vec<TocItem> = (1..=255u8).chain([255, 1]).map(|l| TocItem::new(l, "x", "OEBPS/a.xhtml", "")).collect();
        let built = toc::build_ncx(&items, "OEBPS", "书", "u");
        assert_eq!(built.matches("<navPoint ").count(), built.matches("</navPoint>").count());
        let nav = toc::build_nav(&items, "OEBPS", "目录");
        assert_eq!(nav.matches("<ol>").count(), nav.matches("</ol>").count());
    }

    // ───────── 2026-10-08 清洗层审计（`audit_*` 复现用例搬过来，改成断言） ─────────

    const AUDIT_OPF: &str = r#"<?xml version="1.0"?><package version="2.0"><metadata><dc:title>t</dc:title></metadata><manifest><item id="ncx" href="toc.ncx" media-type="application/x-dtbncx+xml"/><item id="c1" href="c1.xhtml" media-type="application/xhtml+xml"/></manifest><spine toc="ncx"><itemref idref="c1"/></spine></package>"#;

    /// 锚点在最后一个 `</body>` 后面：以前切片越界 panic，整本失败。
    #[test]
    fn audit_anchor_after_body_no_panic() {
        let ncx = r#"<ncx><navMap><navPoint id="a"><navLabel><text>第一章</text></navLabel><content src="c1.xhtml#x"/></navPoint><navPoint id="b"><navLabel><text>第二章</text></navLabel><content src="c1.xhtml"/></navPoint></navMap></ncx>"#;
        let c1 = r#"<html><head></head><body><p>正文</p></body><p id="x">尾巴</p></html>"#;
        let mut v = vec![e("content.opf", AUDIT_OPF), e("toc.ncx", ncx), e("c1.xhtml", c1)];
        let mut rep = WashReport::default();
        toc::repair_ncx_targets(&mut v, &mut rep);
        let mut v = vec![e("content.opf", AUDIT_OPF), e("toc.ncx", ncx), e("c1.xhtml", c1)];
        assert!(wash_entries(&mut v, &WashOpts { repair_only: true, kindle_rules: true, ..Default::default() }).is_ok());
    }

    /// `@charset` 开头的样式表：第一条规则以前被当成选择器的一部分，没去掉下边距。
    #[test]
    fn audit_tail_spacing_after_charset() {
        let cls: HashSet<String> = ["chap".to_string()].into_iter().collect();
        let out = layout::strip_tail_spacing("@charset \"utf-8\";\n.chap{margin-bottom:2em;color:red}", &cls);
        assert_eq!(out, "@charset \"utf-8\";\n.chap{color:red;}");
    }

    /// 前导零任意多的数字引用都合法（以前只认 8 位，`&#000000065;` 被转义成字面文字）；有效位太多的算非法字符。
    #[test]
    fn audit_long_numeric_ref() {
        let mut fx = normalize::XmlFixes::default();
        assert_eq!(normalize::normalize_markup("<p>&#000000065;&#x00000000041;</p>", true, &mut fx), "<p>&#000000065;&#x00000000041;</p>");
        assert_eq!(normalize::normalize_markup("<p>a&#99999999999;b</p>", true, &mut fx), "<p>ab</p>");
    }

    /// `src="a&amp;b.jpg"` 指的是 `a&b.jpg`：以前不还原字符引用，图被当成死引用删了。
    #[test]
    fn audit_dead_img_with_escaped_ampersand() {
        let opf = r#"<?xml version="1.0"?><package version="2.0"><metadata><dc:title>t</dc:title></metadata><manifest><item id="c1" href="c1.xhtml" media-type="application/xhtml+xml"/><item id="i" href="a&amp;b.jpg" media-type="image/jpeg"/></manifest><spine><itemref idref="c1"/></spine></package>"#;
        let c1 = r#"<html><head></head><body><p>正文</p><img src="a&amp;b.jpg"/><img src="gone.jpg"/></body></html>"#;
        let mut v = vec![e("content.opf", opf), e("c1.xhtml", c1), e("a&b.jpg", "x")];
        let rep = wash_entries(&mut v, &WashOpts { repair_only: true, ..Default::default() }).unwrap();
        let out = s(&v, "c1.xhtml");
        assert!(out.contains(r#"<img src="a&amp;b.jpg"/>"#) && !out.contains("gone.jpg"), "{out}");
        assert_eq!(rep.dead_refs_removed, 1);
    }

    /// 注入的 `<link href>`：百分号编码（目录名有空格）、XML 转义，重复注入不叠加。
    #[test]
    fn audit_injected_link_href_encoded() {
        let h = "<html><head><title>x</title></head><body></body></html>";
        let once = typeset::inject_css_link_first(h, "a&b.css");
        assert!(once.contains(r#"href="a&amp;b.css""#), "{once}");
        assert_eq!(typeset::inject_css_link_first(&once, "a&b.css"), once);
        assert_eq!(typeset::inject_css_link(&once, "a&b.css"), once);
        let opf = r#"<?xml version="1.0"?><package version="2.0"><metadata><dc:title>t</dc:title></metadata><manifest><item id="c1" href="../A%26B/c1.xhtml" media-type="application/xhtml+xml"/></manifest><spine><itemref idref="c1"/></spine></package>"#;
        let mut v = vec![e("O P/content.opf", opf), e("A&B/c1.xhtml", "<html><head><title>x</title></head><body><p>正文</p></body></html>")];
        wash_entries(&mut v, &WashOpts { repair_only: true, kindle_rules: true, ..Default::default() }).unwrap();
        let out = s(&v, "A&B/c1.xhtml");
        assert!(out.contains(r#"href="../O%20P/eink-ua.css""#), "{out}");
    }

    /// 不是 UTF-8 的 html：自动目录只收已有 id 的标题，不补 id、不写回（以前按 lossy 转出的文字写回，原字节被改坏）。
    #[test]
    fn audit_non_utf8_html_not_rewritten() {
        let raw: &[u8] = b"<html><body><h1>\xb5\xda\xd2\xbb</h1><h2 id=\"a\">x</h2></body></html>";
        let mut v = vec![Entry { name: "c1.xhtml".into(), data: raw.to_vec() }];
        let items = toc::collect_toc_headings(&mut v, &["c1.xhtml".to_string()], None);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].frag, "a");
        assert_eq!(v[0].data, raw);
    }

    /// 补节进目录：节插在所属的章后面、深一级；按位置排好的走二分（和逐条插入的结果相同）。
    #[test]
    fn merge_sections_places_each_under_its_chapter() {
        let opf = r#"<?xml version="1.0"?><package version="2.0"><metadata><dc:title>t</dc:title></metadata><manifest><item id="ncx" href="toc.ncx" media-type="application/x-dtbncx+xml"/><item id="c1" href="c1.xhtml" media-type="application/xhtml+xml"/><item id="c2" href="c2.xhtml" media-type="application/xhtml+xml"/></manifest><spine toc="ncx"><itemref idref="c1"/><itemref idref="c2"/></spine></package>"#;
        let ncx = r#"<ncx><navMap><navPoint id="a"><navLabel><text>第一章</text></navLabel><content src="c1.xhtml"/></navPoint><navPoint id="b"><navLabel><text>第二章</text></navLabel><content src="c2.xhtml"/></navPoint></navMap></ncx>"#;
        let c1 = r#"<html><body><h1>第一章</h1><h2 id="s1">一</h2><p>甲</p><h2 id="s2">二</h2><p>乙</p></body></html>"#;
        let c2 = r#"<html><body><h1>第二章</h1><h2 id="s3">三</h2><p>丙</p></body></html>"#;
        let sec = |p: &str, id: &str, l: &str| toc::SectionRef { path: p.into(), id: id.into(), label: l.into() };
        let book = || vec![e("content.opf", opf), e("toc.ncx", ncx), e("c1.xhtml", c1), e("c2.xhtml", c2)];
        let want = [(1, "第一章"), (2, "一"), (2, "二"), (1, "第二章"), (2, "三")];
        let got = |v: &[Entry]| crate::ncx::parse_nav_points(&s(v, "toc.ncx")).into_iter().map(|p| (p.depth, p.label)).collect::<Vec<_>>();
        let mut v = book();
        assert_eq!(toc::merge_sections_into_toc(&mut v, &[sec("c1.xhtml", "s1", "一"), sec("c1.xhtml", "s2", "二"), sec("c2.xhtml", "s3", "三")], "目录"), 3);
        assert_eq!(got(&v), want.iter().map(|(d, l)| (*d, l.to_string())).collect::<Vec<_>>());
        // 位置没排好（逐条插入的老办法）：结果一样
        let mut v = book();
        toc::merge_sections_into_toc(&mut v, &[sec("c2.xhtml", "s3", "三"), sec("c1.xhtml", "s1", "一"), sec("c1.xhtml", "s2", "二")], "目录");
        assert_eq!(got(&v), want.iter().map(|(d, l)| (*d, l.to_string())).collect::<Vec<_>>());
    }

    /// 选择器最后一段：按空白和 `>`、`+`、`~` 切；标签、类按 `.` 拆，伪类和属性选择器不算。
    #[test]
    fn last_compound_of_selectors() {
        assert_eq!(crate::cascade::last_compound(" div.a > p.b:first-child "), "p.b:first-child");
        assert_eq!(crate::cascade::last_compound("h1+p.x"), "p.x");
        assert_eq!(crate::cascade::compound_tag_classes("P.a.b[title]"), ("p".to_string(), vec!["a", "b"]));
        assert_eq!(crate::cascade::compound_tag_classes(".x"), (String::new(), vec!["x"]));
    }

    /// 分部前缀认全角数字、「零」「两」和「编」（以前只有定章节那套认，目录重建这套不认）。
    #[test]
    fn part_prefix_shares_chapter_numbers() {
        for t in ["第１部　雪人", "第两卷", "第一编 总论", "第一〇部"] {
            assert!(toc::part_prefix_re().is_match(t), "{t}");
        }
        assert!(!toc::part_prefix_re().is_match("第一章"));
    }
