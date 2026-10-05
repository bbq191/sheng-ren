//! CSS 声明处理：按属性黑名单剥声明、`text-indent` 归一（`filter_css`）、html 内联 style/`<style>` 清洗（`wash_html`）。
use super::*;

// ───────────────────────── 2–4. CSS 声明处理 ─────────────────────────

/// CSS 规则 `选择器{声明}`（只匹配最内层：`@media{}` 里的规则由"从内向外"匹配到）。`filter_css`、章尾容器
/// 去下边距（`layout::strip_tail_spacing`）、`layout::Drawn`、`typeset::indent_classes_of` 共用。
/// ⚠ 选择器（第 1 组）会带上前面的 `/* … */` 注释：拿它判断之前先过 [`strip_css_comments`]，写回仍用原文。
pub(super) fn css_rule_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r#"(?s)([^{}]+)\{([^{}]*)\}"#).unwrap())
}

/// 去掉 `/* … */` 注释（换成一个空格；没闭合的去到末尾）。只用来判断选择器（2026-09-30 审计：`/* p 的边距 */ .note{…}`
/// 被当成 p 规则改了边距，`/* footnote */` 让普通规则被当成注释容器，`/* fonts */ @font-face{…}` 没认出是 @font-face、字体名被剥）。
pub(super) fn strip_css_comments(s: &str) -> Cow<'_, str> {
    if !s.contains("/*") {
        return Cow::Borrowed(s);
    }
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find("/*") {
        out.push_str(&rest[..i]);
        out.push(' ');
        rest = rest[i + 2..].find("*/").map_or("", |j| &rest[i + 2 + j + 2..]);
    }
    out.push_str(rest);
    Cow::Owned(out)
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum Spacing {
    /// 不动 margin/padding。
    Keep,
    /// 上下归零、左右保留（p/div）。
    Vertical,
    /// 全部删除（body/html/@page）。
    All,
}

/// 对一段声明文本：剥 `filter` 里的属性；按 `spacing` 处理 margin/padding。（测试用薄封装）
#[cfg(test)]
pub(super) fn filter_decls(decls: &str, filter: &[String], spacing: Spacing) -> String {
    filter_decls_with(decls, filter, spacing, false, None, &HashSet::new())
}

/// 值是否"非零缩进"（`0` / `0em` / `0.0pt` 之类算零；负值=悬挂缩进，保留不动）。
pub(super) fn is_positive_indent(val: &str) -> bool {
    let v = val.trim().trim_end_matches("!important").trim();
    let num: String = v.chars().take_while(|c| c.is_ascii_digit() || *c == '.' || *c == '-' || *c == '+').collect();
    num.parse::<f64>().map(|n| n > 0.0).unwrap_or(false)
}

