//! 竖排第一期（2026-11-11，Kobo、汉王的中文文字书）：书判定、引号、ASCII 全角、竖排样式和书 CSS 横转竖。
//!
//! 实机结论（vinci 2026-11-11，测试书 01–13）：
//! - Kobo（KEPUB/Nickel）：`html`/`body` 写 `writing-mode:vertical-rl` 就竖排；spine rtl 时点左侧下一页；ruby 在右侧。
//! - Kobo 上半角数字、西文躺倒会叠到前一个汉字上，插空格、`margin`、`text-orientation:upright`、縦中横都治不好；
//!   **全部转全角竖立**干净。汉王同样选全角。
//! - Bigme（xReader）不竖排、KOReader（上游）也不会竖排 → Bigme 不开。
//!
//! 本模块只做与设备无关的判定和文字 / CSS 改写；开关来自 profile（`vertical_text`、`vertical_ascii`），
//! 哪本书竖排由书库按书决定（书自己的 `layout.vertical` → 目录清单 → 自动判定）。

use crate::epubzip::Entry;
use crate::html;
use crate::wash::opf;
use profile::VerticalAscii;
use regex::Regex;
use std::sync::OnceLock;

/// 自动判定的抽样长度（非空白字符数，按 spine 顺序取，PRD 原案，vinci 2026-11-11 同意）。
const SAMPLE_CHARS: usize = 20_000;
/// `dc:language` 是 `zh*` 时，汉字至少占抽样的这么多。
const HAN_MIN_WITH_ZH_LANG: f64 = 0.30;
/// `dc:language` 缺失或误标时，汉字至少占这么多、假名少于 [`KANA_MAX`] 才算中文。
const HAN_MIN_WITHOUT_LANG: f64 = 0.60;
/// 假名占抽样超过这个比例就不是中文书（日文）。
const KANA_MAX: f64 = 0.02;

/// 竖排样式表的名字（放 OPF 同目录）。
pub const VERTICAL_CSS_NAME: &str = "eink-vertical.css";
/// 竖排样式表的 manifest id。
pub const VERTICAL_CSS_ID: &str = "eink-vertical-css";

/// 是不是固定版式（OPF 的 `rendition:layout = pre-paginated`，或 KindleGen 约定的 `fixed-layout = true`）。
pub fn is_fixed_layout(entries: &[Entry]) -> bool {
    let Some(o) = crate::wash::parse_opf(entries) else {
        return false;
    };
    fixed_layout_opf(&String::from_utf8_lossy(&entries[o.index].data))
}

fn fixed_layout_opf(opf: &str) -> bool {
    use html::TagKind;
    for t in html::tags(opf).filter(|t| t.kind == TagKind::Open && opf::is_local(t.name, "meta")) {
        let tag = &opf[t.start..t.end];
        let name = html::attr_value(tag, "name").unwrap_or("");
        let property = html::attr_value(tag, "property").unwrap_or("");
        let content = html::attr_value(tag, "content").unwrap_or("");
        if name.eq_ignore_ascii_case("fixed-layout") && content.trim().eq_ignore_ascii_case("true")
        {
            return true;
        }
        if property.eq_ignore_ascii_case("rendition:layout") {
            if content.trim().eq_ignore_ascii_case("pre-paginated") {
                return true;
            }
            // EPUB 3 的写法是元素内容：<meta property="rendition:layout">pre-paginated</meta>
            if html::find_close(opf, t.end, t.name).is_some_and(|c| {
                opf[t.end..c.start]
                    .trim()
                    .eq_ignore_ascii_case("pre-paginated")
            }) {
                return true;
            }
        }
        if name.eq_ignore_ascii_case("rendition:layout")
            && content.trim().eq_ignore_ascii_case("pre-paginated")
        {
            return true;
        }
    }
    false
}

/// 抽样一段文字里汉字、假名各有多少（只看 CJK 统一汉字、扩展 A 和平假名、片假名）。
fn script_counts(text: &str) -> (usize, usize, usize) {
    let (mut han, mut kana, mut total) = (0, 0, 0);
    for c in text.chars().filter(|c| !c.is_whitespace()) {
        total += 1;
        let u = c as u32;
        if (0x4E00..=0x9FFF).contains(&u) || (0x3400..=0x4DBF).contains(&u) {
            han += 1;
        } else if (0x3040..=0x30FF).contains(&u) {
            kana += 1;
        }
    }
    (han, kana, total)
}

