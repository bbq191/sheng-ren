//! 脚注：识别 noteref/aside/duokan 各形态，收集被引用的注释块，就地关联重排（`preserve_relink_footnotes`）。
use super::*;

/// 注释索引的键：(注释块所在文件的 zip 路径, id)。只按 id 做键时，不清洗（`--no-wash`）的书两章都有 `id="fn1"` 就只剩一条
/// （2026-09-28 审计）。
pub type NoteKey = (String, String);

/// 文档里的一个 `<a …>…</a>`（到它后面第一个 `</a>`）：在全部标签序列里的下标（开、闭）与字节范围。
struct AElem {
    /// 开标签在 `tags` 里的下标。
    open_ix: usize,
    close_ix: usize,
    start: usize,
    open_end: usize,
    close_start: usize,
    end: usize,
}

/// 全部 `<a>` 元素（不重叠，文档序）。属性值里的 `>`、注释里的 `<a>` 都不会弄错（`html::tags`）。
fn a_elems(tags: &[html::Tag]) -> Vec<AElem> {
    let mut out = Vec::new();
    let mut k = 0;
    while k < tags.len() {
        let t = &tags[k];
        if t.kind == html::TagKind::Open && t.is("a") {
            if let Some(m) = (k + 1..tags.len()).find(|&m| tags[m].kind == html::TagKind::Close && tags[m].is("a")) {
                out.push(AElem { open_ix: k, close_ix: m, start: t.start, open_end: t.end, close_start: tags[m].start, end: tags[m].end });
                k = m + 1;
                continue;
            }
        }
        k += 1;
    }
    out
}

/// `<a>` 开标签是不是 noteref：`epub:type`/`type` 的值里有 `noteref`（任意引号）。
fn is_noteref(open: &str) -> bool {
    html::attrs(open).iter().any(|a| (a.is("type") || a.name.to_ascii_lowercase().ends_with(":type")) && a.value.split_whitespace().any(|v| v == "noteref"))
}

/// 链接指向的注释键：锚点先百分号解码（NCX/正文常把中文锚点写成 `%E6%B3%A8`），路径按本章解析（`#x` 就是本章）。
fn note_key(name: &str, href: &str) -> Option<NoteKey> {
    let (path, frag) = crate::epubzip::resolve_href(name, href);
    let frag = frag.filter(|f| !f.is_empty())?;
    Some((path, html::frag_id(frag).into_owned()))
}
/// 微读 **duokan 图片脚注**标记：`<sup..><a href="#frag">&lt;img ... class="duokan-footnote.." ../&gt;</a></sup>`。
/// 与 qqreader 版不同：注释块**已在同文件** `<p id="frag">`（前向锚有效），且标记里的 `<img>` 被
/// **实体转义**成字面文本、又是微读 CDN 远程图 → 设备离线渲染成一坨死文本、点不动。故只需把标记
/// 内容换成干净可点上标数字、保留 `href="#frag"`（注释块原地不动即可跳）。group1=`<a>` 属性、
/// group2=转义 img 内层（含 class/alt）。
pub(super) fn duokan_footnote_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| {
        Regex::new(r#"(?si)<sup\b[^>]*>\s*<a\b([^>]*)>\s*&lt;img\b(.*?)/?&gt;\s*</a>\s*</sup>"#).unwrap()
    })
}
/// Calibre 洗过的 duokan 形态（`ebook-convert` AZW3/EPUB→EPUB 后）：标记里的 `<img>` 是**真标签**（非实体
/// 转义）、src 已是本地图（图片内容仅是"注释N"小图，点不动、设备上一坨小块），`<a>` 上带回链落点
/// `id="c_X_Y"`。group1=`<a>` 属性、group2=img 属性。与转义版共用同一替换逻辑。
pub(super) fn duokan_footnote_img_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| {
        Regex::new(r#"(?si)<sup\b[^>]*>\s*<a\b([^>]*)>\s*<img\b([^>]*?)/?>\s*</a>\s*</sup>"#).unwrap()
    })
}
/// 把**指向本文件**的带文件名 href（`href="part0004.html#frag"` 写在 part0004.html 里，Calibre
/// `ebook-convert` 的固定写法）归一成裸 `#frag`。reMarkable 只跟同文件裸锚；且优化器各 pass
/// （`break_footnote_cycles` 只认裸锚、`referenced_note_frags` 把带路径的一律当跨文件尾注搬走）
/// 都以"裸锚=同文件"为前提，不归一会把同章脚注误当跨文件处理（标记丢 id、注释块被搬成无效嵌套）。
/// `own` 传本文件在 zip 里的完整路径时按路径解析比对（2026-09-27 审计：只比文件名会把 `a/x.html` 里指向
/// `../b/x.html#f` 的链接误当本文件）；只传文件名（不含 `/`）时退回按路径末段比对。非本文件、无 fragment 的 href 不动。
pub fn normalize_self_hrefs(html: &str, own: &str) -> String {
    if own.is_empty() {
        return html.to_string();
    }
    let own_dir = own.contains('/').then(|| crate::epubzip::dir_of(own));
    html::edit_attrs(html, &["href"], |_, a| {
        let (path, frag) = html::split_href(a.value);
        let Some(frag) = frag else { return Edit::Keep };
        if path.is_empty() || html::is_external(path) {
            return Edit::Keep;
        }
        let path = crate::epubzip::percent_decode(path);
        let same = match own_dir {
            Some(d) => crate::epubzip::resolve(d, &path) == own,
            None => path.rsplit('/').next() == Some(own),
        };
        if same { Edit::Set(format!("#{frag}")) } else { Edit::Keep }
    })
    .into_owned()
}
/// 从（转义的）img 内层抽注释序号：`alt="注释12"` → `12`。取不到返回 None（调用方用章内计数兜底）。
pub(super) fn duokan_note_num(img_inner: &str) -> Option<String> {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r#"(?i)alt="[^"]*?(\d+)"#).unwrap())
        .captures(img_inner)
        .map(|c| c.get(1).unwrap().as_str().to_string())
}
/// duokan 注释块里的**回链** `<a ... href="#c_X_Y">注释文字</a>`（跳回标记的 `c_X_Y` 锚，
/// 但该 id 在书里根本不存在=悬空）。**必须去链成纯文本**：reMarkable 链接索引器遇到"注释块内含
/// 出链"会把整个脚注对判为互指对而**整对丢弃 → 正向 marker 也点不动**（同 break_footnote_cycles
/// 的病理，但那里只拆真 2-环、够不到这悬空回链）。去链后注释跟 qqreader 版一样纯文本、正向恢复可跳。
pub(super) fn duokan_backlink_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r##"(?si)<a\b[^>]*\bhref="#c_\d+_\d+"[^>]*>(.*?)</a>"##).unwrap())
}
/// duokan 注释块是**无效嵌套 `<p>`**：`<p id="a_X_Y"><p class="pfootnotetext">注释文字</p></p>`。
/// 多条注释成簇相邻时，reMarkable 渲染无效嵌套 `<p>` 会自动闭合外层 + 杂散 `</p>` → **吞/合并其中一条**
/// （"缺一条"根因）。拍平成合法单段 `<p id="a_X_Y">注释文字</p>`，锚点保留、每条独立可跳。
/// group1=id（a_X_Y）、group2=内层 `<p>` 属性、group3=注释文字。
pub(super) fn duokan_note_block_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| {
        Regex::new(r##"(?si)<p\b[^>]*\bid="(a_\d+_\d+)"[^>]*>\s*<p\b([^>]*)>(.*?)</p>\s*</p>"##).unwrap()
    })
}
/// 属性串里 `href` 的锚点（`href="x.html#f"` / `href='#f'` → `f`）。
pub(super) fn href_fragment(attrs: &str) -> Option<String> {
    html::attr_value(attrs, "href").and_then(|v| html::split_href(v).1).filter(|f| !f.is_empty()).map(str::to_string)
}
/// aside 注释内层的回链 `href="partXXXX.html#frag"` 去跨文件前缀成同章 `href="#frag"`
/// （内联后 marker 与注释同章，回链落点也在该章 → 同章可返回）。无 fragment 的 href 不动。
pub(super) fn deprefix_footnote_hrefs(s: &str) -> String {
    html::edit_attrs(s, &["href"], |_, a| match html::split_href(a.value) {
        (p, Some(f)) if !p.is_empty() => Edit::Set(format!("#{f}")),
        _ => Edit::Keep,
    })
    .into_owned()
}

