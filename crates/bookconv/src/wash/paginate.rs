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

/// 一个标题（位置已扩到只包着它的元素）。
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

    /// 给这些元素（下标）取 id：已有的用原来的，没有的补 `{prefix}-N`（N 从 `counter` 往上数、文件里没出现过）。
    /// `id=""` 的给 `None`（不补也不改，拿不准）。补了就重新解析；插入属性不改变元素结构，元素下标和标题都按原样对回。
    fn ensure_ids(&mut self, elems: &[usize], prefix: &str, counter: &mut usize) -> Vec<Option<String>> {
        let mut inserts: Vec<(usize, String)> = Vec::new();
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
                        if !self.html.contains(id.as_str()) {
                            inserts.push((sp.open_start + 1 + sp.name.len(), format!(" id=\"{id}\"")));
                            break id;
                        }
                    }),
                }
            })
            .collect();
        if !inserts.is_empty() {
            inserts.sort_unstable_by_key(|x| x.0);
            for (pos, attr) in inserts.into_iter().rev() {
                self.html.insert_str(pos, &attr);
            }
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
    let mut out = String::with_capacity(text.len() + 64);
    let mut last = 0;
    let mut label = String::new();
    let mut label_start: Option<usize> = None;
    for t in html::tags(&text) {
        if t.is("text") && t.kind == html::TagKind::Open {
            label_start = Some(t.end);
        } else if t.is("text") && t.kind == html::TagKind::Close {
            if let Some(s) = label_start.take() {
                label = squash(&crate::util::xml_unescape(&text[s..t.start]));
            }
        } else if t.is("content") && t.is_start() {
            let tag = &text[t.start..t.end];
            let Some(a) = html::attr(tag, "src") else { continue };
            let (p, frag) = html::split_href(a.value);
            if frag.is_some() {
                continue;
            }
            let path = posix_norm(&resolve(dir_of(&ncx), &percent_decode(p)));
            if let Some((_, _, id)) = targets.iter().find(|(f, l, _)| *f == path && squash(l) == label) {
                let at = t.start + a.value_end;
                out.push_str(&text[last..at]);
                out.push('#');
                out.push_str(&crate::util::xml_escape(id));
                last = at;
            }
        }
    }
    if last > 0 {
        out.push_str(&text[last..]);
        e.data = out.into_bytes();
    }
}

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
            (b.len() == 2 && b[0] == b'h' && (b'1'..=b'6').contains(&b[1]) && sp.closed()).then(|| (i, b[1] - b'0'))
        })
        .collect()
}

/// 全书没有 `<hN>` 时的退路：目录（NCX）指向的短段落（`<p>`/`<div>`，≤60 字）当标题，目录层级当级别。
/// 目录只指到文件、不带锚点（`frag` 为空）时取该文件第一个有文字的段落。
/// Calibre 转出的中文书常把章名写成 `<p class="block_7">緣起首回…</p>`，只能靠目录认出来。
fn toc_candidates(html: &str, spans: &[Span], targets: &[TocTarget]) -> Vec<(usize, u8)> {
    let mut out = Vec::new();
    for t in targets {
        let hit = spans.iter().position(|sp| {
            if t.frag.is_empty() {
                leaf_block(html, sp) && has_visible(&html[sp.open_end..sp.close_start])
            } else {
                matches!(sp.name.as_str(), "p" | "div") && sp.closed() && html::attr_value(&html[sp.open_start..sp.open_end], "id") == Some(t.frag.as_str())
            }
        });
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
    /// 锚点（已解码；只指到文件时为空）。
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
        let (p, frag) = html::split_href(&target);
        let path = posix_norm(&resolve(dir_of(ncx), &percent_decode(p)));
        map.entry(path).or_default().push(TocTarget { frag: html::frag_id(frag.unwrap_or("")).into_owned(), depth: depth.min(6) as u8, label });
    }
    map
}

/// 比较标题文字用：去掉所有空白（含全角空格）。
fn squash(t: &str) -> String {
    t.chars().filter(|c| !c.is_whitespace()).collect()
}

/// 章名写成普通段落的书（好读：第一个正文文件的 `<h3>` 是书名，"第一章"只是一行字）：目录只指到文件、标签跟文件里某个
/// 短段落的文字一样、而该文件的 `<hN>` 里没有这个文字时，把这个段落当作跟"目录认得出的其它章标题"同一级的标题候选。
/// 级别取目录标签对得上的 `<hN>` 里最常见的那一级；一个都对不上就不补（拿不准）。返回 (文件下标, 候选)。
fn toc_label_paragraphs(files: &[FileInfo], targets: &HashMap<String, Vec<TocTarget>>) -> Vec<(usize, (usize, u8))> {
    let labels: HashSet<String> = targets.values().flatten().map(|t| squash(&t.label)).filter(|l| !l.is_empty()).collect();
    let mut count = [0usize; 7];
    for h in files.iter().flat_map(|f| f.heads.iter()).filter(|h| labels.contains(&squash(&h.text))) {
        count[h.level as usize] += 1;
    }
    let Some(level) = (1..=6u8).filter(|&l| count[l as usize] > 0).max_by_key(|&l| (count[l as usize], std::cmp::Reverse(l))) else { return Vec::new() };
    let mut out = Vec::new();
    for (fi, f) in files.iter().enumerate() {
        for t in targets.get(&f.path).into_iter().flatten().filter(|t| t.frag.is_empty()) {
            let label = squash(&t.label);
            if label.is_empty() || label.chars().count() > 60 || f.heads.iter().any(|h| squash(&h.text) == label) {
                continue;
            }
            let hit = f.spans.iter().position(|sp| sp.open_start >= f.lo && leaf_block(&f.html, sp) && squash(&plain_text(&f.html[sp.open_end..sp.close_start])) == label);
            if let Some(i) = hit {
                out.push((fi, (i, level)));
            }
        }
    }
    out
}

