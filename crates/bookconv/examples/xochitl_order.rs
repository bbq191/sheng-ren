//! Move 产物体检：照 CSS 层叠和照 xochitl 层叠（`Sheet::cascade_xochitl`：`!important` 不算、优先级一样比规则在各自样式表里的序号）
//! 算出来不一样的元素、属性，按「属性: 两边的值」汇总、各给一个例子。用法见 docs/development.md#回归。
//! `cargo run --release -p bookconv --example xochitl_order -- 产物.epub…`
use bookconv::cascade::{Rules, Sheet};
use std::collections::{BTreeMap, HashMap};
use bookconv::zip;
use std::io::Read;
use std::sync::Arc;

fn main() {
    for path in std::env::args().skip(1) {
        let f = std::fs::File::open(&path).unwrap();
        let mut z = zip::ZipArchive::new(f).unwrap();
        let mut css: HashMap<String, Arc<Rules>> = HashMap::new();
        let mut docs = Vec::new();
        for i in 0..z.len() {
            let mut e = z.by_index(i).unwrap();
            let name = e.name().to_string();
            let mut s = String::new();
            if name.ends_with(".css") {
                let mut b = Vec::new();
                e.read_to_end(&mut b).unwrap();
                s = String::from_utf8_lossy(&b).into_owned();
                css.insert(name.clone(), Rules::parse(&s, &name, None));
            } else if (name.ends_with(".html") || name.ends_with(".xhtml") || name.ends_with(".htm")) && e.read_to_string(&mut s).is_ok() {
                docs.push((name, s));
            }
        }
        let mut diffs: BTreeMap<String, (usize, String)> = BTreeMap::new();
        let mut n_el = 0;
        for (name, s) in &docs {
            let html = scraper::Html::parse_document(s);
            let sheet = Sheet::for_doc(&html, name, None, |p| css.get(p).cloned());
            for el in html.root_element().descendants().filter_map(scraper::ElementRef::wrap) {
                let a = sheet.cascade(&el, name);
                let b = sheet.cascade_xochitl(&el, name);
                let mut any = false;
                for (k, v) in &a {
                    let w = b.get(k).map(String::as_str).unwrap_or("");
                    let strip = |x: &str| x.trim().trim_end_matches("!important").trim().to_string();
                    if strip(v) != strip(w) {
                        any = true;
                        let key = format!("{k}: CSS {} / Move {}", strip(v), strip(w));
                        let ent = diffs.entry(key).or_insert((0, String::new()));
                        ent.0 += 1;
                        if ent.1.is_empty() {
                            let cls = el.value().attr("class").unwrap_or("");
                            let text: String = el.text().collect::<String>().chars().take(20).collect();
                            ent.1 = format!("{name} <{} class=\"{cls}\"> {text}", el.value().name());
                        }
                    }
                }
                n_el += usize::from(any);
            }
        }
        println!("== {path}：{n_el} 个元素不一样");
        for (k, (n, ex)) in &diffs {
            println!("  {n:>6}  {k}    例：{ex}");
        }
    }
}
