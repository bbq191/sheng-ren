//! 目录：目录文件判定、自动生成目录（ncx+nav）、已有扁平目录按"第X部"重建为两级、章节分页后补节。
//!
//! 重建已有目录时只换 nav 文档里的 `<nav epub:type="toc">`：landmarks、page-list 等其它 `<nav>` 和 head 原样保留，
//! 原 toc 的标题（`<h1>目录</h1>`/`<h2>Contents</h2>`）沿用；新建时标题按书的语言（中文"目录"、其它"Contents"）。
use super::*;

pub fn is_toc_file(name: &str) -> bool {
    let l = name.to_ascii_lowercase();
    let base = l.rsplit('/').next().unwrap_or(&l);
    l.ends_with(".ncx") || (base.starts_with("nav") && (base.ends_with(".xhtml") || base.ends_with(".html")))
}

/// `src="路径#锚点"`/`href=…`（只认双引号；三段捕获：属性名 / 路径 / `#锚点`）。质量门 `check` 用；清洗层内部走 `html::link_values`。
pub fn href_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r##"(src|href)="([^"#]+)(#[^"]*)?""##).unwrap())
}

/// 一条目录：级别（h 级别或目录深度）、标题（纯文本）、目标文件的 zip 路径、锚点（原文，空 = 指文件本身）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct TocItem {
    pub level: u8,
    pub title: String,
    pub path: String,
    pub frag: String,
}

impl TocItem {
    pub(super) fn new(level: u8, title: impl Into<String>, path: impl Into<String>, frag: impl Into<String>) -> Self {
        TocItem { level, title: title.into(), path: path.into(), frag: frag.into() }
    }
}

/// 新建目录时的标题：中文书"目录"，其它"Contents"。
pub(super) fn toc_title(lang: LangMode) -> &'static str {
    if lang == LangMode::Latin { "Contents" } else { "目录" }
}

// ───────────────────────── 6. 自动目录 ─────────────────────────

/// 目录条目数（ncx `src` + nav `href`，排除 toc 文件自指与非 html 目标）。
pub fn toc_entry_count(entries: &[Entry]) -> usize {
    let mut n = 0;
    for e in entries.iter().filter(|e| is_toc_file(&e.name)) {
        let t = String::from_utf8_lossy(&e.data);
        n += html::link_values(&t)
            .into_iter()
            .filter(|v| {
                let l = html::split_href(v).0.to_ascii_lowercase();
                !l.starts_with("http") && (l.ends_with(".xhtml") || l.ends_with(".html") || l.ends_with(".htm"))
            })
            .count();
    }
    n
}

/// 把出现过的 h 级别稠密化为连续深度 1..N（如书用 {h1,h3} → 各条 rank 1/2），供嵌套用。
pub(super) fn dense_ranks(items: &[TocItem]) -> Vec<u8> {
    let mut levels: Vec<u8> = items.iter().map(|i| i.level).collect();
    levels.sort_unstable();
    levels.dedup();
    items.iter().map(|i| (levels.iter().position(|&l| l == i.level).unwrap_or(0) as u8) + 1).collect()
}

/// 从 spine 各章 h1–h6 生成目录条目；标题没有 id 就补 `id="eink-toc-N"`（已有 `id`——单引号也算——沿用，不追加第二个）。
/// 与分页的标题识别（`paginate::collect_headings`）不同：这里只要 `<hN>` 有文字就收，不判角色。
pub(super) fn collect_toc_headings(entries: &mut [Entry], spine: &[String], nav_doc: Option<&String>) -> Vec<TocItem> {
    let mut out = Vec::new();
    let mut counter = 0usize;
    for p in spine {
        if Some(p) == nav_doc {
            continue;
        }
        let Some(e) = entries.iter_mut().find(|e| &e.name == p) else { continue };
        let html = String::from_utf8_lossy(&e.data).into_owned();
        let mut edits: Vec<(usize, usize, String)> = Vec::new();
        let mut it = html::tags(&html);
        while let Some(t) = it.next() {
            let Some(level) = t.heading_level().filter(|_| t.kind == html::TagKind::Open) else { continue };
            // 到下一个 h 闭合标签（任何级别，与旧正则 `<hN>.*?</h[1-6]>` 一致）
            let Some(close) = it.by_ref().find(|u| u.kind == html::TagKind::Close && u.heading_level().is_some()) else { break };
            let title = plain_text(&html[t.end..close.start]);
            if title.is_empty() {
                continue;
            }
            let open = &html[t.start..t.end];
            let frag = match html::attr_value(open, "id").filter(|v| !v.is_empty()) {
                Some(id) => id.to_string(),
                None => {
                    counter += 1;
                    let f = format!("eink-toc-{counter}");
                    edits.push((t.start, t.end, html::set_attr(open, "id", &f)));
                    f
                }
            };
            out.push(TocItem::new(level, title, p.clone(), frag));
        }
        if !edits.is_empty() {
            e.data = html::apply_edits(&html, edits).into_bytes();
        }
    }
    out
}

