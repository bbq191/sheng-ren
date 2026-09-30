//! 排版细化（2026-09-27 用户确定的四条 + 章尾空白页）。只改样式与空白结构，不动正文文字。
//!
//! - **语言属性**：`<html>` 缺 `lang`/`xml:lang` 时按书的语言补上。阅读器靠它选中文字体、决定中文标点禁则与英文断字。
//! - **对齐**：中文/英文正文都两端对齐（规则在 `wash_css`）；书里用 `align=` 属性或行内 `text-align` 写的居中/居右，
//!   换成类 `eink-center`/`eink-right` 保留下来——类选择器压得过 `p{}`，而且 xochitl 不认行内样式，换成类后它也能居中。
//! - **行高**：`line-height` 与字号字体一样剥掉（`DEFAULT_FILTER_PROPS`），让设备的行距设置生效。
//! - **章尾空白页**：①每个文件末尾没有可见内容的空段落/空 div/换行删掉（被链接指到的保留；书的样式表给它的类、元素名
//!   或它自己的 `style` 画了边框/背景/阴影/`::before` 内容的也保留——那是 CSS 画的分隔线，见 [`Drawn`]）；②包住文件结尾的容器
//!   去掉下边距、下内边距和"之后分页"（`.chapter-content{margin-bottom:1.5em}` 在一章刚好写满一页时会单独推出一页）；
//!   ③`height`/`min-height` 的 `vh` 值剥掉（占满一屏的书名页/封面页加上页眉页脚会溢出成空白页，在 `filter_decls_with`）。

use super::*;

/// `<html>` 缺语言属性时补上 `lang` 与 `xml:lang`（按完整属性名认，单引号的 `lang='en'` 也算有，不会再补出重复属性）。
pub(super) fn ensure_html_lang(html: &str, tag: &str) -> String {
    let Some(t) = html::tags(html).find(|t| t.is_start() && t.is("html")) else { return html.to_string() };
    let open = &html[t.start..t.end];
    let has_lang = html::attr(open, "lang").is_some();
    let has_xml_lang = html::attr(open, "xml:lang").is_some();
    if has_lang && has_xml_lang {
        return html.to_string();
    }
    let mut extra = String::new();
    if !has_lang {
        extra.push_str(&format!(" lang=\"{tag}\""));
    }
    if !has_xml_lang {
        extra.push_str(&format!(" xml:lang=\"{tag}\""));
    }
    let insert = t.start + 1 + t.name.len(); // "<html" 之后
    format!("{}{}{}", &html[..insert], extra, &html[insert..])
}

