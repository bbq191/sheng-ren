//! 无效引用清理：指向书内不存在文件的 `<img>` 与字体全缺的 `@font-face`。
use super::*;

// ───────────────────────── 无效引用清理 ─────────────────────────

/// 引用指的不是书内文件（空值、纯锚点 `#x`、`data:`、`http(s):`、`//` 开头）——谈不上"书内缺文件"，一律不判无效。
/// 和 [`crate::html::is_external`]（"带协议的书外链接"，全书改链接时用）口径不同：这里空值和纯锚点也算，别的协议（`res:` 等）不算。
pub(super) fn is_non_file_ref(r: &str) -> bool {
    let l = r.trim().to_ascii_lowercase();
    l.is_empty() || l.starts_with('#') || l.starts_with("data:") || l.starts_with("http:") || l.starts_with("https:") || l.starts_with("//")
}

/// 文件 `base_file` 里的引用 `rel` 指向的书内文件是否存在。大小写不同也算存在（别的阅读器可能容错，宁可留着）。
/// `xml`：引用是 XHTML 的属性值，先还原字符引用（`a&amp;b.jpg` 指的是 `a&b.jpg`；按 [`crate::epubzip::resolve_link`] 解析）；
/// 样式表里的 `url()` 不还原。`?` 后面的查询串不算路径（书里的文件名不会有 `?`：改安全文件名时已换掉）。
pub(super) fn ref_exists(exact: &HashSet<String>, lower: &HashSet<String>, base_file: &str, rel: &str, xml: bool) -> bool {
    let rel = rel.split('?').next().unwrap_or("");
    let target = if xml {
        crate::epubzip::resolve_link(base_file, rel).0
    } else {
        resolve(dir_of(base_file), &percent_decode(rel.split('#').next().unwrap_or("")))
    };
    exact.contains(&target) || lower.contains(&target.to_ascii_lowercase())
}

/// 去掉 `<img>` 里 src 指向书内不存在文件的标签。alt 有实际内容（非空且不是我们自己封面转换写的 "cover"）的留着，
/// 这类图坏了阅读器还可能显示替代文字，不冒丢内容的风险。返回 (新文本, 去掉个数)。
pub(super) fn drop_dead_imgs(html: &str, base_file: &str, exact: &HashSet<String>, lower: &HashSet<String>) -> (String, usize) {
    let mut edits: Vec<(usize, usize, String)> = Vec::new();
    for t in html::tags(html).filter(|t| t.is_start() && t.is("img")) {
        let tag = &html[t.start..t.end];
        let Some(r) = html::attr_value(tag, "src") else { continue };
        if is_non_file_ref(r) || ref_exists(exact, lower, base_file, r, true) {
            continue;
        }
        let alt_text = html::attr_value(tag, "alt").map(str::trim).unwrap_or_default();
        if !alt_text.is_empty() && alt_text != "cover" {
            continue;
        }
        edits.push((t.start, t.end, String::new()));
    }
    let n = edits.len();
    if n == 0 {
        return (html.to_string(), 0);
    }
    (html::apply_edits(html, edits), n)
}

