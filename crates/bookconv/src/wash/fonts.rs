//! 嵌入字体怎么留（用户 2026-10-05）：**正文和批注不用嵌入字体，批注比正文小一号；其余内容可以用书里嵌入的字体**。
//!
//! - **正文字体**：全书段落（`<p>`）覆盖字数最多的那个字体（按行内 `style`、类规则、`p`/`body` 规则依次找）。
//! - **保留的字体**：书里用 `@font-face` 嵌了文件（`src` 的 `url()` 指到书里存在的文件）、又不是正文字体的。
//!   别的 `font-family`（正文字体、系统字体名如楷体黑体）照旧去掉，交给阅读器（见 `crate::cssunlock`）。
//! - **批注**：①注释块（选择器含 footnote/fnote，见 `css::is_footnote_container_selector`，字体一律去掉）；
//!   ②以「注：」「注1：」「按:」「批注」这类开头的段落（「注」「按」后面可以跟编号）；③自己用了书里**嵌入的**单独字体、
//!   而且被括号包住（括号在元素里面或紧挨在外面）的行内元素（《绍宋》的 `（<span class="kuohao">大辛</span>）`）。
//!   ③只认嵌入字体：系统字体名（仿宋、Times New Roman）的括号文字多半是正文的一部分——《飘》括号里的心理旁白、
//!   各书括号里的外文原名——缩小字号就改了原书（2026-10-05 真书回归）。
//!   ②③加上 `eink-annot` 类，`eink-wash.css` 里让它跟着阅读器字体、字号 0.85em（`span.eink-annot` 这种写法：
//!   xochitl 吃不下 `!important`，标签加类的优先级和书里的 `span.kuohao` 一样，靠排在最后胜出）。
//!   只被批注用到的嵌入字体（《绍宋》的 kuohao）不进保留集合，书里所有地方都去掉。

use super::*;
use crate::epubzip::{dir_of, posix_norm, resolve, Entry};

/// 批注元素的类（样式在 `eink-wash.css`）。
pub(crate) const ANNOT_CLASS: &str = "eink-annot";

/// 字体分析的结果（清洗前、对原书做一次）。
#[derive(Debug, Default, Clone)]
pub struct FontPlan {
    /// 保留的字体名（规范化：去引号、小写）。
    pub keep: HashSet<String>,
    /// 正文字体（规范化后的名字）。
    pub body: Option<String>,
    /// 书里用 `@font-face` 嵌了文件的字体（规范化后的名字）。
    embedded: HashSet<String>,
    /// 类名 → 字体（书自带样式表里的类规则，后面的覆盖前面的）。
    class_family: HashMap<String, String>,
    /// 标签名 → 字体（只认裸标签规则：`p{}`、`body{}`、`span{}`）。
    tag_family: HashMap<String, String>,
}

/// 字体名规范化：取第一个、去引号、小写。
pub(crate) fn norm_family(value: &str) -> Option<String> {
    let first = value.split(',').next()?.trim().trim_end_matches("!important").trim().trim_matches(['"', '\'']).trim();
    (!first.is_empty() && !first.eq_ignore_ascii_case("inherit") && !first.eq_ignore_ascii_case("initial")).then(|| first.to_ascii_lowercase())
}

fn family_of(decls: &str) -> Option<String> {
    let mut out = None;
    for d in html::css_decls(decls) {
        if d.prop.eq_ignore_ascii_case("font-family") {
            out = norm_family(d.value).or(out);
        } else if d.prop.eq_ignore_ascii_case("font") {
            // `font: italic 1em "楷体", serif` → 字号之后的部分是字体。
            let v = d.value;
            if let Some(i) = v.find(['"', '\'']) {
                out = norm_family(&v[i..]).or(out);
            }
        }
    }
    out
}

/// (选择器或字体名, 声明或 src)。
type Pairs = Vec<(String, String)>;

/// 一段 CSS 里的 (选择器, 声明) 和 `@font-face`（字体名, src）。
fn css_rules(css: &str) -> (Pairs, Pairs) {
    let (mut rules, mut faces) = (Vec::new(), Vec::new());
    for c in css_rule_re().captures_iter(css) {
        let (_, sel) = split_leading_statements(&c[1]);
        let sel = strip_css_comments(sel);
        let sel = sel.trim();
        if sel.starts_with("@font-face") {
            let fam = html::css_decls(&c[2]).iter().find(|d| d.prop.eq_ignore_ascii_case("font-family")).and_then(|d| norm_family(d.value));
            let src: String = html::css_decls(&c[2]).iter().filter(|d| d.prop.eq_ignore_ascii_case("src")).map(|d| d.value.to_string()).collect::<Vec<_>>().join(",");
            if let Some(f) = fam {
                faces.push((f, src));
            }
        } else if !sel.starts_with('@') {
            rules.push((sel.to_string(), c[2].to_string()));
        }
    }
    (rules, faces)
}

