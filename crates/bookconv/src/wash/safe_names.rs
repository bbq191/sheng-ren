//! 文件名里有安卓存储（FAT/exFAT）不能用的字符（`*:?"<>|\` 和控制字符）的条目改名，引用一起改。
//!
//! 2026-10-06 掌阅自带阅读器：《春雪》《飘》的封面图叫 `**::**::…jpg`，OPF 里写成 `%2A%2A%3A%3A…`，书架没有封面缩略图
//! （名字正常的书都有）。只改文件名部分；目录名里有这类字符的不动（没见过）。
use super::*;

/// 安卓存储上不能出现在文件名里的字符。
fn is_bad(c: char) -> bool {
    matches!(c, '*' | ':' | '?' | '"' | '<' | '>' | '|' | '\\') || c.is_control()
}

/// 改名后的文件名：坏字符换 `_`；和已有的撞了就在扩展名前加 `-2`、`-3`……
fn safe_file_name(file: &str, dir: &str, taken: &HashSet<String>) -> String {
    let clean: String = file.chars().map(|c| if is_bad(c) { '_' } else { c }).collect();
    let (stem, ext) = match clean.rfind('.') {
        Some(i) if i > 0 => (&clean[..i], &clean[i..]),
        _ => (clean.as_str(), ""),
    };
    let join = |f: &str| if dir.is_empty() { f.to_string() } else { format!("{dir}/{f}") };
    let mut n = 1;
    let mut cand = clean.clone();
    while taken.contains(&join(&cand)) {
        n += 1;
        cand = format!("{stem}-{n}{ext}");
    }
    cand
}