/// 这本书是不是中文文字书（规则 2）：按 spine 抽前 [`SAMPLE_CHARS`] 个非空白字符，`dc:language` 是 `zh*`
/// 时汉字 ≥ 30%、否则汉字 ≥ 60% 且假名 < 2%；假名 ≥ 2% 一律不是。固定版式不算。
pub fn is_zh_text(entries: &[Entry]) -> bool {
    if is_fixed_layout(entries) {
        return false;
    }
    let Some(o) = crate::wash::parse_opf(entries) else {
        return false;
    };
    let opf_text = String::from_utf8_lossy(&entries[o.index].data);
    let lang = opf::opf_dc(&opf_text).language;
    let zh_lang = lang.trim().to_ascii_lowercase().starts_with("zh");
    let by_name = crate::wash::name_index(entries);
    let (mut han, mut kana, mut total) = (0, 0, 0);
    for p in &o.spine {
        if total >= SAMPLE_CHARS {
            break;
        }
        if crate::util::is_image_ext(p) {
            continue;
        }
        let Some(&i) = by_name.get(p.as_str()) else {
            continue;
        };
        let Ok(text) = std::str::from_utf8(&entries[i].data) else {
            continue;
        };
        let body = crate::comic_detect::strip_noise_tags(text);
        let plain = crate::wash::plain_text(&body);
        let (h, k, t) = script_counts(&plain);
        han += h;
        kana += k;
        total += t;
    }
    if total == 0 {
        return false;
    }
    let (han, kana) = (han as f64 / total as f64, kana as f64 / total as f64);
    if kana >= KANA_MAX {
        return false;
    }
    if zh_lang {
        han >= HAN_MIN_WITH_ZH_LANG
    } else {
        han >= HAN_MIN_WITHOUT_LANG
    }
}

/// 原书本来就是竖排：任一样式表或 `<style>` 里根元素 / 正文块写了 `writing-mode: vertical-*`（认 `-webkit-`、
/// `-epub-` 前缀），或 `<html>`/`<body>` 的行内 `style` 这么写。
pub fn is_natively_vertical(entries: &[Entry]) -> bool {
    static VERTICAL_DECL: OnceLock<Regex> = OnceLock::new();
    let re = VERTICAL_DECL.get_or_init(|| {
        Regex::new(r"(?i)(?:-webkit-|-epub-)?writing-mode\s*:\s*vertical-(?:rl|lr)").unwrap()
    });
    for e in entries {
        let Ok(text) = std::str::from_utf8(&e.data) else {
            continue;
        };
        if e.name.to_ascii_lowercase().ends_with(".css") {
            if rule_sets_vertical(text, re) {
                return true;
            }
            continue;
        }
        if !crate::epubzip::is_html_entry(&e.name, &e.data) {
            continue;
        }
        if inline_vertical(text) {
            return true;
        }
        for b in html::style_blocks(text) {
            if rule_sets_vertical(&text[b.content_start..b.content_end], re) {
                return true;
            }
        }
    }
    false
}

fn inline_vertical(html: &str) -> bool {
    use html::TagKind;
    html::tags(html)
        .filter(|t| t.kind == TagKind::Open && (t.is("html") || t.is("body")))
        .any(|t| {
            html::attr_value(&html[t.start..t.end], "style").is_some_and(|s| {
                s.to_ascii_lowercase().contains("writing-mode")
                    && s.to_ascii_lowercase().contains("vertical-")
            })
        })
}

fn rule_sets_vertical(css: &str, re: &Regex) -> bool {
    for r in crate::cascade::rule_spans(css) {
        let sel = crate::cascade::rule_selector(r.prelude);
        let sel = sel.trim();
        if sel.is_empty()
            || sel.starts_with('@')
            || ["table", "img", "svg", "ruby", "rt", "rp"]
                .iter()
                .any(|n| sel.to_ascii_lowercase().contains(n))
        {
            continue;
        }
        if re.is_match(r.body) {
            return true;
        }
    }
    false
}

// ───────────────────────── 文字节点 ─────────────────────────

