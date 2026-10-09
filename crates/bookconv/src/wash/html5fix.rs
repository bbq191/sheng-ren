//! 规范整理修不好的 XHTML（交叉嵌套 `<p><i>甲</p></i>`、中间没关的 `<p>甲<p>乙`、认不出的实体……）按 **HTML5 解析算法**重新
//! 解析、写回成 XHTML（2026-10-09 用户定：标签未关闭这类语法错误还是要修）。HTML5 规范把"浏览器怎么理解坏掉的 HTML"写死了
//! （隐含的结束标签、格式元素的收养算法），阅读器的 HTML 引擎也是这么理解的，按它修等于把阅读器本来就会看到的结构写成合法 XML；
//! xochitl 等严格按 XML 读的阅读器遇到不合法的整章空白（见 docs/xochitl.md）。
//!
//! 字一个不改：文字、属性值原样（字符引用还原后再按 XML 转义），注释保留；结构按 HTML5 的规则修。写回后还不是合法 XML 的
//! 不用（调用方保留原来的）。只在 [`super::normalize`] 的配对修复没能修好时才用：能修好的书逐字节不变。

use scraper::{Html, Node};

const XHTML_NS: &str = "http://www.w3.org/1999/xhtml";
const SVG_NS: &str = "http://www.w3.org/2000/svg";
const MATHML_NS: &str = "http://www.w3.org/1998/Math/MathML";
const XLINK_NS: &str = "http://www.w3.org/1999/xlink";

fn escape_text(s: &str, out: &mut String) {
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            c => out.push(c),
        }
    }
}

fn escape_attr(s: &str, out: &mut String) {
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '"' => out.push_str("&quot;"),
            c => out.push(c),
        }
    }
}

fn qual(prefix: Option<&str>, local: &str) -> String {
    match prefix {
        Some(p) => format!("{p}:{local}"),
        None => local.to_string(),
    }
}

/// 一个元素和它的子树写成 XHTML。`parent_ns`：父元素的命名空间（换了命名空间的 `<svg>`、`<math>` 补 `xmlns`）。
fn write_node(node: ego_tree::NodeRef<Node>, parent_ns: &str, out: &mut String) {
    match node.value() {
        Node::Text(t) => escape_text(t, out),
        Node::Comment(c) => {
            // XML 注释里不能有 `--`、不能以 `-` 结尾
            let c = c.replace("--", "- -");
            out.push_str("<!--");
            out.push_str(&c);
            if c.ends_with('-') {
                out.push(' ');
            }
            out.push_str("-->");
        }
        Node::Element(el) => {
            let ns: &str = &el.name.ns;
            let name = qual(el.name.prefix.as_deref(), &el.name.local);
            out.push('<');
            out.push_str(&name);
            let mut has_xmlns = false;
            let mut has_xlink_decl = false;
            for (k, v) in el.attrs.iter() {
                let an = qual(k.prefix.as_deref(), &k.local);
                has_xmlns |= an == "xmlns";
                has_xlink_decl |= an == "xmlns:xlink";
                out.push(' ');
                out.push_str(&an);
                out.push_str("=\"");
                escape_attr(v, out);
                out.push('"');
            }
            // 根元素、换了命名空间的外来元素补 `xmlns`；子树里用到 `xlink:` 属性的 svg 补 `xmlns:xlink`
            if !has_xmlns && ns != parent_ns && [XHTML_NS, SVG_NS, MATHML_NS].contains(&ns) {
                out.push_str(&format!(" xmlns=\"{ns}\""));
            }
            if !has_xlink_decl && ns == SVG_NS && parent_ns != SVG_NS && uses_xlink(node) {
                out.push_str(&format!(" xmlns:xlink=\"{XLINK_NS}\""));
            }
            let empty = node.children().next().is_none();
            if empty && (ns != XHTML_NS || crate::html::is_void(&el.name.local)) {
                out.push_str("/>");
                return;
            }
            out.push('>');
            for c in node.children() {
                write_node(c, ns, out);
            }
            out.push_str("</");
            out.push_str(&name);
            out.push('>');
        }
        _ => {}
    }
}

fn uses_xlink(node: ego_tree::NodeRef<Node>) -> bool {
    node.descendants().any(|d| match d.value() {
        Node::Element(el) => el.attrs.keys().any(|k| &*k.ns == XLINK_NS),
        _ => false,
    })
}

