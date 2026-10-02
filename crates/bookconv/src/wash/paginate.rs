//! 章节分页（2026-09-27 用户需求，文字书硬性要求）：**章标题独立一页；节标题与正文同页；节与节、节与章之间分页**。
//!
//! 做法是把 spine 里的章节文件按标题**拆成多个文件**（每个文件在所有阅读器里都从新的一页开始），而不是依赖
//! CSS `page-break-*`——后者各阅读器支持不一，xochitl 未验证。
//!
//! 标题角色按全书实际用到的 `<h1>`–`<h6>` 级别判定（[`classify`]）：
//! - **Title**（独立一页）：最浅一级；若最浅一级是"第X部/卷"或全书只出现一次（书名），或下一级多为"第X章/Chapter"，
//!   则下一级也算 Title（部、章各占一页）。
//! - **Subtitle**：紧跟在 Title 后面、中间没有正文的那一级（如 `<h1>第一章</h1><h2>风起</h2>`），与 Title 同页。
//! - **Section**（新起一页，与正文同页）：Title/Subtitle 之后的下一级。
//! - 更深的标题不分页。
//!
//! 拆分时：标题外面只包着它自己的元素（`<div class="title"><h2>…</h2></div>`）整体算标题；切点处仍打开的包裹元素
//! 在前一份补闭合、后一份重新打开（去掉 `id`，id 只留一处）。全书指向被拆文件的链接（正文、目录页、NCX、OPF guide）
//! 都改指到 id 所在的那一份；落在某一份末尾、后面已无内容的空锚点（`<a id="x"></a><h2>…`）改指下一份的开头。
//!
//! 注释：xochitl 正文链接只认同一文件内的 `#锚点`（上游规范 §3 规则 8）。原来同文件的"注释标号 → 章末注释"拆开后会跨文件，
//! 所以**像注释标号的链接**（在 `<sup>` 里、或文字是 `1`/`[1]`/`①`/`*`/`注1` 这类）指向后面某一份里的 `<p>`/`<li>`/`<div>`/`<aside>`
//! 注释块时，把注释块搬到引用它的那一份末尾。锚点可以在块自己身上，也可以是块开头的 `<a id>`/`<a name>`。回链（注释 → 正文）
//! 指向前面，不搬。保守：块里有标题、或文字超过 [`NOTE_MAX_CHARS`]（多半是包住整章的 div）不搬。
//!
//! 锚点：`id`（任何元素、两种引号）与 `<a name>` 都算；链接里的锚点先百分号解码再对（NCX 常写 `#%E6%B3%A8`）。
//!
//! 保守：漫画书、目录页（链接文字占大半）、解析不了 `<body>` 的文件不动；拆不出有内容的两份就不拆。幂等：已拆过的书再跑不会再拆。

use super::*;
use crate::html::{has_visible, last_visible_end, parse_spans, Span};

/// 片段里的文字字数（不含空白）。
fn text_len(fragment: &str) -> usize {
    plain_text(fragment).chars().filter(|c| !c.is_whitespace()).count()
}

/// 标题页后面只跟着这么短的文字（书名页的"作者：某某"之类）时不另起一页。
const TITLE_TAIL_MIN_CHARS: usize = 30;

// ───────────────────────── 标题角色 ─────────────────────────

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Role {
    Title,
    Subtitle,
    Section,
    Other,
}

const CN_NUM: &str = "0-9０-９〇零一二三四五六七八九十百千两";

