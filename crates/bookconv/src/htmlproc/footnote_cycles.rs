//! 脚注双向互指环的拆解（marker↔注释互相回链会让 xochitl 原生"返回"条失效）。
use super::*;

/// reMarkable 导入 EPUB 时把章内锚点烘焙成静态索引；一旦某章存在"互指对"
/// （marker 链到注释、注释又链回 marker——几乎所有电子书脚注的标准双向结构），
/// 它的索引器会把这一对链接**整对丢弃**，导致脚注在设备上连可点热区都没有。
/// 破环：定位每个 2-环，把"源元素较晚出现"那条（即注释里的回链）**去链化**
/// ——`<a>`→`<span>`、删掉 href、保留 id——正向 `marker→注释` 即恢复可点，
/// 返回交给 reMarkable 原生"返回第 X 页"条。真机 chap_0010 8 条脚注实测全通。
pub fn break_footnote_cycles(html: &str) -> String {
    // 1) 收集 id → 首次出现的字节位置（只认完整属性名 `id`，`data-id` 不算）
    let mut id_pos: HashMap<&str, usize> = HashMap::new();
    let mut ids_sorted: Vec<(usize, &str)> = Vec::new();
    // 同一趟顺手记下块级闭合标签的位置（判"中间有没有块结束"）与同文件锚点 <a href="#frag">。
    let mut block_closes: Vec<usize> = Vec::new();
    let mut links: Vec<(usize, usize, Option<&str>, String)> = Vec::new(); // (开标签起, 止, 自带 id, 目标)
    for t in html::tags(html) {
        if t.kind == html::TagKind::Close {
            if html::is_block(t.name) {
                block_closes.push(t.start);
            }
            continue;
        }
        if !t.is_start() {
            continue;
        }
        let tag = &html[t.start..t.end];
        let attrs = html::attrs(tag);
        let own_id = attrs.iter().find(|a| a.is("id") && !a.value.is_empty()).map(|a| a.value);
        if let Some(id) = own_id {
            id_pos.entry(id).or_insert(t.start);
            ids_sorted.push((t.start, id));
        }
        if t.kind == html::TagKind::Open && t.is("a") {
            // href 必须是同文件裸锚点 #frag
            if let Some(f) = attrs.iter().find(|a| a.is("href")).and_then(|a| a.value.strip_prefix('#')) {
                links.push((t.start, t.end, own_id, html::frag_id(f).into_owned()));
            }
        }
    }
    if id_pos.is_empty() {
        return html.to_string();
    }

    // 最近前置 id（中间无块级闭合标签才算同元素范围内）。`ids_sorted` 按位置升序：二分找"位置 < open_start 的最后一个"。
    let nearest = |open_start: usize| -> Option<&str> {
        let k = ids_sorted.partition_point(|(p, _)| *p < open_start);
        let (bp, id) = *ids_sorted.get(k.checked_sub(1)?)?;
        let c = block_closes.partition_point(|&p| p < bp);
        if block_closes.get(c).is_some_and(|&p| p < open_start) {
            None
        } else {
            Some(id)
        }
    };

    // 2) 同文件锚点：source_id（自带 id 优先，否则最近前置 id）+ target
    struct Anchor<'a> {
        open_start: usize,
        open_end: usize,
        source: &'a str,
        target: String,
    }
    let mut anchors: Vec<Anchor> = Vec::new();
    let mut edges: HashSet<(&str, &str)> = HashSet::new();
    for (open_start, open_end, own_id, target) in &links {
        if !id_pos.contains_key(target.as_str()) {
            continue;
        }
        let Some(source) = own_id.or_else(|| nearest(*open_start)) else { continue };
        anchors.push(Anchor { open_start: *open_start, open_end: *open_end, source, target: target.clone() });
    }
    for a in &anchors {
        edges.insert((a.source, a.target.as_str()));
    }

    // 3) 找 2-环 (A->B 且 B->A)，标记"源元素较晚"那条 <a> 去链化
    let mut kill: Vec<(usize, usize)> = Vec::new(); // (open_start, open_end)
    for a in &anchors {
        if a.source == a.target {
            continue;
        }
        if edges.contains(&(a.target.as_str(), a.source)) && id_pos[a.source] > id_pos[a.target.as_str()] {
            // 本锚点的源元素较晚 → 它是注释里的回链 → 去链
            kill.push((a.open_start, a.open_end));
        }
    }
    if kill.is_empty() {
        return html.to_string();
    }

    // 4) 改写：<a ...(去href)...> → <span ...>，对应的 </a>（不分大小写）→ </span>
    let mut edits: Vec<(usize, usize, String)> = Vec::new();
    kill.sort_unstable();
    for (open_start, open_end) in kill {
        let Some(close) = html::find_close(html, open_end, "a") else { continue };
        let no_href = html::remove_attr(&html[open_start..open_end], "href");
        edits.push((open_start, open_end, format!("<span{}", &no_href[2..]))); // 去掉 "<a"
        edits.push((close.start, close.end, "</span>".to_string()));
    }
    edits.sort_by_key(|e| e.0);
    html::apply_edits(html, edits)
}

