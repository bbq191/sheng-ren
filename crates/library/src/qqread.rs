//! QQ 阅读（`novel.qq.com`）：网络文学、出版小说的书目源，豆瓣没有的书在这里找（2026-10-08 用户要，例子《疯探》
//! `https://novel.qq.com/detail/1056370737`）。
//!
//! 没有公开 API：用网页自己的搜索接口 `GET /api/search?keywords=…&pageIndex=1&pageSize=10`，回 JSON
//! `{"code":0,"data":{"books":[{id,title,author,intro,categories:[{name}],cover,…}]}}`，不要登录、不要来源页（2026-10-08 实测）。
//! 搜索是按关键词模糊匹配：「书名 作者」常常反而搜不到（「疯探 空城」只回别的书），所以先搜书名、搜不到再带上作者。
//! 条目的详情页是 `https://novel.qq.com/detail/<id>`。

use crate::matching::hit_matches;
use crate::metadata::{tags_without_authors, BookInfo, BookSource};
use crate::net::{enc, Net};

/// 搜索结果里书名、作者对得上的一本。
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Hit {
    pub(crate) id: u64,
    pub(crate) title: String,
    pub(crate) author: String,
    pub(crate) intro: String,
    /// 分类名（「侦探/悬疑/推理」拆开；去掉「出版」这种频道名）。
    pub(crate) categories: Vec<String>,
    pub(crate) cover: String,
}

impl Hit {
    pub(crate) fn url(&self) -> String {
        format!("https://novel.qq.com/detail/{}", self.id)
    }
    pub(crate) fn label(&self) -> String {
        format!("QQ阅读 {} {}", self.title, self.author).trim().to_string()
    }
}

/// 不算标签的分类：频道名，不说明书的内容。
const NOT_TAGS: &[&str] = &["出版", "男生", "女生"];

/// QQ 阅读（[`BookSource`]）：书名、作者、简介、分类、封面都在搜索结果里，不用再取详情页。
pub(crate) struct QqRead;

impl BookSource for QqRead {
    type Hit = Hit;

    /// 先搜书名，搜不到再搜「书名 作者」（关键词模糊匹配，带上作者常常反而搜不到）。
    fn queries(&self, title: &str, authors: &[String]) -> Vec<String> {
        std::iter::once(title.to_string()).chain(crate::matching::author_for_query(authors).map(|a| format!("{title} {a}"))).collect()
    }

    fn search_url(&self, q: &str) -> String {
        format!("https://novel.qq.com/api/search?keywords={}&pageIndex=1&pageSize=10", enc(q))
    }

    /// `code` 不是 0 算出错（被拦等）。
    fn matching_hits(&self, v: &serde_json::Value, nt: &str, want: &[String]) -> Result<Vec<Hit>, String> {
        if v["code"].as_i64() != Some(0) {
            return Err(format!("QQ 阅读回 code {}（{}）", v["code"], v["msg"].as_str().unwrap_or("")));
        }
        Ok(matching_hits(v, nt, want))
    }

    fn cover(&self, net: &Net, hits: &[Hit]) -> Option<(usize, Vec<u8>, &'static str)> {
        first_cover(net, hits)
    }

    fn cover_origin(&self, hit: &Hit) -> (String, String) {
        (hit.cover.clone(), hit.label())
    }

    /// 封面用的那个条目的简介、分类（当标签，去掉作者名）。
    fn info(&self, _net: &Net, hits: &[Hit], chosen: usize, authors: &[String]) -> Option<BookInfo> {
        let h = hits.get(chosen)?;
        let subjects = tags_without_authors(h.categories.iter(), authors.iter());
        Some(BookInfo { source: format!("QQ阅读 {}", h.url()), description: h.intro.clone(), subjects, ..Default::default() })
    }
}

/// 搜索结果里书名（简体化后）相同、或只差卷次后缀（≤2 字），且作者对得上（书里没写作者时书名要完全相同）的，保持网站给的顺序。
fn matching_hits(v: &serde_json::Value, nt: &str, want: &[String]) -> Vec<Hit> {
    let mut out = Vec::new();
    for b in v["data"]["books"].as_array().into_iter().flatten() {
        let s = |k: &str| b[k].as_str().unwrap_or("").trim().to_string();
        let (title, author) = (s("title"), s("author"));
        let Some(id) = b["id"].as_u64() else { continue };
        if !hit_matches(&title, &author, nt, want) {
            continue;
        }
        let mut categories: Vec<String> = Vec::new();
        for c in b["categories"].as_array().into_iter().flatten() {
            for name in c["name"].as_str().unwrap_or("").split('/').map(str::trim).filter(|n| !n.is_empty() && !NOT_TAGS.contains(n)) {
                if !categories.iter().any(|x| x == name) {
                    categories.push(name.to_string());
                }
            }
        }
        // 封面网址有的是 http：图片服务器两种都认，统一用 https
        let cover = s("cover");
        let cover = cover.strip_prefix("http://").map_or(cover.clone(), |r| format!("https://{r}"));
        out.push(Hit { id, title, author, intro: s("intro"), categories, cover });
    }
    out
}

/// 头几个对得上的条目里第一张像封面的图。返回（条目下标, 图片, 扩展名）。
fn first_cover(net: &Net, hits: &[Hit]) -> Option<(usize, Vec<u8>, &'static str)> {
    hits.iter().enumerate().take(3).filter(|(_, h)| !h.cover.is_empty()).find_map(|(i, h)| {
        let bytes = net.fetch(&h.cover).ok()?;
        crate::cover::plausible_cover(&bytes).map(|ext| (i, bytes, ext))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::matching::{norm_author, norm_s};

    #[test]
    fn hits_need_title_and_author_and_categories_become_tags() {
        let v: serde_json::Value = serde_json::from_str(
            r#"{"code":0,"data":{"books":[
                {"id":1059362647,"title":"从大学球探开始","author":"肉末大茄子","intro":"x","categories":[]},
                {"id":1056370737,"title":"疯探","author":"空城","intro":"这座城市病了。","cover":"http://ccstatic-1252317822.file.myqcloud.com/a.JPEG",
                 "categories":[{"name":"出版"},{"name":"小说"},{"name":"侦探/悬疑/推理"}]},
                {"id":3,"title":"疯探","author":"别人","intro":"","categories":[]}
            ]}}"#,
        )
        .unwrap();
        let hits = matching_hits(&v, &norm_s("疯探"), &[norm_author("空城")]);
        assert_eq!(hits.len(), 1, "{hits:?}");
        let h = &hits[0];
        assert_eq!((h.id, h.intro.as_str()), (1056370737, "这座城市病了。"));
        assert_eq!(h.categories, ["小说", "侦探", "悬疑", "推理"], "频道名「出版」不算，「侦探/悬疑/推理」拆开");
        assert_eq!(h.cover, "https://ccstatic-1252317822.file.myqcloud.com/a.JPEG");
        assert_eq!(h.url(), "https://novel.qq.com/detail/1056370737");
        assert!(matching_hits(&v, &norm_s("疯探"), &[]).len() == 2, "没写作者时书名完全相同就算");
        assert!(matching_hits(&serde_json::json!({"code":0,"data":null}), "疯探", &[]).is_empty());
    }
}