/// 注释正文外层包一层带 `id` 落点的标签：用 `<div>` 不用 `<p>`。`collect_footnote_notes` 只搬
/// aside/li/div 源块的**内层** html，源块自带 `<p>`（甚至嵌套块级标签）很常见——`<p id="frag">…</p>`
/// 套出 `<p><p>…</p></p>` 是非法内容模型（`<p>` 不能合法含块级子元素），此前无防护代码也无样本验证过
/// 真机渲染表现；`<div>` 天然兼容块级/内联两种子内容，同样能挂 `id` 当锚点落点，零风险替换。
///
/// 弹窗模式（`popup`）用 `<aside epub:type="footnote">`：KOReader 靠它认出注释（弹窗显示；配合"隐藏非线性内容"还能不在章末重复显示）。
/// 每条都带 `eink-note` 类：样式表里 `page-break-inside:avoid`，一条注释不被拆到两页（长过一页的阅读器照常断开）。
fn footnote_block(frag: &str, inner: &str, popup: bool) -> String {
    if popup {
        format!("<aside epub:type=\"footnote\" id=\"{frag}\" class=\"eink-note\">{inner}</aside>")
    } else {
        format!("<div id=\"{frag}\" class=\"eink-note\">{inner}</div>")
    }
}

/// 标号里的 `<img>`（多看等书用小图标当注释标号）加上 `eink-noteicon` 类：样式表把它限成一个字高
/// （xochitl、KOReader 对没写宽高的 `<img>` 都按图片本身的像素画，80×80 的图标在正文里撑成一大块，真机《甲午：摇摆的战争》）。
fn mark_note_icons(content: &str) -> String {
    let mut edits = Vec::new();
    for t in html::tags(content) {
        if t.kind == html::TagKind::Close || !t.is("img") {
            continue;
        }
        let open = &content[t.start..t.end];
        let new = match html::attrs(open).into_iter().find(|a| a.is("class")) {
            Some(a) => html::set_attr(open, "class", &format!("{} eink-noteicon", a.value)),
            None => format!("<img class=\"eink-noteicon\"{}", &open[4..]),
        };
        edits.push((t.start, t.end, new));
    }
    if edits.is_empty() {
        content.to_string()
    } else {
        html::apply_edits(content, edits)
    }
}

/// 同文件链接 `<a href="#x">` 里**只有图、没有文字**，而且链接或图带注释类（`note`/`footnote`/`eink-noteicon`），或目标元素
/// 有注释语义（[`note_semantic`]）→ 图换成上标数字。给注释只能跳转、不认纯图链接的阅读器用（profile `note_icons = "number"`，xochitl）：
/// 2026-09-29 Move 真机（上游设备增强项目的《注释探针》）：xochitl 里**只有图、没有文字的链接点了没反应**（80/32/24/16 像素、
/// 带不带 `<sup>` 都一样），图标还按原图像素画、外链 CSS 的 `height:1em` 限不住（80×80 的图标撑成一大块，《甲午：摇摆的战争》）。
/// 编号依次取：目标注释开头写的 `[14]` 这类作者编号（[`note_number`]）→ 图标 alt 里的"注释N" → 本章顺序数。
/// `<sup>` 已包着链接时不再套一层。放在注释重排之后跑：多看标号、重排保留的标号到这里都是带 `eink-noteicon` 的图。
pub fn number_icon_note_links(html_text: &str) -> String {
    let tags: Vec<html::Tag> = html::tags(html_text).collect();
    let mut counter = 0usize;
    let mut edits: Vec<(usize, usize, String)> = Vec::new();
    for e in a_elems(&tags) {
        let open = &html_text[e.start..e.open_end];
        let Some(frag) = html::attr_value(open, "href").and_then(|h| h.strip_prefix('#')).filter(|f| !f.is_empty()) else { continue };
        let content = &html_text[e.open_end..e.close_start];
        let imgs: Vec<&str> = html::tags(content).filter(|t| t.kind != html::TagKind::Close && (t.is("img") || t.is("image"))).map(|t| &content[t.start..t.end]).collect();
        if imgs.is_empty() || !html::plain_text(content).trim().is_empty() {
            continue;
        }
        let noteish = |tag: &str| html::attr_value(tag, "class").is_some_and(|v| v.to_ascii_lowercase().contains("note"));
        let target = tags.iter().find(|t| t.is_start() && html::attr_value(&html_text[t.start..t.end], "id") == Some(frag));
        if !(noteish(open) || imgs.iter().any(|t| noteish(t)) || target.is_some_and(|t| note_semantic(&html_text[t.start..t.end]))) {
            continue;
        }
        counter += 1;
        let num = note_number(element_by_id(html_text, &tags, frag))
            .or_else(|| imgs.iter().find_map(|t| duokan_note_num(t)))
            .unwrap_or_else(|| counter.to_string());
        let in_sup = e.open_ix.checked_sub(1).is_some_and(|k| tags[k].kind == html::TagKind::Open && tags[k].is("sup") && html_text[tags[k].end..e.start].trim().is_empty());
        let label = if in_sup { xml_escape(&num) } else { format!("<sup>{}</sup>", xml_escape(&num)) };
        edits.push((e.open_end, e.close_start, label));
    }
    if edits.is_empty() {
        html_text.to_string()
    } else {
        html::apply_edits(html_text, edits)
    }
}

/// 注释正文开头写的编号：`[14]`、`［14］`、`【14】`、`(14)`、`14.`、`14、`、`14．`——作者给的编号，全书连续，比按章数出来的准。
/// 必须带括号或编号后的标点，免得把"1886年……"的年份当编号。
fn note_number(note_html: &str) -> Option<String> {
    static R: OnceLock<Regex> = OnceLock::new();
    let t = html::plain_text(note_html);
    let c = R.get_or_init(|| Regex::new(r"^\s*(?:[\[［【(（]\s*(\d{1,4})\s*[\]］】)）]|(\d{1,4})\s*[.、．])").unwrap()).captures(&t)?;
    c.get(1).or_else(|| c.get(2)).map(|m| m.as_str().to_string())
}

/// 同文件里 `id="frag"` 那个元素的内容；找不到返回空串。
fn element_by_id<'a>(html_text: &'a str, tags: &[html::Tag], frag: &str) -> &'a str {
    let Some(open) = tags.iter().find(|t| t.is_start() && html::attr_value(&html_text[t.start..t.end], "id") == Some(frag)) else { return "" };
    match html::find_close(html_text, open.end, open.name) {
        Some(close) => &html_text[open.end..close.start],
        None => "",
    }
}

