//! Bangumi（`bgm.tv`）：漫画的书目源（2026-10-12 用户定：漫画也要补元数据；豆瓣的搜索建议不收漫画，Bangumi 漫画条目全）。
//!
//! 正式公开的 API，不要账号、不要 key，要求带说明来历的 User-Agent（[`crate::net::UA`]）：
//! `POST https://api.bgm.tv/v0/search/subjects?limit=10`，请求体 `{"keyword": …, "filter": {"type": [1]}}`（1＝书籍），回
//! `{"data":[{id, name, name_cn, summary, platform（「漫画」「小说」…）, tags:[{name, count}], infobox:[{key, value}], images:{large}}], "total"}`。
//! 作者在 `infobox` 的「作者」，漫画多写「原作」「作画」（字符串，或 `[{v}]` 列表），几栏合起来当署名。条目是作品（整套漫画），没有某一卷的出版社、ISBN，只取作品层面的：
//! 简介、标签、原名（日文名）、作者。条目页是 `https://bgm.tv/subject/<id>`。只给漫画查（[`BookSource::applies`]）。

use crate::matching::{hit_matches, norm_s};
use crate::metadata::{tags_without_authors, BookInfo, BookSource, Lookup};
use crate::net::Net;

/// 搜索结果里书名、作者对得上的一部作品。
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Hit {
    pub(crate) id: u64,
    /// 中文名（没有就是原名）。
    pub(crate) title: String,
    /// 原名（日文名等）；和中文名一样时空着。
    pub(crate) original: String,
    pub(crate) author: String,
    pub(crate) summary: String,
    pub(crate) tags: Vec<String>,
    pub(crate) cover: String,
}

impl Hit {
    pub(crate) fn url(&self) -> String {
        format!("https://bgm.tv/subject/{}", self.id)
    }
    pub(crate) fn label(&self) -> String {
        format!("Bangumi {} {}", self.title, self.author).trim().to_string()
    }
}

/// 不算标签的：说的是载体不是内容，或者是用户自己的阅读状态（「没看完」）。
const NOT_TAGS: &[&str] = &["漫画", "漫畫", "单行本", "單行本", "未收录", "漫画系列", "没看完", "想看", "在看", "看过", "看完", "未读", "已读", "弃"];

/// Bangumi（[`BookSource`]）：书名、作者、简介、标签、封面都在搜索结果里，不用再取条目页。
pub(crate) struct Bangumi;

impl BookSource for Bangumi {
    type Hit = Hit;

    /// 只搜书名（关键词搜索，带上作者反而搜不到），繁体书名先搜简体（中文名多是简体：「哆啦A夢」搜不出整部作品
    /// 《哆啦A梦》，只有单卷）；搜不到时 [`BookSource::search`] 再用去掉卷次的书名搜。
    fn queries(&self, title: &str, _authors: &[String]) -> Vec<String> {
        let simp = fast2s::convert(title);
        if simp == title { vec![title.to_string()] } else { vec![simp, title.to_string()] }
    }

    fn search_url(&self, _q: &str) -> String {
        "https://api.bgm.tv/v0/search/subjects?limit=10".to_string()
    }

    fn fetch_hits(&self, net: &Net, q: &str) -> Result<serde_json::Value, String> {
        net.post_json(&self.search_url(q), &serde_json::json!({ "keyword": q, "filter": { "type": [1] } }))
    }

    fn applies(&self, l: &Lookup<'_>) -> bool {
        l.comic
    }

    /// 回执里没有 `data` 列表算出错。
    fn matching_hits(&self, v: &serde_json::Value, nt: &str, want: &[String]) -> Result<Vec<Hit>, String> {
        if !v["data"].is_array() {
            return Err(format!("Bangumi 回执里没有结果列表（{}）", v["description"].as_str().or(v["title"].as_str()).unwrap_or("")));
        }
        Ok(matching_hits(v, nt, want))
    }