/// 块级元素的界限：文字按这些元素分段（引号配对不跨段、不跨块）。
const BLOCK_TAGS: &[&str] = &[
    "p",
    "div",
    "li",
    "blockquote",
    "h1",
    "h2",
    "h3",
    "h4",
    "h5",
    "h6",
    "td",
    "th",
    "dt",
    "dd",
    "section",
    "article",
    "aside",
    "figure",
    "figcaption",
    "header",
    "footer",
    "main",
    "tr",
    "table",
    "body",
    "html",
];

fn is_block(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    BLOCK_TAGS.contains(&n.as_str())
}

/// 可改写的文字段，按块级元素分组。跳过 `skip` 里的元素（含其内容）和注释、CDATA。
fn text_groups(html: &str, skip: &[&str]) -> Vec<Vec<(usize, usize)>> {
    use html::TagKind;
    let mut groups: Vec<Vec<(usize, usize)>> = Vec::new();
    let mut cur: Vec<(usize, usize)> = Vec::new();
    let mut pos = 0;
    let mut skipping: Option<String> = None;
    for t in html::tags(html) {
        if skipping.is_none() && t.start > pos {
            cur.push((pos, t.start));
        }
        pos = t.end;
        match &skipping {
            Some(name) => {
                if t.kind == TagKind::Close && t.is(name) {
                    skipping = None;
                }
            }
            None => {
                if is_block(t.name) && !cur.is_empty() {
                    groups.push(std::mem::take(&mut cur));
                }
                if t.kind == TagKind::Open && skip.iter().any(|s| t.is(s)) {
                    skipping = Some(t.name.to_ascii_lowercase());
                }
            }
        }
    }
    if skipping.is_none() && pos < html.len() {
        cur.push((pos, html.len()));
    }
    if !cur.is_empty() {
        groups.push(cur);
    }
    groups
}

/// 在每一段文字上做等长替换（引号转换、全角都是 1 字符换 1 字符）。
fn map_text_groups(html: &str, skip: &[&str], f: impl Fn(&str) -> String) -> String {
    let mut edits = Vec::new();
    for group in text_groups(html, skip) {
        let mut joined = String::new();
        for &(s, e) in &group {
            joined.push_str(&html[s..e]);
        }
        let converted = f(&joined);
        let want: Vec<char> = joined.chars().collect();
        let got: Vec<char> = converted.chars().collect();
        if got.len() != want.len() {
            continue; // 只接受 1 字符换 1 字符，避免把偏移弄乱
        }
        let mut at = 0;
        for &(s, e) in &group {
            let n = html[s..e].chars().count();
            if got[at..at + n] != want[at..at + n] {
                edits.push((s, e, got[at..at + n].iter().collect::<String>()));
            }
            at += n;
        }
    }
    if edits.is_empty() {
        html.to_string()
    } else {
        html::apply_edits(html, edits)
    }
}

/// 引号（GB/T 15834）：成对的弯引号 “”→『』、‘’→「」；ASCII 直引号也转。撇号、落单的不当引号；
/// 只在一个段落里成对、嵌套合法的才转；引号里全是西文（没有汉字）的弯引号不转。
pub fn convert_quotes(html: &str) -> String {
    let decoded = decode_quote_refs(html);
    map_text_groups(
        &decoded,
        &["code", "pre", "script", "style"],
        convert_quotes_text,
    )
}

/// 把文字节点里的直引号字符引用（`&quot;`、`&apos;`、`&#34;`、`&#39;`、十六进制写法）还原成字面引号，
/// 好和字面引号一起配对；`code`/`pre`/`script`/`style` 不动。输出仍是合法 XHTML（文字里的 `"` 不用转义）。
fn decode_quote_refs(html: &str) -> String {
    let mut edits = Vec::new();
    for group in text_groups(html, &["code", "pre", "script", "style"]) {
        for (s, e) in group {
            let out = decode_quote_refs_text(&html[s..e]);
            if out != html[s..e] {
                edits.push((s, e, out));
            }
        }
    }
    if edits.is_empty() {
        html.to_string()
    } else {
        html::apply_edits(html, edits)
    }
}