/// 同 `filter_decls`，另把书里**非零** `text-indent` 统一改成 `indent`（Some 时）。
/// 为什么：书自带的类规则（calibre 转 AZW3 常见 `.calibre_ {text-indent:2em}`）xochitl 不认（只认裸 `p{}`），
/// 按标准 CSS 渲染的阅读器认、且类规则特异性高于我们的 `p{}`——不统一就同一本书在 xochitl 上 1.2em、别的阅读器上 2em
/// （2026-09-06 Phase E 英文书对照发现）。`text-indent:0`（诗歌/引文/列表明示不缩进）与负值保留。
/// `filter` 里的属性按 [`crate::cssunlock::unlock`] 解锁（字体去掉、相对字号保留、`font`/`background` 简写只留样式和颜色……）；
/// `base_text` = 这条规则作用在正文整体那一层（见 [`is_base_text_selector`]）。
pub(super) fn filter_decls_with(decls: &str, filter: &[String], spacing: Spacing, base_text: bool, indent: Option<&str>, keep_fonts: &HashSet<String>) -> String {
    use crate::cssunlock::{unlock, Unlock};
    let mut out: Vec<String> = Vec::new();
    // 声明按分号切，引号/括号（`url(data:…;base64,…)`）/字符引用里的分号不算（`html::css_decls`）。
    for d in html::css_decls(decls) {
        let prop = d.prop.to_ascii_lowercase();
        let val = d.value;
        // 保留背景图的模式（过滤清单里有 `background`、没有 `background-image`）：简写拆成分项（见 `background_longhands`）
        if prop == "background" && filter.contains(&prop) && !filter.iter().any(|f| f == "background-image") {
            out.extend(crate::cssunlock::background_longhands(val, !filter.iter().any(|f| f == "background-size")));
            continue;
        }
        if filter.contains(&prop) {
            match unlock(&prop, val, base_text, keep_fonts) {
                Unlock::Keep => {}
                Unlock::Drop => continue,
                Unlock::Replace(v) => {
                    out.extend(v);
                    continue;
                }
            }
        }
        // 占满一屏的高度（书名页/封面页常用）加上页眉页脚会溢出成空白页（章尾空白页，2026-09-27）。
        if (prop == "height" || prop == "min-height") && val.to_ascii_lowercase().contains("vh") {
            continue;
        }
        if prop == "text-indent" {
            if let Some(ind) = indent {
                if is_positive_indent(val) {
                    out.push(format!("text-indent:{ind}"));
                    continue;
                }
            }
        }
        let is_box = prop == "margin" || prop == "padding";
        let is_box_side = prop.starts_with("margin-") || prop.starts_with("padding-");
        match spacing {
            Spacing::Keep => {}
            Spacing::All if is_box || is_box_side => continue,
            Spacing::Vertical if prop.ends_with("-top") || prop.ends_with("-bottom") => {
                if is_box_side {
                    continue;
                }
            }
            Spacing::Vertical if is_box => {
                // 简写：保留左右
                match box_sides(val) {
                    BoxSides::Sides([_, r, _, l], important) => {
                        out.push(if r == l { format!("{prop}:0 {r}{important}") } else { format!("{prop}:0 {r} 0 {l}{important}") });
                    }
                    // `margin:inherit` 之类：上下交给我们的 `p{}`，左右照原值写成分项（此前写成非法的 `margin:0 inherit`）
                    BoxSides::Keyword(k, important) => out.push(format!("{prop}-right:{k}{important};{prop}-left:{k}{important}")),
                    // 拆不清（calc() 里带空格之类）：拿不准就原样留着
                    BoxSides::Unknown => out.push(format!("{prop}:{val}")),
                }
                continue;
            }
            _ => {}
        }
        out.push(format!("{prop}:{val}"));
    }
    // 尾分号：xochitl 丢规则里最后一个无分号的声明（书 css `.calibre_ {…;margin:0}` 的 margin 曾被吞；若 text-indent 排最后就没缩进）
    let mut joined = out.join(";");
    if !joined.is_empty() {
        joined.push(';');
    }
    joined
}

/// `margin`/`padding` 简写拆成的四边。
#[derive(Debug, PartialEq, Eq)]
pub(super) enum BoxSides<'a> {
    /// 上、右、下、左，和 `" !important"`（没有就是空串）。
    Sides([&'a str; 4], &'static str),
    /// 整体一个全局关键字（`inherit`/`initial`/`unset`/`revert`），不能跟别的边混写在一个简写里。
    Keyword(&'a str, &'static str),
    /// 拆不清（超过 4 个值、括号不配对）。
    Unknown,
}

/// 拆 `margin`/`padding` 简写的值（`css.rs` 段距归零与 `layout.rs` 章尾去下边距共用）：1–4 个值按 CSS 规则展开成四边；
/// 括号里的空格不算分隔（`calc(1em + 2px)`）。
pub(super) fn box_sides(val: &str) -> BoxSides<'_> {
    let v = val.trim();
    let lower = v.to_ascii_lowercase();
    let (v, important) = match lower.rfind('!') {
        Some(i) if lower[i + 1..].trim() == "important" => (v[..i].trim_end(), " !important"),
        _ => (v, ""),
    };
    let mut parts: Vec<&str> = Vec::new();
    let (mut depth, mut start) = (0i32, None::<usize>);
    for (i, ch) in v.char_indices() {
        match ch {
            '(' => depth += 1,
            ')' => depth -= 1,
            c if c.is_whitespace() && depth == 0 => {
                if let Some(s) = start.take() {
                    parts.push(&v[s..i]);
                }
                continue;
            }
            _ => {}
        }
        start.get_or_insert(i);
    }
    if let Some(s) = start {
        parts.push(&v[s..]);
    }
    if depth != 0 {
        return BoxSides::Unknown;
    }
    let keyword = |p: &str| matches!(p.to_ascii_lowercase().as_str(), "inherit" | "initial" | "unset" | "revert" | "revert-layer");
    match parts[..] {
        [k] if keyword(k) => BoxSides::Keyword(k, important),
        _ if parts.iter().any(|p| keyword(p)) => BoxSides::Unknown,
        [a] => BoxSides::Sides([a, a, a, a], important),
        [a, b] => BoxSides::Sides([a, b, a, b], important),
        [a, b, c] => BoxSides::Sides([a, b, c, b], important),
        [a, b, c, d] => BoxSides::Sides([a, b, c, d], important),
        _ => BoxSides::Unknown,
    }
}

