//! 基础 HTML 规整：同章内链规整、单标签重复 id 折叠、全书 id 去重。属性一律走 `crate::html`（完整属性名、两种引号）。
use super::*;

/// 规整脚注类内链：链接指向本文件、目标锚点就在本章内 → href 规整成裸 `#锚点`（xochitl 唯一会跳的类别）。
/// `self_file` 是本文件的 zip 路径：给了就要求链接的路径部分解析出来正是它（2026-10-08 审计：此前只比 id 不比路径，
/// 别的文件里碰巧同名的锚点也被改成指向本章）；`None` = 不知道本章原来的文件名（`epub::assemble` 组装转换器的章节：
/// 正文是从一个来源页面抽出来的，链接里的文件名是来源页面的旧名，和组装后的章节文件名对不上），只比 id。
/// 跨文件/外链不动。对齐 epub._fix_internal_links。
pub fn fix_internal_links(html: &str, self_file: Option<&str>) -> String {
    if !html.contains("href") {
        return html.to_string();
    }
    let ids: HashSet<&str> = html::anchors(html).into_iter().map(|(v, _)| v).collect();
    html::edit_attrs(html, &["href"], |t, a| {
        if !t.is("a") {
            return Edit::Keep;
        }
        let (path, frag) = html::split_href(a.value);
        match frag {
            Some(f) if !path.is_empty() && !html::is_external(path) && ids.contains(html::frag_id(f).as_ref()) && self_file.is_none_or(|me| crate::epubzip::resolve_link(me, a.value).0 == me) => Edit::Set(format!("#{f}")),
            _ => Edit::Keep,
        }
    })
    .into_owned()
}

/// 折叠单个开始标签上的**重复 `id=` 属性**：每标签只保留第一个 id、删除后续的。
/// 同元素两个 `id` 属性是非法 XHTML——reMarkable 用严格 XML 解析，遇重复属性**整章渲染失败**（只出前
/// 几页，真机《消失的爱人》只 7 页根因，2026-09-01）。转换器（kf8 aid→id / mobi 注入 fpN）把我们的锚点
/// id 放在首位，故"保留首个"= 保住锚点、丢弃冗余的既存 id（calibre `filepos`/`calibre_pb` 等）；也兜底
/// 第三方 EPUB 本就带的重复 id 属性。按完整属性名认：`aid=`、`data-id=` 都不算 id（2026-09-27 审计：旧正则
/// `\bid="` 把 `data-id="y"` 当成第二个 id 删掉，留下 `data->`）。
pub fn collapse_dup_id_attrs(html: &str) -> String {
    let mut edits: Vec<(usize, usize, String)> = Vec::new();
    for t in html::tags(html).filter(|t| t.is_start()) {
        let tag = &html[t.start..t.end];
        let ids: Vec<html::Attr> = html::attrs(tag).into_iter().filter(|a| a.is("id")).collect();
        for a in ids.iter().skip(1) {
            // 连同前导空白一起删，避免留下双空格
            edits.push((t.start + tag[..a.start].trim_end().len(), t.start + a.end, String::new()));
        }
    }
    if edits.is_empty() {
        return html.to_string();
    }
    html::apply_edits(html, edits)
}

/// 全书 id 去重：reMarkable 锚点是**全书命名空间**，多章重用同一 id（如每章都有 `id="fn1"`）会让
/// 所有 `#fn1` 都跳到全书第一处。按章调用、跨章累积 `seen`：本章某 id 若已在别章出现过，就把它
/// （及本章内指向它的**同文件** `href="#id"`）改成全书唯一名。跨文件 `href="f#id"` 这里看不到——开了清洗时
/// 清洗层（`wash::dedup_ids_across_book`）已先在全书范围改名并同步改写所有文件的链接，走到这里不会再有跨章重复；
/// 这里只是不清洗时的兜底。**先折叠单元素重复 id 属性**（非法 XHTML 兜底，见 `collapse_dup_id_attrs`），再把本章里重复的 id
/// 改开（[`rename_repeated_ids`]），最后做跨章值去重。
///
/// 开了清洗时这三步多半什么也不改，但不能整个省掉：清洗层的全书去重之后，规范整理还会新写文件（补的 nav），优化器第二遍又往
/// 各章搬注释（可能撞上本章原有的 id），这些只有这里看得到。所以先一遍扫出本章的 id（[`chapter_id_scan`]）：没有一个标签带两个
/// `id`、本章 id 互不相同、也都没在别章出现过时三步都不会改——直接记进 `seen` 返回原文，结果和走完三步相同，只是不再把整章扫三遍。
pub fn dedup_ids_in_chapter(html: &str, seen: &mut HashSet<String>) -> String {
    if let Some(ids) = chapter_id_scan(html) {
        let distinct: HashSet<&str> = ids.iter().copied().collect();
        if distinct.len() == ids.len() && !ids.iter().any(|id| seen.contains(*id)) {
            seen.extend(ids.into_iter().map(str::to_string));
            return html.to_string();
        }
    }
    let html = &collapse_dup_id_attrs(html);
    let html = &rename_repeated_ids(html, seen);
    let rename = plan_id_renames(html, seen);
    if rename.is_empty() {
        return html.to_string();
    }
    rename_ids(html, &rename)
}

/// 本章开标签上的全部非空 id（文档序）；有标签带不止一个 `id` 属性（要先 [`collapse_dup_id_attrs`]）→ `None`。
fn chapter_id_scan(html: &str) -> Option<Vec<&str>> {
    let mut ids = Vec::new();
    for t in html::tags(html).filter(|t| t.is_start()) {
        let mut n = 0;
        for a in html::attrs(&html[t.start..t.end]).into_iter().filter(|a| a.is("id")) {
            n += 1;
            if n > 1 {
                return None;
            }
            if !a.value.is_empty() {
                ids.push(a.value);
            }
        }
    }
    Some(ids)
}