/// 根元素 `<html>` 上没声明 `epub` 命名空间就补上（用了 `epub:type` 的文件不声明就不是合法 XML）。
pub(crate) fn ensure_epub_ns(doc: &str) -> String {
    let Some(t) = html::tags(doc).find(|t| t.kind != html::TagKind::Close && t.is("html")) else { return doc.to_string() };
    let open = &doc[t.start..t.end];
    if html::attrs(open).iter().any(|a| a.name == "xmlns:epub") {
        return doc.to_string();
    }
    let at = t.start + 5; // `<html` 之后
    format!("{} xmlns:epub=\"http://www.idpf.org/2007/ops\"{}", &doc[..at], &doc[at..])
}

// ===== 优化器：跨文件普通尾注收集 + 全书 id 去重 =====
// （break_footnote_cycles 管同文件裸锚点环、preserve_relink_footnotes 管 noteref+跨文件普通尾注，
//  两者产物再过 dedup_ids_in_chapter 保证 id 全书唯一——reMarkable 锚点是全书命名空间。）

/// 元素开标签是否带"注释"语义：epub:type/type/class 含 footnote|endnote|rearnote|note。
/// 只认语义确证的块 → 目录页/普通交叉引用的跨文件链接绝不会被误当尾注搬走。
pub(crate) fn note_semantic(open_tag: &str) -> bool {
    html::attrs(open_tag).iter().any(|a| {
        let n = a.name.to_ascii_lowercase();
        (n == "type" || n == "class" || n.ends_with(":type")) && a.value.to_ascii_lowercase().contains("note")
    })
}
/// href 里的**跨文件** fragment：`href="非空路径#frag"` → Some(frag)；同文件 `href="#frag"` → None。
pub(super) fn href_crossfile_fragment(attrs: &str) -> Option<String> {
    let (path, frag) = html::split_href(html::attr_value(attrs, "href")?);
    frag.filter(|f| !path.is_empty() && !f.is_empty()).map(str::to_string)
}

/// 一章里会被 [`preserve_relink_footnotes`] 搬运的注释引用：noteref marker（任意路径）+ 跨文件普通 `<a>`，解析成
/// (目标文件, id)（`name` = 本章 zip 路径）。优化器据此只搬**被引用**的注释块（[`collect_footnote_notes`] 的过滤集），
/// 未被引用的原地不动。与 `preserve_relink_footnotes` 认 marker 的口径完全一致（都走 [`a_elems`]），收集了的注释一定有人接。
pub fn referenced_note_keys(html_text: &str, name: &str) -> Vec<NoteKey> {
    let tags: Vec<html::Tag> = html::tags(html_text).collect();
    let mut out = Vec::new();
    for a in a_elems(&tags) {
        let open = &html_text[a.start..a.open_end];
        let Some(href) = html::attr_value(open, "href") else { continue };
        if html::is_external(html::split_href(href).0) {
            continue;
        }
        if is_noteref(open) || href_crossfile_fragment(open).is_some() {
            out.extend(note_key(name, href));
        }
    }
    out
}

/// 同 [`referenced_note_keys`]，只要锚点（不解析文件；测试与只看单章的调用方用）。
pub fn referenced_note_frags(html_text: &str) -> Vec<String> {
    referenced_note_keys(html_text, "").into_iter().map(|(_, id)| id).collect()
}

/// 优化器版注释收集：从一章里抽出**被引用(referenced)**的注释块——`<aside>`/`<p>`/`<li>`/`<div>` 且带注释
/// 语义([`note_semantic`])——按 id 建索引、从原位移除，交给 [`preserve_relink_footnotes`] 搬进引用
/// 它的那一章。**只搬 referenced 里的**（`referenced` = 本章里被引用的 id）：未被任何 marker 引用的块原样留在原处
/// （杜绝"移走又没人接=内容丢失"）。按元素（`html::parse_spans`）切，不再用正则（2026-09-28 审计：`<p/>` 会吞下一段、
/// 属性值里的 `>` 会截断）；外层块收了，里面的块就不再单独收；含嵌套 `<div>` 的 `<div>` 仍不搬（多半是包住整个注释区的容器）。
/// `require_semantic`：优化器传 true——对任意导入书要求块自带 footnote/note 语义（`note_semantic`），
/// 防把普通跨文件交叉引用误当尾注搬走；传 **false** 时"**id 被 marker 引用**"本身即注释的充分证据
/// （微读注释块 class 是混淆名，如 `class_s1r`，13·67 的 `<p>` 注释即此）。
pub fn collect_footnote_notes(
    html_text: &str,
    referenced: &std::collections::HashSet<String>,
    require_semantic: bool,
) -> (String, Vec<(String, String)>) {
    let mut index: Vec<(String, String)> = Vec::new();
    // 快速路径：块只有在开标签带 `id="X"` 且 X 被引用时才会被搬走。本章任何位置都找不到一个被引用的
    // `id="…"` 值 → 结果必然与原文相同，不必解析（2026-09-24 审计实测这一步占章节变换耗时的大头，绝大多数章节其实没有注释块）。
    if !mentions_referenced_id(html_text, referenced) {
        return (html_text.to_string(), index);
    }
    let spans = html::parse_spans(html_text, 0, html_text.len());
    let mut edits: Vec<(usize, usize, String)> = Vec::new();
    let mut taken_until = 0usize;
    for sp in &spans {
        if sp.open_start < taken_until || !sp.closed() || !matches!(sp.name.as_str(), "aside" | "p" | "li" | "div") {
            continue;
        }
        let open = &html_text[sp.open_start..sp.open_end];
        let inner = &html_text[sp.open_end..sp.close_start];
        if sp.name == "div" && inner.contains("<div") {
            continue; // 嵌套 div：原样保留不搬
        }
        let Some(id) = html::attr_value(open, "id").filter(|v| !v.is_empty()) else { continue };
        if referenced.contains(id) && (!require_semantic || note_semantic(open)) {
            index.push((id.to_string(), inner.to_string()));
            edits.push((sp.open_start, sp.close_end, String::new()));
            taken_until = sp.close_end;
        }
    }
    if edits.is_empty() {
        return (html_text.to_string(), index);
    }
    (html::apply_edits(html_text, edits), index)
}

/// `html` 里有没有某处 `id="X"`/`id='X'`（不分大小写、不看词边界，逐个出现位置都查）的 X 在 `referenced` 里。
/// 是开标签上 `id` 属性可能取到的值的超集，所以返回 `false` 时可以断定没有块会被搬走。
fn mentions_referenced_id(html: &str, referenced: &std::collections::HashSet<String>) -> bool {
    if referenced.is_empty() {
        return false;
    }
    let b = html.as_bytes();
    let mut i = 0;
    while i + 4 <= b.len() {
        if b[i..i + 3].eq_ignore_ascii_case(b"id=") && matches!(b[i + 3], b'"' | b'\'') {
            let (q, start) = (b[i + 3], i + 4);
            if let Some(len) = b[start..].iter().position(|&c| c == q) {
                // start/start+len 都落在 ASCII 字节（`"` 之后、`"` 处）上，必是字符边界。
                if len > 0 && referenced.contains(&html[start..start + len]) {
                    return true;
                }
            }
        }
        i += 1;
    }
    false
}

