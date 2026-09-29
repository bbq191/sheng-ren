//! 基础 HTML 规整：同章内链规整、单标签重复 id 折叠、全书 id 去重。属性一律走 `crate::html`（完整属性名、两种引号）。
use super::*;

/// 规整脚注类内链：目标锚点就在本章内 → href 规整成裸 `#锚点`（xochitl 唯一会跳的类别）。
/// 跨文件/外链不动。对齐 epub._fix_internal_links。
pub fn fix_internal_links(html: &str) -> String {
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
            Some(f) if !path.is_empty() && !html::is_external(path) && ids.contains(html::frag_id(f).as_ref()) => Edit::Set(format!("#{f}")),
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
/// 这里只是不清洗时的兜底。**先折叠单元素重复 id 属性**（非法 XHTML 兜底，见 `collapse_dup_id_attrs`），再做跨章值去重。
pub fn dedup_ids_in_chapter(html: &str, seen: &mut HashSet<String>) -> String {
    let html = &collapse_dup_id_attrs(html);
    let rename = plan_id_renames(html, seen);
    if rename.is_empty() {
        return html.to_string();
    }
    rename_ids(html, &rename)
}

/// 本章里已在别处（`seen`）出现过的 id → 新名（`{id}-x{n}`，全书唯一）；本章新见到的 id 记进 `seen`。
pub(crate) fn plan_id_renames(html: &str, seen: &mut HashSet<String>) -> HashMap<String, String> {
    let mut rename: HashMap<String, String> = HashMap::new();
    let mut local: HashSet<String> = HashSet::new();
    let ids: Vec<String> = html::tags(html)
        .filter(|t| t.is_start())
        .flat_map(|t| html::attrs(&html[t.start..t.end]).into_iter().filter(|a| a.is("id") && !a.value.is_empty()).map(|a| a.value.to_string()).collect::<Vec<_>>())
        .collect();
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