/// 同一章里同一个 id 出现不止一次（`<p id="a">…<div id="a">`，不合法；不清洗的书、搬进本章的注释撞上本章原有的 id 都会这样）：
/// 第一个不动，后面的元素各改成全书唯一的新名（`{id}-x{n}`，避开本章全部 id 和 `seen`）。本章指向它的 `#id` 不改——仍指向第一个
/// （阅读器遇到重复 id 本来也是跳第一个）。没有重复时原样返回。
fn rename_repeated_ids(html: &str, seen: &HashSet<String>) -> String {
    let tags: Vec<html::Tag> = html::tags(html).filter(|t| t.is_start()).collect();
    let ids: Vec<(usize, html::Attr)> = tags.iter().flat_map(|t| html::attrs(&html[t.start..t.end]).into_iter().filter(|a| a.is("id") && !a.value.is_empty()).map(move |a| (t.start, a))).collect();
    let all: HashSet<&str> = ids.iter().map(|(_, a)| a.value).collect();
    if all.len() == ids.len() {
        return html.to_string();
    }
    let (mut first, mut taken): (HashSet<&str>, HashSet<String>) = (HashSet::new(), HashSet::new());
    let mut edits: Vec<(usize, usize, String)> = Vec::new();
    for (pos, a) in &ids {
        if first.insert(a.value) {
            continue;
        }
        let mut n = 2usize;
        let mut cand = format!("{}-x{n}", a.value);
        while seen.contains(&cand) || all.contains(cand.as_str()) || taken.contains(&cand) {
            n += 1;
            cand = format!("{}-x{n}", a.value);
        }
        taken.insert(cand.clone());
        edits.push((pos + a.value_start, pos + a.value_end, cand));
    }
    html::apply_edits(html, edits)
}

/// 本章里已在别处（`seen`）出现过的 id → 新名（`{id}-x{n}`，全书唯一）；本章新见到的 id 记进 `seen`。
pub(crate) fn plan_id_renames(html: &str, seen: &mut HashSet<String>) -> HashMap<String, String> {
    plan_id_renames_of(chapter_ids(html), seen)
}

/// 本章开标签上的全部非空 id，文档序（[`plan_id_renames`] 的前一半：只看本章，可以各章同时做）。
pub(crate) fn chapter_ids(html: &str) -> Vec<String> {
    html::tags(html)
        .filter(|t| t.is_start())
        .flat_map(|t| html::attrs(&html[t.start..t.end]).into_iter().filter(|a| a.is("id") && !a.value.is_empty()).map(|a| a.value.to_string()).collect::<Vec<_>>())
        .collect()
}

/// [`plan_id_renames`] 的后一半：按本章的 id（[`chapter_ids`]）和全书已见过的 `seen` 定改名。
pub(crate) fn plan_id_renames_of(ids: Vec<String>, seen: &mut HashSet<String>) -> HashMap<String, String> {
    let mut rename: HashMap<String, String> = HashMap::new();
    let mut local: HashSet<String> = HashSet::new();
    // 本章全部 id（新名不能撞上本章后面才出现的同名 id，2026-09-28 审计：`fn1` 改成 `fn1-x2`，本章后面本来就有个 `fn1-x2`）
    let all_local: HashSet<&str> = ids.iter().map(String::as_str).collect();
    for id in ids.iter().cloned() {
        if !local.insert(id.clone()) {
            continue; // 本章内同 id 只决策一次
        }
        if seen.contains(&id) {
            let mut n = 2usize;
            let mut cand = format!("{id}-x{n}");
            while seen.contains(&cand) || local.contains(&cand) || all_local.contains(cand.as_str()) {
                n += 1;
                cand = format!("{id}-x{n}");
            }
            seen.insert(cand.clone());
            local.insert(cand.clone());
            rename.insert(id, cand);
        } else {
            seen.insert(id);
        }
    }
    rename
}

/// 按 `rename` 一遍改写本章的 `id="旧"` 与同文件 `href="#旧"`（值按原文精确匹配，锚点先百分号解码再对）。
pub(crate) fn rename_ids(html: &str, rename: &HashMap<String, String>) -> String {
    html::edit_attrs(html, &["id", "href"], |_, a| {
        if a.is("id") {
            return rename.get(a.value).map_or(Edit::Keep, |n| Edit::Set(n.clone()));
        }
        match a.value.strip_prefix('#') {
            Some(f) => rename.get(html::frag_id(f).as_ref()).map_or(Edit::Keep, |n| Edit::Set(format!("#{n}"))),
            None => Edit::Keep,
        }
    })
    .into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 给了本文件名：只有指向本文件的链接改成裸锚点，别的文件里同名锚点的链接不动；不给（`epub::assemble`）照旧只比 id。
    #[test]
    fn fix_internal_links_checks_own_file() {
        let html = r#"<p id="n1"><a href="c1.xhtml#n1">a</a><a href="c2.xhtml#n1">b</a><a href="../Text/c1.xhtml#n1">c</a></p>"#;
        assert_eq!(
            fix_internal_links(html, Some("OEBPS/Text/c1.xhtml")),
            r##"<p id="n1"><a href="#n1">a</a><a href="c2.xhtml#n1">b</a><a href="#n1">c</a></p>"##
        );
        assert_eq!(fix_internal_links(html, None), r##"<p id="n1"><a href="#n1">a</a><a href="#n1">b</a><a href="#n1">c</a></p>"##);
    }
}
