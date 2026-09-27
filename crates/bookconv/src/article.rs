//! 网文抓取：网页文章 URL → 抓取 → 可读性抽取（Mozilla Readability 纯 Rust 移植 `readability-rust`）→
//! 白名单重序列化成合法 XHTML → 组干净 EPUB 字节（与设备无关的母版）。**不落盘**，去向由调用方定
//! （`booklib add <网址>` 入库）。
//!
//! 能力边界（据实告知，非 bug）：可读性抽取对**静态 HTML 的文章/博客/新闻**效果好；SPA（纯 JS 渲染）、
//! 付费墙、反爬站抽不出——纯 Rust 无头浏览器的天花板。网页按 HTTP 头 / `<meta charset>` 解码（GBK 等中文站可用）。
//! 配图最多下载 [`MAX_IMAGES`] 张（4 路并发）；超出上限或下载失败的图**保留原远程地址**，不从正文里删掉。
//! 当前恒产**单章**（单篇文章足够；多章连载的目录抓取+分章列为后续）。

use crate::convert::common;
use crate::epub::{Book, BookMeta, Chapter, Resource};
use crate::netimg::{fetch_image, http_agent, UA};
use crate::util::xml_escape;
use readability_rust::Readability;
use regex::Regex;

/// 单篇下载进 EPUB 的配图上限（异常页几百张图时不无限下载）；超出的保留远程地址。
const MAX_IMAGES: usize = 40;
/// 同时下载的配图数。
const IMAGE_FETCH_THREADS: usize = 4;
/// 网页正文字节上限。
const MAX_PAGE_BYTES: u64 = 20 * 1024 * 1024;

/// 网页文章 URL → 抓取+抽取+组 EPUB 母版。返回 (epub 字节, 标题)。
pub fn build_article_epub(url: &str) -> Result<(Vec<u8>, String), String> {
    let url = strip_bad_params(url.trim());
    let url = url.as_str();
    if !(url.starts_with("http://") || url.starts_with("https://")) {
        return Err("请输入 http(s) 网址".into());
    }
    let html = fetch_text(url)?;
    let mut r = Readability::new_with_base_uri(&html, url, None) // base_uri：相对图片/链接解析成绝对
        .map_err(|e| format!("解析 HTML 失败: {e:?}"))?;
    let article = r.parse().ok_or("抽取失败（可能是 SPA/付费墙/非文章页）")?;

    let title = article
        .title
        .map(|t| t.trim().to_string())
        .filter(|t| !t.is_empty())
        .unwrap_or_else(|| host_of(url));
    let content = article.content.filter(|c| !c.trim().is_empty()).ok_or("正文为空")?;
    let lang = article.lang.filter(|l| !l.trim().is_empty()).unwrap_or_else(|| "zh".into());
    let author = article.byline.unwrap_or_default();

    let (body, resources) = process_content(&content, url);
    if body.trim().is_empty() {
        return Err("正文处理后为空".into());
    }
    // 文章来源 + 原链接放正文末尾（可回溯）。
    let body = format!("{body}<hr/><p><small>来源：{} · <a href=\"{}\">{}</a></small></p>", xml_escape(&host_of(url)), xml_escape(url), xml_escape(url));

    let mut book = Book {
        meta: BookMeta {
            book_id: format!("readlater:{}", common::sanitize_id(&title)),
            title: title.clone(),
            author,
            language: lang,
            publisher: host_of(url),
            cover: None,
            cover_ext: "jpg".into(),
            cover_media_type: "image/jpeg".into(),
        },
        chapters: vec![Chapter { title: title.clone(), html_body: body, level: 1 }],
        resources,
        nav: Vec::new(),
    };
    let epub = common::assemble_master(&mut book)?;
    Ok((epub, title))
}