/// 修微读 **duokan 图片脚注标记**（导入优化器 `optimize_epub` 用）：
/// `<sup><a href="#frag">&lt;img class="duokan-footnote.." alt="注释N"/&gt;</a></sup>`
/// → `<a href="#frag"><sup>N</sup></a>`。duokan 的注释块**已在同文件** `<p id="frag">`（前向锚有效），
/// 故只需把"实体转义 + 远程 CDN 图 = 离线不可点"的死图标记换成干净可点上标数字、保留 href；注释块不动。
/// 非 duokan 脚注 img（`duokan-footnote` 类既不在 img 也不在外层 `<a>` 上）或跨文件锚点原样放行。
/// **Calibre 洗后形态**（2026-09-02，上游 Calibre 清洗脚本的产物）：img 是真标签+本地图、`<a>` 带 `id="c_X_Y"`、
/// 注释块是合法 `<li id="a_X_Y"><p>…<a href="#c_X_Y">`（真 2-环）——真 img 也换上标、**id 保留**，
/// 环交给前置的 `break_footnote_cycles` 拆（回链去链、id 留作落点）；合法嵌套的 li 不动。
pub fn fix_duokan_markers(html: &str) -> String {
    let mut local = 0usize;
    // 标记替换（转义 img 版 / Calibre 真 img 版共用）：保留 href 与 `<a>` 自带 id（Calibre 形态的
    // 回链落点，丢了则注释里的回链悬空 → reMarkable 判互指整对丢弃）。
    // `escaped`：标记里的 <img> 是被转义成字面文字的（微读 CDN 图，离线显示成一串代码）；否则是真的 <img> 图标。
    let mut rewrite = |a_attrs: &str, img_inner: &str, whole: &str, escaped: bool| -> String {
        // "duokan-footnote" 类名有两种真实变体：常见形态在 img 自己的 class 上（`img_inner`）；
        // 2026-09-23 真机《甲午：摇摆的战争》核实还有一种把这个类挂在外层 <a> 上、img 自己只是普通
        // `class="exs"` 图标——两种都得认，只查 img_inner 会漏判、图标原样穿透到 xochitl 按固有
        // 像素撑成巨大方块（真机坐实）。
        let is_duokan = img_inner.contains("duokan-footnote") || a_attrs.contains("duokan-footnote");
        match (is_duokan, href_fragment(a_attrs)) {
            (true, Some(frag)) => {
                let id_attr = html::attr_value(a_attrs, "id").map(|v| format!(" id=\"{v}\"")).unwrap_or_default();
                if escaped {
                    // 转义成文字的那种：原书显示的就是一串 `<img …>` 代码、图在网上离线看不到，换成它自己 alt 里的注释序号（"注释12" → 12）
                    local += 1;
                    let num = duokan_note_num(img_inner).unwrap_or_else(|| local.to_string());
                    format!("<a href=\"#{frag}\"{id_attr}><sup>{}</sup></a>", xml_escape(&num))
                } else {
                    // 真图标：原样保留（不再换成上标数字——那是多出来的字，用户 2026-09-29 定），只加 `eink-noteicon` 类限成一个字高
                    // （没写宽高的 80×80 图标在 xochitl 上撑成一大块，真机《甲午：摇摆的战争》）
                    format!("<sup><a href=\"#{frag}\"{id_attr}>{}</a></sup>", mark_note_icons(&format!("<img{}/>", img_inner.trim_end().trim_end_matches('/'))))
                }
            }
            _ => whole.to_string(),
        }
    };
    let markers = duokan_footnote_re()
        .replace_all(html, |c: &regex::Captures| {
            rewrite(c.get(1).unwrap().as_str(), c.get(2).unwrap().as_str(), c.get(0).unwrap().as_str(), true)
        })
        .into_owned();
    let markers = duokan_footnote_img_re()
        .replace_all(&markers, |c: &regex::Captures| {
            rewrite(c.get(1).unwrap().as_str(), c.get(2).unwrap().as_str(), c.get(0).unwrap().as_str(), false)
        })
        .into_owned();
    // 去链注释块里的悬空回链（#c_X_Y），否则 reMarkable 判互指对整对丢弃、正向也点不动。
    let unlinked = duokan_backlink_re()
        .replace_all(&markers, |c: &regex::Captures| c.get(1).unwrap().as_str().to_string())
        .into_owned();
    // 拍平注释块的无效嵌套 <p>（成簇相邻时会被 reMarkable 吞掉一条），锚点保留。
    duokan_note_block_re()
        .replace_all(&unlinked, |c: &regex::Captures| {
            format!("<p id=\"{}\"{}>{}</p>", c.get(1).unwrap().as_str(), c.get(2).unwrap().as_str(), c.get(3).unwrap().as_str())
        })
        .into_owned()
}

/// 注释正文去标签成纯内联文本（供 `FootnoteMode::Inline` 塞进 `<span>`，杜绝块级标签造成非法嵌套
/// →xochitl 严格 XML 整章白屏）。折叠空白；结果要写回 XHTML，所以再转义一次。
pub(super) fn inline_note_text(html: &str) -> String {
    xml_escape(&html::plain_text(html))
}