/// `src` 里的 `url(...)` 有没有一个指到书里存在的文件。
fn src_embedded(src: &str, base_dir: &str, names: &HashSet<&str>) -> bool {
    let mut rest = src;
    while let Some(i) = html::find_ci(rest, "url(") {
        let after = &rest[i + 4..];
        let Some(j) = after.find(')') else { break };
        let raw = after[..j].trim().trim_matches(['"', '\'']).trim();
        if !raw.is_empty() && !raw.contains("://") && !raw.starts_with("data:") {
            let path = posix_norm(&resolve(base_dir, &crate::epubzip::percent_decode(raw.split(['#', '?']).next().unwrap_or(raw))));
            if names.contains(path.as_str()) {
                return true;
            }
        }
        rest = &after[j..];
    }
    false
}

/// 选择器最后一段的标签和类：`div.a p.b` → ("p", ["b"])。
fn last_compound(sel: &str) -> (String, Vec<String>) {
    let last = sel.rsplit(|c: char| c.is_whitespace() || c == '>' || c == '+' || c == '~').next().unwrap_or("");
    let last = last.split([':', '[']).next().unwrap_or("");
    let mut parts = last.split('.');
    let tag = parts.next().unwrap_or("").to_ascii_lowercase();
    (tag, parts.filter(|p| !p.is_empty()).map(str::to_string).collect())
}

impl FontPlan {
    /// 元素自己（行内 style、类规则、裸标签规则）指定的字体。
    fn own_family(&self, tag: &str) -> Option<String> {
        if let Some(f) = html::attr_value(tag, "style").and_then(family_of) {
            return Some(f);
        }
        if let Some(cls) = html::attr_value(tag, "class") {
            if let Some(f) = cls.split_whitespace().rev().find_map(|c| self.class_family.get(c)) {
                return Some(f.clone());
            }
        }
        None
    }
}