fn part_like(t: &str) -> bool {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(&format!(r#"(?i)^\s*(第[{CN_NUM}]+\s*[部卷篇辑编]|(part|book|volume)\b)"#)).unwrap()).is_match(t)
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

/// 一个待分页的 spine 文件。
struct FileInfo {
    idx: usize,
    path: String,
    html: String,
    /// `<body>` 内容的范围。
    lo: usize,
    hi: usize,
    spans: Vec<Span>,
    heads: Vec<Heading>,
}

impl FileInfo {
    /// 在现有标题之外再认几个元素为标题（按文档序重排后重算）。
    fn add_headings(&mut self, extra: &[(usize, u8)]) {
        let mut c: Vec<(usize, u8)> = self.heads.iter().map(|h| (h.span, h.level)).chain(extra.iter().copied()).collect();
        sort_candidates(&mut c, &self.spans);
        self.heads = collect_headings(&self.html, &self.spans, &c);
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
            self.heads = collect_headings(&self.html, &self.spans, &cands);
        }
        ids
    }
}

/// NCX 里只指到文件（没有锚点）、标签是给定文字的条目，改指到 `文件#id`。`targets` = [(文件 zip 路径, 标签, id)]。
fn retarget_ncx_to_ids(entries: &mut [Entry], opf: &Opf, targets: &[(String, String, String)]) {
    let Some(ncx) = opf.ncx.clone() else { return };
    let Some(e) = entries.iter_mut().find(|e| e.name == ncx) else { return };
    let text = String::from_utf8_lossy(&e.data).into_owned();
    let new = crate::ncx::rewrite_content_srcs(&text, |label, src| {
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
        if ps.close_start < e || has_visible(&html[ps.open_end..s]) || has_visible(&html[e..ps.close_start]) {
            break;
        }
        s = ps.open_start;
        e = ps.close_end;
        cur = p;
    }
    (s, e)
}

/// `<h1>`–`<h6>` 标题：(元素下标, 级别)。
fn h_candidates(spans: &[Span]) -> Vec<(usize, u8)> {
    spans.iter().enumerate().filter(|(_, sp)| sp.closed()).filter_map(|(i, sp)| sp.heading_level().map(|l| (i, l))).collect()
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
        !inner.contains("<p") && !inner.contains("<div")
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

/// 节号段落之间（以及最后一个之后）至少要有这么多字，才像"一节正文"（排除目录样的一串数字）。
const NUMBERED_SECTION_MIN_CHARS: usize = 50;

/// 节标题只是独占一段的数字（好读：`　　１`、`　　一`）：文件里第一个标题之后、从 1 开始严格连续、至少 2 个、每节都有正文
/// 的这种段落（元素下标）。楼层号（不从 1 开始）、目录样的数字列表（中间没有正文）、中途断号的都不算。
fn numbered_sections(f: &FileInfo) -> Vec<usize> {
    let Some(after) = f.heads.first().map(|h| h.end) else { return Vec::new() };
    let mut seq: Vec<usize> = Vec::new();
    for (i, sp) in f.spans.iter().enumerate().filter(|(_, sp)| sp.open_start >= after && leaf_block(&f.html, sp)) {
        let Some(v) = section_number(&plain_text(&f.html[sp.open_end..sp.close_start])) else { continue };
        if v as usize != seq.len() + 1 {
            return Vec::new();
        }
        seq.push(i);
    }
    if seq.len() < 2 {
        return Vec::new();
    }
    let ends = seq.iter().skip(1).map(|&i| f.spans[i].open_start).chain(std::iter::once(f.hi));
    let enough = seq.iter().zip(ends).all(|(&i, e)| text_len(&f.html[f.spans[i].close_end..e]) >= NUMBERED_SECTION_MIN_CHARS);
    if enough { seq } else { Vec::new() }
}

/// 全书里的一个元素：(文件下标, 元素下标)。
type ElemRef = (usize, usize);

/// `pos` 之后第一个有文字的段落（元素下标），中间不能有别的可见内容。
fn first_block_after(f: &FileInfo, pos: usize) -> Option<usize> {
    let i = f.spans.iter().position(|sp| sp.open_start >= pos && leaf_block(&f.html, sp) && has_visible(&f.html[sp.open_end..sp.close_start]))?;
    (!has_visible(&f.html[pos..f.spans[i].open_start])).then_some(i)
}

/// 和章标题同级、只写节号的标题（《13級階梯》：`<h3>第一章 出獄</h3>` 的文件里第 1 节是单独一段 `１`，后面的文件是
/// `<h3>２</h3>`、`<h3>３</h3>`）：紧跟在一个非数字标题后面、同级、从 1 严格连续编号的标题（或者从 2 开始、而章标题后面
/// 第一段就是 `１`），每节都有正文，一章至少 2 节；**全书至少两章**这样才认——全书只有一串的
/// （《月亮和六便士》`<h3>二</h3>`…`<h3>五十八</h3>` 是章）不算。返回 (要降成节的标题, 补认成节标题的 `１` 段落)，
/// 都是 (文件下标, 元素下标)。
fn numbered_heading_runs(files: &[FileInfo]) -> (Vec<ElemRef>, Vec<ElemRef>) {
    let seq: Vec<(usize, usize)> = files.iter().enumerate().flat_map(|(fi, f)| (0..f.heads.len()).map(move |hi| (fi, hi))).collect();
    let head = |(fi, hi): (usize, usize)| &files[fi].heads[hi];
    // 标题到本文件下一个标题（或正文末尾）之间的字数
    let body_after = |(fi, hi): (usize, usize), from: usize| {
        let f = &files[fi];
        let end = f.heads.get(hi + 1).map_or(f.hi, |n| n.start);
        text_len(&f.html[from.min(end)..end])
    };
    let (mut heads, mut paras) = (Vec::new(), Vec::new());
    let mut runs = 0;
    let mut i = 0;
    while i < seq.len() {
        let ch = head(seq[i]);
        if section_number(&ch.text).is_some() {
            i += 1;
            continue;
        }
        // 第 1 节的 `１`：章标题后面第一段；章标题所在文件后面已经没有内容（分页过的书，`１` 已切到下一份开头）时，
        // 看紧接着的下一个没有标题的文件的开头。
        let (cfi, _) = seq[i];
        let at = if has_visible(&files[cfi].html[ch.end..files[cfi].hi]) {
            Some((cfi, ch.end))
        } else {
            files.get(cfi + 1).filter(|n| n.heads.is_empty()).map(|n| (cfi + 1, n.lo))
        };
        let para1 = at.and_then(|(pfi, pos)| {
            let f = &files[pfi];
            let e = first_block_after(f, pos)?;
            let after = if pfi == cfi { body_after(seq[i], f.spans[e].close_end) } else { text_len(&f.html[f.spans[e].close_end..f.hi]) };
            (section_number(&plain_text(&f.html[f.spans[e].open_end..f.spans[e].close_start])) == Some(1) && after >= NUMBERED_SECTION_MIN_CHARS).then_some((pfi, e))
        });
        let mut expect = if para1.is_some() { 2 } else { 1 };
        let mut j = i + 1;
        while j < seq.len() {
            let h = head(seq[j]);
            if h.level != ch.level || section_number(&h.text) != Some(expect) || body_after(seq[j], h.end) < NUMBERED_SECTION_MIN_CHARS {
                break;
            }
            expect += 1;
            j += 1;
        }
        if expect > 2 {
            runs += 1;
            paras.extend(para1);
            heads.extend(seq[i + 1..j].iter().map(|&(fi, hi)| (fi, files[fi].heads[hi].span)));
        }
        i = j;
    }
    if runs >= 2 { (heads, paras) } else { (Vec::new(), Vec::new()) }
}

fn collect_headings(html: &str, spans: &[Span], candidates: &[(usize, u8)]) -> Vec<Heading> {
    let mut out: Vec<Heading> = Vec::new();
    for &(i, level) in candidates {
        let sp = &spans[i];
        let text = plain_text(&html[sp.open_end..sp.close_start]);
        if text.is_empty() {
            continue;
        }
        let (start, end) = expand_range(html, spans, i, sp.open_start, sp.close_end);
        let adjacent_to_prev = match out.last() {
            Some(prev) => prev.end <= start && !has_visible(&html[prev.end..start]),
            None => false,
        };
        out.push(Heading { level, text, start, end, span: i, adjacent_to_prev });
    }
    out
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

/// 目录页：链接文字占可见文字一半以上（至少 3 个链接）。这种页不拆。
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

// ───────────────────────── 切分 ─────────────────────────

/// 切点（`html` 里的字节偏移，严格落在 body 内），切出的每一份都有文字。
fn cut_points(html: &str, lo: usize, hi: usize, spans: &[Span], hs: &[Heading], roles: &[Role; 7]) -> Vec<usize> {
    let mut cuts: Vec<usize> = Vec::new();
    let mut title_ends: HashSet<usize> = HashSet::new();
    let mut section_starts: HashSet<usize> = HashSet::new();
    // 部标题（第X部/卷/篇）的结尾、以及标题的开头：部标题后面紧挨着章标题时，部、章各占一页
    let mut part_ends: HashSet<usize> = HashSet::new();
    let mut title_starts: HashSet<usize> = HashSet::new();
    let mut i = 0;
    while i < hs.len() {
        match roles[hs[i].level as usize] {
            Role::Title => {
                let (gs, mut ge) = (hs[i].start, hs[i].end);
                let mut j = i + 1;
                while j < hs.len() && roles[hs[j].level as usize] == Role::Subtitle && hs[j].adjacent_to_prev {
                    ge = hs[j].end;
                    j += 1;
                }
                let (gs, ge) = expand_range(html, spans, hs[i].span, gs, ge);
                cuts.push(gs);
                cuts.push(ge);
                title_ends.insert(ge);
                title_starts.insert(gs);
                if part_like(&hs[i].text) {
                    part_ends.insert(ge);
                }
                i = j;
            }
            Role::Section => {
                cuts.push(hs[i].start);
                section_starts.insert(hs[i].start);
                i += 1;
            }
            _ => i += 1,
        }
    }
    cuts.retain(|&c| c > lo && c < hi);
    cuts.sort_unstable();
    cuts.dedup();
    // 各份字数只算一次：切点都在元素边界上，合并两份的字数就是两份相加（此前每删一个切点把全部分段重数一遍，平方级）。
    let bounds: Vec<usize> = std::iter::once(lo).chain(cuts.iter().copied()).chain(std::iter::once(hi)).collect();
    let mut lens: Vec<usize> = bounds.windows(2).map(|w| text_len(&html[w[0]..w[1]])).collect();
    let remove_cut = |cuts: &mut Vec<usize>, lens: &mut Vec<usize>, c: usize| {
        let l = lens.remove(c + 1);
        lens[c] += l;
        cuts.remove(c);
    };
    // 没有文字的一份（空白，或只有装饰图/分隔线）并入前一份；在最前面就并入后一份。图片仍跟着原来的上下文。
    while !cuts.is_empty() {
        let Some(k) = lens.iter().position(|&l| l == 0) else { break };
        remove_cut(&mut cuts, &mut lens, k.saturating_sub(1));
    }
    // 标题页后面只跟着很短的文字（书名页的作者行）：不另起一页。后面紧接着是节标题时照常分页（节再短也是一节）；
    // 部标题后面紧接着章标题时也照常分页——章名再短也是一章（《雪人》`<h3>第二部</h3>` 后面紧跟"10 粉筆"，此前两个挤在一页）。
    let keep = |c: usize| section_starts.contains(&c) || (part_ends.contains(&c) && title_starts.contains(&c));
    while let Some(k) = (0..cuts.len()).find(|&k| title_ends.contains(&cuts[k]) && !keep(cuts[k]) && lens[k + 1] < TITLE_TAIL_MIN_CHARS) {
        remove_cut(&mut cuts, &mut lens, k);
    }
    cuts
}

/// 拆出的一份：`open`（重新打开的包裹元素）+ `body`（原文片段）+ `close`（补闭合）。
struct Piece {
    path: String,
    open: String,
    body: String,
    close: String,
}

/// 偏移 `o` 处仍打开的元素（外层在前）：从 `o` 之前最近开始的元素往上找第一个包住 `o` 的，再取它的祖先链。
fn open_stack_at(spans: &[Span], o: usize) -> Vec<usize> {
    let contains = |s: &Span| !s.void && s.open_end <= o && s.close_start >= o && s.close_end > s.open_end;
    let mut cur = spans.partition_point(|s| s.open_end <= o).checked_sub(1);
    while let Some(i) = cur {
        if contains(&spans[i]) {
            break;
        }
        cur = spans[i].parent;
    }
    let mut chain = Vec::new();
    while let Some(i) = cur {
        chain.push(i);
        cur = spans[i].parent;
    }
    chain.reverse();
    chain
}

fn split_body(html: &str, lo: usize, hi: usize, spans: &[Span], cuts: &[usize]) -> Vec<(String, String, String)> {
    let bounds: Vec<usize> = std::iter::once(lo).chain(cuts.iter().copied()).chain(std::iter::once(hi)).collect();
    (0..bounds.len() - 1)
        .map(|k| {
            let (a, b) = (bounds[k], bounds[k + 1]);
            let open: String = if k == 0 { String::new() } else { open_stack_at(spans, a).iter().map(|&i| html::remove_attr(&html[spans[i].open_start..spans[i].open_end], "id")).collect() };
            let close: String = if k + 1 == bounds.len() - 1 { String::new() } else { open_stack_at(spans, b).iter().rev().map(|&i| format!("</{}>", &html[spans[i].open_start + 1..spans[i].open_start + 1 + spans[i].name.len()])).collect() };
            (open, html[a..b].to_string(), close)
        })
        .collect()
}

/// 链接像注释标号：文字是 `1`/`[1]`/`①`/`*`/`注1` 这类，或在 `<sup>` 里、包着 `<sup>`，或带 noteref/footnote 字样。
/// `sup_parent`：这个 `<a>` 紧挨着包在 `<sup>` 里。
fn marker_like(a_tag_and_inner: &str, inner: &str, sup_parent: bool) -> bool {
    static TEXT: OnceLock<Regex> = OnceLock::new();
    let text = plain_text(inner);
    let text_re = TEXT.get_or_init(|| Regex::new(r#"^[\[\(（〔【<]?\s*(\d{1,4}|[*†‡§]{1,3}|[①-⑳]|[ⅰ-ⅹ]|注\s*\d{0,4}|[a-z])\s*[\]\)）〕】>]?$"#).unwrap());
    let l = a_tag_and_inner.to_ascii_lowercase();
    text_re.is_match(&text) || inner.to_ascii_lowercase().contains("<sup") || sup_parent || l.contains("noteref") || l.contains("footnote")
}

/// 搬注释块的上限：块里的字数超过这么多就不搬（真注释很少超过一页；再长多半是包住整章的 div，搬了会把正文挪走）。
const NOTE_MAX_CHARS: usize = 1500;

/// 一份的解析结果（元素 + 锚点 → 带它的元素下标，文档序）。每份只解析一次：搬走的注释块先记在 [`Removed`] 里，
/// 轮到这一份当"引用方"时才一次删掉、重新解析（此前每搬一条就删一次、整份重新解析，注释多的章是平方级）。
struct PieceIndex {
    spans: Vec<Span>,
    anchors: HashMap<String, Vec<usize>>,
}

impl PieceIndex {
    fn new(body: &str) -> Self {
        let spans = parse_spans(body, 0, body.len());
        let mut anchors: HashMap<String, Vec<usize>> = HashMap::new();
        for (i, s) in spans.iter().enumerate() {
            let open = &body[s.open_start..s.open_end];
            for a in html::attrs(open) {
                if (a.is("id") || (a.is("name") && s.name == "a")) && !a.value.is_empty() {
                    let v = anchors.entry(a.value.to_string()).or_default();
                    if v.last() != Some(&i) {
                        v.push(i);
                    }
                }
            }
        }
        PieceIndex { spans, anchors }
    }
}

/// 一份里已经搬走、还没真删的注释块：起点 → 终点（互不相交；后搬的块包住先搬的，先搬的并进去）。
/// 删掉一个完整的块（开闭标签配对）不改变其余部分的解析结果，所以原来的 [`PieceIndex`] 去掉这些范围里的元素照样能用。
type Removed = std::collections::BTreeMap<usize, usize>;

fn in_removed(removed: &Removed, pos: usize) -> bool {
    removed.range(..=pos).next_back().is_some_and(|(_, &e)| pos < e)
}

/// `body[lo..hi]` 去掉已搬走的块。
fn text_without<'b>(body: &'b str, lo: usize, hi: usize, removed: &Removed) -> Cow<'b, str> {
    let cut: Vec<(usize, usize, String)> = removed.range(lo..hi).map(|(&s, &e)| (s - lo, e.min(hi) - lo, String::new())).collect();
    if cut.is_empty() { Cow::Borrowed(&body[lo..hi]) } else { Cow::Owned(html::apply_edits(&body[lo..hi], cut)) }
}

/// `body` 里带锚点 `id` 的注释块：锚点元素本身是 `<p>`/`<li>`/`<div>`/`<aside>`，或锚点在这种块的最前面（块里锚点之前没有
/// 可见内容）时取最近的这种祖先。块里有标题或字数超过 [`NOTE_MAX_CHARS`] 时不算。
/// 还要确认它**是注释**（2026-09-28 审计：交叉引用"见第<a href="#a12">12</a>条"的文字也像标号，此前会把第 12 条正文段落搬走）：
/// 块、锚点或它们的祖先带注释语义（`class`/`epub:type` 里有 note，`htmlproc::note_semantic`），或者块在文件末尾的注释区
/// （`at_end`：这是最后一份，块后面到文件末尾除了别的注释块（`note_ids` 里的锚点）没有可见内容）。
/// `removed` 里的块当作已经不在（见 [`Removed`]）；`memo` 见 [`only_notes_after`]。
fn note_block(body: &str, ix: &PieceIndex, id: &str, at_end: bool, note_ids: &HashSet<String>, removed: &Removed, memo: &mut Memo) -> Option<usize> {
    let spans = &ix.spans;
    let ai = ix.anchors.get(id)?.iter().copied().find(|&k| !in_removed(removed, spans[k].open_start))?;
    let mut bi = ai;
    loop {
        let s = &spans[bi];
        if matches!(s.name.as_str(), "p" | "li" | "div" | "aside") && s.closed() {
            break;
        }
        let p = s.parent?;
        if has_visible(&text_without(body, spans[p].open_end, spans[ai].open_start, removed)) {
            return None;
        }
        bi = p;
    }
    let b = &spans[bi];
    // 块里的元素紧跟在块后面（文档序）
    let has_heading =
        spans[bi + 1..].iter().take_while(|s| s.open_start < b.close_start).any(|s| s.close_end <= b.close_start && s.heading_level().is_some() && !in_removed(removed, s.open_start));
    if has_heading || text_len(&text_without(body, b.open_end, b.close_start, removed)) > NOTE_MAX_CHARS {
        return None;
    }
    let semantic = {
        let mut cur = Some(ai);
        let mut found = false;
        while let Some(i) = cur {
            if crate::htmlproc::note_semantic(&body[spans[i].open_start..spans[i].open_end]) {
                found = true;
                break;
            }
            cur = spans[i].parent;
        }
        found
    };
    (semantic || (at_end && only_notes_after(body, ix, b.close_end, note_ids, removed, memo))).then_some(bi)
}

/// [`only_notes_after`] 的结果缓存：位置 → 结果。只依赖位置之后的内容，搬走一块时丢掉这块终点之前的。
type Memo = std::collections::BTreeMap<usize, bool>;

/// `body[from..]` 里除了锚点在 `note_ids` 里的注释块（p/li/div/aside）和已搬走的块（`removed`）之外没有可见内容。
///
/// 从 `from` 往后按文档序看下一个块：它前面有可见内容 → 否；它是注释块（或已搬走）→ 跳过整块，从块尾接着看；
/// 不是注释块 → 进到块里接着看（块里的注释块照样算）。每一步的结果都等于下一步的结果，所以一路走过的位置结果相同，
/// 记进 `memo`：按阅读顺序查一条条注释时，后面的注释区只走一遍（此前每条注释都把后面整段重新拼一遍、扫一遍，
/// 块里找第一个 `<a>` 又从头扫全部元素）。
fn only_notes_after(body: &str, ix: &PieceIndex, from: usize, note_ids: &HashSet<String>, removed: &Removed, memo: &mut Memo) -> bool {
    let spans = &ix.spans;
    let is_note = |k: usize, s: &Span| {
        let open = &body[s.open_start..s.open_end];
        let first_anchor = html::attr_value(open, "id").or_else(|| {
            // 锚点在块开头的 `<a id>`/`<a name>`：块里的元素紧跟在块后面（文档序）
            spans[k + 1..].iter().take_while(|c| c.open_start < s.close_start).find(|c| c.close_end <= s.close_start && c.name == "a" && !in_removed(removed, c.open_start)).and_then(|c| {
                let o = &body[c.open_start..c.open_end];
                html::attr_value(o, "id").or_else(|| html::attr_value(o, "name"))
            })
        });
        first_anchor.is_some_and(|a| note_ids.contains(html::frag_id(a).as_ref()))
    };
    let mut chain: Vec<usize> = Vec::new();
    let mut pos = from;
    let val = loop {
        if let Some(&v) = memo.get(&pos) {
            break v;
        }
        chain.push(pos);
        let first = spans.partition_point(|s| s.open_start < pos);
        let next = spans.iter().enumerate().skip(first).find(|(_, s)| s.closed() && matches!(s.name.as_str(), "p" | "li" | "div" | "aside"));
        let Some((k, s)) = next else { break !has_visible(&body[pos..]) };
        if has_visible(&body[pos..s.open_start]) {
            break false;
        }
        pos = if removed.get(&s.open_start) == Some(&s.close_end) || is_note(k, s) { s.close_end } else { s.open_end };
    };
    for p in chain {
        memo.insert(p, val);
    }
    val
}

/// 一份里像注释标号的同文件链接指向的 id（按文档序）。
fn wanted_notes(body: &str, ix: &PieceIndex) -> Vec<String> {
    ix.spans
        .iter()
        .filter(|s| s.name == "a" && s.closed())
        .filter_map(|s| {
            let frag = html::attr_value(&body[s.open_start..s.open_end], "href")?.strip_prefix('#')?;
            let sup_parent = s.parent.is_some_and(|p| ix.spans[p].name == "sup" && !has_visible(&body[ix.spans[p].open_end..s.open_start]));
            marker_like(&body[s.open_start..s.close_end], &body[s.open_end..s.close_start], sup_parent).then(|| html::frag_id(frag).into_owned())
        })
        .collect()
}

/// 把"注释标号 → 后面某一份里的注释块"的注释块搬到引用它的那一份末尾。返回 (搬了几条, 每份是否被搬走过注释块)。
fn relocate_notes(pieces: &mut [Piece]) -> (usize, Vec<bool>) {
    let n = pieces.len();
    let last = n - 1;
    let mut moved = 0;
    let mut emptied = vec![false; n];
    let mut cache: Vec<PieceIndex> = pieces.iter().map(|p| PieceIndex::new(&p.body)).collect();
    // 各份搬走、还没真删的块（轮到这一份当引用方时一次删掉），和 `only_notes_after` 的缓存。
    let mut removed: Vec<Removed> = vec![Removed::new(); n];
    let mut memo: Vec<Memo> = vec![Memo::new(); n];
    // 全文件里所有像注释标号的链接指向的 id（判断"文件末尾注释区"用）。
    let mut note_ids: HashSet<String> = HashSet::new();
    for (p, ix) in pieces.iter().zip(&cache) {
        note_ids.extend(wanted_notes(&p.body, ix));
    }
    for j in 0..n {
        if !removed[j].is_empty() {
            let edits = std::mem::take(&mut removed[j]).into_iter().map(|(s, e)| (s, e, String::new())).collect();
            pieces[j].body = html::apply_edits(&pieces[j].body, edits);
            cache[j] = PieceIndex::new(&pieces[j].body);
            memo[j].clear();
        }
        let wanted = wanted_notes(&pieces[j].body, &cache[j]);
        for id in wanted {
            let found = (j + 1..n).find_map(|i| note_block(&pieces[i].body, &cache[i], &id, i == last, &note_ids, &removed[i], &mut memo[i]).map(|b| (i, b)));
            let Some((i, bi)) = found else { continue };
            let (body, sp, rm) = (&pieces[i].body, &cache[i].spans[bi], &mut removed[i]);
            let inner = text_without(body, sp.open_end, sp.close_start, rm);
            let block = if sp.name == "li" {
                format!("<div{}{}</div>", &body[sp.open_start + 3..sp.open_end], inner)
            } else {
                format!("{}{}{}", &body[sp.open_start..sp.open_end], inner, &body[sp.close_start..sp.close_end])
            };
            // 记下要删的范围：块里先搬走的并进来
            let (s, e) = (sp.open_start, sp.close_end);
            let inside: Vec<usize> = rm.range(s..e).map(|(&k, _)| k).collect();
            for k in inside {
                rm.remove(&k);
            }
            rm.insert(s, e);
            memo[i] = memo[i].split_off(&e);
            pieces[j].body.push_str(&block);
            emptied[i] = true;
            moved += 1;
        }
    }
    (moved, emptied)
}

/// 片段里的可见内容全在 `<h1>`–`<h6>` 里（至少有一个标题）。
fn only_headings(body: &str) -> bool {
    let spans = parse_spans(body, 0, body.len());
    let heads: Vec<(usize, usize, String)> =
        spans.iter().filter(|s| s.closed() && s.heading_level().is_some()).map(|s| (s.open_start, s.close_end, String::new())).collect();
    !heads.is_empty() && !has_visible(&html::apply_edits(body, heads))
}

// ───────────────────────── 链接改写 ─────────────────────────

/// 一个被拆文件：各份路径 + id → (所在份下标, 是否保留 #frag)。
struct Split {
    pieces: Vec<String>,
    ids: HashMap<String, (usize, bool)>,
}

fn piece_ids(pieces: &[Piece]) -> HashMap<String, (usize, bool)> {
    let mut ids = HashMap::new();
    for (k, p) in pieces.iter().enumerate() {
        let tail_from = if k + 1 < pieces.len() { last_visible_end(&p.body).unwrap_or(0) } else { usize::MAX };
        for (id, pos) in html::anchors(&p.body) {
            let v = if pos >= tail_from { (k + 1, false) } else { (k, true) };
            ids.entry(id.to_string()).or_insert(v);
        }
    }
    ids
}

/// 指向被拆文件 `target`、锚点 `frag`（原文）的链接的新值：改指到 id 所在的那一份；`cur` = 链接所在文件。不用改 → `None`。
fn retarget_split(target: &str, frag: Option<&str>, cur: &str, splits: &HashMap<String, Split>) -> Option<String> {
    let frag = frag.filter(|f| !f.is_empty())?;
    let split = splits.get(target)?;
    let &(k, keep) = split.ids.get(html::frag_id(frag).as_ref())?;
    let dest = &split.pieces[k];
    Some(if dest == cur {
        if keep { format!("#{frag}") } else { crate::epubzip::encode_href_path(dest.rsplit('/').next().unwrap_or(dest)) }
    } else {
        crate::epubzip::href_to(dir_of(cur), dest, if keep { frag } else { "" })
    })
}

/// 改写拆出来的一份里指向被拆文件的 `href`/`src`（两种引号都认，锚点解码后对 id）。`cur` = 这一份的路径，
/// `origin` = 裸 `#frag` 指的文件（原文件路径）。其它文件走 `wash::rewrite_book_links`。
fn rewrite_links(text: &str, cur: &str, origin: &str, splits: &HashMap<String, Split>) -> String {
    html::rewrite_links(text, |v| {
        let (p, frag) = html::split_href(v);
        if html::is_external(p) {
            return None;
        }
        let target = if p.is_empty() { origin.to_string() } else { crate::epubzip::resolve_link(cur, v).0 };
        retarget_split(&target, frag, cur, splits)
    })
    .into_owned()
}

fn piece_path(orig: &str, k: usize, taken: &HashSet<String>) -> String {
    let (stem, ext) = match orig.rfind('.') {
        Some(i) if i > orig.rfind('/').map_or(0, |j| j + 1) => (&orig[..i], &orig[i..]),
        _ => (orig, ""),
    };
    let mut n = k;
    loop {
        let p = format!("{stem}-p{n}{ext}");
        if !taken.contains(&p) {
            return p;
        }
        n += 1;
    }
}

/// 在 OPF 里把拆出来的新文件登记进 manifest（紧跟原项）和 spine（紧跟原 itemref）。`splits` = [(原文件, 新拆出的各份)]，
/// 一趟改完（此前每个被拆文件都把越来越长的 OPF 重新解析、整份复制一遍）。
fn register_in_opf(opf_text: &str, opf_dir: &str, splits: &[(String, Vec<String>)]) -> String {
    let items = manifest_items(opf_text);
    let by_path: HashMap<String, &ManifestItem> = items.iter().map(|it| (it.path(opf_dir), it)).collect();
    let irefs: HashMap<&str, html::Tag> = html::tags(opf_text)
        .filter(|t| t.is_start() && opf::is_local(t.name, "itemref"))
        .filter_map(|t| tag_attr(&opf_text[t.start..t.end], "idref").map(|id| (id, t)))
        .collect();
    // 新 id 不能撞上 OPF 里已有的 id（撞了 manifest 就有两个同 id 的项）
    let ids: HashSet<&str> = html::tags(opf_text).filter(|t| t.is_start()).filter_map(|t| html::attr_value(&opf_text[t.start..t.end], "id")).collect();
    let mut fresh: Vec<String> = Vec::new();
    let mut edits: Vec<(usize, usize, String)> = Vec::new();
    for (orig, new_paths) in splits {
        let Some(it) = by_path.get(orig) else { continue };
        let props: Vec<&str> = it.properties.split_whitespace().filter(|p| *p != "nav" && *p != "cover-image").collect();
        let props = if props.is_empty() { String::new() } else { format!(" properties=\"{}\"", props.join(" ")) };
        // 新项跟着原项的命名空间前缀（`<opf:item>` 的 OPF 里写 `<opf:item>`）
        let prefix = html::tags(it.tag).next().map_or("", |t| &t.name[..t.name.len().saturating_sub(4)]);
        let mut new_items = String::new();
        let mut new_refs = String::new();
        let iref = irefs.get(it.id).map(|t| html::remove_attr(&opf_text[t.start..t.end], "id"));
        for (k, p) in new_paths.iter().enumerate() {
            let mut nid = format!("{}-p{}", it.id, k + 2);
            while ids.contains(nid.as_str()) || fresh.contains(&nid) {
                nid.push_str("-x");
            }
            fresh.push(nid.clone());
            new_items.push_str(&format!("<{prefix}item id=\"{nid}\" href=\"{}\" media-type=\"{}\"{props}/>", crate::epubzip::href_to(opf_dir, p, ""), it.media_type));
            if let Some(r) = &iref {
                new_refs.push_str(&html::set_attr(r, "idref", &nid));
            }
        }
        edits.push((it.pos + it.tag.len(), it.pos + it.tag.len(), new_items));
        if let Some(t) = irefs.get(it.id).filter(|_| !new_refs.is_empty()) {
            edits.push((t.end, t.end, new_refs));
        }
    }
    edits.sort_by_key(|e| e.0);
    html::apply_edits(opf_text, edits)
}

/// body 之外、`<head>` 之外的杂散文字（坏书把 CSS 写在 `<html>` 与 `<head>` 之间，宽松的阅读器会当正文显示）去掉。
/// 只用于拆出来的第 2 份起的外壳：第一份原样保留，后面各份不再复制一遍，免得全书凭空多出文字（2026-09-27《金庸全集》
/// part0481 的 `p { text-indent:2em; }` 被复制进拆出的份）。
fn strip_stray_text(shell: &str) -> String {
    let mut out = String::with_capacity(shell.len());
    let (mut pos, mut head_depth) = (0, 0usize);
    // 空白与字节序标记（U+FEFF，文件开头常见）不算杂散文字。
    let blank = |t: &str| t.chars().all(|c| c.is_whitespace() || c == '\u{feff}');
    for t in html::tags(shell) {
        let text = &shell[pos..t.start];
        if head_depth > 0 || blank(text) {
            out.push_str(text);
        }
        out.push_str(&shell[t.start..t.end]);
        if t.is("head") {
            match t.kind {
                html::TagKind::Open => head_depth += 1,
                html::TagKind::Close => head_depth = head_depth.saturating_sub(1),
                _ => {}
            }
        }
        pos = t.end;
    }
    let rest = &shell[pos..];
    if head_depth > 0 || blank(rest) {
        out.push_str(rest);
    }
    out
}

/// 清洗层入口：拆分全书章节文件并改写链接。`toc_heading`：要新建 nav 时的目录标题（按书的语言）。
/// 漫画不拆：调用方（`wash_entries`）已判过，漫画不会调到这里。
pub(super) fn paginate_sections(entries: &mut Vec<Entry>, toc_heading: &str, rep: &mut WashReport) {
    let Some(opf) = parse_opf(entries) else { return };
    // 1. 收集 spine 各文件的标题（目录页、导航文件不算）。
    let mut files: Vec<FileInfo> = Vec::new();
    for path in &opf.spine {
        if Some(path) == opf.nav_doc.as_ref() || is_toc_file(path) {
            continue;
        }
        let Some(idx) = entries.iter().position(|e| &e.name == path) else { continue };
        let Ok(html) = std::str::from_utf8(&entries[idx].data) else { continue };
        let Some((lo, hi)) = html::body_range(html) else { continue };
        let spans = parse_spans(html, lo, hi);
        if looks_like_toc_page(html, lo, hi, &spans) {
            continue;
        }
        let heads = collect_headings(html, &spans, &h_candidates(&spans));
        files.push(FileInfo { idx, path: path.clone(), html: html.to_string(), lo, hi, spans, heads });
    }
    // 只信书自带的目录：本次清洗自动生成的目录（`toc_generated > 0`）是按文件/页数凑的，不代表章节结构。
    let mut label_paragraphs: HashSet<(usize, usize)> = HashSet::new(); // (文件下标, 元素下标)
    if rep.toc_generated == 0 {
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
    // 和章同级、只写节号的标题（《13級階梯》）降成节：级别取没用过的更深一级，章标题后面单独一段的 `１` 也算节标题。
    let mut demoted: HashSet<ElemRef> = HashSet::new();
    let mut demoted_level = None;
    let deepest = files.iter().flat_map(|f| f.heads.iter()).map(|h| h.level).max().unwrap_or(0);
    if deepest < 6 {
        let (heads, paras) = numbered_heading_runs(&files);
        if !heads.is_empty() {
            let level = deepest + 1;
            for (fi, f) in files.iter_mut().enumerate() {
                for h in f.heads.iter_mut().filter(|h| heads.contains(&(fi, h.span))) {
                    h.level = level;
                }
                let extra: Vec<(usize, u8)> = paras.iter().filter(|p| p.0 == fi).map(|p| (p.1, level)).collect();
                if !extra.is_empty() {
                    f.add_headings(&extra);
                }
            }
            demoted.extend(heads);
            demoted.extend(paras);
            demoted_level = Some(level);
        }
    }
    let mut roles = {
        let all: Vec<&Heading> = files.iter().flat_map(|f| f.heads.iter()).collect();
        classify(&all)
    };
    if let Some(l) = demoted_level {
        roles[l as usize] = Role::Section;
    }
    if !roles.contains(&Role::Title) {
        return;
    }
    // 全书没有节一级的标题时，认独占一段的节号（好读：`１`、`２`…）当节标题，级别取没用过的更深一级。
    let deepest = files.iter().flat_map(|f| f.heads.iter()).map(|h| h.level).max().unwrap_or(0);
    if !roles.contains(&Role::Section) && deepest < 6 {
        let level = deepest + 1;
        let mut found = false;
        for f in files.iter_mut() {
            let secs: Vec<(usize, u8)> = numbered_sections(f).into_iter().map(|i| (i, level)).collect();
            if !secs.is_empty() {
                f.add_headings(&secs);
                found = true;
            }
        }
        if found {
            roles[level as usize] = Role::Section;
        }
    }
    // 节标题要进目录：没有 id 的补一个（插入后该文件重新解析，偏移变了）。记下 (原文件, id, 标题文字)。
    // 只收**跟所属章标题在同一个原文件里**的节：单独成文件、前面没有章标题的"节"多半是附页（内容简介、版权声明），
    // 作者目录没列它就不补（《疯探》）。
    let mut sections: Vec<SectionRef> = Vec::new();
    let mut touched: HashSet<usize> = HashSet::new();
    let (mut ch_no, mut sec_no) = (0usize, 0usize);
    // 目录认出的段落章名前面还有别的内容（好读第一个正文文件：书名页在前）时，目录原来只指到文件、会落在书名页上：
    // 给段落补 id、目录改指这个 id（切分后链接改写会把它指到章名所在的那一份）。
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
            // 降成节的数字标题单独成文件，前面没有章标题，但书自带目录列着它们（跟在章后面）
            .filter(|&(hi, h)| roles[h.level as usize] == Role::Section && (first_title.is_some_and(|t| t < hi) || demoted.contains(&(fi, h.span))))
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
    // 2. 逐文件切分。
    let mut taken: HashSet<String> = entries.iter().map(|e| e.name.clone()).collect();
    let mut splits: HashMap<String, Split> = HashMap::new();
    let mut outputs: Vec<(usize, Vec<Piece>, String, String)> = Vec::new(); // (条目下标, 各份, 前缀, 后缀)
    for f in &files {
        let cuts = cut_points(&f.html, f.lo, f.hi, &f.spans, &f.heads, &roles);
        if cuts.is_empty() {
            continue;
        }
        let parts = split_body(&f.html, f.lo, f.hi, &f.spans, &cuts);
        let mut pieces: Vec<Piece> = Vec::new();
        for (k, (open, body, close)) in parts.into_iter().enumerate() {
            let path = if k == 0 { f.path.clone() } else { piece_path(&f.path, k + 1, &taken) };
            taken.insert(path.clone());
            pieces.push(Piece { path, open, body, close });
        }
        let (moved, mut emptied) = relocate_notes(&mut pieces);
        rep.paginate_notes_moved += moved;
        // 搬走注释后变空的份（只剩注释块的尾巴），或者只剩一个标题（`<h1>注释</h1>` 底下的注释都搬走了）的，并回前一份。
        // 前一份的补闭合换成被并那份的：前一份末尾补闭合的包裹元素，正是被并那份开头重新打开的那些，两者抵消
        // （2026-09-28 审计：此前只拼了正文，前一份留着自己的补闭合，又接上被并那份的，产出 `</div></div></body>`）。
        let mut k = 1;
        while k < pieces.len() {
            if has_visible(&pieces[k].body) && !(emptied[k] && only_headings(&pieces[k].body)) {
                k += 1;
                continue;
            }
            let p = pieces.remove(k);
            let was_emptied = emptied.remove(k);
            pieces[k - 1].body.push_str(&p.body);
            pieces[k - 1].close = p.close;
            // 并完的那一份再看一遍（它可能也只剩一个标题了）
            emptied[k - 1] |= was_emptied;
            k = (k - 1).max(1);
        }
        if pieces.len() < 2 {
            continue;
        }
        let ids = piece_ids(&pieces);
        splits.insert(f.path.clone(), Split { pieces: pieces.iter().map(|p| p.path.clone()).collect(), ids });
        let prefix = f.html[..f.lo].to_string();
        let suffix = f.html[f.hi..].to_string();
        outputs.push((f.idx, pieces, prefix, suffix));
    }
    // 没拆但补了 id 的文件写回。
    let split_paths: HashSet<&String> = splits.keys().collect();
    for fi in &touched {
        let f = &files[*fi];
        if !split_paths.contains(&f.path) {
            entries[f.idx].data = f.html.clone().into_bytes();
        }
    }
    // 节所在的份（拆过的文件按 id 找份）。
    for s in sections.iter_mut() {
        if let Some(p) = splits.get(&s.path).and_then(|sp| sp.ids.get(&s.id).map(|&(k, _)| sp.pieces[k].clone())) {
            s.path = p;
        }
    }
    if splits.is_empty() {
        rep.toc_sections_added += merge_sections_into_toc(entries, &sections, toc_heading);
        return;
    }
    // 3. 改写全书链接（被拆的文件自己在第 4 步按"原文件"解析裸 #frag）。
    rewrite_book_links(entries, |n| splits.contains_key(n), |l| retarget_split(&l.target, l.frag, l.file, &splits));
    // 4. 写出各份（第一份沿用原文件名），登记进 OPF。
    let opf_path = entries[opf.index].name.clone();
    let mut inserts: HashMap<usize, Vec<Entry>> = HashMap::new();
    let mut registered: Vec<(String, Vec<String>)> = Vec::new();
    for (idx, pieces, prefix, suffix) in outputs {
        let orig = entries[idx].name.clone();
        // 后面各份的 `<body>` 去掉 id（id 只留在第一份）。prefix 以 body 开标签结尾。
        // 原文件开头的字节序标记（U+FEFF）不复制进后面各份：它不是文字，每份都带一个没有意义（2026-09-29）。
        let later_prefix = match html::tags(&prefix).filter(|t| t.is_start() && t.is("body")).last().filter(|t| t.end == prefix.len()) {
            Some(t) => format!("{}{}", strip_stray_text(&prefix[..t.start]), html::remove_attr(&prefix[t.start..], "id")),
            None => strip_stray_text(&prefix),
        };
        let later_prefix = later_prefix.trim_start_matches('\u{feff}');
        let later_suffix = strip_stray_text(&suffix);
        let mut new_entries = Vec::new();
        for (k, p) in pieces.iter().enumerate() {
            let content = format!("{}{}{}", p.open, p.body, p.close);
            let content = rewrite_links(&content, &p.path, &orig, &splits);
            let html = if k == 0 { format!("{prefix}{content}{suffix}") } else { format!("{later_prefix}{content}{later_suffix}") };
            if k == 0 {
                entries[idx].data = html.into_bytes();
            } else {
                new_entries.push(Entry { name: p.path.clone(), data: html.into_bytes() });
            }
        }
        rep.sections_paginated += new_entries.len();
        registered.push((orig, new_entries.iter().map(|e| e.name.clone()).collect()));
        inserts.insert(idx, new_entries);
    }
    if let Some(i) = entries.iter().position(|e| e.name == opf_path) {
        let opf_text = String::from_utf8_lossy(&entries[i].data).into_owned();
        entries[i].data = register_in_opf(&opf_text, dir_of(&opf_path), &registered).into_bytes();
    }
    // 新条目紧跟原条目：整表重建一次（此前逐条 `insert`，每次都挪动后面全部条目）。
    let old = std::mem::take(entries);
    entries.reserve(old.len() + rep.sections_paginated);
    for (i, e) in old.into_iter().enumerate() {
        entries.push(e);
        if let Some(new_entries) = inserts.remove(&i) {
            entries.extend(new_entries);
        }
    }
    rep.toc_sections_added += merge_sections_into_toc(entries, &sections, toc_heading);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stray_text_outside_head_not_copied_to_later_pieces() {
        let shell = "\u{feff}<?xml version=\"1.0\"?><html><link href=\"a.css\"/>\np {\n\ttext-indent:2em;\n}\n<head><title>第三十八回</title></head><body>";
        assert_eq!(strip_stray_text(shell), "\u{feff}<?xml version=\"1.0\"?><html><link href=\"a.css\"/><head><title>第三十八回</title></head><body>");
        assert_eq!(strip_stray_text("\n</html>\n"), "\n</html>\n");
    }
}
