//! OPF 视图：定位 OPF、解出 manifest/spine/nav/ncx，以及 OPF 里的标识符/书名。
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
    #[allow(dead_code)]
    pub items: HashMap<String, String>,
    /// spine 顺序的 zip 路径
    pub spine: Vec<String>,
    pub nav_doc: Option<String>,
    #[allow(dead_code)]
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

/// OPF 文本里全部带 `id` 与 `href` 的 manifest 项（文档序）。`parse_opf`、`ensure_cover_declared`、占位封面探测共用。
pub fn manifest_items(opf_text: &str) -> Vec<ManifestItem<'_>> {
    html::tags(opf_text)
        .filter(|t| t.is_start() && t.is("item"))
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

/// `<meta name="cover" …>` 标签（`ensure_cover_declared` 与占位封面探测共用）。
pub fn cover_meta_re() -> &'static Regex {
    static META: OnceLock<Regex> = OnceLock::new();
    META.get_or_init(|| Regex::new(r#"(?s)<meta\b[^>]*\bname\s*=\s*"cover"[^>]*?/?>"#).unwrap())
}

pub fn parse_opf(entries: &[Entry]) -> Option<Opf> {
    let index = find_opf(entries)?;
    let dir = dir_of(&entries[index].name).to_string();
    let text = String::from_utf8_lossy(&entries[index].data);
    let mut items = HashMap::new();
    let mut nav_doc = None;
    let mut ncx = None;
    for it in manifest_items(&text) {
        let path = resolve(&dir, &percent_decode(it.href));
        if it.properties.split_whitespace().any(|x| x == "nav") {
            nav_doc = Some(path.clone());
        }
        if it.media_type.contains("dtbncx") {
            ncx = Some(path.clone());
        }
        items.insert(it.id.to_string(), path);
    }
    let spine: Vec<String> = html::tags(&text)
        .filter(|t| t.is_start() && t.is("itemref"))
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
    let uid_attr = html::tags(&text).find(|t| t.is_start() && t.is("package")).and_then(|t| tag_attr(&text[t.start..t.end], "unique-identifier"))?;
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

/// 从 OPF 文本读 [`OpfDc`]。书库入库、AZW3 写出、自动目录标题共用。
pub fn opf_dc(opf: &str) -> OpfDc {
    static DC: OnceLock<Regex> = OnceLock::new();
    let re = DC.get_or_init(|| {
        Regex::new(r#"(?s)<dc:(title|creator|publisher|language|date|description)\b[^>]*>(.*?)</dc:(?:title|creator|publisher|language|date|description)\s*>"#).unwrap()
    });
    let mut dc = OpfDc::default();
    for c in re.captures_iter(opf) {
        let v = plain_text(&c[2]);
        if v.is_empty() {
            continue;
        }
        let slot = match &c[1] {
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