/// 优化器专用脚注处理：**保留标号原样**（上标、数字、图标都不动，前后一个字不加），只把 `<a>` 的 href 规整成同章 `#frag`，
/// 被引用的注释块（index 提供，跨文件也行；键是 (注释所在文件, id)，`name` = 本章 zip 路径）移到本章末尾 `<div class="footnotes">`。
/// - `Anchor`（跳转，xochitl）：去掉 xochitl 不认的 `epub:type`，点标号跳到章末、用阅读器的"返回"回来；
/// - `Popup`（KOReader）：标号标 `epub:type="noteref"`、注释块是 `<aside epub:type="footnote">`，KOReader 点标号弹窗显示；
/// - `Inline`：注释文字就地内联〔…〕（只在测试里用）。
///
/// marker 按元素认（[`a_elems`]，与 [`referenced_note_keys`] 同一口径）：`<sup>` 整个包住的 noteref、其余 noteref、跨文件普通 `<a>`。
/// 2026-09-29 起不再在标号后追加 `[N]`（用户定：严格说多了字符）。
pub fn preserve_relink_footnotes(html_text: &str, name: &str, index: &std::collections::HashMap<NoteKey, String>, mode: crate::optimize::FootnoteMode) -> String {
    use crate::optimize::FootnoteMode;
    if index.is_empty() {
        return html_text.to_string();
    }
    let popup = mode == FootnoteMode::Popup;
    let noteref = if popup { " epub:type=\"noteref\"" } else { "" };
    let mut appended: Vec<String> = Vec::new();
    let mut seen: std::collections::HashSet<NoteKey> = std::collections::HashSet::new();

    let mut make = |frag: &str, key: &NoteKey, text: &str, content: &str, sup_wrapped: bool| -> String {
        if mode == FootnoteMode::Inline {
            // 内联：**丢弃原 marker**（很多书 marker 是图标 <img>，按固有尺寸渲染=巨大且每条重复），
            // 就地只留内联注释 `〔…〕`（注释**去标签成纯文本**，杜绝块级标签塞进 <p> 致 xochitl 严格 XML 白屏）。
            return format!("<span class=\"eink-fnote\">〔{}〕</span>", inline_note_text(text));
        }
        if seen.insert(key.clone()) {
            // 注释放章末。不加可点回链——真机实测 reMarkable 会丢弃"marker↔注释"互指里较晚那条
            // (注释回链)，加了也点不了、反成死链迷惑人。返回靠阅读器原生。
            appended.push(footnote_block(&key.1, &deprefix_footnote_hrefs(text), popup));
        }
        let link = format!("<a href=\"#{frag}\"{noteref}>{}</a>", mark_note_icons(content));
        if sup_wrapped {
            format!("<sup>{link}</sup>")
        } else {
            link
        }
    };

    let tags: Vec<html::Tag> = html::tags(html_text).collect();
    let elems = a_elems(&tags);
    let only_ws = |a: usize, b: usize| html_text[a..b].trim().is_empty();
    // 每个 <a>：(href, 锚点原文, 注释键, 注释正文)——只有键在 index 里的才算 marker。
    let lookup = |e: &AElem| -> Option<(&str, &str, NoteKey, &String)> {
        let href = html::attr_value(&html_text[e.start..e.open_end], "href")?;
        if html::is_external(html::split_href(href).0) {
            return None;
        }
        let frag = html::split_href(href).1?;
        let key = note_key(name, href)?;
        let text = index.get(&key)?;
        Some((href, frag, key, text))
    };
    let mut done = vec![false; elems.len()];
    let mut edits: Vec<(usize, usize, String)> = Vec::new();
    // 1) <sup> 整体包裹的图标 noteref；2) 剩余裸 noteref
    for pass_sup in [true, false] {
        for (i, e) in elems.iter().enumerate() {
            if done[i] || !is_noteref(&html_text[e.start..e.open_end]) {
                continue;
            }
            let range = if pass_sup {
                let sup_open = e.open_ix.checked_sub(1).map(|k| &tags[k]).filter(|t| t.kind == html::TagKind::Open && t.is("sup") && only_ws(t.end, e.start));
                let sup_close = tags.get(e.close_ix + 1).filter(|t| t.kind == html::TagKind::Close && t.is("sup") && only_ws(e.end, t.start));
                match (sup_open, sup_close) {
                    (Some(o), Some(c)) => (o.start, c.end),
                    _ => continue,
                }
            } else {
                (e.start, e.end)
            };
            let Some((_, frag, key, text)) = lookup(e) else {
                done[i] = true; // 查不到（不是收集来的注释）：三遍都不会改它
                continue;
            };
            done[i] = true;
            edits.push((range.0, range.1, make(frag, &key, text, &html_text[e.open_end..e.close_start], pass_sup)));
        }
    }
    // 3) 跨文件普通 <a href="其他文件#frag">：非 noteref，但 frag 已被 collect_footnote_notes 收进
    //    index（确证是注释）→ Inline 内联〔…〕/ Anchor 改同章锚点 + 注释搬章末。目标不在 index 的（目录/交叉引用）不动。
    for (i, e) in elems.iter().enumerate() {
        if done[i] || href_crossfile_fragment(&html_text[e.start..e.open_end]).is_none() {
            continue;
        }
        let Some((_, frag, key, text)) = lookup(e) else { continue };
        let content = &html_text[e.open_end..e.close_start];
        let new = make(frag, &key, text, content, false);
        edits.push((e.start, e.end, new));
    }
    if edits.is_empty() {
        return html_text.to_string();
    }
    edits.sort_by_key(|e| e.0);
    let out = html::apply_edits(html_text, edits);
    if appended.is_empty() {
        return if popup { ensure_epub_ns(&out) } else { out };
    }
    // ⚠ 注释区必须插到 </body> **之内**。optimize 处理的是完整 xhtml，若加到文件末尾就落在
    // </body></html> 外面=无效 HTML，xochitl 不为其中的 id 建锚点 → marker 死链、点不动。
    let block = format!("\n<hr/>\n<div class=\"footnotes\">\n{}\n</div>\n", appended.join("\n"));
    let out = match out.rfind("</body>") {
        Some(pos) => format!("{}{}{}", &out[..pos], block, &out[pos..]),
        None => format!("{out}{block}"),
    };
    if popup {
        ensure_epub_ns(&out)
    } else {
        out
    }
}


#[cfg(test)]
mod footnote_inline_tests {
    use super::*;
    use std::collections::{HashMap, HashSet};

