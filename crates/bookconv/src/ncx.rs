//! EPUB 的 NCX 目录解析。

/// 线性扫 `toc.ncx`，展平成 `(depth, title, target)` 序列（title、target 的字符引用都已还原，见 [`NavPoint`]）（不建真正的树——跟本 crate 一贯的
/// "扁平+depth"写法一致，如 `wash::toc::dense_ranks`）。NCX 规范保证 `<navLabel>` 和 `<content>`
/// 总是先于自己的子 `<navPoint>` 出现，扫描时按"刚看到 content 就用当前 depth/title 落地一条"
/// 处理即可，不用等子节点扫完。
///
/// 按标签扫（`crate::html::tags`），不要求 `<content src>` 紧跟 `navLabel`、也不要求 `src` 是第一个属性
/// （`<content id="x" src='a.html'/>` 也认）；有 `<navMap>` 时只收 navMap 里的条目（`<pageList>` 的页码不算目录）。
pub fn parse_ncx_flat(ncx_text: &str) -> Vec<(usize, String, String)> {
    parse_nav_points(ncx_text).into_iter().map(|p| (p.depth, p.label, p.src)).collect()
}

/// 展平的一条 NCX 目录（见 [`parse_ncx_flat`]），另带所在 `<navPoint …>` 开标签原文（重写目录时保留它的 `id`、`class` 等属性）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NavPoint {
    pub depth: usize,
    /// 标签文字（字符引用已还原）。
    pub label: String,
    /// `<content src>` 的值，**字符引用已还原**（`a&amp;b.xhtml` → `a&b.xhtml`；百分号编码照留，拆路径、锚点时再解码）。
    /// 写回 XML 时调用方要再转义（`replace_nav_map` 会转义）。2026-09-30 审计：此前给原文，调用方拿去解析路径、再经
    /// `replace_nav_map` 转义一次，`a&amp;b.xhtml` 变成 `a&amp;amp;b.xhtml`。
    pub src: String,
    /// 所在 `<navPoint …>` 开标签原文；`content` 不在任何 navPoint 里时为空。
    pub open_tag: String,
}

/// 同 [`parse_ncx_flat`]，每条带 navPoint 开标签原文。
pub fn parse_nav_points(ncx_text: &str) -> Vec<NavPoint> {
    use crate::html::{self, TagKind};
    let has_navmap = html::tags(ncx_text).any(|t| t.kind == TagKind::Open && t.is("navMap"));
    let mut in_map = !has_navmap;
    let (mut depth, mut in_label) = (0usize, false);
    let mut text_start: Option<usize> = None;
    let mut cur_title = String::new();
    let mut opens: Vec<&str> = Vec::new();
    let mut out = Vec::new();
    for t in html::tags(ncx_text) {
        if t.is("navMap") {
            in_map = t.kind == TagKind::Open || (!has_navmap && in_map);
            continue;
        }
        if !in_map {
            continue;
        }
        match t.kind {
            TagKind::Open if t.is("navPoint") => {
                depth += 1;
                opens.push(&ncx_text[t.start..t.end]);
            }
            TagKind::Close if t.is("navPoint") => {
                depth = depth.saturating_sub(1);
                opens.pop();
            }
            TagKind::Open if t.is("navLabel") => in_label = true,
            TagKind::Close if t.is("navLabel") => in_label = false,
            TagKind::Open if t.is("text") && in_label => text_start = Some(t.end),
            TagKind::Close if t.is("text") => {
                if let Some(s) = text_start.take() {
                    // NCX 里是转义过的 XML 文本；标题之后会当书签/目录项再转义一次，先还原字符引用，
                    // 否则 `卷一 &amp; 卷二` 会以字面 `&amp;` 出现在书签里。
                    cur_title = crate::util::xml_unescape(ncx_text[s..t.start].trim()).into_owned();
                }
            }
            TagKind::Open | TagKind::SelfClosing if t.is("content") => {
                if let Some(src) = html::attr_value(&ncx_text[t.start..t.end], "src") {
                    let open_tag = opens.last().map_or(String::new(), |s| s.to_string());
                    out.push(NavPoint { depth, label: std::mem::take(&mut cur_title), src: crate::util::xml_unescape(src).into_owned(), open_tag });
                }
            }
            _ => {}
        }
    }
    out
}