fn decode_quote_refs_text(s: &str) -> String {
    static RE: OnceLock<Regex> = OnceLock::new();
    let re =
        RE.get_or_init(|| Regex::new(r"(?i)&(?:quot|apos|#0*34|#0*39|#x0*22|#x0*27);").unwrap());
    re.replace_all(s, |c: &regex::Captures| -> String {
        let m = c.get(0).unwrap().as_str().to_ascii_lowercase();
        if m.contains("39") || m.contains("27") {
            "'".to_string()
        } else {
            "\"".to_string()
        }
    })
    .into_owned()
}

fn is_han(c: char) -> bool {
    let u = c as u32;
    (0x4E00..=0x9FFF).contains(&u) || (0x3400..=0x4DBF).contains(&u)
}

fn convert_quotes_text(s: &str) -> String {
    let chars: Vec<char> = s.chars().collect();
    let mut out = chars.clone();
    // 前后都是 ASCII 字母/数字的 ' / ’ 是撇号，不当引号（don't、90's、don’t）
    let apostrophe = |i: usize| -> bool {
        i > 0
            && i + 1 < chars.len()
            && chars[i - 1].is_ascii_alphanumeric()
            && chars[i + 1].is_ascii_alphanumeric()
    };
    // 弯引号：先配对、检查嵌套；配不上整段不转
    let mut stack: Vec<(char, usize)> = Vec::new();
    let mut pairs: Vec<(usize, usize, char)> = Vec::new();
    let mut ok = true;
    for (i, &c) in chars.iter().enumerate() {
        match c {
            '“' => stack.push(('“', i)),
            '‘' => stack.push(('‘', i)),
            '”' => match stack.pop() {
                Some(('“', j)) => pairs.push((j, i, '“')),
                _ => {
                    ok = false;
                    break;
                }
            },
            '’' => {
                if apostrophe(i) {
                    continue;
                }
                match stack.pop() {
                    Some(('‘', j)) => pairs.push((j, i, '‘')),
                    _ => {
                        ok = false;
                        break;
                    }
                }
            }
            _ => {}
        }
    }
    if ok && stack.is_empty() {
        for (open, close, kind) in pairs {
            let western = !chars[open + 1..close].iter().any(|&c| is_han(c));
            if western {
                continue;
            }
            let (o, c) = if kind == '“' {
                ('『', '』')
            } else {
                ('「', '」')
            };
            out[open] = o;
            out[close] = c;
        }
    }
    // 直引号：撇号（前后都是 ASCII 字母/数字）不算；余下的按出现顺序成对
    for quote in ['"', '\''] {
        let positions: Vec<usize> = chars
            .iter()
            .enumerate()
            .filter(|&(i, &c)| c == quote && (quote != '\'' || !apostrophe(i)))
            .map(|(i, _)| i)
            .collect();
        for pair in positions.chunks(2) {
            if pair.len() == 2 {
                let (o, c) = if quote == '"' {
                    ('『', '』')
                } else {
                    ('「', '」')
                };
                out[pair[0]] = o;
                out[pair[1]] = c;
            }
        }
    }
    out.into_iter().collect()
}

/// 半角 ASCII（U+0021–U+007E）转全角；空格、标签、属性、代码/注音/样式、注释都不动。
/// 字符引用整体认出来：代表 U+0021–U+007E 的换成对应全角字面字符，别的原样留。
pub fn fullwidth_ascii(html: &str) -> String {
    let skip = [
        "head", "script", "style", "code", "pre", "kbd", "samp", "svg", "math", "rt", "rp",
    ];
    let mut edits = Vec::new();
    for group in text_groups(html, &skip) {
        for (s, e) in group {
            let out = fullwidth_segment(&html[s..e]);
            if out != html[s..e] {
                edits.push((s, e, out));
            }
        }
    }
    if edits.is_empty() {
        html.to_string()
    } else {
        html::apply_edits(html, edits)
    }
}

fn fullwidth_segment(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    while i < s.len() {
        let c = s[i..].chars().next().unwrap();
        if c == '&' {
            if let Some(rel) = s[i..].find(';').filter(|&r| r <= 40) {
                let r = &s[i..i + rel + 1];
                let decoded = html::unescape_entities(r);
                if decoded.as_ref() != r {
                    let mut it = decoded.chars();
                    if let (Some(d), None) = (it.next(), it.next()) {
                        if (0x21..=0x7E).contains(&(d as u32)) {
                            out.push(char::from_u32(d as u32 + 0xFEE0).unwrap());
                            i += rel + 1;
                            continue;
                        }
                    }
                }
                out.push_str(r);
                i += rel + 1;
                continue;
            }
        }
        if (0x21..=0x7E).contains(&(c as u32)) {
            out.push(char::from_u32(c as u32 + 0xFEE0).unwrap());
        } else {
            out.push(c);
        }
        i += c.len_utf8();
    }
    out
}

