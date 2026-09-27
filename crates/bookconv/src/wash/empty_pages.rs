//! 空页清理：删掉纯空白页并把指向它的链接（目录、正文里的目录页、OPF `<guide>`）改指向邻页。
use super::*;

// ───────────────────────── 5. 空页清理 ─────────────────────────

/// 页面 body 里没有读者看得见的内容（口径见 `html::has_visible`：非空白文字，或图片/分隔线/表格等媒体）。
/// 2026-09-27：与章节分页共用同一套判定（此前这里只认 img/svg/image/video/audio，只有 `<hr/>`/表格的页会被当空页删掉）。
pub(super) fn is_empty_page(html: &str) -> bool {
    !html::has_visible(html::first_body_inner(html).unwrap_or(""))
}

/// 一遍扫描删掉 OPF 里 `id`/`idref` 属于 `ids` 的 `<item>`/`<itemref>`（连同紧随的空白与空闭合标签）。
/// 此前对每个被删页各编译两个正则、各整份扫描一遍 OPF：Calibre MOBI 转出的书常有上千个 `mbppagebreak` 独占页，
/// 即上千次编译 + 上千次整份 OPF 扫描（`bench_tmp emptypages`：1500 页/750 空页，host 380ms → 见提交说明）。
pub(super) fn drop_opf_refs(opf: &str, ids: &HashSet<&str>) -> String {
    static ITEMREF: OnceLock<Regex> = OnceLock::new();
    static ITEM: OnceLock<Regex> = OnceLock::new();
    static IDREF_ATTR: OnceLock<Regex> = OnceLock::new();
    static ID_ATTR: OnceLock<Regex> = OnceLock::new();
    let itemref = ITEMREF.get_or_init(|| Regex::new(r#"<itemref\b[^>]*>(?:\s*</itemref>)?\s*"#).unwrap());
    let item = ITEM.get_or_init(|| Regex::new(r#"<item\b[^>]*>(?:\s*</item>)?\s*"#).unwrap());
    let idref_attr = IDREF_ATTR.get_or_init(|| Regex::new(r#"\bidref="([^"]*)""#).unwrap());
    let id_attr = ID_ATTR.get_or_init(|| Regex::new(r#"\bid="([^"]*)""#).unwrap());
    let drop_if = |c: &regex::Captures, attr: &Regex| -> String {
        let whole = c.get(0).unwrap().as_str();
        let tag_end = whole.find('>').map_or(whole.len(), |i| i + 1);
        if attr.captures_iter(&whole[..tag_end]).any(|a| ids.contains(&a[1])) {
            String::new()
        } else {
            whole.to_string()
        }
    };
    let out = itemref.replace_all(opf, |c: &regex::Captures| drop_if(c, idref_attr));
    item.replace_all(&out, |c: &regex::Captures| drop_if(c, id_attr)).into_owned()
}

pub(super) fn remove_empty_pages(entries: &mut Vec<Entry>, rep: &mut WashReport) {
    let Some(opf) = parse_opf(entries) else { return };
    let mut removed: Vec<String> = Vec::new();
    // 条目名索引建一次（此前每个 spine 页线性找一遍全书条目，几千页漫画是"页数 × 条目数"次比较；同名取第一条）。
    let mut by_name: HashMap<&str, &Entry> = HashMap::with_capacity(entries.len());
    for e in entries.iter() {
        by_name.entry(e.name.as_str()).or_insert(e);
    }
    for p in &opf.spine {
        if Some(p) == opf.nav_doc.as_ref() {
            continue;
        }
        if let Some(e) = by_name.get(p.as_str()) {
            if is_html_entry(&e.name, &e.data) && is_empty_page(&String::from_utf8_lossy(&e.data)) {
                removed.push(p.clone());
            }
        }
    }
    if removed.is_empty() || removed.len() >= opf.spine.len() {
        return; // 全空不动（别把书删没）
    }
    let removed_set: HashSet<&String> = removed.iter().collect();
    // 替换目标：spine 里下一篇未删的，没有则上一篇
    let replacement = |p: &String| -> Option<String> {
        let i = opf.spine.iter().position(|x| x == p)?;
        opf.spine[i + 1..].iter().chain(opf.spine[..i].iter().rev()).find(|x| !removed_set.contains(x)).cloned()
    };
    let repl: HashMap<String, String> = removed.iter().filter_map(|p| replacement(p).map(|r| (p.clone(), r))).collect();
    // OPF：删 itemref + item
    let ids: HashSet<&str> = opf.items.iter().filter(|(_, v)| removed_set.contains(v)).map(|(k, _)| k.as_str()).collect();
    let text = drop_opf_refs(&String::from_utf8_lossy(&entries[opf.index].data), &ids);
    entries[opf.index].data = text.into_bytes();
    // 全书指向被删页的链接 → 改指替换页（空页没有内容，锚点一并去掉）：目录（ncx/nav）、正文里的目录页、OPF `<guide>`。
    for e in entries.iter_mut() {
        let l = e.name.to_ascii_lowercase();
        if removed_set.contains(&e.name) || !(is_html_entry(&e.name, &e.data) || l.ends_with(".ncx") || l.ends_with(".opf")) {
            continue;
        }
        let dir = dir_of(&e.name).to_string();
        let text = String::from_utf8_lossy(&e.data);
        let new = html::rewrite_links(&text, |v| {
            let (p, _) = html::split_href(v);
            if p.is_empty() || html::is_external(p) {
                return None;
            }
            repl.get(&resolve(&dir, &percent_decode(p))).map(|r| relative_to(&dir, r))
        });
        if let Cow::Owned(new) = new {
            e.data = new.into_bytes();
        }
    }
    entries.retain(|e| !removed_set.contains(&e.name));
    rep.empty_pages_removed = removed;
}