/// 逐个 `<content src>`（navMap、pageList 里的都算）调 `f(最近一个 <text> 的文字（字符引用已还原）, src 原文)`，返回新值
/// （属性值原文，调用方已转义）就就地换掉。注意 src 给的是**原文**（和 [`NavPoint::src`] 不同）：拿它解析路径前先
/// `xml_unescape`，原样拼进新值时不要再转义。一处都没改 → `None`。定章节后目录改指到补的 id、修复指错位置的目录共用。
pub fn rewrite_content_srcs(ncx: &str, mut f: impl FnMut(&str, &str) -> Option<String>) -> Option<String> {
    use crate::html::{self, TagKind};
    let mut edits: Vec<(usize, usize, String)> = Vec::new();
    let mut label = String::new();
    let mut label_start: Option<usize> = None;
    for t in html::tags(ncx) {
        if t.is("text") && t.kind == TagKind::Open {
            label_start = Some(t.end);
        } else if t.is("text") && t.kind == TagKind::Close {
            if let Some(s) = label_start.take() {
                label = crate::util::xml_unescape(&ncx[s..t.start]).into_owned();
            }
        } else if t.is("content") && t.is_start() {
            let Some(a) = html::attr(&ncx[t.start..t.end], "src") else { continue };
            if let Some(v) = f(&label, a.value) {
                edits.push((t.start + a.value_start, t.start + a.value_end, v));
            }
        }
    }
    (!edits.is_empty()).then(|| html::apply_edits(ncx, edits))
}

/// 重写 navMap 用的一条：`depth` 从 1 起；`src` 是属性值原文（未转义）；`open_tag` 是原来的 `<navPoint …>`（`None` = 新加的）。
pub struct NewNavPoint<'a> {
    pub depth: u8,
    pub label: &'a str,
    pub src: &'a str,
    pub open_tag: Option<&'a str>,
}

