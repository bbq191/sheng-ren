//! 目录：目录文件判定、自动生成目录（ncx+nav）、已有扁平目录按"第X部"重建为两级、定章节后补节。
//!
//! 重建已有目录时只换 nav 文档里的 `<nav epub:type="toc">`：landmarks、page-list 等其它 `<nav>` 和 head 原样保留，
//! 原 toc 的标题（`<h1>目录</h1>`/`<h2>Contents</h2>`）沿用；新建时标题按书的语言（中文"目录"、其它"Contents"）。
use super::*;

/// 按文件名看是不是目录文件：NCX（`*.ncx`），或文件名正好是 `nav.xhtml`/`nav.html`（不分大小写）。
/// OPF 声明的导航文档（`properties="nav"`，不一定叫 nav）由调用方另外认（`Opf::nav_doc`）。
/// 2026-09-30 审计：此前只看文件名以 `nav` 开头，`navarre.xhtml`、`navy.html` 这类正文章节被当成目录、整章跳过清洗和定章节。
pub fn is_toc_file(name: &str) -> bool {
    let l = name.to_ascii_lowercase();
    let base = l.rsplit('/').next().unwrap_or(&l);
    l.ends_with(".ncx") || base == "nav.xhtml" || base == "nav.html"
}

/// 一条目录：级别（h 级别或目录深度）、标题（纯文本）、目标文件的 zip 路径、锚点（空 = 指文件本身）。
/// 锚点是**字符引用已还原**的值（百分号编码照原样）：写进 NCX/nav 时由 `build_ncx`/`nav_ol`/`replace_nav_map` 转义一次；
/// 从 `id` 属性原文取来的要先 `xml_unescape`（2026-09-30 审计：此前原文直接进来，`id="a&amp;b"` 写成 `#a&amp;amp;b`）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TocItem {
    pub level: u8,
    pub title: String,
    pub path: String,
    pub frag: String,
    /// 来自书自带 NCX 的条目：原来的 `<navPoint …>` 开标签（重写目录时沿用它的 id 等属性）；新生成的为 `None`。
    pub np: Option<String>,
}

impl TocItem {
    pub(super) fn new(level: u8, title: impl Into<String>, path: impl Into<String>, frag: impl Into<String>) -> Self {
        TocItem { level, title: title.into(), path: path.into(), frag: frag.into(), np: None }
    }
}

/// NCX 里一条 `<content src>`（[`crate::ncx::NavPoint::src`]，字符引用已还原）→ (目标文件的 zip 路径, 锚点原文——没有是空串)。
/// 路径百分号解码、相对 NCX 所在目录解析；锚点原样（写回目录时照用）。
fn ncx_target<'a>(ncx_path: &str, src: &'a str) -> (String, &'a str) {
    let (path, frag) = crate::epubzip::resolve_href(ncx_path, src);
    (path, frag.unwrap_or(""))
}

/// 新建目录时的标题：中文书"目录"，其它"Contents"。
pub(super) fn toc_title(lang: LangMode) -> &'static str {
    if lang == LangMode::Latin { "Contents" } else { "目录" }
}

// ───────────────────────── 6. 自动目录 ─────────────────────────