/// 竖排书正文章节的文字处理：先引号（直、弯）、再全角。
pub fn transform_text(html: &str, ascii: VerticalAscii) -> String {
    let out = convert_quotes(html);
    if ascii == VerticalAscii::Fullwidth {
        fullwidth_ascii(&out)
    } else {
        out
    }
}

/// `toc.ncx`：只改 `<text>` 元素里的文字（引号 + 全角），属性一概不动。
pub fn transform_ncx(ncx: &str, ascii: VerticalAscii) -> String {
    use html::TagKind;
    let opens: Vec<_> = html::tags(ncx)
        .filter(|t| t.kind == TagKind::Open && opf::is_local(t.name, "text"))
        .filter_map(|t| html::find_close(ncx, t.end, t.name).map(|c| (t.end, c.start)))
        .collect();
    let mut edits = Vec::new();
    for (s, e) in opens {
        let decoded = decode_quote_refs_text(&ncx[s..e]);
        let mut out = convert_quotes_text(&decoded);
        if ascii == VerticalAscii::Fullwidth {
            out = fullwidth_segment(&out);
        }
        if out != ncx[s..e] {
            edits.push((s, e, out));
        }
    }
    if edits.is_empty() {
        ncx.to_string()
    } else {
        html::apply_edits(ncx, edits)
    }
}

// ───────────────────────── 样式 ─────────────────────────

/// 竖排样式表：根元素竖排（原书本来竖排时不写）、段距用物理左右外边距、表格整张横排。
pub fn vertical_css(native: bool) -> String {
    let mut s = String::new();
    if !native {
        s.push_str("html{writing-mode:vertical-rl;-webkit-writing-mode:vertical-rl;-epub-writing-mode:vertical-rl;}\n");
    }
    // 段距用方法 a（实机选定）：物理左右外边距，盖过书里横排的上下段距
    s.push_str("p{margin-left:1em;margin-right:1em;}\n");
    s.push_str("table{writing-mode:horizontal-tb;-webkit-writing-mode:horizontal-tb;-epub-writing-mode:horizontal-tb;}\n");
    s
}

/// OPF：spine 写 rtl、补 `primary-writing-mode`。已经有 `primary-writing-mode` 的不重复加。
pub fn vertical_opf(opf_text: &str) -> String {
    let out = crate::direction::set_spine_direction(opf_text, crate::direction::PageDirection::Rtl);
    add_primary_writing_mode(&out)
}

/// 只补 `<meta name="primary-writing-mode" content="vertical-rl"/>`（按书指定了翻页方向时 spine 方向以书为准）。
pub fn add_primary_writing_mode(opf_text: &str) -> String {
    if opf::meta_content(opf_text, "primary-writing-mode").is_some() {
        return opf_text.to_string();
    }
    opf::insert_metadata(
        opf_text,
        r#"<meta name="primary-writing-mode" content="vertical-rl"/>"#,
    )
    .unwrap_or_else(|| opf_text.to_string())
}