/// 独占一段的节号的值：1–3 位阿拉伯/全角数字，或一…九十九的中文数字。
pub(super) fn section_number(t: &str) -> Option<u32> {
    let t = squash(t);
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
    // 标题页后面只跟着很短的文字（书名页的作者行）：不另起一页。后面紧接着是节标题时照常分页（节再短也是一节）。
    while let Some(k) = (0..cuts.len()).find(|&k| title_ends.contains(&cuts[k]) && !section_starts.contains(&cuts[k]) && lens[k + 1] < TITLE_TAIL_MIN_CHARS) {
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

fn marker_like(a_tag_and_inner: &str, inner: &str, before: &str) -> bool {
    static TEXT: OnceLock<Regex> = OnceLock::new();
    static SUP_BEFORE: OnceLock<Regex> = OnceLock::new();
    let text = plain_text(inner);
    let text_re = TEXT.get_or_init(|| Regex::new(r#"^[\[\(（〔【<]?\s*(\d{1,4}|[*†‡§]{1,3}|[①-⑳]|[ⅰ-ⅹ]|注\s*\d{0,4}|[a-z])\s*[\]\)）〕】>]?$"#).unwrap());
    let sup_before = SUP_BEFORE.get_or_init(|| Regex::new(r#"(?i)<sup\b[^>]*>\s*$"#).unwrap());
    let l = a_tag_and_inner.to_ascii_lowercase();
    text_re.is_match(&text) || inner.to_ascii_lowercase().contains("<sup") || sup_before.is_match(before) || l.contains("noteref") || l.contains("footnote")
}

/// 搬注释块的上限：块里的字数超过这么多就不搬（真注释很少超过一页；再长多半是包住整章的 div，搬了会把正文挪走）。
const NOTE_MAX_CHARS: usize = 1500;

/// `body` 里带锚点 `id` 的注释块：锚点元素本身是 `<p>`/`<li>`/`<div>`/`<aside>`，或锚点在这种块的最前面（块里锚点之前没有
/// 可见内容）时取最近的这种祖先。块里有标题或字数超过 [`NOTE_MAX_CHARS`] 时不算。
fn note_block(body: &str, spans: &[Span], id: &str) -> Option<usize> {
    let ai = spans.iter().position(|s| {
        let open = &body[s.open_start..s.open_end];
        html::attr_value(open, "id") == Some(id) || (s.name == "a" && html::attr_value(open, "name") == Some(id))
    })?;
    let mut bi = ai;
    loop {
        let s = &spans[bi];
        if matches!(s.name.as_str(), "p" | "li" | "div" | "aside") && s.closed() {
            break;
        }
        let p = s.parent?;
        if has_visible(&body[spans[p].open_end..spans[ai].open_start]) {
            return None;
        }
        bi = p;
    }
    let b = &spans[bi];
    let has_heading = spans.iter().any(|s| s.open_start >= b.open_end && s.close_end <= b.close_start && s.name.len() == 2 && s.name.starts_with('h') && s.name.as_bytes()[1].is_ascii_digit());
    (!has_heading && text_len(&body[b.open_end..b.close_start]) <= NOTE_MAX_CHARS).then_some(bi)
}

/// 把"注释标号 → 后面某一份里的注释块"的注释块搬到引用它的那一份末尾。返回搬了几条。
fn relocate_notes(pieces: &mut [Piece]) -> usize {
    let mut moved = 0;
    for j in 0..pieces.len() {
        let wanted: Vec<String> = {
            let body = &pieces[j].body;
            parse_spans(body, 0, body.len())
                .iter()
                .filter(|s| s.name == "a" && s.closed())
                .filter_map(|s| {
                    let frag = html::attr_value(&body[s.open_start..s.open_end], "href")?.strip_prefix('#')?;
                    let mut from = s.open_start.saturating_sub(80);
                    while !body.is_char_boundary(from) {
                        from += 1;
                    }
                    marker_like(&body[s.open_start..s.close_end], &body[s.open_end..s.close_start], &body[from..s.open_start]).then(|| html::frag_id(frag).into_owned())
                })
                .collect()
        };
        for id in wanted {
            let found = (j + 1..pieces.len()).filter(|&i| pieces[i].body.contains(id.as_str())).find_map(|i| {
                let body = &pieces[i].body;
                let spans = parse_spans(body, 0, body.len());
                note_block(body, &spans, &id).map(|b| (i, spans[b].clone()))
            });
            let Some((i, sp)) = found else { continue };
            let body = &pieces[i].body;
            let block = if sp.name == "li" {
                format!("<div{}{}</div>", &body[sp.open_start + 3..sp.open_end], &body[sp.open_end..sp.close_start])
            } else {
                body[sp.open_start..sp.close_end].to_string()
            };
            pieces[i].body.replace_range(sp.open_start..sp.close_end, "");
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
        for (id, pos) in html::anchors(&p.body) {
            let v = if pos >= tail_from { (k + 1, false) } else { (k, true) };
            ids.entry(id.to_string()).or_insert(v);
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

/// 改写 `text` 里指向被拆文件的 `href`/`src`（两种引号都认，锚点解码后对 id）。`cur` = 这段文字所在文件的路径，
/// `origin` = 裸 `#frag` 指的文件（拆出来的份里是原文件路径，其它文件就是自己）。
fn rewrite_links(text: &str, cur: &str, origin: &str, splits: &HashMap<String, Split>) -> String {
    html::rewrite_links(text, |v| {
        let (p, frag) = html::split_href(v);
        if html::is_external(p) {
            return None;
        }
        let frag = frag.filter(|f| !f.is_empty())?;
        let target = if p.is_empty() { origin.to_string() } else { posix_norm(&resolve(dir_of(cur), &percent_decode(p))) };
        let split = splits.get(&target)?;
        let &(k, keep) = split.ids.get(html::frag_id(frag).as_ref())?;
        let dest = &split.pieces[k];
        Some(if dest == cur {
            if keep { format!("#{frag}") } else { encode_href_path(dest.rsplit('/').next().unwrap_or(dest)) }
        } else {
            let rel = encode_href_path(&relative_to(dir_of(cur), dest));
            if keep { format!("{rel}#{frag}") } else { rel }
        })
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
    let by_path: HashMap<String, &ManifestItem> = items.iter().map(|it| (posix_norm(&resolve(opf_dir, &percent_decode(it.href))), it)).collect();
    let irefs: HashMap<&str, html::Tag> = html::tags(opf_text)
        .filter(|t| t.is_start() && t.is("itemref"))
        .filter_map(|t| tag_attr(&opf_text[t.start..t.end], "idref").map(|id| (id, t)))
        .collect();
    let mut edits: Vec<(usize, usize, String)> = Vec::new();
    for (orig, new_paths) in splits {
        let Some(it) = by_path.get(orig) else { continue };
        let props: Vec<&str> = it.properties.split_whitespace().filter(|p| *p != "nav" && *p != "cover-image").collect();
        let props = if props.is_empty() { String::new() } else { format!(" properties=\"{}\"", props.join(" ")) };
        let mut new_items = String::new();
        let mut new_refs = String::new();
        let iref = irefs.get(it.id).map(|t| html::remove_attr(&opf_text[t.start..t.end], "id"));
        for (k, p) in new_paths.iter().enumerate() {
            let nid = format!("{}-p{}", it.id, k + 2);
            new_items.push_str(&format!("<item id=\"{nid}\" href=\"{}\" media-type=\"{}\"{props}/>", encode_href_path(&relative_to(opf_dir, p)), it.media_type));
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
pub(super) fn paginate_sections(entries: &mut Vec<Entry>, toc_heading: &str, rep: &mut WashReport) {
    if crate::comic_detect::is_comic(entries) {
        return;
    }
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
    let mut roles = {
        let all: Vec<&Heading> = files.iter().flat_map(|f| f.heads.iter()).collect();
        classify(&all)
    };
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
            .filter(|&(hi, h)| roles[h.level as usize] == Role::Section && first_title.is_some_and(|t| t < hi))
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
    for s in sections.iter_mut() {
        if let Some(p) = splits.get(&s.path).and_then(|sp| sp.ids.get(&s.id).map(|&(k, _)| sp.pieces[k].clone())) {
            s.path = p;
        }
    }
    if splits.is_empty() {
        rep.toc_sections_added += merge_sections_into_toc(entries, &sections, toc_heading);
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
    let mut inserts: HashMap<usize, Vec<Entry>> = HashMap::new();
    let mut registered: Vec<(String, Vec<String>)> = Vec::new();
    for (idx, pieces, prefix, suffix) in outputs {
        let orig = entries[idx].name.clone();
        // 后面各份的 `<body>` 去掉 id（id 只留在第一份）。prefix 以 body 开标签结尾。
        let later_prefix = match html::tags(&prefix).filter(|t| t.is_start() && t.is("body")).last().filter(|t| t.end == prefix.len()) {
            Some(t) => format!("{}{}", strip_stray_text(&prefix[..t.start]), html::remove_attr(&prefix[t.start..], "id")),
            None => strip_stray_text(&prefix),
        };
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
