//! 体检：一本书用到的 CSS 写法里，哪些在这台阅读器上和 Kindle 不一样又没处理（broken）、哪些没验证过（怪癖表里没有）。
//!
//! 怪癖表是 `data/css-quirks.toml`（真机、测试书验证过的结论，`tools/quirks/probe.py` 量的写回那里）。按 [`crate::kindle_model::Chapter`]
//! 层叠，每个看得见的元素上生效的每条声明归成「属性 · 元素 · 值 · 写法」查表，同样的合起来计元素数。只报告、不改书
//! （2026-10-13 定：sync 时顺带提示，清单记进生成记录，`booklib list` 看）。
use crate::cascade::{MediaEnv, Origin};
use crate::epubzip::Entry;
use crate::kindle_model::{book_css, Chapter};
use serde::Deserialize;
use std::collections::BTreeMap;
use std::sync::OnceLock;

/// 一种写法在这台上的结论。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Verdict {
    /// 和 Kindle 不一样、没处理。
    Broken,
    /// 怪癖表里没有。
    Unknown,
}

/// 一种写法（同样的合在一起）。
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Finding {
    pub verdict: Verdict,
    /// 属性。
    pub prop: String,
    /// 元素：`p`、`heading`、`block`、`inline`、`img`、`cell`（表格单元格）、`list`（列表项）。
    pub on: &'static str,
    /// 值：`em`、`rem`、`px`、`pt`、`pct`、`number`、`zero`、`neg`、`keyword`；边框不分（`-`）。
    pub value: &'static str,
    /// 写法：`sheet`、`style`、`inline`（带 `!important` 的另加 `!`）。
    pub via: String,
    /// 用到它的元素数。
    pub count: usize,
    /// 怪癖表的说明（没验证过的空）。
    pub note: String,
}

impl Finding {
    /// 一行：`✗ <style> 里 text-indent（em，p）×504：说明` / `? letter-spacing（em，p）×3`。
    pub fn line(&self) -> String {
        let via = match self.via.trim_end_matches('!') {
            "style" => "<style> 里 ",
            "inline" => "行内 style ",
            _ => "",
        };
        let imp = if self.via.ends_with('!') { " !important" } else { "" };
        let mark = if self.verdict == Verdict::Broken { '✗' } else { '?' };
        let note = if self.note.is_empty() { String::new() } else { format!("：{}", self.note) };
        format!("{mark} {via}{}{imp}（{}）×{}{note}", self.prop, kind_on(self.value, self.on), self.count)
    }
}

#[derive(Deserialize)]
struct Table {
    rule: Vec<Row>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Row {
    prop: Vec<String>,
    #[serde(default)]
    on: Vec<String>,
    #[serde(default)]
    value: Vec<String>,
    #[serde(default)]
    via: Vec<String>,
    ireader: Option<String>,
    #[serde(default)]
    note: String,
    #[allow(dead_code)]
    src: String,
}

impl Row {
    fn status(&self, reader: &str) -> Option<&str> {
        match reader {
            "ireader" => self.ireader.as_deref(),
            _ => None,
        }
    }

    fn matches(&self, prop: &str, on: &str, value: &str, via: Origin, important: bool) -> bool {
        let any = |list: &[String], v: &str| list.is_empty() || list.iter().any(|x| x == v);
        let via_ok = self.via.is_empty() || self.via.iter().any(|v| v == via_name(via) || (v == "important" && important));
        self.prop.iter().any(|p| match p.strip_suffix('*') {
            Some(pre) => prop.starts_with(pre),
            None => p == prop,
        }) && any(&self.on, on)
            && any(&self.value, value)
            && via_ok
    }
}

fn via_name(o: Origin) -> &'static str {
    match o {
        Origin::Sheet => "sheet",
        Origin::Style => "style",
        Origin::Inline => "inline",
    }
}

fn table() -> &'static Table {
    static T: OnceLock<Table> = OnceLock::new();
    T.get_or_init(|| toml::from_str(include_str!("../data/css-quirks.toml")).expect("css-quirks.toml 格式不对（单测会先报）"))
}

/// 怪癖表里有这台的一栏（内置的 `ireader`；别的模式不体检）。
pub fn covers(reader: &str) -> bool {
    reader == "ireader"
}

/// 元素归类。
fn element_kind(tag: &str, is_block: bool) -> &'static str {
    match tag {
        "p" => "p",
        "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => "heading",
        "img" | "svg" | "image" => "img",
        "td" | "th" => "cell",
        "li" => "list",
        _ if is_block => "block",
        _ => "inline",
    }
}

