//! 豆瓣：按书名找条目（中文版，最贴近用户手上的书），取封面和条目页上的元数据。
//!
//! 豆瓣没有公开 API：找条目用网页的搜索建议接口，元数据从条目页的 HTML 里取（`#info` 信息栏、`#link-report` 内容简介、
//! 页面里的标签串）。请求要少（全程节流）；图片服务器要带来源页。页面改版了取不到就当没找到，交给下一个源。

use crate::metadata::{tags_without_authors, BookInfo, BookSource, Edition};
use crate::net::{enc, Net};
use bookconv::html::plain_text;

const REFERER: &str = "https://book.douban.com/";

/// 搜索建议里对得上的条目。
#[derive(Clone, Debug)]
pub(crate) struct Hit {
    pub(crate) id: String,
    pub(crate) title: String,
    pub(crate) author: String,
    pub(crate) year: String,
    /// 封面大图（`/l/`）网址。
    pub(crate) pic: String,
}

impl Hit {
    pub(crate) fn label(&self) -> String {
        format!("豆瓣 {} {} {}", self.title, self.author, self.year).trim().to_string()
    }
    pub(crate) fn url(&self) -> String {
        format!("https://book.douban.com/subject/{}/", self.id)
    }
}

/// 豆瓣（[`BookSource`]）：封面挑前几个条目里分辨率最高的，元数据取条目页（封面那个条目先试，再试一个）。
pub(crate) struct Douban;

impl BookSource for Douban {
    type Hit = Hit;

    /// 搜「书名 作者」，搜不到再只搜书名（书里没写作者时只搜书名）。书名同时是人名的（《张居正》）光搜书名回空列表，
    /// 带上作者才有书（2026-10-08 实测）。
    fn queries(&self, title: &str, authors: &[String]) -> Vec<String> {
        crate::matching::author_for_query(authors).map(|a| format!("{title} {a}")).into_iter().chain(std::iter::once(title.to_string())).collect()
    }

    fn search_url(&self, q: &str) -> String {
        format!("https://book.douban.com/j/subject_suggest?q={}", enc(q))
    }

    fn matching_hits(&self, v: &serde_json::Value, nt: &str, want: &[String]) -> Result<Vec<Hit>, String> {
        Ok(matching_hits(v, nt, want))
    }

    /// 书籍搜索页 `search.douban.com/book/subject_search`：搜索建议不收漫画（「哆啦」回空列表），这里收（2026-10-12 实测）。
    /// 结果在页面里的 `window.__DATA__`（明文 JSON：`items[]` 的 `title`、`abstract`「作者 / 译者 / 出版社 / 日期 / 价格」、`id`、
    /// `cover_url`，`tpl_name` 为 `search_subject` 的才是书）。
    fn fallback_search(&self, net: &Net, title: &str, nt: &str, want: &[String]) -> Vec<Hit> {
        let url = format!("https://search.douban.com/book/subject_search?search_text={}&cat=1001", enc(title));
        let html = match net.fetch_ref(&url, Some(REFERER)) {
            Ok(b) => String::from_utf8_lossy(&b).into_owned(),
            Err(_) => return Vec::new(),
        };
        match search_page_items(&html) {
            Some(v) => matching_hits(&v, nt, want),
            None => {
                net.note_transient(&format!("{url}：页面里没有 window.__DATA__（可能被拦或改版了）"));
                Vec::new()
            }
        }
    }

    fn cover(&self, net: &Net, hits: &[Hit]) -> Option<(usize, Vec<u8>, &'static str)> {
        best_cover(net, hits)
    }

