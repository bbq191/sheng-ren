//! 把写出的 AZW3（KF8）读回成可读的 EPUB，**只给写出器做回读自检和测试用**，不是输入格式（入库只收 EPUB、CBZ）。
//! clean-room：依 MobileRead 的 MOBI/KF8 文档 + 对样本的黑盒分析，不看 KindleUnpack 的代码。
//!
//! 关键简化（真样本验证）：KF8 的 rawML（PalmDOC 解压全部文本记录后拼接）本身**已是重组好的
//! XHTML 文档序列**（skeleton 与其 fragment 交错、按存储序≈阅读序）。故**跳过最复杂的
//! INDX/CNCX skeleton+fragment 重组**——按 NCX 位置（没有 NCX 时按 `<html>` 边界）切章 + 清洗结构标签 +
//! 重写 kindle:embed 图片 + 把 kindle:pos 内链解析成书内锚点，得到可读 EPUB。HUFF/CDIC 压缩与 DRM 明确拒绝。
//! 引用里的数字（资源序号、片段号、片段内偏移）都是 base32（`palm::base32_decode`）。

use super::palm;
use bookconv::epub::{Chapter, Resource};
use regex::Regex;
use std::collections::{HashMap, HashSet};

/// 第 `i` 章（0 起）在组出来的 EPUB 里的文件名（`bookconv::epub` 组包时的命名：`chap_0001.xhtml` 起）。
fn chapter_filename(i: usize) -> String {
    format!("chap_{:04}.xhtml", i + 1)
}

/// AZW3 字节 → (母版 EPUB 字节, 书名)。
pub fn azw3_to_epub(data: &[u8]) -> Result<(Vec<u8>, String), String> {
    let records = palm::parse_palmdb(data)?;
    if records.len() < 2 {
        return Err("AZW3 记录太少".into());
    }
    let h = palm::parse_header(records[0])?;
    let exth = palm::parse_exth(h.mobi, h.mobi_hlen);

    // 解压文本记录 1..=trecs（剥 trailing bytes）→ rawML。NCX 位置、片段起点都是原始字节偏移，经 `raw.pos` 换算。
    let raw = palm::RawText::decode(palm::decompress_text(&records, &h));
    let rawml = raw.text.as_str();

    // 图片资源：`kindle:embed:XXXX` 是 1 起的资源序号（base32）。扫 rawML 用到的序号，为其建资源 + 序号→路径映射。
    let images = palm::collect_images(&records);
    let mut used: Vec<usize> = embed_re().captures_iter(rawml).filter_map(|c| palm::base32_decode(&c[1])).collect();
    used.sort_unstable();
    used.dedup();
    let mut resources: Vec<Resource> = Vec::new();
    let mut embed_path: HashMap<usize, String> = HashMap::new();
    for n in used {
        let Some(img) = palm::resource_image(&records, &h, &images, n) else { continue };
        let path = format!("images/embed{n}.{}", img.ext);
        embed_path.insert(n, path.clone());
        resources.push(Resource { path, media_type: img.mime.into(), bytes: img.bytes.to_vec() });
    }

    // 内链重映射上下文：fragment 起始表 + aid 位置表，把 kindle:pos 链接转真锚点（脚注/目录跳转可用）。
    // 索引里的位置是片段插回骨架后的坐标，先经 Kf8Map 换成 rawML 存储坐标（索引不全时退回不换算），再换成解码后位置。
    let map = palm::Kf8Map::parse(&records, &h);
    let stored = |a: usize| map.as_ref().map_or(a, |m| m.to_stored(a));
    let frag_starts: Vec<usize> = map
        .as_ref()
        .map_or_else(|| palm::parse_fragment_starts(&records, &h), |m| m.stored_starts())
        .into_iter()
        .map(|p| raw.pos(p).unwrap_or(usize::MAX))
        .collect();
    let link_ctx = if frag_starts.is_empty() { None } else { Some(LinkCtx::build(rawml, frag_starts)) };
    // 切章：有 NCX 真目录则**按 NCX 位置切**（一章一 spine + 一 nav，含层级）；否则按 <html> 块切。
    let ncx: Vec<palm::NcxEntry> = palm::parse_ncx(&records, &h)
        .into_iter()
        .filter_map(|e| Some(palm::NcxEntry { pos: raw.pos(stored(e.pos))?, ..e }))
        .collect();
    let chapters = build_chapters(rawml, &embed_path, &ncx, &exth.title, link_ctx.as_ref());
    if chapters.is_empty() || chapters.iter().all(|c| c.html_body.trim().is_empty()) {
        return Err("AZW3 无可读正文（可能是 KF8 变体/加密）".into());
    }

    let title = if exth.title.trim().is_empty() {
        first_title(rawml)
            .filter(|s| !s.is_empty())
            .or_else(|| Some(palm::palmdb_name(data)))
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "未命名".into())
    } else {
        exth.title.clone()
    };
    // 封面：EXTH 201/202 → 资源；缺省不硬塞（不误挑正文插图当封面）。
    let cover = palm::pick_cover(&records, &h, &images, &exth);
    palm::assemble_book("azw3", title, exth, cover, chapters, resources)
}