pub(super) fn build_ncx(items: &[TocItem], ncx_dir: &str, title: &str, uid: &str) -> String {
    let ranks = dense_ranks(items);
    let depth_max = ranks.iter().copied().max().unwrap_or(1);
    let mut s = format!(r#"<?xml version="1.0" encoding="UTF-8"?>
<ncx xmlns="http://www.daisy.org/z3986/2005/ncx/" version="2005-1"><head><meta name="dtb:uid" content="{}"/><meta name="dtb:depth" content="{depth_max}"/></head><docTitle><text>{}</text></docTitle><navMap>"#, xml_escape(uid), xml_escape(title));
    let mut depth = 0u8; // 当前打开的 navPoint 层数
    for (i, it) in items.iter().enumerate() {
        let d = ranks[i].min(depth + 1); // 钳制：不跳跃深入 >1 层，保证良构
        if d <= depth {
            for _ in 0..(depth - d + 1) {
                s.push_str("</navPoint>");
            }
        }
        let href = toc_href(&relative_to(ncx_dir, &it.path), &it.frag);
        s.push_str(&format!(r#"<navPoint id="np{}" playOrder="{}"><navLabel><text>{}</text></navLabel><content src="{}"/>"#, i + 1, i + 1, xml_escape(&it.title), xml_escape(&href)));
        depth = d;
    }
    for _ in 0..depth {
        s.push_str("</navPoint>");
    }
    s.push_str("</navMap></ncx>");
    s
}

/// nav 里目录的 `<ol>` 树。
fn nav_ol(items: &[TocItem], nav_dir: &str) -> String {
    let ranks = dense_ranks(items);
    let mut s = String::new();
    let mut depth = 0u8; // 当前打开的 <ol> 层数
    for (i, it) in items.iter().enumerate() {
        let d = ranks[i].min(depth + 1);
        if d > depth {
            for _ in depth..d {
                s.push_str("<ol>");
            }
        } else {
            for _ in d..depth {
                s.push_str("</li></ol>");
            }
            s.push_str("</li>");
        }
        depth = d;
        let href = toc_href(&relative_to(nav_dir, &it.path), &it.frag);
        s.push_str(&format!(r#"<li><a href="{}">{}</a>"#, xml_escape(&href), xml_escape(&it.title)));
    }
    for _ in 0..depth {
        s.push_str("</li></ol>");
    }
    s
}

/// 新建一份 nav 文档（`heading` = 目录标题）。
pub(super) fn build_nav(items: &[TocItem], nav_dir: &str, heading: &str) -> String {
    let h = xml_escape(heading);
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<html xmlns="http://www.w3.org/1999/xhtml" xmlns:epub="http://www.idpf.org/2007/ops"><head><title>{h}</title></head><body><nav epub:type="toc" id="toc"><h1>{h}</h1>{}</nav></body></html>"#,
        nav_ol(items, nav_dir)
    )
}

/// 写 nav：已有 nav 文档里找得到 `<nav epub:type="toc">` 就只换它的内容（开标签与原标题保留，其它 `<nav>` 不动）；
/// 找不到（或没有 nav 文档）就新建一份。
pub(super) fn write_nav(existing: Option<&str>, items: &[TocItem], nav_dir: &str, heading: &str) -> String {
    let Some(doc) = existing else { return build_nav(items, nav_dir, heading) };
    let spans = html::parse_spans(doc, 0, doc.len());
    let toc = spans.iter().position(|s| {
        s.name == "nav" && s.closed() && html::attr_value(&doc[s.open_start..s.open_end], "epub:type").is_some_and(|t| t.split_whitespace().any(|x| x == "toc"))
    });
    let Some(ti) = toc else { return build_nav(items, nav_dir, heading) };
    let nav = &spans[ti];
    // 原 toc 的标题：nav 的直接子元素里第一个 h1–h6。
    let title = spans.iter().find(|s| s.parent == Some(ti) && s.closed() && s.name.len() == 2 && s.name.starts_with('h') && s.name.as_bytes()[1].is_ascii_digit());
    let title = title.map_or(String::new(), |s| doc[s.open_start..s.close_end].to_string());
    format!("{}{}{}{}{}", &doc[..nav.open_end], title, nav_ol(items, nav_dir), &doc[nav.close_start..nav.close_end], &doc[nav.close_end..])
}

/// 目录条目的 href：`frag` 为空（正文没有锚点可指，退化条目直接指文件本身）时不带 `#`。
pub(super) fn toc_href(rel_path: &str, frag: &str) -> String {
    if frag.is_empty() { rel_path.to_string() } else { format!("{rel_path}#{frag}") }
}

/// 标题文本"标题+编号"拆分启发式（EPUB 线原则①：原书标题跟小节/章节编号拼在一行，如「第一章 1」，
/// TOC 要显示成两级——父级标题 + 缩进子级编号）。只在编号看起来像"小节序号"而非"印刷页码残留"时拆：
/// 数字编号要求 ≤99（页码常见三位数以上，且小节编号在同一本书里通常不会突然跳到几十以上）；中文数字编号
/// （〇一二三四五六七八九十百千，常见于章节内小节"之一/之二"变体的「1」以中文数字呈现）不做位数限制，
/// 因为原书不会用中文数字写页码。标题与编号之间的分隔允许普通空格与全角空格（U+3000）。
pub(super) fn split_numbered_title(title: &str) -> Option<(String, String)> {
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| Regex::new(r#"^(.+?)[ \u{3000}\t]+([0-9]+|[〇一二三四五六七八九十百千]+)$"#).unwrap());
    let c = re.captures(title.trim())?;
    let head = c[1].trim();
    let num = &c[2];
    if head.is_empty() {
        return None;
    }
    if let Ok(n) = num.parse::<u32>() {
        if n == 0 || n > 99 {
            return None; // 三位数以上大概率是印刷页码残留，不是小节编号，原样保留避免拆错
        }
    }
    Some((head.to_string(), num.to_string()))
}

/// 对已收集的标题条目做"标题+编号"拆分：命中的条目拆成父级(标题) + 子级(编号)两条，子级 level = 父级+1、
/// 指向同一锚点（不是两个可跳转目标，只是给 TOC 一个视觉分级，跟 `dense_ranks` 的嵌套机制天然兼容）。
pub(super) fn split_numbered_titles(items: Vec<TocItem>) -> Vec<TocItem> {
    let mut out = Vec::with_capacity(items.len());
    for it in items {
        match split_numbered_title(&it.title) {
            Some((head, num)) => {
                out.push(TocItem::new(it.level, head, it.path.clone(), it.frag.clone()));
                out.push(TocItem::new(it.level.saturating_add(1), num, it.path, it.frag));
            }
            None => out.push(it),
        }
    }
    out
}

/// 无任何 h1–h6 语义标题时的兜底 TOC：退化到按 spine 文件边界逐条生成，条目文本取该文件正文首个非空
/// 文本片段（截断），纯图片页/取不到文本则用"正文 N"占位——保证"没有目录的书优化后至少有可用目录"这个
/// 底线，而不是无声放弃。只在**多数** spine 文件确实有可提取文本时才生成，避免给纯图片书（漫画/画册）
/// 灌一堆没有信息量的"正文 N"占目录——那种书更适合交给漫画识别走专门路径，不该占用这条兜底。
pub(super) fn fallback_spine_toc(entries: &[Entry], spine: &[String], nav_doc: Option<&String>) -> Vec<TocItem> {
    let pages: Vec<&String> = spine.iter().filter(|p| Some(*p) != nav_doc).collect();
    if pages.is_empty() {
        return Vec::new();
    }
    let texts: Vec<Option<String>> = pages
        .iter()
        .map(|p| {
            let e = entries.iter().find(|e| &&e.name == p)?;
            let html = std::str::from_utf8(&e.data).ok()?;
            let t = plain_text(html::first_body_inner(html).unwrap_or(""));
            if t.is_empty() { None } else { Some(t) }
        })
        .collect();
    let with_text = texts.iter().filter(|t| t.is_some()).count();
    if with_text * 2 < pages.len() {
        return Vec::new(); // 多数页面没有可提取文本(疑似漫画/画册)，不生成兜底目录
    }
    pages
        .iter()
        .zip(texts.iter())
        .enumerate()
        .map(|(i, (p, t))| {
            let title = match t {
                Some(s) => s.chars().take(24).collect::<String>(),
                None => format!("正文 {}", i + 1),
            };
            TocItem::new(1, title, (*p).clone(), "")
        })
        .collect()
}

/// 按 spine 页分段的兜底目录：每 `FALLBACK_TOC_PAGES` 页一条，标题"第 N–M 页"，指向该段第一页。
pub(super) fn page_chunk_toc(spine: &[String], nav_doc: Option<&String>) -> Vec<TocItem> {
    let pages: Vec<&String> = spine.iter().filter(|p| Some(*p) != nav_doc).collect();
    crate::ncx::page_chunk_titles(pages.len()).into_iter().map(|(start, title)| TocItem::new(1, title, pages[start].clone(), "")).collect()
}

/// 分部标题前缀："第X部/卷/篇/辑"（X 为阿拉伯数字或中文数字），后面可能紧跟同一条目剩下的文本
/// （如"第一部　01　雪人"里"01　雪人"是这条目自己的章节标识，不是下一条的）。
pub(super) fn part_prefix_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r#"^(第[0-9〇一二三四五六七八九十百千]+[部卷篇辑])[ \u{3000}\t]*(.*)$"#).unwrap())
}

/// 把 ncx/nav 按 `items` 重写（ncx 整份重建；nav 只换 toc 部分，见 [`write_nav`]）。
fn rewrite_toc_files(entries: &mut [Entry], opf: &Opf, ncx_path: &str, items: &[TocItem], heading: &str) {
    let title = opf_book_title(entries, opf.index);
    let uid = opf_unique_identifier(entries).unwrap_or_else(|| WASH_MARK.to_string());
    let ncx = build_ncx(items, dir_of(ncx_path), &title, &uid).into_bytes();
    if let Some(e) = entries.iter_mut().find(|e| e.name == ncx_path) {
        e.data = ncx;
    }
    if let Some(nav_path) = opf.nav_doc.as_deref() {
        if let Some(e) = entries.iter_mut().find(|e| e.name == nav_path) {
            let old = String::from_utf8_lossy(&e.data).into_owned();
            e.data = write_nav(Some(&old), items, dir_of(nav_path), heading).into_bytes();
        }
    }
}

/// 书自带的扁平目录如果符合"第X部　编号　章名"这种排版惯例（分部标题只在每部第一条出现、其余
/// 条目隐式归属该部——真机《雪人》坐实：reMarkable 原生目录面板显示的是完全扁平的列表，"01 雪人"
/// 没有嵌在"第一部"下面），重建成两级：分部标题单独成一条父级（沿用该条目自己的跳转目标——分部
/// 标题这条本身就是这部的开篇章节，能跳）；分部前缀后剩下的文本（如"01　雪人"）连同后续不带
/// 前缀的条目一起降一级当子级。**一条"第X部"前缀都没匹配到＝原样不动**——不是所有书都用这种
/// 排版惯例，没信号时贸然重建有误伤风险，见 §03az。
pub(super) fn restructure_existing_toc_parts(entries: &mut [Entry], mode: AutoToc, heading: &str, rep: &mut WashReport) {
    if mode == AutoToc::Off {
        return;
    }
    let Some(opf) = parse_opf(entries) else { return };
    let Some(ncx_path) = opf.ncx.clone() else { return };
    let Some(e) = entries.iter().find(|e| e.name == ncx_path) else { return };
    let Ok(ncx_text) = std::str::from_utf8(&e.data) else { return };
    // 标题里的空白折叠成单个空格（全角空格分隔的"第一部　01　雪人"→"第一部 01 雪人"，跟标题里其它空白一视同仁）。
    let flat: Vec<(String, String)> = crate::ncx::parse_ncx_flat(ncx_text).into_iter().map(|(_, t, src)| (t.split_whitespace().collect::<Vec<_>>().join(" "), src)).collect();
    let re = part_prefix_re();
    if flat.len() < 2 || !flat.iter().any(|(t, _)| re.is_match(t)) {
        return;
    }
    let ncx_dir = dir_of(&ncx_path).to_string();
    let mut items: Vec<TocItem> = Vec::with_capacity(flat.len());
    let mut in_part = false;
    for (title, src) in &flat {
        let (raw_path, frag) = html::split_href(src);
        let (path, frag) = (resolve(&ncx_dir, &percent_decode(raw_path)), frag.unwrap_or("").to_string());
        if let Some(c) = re.captures(title) {
            items.push(TocItem::new(1, &c[1], path.clone(), frag.clone()));
            let rest = c[2].trim();
            if !rest.is_empty() {
                items.push(TocItem::new(2, rest, path, frag));
            }
            in_part = true;
        } else {
            items.push(TocItem::new(if in_part { 2 } else { 1 }, title.clone(), path, frag));
        }
    }
    rewrite_toc_files(entries, &opf, &ncx_path, &items, heading);
    rep.toc_parts_restructured = items.len();
}

pub(super) fn auto_toc(entries: &mut Vec<Entry>, mode: AutoToc, heading: &str, rep: &mut WashReport) {
    if mode == AutoToc::Off || (mode == AutoToc::IfMissing && toc_entry_count(entries) > 0) {
        return;
    }
    let Some(opf) = parse_opf(entries) else { return };
    let headings = collect_toc_headings(entries, &opf.spine, opf.nav_doc.as_ref());
    let headings = if headings.is_empty() { fallback_spine_toc(entries, &opf.spine, opf.nav_doc.as_ref()) } else { split_numbered_titles(headings) };
    // 纯图片书（漫画/画册）：没有标题也没有可提取文字，`fallback_spine_toc` 故意不生成"正文 N"。但用户要求
    // **所有书都要有目录**（2026-09-20，乱马源书 NCX 是空的，转出来没目录），所以按页分段生成"第 N–M 页"
    // ——如实标注不是章节，只为能按段跳转（同 `ncx::page_chunk_titles`，PDF 路径也是这套）。
    let headings = if headings.is_empty() { page_chunk_toc(&opf.spine, opf.nav_doc.as_ref()) } else { headings };
    if headings.is_empty() {
        return;
    }
    let title = opf_book_title(entries, opf.index);
    let ncx_path = opf.ncx.clone().unwrap_or_else(|| resolve(&opf.dir, "toc.ncx"));
    let nav_path = opf.nav_doc.clone().unwrap_or_else(|| resolve(&opf.dir, "nav.xhtml"));
    let uid = opf_unique_identifier(entries).unwrap_or_else(|| WASH_MARK.to_string());
    let ncx = build_ncx(&headings, dir_of(&ncx_path), &title, &uid).into_bytes();
    let old_nav = entries.iter().find(|e| e.name == nav_path).map(|e| String::from_utf8_lossy(&e.data).into_owned());
    let nav = write_nav(old_nav.as_deref(), &headings, dir_of(&nav_path), heading).into_bytes();
    let mut text = String::from_utf8_lossy(&entries[opf.index].data).into_owned();
    if opf.ncx.is_none() {
        // id 必须叫 "ncx"（不是随便起的标记）——xochitl 定位目录文件靠二进制里硬编码死查这个
        // 字符串字面量，见 `fix_ncx_manifest_id` 的注释。
        text = text.replacen("</manifest>", &format!(r#"<item id="ncx" href="{}" media-type="application/x-dtbncx+xml"/></manifest>"#, relative_to(&opf.dir, &ncx_path)), 1);
        if let Some(t) = html::tags(&text).find(|t| t.is_start() && t.is("spine")) {
            let tag = &text[t.start..t.end];
            if html::attr(tag, "toc").is_none() {
                text = format!("{}{}{}", &text[..t.start], html::set_attr(tag, "toc", "ncx"), &text[t.end..]);
            }
        }
    }
    if opf.nav_doc.is_none() {
        text = text.replacen("</manifest>", &format!(r#"<item id="eink-nav" href="{}" media-type="application/xhtml+xml" properties="nav"/></manifest>"#, relative_to(&opf.dir, &nav_path)), 1);
    }
    entries[opf.index].data = text.into_bytes();
    for (path, data) in [(ncx_path, ncx), (nav_path, nav)] {
        match entries.iter_mut().find(|e| e.name == path) {
            Some(e) => e.data = data,
            None => entries.push(Entry { name: path, data }),
        }
    }
    rep.toc_generated = headings.len();
}

/// 分页拆出来的一节：所在文件（拆过的是那一份）、标题 id、标题文字。
pub(super) struct SectionRef {
    pub path: String,
    pub id: String,
    pub label: String,
}

/// 章节分页后把书自带目录里**漏掉的节**补进去（用户 2026-09-27：节要缩进出现在目录里）。`sections` 按阅读顺序。
/// 书自带的条目原样保留、顺序不动；缺的节插在阅读顺序上它之前的最后一条后面，
/// 层级 = 往前找到的第一条"章"（非节条目）的下一级，前一条本身就是节时同级。没有 NCX 的书不动（自动目录已含全部标题）。
/// 返回补了几条。每条目录/节的阅读位置（spine 序号, 文件内偏移）只算一次（此前每插一节都把全部条目重算一遍，
/// 章节多的书是平方级）。
pub(super) fn merge_sections_into_toc(entries: &mut [Entry], sections: &[SectionRef], heading: &str) -> usize {
    let Some(opf) = parse_opf(entries) else { return 0 };
    let Some(ncx_path) = opf.ncx.clone() else { return 0 };
    let Some(ncx) = entries.iter().find(|e| e.name == ncx_path) else { return 0 };
    let flat = crate::ncx::parse_ncx_flat(&String::from_utf8_lossy(&ncx.data));
    if flat.is_empty() {
        return 0;
    }
    let spine_pos: HashMap<&str, usize> = opf.spine.iter().enumerate().map(|(i, p)| (p.as_str(), i)).collect();
    // 文件 → (锚点 → 偏移)，按需建一次。
    let mut anchor_cache: HashMap<String, HashMap<String, usize>> = HashMap::new();
    let mut key_of = |path: &str, frag: &str| -> (usize, usize) {
        let sp = spine_pos.get(path).copied().unwrap_or(usize::MAX);
        if frag.is_empty() {
            return (sp, 0);
        }
        let map = anchor_cache.entry(path.to_string()).or_insert_with(|| {
            let mut m = HashMap::new();
            if let Some(e) = entries.iter().find(|e| e.name == path) {
                for (id, pos) in html::anchors(&String::from_utf8_lossy(&e.data)) {
                    m.entry(id.to_string()).or_insert(pos);
                }
            }
            m
        });
        (sp, map.get(frag).copied().unwrap_or(0))
    };
    let sec_ids: HashSet<(&str, &str)> = sections.iter().map(|s| (s.path.as_str(), s.id.as_str())).collect();
    let sec_paths: HashSet<&str> = sections.iter().map(|s| s.path.as_str()).collect();
    struct Item {
        toc: TocItem,
        is_sec: bool,
        key: (usize, usize),
    }
    let mut items: Vec<Item> = flat
        .into_iter()
        .map(|(depth, label, target)| {
            let (p, f) = html::split_href(&target);
            let path = posix_norm(&resolve(dir_of(&ncx_path), &percent_decode(p)));
            let f = f.unwrap_or("");
            let id = html::frag_id(f).into_owned();
            let is_sec = if id.is_empty() { sec_paths.contains(path.as_str()) } else { sec_ids.contains(&(path.as_str(), id.as_str())) };
            let key = key_of(&path, &id);
            Item { toc: TocItem::new(depth.max(1) as u8, label, path, f), is_sec, key }
        })
        .collect();
    // 已在目录里的节：(路径, id) 或"指向该份文件本身"。
    let mut present: HashSet<(String, String)> = items.iter().filter(|it| it.is_sec).map(|it| (it.toc.path.clone(), html::frag_id(&it.toc.frag).into_owned())).collect();
    let mut added = 0;
    for s in sections {
        if present.contains(&(s.path.clone(), s.id.clone())) || present.contains(&(s.path.clone(), String::new())) {
            continue;
        }
        let key = key_of(&s.path, &s.id);
        let at = items.iter().rposition(|it| it.key <= key).map_or(0, |i| i + 1);
        let depth = match items[..at].last() {
            Some(prev) if prev.is_sec => prev.toc.level,
            Some(prev) => prev.toc.level + 1,
            None => 1,
        };
        items.insert(at, Item { toc: TocItem::new(depth, s.label.clone(), s.path.clone(), s.id.clone()), is_sec: true, key });
        present.insert((s.path.clone(), s.id.clone()));
        added += 1;
    }
    if added == 0 {
        return 0;
    }
    let toc: Vec<TocItem> = items.into_iter().map(|it| it.toc).collect();
    rewrite_toc_files(entries, &opf, &ncx_path, &toc, heading);
    added
}
