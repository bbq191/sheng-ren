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
//! 注释块时，把注释块搬到引用它的那一份末尾。回链（注释 → 正文）指向前面，不搬。
//!
//! 保守：漫画书、目录页（链接文字占大半）、解析不了 `<body>` 的文件不动；拆不出有内容的两份就不拆。幂等：已拆过的书再跑不会再拆。

use super::*;

const VOID: &[&str] = &["area", "base", "br", "col", "embed", "hr", "img", "input", "link", "meta", "param", "source", "track", "wbr"];

/// 一个元素在 html 里的位置（字节偏移）。没有闭合标签的元素 `close_start == close_end`。
struct Span {
    name: String,
    open_start: usize,
    open_end: usize,
    close_start: usize,
    close_end: usize,
    parent: Option<usize>,
    void: bool,
}

fn tag_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r#"(?s)<!--.*?-->|<!\[CDATA\[.*?\]\]>|<[?!][^>]*>|<(/?)([A-Za-z][A-Za-z0-9:_-]*)\b[^>]*?(/?)>"#).unwrap())
}

/// 解析 `html[lo..hi]` 里的元素（容错：闭合标签找不到对应开标签就忽略，中间没闭合的元素视为在此处隐式闭合）。
fn parse_spans(html: &str, lo: usize, hi: usize) -> Vec<Span> {
    let mut spans: Vec<Span> = Vec::new();
    let mut stack: Vec<usize> = Vec::new();
    for c in tag_re().captures_iter(&html[lo..hi]) {
        let m = c.get(0).unwrap();
        let (s, e) = (lo + m.start(), lo + m.end());
        let Some(name) = c.get(2) else { continue };
        let name = name.as_str().to_ascii_lowercase();
        if !c[1].is_empty() {
            if let Some(pos) = stack.iter().rposition(|&i| spans[i].name == name) {
                while stack.len() > pos + 1 {
                    let i = stack.pop().unwrap();
                    spans[i].close_start = s;
                    spans[i].close_end = s;
                }
                let i = stack.pop().unwrap();
                spans[i].close_start = s;
                spans[i].close_end = e;
            }
            continue;
        }
        let void = !c[3].is_empty() || VOID.contains(&name.as_str());
        let parent = stack.last().copied();
        spans.push(Span { name, open_start: s, open_end: e, close_start: if void { e } else { hi }, close_end: if void { e } else { hi }, parent, void });
        if !void {
            stack.push(spans.len() - 1);
        }
    }
    spans
}