/// 只换 `<navMap>` 里的 navPoint 树，NCX 其余部分（`<head>`、`docTitle`、`pageList`、`navList`、navMap 自己的 `navInfo`）原样保留；
/// 原有条目沿用自己的 `<navPoint>` 开标签（`id`、`class` 不变），新加的条目给一个 NCX 里没出现过的 `eink-np-N`。
/// `playOrder` 按新树的顺序从 1 重排，`dtb:depth` 改成新树的深度。层级每次最多深入一层（保证良构）。没有成对的 `<navMap>` → `None`。
pub fn replace_nav_map(ncx: &str, points: &[NewNavPoint]) -> Option<String> {
    use crate::html::{self, TagKind};
    use crate::util::xml_escape;
    let open = html::tags(ncx).find(|t| t.kind == TagKind::Open && t.is("navMap"))?;
    let close = html::tags_in(ncx, open.end, ncx.len()).filter(|t| t.kind == TagKind::Close && t.is("navMap")).last()?;
    // navMap 里第一个 navPoint 之前的东西（navInfo/navLabel）保留
    let first_np = html::tags_in(ncx, open.end, close.start).find(|t| t.is_start() && t.is("navPoint")).map_or(close.start, |t| t.start);
    let mut s = String::new();
    let (mut depth, mut max_depth, mut fresh) = (0u8, 0u8, 0usize);
    for (i, p) in points.iter().enumerate() {
        // 层数到 u8 上限就不再深入（不可信的 NCX 嵌套 255 层以上时 `depth + 1` 溢出）
        let d = p.depth.max(1).min(depth.saturating_add(1));
        if d <= depth {
            for _ in 0..(depth - d + 1) {
                s.push_str("</navPoint>");
            }
        }
        let order = (i + 1).to_string();
        let mut fresh_id = || loop {
            fresh += 1;
            let id = format!("eink-np-{fresh}");
            if !ncx.contains(id.as_str()) {
                break id;
            }
        };
        let tag = match p.open_tag.filter(|t| !t.is_empty()) {
            // 原来没有 id 的（NCX 规范要求有）补一个
            Some(t) if html::attr_value(t, "id").is_some_and(|v| !v.is_empty()) => html::set_attr(t, "playOrder", &order),
            Some(t) => html::set_attr(&html::set_attr(t, "id", &fresh_id()), "playOrder", &order),
            None => format!(r#"<navPoint id="{}" playOrder="{order}">"#, fresh_id()),
        };
        s.push_str(&tag);
        s.push_str(&format!(r#"<navLabel><text>{}</text></navLabel><content src="{}"/>"#, xml_escape(p.label), xml_escape(p.src)));
        depth = d;
        max_depth = max_depth.max(d);
    }
    for _ in 0..depth {
        s.push_str("</navPoint>");
    }
    let mut out = format!("{}{}{}", &ncx[..first_np], s, &ncx[close.start..]);
    // dtb:depth 跟着新树（没有就补在 `</head>` 前）
    let depth = max_depth.max(1).to_string();
    if let Some(m) = html::tags(&out).find(|t| t.is_start() && t.is("meta") && html::attr_value(&out[t.start..t.end], "name") == Some("dtb:depth")) {
        let tag = html::set_attr(&out[m.start..m.end], "content", &depth);
        out.replace_range(m.start..m.end, &tag);
    } else if let Some(h) = html::tags(&out).find(|t| t.kind == TagKind::Close && t.is("head")) {
        out.insert_str(h.start, &format!(r#"<meta name="dtb:depth" content="{depth}"/>"#));
    }
    Some(out)
}

// ───────────────────────── 兜底书签 ─────────────────────────

/// 页码分段书签：没有可用目录的漫画（PDF 裁白边产物、清洗层给漫画补的目录）按页分段，至少能按段跳转。
/// 没有源目录时的兜底书签：每 [`FALLBACK_TOC_PAGES`] 页一条，标题"第 N–M 页"（页码 1 起），如实标注不是章节。
pub const FALLBACK_TOC_PAGES: usize = 20;
/// 每 [`FALLBACK_TOC_PAGES`]（20）页一条书签：`(起始页下标（0 起）, "第 N–M 页")`。
pub fn page_chunk_titles(total_pages: usize) -> Vec<(usize, String)> {
    (0..total_pages)
        .step_by(FALLBACK_TOC_PAGES)
        .map(|start| (start, format!("第 {}–{} 页", start + 1, (start + FALLBACK_TOC_PAGES).min(total_pages))))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn page_chunk_titles_boundaries() {
        assert_eq!(page_chunk_titles(1), vec![(0, "第 1–1 页".to_string())]);
        assert_eq!(page_chunk_titles(20).len(), 1);
        assert_eq!(page_chunk_titles(21).last(), Some(&(20, "第 21–21 页".to_string())));
    }

    #[test]
    fn replace_nav_map_deeper_than_u8_stays_well_formed() {
        let ncx = "<ncx><head></head><navMap></navMap></ncx>";
        let points: Vec<NewNavPoint> = (0..300).map(|_| NewNavPoint { depth: 255, label: "x", src: "a.html", open_tag: None }).collect();
        let out = replace_nav_map(ncx, &points).unwrap();
        assert_eq!(out.matches("<navPoint ").count(), 300);
        assert_eq!(out.matches("</navPoint>").count(), 300);
        assert!(out.contains(r#"<meta name="dtb:depth" content="255"/>"#));
    }

    #[test]
    fn ncx_flat_depth_and_titles_decoded_once() {
        let ncx = r#"<navMap><navPoint><navLabel><text>猫 &amp; 鼠</text></navLabel><content src="t/a.html"/>
<navPoint><navLabel><text>卷&#20108;</text></navLabel><content src="t/b.html#x"/></navPoint></navPoint></navMap>"#;
        assert_eq!(parse_ncx_flat(ncx), [(1, "猫 & 鼠".to_string(), "t/a.html".to_string()), (2, "卷二".to_string(), "t/b.html#x".to_string())]);
        // content 带别的属性、单引号；navLabel 与 content 之间隔着别的元素；pageList 不算目录。
        let ncx2 = r#"<ncx><navMap><navPoint id="n1"><navLabel id="l"><text>甲</text></navLabel><!-- x --><content id="c1" src='a.html#%E6%B3%A8'/></navPoint></navMap>
<pageList><pageTarget><navLabel><text>1</text></navLabel><content src="a.html#p1"/></pageTarget></pageList></ncx>"#;
        assert_eq!(parse_ncx_flat(ncx2), [(1, "甲".to_string(), "a.html#%E6%B3%A8".to_string())]);
        // src 的字符引用还原一次（百分号编码照留）
        let ncx3 = r#"<navMap><navPoint><navLabel><text>丙</text></navLabel><content src="a&amp;b%20c.html#x&amp;y"/></navPoint></navMap>"#;
        assert_eq!(parse_nav_points(ncx3)[0].src, "a&b%20c.html#x&y");
    }
}
