//! 给没有封面的书联网找封面：`booklib cover`。
//!
//! 来源（参照 Koodo Reader 用的几个书目源：豆瓣、Open Library 等，只借鉴"用哪些源"，代码是自己写的——它是 AGPL-3.0）：
//! 0. **豆瓣**：中文版封面，最贴近用户手上的书。搜索建议接口按书名找，书名简体化后比对（好读是繁体、豆瓣多是简体），
//!    作者去掉国籍前缀后按字重合度核对；取大图，挑分辨率最高的。没有公开 API，请求很少且节流。
//!
//! 豆瓣没有时找**原作**的封面（用户 2026-09-27：原版封面也可以）：
//! 1. **Wikidata**：先按书名找（名著如《一九八四》《動物農莊》一找就中），书名完全对得上的作品里挑作者名最像的；
//!    找不到（"雪人""告白"这种书名撞车太多，或译名不同）再找作者，在他的作品里按书名找。中文书名比对所有中文标签
//!    和别名（繁简都有）；比较前统一全角半角、去掉括号里的消歧义说明和标点；译名用字不同（歐威爾/奧威爾、階梯/台階）
//!    时按字重合度判断，只接受足够像的结果。
//! 2. **Open Library**：作品有 Open Library ID 就取它的封面；没有就按英文书名、原文书名搜；再没有用 Wikimedia Commons
//!    上的作品图片（多是初版封面）。
//!
//! 3. 都找不到：**生成**一张——书名在上、作者头像（Wikidata 人物照片）居中、作者名在下，样式按书的 id 挑
//!    （见 `covergen`）。没有作者照片时中间画作者名的第一个字。
//!
//! 找到的封面存在 `masters/<id>/cover.<ext>`，来源记进 `meta.json`（哪个作品、哪个网址），错了可以 `--clear` 去掉。
//! 生成时书里没有封面才放进去（只在 OPF 里声明封面图，不加封面页，正文不变）。原件不动。

use crate::fsutil::sha256_hex;
use crate::{Library, Meta};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::cell::Cell;
use std::io::{Read, Seek, Write};
use std::path::Path;
use std::time::{Duration, Instant};

/// 书库里存着的封面（`Meta::cover`）。
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct CoverInfo {
    /// `masters/<id>/` 下的文件名。
    pub file: String,
    pub sha256: String,
    /// 从哪下载的。
    pub source_url: String,
    /// 匹配到的作品：Wikidata id 和作品名（给人核对）。
    pub work: String,
}

/// 一次找封面的结果。
pub enum CoverResult {
    Found(CoverInfo),
    /// 书里本来就有封面。
    HasCover,
    /// 已经找过、存着了（`force` 才重找）。
    Existing(CoverInfo),
    /// 没找到原作封面，生成了一张（第二项是没找到的原因）。
    Generated(CoverInfo, String),
}

const UA: &str = "booklib/0.1 (personal e-book library tool)";

/// 带节流和重试的 HTTP：Wikidata 限速严（连续请求会 429），每个请求间隔至少 1.2 秒，429 时按 Retry-After 等。
struct Net {
    agent: ureq::Agent,
    last: Cell<Option<Instant>>,
}

impl Net {
    fn new() -> Net {
        Net { agent: ureq::AgentBuilder::new().timeout(Duration::from_secs(30)).user_agent(UA).build(), last: Cell::new(None) }
    }

    fn fetch(&self, url: &str) -> Result<Vec<u8>, String> {
        self.fetch_ref(url, None)
    }