/// 片段里有没有读者看得见的内容：非空白文字，或图片/表格/分隔线等媒体。
pub(super) fn has_visible(fragment: &str) -> bool {
    static MEDIA: OnceLock<Regex> = OnceLock::new();
    static TAG: OnceLock<Regex> = OnceLock::new();
    if MEDIA.get_or_init(|| Regex::new(r#"(?i)<(img|svg|image|hr|table|video|audio|math|object)\b"#).unwrap()).is_match(fragment) {
        return true;
    }
    let text = TAG.get_or_init(|| Regex::new(r#"(?s)<[^>]*>"#).unwrap()).replace_all(fragment, "");
    let text = text.replace("&nbsp;", " ").replace("&#160;", " ").replace("&#xa0;", " ");
    text.chars().any(|c| !c.is_whitespace())
}

/// 片段里的文字字数（不含空白）。
fn text_len(fragment: &str) -> usize {
    plain_text(fragment).chars().filter(|c| !c.is_whitespace()).count()
}

/// 标题页后面只跟着这么短的文字（书名页的"作者：某某"之类）时不另起一页。
const TITLE_TAIL_MIN_CHARS: usize = 30;

/// 片段里最后一处可见内容结束的偏移（没有可见内容时 `None`）。
pub(super) fn last_visible_end(fragment: &str) -> Option<usize> {
    let mut last = None;
    let mut pos = 0;
    for m in tag_re().find_iter(fragment) {
        if has_visible(&fragment[pos..m.start()]) {
            last = Some(m.start());
        }
        if has_visible(m.as_str()) {
            last = Some(m.end());
        }
        pos = m.end();
    }
    if has_visible(&fragment[pos..]) {
        last = Some(fragment.len());
    }
    last
}

/// 片段里有闭合标签的元素：(开标签起点, 开标签终点, 闭标签起点)。
pub(super) fn parse_spans_pub(html: &str) -> Vec<(usize, usize, usize)> {
    parse_spans(html, 0, html.len()).into_iter().filter(|s| !s.void && s.close_end > s.close_start).map(|s| (s.open_start, s.open_end, s.close_start)).collect()
}

fn body_bounds(html: &str) -> Option<(usize, usize)> {
    static OPEN: OnceLock<Regex> = OnceLock::new();
    let open = OPEN.get_or_init(|| Regex::new(r#"(?is)<body\b[^>]*>"#).unwrap()).find(html)?;
    let close = html.to_ascii_lowercase().rfind("</body>")?;
    (close >= open.end()).then_some((open.end(), close))
}

fn strip_id_attr(open_tag: &str) -> String {
    static ID: OnceLock<Regex> = OnceLock::new();
    ID.get_or_init(|| Regex::new(r#"(?i)\s+id\s*=\s*("[^"]*"|'[^']*')"#).unwrap()).replace_all(open_tag, "").into_owned()
}

fn id_attr_re() -> &'static Regex {
    static ID: OnceLock<Regex> = OnceLock::new();
    ID.get_or_init(|| Regex::new(r#"(?i)<[A-Za-z][^>]*?\sid\s*=\s*"([^"]+)""#).unwrap())
}

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
    spans
        .iter()
        .enumerate()
        .filter_map(|(i, sp)| {
            let b = sp.name.as_bytes();
            (b.len() == 2 && b[0] == b'h' && (b'1'..=b'6').contains(&b[1]) && sp.close_end > sp.close_start).then(|| (i, b[1] - b'0'))
        })
        .collect()
}

/// 全书没有 `<hN>` 时的退路：目录（NCX）指向的短段落（`<p>`/`<div>`，≤60 字）当标题，目录层级当级别。
/// 目录只指到文件、不带锚点（`frag` 为空）时取该文件第一个有文字的段落。
/// Calibre 转出的中文书常把章名写成 `<p class="block_7">緣起首回…</p>`，只能靠目录认出来。
fn toc_candidates(html: &str, spans: &[Span], frags: &[(String, u8)]) -> Vec<(usize, u8)> {
    let mut out = Vec::new();
    for (frag, depth) in frags {
        let hit = spans.iter().position(|sp| {
            let block = matches!(sp.name.as_str(), "p" | "div") && sp.close_end > sp.close_start;
            if frag.is_empty() {
                block && !html[sp.open_end..sp.close_start].contains("<p") && !html[sp.open_end..sp.close_start].contains("<div") && has_visible(&html[sp.open_end..sp.close_start])
            } else {
                block && tag_attr(&html[sp.open_start..sp.open_end], "id") == Some(frag.as_str())
            }
        });
        if let Some(i) = hit {
            let n = plain_text(&html[spans[i].open_end..spans[i].close_start]).chars().count();
            if (1..=60).contains(&n) {
                out.push((i, (*depth).clamp(1, 6)));
            }
        }
    }
    out.sort_by_key(|&(i, _)| spans[i].open_start);
    out.dedup_by_key(|x| x.0);
    out
}

/// NCX 目录：zip 路径 → [(frag, 深度)]。
fn toc_targets(entries: &[Entry], opf: &Opf) -> HashMap<String, Vec<(String, u8)>> {
    let mut map: HashMap<String, Vec<(String, u8)>> = HashMap::new();
    let Some(ncx) = opf.ncx.as_ref() else { return map };
    let Some(e) = entries.iter().find(|e| &e.name == ncx) else { return map };
    let text = String::from_utf8_lossy(&e.data);
    for (depth, _, target) in crate::ncx::parse_ncx_flat(&text) {
        let (p, frag) = target.split_once('#').unwrap_or((target.as_str(), ""));
        let path = posix_norm(&resolve(dir_of(ncx), &percent_decode(p)));
        map.entry(path).or_default().push((frag.to_string(), depth.min(6) as u8));
    }
    map
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
fn looks_like_toc_page(body: &str) -> bool {
    static A: OnceLock<Regex> = OnceLock::new();
    let a = A.get_or_init(|| Regex::new(r#"(?is)<a\b[^>]*\bhref="[^"]*"[^>]*>(.*?)</a>"#).unwrap());
    let links: Vec<_> = a.captures_iter(body).collect();
    if links.len() < 3 {
        return false;
    }
    let link_chars: usize = links.iter().map(|c| plain_text(&c[1]).chars().count()).sum();
    let all_chars = plain_text(body).chars().count().max(1);
    link_chars * 2 > all_chars
}

// ───────────────────────── 切分 ─────────────────────────

/// 切点（`html` 里的字节偏移，严格落在 body 内），切出的每一份都有文字。
fn cut_points(html: &str, lo: usize, hi: usize, spans: &[Span], hs: &[Heading], roles: &[Role; 7]) -> Vec<usize> {
    let mut cuts: Vec<usize> = Vec::new();
    let mut title_ends: HashSet<usize> = HashSet::new();
    let mut section_starts: HashSet<usize> = HashSet::new();
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
    let bounds = |cuts: &[usize]| -> Vec<usize> { std::iter::once(lo).chain(cuts.iter().copied()).chain(std::iter::once(hi)).collect() };
    // 没有文字的一份（空白，或只有装饰图/分隔线）并入前一份；在最前面就并入后一份。图片仍跟着原来的上下文。
    while !cuts.is_empty() {
        let b = bounds(&cuts);
        let Some(k) = (0..b.len() - 1).find(|&k| text_len(&html[b[k]..b[k + 1]]) == 0) else { break };
        cuts.remove(if k == 0 { 0 } else { k - 1 });
    }
    // 标题页后面只跟着很短的文字（书名页的作者行）：不另起一页。后面紧接着是节标题时照常分页（节再短也是一节）。
    while let Some(k) = {
        let b = bounds(&cuts);
        (0..cuts.len()).find(|&k| title_ends.contains(&cuts[k]) && !section_starts.contains(&cuts[k]) && text_len(&html[b[k + 1]..b[k + 2]]) < TITLE_TAIL_MIN_CHARS)
    } {
        cuts.remove(k);
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

fn open_stack_at(spans: &[Span], o: usize) -> Vec<usize> {
    spans.iter().enumerate().filter(|(_, s)| !s.void && s.open_end <= o && s.close_start >= o && s.close_end > s.open_end).map(|(i, _)| i).collect()
}

fn split_body(html: &str, lo: usize, hi: usize, spans: &[Span], cuts: &[usize]) -> Vec<(String, String, String)> {
    let bounds: Vec<usize> = std::iter::once(lo).chain(cuts.iter().copied()).chain(std::iter::once(hi)).collect();
    (0..bounds.len() - 1)
        .map(|k| {
            let (a, b) = (bounds[k], bounds[k + 1]);
            let open: String = if k == 0 { String::new() } else { open_stack_at(spans, a).iter().map(|&i| strip_id_attr(&html[spans[i].open_start..spans[i].open_end])).collect() };
            let close: String = if k + 1 == bounds.len() - 1 { String::new() } else { open_stack_at(spans, b).iter().rev().map(|&i| format!("</{}>", &html[spans[i].open_start + 1..spans[i].open_start + 1 + spans[i].name.len()])).collect() };
            (open, html[a..b].to_string(), close)
        })
        .collect()
}

fn marker_like(a_tag_and_inner: &str, inner: &str, before: &str) -> bool {
    static TEXT: OnceLock<Regex> = OnceLock::new();
    static SUP_BEFORE: OnceLock<Regex> = OnceLock::new();
    let text = plain_text(inner);
    let text_re = TEXT.get_or_init(|| Regex::new(r#"^[\[\(（〔【<]?\s*(\d{1,4}|[*†‡§]{1,3}|[①-⑳]|[ⅰ-ⅹ]|注\s*\d{0,4}|[a-z])\s*[\]\)）〕】>]?$"#).unwrap());
    let sup_before = SUP_BEFORE.get_or_init(|| Regex::new(r#"(?i)<sup\b[^>]*>\s*$"#).unwrap());
    let l = a_tag_and_inner.to_ascii_lowercase();
    text_re.is_match(&text) || inner.to_ascii_lowercase().contains("<sup") || sup_before.is_match(before) || l.contains("noteref") || l.contains("footnote")
}

/// 把"注释标号 → 后面某一份里的注释块"的注释块搬到引用它的那一份末尾。返回搬了几条。
fn relocate_notes(pieces: &mut [Piece]) -> usize {
    static A: OnceLock<Regex> = OnceLock::new();
    let a_re = A.get_or_init(|| Regex::new(r##"(?is)<a\b[^>]*\bhref="#([^"]+)"[^>]*>(.*?)</a>"##).unwrap());
    let mut moved = 0;
    for j in 0..pieces.len() {
        let wanted: Vec<String> = a_re
            .captures_iter(&pieces[j].body)
            .filter(|c| {
                let m = c.get(0).unwrap();
                let body = &pieces[j].body;
                let mut from = m.start().saturating_sub(80);
                while !body.is_char_boundary(from) {
                    from += 1;
                }
                marker_like(m.as_str(), &c[2], &body[from..m.start()])
            })
            .map(|c| c[1].to_string())
            .collect();
        for frag in wanted {
            let needle = format!("id=\"{frag}\"");
            let Some(i) = (j + 1..pieces.len()).find(|&i| pieces[i].body.contains(&needle)) else { continue };
            let body = &pieces[i].body;
            let spans = parse_spans(body, 0, body.len());
            let Some(sp) = spans.iter().find(|s| {
                matches!(s.name.as_str(), "p" | "li" | "div" | "aside") && s.close_end > s.close_start && tag_attr(&body[s.open_start..s.open_end], "id") == Some(frag.as_str())
            }) else {
                continue;
            };
            let mut block = body[sp.open_start..sp.close_end].to_string();
            if sp.name == "li" {
                block = format!("<div{}</div>", &block[3..block.len() - 5]);
            }
            let (s, e) = (sp.open_start, sp.close_end);
            pieces[i].body.replace_range(s..e, "");
            pieces[j].body.push_str(&block);
            moved += 1;
        }
    }
    moved
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
        for c in id_attr_re().captures_iter(&p.body) {
            let pos = c.get(0).unwrap().start();
            let v = if pos >= tail_from { (k + 1, false) } else { (k, true) };
            ids.entry(c[1].to_string()).or_insert(v);
        }
    }
    ids
}

fn encode_href_path(p: &str) -> String {
    let mut out = String::with_capacity(p.len());
    for b in p.bytes() {
        if b.is_ascii_alphanumeric() || b"-._~/".contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// 改写 `text` 里指向被拆文件的 `href`/`src`。`cur` = 这段文字所在文件的路径，`origin` = 裸 `#frag` 指的文件
/// （拆出来的份里是原文件路径，其它文件就是自己）。
fn rewrite_links(text: &str, cur: &str, origin: &str, splits: &HashMap<String, Split>) -> String {
    static ATTR: OnceLock<Regex> = OnceLock::new();
    let re = ATTR.get_or_init(|| Regex::new(r#"(?i)\b(href|src)(\s*=\s*)"([^"]*)""#).unwrap());
    re.replace_all(text, |c: &regex::Captures| {
        let whole = c[0].to_string();
        let v = &c[3];
        let (p, frag) = match v.split_once('#') {
            Some((p, f)) => (p, Some(f)),
            None => (v, None),
        };
        if p.contains(':') {
            return whole;
        }
        let Some(frag) = frag.filter(|f| !f.is_empty()) else { return whole };
        let target = if p.is_empty() { origin.to_string() } else { posix_norm(&resolve(dir_of(cur), &percent_decode(p))) };
        let Some(split) = splits.get(&target) else { return whole };
        let Some(&(k, keep)) = split.ids.get(frag) else { return whole };
        let dest = &split.pieces[k];
        let new = if dest == cur {
            if keep { format!("#{frag}") } else { encode_href_path(dest.rsplit('/').next().unwrap_or(dest)) }
        } else {
            let rel = encode_href_path(&relative_to(dir_of(cur), dest));
            if keep { format!("{rel}#{frag}") } else { rel }
        };
        format!("{}{}\"{}\"", &c[1], &c[2], new)
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

/// 在 OPF 里把拆出来的新文件登记进 manifest（紧跟原项）和 spine（紧跟原 itemref）。
fn register_in_opf(opf_text: &str, opf_dir: &str, orig: &str, new_paths: &[String]) -> String {
    let items = manifest_items(opf_text);
    let Some(it) = items.iter().find(|it| posix_norm(&resolve(opf_dir, &percent_decode(it.href))) == orig) else { return opf_text.to_string() };
    let (id, tag, media) = (it.id.to_string(), it.tag.to_string(), it.media_type.to_string());
    let props: Vec<&str> = it.properties.split_whitespace().filter(|p| *p != "nav" && *p != "cover-image").collect();
    let props = if props.is_empty() { String::new() } else { format!(" properties=\"{}\"", props.join(" ")) };
    let mut new_items = String::new();
    let mut new_refs = Vec::new();
    for (k, p) in new_paths.iter().enumerate() {
        let nid = format!("{id}-p{}", k + 2);
        new_items.push_str(&format!("<item id=\"{nid}\" href=\"{}\" media-type=\"{media}\"{props}/>", encode_href_path(&relative_to(opf_dir, p))));
        new_refs.push(nid);
    }
    let mut out = opf_text.replacen(&tag, &format!("{tag}{new_items}"), 1);
    static IREF: OnceLock<Regex> = OnceLock::new();
    let iref = IREF.get_or_init(|| Regex::new(r#"(?s)<itemref\b[^>]*?/?>"#).unwrap());
    if let Some(m) = iref.find_iter(&out).find(|m| tag_attr(m.as_str(), "idref") == Some(id.as_str())) {
        let r = strip_id_attr(m.as_str());
        let extra: String = new_refs.iter().map(|nid| r.replacen(&format!("\"{id}\""), &format!("\"{nid}\""), 1)).collect();
        out.insert_str(m.end(), &extra);
    }
    out
}

/// 清洗层入口：拆分全书章节文件并改写链接。
pub(super) fn paginate_sections(entries: &mut Vec<Entry>, rep: &mut WashReport) {
    if crate::comic_detect::is_comic(entries) {
        return;
    }
    let Some(opf) = parse_opf(entries) else { return };
    // 1. 收集 spine 各文件的标题（目录页、导航文件不算）。
    struct FileInfo {
        idx: usize,
        path: String,
        html: String,
        lo: usize,
        hi: usize,
        spans: Vec<Span>,
        heads: Vec<Heading>,
    }
    let mut files: Vec<FileInfo> = Vec::new();
    for path in &opf.spine {
        if Some(path) == opf.nav_doc.as_ref() || is_toc_file(path) {
            continue;
        }
        let Some(idx) = entries.iter().position(|e| &e.name == path) else { continue };
        let Ok(html) = std::str::from_utf8(&entries[idx].data) else { continue };
        let Some((lo, hi)) = body_bounds(html) else { continue };
        if looks_like_toc_page(&html[lo..hi]) {
            continue;
        }
        let spans = parse_spans(html, lo, hi);
        let heads = collect_headings(html, &spans, &h_candidates(&spans));
        files.push(FileInfo { idx, path: path.clone(), html: html.to_string(), lo, hi, spans, heads });
    }
    // 只信书自带的目录：本次清洗自动生成的目录（`toc_generated > 0`）是按文件/页数凑的，不代表章节结构。
    if rep.toc_generated == 0 && files.iter().all(|f| f.heads.is_empty()) {
        let targets = toc_targets(entries, &opf);
        for f in files.iter_mut() {
            if let Some(frags) = targets.get(&f.path) {
                f.heads = collect_headings(&f.html, &f.spans, &toc_candidates(&f.html, &f.spans, frags));
            }
        }
    }
    let roles = {
        let all: Vec<&Heading> = files.iter().flat_map(|f| f.heads.iter()).collect();
        classify(&all)
    };
    if !roles.contains(&Role::Title) {
        return;
    }
    // 节标题要进目录：没有 id 的补一个（插入后该文件重新解析，偏移变了）。记下 (原文件, id, 标题文字)。
    // 只收**跟所属章标题在同一个原文件里**的节：单独成文件、前面没有章标题的"节"多半是附页（内容简介、版权声明），
    // 作者目录没列它就不补（《疯探》）。
    let mut sections: Vec<(String, String, String)> = Vec::new();
    let mut touched: HashSet<usize> = HashSet::new();
    let mut sec_no = 0usize;
    for (fi, f) in files.iter_mut().enumerate() {
        let mut inserts: Vec<(usize, String)> = Vec::new();
        let first_title = f.heads.iter().position(|h| roles[h.level as usize] == Role::Title);
        for (hi, h) in f.heads.iter().enumerate().filter(|(_, h)| roles[h.level as usize] == Role::Section) {
            if first_title.is_none_or(|t| t > hi) {
                continue;
            }
            let sp = &f.spans[h.span];
            let open = &f.html[sp.open_start..sp.open_end];
            let id = match tag_attr(open, "id") {
                Some(id) => id.to_string(),
                None => loop {
                    sec_no += 1;
                    let id = format!("eink-sec-{sec_no}");
                    if !f.html.contains(&format!("id=\"{id}\"")) {
                        inserts.push((sp.open_start + 1 + sp.name.len(), format!(" id=\"{id}\"")));
                        break id;
                    }
                },
            };
            sections.push((f.path.clone(), id, h.text.clone()));
        }
        if inserts.is_empty() {
            continue;
        }
        for (pos, attr) in inserts.into_iter().rev() {
            f.html.insert_str(pos, &attr);
        }
        let (lo, hi) = body_bounds(&f.html).expect("插入 id 不影响 body 边界");
        f.lo = lo;
        f.hi = hi;
        f.spans = parse_spans(&f.html, lo, hi);
        let cands: Vec<(usize, u8)> = {
            // 重新按原来的方式认标题：先 h 标签；原来是目录退路认出来的就按原级别对回同一批元素。
            let hc = h_candidates(&f.spans);
            if !hc.is_empty() { hc } else { f.heads.iter().map(|h| (h.span, h.level)).collect() }
        };
        f.heads = collect_headings(&f.html, &f.spans, &cands);
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
        rep.paginate_notes_moved += relocate_notes(&mut pieces);
        // 搬走注释后变空的份（只剩注释块的尾巴）并回前一份。
        let mut k = 1;
        while k < pieces.len() {
            if has_visible(&pieces[k].body) {
                k += 1;
                continue;
            }
            let p = pieces.remove(k);
            pieces[k - 1].body.push_str(&p.body);
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
    let sections: Vec<(String, String, String)> = sections
        .into_iter()
        .map(|(path, id, label)| {
            let path = splits.get(&path).and_then(|s| s.ids.get(&id).map(|&(k, _)| s.pieces[k].clone())).unwrap_or(path);
            (path, id, label)
        })
        .collect();
    if splits.is_empty() {
        rep.toc_sections_added += merge_sections_into_toc(entries, &sections);
        return;
    }
    // 3. 改写全书链接（拆出来的各份按"原文件"解析裸 #frag）。
    let split_idx: HashSet<usize> = outputs.iter().map(|o| o.0).collect();
    for (i, e) in entries.iter_mut().enumerate() {
        let l = e.name.to_ascii_lowercase();
        if split_idx.contains(&i) || !(is_html_entry(&e.name, &e.data) || l.ends_with(".ncx") || l.ends_with(".opf")) {
            continue;
        }
        let Ok(t) = std::str::from_utf8(&e.data) else { continue };
        let new = rewrite_links(t, &e.name, &e.name, &splits);
        if new != t {
            e.data = new.into_bytes();
        }
    }
    // 4. 写出各份（第一份沿用原文件名），登记进 OPF。
    let opf_path = entries[opf.index].name.clone();
    let mut opf_text = String::from_utf8_lossy(&entries[opf.index].data).into_owned();
    static BODY_OPEN: OnceLock<Regex> = OnceLock::new();
    let body_open = BODY_OPEN.get_or_init(|| Regex::new(r#"(?is)<body\b[^>]*>$"#).unwrap());
    let mut inserts: Vec<(usize, Vec<Entry>)> = Vec::new();
    for (idx, pieces, prefix, suffix) in outputs {
        let orig = entries[idx].name.clone();
        let later_prefix = match body_open.find(&prefix) {
            Some(m) => format!("{}{}", &prefix[..m.start()], strip_id_attr(m.as_str())),
            None => prefix.clone(),
        };
        let mut new_entries = Vec::new();
        for (k, p) in pieces.iter().enumerate() {
            let content = format!("{}{}{}", p.open, p.body, p.close);
            let content = rewrite_links(&content, &p.path, &orig, &splits);
            let html = format!("{}{}{}", if k == 0 { &prefix } else { &later_prefix }, content, suffix);
            if k == 0 {
                entries[idx].data = html.into_bytes();
            } else {
                new_entries.push(Entry { name: p.path.clone(), data: html.into_bytes() });
            }
        }
        rep.sections_paginated += new_entries.len();
        let paths: Vec<String> = new_entries.iter().map(|e| e.name.clone()).collect();
        opf_text = register_in_opf(&opf_text, dir_of(&opf_path), &orig, &paths);
        inserts.push((idx, new_entries));
    }
    if let Some(i) = entries.iter().position(|e| e.name == opf_path) {
        entries[i].data = opf_text.into_bytes();
    }
    inserts.sort_by_key(|x| std::cmp::Reverse(x.0));
    for (idx, new_entries) in inserts {
        for (off, e) in new_entries.into_iter().enumerate() {
            entries.insert(idx + 1 + off, e);
        }
    }
    rep.toc_sections_added += merge_sections_into_toc(entries, &sections);
}