/// 报告用的属性名；不起作用的不报（`None`）：行内元素上的 `text-indent`、`text-align`（只对块起作用），等于没写的
/// `font-size:1em`/`100%`。边框的分项按边合成 `border-top`/`border-right`/`border-bottom`/`border-left`（宽度、样式、颜色），
/// 圆角合成 `border-radius`。
fn reported_prop(prop: &str, on: &str, v: &str) -> Option<String> {
    if on == "inline" && matches!(prop, "text-indent" | "text-align") {
        return None;
    }
    if prop == "font-size" && matches!(v.trim().to_ascii_lowercase().as_str(), "1em" | "100%") {
        return None;
    }
    // 拼错的分项（《春雪》的 `padding-botton`）：哪台都不认、Kindle 也不认，一样
    for pre in ["margin-", "padding-"] {
        if let Some(side) = prop.strip_prefix(pre) {
            if !matches!(side, "top" | "right" | "bottom" | "left" | "inline" | "block" | "inline-start" | "inline-end" | "block-start" | "block-end") {
                return None;
            }
        }
    }
    if prop.ends_with("-radius") {
        return Some("border-radius".to_string());
    }
    for side in ["top", "right", "bottom", "left"] {
        if prop.starts_with(&format!("border-{side}")) {
            return Some(format!("border-{side}"));
        }
    }
    Some(prop.to_string())
}

/// 值归类：看第一个词。
fn value_kind(v: &str) -> &'static str {
    let t = v.trim().to_ascii_lowercase();
    let tok = t.split_whitespace().next().unwrap_or("");
    let end = tok.find(|c: char| !(c.is_ascii_digit() || c == '.' || c == '-' || c == '+')).unwrap_or(tok.len());
    let Ok(n) = tok[..end].parse::<f64>() else { return "keyword" };
    let unit = &tok[end..];
    if n == 0.0 {
        return "zero";
    }
    if n < 0.0 && !unit.is_empty() {
        return "neg";
    }
    match unit {
        "em" | "ex" | "ch" => "em",
        "rem" => "rem",
        "px" => "px",
        "%" => "pct",
        "pt" | "pc" | "in" | "cm" | "mm" | "q" => "pt",
        "" => "number",
        _ => "keyword",
    }
}

/// 给 `reader` 这台体检一本书（`entries` 是要优化的那份，`kindle_rules` 改写之前）：没处理的不一样、没验证过的写法，
/// 不一样的在前、同一类里用得多的在前。
pub fn audit(entries: &[Entry], media: Option<&MediaEnv>, reader: &str) -> Vec<Finding> {
    if !covers(reader) {
        return Vec::new();
    }
    let css = book_css(entries, media);
    let chapters: Vec<&Entry> = entries.iter().filter(|e| crate::wash::is_chapter_entry(e)).collect();
    // (属性, 元素, 值, 来源, !important) → 元素数
    type Key = (String, &'static str, &'static str, Origin, bool);
    let counts: Vec<BTreeMap<Key, usize>> = crate::util::par_map(&chapters, |e| {
        let mut m = BTreeMap::new();
        let Ok(text) = std::str::from_utf8(&e.data) else { return m };
        let Some(ch) = Chapter::parse(&e.name, text, media, &css) else { return m };
        for (i, el) in ch.els.iter().enumerate() {
            if el.hidden {
                continue;
            }
            let on = element_kind(ch.el(i).value().name(), el.is_block);
            // 看得见的边框（有宽度、样式不是 none/hidden、不是全透明）才算：`border:0`、`border-width:0` 不报
            let visible = |side: usize| {
                el.comp.border[side].as_ref().is_some_and(|b| b.width != crate::cascade::BorderWidth::Pt(0.0) && b.width != crate::cascade::BorderWidth::Em(0.0) && b.color.is_none_or(|c| c >> 24 != 0))
            };
            let marker = el.comp.list_style.as_deref() != Some("none") && el.comp.display.as_deref().is_none_or(|v| v.eq_ignore_ascii_case("list-item"));
            // 同一个元素上同一种写法只计一次（边框的分项合成了一个）
            let mut keys: std::collections::BTreeSet<Key> = std::collections::BTreeSet::new();
            for (prop, v) in &el.decls {
                let Some(name) = reported_prop(prop, on, v) else { continue };
                // 没有项目符号的列表项（`list-style-type:none`、`display` 不是 list-item）：左右内边距同普通块（并进外边距，box_fix）
                let on = if on == "list" && !marker && matches!(name.as_str(), "padding-left" | "padding-right") { "block" } else { on };
                let side = ["border-top", "border-right", "border-bottom", "border-left"].iter().position(|s| *s == name);
                if side.is_some_and(|s| !visible(s)) || (name == "border-radius" && !(0..4).any(visible)) {
                    continue;
                }
                let (origin, imp) = el.origin.get(prop).copied().unwrap_or((Origin::Sheet, false));
                // 边框的分项有宽度、样式、颜色，不分值的种类
                let kind = if name.starts_with("border") { "-" } else { value_kind(v) };
                keys.insert((name, on, kind, origin, imp));
            }
            for k in keys {
                *m.entry(k).or_default() += 1;
            }
        }
        m
    });
    let mut all: BTreeMap<Key, usize> = BTreeMap::new();
    for m in counts {
        for (k, n) in m {
            *all.entry(k).or_default() += n;
        }
    }
    let t = table();
    let mut out: Vec<Finding> = all
        .into_iter()
        .filter_map(|((prop, on, value, origin, imp), count)| {
            // 第一条对得上、而且这台有结论的（某台专有的条目可以写在前面，不挡别的台）
            let row = t.rule.iter().find(|r| r.matches(&prop, on, value, origin, imp) && r.status(reader).is_some());
            let verdict = match row.and_then(|r| r.status(reader)) {
                Some("broken") => Verdict::Broken,
                Some(_) => return None,
                None => Verdict::Unknown,
            };
            let note = match (verdict, row) {
                (Verdict::Broken, Some(r)) => r.note.clone(),
                _ => String::new(),
            };
            let via = format!("{}{}", via_name(origin), if imp { "!" } else { "" });
            Some(Finding { verdict, prop, on, value, via, count, note })
        })
        .collect();
    out.sort_by(|a, b| a.verdict.cmp(&b.verdict).then(b.count.cmp(&a.count)).then_with(|| a.cmp(b)));
    out
}

