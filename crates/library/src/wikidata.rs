//! Wikidata：按书名、作者找原作（原文名、Open Library ID、作品图片、首次出版时间），以及作者照片。

use crate::matching::{norm, similarity};
use crate::net::{enc, Net};

/// 候选作品。
#[derive(Default, Debug)]
pub(crate) struct Work {
    pub(crate) qid: String,
    pub(crate) zh: Vec<String>,
    pub(crate) en: String,
    /// 日文名（日本作品没有英文名时用它搜封面）。
    pub(crate) ja: String,
    pub(crate) original: String,
    /// 作者英文名（搜封面时一起给，减少同名书）。
    pub(crate) author_en: String,
    pub(crate) ol: String,
    pub(crate) image: String,
    pub(crate) author_names: Vec<String>,
    /// 首次出版时间（P577，只取年份）。
    pub(crate) published: String,
}

/// SPARQL 查询。
fn sparql(net: &Net, q: &str) -> Result<serde_json::Value, String> {
    net.json(&format!("https://query.wikidata.org/sparql?format=json&query={}", enc(q)))
}

/// 人名的几种写法：原样、间隔号统一成 `·`（好读用 `．`，Wikidata 用 `·`/`‧`），`letters_only` 时再加上只留字母数字的。
fn name_variants(a: &str, letters_only: bool) -> Vec<String> {
    let mut v = vec![a.to_string(), a.replace(['．', '‧', '・', '•', '.'], "·")];
    if letters_only {
        v.push(a.chars().filter(|c| c.is_alphanumeric()).collect());
    }
    v
}

/// 按几种写法搜条目，合并去重。`first_hit`：某种写法搜到了就不再试后面的。
fn search_variants(net: &Net, variants: &[String], first_hit: bool) -> Result<Vec<String>, String> {
    let mut ids: Vec<String> = Vec::new();
    for v in variants {
        for id in wikidata_search(net, v)? {
            if !ids.contains(&id) {
                ids.push(id);
            }
        }
        if first_hit && !ids.is_empty() {
            break;
        }
    }
    Ok(ids)
}

pub(crate) fn wikidata_search(net: &Net, q: &str) -> Result<Vec<String>, String> {
    let mut ids = Vec::new();
    for lang in ["zh", "zh-hant"] {
        let url = format!("https://www.wikidata.org/w/api.php?action=wbsearchentities&type=item&limit=7&format=json&language={lang}&uselang={lang}&search={}", enc(q));
        for x in net.json(&url)?["search"].as_array().into_iter().flatten() {
            if let Some(id) = x["id"].as_str().filter(|id| !ids.iter().any(|i: &String| i == id)) {
                ids.push(id.to_string());
            }
        }
    }
    Ok(ids)
}

/// 作品及其中文名、英文名、原文名、Open Library ID、图片、作者名。`filter` 是 SPARQL 里限定 `?w` 的那一句。
pub(crate) fn works(net: &Net, filter: &str) -> Result<Vec<Work>, String> {
    let q = format!(
        r#"SELECT ?w ?l ?en ?ja ?orig ?ol ?img ?an ?aen ?pub WHERE {{ {filter}
  OPTIONAL {{ {{ ?w rdfs:label ?l }} UNION {{ ?w skos:altLabel ?l }} FILTER(STRSTARTS(LANG(?l),"zh")) }}
  OPTIONAL {{ ?w rdfs:label ?en FILTER(LANG(?en)="en") }}
  OPTIONAL {{ ?w rdfs:label ?ja FILTER(LANG(?ja)="ja") }}
  OPTIONAL {{ ?w wdt:P50/rdfs:label ?aen FILTER(LANG(?aen)="en") }}
  OPTIONAL {{ ?w wdt:P1476 ?orig }}
  OPTIONAL {{ ?w wdt:P648 ?ol }}
  OPTIONAL {{ ?w wdt:P18 ?img }}
  OPTIONAL {{ ?w wdt:P577 ?pub }}
  OPTIONAL {{ ?w wdt:P50 ?a . {{ ?a rdfs:label ?an }} UNION {{ ?a skos:altLabel ?an }} FILTER(STRSTARTS(LANG(?an),"zh")) }}
}} LIMIT 2000"#
    );
    let v = sparql(net, &q)?;
    let mut out: Vec<Work> = Vec::new();
    for r in v["results"]["bindings"].as_array().into_iter().flatten() {
        let g = |k: &str| r[k]["value"].as_str().unwrap_or("").to_string();
        let qid = g("w").rsplit('/').next().unwrap_or("").to_string();
        let i = match out.iter().position(|w| w.qid == qid) {
            Some(i) => i,
            None => {
                out.push(Work { qid, ..Default::default() });
                out.len() - 1
            }
        };
        let w = &mut out[i];
        for (dst, k) in [(&mut w.en, "en"), (&mut w.ja, "ja"), (&mut w.original, "orig"), (&mut w.ol, "ol"), (&mut w.image, "img"), (&mut w.author_en, "aen"), (&mut w.published, "pub")] {
            if dst.is_empty() {
                *dst = g(k);
            }
        }
        for (list, k) in [(&mut w.zh, "l"), (&mut w.author_names, "an")] {
            let v = g(k);
            if !v.is_empty() && !list.contains(&v) {
                list.push(v);
            }
        }
    }
    Ok(out)
}