    fn hit_author<'h>(&self, hit: &'h Hit) -> &'h str {
        &hit.author
    }

    fn cover_origin(&self, hit: &Hit) -> (String, String) {
        (hit.pic.clone(), hit.label())
    }

    fn info(&self, net: &Net, hits: &[Hit], chosen: usize, authors: &[String], translators: &[String]) -> Option<BookInfo> {
        // 书里写了译者：挑译者对得上的那个版本（《飘》豆瓣第一个条目是李美华译本，书是梅静译本），最多看 5 个；都对不上用封面那个，
        // 它的出版社、ISBN 和书冲突，写进书时不用（`metadata::plan`）
        let order: Vec<usize> = std::iter::once(chosen).chain((0..hits.len()).filter(|&i| i != chosen)).filter(|&i| i < hits.len()).collect();
        if !translators.is_empty() {
            let mut first: Option<BookInfo> = None;
            for &i in order.iter().take(5) {
                let Ok(s) = subject(net, &hits[i].id) else { continue };
                let same = crate::matching::same_person_any(&s.translators, translators);
                let info = info_of(&hits[i], s, authors);
                if same {
                    return Some(info);
                }
                first.get_or_insert(info);
            }
            return first;
        }
        for &i in order.iter().take(2) {
            if let Ok(s) = subject(net, &hits[i].id) {
                return Some(info_of(&hits[i], s, authors));
            }
        }
        None
    }
}

/// 条目页 → 元数据：作品层面的（简介、标签、原作名）和这个版本的（只供参考，不写进书）。
fn info_of(hit: &Hit, s: Subject, authors: &[String]) -> BookInfo {
    // 标签里去掉纯数字（年份）和作者名（豆瓣标签常带作者）
    let subjects = tags_without_authors(s.tags.iter().filter(|t| !t.chars().all(|c| c.is_ascii_digit())), authors.iter().chain(s.authors.iter()));
    BookInfo {
        source: format!("豆瓣 {}", hit.url()),
        title: s.title.clone(),
        // 带国籍的作者署名（搜索建议的「[美] 玛格丽特·米切尔」，条目页的同样写法）：书里的作者没写国籍时补上
        authors: if s.authors.is_empty() { hit.author.split(['/', '、']).map(|a| a.trim().to_string()).filter(|a| !a.is_empty()).collect() } else { s.authors.clone() },
        original_title: s.original_title,
        first_published: String::new(),
        description: s.description,
        subjects,
        edition: Some(Edition {
            title: if s.subtitle.is_empty() { s.title } else { format!("{}：{}", s.title, s.subtitle) },
            authors: s.authors,
            translators: s.translators,
            publisher: s.publisher,
            pubdate: s.pubdate,
            isbn: s.isbn,
        }),
    }
}

/// 搜索建议返回的条目里书名（简体化后）相同、或只差卷次后缀（≤2 字），且作者对得上的条目（没有作者信息时书名要完全相同），
/// 保持豆瓣给的顺序。
fn matching_hits(v: &serde_json::Value, nt: &str, want_authors: &[String]) -> Vec<Hit> {
    let mut hits = Vec::new();
    for x in v.as_array().into_iter().flatten() {
        let g = |k: &str| x[k].as_str().unwrap_or("").to_string();
        let (title, author, pic, id) = (g("title"), g("author_name"), g("pic"), g("id"));
        if id.is_empty() || x["type"].as_str().is_some_and(|ty| ty != "b") {
            continue;
        }
        if crate::matching::hit_matches(&title, &author, nt, want_authors) {
            let pic = pic.replace("/view/subject/s/", "/view/subject/l/").replace("/view/subject/m/", "/view/subject/l/");
            hits.push(Hit { id, title, author, year: g("year"), pic });
        }
    }
    hits
}

