//! 空页清理：删掉纯空白页并把指向它的链接（目录、正文里的目录页、OPF `<guide>`）改指向邻页。
use super::*;

// ───────────────────────── 5. 空页清理 ─────────────────────────

/// 页面 body 里没有读者看得见的内容（口径见 `html::has_visible`：非空白文字，或图片/分隔线/表格等媒体）。
/// 2026-09-27：与章节分页共用同一套判定（此前这里只认 img/svg/image/video/audio，只有 `<hr/>`/表格的页会被当空页删掉）。
pub(super) fn is_empty_page(html: &str) -> bool {
    !html::has_visible(html::first_body_inner(html).unwrap_or(""))
}

pub(super) fn remove_empty_pages(entries: &mut Vec<Entry>, rep: &mut WashReport) {
    let Some(opf) = parse_opf(entries) else { return };
    let mut removed: Vec<String> = Vec::new();
    // 条目名索引建一次（此前每个 spine 页线性找一遍全书条目，几千页漫画是"页数 × 条目数"次比较；同名取第一条）。
    let by_name = name_index(entries);
    for p in &opf.spine {
        if Some(p) == opf.nav_doc.as_ref() {
            continue;
        }
        if let Some(e) = by_name.get(p.as_str()).map(|&i| &entries[i]) {
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
    // OPF：删 item + itemref（`wash::opf::remove_items`，单双引号都认；此前正则只认双引号，单引号 OPF 的空页删不掉）。
    // 删不掉就整个不做（下面改链接会把目录指到邻页，spine 里却还有这页）。
    let opf_dir = opf.dir.clone();
    let text = String::from_utf8_lossy(&entries[opf.index].data).into_owned();
    let Some(text) = opf::remove_items(&text, |it| removed_set.contains(&it.path(&opf_dir))) else { return };
    entries[opf.index].data = text.into_bytes();
    // 全书指向被删页的链接 → 改指替换页（空页没有内容，锚点一并去掉）：目录（ncx/nav）、正文里的目录页、OPF `<guide>`。
    rewrite_book_links(entries, |n| removed_set.contains(&n.to_string()), |l| {
        if html::split_href(l.value).0.is_empty() {
            return None;
        }
        repl.get(&l.target).map(|r| crate::epubzip::href_to(dir_of(l.file), r, ""))
    });
    entries.retain(|e| !removed_set.contains(&e.name));
    rep.empty_pages_removed = removed;
}