/// `kindle:embed:XXXX` 里的资源序号（base32 数字 0-9A-V）。
fn embed_re() -> &'static Regex {
    static RE: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
    RE.get_or_init(|| Regex::new(r#"(?i)kindle:embed:([0-9A-V]+)"#).unwrap())
}

/// `<title>…</title>`（第 1 组是内容）。
fn title_re() -> &'static Regex {
    static RE: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
    RE.get_or_init(|| Regex::new(r#"(?is)<title[^>]*>(.*?)</title>"#).unwrap())
}

fn first_title(rawml: &str) -> Option<String> {
    title_re().captures(rawml).map(|c| c[1].trim().to_string()).filter(|s| !s.is_empty())
}

/// 段内 `<title>` 纯文本（KF8 常=书名，仅无 NCX 时作退化标题）。
fn seg_title(seg: &str) -> String {
    palm::first_match_text(title_re(), seg)
}

/// 去结构壳（可含多个 skeleton 壳）+ 映射 kindle:embed 图 + 去 flow + aid→id 建锚 → 正文 HTML。
/// **不去链** `<a>`：内链（kindle:pos）原样保留，由 `remap_links` 两遍解析成真锚点（脚注/目录跳转可用）。
fn clean(seg: &str, embed_path: &HashMap<usize, String>) -> String {
    static RE_IMG: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
    static RE_FLOW: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
    let re_img = RE_IMG.get_or_init(|| Regex::new(r#"(?is)<img\b[^>]*\bsrc=["']kindle:embed:([0-9A-V]+)[^"']*["'][^>]*>"#).unwrap());
    let re_flow = RE_FLOW.get_or_init(|| Regex::new(r#"(?is)<link\b[^>]*kindle:flow[^>]*>"#).unwrap());
    let s = palm::strip_shell(seg);
    let s = re_img.replace_all(&s, |cap: &regex::Captures| {
        let n = palm::base32_decode(&cap[1]).unwrap_or(0);
        match embed_path.get(&n) {
            Some(p) => format!("<img src=\"{p}\"/>"),
            None => String::new(),
        }
    });
    let s = re_flow.replace_all(&s, "");
    aid_to_id(&s).trim().to_string()
}

/// KF8 元素普遍带 aid（唯一）→ 转成 `id="aid<X>"` 当锚点（内链目标 + 脚注回跳目标）。
/// calibre 做的 AZW3 元素常**同时**带 aid 与既存 `id="filepos…"`/`calibre_pb_…`：同一标签上的既存 id 先删掉，
/// 否则转换后同标签出现两个 id 属性 = 非法 XHTML → reMarkable 严格 XML 解析遇重复属性整章失败（真机《消失的爱人》
/// 只 7 页根因，2026-09-01）。KF8 内链走 kindle:pos→#aid<X>，既存 filepos id 无链接引用，删之安全。
/// 属性按 `bookconv::html` 解析：单双引号都认，`data-aid` 不算 aid、`data-id` 不算 id。
fn aid_to_id(s: &str) -> std::borrow::Cow<'_, str> {
    let mut edits: Vec<(usize, usize, String)> = Vec::new();
    for t in bookconv::html::tags(s).filter(|t| t.is_start()) {
        let tag = &s[t.start..t.end];
        let attrs = bookconv::html::attrs(tag);
        let Some(first_aid) = attrs.iter().position(|a| a.is("aid")) else { continue };
        for (k, a) in attrs.iter().enumerate() {
            if k == first_aid {
                edits.push((t.start + a.start, t.start + a.end, format!("id=\"aid{}\"", a.value.replace('"', "&quot;"))));
            } else if a.is("id") || a.is("aid") {
                edits.push((t.start + tag[..a.start].trim_end().len(), t.start + a.end, String::new()));
            }
        }
    }
    if edits.is_empty() {
        return std::borrow::Cow::Borrowed(s);
    }
    std::borrow::Cow::Owned(bookconv::html::apply_edits(s, edits))
}

/// 内链重映射上下文：fragment 起始偏移表 + rawML 中 aid 位置表（排序），把 `kindle:pos:fid:off` 解析成
/// 目标章文件 + `#aid<X>` 锚点。fid、off 都是 base32（数字 0-9A-V，见 `palm::base32`）；`frag_starts` 是各片段
/// 在（解码后）rawML 里的存储起点，off 按字节加上去（KF8 正文是 UTF-8，解码前后字节一致）。
struct LinkCtx {
    frag_starts: Vec<usize>,
    /// (标签在 rawML 的起点, 清洗后它的 id)，按位置升序。带 aid 的标签 → `aid<X>`（清洗时 aid 转成 id、原有 id 删掉）；
    /// 不带 aid 但有 id 的标签（calibre 等工具做的 AZW3 常只有 `id="filepos…"`）→ 原 id。
    anchors: Vec<(usize, String)>,
}
impl LinkCtx {
    fn build(rawml: &str, frag_starts: Vec<usize>) -> Self {
        // 记标签起点（不是属性的位置）：链接偏移指向目标标签的 `<`，"≤ 目标的最近一个"才是它自己。
        let anchors = bookconv::html::tags(rawml)
            .filter(|t| t.is_start())
            .filter_map(|t| {
                let tag = &rawml[t.start..t.end];
                let id = match bookconv::html::attr_value(tag, "aid").filter(|v| !v.is_empty()) {
                    Some(aid) => format!("aid{aid}"),
                    None => bookconv::html::attr_value(tag, "id").filter(|v| !v.is_empty())?.to_string(),
                };
                Some((t.start, id))
            })
            .collect();
        LinkCtx { frag_starts, anchors }
    }
    /// (fid, off) → (目标 rawML 偏移, 目标锚点 id)。找 ≤ 目标偏移的最近锚点（= 目标元素或包含它的元素）。
    fn resolve(&self, fid: usize, off: usize) -> Option<(usize, String)> {
        let base = *self.frag_starts.get(fid)?;
        let t = base.checked_add(off)?;
        let idx = self.anchors.partition_point(|(p, _)| *p <= t);
        if idx == 0 {
            return None;
        }
        Some((t, self.anchors[idx - 1].1.clone()))
    }
}

/// 两遍第二遍：把各章 HTML 里保留的 `href="kindle:pos:fid:F:off:O"` 重写成 `chap_N.xhtml#锚点`。
/// 只换 href 属性值（保留 `<a>` 其余属性——尤其自身 id，是脚注回跳的目标）。解析不出 → `href="#"`（惰性）。
/// `ch_ranges`=各最终章 rawML [start,end) 定位目标章；`live_ids`=清洗后实际存活的 id 集合——目标 aid
/// 在 shell 元素上（body/html，已被剥）时锚点不存在，退回只跳章文件（对指向 fragment 首的 TOC 链正确）。
fn remap_links(
    html: &str,
    ctx: &LinkCtx,
    ch_ranges: &[(usize, usize)],
    live_ids: &HashSet<String>,
) -> String {
    // 属性按 `bookconv::html` 解析（单双引号都认）。`xlink:href` 也管：SVG 里残留的 kindle: 引用同样是死链。
    bookconv::html::edit_attrs(html, &["href", "xlink:href"], |_, a| {
        let v = a.value;
        if !v.get(..7).is_some_and(|p| p.eq_ignore_ascii_case("kindle:")) {
            return bookconv::html::Edit::Keep;
        }
        let resolved = parse_kindle_pos(v).and_then(|(fid, off)| ctx.resolve(fid, off));
        bookconv::html::Edit::Set(match resolved {
            Some((t, id)) => {
                let ci = ch_ranges.partition_point(|(a, _)| *a <= t).saturating_sub(1);
                let file = chapter_filename(ci);
                if live_ids.contains(&id) {
                    format!("{file}#{id}")
                } else {
                    file // 锚在被剥的壳上 → 跳章首
                }
            }
            // 解析不出的 pos、其余 kindle: 内链 → 惰性 #，避免残留 kindle: 死链
            None => "#".to_string(),
        })
    })
    .into_owned()
}

/// `kindle:pos:fid:XXXX:off:YYYYYYYYYY`（大小写不敏感，数字是 base32）→ (fid, off)；不是这个形状返回 `None`。
fn parse_kindle_pos(v: &str) -> Option<(usize, usize)> {
    const PREFIX: &str = "kindle:pos:fid:";
    if !v.get(..PREFIX.len())?.eq_ignore_ascii_case(PREFIX) {
        return None;
    }
    let rest = &v[PREFIX.len()..];
    let at = rest.to_ascii_lowercase().find(":off:")?;
    Some((palm::base32_decode(&rest[..at])?, palm::base32_decode(&rest[at + 5..])?))
}

/// 把 rawML 切成章：
/// - **有 NCX 真目录**：按 NCX 位置切（吸附到 pos 前最近的 `<` 避免切进标签中间），一章=一 spine+一 nav，
///   标题=真章名、层级=NCX 层级+1；首条 NCX 前的前置内容（封面/版权页）作无标题段（进 spine 不进 nav）。
///   **能把塞进同一 skeleton 的多章拆开**（如《恶意》24 章仅 11 个 skeleton 块）。
/// - **无 NCX**：退化按 `<html>` 块切；标题取块内 `<title>`，等于书名的清空（避免满屏书名重复）。
fn build_chapters(
    rawml: &str,
    embed_path: &HashMap<usize, String>,
    ncx: &[palm::NcxEntry],
    book_title: &str,
    link_ctx: Option<&LinkCtx>,
) -> Vec<Chapter> {
    // 第一遍：切段 + 清洗（含 aid→id、保留 kindle:pos 链），记录每章的 rawML [start,end)。
    // 元组 = (Chapter, rawML 段起点)；段终点由下一章起点/rawml 末尾给出。
    let mut built: Vec<(Chapter, usize)> = Vec::new();

    if !ncx.is_empty() {
        // 按 NCX **精确位置**切（各章位置互异，不往前后吸附以免邻近章被去重合并丢章）。唯一的例外：切点落在
        // 某个标签中间时吸附回该标签的 `<`（`snap_out_of_tag`），不把一个标签劈成两半。
        let mut cuts: Vec<(usize, String, i64)> = ncx
            .iter()
            .filter(|e| e.pos <= rawml.len())
            .map(|e| {
                // NCX 偏移是字节位置，可能落在多字节字符中间 → 下取到字符边界（最多回退 3 字节）
                (snap_out_of_tag(rawml, palm::char_floor(rawml, e.pos)), e.label.chars().take(80).collect::<String>(), (e.level as i64 + 1).clamp(1, 6))
            })
            .collect();
        cuts.sort_by_key(|c| c.0);
        cuts.dedup_by_key(|c| c.0); // 仅去完全相同位置
        if cuts.is_empty() {
            // NCX 位置全都越过正文末尾（索引坏了）：不按它切，退回按 <html> 块切。
            return build_chapters(rawml, embed_path, &[], book_title, link_ctx);
        }

        // 首条 NCX 前的前置内容（封面/版权页）→ 无标题段（进 spine 不进 nav），rawML 起点 0
        let first_cut = cuts[0].0;
        if first_cut > 0 {
            let html = clean(&rawml[..first_cut], embed_path);
            if !html.is_empty() {
                built.push((Chapter { title: String::new(), html_body: html, level: 1 }, 0));
            }
        }
        for i in 0..cuts.len() {
            let start = cuts[i].0;
            let end = if i + 1 < cuts.len() { cuts[i + 1].0 } else { rawml.len() };
            let html = clean(&rawml[start..end], embed_path);
            if html.is_empty() {
                continue;
            }
            built.push((Chapter { title: cuts[i].1.clone(), html_body: html, level: cuts[i].2 }, start));
        }
    } else {
        // 无 NCX：按 <html> 块切
        let lower = rawml.to_ascii_lowercase();
        let mut starts: Vec<usize> = Vec::new();
        let mut from = 0;
        while let Some(rel) = lower[from..].find("<html") {
            let pos = from + rel;
            starts.push(pos);
            from = pos + 5;
        }
        if starts.is_empty() {
            starts.push(0);
        }
        for i in 0..starts.len() {
            let end = if i + 1 < starts.len() { starts[i + 1] } else { rawml.len() };
            let seg = &rawml[starts[i]..end];
            let mut title = seg_title(seg);
            if title == book_title {
                title.clear(); // 等于书名 → 清空，避免满屏重复
            }
            let html = clean(seg, embed_path);
            if html.is_empty() {
                continue;
            }
            built.push((Chapter { title, html_body: html, level: 1 }, starts[i]));
        }
    }

    // 第二遍：内链重映射（有 fragment/aid 上下文时）。ch_ranges = 各最终章 rawML [start, end)。
    if let Some(ctx) = link_ctx {
        let n = built.len();
        let ch_ranges: Vec<(usize, usize)> = (0..n)
            .map(|i| (built[i].1, if i + 1 < n { built[i + 1].1 } else { rawml.len() }))
            .collect();
        // 收集清洗后实际存活的 id（shell 上的 aid、带 aid 标签原有的 id 已被剥，不在此集）→ 决定锚点 vs 跳章首。
        let mut live_ids: HashSet<String> = HashSet::new();
        for (ch, _) in &built {
            for t in bookconv::html::tags(&ch.html_body).filter(|t| t.is_start()) {
                if let Some(id) = bookconv::html::attr_value(&ch.html_body[t.start..t.end], "id").filter(|v| !v.is_empty()) {
                    live_ids.insert(id.to_string());
                }
            }
        }
        for (ch, _) in built.iter_mut() {
            ch.html_body = remap_links(&ch.html_body, ctx, &ch_ranges, &live_ids);
        }
    }
    built.into_iter().map(|(c, _)| c).collect()
}

/// 切点 `pos` 落在某个标签里面（它前面最近的 `<` 起的那个标签到 `pos` 还没结束）时吸附回那个 `<`，否则原样。
/// 此前在切出来的段里"裁到第一个 `>`"：前一章尾部留下半截标签、这一章丢掉那个标签（连同它的 aid 锚点），
/// 切点落在含 `>` 的正文文字里时还会把这段文字当残片裁掉。标签边界按 `bookconv::html` 扫（属性值里的 `>` 不算结束）。
fn snap_out_of_tag(rawml: &str, pos: usize) -> usize {
    let Some(lt) = rawml[..pos].rfind('<') else { return pos };
    match bookconv::html::tags_in(rawml, lt, rawml.len()).next() {
        Some(t) if t.start == lt && t.end > pos => lt,
        _ => pos,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cleaner_strips_shell_and_maps_embed() {
        let mut ep = HashMap::new();
        ep.insert(2usize, "images/embed2.jpg".to_string());
        let seg = r#"<?xml version="1.0"?><html xmlns="x"><head><title>第一章</title><link href="kindle:flow:0001"/></head><body aid="0"><p>正文</p></body></html><img src="kindle:embed:0002?mime=image/jpeg"/>"#;
        assert_eq!(seg_title(seg), "第一章");
        let b = clean(seg, &ep);
        assert!(b.contains("<p>正文</p>"), "{b}");
        assert!(b.contains("<img src=\"images/embed2.jpg\"/>"), "{b}");
        assert!(!b.contains("kindle:") && !b.contains("<head") && !b.contains("<body") && !b.contains("<html"), "{b}");
    }

    #[test]
    fn build_chapters_ncx_splits_within_one_skeleton() {
        // 单个 <html> 块里塞两章，NCX 两条 → 按位置切成两章、各拿真名+层级（验《恶意》式多章一块）
        let ep = HashMap::new();
        let rawml = r#"<html><body><p>甲章正文</p><p>乙章正文</p></body></html>"#;
        let p_yi = rawml.find("<p>乙章正文").unwrap();
        let ncx = vec![
            palm::NcxEntry { pos: rawml.find("<p>甲章正文").unwrap(), label: "甲章".into(), level: 0 },
            palm::NcxEntry { pos: p_yi, label: "乙章".into(), level: 1 },
        ];
        let chs = build_chapters(rawml, &ep, &ncx, "书名", None);
        let titled: Vec<_> = chs.iter().filter(|c| !c.title.is_empty()).collect();
        assert_eq!(titled.len(), 2, "两章都在: {:?}", chs.iter().map(|c| &c.title).collect::<Vec<_>>());
        assert_eq!(titled[0].title, "甲章");
        assert_eq!(titled[0].level, 1);
        assert_eq!(titled[1].title, "乙章");
        assert_eq!(titled[1].level, 2);
        assert!(titled[0].html_body.contains("甲章正文") && !titled[0].html_body.contains("乙章正文"));
        assert!(titled[1].html_body.contains("乙章正文"));
    }

    #[test]
    fn build_chapters_falls_back_when_all_ncx_positions_past_end() {
        let rawml = r#"<html><body><p>a</p></body></html>"#;
        let ncx = vec![palm::NcxEntry { pos: rawml.len() + 100, label: "坏".into(), level: 0 }];
        let chs = build_chapters(rawml, &HashMap::new(), &ncx, "书名", None);
        assert_eq!(chs.len(), 1);
        assert!(chs[0].html_body.contains("<p>a</p>"));
    }

    #[test]
    fn build_chapters_no_ncx_dedups_booktitle() {
        // 无 NCX → 按 <html> 块切，块 <title>=书名的清空、非书名保留
        let ep = HashMap::new();
        let rawml = r#"<html><head><title>书名</title></head><body><p>a</p></body></html><html><head><title>序言</title></head><body><p>b</p></body></html>"#;
        let chs = build_chapters(rawml, &ep, &[], "书名", None);
        assert_eq!(chs.len(), 2);
        assert_eq!(chs[0].title, "");
        assert_eq!(chs[1].title, "序言");
    }

    #[test]
    fn snap_out_of_tag_moves_cut_back_to_tag_start_only_inside_tags() {
        let raw = r#"<p>甲 &gt; 乙 > 丙</p><p class="x" title="a>b" aid="Q2">丁</p>"#;
        let tag2 = raw.find("<p class").unwrap();
        assert_eq!(snap_out_of_tag(raw, raw.find("class").unwrap()), tag2, "切进标签属性里 → 吸附回 <");
        assert_eq!(snap_out_of_tag(raw, raw.find("b\"").unwrap()), tag2, "属性值里的 > 不算标签结束");
        assert_eq!(snap_out_of_tag(raw, tag2), tag2, "正好在 < 上不动");
        let in_text = raw.find(" 丙").unwrap();
        assert_eq!(snap_out_of_tag(raw, in_text), in_text, "正文文字里（前面有 > 字符）不动");
        assert_eq!(snap_out_of_tag("纯文本无标签", 3), 3);
    }

    #[test]
    fn ncx_cut_inside_tag_keeps_whole_tag_and_text_with_gt() {
        // NCX 位置落在第二个 <p> 的属性中间：此前第一章尾部留下半截 `<p class="x" `、第二章裁掉属性残片连同 aid 锚点
        let raw = r#"<html><body><p>a > b</p><p class="x" aid="Q2">乙章</p></body></html>"#;
        let ncx = vec![
            palm::NcxEntry { pos: raw.find("<p>a").unwrap(), label: "甲".into(), level: 0 },
            palm::NcxEntry { pos: raw.find("aid=").unwrap(), label: "乙".into(), level: 0 },
        ];
        let chs = build_chapters(raw, &HashMap::new(), &ncx, "书名", None);
        let titled: Vec<_> = chs.iter().filter(|c| !c.title.is_empty()).collect();
        assert_eq!(titled[0].html_body, "<p>a > b</p>", "正文里的 > 不丢、没有半截标签");
        assert_eq!(titled[1].html_body, r#"<p class="x" id="aidQ2">乙章</p>"#);
    }

    #[test]
    fn aid_and_ids_recognized_with_single_quotes() {
        let seg = r#"<p aid='A1' id='old' data-aid="no">x</p><p data-id="k">y</p>"#;
        assert_eq!(clean(seg, &HashMap::new()), r#"<p id="aidA1" data-aid="no">x</p><p data-id="k">y</p>"#);
        let raw = r#"<p data-id="d">x</p><p id='s'>y</p>"#;
        let ctx = LinkCtx::build(raw, vec![0]);
        assert_eq!(ctx.anchors, vec![(raw.find("<p id").unwrap(), "s".to_string())], "data-id 不是锚点，单引号 id 是");
        let out = remap_links(r#"<a href='kindle:pos:fid:0000:off:0000000014'>x</a><image xlink:href="kindle:embed:0001"/>"#, &ctx, &[(0, 100)], &HashSet::new());
        assert_eq!(out, r##"<a href='chap_0001.xhtml'>x</a><image xlink:href="#"/>"##);
    }

    #[test]
    fn cleaner_converts_aid_to_id_and_keeps_links() {
        let ep = HashMap::new();
        let seg = r#"<html><body><p aid="X1">注<a href="kindle:pos:fid:0002:off:0000000005" aid="X2">[1]</a></p></body></html>"#;
        let b = clean(seg, &ep);
        assert!(b.contains(r#"id="aidX1""#), "aid→id: {b}");
        assert!(b.contains(r#"id="aidX2""#), "链接自身 aid→id(回跳锚): {b}");
        assert!(b.contains("kindle:pos:fid:0002"), "clean 阶段保留内链待重映射: {b}");
    }

    #[test]
    fn cleaner_no_dup_id_when_element_has_existing_id() {
        // calibre 做的 AZW3：元素同时带 aid 与既存 id="filepos..."（真机《消失的爱人》崩因）
        let ep = HashMap::new();
        let seg = r#"<html><body><p aid="5N3C1" class="calibre6" id="filepos18251">正文</p><div id="calibre_pb_6" class="mbppagebreak" aid="5N3C4"></div></body></html>"#;
        let b = clean(seg, &ep);
        // 每个标签只能有一个 id 属性（否则非法 XHTML → reMarkable 整章渲染失败）
        for tag in b.split('>') {
            let cnt = tag.matches(" id=").count() + if tag.starts_with("id=") { 1 } else { 0 };
            assert!(cnt <= 1, "标签出现重复 id 属性: <{tag}> in {b}");
        }
        // 锚点保留为 aid 派生的 id（内链目标），既存 filepos/calibre_pb id 被删
        assert!(b.contains(r#"id="aid5N3C1""#), "aid 锚点应保留: {b}");
        assert!(b.contains(r#"id="aid5N3C4""#), "aid 锚点应保留: {b}");
        assert!(!b.contains("filepos18251"), "既存 filepos id 应删除: {b}");
    }

    #[test]
    fn link_anchors_use_tag_start_and_fall_back_to_plain_id() {
        let raw = r#"<p id="filepos9">甲</p><p class="c" aid="Q" id="old">乙</p><span>丙</span>"#;
        let ctx = LinkCtx::build(raw, vec![0]);
        let q = raw.find("<p class").unwrap();
        assert_eq!(ctx.anchors, vec![(0, "filepos9".to_string()), (q, "aidQ".to_string())]);
        // 偏移正好落在第二个 <p> 的 `<` 上 → 就是它，不是前一个元素
        assert_eq!(ctx.resolve(0, q).unwrap().1, "aidQ");
    }

    #[test]
    fn remap_links_resolves_kindle_pos_to_anchor() {
        // fragment 2 起于 rawML 100；aid "A2" 在位置 100 → kindle:pos:fid:2:off:0 → chap#? #aidA2
        let ctx = LinkCtx {
            frag_starts: vec![0, 50, 100],
            anchors: vec![(0, "aidA0".into()), (50, "aidA1".into()), (100, "aidA2".into())],
        };
        // 两章：[0,80) 与 [80,200)。目标 rawoff=100 落第二章(idx1)。
        let ranges = vec![(0usize, 80usize), (80usize, 200usize)];
        let live: HashSet<String> = ["aidA2".to_string()].into_iter().collect();
        let html = r#"<a href="kindle:pos:fid:0002:off:0000000000">跳</a>"#;
        let out = remap_links(html, &ctx, &ranges, &live);
        assert!(out.contains(r#"href="chap_0002.xhtml#aidA2""#), "{out}");
        // 目标 aid 不在存活集（shell 上被剥）→ 退回只跳章文件
        let no_anchor = remap_links(html, &ctx, &ranges, &HashSet::new());
        assert!(no_anchor.contains(r#"href="chap_0002.xhtml""#) && !no_anchor.contains("#aid"), "{no_anchor}");
        // 解析不出（fid 越界）→ 惰性 #
        let bad = remap_links(r#"<a href="kindle:pos:fid:00FF:off:0000000000">x</a>"#, &ctx, &ranges, &live);
        assert!(bad.contains(r##"href="#""##), "{bad}");
    }
}