#[cfg(test)]
mod footnote_tests {
    use super::*;
    /// 真实微信读书脚注（marker/注释同章、href 带旧文件名前缀）应被规整成裸 `#锚点`
    /// 且该章判定为含章内锚点（不切分）——这是 xochitl 唯一会原生跳转的形态。
    #[test]
    fn real_footnote_normalizes() {
        // chap_0005 真实脚注：marker(id=zw1)→text00004.html#zhu1；注释(id=zhu1)→text00004.html#zw1，两者同章。
        let html = r#"<p><a href="text00004.html#zhu1" id="zw1">[1]</a>正文</p><p><a href="text00004.html#zw1" id="zhu1">[1]</a>注释文字</p>"#;
        let out = fix_internal_links(html);
        eprintln!("IN : {html}");
        eprintln!("OUT: {out}");
        assert!(out.contains(r##"href="#zhu1""##), "marker 未规整成裸锚点");
        assert!(out.contains(r##"href="#zw1""##), "注释回链未规整成裸锚点");
    }

    #[test]
    fn kindle_backlink_broken_forward_kept() {
        // Kindle 形态：marker 的 id 在 <small> 上、注释段的回链在独立 <a> 上。
        let html = r##"<p>正文小鼠波波<sup class="c4"><small id="filepos16001"><a href="#filepos21550"><span>[1]</span></a></small></sup>后续</p>
<p id="filepos21550" class="c12"><a href="#filepos16001"><span>[1]</span></a><span>注释文字</span></p>"##;
        let out = break_footnote_cycles(html);
        // 正向 marker→注释 保留
        assert!(out.contains(r##"<a href="#filepos21550"><span>[1]</span></a>"##), "正向脚注链接被误删");
        // 注释里的回链去链化：<a href="#filepos16001"> 不再是链接
        assert!(!out.contains(r##"href="#filepos16001""##), "回链未去链");
        // 注释段 id 保留（正向链接的落点）
        assert!(out.contains(r##"<p id="filepos21550""##), "注释段 id 丢失");
    }

    #[test]
    fn weread_backlink_broken_id_preserved() {
        // weread 形态：id 与回链在同一个 <a> 上，去链后必须保留 id。
        let html = r##"<p>正文<a href="#zhu1" id="zw1">[1]</a>后续</p>
<p><a href="#zw1" id="zhu1">[1]</a>注释文字</p>"##;
        let out = break_footnote_cycles(html);
        assert!(out.contains(r##"<a href="#zhu1" id="zw1">[1]</a>"##), "正向 marker 被误删");
        // 注释锚点去链但保留 id=zhu1（marker 的落点）
        assert!(out.contains(r##"id="zhu1""##), "注释 id 丢失（正向落点会断）");
        assert!(!out.contains(r##"href="#zw1""##), "注释回链未去链");
        assert!(out.contains("<span"), "回链应变成 span");
    }

    #[test]
    fn uppercase_close_and_single_quotes_handled() {
        // 大写 `</A>`、单引号属性：回链照样去链，配对的是自己的闭合标签。
        let html = "<p>正文<a href='#n1' id='r1'>[1]</a>后续</p>\n<p><A href='#r1' id='n1'>[1]</A>注释文字</p>";
        let out = break_footnote_cycles(html);
        assert_eq!(out, "<p>正文<a href='#n1' id='r1'>[1]</a>后续</p>\n<p><span id='n1'>[1]</span>注释文字</p>");
    }

    #[test]
    fn no_cycle_left_untouched() {
        // 单向 TOC 链接（无回指）不应被动
        let html = r##"<p><a href="#c1">章一</a></p><h2 id="c1">章一</h2>"##;
        assert_eq!(break_footnote_cycles(html), html);
    }
}
