//! 空页清理：删掉纯空白页并把指向它的链接（目录、正文里的目录页、OPF `<guide>`）改指向邻页。
use super::*;

// ───────────────────────── 5. 空页清理 ─────────────────────────

/// 页面 body 里没有读者看得见的内容（口径见 `html::has_visible`：非空白文字，或图片/分隔线/表格等媒体）。
/// 2026-09-27：与定章节共用同一套判定（此前这里只认 img/svg/image/video/audio，只有 `<hr/>`/表格的页会被当空页删掉）。
/// 找不到完整的 `<body>…</body>`（截断的页、没有 body 的片段）不算空页：拿不准就不删（以前按空 body 算，整页正文被删掉）。
pub(super) fn is_empty_page(html: &str) -> bool {
    html::first_body_inner(html).is_some_and(|body| !html::has_visible(body))
}

pub(super) fn remove_empty_pages(entries: &mut Vec<Entry>, rep: &mut WashReport) {
    remove_pages(entries, rep, |e| is_empty_page(&String::from_utf8_lossy(&e.data)));
}

/// Kindle 上不出页的空页（照 Send to Kindle 统一的掌阅、Move：`kindle_rules`，KINDLE_RULES 35，2026-10-15 定）：KFX 写出器
/// 不给没有块的文件出版面（`write/layout.rs` 的 `empty_docs`），书里刻意留的空白页（《疯探》`blank.html` 只有一个空 div）、只有
/// 隐藏元素的页 Kindle 上直接跳过，掌阅、Move 照排成一页空白。看得见的口径照写出器：不隐藏的非空白文字、图片这类媒体
/// （[`html::MEDIA_ELEMENTS`]），以及带背景色、背景图的元素（写出器写成容器，Kindle 照画）；拿不准（解析不了）的不删。
pub(super) fn remove_kindle_empty_pages(entries: &mut Vec<Entry>, rep: &mut WashReport, media: Option<&crate::cascade::MediaEnv>) {
    let css = crate::kindle_model::book_css(entries, media);
    // 捷径（2026-10-15）：判空页原来每章都要完整层叠（层叠占 `Chapter::parse` 七成），可空页极少。元素只会因为一条 `display:none`
    // 声明被藏起来（[`crate::cascade::Computed`] 的 `display` 只从声明来）：全书样式表里没有这样的声明、这一章原文里连 `display`
    // 都没出现（`<style>`、行内 `style` 也就写不了）时，没有元素是隐藏的，DOM 的正文里只要有不是空白的文字就一定不是空页——
    // 和下面完整判断的结论一样，只是不用层叠。DOM 照样用 html5ever 解析（和 `Chapter::parse` 同一棵树），没找到字的照旧完整判断。
    let book_may_hide = css.values().any(|r| r.any_decl(|d| d.prop == "display" && d.value.trim().eq_ignore_ascii_case("none")));
    let blank = |e: &Entry| {
        let Ok(text) = std::str::from_utf8(&e.data) else { return false };
        if !book_may_hide && !text.as_bytes().windows(7).any(|w| w.eq_ignore_ascii_case(b"display")) {
            let html = scraper::Html::parse_document(text);
            let body = html.root_element().children().filter_map(scraper::ElementRef::wrap).find(|el| el.value().name() == "body");
            let has_text = |b: scraper::ElementRef| b.descendants().any(|d| matches!(d.value(), scraper::Node::Text(t) if t.chars().any(|c| !c.is_whitespace())));
            if body.is_some_and(has_text) {
                return false;
            }
        }
        let Some(ch) = crate::kindle_model::Chapter::parse(&e.name, text, media, &css) else { return false };
        let painted = |c: &crate::cascade::Computed| c.bg_image.is_some() || c.background.is_some_and(|b| b >> 24 != 0);
        if painted(&ch.body_comp) {
            return false;
        }
        // 最近的元素祖先不隐藏（`hidden` 已经含外面的 `display:none`）
        let shown = |d: ego_tree::NodeRef<scraper::Node>| d.ancestors().find_map(|a| ch.index_of(a.id())).is_none_or(|i| !ch.els[i].hidden);
        !ch.body().descendants().any(|d| match d.value() {
            scraper::Node::Text(t) => t.chars().any(|c| !c.is_whitespace()) && shown(d),
            scraper::Node::Element(v) => {
                ch.index_of(d.id()).is_some_and(|i| !ch.els[i].hidden && (painted(&ch.els[i].comp) || html::MEDIA_ELEMENTS.iter().any(|m| v.name().eq_ignore_ascii_case(m))))
            }
            _ => false,
        })
    };
    let names: HashSet<String> = crate::util::par_map(entries, |e| (is_chapter_entry(e) && blank(e)).then(|| e.name.clone())).into_iter().flatten().collect();
    drop(css);
    if !names.is_empty() {
        remove_pages(entries, rep, |e| names.contains(&e.name));
    }
}

/// 删掉 spine 里 `empty` 判成空的页（导航文档不算），指向它们的链接改指邻页。
fn remove_pages(entries: &mut Vec<Entry>, rep: &mut WashReport, empty: impl Fn(&Entry) -> bool + Sync) {
    let Some(opf) = parse_opf(entries) else { return };
    let mut removed: Vec<String> = Vec::new();
    // 条目名索引建一次（此前每个 spine 页线性找一遍全书条目，几千页漫画是"页数 × 条目数"次比较；同名取第一条）。
    let by_name = name_index(entries);
    // 各页独立判断，多线程（`util::par_map`），按 spine 顺序收回
    let empty = crate::util::par_map(&opf.spine, |p| {
        Some(p) != opf.nav_doc.as_ref() && by_name.get(p.as_str()).map(|&i| &entries[i]).is_some_and(|e| is_html_entry(&e.name, &e.data) && empty(e))
    });
    for (p, empty) in opf.spine.iter().zip(empty) {
        if empty {
            removed.push(p.clone());
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