/// sync 时打的摘要（最多两行）：不一样又没处理的前几种、没验证过的有几种。没有就空。
pub fn summary(findings: &[Finding], reader: &str) -> Vec<String> {
    const SHOW: usize = 4;
    let mut out = Vec::new();
    let broken: Vec<&Finding> = findings.iter().filter(|f| f.verdict == Verdict::Broken).collect();
    if !broken.is_empty() {
        let shown: Vec<String> = broken.iter().take(SHOW).map(|f| short(f)).collect();
        let more = if broken.len() > SHOW { format!(" 等 {} 种", broken.len()) } else { String::new() };
        out.push(format!("体检：{reader} 上和 Kindle 不一样、没处理：{}{more}（booklib list 看全部）", shown.join("；")));
    }
    let unknown: Vec<&Finding> = findings.iter().filter(|f| f.verdict == Verdict::Unknown).collect();
    if !unknown.is_empty() {
        let shown: Vec<String> = unknown.iter().take(3).map(|f| short(f)).collect();
        out.push(format!("体检：{reader} 上没验证过的写法 {} 种（最多的：{}）", unknown.len(), shown.join("；")));
    }
    out
}

fn short(f: &Finding) -> String {
    let via = match f.via.trim_end_matches('!') {
        "style" => "<style> ",
        "inline" => "行内 ",
        _ => "",
    };
    format!("{via}{}（{}）×{}", f.prop, kind_on(f.value, f.on), f.count)
}