/// 分析全书：嵌入的字体、正文字体、类和标签的字体。
pub fn analyze(entries: &[Entry]) -> FontPlan {
    let names: HashSet<&str> = entries.iter().map(|e| e.name.as_str()).collect();
    let mut plan = FontPlan::default();
    let mut embedded: HashSet<String> = HashSet::new();
    let mut take = |css: &str, base_dir: &str, plan: &mut FontPlan| {
        let (rules, faces) = css_rules(css);
        for (fam, src) in faces {
            if src_embedded(&src, base_dir, &names) {
                embedded.insert(fam);
            }
        }
        for (sel, decls) in rules {
            let Some(fam) = family_of(&decls) else { continue };
            for part in sel.split(',') {
                let part = part.trim();
                let (tag, classes) = last_compound(part);
                if classes.is_empty() {
                    // 裸标签只认整个选择器就是这个标签（`p{}`）：`blockquote.bq p{}` 只管引文块里的段落，
                    // 不能当成全书段落的字体（《绍宋》正文因此被认成 letter）。
                    if !tag.is_empty() && part.eq_ignore_ascii_case(&tag) {
                        plan.tag_family.insert(tag, fam.clone());
                    }
                } else {
                    for c in classes {
                        plan.class_family.insert(c, fam.clone());
                    }
                }
            }
        }
    };
    for e in entries.iter().filter(|e| e.name.to_ascii_lowercase().ends_with(".css")) {
        match std::str::from_utf8(&e.data) {
            Ok(t) => take(t, dir_of(&e.name), &mut plan),
            // 不是 UTF-8 的样式表（Big5 之类）按单字节读：选择器、ASCII 的字体名照样认得（《啸风山庄》的 CSS.css）
            Err(_) => take(&e.data.iter().map(|&b| char::from(b)).collect::<String>(), dir_of(&e.name), &mut plan),
        }
    }
    // 正文字体：段落按字数投票。各文件的 `<style>` 按文件顺序陆续加进 `plan`，后面文件的段落按加过的规则认字体，所以分两步：
    // 先多线程把每个文件的 `<style>` 和段落（开标签、字数）扫出来（最花时间的是数字数），再按文件顺序逐个加规则、投票。
    let html_files: Vec<&Entry> = entries.iter().filter(|e| is_html_entry(&e.name, &e.data) && !is_toc_file(&e.name)).collect();
    let scanned = crate::util::par_map(&html_files, |e| {
        let Ok(h) = std::str::from_utf8(&e.data) else { return (Vec::new(), Vec::new()) };
        let styles: Vec<&str> = html::tags(h)
            .filter(|t| t.is_start() && t.name.eq_ignore_ascii_case("style"))
            .filter_map(|t| html::find_close(h, t.end, "style").map(|close| &h[t.end..close.start]))
            .collect();
        let paras: Vec<(&str, usize)> = html::tags(h)
            .filter(|t| t.is_start() && t.name.eq_ignore_ascii_case("p"))
            .filter_map(|t| {
                let close = html::find_close(h, t.end, "p")?;
                Some((&h[t.start..t.end], html::plain_text(&h[t.end..close.start]).chars().filter(|c| !c.is_whitespace()).count()))
            })
            .collect();
        (styles, paras)
    });
    let mut votes: HashMap<String, usize> = HashMap::new();
    for (e, (styles, paras)) in html_files.iter().zip(scanned) {
        for css in styles {
            take(css, dir_of(&e.name), &mut plan);
        }
        let fallback = plan.tag_family.get("p").or_else(|| plan.tag_family.get("body")).or_else(|| plan.tag_family.get("html")).cloned();
        for (tag, n) in paras {
            if let Some(f) = plan.own_family(tag).or_else(|| fallback.clone()) {
                *votes.entry(f).or_default() += n;
            }
        }
    }
    plan.body = votes.into_iter().max_by(|a, b| a.1.cmp(&b.1).then_with(|| b.0.cmp(&a.0))).map(|(f, _)| f);
    // 只被批注用到的字体不留：看每个嵌入字体有没有用在批注以外的元素上。
    plan.embedded = embedded.clone();
    // 各文件独立找（多线程），再并起来
    let used_elsewhere: HashSet<String> = crate::util::par_map(&html_files, |e| {
        let mut used: HashSet<String> = HashSet::new();
        let Ok(h) = std::str::from_utf8(&e.data) else { return used };
        for t in html::tags(h).filter(|t| t.is_start()) {
            let tag = &h[t.start..t.end];
            if let Some(f) = plan.own_family(tag) {
                if !embedded.contains(&f) || used.contains(&f) {
                    continue;
                }
                if !is_annotation(h, &t, &plan) {
                    used.insert(f);
                }
            }
        }
        used
    })
    .into_iter()
    .flatten()
    .collect();
    plan.keep = embedded.into_iter().filter(|f| Some(f) != plan.body.as_ref() && used_elsewhere.contains(f)).collect();
    plan
}

/// 正文字体，按继承算：每个元素的字体是它自己（行内 style、类规则、裸标签规则）指定的，没有就跟着父元素，全书文字按字数投票，
/// 取最多的（和 KFX 写出器的口径一样：字数最多的计算字体）。[`analyze`] 的 [`FontPlan::body`] 只看段落自己和 `p`/`body` 规则，
/// 字体写在 `<body class="…">` 上、段落继承的书认不出（《平凡的世界》的 FZLanTingSong）；Send to Kindle 规则（`kindle_rules`）用这个。
pub(crate) fn body_font_by_text(entries: &[Entry], plan: &FontPlan) -> Option<String> {
    const VOID: [&str; 14] = ["br", "img", "hr", "meta", "link", "input", "area", "base", "col", "embed", "source", "track", "wbr", "param"];
    const INLINE: [&str; 17] = ["span", "a", "b", "i", "em", "strong", "font", "small", "big", "sup", "sub", "u", "s", "cite", "code", "ruby", "rt"];
    let html_files: Vec<&Entry> = entries.iter().filter(|e| is_html_entry(&e.name, &e.data) && !is_toc_file(&e.name)).collect();
    let counts = crate::util::par_map(&html_files, |e| {
        let mut votes: HashMap<Option<String>, usize> = HashMap::new();
        let Ok(h) = std::str::from_utf8(&e.data) else { return votes };
        // (标签名, 字体)；跳过 head/style/script 里的字
        let mut stack: Vec<(String, Option<String>)> = Vec::new();
        let mut last = 0;
        let mut skip = 0usize;
        for t in html::tags(h) {
            if skip == 0 {
                let n = html::plain_text(&h[last..t.start]).chars().filter(|c| !c.is_whitespace()).count();
                if n > 0 {
                    *votes.entry(stack.last().and_then(|s| s.1.clone())).or_default() += n;
                }
            }
            last = t.end;
            let name = t.name.to_ascii_lowercase();
            match t.kind {
                html::TagKind::Open if !VOID.contains(&name.as_str()) => {
                    if matches!(name.as_str(), "head" | "style" | "script" | "title") {
                        skip += 1;
                    }
                    let raw = &h[t.start..t.end];
                    let parent = stack.last().and_then(|s| s.1.clone());
                    // 行内元素不改计数用的字体：字算给所在的块（KFX 写出器按段落的字体数，行内的另写成区间）
                    let fam = if INLINE.contains(&name.as_str()) { parent } else { plan.own_family(raw).or_else(|| plan.tag_family.get(&name).cloned()).or(parent) };
                    stack.push((name, fam));
                }
                html::TagKind::Close => {
                    if let Some(i) = stack.iter().rposition(|s| s.0 == name) {
                        stack.truncate(i);
                        if matches!(name.as_str(), "head" | "style" | "script" | "title") {
                            skip = skip.saturating_sub(1);
                        }
                    }
                }
                _ => {}
            }
        }
        votes
    });
    let mut total: HashMap<Option<String>, usize> = HashMap::new();
    for v in counts {
        for (k, n) in v {
            *total.entry(k).or_default() += n;
        }
    }
    total.into_iter().max_by(|a, b| a.1.cmp(&b.1).then_with(|| b.0.cmp(&a.0))).and_then(|(f, _)| f)
}