/// 书籍搜索页的 `window.__DATA__` → 搜索建议同样的条目列表（`title`、`author_name`、`id`、`pic`、`type`），交给 [`matching_hits`]。
/// 页面里没有这段数据返回 `None`。
fn search_page_items(html: &str) -> Option<serde_json::Value> {
    let start = html.find("window.__DATA__")?;
    let rest = &html[start..];
    let brace = rest.find('{')?;
    // 一个 JSON 值：用流式解析读到它结束为止（后面是 `;` 和别的脚本）
    let mut de = serde_json::Deserializer::from_str(&rest[brace..]).into_iter::<serde_json::Value>();
    let data = de.next()?.ok()?;
    let items = data["items"].as_array()?;
    Some(serde_json::Value::Array(
        items
            .iter()
            .filter(|it| it["tpl_name"].as_str() == Some("search_subject"))
            .map(|it| {
                // 「作者 / 作者 / 译者 / 出版社 / 日期 / 价格」分不清哪几段是作者：整串当署名（比作者时出版社、译者对不上作者，不碍事；
                // 几位作者都要在里面，《杀死一只知更鸟》图像小说靠第二位作者和小说分开）
                let author = it["abstract"].as_str().unwrap_or("").trim().to_string();
                serde_json::json!({
                    "title": it["title"].as_str().unwrap_or(""),
                    "author_name": author,
                    "id": it["id"].as_i64().map(|i| i.to_string()).or_else(|| it["id"].as_str().map(str::to_string)).unwrap_or_default(),
                    "pic": it["cover_url"].as_str().unwrap_or(""),
                    "year": "",
                    "type": "b",
                })
            })
            .collect(),
    ))
}