/// CSS（样式表、`<style>`、`style` 属性）里的 `url(…)`：书内文件改了名的换成新路径（相对 `base_dir`）。
fn rewrite_css_urls(text: &str, base_dir: &str, map: &HashMap<String, String>) -> Option<String> {
    static URL: OnceLock<Regex> = OnceLock::new();
    let url = URL.get_or_init(|| Regex::new(r#"(?i)url\(\s*(?:"([^"]*)"|'([^']*)'|([^)\s'"]*))\s*\)"#).unwrap());
    let mut changed = false;
    let out = url.replace_all(text, |c: &regex::Captures| {
        let raw = c.get(1).or_else(|| c.get(2)).or_else(|| c.get(3)).map_or("", |m| m.as_str());
        if dead_refs::is_non_file_ref(raw) {
            return c[0].to_string();
        }
        let p = raw.split(['#', '?']).next().unwrap_or("");
        let target = resolve(base_dir, &percent_decode(&crate::util::xml_unescape(p)));
        match map.get(&target) {
            Some(new) => {
                changed = true;
                format!("url(\"{}\")", crate::epubzip::href_to(base_dir, new, ""))
            }
            None => c[0].to_string(),
        }
    });
    changed.then(|| out.into_owned())
}

/// 改名，并改写所有引用：OPF 清单的 `href`、全书 `href`/`src`/`xlink:href`（[`rewrite_book_links`]）、CSS 的 `url()`。
pub(super) fn rename_unsafe_entries(entries: &mut [Entry], rep: &mut WashReport) {
    let mut taken: HashSet<String> = entries.iter().map(|e| e.name.clone()).collect();
    let mut map: HashMap<String, String> = HashMap::new();
    for e in entries.iter() {
        if e.name == "mimetype" || e.name.starts_with("META-INF/") {
            continue;
        }
        let (dir, file) = match e.name.rfind('/') {
            Some(i) => (&e.name[..i], &e.name[i + 1..]),
            None => ("", e.name.as_str()),
        };
        if !file.chars().any(is_bad) || dir.chars().any(is_bad) {
            continue;
        }
        let new_file = safe_file_name(file, dir, &taken);
        let new = if dir.is_empty() { new_file } else { format!("{dir}/{new_file}") };
        taken.insert(new.clone());
        map.insert(e.name.clone(), new);
    }
    if map.is_empty() {
        return;
    }
    for e in entries.iter_mut() {
        if let Some(new) = map.get(&e.name) {
            e.name = new.clone();
        }
    }
    // 全书 `href`/`src`/`xlink:href`。不用 `rewrite_book_links`：它把带 `:` 的值当书外链接跳过，而原书 nav 里常有没编码的
    // `****:*::.xhtml`（《飘》）。这里只改解析出来正好是改了名的文件的，真的网址对不上条目名、不会误改。
    // 只改了文件名、目录不变，所以按改名后的条目名解析相对链接结果一样。OPF 的 manifest 下面另改。
    for e in entries.iter_mut() {
        let l = e.name.to_ascii_lowercase();
        let is_opf = l.ends_with(".opf");
        if !(is_opf || l.ends_with(".ncx") || is_html_entry(&e.name, &e.data)) {
            continue;
        }
        let Ok(text) = std::str::from_utf8(&e.data) else { continue };
        let name = e.name.as_str();
        let new = html::edit_attrs(text, &["href", "src", "xlink:href"], |t, a| {
            if is_opf && opf::is_local(t.name, "item") {
                return Edit::Keep;
            }
            let (target, _) = crate::epubzip::resolve_link(name, a.value);
            match map.get(&target) {
                Some(new) => Edit::Set(crate::epubzip::href_to(dir_of(name), new, html::split_href(a.value).1.unwrap_or(""))),
                None => Edit::Keep,
            }
        });
        if let Cow::Owned(new) = new {
            e.data = new.into_bytes();
        }
    }
    if let Some(oi) = find_opf(entries) {
        let opf_dir = dir_of(&entries[oi].name).to_string();
        let text = String::from_utf8_lossy(&entries[oi].data).into_owned();
        let edits: Vec<(usize, usize, String)> = manifest_items(&text)
            .iter()
            .filter_map(|it| map.get(&it.path(&opf_dir)).map(|new| (it.pos, it.pos + it.tag.len(), html::set_attr(it.tag, "href", &crate::epubzip::href_to(&opf_dir, new, "")))))
            .collect();
        if !edits.is_empty() {
            entries[oi].data = html::apply_edits(&text, edits).into_bytes();
        }
    }
    for e in entries.iter_mut() {
        let l = e.name.to_ascii_lowercase();
        if !(l.ends_with(".css") || is_html_entry(&e.name, &e.data)) {
            continue;
        }
        let Ok(text) = std::str::from_utf8(&e.data) else { continue };
        if let Some(new) = rewrite_css_urls(text, dir_of(&e.name), &map) {
            e.data = new.into_bytes();
        }
    }
    let mut renamed: Vec<(String, String)> = map.into_iter().collect();
    renamed.sort();
    rep.renamed = renamed;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn e(name: &str, data: &str) -> Entry {
        Entry { name: name.into(), data: data.as_bytes().to_vec() }
    }

    /// 《春雪》：封面图名全是 `*` 和 `:`，OPF 里百分号编码；正文、CSS 里也引用它。
    #[test]
    fn unsafe_image_names_renamed_with_references() {
        let enc = "%2A%2A%3A%3A.jpg";
        let mut v = vec![
            e("mimetype", "application/epub+zip"),
            e("OEBPS/content.opf", &format!(r#"<package><manifest><item id="f45" properties="cover-image" href="Images/{enc}" media-type="image/jpeg"/><item id="c" href="Text/c.xhtml" media-type="application/xhtml+xml"/><item id="s" href="Styles/s.css" media-type="text/css"/></manifest><spine><itemref idref="c"/></spine></package>"#)),
            e("OEBPS/Text/c.xhtml", &format!(r#"<html><body><img src="../Images/{enc}"/><div style="background:url('../Images/{enc}')"></div></body></html>"#)),
            e("OEBPS/Styles/s.css", &format!("body{{background-image:url(../Images/{enc})}}")),
            e("OEBPS/nav.xhtml", r#"<html><body><nav><ol><li><a href="Text/**::x.xhtml#a">一</a></li><li><a href="http://e.com/a:b">外</a></li></ol></nav></body></html>"#),
            e("OEBPS/Text/**::x.xhtml", "<html><body><p id='a'>x</p></body></html>"),
            e("OEBPS/Images/**::.jpg", "jpg"),
            e("OEBPS/Images/__--.jpg", "jpg"),
        ];
        let mut rep = WashReport::default();
        rename_unsafe_entries(&mut v, &mut rep);
        assert_eq!(rep.renamed, [("OEBPS/Images/**::.jpg".to_string(), "OEBPS/Images/____.jpg".to_string()), ("OEBPS/Text/**::x.xhtml".to_string(), "OEBPS/Text/____x.xhtml".to_string())]);
        // 没编码、带 `:` 的书内链接也改；真的网址不动
        let nav = String::from_utf8(v.iter().find(|x| x.name == "OEBPS/nav.xhtml").unwrap().data.clone()).unwrap();
        assert!(nav.contains(r#"href="Text/____x.xhtml#a""#) && nav.contains(r#"href="http://e.com/a:b""#), "{nav}");
        assert!(v.iter().any(|x| x.name == "OEBPS/Images/____.jpg"));
        let s = |n: &str| String::from_utf8(v.iter().find(|x| x.name == n).unwrap().data.clone()).unwrap();
        assert!(s("OEBPS/content.opf").contains(r#"href="Images/____.jpg""#), "{}", s("OEBPS/content.opf"));
        assert!(s("OEBPS/Text/c.xhtml").contains(r#"src="../Images/____.jpg""#) && s("OEBPS/Text/c.xhtml").contains(r#"url("../Images/____.jpg")"#), "{}", s("OEBPS/Text/c.xhtml"));
        assert_eq!(s("OEBPS/Styles/s.css"), r#"body{background-image:url("../Images/____.jpg")}"#);
        // 正常的名字不动
        assert!(v.iter().any(|x| x.name == "OEBPS/Images/__--.jpg"));
    }

    #[test]
    fn collision_gets_suffix() {
        let taken: HashSet<String> = ["I/a_b.jpg".to_string()].into();
        assert_eq!(safe_file_name("a:b.jpg", "I", &taken), "a_b-2.jpg");
    }
}
