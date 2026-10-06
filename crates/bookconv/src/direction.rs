//! 翻页方向：OPF `<spine page-progression-direction="rtl|ltr">` 的读与写（2026-09-25）。
//!
//! 阅读器按这个属性决定"日漫从右往左翻页"。calibre 转出的日漫 OPF 大多不写它，所以优化时可以**按书手动**
//! 指定方向（`OptimizeOpts::page_direction`），由这里把属性写进 OPF。只动 `<spine>` 开标签上这一个属性，
//! OPF 其余字节原样——不改书的内容（规范白皮书 §2 底线 1）。
//!
//! 不自动判：漫画识别（`comic_detect`）只能看出"是漫画"，看不出"是日漫"——国漫、美漫是从左往右，
//! 自动设 rtl 会把它们翻反（规范白皮书 §4.6）。
use crate::html;

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

/// 第一个 `<spine …>` 开标签（认带前缀的 `<opf:spine>`、自闭合；按 `crate::html` 扫，注释里的不算）。
fn spine_tag(opf: &str) -> Option<html::Tag<'_>> {
    html::tags(opf).find(|t| t.is_start() && crate::wash::opf::is_local(t.name, "spine"))
}

const ATTR: &str = "page-progression-direction";

/// 读 OPF 文本里写明的方向：`rtl`/`ltr` → `Some`；没写、写 `default` 或别的值 → `None`。
pub fn spine_direction(opf: &str) -> Option<PageDirection> {
    let t = spine_tag(opf)?;
    PageDirection::parse(html::attr_value(&opf[t.start..t.end], ATTR)?)
}

/// 把 OPF 的 spine 方向改成 `dir`：已有属性就改值（写成双引号），没有就插在 `<spine` 标签名后面；找不到 `<spine>` 原样返回。
/// 已是这个值时返回的字符串与输入逐字节相同。2026-10-06 审计：此前按正则找，`page-progression-direction=rtl`（没引号）认不出，
/// 又插进一个同名属性，OPF 不合法；注释里的 `<spine>` 也会被改。
pub fn set_spine_direction(opf: &str, dir: PageDirection) -> String {
    let Some(t) = spine_tag(opf) else { return opf.to_string() };
    let tag = &opf[t.start..t.end];
    let new_attr = format!(r#"{ATTR}="{}""#, dir.as_str());
    let new_tag = match html::attr(tag, ATTR) {
        Some(a) if a.value == dir.as_str() => return opf.to_string(),
        Some(a) => format!("{}{new_attr}{}", &tag[..a.start], &tag[a.end..]),
        // 插在标签名之后：`<spine toc="ncx">` → `<spine page-progression-direction="rtl" toc="ncx">`。
        None => {
            let name_end = 1 + t.name.len();
            format!("{} {new_attr}{}", &tag[..name_end], &tag[name_end..])
        }
    };
    format!("{}{}{}", &opf[..t.start], new_tag, &opf[t.end..])
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
        // 没引号的值就地改，不再多插一个同名属性；注释里的 spine、名字只是以 spine 开头的元素不算
        assert_eq!(set_spine_direction("<spine page-progression-direction=rtl toc=ncx>", PageDirection::Ltr), r#"<spine page-progression-direction="ltr" toc=ncx>"#);
        assert_eq!(spine_direction("<spine page-progression-direction=rtl toc=ncx>"), Some(PageDirection::Rtl));
        let c = r#"<!-- <spine page-progression-direction="rtl"> --><spine-x/><opf:spine toc="ncx"/>"#;
        assert_eq!(spine_direction(c), None);
        assert_eq!(set_spine_direction(c, PageDirection::Rtl), r#"<!-- <spine page-progression-direction="rtl"> --><spine-x/><opf:spine page-progression-direction="rtl" toc="ncx"/>"#);
    }
}
