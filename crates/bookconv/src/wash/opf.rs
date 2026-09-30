//! OPF 的读与改：定位 OPF、解出 manifest/spine/nav/ncx、标识符/书名；往 `</manifest>`/`</metadata>` 前插入、按条件删 manifest 项
//! （连同 spine 引用）、封面声明。全书改 OPF 的地方（清洗层、优化器、漫画标签、`ebook-meta`）都走这里。
//!
//! 都基于 `crate::html` 的标签扫描：单双引号、注释、命名空间前缀（`<opf:manifest>`、`<opf:item>`）一视同仁；
//! 插入的元素跟着所在容器的前缀走（前缀 OPF 里写 `<opf:item>`，免得落到空命名空间）。
use super::*;

pub(super) fn find_opf(entries: &[Entry]) -> Option<usize> {
    // container.xml 指向优先，否则第一个 .opf
    if let Some(c) = entries.iter().find(|e| e.name == "META-INF/container.xml") {
        let t = String::from_utf8_lossy(&c.data);
        let full = html::tags(&t).filter(|g| g.is_start() && g.is("rootfile")).find_map(|g| tag_attr(&t[g.start..g.end], "full-path").map(posix_norm));
        if let Some(p) = full {
            if let Some(i) = entries.iter().position(|e| e.name == p) {
                return Some(i);
            }
        }
    }
    entries.iter().position(|e| e.name.to_ascii_lowercase().ends_with(".opf"))
}

// ───────────────────────── OPF 视图 ─────────────────────────

pub struct Opf {
    pub index: usize,
    pub dir: String,
    /// manifest id → zip 路径
    pub items: HashMap<String, String>,
    /// spine 顺序的 zip 路径
    pub spine: Vec<String>,
    pub nav_doc: Option<String>,
    pub ncx: Option<String>,
}

/// 第一个名为 `name`（不分大小写、完整属性名）的属性值（原文；双引号、单引号、无引号都认）。`text` 可以是一个标签、
/// 只有属性的片段，也可以是整份文档（取第一个带这个属性的开标签，如 container.xml 的 `full-path`）。
/// OPF manifest 项、`<meta name="cover">`、container.xml 的 `<rootfile>` 等共用。
pub fn tag_attr<'a>(text: &'a str, name: &str) -> Option<&'a str> {
    if !text.trim_start().starts_with('<') {
        return html::attr_value(text, name);
    }
    html::tags(text).filter(|t| t.is_start()).find_map(|t| html::attr_value(&text[t.start..t.end], name))
}

/// OPF manifest 的一项（`href` 未解码、未解析成 zip 路径；属性重复时后者为准，与此前 HashMap 收集的行为一致）。
pub struct ManifestItem<'a> {
    /// 整个 `<item …>` 标签原文（改写 OPF 时按原文定位）。
    pub tag: &'a str,
    /// `tag` 在 OPF 文本里的起点（就地改写用）。
    pub pos: usize,
    pub id: &'a str,
    pub href: &'a str,
    pub media_type: &'a str,
    pub properties: &'a str,
}

impl ManifestItem<'_> {
    /// 这一项的 zip 路径：`href` 原文先还原字符引用、再百分号解码，相对 OPF 目录解析（2026-09-30 审计：此前不还原，
    /// `href="a&amp;b.xhtml"` 对不上条目 `a&b.xhtml`）。
    pub fn path(&self, opf_dir: &str) -> String {
        resolve(opf_dir, &percent_decode(&crate::util::xml_unescape(self.href)))
    }
}