/// 目录条目数（ncx `src` + nav `href`，排除 toc 文件自指与非 html 目标）。目录文件＝OPF 声明的 nav 文档（`properties="nav"`）
/// 与 NCX，再加上文件名像目录的（[`is_toc_file`]）——nav 文档不一定叫 `nav.xhtml`（2026-09-28 审计：叫 `toc.xhtml`、又没有 NCX 的书
/// 被当成"没有目录"，自带目录被自动目录覆盖）。
pub fn toc_entry_count(entries: &[Entry]) -> usize {
    let declared: Vec<String> = parse_opf(entries).map(|o| o.nav_doc.into_iter().chain(o.ncx).collect()).unwrap_or_default();
    let mut n = 0;
    for e in entries.iter().filter(|e| is_toc_file(&e.name) || declared.contains(&e.name)) {
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
    items.iter().map(|i| u8::try_from(levels.iter().position(|&l| l == i.level).unwrap_or(0) + 1).unwrap_or(u8::MAX)).collect()
}

/// 从 spine 各章 h1–h6 生成目录条目；标题没有 id 就补 `id="eink-toc-N"`（已有 `id`——单引号也算——沿用，不追加第二个）。
/// 与定章节的标题识别（`chapters::collect_headings`）不同：这里只要 `<hN>` 有文字就收，不判角色。
pub(super) fn collect_toc_headings(entries: &mut [Entry], spine: &[String], nav_doc: Option<&String>) -> Vec<TocItem> {
    let mut out = Vec::new();
    let mut counter = 0usize;
    let pos: Vec<Option<usize>> = {
        let index = name_index(entries);
        spine.iter().map(|p| index.get(p.as_str()).copied()).collect()
    };
    for (p, i) in spine.iter().zip(pos) {
        if Some(p) == nav_doc {
            continue;
        }
        let Some(i) = i else { continue };
        let e = &mut entries[i];
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
                Some(id) => crate::util::xml_unescape(id).into_owned(),
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
        let d = ranks[i].min(depth.saturating_add(1)); // 钳制：不跳跃深入 >1 层，保证良构
        if d <= depth {
            for _ in 0..(depth - d + 1) {
                s.push_str("</navPoint>");
            }
        }
        let href = crate::epubzip::href_to(ncx_dir, &it.path, &it.frag);
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
        let d = ranks[i].min(depth.saturating_add(1));
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
        let href = crate::epubzip::href_to(nav_dir, &it.path, &it.frag);
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
    let title = spans.iter().find(|s| s.parent == Some(ti) && s.closed() && s.heading_level().is_some());
    let title = title.map_or(String::new(), |s| doc[s.open_start..s.close_end].to_string());
    format!("{}{}{}{}{}", &doc[..nav.open_end], title, nav_ol(items, nav_dir), &doc[nav.close_start..nav.close_end], &doc[nav.close_end..])
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
    let index = name_index(entries);
    let texts: Vec<Option<String>> = pages
        .iter()
        .map(|p| {
            let e = &entries[*index.get(p.as_str())?];
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

/// 把 ncx/nav 按 `items` 重写。ncx 只换 navMap（`ncx::replace_nav_map`：`head`、`pageList` 等原样，原条目的 navPoint id 沿用；
/// 2026-09-28 审计：此前整份重建，id 全换、页码表丢了）；没有 navMap 才整份重建。nav 只换 toc 部分，见 [`write_nav`]。
fn rewrite_toc_files(entries: &mut [Entry], opf: &Opf, ncx_path: &str, items: &[TocItem], heading: &str) {
    let ncx_dir = dir_of(ncx_path);
    let old = entries.iter().find(|e| e.name == ncx_path).map(|e| String::from_utf8_lossy(&e.data).into_owned());
    let ranks = dense_ranks(items);
    let srcs: Vec<String> = items.iter().map(|it| crate::epubzip::href_to(ncx_dir, &it.path, &it.frag)).collect();
    let points: Vec<crate::ncx::NewNavPoint> =
        items.iter().zip(&ranks).zip(&srcs).map(|((it, &depth), src)| crate::ncx::NewNavPoint { depth, label: &it.title, src, open_tag: it.np.as_deref() }).collect();
    let ncx = match old.as_deref().and_then(|t| crate::ncx::replace_nav_map(t, &points)) {
        Some(t) => t.into_bytes(),
        None => {
            let title = opf_book_title(entries, opf.index);
            let uid = opf_unique_identifier(entries).unwrap_or_else(|| WASH_MARK.to_string());
            build_ncx(items, ncx_dir, &title, &uid).into_bytes()
        }
    };
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
/// 排版惯例，没信号时贸然重建有误伤风险，见 §03az。**只重建完全扁平的目录**：已经分了层级的（部下面已经挂着章）
/// 原样不动（2026-09-28 审计：此前把已嵌套的目录压平重排）。重建时沿用原条目的 navPoint id，NCX 其余部分不动。
pub(super) fn restructure_existing_toc_parts(entries: &mut [Entry], mode: AutoToc, heading: &str, rep: &mut WashReport) {
    if mode == AutoToc::Off {
        return;
    }
    let Some(opf) = parse_opf(entries) else { return };
    let Some(ncx_path) = opf.ncx.clone() else { return };
    let Some(e) = entries.iter().find(|e| e.name == ncx_path) else { return };
    let Ok(ncx_text) = std::str::from_utf8(&e.data) else { return };
    let points = crate::ncx::parse_nav_points(ncx_text);
    if points.iter().any(|p| p.depth != 1) {
        return;
    }
    // 标题里的空白折叠成单个空格（全角空格分隔的"第一部　01　雪人"→"第一部 01 雪人"，跟标题里其它空白一视同仁）。
    let flat: Vec<(String, &crate::ncx::NavPoint)> = points.iter().map(|p| (p.label.split_whitespace().collect::<Vec<_>>().join(" "), p)).collect();
    let to_item = |depth: u8, title: &str, p: &crate::ncx::NavPoint| {
        let (path, frag) = ncx_target(&ncx_path, &p.src);
        let np = Some(p.open_tag.clone()).filter(|t| !t.is_empty());
        TocItem { np, ..TocItem::new(depth, title, path, frag) }
    };
    let titles: Vec<&str> = flat.iter().map(|(t, _)| t.as_str()).collect();
    if let Some(depths) = collection_depths(&titles) {
        let items: Vec<TocItem> = flat.iter().zip(&depths).map(|((t, p), &d)| to_item(d, t, p)).collect();
        rewrite_toc_files(entries, &opf, &ncx_path, &items, heading);
        rep.toc_parts_restructured = items.len();
        return;
    }
    let re = part_prefix_re();
    if flat.len() < 2 || !flat.iter().any(|(t, _)| re.is_match(t)) {
        return;
    }
    let mut items: Vec<TocItem> = Vec::with_capacity(flat.len());
    let mut in_part = false;
    for (title, p) in &flat {
        let (path, frag) = ncx_target(&ncx_path, &p.src);
        let np = Some(p.open_tag.clone()).filter(|t| !t.is_empty());
        if let Some(c) = re.captures(title) {
            items.push(TocItem { np, ..TocItem::new(1, &c[1], path.clone(), frag) });
            let rest = c[2].trim();
            if !rest.is_empty() {
                items.push(TocItem::new(2, rest, path, frag));
            }
            in_part = true;
        } else {
            items.push(TocItem { np, ..TocItem::new(if in_part { 2 } else { 1 }, title.clone(), path, frag) });
        }
    }
    rewrite_toc_files(entries, &opf, &ncx_path, &items, heading);
    rep.toc_parts_restructured = items.len();
}

/// 一层平排的合集目录（福尔摩斯全集：「第一册」「书名页」「目录」「暗红习作」「第一部」「第一章……」…「四签名」「第一章……」…）的层级。
/// 认两种信号，都没有时返回 `None`（按「第X部」重建或原样不动）：
/// - **册**：「第X册」「上册/中册/下册」至少两条——各册是最上一层，后面到下一册之前的条目都挂在它下面；第一册前面的（出版说明、序言）不动。
/// - **作品**：普通条目后面（跳过题献、前言这类）紧跟「第一部」或「第一章」，即编号从头开始——全书至少两处才算合集，
///   这样的条目是作品，后面的部、章、题献挂在它下面，到下一个普通条目为止（第六册的短篇一篇篇平排）。
///   只有一处的是普通小说（「序章」「第一部」「第一章」…「第二部」「第一章」），不当作品。
///
/// 「部」下面挂着它后面的章，到下一个部或普通条目为止。
fn collection_depths(titles: &[&str]) -> Option<Vec<u8>> {
    #[derive(Clone, Copy, PartialEq)]
    enum K {
        Volume,
        Part,
        Chapter,
        Front,
        Plain,
    }
    static RE: OnceLock<[Regex; 4]> = OnceLock::new();
    let [vol, part, chap, front] = RE.get_or_init(|| {
        const N: &str = "0-9０-９〇零一二三四五六七八九十百千两";
        [
            Regex::new(&format!(r#"^(第[{N}]+册|[上中下]册)$"#)).unwrap(),
            Regex::new(&format!(r#"^第[{N}]+[部卷篇辑]"#)).unwrap(),
            Regex::new(&format!(r#"^第[{N}]+[章回]"#)).unwrap(),
            Regex::new(r#"^(题献|献词|献辞|题记|前言|序|序言|序章|引言|引子|楔子|译序|译者序|出版说明|书名页|目录|版权页?)$"#).unwrap(),
        ]
    });
    let kind = |t: &str| {
        let compact: String = t.chars().filter(|c| !c.is_whitespace()).collect();
        if vol.is_match(&compact) {
            K::Volume
        } else if part.is_match(t) {
            K::Part
        } else if chap.is_match(t) {
            K::Chapter
        } else if front.is_match(&compact) {
            K::Front
        } else {
            K::Plain
        }
    };
    let first = |t: &str| {
        let rest = t.strip_prefix('第').unwrap_or("");
        let mut cs = rest.chars();
        matches!(cs.next(), Some('一' | '1' | '１')) && cs.next().is_some_and(|c| matches!(c, '部' | '卷' | '篇' | '辑' | '章' | '回'))
    };
    let kinds: Vec<K> = titles.iter().map(|t| kind(t)).collect();
    // 作品：普通条目，往后跳过 Front 的第一条是编号从 1 起的部/章
    let work: Vec<bool> = (0..titles.len())
        .map(|i| kinds[i] == K::Plain && (i + 1..titles.len()).find(|&j| kinds[j] != K::Front).is_some_and(|j| matches!(kinds[j], K::Part | K::Chapter) && first(titles[j])))
        .collect();
    let volumes = kinds.iter().filter(|k| **k == K::Volume).count();
    let works = work.iter().filter(|w| **w).count();
    let (use_vol, use_work) = (volumes >= 2, works >= 2);
    if !use_vol && !use_work {
        return None;
    }
    let mut out = Vec::with_capacity(titles.len());
    let (mut base, mut in_work, mut in_part) = (1u8, false, false);
    for (i, &k) in kinds.iter().enumerate() {
        let d = match k {
            K::Volume if use_vol => {
                base = 2;
                in_work = false;
                in_part = false;
                1
            }
            _ if use_work && work[i] => {
                in_work = true;
                in_part = false;
                base
            }
            K::Part => {
                in_part = true;
                base + in_work as u8
            }
            K::Chapter => base + in_work as u8 + in_part as u8,
            K::Front if in_work => base + 1,
            _ => {
                in_work = false;
                in_part = false;
                base
            }
        };
        out.push(d);
    }
    Some(out)
}

/// 目录里任意一层的兄弟条目中有「第X部/卷/篇」、而它自己下面没有子条目时，把它后面到下一个「部」之前的兄弟条目
/// 缩进到它下面（福尔摩斯全集里《恐怖谷》：书下面「第一部 伯尔斯通惨剧」「第一章 警讯」…「第二部」「第一章」…平排）。
/// 第一个「部」前面的条目（书名页、前言）不动。全书一层平排的目录由 [`restructure_existing_toc_parts`] 处理（还要拆标签）。
/// 用户 2026-10-05：目录按书/卷 → 章 → 节嵌套。返回缩进了几条。
pub(super) fn nest_parts_among_siblings(entries: &mut [Entry], mode: AutoToc, heading: &str, rep: &mut WashReport) {
    if mode == AutoToc::Off {
        return;
    }
    let Some(opf) = parse_opf(entries) else { return };
    let Some(ncx_path) = opf.ncx.clone() else { return };
    let Some(e) = entries.iter().find(|e| e.name == ncx_path) else { return };
    let Ok(ncx_text) = std::str::from_utf8(&e.data) else { return };
    let points = crate::ncx::parse_nav_points(ncx_text);
    if points.len() < 2 || points.iter().all(|p| p.depth == 1) {
        return;
    }
    let re = part_prefix_re();
    let is_part = |l: &str| re.is_match(&l.split_whitespace().collect::<Vec<_>>().join(" "));
    let mut depth: Vec<usize> = points.iter().map(|p| p.depth).collect();
    let mut moved = 0usize;
    // 每条的子树结束位置（下一个层级不比它深的条目）。
    let subtree_end = |depth: &[usize], i: usize| (i + 1..depth.len()).find(|&j| depth[j] <= depth[i]).unwrap_or(depth.len());
    let mut i = 0;
    while i < points.len() {
        let d = depth[i];
        let end = subtree_end(&depth, i);
        let childless = end == i + 1;
        if is_part(&points[i].label) && childless {
            // 后面同一父条目下、到下一个「部」（或父条目结束）之前的兄弟条目，连子树一起深一层。
            let mut j = end;
            let mut last = end;
            while j < depth.len() && depth[j] >= d {
                if depth[j] == d && is_part(&points[j].label) {
                    break;
                }
                last = j + 1;
                j += 1;
            }
            if last > end {
                for dk in &mut depth[end..last] {
                    *dk += 1;
                }
                moved += (end..last).filter(|&k| depth[k] == d + 1).count();
            }
        }
        i += 1;
    }
    if moved == 0 {
        return;
    }
    let items: Vec<TocItem> = points
        .iter()
        .zip(&depth)
        .map(|(p, &d)| {
            let (path, frag) = ncx_target(&ncx_path, &p.src);
            let np = Some(p.open_tag.clone()).filter(|t| !t.is_empty());
            TocItem { np, ..TocItem::new(d.min(255) as u8, p.label.clone(), path, frag) }
        })
        .collect();
    rewrite_toc_files(entries, &opf, &ncx_path, &items, heading);
    rep.toc_parts_restructured += moved;
}

pub(super) fn auto_toc(entries: &mut Vec<Entry>, mode: AutoToc, heading: &str, rep: &mut WashReport) {
    if mode == AutoToc::Off || (mode == AutoToc::IfMissing && toc_entry_count(entries) > 0) {
        return;
    }
    let Some(opf) = parse_opf(entries) else { return };
    let headings = collect_toc_headings(entries, &opf.spine, opf.nav_doc.as_ref());
    let headings = if headings.is_empty() { fallback_spine_toc(entries, &opf.spine, opf.nav_doc.as_ref()) } else { headings };
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
        let href = crate::epubzip::href_to(&opf.dir, &ncx_path, "");
        if let Some(t) = opf::insert_manifest_items(&text, &[opf::NewItem { id: "ncx", href: &href, media_type: "application/x-dtbncx+xml", properties: "" }]) {
            text = t;
        }
        if let Some(t) = html::tags(&text).find(|t| t.is_start() && opf::is_local(t.name, "spine")) {
            let tag = &text[t.start..t.end];
            if html::attr(tag, "toc").is_none() {
                text = format!("{}{}{}", &text[..t.start], html::set_attr(tag, "toc", "ncx"), &text[t.end..]);
            }
        }
    }
    if opf.nav_doc.is_none() {
        let href = crate::epubzip::href_to(&opf.dir, &nav_path, "");
        if let Some(t) = opf::insert_manifest_items(&text, &[opf::NewItem { id: "eink-nav", href: &href, media_type: "application/xhtml+xml", properties: "nav" }]) {
            text = t;
        }
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

/// 要进目录的一节：所在文件、标题 id、标题文字。
pub(super) struct SectionRef {
    pub path: String,
    pub id: String,
    pub label: String,
}

/// 定完章节后把书自带目录里**漏掉的节**补进去（用户 2026-09-27：节要缩进出现在目录里）。`sections` 按阅读顺序。
/// 书自带的条目原样保留、顺序不动；缺的节插在阅读顺序上它之前的最后一条后面，
/// 层级 = 往前找到的第一条"章"（非节条目）的下一级，前一条本身就是节时同级。没有 NCX 的书不动（自动目录已含全部标题）。
/// 书自带目录里**已有**的节条目（和章平排的，如《13級階梯》`第一章　出獄　　１`、`　　２`、`　　３`）缩进到所属章下面、
/// 标签去掉首尾空白；章条目标签末尾跟着第一节的节号（`第一章　出獄　　１`）时去掉节号——节号另成一条。
/// 返回补了几条。每条目录/节的阅读位置（spine 序号, 文件内偏移）只算一次（此前每插一节都把全部条目重算一遍，
/// 章节多的书是平方级）。
pub(super) fn merge_sections_into_toc(entries: &mut [Entry], sections: &[SectionRef], heading: &str) -> usize {
    let Some(opf) = parse_opf(entries) else { return 0 };
    let Some(ncx_path) = opf.ncx.clone() else { return 0 };
    let Some(ncx) = entries.iter().find(|e| e.name == ncx_path) else { return 0 };
    let flat = crate::ncx::parse_nav_points(&String::from_utf8_lossy(&ncx.data));
    if flat.is_empty() {
        return 0;
    }
    let spine_pos: HashMap<&str, usize> = opf.spine.iter().enumerate().map(|(i, p)| (p.as_str(), i)).collect();
    // 文件 → (锚点 → 偏移)，按需建一次。
    // 锚点、节 id 一律按字符引用还原后的值比（NCX 的 src 已还原，见 `ncx::NavPoint::src`；id 属性是原文）。
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
                    m.entry(crate::util::xml_unescape(id).into_owned()).or_insert(pos);
                }
            }
            m
        });
        (sp, map.get(frag).copied().unwrap_or(0))
    };
    let sec_id = |s: &SectionRef| crate::util::xml_unescape(&s.id).into_owned();
    let sec_ids: HashSet<(&str, String)> = sections.iter().map(|s| (s.path.as_str(), sec_id(s))).collect();
    // 只指到文件的条目算节，要求节标题就在那个文件开头（文件开头是章标题、节在后面时，这条目录是章）
    let sec_paths: HashSet<&str> = sections_starting_files(entries, sections);
    struct Item {
        toc: TocItem,
        is_sec: bool,
        key: (usize, usize),
    }
    let mut items: Vec<Item> = flat
        .into_iter()
        .map(|crate::ncx::NavPoint { depth, label, src: target, open_tag }| {
            let (path, f) = ncx_target(&ncx_path, &target);
            let id = html::frag_id(f).into_owned();
            let is_sec = if id.is_empty() { sec_paths.contains(path.as_str()) } else { sec_ids.contains(&(path.as_str(), id.clone())) };
            let key = key_of(&path, &id);
            // 层级到 u8 为止（此前 `as u8` 截断：嵌套 256 层的条目成了 0 层）
            Item { toc: TocItem { np: Some(open_tag).filter(|t| !t.is_empty()), ..TocItem::new(depth.clamp(1, 255) as u8, label, path, f) }, is_sec, key }
        })
        .collect();
    // 已在目录里的节：(路径, id) 或"指向该份文件本身"。
    let mut present: HashSet<(String, String)> = items.iter().filter(|it| it.is_sec).map(|it| (it.toc.path.clone(), html::frag_id(&it.toc.frag).into_owned())).collect();
    let mut added = 0;
    for s in sections {
        let id = sec_id(s);
        if present.contains(&(s.path.clone(), id.clone())) || present.contains(&(s.path.clone(), String::new())) {
            continue;
        }
        let key = key_of(&s.path, &id);
        let at = items.iter().rposition(|it| it.key <= key).map_or(0, |i| i + 1);
        let depth = match items[..at].last() {
            Some(prev) if prev.is_sec => prev.toc.level,
            Some(prev) => prev.toc.level.saturating_add(1),
            None => 1,
        };
        items.insert(at, Item { toc: TocItem::new(depth, s.label.clone(), s.path.clone(), id.clone()), is_sec: true, key });
        present.insert((s.path.clone(), id));
        added += 1;
    }
    // 书自带的节条目缩进到所属章下面、标签去掉首尾空白。
    let mut changed = added > 0;
    let mut chapter: Option<usize> = None;
    for i in 0..items.len() {
        if !items[i].is_sec {
            chapter = Some(i);
            continue;
        }
        let Some(c) = chapter else { continue };
        let want = items[c].toc.level.saturating_add(1);
        if items[i].toc.level < want {
            items[i].toc.level = want.min(6);
            changed = true;
        }
        let trimmed = items[i].toc.title.trim().to_string();
        if trimmed != items[i].toc.title {
            items[i].toc.title = trimmed;
            changed = true;
        }
    }
    if !changed {
        return 0;
    }
    let toc: Vec<TocItem> = items.into_iter().map(|it| it.toc).collect();
    rewrite_toc_files(entries, &opf, &ncx_path, &toc, heading);
    added
}

/// 节标题（`id`）是不是在文件 `path` 的正文最前面（前面没有可见内容）。
fn section_starts_file(t: &str, lo: usize, anchors: &HashMap<String, usize>, id: &str) -> bool {
    anchors.get(id).is_some_and(|&p| p >= lo && !html::has_visible(&t[lo..p]))
}

/// 标题就在文件开头的节（[`section_starts_file`]）所在的文件。每个文件的正文范围、锚点表只算一次（以前每个节都把整个文件的锚点
/// 重扫一遍，节多的大文件是平方级），各文件、各节多线程算。
fn sections_starting_files<'s>(entries: &[Entry], sections: &'s [SectionRef]) -> HashSet<&'s str> {
    let index = name_index(entries);
    let mut paths: Vec<&str> = sections.iter().map(|s| s.path.as_str()).collect();
    paths.sort_unstable();
    paths.dedup();
    // 文件 → (文字, 正文开头, 锚点 → 第一处的位置)
    let files = crate::util::par_map(&paths, |path| {
        let e = &entries[*index.get(path)?];
        let t = String::from_utf8_lossy(&e.data);
        let (lo, _) = html::body_range(&t)?;
        Some((t, lo))
    });
    let anchors = crate::util::par_map(&files, |f| {
        let mut m: HashMap<String, usize> = HashMap::new();
        if let Some((t, _)) = f {
            for (a, p) in html::anchors(t) {
                m.entry(a.to_string()).or_insert(p);
            }
        }
        m
    });
    let by_path: HashMap<&str, usize> = paths.iter().enumerate().map(|(i, p)| (*p, i)).collect();
    let starts = crate::util::par_map(sections, |s| {
        let i = by_path[s.path.as_str()];
        files[i].as_ref().is_some_and(|(t, lo)| section_starts_file(t, *lo, &anchors[i], &s.id))
    });
    sections.iter().zip(starts).filter(|(_, ok)| *ok).map(|(s, _)| s.path.as_str()).collect()
}

// ───────────────────────── 书自带目录指错位置的修复 ─────────────────────────

/// 条目名 → 下标（同名取第一条；清洗层、漫画识别按名字找条目都用这一份）。
pub(crate) fn name_index(entries: &[Entry]) -> HashMap<&str, usize> {
    let mut m = HashMap::with_capacity(entries.len());
    for (i, e) in entries.iter().enumerate() {
        m.entry(e.name.as_str()).or_insert(i);
    }
    m
}

/// 一个文件的正文范围与锚点表（锚点 → 偏移，同名取第一处）。
type FileAnchors<'a> = (Option<(usize, usize)>, HashMap<Cow<'a, str>, usize>);

/// 读"某文件某锚点后面的文字"用的缓存：每个文件的正文范围与锚点表只算一次（此前每条目录都把目标文件的锚点整份重扫一遍）。
struct TextAt<'a> {
    entries: &'a [Entry],
    index: HashMap<&'a str, usize>,
    /// 文件 → (正文范围, 锚点 → 偏移（同名取第一处）)
    files: HashMap<String, FileAnchors<'a>>,
}

impl<'a> TextAt<'a> {
    fn new(entries: &'a [Entry]) -> Self {
        TextAt { entries, index: name_index(entries), files: HashMap::new() }
    }

    fn text(&self, path: &str) -> Option<&'a str> {
        let entries = self.entries;
        std::str::from_utf8(&entries[*self.index.get(path)?].data).ok()
    }

    /// `path` 文件里从锚点 `frag`（空 = 正文开头）往后的可见文字（去空白），最多取 `max` 个字。锚点不存在返回 `None`。
    fn get(&mut self, path: &str, frag: &str, max: usize) -> Option<String> {
        let t = self.text(path)?;
        let (body, anchors) = self.files.entry(path.to_string()).or_insert_with(|| {
            let mut m = HashMap::new();
            for (id, pos) in html::anchors(t) {
                m.entry(crate::util::xml_unescape(id)).or_insert(pos);
            }
            (html::body_range(t), m)
        });
        let (lo, hi) = (*body)?;
        let pos = if frag.is_empty() { lo } else { (*anchors.get(frag)?).max(lo) };
        // 只看锚点后面一小段（大文件里整段转纯文本太慢）；按字符边界截
        let mut end = (pos + 16 * 1024).min(hi);
        while !t.is_char_boundary(end) {
            end -= 1;
        }
        Some(squash_ws(&plain_text(&t[pos..end])).chars().take(max).collect())
    }

    fn starts_with(&mut self, path: &str, frag: &str, label: &str) -> bool {
        self.get(path, frag, label.chars().count()).is_some_and(|t| t == label)
    }
}

/// 书自带目录（NCX）的条目指错了位置时改指到对的地方（2026-09-28：《占星术杀人魔法》NCX 整体错位，点"第一章"跳进登场人物表；
/// 《福尔摩斯探案全集》几条指进别的章节正文、一条指在上一个文件末尾）。**只改能核实的**：
/// - 条目的目标处文字**不是**以条目标题开头（去空白后比），才算指错；
/// - 书里（通常是目录页）有**标题完全相同**的链接，它指的地方文字以这个标题开头、且这样的地方只有一处 → 改指过去；
/// - 或者全书只有一个文字完全相同的 `<h1>`–`<h6>` 标题（有 id、或就在文件开头）→ 改指这个标题；
/// - 或者条目的锚点在文件末尾（后面没有文字）、下一个 spine 文件的开头以这个标题开头 → 改指下一个文件。
///
/// 标题与正文写法不同（目录写"Chapter 1"、正文写"一"）核实不了的，一律不动。
pub(super) fn repair_ncx_targets(entries: &mut [Entry], rep: &mut WashReport) {
    let Some(opf) = parse_opf(entries) else { return };
    let Some(ncx_path) = opf.ncx.clone() else { return };
    let Some(ncx_text) = entries.iter().find(|e| e.name == ncx_path).and_then(|e| String::from_utf8(e.data.clone()).ok()) else { return };
    let ncx_dir = dir_of(&ncx_path).to_string();
    // 属性原文 → 字符引用还原 → 百分号解码（锚点表也按还原后的 id 建，见 `TextAt::get`）
    let resolve_decoded = |base: &str, href: &str| -> (String, String) {
        let (path, frag) = crate::epubzip::resolve_link(base, href);
        (path, frag.unwrap_or_default())
    };
    let mut cache = TextAt::new(entries);
    // 书里所有链接：标题 → 目标（同一标题可能有好几个目标，比如每个故事都有"第一章"）
    let mut links: HashMap<String, Vec<(String, String)>> = HashMap::new();
    // 书里的 `<h1>`–`<h6>` 标题：文字 → 能指到它的位置（有 id 指 id；没 id 但在文件开头指文件；都不行记 None）
    let mut headings: HashMap<String, Vec<Option<(String, String)>>> = HashMap::new();
    // 各文件独立扫（多线程，`util::par_map`），按 spine 顺序并起来
    type Target = (String, String);
    // (链接：标题 → 目标, 标题：文字 → 能指到它的位置)
    type FileScan = (Vec<(String, Target)>, Vec<(String, Option<Target>)>);
    let per_file = crate::util::par_map(&opf.spine, |path| -> FileScan {
        let (mut file_links, mut file_heads) = (Vec::new(), Vec::new());
        let Some(t) = cache.text(path) else { return (file_links, file_heads) };
        for g in html::tags(t).filter(|g| g.kind == html::TagKind::Open && g.is("a")) {
            let Some(href) = html::attr_value(&t[g.start..g.end], "href") else { continue };
            if href.contains("://") {
                continue;
            }
            let Some(close) = html::find_close(t, g.end, "a") else { continue };
            let label = squash_ws(&plain_text(&t[g.end..close.start]));
            if label.is_empty() || label.chars().count() > 60 {
                continue;
            }
            file_links.push((label, resolve_decoded(path, href)));
        }
        let Some((lo, _)) = html::body_range(t) else { return (file_links, file_heads) };
        for g in html::tags(t).filter(|g| g.kind == html::TagKind::Open && g.start >= lo && g.heading_level().is_some()) {
            let Some(close) = html::find_close(t, g.end, g.name) else { continue };
            let label = squash_ws(&plain_text(&t[g.end..close.start]));
            if label.is_empty() {
                continue;
            }
            let target = match html::attr_value(&t[g.start..g.end], "id").filter(|id| !id.is_empty()) {
                Some(id) => Some((path.clone(), crate::util::xml_unescape(id).into_owned())),
                None => (!html::has_visible(&t[lo..g.start])).then(|| (path.clone(), String::new())),
            };
            file_heads.push((label, target));
        }
        (file_links, file_heads)
    });
    for (file_links, file_heads) in per_file {
        for (label, target) in file_links {
            let v = links.entry(label).or_default();
            if !v.contains(&target) {
                v.push(target);
            }
        }
        for (label, target) in file_heads {
            headings.entry(label).or_default().push(target);
        }
    }
    let spine_pos: HashMap<&str, usize> = opf.spine.iter().enumerate().map(|(i, p)| (p.as_str(), i)).collect();
    let spine_next = |path: &str| spine_pos.get(path).and_then(|&i| opf.spine.get(i + 1)).cloned();

    // 逐条看 NCX，就地改 `<content src>`
    let mut fixed = 0usize;
    let new = crate::ncx::rewrite_content_srcs(&ncx_text, |label, src| {
        let label = squash_ws(label);
        if label.is_empty() {
            return None;
        }
        let (path, frag) = resolve_decoded(&ncx_path, src);
        if cache.starts_with(&path, &frag, &label) {
            return None;
        }
        // 标题完全相同、目标处文字以标题开头的链接；核实得上的目标只有一个才用（有好几个就拿不准）
        let verified: Vec<&(String, String)> = links.get(&label).into_iter().flatten().filter(|(p, f)| cache.starts_with(p, f, &label)).collect();
        let fix = (verified.len() == 1)
            .then(|| verified[0].clone())
            .or_else(|| {
                // 全书唯一一个文字相同的标题
                match headings.get(&label).map(Vec::as_slice) {
                    Some([Some(t)]) => Some(t.clone()),
                    _ => None,
                }
            })
            .or_else(|| {
                // 锚点在文件末尾：下一个文件开头是这个标题
                let at_end = !frag.is_empty() && cache.get(&path, &frag, 1).is_some_and(|t| t.is_empty());
                let next = spine_next(&path)?;
                (at_end && cache.starts_with(&next, "", &label)).then_some((next, String::new()))
            });
        let (p, f) = fix?;
        fixed += 1;
        Some(xml_escape(&crate::epubzip::href_to(&ncx_dir, &p, &f)))
    });
    if let Some(new) = new {
        if let Some(e) = entries.iter_mut().find(|e| e.name == ncx_path) {
            e.data = new.into_bytes();
        }
        rep.ncx_targets_repaired += fixed;
    }
}

#[cfg(test)]
mod collection_tests {
    use super::collection_depths;

    /// 福尔摩斯全集的平排目录：册 → 作品 → 部 → 章；短篇平排在册下面；题献挂在作品下面。
    #[test]
    fn flat_collection_gets_volume_work_part_levels() {
        let t = [
            "出版说明", "第一册", "书名页", "暗红习作", "第一部", "第一章 歇洛克·福尔摩斯先生", "第二部", "第一章 盐碱之原", "四签名", "第一章 演绎法",
            "第二册", "书名页", "波希米亚丑闻", "红发俱乐部", "第五册", "巴斯克维尔的猎犬", "题　献", "第一章 歇洛克·福尔摩斯先生", "恐怖谷", "第一部 伯尔斯通惨剧", "第一章 警　讯",
        ];
        assert_eq!(collection_depths(&t).unwrap(), [1, 1, 2, 2, 3, 4, 3, 4, 2, 3, 1, 2, 2, 2, 1, 2, 3, 3, 2, 3, 4]);
    }

    /// 普通小说（编号只从头开始一次、没有册）不当合集：交给按「第X部」重建。
    #[test]
    fn single_novel_is_not_a_collection() {
        assert_eq!(collection_depths(&["序章", "第一部", "第一章", "第二章", "第二部", "第一章"]), None);
        assert_eq!(collection_depths(&["第一部 01 雪人", "02 大雪", "第二部 03 雪地"]), None);
    }
}