/// 前几个条目里挑分辨率最高、像封面的封面图（老条目只有 200 多像素宽的小图）。返回（条目下标, 图片, 扩展名）。
fn best_cover(net: &Net, hits: &[Hit]) -> Option<(usize, Vec<u8>, &'static str)> {
    let mut best: Option<(u64, usize, Vec<u8>, &'static str)> = None;
    for (i, h) in hits.iter().enumerate().take(4).filter(|(_, h)| !h.pic.is_empty()) {
        let Ok(bytes) = net.fetch_ref(&h.pic, Some(REFERER)) else { continue };
        let Some(ext) = crate::cover::plausible_cover(&bytes) else { continue };
        let area = image::ImageReader::new(std::io::Cursor::new(&bytes)).with_guessed_format().ok().and_then(|r| r.into_dimensions().ok()).map_or(0, |(w, h)| w as u64 * h as u64);
        if best.as_ref().is_none_or(|b| area > b.0) {
            best = Some((area, i, bytes, ext));
        }
        if area >= 600 * 900 {
            break;
        }
    }
    best.map(|(_, i, b, e)| (i, b, e))
}

/// 条目页上的元数据。
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Subject {
    pub(crate) title: String,
    pub(crate) subtitle: String,
    pub(crate) original_title: String,
    pub(crate) authors: Vec<String>,
    pub(crate) translators: Vec<String>,
    pub(crate) publisher: String,
    pub(crate) pubdate: String,
    pub(crate) isbn: String,
    /// 内容简介，段落之间用换行分开。
    pub(crate) description: String,
    pub(crate) tags: Vec<String>,
}

fn subject(net: &Net, id: &str) -> Result<Subject, String> {
    let url = format!("https://book.douban.com/subject/{}/", enc(id));
    let html = String::from_utf8_lossy(&net.fetch_ref(&url, Some(REFERER))?).into_owned();
    let s = parse_subject(&html);
    if s.title.is_empty() && s.description.is_empty() {
        return Err(format!("{url}：页面里没认出书目信息（可能要登录或页面改版了）"));
    }
    Ok(s)
}

/// 从 `from` 开始的第一个 `<div …>` 到与它配对的 `</div>`（按嵌套计数）之间的内容。
fn div_inner(html: &str, from: usize) -> Option<&str> {
    let start = from + html[from..].find('>')? + 1;
    let mut depth = 1;
    let mut i = start;
    while depth > 0 {
        let open = html[i..].find("<div").map(|p| p + i);
        let close = html[i..].find("</div").map(|p| p + i)?;
        match open {
            Some(o) if o < close => {
                depth += 1;
                i = o + 4;
            }
            _ => {
                depth -= 1;
                i = close + 5;
            }
        }
    }
    Some(&html[start..i - 5])
}

/// 多段 HTML → 纯文本，段落之间用换行。
fn paragraphs(html: &str) -> String {
    html.split("</p>").map(plain_text).filter(|p| !p.is_empty()).collect::<Vec<_>>().join("\n")
}

fn split_names(v: &str) -> Vec<String> {
    v.split(" / ").map(str::trim).filter(|x| !x.is_empty()).map(str::to_string).collect()
}

fn parse_subject(html: &str) -> Subject {
    let mut s = Subject::default();
    // 标签写坏了（属性后面直接是 `</span>`、没有 `>`）时 `>` 在 `</span>` 里面，起点会越过终点：用 `get` 取，取不到就算没有
    if let Some(i) = html.find("property=\"v:itemreviewed\"") {
        if let (Some(e), Some(g)) = (html[i..].find("</span>"), html[i..].find('>')) {
            s.title = html.get(i + g + 1..i + e).map(plain_text).unwrap_or_default();
        }
    }
    // 信息栏：一项一行（`<br/>` 分开），"作者: A / B"
    if let Some(inner) = html.find("id=\"info\"").and_then(|i| div_inner(html, i)) {
        for line in inner.split("<br") {
            let line = plain_text(line.trim_start_matches(['/', ' ']).trim_start_matches('>'));
            let Some((k, v)) = line.split_once([':', '：']) else { continue };
            let v = v.trim().to_string();
            match k.trim() {
                "作者" => s.authors = split_names(&v),
                "译者" => s.translators = split_names(&v),
                "出版社" => s.publisher = v,
                "出版年" => s.pubdate = v,
                "ISBN" => s.isbn = v,
                "原作名" => s.original_title = v,
                "副标题" => s.subtitle = v,
                _ => {}
            }
        }
    }
    // 内容简介：`#link-report` 里（到"作者简介"之前）最长的那个 `.intro`（有折叠时完整版是后一个）
    if let Some(i) = html.find("id=\"link-report\"") {
        let end = html[i..].find("作者简介").map_or(html.len(), |e| i + e);
        let region = &html[i..end];
        let mut best = String::new();
        let mut at = 0;
        while let Some(p) = region[at..].find("class=\"intro\"") {
            let p = at + p;
            if let Some(inner) = div_inner(region, p) {
                let text = paragraphs(inner);
                if text.chars().count() > best.chars().count() {
                    best = text;
                }
            }
            at = p + 1;
        }
        s.description = best.trim_end_matches("(展开全部)").trim().to_string();
    }
    // 标签：页面脚本里的 `criteria = '7:标签|7:标签|…|3:/subject/…'`（常用标签在页面上是异步加载的）
    if let Some(i) = html.find("criteria = '") {
        let rest = &html[i + "criteria = '".len()..];
        // 是 JS 字符串字面量：用户标签里的 `:` 写成 `\:`（《古龙作品全集》「古龙作品全集\:全53册(精装)」），按 JS 去掉转义
        let mut raw = String::new();
        let mut cs = rest.chars();
        while let Some(c) = cs.next() {
            match c {
                '\'' => break,
                '\\' => raw.extend(cs.next()),
                _ => raw.push(c),
            }
        }
        s.tags = raw.split('|').filter_map(|x| x.strip_prefix("7:")).map(str::trim).filter(|x| !x.is_empty()).map(str::to_string).collect();
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::matching::{norm_author, norm_s};

    #[test]
    fn parses_subject_page() {
        let html = r#"<h1><span property="v:itemreviewed">白夜行</span></h1>
<div id="info"><span><span class="pl"> 作者</span>:
  <a href="/author/1">[日] 东野圭吾</a></span><br/>
<span><span class="pl"> 译者</span>: <a>刘姿君</a> / <a>某某</a></span><br/>
<span class="pl">出版社:</span> <a>南海出版公司</a><br/>
<span class="pl">出版年:</span> 2013-1-1<br/>
<span class="pl">原作名:</span> 白夜行<br/>
<span class="pl">ISBN:</span> 9787544258609<br/>
<div class="x">嵌套</div></div>
<div class="indent" id="link-report"><span class="short"><div class="intro"><p>短的……</p></div></span>
<span class="all hidden"><div class="intro"><p>第一段 &amp; 引号</p><p>第二段</p></div></span></div>
<span>作者简介</span><div class="intro"><p>作者的简介不要</p></div>
<script>criteria = '7:东野圭吾|7:推理|7:日本文学|7:全集\:全53册\'精装|3:/subject/10554308/',</script>"#;
        let s = parse_subject(html);
        assert_eq!(s.title, "白夜行");
        assert_eq!(s.authors, ["[日] 东野圭吾"]);
        assert_eq!(s.translators, ["刘姿君", "某某"]);
        assert_eq!(s.publisher, "南海出版公司");
        assert_eq!(s.pubdate, "2013-1-1");
        assert_eq!(s.isbn, "9787544258609");
        assert_eq!(s.original_title, "白夜行");
        assert_eq!(s.description, "第一段 & 引号\n第二段");
        assert_eq!(s.tags, ["东野圭吾", "推理", "日本文学", "全集:全53册'精装"]);
    }

    #[test]
    fn hits_need_title_and_author() {
        let v: serde_json::Value = serde_json::from_str(r#"[
            {"title":"熊召政","type":"a","id":"112478"},
            {"title":"张居正","author_name":"熊召政","year":"2022","type":"b","id":"34955795","pic":"https://img2.doubanio.com/view/subject/s/public/s1.jpg"},
            {"title":"张居正・木兰歌","author_name":"熊召政","type":"b","id":"1034733"},
            {"title":"张居正","author_name":"朱东润","type":"b","id":"1"}
        ]"#).unwrap();
        let hits = matching_hits(&v, &norm_s("张居正"), &[norm_author("熊召政")]);
        assert_eq!(hits.iter().map(|h| h.id.as_str()).collect::<Vec<_>>(), ["34955795"], "作者条目、书名多出一截的、作者对不上的都不要");
        assert!(hits[0].pic.contains("/view/subject/l/"), "封面换大图");
    }

    #[test]
    fn malformed_pages_do_not_panic() {
        // 书名标签没有 `>`：以前切片起点越过终点 panic
        assert_eq!(parse_subject(r#"<span property="v:itemreviewed"</span>"#).title, "");
        for html in [r#"<div id="info""#, r#"<div id="link-report"><div class="intro">"#, "criteria = '7:a", ""] {
            let _ = parse_subject(html);
        }
    }

    /// 书里有两位作者：同名的小说、图像小说条目里只留两位都对得上的（《杀死一只知更鸟》漫画版，2026-10-12）。
    #[test]
    fn two_author_books_pick_entries_with_both_authors() {
        let hit = |id: &str, author: &str| Hit { id: id.into(), title: "杀死一只知更鸟".into(), author: author.into(), year: String::new(), pic: String::new() };
        let hits = vec![hit("26879778", "[美] 哈珀·李 / 李育超 / 译林出版社 / 2017-2"), hit("35016096", "[美] 哈珀·李 / [英] 弗雷德·福德姆 / 刘勇军 / 上海文艺出版社 / 2020-5")];
        let want = crate::matching::author_keys(&["哈珀· 李,弗雷德·福德姆".into()]);
        assert_eq!(Douban.most_authors(hits.clone(), &want).iter().map(|h| h.id.as_str()).collect::<Vec<_>>(), ["35016096"]);
        let one = crate::matching::author_keys(&["哈珀· 李".into()]);
        assert_eq!(Douban.most_authors(hits, &one).len(), 2, "一位作者的书不筛");
    }
}