/// 去掉已知会触发反爬拦截的查询参数（当前：微信公众号的 `poc_token`——带它返回"请在微信打开"拦截页，
/// 去掉后裸链 `/s/<id>` 正文完整可抓）。只删这个已知有害参数，其余查询原样保留。
fn strip_bad_params(url: &str) -> String {
    static R: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
    let re = R.get_or_init(|| Regex::new(r#"(?i)([?&])poc_token=[^&#]*"#).unwrap());
    let s = re.replace_all(url, "$1").into_owned();
    // 清理替换后残留的分隔符：`?&`→`?`、`&&`→`&`、尾部 `?`/`&`
    s.replace("?&", "?").replace("&&", "&").trim_end_matches(['?', '&']).to_string()
}

/// 抓网页 HTML 并解码成文本：编码按 HTTP 头 `Content-Type` 的 charset → BOM → 页面 `<meta charset>` 的顺序认，
/// 都没有按 UTF-8（坏字节换成 U+FFFD，不整页失败）。
fn fetch_text(url: &str) -> Result<String, String> {
    let resp = http_agent(30).get(url).set("User-Agent", UA).set("Accept", "text/html,application/xhtml+xml").call().map_err(|e| format!("抓取失败: {e}"))?;
    let header_charset = resp.header("Content-Type").and_then(charset_param).map(str::to_string);
    let mut bytes = Vec::new();
    use std::io::Read;
    resp.into_reader().take(MAX_PAGE_BYTES).read_to_end(&mut bytes).map_err(|e| format!("读取网页正文失败: {e}"))?;
    Ok(decode_html(&bytes, header_charset.as_deref()))
}

/// `text/html; charset=gbk` → `gbk`。
fn charset_param(content_type: &str) -> Option<&str> {
    content_type.split(';').skip(1).find_map(|p| {
        let (k, v) = p.split_once('=')?;
        k.trim().eq_ignore_ascii_case("charset").then(|| v.trim().trim_matches(['"', '\'']))
    })
}

fn decode_html(bytes: &[u8], header_charset: Option<&str>) -> String {
    let by_label = |l: &str| encoding_rs::Encoding::for_label(l.trim().as_bytes());
    let (enc, body) = if let Some(enc) = header_charset.and_then(by_label) {
        (enc, bytes)
    } else if let Some((enc, bom_len)) = encoding_rs::Encoding::for_bom(bytes) {
        (enc, &bytes[bom_len..])
    } else {
        (meta_charset(bytes).and_then(|l| by_label(&l)).unwrap_or(encoding_rs::UTF_8), bytes)
    };
    enc.decode_without_bom_handling(body).0.into_owned()
}

/// 页面开头的 `<meta charset="gbk">` 或 `<meta http-equiv="Content-Type" content="text/html; charset=gb2312">`。
fn meta_charset(bytes: &[u8]) -> Option<String> {
    static R: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
    let re = R.get_or_init(|| Regex::new(r#"(?i)<meta\b[^>]*?charset\s*=\s*["']?\s*([A-Za-z0-9_:.\-]+)"#).unwrap());
    let head = String::from_utf8_lossy(&bytes[..bytes.len().min(4096)]);
    re.captures(&head).map(|c| c[1].to_string())
}

/// 处理抽取出的正文 HTML → 合法 XHTML 正文 + 内嵌图片资源。
/// **用真 HTML 解析器（scraper）重解析 + 白名单重序列化**——readability 输出是 HTML5，直接塞进 XHTML
/// 章会崩（`<link>`/`<source>` 空元素不自闭、`&nbsp;` 实体、`data-mw` 属性里塞未转义 `<`；正则补不了）。
/// 重解析后按白名单只保排版标签、属性/文本全转义、空元素自闭，无论输入多脏都产出合法 XHTML。
/// 图片分两步：遍历时先记下地址、在正文里留占位，遍历完再并发下载前 [`MAX_IMAGES`] 张，最后回填。
fn process_content(content: &str, page_url: &str) -> (String, Vec<Resource>) {
    let (out, imgs) = serialize_content(content);
    let referer = origin_of(page_url); // 抓图带 Referer（微信 mmbiz 等防盗链需要）
    let fetched = fetch_images(&imgs, &referer);
    let mut resources: Vec<Resource> = Vec::new();
    let mut tags: Vec<String> = Vec::with_capacity(imgs.len());
    for (img, got) in imgs.iter().zip(fetched) {
        let src = match got {
            Some((bytes, ext, mime)) => {
                let path = format!("images/rl{}.{ext}", resources.len() + 1);
                resources.push(Resource { path: path.clone(), media_type: mime.into(), bytes });
                path
            }
            None => img.src.clone(), // 超出上限/下载失败：保留原远程地址，不删图
        };
        tags.push(format!("<img src=\"{}\" alt=\"{}\"/>", xml_escape(&src), xml_escape(&img.alt)));
    }
    (fill_image_marks(&out, &tags), resources)
}

/// 正文里的一张图：地址（已按页面 URL 解析成绝对地址）+ alt。
struct ImgRef {
    src: String,
    alt: String,
}

/// 图片占位：`\u{1}序号\u{1}`（控制字符不会出现在转义后的正文里）。
const IMG_MARK: char = '\u{1}';

fn serialize_content(content: &str) -> (String, Vec<ImgRef>) {
    let frag = scraper::Html::parse_fragment(content);
    let mut imgs: Vec<ImgRef> = Vec::new();
    let mut out = String::new();
    for child in frag.tree.root().children() {
        emit_node(child, &mut out, &mut imgs);
    }
    (out, imgs)
}

fn fill_image_marks(out: &str, tags: &[String]) -> String {
    let mut res = String::with_capacity(out.len() + tags.iter().map(|t| t.len()).sum::<usize>());
    let mut parts = out.split(IMG_MARK);
    res.push_str(parts.next().unwrap_or(""));
    while let (Some(n), Some(rest)) = (parts.next(), parts.next()) {
        if let Some(tag) = n.parse::<usize>().ok().and_then(|i| tags.get(i)) {
            res.push_str(tag);
        }
        res.push_str(rest);
    }
    res
}

/// 下载到的一张图：(字节, 扩展名, MIME)。
type Fetched = (Vec<u8>, &'static str, &'static str);

/// 并发下载前 [`MAX_IMAGES`] 张图（[`IMAGE_FETCH_THREADS`] 路），结果与 `imgs` 一一对应；超出上限的为 `None`。
fn fetch_images(imgs: &[ImgRef], referer: &str) -> Vec<Option<Fetched>> {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Mutex;
    let n = imgs.len().min(MAX_IMAGES);
    let results: Mutex<Vec<Option<Fetched>>> = Mutex::new((0..imgs.len()).map(|_| None).collect());
    if n == 0 {
        return results.into_inner().unwrap();
    }
    let ag = http_agent(20);
    let next = AtomicUsize::new(0);
    std::thread::scope(|sc| {
        for _ in 0..IMAGE_FETCH_THREADS.min(n) {
            sc.spawn(|| loop {
                let i = next.fetch_add(1, Ordering::Relaxed);
                if i >= n {
                    break;
                }
                let got = fetch_image(&ag, &imgs[i].src, referer, None);
                results.lock().unwrap()[i] = got;
            });
        }
    });
    results.into_inner().unwrap()
}

/// URL 的 origin（scheme://host/），作抓图 Referer。
fn origin_of(url: &str) -> String {
    let after = match url.split_once("://") {
        Some((scheme, rest)) => format!("{scheme}://{}", rest.split('/').next().unwrap_or(rest)),
        None => return String::new(),
    };
    format!("{after}/")
}

/// 白名单排版标签（其余标签「拆壳」——丢标签保子内容）。
fn is_whitelisted(name: &str) -> bool {
    matches!(
        name,
        "p" | "div" | "span" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6" | "a" | "em" | "strong" | "b" | "i" | "u" | "s" | "sub" | "sup" | "code" | "pre" | "blockquote" | "ul" | "ol" | "li" | "table" | "thead" | "tbody" | "tr" | "td" | "th" | "caption" | "figure" | "figcaption" | "br" | "hr" | "small" | "mark" | "section" | "article" | "dl" | "dt" | "dd"
    )
}
/// 整棵丢弃（含子内容）的标签——脚本/样式/媒体/表单/页面 chrome/元数据。
fn is_drop_subtree(name: &str) -> bool {
    matches!(
        name,
        "script" | "style" | "noscript" | "link" | "meta" | "head" | "title" | "video" | "audio" | "iframe" | "object" | "embed" | "source" | "track" | "svg" | "canvas" | "form" | "input" | "button" | "select" | "textarea" | "nav" | "footer" | "header" | "aside"
    )
}

/// 递归发射一个节点成 XHTML。图片只留占位（见 [`IMG_MARK`]），地址记进 `imgs`。
fn emit_node(node: ego_tree::NodeRef<scraper::node::Node>, out: &mut String, imgs: &mut Vec<ImgRef>) {
    use scraper::node::Node;
    match node.value() {
        Node::Text(t) => out.push_str(&xml_escape(&t.text)),
        Node::Element(el) => {
            let name = el.name();
            if is_drop_subtree(name) {
                return;
            }
            if name == "img" {
                // 懒加载兜底：真实图 URL 常在 data-src（微信/知乎等），src 缺失或是 data: 占位时用 data-src。
                let lazy = el.attr("data-src").filter(|s| !s.is_empty());
                let src = el.attr("src").filter(|s| !s.is_empty() && !(s.starts_with("data:") && lazy.is_some())).or(lazy);
                if let Some(src) = src {
                    out.push(IMG_MARK);
                    out.push_str(&imgs.len().to_string());
                    out.push(IMG_MARK);
                    imgs.push(ImgRef { src: src.to_string(), alt: el.attr("alt").unwrap_or("").to_string() });
                }
                return; // 空元素；没有任何地址的图没东西可留
            }
            if !is_whitelisted(name) {
                for c in node.children() {
                    emit_node(c, out, imgs); // 拆壳：丢未知标签、保子内容
                }
                return;
            }
            if name == "br" || name == "hr" {
                out.push('<');
                out.push_str(name);
                out.push_str("/>");
                return;
            }
            out.push('<');
            out.push_str(name);
            for (k, v) in el.attrs() {
                if keep_attr(name, k) {
                    out.push_str(&format!(" {k}=\"{}\"", xml_escape(v)));
                }
            }
            out.push('>');
            for c in node.children() {
                emit_node(c, out, imgs);
            }
            out.push_str(&format!("</{name}>"));
        }
        _ => {} // 注释/doctype/PI 丢弃
    }
}

/// 属性白名单：只保排版必需（链接 href、表格跨行列），其余（class/style/data-*/id 等）全丢——
/// 阅读器用自己的样式，脏属性只会带来 XHTML 风险。
fn keep_attr(tag: &str, attr: &str) -> bool {
    match tag {
        "a" => attr == "href",
        "td" | "th" => attr == "colspan" || attr == "rowspan",
        _ => false,
    }
}

fn host_of(url: &str) -> String {
    url.split("://").nth(1).and_then(|rest| rest.split('/').next()).unwrap_or(url).trim_start_matches("www.").to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_produces_valid_xhtml_from_dirty_html() {
        // 脏输入：style/script 块、未自闭 <br>/<link>、未知标签 <custom>、data 属性、&nbsp;
        let dirty = r#"<div class="x" data-mw="{&quot;a&quot;:&quot;<b>&quot;}"><style>.y{}</style>
            <p>正文<br>换行</p><script>bad()</script><link rel="z">
            <custom>拆壳保留</custom><a href="http://e.com" class="lnk">链接</a>
            <span typeof="mw:Entity">&nbsp;</span></div>"#;
        let (body, _imgs) = serialize_content(dirty);
        // 包成 XHTML 章能被 XML 解析器接受（严格良构）
        let doc = format!(r#"<?xml version="1.0"?><root xmlns="http://www.w3.org/1999/xhtml">{body}</root>"#);
        let mut r = quick_xml::Reader::from_str(&doc);
        loop {
            match r.read_event() {
                Ok(quick_xml::events::Event::Eof) => break,
                Ok(_) => {}
                Err(e) => panic!("产物非良构 XHTML: {e}\n{body}"),
            }
        }
        assert!(body.contains("<br/>"), "br 自闭: {body}");
        assert!(body.contains("正文") && body.contains("换行"), "正文保留: {body}");
        assert!(body.contains("拆壳保留"), "未知标签拆壳保子内容: {body}");
        assert!(body.contains(r#"<a href="http://e.com">链接</a>"#), "链接保 href 去 class: {body}");
        assert!(!body.contains("style") && !body.contains("script") && !body.contains("data-mw"), "脏东西去净: {body}");
    }

    #[test]
    fn images_beyond_limit_or_failed_keep_remote_src() {
        // 下载不到（非 http 地址 → fetch_image 直接 None，不联网）的图保留原地址，不从正文里消失
        let (out, imgs) = serialize_content(r#"<p>前<img src="ftp://x/a.png" alt="图 &quot;一&quot;">后</p><p><img src="data:image/png;base64,AA" data-src="ftp://x/b.png"></p>"#);
        assert_eq!(imgs.len(), 2);
        assert_eq!(imgs[1].src, "ftp://x/b.png", "懒加载用 data-src");
        let fetched = fetch_images(&imgs, "");
        assert!(fetched.iter().all(|f| f.is_none()));
        let tags: Vec<String> = imgs.iter().map(|i| format!("<img src=\"{}\" alt=\"{}\"/>", xml_escape(&i.src), xml_escape(&i.alt))).collect();
        let body = fill_image_marks(&out, &tags);
        assert_eq!(body, r#"<p>前<img src="ftp://x/a.png" alt="图 &quot;一&quot;"/>后</p><p><img src="ftp://x/b.png" alt=""/></p>"#);
    }

    #[test]
    fn non_utf8_pages_decoded_by_header_or_meta() {
        let gbk = b"<html><head><meta charset=\"gbk\"></head><body>\xd6\xd0\xce\xc4</body></html>";
        assert!(decode_html(gbk, None).contains("中文"), "按 <meta charset> 解 GBK");
        assert!(decode_html(b"\xd6\xd0\xce\xc4", Some("GB2312")).contains("中文"), "按 HTTP 头解");
        assert_eq!(charset_param("text/html; Charset=\"gbk\""), Some("gbk"));
        let old = b"<meta http-equiv=\"Content-Type\" content=\"text/html; charset=gb2312\"><p>\xd6\xd0</p>";
        assert!(decode_html(old, None).contains("中"));
        assert!(decode_html("纯 UTF-8".as_bytes(), None).contains("纯 UTF-8"));
    }

    #[test]
    fn host_of_strips_scheme_and_www() {
        assert_eq!(host_of("https://www.example.com/a/b"), "example.com");
        assert_eq!(host_of("http://blog.rust-lang.org/x"), "blog.rust-lang.org");
    }

    #[test]
    fn strip_poc_token_keeps_other_params() {
        assert_eq!(strip_bad_params("https://mp.weixin.qq.com/s/ID?poc_token=ABC"), "https://mp.weixin.qq.com/s/ID");
        assert_eq!(strip_bad_params("https://x/s/ID?a=1&poc_token=ABC&b=2"), "https://x/s/ID?a=1&b=2");
        assert_eq!(strip_bad_params("https://x/s/ID"), "https://x/s/ID"); // 无 token 原样
    }

    #[test]
    fn origin_of_extracts_scheme_host() {
        assert_eq!(origin_of("https://mp.weixin.qq.com/s/ID?x=1"), "https://mp.weixin.qq.com/");
        assert_eq!(origin_of("notaurl"), "");
    }

    #[test]
    fn build_rejects_non_http() {
        assert!(build_article_epub("ftp://x").is_err());
        assert!(build_article_epub("not a url").is_err());
    }
}