    fn cover(&self, net: &Net, hits: &[Hit]) -> Option<(usize, Vec<u8>, &'static str)> {
        hits.iter().enumerate().take(3).filter(|(_, h)| !h.cover.is_empty()).find_map(|(i, h)| {
            let bytes = net.fetch(&h.cover).ok()?;
            crate::cover::plausible_cover(&bytes).map(|ext| (i, bytes, ext))
        })
    }

    fn hit_author<'h>(&self, hit: &'h Hit) -> &'h str {
        &hit.author
    }

    fn cover_origin(&self, hit: &Hit) -> (String, String) {
        (hit.cover.clone(), hit.label())
    }

    /// 封面用的那个条目（没找封面就是第一个）：简介、标签（去掉作者名、载体词）、原名、作者。
    fn info(&self, _net: &Net, hits: &[Hit], chosen: usize, authors: &[String], _translators: &[String]) -> Option<BookInfo> {
        let h = hits.get(chosen)?;
        let author_list = [h.author.clone()];
        let subjects = tags_without_authors(h.tags.iter(), authors.iter().chain(author_list.iter()));
        Some(BookInfo {
            source: format!("Bangumi {}", h.url()),
            title: h.title.clone(),
            authors: h.author.split(['、', '/']).map(|a| a.trim().to_string()).filter(|a| !a.is_empty()).collect(),
            original_title: h.original.clone(),
            description: h.summary.trim().replace("\r\n", "\n"),
            subjects,
            ..Default::default()
        })
    }
}

/// `infobox` 里一项的文字：字符串，或 `[{v: …}]` 列表（几个值用顿号连起来）。
fn infobox(item: &serde_json::Value, key: &str) -> String {
    let Some(v) = item["infobox"].as_array().and_then(|a| a.iter().find(|x| x["key"].as_str() == Some(key))).map(|x| &x["value"]) else { return String::new() };
    match v {
        serde_json::Value::String(s) => s.trim().to_string(),
        serde_json::Value::Array(a) => a.iter().filter_map(|x| x["v"].as_str()).map(str::trim).filter(|s| !s.is_empty()).collect::<Vec<_>>().join("、"),
        _ => String::new(),
    }
}