/// 清理 `@font-face` 里必然读不到的字体来源：`url()` 指向书内不存在的文件，或设备路径（DuoKan 的
/// `res:///sdcard/...`、`res:///opt/sony/...`，任何阅读器都读不到）。
/// - 一条规则的 `url()` 全死且没有 `local()` 候选 → 整条删；
/// - 有 `local()` 候选或还有活的 `url()` → 只剔除死 `url()`（连同后面的 `format()` 和一个逗号），其余保留。
///
/// 外部（http/data）来源视为活。返回 (新 css, 改动的规则数)。
pub(super) fn drop_dead_font_faces(css: &str, base_file: &str, exact: &HashSet<String>, lower: &HashSet<String>) -> (String, usize) {
    static FORMAT: OnceLock<Regex> = OnceLock::new();
    let format = FORMAT.get_or_init(|| Regex::new(r#"^\s*(?i:format)\([^)]*\)"#).unwrap());
    let mut n = 0;
    let mut edits = Vec::new();
    // `@font-face` 规则按层叠同一套解析找（`cascade::font_face_rules`；以前一条正则遇到字符串里的 `}` 就截断）；
    // 改的是从 `@font-face` 起到 `}` 的这一段，前面的注释、`@import …;` 语句不动
    for r in crate::cascade::font_face_rules(css) {
        let start = r.start + html::rfind_ci(r.prelude, "@font-face").unwrap_or(0);
        let block = &css[start..r.end];
        let has_local = block.to_ascii_lowercase().contains("local(");
        let mut dead: Vec<(usize, usize)> = Vec::new();
        let mut total = 0;
        for u in html::css_urls(block) {
            total += 1;
            let r = u.value;
            let device_path = r.trim().to_ascii_lowercase().starts_with("res:");
            if device_path || !(is_non_file_ref(r) || ref_exists(exact, lower, base_file, r, false)) {
                // 连同后面的 `format(…)` 一起删
                let end = u.end + format.find(&block[u.end..]).map_or(0, |m| m.end());
                dead.push((u.start, end));
            }
        }
        if dead.is_empty() {
            continue;
        }
        n += 1;
        if !has_local && dead.len() == total {
            edits.push((start, r.end, String::new()));
            continue;
        }
        let mut out = block.to_string();
        for (st, en) in dead.into_iter().rev() {
            // 连同一个逗号一起删：优先吃前面的 ","，没有就吃后面的
            let before = out[..st].trim_end();
            if before.ends_with(',') {
                let cut = before.len() - 1;
                out.replace_range(cut..en, "");
            } else {
                let after = out[en..].trim_start();
                let skip = if after.starts_with(',') { out.len() - after.len() + 1 } else { en };
                out.replace_range(st..skip, "");
            }
        }
        edits.push((start, r.end, out));
    }
    (html::apply_edits(css, edits), n)
}

/// 清掉书里指向不存在文件的 `<img>` 和字体全缺的 `@font-face`。xochitl 遇到会逐次报 `Unable to find file`/
/// `Failed to open` 并白白尝试加载（真机日志 2026-09-20：一本书打开就 66 次 cover.jpg 找不到 + 一批 DuoKan 字体）。
/// 只删这两类**必然失效**的引用，不碰文字。远程/data 引用、大小写差异、带替代文字的图一律保留。
pub(super) fn drop_dead_refs(entries: &mut [Entry], rep: &mut WashReport) {
    let exact: HashSet<String> = entries.iter().map(|e| e.name.clone()).collect();
    let lower: HashSet<String> = exact.iter().map(|n| n.to_ascii_lowercase()).collect();
    // 各文件独立，多线程做（`util::par_map_mut`）
    let removed = crate::util::par_map_mut(entries, |e| {
        let is_css = is_css_name(&e.name);
        if !is_css && !is_html_entry(&e.name, &e.data) {
            return 0;
        }
        let Ok(text) = std::str::from_utf8(&e.data) else { return 0 };
        let base = e.name.as_str();
        let (new, n) = if is_css {
            drop_dead_font_faces(text, base, &exact, &lower)
        } else {
            let (t, a) = drop_dead_imgs(text, base, &exact, &lower);
            let mut b = 0;
            // `<style>` 里的 `url()` 不还原字符引用（同以前；HTML 解析时 `<style>` 的内容是原样文字）
            let t = html::edit_style_blocks(&t, |_, css| {
                let (css, k) = drop_dead_font_faces(css, base, &exact, &lower);
                b += k;
                Edit::Set(css)
            });
            (t.into_owned(), a + b)
        };
        if n > 0 {
            e.data = new.into_bytes();
        }
        n
    });
    let total: usize = removed.into_iter().sum();
    rep.dead_refs_removed = total;
}