/// 交给 HTML5 解析前先去掉只有 XML 才认的写法（2026-10-09 审计）：开头的 `<?xml …?>` 声明（HTML5 当成伪注释，写回多出一行
/// `<!--?xml …?-->`）；CDATA 段（HTML5 正文里当成注释——里面的字会丢；`<style>`、`<script>` 里原样当文字，写回成 `&lt;![CDATA[`，
/// 第一条样式规则作废）：正文里的换成转义后的文字，`<style>`、`<script>` 里的去掉 CDATA 记号、内容原样。
fn strip_xml_only(text: &str) -> std::borrow::Cow<'_, str> {
    let mut t = text.trim_start_matches('\u{feff}').trim_start();
    if t.starts_with("<?xml") {
        if let Some(e) = t.find("?>") {
            t = &t[e + 2..];
        }
    }
    if !t.contains("<![CDATA[") {
        return std::borrow::Cow::Borrowed(t);
    }
    let mut out = String::with_capacity(t.len());
    let mut rest = t;
    while let Some(i) = rest.find("<![CDATA[") {
        out.push_str(&rest[..i]);
        let body = &rest[i + 9..];
        let (content, after) = match body.find("]]>") {
            Some(e) => (&body[..e], &body[e + 3..]),
            None => (body, ""),
        };
        let lower = out.to_ascii_lowercase();
        let open = |tag: &str| lower.rfind(&format!("<{tag}")).map(|o| lower.rfind(&format!("</{tag}")).is_none_or(|c| c < o)).unwrap_or(false);
        if open("style") || open("script") {
            out.push_str(content);
        } else {
            escape_text(content, &mut out);
        }
        rest = after;
    }
    out.push_str(rest);
    std::borrow::Cow::Owned(out)
}

/// 按 HTML5 解析算法重新解析整份 XHTML、写回成合法的 XHTML；写回的不是合法 XML 时 `None`。
pub(super) fn reparse(text: &str) -> Option<String> {
    let doc = Html::parse_document(&strip_xml_only(text));
    let mut out = String::with_capacity(text.len() + 256);
    out.push_str("<?xml version=\"1.0\" encoding=\"utf-8\"?>\n<!DOCTYPE html>\n");
    for c in doc.tree.root().children() {
        match c.value() {
            Node::Element(_) => write_node(c, "", &mut out),
            Node::Comment(_) => {
                write_node(c, "", &mut out);
                out.push('\n');
            }
            _ => {}
        }
    }
    out.push('\n');
    super::normalize::well_formed_xml(&out).then_some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text_of(s: &str) -> String {
        let doc = Html::parse_document(s);
        doc.root_element().text().collect::<String>().split_whitespace().collect()
    }

    #[test]
    fn xml_declaration_and_cdata_survive() {
        let src = "<?xml version=\"1.0\" encoding=\"utf-8\"?>\n<html xmlns=\"http://www.w3.org/1999/xhtml\"><head><style><![CDATA[ div > p { color: red } ]]></style></head><body><p><i>甲</p></i><p><![CDATA[a < b & c]]></p></body></html>";
        let out = reparse(src).unwrap();
        assert!(!out.contains("<!--?xml"), "声明不变成注释：{out}");
        assert!(out.contains("<style> div &gt; p { color: red } </style>"), "样式里去掉 CDATA 记号：{out}");
        assert!(out.contains("<p>a &lt; b &amp; c</p>"), "正文 CDATA 的字不丢：{out}");
    }

    #[test]
    fn misnested_and_unclosed_fixed_by_html5_rules() {
        let src = "<html xmlns=\"http://www.w3.org/1999/xhtml\"><head><title>t</title></head><body><p><i>甲</p></i><p>乙<p>丙 &amp; 丁</body></html>";
        let out = reparse(src).unwrap();
        assert!(out.contains("<p><i>甲</i></p>"), "交叉嵌套按收养算法修：{out}");
        assert!(out.contains("<p>乙</p><p>丙 &amp; 丁</p>"), "<p> 遇到下一个 <p> 隐含结束：{out}");
        assert_eq!(text_of(src), text_of(&out), "字一个不改");
    }

    #[test]
    fn svg_namespaces_voids_comments_and_attrs_kept() {
        let src = r#"<html xmlns="http://www.w3.org/1999/xhtml" xmlns:epub="http://www.idpf.org/2007/ops"><head><title>t</title><link rel="stylesheet" href="a.css"><style>p > a { color: red }</style></head><body epub:type="bodymatter"><!-- 注--释 --><div><svg viewBox="0 0 10 10"><image xlink:href="c.jpg" width="10" height="10"></image></svg><br><img src="a&amp;b.png" alt='"x"'></div><p>没关</body></html>"#;
        let out = reparse(src).unwrap();
        for want in [
            r#"<html xmlns="http://www.w3.org/1999/xhtml" xmlns:epub="http://www.idpf.org/2007/ops">"#,
            r#"<link rel="stylesheet" href="a.css"/>"#,
            "<style>p &gt; a { color: red }</style>",
            r#"<body epub:type="bodymatter">"#,
            "<!-- 注- -释 -->",
            r#"<svg viewBox="0 0 10 10" xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink"><image xlink:href="c.jpg" width="10" height="10"/></svg>"#,
            "<br/>",
            r#"<img src="a&amp;b.png" alt="&quot;x&quot;"/>"#,
            "<p>没关</p></body></html>",
        ] {
            assert!(out.contains(want), "缺 {want}\n{out}");
        }
    }
}
