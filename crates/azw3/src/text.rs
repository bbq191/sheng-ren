//! 正文排版：每个 XHTML 文档拆成**骨架**（`<html><head>…<body aid="X"></body></html>`）和**片段**（body 里的内容），
//! 在 rawML 里依次存"骨架、片段、骨架、片段…"；片段在阅读器里插回骨架的 `</body>` 之前（KF8 样本黑盒分析得出）。
//!
//! 引用改写：图片 → `kindle:embed:XXXX?mime=…`（资源序号，1 起，base32 四位）；CSS → `kindle:flow:XXXX?mime=text/css`；
//! 书内链接 → `kindle:pos:fid:XXXX:off:YYYYYYYYYY`（fid = 片段序号，off = 目标在片段里的字节偏移，base32）。
//! 链接先写成等长占位串，所有文档排完、偏移定下来之后再回填，回填不改变任何偏移。

use crate::book::Loaded;
use bookconv::epubzip::{dir_of, percent_decode, posix_norm, resolve};
use regex::Regex;
use std::collections::HashMap;
use std::sync::OnceLock;

const B32: &[u8] = b"0123456789ABCDEFGHIJKLMNOPQRSTUV";

pub fn base32(mut v: u32, width: usize) -> String {
    let mut s = Vec::new();
    loop {
        s.push(B32[(v % 32) as usize]);
        v /= 32;
        if v == 0 {
            break;
        }
    }
    while s.len() < width {
        s.push(b'0');
    }
    s.reverse();
    String::from_utf8(s).unwrap()
}

const POS_PLACEHOLDER: &str = "kindle:pos:fid:####:off:##########";

pub struct Fragment {
    /// 插入位置（组装后文本里的绝对偏移）= 骨架起点 + 骨架里 `</body>` 的偏移。
    pub insert_pos: u32,
    pub len: u32,
    pub aid: String,
}

pub struct NcxItem {
    pub label: String,
    pub level: u32,
    pub pos: u32,
    pub fid: u32,
    pub off: u32,
}

pub struct Layout {
    /// 第 0 条流：全部骨架与片段。
    pub flow0: Vec<u8>,
    /// 各骨架 (起点, 长度)。
    pub skeletons: Vec<(u32, u32)>,
    pub fragments: Vec<Fragment>,
    /// CSS 流（第 1 条起）。
    pub css_flows: Vec<Vec<u8>>,
    pub ncx: Vec<NcxItem>,
}

fn attr_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r#"(?i)(\s)(href|src|xlink:href)(\s*=\s*)"([^"]*)""#).unwrap())
}

fn url_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r#"(?i)url\(\s*['"]?([^'")]+)['"]?\s*\)"#).unwrap())
}

