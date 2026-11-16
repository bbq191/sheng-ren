//! 原 `kindle_rules.rs` 的单元测试（拆分后测试路径不变：`wash::kindle_rules::tests::…`）。
use super::*;

fn ctx() -> Ctx {
    Ctx {
        body_fonts: ["宋体".to_string()].into_iter().collect(),
        body_classes: ["calibre".to_string()].into_iter().collect(),
        neg_left: false,
        neg_right: false,
        font_scale: None,
        opaque_rgba: false,
        drop_single_backgrounds: false,
        tiled: HashSet::new(),
        bg_fixes: HashMap::new(),
        light_on_page: HashSet::new(),
    }
}

#[test]
fn rules_like_send_to_kindle() {
    let c = ctx();
    let mut n = 0;
    let css = r#"body{margin:0 5pt;font-family:"宋体",serif} .calibre{padding-left:1em;margin-top:1em} p{font-family:"宋体"} h1{color:#fff;background-color:#f0a200} .pale{color:#f5ac00} .dark{background-color:#0168b7} .ok{color:#333}"#;
    let out = rewrite_css(css, &c, &mut n);
    assert!(out.contains("body{margin-top:0;margin-bottom:0;}"), "{out}");
    assert!(
        out.contains(".calibre{margin-top:1em}"),
        "只用在 body 上的类去掉左右：{out}"
    );
    assert!(out.contains("p{}"), "正文字体整条去掉：{out}");
    let white = rewrite_css(".t{color:#fff}", &c, &mut n);
    assert_eq!(
        white, ".t{color:#fff}",
        "没写背景色的白字不动（多半压在背景图上）"
    );
    assert!(
        out.contains("h1{color:#454545;background-color:#f0a200}")
            && out.contains(".pale{color:#9d6e00;}"),
        "{out}"
    );
    assert!(
        out.contains(".dark{background-color:#0168b7;color:#e4e4e4;}")
            && out.contains(".ok{color:#333}"),
        "{out}"
    );
    // 有负的左外边距：body 的左边距照留
    let c2 = Ctx {
        neg_left: true,
        ..ctx()
    };
    let out2 = rewrite_css("body{margin:0 5pt}", &c2, &mut n);
    assert_eq!(out2, "body{margin-top:0;margin-bottom:0;margin-left:5pt;}");
}

/// 负外边距那一侧：分项、简写的 margin 和 padding 都照留（以前简写的 padding 左右照删，和分项的 `padding-left` 不一致）。
#[test]
fn negative_side_kept_for_padding_shorthand_too() {
    let c = Ctx {
        neg_left: true,
        ..ctx()
    };
    let mut n = 0;
    assert_eq!(
        rewrite_css("body{padding-left:3em}", &c, &mut n),
        "body{padding-left:3em}"
    );
    assert_eq!(
        rewrite_css("body{padding:0 3em}", &c, &mut n),
        "body{padding-top:0;padding-bottom:0;padding-left:3em;}"
    );
    assert_eq!(
        rewrite_css("body{margin:1em 0 1em 2em}", &c, &mut n),
        "body{margin-top:1em;margin-bottom:1em;margin-left:2em;}"
    );
}

/// 带空格的 `calc()` 不再被空格拆坏（以前 `calc(1em` `+` `2px)` 当成三个值，写出坏 CSS）。
#[test]
fn calc_in_box_shorthand_not_broken() {
    let c = ctx();
    let mut n = 0;
    assert_eq!(
        rewrite_css("body{margin:0 calc(1em + 2px)}", &c, &mut n),
        "body{margin-top:0;margin-bottom:0;}"
    );
    let c2 = Ctx {
        neg_left: true,
        ..ctx()
    };
    assert_eq!(rewrite_css("body{margin:0 calc(1em + 2px) !important}", &c2, &mut n), "body{margin-top:0 !important;margin-bottom:0 !important;margin-left:calc(1em + 2px) !important;}");
}