pub(super) fn selector_spacing(selector: &str) -> Spacing {
    static ELEM_P: OnceLock<Regex> = OnceLock::new();
    static ELEM_BODY: OnceLock<Regex> = OnceLock::new();
    let p = ELEM_P.get_or_init(|| Regex::new(r#"(?i)(^|[\s,>+~])(p|div)(?:[\s,.#:\[]|$)"#).unwrap());
    let b = ELEM_BODY.get_or_init(|| Regex::new(r#"(?i)(^|[\s,>+~])(body|html)(?:[\s,.#:\[]|$)|^@page\b"#).unwrap());
    let s = selector.trim();
    if b.is_match(s) {
        Spacing::All
    } else if p.is_match(s) {
        Spacing::Vertical
    } else {
        Spacing::Keep
    }
}

/// 选择器是不是作用在"正文整体那一层"：每个逗号分项的最后一个复合选择器都是不带类、id、属性的 `body`/`html`/`p`/`div`
/// （`body`、`div.chapter p`、`html, body` 算；`p.small`、`.note`、`h1` 不算）。这一层上的相对字号也去掉（[`crate::cssunlock`]）。
pub(super) fn is_base_text_selector(selector: &str) -> bool {
    let parts: Vec<&str> = selector.split(',').map(str::trim).filter(|p| !p.is_empty()).collect();
    !parts.is_empty()
        && parts.iter().all(|p| {
            let last = p.rsplit(|c: char| c.is_whitespace() || c == '>' || c == '+' || c == '~').next().unwrap_or("");
            matches!(last.to_ascii_lowercase().as_str(), "body" | "html" | "p" | "div" | ":root")
        })
}

/// 选择器是不是"注释容器类"：书自带的 `duokan-footnote-item`/`duokan-footnote-content` 这类，以及我们
/// 自己生成的 `.footnotes`（章末块）/`.eink-fnote`（Inline 内联注释）——判据是选择器文本含
/// "footnote"/"fnote"（大小写不敏感），不追求穷举每本书的命名，覆盖到目前真机见过的形态。
pub(super) fn is_footnote_container_selector(sel: &str) -> bool {
    let l = sel.to_ascii_lowercase();
    l.contains("footnote") || l.contains("fnote")
}

/// 注释容器专用的字号：比正文小一号（五号→小五是 0.857，用户 2026-09-29），相对单位（随用户当前字号缩放，不是又一个"锁死"）。
pub(super) const FOOTNOTE_FONT_SIZE: &str = "0.85em";

/// 整段 CSS（文件或 <style> 内容）：逐规则剥锁 + 边距处理。`@media{}` 嵌套靠"从内向外"匹配最内层规则。
/// 注释容器类是唯一的例外分支：§03av EPUB 线原则②"解锁字号但保留原书颜色/加粗"保护的是**正文语义
/// 加粗**，注释容器类的 `font-weight:bold` 是原书模板写死的装饰样式，不是语义强调——2026-09-23 真机
/// 《甲午：摇摆的战争》坐实（`duokan-footnote-item{font-weight:bold}` 导致全书注释永远加粗，用户反馈
/// "跳转注释后字体不对"，追下去发现其实是加粗不是字体），用户拍板"只剥注释容器类的字重，不碰正文；
/// 注释字号固定比正文小一档"。
pub fn filter_css(css: &str, opts: &WashOpts) -> String {
    css_rule_re().replace_all(css, |c: &regex::Captures| {
        // 规则前面的语句式 at-rule（`@import url(a.css);`、`@charset "utf-8";`）会被正则算进选择器里：拆出来原样保留，
        // 后面的才是真正的选择器（2026-09-28 审计：样式表开头的 `@import` 让紧跟的第一条规则整条跳过，字体锁没剥）。
        let (lead, sel) = split_leading_statements(&c[1]);
        // 判断用去掉注释的选择器；写回用原文（注释照留）。
        let clean = strip_css_comments(sel);
        let trimmed = clean.trim_start();
        if trimmed.starts_with("@font-face") || trimmed.starts_with("@import") {
            return c[0].to_string();
        }
        let spacing = match selector_spacing(&clean) {
            Spacing::Vertical if opts.keep_para_spacing => Spacing::Keep,
            s => s,
        };
        if is_footnote_container_selector(&clean) {
            let mut filter = opts.filter_props.clone();
            if !filter.iter().any(|p| p == "font-weight") {
                filter.push("font-weight".to_string());
            }
            // base_text=true：书自己的字号（哪怕是相对的）一律去掉，换成下面统一的注释字号
            // 注释容器的字体一律去掉（批注不用嵌入字体，用户 2026-10-05）。
            let mut decls = filter_decls_with(&c[2], &filter, spacing, true, Some(indent_for(opts)), &HashSet::new());
            decls.push_str(&format!("font-size:{FOOTNOTE_FONT_SIZE};"));
            return format!("{lead}{sel}{{{decls}}}");
        }
        format!("{lead}{}{{{}}}", sel, filter_decls_with(&c[2], &opts.filter_props, spacing, is_base_text_selector(&clean), Some(indent_for(opts)), &opts.keep_fonts))
    }).into_owned()
}

/// 选择器文本开头的语句式 at-rule（以 `@` 开头、到括号、引号和注释之外的 `;` 为止，可以有好几条，前后可以夹注释）
/// 拆成 (这些语句, 其余)。没有就是 `("", 原文)`。
pub(super) fn split_leading_statements(sel: &str) -> (&str, &str) {
    let mut cut = 0;
    loop {
        let rest = &sel[cut..];
        if !strip_css_comments(rest).trim_start().starts_with('@') {
            break;
        }
        let (mut depth, mut quote) = (0usize, None::<char>);
        let mut end = None;
        let mut skip_to = 0; // 注释结束处
        for (i, ch) in rest.char_indices() {
            if i < skip_to {
                continue;
            }
            if quote.is_none() && rest[i..].starts_with("/*") {
                skip_to = rest[i + 2..].find("*/").map_or(rest.len(), |j| i + 2 + j + 2);
                continue;
            }
            match (quote, ch) {
                (Some(q), c) if c == q => quote = None,
                (Some(_), _) => {}
                (None, '"' | '\'') => quote = Some(ch),
                (None, '(') => depth += 1,
                (None, ')') => depth = depth.saturating_sub(1),
                (None, ';') if depth == 0 => {
                    end = Some(i + 1);
                    break;
                }
                _ => {}
            }
        }
        match end {
            Some(e) => cut += e,
            None => break,
        }
    }
    (&sel[..cut], &sel[cut..])
}

/// 本书的首行缩进值（拉丁 1.2em / 中文 2em；Auto 兜底中文）。`wash_css` 与书 css 统一改写共用。
pub(super) fn indent_for(opts: &WashOpts) -> &'static str {
    if opts.lang == LangMode::Latin { "1.2em" } else { "2em" }
}

/// (x)html：`style=""`（按标签名定边距策略）+ `<style>` 块剥锁；注入清洗样式块；折叠重复 id。
pub fn wash_html(html: &str, opts: &WashOpts) -> (String, usize) {
    wash_html_with(html, opts, &HashSet::new())
}

/// 同 [`wash_html`]；`indent_classes` = 书的外链样式表里写了 `text-indent` 的类（`typeset::indent_classes_of`，英文首段顶格用；
/// 本文件 `<style>` 里的另外算上）。
pub(super) fn wash_html_with(html: &str, opts: &WashOpts, indent_classes: &HashSet<String>) -> (String, usize) {
    let before_dup = count_dup_id_tags(html);
    let s = collapse_dup_id_attrs(html);
    // 只认名字正好是 `style` 的属性（`data-style`、SVG `font-style` 不算），就地改值、保留原引号。
    let s = html::edit_attrs(&s, &["style"], |t, a| {
        let spacing = match t.name.to_ascii_lowercase().as_str() {
            "body" | "html" => Spacing::All,
            "p" | "div" if !opts.keep_para_spacing => Spacing::Vertical,
            _ => Spacing::Keep,
        };
        let base_text = matches!(t.name.to_ascii_lowercase().as_str(), "body" | "html");
        let cleaned = filter_decls_with(a.value, &opts.filter_props, spacing, base_text, Some(indent_for(opts)), &opts.keep_fonts);
        if cleaned.is_empty() {
            Edit::Remove
        } else if cleaned == a.value {
            Edit::Keep
        } else {
            Edit::Set(cleaned)
        }
    })
    .into_owned();
    let s = html::style_block_re().replace_all(&s, |c: &regex::Captures| {
        if c[1].contains(WASH_MARK) {
            // 旧版（v9 及以前）注入的内联 <style class="eink-wash"> 块：xochitl 本就无视它，重洗时清掉（已改外链 css）。
            String::new()
        } else {
            format!("{}{}{}", &c[1], filter_css(&c[2], opts), &c[3])
        }
    }).into_owned();
    // 不再注入内联 <style>（xochitl 无视内联）；排版规则由 wash_entries 写成外链 css + 逐 html 加 <link>。
    let s = match opts.lang {
        LangMode::Latin => {
            let mut classes = indent_classes.clone();
            for c in html::style_block_re().captures_iter(&s) {
                classes.extend(indent_classes_of(&c[2]));
            }
            flush_first_para_after_heading(&s, &classes)
        }
        _ => cjk_paragraphize(&s),
    };
    (s, before_dup)
}