/// 居中/居右的块（`align=` 属性或行内 `text-align`）追加类 `eink-center`/`eink-right`（原属性保留；已有 `class` 就地追加，
/// 单引号也认，不产生第二个 `class`）。
pub(super) fn align_classes(html: &str) -> String {
    static STYLE_ALIGN: OnceLock<Regex> = OnceLock::new();
    let style_align = STYLE_ALIGN.get_or_init(|| Regex::new(r#"(?i)text-align\s*:\s*(center|right)"#).unwrap());
    let mut edits: Vec<(usize, usize, String)> = Vec::new();
    for t in html::tags(html).filter(|t| t.is_start() && (t.is("p") || t.is("div") || t.heading_level().is_some())) {
        let tag = &html[t.start..t.end];
        let align = html::attr_value(tag, "align")
            .map(|a| a.trim().to_ascii_lowercase())
            .filter(|a| a == "center" || a == "right")
            .or_else(|| html::attr_value(tag, "style").and_then(|s| style_align.captures(s).map(|m| m[1].to_ascii_lowercase())));
        let Some(align) = align else { continue };
        let class = format!("eink-{align}");
        if !html::has_class(tag, &class) {
            edits.push((t.start, t.end, html::add_class(tag, &class)));
        }
    }
    if edits.is_empty() {
        return html.to_string();
    }
    html::apply_edits(html, edits)
}

/// 全书被链接指到的 fragment（`#x`）。
fn referenced_frags(entries: &[Entry]) -> HashSet<String> {
    let mut set = HashSet::new();
    for e in entries {
        let l = e.name.to_ascii_lowercase();
        if is_html_entry(&e.name, &e.data) || l.ends_with(".ncx") || l.ends_with(".opf") {
            let t = String::from_utf8_lossy(&e.data);
            for v in html::link_values(&t) {
                if let (_, Some(f)) = html::split_href(v) {
                    set.insert(html::frag_id(f).into_owned());
                }
            }
        }
    }
    set
}

/// 书里"会画出东西"的 CSS：选择器里出现的类名与裸元素名。空元素只要带着这样的类（或自己的 `style` 会画），就不是空白。
#[derive(Default)]
pub(super) struct Drawn {
    classes: HashSet<String>,
    elements: HashSet<String>,
}

impl Drawn {
    /// 从全书样式表（独立 .css 与 html 里的 `<style>`）收集。
    pub(super) fn collect(entries: &[Entry]) -> Drawn {
        static CLASS: OnceLock<Regex> = OnceLock::new();
        static ELEM: OnceLock<Regex> = OnceLock::new();
        let class_re = CLASS.get_or_init(|| Regex::new(r#"\.([A-Za-z0-9_-]+)"#).unwrap());
        let elem_re = ELEM.get_or_init(|| Regex::new(r#"(?:^|[\s,>+~])([A-Za-z][A-Za-z0-9]*)"#).unwrap());
        let mut d = Drawn::default();
        let mut add_css = |css: &str| {
            for c in css_rule_re().captures_iter(css) {
                let sel = strip_css_comments(&c[1]);
                let pseudo = sel.contains(":before") || sel.contains(":after");
                if !html::css_decls(&c[2]).iter().any(|x| decl_draws(x.prop, x.value, pseudo)) {
                    continue;
                }
                for part in sel.split(',') {
                    d.classes.extend(class_re.captures_iter(part).map(|m| m[1].to_string()));
                    if !part.contains('.') {
                        d.elements.extend(elem_re.captures_iter(part).map(|m| m[1].to_ascii_lowercase()));
                    }
                }
            }
        };
        for e in entries {
            if e.name.to_ascii_lowercase().ends_with(".css") {
                add_css(&String::from_utf8_lossy(&e.data));
            } else if is_html_entry(&e.name, &e.data) {
                let t = String::from_utf8_lossy(&e.data);
                for c in html::style_block_re().captures_iter(&t) {
                    add_css(&c[2]);
                }
            }
        }
        d
    }

    /// 这个空元素（开标签）会不会画出东西。
    fn draws(&self, name: &str, open: &str) -> bool {
        self.elements.contains(&name.to_ascii_lowercase())
            || html::attr_value(open, "class").is_some_and(|c| c.split_whitespace().any(|x| self.classes.contains(x)))
            || html::attr_value(open, "style").is_some_and(|st| html::css_decls(st).iter().any(|x| decl_draws(x.prop, x.value, false)))
    }
}

/// 一条声明会不会画出看得见的东西：有线型的边框/轮廓、阴影、背景，或伪元素的 `content`。
fn decl_draws(prop: &str, value: &str, pseudo: bool) -> bool {
    let p = prop.to_ascii_lowercase();
    let v = value.to_ascii_lowercase();
    let v = v.trim_end_matches("!important").trim();
    let blank = matches!(v, "" | "none" | "0" | "transparent" | "inherit" | "initial" | "unset" | "hidden");
    if p.starts_with("border") || p.starts_with("outline") {
        return ["solid", "dashed", "dotted", "double", "groove", "ridge", "inset", "outset"].iter().any(|k| v.contains(k));
    }
    match p.as_str() {
        "box-shadow" | "background" | "background-color" | "background-image" => !blank,
        "content" => pseudo && !blank && v != "\"\"" && v != "''",
        _ => false,
    }
}

/// 删掉 `html` 的 body 末尾（最后一处可见内容之后）没有内容的空元素与换行；返回 (新 html, 删了几个, 包住结尾的容器的类名)。
fn trim_tail(html: &str, referenced: &HashSet<String>, drawn: &Drawn) -> (String, usize, Vec<String>) {
    static EMPTY: OnceLock<Regex> = OnceLock::new();
    let empty = EMPTY.get_or_init(|| {
        Regex::new(r#"(?is)<(p|div|span|section)\b([^>]*)>(?:\s|&nbsp;|&#160;|&#xa0;|\u{3000}|<br\s*/?>)*</(?:p|div|span|section)>|<(?:p|div|span)\b[^>]*/>|<br\b[^>]*/?>"#).unwrap()
    });
    let Some((lo, close)) = html::body_range(html) else { return (html.to_string(), 0, Vec::new()) };
    let inner = &html[lo..close];
    let Some(vis_end) = html::last_visible_end(inner) else { return (html.to_string(), 0, Vec::new()) };
    let (head, mut tail) = (inner[..vis_end].to_string(), inner[vis_end..].to_string());
    let mut removed = 0;
    loop {
        let mut n = 0;
        let next = empty
            .replace_all(&tail, |c: &regex::Captures| {
                let whole = &c[0];
                let open_end = html::tags(whole).next().map_or(whole.len(), |t| t.end);
                let open = &whole[..open_end];
                // 被链接指到的留着；样式会画出东西的留着（CSS 画的分隔线、边框，2026-09-27 审计）；看得见的留着。
                let name = html::tags(open).next().map_or("", |t| t.name);
                let keep = html::attr_value(open, "id").is_some_and(|id| referenced.contains(id)) || drawn.draws(name, open) || html::has_visible(whole);
                if keep {
                    return whole.to_string();
                }
                n += 1;
                String::new()
            })
            .into_owned();
        tail = next;
        if n == 0 {
            break;
        }
        removed += n;
    }
    // 包住结尾的容器：在最后一处可见内容之前打开、之后闭合，**而且里面还包着段落/div/标题**的元素（段落自己不算——
    // 书常给所有段落用同一个类，去掉它的下边距会改变全书段距）；我们自己的 eink- 类不算。
    static BLOCK: OnceLock<Regex> = OnceLock::new();
    let block = BLOCK.get_or_init(|| Regex::new(r#"(?i)<(p|div|h[1-6]|section|blockquote)\b"#).unwrap());
    let new_inner = format!("{head}{tail}");
    let mut classes = Vec::new();
    for sp in html::parse_spans(&new_inner, 0, new_inner.len()).iter().filter(|s| s.closed()) {
        if sp.open_start < vis_end && sp.close_start >= vis_end && block.is_match(&new_inner[sp.open_end..sp.close_start]) {
            if let Some(cls) = html::attr_value(&new_inner[sp.open_start..sp.open_end], "class") {
                classes.extend(cls.split_whitespace().filter(|c| !c.starts_with("eink-")).map(str::to_string));
            }
        }
    }
    (format!("{}{}{}", &html[..lo], new_inner, &html[close..]), removed, classes)
}

/// 规则选择器是不是只选了这些类之一（`.x` 或 `元素.x`）。
fn selects_class(selector: &str, classes: &HashSet<String>) -> bool {
    static SEL: OnceLock<Regex> = OnceLock::new();
    let re = SEL.get_or_init(|| Regex::new(r#"^\s*[A-Za-z0-9]*\.([A-Za-z0-9_-]+)\s*$"#).unwrap());
    re.captures(selector).is_some_and(|c| classes.contains(&c[1]))
}

/// 从这些类的规则里去掉下边距、下内边距和"之后分页"（简写的 margin/padding 只把下边改成 0）。
pub(super) fn strip_tail_spacing(css: &str, classes: &HashSet<String>) -> String {
    css_rule_re().replace_all(css, |c: &regex::Captures| {
        if !selects_class(&strip_css_comments(&c[1]), classes) {
            return c[0].to_string();
        }
        let mut out: Vec<String> = Vec::new();
        for d in html::css_decls(&c[2]) {
            let prop = d.prop.to_ascii_lowercase();
            let val = d.value;
            match prop.as_str() {
                "margin-bottom" | "padding-bottom" | "page-break-after" | "break-after" => continue,
                "margin" | "padding" => match box_sides(val) {
                    BoxSides::Sides([t, r, _, l], important) => out.push(format!("{prop}:{t} {r} 0 {l}{important}")),
                    // `margin:inherit`：下边不要，其余三边照原值写成分项
                    BoxSides::Keyword(k, important) => out.push(format!("{prop}-top:{k}{important};{prop}-right:{k}{important};{prop}-left:{k}{important}")),
                    BoxSides::Unknown => out.push(format!("{prop}:{val}")),
                },
                _ => out.push(format!("{prop}:{val}")),
            }
        }
        let mut body = out.join(";");
        if !body.is_empty() {
            body.push(';');
        }
        format!("{}{{{}}}", &c[1], body)
    })
    .into_owned()
}

/// 章尾空白页：逐个 spine 文件删末尾空元素，再把包住结尾的容器类的下边距/之后分页从样式表里去掉。
pub(super) fn remove_chapter_end_blanks(entries: &mut [Entry], rep: &mut WashReport) {
    let Some(opf) = parse_opf(entries) else { return };
    let referenced = referenced_frags(entries);
    let drawn = Drawn::collect(entries);
    let mut tail_classes: HashSet<String> = HashSet::new();
    for path in &opf.spine {
        if Some(path) == opf.nav_doc.as_ref() || is_toc_file(path) {
            continue;
        }
        let Some(e) = entries.iter_mut().find(|e| &e.name == path) else { continue };
        let Ok(html) = std::str::from_utf8(&e.data) else { continue };
        let (new, removed, classes) = trim_tail(html, &referenced, &drawn);
        tail_classes.extend(classes);
        if removed > 0 {
            rep.trailing_blanks_removed += removed;
            e.data = new.into_bytes();
        }
    }
    if tail_classes.is_empty() {
        return;
    }
    for e in entries.iter_mut().filter(|e| e.name.to_ascii_lowercase().ends_with(".css") && !is_wash_css_name(&e.name)) {
        let Ok(css) = std::str::from_utf8(&e.data) else { continue };
        let new = strip_tail_spacing(css, &tail_classes);
        if new != css {
            rep.tail_spacing_rules_fixed += 1;
            e.data = new.into_bytes();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn html_lang_added_only_when_missing() {
        assert_eq!(ensure_html_lang(r#"<html xmlns="x"><body/></html>"#, "zh-CN"), r#"<html lang="zh-CN" xml:lang="zh-CN" xmlns="x"><body/></html>"#);
        let has = r#"<html lang="en" xml:lang="en">"#;
        assert_eq!(ensure_html_lang(has, "zh"), has, "已有的不改");
        assert_eq!(ensure_html_lang(r#"<html xml:lang="ja">"#, "zh"), r#"<html lang="zh" xml:lang="ja">"#, "只补缺的那个");
        assert_eq!(ensure_html_lang(r#"<html lang='en' xml:lang='en'>"#, "zh"), r#"<html lang='en' xml:lang='en'>"#, "单引号也算已有");
    }

    #[test]
    fn center_and_right_become_classes() {
        let h = r#"<p style="text-align:center;">诗</p><p align="right">落款</p><div class="a" style="text-align: center">x</div><p>正文</p><p class='b' align='center'>单引号</p>"#;
        assert_eq!(
            align_classes(h),
            r#"<p style="text-align:center;" class="eink-center">诗</p><p align="right" class="eink-right">落款</p><div class="a eink-center" style="text-align: center">x</div><p>正文</p><p class='b eink-center' align='center'>单引号</p>"#
        );
        assert_eq!(align_classes(&align_classes(h)), align_classes(h), "幂等");
    }

    #[test]
    fn trailing_empties_removed_but_referenced_anchor_kept_and_wrappers_reported() {
        let mut refs = HashSet::new();
        refs.insert("keep".to_string());
        let css = ".rule{border-bottom:1px solid #000}\n.softbreak{border-top:0;display:block;margin-bottom:0.7em}\n.star:after{content:'*'}";
        let drawn = Drawn::collect(&[Entry { name: "s.css".into(), data: css.as_bytes().to_vec() }]);
        let html = r#"<html><body><div class="chapter-content"><p>最后一段。</p><p class="softbreak"></p><br/></div> <div id="keep"></div><div class="calibre4" id="calibre_pb_7"></div><p>&nbsp;</p><div class="rule"></div><p style='border-top:1px solid'></p><div class="x star"></div></body></html>"#;
        let (out, n, classes) = trim_tail(html, &refs, &drawn);
        assert_eq!(
            out,
            r#"<html><body><div class="chapter-content"><p>最后一段。</p></div> <div id="keep"></div><div class="rule"></div><p style='border-top:1px solid'></p><div class="x star"></div></body></html>"#,
            "CSS 画了线/伪元素内容的空元素留着，只有外边距的删掉"
        );
        assert_eq!(n, 4);
        assert_eq!(classes, ["chapter-content"], "包住结尾、里面还有段落的容器；段落自己不算");
        let (_, n2, _) = trim_tail(&out, &refs, &drawn);
        assert_eq!(n2, 0, "幂等");
    }

    #[test]
    fn tail_container_loses_bottom_spacing_only() {
        let mut cls = HashSet::new();
        cls.insert("chapter-content".to_string());
        let css = ".chapter-content {margin:1em 0 1.5em;padding-bottom:1em;page-break-after:always;text-align:justify}\n.other{margin-bottom:1em}\ndiv.chapter-content{margin-bottom:2em}";
        assert_eq!(
            strip_tail_spacing(css, &cls),
            ".chapter-content {margin:1em 0 0 0;text-align:justify;}\n.other{margin-bottom:1em}\ndiv.chapter-content{}"
        );
    }

    #[test]
    fn book_like_fengtan_gets_no_chapter_end_blank_sources() {
        let e = |n: &str, d: &str| Entry { name: n.into(), data: d.as_bytes().to_vec() };
        let long = "正文足够长的一段文字，确保不会被当成书名页的作者行，超过三十个字的门槛。";
        let mut v = vec![
            e("OEBPS/content.opf", r#"<package version="3.0"><metadata><dc:title>书</dc:title><dc:language>zh-CN</dc:language></metadata><manifest><item id="css" href="main.css" media-type="text/css"/><item id="c1" href="c1.html" media-type="application/xhtml+xml"/></manifest><spine><itemref idref="c1"/></spine></package>"#),
            e("OEBPS/main.css", ".chapter-page {height:100vh;}\n.chapter-content {margin:0;line-height:130%;margin-bottom:1.5em;}\np{text-indent:2em;line-height:130%;}"),
            e("OEBPS/c1.html", &format!(r#"<html><head><link rel="stylesheet" href="main.css"/></head><body><div class="chapter-page"><h1>第一章</h1><div class="chapter-content"><p>{long}</p><p style="text-align:center">诗句</p><p>{long}</p><p></p></div></div></body></html>"#)),
        ];
        let rep = super::super::wash_entries(&mut v, &WashOpts::default()).unwrap();
        let css = String::from_utf8(v.iter().find(|x| x.name == "OEBPS/main.css").unwrap().data.clone()).unwrap();
        assert!(!css.contains("line-height") && !css.contains("100vh") && !css.contains("margin-bottom:1.5em"), "{css}");
        let body: String = v.iter().filter(|x| x.name.starts_with("OEBPS/c1")).map(|x| String::from_utf8_lossy(&x.data).into_owned()).collect();
        assert!(body.contains(r#"lang="zh-CN""#) && body.contains("eink-center") && !body.contains("<p></p>"), "{body}");
        assert!(rep.trailing_blanks_removed >= 1 && rep.tail_spacing_rules_fixed == 1);
        let wash = String::from_utf8(v.iter().find(|x| x.name == "OEBPS/eink-wash.css").unwrap().data.clone()).unwrap();
        assert!(wash.contains("text-align:justify;") && wash.contains(".eink-center{text-align:center;"), "{wash}");
    }
}