/// 中文名或原名和书名对得上、作者对得上（书里没写作者时书名要完全相同）的条目；漫画（`platform`「漫画」）排在前面，
/// 同类里书名正好对上的排在前面，其余保持网站给的顺序。
fn matching_hits(v: &serde_json::Value, nt: &str, want: &[String]) -> Vec<Hit> {
    let mut comics = Vec::new();
    let mut rest = Vec::new();
    for x in v["data"].as_array().into_iter().flatten() {
        let Some(id) = x["id"].as_u64() else { continue };
        let s = |k: &str| x[k].as_str().unwrap_or("").trim().to_string();
        let (name, name_cn) = (s("name"), s("name_cn"));
        // 漫画多半不写「作者」，写「原作」「作画」（《死亡笔记》原作大場つぐみ、作画小畑健）：几栏合在一起当署名
        let author = ["作者", "原作", "作画", "脚本"].iter().map(|k| infobox(x, k)).filter(|a| !a.is_empty()).collect::<Vec<_>>().join("、");
        if ![&name_cn, &name].iter().any(|t| !t.is_empty() && hit_matches(t, &author, nt, want)) {
            continue;
        }
        let tags = x["tags"].as_array().into_iter().flatten().filter_map(|t| t["name"].as_str()).map(str::trim).filter(|t| !t.is_empty() && !NOT_TAGS.contains(t)).map(str::to_string).collect();
        let title = if name_cn.is_empty() { name.clone() } else { name_cn.clone() };
        let original = if name_cn.is_empty() || name == name_cn { String::new() } else { name };
        let hit = Hit { id, title, original, author, summary: s("summary"), tags, cover: x["images"]["large"].as_str().unwrap_or("").to_string() };
        // 书名正好对上的（整部作品「哆啦A梦」）排在靠去掉卷次才对上的（单卷「ドラえもん 1」）前面
        let exact = [&hit.title, &hit.original].iter().any(|t| norm_s(t) == nt);
        if x["platform"].as_str() == Some("漫画") {
            comics.push((!exact, hit));
        } else {
            rest.push((!exact, hit));
        }
    }
    comics.sort_by_key(|(inexact, _)| *inexact);
    rest.sort_by_key(|(inexact, _)| *inexact);
    comics.into_iter().chain(rest).map(|(_, h)| h).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::matching::author_keys;

    /// 2026-10-12 实测的回执（删减）：中文名、原名、作者（infobox 的字符串和列表两种写法）、平台、标签。
    #[test]
    fn hits_match_by_chinese_or_original_name_and_author() {
        let v = serde_json::json!({"total": 3, "data": [
            {"id": 50316, "name": "ドラえもん", "name_cn": "哆啦a梦（松本大洋版）", "platform": "漫画", "summary": "短篇", "tags": [], "infobox": [], "images": {}},
            {"id": 111013, "name": "ドラえもん 1", "name_cn": "哆啦A梦 1", "platform": "漫画", "summary": "单卷", "tags": [], "infobox": [{"key": "作者", "value": "藤子・F・不二雄"}], "images": {}},
            {"id": 1, "name": "ドラえもん", "name_cn": "哆啦A梦", "platform": "小说", "summary": "小说版", "tags": [], "infobox": [{"key": "作者", "value": "藤子・F・不二雄"}], "images": {}},
            {"id": 27684, "name": "ドラえもん", "name_cn": "哆啦A梦", "platform": "漫画", "summary": "故事从一个小学4年级学生野比大雄开始。", "tags": [{"name": "漫画", "count": 9}, {"name": "藤子・F・不二雄", "count": 8}, {"name": "科幻", "count": 3}], "infobox": [{"key": "作者", "value": [{"v": "藤子・F・不二雄"}]}], "images": {"large": "https://lain.bgm.tv/pic/cover/l/x.jpg"}}
        ]});
        let hits = Bangumi.matching_hits(&v, &norm_s("哆啦A夢"), &author_keys(&["藤子·F·不二雄".into()])).unwrap();
        assert_eq!(hits.iter().map(|h| h.id).collect::<Vec<_>>(), [27684, 111013, 1], "作者对不上的不要；漫画排前面，书名正好对上的整部作品排在单卷前面");
        let info = Bangumi.info(&Net::new(), &hits, 0, &["藤子·F·不二雄".into()], &[]).unwrap();
        assert_eq!((info.original_title.as_str(), info.subjects.clone()), ("ドラえもん", vec!["科幻".to_string()]), "标签去掉载体词和作者");
        assert!(info.source.ends_with("bgm.tv/subject/27684"));
        assert!(Bangumi.matching_hits(&serde_json::json!({"title": "Bad Request"}), "x", &[]).is_err());
    }

    /// 漫画条目写「原作」「作画」不写「作者」（《死亡笔记》，2026-10-12）：照样认得出作者。
    #[test]
    fn manga_credits_from_original_and_art() {
        let v = serde_json::json!({"data": [{"id": 1674, "name": "DEATH NOTE", "name_cn": "死亡笔记", "platform": "漫画", "summary": "", "tags": [], "images": {},
            "infobox": [{"key": "原作", "value": "大場つぐみ"}, {"key": "作画", "value": "小畑健"}]}]});
        let hits = Bangumi.matching_hits(&v, &norm_s("死亡筆記"), &author_keys(&["大場鶇,小畑健".into()])).unwrap();
        assert_eq!(hits.iter().map(|h| h.id).collect::<Vec<_>>(), [1674]);
        assert_eq!(hits[0].author, "大場つぐみ、小畑健");
    }
}