    /// `referer`：豆瓣图片服务器不带来源页会拒绝（HTTP 418）。
    fn fetch_ref(&self, url: &str, referer: Option<&str>) -> Result<Vec<u8>, String> {
        let mut last_err = String::new();
        for _ in 0..4 {
            if let Some(t) = self.last.get() {
                let gap = Duration::from_millis(1200);
                if t.elapsed() < gap {
                    std::thread::sleep(gap - t.elapsed());
                }
            }
            self.last.set(Some(Instant::now()));
            let mut req = self.agent.get(url);
            if let Some(r) = referer {
                req = req.set("Referer", r);
            }
            match req.call() {
                Ok(r) => {
                    let mut buf = Vec::new();
                    r.into_reader().take(20 << 20).read_to_end(&mut buf).map_err(|e| e.to_string())?;
                    return Ok(buf);
                }
                Err(ureq::Error::Status(404, _)) => return Err("404".into()),
                Err(ureq::Error::Status(code, r)) if code == 429 || code >= 500 => {
                    let wait = r.header("Retry-After").and_then(|v| v.parse().ok()).unwrap_or(5u64).min(60);
                    last_err = format!("HTTP {code}");
                    std::thread::sleep(Duration::from_secs(wait));
                }
                Err(e) => {
                    last_err = e.to_string();
                    std::thread::sleep(Duration::from_secs(2));
                }
            }
        }
        Err(format!("{url}: {last_err}"))
    }

    fn json(&self, url: &str) -> Result<Value, String> {
        serde_json::from_slice(&self.fetch(url)?).map_err(|e| format!("{url}: {e}"))
    }
}