/// 书自己的 CSS 横转竖（只对块级规则的盒子属性）：返回 (新 CSS, 没转的复杂块数)。
/// `top→right、right→bottom、bottom→left、left→top`；`width↔height`、`max/min` 同理；`float` 去掉；
/// 绝对/固定定位、负边距的规则整条原样留、计数。
pub fn rewrite_css(css: &str) -> (String, usize) {
    if css.trim().is_empty() {
        return (css.to_string(), 0);
    }
    let mut edits = Vec::new();
    let mut complex = 0;
    for r in crate::cascade::rule_spans(css) {
        let sel = crate::cascade::rule_selector(r.prelude);
        if sel.trim().is_empty() || sel.trim_start().starts_with('@') {
            continue;
        }
        let decls = html::css_decls(r.body);
        if decls.is_empty() {
            continue;
        }
        let is_complex = decls.iter().any(|d| {
            let p = d.prop.to_ascii_lowercase();
            (p == "position" && {
                let v = d.value.to_ascii_lowercase();
                v.contains("absolute") || v.contains("fixed")
            }) || ((p.starts_with("margin") || p.starts_with("padding"))
                && d.value
                    .split_whitespace()
                    .any(|v| v.trim_start().starts_with('-')))
        });
        if is_complex {
            complex += 1;
            continue;
        }
        let mut body = String::new();
        let mut changed = false;
        for d in decls {
            let prop = d.prop.to_ascii_lowercase();
            if prop == "float" {
                changed = true;
                continue;
            }
            let (new_prop, value) = map_box_decl(&prop, d.value);
            if new_prop != prop || value != d.value {
                changed = true;
            }
            body.push_str(&format!("{new_prop}:{value};"));
        }
        if changed {
            edits.push((r.body_start, r.body_start + r.body.len(), body));
        }
    }
    if edits.is_empty() {
        (css.to_string(), complex)
    } else {
        (html::apply_edits(css, edits), complex)
    }
}

fn map_box_decl(prop: &str, value: &str) -> (String, String) {
    // 圆角：物理角跟着 top→right→bottom→left 转（top-left → top-right → bottom-right → bottom-left → top-left）
    if let Some(rest) = prop.strip_prefix("border-") {
        for (from, to) in [
            ("top-left", "top-right"),
            ("top-right", "bottom-right"),
            ("bottom-right", "bottom-left"),
            ("bottom-left", "top-left"),
        ] {
            if let Some(suffix) = rest.strip_prefix(&format!("{from}-")) {
                return (format!("border-{to}-{suffix}"), value.to_string());
            }
        }
        // border-top-width / border-left-style / border-bottom-color 这类三段式
        if let Some((side, leaf)) = rest.split_once('-') {
            let new = match side {
                "top" => "right",
                "right" => "bottom",
                "bottom" => "left",
                "left" => "top",
                _ => "",
            };
            if !new.is_empty() && matches!(leaf, "width" | "style" | "color") {
                return (format!("border-{new}-{leaf}"), value.to_string());
            }
        }
    }
    let map_side = |p: &str| -> Option<String> {
        let (head, side) = p.rsplit_once('-')?;
        let new = match side {
            "top" => "right",
            "right" => "bottom",
            "bottom" => "left",
            "left" => "top",
            _ => return None,
        };
        Some(format!("{head}-{new}"))
    };
    if let Some(p) = map_side(prop) {
        return (p, value.to_string());
    }
    match prop {
        "width" => ("height".into(), value.into()),
        "height" => ("width".into(), value.into()),
        "max-width" => ("max-height".into(), value.into()),
        "max-height" => ("max-width".into(), value.into()),
        "min-width" => ("min-height".into(), value.into()),
        "min-height" => ("min-width".into(), value.into()),
        // 四边一起写的盒子简写：按 top→right→bottom→left 旋转值的顺序
        "margin" | "padding" | "border-width" | "border-style" | "border-color" => {
            (prop.into(), rotate_box_values(value))
        }
        _ => (prop.into(), value.into()),
    }
}

