//! 翻页方向：OPF `<spine page-progression-direction="rtl|ltr">` 的读与写（2026-09-25）。
//!
//! 阅读器按这个属性决定"日漫从右往左翻页"。calibre 转出的日漫 OPF 大多不写它，所以优化时可以**按书手动**
//! 指定方向（`OptimizeOpts::page_direction`），由这里把属性写进 OPF。只动 `<spine>` 开标签上这一个属性，
//! OPF 其余字节原样——不改书的内容（规范白皮书 §2 底线 1）。
//!
//! 不自动判：漫画识别（`comic_detect`）只能看出"是漫画"，看不出"是日漫"——国漫、美漫是从左往右，
//! 自动设 rtl 会把它们翻反（规范白皮书 §4.6）。
use regex::Regex;
use std::sync::OnceLock;

/// 书的翻页方向（EPUB 3 `page-progression-direction` 的两个显式值；`default` 与缺省按 `Ltr` 以外的"未写"处理）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PageDirection {
    /// 从左往右（普通书、国漫、美漫）。
    Ltr,
    /// 从右往左（日漫）。
    Rtl,
}

impl PageDirection {
    /// 写进 OPF 的属性值。
    pub fn as_str(self) -> &'static str {
        match self {
            PageDirection::Ltr => "ltr",
            PageDirection::Rtl => "rtl",
        }
    }
    /// `"rtl"`/`"ltr"` → 方向；其它（含 `"auto"`、`"default"`、空）→ `None`。
    pub fn parse(s: &str) -> Option<PageDirection> {
        match s.trim().to_ascii_lowercase().as_str() {
            "rtl" => Some(PageDirection::Rtl),
            "ltr" => Some(PageDirection::Ltr),
            _ => None,
        }
    }
}

/// `<spine …>` 开标签（允许命名空间前缀如 `<opf:spine`，允许自闭合）。
fn spine_tag_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r#"(?s)<(?:[A-Za-z_][\w.-]*:)?spine\b[^>]*>"#).unwrap())
}

/// 开标签里的 `page-progression-direction="…"`（单双引号都认）。
fn attr_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r#"(?s)(\s)page-progression-direction\s*=\s*(?:"([^"]*)"|'([^']*)')"#).unwrap())
}

/// 读 OPF 文本里写明的方向：`rtl`/`ltr` → `Some`；没写、写 `default` 或别的值 → `None`。
pub fn spine_direction(opf: &str) -> Option<PageDirection> {
    let tag = spine_tag_re().find(opf)?.as_str();
    let c = attr_re().captures(tag)?;
    PageDirection::parse(c.get(2).or_else(|| c.get(3)).map_or("", |m| m.as_str()))
}

/// 把 OPF 的 spine 方向改成 `dir`：已有属性就改值，没有就插在 `<spine` 标签名后面；找不到 `<spine>` 原样返回。
/// 已是这个值时返回的字符串与输入逐字节相同。
pub fn set_spine_direction(opf: &str, dir: PageDirection) -> String {
    let Some(m) = spine_tag_re().find(opf) else { return opf.to_string() };
    let tag = m.as_str();
    let new_tag = if attr_re().is_match(tag) {
        attr_re().replace(tag, |c: &regex::Captures| format!(r#"{}page-progression-direction="{}""#, &c[1], dir.as_str())).into_owned()
    } else {
        // 插在标签名之后：`<spine toc="ncx">` → `<spine page-progression-direction="rtl" toc="ncx">`。
        let name_end = tag.find(|c: char| c.is_whitespace() || c == '>' || c == '/').unwrap_or(tag.len());
        format!(r#"{} page-progression-direction="{}"{}"#, &tag[..name_end], dir.as_str(), &tag[name_end..])
    };
    if new_tag == tag {
        return opf.to_string();
    }
    format!("{}{}{}", &opf[..m.start()], new_tag, &opf[m.end()..])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_explicit_values_only() {
        assert_eq!(spine_direction(r#"<package><spine toc="ncx" page-progression-direction="rtl"/></package>"#), Some(PageDirection::Rtl));
        assert_eq!(spine_direction(r#"<spine page-progression-direction='ltr'>"#), Some(PageDirection::Ltr));
        assert_eq!(spine_direction(r#"<opf:spine page-progression-direction="rtl">"#), Some(PageDirection::Rtl));
        assert_eq!(spine_direction(r#"<spine page-progression-direction="default">"#), None);
        assert_eq!(spine_direction(r#"<spine toc="ncx">"#), None);
        assert_eq!(spine_direction("<package/>"), None);
        // 只看 spine 开标签，不被正文别处的同名字样骗到
        assert_eq!(spine_direction(r#"<meta content='page-progression-direction="rtl"'/><spine toc="ncx">"#), None);
    }

    #[test]
    fn set_inserts_replaces_and_keeps_everything_else() {
        let opf = r#"<?xml version="1.0"?><package><manifest/><spine toc="ncx"><itemref idref="a"/></spine></package>"#;
        let rtl = set_spine_direction(opf, PageDirection::Rtl);
        assert_eq!(rtl, r#"<?xml version="1.0"?><package><manifest/><spine page-progression-direction="rtl" toc="ncx"><itemref idref="a"/></spine></package>"#);
        assert_eq!(spine_direction(&rtl), Some(PageDirection::Rtl));
        let ltr = set_spine_direction(&rtl, PageDirection::Ltr);
        assert_eq!(ltr, rtl.replace(r#""rtl""#, r#""ltr""#), "已有属性只改值");
        assert_eq!(set_spine_direction(&ltr, PageDirection::Ltr), ltr, "已是这个值 → 逐字节不变");
        // 自闭合、单引号、无属性
        assert_eq!(set_spine_direction("<spine/>", PageDirection::Rtl), r#"<spine page-progression-direction="rtl"/>"#);
        assert_eq!(set_spine_direction("<spine>", PageDirection::Rtl), r#"<spine page-progression-direction="rtl">"#);
        assert_eq!(set_spine_direction(r#"<spine page-progression-direction='rtl' toc="x">"#, PageDirection::Ltr), r#"<spine page-progression-direction="ltr" toc="x">"#);
        assert_eq!(set_spine_direction("<package/>", PageDirection::Rtl), "<package/>", "没有 spine 不动");
    }
}