    #[test]
    fn normalize_self_hrefs_only_own_file() {
        let html = r##"<a href="part0004.html#a_1">本文件</a><a href="text/part0004.html#b">带目录本文件</a><a href="part0005.html#c">别的文件</a><a href="part0004.html">无锚</a><a href="#d">已裸</a>"##;
        let out = normalize_self_hrefs(html, "part0004.html");
        assert!(out.contains(r##"href="#a_1""##), "本文件应归一: {out}");
        assert!(out.contains(r##"href="#b""##), "只给文件名时按末段比对: {out}");
        assert!(out.contains(r##"href="part0005.html#c""##), "跨文件不动: {out}");
        assert!(out.contains(r##"href="part0004.html">"##), "无 fragment 不动: {out}");
        assert_eq!(normalize_self_hrefs(html, ""), html, "空 basename 原样");
        // 给完整路径时按路径解析：同名不同目录的文件不是本文件
        let h = r##"<a href="../b/x.html#f">别处同名</a><a href='x.html#g'>本文件</a><a href="../a/x.html#h">绕一圈回来</a>"##;
        let out = normalize_self_hrefs(h, "OEBPS/a/x.html");
        assert_eq!(out, r##"<a href="../b/x.html#f">别处同名</a><a href='#g'>本文件</a><a href="#h">绕一圈回来</a>"##);
    }

    #[test]
    fn fix_duokan_markers_calibre_real_img_keeps_id() {
        // Calibre 洗后：真 <img>（本地图）+ <a> 自带回链落点 id。图标保留（加限高的类）、id 必须保留，不换成数字。
        let html = r##"<sup class="calibre4"><a class="duokan-footnote" href="#a_2_1" id="c_2_1"><img alt="注释7" class="duokan-footnote1" src="../images/00003.png"/></a></sup>"##;
        let out = fix_duokan_markers(html);
        assert_eq!(out, r##"<sup><a href="#a_2_1" id="c_2_1"><img alt="注释7" class="duokan-footnote1 eink-noteicon" src="../images/00003.png"/></a></sup>"##);
        // 转义成文字的那种（微读 CDN 图）：原书显示的是一串代码，换成它 alt 里的序号
        let esc = r##"<sup><a href="#fo3">&lt;img class="duokan-footnote" alt="注释3" src="https://cdn/x.png"/&gt;</a></sup>"##;
        assert_eq!(fix_duokan_markers(esc), r##"<a href="#fo3"><sup>3</sup></a>"##);
        // 非 duokan 的真 img 链接不碰
        let other = r##"<sup><a href="#x"><img alt="图" class="icon" src="i.png"/></a></sup>"##;
        assert_eq!(fix_duokan_markers(other), other);
    }

    #[test]
    fn fix_duokan_markers_shared_fn() {
        // 多标记按 alt 号、非 duokan 的转义 img 不碰。
        let html = r##"<sup><a href="#a_1_1">&lt;img alt="注释1" class="duokan-footnote1" src="https://res.weread.qq.com/a.png"/&gt;</a></sup>正文<sup><a href="#a_1_2">&lt;img alt="注释2" class="duokan-footnote1" src="https://res.weread.qq.com/b.png"/&gt;</a></sup>
<sup><a href="#x">&lt;img alt="别的" class="other-icon"/&gt;</a></sup>"##;
        let out = fix_duokan_markers(html);
        assert!(out.contains(r##"<a href="#a_1_1"><sup>1</sup></a>"##), "标记1: {out}");
        assert!(out.contains(r##"<a href="#a_1_2"><sup>2</sup></a>"##), "标记2按 alt 号: {out}");
        assert!(!out.contains("duokan-footnote"), "duokan img 全清: {out}");
        assert!(out.contains(r##"class="other-icon""##), "非 duokan 的转义 img 不该被碰: {out}");
    }

    #[test]
    fn duokan_img_footnote_marker_becomes_tappable_sup() {
        // 《人骨拼图》真实 duokan 形态：标记的 <img> 被**实体转义**成字面死文本、又是微读 CDN
        // 远程图（离线渲染成一坨、点不动）；注释块**已同文件** <p id="a_8_1">（前向锚 #a_8_1 有效）。
        // 修复=标记换成干净可点上标数字、保留 href、注释块原地不动。
        // 真实结构：注释块是无效嵌套 <p>、内含悬空回链 <a href="#c_8_1">。
        let ch = r##"<p>喝了太多冰镇台克利<sup class="calibre4"><a href="#a_8_1">&lt;img alt="注释1" class="duokan-footnote1" src="https://res.weread.qq.com/x_00003.png" data-w="48px" data-ratio="1.000"/&gt;</a></sup>。</p>
<p id="a_8_1"><p class="pfootnotetext"><a class="calibre6" href="#c_8_1">一种由朗姆酒、柠檬汁和糖混合的加冰鸡尾酒。</a></p></p>"##;
        let out = fix_duokan_markers(ch);
        assert!(out.contains(r##"<a href="#a_8_1"><sup>1</sup></a>"##), "标记未换成可点上标: {out}");
        assert!(!out.contains("&lt;img"), "转义死图标记未清除: {out}");
        assert!(!out.contains("res.weread.qq.com"), "远程 CDN 图未去掉: {out}");
        assert!(out.contains("加冰鸡尾酒"), "注释文本不该丢: {out}");
        // 悬空回链去链（否则 reMarkable 判互指对整对丢弃、正向点不动）。
        assert!(!out.contains(r##"href="#c_8_1""##), "注释回链未去链: {out}");
        // 嵌套 <p> 拍平成合法单段（成簇相邻时无效嵌套会被 reMarkable 吞一条=缺一条）。
        assert!(out.contains(r##"<p id="a_8_1" class="pfootnotetext">"##), "嵌套 p 未拍平: {out}");
    }

    #[test]
    fn weread_p_note_collected_without_semantic() {
        // 13·67 真实形态：注释章里 <aside> 与 <p class="class_s1r">（**无 note 语义**）交替。
        // collect_footnote_notes(require_semantic=false) 须靠 id∈referenced 收 <p> 注释。
        let text = r#"<p>被叫做"大帮"<span id="a541"/><a href="part0040.html#a51T" class="class_s4yp"> 3 </a>后续</p>"#;
        let notes = r#"<aside id="a51U" type="footnote" class="class_svk"><a href="x#y">4</a>. CID</aside><p id='a51T' class="class_s1r"><a href="part0037.html#a541">3</a>. 帮办、大帮：一词八十年代已式微</p>"#;
        let referenced: HashSet<String> = referenced_note_frags(text).into_iter().collect();
        let (notes_cleaned, collected) = collect_footnote_notes(notes, &referenced, false);
        let map: HashMap<String, String> = collected.into_iter().collect();
        assert!(map.contains_key("a51T"), "单引号 id 的 <p> 注释也要收: keys={:?}", map.keys().collect::<Vec<_>>());
        assert!(!notes_cleaned.contains("a51T"), "a51T 注释未从原位移除");
    }
}

#[cfg(test)]
mod optimizer_footnote_tests {
    use super::*;
    use std::collections::{HashMap, HashSet};

    // ---- collect_footnote_notes：只搬被引用 + 语义确证的块 ----
    #[test]
    fn collects_referenced_p_and_li_notes() {
        // 书末尾注文件：一个 <p> 尾注 + 一个 <li> 尾注 + 一个非注释 <p>（普通段）。
        let notes = r#"<p class="footnote" id="n1">注释一 <a href="text01.xhtml#r1">↩</a></p><ol><li class="endnote" id="n2">注释二</li></ol><p id="plain">普通段落无语义</p>"#;
        let mut referenced = HashSet::new();
        referenced.insert("n1".to_string());
        referenced.insert("n2".to_string());
        referenced.insert("plain".to_string()); // 即便被引用，无注释语义也不搬
        let (cleaned, idx) = collect_footnote_notes(notes, &referenced, true);
        let ids: Vec<&str> = idx.iter().map(|(i, _)| i.as_str()).collect();
        assert!(ids.contains(&"n1") && ids.contains(&"n2"), "referenced 的 p/li 注释应被收: {ids:?}");
        assert!(!ids.contains(&"plain"), "无注释语义的 <p> 不该被搬: {ids:?}");
        assert!(!cleaned.contains(r##"id="n1""##) && !cleaned.contains(r##"id="n2""##), "已收注释应从原位移除: {cleaned}");
        assert!(cleaned.contains(r##"id="plain""##), "普通段落应原样保留: {cleaned}");
    }

    #[test]
    fn skips_unreferenced_notes_no_loss() {
        // 未被任何 marker 引用的注释块必须原地保留（杜绝内容丢失）。
        let notes = r#"<p class="footnote" id="orphan">没人引用的注释文本</p>"#;
        let (cleaned, idx) = collect_footnote_notes(notes, &HashSet::new(), true);
        assert!(idx.is_empty(), "无引用不该收");
        assert_eq!(cleaned, notes, "无引用的注释应原样不动: {cleaned}");
    }

    #[test]
    fn collects_flat_div_notes_but_skips_nested() {
        let mut referenced = HashSet::new();
        referenced.insert("d1".to_string());
        referenced.insert("d2".to_string());
        // d1 扁平 div 注释应收；d2 含嵌套 div → 跳过不搬（零丢失）
        let notes = r#"<div class="footnote" id="d1">扁平注释文本</div><div class="footnote" id="d2">外<div>内嵌</div>层</div>"#;
        let (cleaned, idx) = collect_footnote_notes(notes, &referenced, true);
        let ids: Vec<&str> = idx.iter().map(|(i, _)| i.as_str()).collect();
        assert!(ids.contains(&"d1") && !ids.contains(&"d2"), "扁平 div 收、嵌套 div 跳过: {ids:?}");
        assert!(!cleaned.contains(r##"id="d1""##) && cleaned.contains(r##"id="d2""##), "{cleaned}");
    }

    // ---- referenced_note_frags：noteref + 跨文件普通 <a> ----
    #[test]
    fn scans_noteref_and_crossfile_markers() {
        let ch = r##"<p>甲<a epub:type="noteref" href="notes.xhtml#a1">1</a>乙<a href="notes.xhtml#a2">2</a>丙<a href="#local">本地</a></p>"##;
        let mut fr = referenced_note_frags(ch);
        fr.sort();
        assert!(fr.contains(&"a1".to_string()), "noteref frag 应收: {fr:?}");
        assert!(fr.contains(&"a2".to_string()), "跨文件普通 <a> frag 应收: {fr:?}");
        assert!(!fr.contains(&"local".to_string()), "同文件裸锚点不归优化器搬运（break_cycles 管）: {fr:?}");
    }

    // ---- preserve_relink_footnotes：跨文件普通尾注（"中间章节点了不跳"的主因）----
    #[test]
    fn crossfile_plain_endnote_relinked() {
        let mut index: HashMap<NoteKey, String> = HashMap::new();
        index.insert(("notes/notes.xhtml".to_string(), "n12".to_string()), "第十二条注释文本".to_string());
        let chapter = r#"<html><body><p>正文波波<a href="../notes/notes.xhtml#n12">12</a>后续</p></body></html>"#;
        let out = preserve_relink_footnotes(chapter, "c.xhtml", &index, crate::optimize::FootnoteMode::Anchor);
        assert!(out.contains(r##"<a href="#n12">12</a>"##), "跨文件 marker 未改成同章锚点: {out}");
        assert!(!out.contains("notes.xhtml"), "跨文件 href 前缀未去掉: {out}");
        assert!(out.contains(r##"<div id="n12" class="eink-note">第十二条注释文本</div>"##), "注释未搬进本章章末: {out}");
        // 注释区必须落在 </body> 之内
        let body_end = out.find("</body>").unwrap();
        assert!(out[..body_end].contains(r##"<div class="footnotes">"##), "注释区落到 </body> 外: {out}");
        // Inline 模式：注释就地内联〔…〕、不跳转、无章末 div
        let inl = preserve_relink_footnotes(chapter, "c.xhtml", &index, crate::optimize::FootnoteMode::Inline);
        assert!(inl.contains("〔第十二条注释文本〕") && !inl.contains(r##"<div class="footnotes">"##) && !inl.contains(r##"href="#n12""##), "Inline 应内联常显不跳转: {inl}");
    }

    // 注释源块自带块级标签（<aside>/<li>/<div> 包一层 <p>）是常见形态；collect_footnote_notes 只搬
    // 内层 html，若外层落点仍用 <p> 包一遍就会产出 <p><p>…</p></p> 这种非法内容模型（<p> 不能合法
    // 含块级子元素）。这条测试端到端跑 collect_footnote_notes → preserve_relink_footnotes 全链路，
    // 断言输出永远是 <div id> 落点、不出现嵌套 <p>。
    #[test]
    fn note_source_block_with_nested_p_does_not_produce_illegal_nested_p() {
        let notes_chapter = r##"<aside class="footnote" id="fn1"><p>注释正文，含<a href="#backref">回链</a></p></aside>"##;
        let mut referenced = HashSet::new();
        referenced.insert("fn1".to_string());
        let (_, idx_vec) = collect_footnote_notes(notes_chapter, &referenced, true);
        let index: HashMap<NoteKey, String> = idx_vec.into_iter().map(|(id, t)| (("notes.xhtml".to_string(), id), t)).collect();
        assert!(index.get(&("notes.xhtml".to_string(), "fn1".to_string())).unwrap().contains("<p>"), "前提：源块内层确实带 <p>，测试才有意义");

        let chapter = r#"<html><body><p>正文<a href="notes.xhtml#fn1">1</a>续</p></body></html>"#;
        let out = preserve_relink_footnotes(chapter, "c.xhtml", &index, crate::optimize::FootnoteMode::Anchor);
        assert!(!out.contains("<p><p>") && !out.contains("<p><a href=\"#backref\">"), "落点不该是 <p> 包 <p>: {out}");
        assert!(out.contains(r#"<div id="fn1" class="eink-note"><p>注释正文"#), "落点应是 <div id> 包住源块内层 html 原样: {out}");
    }

    /// `<sup>` 包裹的图标 noteref：标号原样保留（不再换成 `[N]`，2026-09-29 用户定），图标加 `eink-noteicon` 类限成一个字高。
    #[test]
    fn sup_wrapped_image_marker_kept_with_icon_class() {
        let mut index: HashMap<NoteKey, String> = HashMap::new();
        index.insert(("c.xhtml".to_string(), "fo14".to_string()), "注释文字".to_string());
        let chapter = r##"<p>正文<sup><a type="noteref" href="#fo14"><img alt="" src="../Images/note.png"/></a></sup>续</p>"##;
        let out = preserve_relink_footnotes(chapter, "c.xhtml", &index, crate::optimize::FootnoteMode::Anchor);
        assert!(out.contains(r##"<sup><a href="#fo14"><img class="eink-noteicon" alt="" src="../Images/note.png"/></a></sup>续"##), "{out}");
        assert!(!out.contains("[1]"), "不加 [N]: {out}");
    }

    /// 弹窗模式（KOReader）：标号 `epub:type="noteref"`、注释块 `<aside epub:type="footnote">`，根元素补 epub 命名空间；
    /// 可见文字和跳转模式一样（标号原样，一个字不加）。
    #[test]
    fn popup_mode_marks_noteref_and_aside_footnote() {
        let mut index: HashMap<NoteKey, String> = HashMap::new();
        index.insert(("notes.xhtml".to_string(), "n1".to_string()), "注释一".to_string());
        let chapter = r#"<html xmlns="http://www.w3.org/1999/xhtml"><body><p>正文<sup><a epub:type="noteref" href="notes.xhtml#n1">1</a></sup>续<a class="x" href="notes.xhtml#n1">1</a></p></body></html>"#;
        let out = preserve_relink_footnotes(chapter, "c.xhtml", &index, crate::optimize::FootnoteMode::Popup);
        assert!(out.starts_with(r#"<html xmlns:epub="http://www.idpf.org/2007/ops" xmlns="http://www.w3.org/1999/xhtml">"#), "{out}");
        assert!(out.contains(r##"<sup><a href="#n1" epub:type="noteref">1</a></sup>续<a href="#n1" epub:type="noteref">1</a>"##), "{out}");
        assert_eq!(out.matches(r#"<aside epub:type="footnote" id="n1" class="eink-note">注释一</aside>"#).count(), 1, "同一条注释只放一次: {out}");
        let jump = preserve_relink_footnotes(chapter, "c.xhtml", &index, crate::optimize::FootnoteMode::Anchor);
        assert!(!jump.contains("epub:"), "跳转模式不带 epub:type（xochitl 不认）: {jump}");
        assert_eq!(ensure_epub_ns(&out), out, "命名空间只补一次");
    }

    /// 真机《甲午：摇摆的战争》坐实的真实结构：`duokan-footnote` 类挂在外层 `<a>` 上（不在 `<img>`
    /// 自己的 class 里），此前 `fix_duokan_markers` 只查 `img_inner` 会漏判、80×80 图标原样穿透到
    /// xochitl，按固有像素撑成巨大方块。
    #[test]
    fn fix_duokan_markers_detects_class_on_outer_a_not_just_img() {
        let out = fix_duokan_markers(r##"<sup><a class="duokan-footnote" href="#fo14" id="foref14"><img alt="" class="exs" src="../Images/note.png"/></a></sup>"##);
        assert_eq!(out, r##"<sup><a href="#fo14" id="foref14"><img alt="" class="exs eink-noteicon" src="../Images/note.png"/></a></sup>"##, "图标原样保留、加限高的类，不换成数字");
    }

    #[test]
    fn number_icon_note_links_for_readers_without_image_links() {
        // 多看标号（fix_duokan_markers 保留图标之后）：编号取注释开头的 [14]
        let ch = r##"<p>正文<sup><a href="#fo14" id="foref14"><img alt="" class="exs eink-noteicon" src="../Images/note.png"/></a></sup>续</p><div id="fo14" class="eink-note"><p>[14] 注释文字</p></div>"##;
        let out = number_icon_note_links(ch);
        assert!(out.contains(r##"<sup><a href="#fo14" id="foref14">14</a></sup>续"##), "{out}");
        assert!(out.contains("[14] 注释文字"), "注释本身不动");
        // 没有作者编号：取 alt 里的"注释7"；再没有按本章顺序数；没包 <sup> 的补一层
        let alt = r##"<a href="#n1"><img class="eink-noteicon" alt="注释7" src="i.png"/></a><a href="#n2"><img class="eink-noteicon" src="i.png"/></a><div id="n1">甲</div><div id="n2">乙</div>"##;
        assert_eq!(number_icon_note_links(alt), r##"<a href="#n1"><sup>7</sup></a><a href="#n2"><sup>2</sup></a><div id="n1">甲</div><div id="n2">乙</div>"##);
        // 没有注释特征的图片链接（插图放大、目录图标）、链接里有字的：不动
        for keep in [r##"<a href="#fig1"><img src="small.png"/></a><div id="fig1"><img src="big.png"/></div>"##, r##"<a href="#fo1" class="note"><img src="i.png"/>注</a><p id="fo1">x</p>"##] {
            assert_eq!(number_icon_note_links(keep), keep);
        }
        // 目标有注释语义也算
        let sem = r##"<a href="#f"><img src="i.png"/></a><aside epub:type="footnote" id="f">（3）说明</aside>"##;
        assert!(number_icon_note_links(sem).starts_with(r##"<a href="#f"><sup>3</sup></a>"##));
    }

    #[test]
    fn inline_drops_image_marker() {
        // 《飘》形态：noteref <a> 包着图标 <img>。内联模式必须丢弃图标（否则 xochitl 按固有尺寸渲染=巨大且每条重复）。
        let mut index: HashMap<NoteKey, String> = HashMap::new();
        index.insert(("c.xhtml".to_string(), "fn1".to_string()), "注释文字".to_string());
        let chapter = r##"<p>正文<a epub:type="noteref" href="#fn1"><span class="koboSpan"><img alt="note" src="../Images/i.png"/></span></a>后续</p>"##;
        let out = preserve_relink_footnotes(chapter, "c.xhtml", &index, crate::optimize::FootnoteMode::Inline);
        assert!(!out.contains("<img"), "内联模式应丢弃图标 marker: {out}");
        assert!(out.contains("〔注释文字〕"), "应内联注释: {out}");
    }

    #[test]
    fn crossfile_nonnote_link_untouched() {
        // 目标不在 index（不是注释）→ 跨文件链接原样不动，绝不误搬目录/交叉引用。
        let index: HashMap<NoteKey, String> = HashMap::new();
        let chapter = r#"<p>见<a href="chap03.xhtml#sec2">第三章</a></p>"#;
        assert_eq!(preserve_relink_footnotes(chapter, "c.xhtml", &index, crate::optimize::FootnoteMode::Anchor), chapter);
    }

    /// 2026-09-28 审计：改名 `fn1`→`fn1-x2` 不能撞上本章后面本来就有的 `fn1-x2`。
    #[test]
    fn dedup_new_name_avoids_later_id_in_same_chapter() {
        let mut seen: HashSet<String> = ["fn1".to_string()].into_iter().collect();
        let ch = r##"<p id="fn1">甲</p><a href="#fn1">1</a><p id="fn1-x2">乙</p>"##;
        let out = crate::htmlproc::dedup_ids_in_chapter(ch, &mut seen);
        assert_eq!(out, r##"<p id="fn1-x3">甲</p><a href="#fn1-x3">1</a><p id="fn1-x2">乙</p>"##);
    }

    // ---- dedup_ids_in_chapter：跨章 id 撞车（"跳到错章"的次因）----
    #[test]
    fn dedup_renames_colliding_ids_across_chapters() {
        let mut seen = HashSet::new();
        let ch1 = r##"<p>甲<a href="#fn1">1</a></p><p id="fn1">注释甲</p>"##;
        let out1 = dedup_ids_in_chapter(ch1, &mut seen);
        assert_eq!(out1, ch1, "首章 id 不冲突，原样");
        // 第二章又用了 id="fn1" → 必须改名，且本章内 href="#fn1" 同步改
        let ch2 = r##"<p>乙<a href="#fn1">1</a></p><p id="fn1">注释乙</p>"##;
        let out2 = dedup_ids_in_chapter(ch2, &mut seen);
        assert!(!out2.contains(r##"id="fn1""##), "撞车 id 未改名: {out2}");
        // 改名后 marker 与目标仍配对（同一新 id）
        let new_id = Regex::new(r##"id="(fn1-x\d+)""##).unwrap().captures(&out2).map(|c| c.get(1).unwrap().as_str().to_string());
        let new_id = new_id.unwrap_or_else(|| panic!("未生成唯一新 id: {out2}"));
        assert!(out2.contains(&format!(r##"href="#{new_id}""##)), "本章 href 未随之改名: {out2}");
        assert!(out2.contains("注释乙"), "注释文本不应丢");
    }

    #[test]
    fn dedup_leaves_crossfile_href_alone() {
        // 跨文件 href="f#fn1" 不因本章 id 改名而被动（它指向别的文件）。
        let mut seen = HashSet::new();
        seen.insert("fn1".to_string()); // 假装别章已用过 fn1
        let ch = r#"<p id="fn1">本章注释</p><p>另见<a href="other.xhtml#fn1">跨文件</a></p>"#;
        let out = dedup_ids_in_chapter(ch, &mut seen);
        assert!(!out.contains(r##"<p id="fn1">"##), "本章 id 应改名");
        assert!(out.contains(r##"href="other.xhtml#fn1""##), "跨文件 href 不该被改: {out}");
    }

    #[test]
    fn dedup_renames_many_ids_in_one_pass_and_matches_case_exactly() {
        // 一章里多个 id 同时撞车：各自改成互不相同的新名，且各自的同文件 href 跟着改；
        // 大小写不同的 id（HTML id 区分大小写）不能被误当同一个而一起改。
        let mut seen: HashSet<String> = ["fn1", "fn2", "Fn1"].iter().map(|s| s.to_string()).collect();
        let ch = r##"<p id="fn1">甲</p><a href="#fn1">1</a><p id="fn2">乙</p><a href="#fn2">2</a><p id="Fn1">丙</p><a href="#Fn1">3</a><a href="#keep">4</a>"##;
        let out = dedup_ids_in_chapter(ch, &mut seen);
        let ids: Vec<String> = Regex::new(r##"\bid="([^"]+)""##).unwrap().captures_iter(&out).map(|c| c[1].to_string()).collect();
        assert_eq!(ids.len(), 3);
        assert!(ids.iter().collect::<HashSet<_>>().len() == 3, "三个新 id 必须互不相同: {out}");
        for id in &ids {
            assert!(id.contains("-x"), "都应改名: {out}");
            assert!(out.contains(&format!(r##"href="#{id}""##)), "href 未随 {id} 改名: {out}");
        }
        assert!(out.contains(r##"href="#keep""##), "无关 href 原样: {out}");
        assert!(ids[2].starts_with("Fn1-x"), "大写 Fn1 改名后保留自己的大小写前缀: {out}");
    }

    // ---- collapse_dup_id_attrs：同元素双 id 属性（非法 XHTML → reMarkable 整章渲染失败）----
    #[test]
    fn collapse_keeps_first_id_and_drops_rest() {
        // 首位是注入锚点(aid/fp)，既存 id 在后 → 保住首位、删后续
        let h = r#"<p id="aid5N3C1" class="calibre6" id="filepos18251">正文</p>"#;
        let out = collapse_dup_id_attrs(h);
        assert_eq!(out, r#"<p id="aid5N3C1" class="calibre6">正文</p>"#, "应保首位 id、删后续: {out}");
        // 无重复 → 原样；不误伤 aid=（不是 id 属性）
        let ok = r#"<p aid="X" id="y">t</p>"#;
        assert_eq!(collapse_dup_id_attrs(ok), ok, "单 id 应原样、aid 不误删: {}", collapse_dup_id_attrs(ok));
        // 三个 id 也只留首个
        let three = r#"<div id="a" id="b" id="c"></div>"#;
        assert_eq!(collapse_dup_id_attrs(three), r#"<div id="a"></div>"#);
    }
}
