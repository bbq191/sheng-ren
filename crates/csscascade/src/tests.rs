//! 层叠引擎的测试（原 `lib.rs` 里的 `mod tests`）。

use super::*;
use crate::rules::{Need, RuleIndex, MAX_MEDIA_NESTING};
use crate::shorthand::four_values;
use scraper::{ElementRef, Selector};
use std::collections::HashMap;
use std::sync::Arc;

/// `@media` 套得再深也不栈溢出（以前两万层整个进程中止）；超过上限的那几层不收，浅的照收。
#[test]
fn deeply_nested_media_does_not_overflow() {
    let nest = |n: usize| format!("{}p{{color:red}}{}", "@media all{".repeat(n), "}".repeat(n));
    assert_eq!(Rules::parse(&nest(3), "", None).len(), 1);
    assert_eq!(Rules::parse(&nest(MAX_MEDIA_NESTING), "", None).len(), 1);
    assert_eq!(
        Rules::parse(&nest(MAX_MEDIA_NESTING + 1), "", None).len(),
        0
    );
    let deep = format!("{}q{{color:red}}", nest(20000));
    let r = Rules::parse(&deep, "", None);
    assert_eq!(r.len(), 1, "后面的规则照收");
}

#[test]
fn decls_and_shorthand() {
    let d = parse_decls(
        "margin: 1em 0 2em; color: red !important; background: url(x.png) #fff no-repeat",
    );
    let get = |p: &str| d.iter().find(|x| x.prop == p).map(|x| x.value.as_str());
    assert_eq!(get("margin-top"), Some("1em"));
    assert_eq!(get("margin-right"), Some("0"));
    assert_eq!(get("margin-bottom"), Some("2em"));
    assert_eq!(get("margin-left"), Some("0"));
    assert_eq!(get("background-color"), Some("#fff"));
    assert!(d.iter().any(|x| x.prop == "color" && x.important));
    // 和优化器同一个切法：引号里的 `;`、转义的引号、`url(data:…;base64,…)` 不切；坏引号不吞掉后面的声明；`*color` 不收
    let d = parse_decls(
        r#"font-family: "a;\"b"; background-image: url(data:image/png;base64,AA==); *color: blue; color: red"#,
    );
    let got: Vec<(&str, &str)> = d
        .iter()
        .map(|x| (x.prop.as_str(), x.value.as_str()))
        .collect();
    assert_eq!(
        got,
        [
            ("font-family", r#""a;\"b""#),
            ("background-image", "url(data:image/png;base64,AA==)"),
            ("color", "red")
        ]
    );
    let d = parse_decls("font-family: Georgia'; color: red");
    assert!(
        d.iter().any(|x| x.prop == "color" && x.value == "red"),
        "{d:?}"
    );
}

#[test]
fn prefilter_is_only_a_necessary_condition() {
    assert_eq!(
        Need::of("div > p.a.B#x"),
        Some(Need {
            tag: Some("p".into()),
            classes: vec!["a".into(), "B".into()],
            ids: vec!["x".into()]
        })
    );
    assert_eq!(
        Need::of(".a"),
        Some(Need {
            tag: None,
            classes: vec!["a".into()],
            ids: vec![]
        })
    );
    for s in [
        "p:first-child",
        "a[href]",
        "*",
        "p *",
        "svg|a",
        r"p.a\:b",
        "p > ",
        "p.",
        "p..a",
    ] {
        assert_eq!(Need::of(s), None, "{s}");
    }
    // 用预筛和不用预筛，每个元素层叠出来的结果一样
    // 大小写混写的类、id、标签名（分桶的键是 ASCII 小写），同一个元素上两个只差大小写的类（去重后不能重复试同一条规则），
    // 非 ASCII 的类名
    let css = "p{color:red} p.a{color:blue} .A{text-indent:1em} div p.b{margin-top:1em} #X{font-size:2em} h1+p{color:green} span.c.d{font-weight:bold} li > span{color:gray} p:first-child{margin-left:1em} P.Q{text-align:center} SPAN{font-style:italic} .正文{line-height:2} .q.Q{margin-bottom:1em} *{orphans:2} [title]{widows:2}";
    let html = scraper::Html::parse_document(
        r#"<html><body><div><p class="a b" id="x">1</p><h1>t</h1><p class="A q Q">2<span class="d c 正文" title="t">3</span></p><ul><li><span>4</span></li></ul></div></body></html>"#,
    );
    let mut with = Sheet::default();
    with.add(css, 0);
    let mut without = Sheet::default();
    without.add(css, 0);
    // 不预筛也不分桶：规则全进 `other`，每个元素逐条完整匹配（分桶、预筛都只是缩小范围，结果必须一样）
    for part in &mut without.parts {
        let rules = Arc::get_mut(&mut part.0).unwrap();
        for r in &mut rules.list {
            r.need = None;
        }
        rules.index = RuleIndex::build(&rules.list);
        assert!(
            rules.index.by_id.is_empty()
                && rules.index.by_class.is_empty()
                && rules.index.by_tag.is_empty()
        );
    }
    let mut n = 0;
    for el in html
        .root_element()
        .descendants()
        .filter_map(ElementRef::wrap)
    {
        assert_eq!(
            with.cascade(&el, ""),
            without.cascade(&el, ""),
            "{}",
            el.html()
        );
        n += 1;
    }
    assert!(n > 8);
}

#[test]
fn specificity_counts() {
    assert_eq!(specificity("p"), (0, 0, 1));
    assert_eq!(specificity("p.a"), (0, 1, 1));
    assert_eq!(specificity("#x .a span"), (1, 1, 1));
    assert_eq!(specificity("div > p:first-child"), (0, 1, 2));
    assert_eq!(specificity("*"), (0, 0, 0));
    // 括号里的字不计成标签；:not/:is 取参数里最高的，:where 算 0
    assert_eq!(specificity("li:nth-child(2n+1)"), (0, 1, 1));
    assert_eq!(specificity("p:not(.a)"), (0, 1, 1));
    assert_eq!(specificity("p:not(#x, span)"), (1, 0, 1));
    assert_eq!(specificity("p:where(.a #b)"), (0, 0, 1));
    assert_eq!(specificity("p:is(div p, .a)"), (0, 1, 1));
    assert_eq!(specificity("p::first-line"), (0, 0, 2));
    assert_eq!(specificity("p:before"), (0, 0, 2));
    assert_eq!(specificity("a[href='x y']:lang(zh-Hant)"), (0, 2, 1));
    assert_eq!(specificity(r"p.a\.b"), (0, 1, 1));
}

#[test]
fn cascade_order_and_media() {
    let mut s = Sheet::default();
    s.add("@charset \"utf-8\"; p { color: red } .a { color: blue } p { color: green } @media amzn-mobi { p.a { color: black } } @font-face { font-family: x }", 0);
    let html = scraper::Html::parse_document(
        "<html><body><p class=\"a\" style=\"text-indent:2em\">x</p></body></html>",
    );
    let p = html.select(&Selector::parse("p").unwrap()).next().unwrap();
    let d = s.cascade(&p, "");
    assert_eq!(d.get("color").map(String::as_str), Some("blue"));
    assert_eq!(d.get("text-indent").map(String::as_str), Some("2em"));
}

#[test]
fn font_shorthand_expands_and_resets() {
    assert_eq!(
        parse_font_shorthand("normal small-caps 700 condensed larger / 2em  'A B', serif"),
        Some(FontShorthand {
            style: None,
            variant: Some("small-caps"),
            weight: Some("700"),
            stretch: Some("condensed"),
            size: "larger",
            line_height: Some("2em"),
            family: "'A B', serif"
        })
    );
    assert_eq!(parse_font_shorthand("bold bold 1em x"), None);
    // 层叠里展开成分项：没写的重置成 normal（盖掉前面规则的粗体、行高），后面的分项照常覆盖简写
    let mut s = Sheet::default();
    s.add("p { font-weight: bold; line-height: 2 } p { font: italic 15pt/1.5 \"宋体\", serif } .a { font-family: 楷体 } .b { font: caption }", 0);
    let html = scraper::Html::parse_document("<html><body><p class=\"a b\">x</p></body></html>");
    let p = html.select(&Selector::parse("p").unwrap()).next().unwrap();
    let d = s.cascade(&p, "");
    let get = |k: &str| d.get(k).map(String::as_str);
    assert_eq!(
        (
            get("font-style"),
            get("font-weight"),
            get("font-size"),
            get("line-height")
        ),
        (Some("italic"), Some("normal"), Some("15pt"), Some("1.5"))
    );
    assert_eq!(
        get("font-family"),
        Some("楷体"),
        "后面的分项覆盖简写；认不出的简写（caption）整条不认"
    );
    assert_eq!(Computed::derive(&Computed::root(), &d, "p").font_size, 1.25);
}

/// xochitl 的层叠顺序（探针「测试-规则顺序」1–5）：`!important` 不算；优先级一样比各自样式表里的序号，再比链接先后；
/// 优先级高的照样胜；行内 `style` 最后。
#[test]
fn xochitl_order_by_index_in_own_sheet() {
    let mut s = Sheet::default();
    let n = s.add(".f0{color:red} .f1{color:red} div.x{text-indent:3em} div.y{margin-left:3em} div.z{margin-top:3em}", 0);
    s.add("div.x{text-indent:1em !important} div.y.y{margin-left:1em} p{color:red} q{color:red} div.z{margin-top:1em}", n);
    let html = scraper::Html::parse_document(
        r#"<html><body><div class="x y z" style="margin-right:2em">x</div></body></html>"#,
    );
    let el = html
        .select(&Selector::parse("div").unwrap())
        .next()
        .unwrap();
    let (css, move_) = (s.cascade(&el, ""), s.cascade_xochitl(&el, ""));
    let get = |d: &Decls, k: &str| d.get(k).cloned().unwrap_or_default();
    assert_eq!(
        (
            get(&css, "text-indent").as_str(),
            get(&move_, "text-indent").as_str()
        ),
        ("1em", "3em")
    );
    assert_eq!(get(&move_, "margin-left"), "1em");
    assert_eq!(
        get(&move_, "margin-top"),
        "1em",
        "序号一样（都是第 5 条）后链接的胜"
    );
    assert_eq!(get(&move_, "margin-right"), "2em");
}

/// `<img width="60%">` 当 CSS 宽度（样式表写了的优先）；像素宽度不收。
#[test]
fn img_width_attribute_percent_is_a_hint() {
    let html = scraper::Html::parse_document(
        r#"<html><body><img id="a" width="60%"/><img id="b" width="566"/><img id="c" class="w" width="60%"/></body></html>"#,
    );
    let mut s = Sheet::default();
    s.add("img.w{width:30%}", 0);
    let width = |id: &str| {
        let el = html
            .select(&Selector::parse(&format!("#{id}")).unwrap())
            .next()
            .unwrap();
        let mut d = s.cascade(&el, "");
        presentational_hints(&el, &mut d);
        d.get("width").cloned()
    };
    assert_eq!(width("a").as_deref(), Some("60%"));
    assert_eq!(width("b"), None, "像素宽度不收");
    assert_eq!(width("c").as_deref(), Some("30%"), "样式表优先");
}

#[test]
fn media_queries_by_env() {
    let env = MediaEnv {
        width: 1104.0,
        height: 1546.0,
        device_width: 1272.0,
        device_height: 1696.0,
        color: false,
    };
    let ok = |q: &str| media_ok(q, Some(&env));
    for q in [
        "",
        "all",
        "screen",
        "amzn-kf8",
        "only screen",
        "not amzn-mobi",
        "not print",
        "print, screen",
        "SCREEN",
    ] {
        assert!(ok(q), "{q}");
    }
    for q in [
        "print",
        "amzn-mobi",
        "speech",
        "not screen",
        "screen and",
        "screen (min-width: 1px)",
        "and (color)",
    ] {
        assert!(!ok(q), "{q}");
    }
    // 特性按阅读范围、屏幕求值
    for q in [
        "screen and (min-width: 600px)",
        "(max-width: 1104px)",
        "(min-height: 1500px) and (orientation: portrait)",
        "(min-device-width: 1272px)",
        "(max-aspect-ratio: 3/4)",
        "(monochrome)",
        "not (color)",
        "(min-width: 10in)",
        "(color: 0)",
        "not screen and (max-width: 480px)",
    ] {
        assert!(ok(q), "{q}");
    }
    for q in [
        "screen and (max-width: 480px)",
        "(min-width: 1105px)",
        "(orientation: landscape)",
        "(color)",
    ] {
        assert!(!ok(q), "{q}");
    }
    // 求值不了的不收，带 not 也不收
    for q in [
        "(min-width: 30em)",
        "not screen and (min-width: 30em)",
        "(min-resolution: 2dppx)",
        "(width >= 600px)",
        "(min-monochrome: 4)",
        "(min-width: 600px) or (color)",
    ] {
        assert!(!ok(q), "{q}");
    }
    // 没有环境：只看媒体类型
    assert!(media_ok("screen", None) && !media_ok("screen and (min-width: 1px)", None));
    // 样式表里的 @media
    let mut with = Sheet::default();
    with.add_rules(Rules::parse("p{color:red} @media screen and (max-width: 480px){p{color:blue}} @media (min-width: 600px){p{text-indent:2em}}", "", Some(&env)), 0);
    let html = scraper::Html::parse_document("<p>x</p>");
    let p = html.select(&Selector::parse("p").unwrap()).next().unwrap();
    let d = with.cascade(&p, "");
    assert_eq!(
        (
            d.get("color").map(String::as_str),
            d.get("text-indent").map(String::as_str)
        ),
        (Some("red"), Some("2em"))
    );
}

/// 落单的引号按 CSS 规范在换行处结束（坏字符串），后面的规则照收；以前 `{}` 配不上、后面全废。到文末没配上的块在文末收尾。
#[test]
fn lone_quote_ends_at_newline() {
    let css = "p { font-family: \"Georgia;\n color: gray }\nh1 { color: red }\n.a { text-indent: 2em }\n.b { content: 'x\\'}'; color: blue }\n.c { color: green";
    let mut s = Sheet::default();
    s.add(css, 0);
    let html = scraper::Html::parse_document(
        r#"<html><body><h1 class="a">t</h1><p class="b c">x</p></body></html>"#,
    );
    let get = |sel: &str, k: &str| {
        let el = html.select(&Selector::parse(sel).unwrap()).next().unwrap();
        s.cascade(&el, "").get(k).cloned()
    };
    assert_eq!(get("h1", "color").as_deref(), Some("red"));
    assert_eq!(get("h1", "text-indent").as_deref(), Some("2em"));
    // 引号里转义的引号、`}` 不算；没收尾的最后一块照收（后出现的 green 盖过 blue）
    assert_eq!(get("p", "color").as_deref(), Some("green"));
    // 拆分、配括号、优先级共用一个扫描器：引号里的逗号、括号不算
    assert_eq!(
        split_top(r#"a, "b,c", d(e,f), 'g\',h'"#, ','),
        ["a", r#" "b,c""#, " d(e,f)", r#" 'g\',h'"#]
    );
    assert_eq!(
        rule_spans("p{a{b}\"}\"c}x")
            .iter()
            .map(|r| (r.body, r.end))
            .collect::<Vec<_>>(),
        [("a{b}\"}\"c", 11)]
    );
    assert_eq!(specificity(r#"a[title=")"]:not(.x)"#), (0, 2, 1));
    assert!(media_ok(
        "screen and (min-width: 1px)",
        Some(&MediaEnv {
            width: 10.0,
            height: 10.0,
            device_width: 10.0,
            device_height: 10.0,
            color: false
        })
    ));
}

/// 规则位置：字符串、注释里的花括号不算；`@media` 里的给出并带上条件；开头的语句和注释留在 prelude 里（判断时 `rule_selector` 去掉）；
/// `@font-face` 这类没有块的 at 规则照给；空 prelude 的块不给；到文末没收尾的在文末收尾。
#[test]
fn rule_spans_positions_and_parents() {
    let css = "@charset \"utf-8\"; /* { */ p { content: '}'; color: red }\n@media screen { .a { x: 1 } @supports (y) { .b { z: 2 } } }\n@font-face { font-family: f }\n}{orphan} .c { q: 1";
    let r = rule_spans(css);
    let got: Vec<(String, &str, Vec<&str>, bool)> = r
        .iter()
        .map(|r| {
            (
                rule_selector(r.prelude).trim().to_string(),
                r.body.trim(),
                r.parents.iter().map(|p| p.trim()).collect(),
                r.closed,
            )
        })
        .collect();
    assert_eq!(
        got,
        [
            ("p".to_string(), "content: '}'; color: red", vec![], true),
            (".a".to_string(), "x: 1", vec!["@media screen"], true),
            (
                ".b".to_string(),
                "z: 2",
                vec!["@media screen", "@supports (y)"],
                true
            ),
            ("@font-face".to_string(), "font-family: f", vec![], true),
            (".c".to_string(), "q: 1", vec![], false),
        ]
    );
    assert_eq!(
        &css[r[0].body_start..r[0].end],
        " content: '}'; color: red }"
    );
    assert!(r[0].prelude.starts_with("@charset"));
    // 层叠只收条件成立的 `@media` 里的，`@supports` 里的不收；属性选择器里引号中的 `;` 不切选择器
    let mut s = Sheet::default();
    s.add("@media print { p { color: blue } } @supports (x) { p { text-indent: 3em } } [title=\"a;b\"] { text-indent: 1em }", 0);
    let html = scraper::Html::parse_document(r#"<p title="a;b">x</p>"#);
    let p = html.select(&Selector::parse("p").unwrap()).next().unwrap();
    let d = s.cascade(&p, "");
    assert_eq!(
        (d.get("color"), d.get("text-indent").map(String::as_str)),
        (None, Some("1em"))
    );
}

/// 去注释换成空格、字符串里的不算；切词认引号、注释当分隔；四值简写拆不清的不认。
#[test]
fn text_tools() {
    assert_eq!(
        strip_comments("a/* x */b 'c/*d*/' /* 没闭合"),
        "a b 'c/*d*/'  "
    );
    assert!(matches!(
        strip_comments("p{}"),
        std::borrow::Cow::Borrowed(_)
    ));
    assert_eq!(
        tokens("1px/**/solid  \"A B\"\trgb(0, 0, 0)").0,
        ["1px", "solid", "\"A B\"", "rgb(0, 0, 0)"]
    );
    assert!(!tokens("calc(1px").1);
    assert_eq!(
        background_tokens("url(a/b.png) center/cover"),
        ["url(a/b.png)", "center", "/", "cover"]
    );
    assert_eq!(
        box_sides("calc(1em + 2px) 0"),
        BoxSides::Sides(["calc(1em + 2px)", "0", "calc(1em + 2px)", "0"], "")
    );
    assert_eq!(four_values("1px 2px 3px 4px 5px"), None);
    assert_eq!(four_values("inherit"), Some(["inherit"; 4]));
    assert_eq!(
        split_leading_statements("@import url('a;b.css'); /* c */ @charset \"x\";\n.x"),
        ("@import url('a;b.css'); /* c */ @charset \"x\";", "\n.x")
    );
    assert_eq!(
        compound_tag_classes("body..x"),
        ("body".to_string(), vec!["x"])
    );
}

/// `background:none`、只写颜色的 `background` 简写按 CSS 把没写的项（背景图等）重置成初始值；位置、尺寸回到不写。
#[test]
fn background_shorthand_resets_unset_parts() {
    let mut s = Sheet::default();
    s.add("div { background-image: url(a.png); background-repeat: no-repeat; background-position: center } div.n { background: none } div.c { background: #eee } div.u { background: url(b.png) }", 0);
    let html = scraper::Html::parse_document(
        r#"<html><body><div>0</div><div class="n">1</div><div class="c">2</div><div class="u">3</div></body></html>"#,
    );
    let comps: Vec<Computed> = html
        .select(&Selector::parse("div").unwrap())
        .map(|el| Computed::derive(&Computed::root(), &s.cascade(&el, ""), "div"))
        .collect();
    assert_eq!(comps[0].bg_image.as_deref(), Some("a.png"));
    assert!(comps[0].bg_no_repeat && comps[0].bg_position[0].is_some());
    assert_eq!(
        (
            comps[1].bg_image.as_deref(),
            comps[1].bg_no_repeat,
            comps[1].bg_position
        ),
        (None, false, [None, None])
    );
    assert_eq!(
        (comps[2].bg_image.as_deref(), comps[2].background),
        (None, Some(0xFFEE_EEEE))
    );
    assert_eq!(
        (
            comps[3].bg_image.as_deref(),
            comps[3].bg_no_repeat,
            comps[3].bg_position
        ),
        (Some("b.png"), false, [None, None])
    );
}

#[test]
fn font_face_urls() {
    let f = font_faces(
        r#"@font-face{font-family:"宋体";src:local("st")}@font-face{font-family:"juan";src:url("../Fonts/juan.ttf")}@font-face{font-family:'b';font-weight:bold;src:local(x),url(../Fonts/b.ttf) format("truetype")}"#,
        "OEBPS/Styles/s.css",
    );
    assert_eq!(f.len(), 2);
    assert_eq!(
        f[0],
        FontFace {
            family: "juan".into(),
            path: "OEBPS/Fonts/juan.ttf".into(),
            bold: false,
            italic: false
        }
    );
    assert!(f[1].bold && f[1].path == "OEBPS/Fonts/b.ttf");
}

#[test]
fn colors_and_lengths() {
    assert_eq!(parse_color("#01a0ea"), Some(0xFF01A0EA));
    assert_eq!(parse_color("#fff"), Some(0xFFFFFFFF));
    // #rgba、#rrggbbaa：透明度写进最高字节（同 rgba()）
    assert_eq!(parse_color("#f008"), Some(0x88FF0000));
    assert_eq!(parse_color("#01a0ea80"), Some(0x8001A0EA));
    assert_eq!(parse_color("#12345"), None);
    assert_eq!(parse_color("#+12"), None);
    // 透明度舍去小数（同 Send to Kindle：0.7 → 0xB2、0.5 → 0x7F）；0.6 × 255 的浮点误差不能舍成 152
    assert_eq!(parse_color("rgba(128, 0, 0, 0.7)"), Some(0xB2800000));
    assert_eq!(parse_color("rgba(245, 245, 220, 0.5)"), Some(0x7FF5F5DC));
    assert_eq!(parse_color("rgba(0, 0, 0, 0.6)"), Some(0x99000000));
    assert_eq!(parse_color("rgb(100%, 0%, 50%)"), Some(0xFFFF0080));
    assert_eq!(parse_color("rgba(0, 0, 0, 50%)"), Some(0x7F000000));
    assert_eq!(parse_len("1.5em"), Some(Len::Em(1.5)));
    assert_eq!(parse_len("30%"), Some(Len::Percent(30.0)));
    assert_eq!(parse_len("4px"), Some(Len::Pt(3.0)));
}

#[test]
fn computed_inherits() {
    let mut d = HashMap::new();
    d.insert("font-size".to_string(), "2em".to_string());
    d.insert("margin-top".to_string(), "1em".to_string());
    let h = Computed::derive(&Computed::root(), &d.into_iter().collect(), "h1");
    assert_eq!(h.font_size, 2.0);
    assert!(h.bold);
    let child = Computed::derive(&h, &Decls::default(), "span");
    assert_eq!(child.font_size, 2.0);
    assert_eq!(child.margin[0], None);
    // 负字号作废，照父元素
    let neg = Computed::derive(
        &h,
        &Decls::from_iter([("font-size".to_string(), "-1em".to_string())]),
        "span",
    );
    assert_eq!(neg.font_size, 2.0);
}