/// 文字像批注（以「注：」「注1：」「按:」「批注」这类开头，见 [`note_prefix`]）。
pub(crate) fn looks_like_note(text: &str) -> bool {
    note_prefix(text.trim_start_matches(|c: char| c.is_whitespace() || c == '\u{3000}'))
}

/// 段落开头是不是批注标记：「批注」，或「注」「按」后面跟可选的编号、再跟冒号（`注：`、`注1：`、`按:`）。
fn note_prefix(text: &str) -> bool {
    if text.starts_with("批注") {
        return true;
    }
    let mut cs = text.chars();
    if !matches!(cs.next(), Some('注' | '按')) {
        return false;
    }
    let rest: String = cs.take(6).collect();
    let rest = rest.trim_start_matches(|c: char| c.is_ascii_digit() || ('０'..='９').contains(&c) || "一二三四五六七八九十〇".contains(c));
    rest.starts_with(['：', ':'])
}
const OPEN_BRACKETS: [char; 2] = ['（', '('];
const CLOSE_BRACKETS: [char; 2] = ['）', ')'];

/// 这个元素是不是批注（规则②③；规则①注释块由样式表那边按选择器处理）。
/// 先按元素名、字体筛掉不可能的，再去找闭合标签（2026-10-06 审计：此前每个开标签都先往后找闭合标签，`<br/>`、`<img>` 这种
/// 没有闭合标签的一路找到文件末尾，`<br>` 多的章节是平方级）。
fn is_annotation(h: &str, t: &html::Tag, plan: &FontPlan) -> bool {
    let name = t.name.to_ascii_lowercase();
    let tag = &h[t.start..t.end];
    let inner_of = |close: &html::Tag| &h[t.end..close.start];
    match name.as_str() {
        "p" | "div" => {
            let Some(close) = html::find_close(h, t.end, &name) else { return false };
            let inner = inner_of(&close);
            let text = html::plain_text(inner);
            let text = text.trim_start_matches(|c: char| c.is_whitespace() || c == '\u{3000}');
            // 只认叶子块：里面再有段落的 div 不算（免得整章被当成批注）。
            !html::tags(inner).any(|x| x.is_start() && matches!(x.name.to_ascii_lowercase().as_str(), "p" | "div" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6"))
                && note_prefix(text)
        }
        "span" | "font" | "small" | "em" | "i" | "cite" => {
            let own = plan.own_family(tag);
            if !own.as_ref().is_some_and(|f| plan.embedded.contains(f)) || own == plan.body {
                return false;
            }
            let Some(close) = html::find_close(h, t.end, &name) else { return false };
            let text = html::plain_text(inner_of(&close));
            let text = text.trim();
            let inside = text.starts_with(OPEN_BRACKETS) && text.ends_with(CLOSE_BRACKETS);
            let before = h[..t.start].trim_end().chars().last().is_some_and(|c| OPEN_BRACKETS.contains(&c));
            let after = h[close.end..].trim_start().chars().next().is_some_and(|c| CLOSE_BRACKETS.contains(&c));
            !text.is_empty() && (inside || (before && after))
        }
        _ => false,
    }
}

/// 给批注（规则②③）加 `eink-annot` 类。返回改后的 html 和加了几处。
pub fn mark_annotations(h: &str, plan: &FontPlan) -> (String, usize) {
    let mut edits: Vec<(usize, usize, String)> = Vec::new();
    for t in html::tags(h).filter(|t| t.is_start()) {
        if is_annotation(h, &t, plan) {
            edits.push((t.start, t.end, html::add_class(&h[t.start..t.end], ANNOT_CLASS)));
        }
    }
    let n = edits.len();
    if n == 0 {
        return (h.to_string(), 0);
    }
    (html::apply_edits(h, edits), n)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn e(name: &str, data: &str) -> Entry {
        Entry { name: name.into(), data: data.as_bytes().to_vec() }
    }

    fn book() -> Vec<Entry> {
        vec![
            e("OEBPS/Styles/s.css", r#"@font-face{font-family:"宋体";src:local("st"),url("../Fonts/SongTi.ttf")}@font-face{font-family:"letter";src:url("../Fonts/letter.ttf")}@font-face{font-family:"kuohao";src:url(../Fonts/kuohao.ttf)}@font-face{font-family:"无文件";src:url(../Fonts/none.ttf)}p{font-family:"宋体"}p.letter{font-family:"letter"}blockquote.bq p{font-family:"letter"}span.kuohao{font-family:"kuohao";font-size:90%}p.xz{font-family:"楷体"}"#),
            e("OEBPS/Fonts/SongTi.ttf", "x"),
            e("OEBPS/Fonts/letter.ttf", "x"),
            e("OEBPS/Fonts/kuohao.ttf", "x"),
            e("OEBPS/Text/c1.xhtml", r#"<html><body><p>正文很长很长很长很长很长很长（<span class="kuohao">括注</span>）还有</p><p class="letter">书信</p><p>注：这是批注段落</p><p class="xz">小注</p><p><span class="kuohao">不在括号里</span></p></body></html>"#),
        ]
    }

    #[test]
    fn analyze_finds_body_and_keep() {
        let p = analyze(&book());
        assert_eq!(p.body.as_deref(), Some("宋体"));
        let mut keep: Vec<&str> = p.keep.iter().map(String::as_str).collect();
        keep.sort();
        assert_eq!(keep, ["kuohao", "letter"], "正文字体、没文件的、系统字体名都不留；kuohao 还用在不在括号里的 span 上");
    }

    #[test]
    fn note_prefixes() {
        for t in ["注：甲", "注1：甲", "注12:甲", "注三：甲", "按：甲", "批注甲"] {
            assert!(note_prefix(t), "{t}");
        }
        for t in ["注意：甲", "注定要", "按照", "注"] {
            assert!(!note_prefix(t), "{t}");
        }
    }

    #[test]
    fn font_used_only_by_annotations_is_not_kept() {
        let mut b = book();
        b[4] = e("OEBPS/Text/c1.xhtml", r#"<html><body><p>正文很长很长很长很长很长很长（<span class="kuohao">括注</span>）还有</p><p class="letter">书信</p></body></html>"#);
        let p = analyze(&b);
        assert!(!p.keep.contains("kuohao") && p.keep.contains("letter"), "{:?}", p.keep);
    }

    #[test]
    fn marks_note_paragraphs_and_bracketed_spans() {
        let b = book();
        let p = analyze(&b);
        let (h, n) = mark_annotations(std::str::from_utf8(&b[4].data).unwrap(), &p);
        assert_eq!(n, 2, "{h}");
        assert!(h.contains(r#"（<span class="kuohao eink-annot">括注</span>）"#), "{h}");
        assert!(h.contains(r#"<p class="eink-annot">注：这是批注段落</p>"#), "{h}");
        assert!(h.contains(r#"<span class="kuohao">不在括号里</span>"#), "不在括号里的不算");
        // 系统字体名（没嵌文件）的括号文字是正文的一部分（《飘》的心理旁白），不算批注
        let (h2, n2) = mark_annotations(r#"<p>斯嘉丽想（<span class="xz">一定要跳华尔兹！</span>）</p>"#, &FontPlan { class_family: [("xz".to_string(), "楷体".to_string())].into(), ..p.clone() });
        assert_eq!(n2, 0, "{h2}");
        assert!(h.contains(r#"<p class="letter">书信</p>"#));
    }
}
