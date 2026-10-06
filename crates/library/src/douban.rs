//! 豆瓣：按书名找条目（中文版，最贴近用户手上的书），取封面和条目页上的元数据。
//!
//! 豆瓣没有公开 API：找条目用网页的搜索建议接口，元数据从条目页的 HTML 里取（`#info` 信息栏、`#link-report` 内容简介、
//! 页面里的标签串）。请求要少（全程节流）；图片服务器要带来源页。页面改版了取不到就当没找到，交给下一个源。

use crate::matching::{norm_author, norm_s, similarity};
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

/// 按书名找：书名（简体化后）相同、或只差卷次后缀（≤2 字），且作者对得上的条目（没有作者信息时书名要完全相同）。
/// 依次试各个书名，第一个有结果的为准，保持豆瓣给的顺序。
pub(crate) fn search(net: &Net, titles: &[String], authors: &[String]) -> Vec<Hit> {
    let want_authors: Vec<String> = authors.iter().map(|a| norm_author(a)).filter(|a| !a.is_empty()).collect();
    for t in titles {
        let nt = norm_s(t);
        if nt.is_empty() {
            continue;
        }
        // 出错（被拦、回来的不是 JSON、网络问题）已记成临时错误（`Net::transient_error`），调用方不会当成"没这本书"；
        // 别的书名也不用再试了
        let Ok(v) = net.json_strict(&format!("https://book.douban.com/j/subject_suggest?q={}", enc(t))) else { break };
        let mut hits = Vec::new();
        for x in v.as_array().into_iter().flatten() {
            let g = |k: &str| x[k].as_str().unwrap_or("").to_string();
            let (title, author, pic, id) = (g("title"), g("author_name"), g("pic"), g("id"));
            if id.is_empty() || x["type"].as_str().is_some_and(|ty| ty != "b") {
                continue;
            }
            let dt = norm_s(&title);
            let title_ok = dt == nt || ((nt.starts_with(&dt) || dt.starts_with(&nt)) && nt.chars().count().abs_diff(dt.chars().count()) <= 2);
            let author_ok = if want_authors.is_empty() {
                dt == nt
            } else {
                author.split(['/', '、', ',', '，']).any(|a| want_authors.iter().any(|w| similarity(w, &norm_author(a)) >= 0.6))
            };
            if title_ok && author_ok {
                let pic = pic.replace("/view/subject/s/", "/view/subject/l/").replace("/view/subject/m/", "/view/subject/l/");
                hits.push(Hit { id, title, author, year: g("year"), pic });
            }
        }
        if !hits.is_empty() {
            return hits;
        }
    }
    Vec::new()
}

/// 前几个条目里挑分辨率最高、像封面的封面图（老条目只有 200 多像素宽的小图）。返回（条目下标, 图片, 扩展名）。
pub(crate) fn best_cover(net: &Net, hits: &[Hit]) -> Option<(usize, Vec<u8>, &'static str)> {
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

pub(crate) fn subject(net: &Net, id: &str) -> Result<Subject, String> {
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
        if let Some(e) = rest.find('\'') {
            s.tags = rest[..e].split('|').filter_map(|x| x.strip_prefix("7:")).map(str::trim).filter(|x| !x.is_empty()).map(str::to_string).collect();
        }
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

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
<script>criteria = '7:东野圭吾|7:推理|7:日本文学|3:/subject/10554308/',</script>"#;
        let s = parse_subject(html);
        assert_eq!(s.title, "白夜行");
        assert_eq!(s.authors, ["[日] 东野圭吾"]);
        assert_eq!(s.translators, ["刘姿君", "某某"]);
        assert_eq!(s.publisher, "南海出版公司");
        assert_eq!(s.pubdate, "2013-1-1");
        assert_eq!(s.isbn, "9787544258609");
        assert_eq!(s.original_title, "白夜行");
        assert_eq!(s.description, "第一段 & 引号\n第二段");
        assert_eq!(s.tags, ["东野圭吾", "推理", "日本文学"]);
    }

    #[test]
    fn malformed_pages_do_not_panic() {
        // 书名标签没有 `>`：以前切片起点越过终点 panic
        assert_eq!(parse_subject(r#"<span property="v:itemreviewed"</span>"#).title, "");
        for html in [r#"<div id="info""#, r#"<div id="link-report"><div class="intro">"#, "criteria = '7:a", ""] {
            let _ = parse_subject(html);
        }
    }
}