/// OPF 文本里全部带 `id` 与 `href` 的 manifest 项（文档序）。`parse_opf`、`ensure_cover_declared`、占位封面探测共用。
pub fn manifest_items(opf_text: &str) -> Vec<ManifestItem<'_>> {
    html::tags(opf_text)
        .filter(|t| t.is_start() && is_local(t.name, "item"))
        .filter_map(|t| {
            let tag = &opf_text[t.start..t.end];
            let (mut id, mut href, mut media_type, mut properties) = (None, None, "", "");
            for a in html::attrs(tag) {
                match a.name.to_ascii_lowercase().as_str() {
                    "id" => id = Some(a.value),
                    "href" => href = Some(a.value),
                    "media-type" => media_type = a.value,
                    "properties" => properties = a.value,
                    _ => {}
                }
            }
            Some(ManifestItem { tag, pos: t.start, id: id?, href: href?, media_type, properties })
        })
        .collect()
}

pub fn parse_opf(entries: &[Entry]) -> Option<Opf> {
    let index = find_opf(entries)?;
    let dir = dir_of(&entries[index].name).to_string();
    let text = String::from_utf8_lossy(&entries[index].data);
    let mut items = HashMap::new();
    let mut nav_doc = None;
    let mut ncx = None;
    for it in manifest_items(&text) {
        let path = it.path(&dir);
        if it.properties.split_whitespace().any(|x| x == "nav") {
            nav_doc = Some(path.clone());
        }
        if it.media_type.contains("dtbncx") {
            ncx = Some(path.clone());
        }
        items.insert(it.id.to_string(), path);
    }
    let spine: Vec<String> = html::tags(&text)
        .filter(|t| t.is_start() && is_local(t.name, "itemref"))
        .filter_map(|t| tag_attr(&text[t.start..t.end], "idref").and_then(|id| items.get(id)).cloned())
        .collect();
    Some(Opf { index, dir, items, spine, nav_doc, ncx })
}

/// OPF 的 `unique-identifier` 实际取值（`<package unique-identifier="X">` 指向的那个
/// `<dc:identifier id="X">` 元素的文本内容）。EPUB2 规范要求 `toc.ncx` 的 `dtb:uid` 跟这个值
/// 完全一致——真机《疯探》坐实：这本"番茄小说 EPUB Generator"产物的 `toc.ncx` navMap 结构完全
/// 正确（94 条 navPoint 全部可达），但 `dtb:uid` 是生成器随手写的另一个 uuid，跟 OPF 的
/// `dc:identifier` 对不上；reMarkable 原生目录面板遇到这种不匹配**直接不显示目录入口**（不是
/// 显示空列表），换一本 `dtb:uid` 匹配的书（《雪人》）目录入口就在。见 `fix_ncx_uid`。
pub(super) fn opf_unique_identifier(entries: &[Entry]) -> Option<String> {
    let i = find_opf(entries)?;
    let text = String::from_utf8_lossy(&entries[i].data);
    let uid_attr = html::tags(&text).find(|t| t.is_start() && is_local(t.name, "package")).and_then(|t| tag_attr(&text[t.start..t.end], "unique-identifier"))?;
    let t = html::tags(&text).find(|t| t.kind == html::TagKind::Open && t.is("dc:identifier") && tag_attr(&text[t.start..t.end], "id") == Some(uid_attr))?;
    let close = html::find_close(&text, t.end, "dc:identifier")?;
    Some(crate::util::xml_unescape(text[t.end..close.start].trim()).into_owned())
}

/// OPF 里的 Dublin Core 元数据（纯文本：标签去掉、字符引用还原）。多值的只有作者；其余取第一个非空值。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct OpfDc {
    pub title: String,
    pub creators: Vec<String>,
    pub publisher: String,
    pub language: String,
    pub date: String,
    pub description: String,
}

