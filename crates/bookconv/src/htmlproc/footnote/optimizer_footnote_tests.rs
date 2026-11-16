//! 原 `footnote.rs` 的单元测试（拆分后测试路径不变：`htmlproc::footnote::optimizer_footnote_tests::…`）。
use super::*;
use std::collections::{HashMap, HashSet};

fn note(inner: &str) -> Note {
    Note {
        inner: inner.to_string(),
        list: None,
    }
}

// ---- collect_footnote_notes：只搬被引用 + 语义确证的块 ----
#[test]
fn collects_referenced_p_and_li_notes() {
    // 书末尾注文件：一个 <p> 尾注 + 一个 <li> 尾注 + 一个非注释 <p>（普通段）。
    let notes = r#"<p class="footnote" id="n1">注释一 <a href="text01.xhtml#r1">↩</a></p><ol><li class="endnote" id="n2">注释二</li></ol><p id="plain">普通段落无语义</p>"#;
    let mut referenced = HashSet::new();
    referenced.insert("n1".to_string());
    referenced.insert("n2".to_string());
    referenced.insert("plain".to_string()); // 即便被引用，无注释语义也不搬
    let (cleaned, idx) = collect_footnote_notes(notes, &referenced, true);
    let ids: Vec<&str> = idx.iter().map(|(i, _)| i.as_str()).collect();
    assert!(
        ids.contains(&"n1") && ids.contains(&"n2"),
        "referenced 的 p/li 注释应被收: {ids:?}"
    );
    assert!(
        !ids.contains(&"plain"),
        "无注释语义的 <p> 不该被搬: {ids:?}"
    );
    assert!(
        !cleaned.contains(r##"id="n1""##) && !cleaned.contains(r##"id="n2""##),
        "已收注释应从原位移除: {cleaned}"
    );
    assert!(
        cleaned.contains(r##"id="plain""##),
        "普通段落应原样保留: {cleaned}"
    );
}

#[test]
fn duplicate_note_id_second_stays_in_place() {
    // 《绝叫》：两条不同的注释都叫 footnote-3-15（id 还重复写在里面的 li 上）。只能搬一条，另一条留在原处，一个字不丢。
    let notes = r#"<p>甲</p><aside epub:type="footnote" id="n1"><ol><li id="n1">第一条</li></ol></aside><p>乙</p><aside epub:type="footnote" id="n1"><ol><li id="n1">第二条</li></ol></aside>"#;
    let referenced: HashSet<String> = ["n1".to_string()].into();
    let (cleaned, idx) = collect_footnote_notes(notes, &referenced, true);
    assert_eq!(idx.len(), 1);
    assert!(idx[0].1.inner.contains("第一条"));
    assert_eq!(
        cleaned,
        r#"<p>甲</p><p>乙</p><aside epub:type="footnote" id="n1"><ol><li id="n1">第二条</li></ol></aside>"#
    );
}

#[test]
fn skips_unreferenced_notes_no_loss() {
    // 未被任何 marker 引用的注释块必须原地保留（杜绝内容丢失）。
    let notes = r#"<p class="footnote" id="orphan">没人引用的注释文本</p>"#;
    let (cleaned, idx) = collect_footnote_notes(notes, &HashSet::new(), true);
    assert!(idx.is_empty(), "无引用不该收");
    assert_eq!(cleaned, notes, "无引用的注释应原样不动: {cleaned}");
}

#[test]
fn collects_flat_div_notes_but_skips_nested() {
    let mut referenced = HashSet::new();
    referenced.insert("d1".to_string());
    referenced.insert("d2".to_string());
    // d1 扁平 div 注释应收；d2 含嵌套 div → 跳过不搬（零丢失）
    let notes = r#"<div class="footnote" id="d1">扁平注释文本</div><div class="footnote" id="d2">外<div>内嵌</div>层</div>"#;
    let (cleaned, idx) = collect_footnote_notes(notes, &referenced, true);
    let ids: Vec<&str> = idx.iter().map(|(i, _)| i.as_str()).collect();
    assert!(
        ids.contains(&"d1") && !ids.contains(&"d2"),
        "扁平 div 收、嵌套 div 跳过: {ids:?}"
    );
    assert!(
        !cleaned.contains(r##"id="d1""##) && cleaned.contains(r##"id="d2""##),
        "{cleaned}"
    );
}

// ---- referenced_note_frags：noteref + 跨文件普通 <a> ----
#[test]
fn scans_noteref_and_crossfile_markers() {
    let ch = r##"<p>甲<a epub:type="noteref" href="notes.xhtml#a1">1</a>乙<a href="notes.xhtml#a2">2</a>丙<a href="#local">本地</a></p>"##;
    let mut fr = referenced_note_frags(ch);
    fr.sort();
    assert!(fr.contains(&"a1".to_string()), "noteref frag 应收: {fr:?}");
    assert!(
        fr.contains(&"a2".to_string()),
        "跨文件普通 <a> frag 应收: {fr:?}"
    );
    assert!(
        !fr.contains(&"local".to_string()),
        "同文件裸锚点不归优化器搬运（break_cycles 管）: {fr:?}"
    );
}

// ---- preserve_relink_footnotes：跨文件普通尾注（"中间章节点了不跳"的主因）----
#[test]
fn crossfile_plain_endnote_relinked() {
    let mut index: HashMap<NoteKey, Note> = HashMap::new();
    index.insert(
        ("notes/notes.xhtml".to_string(), "n12".to_string()),
        note("第十二条注释文本"),
    );
    let chapter = r#"<html><body><p>正文波波<a href="../notes/notes.xhtml#n12">12</a>后续</p></body></html>"#;
    let out = preserve_relink_footnotes(
        chapter,
        "c.xhtml",
        &index,
        crate::optimize::FootnoteMode::Anchor,
    );
    assert!(
        out.contains(r##"<a href="#n12">12</a>"##),
        "跨文件 marker 未改成同章锚点: {out}"
    );
    assert!(
        !out.contains("notes.xhtml"),
        "跨文件 href 前缀未去掉: {out}"
    );
    assert!(
        out.contains(r##"<div id="n12" class="eink-note">第十二条注释文本</div>"##),
        "注释未搬进本章章末: {out}"
    );
    // 注释区必须落在 </body> 之内
    let body_end = out.find("</body>").unwrap();
    assert!(
        out[..body_end].contains(r##"<div class="footnotes">"##),
        "注释区落到 </body> 外: {out}"
    );
    // 大写的 </BODY> 也认：注释区不能落到 </html> 后面
    let upper = chapter.replace("</body></html>", "</BODY></HTML>");
    let out = preserve_relink_footnotes(
        &upper,
        "c.xhtml",
        &index,
        crate::optimize::FootnoteMode::Anchor,
    );
    assert!(
        out.ends_with("</div>\n</BODY></HTML>"),
        "注释区落到 </BODY> 外: {out}"
    );
}

// 注释源块自带块级标签（<aside>/<li>/<div> 包一层 <p>）是常见形态；collect_footnote_notes 只搬
// 内层 html，若外层落点仍用 <p> 包一遍就会产出 <p><p>…</p></p> 这种非法内容模型（<p> 不能合法
// 含块级子元素）。这条测试端到端跑 collect_footnote_notes → preserve_relink_footnotes 全链路，
// 断言输出永远是 <div id> 落点、不出现嵌套 <p>。
#[test]
fn note_source_block_with_nested_p_does_not_produce_illegal_nested_p() {
    let notes_chapter = r##"<aside class="footnote" id="fn1"><p>注释正文，含<a href="#backref">回链</a></p></aside>"##;
    let mut referenced = HashSet::new();
    referenced.insert("fn1".to_string());
    let (_, idx_vec) = collect_footnote_notes(notes_chapter, &referenced, true);
    let index: HashMap<NoteKey, Note> = idx_vec
        .into_iter()
        .map(|(id, t)| (("notes.xhtml".to_string(), id), t))
        .collect();
    assert!(
        index
            .get(&("notes.xhtml".to_string(), "fn1".to_string()))
            .unwrap()
            .inner
            .contains("<p>"),
        "前提：源块内层确实带 <p>，测试才有意义"
    );

    let chapter = r#"<html><body><p>正文<a href="notes.xhtml#fn1">1</a>续</p></body></html>"#;
    let out = preserve_relink_footnotes(
        chapter,
        "c.xhtml",
        &index,
        crate::optimize::FootnoteMode::Anchor,
    );
    assert!(
        !out.contains("<p><p>") && !out.contains("<p><a href=\"#backref\">"),
        "落点不该是 <p> 包 <p>: {out}"
    );
    assert!(
        out.contains(r#"<div id="fn1" class="eink-note"><p>注释正文"#),
        "落点应是 <div id> 包住源块内层 html 原样: {out}"
    );
}

/// `<sup>` 包裹的图标 noteref：标号原样保留（不再换成 `[N]`，2026-09-29 定），图标加 `eink-noteicon` 类限成一个字高。
#[test]
fn sup_wrapped_image_marker_kept_with_icon_class() {
    let mut index: HashMap<NoteKey, Note> = HashMap::new();
    index.insert(
        ("c.xhtml".to_string(), "fo14".to_string()),
        note("注释文字"),
    );
    let chapter = r##"<p>正文<sup><a type="noteref" href="#fo14"><img alt="" src="../Images/note.png"/></a></sup>续</p>"##;
    let out = preserve_relink_footnotes(
        chapter,
        "c.xhtml",
        &index,
        crate::optimize::FootnoteMode::Anchor,
    );
    assert!(out.contains(r##"<sup><a href="#fo14"><img class="eink-noteicon" alt="" src="../Images/note.png"/></a></sup>续"##), "{out}");
    assert!(!out.contains("[1]"), "不加 [N]: {out}");
}

/// 弹窗模式：标号 `epub:type="noteref"`、注释块 `<aside epub:type="footnote">`，根元素补 epub 命名空间；
/// 可见文字和跳转模式一样（标号原样，一个字不加）。
#[test]
fn popup_mode_marks_noteref_and_aside_footnote() {
    let mut index: HashMap<NoteKey, Note> = HashMap::new();
    index.insert(
        ("notes.xhtml".to_string(), "n1".to_string()),
        note("注释一"),
    );
    let chapter = r#"<html xmlns="http://www.w3.org/1999/xhtml"><body><p>正文<sup><a epub:type="noteref" href="notes.xhtml#n1">1</a></sup>续<a class="x" href="notes.xhtml#n1">1</a></p></body></html>"#;
    let out = preserve_relink_footnotes(
        chapter,
        "c.xhtml",
        &index,
        crate::optimize::FootnoteMode::Popup,
    );
    assert!(out.starts_with(r#"<html xmlns:epub="http://www.idpf.org/2007/ops" xmlns="http://www.w3.org/1999/xhtml">"#), "{out}");
    assert!(out.contains(r##"<sup><a epub:type="noteref" href="#n1">1</a></sup>续<a class="x" href="#n1" epub:type="noteref">1</a>"##), "{out}");
    assert_eq!(
        out.matches(r#"<aside epub:type="footnote" id="n1" class="eink-note">注释一</aside>"#)
            .count(),
        1,
        "同一条注释只放一次: {out}"
    );
    let jump = preserve_relink_footnotes(
        chapter,
        "c.xhtml",
        &index,
        crate::optimize::FootnoteMode::Anchor,
    );
    assert!(
        !jump.contains("epub:"),
        "跳转模式不带 epub:type（xochitl 不认）: {jump}"
    );
    assert_eq!(ensure_epub_ns(&out), out, "命名空间只补一次");
}

/// 真机《甲午：摇摆的战争》坐实的真实结构：`duokan-footnote` 类挂在外层 `<a>` 上（不在 `<img>`
/// 自己的 class 里），此前 `fix_duokan_markers` 只查 `img_inner` 会漏判、80×80 图标原样穿透到
/// xochitl，按固有像素撑成巨大方块。
#[test]
fn fix_duokan_markers_detects_class_on_outer_a_not_just_img() {
    let out = fix_duokan_markers(
        r##"<sup><a class="duokan-footnote" href="#fo14" id="foref14"><img alt="" class="exs" src="../Images/note.png"/></a></sup>"##,
    );
    assert_eq!(
        out,
        r##"<sup><a href="#fo14" id="foref14"><img alt="" class="exs eink-noteicon" src="../Images/note.png"/></a></sup>"##,
        "图标原样保留、加限高的类，不换成数字"
    );
}

#[test]
fn number_icon_note_links_for_readers_without_image_links() {
    // 多看标号（fix_duokan_markers 保留图标之后）：编号取注释开头的 [14]
    let ch = r##"<p>正文<sup><a href="#fo14" id="foref14"><img alt="" class="exs eink-noteicon" src="../Images/note.png"/></a></sup>续</p><div id="fo14" class="eink-note"><p>[14] 注释文字</p></div>"##;
    let out = number_icon_note_links(ch);
    assert!(
        out.contains(r##"<sup><a href="#fo14" id="foref14">14</a></sup>续"##),
        "{out}"
    );
    assert!(out.contains("[14] 注释文字"), "注释本身不动");
    // 没有作者编号：取 alt 里的"注释7"；再没有按本章顺序数；没包 <sup> 的补一层
    let alt = r##"<a href="#n1"><img class="eink-noteicon" alt="注释7" src="i.png"/></a><a href="#n2"><img class="eink-noteicon" src="i.png"/></a><div id="n1">甲</div><div id="n2">乙</div>"##;
    assert_eq!(
        number_icon_note_links(alt),
        r##"<a href="#n1"><sup>7</sup></a><a href="#n2"><sup>2</sup></a><div id="n1">甲</div><div id="n2">乙</div>"##
    );
    // 没有注释特征的图片链接（插图放大、目录图标）、链接里有字的：不动
    for keep in [
        r##"<a href="#fig1"><img src="small.png"/></a><div id="fig1"><img src="big.png"/></div>"##,
        r##"<a href="#fo1" class="note"><img src="i.png"/>注</a><p id="fo1">x</p>"##,
    ] {
        assert_eq!(number_icon_note_links(keep), keep);
    }
    // 目标有注释语义也算
    let sem = r##"<a href="#f"><img src="i.png"/></a><aside epub:type="footnote" id="f">（3）说明</aside>"##;
    assert!(number_icon_note_links(sem).starts_with(r##"<a href="#f"><sup>3</sup></a>"##));
}

/// 快速预判要认得 `html::attrs` 认的所有 id 写法（此前只认 `id="…"`/`id='…'`，`id = "n1"`、不带引号的注释块收不走）。
#[test]
fn mentions_referenced_id_covers_spaced_and_unquoted_ids() {
    let referenced: HashSet<String> = ["n1".to_string()].into_iter().collect();
    for html in [
        r#"<p id = "n1" class="footnote">注</p>"#,
        r#"<p ID=n1 class="footnote">注</p>"#,
        r#"<p class="footnote" id='n1'>注</p>"#,
    ] {
        assert!(mentions_referenced_id(html, &referenced), "{html}");
        let (cleaned, idx) = collect_footnote_notes(html, &referenced, true);
        assert_eq!(idx.len(), 1, "{html}");
        assert!(!cleaned.contains('注'), "{cleaned}");
    }
    for html in ["<p>did n1 idea</p>", r#"<p id="n10">x</p>"#] {
        assert!(!mentions_referenced_id(html, &referenced), "{html}");
    }
}

/// 图标加类不累积（重优化同一本书）；`class=""` 不留前导空格。
#[test]
fn mark_note_icons_is_idempotent() {
    let once = mark_note_icons(
        r#"<img class="exs" src="a.png"/><img class="" src="b.png"/><img src="c.png"/>"#,
    );
    assert_eq!(
        once,
        r#"<img class="exs eink-noteicon" src="a.png"/><img class="eink-noteicon" src="b.png"/><img class="eink-noteicon" src="c.png"/>"#
    );
    assert_eq!(mark_note_icons(&once), once);
}

/// 同一章里重复的 id：第二个起改成唯一的新名，链接仍指向第一个。
#[test]
fn dedup_renames_repeated_id_within_one_chapter() {
    let mut seen: HashSet<String> = HashSet::new();
    let ch = r##"<p id="a">甲</p><a href="#a">1</a><div id="a">乙</div><p id="a-x2">丙</p><span id=a>丁</span>"##;
    let out = dedup_ids_in_chapter(ch, &mut seen);
    assert_eq!(
        out,
        r##"<p id="a">甲</p><a href="#a">1</a><div id="a-x3">乙</div><p id="a-x2">丙</p><span id=a-x4>丁</span>"##
    );
    // 下一章再用 `a`、`a-x3`：照常按跨章改名
    let out2 = dedup_ids_in_chapter(r##"<p id="a-x3">戊</p>"##, &mut seen);
    assert!(!out2.contains(r#"id="a-x3""#), "{out2}");
}

#[test]
fn crossfile_nonnote_link_untouched() {
    // 目标不在 index（不是注释）→ 跨文件链接原样不动，绝不误搬目录/交叉引用。
    let index: HashMap<NoteKey, Note> = HashMap::new();
    let chapter = r#"<p>见<a href="chap03.xhtml#sec2">第三章</a></p>"#;
    assert_eq!(
        preserve_relink_footnotes(
            chapter,
            "c.xhtml",
            &index,
            crate::optimize::FootnoteMode::Anchor
        ),
        chapter
    );
}

/// 2026-09-28 审计：改名 `fn1`→`fn1-x2` 不能撞上本章后面本来就有的 `fn1-x2`。
#[test]
fn dedup_new_name_avoids_later_id_in_same_chapter() {
    let mut seen: HashSet<String> = ["fn1".to_string()].into_iter().collect();
    let ch = r##"<p id="fn1">甲</p><a href="#fn1">1</a><p id="fn1-x2">乙</p>"##;
    let out = crate::htmlproc::dedup_ids_in_chapter(ch, &mut seen);
    assert_eq!(
        out,
        r##"<p id="fn1-x3">甲</p><a href="#fn1-x3">1</a><p id="fn1-x2">乙</p>"##
    );
}

// ---- dedup_ids_in_chapter：跨章 id 撞车（"跳到错章"的次因）----
#[test]
fn dedup_renames_colliding_ids_across_chapters() {
    let mut seen = HashSet::new();
    let ch1 = r##"<p>甲<a href="#fn1">1</a></p><p id="fn1">注释甲</p>"##;
    let out1 = dedup_ids_in_chapter(ch1, &mut seen);
    assert_eq!(out1, ch1, "首章 id 不冲突，原样");
    // 第二章又用了 id="fn1" → 必须改名，且本章内 href="#fn1" 同步改
    let ch2 = r##"<p>乙<a href="#fn1">1</a></p><p id="fn1">注释乙</p>"##;
    let out2 = dedup_ids_in_chapter(ch2, &mut seen);
    assert!(!out2.contains(r##"id="fn1""##), "撞车 id 未改名: {out2}");
    // 改名后 marker 与目标仍配对（同一新 id）
    let new_id = Regex::new(r##"id="(fn1-x\d+)""##)
        .unwrap()
        .captures(&out2)
        .map(|c| c.get(1).unwrap().as_str().to_string());
    let new_id = new_id.unwrap_or_else(|| panic!("未生成唯一新 id: {out2}"));
    assert!(
        out2.contains(&format!(r##"href="#{new_id}""##)),
        "本章 href 未随之改名: {out2}"
    );
    assert!(out2.contains("注释乙"), "注释文本不应丢");
}

#[test]
fn dedup_leaves_crossfile_href_alone() {
    // 跨文件 href="f#fn1" 不因本章 id 改名而被动（它指向别的文件）。
    let mut seen = HashSet::new();
    seen.insert("fn1".to_string()); // 假装别章已用过 fn1
    let ch = r#"<p id="fn1">本章注释</p><p>另见<a href="other.xhtml#fn1">跨文件</a></p>"#;
    let out = dedup_ids_in_chapter(ch, &mut seen);
    assert!(!out.contains(r##"<p id="fn1">"##), "本章 id 应改名");
    assert!(
        out.contains(r##"href="other.xhtml#fn1""##),
        "跨文件 href 不该被改: {out}"
    );
}

#[test]
fn dedup_renames_many_ids_in_one_pass_and_matches_case_exactly() {
    // 一章里多个 id 同时撞车：各自改成互不相同的新名，且各自的同文件 href 跟着改；
    // 大小写不同的 id（HTML id 区分大小写）不能被误当同一个而一起改。
    let mut seen: HashSet<String> = ["fn1", "fn2", "Fn1"]
        .iter()
        .map(|s| s.to_string())
        .collect();
    let ch = r##"<p id="fn1">甲</p><a href="#fn1">1</a><p id="fn2">乙</p><a href="#fn2">2</a><p id="Fn1">丙</p><a href="#Fn1">3</a><a href="#keep">4</a>"##;
    let out = dedup_ids_in_chapter(ch, &mut seen);
    let ids: Vec<String> = Regex::new(r##"\bid="([^"]+)""##)
        .unwrap()
        .captures_iter(&out)
        .map(|c| c[1].to_string())
        .collect();
    assert_eq!(ids.len(), 3);
    assert!(
        ids.iter().collect::<HashSet<_>>().len() == 3,
        "三个新 id 必须互不相同: {out}"
    );
    for id in &ids {
        assert!(id.contains("-x"), "都应改名: {out}");
        assert!(
            out.contains(&format!(r##"href="#{id}""##)),
            "href 未随 {id} 改名: {out}"
        );
    }
    assert!(out.contains(r##"href="#keep""##), "无关 href 原样: {out}");
    assert!(
        ids[2].starts_with("Fn1-x"),
        "大写 Fn1 改名后保留自己的大小写前缀: {out}"
    );
}

// ---- collapse_dup_id_attrs：同元素双 id 属性（非法 XHTML → reMarkable 整章渲染失败）----
#[test]
fn collapse_keeps_first_id_and_drops_rest() {
    // 首位是注入锚点(aid/fp)，既存 id 在后 → 保住首位、删后续
    let h = r#"<p id="aid5N3C1" class="calibre6" id="filepos18251">正文</p>"#;
    let out = collapse_dup_id_attrs(h);
    assert_eq!(
        out, r#"<p id="aid5N3C1" class="calibre6">正文</p>"#,
        "应保首位 id、删后续: {out}"
    );
    // 无重复 → 原样；不误伤 aid=（不是 id 属性）
    let ok = r#"<p aid="X" id="y">t</p>"#;
    assert_eq!(
        collapse_dup_id_attrs(ok),
        ok,
        "单 id 应原样、aid 不误删: {}",
        collapse_dup_id_attrs(ok)
    );
    // 三个 id 也只留首个
    let three = r#"<div id="a" id="b" id="c"></div>"#;
    assert_eq!(collapse_dup_id_attrs(three), r#"<div id="a"></div>"#);
}

/// 2026-10-03 审计：重建标号时整个 `<a>` 重写成 `<a href>`，丢了 `id="fnref1"`，注释里指回 `#fnref1` 的回链成了死链（pandoc 写法）。
/// 现在只改 `href`，`id`/`class` 和 `<sup>` 的属性都留着；质量门查同文件锚点，回链指得到。
#[test]
fn marker_keeps_id_so_note_backlink_resolves() {
    let ch = r##"<html><body><p>正文<sup class="s"><a href="#fn1" class="footnote-ref" id="fnref1" epub:type="noteref">1</a></sup>续</p><section class="footnotes" epub:type="footnotes"><hr/><ol><li id="fn1" class="footnote"><p>注释<a href="#fnref1" class="footnote-back">↩</a></p></li></ol></section></body></html>"##;
    let referenced: HashSet<String> = referenced_note_keys(ch, "OEBPS/c.xhtml")
        .into_iter()
        .map(|k| k.1)
        .collect();
    let (cleaned, idx) = collect_footnote_notes(ch, &referenced, true);
    let index: HashMap<NoteKey, Note> = idx
        .into_iter()
        .map(|(id, n)| (("OEBPS/c.xhtml".to_string(), id), n))
        .collect();
    let out = preserve_relink_footnotes(
        &cleaned,
        "OEBPS/c.xhtml",
        &index,
        crate::optimize::FootnoteMode::Anchor,
    );
    assert!(
        out.contains(
            r##"<sup class="s"><a href="#fn1" class="footnote-ref" id="fnref1">1</a></sup>续"##
        ),
        "标号只改 href、去掉 noteref 类型: {out}"
    );
    assert!(
        out.contains(r##"<a href="#fnref1" class="footnote-back">↩</a>"##),
        "{out}"
    );
    let rep = crate::check::check_entries(
        &[crate::epubzip::Entry {
            name: "OEBPS/c.xhtml".into(),
            data: out.clone().into_bytes(),
        }],
        false,
    );
    assert_eq!(
        rep.dead_anchor_links, 0,
        "回链与标号都指得到: {:?} {out}",
        rep.warnings
    );
    // 旧写法（丢 id）质量门报得出来
    let broken = out.replace(r#" id="fnref1""#, "");
    let rep = crate::check::check_entries(
        &[crate::epubzip::Entry {
            name: "OEBPS/c.xhtml".into(),
            data: broken.into_bytes(),
        }],
        false,
    );
    assert_eq!(rep.dead_anchor_links, 1, "{:?}", rep.warnings);
}

/// 注释块的 id 带字符引用、注释文件名带 `&`：链接解析还原字符引用后才认得出（此前把属性原文直接拿去解析路径）；放过去时 id 再转义。
#[test]
fn note_keys_decode_char_refs_in_path_and_id() {
    let ch = r##"<p>正文<a epub:type="noteref" href="n&amp;b.xhtml#a&amp;1">1</a></p>"##;
    assert_eq!(
        referenced_note_keys(ch, "OEBPS/c.xhtml"),
        [("OEBPS/n&b.xhtml".to_string(), "a&1".to_string())]
    );
    let referenced: HashSet<String> = ["a&1".to_string()].into_iter().collect();
    let (_, idx) = collect_footnote_notes(
        r#"<aside id="a&amp;1" class="footnote">注</aside>"#,
        &referenced,
        true,
    );
    assert_eq!(idx[0].0, "a&1");
    let index: HashMap<NoteKey, Note> = idx
        .into_iter()
        .map(|(id, n)| (("OEBPS/n&b.xhtml".to_string(), id), n))
        .collect();
    let out = preserve_relink_footnotes(
        ch,
        "OEBPS/c.xhtml",
        &index,
        crate::optimize::FootnoteMode::Anchor,
    );
    assert!(
        out.contains(r##"<a href="#a&amp;1">1</a>"##)
            && out.contains(r#"<div id="a&amp;1" class="eink-note">注</div>"#),
        "{out}"
    );
}

/// 有序列表里的注释：列表编号是看得见的内容，搬过去连同编号放成 `<ol start=N>`（相邻、编号连续的合成一组）；
/// 搬空的 `<ol>` 与只剩分隔线的注释区一起清掉，不留空列表、不多一条分隔线。
#[test]
fn ordered_list_notes_keep_numbers_and_leave_no_empty_list() {
    let notes = r##"<html><body><p>后记</p>
<hr/>
<section class="footnotes"><hr/><ol start="3" class="fn"><li id="n3" class="footnote">甲</li><li id="n4" class="footnote">乙</li><li id="n9" class="footnote" value="9">丙</li></ol></section>
</body></html>"##;
    let referenced: HashSet<String> =
        ["n3", "n4", "n9"].iter().map(|s| s.to_string()).collect();
    let (cleaned, idx) = collect_footnote_notes(notes, &referenced, true);
    assert_eq!(
        cleaned, "<html><body><p>后记</p>\n\n</body></html>",
        "空列表、注释区与它前面的分隔线都去掉"
    );
    let nums: Vec<i64> = idx
        .iter()
        .map(|(_, n)| n.list.as_ref().unwrap().number)
        .collect();
    assert_eq!(nums, [3, 4, 9]);
    assert_eq!(
        idx[0].1.list.as_ref().unwrap().ol_open,
        r#"<ol class="fn">"#
    );
    let index: HashMap<NoteKey, Note> = idx
        .into_iter()
        .map(|(id, n)| (("n.xhtml".to_string(), id), n))
        .collect();
    let ch = r##"<html><body><p>正文<a href="n.xhtml#n3">3</a><a href="n.xhtml#n4">4</a><a href="n.xhtml#n9">9</a></p></body></html>"##;
    let out =
        preserve_relink_footnotes(ch, "c.xhtml", &index, crate::optimize::FootnoteMode::Anchor);
    assert!(out.contains("<div class=\"footnotes\">\n<ol class=\"fn\" start=\"3\">\n<li id=\"n3\" class=\"footnote eink-note\">甲</li>\n<li id=\"n4\" class=\"footnote eink-note\">乙</li>\n</ol>\n<ol class=\"fn\" start=\"9\">\n<li id=\"n9\" class=\"footnote eink-note\" value=\"9\">丙</li>\n</ol>\n</div>"), "{out}");
    assert_eq!(out.matches("<hr/>").count(), 1, "{out}");
    // 只搬走一部分：列表还在，编号照算（倒序列表也认）
    let part = r#"<ol id="R" reversed="reversed"><li id="a" class="footnote">一</li><li id="b" class="footnote">二</li></ol>"#;
    let (cleaned, idx) =
        collect_footnote_notes(part, &["b".to_string()].into_iter().collect(), true);
    assert_eq!(
        cleaned,
        r#"<ol id="R" reversed="reversed"><li id="a" class="footnote">一</li></ol>"#
    );
    assert_eq!(
        (
            idx[0].1.list.as_ref().unwrap().number,
            idx[0].1.list.as_ref().unwrap().ol_open.as_str()
        ),
        (1, "<ol>")
    );
    // 无序列表、带 id 的容器：编号不记 / 容器不删
    let (cleaned, idx) = collect_footnote_notes(
        r#"<div class="notes" id="end"><ul><li id="u" class="footnote">x</li></ul></div>"#,
        &["u".to_string()].into_iter().collect(),
        true,
    );
    assert_eq!(cleaned, r#"<div class="notes" id="end"></div>"#);
    assert!(idx[0].1.list.is_none());
}

/// 多看注释的三处识别改用 `crate::html`：单引号也认，`data-id`/`data-alt`/`data-href` 不算。
#[test]
fn duokan_patterns_use_attribute_parsing() {
    assert_eq!(
        duokan_note_num(r#"<img data-alt="注释9" alt='注释12' src="x"/>"#),
        Some("12".to_string())
    );
    assert_eq!(duokan_note_num(r#"<img data-alt="注释9"/>"#), None);
    for (alt, num) in [
        ("注12", Some("12")),
        ("note 3", Some("3")),
        ("[7]", Some("7")),
        ("5", Some("5")),
        ("note", None),
        ("1940年5月，纳粹空军轰炸鹿特丹。", None),
        ("恩奈弗（1869—1947）", None),
    ] {
        assert_eq!(
            duokan_note_num(&format!(r#"<img alt="{alt}"/>"#)).as_deref(),
            num,
            "{alt}"
        );
    }
    let blocks =
        r#"<p id='a_1_2'><p class="t">注</p></p><p data-id="a_1_3"><p class="t">别</p></p>"#;
    assert_eq!(
        flatten_duokan_note_blocks(blocks),
        r#"<p id="a_1_2" class="t">注</p><p data-id="a_1_3"><p class="t">别</p></p>"#
    );
    let back = r##"<a class='x' href='#c_1_2'>回</a><a data-href="#c_1_3" href="#y">留</a><a href="#c_1_x">留</a>"##;
    assert_eq!(
        unlink_duokan_backlinks(back),
        r##"回<a data-href="#c_1_3" href="#y">留</a><a href="#c_1_x">留</a>"##
    );
}

/// 快速预判只看标签里的 `id` 属性：正文里写着的 `id="n1"` 不算。
#[test]
fn mentions_referenced_id_ignores_text() {
    let referenced: HashSet<String> = ["n1".to_string()].into_iter().collect();
    assert!(!mentions_referenced_id(
        r#"<p>写法是 id="n1" 这样</p><p data-id="n1">x</p>"#,
        &referenced
    ));
    assert!(mentions_referenced_id(
        r#"<p class="a"id="n1">x</p>"#,
        &referenced
    ));
    assert!(
        mentions_referenced_id(r#"<p title='a>b' id="n1">x</p>"#, &referenced),
        "属性值里的 > 不截断标签"
    );
}