/// `font` 简写里的正文字体：拆成分项、不写字体族；别的字体、拆不清的不动。
#[test]
fn body_font_in_font_shorthand() {
    let c = ctx();
    let mut n = 0;
    assert_eq!(
        rewrite_css(r#"p{font:italic bold 1.2em/1.5 "宋体", serif}"#, &c, &mut n),
        "p{font-style:italic;font-variant:normal;font-weight:bold;font-size:1.2em;line-height:1.5;}"
    );
    assert_eq!(rewrite_css(r#"p{font: 12pt 宋体 !important}"#, &c, &mut n), "p{font-style:normal !important;font-variant:normal !important;font-weight:normal !important;font-size:12pt !important;line-height:normal !important;}");
    for keep in [
        r#"p{font:1em "楷体"}"#,
        "p{font:caption}",
        "p{font:calc(1em + 1px) 宋体}",
        "p{font:inherit}",
        "p{font:bold 宋体}",
    ] {
        assert_eq!(rewrite_css(keep, &c, &mut n), keep);
    }
}

/// 字号归一：body 上的相对、绝对字号和别处的绝对字号乘系数，别处的相对字号、关键字不动；`font` 简写里的字号同样。
#[test]
fn font_sizes_scaled_to_body_text() {
    let c = Ctx {
        font_scale: Some(0.8),
        ..ctx()
    };
    let mut n = 0;
    assert_eq!(rewrite_css("body{font-size:125%} p{font-size:15pt} h1{font-size:1.5em} .s{font-size:smaller} .r{font-size:2rem}", &c, &mut n), "body{font-size:100%;} p{font-size:16px;} h1{font-size:1.5em} .s{font-size:smaller} .r{font-size:25.6px;}");
    assert_eq!(
        rewrite_css(".calibre{font-size:1.25em !important}", &c, &mut n),
        ".calibre{font-size:1em !important;}",
        "只用在 body 上的类"
    );
    assert_eq!(
        rewrite_css(r#"h2{font:bold 20px/1.2 "楷体"}"#, &c, &mut n),
        r#"h2{font:bold 16px/1.2 "楷体";}"#
    );
    assert_eq!(rewrite_css(r#"p{font:italic 15pt "宋体", serif}"#, &c, &mut n), "p{font-style:italic;font-variant:normal;font-weight:normal;font-size:16px;line-height:normal;}");
    assert_eq!(
        rewrite_css(r#"p{font:1.2em "楷体"}"#, &c, &mut n),
        r#"p{font:1.2em "楷体"}"#
    );
    // 正文已经是 1em：相对字号不动，绝对字号照样折成 px（掌阅的 pt 按 px 的数算）
    let one = Ctx {
        font_scale: Some(1.0),
        ..ctx()
    };
    assert_eq!(
        rewrite_css(
            "body{font-size:90%} p{font-size:12pt} .a{font-size:16px} .b{font-size:0.75rem}",
            &one,
            &mut n
        ),
        "body{font-size:90%} p{font-size:16px;} .a{font-size:16px} .b{font-size:12px;}"
    );
    // 外边距、内边距、首行缩进、行高里的绝对长度跟着乘；边框、宽高、相对单位、calc 不动；body 的左右照删
    assert_eq!(
        rewrite_css("p{margin:9px 0 20px;text-indent:15pt;line-height:20px;padding-top:1em;border-width:2px;width:300px;margin-left:calc(2px + 1em)} body{margin:10px 5px}", &c, &mut n),
        "p{margin:7.2px 0 16px;text-indent:12pt;line-height:16px;padding-top:1em;border-width:2px;width:300px;margin-left:calc(2px + 1em)} body{margin-top:10px;margin-bottom:10px;}"
    );
    assert_eq!(fmt_num(100.0 / 1.15), "86.9565");
    assert_eq!(fmt_num(12.0), "12");
}

/// 整段只有一个行内元素时它的字体算成段落的（同写出器）：《柏林谍影》每段 `<span style="font-family:'PingFang SC'">`。
/// 段落里只有一部分字的 span、`<sup>`、隔着 `<br/>` 的不算。
#[test]
fn whole_paragraph_span_font_counts_as_paragraph_font() {
    let e = |name: &str, data: &str| Entry {
        name: name.into(),
        data: data.as_bytes().to_vec(),
    };
    let page = |body: &str| {
        format!(
            r#"<html xmlns="http://www.w3.org/1999/xhtml"><head></head><body>{body}</body></html>"#
        )
    };
    let long = "正文".repeat(50);
    let facts = |body: String| book_facts(&[e("OEBPS/c1.xhtml", &page(&body))], None).body_font;
    let span = |t: &str| format!(r#"<span style="font-family:'PingFang SC';">{t}</span>"#);
    assert_eq!(
        facts(format!(
            "<p>{}</p><p> <b>{}</b> </p>",
            span(&long),
            span(&long)
        ))
        .as_deref(),
        Some("pingfang sc"),
        "整段一个 span、套在 <b> 里也算"
    );
    assert_eq!(
        facts(format!("<p>{long}{}</p>", span("几个字"))),
        None,
        "只盖住段落一部分：字算给段落"
    );
    assert_eq!(
        facts(format!("<p>{}<br/>{long}</p>", span(&long))),
        None,
        "隔着 <br/> 不是整段"
    );
    assert_eq!(
        facts(format!(
            r#"<p><sup style="font-family:'PingFang SC';">{long}</sup></p>"#
        )),
        None,
        "上标不并进段落"
    );
}

/// 全书正文字号按块、按字数算（行内元素改字号的字算给块），`html` 上写了字号的不归一。
#[test]
fn base_font_size_and_scale() {
    let e = |name: &str, data: &str| Entry {
        name: name.into(),
        data: data.as_bytes().to_vec(),
    };
    let page = |body: &str| {
        format!(
            r#"<html xmlns="http://www.w3.org/1999/xhtml"><head><link rel="stylesheet" href="s.css"/></head><body>{body}</body></html>"#
        )
    };
    let long = "正文".repeat(50);
    let book = |css: &str| {
        vec![
            e("OEBPS/s.css", css),
            e(
                "OEBPS/c1.xhtml",
                &page(&format!(
                    r#"<h1>标题</h1><p>{long}<span class="big">大字大字</span></p><p class="note">注释</p>"#
                )),
            ),
        ]
    };
    let base = |b: &[Entry], m: Option<&MediaEnv>| book_facts(b, m).base_font_size;
    let scale = |b: &[Entry]| font_scale(b, base(b, None));
    assert_eq!(
        base(
            &book("p{font-size:15pt} .big{font-size:3em} .note{font-size:0.5em}"),
            None
        ),
        1.25
    );
    assert_eq!(scale(&book("p{font-size:15pt}")), Some(0.8));
    assert_eq!(scale(&book("p{font-size:1em}")), Some(1.0));
    assert_eq!(
        scale(&book("html{font-size:80%} p{font-size:15pt}")),
        None,
        "html 上写了字号"
    );
    // `@media` 按阅读模式的阅读范围求（同 KFX 写出器）：掌阅 Air 3 Pro 1440 宽，`min-width:1200px` 成立、`min-width:2000px` 不成立；
    // 没给环境时特性条件一律不成立
    let ireader = MediaEnv {
        width: 1440.0,
        height: 1920.0,
        device_width: 1440.0,
        device_height: 1920.0,
        color: false,
    };
    let css = "p{font-size:1em} @media (min-width:1200px){p{font-size:15pt}} @media (min-width:2000px){p{font-size:2em}}";
    assert_eq!(base(&book(css), Some(&ireader)), 1.25);
    assert_eq!(base(&book(css), None), 1.0);
}

/// 正文字体按全书层叠算（写在 `<body class>` 上、段落继承的也认；行内元素的字体算给所在的块），负外边距按块的计算值、
/// 按文件字数取多数，body 上的类别处也用到的不算。
#[test]
fn book_facts_font_negative_margin_and_body_classes() {
    let e = |name: &str, data: &str| Entry {
        name: name.into(),
        data: data.as_bytes().to_vec(),
    };
    let page = |body: &str| {
        format!(
            r#"<html xmlns="http://www.w3.org/1999/xhtml"><head><link rel="stylesheet" href="s.css"/></head>{body}</html>"#
        )
    };
    let css = r#".song{font-family:"FZLanTingSong", serif} .kai{font-family:楷体} .hang{margin:0 1em 0 -2em} .wrap{margin-right:-5%}"#;
    let long = "正文".repeat(60);
    let book = vec![
        e("OEBPS/s.css", css),
        // 字体写在 body 的类上、段落继承；行内的楷体算给段落；用了负的左外边距
        e(
            "OEBPS/c1.xhtml",
            &page(&format!(
                r#"<body class="song main"><p>{long}<span class="kai">楷体楷体楷体楷体楷体楷体楷体楷体楷体楷体楷体楷体</span></p><p class="hang">悬挂</p></body>"#
            )),
        ),
        // 字少的文件用了负的右外边距：字数不占多数，不留；`main` 在这里不在 body 上
        e(
            "OEBPS/c2.xhtml",
            &page(
                r#"<body class="song"><div class="wrap main"><p class="kai">短短的楷体</p></div></body>"#,
            ),
        ),
    ];
    let f = book_facts(&book, None);
    assert_eq!(f.body_font.as_deref(), Some("fzlantingsong"));
    assert_eq!(f.neg, [true, false]);
    assert_eq!(f.body_classes, HashSet::from(["song".to_string()]));
}

/// 改完自查：两份样式表都给正文写了字体，后链的胜出；删掉胜出的那条，先链的那条不能露出来（《嘯風山莊》）。
/// 露出来的字体本来就生效的少数字一起换成阅读器字体；别的字体（标题的黑体）不动。
#[test]
fn exposed_body_font_removed_too() {
    let e = |name: &str, data: &str| Entry {
        name: name.into(),
        data: data.as_bytes().to_vec(),
    };
    let page = |body: &str| {
        format!(
            r#"<html xmlns="http://www.w3.org/1999/xhtml"><head><link rel="stylesheet" href="a.css"/><link rel="stylesheet" href="b.css"/></head><body>{body}</body></html>"#
        )
    };
    let long = "正文".repeat(200);
    let mut book = vec![
        e(
            "OEBPS/a.css",
            r#"p.p{font-family:"Light", serif} p.t{font-family:"Light"} h1{font-family:"Hei"}"#,
        ),
        e("OEBPS/b.css", r#"p.p{font-family:"DemiBold", serif}"#),
        e(
            "OEBPS/c1.xhtml",
            &page(&format!(
                r#"<h1>标题标题</h1><p class="p">{long}</p><p class="t">年表年表</p>"#
            )),
        ),
    ];
    let mut rep = WashReport::default();
    apply(&mut book, &mut rep, &WashOpts::default());
    let css = |i: usize| String::from_utf8(book[i].data.clone()).unwrap();
    assert_eq!(css(1), "p.p{}", "胜出的正文字体去掉");
    assert_eq!(
        css(0),
        r#"p.p{} p.t{} h1{font-family:"Hei"}"#,
        "露出来的 Light 一并去掉，黑体不动"
    );
    let after = book_facts(&book, None);
    assert_eq!(after.body_font, None, "正文按层叠算没有指定字体");
}

/// 不认 `rgba()` 的阅读器（掌阅）：规则、行内样式里的 `rgba()` 按白底合成不透明色（《飘》「第一卷」标题的半透明白条）；
/// 认的（Kindle 那边不过 `kindle_rules`，Move 认）不动。
#[test]
fn rgba_made_opaque_only_when_reader_lacks_it() {
    let mut n = 0;
    let c = Ctx {
        opaque_rgba: true,
        ..ctx()
    };
    assert_eq!(
        rewrite_css(
            "h1{color:#2e5b60;background-color:rgba(255,255,255,0.8)}",
            &c,
            &mut n
        ),
        "h1{color:#2e5b60;background-color:#ffffff}"
    );
    assert_eq!(
        rewrite_css(".x{border-color:rgba(0,0,0,0.5)}", &c, &mut n),
        ".x{border-color:#808080}"
    );
    let keep = "h1{background-color:rgba(255,255,255,0.8)}";
    assert_eq!(rewrite_css(keep, &ctx(), &mut n), keep);
}

/// 不认 `no-repeat` 的阅读器（Move）：写了不平铺的背景图去掉（简写里的背景色照留）；本来就平铺的、认 `no-repeat` 的不动。
#[test]
fn single_backgrounds_dropped_when_reader_tiles() {
    let mut n = 0;
    let c = Ctx {
        drop_single_backgrounds: true,
        ..ctx()
    };
    assert_eq!(
        rewrite_css(
            r#"body.fen{background:url('a.jpg') no-repeat bottom center;background-size:100% auto;background-color:#fff}"#,
            &c,
            &mut n
        ),
        "body.fen{background-size:100% auto;background-color:#fff}"
    );
    assert_eq!(
        rewrite_css(r#".a{background:#eee url(a.png) no-repeat}"#, &c, &mut n),
        ".a{background-color:#eee;}"
    );
    assert_eq!(
        rewrite_css(
            r#".b{background-image:url(b.png);background-repeat:repeat-x}"#,
            &c,
            &mut n
        ),
        ".b{background-repeat:repeat-x}"
    );
    for keep in [
        r#".t{background-image:url(paper.jpg)}"#,
        r#".t{background:url(paper.jpg) repeat}"#,
    ] {
        assert_eq!(rewrite_css(keep, &c, &mut n), keep, "本来就平铺的照留");
    }
    let keep = r#".a{background:url(a.png) no-repeat}"#;
    assert_eq!(
        rewrite_css(keep, &ctx(), &mut n),
        keep,
        "认 no-repeat 的阅读器不动"
    );
    // 做成整页图块的背景图（`bgfit::plan_tiles`）：规则照留，按样式表所在位置解析 url
    let t = Ctx {
        tiled: ["OEBPS/Images/a.jpg".to_string()].into_iter().collect(),
        ..c
    };
    let fen = r#"body.fen{background:url('../Images/a.jpg') no-repeat bottom center;background-size:100% auto}"#;
    assert_eq!(rewrite_css_in(fen, "OEBPS/Styles/s.css", &t, &mut n), fen);
    assert_eq!(
        rewrite_css_in(fen, "s.css", &t, &mut n),
        "body.fen{background-size:100% auto}",
        "别的图照旧去掉"
    );
    // 掌阅：照 Kindle 算好的像素尺寸、位置接在规则末尾（规则本身不动）
    let f = Ctx {
        bg_fixes: [(
            "OEBPS/Images/a.jpg".to_string(),
            "background-size:1264px 932px;background-position:0px 263px;".to_string(),
        )]
        .into_iter()
        .collect(),
        ..ctx()
    };
    assert_eq!(rewrite_css_in(fen, "OEBPS/Styles/s.css", &f, &mut n), "body.fen{background:url('../Images/a.jpg') no-repeat bottom center;background-size:100% auto;background-size:1264px 932px;background-position:0px 263px;}");
}

/// 白页面上的白字（全书层叠里用它的字都不在背景上，`light_on_page`）照 Kindle 调到 4.5:1；不在集合里的（背景图、背景色上的）不动。
#[test]
fn white_text_on_page_gets_contrast() {
    let mut n = 0;
    let on_page = Ctx {
        light_on_page: [0xFFFF_FFFF].into_iter().collect(),
        ..ctx()
    };
    assert_eq!(
        rewrite_css(".t{color:#fff}", &on_page, &mut n),
        ".t{color:#767676;}"
    );
    assert_eq!(
        rewrite_css(".t{color:#fff}", &ctx(), &mut n),
        ".t{color:#fff}"
    );
    assert_eq!(
        rewrite_css(
            ".t{color:#fff;background-image:url(a.jpg)}",
            &on_page,
            &mut n
        ),
        ".t{color:#fff;background-image:url(a.jpg)}",
        "规则自己有背景图：不调"
    );
    // 规则自己写了背景色照按它算（不看集合）
    assert_eq!(
        rewrite_css(".t{color:#fff;background-color:#333}", &on_page, &mut n),
        ".t{color:#fff;background-color:#333}"
    );
}

/// `book_facts` 按元素分：白页面上的白字进 `light_on_page`，同色字有一处压在背景图、背景色上就不进。
#[test]
fn light_on_page_from_cascade() {
    let page = |body: &str, css: &str| {
        let entries = vec![
            Entry {
                name: "OEBPS/s.css".into(),
                data: css.as_bytes().to_vec(),
            },
            Entry {
                name: "OEBPS/c1.xhtml".into(),
                data: format!(r#"<html xmlns="http://www.w3.org/1999/xhtml"><head><link href="s.css" rel="stylesheet" type="text/css"/></head><body>{body}</body></html>"#).into_bytes(),
            },
        ];
        book_facts(&entries, None).light_on_page
    };
    assert!(
        page(r#"<p><span class="w">章名</span></p>"#, ".w{color:#fff}").contains(&0xFFFF_FFFF)
    );
    let img = r#"<div class="bg"><span class="w">扉页</span></div>"#;
    assert!(page(img, ".w{color:#fff} .bg{background-image:url(a.jpg)}").is_empty());
    assert!(page(img, ".w{color:#fff} .bg{background-color:#333}").is_empty());
    assert!(
        page(
            &format!(r#"<p><span class="w">章名</span></p>{img}"#),
            ".w{color:#fff} .bg{background-color:#333}"
        )
        .is_empty(),
        "同色字有一处在背景上：不调"
    );
    // 跨文件：扉页（背景图上白字）和正文（白页面上白字）分在两章，照样不调
    let two = vec![
        Entry {
            name: "OEBPS/s.css".into(),
            data: b".w{color:#fff} .bg{background-image:url(a.jpg)}".to_vec(),
        },
        Entry {
            name: "OEBPS/c1.xhtml".into(),
            data: r#"<html xmlns="http://www.w3.org/1999/xhtml"><head><link href="s.css" rel="stylesheet" type="text/css"/></head><body><p class="w">章名</p></body></html>"#.as_bytes().to_vec(),
        },
        Entry {
            name: "OEBPS/c0.xhtml".into(),
            data: format!(r#"<html xmlns="http://www.w3.org/1999/xhtml"><head><link href="s.css" rel="stylesheet" type="text/css"/></head><body>{img}</body></html>"#).into_bytes(),
        },
    ];
    assert!(book_facts(&two, None).light_on_page.is_empty());
}

/// 掌阅（h = 1300 ÷ 58 ÷ 32 ≈ 0.70）：`eink-ua.css` 的列表左缩进 1.5em 跟着乘（≈ 1.05em，占页宽同 Kindle）；
/// 书自己写的 `padding-left`、`margin-left` 在自己的样式表里照常乘，`eink-ua.css` 排在最前，层叠里书写的胜出（见
/// `optimize::tests::text_repair_only_keeps_content` 的 link 顺序）。
#[test]
fn ua_list_indent_h_scaled() {
    let h = 1300.0 / 58.0 / 32.0;
    let ua = crate::wash::kindle_rules_scale_ua_for_test(None, Some(h));
    assert!(
        ua.contains("ul{padding-left:1.0") && ua.contains("ol{padding-left:1.0"),
        "{ua}"
    );
    let mut e = [Entry {
        name: "OEBPS/s.css".into(),
        data: b"ul{padding-left:0;margin-left:2em}".to_vec(),
    }];
    scale_horizontal_entries(&mut e, 0.5);
    assert_eq!(e[0].data, b"ul{padding-left:0;margin-left:1em}");
}

#[test]
fn written_colors_are_opaque_hex() {
    assert_eq!(css_color(0xFF12_AB34), "#12ab34");
    // 半透明的浅色字：按合成到背景上的颜色调，写出来是不透明的 #hex（不写 rgba：掌阅不认）
    let c = ctx();
    let mut n = 0;
    let out = rewrite_css(".x{color:rgba(0,0,0,0.4)}", &c, &mut n);
    assert!(
        out.starts_with(".x{color:#") && !out.contains("rgba"),
        "{out}"
    );
}

/// 竖直间距照 Kindle 换算（掌阅的系数，「测试-竖直间距」量的）：em 乘系数，%、px、pt 折成像素，简写只换上下，别的不动。
/// 左右边距的 em 乘系数（分项、简写的左右两个值），px、首行缩进、上下的不动。
#[test]
fn horizontal_em_scaled() {
    let (t, n) = scale_horizontal("margin-left:2em;padding-right:1em !important;margin:1em 2em 3em 4pt;padding-left:10px;text-indent:2em", 0.5);
    assert_eq!(n, 3);
    assert_eq!(t, "margin-left:1em;padding-right:0.5em !important;margin:1em 1em 3em 2pt;padding-left:10px;text-indent:2em");
    assert_eq!(
        scale_horizontal("text-indent:-2em;padding-left:2em", 0.5).0,
        "text-indent:-1em;padding-left:1em"
    );
}

/// body 左右边距逐文件：多数文件（按字数）用了负的左外边距、body 左边距照留；没用的文件 body 加类、左边距写 0；右边两个文件都没用，不动。
#[test]
fn body_margin_per_file() {
    let page = |body: &str| {
        format!(r#"<html xmlns="http://www.w3.org/1999/xhtml"><head><title>t</title><link rel="stylesheet" href="s.css"/></head><body class="b">{body}</body></html>"#).into_bytes()
    };
    let mut entries = vec![
        Entry { name: "O/content.opf".into(), data: br#"<package><manifest><item id="a" href="a.xhtml" media-type="application/xhtml+xml"/><item id="c" href="c.xhtml" media-type="application/xhtml+xml"/></manifest><spine><itemref idref="a"/><itemref idref="c"/></spine></package>"#.to_vec() },
        Entry { name: "O/s.css".into(), data: b"body.b{margin:0 5pt} .t{margin-left:-2em}".to_vec() },
        Entry { name: "O/a.xhtml".into(), data: page(&format!(r#"<p class="t">题</p><p>{}</p>"#, "字".repeat(50))) },
        Entry { name: "O/c.xhtml".into(), data: page("<p>短</p>") },
    ];
    let facts = book_facts(&entries, None);
    assert_eq!(facts.neg, [true, false]);
    assert_eq!(per_file_body_margins(&mut entries, &facts), 1);
    let c = String::from_utf8(entries[3].data.clone()).unwrap();
    assert!(
        c.contains(r#"<body class="b eink-bm1">"#) && c.contains("eink-body.css"),
        "{c}"
    );
    assert!(!String::from_utf8(entries[2].data.clone())
        .unwrap()
        .contains("eink-bm"));
    let css = entries
        .iter()
        .find(|e| e.name == "O/eink-body.css")
        .map(|e| String::from_utf8(e.data.clone()).unwrap())
        .unwrap();
    assert!(
        css.contains("html body.eink-bm1{margin-left:0;padding-left:0;}"),
        "{css}"
    );
}

#[test]
fn vertical_lengths_scaled_like_kindle() {
    let vs = VScale {
        em: 52.5 * (1680.0 / 1546.0) / 37.5,
        kindle_rem_px: 52.5,
        ratio: 1680.0 / 1546.0,
    };
    let (t, n) = scale_vertical("margin-top:1em;margin-bottom:10%;padding-top:30px;padding-bottom:24pt !important;margin-left:2em;", &vs);
    assert_eq!(t, "margin-top:1.5213em;margin-bottom:182.6px;padding-top:64.2px;padding-bottom:114.1px !important;margin-left:2em;");
    assert_eq!(n, 4);
    assert_eq!(
        scale_vertical("margin:1em 2em", &vs).0,
        "margin:1.5213em 2em 1.5213em 2em"
    );
    for keep in [
        "margin:0",
        "margin-top:auto",
        "margin-top:calc(1em + 2px)",
        "padding:0 1em",
        "line-height:2em",
    ] {
        assert_eq!(scale_vertical(keep, &vs), (keep.to_string(), 0), "{keep}");
    }
    assert!(
        scale_vertical_css("p{margin-top:1em;margin-bottom:1em;}\n", &vs)
            .starts_with("p{margin-top:1.5213em;margin-bottom:1.5213em;}")
    );
}
