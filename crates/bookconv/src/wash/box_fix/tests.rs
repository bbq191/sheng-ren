//! 原 `box_fix.rs` 的单元测试（拆分后测试路径不变：`wash::box_fix::tests::…`）。

use super::*;

fn book(body: &str, css: &str) -> Vec<Entry> {
    vec![
        Entry { name: "O/content.opf".into(), data: br#"<package><manifest><item id="c" href="c.xhtml" media-type="application/xhtml+xml"/></manifest><spine><itemref idref="c"/></spine></package>"#.to_vec() },
        Entry { name: "O/s.css".into(), data: css.as_bytes().to_vec() },
        Entry { name: "O/c.xhtml".into(), data: format!(r#"<html xmlns="http://www.w3.org/1999/xhtml"><head><title>t</title><link rel="stylesheet" href="s.css"/></head><body>{body}</body></html>"#).into_bytes() },
    ]
}

fn run(body: &str, css: &str) -> (String, String, Vec<Entry>) {
    let mut b = book(body, css);
    let reader = ReaderModel::for_profile(profile::fixture("xochitl").unwrap());
    apply(
        &mut b,
        &mut WashReport::default(),
        None,
        &reader,
        BoxGeometry {
            area_w: 842.0,
            em_px: 37.8,
            v_rem_px: 49.4,
            h_scale: 1.0,
        },
    );
    let page = String::from_utf8(b[2].data.clone()).unwrap();
    let sheet = b
        .iter()
        .find(|e| e.name == "O/eink-box.css")
        .map(|e| String::from_utf8(e.data.clone()).unwrap())
        .unwrap_or_default();
    (page, sheet, b)
}

/// 悬挂缩进：左内边距并进左外边距；带背景色的、列表不动。
/// 认内边距、px 是屏幕像素的阅读器（掌阅）：px 的左右内边距、首行缩进（都是 0.45pt 一 px）折成 em，em 写的不动；
/// 行内写的删掉、没改的那几边并进类规则。
#[test]
fn px_padding_and_indent_on_padding_reader() {
    let reader = ReaderModel {
        px_margins: false,
        ..ReaderModel::default()
    };
    let mut b = book(
        r#"<p class="a">一</p><p class="e">二</p><p style="padding:8px 16px;text-indent:32px">三</p>"#,
        "p.a{padding-left:20px;text-indent:24px} p.e{padding-left:2em;text-indent:2em}",
    );
    apply(
        &mut b,
        &mut WashReport::default(),
        None,
        &reader,
        BoxGeometry {
            area_w: 1264.0,
            em_px: 37.5,
            v_rem_px: 49.4,
            h_scale: 1.0,
        },
    );
    let page = String::from_utf8(b[2].data.clone()).unwrap();
    let sheet = String::from_utf8(
        b.iter()
            .find(|e| e.name.ends_with(CSS_NAME))
            .unwrap()
            .data
            .clone(),
    )
    .unwrap();
    assert!(
        page.contains(
            r#"<p class="a eink-b1">一</p><p class="e">二</p><p class="eink-b2">三</p>"#
        ),
        "{page}"
    );
    assert!(
        sheet.contains("p.eink-b1{padding-left:0.75em;text-indent:0.9em;}"),
        "{sheet}"
    );
    assert!(sheet.contains("p.eink-b2{padding-left:0.6em;padding-right:0.6em;text-indent:1.2em;padding-top:0.3em;padding-bottom:0.3em;}"), "{sheet}");
}

/// Move（不认内边距、px 是屏幕像素）：首行缩进的 px 也照 Kindle 折成 em（《古龙》居中标题 `text-indent:48px`，探针 Q151）；内边距照旧并进外边距。
#[test]
fn px_indent_folded_on_move() {
    let reader = ReaderModel::for_profile(profile::fixture("xochitl").unwrap());
    let mut b = book(
        r#"<h2 class="c">第一章</h2><p class="e">正文</p>"#,
        "h2.c{font-size:1.66667em;text-align:center;text-indent:48px} p.e{text-indent:2em}",
    );
    apply(
        &mut b,
        &mut WashReport::default(),
        None,
        &reader,
        BoxGeometry {
            area_w: 842.0,
            em_px: 37.8,
            v_rem_px: 49.4,
            h_scale: 1.0,
        },
    );
    let page = String::from_utf8(b[2].data.clone()).unwrap();
    let sheet = String::from_utf8(
        b.iter()
            .find(|e| e.name.ends_with(CSS_NAME))
            .unwrap()
            .data
            .clone(),
    )
    .unwrap();
    assert!(
        page.contains(r#"<h2 class="c eink-b1">第一章</h2>"#),
        "{page}"
    );
    // 1.8em：48px 按 Kindle 的换算折成 em（标题后面那段的上外边距是 Move 不认标题外边距那条写的）
    assert!(sheet.contains("h2.eink-b1{text-indent:1.8em;}"), "{sheet}");
}

/// 左右有框的块包进单格表格，框写在单元格行内（全框四边、单边只有那一边），块的左外边距挪到表格（屏幕像素）、内边距写成单元格的，
/// 前后的间距写到相邻的块；定了宽度的（Move 不认宽度）折成外边距一起框；书里表格、单元格看得见的边框写进行内。
#[test]
fn boxes_framed_and_tables_lined() {
    let body = r#"<p>前</p><div class="box">框</div><p>后</p><p class="bar">引文</p><div class="w">定宽</div><table class="t"><tbody><tr><td>格</td></tr></tbody></table><table><tbody><tr><td>无</td></tr></tbody></table>"#;
    let css = "p{margin:1em 0} div.box{border:1px solid #333;padding:0.5em 0;margin:2em 0} p.bar{border-left:3pt dotted #f00;margin-left:1em} div.w{border:1px solid #000;width:50%} table.t td{border-bottom:1px solid #000}";
    let (page, sheet, _) = run(body, css);
    assert!(page.contains(r#"<p class="eink-b1">前</p><div class="eink-frame eink-b3"><table class="eink-frame" cellspacing="0" width="100%"><tbody><tr><td style="border-top:2px solid #333333;border-right:2px solid #333333;border-bottom:2px solid #333333;border-left:2px solid #333333;padding:25px 0px 25px 0px;vertical-align:top;font-size:1em;line-height:normal;text-align:left;"><div class="box">框</div></td></tr></tbody></table></div><p class="eink-b2">后</p>"#), "{page}");
    // 3pt 点线：0.25 rem × 49.4 ≈ 12px；左外边距 1em 挪到外层 div
    assert!(page.contains(r#"<div class="eink-frame eink-b4"><table class="eink-frame" cellspacing="0" width="100%"><tbody><tr><td style="border-left:12px dotted #ff0000;padding:0px 0px 0px 0px;vertical-align:top;font-size:1em;line-height:normal;text-align:left;"><p class="bar">引文</p></td></tr></tbody></table></div>"#), "{page}");
    assert!(
        sheet.contains(
            "div.eink-b3{margin-left:0em;margin-right:0em;margin-top:0;margin-bottom:0;}"
        ) && sheet.contains(
            "div.eink-b4{margin-left:1em;margin-right:0em;margin-top:0;margin-bottom:0;}"
        ),
        "{sheet}"
    );
    // 定宽 50% 的：宽度折成右外边距（421 ÷ 37.8），一起框
    assert!(page.contains(r#"<div class="eink-frame eink-b5"><table class="eink-frame" cellspacing="0" width="100%"><tbody><tr><td style="border-top:2px solid #000000;border-right:2px solid #000000;border-bottom:2px solid #000000;border-left:2px solid #000000;padding:0px 0px 0px 0px;vertical-align:top;font-size:1em;line-height:normal;text-align:left;"><div class="w">定宽</div>"#), "{page}");
    assert!(
        sheet.contains("div.eink-b5{margin-left:0em;margin-right:11.1376em;"),
        "{sheet}"
    );
    assert!(page.contains(r#"<table class="t" cellspacing="0"><tbody><tr><td style="border-bottom:2px solid #000000;">格</td></tr></tbody></table><table><tbody><tr><td>无</td></tr></tbody></table>"#), "{page}");
}

/// Move：单元格 px 的内边距折成 em（四边一起，行内的删掉）；列表项的上下内边距并进外边距；页中间负的上外边距按 Kindle 折叠后写成
/// 两边非负的。掌阅：行内元素 px 的左右外边距折成 em，Move 上不动（不认）。
#[test]
fn cells_lists_negative_and_inline() {
    let body = r#"<table><tbody><tr><td style="padding:8px 16px">格</td></tr></tbody></table><ul><li class="l">项</li></ul><p class="a">甲</p><p class="n">乙</p><p>丙<span class="s">注</span></p>"#;
    let css = "li.l{padding:10px 0;margin:0} p.a{margin:0 0 2em 0} p.n{margin-top:-0.5em} span.s{margin-left:10px}";
    let (page, sheet, _) = run(body, css);
    assert!(page.contains(r#"<td class="eink-b1">格</td>"#), "{page}");
    assert!(sheet.contains("td.eink-b1{padding-top:0.3em;padding-right:0.6em;padding-bottom:0.3em;padding-left:0.6em;}"), "{sheet}");
    // 10px × 0.45pt ÷ 12 = 0.375em
    assert!(
        page.contains(r#"<li class="l eink-b2">项</li>"#)
            && sheet.contains("li.eink-b2{margin-top:0.375em;margin-bottom:0.375em;}"),
        "{sheet}"
    );
    // 2em 和 −0.5em 折叠成 1.5em，两边都写
    assert!(page.contains(r#"<p class="a eink-b3">甲</p><p class="n eink-b4">乙</p><p>丙<span class="s">注</span></p>"#), "{page}");
    assert!(
        sheet.contains("p.eink-b3{margin-bottom:1.5em;}")
            && sheet.contains("p.eink-b4{margin-top:1.5em;}"),
        "{sheet}"
    );
    let mut b = book(body, css);
    let reader = ReaderModel::for_profile(profile::get("ireader").unwrap());
    apply(
        &mut b,
        &mut WashReport::default(),
        None,
        &reader,
        BoxGeometry {
            area_w: 1264.0,
            em_px: 37.5,
            v_rem_px: 49.4,
            h_scale: 1.0,
        },
    );
    let page = String::from_utf8(b[2].data.clone()).unwrap();
    assert!(
        page.contains(r#"<span class="s eink-b"#)
            && page.contains(r#"<td class="eink-b1">格</td>"#),
        "{page}"
    );
}

/// Move：插了下边框横线图的块，行内照 Kindle 写的行距低于下限 1 的，行内末尾接 1（不然字压在横线上；不写进外链）。
#[test]
fn bottom_rule_block_gets_line_floor() {
    let (page, sheet, _) = run(
        r#"<p>前</p><h1 class="t" style="line-height:0.744;text-align:center">版权信息</h1><p>后</p>"#,
        "h1.t{border-bottom:1px dotted #000}",
    );
    assert!(page.contains(r#"<h1 class="t" style="line-height:0.744;text-align:center;line-height:1;">版权信息<img class="eink-sp eink-r1""#), "{page}");
    assert!(
        !sheet.contains("line-height"),
        "外链里不写不带单位的行高（Move 上会让后面的规则失效）：{sheet}"
    );
}

/// 文件开头（前面没有内容）的块上的分页：行内末尾接 auto；页中间的不动。
#[test]
fn page_break_at_file_start_dropped() {
    let (page, sheet, _) = run(
        r#"<div></div><h1 style="page-break-before:always;text-align:center">第一部</h1><p>正文</p><h2 class="pb">二</h2>"#,
        "h2.pb{page-break-before:always}",
    );
    assert!(page.contains(r#"<div></div><h1 style="page-break-before:always;text-align:center;page-break-before:auto;">第一部</h1>"#) && page.contains(r#"<h2 class="pb">二</h2>"#), "{page}");
    assert!(!sheet.contains("page-break"), "{sheet}");
}

/// 《梁羽生》卷名（`float:right`、定宽、只有左边 auto、页首负上外边距配上内边距）：Kindle 不浮动、只有左边 auto 的靠右、推出页面的
/// 那截不占地方。掌阅标题的 float 写成 none、页首的上外边距 0、上内边距减去推出去的，左边的 auto 留着；Move 多出来的宽放左边。
#[test]
fn floated_title_follows_kindle() {
    let body = r#"<h1 class="bt">卷名</h1><p>正文</p><h2 class="fl">节</h2><p class="bver">1</p><p>说明</p><div class="fd">块</div>"#;
    let css = "h1.bt{width:1.2em;float:right;padding:6em 0 0 0;margin:-6em 10px auto auto} h2.fl{float:left} p.bver{font-size:300%;float:right} div.fd{float:left}";
    let mut b = book(body, css);
    let reader = ReaderModel::for_profile(profile::get("ireader").unwrap());
    apply(
        &mut b,
        &mut WashReport::default(),
        None,
        &reader,
        BoxGeometry {
            area_w: 1264.0,
            em_px: 37.5,
            v_rem_px: 49.4,
            h_scale: 1.0,
        },
    );
    let page = String::from_utf8(b[2].data.clone()).unwrap();
    let sheet = String::from_utf8(
        b.iter()
            .find(|e| e.name.ends_with(CSS_NAME))
            .unwrap()
            .data
            .clone(),
    )
    .unwrap();
    assert!(
        page.contains(
            r#"<h1 class="bt eink-b1">卷名</h1><p>正文</p><h2 class="fl eink-b2">节</h2>"#
        ),
        "{page}"
    );
    assert!(
        sheet.contains("h1.eink-b1{float:none;margin-top:0;padding-top:0em;")
            && !sheet.contains("margin-left:0;")
            && sheet.contains("h2.eink-b2{float:none;}"),
        "{sheet}"
    );
    // 段落上的也写成 none（Q148，《梁羽生》版本号）；div 上的没量过，不动
    assert!(
        page.contains(r#"<p class="bver eink-b3">1</p>"#)
            && sheet.contains("p.eink-b3{float:none;}")
            && page.contains(r#"<div class="fd">块</div>"#),
        "{page}\n{sheet}"
    );
    // Move：窄的定宽块（1.2em 宽）不折成 em 外边距（跟着字号变），包进单格表格，表格宽写占页宽的百分比、靠右（只有左边 auto）；
    // 外层 div 只留块自己的右外边距，块里的外边距、浮动都去掉（单元格里不认）
    let (page, sheet, _) = run(body, css);
    assert!(
        !sheet.contains("float") && sheet.contains("margin-left:0em;margin-right:"),
        "{sheet}"
    );
    assert!(
        page.contains(r#"<table class="eink-frame" cellspacing="0" width=""#)
            && page.contains(r#"%" align="right">"#),
        "{page}"
    );
    let pct: f64 = page
        .split(r#"cellspacing="0" width=""#)
        .nth(1)
        .unwrap()
        .split('%')
        .next()
        .unwrap()
        .parse()
        .unwrap();
    // 1.2em × 2（Move 的 h1 字号）× 37.8px ÷ 842 ≈ 10.8%，不管阅读字号
    assert!(pct > 5.0 && pct < 25.0, "{pct}: {page}");
}

/// Move 单元格缺省顶端对齐：一行两格以上、Kindle 上居中的（没写、写了 baseline、只写在 tr 上的）行内补 middle；写了 top 的、单格的不动。
#[test]
fn cells_get_kindle_vertical_middle() {
    let (page, _, _) = run(
        r#"<table><tbody><tr class="t"><td>甲</td><td class="b">乙</td><td class="p">丙</td></tr><tr><td>丁</td></tr></tbody></table>"#,
        "tr.t{vertical-align:top} td.b{vertical-align:baseline} td.p{vertical-align:top}",
    );
    assert!(page.contains(r#"<td style="vertical-align:middle;">甲</td><td class="b" style="vertical-align:middle;">乙</td><td class="p">丙</td></tr><tr><td>丁</td>"#), "{page}");
}

/// 单元格的宽度照 Send to Kindle 写成列宽百分比（1em＝16px、`width="30"`＝30px，÷（16 + 30 + 「丙」的自然宽度 26））：掌阅写行内
/// `width:P%`、属性删掉，Move 写成属性；掌阅定宽块左边的 auto 写成 0。
#[test]
fn cell_width_dropped_and_auto_margin_zeroed() {
    let body = r#"<table><tbody><tr><td class="a">甲</td><td width="30">乙</td><td>丙</td></tr></tbody></table><p class="c">丁</p>"#;
    let css = "td{vertical-align:top} td.a{width:1em} p.c{width:50%;margin:0 auto}";
    let mut b = book(body, css);
    let reader = ReaderModel::for_profile(profile::get("ireader").unwrap());
    apply(
        &mut b,
        &mut WashReport::default(),
        None,
        &reader,
        BoxGeometry {
            area_w: 1264.0,
            em_px: 37.5,
            v_rem_px: 49.4,
            h_scale: 1.0,
        },
    );
    let page = String::from_utf8(b[2].data.clone()).unwrap();
    let sheet = String::from_utf8(
        b.iter()
            .find(|e| e.name.ends_with(CSS_NAME))
            .unwrap()
            .data
            .clone(),
    )
    .unwrap();
    assert!(page.contains(r#"<td class="a" style="width:22.22%;">甲</td><td style="width:41.67%;">乙</td><td>丙</td>"#), "{page}");
    assert!(
        page.contains(r#"<p class="c eink-b1">丁</p>"#)
            && sheet.contains("p.eink-b1{margin-left:0;}"),
        "{sheet}"
    );
    let (page, _, _) = run(body, css);
    assert!(
        page.contains(
            r#"<td class="a" width="22.22%">甲</td><td width="41.67%">乙</td><td>丙</td>"#
        ),
        "{page}"
    );
}

/// 包框、下边框横线图找的是配对的闭合标签：框里套着 div 的不插错位置。
#[test]
fn frame_around_nested_div() {
    let (page, _, _) = run(
        r#"<p>前</p><div class="f"><div>甲</div><div>乙</div></div><p>后</p>"#,
        "div.f{border:1px solid #000}",
    );
    assert!(page.contains(r#"vertical-align:top;font-size:1em;line-height:normal;text-align:left;"><div class="f"><div>甲</div><div>乙</div></div></td></tr></tbody></table></div><p class="eink-b2">后</p>"#), "{page}");
}

#[test]
fn horizontal_padding_joins_margin() {
    let (page, sheet, _) = run(r#"<p class="h">注一</p><p class="bg">色块</p><ul><li class="h">项</li></ul>"#, "p.h,li.h{text-indent:-2em;padding-left:2em;margin-left:1em} p.bg{padding-left:1em;background-color:#ccc}");
    assert!(page.contains(r#"<p class="h eink-b1">注一</p><p class="bg">色块</p><ul><li class="h">项</li></ul>"#), "{page}");
    assert!(
        sheet.contains("html body p.eink-b1{margin-left:3em;}"),
        "{sheet}"
    );
    // 没有项目符号的列表项（自己写的、从 ul 继承的、display:block 的）左内边距并进外边距（《海葵》目录页）
    let (page, sheet, _) = run(
        r#"<ul><li class="n">甲</li></ul><ul class="u"><li>乙</li></ul><ul><li class="d">丙</li><li class="m">丁</li></ul>"#,
        "li{padding-left:2em;margin-left:0} li.n{list-style-type:none} ul.u{list-style:none} li.d{display:block}",
    );
    assert!(
        page.contains(r#"<li class="n eink-b1">甲</li>"#)
            && page.contains(r#"<li class="eink-b1">乙</li>"#)
            && page.contains(r#"<li class="d eink-b1">丙</li><li class="m">丁</li>"#),
        "{page}"
    );
    assert!(
        sheet.contains("html body li.eink-b1{margin-left:2em;}"),
        "{sheet}"
    );
}

/// px、% 的左右外边距折成 em：px 按 0.45pt、12pt 一根 em（50px → 1.875em），% 按内容区宽（10% of 842 → 84.2px ÷ 37.8 → 2.2275em）；
/// 掌阅只折 px；`margin:auto` 那一侧、em 写的不动；带背景色的块也折（不并内边距）。
#[test]
fn px_and_percent_side_margins_become_em() {
    let body = r#"<p class="px">甲</p><p class="pc">乙</p><p class="auto">丙</p><p class="em">丁</p><p class="bg">戊</p>"#;
    let css = "p.px{margin-left:50px} p.pc{margin-right:10%} p.auto{margin:0 auto;width:50%} p.em{margin-left:2em} p.bg{margin-left:50px;padding-left:1em;background-color:#ccc}";
    let (page, sheet, _) = run(body, css);
    assert!(page.contains(r#"<p class="px eink-b1">甲</p><p class="pc eink-b2">乙</p><p class="auto eink-b3">丙</p><p class="em">丁</p><p class="bg eink-b1">戊</p>"#), "{page}");
    assert!(
        sheet.contains("html body p.eink-b1{margin-left:1.875em;}")
            && sheet.contains("html body p.eink-b2{margin-right:2.2275em;}"),
        "{sheet}"
    );
    // Move 不认块宽度：50% 折成右边 (842 − 421) ÷ 37.8（Kindle 不按 margin:auto 居中）
    assert!(
        sheet.contains("html body p.eink-b3{margin-left:0em;margin-right:11.1376em;}"),
        "{sheet}"
    );
    // 行内写了外边距的：行内的删掉（行内的压过样式表），没改的那几边照现在的值并进类规则
    let (page, sheet, _) = run(r#"<p style="margin:1em 0 0 50px;color:red">己</p>"#, "");
    assert!(
        page.contains(r#"<p style="color:red" class="eink-b1">己</p>"#),
        "{page}"
    );
    assert!(sheet.contains("html body p.eink-b1{margin-left:1.875em;margin-top:1em;margin-right:0em;margin-bottom:0em;}"), "{sheet}");
    let mut b = book(body, css);
    let reader = ReaderModel::for_profile(profile::get("ireader").unwrap());
    apply(
        &mut b,
        &mut WashReport::default(),
        None,
        &reader,
        BoxGeometry {
            area_w: 1264.0,
            em_px: 37.5,
            v_rem_px: 49.4,
            h_scale: 1.0,
        },
    );
    let page = String::from_utf8(b[2].data.clone()).unwrap();
    assert!(
        page.contains(r#"<p class="px eink-b1">甲</p><p class="pc">乙</p>"#),
        "掌阅的 % 照 CSS：{page}"
    );
}

/// 上下内边距折进相邻块的间距：后一块不是标题写它的上外边距，是标题写前一块的下外边距；标题的外边距也照 Kindle 写到相邻的块上。
/// 页首的标题前面的空块、垫图不配对。
#[test]
fn vertical_gaps_move_to_honored_side() {
    let (page, _, _) = run(
        r#"<div class="eink-sp"><img class="eink-sp-1" src="s.png" alt=""/></div><div id="pb"></div><h2 class="t">首</h2><p>文</p>"#,
        "h2.t{margin:2em 0 1.5em}",
    );
    assert!(page.contains(r#"<div class="eink-sp"><img class="eink-sp-1" src="s.png" alt=""/></div><div id="pb"></div><h2 class="t">首</h2><p class="eink-b1">文</p>"#), "{page}");
    let body = r#"<p class="a">甲</p><p class="b">乙</p><h2 class="t">标题</h2><p>丙</p>"#;
    let (page, sheet, _) = run(body, "p{margin:1em 0} p.a{padding-bottom:0.5em} p.b{padding-top:1em} h2.t{margin:2em 0 1.5em}");
    // h2 字号 1.5em：上外边距 2em＝3 根 em、下 1.5em＝2.25。甲→乙：collapse(1,1)+0.5+1 = 2.5 写在乙上；乙→标题：collapse(1,3) = 3
    // 写在乙的下外边距；标题→丙：collapse(2.25,1) = 2.25 写在丙上
    assert!(page.contains(r#"<p class="a">甲</p><p class="b eink-b1">乙</p><h2 class="t">标题</h2><p class="eink-b2">丙</p>"#), "{page}");
    assert!(
        sheet.contains("html body p.eink-b1{margin-top:2.5em;margin-bottom:3em;}")
            && sheet.contains("html body p.eink-b2{margin-top:2.25em;}"),
        "{sheet}"
    );
}

/// 上下边框插横线图（块里最前、最后），连着的内边距画进图里、不再算进间距；文件开头的块也插（KINDLE_RULES 35，垫图让出它的高）；
/// 左右边框不管。
#[test]
fn top_and_bottom_borders_become_rule_images() {
    let body = r#"<h2 class="u">标题</h2><p>正文</p><div class="box">框</div>"#;
    let (page, sheet, b) = run(body, "h2.u{border-top:1px solid #000;border-bottom:2px dotted #f00;padding-bottom:0.5em} div.box{border-top:1px solid #000;border-bottom:1px solid #000}");
    assert!(page.contains(r#"<h2 class="u"><img class="eink-sp eink-r1" src="eink-rule-1.png" alt=""/>标题<img class="eink-sp eink-r2" src="eink-rule-2.png" alt=""/></h2>"#), "{page}");
    assert!(page.contains(r#"<div class="box"><img class="eink-sp eink-r1" src="eink-rule-1.png" alt=""/>框<img class="eink-sp eink-r3" src="eink-rule-3.png" alt=""/></div>"#), "{page}");
    assert!(
        sheet.contains(
            "html body img.eink-r1{display:block;width:100%;height:auto;margin:0;padding:0;}"
        ),
        "{sheet}"
    );
    let png = image::load_from_memory(
        &b.iter()
            .find(|e| e.name == "O/eink-rule-2.png")
            .unwrap()
            .data,
    )
    .unwrap()
    .to_rgba8();
    // 下边框：上面透明（0.5em 内边距），最下面一行红色点线
    let h = png.height();
    assert!(h > 20, "{h}");
    assert_eq!(png.get_pixel(0, 0).0[3], 0);
    assert_eq!(png.get_pixel(0, h - 1).0, [255, 0, 0, 255]);
    let opf = String::from_utf8(b[0].data.clone()).unwrap();
    assert!(
        opf.contains(r#"href="eink-rule-1.png" media-type="image/png""#),
        "{opf}"
    );
}

/// 左右有框的列表也包进单格表格（KINDLE_RULES 35，《雪国》注释列表）：列表自己的左右内边距留给项目符号，单元格只写上下的。
#[test]
fn framed_list_goes_into_table() {
    let (page, _, _) = run(r#"<p>前</p><ol class="n"><li>甲</li></ol><p>后</p>"#, "ol.n{border:1px dashed #336666;padding:10px 2em} td{font-size:0.6em;line-height:100%} table{text-align:center}");
    let at = page.find(r#"<ol class="n">"#).expect("列表还在");
    let td = &page[page[..at].rfind("<td").unwrap()..at];
    assert!(td.contains("dashed #336666") && td.contains("px 0px 19px 62px;vertical-align:top;font-size:1em;line-height:normal;text-align:left;"), "列表左内边距补到 Kindle 的缩进（842×0.103−25）: {page}");
    assert!(
        td.contains("padding:") && !td.contains("padding:0px 0px 0px 0px"),
        "上下内边距写到单元格: {page}"
    );
    // 书里给 td、table 的规则落在我们的表格上：单元格照外层写回字号、行距、对齐
    assert!(
        td.contains("font-size:1em;line-height:normal;text-align:left;"),
        "{page}"
    );
}

/// 页首的块上内边距很大（《梁羽生》卷名 `padding-top:6em`，字号 3 倍）：上横线图照插（连着透明的一截），垫图不扣它（扣了垫图就
/// 没了，卷名竖排页被推到页底）。这里只看横线图在（垫图的高在 `top_spacer` 的测试里）。
#[test]
fn big_top_padding_at_page_top_keeps_rule() {
    let (page, _, _) = run(
        r#"<div class="eink-sp"><img class="eink-sp eink-sp-36_6" src="s.png" alt=""/></div><h1 class="t">卷名</h1><p>正文</p>"#,
        "h1.t{font-size:3em;border-top:5px solid #ddd;padding-top:6em}",
    );
    assert!(
        page.contains(r#"<h1 class="t"><img class="eink-sp eink-r"#),
        "{page}"
    );
}

/// 页首块的上横线图有多高（垫图要让出来）：线粗 + 不认内边距时的上内边距；左右有框（包进表格）的、认边框的阅读器不算。
#[test]
fn page_top_rule_height() {
    let reader = ReaderModel::for_profile(profile::fixture("xochitl").unwrap());
    let comp = |css: &str| {
        let b = book(r#"<h1 class="t">第一部</h1>"#, css);
        let text = String::from_utf8(b[2].data.clone()).unwrap();
        let css = km::book_css(&b, None);
        let ch = Chapter::parse("O/c.xhtml", &text, None, &css).unwrap();
        ch.els[0].comp.as_ref().clone()
    };
    let h = page_top_rule_px(
        &comp("h1.t{border-top:2px dotted #000;padding-top:1em}"),
        "h1",
        &reader,
        50.0,
    )
    .unwrap();
    // 2px = 0.9pt = 0.075 rem → 3.75px；上内边距 1em（h1 字号 2em）= 2 rem → 100px
    assert!((h - 103.75).abs() < 0.01, "{h}");
    assert_eq!(
        page_top_rule_px(&comp("h1.t{border:2px solid #000}"), "h1", &reader, 50.0),
        None
    );
    // 上内边距大到拿来推字的（《梁羽生》卷名）：只画线（2px＝0.075 rem → 3.75px），距离在垫图里
    let h = page_top_rule_px(
        &comp("h1.t{border-top:2px solid #000;padding-top:3em}"),
        "h1",
        &reader,
        50.0,
    )
    .unwrap();
    assert!((h - 3.75).abs() < 0.01, "{h}");
    assert_eq!(
        page_top_rule_px(
            &comp("h1.t{border-bottom:2px solid #000}"),
            "h1",
            &reader,
            50.0
        ),
        None
    );
    let ir = ReaderModel::for_profile(profile::get("ireader").unwrap());
    assert_eq!(
        page_top_rule_px(&comp("h1.t{border-top:2px solid #000}"), "h1", &ir, 50.0),
        None
    );
}
