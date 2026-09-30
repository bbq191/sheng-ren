//! 漫画设成阅读器页边距 1 之后（profile 的 `comic_reader_margins`，xochitl）各种页面的补救。
//!
//! xochitl 的页边距设成 1，整页图左右离屏幕边缘就只有 1px（用户要的），但同一本漫画里的其它页也跟着贴边或出问题。
//! 下面三条都是 2026-09-21 在 Move 上读 xochitl 排出的 PDF 实测出来的（上游设备增强项目的真机诊断，本仓库 2026-09-29 照着重新实现）：
//! - **纯文字页**（版权页、前情提要、章节标题页）：字贴着屏幕边。`<body>` 加 [`TEXT_PAGE_CLASS`]，规则是左右
//!   `margin` 17.8pt，也就是默认 56 档的留白（56px × 303pt/954px）。xochitl 不认 `padding`（写在哪儿都不生效），`margin` 用 pt 精确生效，`%` 换算怪异。
//! - **有图的页**：`<body>` 带任何类，图就被吃掉约 20pt 宽，而且改类里的规则也没用，只能把 `<body>` 的 `class` 整个去掉
//!   （图页的排版不靠 body 类）。
//! - **图文混排页**：不能给 body 加边距（图会被压小），只给文字块（`<p>`、`<h1>`–`<h6>`、清洗层的 `div.eink-flush`）加
//!   [`TEXT_BLOCK_CLASS`]。规则必须带元素名（`p.eink-tx`），不然压不过书自带的类规则；每个选择器单独一条（xochitl 的 CSS 解析器很脆）。
//!
//! 只在整本判成漫画、profile 开了 `comic_reader_margins` 时做（`optimize` 的第二遍）。幂等：已经处理过的页返回 `None`。
use crate::html;
use crate::wash::opf::is_local;

/// 纯文字页 `<body>` 上的类。
pub const TEXT_PAGE_CLASS: &str = "eink-textpage";
/// 混排页文字块上的类。
pub const TEXT_BLOCK_CLASS: &str = "eink-tx";
/// 追加进 `eink-wash.css` 的规则（末尾分号必须有：xochitl 丢掉最后一个没分号的声明）。
pub const CSS_RULES: &str = ".eink-textpage{margin-left:17.8pt;margin-right:17.8pt;}
p.eink-tx{margin-left:17.8pt;margin-right:17.8pt;}
h1.eink-tx{margin-left:17.8pt;margin-right:17.8pt;}
h2.eink-tx{margin-left:17.8pt;margin-right:17.8pt;}
h3.eink-tx{margin-left:17.8pt;margin-right:17.8pt;}
h4.eink-tx{margin-left:17.8pt;margin-right:17.8pt;}
h5.eink-tx{margin-left:17.8pt;margin-right:17.8pt;}
h6.eink-tx{margin-left:17.8pt;margin-right:17.8pt;}
div.eink-tx{margin-left:17.8pt;margin-right:17.8pt;}
";

const MEDIA: [&str; 6] = ["img", "image", "svg", "video", "object", "canvas"];

/// 是不是要留边的文字块：`<p>`、`<h1>`–`<h6>`，以及清洗层生成的 `<div class="eink-flush">`（普通结构 div 不算，加了会挤压图）。
fn is_text_block(t: &html::Tag, tag: &str) -> bool {
    t.is("p") || t.heading_level().is_some() || (t.is("div") && html::has_class(tag, "eink-flush"))
}

/// 按页面类型处理一页（见模块文档）。没有 `<body>`、空页、已经处理过 → `None`。
pub fn pad_page(page: &str) -> Option<String> {
    let body = html::tags(page).find(|t| t.is_start() && t.is("body"))?;
    let rest = &page[body.end..];
    let has_media = html::tags(rest).any(|t| t.is_start() && MEDIA.iter().any(|m| is_local(t.name, m)));
    let has_text = !html::plain_text(rest).is_empty();
    let body_tag = &page[body.start..body.end];
    let (new_body, new_rest) = match (has_media, has_text) {
        (false, true) if !html::has_class(body_tag, TEXT_PAGE_CLASS) => (html::add_class(body_tag, TEXT_PAGE_CLASS), rest.to_string()),
        (true, _) => {
            let new_body = html::remove_attr(body_tag, "class");
            let mut out = String::with_capacity(rest.len());
            let mut last = 0;
            if has_text {
                for t in html::tags(rest).filter(|t| t.is_start()) {
                    let tag = &rest[t.start..t.end];
                    if is_text_block(&t, tag) && !html::has_class(tag, TEXT_BLOCK_CLASS) {
                        out.push_str(&rest[last..t.start]);
                        out.push_str(&html::add_class(tag, TEXT_BLOCK_CLASS));
                        last = t.end;
                    }
                }
            }
            out.push_str(&rest[last..]);
            (new_body, out)
        }
        _ => return None,
    };
    (new_body != body_tag || new_rest != rest).then(|| format!("{}{new_body}{new_rest}", &page[..body.start]))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_page_gets_body_class() {
        let p = r#"<html><head><title>t</title></head><body class="x"><p>版权页</p></body></html>"#;
        let out = pad_page(p).unwrap();
        assert!(out.contains(r#"<body class="x eink-textpage">"#), "{out}");
        assert_eq!(pad_page(&out), None, "幂等");
    }

    #[test]
    fn image_page_drops_body_class() {
        let p = r#"<body class="calibre2" id="b"><div><img src="a.jpg" alt="第 1 頁"/></div></body>"#;
        assert_eq!(pad_page(p).unwrap(), r#"<body id="b"><div><img src="a.jpg" alt="第 1 頁"/></div></body>"#);
        assert_eq!(pad_page(r#"<body><div><img src="a.jpg"/></div></body>"#), None, "没有类、没有字：不用动");
    }

    #[test]
    fn mixed_page_pads_text_blocks_only() {
        let p = r#"<body class="c"><h2>第十七回</h2><div class="fs"><img src="a.jpg"/></div><div class="eink-flush">说明</div><p class='k'>字</p></body>"#;
        let out = pad_page(p).unwrap();
        assert_eq!(
            out,
            r#"<body><h2 class="eink-tx">第十七回</h2><div class="fs"><img src="a.jpg"/></div><div class="eink-flush eink-tx">说明</div><p class='k eink-tx'>字</p></body>"#
        );
        assert_eq!(pad_page(&out), None, "幂等");
    }

    #[test]
    fn empty_page_untouched() {
        assert_eq!(pad_page("<body class='x'><p> </p></body>"), None);
    }
}