/// `[top, right, bottom, left]` → `[left, top, right, bottom]`（竖排后的新 top 是旧 left）。
fn rotate_box_values(value: &str) -> String {
    let vals: Vec<&str> = value.split_whitespace().collect();
    let new = match vals.len() {
        1 => vals,
        2 => vec![vals[1], vals[0]],
        3 => vec![vals[1], vals[0], vals[1], vals[2]],
        4 => vec![vals[3], vals[0], vals[1], vals[2]],
        _ => return value.to_string(),
    };
    new.join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn e(name: &str, data: &str) -> Entry {
        Entry {
            name: name.into(),
            data: data.as_bytes().to_vec(),
        }
    }

    fn book(opf_extra: &str, chapters: &[&str]) -> Vec<Entry> {
        let items: String = chapters
            .iter()
            .enumerate()
            .map(|(i, _)| {
                format!(r#"<item id="c{i}" href="c{i}.xhtml" media-type="application/xhtml+xml"/>"#)
            })
            .collect();
        let spine: String = (0..chapters.len())
            .map(|i| format!(r#"<itemref idref="c{i}"/>"#))
            .collect();
        let mut v = vec![e(
            "content.opf",
            &format!(
                r#"<package version="3.0"><metadata xmlns:dc="http://purl.org/dc/elements/1.1/"><dc:title>书</dc:title>{opf_extra}</metadata><manifest>{items}</manifest><spine>{spine}</spine></package>"#
            ),
        )];
        for (i, c) in chapters.iter().enumerate() {
            v.push(e(&format!("c{i}.xhtml"), c));
        }
        v
    }

    #[test]
    fn zh_text_detection_follows_language_and_scripts() {
        let zh = "这是一本中文书。".repeat(100);
        let ja = "これは日本語の本です。".repeat(100);
        assert!(is_zh_text(&book(
            "<dc:language>zh-CN</dc:language>",
            &[&zh]
        )));
        assert!(is_zh_text(&book(
            "<dc:language>zh-TW</dc:language>",
            &[&zh]
        )));
        assert!(is_zh_text(&book("<dc:language>en</dc:language>", &[&zh])));
        assert!(!is_zh_text(&book("<dc:language>ja</dc:language>", &[&ja])));
        assert!(!is_zh_text(&book(
            "<dc:language>zh</dc:language>",
            &[&"hello world ".repeat(200)]
        )));
        let half = format!("{}{}", zh, "hello world ".repeat(200));
        assert!(!is_zh_text(&book(
            "<dc:language>zh</dc:language>",
            &[&half]
        )));
        assert!(!is_zh_text(&book(
            "<dc:language>zh</dc:language><meta property=\"rendition:layout\">pre-paginated</meta>",
            &[&zh]
        )));
    }

    #[test]
    fn native_vertical_is_detected() {
        let zh = "<html><head><style>p{color:red}</style></head><body><p>正文</p></body></html>";
        assert!(!is_natively_vertical(&book(
            "<dc:language>zh</dc:language>",
            &[zh]
        )));
        let css = vec![
            e("O/content.opf", "<package/>"),
            e("O/s.css", "html{writing-mode:vertical-rl}"),
        ];
        assert!(is_natively_vertical(&css));
        let inline =
            "<html><body style=\"-webkit-writing-mode:vertical-rl\"><p>正文</p></body></html>";
        assert!(is_natively_vertical(&book(
            "<dc:language>zh</dc:language>",
            &[inline]
        )));
        let table_only = vec![
            e("O/content.opf", "<package/>"),
            e("O/s.css", "table{writing-mode:vertical-rl}"),
        ];
        assert!(!is_natively_vertical(&table_only));
    }

    #[test]
    fn quotes_convert_only_complete_pairs_in_a_paragraph() {
        assert_eq!(convert_quotes("<p>他说“好”。</p>"), "<p>他说『好』。</p>");
        assert_eq!(convert_quotes("<p>“外‘内’外”</p>"), "<p>『外「内」外』</p>");
        assert_eq!(convert_quotes("<p>他说“好</p>"), "<p>他说“好</p>");
        assert_eq!(convert_quotes("<p>“Hello”</p>"), "<p>“Hello”</p>");
        assert_eq!(convert_quotes("<p>已是「」『』</p>"), "<p>已是「」『』</p>");
    }

    #[test]
    fn straight_quotes_and_apostrophes() {
        assert_eq!(convert_quotes("<p>他说\"好\"</p>"), "<p>他说『好』</p>");
        assert_eq!(convert_quotes("<p>'甲'和'乙'</p>"), "<p>「甲」和「乙」</p>");
        assert_eq!(
            convert_quotes("<p>\"外'内'外\"</p>"),
            "<p>『外「内」外』</p>"
        );
        assert_eq!(
            convert_quotes("<p>don't 90's rock'n'roll</p>"),
            "<p>don't 90's rock'n'roll</p>"
        );
        assert_eq!(convert_quotes("<p>12\" 和 5'</p>"), "<p>12\" 和 5'</p>");
        assert_eq!(
            convert_quotes("<p>甲\"乙</p><p>丙\"丁</p>"),
            "<p>甲\"乙</p><p>丙\"丁</p>",
            "配对不跨段落"
        );
        assert_eq!(
            convert_quotes("<p>他说“don’t worry”就走了，“这里是中文”。</p>"),
            "<p>他说“don’t worry”就走了，『这里是中文』。</p>",
            "弯撇号按撇号算，不打断同段别的弯引号；全西文的弯引号仍不转"
        );
        assert_eq!(
            convert_quotes("<p><code>\"不转\"</code>\"转\"</p>"),
            "<p><code>\"不转\"</code>『转』</p>"
        );
    }

    #[test]
    fn fullwidth_text_only_and_entities() {
        assert_eq!(fullwidth_ascii("<p>是2026年</p>"), "<p>是２０２６年</p>");
        assert_eq!(
            fullwidth_ascii("<p>与KOReader</p>"),
            "<p>与ＫＯＲｅａｄｅｒ</p>"
        );
        assert_eq!(fullwidth_ascii("<p>US$6</p>"), "<p>ＵＳ＄６</p>");
        assert_eq!(
            fullwidth_ascii(r#"<p><a href="x.html#n1">1</a></p>"#),
            r#"<p><a href="x.html#n1">１</a></p>"#
        );
        assert_eq!(
            fullwidth_ascii("<p><code>abc</code><rt>abc</rt><style>a{}</style></p>"),
            "<p><code>abc</code><rt>abc</rt><style>a{}</style></p>"
        );
        assert_eq!(
            fullwidth_ascii("<p>&amp;&lt;&#x41;&#x4E2D;</p>"),
            "<p>＆＜Ａ&#x4E2D;</p>"
        );
        assert_eq!(
            fullwidth_ascii("<p>Hello world</p>"),
            "<p>Ｈｅｌｌｏ ｗｏｒｌｄ</p>"
        );
        assert_eq!(
            fullwidth_ascii("<p><!-- abc --></p>"),
            "<p><!-- abc --></p>"
        );
    }

    #[test]
    fn ncx_text_gets_quotes_and_fullwidth_but_attributes_do_not() {
        let ncx = r#"<ncx><docTitle><text>第1章 "标题"</text></docTitle><navMap><navPoint id="n1" playOrder="1"><navLabel><text>1. KOReader</text></navLabel><content src="c1.xhtml"/></navPoint></navMap></ncx>"#;
        let out = transform_ncx(ncx, VerticalAscii::Fullwidth);
        assert!(out.contains("<text>第１章 『标题』</text>"), "{out}");
        assert!(out.contains("<text>１． ＫＯＲｅａｄｅｒ</text>"), "{out}");
        assert!(out.contains(r#"id="n1" playOrder="1""#), "{out}");
        assert!(out.contains(r#"src="c1.xhtml""#), "{out}");
    }

    #[test]
    fn vertical_css_root_and_table_rules() {
        let css = vertical_css(false);
        assert!(css.contains("html{writing-mode:vertical-rl;-webkit-writing-mode:vertical-rl;-epub-writing-mode:vertical-rl;}"));
        assert!(css.contains("table{writing-mode:horizontal-tb"));
        assert!(!vertical_css(true).contains("html{"));
        for line in css.lines() {
            assert!(line.ends_with('}'), "{line}");
        }
    }

    #[test]
    fn book_css_boxes_rotate_and_complex_blocks_stay() {
        let (out, complex) = rewrite_css(
            "p{margin-top:1em;margin-right:2em;padding-bottom:3px;width:50%;float:left;text-align:center;text-indent:2em}\n\
             div{position:absolute;margin-top:1em}\n\
             img{max-width:100%}\n\
             .x{margin:1em 2em 3em 4em}\n\
             .b{border-top-width:2px;border-left-style:solid;border-bottom-color:red;border-top-left-radius:4px}",
        );
        assert!(out.contains("p{margin-right:1em;margin-bottom:2em;padding-left:3px;height:50%;text-align:center;text-indent:2em;}"), "{out}");
        assert!(
            out.contains("div{position:absolute;margin-top:1em}"),
            "{out}"
        );
        assert!(out.contains("img{max-height:100%;}"), "{out}");
        assert!(out.contains(".x{margin:4em 1em 2em 3em;}"), "{out}");
        assert!(
            out.contains(".b{border-right-width:2px;border-top-style:solid;border-left-color:red;border-top-right-radius:4px;}"),
            "{out}"
        );
        assert_eq!(complex, 1);
    }
}