/// CSS 里的 `url(图片)` 改成 `kindle:embed`；去掉 `@font-face`（不嵌字体，字体交给阅读器设置）。
fn rewrite_css(css: &str, css_path: &str, res: &HashMap<String, (u32, &'static str)>) -> String {
    static FACE: OnceLock<Regex> = OnceLock::new();
    let css = FACE.get_or_init(|| Regex::new(r#"(?is)@font-face\s*\{[^}]*\}"#).unwrap()).replace_all(css, "");
    url_re()
        .replace_all(&css, |c: &regex::Captures| {
            let p = posix_norm(&resolve(dir_of(css_path), &percent_decode(&c[1])));
            match res.get(&p) {
                Some((n, mime)) => format!("url(kindle:embed:{}?mime={mime})", base32(*n, 4)),
                None => c[0].to_string(),
            }
        })
        .into_owned()
}

/// 一个文档改写后的样子：骨架前半（到 body 开标签为止）、片段、骨架后半，外加片段里链接的目标（按出现顺序）。
struct Rewritten {
    head: String,
    frag: String,
    tail: String,
    links: Vec<(usize, String)>,
}

fn rewrite_doc(
    html: &str,
    path: &str,
    aid: &str,
    res: &HashMap<String, (u32, &'static str)>,
    flows: &HashMap<String, u32>,
    doc_index: &HashMap<String, usize>,
) -> Result<Rewritten, String> {
    static SCRIPT: OnceLock<Regex> = OnceLock::new();
    static TAG: OnceLock<Regex> = OnceLock::new();
    static AID: OnceLock<Regex> = OnceLock::new();
    static BODY: OnceLock<Regex> = OnceLock::new();
    static STYLE: OnceLock<Regex> = OnceLock::new();
    let html = SCRIPT.get_or_init(|| Regex::new(r#"(?is)<script\b.*?</script>"#).unwrap()).replace_all(html, "");
    let html = STYLE
        .get_or_init(|| Regex::new(r#"(?is)(<style\b[^>]*>)(.*?)(</style>)"#).unwrap())
        .replace_all(&html, |c: &regex::Captures| format!("{}{}{}", &c[1], rewrite_css(&c[2], path, res), &c[3]));
    let aid_re = AID.get_or_init(|| Regex::new(r#"(?i)\s+aid\s*=\s*"[^"]*""#).unwrap());
    let mut links: Vec<(usize, String)> = Vec::new();
    let out = TAG
        .get_or_init(|| Regex::new(r#"(?s)<([A-Za-z][A-Za-z0-9:]*)\b[^>]*>"#).unwrap())
        .replace_all(&html, |c: &regex::Captures| {
            let name = c[1].to_ascii_lowercase();
            let tag = aid_re.replace_all(&c[0], "");
            let is_css_link = name == "link" && tag.to_ascii_lowercase().contains("stylesheet");
            // 非样式表的 <link>（Adobe 的 page-template.xpgt 等）和指向书里不存在的样式表的 <link>，Kindle 用不上，去掉。
            if name == "link" {
                let href = attr_re().captures_iter(&tag).find(|a| a[2].eq_ignore_ascii_case("href")).map(|a| a[4].to_string()).unwrap_or_default();
                let target = posix_norm(&resolve(dir_of(path), &percent_decode(href.split('#').next().unwrap_or(""))));
                if !is_css_link || !flows.contains_key(&target) {
                    return String::new();
                }
            }
            attr_re()
                .replace_all(&tag, |a: &regex::Captures| {
                    let (lead, attr, eq, v) = (&a[1], &a[2], &a[3], &a[4]);
                    let keep = || a[0].to_string();
                    if v.contains(':') && !v.starts_with('#') {
                        return keep(); // http:、mailto:、data: 等外部地址
                    }
                    let (p, frag) = v.split_once('#').unwrap_or((v, ""));
                    let target = if p.is_empty() { path.to_string() } else { posix_norm(&resolve(dir_of(path), &percent_decode(p))) };
                    let new = if is_css_link {
                        match flows.get(&target) {
                            Some(n) => format!("kindle:flow:{}?mime=text/css", base32(*n, 4)),
                            None => return keep(),
                        }
                    } else if let Some((n, mime)) = res.get(&target).filter(|_| name != "a") {
                        format!("kindle:embed:{}?mime={mime}", base32(*n, 4))
                    } else if let Some(&j) = doc_index.get(&target).filter(|_| name == "a" || name == "area") {
                        links.push((j, frag.to_string()));
                        POS_PLACEHOLDER.to_string()
                    } else {
                        return keep();
                    };
                    format!("{lead}{attr}{eq}\"{new}\"")
                })
                .into_owned()
        })
        .into_owned();
    let body = BODY.get_or_init(|| Regex::new(r#"(?is)<body\b[^>]*>"#).unwrap()).find(&out).ok_or_else(|| format!("{path} 没有 <body>"))?;
    let close = out.to_ascii_lowercase().rfind("</body>").filter(|&c| c >= body.end()).ok_or_else(|| format!("{path} 没有 </body>"))?;
    let open = &out[body.start()..body.end()];
    let open = format!("{} aid=\"{aid}\">", open.trim_end_matches('>').trim_end_matches('/').trim_end());
    Ok(Rewritten { head: format!("{}{}", &out[..body.start()], open), frag: out[body.end()..close].to_string(), tail: out[close..].to_string(), links })
}

fn id_offsets(frag: &str) -> HashMap<String, usize> {
    static ID: OnceLock<Regex> = OnceLock::new();
    let re = ID.get_or_init(|| Regex::new(r#"(?i)<[A-Za-z][^>]*?\sid\s*=\s*"([^"]+)""#).unwrap());
    let mut m = HashMap::new();
    for c in re.captures_iter(frag) {
        m.entry(c[1].to_string()).or_insert(c.get(0).unwrap().start());
    }
    m
}

/// `res`：图片路径 → (资源序号 1 起, mime)。
pub fn layout(book: &Loaded, res: &HashMap<String, (u32, &'static str)>) -> Result<Layout, String> {
    let flows: HashMap<String, u32> = book.css.iter().enumerate().map(|(i, (p, _))| (p.clone(), i as u32 + 1)).collect();
    let css_flows: Vec<Vec<u8>> = book.css.iter().map(|(p, c)| rewrite_css(c, p, res).into_bytes()).collect();
    let doc_index: HashMap<String, usize> = book.docs.iter().enumerate().map(|(i, d)| (d.path.clone(), i)).collect();
    let mut docs = Vec::new();
    for (i, d) in book.docs.iter().enumerate() {
        docs.push(rewrite_doc(&d.html, &d.path, &base32(i as u32, 1), res, &flows, &doc_index)?);
    }
    let ids: Vec<HashMap<String, usize>> = docs.iter().map(|d| id_offsets(&d.frag)).collect();
    let resolve_off = |j: usize, frag: &str| -> u32 { if frag.is_empty() { 0 } else { ids[j].get(frag).copied().unwrap_or(0) as u32 } };

    let mut flow0: Vec<u8> = Vec::new();
    let mut skeletons = Vec::new();
    let mut fragments = Vec::new();
    for (i, d) in docs.iter_mut().enumerate() {
        // 回填链接占位串（等长，偏移不变）
        let mut k = 0;
        while let Some(p) = d.frag.find(POS_PLACEHOLDER) {
            let (j, frag) = &d.links[k];
            let real = format!("kindle:pos:fid:{}:off:{}", base32(*j as u32, 4), base32(resolve_off(*j, frag), 10));
            d.frag.replace_range(p..p + POS_PLACEHOLDER.len(), &real);
            k += 1;
        }
        let skel = format!("{}{}", d.head, d.tail);
        let start = flow0.len() as u32;
        skeletons.push((start, skel.len() as u32));
        fragments.push(Fragment { insert_pos: start + d.head.len() as u32, len: d.frag.len() as u32, aid: base32(i as u32, 1) });
        flow0.extend_from_slice(skel.as_bytes());
        flow0.extend_from_slice(d.frag.as_bytes());
    }
    let ncx = book
        .toc
        .iter()
        .filter_map(|t| {
            let j = *doc_index.get(&t.path)?;
            let off = resolve_off(j, &t.frag);
            Some(NcxItem { label: t.label.clone(), level: t.level, pos: fragments[j].insert_pos + off, fid: j as u32, off })
        })
        .collect();
    Ok(Layout { flow0, skeletons, fragments, css_flows, ncx })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base32_matches_kindle_digits() {
        assert_eq!(base32(9, 4), "0009");
        assert_eq!(base32(31, 4), "000V");
        assert_eq!(base32(32, 4), "0010");
        assert_eq!(base32(0, 10), "0000000000");
        assert_eq!(POS_PLACEHOLDER.len(), "kindle:pos:fid:0000:off:0000000000".len());
    }
}