/// 从 OPF 文本读 [`OpfDc`]。书库入库、自动目录标题共用。
/// 按标签扫（注释里的、自闭合的 `<dc:title/>` 不算；属性值里的 `>` 不截断）：`<dc:X …>` 到它后面第一个这六种之一的闭合标签。
pub fn opf_dc(opf: &str) -> OpfDc {
    fn local(name: &str) -> Option<&str> {
        name.strip_prefix("dc:").filter(|n| ["title", "creator", "publisher", "language", "date", "description"].contains(n))
    }
    let mut dc = OpfDc::default();
    let mut tags = html::tags(opf);
    while let Some(t) = tags.next() {
        let Some(kind) = local(t.name).filter(|_| t.kind == html::TagKind::Open) else { continue };
        let Some(close) = tags.by_ref().find(|c| c.kind == html::TagKind::Close && local(c.name).is_some()) else { break };
        let v = plain_text(&opf[t.end..close.start]);
        if v.is_empty() {
            continue;
        }
        let slot = match kind {
            "creator" => {
                dc.creators.push(v);
                continue;
            }
            "title" => &mut dc.title,
            "publisher" => &mut dc.publisher,
            "language" => &mut dc.language,
            "date" => &mut dc.date,
            _ => &mut dc.description,
        };
        if slot.is_empty() {
            *slot = v;
        }
    }
    dc
}

/// OPF `<dc:title>` 的纯文本内容，取不到时兜底"目录"。
pub(super) fn opf_book_title(entries: &[Entry], opf_index: usize) -> String {
    Some(opf_dc(&String::from_utf8_lossy(&entries[opf_index].data)).title).filter(|t| !t.is_empty()).unwrap_or_else(|| "目录".into())
}

// ───────────────────────── 改写 OPF ─────────────────────────

/// 元素名去掉命名空间前缀后是不是 `local`（不分大小写）：`item`、`opf:item` 都算 `item`。
pub fn is_local(name: &str, local: &str) -> bool {
    name.rsplit(':').next().is_some_and(|n| n.eq_ignore_ascii_case(local))
}

/// 第一个 `</local>`（允许前缀）的起点与前缀（`"opf:"` 或 `""`）。
fn container_close<'a>(opf: &'a str, local: &str) -> Option<(usize, &'a str)> {
    html::tags(opf).find(|t| t.kind == html::TagKind::Close && is_local(t.name, local)).map(|t| (t.start, &t.name[..t.name.len() - local.len()]))
}

/// 要新加的一条 manifest 项。`href` 是属性值原文（相对 OPF 目录、已百分号编码，见 `epubzip::href_to`），写入时再 XML 转义。
pub struct NewItem<'a> {
    pub id: &'a str,
    pub href: &'a str,
    pub media_type: &'a str,
    /// 空 = 不写 `properties`。
    pub properties: &'a str,
}

