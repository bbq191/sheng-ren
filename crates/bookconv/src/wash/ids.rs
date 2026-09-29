//! 全书 id 去重：xochitl 的锚点是**全书命名空间**，两章都用 `id="fn1"` 时所有 `#fn1` 都跳到第一处。
//!
//! 优化器逐章做的 `htmlproc::dedup_ids_in_chapter` 只看得见本章，改了名却改不到别的文件（NCX、nav、目录页、
//! 其它章）里指向它的 `章.html#fn1`，那些链接就断了（2026-09-27 审计）。清洗层看得见整本书：在这里先按阅读顺序
//! （spine 在前，其余 html 在后）把后出现的重复 id 改名（`{id}-x{n}`，与逐章去重同一规则），再把全书所有指向
//! （该文件, 旧 id）的链接改成新名。走完这一步全书 id 已唯一，优化器那一遍不会再改名。
//!
//! 只改 html 里的 `id` 属性；NCX `navPoint`、OPF `item` 的 id 不是锚点，不动。同一文件里自己重复的 id 不在这里处理
//! （分不清链接指的是哪一个，保守不动）。
use super::*;

pub(super) fn dedup_ids_across_book(entries: &mut [Entry], rep: &mut WashReport) {
    let Some(opf) = parse_opf(entries) else { return };
    let mut order: Vec<usize> = Vec::new();
    let mut in_order: HashSet<usize> = HashSet::new();
    let by_name: HashMap<&str, usize> = entries.iter().enumerate().map(|(i, e)| (e.name.as_str(), i)).collect();
    for p in &opf.spine {
        if let Some(&i) = by_name.get(p.as_str()) {
            if in_order.insert(i) {
                order.push(i);
            }
        }
    }
    for (i, e) in entries.iter().enumerate() {
        if !in_order.contains(&i) && is_html_entry(&e.name, &e.data) {
            order.push(i);
        }
    }
    let mut seen: HashSet<String> = HashSet::new();
    let mut renames: HashMap<String, HashMap<String, String>> = HashMap::new();
    for &i in &order {
        let e = &entries[i];
        if !is_html_entry(&e.name, &e.data) {
            continue;
        }
        let Ok(t) = std::str::from_utf8(&e.data) else { continue };
        let r = crate::htmlproc::plan_id_renames(t, &mut seen);
        if !r.is_empty() {
            renames.insert(e.name.clone(), r);
        }
    }
    if renames.is_empty() {
        return;
    }
    rep.dup_ids_renamed = renames.values().map(HashMap::len).sum();
    // 先改 id 属性本身，再改全书指向它们的链接。
    for e in entries.iter_mut() {
        let Some(own) = renames.get(&e.name) else { continue };
        let Ok(text) = std::str::from_utf8(&e.data) else { continue };
        let new = html::edit_attrs(text, &["id"], |_, a| own.get(a.value).map_or(Edit::Keep, |n| Edit::Set(n.clone())));
        if let Cow::Owned(new) = new {
            e.data = new.into_bytes();
        }
    }
    rewrite_book_links(entries, |_| false, |l| {
        let frag = l.frag.filter(|f| !f.is_empty())?;
        let n = renames.get(&l.target)?.get(html::frag_id(frag).as_ref())?;
        Some(format!("{}#{n}", html::split_href(l.value).0))
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn duplicate_ids_renamed_and_links_from_other_files_follow() {
        let e = |n: &str, d: &str| Entry { name: n.into(), data: d.as_bytes().to_vec() };
        let mut v = vec![
            e("OEBPS/content.opf", r#"<package><manifest><item id="a" href="a.xhtml" media-type="application/xhtml+xml"/><item id="b" href="t/b.xhtml" media-type="application/xhtml+xml"/><item id="ncx" href="toc.ncx" media-type="application/x-dtbncx+xml"/></manifest><spine><itemref idref="a"/><itemref idref="b"/></spine><guide><reference href="t/b.xhtml#fn1" type="text"/></guide></package>"#),
            e("OEBPS/a.xhtml", r##"<html><body><p id="fn1">甲</p><a href="t/b.xhtml#fn1">去乙注</a><a href="#fn1">本章</a></body></html>"##),
            e("OEBPS/t/b.xhtml", r##"<html><body><p id='fn1' data-id="fn1">乙</p><a href="#fn1">本章</a><a href="../a.xhtml#fn1">去甲</a></body></html>"##),
            e("OEBPS/toc.ncx", r#"<ncx><navMap><navPoint id="fn1"><navLabel><text>乙</text></navLabel><content src="t/b.xhtml#fn1"/></navPoint></navMap></ncx>"#),
        ];
        let mut rep = WashReport::default();
        dedup_ids_across_book(&mut v, &mut rep);
        assert_eq!(rep.dup_ids_renamed, 1);
        let s = |n: &str| String::from_utf8(v.iter().find(|x| x.name == n).unwrap().data.clone()).unwrap();
        assert_eq!(s("OEBPS/a.xhtml"), r##"<html><body><p id="fn1">甲</p><a href="t/b.xhtml#fn1-x2">去乙注</a><a href="#fn1">本章</a></body></html>"##);
        assert_eq!(s("OEBPS/t/b.xhtml"), r##"<html><body><p id='fn1-x2' data-id="fn1">乙</p><a href="#fn1-x2">本章</a><a href="../a.xhtml#fn1">去甲</a></body></html>"##);
        assert!(s("OEBPS/toc.ncx").contains(r#"<navPoint id="fn1">"#) && s("OEBPS/toc.ncx").contains(r#"src="t/b.xhtml#fn1-x2""#));
        assert!(s("OEBPS/content.opf").contains(r#"href="t/b.xhtml#fn1-x2""#));
    }
}