/// `值，元素`；不分值的（边框）只写元素。
fn kind_on(value: &str, on: &str) -> String {
    if value == "-" { on.to_string() } else { format!("{value}，{on}") }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_parses_and_statuses_are_known() {
        for r in &table().rule {
            if let Some(s) = &r.ireader {
                assert!(["same", "fixed", "accept", "broken"].contains(&s.as_str()), "怪癖表状态 {s}");
            }
            for v in &r.value {
                assert!(["em", "rem", "px", "pt", "pct", "number", "zero", "neg", "keyword"].contains(&v.as_str()), "值 {v}");
            }
            for v in &r.via {
                assert!(["sheet", "style", "inline", "important"].contains(&v.as_str()), "写法 {v}");
            }
            for v in &r.on {
                assert!(["p", "heading", "block", "inline", "img", "cell", "list"].contains(&v.as_str()), "元素 {v}");
            }
            assert!(!r.src.is_empty());
        }
    }

    /// 体检能查到的所有写法（属性 × 元素 × 值 × 来源 × `!important`）：属性取表里写的，通配的 `margin-*` 另取一个只有它能对上的名字。
    fn all_keys() -> Vec<(String, &'static str, &'static str, Origin, bool)> {
        let mut props: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
        for r in &table().rule {
            for p in &r.prop {
                props.insert(p.strip_suffix('*').map_or_else(|| p.clone(), |pre| format!("{pre}~")));
            }
        }
        let ons = ["p", "heading", "block", "inline", "img", "cell", "list"];
        let values = ["em", "rem", "px", "pt", "pct", "number", "zero", "neg", "keyword", "-"];
        let mut out = Vec::new();
        for p in &props {
            for on in ons {
                for value in values {
                    for origin in [Origin::Sheet, Origin::Style, Origin::Inline] {
                        for imp in [false, true] {
                            out.push((p.clone(), on, value, origin, imp));
                        }
                    }
                }
            }
        }
        out
    }

    /// 一种写法在这台上由哪一条定（同 [`audit`]：第一条对得上、这台有结论的）。
    fn deciding(key: &(String, &'static str, &'static str, Origin, bool), reader: &str) -> Option<usize> {
        let (prop, on, value, origin, imp) = key;
        table().rule.iter().position(|r| r.matches(prop, on, value, *origin, *imp) && r.status(reader).is_some())
    }

    /// 怪癖表里没有用不上的条目：每条在它写了结论的每台上，至少有一种写法是由它定的（排在前面更宽的条目整条挡住了它，
    /// 就是死条目，挪顺序时最容易出）。另外每条都有说明。
    /// `QUIRKS_SNAPSHOT=文件` 时把每台每种写法的结论写进文件：整理表（挪顺序、合并、改分节）前后各写一份，应该一字不差。
    #[test]
    fn no_dead_rows_and_snapshot() {
        let keys = all_keys();
        if let Some(path) = std::env::var_os("QUIRKS_SNAPSHOT") {
            let mut s = String::new();
            for k in &keys {
                let v = deciding(k, "ireader").and_then(|i| table().rule[i].status("ireader")).unwrap_or("?");
                s.push_str(&format!("{} {} {} {}{} {v}\n", k.0, k.1, k.2, via_name(k.3), if k.4 { "!" } else { "" }));
            }
            std::fs::write(path, s).unwrap();
        }
        let decided: Vec<(&str, std::collections::HashSet<usize>)> = ["ireader"].iter().map(|d| (*d, keys.iter().filter_map(|k| deciding(k, d)).collect())).collect();
        for (i, r) in table().rule.iter().enumerate() {
            assert!(!r.note.is_empty(), "第 {} 条 {:?} 没写说明", i + 1, r.prop);
            for (reader, used) in &decided {
                if r.status(reader).is_some() {
                    assert!(used.contains(&i), "第 {} 条 {:?} 在 {reader} 上被前面的条目整条挡住", i + 1, r.prop);
                }
            }
        }
    }

    #[test]
    fn value_kinds() {
        let k: Vec<&str> = ["2em", "1.5rem", "10px", "12pt", "50%", "1.2", "0", "-1em", "bold", "#000", "0.01em", "-2"].iter().map(|v| value_kind(v)).collect();
        assert_eq!(k, ["em", "rem", "px", "pt", "pct", "number", "zero", "neg", "keyword", "keyword", "em", "number"]);
    }

    fn book(chapter: &str, css: &str) -> Vec<Entry> {
        vec![
            Entry { name: "O/content.opf".into(), data: br#"<package><manifest><item id="c" href="c.xhtml" media-type="application/xhtml+xml"/></manifest><spine><itemref idref="c"/></spine></package>"#.to_vec() },
            Entry { name: "O/s.css".into(), data: css.as_bytes().to_vec() },
            Entry { name: "O/c.xhtml".into(), data: chapter.as_bytes().to_vec() },
        ]
    }

    /// 掌阅：列表项上 px 写的左右外边距没处理（报 ✗）；没验证过的另算（报 ?）；`!important`（已处理）、隐藏的不报。
    #[test]
    fn flags_unhandled_and_skips_fixed() {
        let ch = r#"<html xmlns="http://www.w3.org/1999/xhtml"><head><link rel="stylesheet" href="s.css"/></head>
<body><ul><li style="margin-left:10px">一</li><li style="margin-left:10px">二</li></ul><p class="b">三</p><p class="i">四</p><ul class="h"><li style="margin-left:10px">五</li></ul></body></html>"#;
        let b = book(ch, "p.b{word-spacing:0.1em;} p.i{text-align:center !important;} .h{display:none;}");
        let f = audit(&b, None, "ireader");
        let lines: Vec<String> = f.iter().map(Finding::line).collect();
        assert!(lines[0].starts_with("✗ 行内 style margin-left（px，list）×2："), "隐藏的那项不算：{lines:?}");
        assert_eq!(lines[1], "? word-spacing（em，p）×1", "{lines:?}");
        assert_eq!(lines.len(), 2, "!important 已处理、不报：{lines:?}");
        assert!(audit(&b, None, "kindle").is_empty());
        let s = summary(&f, "ireader");
        assert_eq!(s.len(), 2);
        assert!(s[0].starts_with("体检：ireader 上和 Kindle 不一样、没处理：行内 margin-left（px，list）×2"), "{s:?}");
    }
}