/// 在候选作品里挑书名对得上的：完全相同（规整后）优先；否则取字重合度 ≥ 0.75 且唯一的。
pub(crate) fn pick_by_title(mut ws: Vec<Work>, title: &str) -> Option<Work> {
    let t = norm(title);
    if let Some(i) = ws.iter().position(|w| w.zh.iter().any(|l| norm(l) == t)) {
        return Some(ws.swap_remove(i));
    }
    let close: Vec<usize> = (0..ws.len()).filter(|&i| ws[i].zh.iter().any(|l| similarity(&norm(l), &t) >= 0.75)).collect();
    match close[..] {
        [i] => Some(ws.swap_remove(i)),
        _ => None,
    }
}

/// 作品的作者名和给出的作者名最像的程度（译名用字可能不同，按字重合度）。
pub(crate) fn author_score(w: &Work, authors: &[String]) -> f32 {
    authors.iter().flat_map(|a| w.author_names.iter().map(move |n| similarity(&norm(a), &norm(n)))).fold(0.0, f32::max)
}

/// 按书名、作者名在 Wikidata 找原作。
pub(crate) fn find_work(net: &Net, title: &str, authors: &[String]) -> Result<Option<Work>, String> {
    // ① 先按书名找（名著一找就中），书名完全对得上的作品里挑作者名最像的；没有作者信息时只接受唯一的结果
    let ids = wikidata_search(net, title)?;
    if !ids.is_empty() {
        let values: String = ids.iter().take(12).map(|i| format!("wd:{i} ")).collect();
        let t = norm(title);
        let mut cands: Vec<Work> = works(net, &format!("VALUES ?w {{ {values}}} ?w wdt:P50 ?anyauthor ."))?.into_iter().filter(|w| w.zh.iter().any(|l| norm(l) == t)).collect();
        if authors.iter().all(|a| norm(a).is_empty()) {
            if cands.len() == 1 {
                return Ok(cands.pop());
            }
        } else {
            cands.retain(|w| author_score(w, authors) >= 0.6);
            cands.sort_by(|a, b| author_score(b, authors).total_cmp(&author_score(a, authors)));
            if let Some(w) = cands.into_iter().next() {
                return Ok(Some(w));
            }
        }
    }
    // ② 书名没找到（书名撞车太多、或译名不同）：先找作者，再在他的作品里按书名找（允许译名用字不同）
    for a in authors.iter().filter(|a| !norm(a).is_empty()) {
        let ids = search_variants(net, &name_variants(a, true), true)?;
        if ids.is_empty() {
            continue;
        }
        let values: String = ids.iter().take(6).map(|i| format!("wd:{i} ")).collect();
        if let Some(w) = pick_by_title(works(net, &format!("VALUES ?author {{ {values}}} ?w wdt:P50 ?author ."))?, title) {
            return Ok(Some(w));
        }
    }
    Ok(None)
}

/// 作者照片（Wikidata 人物的 P18 图片）：按作者名找人物，名字最像（字重合度 ≥ 0.6）且有照片的那个。
/// 返回（"Q号 名字", 缩到 800 宽的图片网址）；没有返回 `None`，网络出错返回错误。
pub(crate) fn author_portrait(net: &Net, authors: &[String]) -> Result<Option<(String, String)>, String> {
    for a in authors.iter().filter(|a| !norm(a).is_empty()) {
        let ids = search_variants(net, &name_variants(a, false), false)?;
        if ids.is_empty() {
            continue;
        }
        let values: String = ids.iter().take(8).map(|i| format!("wd:{i} ")).collect();
        let q = format!(
            r#"SELECT ?a ?img ?l WHERE {{ VALUES ?a {{ {values}}} ?a wdt:P31 wd:Q5 ; wdt:P18 ?img .
  {{ ?a rdfs:label ?l }} UNION {{ ?a skos:altLabel ?l }} FILTER(STRSTARTS(LANG(?l),"zh")) }}"#
        );
        let v = sparql(net, &q)?;
        let best = v["results"]["bindings"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|r| {
                let g = |k: &str| r[k]["value"].as_str().unwrap_or("").to_string();
                let score = similarity(&norm(a), &norm(&g("l")));
                (score >= 0.6).then(|| (score, g("a").rsplit('/').next().unwrap_or("").to_string(), g("l"), g("img")))
            })
            .max_by(|x, y| x.0.total_cmp(&y.0));
        if let Some((_, qid, name, img)) = best {
            return Ok(Some((format!("{qid} {name}"), format!("{}?width=800", img.replace("http://", "https://")))));
        }
    }
    Ok(None)
}