/// 把 `items` 插到 `</manifest>` 前（元素名跟着 manifest 的前缀）。没有 `</manifest>` → `None`。
pub fn insert_manifest_items(opf: &str, items: &[NewItem]) -> Option<String> {
    let (at, prefix) = container_close(opf, "manifest")?;
    let mut s = String::new();
    for it in items {
        s.push_str(&format!(r#"<{prefix}item id="{}" href="{}" media-type="{}""#, xml_escape(it.id), xml_escape(it.href), xml_escape(it.media_type)));
        if !it.properties.is_empty() {
            s.push_str(&format!(r#" properties="{}""#, xml_escape(it.properties)));
        }
        s.push_str("/>");
    }
    Some(format!("{}{s}{}", &opf[..at], &opf[at..]))
}

/// 把一段元数据插到 `</metadata>` 前。`xml` 里的 `<meta `/`</meta>` 跟着 metadata 的前缀改成 `<opf:meta `/`</opf:meta>`（`dc:` 元素有自己的命名空间，不动）。
/// 没有 `</metadata>` → `None`。
pub fn insert_metadata(opf: &str, xml: &str) -> Option<String> {
    let (at, prefix) = container_close(opf, "metadata")?;
    let xml = if prefix.is_empty() { Cow::Borrowed(xml) } else { Cow::Owned(xml.replace("<meta ", &format!("<{prefix}meta ")).replace("</meta>", &format!("</{prefix}meta>"))) };
    Some(format!("{}{xml}{}", &opf[..at], &opf[at..]))
}

/// 元素的终点：自闭合就是标签本身；开标签后面紧跟（中间只有空白）自己的闭合标签时连它一起（`<item …></item>`）。
pub(crate) fn element_end(text: &str, t: &html::Tag) -> usize {
    if t.kind == html::TagKind::Open {
        let ws = text[t.end..].len() - text[t.end..].trim_start().len();
        if let Some(c) = html::tags_in(text, t.end + ws, text.len()).next() {
            if c.kind == html::TagKind::Close && c.start == t.end + ws && c.name.eq_ignore_ascii_case(t.name) {
                return c.end;
            }
        }
    }
    t.end
}

/// 删掉 `pred` 选中的 manifest 项，连同 spine 里引用它们的 `<itemref>`（各自连同后面的空白）。一个都没选中 → `None`。
pub fn remove_items(opf: &str, pred: impl Fn(&ManifestItem) -> bool) -> Option<String> {
    let ids: HashSet<&str> = manifest_items(opf).iter().filter(|it| pred(it)).map(|it| it.id).collect();
    if ids.is_empty() {
        return None;
    }
    let mut edits: Vec<(usize, usize, String)> = Vec::new();
    for t in html::tags(opf).filter(|t| t.is_start()) {
        let key = if is_local(t.name, "item") {
            "id"
        } else if is_local(t.name, "itemref") {
            "idref"
        } else {
            continue;
        };
        if !html::attr_value(&opf[t.start..t.end], key).is_some_and(|v| ids.contains(v)) {
            continue;
        }
        let end = element_end(opf, &t);
        let ws = opf[end..].len() - opf[end..].trim_start().len();
        edits.push((t.start, end + ws, String::new()));
    }
    Some(html::apply_edits(opf, edits))
}

/// `<meta name="cover" …>`（任意引号、允许前缀）：(起点, 终点——含紧跟的 `</meta>`, `content` 值)，文档序。
pub fn cover_meta_tags(opf: &str) -> Vec<(usize, usize, &str)> {
    html::tags(opf)
        .filter(|t| t.is_start() && is_local(t.name, "meta"))
        .filter_map(|t| {
            let tag = &opf[t.start..t.end];
            (html::attr_value(tag, "name")? == "cover").then(|| (t.start, element_end(opf, &t), html::attr_value(tag, "content").unwrap_or("")))
        })
        .collect()
}

/// manifest 项是不是图片：`media-type` 是 `image/…`，或 href 是常见位图扩展名。
pub fn is_image_item(it: &ManifestItem) -> bool {
    it.media_type.starts_with("image/") || is_image_ext(html::split_href(it.href).0)
}

/// OPF 声明的封面图：`<meta name="cover" content="id">` 指向的图片项优先，其次带 `properties="cover-image"` 的图片项。
/// 声明了但指向的不是图片（Calibre 产物的 `content="cover.txt"`）不算。
pub fn declared_cover(opf: &str) -> Option<ManifestItem<'_>> {
    let mut items = manifest_items(opf);
    let by_meta = cover_meta_tags(opf).into_iter().find_map(|(_, _, id)| items.iter().position(|i| i.id == id && is_image_item(i)));
    let pos = by_meta.or_else(|| items.iter().position(|i| is_image_item(i) && i.properties.split_whitespace().any(|p| p == "cover-image")))?;
    Some(items.swap_remove(pos))
}

/// 封面兜底：前 `max_pages` 个 spine 页（跳过导航页）里第一张图（`<img src>`、SVG `<image href>`）。`in_manifest`：只认能对上
/// manifest 图片项的（要把它声明成封面时必须如此）；否则 manifest 漏登记的图也算（只是读出封面图）。
/// `read(zip 路径)` 取页面文本。返回图片的 zip 路径。
pub fn first_spine_image(opf: &str, opf_dir: &str, max_pages: usize, in_manifest: bool, mut read: impl FnMut(&str) -> Option<String>) -> Option<String> {
    let items = manifest_items(opf);
    let path_of = |it: &ManifestItem| it.path(opf_dir);
    let images: HashSet<String> = items.iter().filter(|i| is_image_item(i)).map(path_of).collect();
    let by_id: HashMap<&str, &ManifestItem> = items.iter().map(|i| (i.id, i)).collect();
    let pages = html::tags(opf)
        .filter(|t| t.is_start() && is_local(t.name, "itemref"))
        .filter_map(|t| by_id.get(tag_attr(&opf[t.start..t.end], "idref")?).copied())
        .filter(|it| !it.properties.split_whitespace().any(|p| p == "nav"))
        .take(max_pages);
    for it in pages {
        let page = path_of(it);
        let Some(text) = read(&page) else { continue };
        for t in html::tags(&text).filter(|t| t.is_start() && (is_local(t.name, "img") || is_local(t.name, "image"))) {
            let tag = &text[t.start..t.end];
            let Some(v) = ["src", "xlink:href", "href"].iter().find_map(|a| html::attr_value(tag, a)) else { continue };
            let (p, _) = html::split_href(v);
            if p.is_empty() || html::is_external(p) {
                continue;
            }
            let path = resolve(dir_of(&page), &percent_decode(&crate::util::xml_unescape(p)));
            if images.contains(&path) || (!in_manifest && is_image_ext(&path)) {
                return Some(path);
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn insert_follows_container_prefix() {
        let opf = r#"<opf:package><opf:metadata><dc:title>t</dc:title></opf:metadata><opf:manifest><opf:item id="a" href="a.xhtml" media-type="application/xhtml+xml"/></opf:manifest></opf:package>"#;
        let out = insert_manifest_items(opf, &[NewItem { id: "n", href: "x&y.css", media_type: "text/css", properties: "" }]).unwrap();
        assert!(out.contains(r#"<opf:item id="n" href="x&amp;y.css" media-type="text/css"/></opf:manifest>"#), "{out}");
        let out = insert_metadata(&out, r#"<meta name="cover" content="c"/><dc:subject>漫画</dc:subject><meta property="dcterms:modified">x</meta>"#).unwrap();
        assert!(out.contains(r#"<opf:meta name="cover" content="c"/><dc:subject>漫画</dc:subject><opf:meta property="dcterms:modified">x</opf:meta></opf:metadata>"#), "{out}");
        assert_eq!(insert_manifest_items("<package/>", &[]), None);
    }

    #[test]
    fn remove_items_handles_single_quotes_and_spine_refs() {
        let opf = "<package><manifest><item id='c0' href='c0.xhtml' media-type='application/xhtml+xml'></item>\n  <item id='c1' href='c1.xhtml' media-type='application/xhtml+xml'/></manifest><spine><itemref idref='c0'/>\n<itemref idref=\"c1\"/></spine></package>";
        let out = remove_items(opf, |it| it.href == "c0.xhtml").unwrap();
        assert_eq!(out, "<package><manifest><item id='c1' href='c1.xhtml' media-type='application/xhtml+xml'/></manifest><spine><itemref idref=\"c1\"/></spine></package>");
        assert_eq!(remove_items(opf, |_| false), None);
    }

    #[test]
    fn declared_cover_and_first_spine_image() {
        let opf = r#"<package><metadata><meta content='t' name='cover'/></metadata><manifest><item id="t" href="cover.txt" media-type="text/plain"/><item id="c" href="i/c.jpg" media-type="image/jpeg" properties="cover-image"/><item id="p" href="p.xhtml" media-type="application/xhtml+xml"/><item id="q" href="i/q.png" media-type="image/png"/></manifest><spine><itemref idref="p"/></spine></package>"#;
        assert_eq!(declared_cover(opf).map(|i| i.id), Some("c"), "meta 指向 txt 不算，退到 cover-image 属性");
        let page = r#"<html><body><img src="ext/none.jpg"/><svg><image xlink:href='i/q.png'/></svg></body></html>"#;
        assert_eq!(first_spine_image(opf, "OEBPS", 12, true, |p| (p == "OEBPS/p.xhtml").then(|| page.to_string())), Some("OEBPS/i/q.png".into()));
    }
}
