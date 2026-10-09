//! 章节与目录：定出全书的书/卷、章、节，让目录**至少索引到节**。**不拆文件**：原书的 XHTML 文件结构原样保留
//! （2026-10-06 用户定：章节不再强制分页，原书怎样就怎样；目录维持原来的层级、补节、改指）。
//!
//! 书自带目录用得上时，按**目录层级**定书/卷、章、节（[`toc_driven`]，用户 2026-10-05：合集和普通书统一）；否则
//! 标题角色按全书实际用到的 `<h1>`–`<h6>` 级别判定（[`classify`]）：
//! - **Title**（书/卷、章）：最浅一级；若最浅一级是"第X部/卷"或全书只出现一次（书名），或下一级多为"第X章/Chapter"，
//!   则下一级也算 Title。
//! - **Subtitle**：紧跟在 Title 后面、中间没有正文的那一级（如 `<h1>第一章</h1><h2>风起</h2>`），算章标题的一部分。
//! - **Section**（节）：Title/Subtitle 之后的下一级。目录漏掉的节补进目录（`toc::merge_sections_into_toc`）。
//! - 更深的标题不进目录。
//!
//! 目录要改指的：只指到文件、标题却在文件中间的章（好读的书名页 + 「第一章」段落）补 id、目录改指到 `文件#id`；
//! 补进目录的节没有 id 的补 `eink-sec-N`。标题外面只包着它自己的元素（`<div class="title"><h2>…</h2></div>`）整体算标题。
//!
//! 锚点：`id`（任何元素、两种引号）与 `<a name>` 都算；链接里的锚点先百分号解码再对（NCX 常写 `#%E6%B3%A8`）。
//!
//! 保守：漫画书、目录页（链接文字占大半）、解析不了 `<body>` 的文件不动。幂等：再跑一遍不会再补。

use super::*;
use crate::html::{has_visible, parse_spans, Span};

// ───────────────────────── 标题角色 ─────────────────────────

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Role {
    Title,
    Subtitle,
    Section,
    Other,
}

/// 章节标题里「第X部」「第X章」的 X 能用的字（正则字符类的内容）：阿拉伯数字（半角、全角）和中文数字。定章节和目录重建
/// （`toc.rs` 的分部前缀、合集目录）共用这一份。
pub(super) const CN_NUM: &str = "0-9０-９〇零一二三四五六七八九十百千两";
/// 「第X部」这一级的量词（正则字符类的内容）。
pub(super) const PART_KINDS: &str = "部卷篇辑编";

