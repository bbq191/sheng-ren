//! 原 `footnote.rs` 的单元测试（拆分后测试路径不变：`htmlproc::footnote::footnote_inline_tests::…`）。
use super::*;
use std::collections::{HashMap, HashSet};

#[test]
fn normalize_self_hrefs_only_own_file() {
    let html = r##"<a href="part0004.html#a_1">本文件</a><a href="text/part0004.html#b">带目录本文件</a><a href="part0005.html#c">别的文件</a><a href="part0004.html">无锚</a><a href="#d">已裸</a>"##;
    let out = normalize_self_hrefs(html, "part0004.html");
    assert!(out.contains(r##"href="#a_1""##), "本文件应归一: {out}");
    assert!(
        out.contains(r##"href="#b""##),
        "只给文件名时按末段比对: {out}"
    );
    assert!(
        out.contains(r##"href="part0005.html#c""##),
        "跨文件不动: {out}"
    );
    assert!(
        out.contains(r##"href="part0004.html">"##),
        "无 fragment 不动: {out}"
    );
    assert_eq!(normalize_self_hrefs(html, ""), html, "空 basename 原样");
    // 给完整路径时按路径解析：同名不同目录的文件不是本文件
    let h = r##"<a href="../b/x.html#f">别处同名</a><a href='x.html#g'>本文件</a><a href="../a/x.html#h">绕一圈回来</a>"##;
    let out = normalize_self_hrefs(h, "OEBPS/a/x.html");
    assert_eq!(
        out,
        r##"<a href="../b/x.html#f">别处同名</a><a href='#g'>本文件</a><a href="#h">绕一圈回来</a>"##
    );
}

#[test]
fn fix_duokan_markers_calibre_real_img_keeps_id() {
    // Calibre 洗后：真 <img>（本地图）+ <a> 自带回链落点 id。图标保留（加限高的类）、id 必须保留，不换成数字。
    let html = r##"<sup class="calibre4"><a class="duokan-footnote" href="#a_2_1" id="c_2_1"><img alt="注释7" class="duokan-footnote1" src="../images/00003.png"/></a></sup>"##;
    let out = fix_duokan_markers(html);
    assert_eq!(
        out,
        r##"<sup><a href="#a_2_1" id="c_2_1"><img alt="注释7" class="duokan-footnote1 eink-noteicon" src="../images/00003.png"/></a></sup>"##
    );
    // 转义成文字的那种（微读 CDN 图）：原书显示的是一串代码，换成它 alt 里的序号
    let esc = r##"<sup><a href="#fo3">&lt;img class="duokan-footnote" alt="注释3" src="https://cdn/x.png"/&gt;</a></sup>"##;
    assert_eq!(
        fix_duokan_markers(esc),
        r##"<a href="#fo3"><sup>3</sup></a>"##
    );
    // 非 duokan 的真 img 链接不碰
    let other = r##"<sup><a href="#x"><img alt="图" class="icon" src="i.png"/></a></sup>"##;
    assert_eq!(fix_duokan_markers(other), other);
}

#[test]
fn fix_duokan_markers_shared_fn() {
    // 多标记按 alt 号、非 duokan 的转义 img 不碰。
    let html = r##"<sup><a href="#a_1_1">&lt;img alt="注释1" class="duokan-footnote1" src="https://res.weread.qq.com/a.png"/&gt;</a></sup>正文<sup><a href="#a_1_2">&lt;img alt="注释2" class="duokan-footnote1" src="https://res.weread.qq.com/b.png"/&gt;</a></sup>
<sup><a href="#x">&lt;img alt="别的" class="other-icon"/&gt;</a></sup>"##;
    let out = fix_duokan_markers(html);
    assert!(
        out.contains(r##"<a href="#a_1_1"><sup>1</sup></a>"##),
        "标记1: {out}"
    );
    assert!(
        out.contains(r##"<a href="#a_1_2"><sup>2</sup></a>"##),
        "标记2按 alt 号: {out}"
    );
    assert!(!out.contains("duokan-footnote"), "duokan img 全清: {out}");
    assert!(
        out.contains(r##"class="other-icon""##),
        "非 duokan 的转义 img 不该被碰: {out}"
    );
}

#[test]
fn duokan_img_footnote_marker_becomes_tappable_sup() {
    // 《人骨拼图》真实 duokan 形态：标记的 <img> 被**实体转义**成字面死文本、又是微读 CDN
    // 远程图（离线渲染成一坨、点不动）；注释块**已同文件** <p id="a_8_1">（前向锚 #a_8_1 有效）。
    // 修复=标记换成干净可点上标数字、保留 href、注释块原地不动。
    // 真实结构：注释块是无效嵌套 <p>、内含悬空回链 <a href="#c_8_1">。
    let ch = r##"<p>喝了太多冰镇台克利<sup class="calibre4"><a href="#a_8_1">&lt;img alt="注释1" class="duokan-footnote1" src="https://res.weread.qq.com/x_00003.png" data-w="48px" data-ratio="1.000"/&gt;</a></sup>。</p>
<p id="a_8_1"><p class="pfootnotetext"><a class="calibre6" href="#c_8_1">一种由朗姆酒、柠檬汁和糖混合的加冰鸡尾酒。</a></p></p>"##;
    let out = fix_duokan_markers(ch);
    assert!(
        out.contains(r##"<a href="#a_8_1"><sup>1</sup></a>"##),
        "标记未换成可点上标: {out}"
    );
    assert!(!out.contains("&lt;img"), "转义死图标记未清除: {out}");
    assert!(
        !out.contains("res.weread.qq.com"),
        "远程 CDN 图未去掉: {out}"
    );
    assert!(out.contains("加冰鸡尾酒"), "注释文本不该丢: {out}");
    // 悬空回链去链（否则 reMarkable 判互指对整对丢弃、正向点不动）。
    assert!(!out.contains(r##"href="#c_8_1""##), "注释回链未去链: {out}");
    // 嵌套 <p> 拍平成合法单段（成簇相邻时无效嵌套会被 reMarkable 吞一条=缺一条）。
    assert!(
        out.contains(r##"<p id="a_8_1" class="pfootnotetext">"##),
        "嵌套 p 未拍平: {out}"
    );
}

#[test]
fn weread_p_note_collected_without_semantic() {
    // 13·67 真实形态：注释章里 <aside> 与 <p class="class_s1r">（**无 note 语义**）交替。
    // collect_footnote_notes(require_semantic=false) 须靠 id∈referenced 收 <p> 注释。
    let text = r#"<p>被叫做"大帮"<span id="a541"/><a href="part0040.html#a51T" class="class_s4yp"> 3 </a>后续</p>"#;
    let notes = r#"<aside id="a51U" type="footnote" class="class_svk"><a href="x#y">4</a>. CID</aside><p id='a51T' class="class_s1r"><a href="part0037.html#a541">3</a>. 帮办、大帮：一词八十年代已式微</p>"#;
    let referenced: HashSet<String> = referenced_note_frags(text).into_iter().collect();
    let (notes_cleaned, collected) = collect_footnote_notes(notes, &referenced, false);
    let map: HashMap<String, Note> = collected.into_iter().collect();
    assert!(
        map.contains_key("a51T"),
        "单引号 id 的 <p> 注释也要收: keys={:?}",
        map.keys().collect::<Vec<_>>()
    );
    assert!(!notes_cleaned.contains("a51T"), "a51T 注释未从原位移除");
}