fn enc(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// 比较用的规整：全角转半角、去掉结尾括号里的消歧义说明（"雪人 (小說)"）、只留字母数字和汉字、小写。
fn norm(s: &str) -> String {
    let s: String = s.chars().map(|c| if ('\u{FF01}'..='\u{FF5E}').contains(&c) { char::from_u32(c as u32 - 0xFEE0).unwrap_or(c) } else { c }).collect();
    let s = s.trim();
    let s = match s.rfind(['(', '（']) {
        Some(i) if s.ends_with([')', '）']) && i > 0 => &s[..i],
        _ => s,
    };
    s.chars().filter(|c| c.is_alphanumeric()).flat_map(char::to_lowercase).collect()
}

/// 字重合度：两边共有的字数 / 较长一边的字数（译名用字不同时判断"是不是同一个名字"）。
fn similarity(a: &str, b: &str) -> f32 {
    let (a, b): (Vec<char>, Vec<char>) = (a.chars().collect(), b.chars().collect());
    if a.is_empty() || b.is_empty() {
        return 0.0;
    }
    let mut rest = b.clone();
    let common = a.iter().filter(|c| rest.iter().position(|x| x == *c).map(|i| rest.remove(i)).is_some()).count();
    common as f32 / a.len().max(b.len()) as f32
}

/// 候选作品。
#[derive(Default, Debug)]
struct Work {
    qid: String,
    zh: Vec<String>,
    en: String,
    /// 日文名（日本作品没有英文名时用它搜封面）。
    ja: String,
    original: String,
    /// 作者英文名（搜封面时一起给，减少同名书）。
    author_en: String,
    ol: String,
    image: String,
    author_names: Vec<String>,
}

fn wikidata_search(net: &Net, q: &str) -> Result<Vec<String>, String> {
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
fn works(net: &Net, filter: &str) -> Result<Vec<Work>, String> {
    let q = format!(
        r#"SELECT ?w ?l ?en ?ja ?orig ?ol ?img ?an ?aen WHERE {{ {filter}
  OPTIONAL {{ {{ ?w rdfs:label ?l }} UNION {{ ?w skos:altLabel ?l }} FILTER(STRSTARTS(LANG(?l),"zh")) }}
  OPTIONAL {{ ?w rdfs:label ?en FILTER(LANG(?en)="en") }}
  OPTIONAL {{ ?w rdfs:label ?ja FILTER(LANG(?ja)="ja") }}
  OPTIONAL {{ ?w wdt:P50/rdfs:label ?aen FILTER(LANG(?aen)="en") }}
  OPTIONAL {{ ?w wdt:P1476 ?orig }}
  OPTIONAL {{ ?w wdt:P648 ?ol }}
  OPTIONAL {{ ?w wdt:P18 ?img }}
  OPTIONAL {{ ?w wdt:P50 ?a . {{ ?a rdfs:label ?an }} UNION {{ ?a skos:altLabel ?an }} FILTER(STRSTARTS(LANG(?an),"zh")) }}
}} LIMIT 2000"#
    );
    let v = net.json(&format!("https://query.wikidata.org/sparql?format=json&query={}", enc(&q)))?;
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
        for (dst, k) in [(&mut w.en, "en"), (&mut w.ja, "ja"), (&mut w.original, "orig"), (&mut w.ol, "ol"), (&mut w.image, "img"), (&mut w.author_en, "aen")] {
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
fn pick_by_title(mut ws: Vec<Work>, title: &str) -> Option<Work> {
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
fn author_score(w: &Work, authors: &[String]) -> f32 {
    authors.iter().flat_map(|a| w.author_names.iter().map(move |n| similarity(&norm(a), &norm(n)))).fold(0.0, f32::max)
}

/// 按书名、作者名在 Wikidata 找原作。
fn find_work(net: &Net, title: &str, authors: &[String]) -> Result<Option<Work>, String> {
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
        let variants = [a.clone(), a.replace(['．', '‧', '・', '•', '.'], "·"), a.chars().filter(|c| c.is_alphanumeric()).collect()];
        let mut ids: Vec<String> = Vec::new();
        for v in variants {
            for id in wikidata_search(net, &v)? {
                if !ids.contains(&id) {
                    ids.push(id);
                }
            }
            if !ids.is_empty() {
                break;
            }
        }
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

/// 拿去找作品的书名：书里的书名，加上原件文件名里的（`作者《书名》` 取书名号里的；其它按常见命名规整）。
/// 用户改过文件名（比如改成更通行的译名）时，文件名里的书名往往更准。规整后相同的只留一个。
fn title_candidates(meta: &Meta) -> Vec<String> {
    let stem = meta.source.rsplit_once('.').map_or(meta.source.as_str(), |(s, _)| s);
    let from_file = match (stem.find('《'), stem.rfind('》')) {
        (Some(a), Some(b)) if b > a => stem[a + '《'.len_utf8()..b].to_string(),
        _ => bookconv::naming::canonical_book_name(stem),
    };
    let mut out: Vec<String> = Vec::new();
    for t in [meta.title.clone(), from_file] {
        if !norm(&t).is_empty() && !out.iter().any(|o| norm(o) == norm(&t)) {
            out.push(t);
        }
    }
    out
}

/// 简体化后再规整（豆瓣多是简体，好读的书是繁体）。
fn norm_s(s: &str) -> String {
    norm(&fast2s::convert(s))
}

/// 作者名去掉国籍前缀（"[美] 欧·亨利"、"(法) 阿尔贝·加缪"）再规整。
fn norm_author(s: &str) -> String {
    let s = s.trim();
    let s = match s.chars().next() {
        Some('[' | '［' | '(' | '（' | '【') => s.find([']', '］', ')', '）', '】']).map_or(s, |i| &s[i + s[i..].chars().next().unwrap().len_utf8()..]),
        _ => s,
    };
    norm_s(s)
}

/// 豆瓣（中文版封面，最贴近用户手上的书）：用搜索建议接口按书名找，书名（简体化后）相同、或只差卷次后缀（≤2 字）
/// 且作者对得上的条目，取大图（`/l/`），挑分辨率最高、像封面的那张。豆瓣没有公开 API，这是网页用的接口，
/// 请求要少（全程节流）；图片服务器要带来源页。
fn douban_cover(net: &Net, titles: &[String], authors: &[String]) -> Option<(Vec<u8>, &'static str, String, String)> {
    let want_authors: Vec<String> = authors.iter().map(|a| norm_author(a)).filter(|a| !a.is_empty()).collect();
    for t in titles {
        let nt = norm_s(t);
        if nt.is_empty() {
            continue;
        }
        let url = format!("https://book.douban.com/j/subject_suggest?q={}", enc(t));
        let Ok(v) = net.json(&url) else { continue };
        let mut cands: Vec<(String, String)> = Vec::new(); // (图片网址, 说明)
        for x in v.as_array().into_iter().flatten() {
            let (title, author, pic) = (x["title"].as_str().unwrap_or(""), x["author_name"].as_str().unwrap_or(""), x["pic"].as_str().unwrap_or(""));
            if pic.is_empty() || x["type"].as_str().is_some_and(|ty| ty != "b") {
                continue;
            }
            let dt = norm_s(title);
            let title_ok = dt == nt || ((nt.starts_with(&dt) || dt.starts_with(&nt)) && nt.chars().count().abs_diff(dt.chars().count()) <= 2);
            let author_ok = if want_authors.is_empty() {
                dt == nt
            } else {
                author.split(['/', '、', ',', '，']).any(|a| want_authors.iter().any(|w| similarity(w, &norm_author(a)) >= 0.6))
            };
            if title_ok && author_ok {
                let large = pic.replace("/view/subject/s/", "/view/subject/l/").replace("/view/subject/m/", "/view/subject/l/");
                let year = x["year"].as_str().unwrap_or("");
                cands.push((large, format!("豆瓣 {title} {author} {year}").trim().to_string()));
            }
        }
        // 前几个候选里挑分辨率最高、像封面的（老条目只有 200 多像素宽的小图）
        let mut best: Option<(u64, Vec<u8>, &'static str, String, String)> = None;
        for (u, label) in cands.into_iter().take(4) {
            let Ok(bytes) = net.fetch_ref(&u, Some("https://book.douban.com/")) else { continue };
            let Some(ext) = plausible_cover(&bytes) else { continue };
            let area = image::ImageReader::new(std::io::Cursor::new(&bytes)).with_guessed_format().ok().and_then(|r| r.into_dimensions().ok()).map_or(0, |(w, h)| w as u64 * h as u64);
            let big_enough = area >= 600 * 900;
            if best.as_ref().is_none_or(|b| area > b.0) {
                best = Some((area, bytes, ext, u, label));
            }
            if big_enough {
                break;
            }
        }
        if let Some((_, b, e, u, l)) = best {
            return Some((b, e, u, l));
        }
    }
    None
}

/// 作者照片（Wikidata 人物的 P18 图片）：按作者名找人物，名字最像（字重合度 ≥ 0.6）且有照片的那个。
/// 返回（"Q号 名字", 缩到 800 宽的图片网址）。
fn author_portrait(net: &Net, authors: &[String]) -> Option<(String, String)> {
    for a in authors.iter().filter(|a| !norm(a).is_empty()) {
        let mut ids: Vec<String> = Vec::new();
        for v in [a.clone(), a.replace(['．', '‧', '・', '•', '.'], "·")] {
            for id in wikidata_search(net, &v).ok()? {
                if !ids.contains(&id) {
                    ids.push(id);
                }
            }
        }
        if ids.is_empty() {
            continue;
        }
        let values: String = ids.iter().take(8).map(|i| format!("wd:{i} ")).collect();
        let q = format!(
            r#"SELECT ?a ?img ?l WHERE {{ VALUES ?a {{ {values}}} ?a wdt:P31 wd:Q5 ; wdt:P18 ?img .
  {{ ?a rdfs:label ?l }} UNION {{ ?a skos:altLabel ?l }} FILTER(STRSTARTS(LANG(?l),"zh")) }}"#
        );
        let v = net.json(&format!("https://query.wikidata.org/sparql?format=json&query={}", enc(&q))).ok()?;
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
            return Some((format!("{qid} {name}"), format!("{}?width=800", img.replace("http://", "https://"))));
        }
    }
    None
}

/// 作品的封面图网址，按可靠程度排好（前面的下载失败或不像封面，就试下一个）：
/// Open Library 作品本身的封面 → 按英文名 + 作者搜 → 按原文名、日文名搜 → Wikimedia Commons 上的作品图片。
fn cover_urls(net: &Net, w: &Work) -> Vec<String> {
    let mut ids: Vec<i64> = Vec::new();
    let mut add = |id: i64| {
        if !ids.contains(&id) {
            ids.push(id);
        }
    };
    if !w.ol.is_empty() {
        if let Ok(v) = net.json(&format!("https://openlibrary.org/works/{}.json", enc(&w.ol))) {
            v["covers"].as_array().into_iter().flatten().filter_map(Value::as_i64).filter(|&c| c > 0).take(3).for_each(&mut add);
        }
    }
    let mut queries: Vec<String> = Vec::new();
    if !w.en.is_empty() {
        if !w.author_en.is_empty() {
            queries.push(format!("title={}&author={}", enc(&w.en), enc(&w.author_en)));
        }
        queries.push(format!("title={}", enc(&w.en)));
    }
    for t in [&w.original, &w.ja] {
        if !t.is_empty() && t != &w.en {
            queries.push(format!("title={}", enc(t)));
        }
    }
    for q in queries {
        if let Ok(v) = net.json(&format!("https://openlibrary.org/search.json?limit=5&fields=cover_i&{q}")) {
            v["docs"].as_array().into_iter().flatten().filter_map(|d| d["cover_i"].as_i64()).take(3).for_each(&mut add);
        }
    }
    let mut urls: Vec<String> = ids.into_iter().take(8).map(|id| format!("https://covers.openlibrary.org/b/id/{id}-L.jpg?default=false")).collect();
    if !w.image.is_empty() {
        urls.push(format!("{}?width=1000", w.image.replace("http://", "https://")));
    }
    urls
}

/// 下载下来的是不是像样的封面：能解码、够大、竖版比例。
fn plausible_cover(bytes: &[u8]) -> Option<&'static str> {
    let (ext, _) = bookconv::convert::common::image_ext_mime(bytes)?;
    let (w, h) = image::ImageReader::new(std::io::Cursor::new(bytes)).with_guessed_format().ok()?.into_dimensions().ok()?;
    (h >= 300 && w >= 180 && (0.45..=1.05).contains(&(w as f32 / h as f32))).then_some(ext)
}

/// EPUB 里有没有封面（有效的封面声明，或第一页里有图——优化时会把它声明成封面）。
pub(crate) fn epub_has_cover(epub: &Path) -> bool {
    bookconv::epubzip::cover_image_of(epub).is_some()
}

/// 复制 `src` 到 `dst`，加上封面：图片放进 OPF 所在目录，manifest 加一项 `properties="cover-image"`，加
/// `<meta name="cover">`。其余条目原样拷（不解压不重压）。
pub(crate) fn inject_cover(src: &Path, dst: &Path, image: &[u8], ext: &str) -> Result<(), String> {
    let f = std::fs::File::open(src).map_err(|e| format!("{}: {e}", src.display()))?;
    let mut zin = bookconv::zip::ZipArchive::new(f).map_err(|e| e.to_string())?;
    let container = read_text(&mut zin, "META-INF/container.xml")?;
    let opf_path = bookconv::wash::tag_attr(&container, "full-path").ok_or("container.xml 里没有 full-path")?.to_string();
    let opf = read_text(&mut zin, &opf_path)?;
    let dir = bookconv::epubzip::dir_of(&opf_path);
    let img_name = format!("eink-cover.{ext}");
    let img_path = if dir.is_empty() { img_name.clone() } else { format!("{dir}/{img_name}") };
    let mime = if ext == "png" { "image/png" } else { "image/jpeg" };
    let item = format!(r#"<item id="eink-cover" href="{img_name}" media-type="{mime}" properties="cover-image"/>"#);
    let meta = r#"<meta name="cover" content="eink-cover"/>"#;
    let mut new_opf = opf.clone();
    let m = new_opf.find("</manifest>").ok_or("OPF 没有 </manifest>")?;
    new_opf.insert_str(m, &item);
    let md = new_opf.find("</metadata>").or_else(|| new_opf.find("</opf:metadata>")).ok_or("OPF 没有 </metadata>")?;
    new_opf.insert_str(md, meta);

    let out = std::fs::File::create(dst).map_err(|e| format!("{}: {e}", dst.display()))?;
    let mut zw = bookconv::zip::ZipWriter::new(out);
    for i in 0..zin.len() {
        let e = zin.by_index_raw(i).map_err(|e| e.to_string())?;
        if e.name() == opf_path {
            drop(e);
            let opts = bookconv::zip::write::SimpleFileOptions::default().compression_method(bookconv::zip::CompressionMethod::Deflated);
            zw.start_file(opf_path.as_str(), opts).map_err(|e| e.to_string())?;
            zw.write_all(new_opf.as_bytes()).map_err(|e| e.to_string())?;
        } else {
            zw.raw_copy_file(e).map_err(|e| e.to_string())?;
        }
    }
    let opts = bookconv::zip::write::SimpleFileOptions::default().compression_method(bookconv::zip::CompressionMethod::Stored);
    zw.start_file(img_path.as_str(), opts).map_err(|e| e.to_string())?;
    zw.write_all(image).map_err(|e| e.to_string())?;
    zw.finish().map_err(|e| e.to_string())?;
    Ok(())
}

fn read_text<R: Read + Seek>(z: &mut bookconv::zip::ZipArchive<R>, name: &str) -> Result<String, String> {
    let b = bookconv::epubzip::read_by_name(z, name)?;
    Ok(String::from_utf8_lossy(&b).into_owned())
}

impl Library {
    /// 给一本书找封面并存进书库。书本来有封面、或者已经找过的跳过（`force` 重找）。
    pub fn fetch_cover(&self, meta: &Meta, force: bool) -> Result<CoverResult, String> {
        if let (Some(c), false) = (&meta.cover, force) {
            return Ok(CoverResult::Existing(c.clone()));
        }
        if self.book_has_own_cover(meta)? {
            return Ok(CoverResult::HasCover);
        }
        let net = Net::new();
        let titles = title_candidates(meta);
        // ① 豆瓣：中文版封面
        if let Some((bytes, ext, url, label)) = douban_cover(&net, &titles, &meta.authors) {
            return self.store_cover(meta, &bytes, ext, url, label).map(CoverResult::Found);
        }
        // ② Wikidata + Open Library：原作封面
        let mut found = None;
        for t in titles {
            if let Some(w) = find_work(&net, &t, &meta.authors)? {
                found = Some(w);
                break;
            }
        }
        let why = match found {
            None => "Wikidata 里找不到书名、作者都对得上的作品".to_string(),
            Some(work) => {
                let name = [&work.en, &work.original, &work.ja].into_iter().find(|n| !n.is_empty()).cloned().unwrap_or_default();
                let label = format!("{} {name}", work.qid);
                for url in cover_urls(&net, &work) {
                    let Ok(bytes) = net.fetch(&url) else { continue };
                    let Some(ext) = plausible_cover(&bytes) else { continue };
                    return self.store_cover(meta, &bytes, ext, url, label).map(CoverResult::Found);
                }
                format!("找到了作品 {label}，但没有可用的封面图")
            }
        };
        // 找不到原作封面：生成一张（书名 + 作者头像）
        let author = meta.authors.first().map(|a| a.trim().to_string()).unwrap_or_default();
        let portrait = author_portrait(&net, &meta.authors);
        let photo = portrait.as_ref().and_then(|(_, url)| net.fetch(url).ok()).filter(|b| image::load_from_memory(b).is_ok());
        let font = crate::covergen::load_font()?;
        let seed = u64::from_str_radix(&meta.id[..12.min(meta.id.len())], 16).unwrap_or(0);
        let bytes = crate::covergen::render(&font, &meta.title, &author, photo.as_deref(), seed)?;
        let (url, work) = match (&portrait, &photo) {
            (Some((who, url)), Some(_)) => (url.clone(), format!("生成（书名 + 作者头像 {who}）")),
            _ => (String::new(), "生成（书名 + 作者名字标，没找到作者头像）".to_string()),
        };
        self.store_cover(meta, &bytes, "jpg", url, work).map(|c| CoverResult::Generated(c, why))
    }

    fn store_cover(&self, meta: &Meta, bytes: &[u8], ext: &str, source_url: String, work: String) -> Result<CoverInfo, String> {
        let info = CoverInfo { file: format!("cover.{ext}"), sha256: sha256_hex(bytes), source_url, work };
        let dir = self.entry_dir(&meta.id);
        crate::fsutil::write_atomic(&dir.join(&info.file), bytes)?;
        let mut m = self.read_meta(&meta.id).ok_or("条目读不出来")?;
        if let Some(old) = m.cover.as_ref().filter(|o| o.file != info.file) {
            let _ = std::fs::remove_file(dir.join(&old.file));
        }
        m.cover = Some(info.clone());
        self.save_meta(&m)?;
        Ok(info)
    }

    /// 去掉找来的封面（找错了的时候）。
    pub fn clear_cover(&self, meta: &Meta) -> Result<bool, String> {
        let mut m = self.read_meta(&meta.id).ok_or("条目读不出来")?;
        let Some(c) = m.cover.take() else { return Ok(false) };
        let _ = std::fs::remove_file(self.entry_dir(&meta.id).join(&c.file));
        self.save_meta(&m)?;
        Ok(true)
    }

    /// 书自己有没有封面（EPUB 看封面声明和第一页的图；要转换的格式转一遍再看；PDF 算有）。
    fn book_has_own_cover(&self, meta: &Meta) -> Result<bool, String> {
        let tmp = tempdir_in(&self.root)?;
        let result = (|| {
            let input = match meta.source() {
                crate::Source::Stored => self.entry_dir(&meta.id).join(&meta.master),
                crate::Source::Original => self.verified_original(meta)?,
            };
            if meta.source_format == "pdf" || meta.master.ends_with(".pdf") {
                return Ok(true);
            }
            let epub = self.epub_input(meta, &input, &tmp)?;
            Ok(epub_has_cover(&epub))
        })();
        let _ = std::fs::remove_dir_all(&tmp);
        result
    }
}

/// 书库里的临时目录（和产物在同一文件系统）。
pub(crate) fn tempdir_in(root: &Path) -> Result<std::path::PathBuf, String> {
    let d = root.join(format!(".tmp-cover-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).map_err(|e| e.to_string())?;
    Ok(d)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_compare_across_width_brackets_and_transliteration() {
        assert_eq!(norm("雪人 (小說)"), "雪人");
        assert_eq!(norm("１３級階梯"), "13級階梯");
        assert_eq!(norm("羅傑．艾克洛命案"), "羅傑艾克洛命案");
        assert!(similarity(&norm("喬治．歐威爾"), &norm("喬治·奧威爾")) >= 0.6);
        assert!(similarity(&norm("13級階梯"), &norm("13級台階")) >= 0.75);
        assert!(similarity(&norm("瘟疫"), &norm("鼠疫")) < 0.75, "不同的书名不能算像");
    }

    #[test]
    fn inject_declares_cover_and_keeps_other_entries() {
        use bookconv::epub::{assemble, Book, BookMeta, Chapter};
        let mut book = Book {
            meta: BookMeta { book_id: "t".into(), title: "书".into(), author: "作者".into(), language: "zh".into(), publisher: "".into(), cover: None, cover_ext: "jpg".into(), cover_media_type: "image/jpeg".into() },
            chapters: vec![Chapter { title: "一".into(), html_body: "<h1>一</h1><p>正文</p>".into(), level: 1 }],
            resources: vec![],
            nav: vec![],
        };
        let d = tempfile::tempdir().unwrap();
        let (src, dst) = (d.path().join("a.epub"), d.path().join("b.epub"));
        std::fs::write(&src, assemble(&mut book).unwrap()).unwrap();
        assert!(!epub_has_cover(&src));
        let mut jpg = Vec::new();
        image::RgbImage::new(400, 600).write_to(&mut std::io::Cursor::new(&mut jpg), image::ImageFormat::Jpeg).unwrap();
        assert_eq!(plausible_cover(&jpg), Some("jpg"));
        inject_cover(&src, &dst, &jpg, "jpg").unwrap();
        assert!(epub_has_cover(&dst));
        let (a, b) = (bookconv::epubzip::read_entries(&std::fs::read(&src).unwrap()).unwrap(), bookconv::epubzip::read_entries(&std::fs::read(&dst).unwrap()).unwrap());
        assert_eq!(b.len(), a.len() + 1);
        for e in a.iter().filter(|e| !e.name.ends_with(".opf")) {
            assert_eq!(b.iter().find(|x| x.name == e.name).unwrap().data, e.data, "{} 原样", e.name);
        }
    }
}