fn part_like(t: &str) -> bool {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(&format!(r#"(?i)^\s*(第[{CN_NUM}]+\s*[{PART_KINDS}]|(part|book|volume)\b)"#)).unwrap()).is_match(t)
}

/// 「第X册」「上册」——合集的装订分册。
fn volume_like(t: &str) -> bool {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(&format!(r#"^\s*(第[{CN_NUM}]+\s*册|[上中下]\s*册)\s*$"#)).unwrap()).is_match(t)
}

fn chapter_like(t: &str) -> bool {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(&format!(r#"(?i)^\s*(第[{CN_NUM}]+\s*[章回]|chapter\b|(序|序言|序章|前言|引言|引子|楔子|尾声|后记|跋)(\s|$)|(prologue|epilogue|preface|introduction|afterword)\b)"#)).unwrap()
    })
    .is_match(t)
}

fn section_like(t: &str) -> bool {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(&format!(r#"^\s*第[{CN_NUM}]+\s*节"#)).unwrap()).is_match(t)
}

/// 一个要找章节标题的 spine 文件。
struct FileInfo {
    idx: usize,
    path: String,
    html: String,
    /// `<body>` 内容的范围。
    lo: usize,
    hi: usize,
    spans: Vec<Span>,
    heads: Vec<Heading>,
    /// 目录驱动认出的标题块：标题元素下标 → 范围（见 [`Extent`]）。重新解析后按元素下标还原范围。
    extents: HashMap<usize, Extent>,
}

/// 由几个元素组成的标题块（《绍宋》：装饰图 + 「第一章」 + 「明道宫」）。
#[derive(Clone, Copy, Debug)]
struct Extent {
    /// 标题块开头的元素（装饰图在前时是图所在的元素）；`None` 表示从 `<body>` 开头算起。
    first: Option<usize>,
    /// 标题块最后一个元素。
    last: usize,
}

impl FileInfo {
    /// 在现有标题之外再认几个元素为标题（按文档序重排后重算）。
    fn add_headings(&mut self, extra: &[(usize, u8)]) {
        let mut c: Vec<(usize, u8)> = self.heads.iter().map(|h| (h.span, h.level)).chain(extra.iter().copied()).collect();
        sort_candidates(&mut c, &self.spans);
        self.heads = collect_headings_ext(&self.html, &self.spans, &c, &self.extents, self.lo);
    }

    /// 给这些元素（下标）取 id：已有的用原来的，没有的补 `{prefix}-N`（N 从 `counter` 往上数、文件里没有这个锚点）。
    /// `id=""` 的给 `None`（不补也不改，拿不准）。补了就重新解析；插入属性不改变元素结构，元素下标和标题都按原样对回。
    fn ensure_ids(&mut self, elems: &[usize], prefix: &str, counter: &mut usize) -> Vec<Option<String>> {
        // 文件里已有的锚点（id 与 `<a name>`）只收集一次，补的 id 一趟插完（此前每补一个都整份查找、逐个插入）。
        let taken: HashSet<&str> = html::anchors(&self.html).into_iter().map(|(v, _)| v).collect();
        let mut inserts: Vec<(usize, usize, String)> = Vec::new();
        let ids = elems
            .iter()
            .map(|&i| {
                let sp = &self.spans[i];
                match html::attr(&self.html[sp.open_start..sp.open_end], "id") {
                    Some(a) if a.value.is_empty() => None,
                    Some(a) => Some(a.value.to_string()),
                    None => Some(loop {
                        *counter += 1;
                        let id = format!("{prefix}-{counter}");
                        if !taken.contains(id.as_str()) {
                            let at = sp.open_start + 1 + sp.name.len();
                            inserts.push((at, at, format!(" id=\"{id}\"")));
                            break id;
                        }
                    }),
                }
            })
            .collect();
        if !inserts.is_empty() {
            inserts.sort_by_key(|x| x.0);
            self.html = html::apply_edits(&self.html, inserts);
            let (lo, hi) = html::body_range(&self.html).expect("插入 id 不影响 body 边界");
            self.lo = lo;
            self.hi = hi;
            self.spans = parse_spans(&self.html, lo, hi);
            let cands: Vec<(usize, u8)> = self.heads.iter().map(|h| (h.span, h.level)).collect();
            self.heads = collect_headings_ext(&self.html, &self.spans, &cands, &self.extents, self.lo);
        }
        ids
    }
}

/// NCX 里只指到文件（没有锚点）、标签是给定文字的条目，改指到 `文件#id`。`targets` = [(文件 zip 路径, 标签, id)]。
fn retarget_ncx_to_ids(entries: &mut [Entry], opf: &Opf, targets: &[(String, String, String)]) {
    let Some(ncx) = opf.ncx.clone() else { return };
    let Some(e) = entries.iter_mut().find(|e| e.name == ncx) else { return };
    // 不是 UTF-8 的 NCX 不改（按 lossy 转出的文字写回会把原字节改坏）
    let Ok(text) = std::str::from_utf8(&e.data) else { return };
    let new = crate::ncx::rewrite_content_srcs(text, |label, src| {
        // src 是属性原文（`resolve_link` 解析时还原字符引用）；拼新值时原文照用。
        let (path, frag) = crate::epubzip::resolve_link(&ncx, src);
        if frag.is_some() {
            return None;
        }
        let label = squash_ws(label);
        let (_, _, id) = targets.iter().find(|(f, l, _)| *f == path && squash_ws(l) == label)?;
        // id 也是属性原文（`ensure_ids` 取的）：还原后再转义一次，不会转两遍（2026-09-30 审计：`a&amp;b` 曾写成 `a&amp;amp;b`）。
        Some(format!("{src}#{}", crate::util::xml_escape(&crate::util::xml_unescape(id))))
    });
    if let Some(new) = new {
        e.data = new.into_bytes();
    }
}

/// 一个标题（位置已扩到只包着它的元素）。
struct Heading {
    level: u8,
    text: String,
    start: usize,
    end: usize,
    /// 解析出的 `<hN>` 元素下标（扩展包裹元素时从它往上找）。
    span: usize,
    /// 和前一个标题之间没有可见内容。
    adjacent_to_prev: bool,
}

/// 从 `node` 往上，只要父元素除了 `[s, e)` 之外没有可见内容，就把范围扩到父元素。
fn expand_range(html: &str, spans: &[Span], node: usize, mut s: usize, mut e: usize) -> (usize, usize) {
    let mut cur = node;
    while let Some(p) = spans[cur].parent {
        let ps = &spans[p];
        if ps.open_start >= s && ps.close_end <= e {
            cur = p; // 已经在范围里的包裹元素（单个标题先扩过一次）
            continue;
        }
        // 范围已经越出父元素的内容（目录驱动的标题块从 `<body>` 开头算起）：不再往上扩。
        if ps.open_end > s || ps.close_start < e || has_visible(&html[ps.open_end..s]) || has_visible(&html[e..ps.close_start]) {
            break;
        }
        s = ps.open_start;
        e = ps.close_end;
        cur = p;
    }
    (s, e)
}

/// `<h1>`–`<h6>` 标题：(元素下标, 级别)。
fn h_candidates(html: &str, spans: &[Span]) -> Vec<(usize, u8)> {
    spans
        .iter()
        .enumerate()
        .filter(|(_, sp)| sp.closed() && !hidden(&html[sp.open_start..sp.open_end]))
        .filter_map(|(i, sp)| sp.heading_level().map(|l| (i, l)))
        .collect()
}

/// 开始标签自己写着不显示（`style="display:none"` 或 `hidden` 属性）。这种标题只给目录定位用（《绍宋》每章开头的
/// `<h2 style="display:none;">`，看得见的章名是后面的图和段落），不当章标题（分页时代切了每章前面多一页空白）。
fn hidden(open_tag: &str) -> bool {
    html::attr(open_tag, "hidden").is_some()
        || html::attr_value(open_tag, "style").is_some_and(|st| {
            html::css_decls(st).iter().any(|d| d.prop.eq_ignore_ascii_case("display") && d.value.trim_end_matches("!important").trim().eq_ignore_ascii_case("none"))
        })
}

/// 全书没有 `<hN>` 时的退路：目录（NCX）指向的短段落（`<p>`/`<div>`，≤60 字）当标题，目录层级当级别。
/// 锚点可以在段落自己身上，也可以是段落前面紧挨着的空元素（`<span id="filepos…"></span>`）。
/// 目录只指到文件、不带锚点（`frag` 为空）时取该文件第一个有文字的段落。
/// Calibre 转出的中文书常把章名写成 `<p class="block_7">緣起首回…</p>`，只能靠目录认出来。
fn toc_candidates(html: &str, spans: &[Span], targets: &[TocTarget]) -> Vec<(usize, u8)> {
    let mut out = Vec::new();
    for t in targets {
        // 目录锚点是字符引用还原后的值（`ncx::NavPoint::src`），id 属性是原文：还原后比。
        let has_id = |sp: &Span| html::attr_value(&html[sp.open_start..sp.open_end], "id").is_some_and(|v| crate::util::xml_unescape(v) == t.frag.as_str());
        let hit = if t.frag.is_empty() {
            spans.iter().position(|sp| leaf_block(html, sp) && has_visible(&html[sp.open_end..sp.close_start]))
        } else {
            spans.iter().position(|sp| matches!(sp.name.as_str(), "p" | "div") && sp.closed() && has_id(sp)).or_else(|| {
                // 锚点是标题段落前面的空元素（MOBI 转来的书：`<span id="filepos…"></span><p><b>第一章</b></p>`）：
                // 取紧跟着的、中间没有别的可见内容的段落。
                let a = spans.iter().position(|sp| sp.closed() && has_id(sp) && !has_visible(&html[sp.open_end..sp.close_start]))?;
                let end = spans[a].close_end;
                let i = spans.iter().position(|sp| sp.open_start >= end && leaf_block(html, sp) && has_visible(&html[sp.open_end..sp.close_start]))?;
                (!has_visible(&html[end..spans[i].open_start])).then_some(i)
            })
        };
        if let Some(i) = hit {
            let n = plain_text(&html[spans[i].open_end..spans[i].close_start]).chars().count();
            if (1..=60).contains(&n) {
                out.push((i, t.depth.clamp(1, 6)));
            }
        }
    }
    sort_candidates(&mut out, spans);
    out
}

fn sort_candidates(c: &mut Vec<(usize, u8)>, spans: &[Span]) {
    c.sort_by_key(|&(i, _)| spans[i].open_start);
    c.dedup_by_key(|x| x.0);
}

/// 不含嵌套块的 `<p>`/`<div>`（一个"段落"）。
fn leaf_block(html: &str, sp: &Span) -> bool {
    matches!(sp.name.as_str(), "p" | "div") && sp.closed() && {
        let inner = &html[sp.open_end..sp.close_start];
        // 按标签名认（`<pre>`、`<param>` 不算，`<P>` 算）
        !html::tags(inner).any(|t| t.is_start() && (t.is("p") || t.is("div")))
    }
}

/// NCX 里指向某个文件的一条目录。
struct TocTarget {
    /// 锚点（字符引用已还原、百分号已解码；只指到文件时为空）。
    frag: String,
    depth: u8,
    label: String,
}

/// NCX 目录：zip 路径 → 指向它的目录条目。
fn toc_targets(entries: &[Entry], opf: &Opf) -> HashMap<String, Vec<TocTarget>> {
    let mut map: HashMap<String, Vec<TocTarget>> = HashMap::new();
    let Some(ncx) = opf.ncx.as_ref() else { return map };
    let Some(e) = entries.iter().find(|e| &e.name == ncx) else { return map };
    let text = String::from_utf8_lossy(&e.data);
    for (depth, label, target) in crate::ncx::parse_ncx_flat(&text) {
        let (path, frag) = crate::epubzip::resolve_href(ncx, &target);
        map.entry(path).or_default().push(TocTarget { frag: html::frag_id(frag.unwrap_or("")).into_owned(), depth: depth.min(6) as u8, label });
    }
    map
}


/// 章名写成普通段落的书（好读：第一个正文文件的 `<h3>` 是书名，"第一章"只是一行字）：目录只指到文件、标签跟文件里某个
/// 短段落的文字一样、而该文件的 `<hN>` 里没有这个文字时，把这个段落当作跟"目录认得出的其它章标题"同一级的标题候选。
/// 级别取目录标签对得上的 `<hN>` 里最常见的那一级；一个都对不上就不补（拿不准）。返回 (文件下标, 候选)。
fn toc_label_paragraphs(files: &[FileInfo], targets: &HashMap<String, Vec<TocTarget>>) -> Vec<(usize, (usize, u8))> {
    let labels: HashSet<String> = targets.values().flatten().map(|t| squash_ws(&t.label)).filter(|l| !l.is_empty()).collect();
    let mut count = [0usize; 7];
    for h in files.iter().flat_map(|f| f.heads.iter()).filter(|h| labels.contains(&squash_ws(&h.text))) {
        count[h.level as usize] += 1;
    }
    let Some(level) = (1..=6u8).filter(|&l| count[l as usize] > 0).max_by_key(|&l| (count[l as usize], std::cmp::Reverse(l))) else { return Vec::new() };
    let mut out = Vec::new();
    for (fi, f) in files.iter().enumerate() {
        for t in targets.get(&f.path).into_iter().flatten().filter(|t| t.frag.is_empty()) {
            let label = squash_ws(&t.label);
            if label.is_empty() || label.chars().count() > 60 || f.heads.iter().any(|h| squash_ws(&h.text) == label) {
                continue;
            }
            let hit = f.spans.iter().position(|sp| sp.open_start >= f.lo && leaf_block(&f.html, sp) && squash_ws(&plain_text(&f.html[sp.open_end..sp.close_start])) == label);
            if let Some(i) = hit {
                out.push((fi, (i, level)));
            }
        }
    }
    out
}

/// 独占一段的节号的值：1–3 位阿拉伯/全角数字，或一…九十九的中文数字。
pub(super) fn section_number(t: &str) -> Option<u32> {
    let t = squash_ws(t);
    let cs: Vec<char> = t.chars().collect();
    if cs.is_empty() || cs.len() > 3 {
        return None;
    }
    let arabic = |c: char| c.to_digit(10).or_else(|| ('０'..='９').contains(&c).then(|| c as u32 - '０' as u32));
    if cs.iter().all(|&c| arabic(c).is_some()) {
        return cs.iter().try_fold(0u32, |acc, &c| Some(acc * 10 + arabic(c)?));
    }
    let digit = |c: char| "一二三四五六七八九".chars().position(|x| x == c).map(|i| i as u32 + 1);
    match cs[..] {
        ['十'] => Some(10),
        [a] => digit(a),
        ['十', b] => Some(10 + digit(b)?),
        [a, '十'] => Some(digit(a)? * 10).filter(|&v| v >= 20),
        [a, '十', b] => Some(digit(a)? * 10 + digit(b)?).filter(|&v| v >= 20),
        _ => None,
    }
}

/// 目录里没有、靠 `<hN>` 补认的节标题最长这么多字（再长多半是用标题标签排的注释、引文）。
const SECTION_TITLE_MAX_CHARS: usize = 40;

/// 全书里的一个元素：(文件下标, 元素下标)。
type ElemRef = (usize, usize);

fn collect_headings(html: &str, spans: &[Span], candidates: &[(usize, u8)]) -> Vec<Heading> {
    collect_headings_ext(html, spans, candidates, &HashMap::new(), 0)
}

/// 同 [`collect_headings`]；`ext` 里有的标题按标题块范围算（`lo` 是 `<body>` 内容开头）。
fn collect_headings_ext(html: &str, spans: &[Span], candidates: &[(usize, u8)], ext: &HashMap<usize, Extent>, lo: usize) -> Vec<Heading> {
    let mut out: Vec<Heading> = Vec::new();
    for &(i, level) in candidates {
        let sp = &spans[i];
        let e = ext.get(&i);
        let text = match e {
            Some(e) => plain_text(&html[e.first.map_or(lo, |f| spans[f].open_start)..spans[e.last].close_end]),
            None => plain_text(&html[sp.open_end..sp.close_start]),
        };
        if text.is_empty() {
            continue;
        }
        let (start, end) = match e {
            Some(e) => {
                let s0 = e.first.map_or(lo, |f| spans[f].open_start);
                (s0, spans[e.last].close_end)
            }
            None => expand_range(html, spans, i, sp.open_start, sp.close_end),
        };
        let adjacent_to_prev = match out.last() {
            Some(prev) => prev.end <= start && !has_visible(&html[prev.end..start]),
            None => false,
        };
        out.push(Heading { level, text, start, end, span: i, adjacent_to_prev });
    }
    out
}

// ───────────────────────── 目录驱动 ─────────────────────────

/// 比较标题文字用：去掉空白（含全角空格、零宽字符）。
fn title_key(t: &str) -> String {
    t.chars().filter(|c| !c.is_whitespace() && *c != '\u{3000}' && *c != '\u{200b}' && *c != '\u{feff}').collect()
}

/// 哪一层目录是「章」（用户 2026-10-05）：最上一层是一本本书或部、卷（下面挂着章）时是第二层，否则是第一层。
/// 最上一层算书/卷级：带子条目的至少两条（只有一条时要像「第X部/卷」），并且①带子条目的多数像「第X卷/部」，或②最上一层多数不像
/// 「第X章/回」、序、后记这类章名（阿加莎全集：书名 → 「1」「2」；《揭露人性》：书 → 「事件之章」 → 手记）。
/// **只看层级，不看下一层是不是数字**（2026-10-08 用户定：有的书「1」「2」「3」是章，纯数字不能当节的依据）。
/// 最上一层带子条目的多数是「第X册」时（福尔摩斯全集：册 → 作品 → 部 → 章），册只是装订单位、不会是章：去掉这一层再判断。
fn chapter_depth(flat: &[(usize, String, String)]) -> usize {
    let Some(top) = flat.iter().map(|x| x.0).min() else { return 1 };
    let has_child = |i: usize| flat.get(i + 1).is_some_and(|n| n.0 > flat[i].0);
    let parents: Vec<usize> = (0..flat.len()).filter(|&i| flat[i].0 == top && has_child(i)).collect();
    if parents.len() >= 2 && parents.iter().filter(|&&i| volume_like(&flat[i].1)).count() * 2 > parents.len() {
        let inner: Vec<(usize, String, String)> = flat.iter().filter(|x| x.0 > top).cloned().collect();
        return chapter_depth(&inner);
    }
    // 只有一个带子条目的：它像「第X部/卷」才算书/卷级（只出了一部的书），否则当章。
    if parents.is_empty() || (parents.len() == 1 && !part_like(&flat[parents[0]].1)) {
        return top;
    }
    let majority = |labels: &[&str], f: &dyn Fn(&str) -> bool| !labels.is_empty() && labels.iter().filter(|l| f(l)).count() * 2 > labels.len();
    let parent_labels: Vec<&str> = parents.iter().map(|&i| flat[i].1.as_str()).collect();
    let top_labels: Vec<&str> = flat.iter().filter(|x| x.0 == top).map(|x| x.1.as_str()).collect();
    if majority(&parent_labels, &part_like) || !majority(&top_labels, &chapter_like) {
        top + 1
    } else {
        top
    }
}

/// 在元素 `i` 自己或它的祖先上写着不显示。
fn in_hidden(html: &str, spans: &[Span], mut i: usize) -> bool {
    loop {
        if hidden(&html[spans[i].open_start..spans[i].open_end]) {
            return true;
        }
        match spans[i].parent {
            Some(p) => i = p,
            None => return false,
        }
    }
}

/// 目录条目在文件里的标题块：(标题元素下标, 范围)。从锚点（没有锚点就从文件开头）往后找标题元素或段落，
/// 一个接一个拼文字，正好拼成目录标签就是标题块（《绍宋》：「第一章」+「明道宫」= 「第一章 明道宫」）；拼不成时
/// 第一个是 `<hN>` 或不超过 60 字就只取它。从文件开头找时，标题前面只隔着装饰图的，图也算进标题块。
fn locate_title(f: &FileInfo, ids: &HashMap<String, usize>, frag: &str, label: &str) -> Option<(usize, Extent)> {
    let want = title_key(label);
    if frag.is_empty() {
        // 只指到文件：从开头拼得上最好；拼不上时文件里别处有完全对得上的（好读：开头是书名页，「第一章」是后面一段），取它。
        let from_start = title_from(f, f.lo, &want, true)?;
        if from_start.2 {
            return Some((from_start.0, from_start.1));
        }
        for (i, sp) in f.spans.iter().enumerate() {
            if sp.open_start <= f.spans[from_start.0].open_start || !sp.closed() || !(sp.heading_level().is_some() || leaf_block(&f.html, sp)) {
                continue;
            }
            if let Some((k, e, true)) = title_from(f, sp.open_start, &want, false) {
                if k == i {
                    return Some((k, e));
                }
            }
        }
        return Some((from_start.0, from_start.1));
    }
    let start = f.spans[*ids.get(frag)?].open_start;
    title_from(f, start, &want, false).map(|(k, e, _)| (k, e))
}

/// 文件里各元素的锚点名 → 第一个带它的元素（下标）：`id`，`<a>` 没有 `id` 时看 `name`（字符引用还原）。[`locate_title`] 按锚点找
/// 元素用（以前每个目录条目都把全文件的元素挨个解析一遍属性，几千条目录的合集是平方级）。
fn anchor_index(f: &FileInfo) -> HashMap<String, usize> {
    let mut m = HashMap::new();
    for (i, sp) in f.spans.iter().enumerate() {
        let open = &f.html[sp.open_start..sp.open_end];
        if let Some(v) = html::attr_value(open, "id").or_else(|| (sp.name == "a").then(|| html::attr_value(open, "name")).flatten()) {
            m.entry(crate::util::xml_unescape(v).into_owned()).or_insert(i);
        }
    }
    m
}

/// 从 `start` 往后拼标题块（见 [`locate_title`]）。返回 (标题元素, 范围, 是否正好拼成目录标签)。
/// `file_start`：从文件开头找（标题前面的装饰图算进标题块，范围从 `<body>` 开头算）。
fn title_from(f: &FileInfo, start: usize, want: &str, file_start: bool) -> Option<(usize, Extent, bool)> {
    let (html, spans) = (&f.html, &f.spans);
    let (mut first_text, mut first_media, mut last): (Option<usize>, Option<usize>, Option<usize>) = (None, None, None);
    let mut acc = String::new();
    let mut taken_end = start;
    // 元素按开标签位置排着：`start` 之前的一个都不看，直接从这里开始
    let from = spans.partition_point(|sp| sp.open_start < start);
    for (i, sp) in spans.iter().enumerate().skip(from) {
        if sp.open_start < taken_end || !sp.closed() {
            continue;
        }
        let is_block = sp.heading_level().is_some() || leaf_block(html, sp);
        if !is_block || in_hidden(html, spans, i) {
            continue;
        }
        let inner = &html[sp.open_end..sp.close_start];
        let text = title_key(&plain_text(inner));
        if text.is_empty() {
            if first_text.is_none() && has_visible(inner) {
                first_media.get_or_insert(i);
                taken_end = sp.close_end;
                continue;
            }
            if first_text.is_none() {
                continue;
            }
            break;
        }
        taken_end = sp.close_end;
        first_text.get_or_insert(i);
        acc.push_str(&text);
        last = Some(i);
        if acc == want {
            break;
        }
        if !want.starts_with(acc.as_str()) {
            last = None;
            break;
        }
    }
    let key = first_text?;
    let (last, exact) = match last {
        Some(l) if title_key(&plain_text(&html[spans[key].open_start..spans[l].close_end])) == want => (l, true),
        _ => {
            let n = plain_text(&html[spans[key].open_end..spans[key].close_start]).chars().count();
            if spans[key].heading_level().is_none() && n > 60 {
                return None;
            }
            (key, false)
        }
    };
    let first = if file_start { None } else { Some(first_media.unwrap_or(key)) };
    Some((key, Extent { first, last }, exact))
}

/// 目录驱动的标题与角色（用户 2026-10-05：按目录层级统一）。书/卷级 → 级别 1、章 → 2（章名只是数字也是章，
/// 2026-10-08 用户定）；节（章的下一层）→ 3；更深 → 4（不进目录）。目录里没有、跟在章标题后面
/// 的更深一级 `<hN>` 也算节（以后补进目录）；紧跟在章标题后面、中间没有正文的短 `<hN>` 并进标题块（副标题）。
/// 目录用不上（没有 NCX、指到 spine 文件的条目不到 2 条、六成以上找不到标题）时返回 `None`，退回按 `<hN>` 级别判断。
/// [`toc_driven`] 的结果。
struct TocDriven {
    roles: [Role; 7],
    /// 只指到文件、标题却不在文件开头的（标题元素）：补 id、目录改指过去。
    late: HashSet<ElemRef>,
    /// 来自书自带目录的节（标题元素）：目录里已经有了，不交给"目录补节"（再补会重复）。
    in_toc: HashSet<ElemRef>,
}

/// 一个文件里的目录标题：(标题元素, 级别, 范围, 在章这一层或更上面)。
type TocTitle = (usize, u8, Extent, bool);

fn toc_driven(files: &mut [FileInfo], entries: &[Entry], opf: &Opf) -> Option<TocDriven> {
    let ncx = opf.ncx.as_ref()?;
    let e = entries.iter().find(|e| &e.name == ncx)?;
    let flat = crate::ncx::parse_ncx_flat(&String::from_utf8_lossy(&e.data));
    let chapter = chapter_depth(&flat);
    let file_of: HashMap<&str, usize> = files.iter().enumerate().map(|(i, f)| (f.path.as_str(), i)).collect();
    let (mut total, mut found) = (0usize, 0usize);
    // 只指到文件、标题却不在文件开头的条目（标题元素）：之后补 id、目录改指过去（同 `toc_label_paragraphs`）。
    let mut late: HashSet<(usize, usize)> = HashSet::new();
    // 来自书自带目录的节
    let mut in_toc: HashSet<ElemRef> = HashSet::new();
    // 每个文件的目录标题：(标题元素, 级别, 范围, 在章这一层或更上面)
    let mut per_file: Vec<Vec<TocTitle>> = vec![Vec::new(); files.len()];
    // 章这一层按分支算：章这一层或更深处挂着子条目的「第X部/卷」（福尔摩斯全集《恐怖谷》：书 → 部 → 章），它下面的
    // 章深一层，「部」自己算书/卷级。
    let mut ancestors: Vec<(usize, bool)> = Vec::new(); // (层级, 是挂着子条目的「部」)
    // 各目录条目指到哪个文件、标题块在哪：只读各文件，多线程先算好（`util::par_map`），下面按目录顺序逐条用
    let anchors: Vec<HashMap<String, usize>> = crate::util::par_map(files, anchor_index);
    let located = crate::util::par_map(&flat, |(_, label, target)| {
        let (path, frag) = crate::epubzip::resolve_href(ncx, target);
        let &fi = file_of.get(path.as_str())?;
        let frag = html::frag_id(frag.unwrap_or("")).into_owned();
        let title = locate_title(&files[fi], &anchors[fi], &frag, label);
        Some((fi, frag, title))
    });
    drop(anchors);
    for (ti, ((depth, label, _), located)) in flat.iter().zip(located).enumerate() {
        while ancestors.last().is_some_and(|a| a.0 >= *depth) {
            ancestors.pop();
        }
        let part_here = *depth >= chapter && part_like(label) && flat.get(ti + 1).is_some_and(|n| n.0 > *depth);
        let eff = chapter + ancestors.iter().filter(|a| a.1 && a.0 >= chapter).count();
        ancestors.push((*depth, part_here));
        let Some((fi, frag, title)) = located else { continue };
        total += 1;
        let Some((key, ext)) = title else { continue };
        found += 1;
        if frag.is_empty() && has_visible(&files[fi].html[files[fi].lo..files[fi].spans[key].open_start]) {
            late.insert((fi, key));
        }
        let level: u8 = match depth.cmp(&eff) {
            _ if part_here => 1,
            std::cmp::Ordering::Less => 1,
            std::cmp::Ordering::Equal => 2,
            std::cmp::Ordering::Greater if *depth == eff + 1 => 3,
            std::cmp::Ordering::Greater => 4,
        };
        if level == 3 {
            in_toc.insert((fi, key));
        }
        let v = &mut per_file[fi];
        let top = *depth <= eff;
        match v.iter_mut().find(|x| x.0 == key) {
            // 同一个标题块被几条目录指到（书和它的第一章指到同一处）：留最浅的一级。
            Some(x) if level < x.1 => *x = (key, level, ext, top),
            Some(_) => {}
            None => v.push((key, level, ext, top)),
        }
    }
    if total < 2 || found * 10 < total * 6 {
        return None;
    }
    // 目录里没有的 `<hN>`：先全书收集（紧跟在章标题后面、中间没有正文的记下来），再按 `<hN>` 级别定是副标题还是节——
    // 这一级在全书**每一处**都紧跟在章标题后面才是副标题（金庸的回目），否则是节（《射雕》附录「成吉思汗家族」的
    // `<h2>` 后面紧跟着第一节 `<h4>祖先</h4>`，不能并进标题）。同按 `<hN>` 级别判断时的 [`classify`]。
    struct Extra {
        fi: usize,
        elem: usize,
        level: u8,
        /// 所属目录标题在本文件 toc 里的下标
        owner: usize,
        adjacent: bool,
        section_ok: bool,
    }
    let mut extras: Vec<Extra> = Vec::new();
    for (fi, f) in files.iter().enumerate() {
        let toc = &mut per_file[fi];
        toc.sort_by_key(|x| f.spans[x.0].open_start);
        let range = |e: &Extent| (e.first.map_or(f.lo, |i| f.spans[i].open_start), f.spans[e.last].close_end);
        for (i, l) in h_candidates(&f.html, &f.spans) {
            let sp = &f.spans[i];
            if toc.iter().any(|(_, _, e, _)| {
                let (a, b) = range(e);
                sp.open_start < b && sp.close_end > a
            }) {
                continue;
            }
            // 前面最近的、目录在章这一层或更上面的标题（数字章名虽然和正文同页，也算：《绝叫》「2」后面的证词小标题是节）
            let Some(owner) = toc.iter().rposition(|(k, _, _, top)| *top && f.spans[*k].open_start < sp.open_start) else { continue };
            let (k, lv, e, _) = toc[owner];
            let (_, b) = range(&e);
            let t = plain_text(&f.html[sp.open_end..sp.close_start]);
            let n = t.chars().count();
            let next_toc_after = toc.get(owner + 1).is_none_or(|nx| f.spans[nx.0].open_start > sp.open_start);
            let sec_like = section_number(&t).is_some() || section_like(&t);
            let adjacent = lv <= 2 && b <= sp.open_start && !has_visible(&f.html[b..sp.open_start]) && n <= 60 && next_toc_after && !sec_like;
            // 只收像节标题的：不超过 40 字、不是「注：」开头（金庸《碧血剑》用 `<h5>` 排回末的注释和长引文）。
            let section_ok = f.spans[k].heading_level().is_none_or(|tl| l > tl) && n <= SECTION_TITLE_MAX_CHARS && !super::fonts::looks_like_note(&t);
            extras.push(Extra { fi, elem: i, level: l, owner, adjacent, section_ok });
        }
    }
    let mut subtitle_level = [false; 7];
    for l in 1..=6u8 {
        let at: Vec<&Extra> = extras.iter().filter(|x| x.level == l).collect();
        subtitle_level[l as usize] = !at.is_empty() && at.iter().all(|x| x.adjacent);
    }
    let mut extra_by_file: Vec<Vec<(usize, u8)>> = vec![Vec::new(); files.len()];
    for x in &extras {
        if subtitle_level[x.level as usize] {
            // 副标题：并进章标题块
            per_file[x.fi][x.owner].2.last = x.elem;
        } else if x.section_ok {
            extra_by_file[x.fi].push((x.elem, 3));
        }
    }
    // 各文件独立，多线程做（`util::par_map_mut`）
    let mut work: Vec<_> = files.iter_mut().zip(per_file).zip(extra_by_file).collect();
    crate::util::par_map_mut(&mut work, |((f, toc), extra)| {
        let (toc, extra) = (std::mem::take(toc), std::mem::take(extra));
        f.extents = toc.iter().map(|(k, _, e, _)| (*k, *e)).collect();
        let mut cands: Vec<(usize, u8)> = toc.iter().map(|(k, lv, _, _)| (*k, *lv)).chain(extra).collect();
        sort_candidates(&mut cands, &f.spans);
        f.heads = collect_headings_ext(&f.html, &f.spans, &cands, &f.extents, f.lo);
    });
    let mut roles = [Role::Other; 7];
    roles[1] = Role::Title;
    roles[2] = Role::Title;
    roles[3] = Role::Section;
    Some(TocDriven { roles, late, in_toc })
}

/// 全书标题级别 → 角色。`headings` 是全书（spine 顺序）所有文件的标题。
fn classify(headings: &[&Heading]) -> [Role; 7] {
    let mut roles = [Role::Other; 7];
    let mut levels: Vec<u8> = headings.iter().map(|h| h.level).collect();
    levels.sort_unstable();
    levels.dedup();
    let Some(&r0) = levels.first() else { return roles };
    let at = |l: u8| headings.iter().filter(move |h| h.level == l);
    let majority = |l: u8, f: fn(&str) -> bool| {
        let n = at(l).count();
        n > 0 && at(l).filter(|h| f(&h.text)).count() * 2 > n
    };
    roles[r0 as usize] = Role::Title;
    let mut rest = levels[1..].iter().copied().peekable();
    if let Some(&r1) = rest.peek() {
        let above_chapter = majority(r0, part_like) || (at(r0).count() == 1 && at(r1).count() >= 2);
        if (above_chapter || majority(r1, chapter_like)) && !majority(r1, section_like) {
            roles[r1 as usize] = Role::Title;
            rest.next();
        }
    }
    if let Some(&s) = rest.peek() {
        let subtitle = !majority(s, section_like)
            && headings.iter().enumerate().filter(|(_, h)| h.level == s).all(|(i, h)| h.adjacent_to_prev && i > 0 && roles[headings[i - 1].level as usize] == Role::Title);
        if subtitle {
            roles[s as usize] = Role::Subtitle;
            rest.next();
        }
    }
    if let Some(s) = rest.next() {
        roles[s as usize] = Role::Section;
    }
    roles
}

/// 目录页：链接文字占可见文字一半以上（至少 3 个链接）。这种页不找标题。
fn looks_like_toc_page(html: &str, lo: usize, hi: usize, spans: &[Span]) -> bool {
    let links: Vec<&Span> = spans.iter().filter(|s| s.name == "a" && s.closed() && html::attr(&html[s.open_start..s.open_end], "href").is_some()).collect();
    if links.len() < 3 {
        return false;
    }
    let link_chars: usize = links.iter().map(|s| plain_text(&html[s.open_end..s.close_start]).chars().count()).sum();
    let all_chars = plain_text(&html[lo..hi]).chars().count().max(1);
    link_chars * 2 > all_chars
}

/// 整页看是不是目录页（口径同 [`looks_like_toc_page`]）。优化器收集注释引用时跳过这种页。
pub(crate) fn is_toc_like_page(html: &str) -> bool {
    let Some((lo, hi)) = html::body_range(html) else { return false };
    looks_like_toc_page(html, lo, hi, &parse_spans(html, lo, hi))
}

/// 清洗层入口：定章节、给要进目录的标题补 id、目录改指、把漏掉的节补进目录。不拆文件。
/// `toc_heading`：要新建 nav 时的目录标题（按书的语言）。漫画不做：调用方（`wash_entries`）已判过。
pub(super) fn chapters_into_toc(entries: &mut [Entry], toc_heading: &str, rep: &mut WashReport) {
    let Some(opf) = parse_opf(entries) else { return };
    // 1. 收集 spine 各文件的标题（目录页、导航文件不算）。
    let index = name_index(entries);
    // 各文件独立，多线程扫（`util::par_map`，按 spine 顺序收回）
    let mut files: Vec<FileInfo> = crate::util::par_map(&opf.spine, |path| {
        if Some(path) == opf.nav_doc.as_ref() || is_toc_file(path) {
            return None;
        }
        let &idx = index.get(path.as_str())?;
        let html = std::str::from_utf8(&entries[idx].data).ok()?;
        let (lo, hi) = html::body_range(html)?;
        let spans = parse_spans(html, lo, hi);
        if looks_like_toc_page(html, lo, hi, &spans) {
            return None;
        }
        let heads = collect_headings(html, &spans, &h_candidates(html, &spans));
        Some(FileInfo { idx, path: path.clone(), html: html.to_string(), lo, hi, spans, heads, extents: HashMap::new() })
    })
    .into_iter()
    .flatten()
    .collect();
    // 只信书自带的目录：本次清洗自动生成的目录（`toc_generated > 0`）是按文件/页数凑的，不代表章节结构。
    // 书自带目录用得上时按目录层级定标题和角色（用户 2026-10-05），否则按 `<hN>` 级别判断（下面这一大段）。
    let (toc_roles, mut label_paragraphs, in_toc) = match (rep.toc_generated == 0).then(|| toc_driven(&mut files, entries, &opf)).flatten() {
        Some(t) => (Some(t.roles), t.late, t.in_toc),
        None => (None, HashSet::new(), HashSet::new()), // (文件下标, 元素下标)
    };
    if rep.toc_generated == 0 && toc_roles.is_none() {
        let targets = toc_targets(entries, &opf);
        if files.iter().all(|f| f.heads.is_empty()) {
            for f in files.iter_mut() {
                if let Some(t) = targets.get(&f.path) {
                    f.heads = collect_headings(&f.html, &f.spans, &toc_candidates(&f.html, &f.spans, t));
                }
            }
        } else {
            // 章名是普通段落、但目录里有它（好读的"第一章"）
            for (fi, c) in toc_label_paragraphs(&files, &targets) {
                files[fi].add_headings(&[c]);
                label_paragraphs.insert((fi, c.0));
            }
        }
    }
    let mut roles = match toc_roles {
        Some(r) => r,
        None => {
            let all: Vec<&Heading> = files.iter().flat_map(|f| f.heads.iter()).collect();
            classify(&all)
        }
    };
    if !roles.contains(&Role::Title) {
        return;
    }
    // 节一级的标题一个也没有跟在同文件的章标题后面（只出现在单独成页的版权页、目录页上，如《ABC谋杀案》的
    // h2「版权信息」「目录」），它不是节：节换成更深一级里跟在章标题后面的那一级（《ABC谋杀案》的 h3「1」「2」）。
    if toc_roles.is_none() {
        let follows_title = |level: usize| {
            files.iter().any(|f| {
                let first = f.heads.iter().position(|h| roles[h.level as usize] == Role::Title);
                first.is_some_and(|t| f.heads[t + 1..].iter().any(|h| h.level as usize == level))
            })
        };
        if let Some(sl) = (0..roles.len()).find(|&l| roles[l] == Role::Section) {
            if !follows_title(sl) {
                if let Some(deeper) = (sl + 1..roles.len()).find(|&l| follows_title(l)) {
                    roles[sl] = Role::Other;
                    roles[deeper] = Role::Section;
                }
            }
        }
    }
    // 节标题要进目录：没有 id 的补一个（插入后该文件重新解析，偏移变了）。记下 (原文件, id, 标题文字)。
    // 只收**跟所属章标题在同一个原文件里**的节：单独成文件、前面没有章标题的"节"多半是附页（内容简介、版权声明），
    // 作者目录没列它就不补（《疯探》）。
    let mut sections: Vec<SectionRef> = Vec::new();
    let mut touched: HashSet<usize> = HashSet::new();
    let (mut ch_no, mut sec_no) = (0usize, 0usize);
    // 目录认出的段落章名前面还有别的内容（好读第一个正文文件：书名页在前）时，目录原来只指到文件、会落在书名页上：
    // 给段落补 id、目录改指到 `文件#id`。
    let mut retarget: Vec<(String, String, String)> = Vec::new(); // (文件, 目录标签, id)
    for (fi, f) in files.iter_mut().enumerate() {
        let late: Vec<(usize, String)> = f
            .heads
            .iter()
            .filter(|h| label_paragraphs.contains(&(fi, h.span)) && has_visible(&f.html[f.lo..h.start]))
            .map(|h| (h.span, h.text.clone()))
            .collect();
        if late.is_empty() {
            continue;
        }
        let ids = f.ensure_ids(&late.iter().map(|x| x.0).collect::<Vec<_>>(), "eink-ch", &mut ch_no);
        for ((_, label), id) in late.into_iter().zip(ids) {
            if let Some(id) = id {
                retarget.push((f.path.clone(), label, id));
            }
        }
        touched.insert(fi);
    }
    if !retarget.is_empty() {
        retarget_ncx_to_ids(entries, &opf, &retarget);
    }
    for (fi, f) in files.iter_mut().enumerate() {
        let first_title = f.heads.iter().position(|h| roles[h.level as usize] == Role::Title);
        let secs: Vec<usize> = f
            .heads
            .iter()
            .enumerate()
            // 目录驱动时节都来自书自带的目录（或跟在同文件的章标题后面）。
            .filter(|&(hi, h)| {
                roles[h.level as usize] == Role::Section
                    && !in_toc.contains(&(fi, h.span))
                    && (toc_roles.is_some() || first_title.is_some_and(|t| t < hi))
            })
            .map(|(hi, _)| hi)
            .collect();
        if secs.is_empty() {
            continue;
        }
        let ids = f.ensure_ids(&secs.iter().map(|&hi| f.heads[hi].span).collect::<Vec<_>>(), "eink-sec", &mut sec_no);
        let labels: Vec<String> = secs.iter().map(|&hi| f.heads[hi].text.clone()).collect();
        for (label, id) in labels.into_iter().zip(ids) {
            if let Some(id) = id {
                sections.push(SectionRef { path: f.path.clone(), id, label });
            }
        }
        touched.insert(fi);
    }
    // 补了 id 的文件写回。
    for fi in &touched {
        let f = &files[*fi];
        entries[f.idx].data = f.html.clone().into_bytes();
    }
    rep.toc_sections_added += merge_sections_into_toc(entries, &sections, toc_heading);
}
