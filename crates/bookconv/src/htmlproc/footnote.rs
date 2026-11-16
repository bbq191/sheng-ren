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
            if let Some(m) = (k + 1..tags.len())
                .find(|&m| tags[m].kind == html::TagKind::Close && tags[m].is("a"))
            {
                out.push(AElem {
                    open_ix: k,
                    close_ix: m,
                    start: t.start,
                    open_end: t.end,
                    close_start: tags[m].start,
                    end: tags[m].end,
                });
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
    html::attrs(open).iter().any(|a| {
        (a.is("type") || a.name.to_ascii_lowercase().ends_with(":type"))
            && a.value.split_whitespace().any(|v| v == "noteref")
    })
}

/// 链接（`href` 属性原文）指向的注释键：按 `epubzip::resolve_link` 解析——字符引用还原、锚点百分号解码（NCX/正文常把中文锚点
/// 写成 `%E6%B3%A8`），路径按本章解析（`#x` 就是本章）。键里的 id 是解码后的值，和注释块 `id` 属性还原字符引用后的值比。
fn note_key(name: &str, href: &str) -> Option<NoteKey> {
    let (path, frag) = crate::epubzip::resolve_link(name, href);
    let path = if path.is_empty() {
        name.to_string()
    } else {
        path
    };
    Some((path, frag.filter(|f| !f.is_empty())?))
}

/// `<a>` 像注释标号（普通链接、没有 noteref 语义时用）：文字是 `1`/`[1]`/`①`/`*`/`注1` 这类，或里面/外面紧挨着 `<sup>`。
fn marker_like_link(html_text: &str, tags: &[html::Tag], e: &AElem) -> bool {
    static TEXT: OnceLock<Regex> = OnceLock::new();
    let re = TEXT.get_or_init(|| {
        Regex::new(
            r#"^[\[\(（〔【<]?\s*(\d{1,4}|[*†‡§]{1,3}|[①-⑳]|[ⅰ-ⅹ]|注\s*\d{0,4})\s*[\]\)）〕】>]?$"#,
        )
        .unwrap()
    });
    let inner = &html_text[e.open_end..e.close_start];
    let text = html::plain_text(inner);
    let only_ws = |a: usize, b: usize| html_text[a..b].trim().is_empty();
    let sup_around =
        e.open_ix.checked_sub(1).map(|k| &tags[k]).is_some_and(|t| {
            t.kind == html::TagKind::Open && t.is("sup") && only_ws(t.end, e.start)
        });
    (re.is_match(&text) || inner.to_ascii_lowercase().contains("<sup") || sup_around)
        && text.chars().count() <= 8
}

/// 标号样的普通链接（同文件、跨文件都算）连同它自己的 id：(注释键, 标号 id)。注释块开头的回链指回这个 id 时，
/// 这一对就是注释（[`collect_footnote_notes`] 的 `backrefs`；同 KFX 写出器配注释的办法，原书多半没有 note 语义：
/// 《罗杰疑案》`<a id="zw1" href="#zhu1"><sup>[1]</sup></a>` ↔ `<p class="fncontent"><a id="zhu1" href="#zw1">[1]</a>…`）。
pub fn marker_backrefs(html_text: &str, name: &str) -> Vec<(NoteKey, String)> {
    let tags: Vec<html::Tag> = html::tags(html_text).collect();
    let mut out = Vec::new();
    for e in a_elems(&tags) {
        let open = &html_text[e.start..e.open_end];
        let Some(href) = html::attr_value(open, "href") else {
            continue;
        };
        if html::is_external(html::split_href(href).0) || !marker_like_link(html_text, &tags, &e) {
            continue;
        }
        let Some(id) = html::attr_value(open, "id").filter(|v| !v.is_empty()) else {
            continue;
        };
        if let Some(k) = note_key(name, href) {
            out.push((k, crate::util::xml_unescape(id).into_owned()));
        }
    }
    out
}

/// 块开头（前面没有看得见的字）的 `<a>` 的 `id`（或 `name`），还原字符引用。
fn leading_anchor_id(inner: &str) -> Option<String> {
    let t = html::tags(inner).find(|t| t.is_start())?;
    if !t.is("a") || html::has_visible(&inner[..t.start]) {
        return None;
    }
    let open = &inner[t.start..t.end];
    html::attr_value(open, "id")
        .or_else(|| html::attr_value(open, "name"))
        .filter(|v| !v.is_empty())
        .map(|v| crate::util::xml_unescape(v).into_owned())
}

/// 块开头（前面没有看得见的字）就是一个指回 `markers` 里某个 id 的链接：注释块的回链。
fn starts_with_backlink(inner: &str, markers: &HashSet<String>) -> bool {
    let tags: Vec<html::Tag> = html::tags(inner).collect();
    let Some(e) = a_elems(&tags).into_iter().next() else {
        return false;
    };
    if html::has_visible(&inner[..e.start]) {
        return false;
    }
    html::attr_value(&inner[e.start..e.open_end], "href")
        .and_then(|h| {
            html::split_href(h)
                .1
                .map(|f| crate::util::xml_unescape(f).into_owned())
        })
        .is_some_and(|f| markers.contains(&f))
}

/// 搬走的注释块（[`collect_footnote_notes`] 收集、[`preserve_relink_footnotes`] 放到引用它的那一章末尾）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Note {
    /// 注释块的内层 html。
    pub inner: String,
    /// 来自有序列表 `<ol><li>` 的注释：列表编号是读者看得见的内容，搬过去时连同编号整个 `<li>` 保留在 `<ol start=N>` 里。
    pub list: Option<ListItem>,
}

/// 有序列表里的一条注释（见 [`Note::list`]）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ListItem {
    /// 原 `<ol>` 开标签，去掉了 `id`、`start`、`reversed`（`class`、`type`、`style` 这些管编号样式的照留）；放过去时按 `number` 写 `start`。
    pub ol_open: String,
    /// 原 `<li>` 开标签原文（`id`、`class`、`value` 都在）。
    pub li_open: String,
    /// 这一项在原书里显示的编号（按 `<ol>` 的 `start`/`reversed` 与各项的 `value` 算）。
    pub number: i64,
}

/// `<li>`（`spans[li]`）在它的父 `<ol>` 里显示的编号与 `<ol>` 开标签；父元素不是 `<ol>`（`<ul>` 没有编号）→ `None`。
/// `cache` 按 `<ol>` 存它全部直属 `<li>` 的编号（[`ol_items`]，每个 `<ol>` 只算一次；以前每个 `<li>` 都重扫整个列表，长注释表是平方级）。
fn list_item(
    html_text: &str,
    spans: &[html::Span],
    li: usize,
    cache: &mut HashMap<usize, Vec<(usize, ListItem)>>,
) -> Option<ListItem> {
    let pi = spans[li].parent?;
    if spans[pi].name != "ol" {
        return None;
    }
    let items = cache
        .entry(pi)
        .or_insert_with(|| ol_items(html_text, spans, pi));
    // 下标按文档序递增
    items
        .binary_search_by_key(&li, |(k, _)| *k)
        .ok()
        .map(|i| items[i].1.clone())
}

/// `<ol>`（`spans[pi]`）的全部直属 `<li>`：(下标, 编号等)，编号按 `<ol>` 的 `start`/`reversed` 与各项的 `value` 算。
fn ol_items(html_text: &str, spans: &[html::Span], pi: usize) -> Vec<(usize, ListItem)> {
    let ol = &spans[pi];
    let ol_open = &html_text[ol.open_start..ol.open_end];
    let reversed = html::attr(ol_open, "reversed").is_some();
    let int = |tag: &str, name: &str| {
        html::attr_value(tag, name).and_then(|v| v.trim().parse::<i64>().ok())
    };
    let items: Vec<usize> = (pi + 1..spans.len())
        .take_while(|&k| spans[k].open_start < ol.close_start)
        .filter(|&k| spans[k].parent == Some(pi) && spans[k].name == "li")
        .collect();
    let start = int(ol_open, "start").unwrap_or(if reversed { items.len() as i64 } else { 1 });
    let step = if reversed { -1 } else { 1 };
    let mut ol_tag = ol_open.to_string();
    for a in ["id", "start", "reversed"] {
        ol_tag = html::remove_attr(&ol_tag, a);
    }
    let mut number = start;
    let mut out = Vec::with_capacity(items.len());
    for (k, &it) in items.iter().enumerate() {
        let open = &html_text[spans[it].open_start..spans[it].open_end];
        number = int(open, "value").unwrap_or(if k == 0 { start } else { number + step });
        out.push((
            it,
            ListItem {
                ol_open: ol_tag.clone(),
                li_open: open.to_string(),
                number,
            },
        ));
    }
    out
}

/// 微读 **duokan 图片脚注**标记：`<sup..><a href="#frag">&lt;img ... class="duokan-footnote.." ../&gt;</a></sup>`。
/// 与 qqreader 版不同：注释块**已在同文件** `<p id="frag">`（前向锚有效），且标记里的 `<img>` 被
/// **实体转义**成字面文本、又是微读 CDN 远程图 → 设备离线渲染成一坨死文本、点不动。故只需把标记
/// 内容换成干净可点上标数字、保留 `href="#frag"`（注释块原地不动即可跳）。group1=`<a>` 属性、
/// group2=转义 img 内层（含 class/alt）。
pub(super) fn duokan_footnote_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| {
        Regex::new(r#"(?si)<sup\b[^>]*>\s*<a\b([^>]*)>\s*&lt;img\b(.*?)/?&gt;\s*</a>\s*</sup>"#)
            .unwrap()
    })
}
/// Calibre 洗过的 duokan 形态（`ebook-convert` AZW3/EPUB→EPUB 后）：标记里的 `<img>` 是**真标签**（非实体
/// 转义）、src 已是本地图（图片内容仅是"注释N"小图，点不动、设备上一坨小块），`<a>` 上带回链落点
/// `id="c_X_Y"`。group1=`<a>` 属性、group2=img 属性。与转义版共用同一替换逻辑。
pub(super) fn duokan_footnote_img_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| {
        Regex::new(r#"(?si)<sup\b[^>]*>\s*<a\b([^>]*)>\s*<img\b([^>]*?)/?>\s*</a>\s*</sup>"#)
            .unwrap()
    })
}
/// 把**指向本文件**的带文件名 href（`href="part0004.html#frag"` 写在 part0004.html 里，Calibre
/// `ebook-convert` 的固定写法）归一成裸 `#frag`。reMarkable 只跟同文件裸锚；且优化器各 pass
/// （`break_footnote_cycles` 只认裸锚、`referenced_note_keys` 把带路径的一律当跨文件尾注搬走）
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
        if same {
            Edit::Set(format!("#{frag}"))
        } else {
            Edit::Keep
        }
    })
    .into_owned()
}
/// 从 img 标签（或转义 img 的内层）抽注释序号：`alt` 属性（任意引号）整个是个标号时取里面的数字，`alt="注释12"` → `12`。
/// 只认「注释12」「注12」「note 3」「[12]」「12」这类短标号：有的书 `alt` 里放的是整条注释（掌阅、QQ 阅读的写法，
/// 《柏林谍影》`alt="1940年5月，纳粹空军轰炸鹿特丹……"`），以前取第一串数字，Move 上标号成了「1940」（2026-10-27 回归查到）。
/// 取不到返回 None（调用方用章内计数兜底）。按属性名完整匹配，`data-alt` 不算。
pub(super) fn duokan_note_num(img_inner: &str) -> Option<String> {
    static R: OnceLock<Regex> = OnceLock::new();
    let alt = html::attr_value(img_inner, "alt")?;
    let r = R.get_or_init(|| {
        Regex::new(
            r"(?i)^\s*(?:注释?|註釋?|(?:foot)?note)?\s*[\[［【(（]?\s*(\d{1,4})\s*[\]］】)）]?\s*$",
        )
        .unwrap()
    });
    r.captures(alt).map(|c| c[1].to_string())
}

/// `前缀数字_数字`（多看的回链落点 `c_3_1`、注释块 id `a_3_1`）。
fn is_duokan_pair_id(v: &str, prefix: &str) -> bool {
    v.strip_prefix(prefix)
        .and_then(|r| r.split_once('_'))
        .is_some_and(|(a, b)| {
            [a, b]
                .iter()
                .all(|x| !x.is_empty() && x.bytes().all(|c| c.is_ascii_digit()))
        })
}

/// duokan 注释块里的**回链** `<a ... href="#c_X_Y">注释文字</a>`（跳回标记的 `c_X_Y` 锚，
/// 但该 id 在书里根本不存在=悬空）。**必须去链成纯文本**：reMarkable 链接索引器遇到"注释块内含
/// 出链"会把整个脚注对判为互指对而**整对丢弃 → 正向 marker 也点不动**（同 break_footnote_cycles
/// 的病理，但那里只拆真 2-环、够不到这悬空回链）。去链后注释跟 qqreader 版一样纯文本、正向恢复可跳。
/// 按元素认（`html::tags`：任意引号，`data-href` 不算）。
fn unlink_duokan_backlinks(html_text: &str) -> String {
    let tags: Vec<html::Tag> = html::tags(html_text).collect();
    let edits: Vec<(usize, usize, String)> = a_elems(&tags)
        .into_iter()
        .filter(|e| {
            html::attr_value(&html_text[e.start..e.open_end], "href")
                .and_then(|h| h.strip_prefix('#'))
                .is_some_and(|f| is_duokan_pair_id(f, "c_"))
        })
        .map(|e| {
            (
                e.start,
                e.end,
                html_text[e.open_end..e.close_start].to_string(),
            )
        })
        .collect();
    if edits.is_empty() {
        html_text.to_string()
    } else {
        html::apply_edits(html_text, edits)
    }
}

/// duokan 注释块是**无效嵌套 `<p>`**：`<p id="a_X_Y"><p class="pfootnotetext">注释文字</p></p>`。
/// 多条注释成簇相邻时，reMarkable 渲染无效嵌套 `<p>` 会自动闭合外层 + 杂散 `</p>` → **吞/合并其中一条**
/// （"缺一条"根因）。拍平成合法单段 `<p id="a_X_Y" + 内层 <p> 的属性>注释文字</p>`，锚点保留、每条独立可跳。
/// 按标签认（`html::tags`：id 任意引号，`data-id` 不算）；两层 `<p>` 之间、两个 `</p>` 之间只能有空白。
fn flatten_duokan_note_blocks(html_text: &str) -> String {
    let tags: Vec<html::Tag> = html::tags(html_text).collect();
    let is_p = |t: &html::Tag, kind: html::TagKind| t.kind == kind && t.is("p");
    let mut edits: Vec<(usize, usize, String)> = Vec::new();
    let mut k = 0;
    while k < tags.len() {
        let t = &tags[k];
        k += 1;
        if !is_p(t, html::TagKind::Open) {
            continue;
        }
        let Some(id) = html::attr_value(&html_text[t.start..t.end], "id")
            .filter(|v| is_duokan_pair_id(v, "a_"))
        else {
            continue;
        };
        let Some(inner) = tags.get(k).filter(|x| {
            is_p(x, html::TagKind::Open) && html_text[t.end..x.start].trim().is_empty()
        }) else {
            continue;
        };
        let Some(c1) = (k + 1..tags.len()).find(|&m| is_p(&tags[m], html::TagKind::Close)) else {
            continue;
        };
        let Some(c2) = tags.get(c1 + 1).filter(|x| {
            is_p(x, html::TagKind::Close) && html_text[tags[c1].end..x.start].trim().is_empty()
        }) else {
            continue;
        };
        let inner_attrs = &html_text[inner.start + 2..inner.end - 1];
        edits.push((
            t.start,
            c2.end,
            format!(
                "<p id=\"{id}\"{inner_attrs}>{}</p>",
                &html_text[inner.end..tags[c1].start]
            ),
        ));
        k = c1 + 2;
    }
    if edits.is_empty() {
        html_text.to_string()
    } else {
        html::apply_edits(html_text, edits)
    }
}
/// 属性串里 `href` 的锚点（`href="x.html#f"` / `href='#f'` → `f`）。
pub(super) fn href_fragment(attrs: &str) -> Option<String> {
    html::attr_value(attrs, "href")
        .and_then(|v| html::split_href(v).1)
        .filter(|f| !f.is_empty())
        .map(str::to_string)
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
/// 弹窗模式（`popup`）用 `<aside epub:type="footnote">`：EPUB 3 标注脚注的写法，支持弹窗的阅读器据此认出注释。
/// 每条都带 `eink-note` 类：样式表里 `page-break-inside:avoid`，一条注释不被拆到两页（长过一页的阅读器照常断开）。
///
/// `id` 是解码后的值（注释键），拼进属性前转义。
fn footnote_block(id: &str, inner: &str, popup: bool) -> String {
    let id = xml_escape(id);
    if popup {
        format!("<aside epub:type=\"footnote\" id=\"{id}\" class=\"eink-note\">{inner}</aside>")
    } else {
        format!("<div id=\"{id}\" class=\"eink-note\">{inner}</div>")
    }
}

/// 有序列表来的注释放到章末时的 `<li>`：原 `<li>` 开标签（id、class、value 都在）加 `eink-note` 类，弹窗模式标 `epub:type="footnote"`。
fn footnote_list_item(item: &ListItem, inner: &str, popup: bool) -> String {
    let mut open = html::add_class(&item.li_open, "eink-note");
    if popup {
        open = html::set_attr(&open, "epub:type", "footnote");
    }
    format!("{open}{inner}</li>")
}

/// 章末注释区里的一条：普通块，或有序列表里的一项（相邻、同一个列表、编号连续的几项合成一个 `<ol start=N>`）。
enum Placed {
    Block(String),
    Item {
        file: String,
        ol_open: String,
        number: i64,
        li: String,
    },
}

/// 把章末注释区的各条拼起来（列表项按 [`Placed`] 的规则分组）。
fn render_placed(placed: &[Placed]) -> String {
    let mut out: Vec<String> = Vec::new();
    let mut k = 0;
    while k < placed.len() {
        match &placed[k] {
            Placed::Block(b) => {
                out.push(b.clone());
                k += 1;
            }
            Placed::Item {
                file,
                ol_open,
                number,
                ..
            } => {
                let mut lis: Vec<&str> = Vec::new();
                let mut next = *number;
                while let Some(Placed::Item {
                    file: f,
                    ol_open: o,
                    number: n,
                    li,
                }) = placed.get(k)
                {
                    if f != file || o != ol_open || *n != next {
                        break;
                    }
                    lis.push(li);
                    next += 1;
                    k += 1;
                }
                out.push(format!(
                    "{}\n{}\n</ol>",
                    html::set_attr(ol_open, "start", &number.to_string()),
                    lis.join("\n")
                ));
            }
        }
    }
    out.join("\n")
}

/// 标号 `<a>` 开标签改成指向本章 `#frag`：其它属性（`id` 回链落点、`class`……）原样保留，只改 `href`；弹窗模式标
/// `epub:type="noteref"`，跳转模式去掉标 noteref 的 `type`/`epub:type`（xochitl 不认）。
fn relinked_marker_open(open: &str, frag: &str, popup: bool) -> String {
    let tag = html::set_attr(open, "href", &format!("#{frag}"));
    if popup {
        return html::set_attr(&tag, "epub:type", "noteref");
    }
    let edits: Vec<(usize, usize, String)> = html::attrs(&tag)
        .into_iter()
        .filter(|a| {
            (a.is("type") || a.name.to_ascii_lowercase().ends_with(":type"))
                && a.value.split_whitespace().any(|v| v == "noteref")
        })
        .map(|a| (tag[..a.start].trim_end().len(), a.end, String::new()))
        .collect();
    if edits.is_empty() {
        tag
    } else {
        html::apply_edits(&tag, edits)
    }
}

/// 标号里的 `<img>`（多看等书用小图标当注释标号）加上 `eink-noteicon` 类：样式表把它限成一个字高
/// （xochitl 对没写宽高的 `<img>` 按图片本身的像素画，80×80 的图标在正文里撑成一大块，真机《甲午：摇摆的战争》）。
/// 已经有这个类的不再加（重优化不累积）；没有 `class` 属性的把它写在最前面。
fn mark_note_icons(content: &str) -> String {
    const CLASS: &str = "eink-noteicon";
    let mut edits = Vec::new();
    for t in html::tags(content) {
        if t.kind == html::TagKind::Close || !t.is("img") {
            continue;
        }
        let open = &content[t.start..t.end];
        if html::has_class(open, CLASS) {
            continue;
        }
        let new = match html::attr(open, "class") {
            Some(_) => html::add_class(open, CLASS),
            None => format!("<img class=\"{CLASS}\"{}", &open[4..]),
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
    // id → 带这个 id 的第一个开标签（此前每个链接都从头扫一遍全部标签，链接多的章节是平方级）
    let mut by_id: HashMap<&str, &html::Tag> = HashMap::new();
    for t in tags.iter().filter(|t| t.is_start()) {
        if let Some(id) = html::attr_value(&html_text[t.start..t.end], "id") {
            by_id.entry(id).or_insert(t);
        }
    }
    let mut counter = 0usize;
    let mut edits: Vec<(usize, usize, String)> = Vec::new();
    for e in a_elems(&tags) {
        let open = &html_text[e.start..e.open_end];
        let Some(frag) = html::attr_value(open, "href")
            .and_then(|h| h.strip_prefix('#'))
            .filter(|f| !f.is_empty())
        else {
            continue;
        };
        let content = &html_text[e.open_end..e.close_start];
        let imgs: Vec<&str> = html::tags(content)
            .filter(|t| t.kind != html::TagKind::Close && (t.is("img") || t.is("image")))
            .map(|t| &content[t.start..t.end])
            .collect();
        if imgs.is_empty() || !html::plain_text(content).trim().is_empty() {
            continue;
        }
        let noteish = |tag: &str| {
            html::attr_value(tag, "class").is_some_and(|v| v.to_ascii_lowercase().contains("note"))
        };
        let target = by_id.get(frag).copied();
        if !(noteish(open)
            || imgs.iter().any(|t| noteish(t))
            || target.is_some_and(|t| note_semantic(&html_text[t.start..t.end])))
        {
            continue;
        }
        counter += 1;
        let num = note_number(target.map_or("", |t| element_content(html_text, t)))
            .or_else(|| imgs.iter().find_map(|t| duokan_note_num(t)))
            .unwrap_or_else(|| counter.to_string());
        let in_sup = e.open_ix.checked_sub(1).is_some_and(|k| {
            tags[k].kind == html::TagKind::Open
                && tags[k].is("sup")
                && html_text[tags[k].end..e.start].trim().is_empty()
        });
        let label = if in_sup {
            xml_escape(&num)
        } else {
            format!("<sup>{}</sup>", xml_escape(&num))
        };
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
    let c = R
        .get_or_init(|| {
            Regex::new(r"^\s*(?:[\[［【(（]\s*(\d{1,4})\s*[\]］】)）]|(\d{1,4})\s*[.、．])")
                .unwrap()
        })
        .captures(&t)?;
    c.get(1)
        .or_else(|| c.get(2))
        .map(|m| m.as_str().to_string())
}

/// 开标签 `open` 那个元素的内容；找不到闭合标签返回空串。
fn element_content<'a>(html_text: &'a str, open: &html::Tag) -> &'a str {
    match html::find_close(html_text, open.end, open.name) {
        Some(close) => &html_text[open.end..close.start],
        None => "",
    }
}

/// 根元素 `<html>` 上没声明 `epub` 命名空间就补上（用了 `epub:type` 的文件不声明就不是合法 XML）。
pub(crate) fn ensure_epub_ns(doc: &str) -> String {
    let Some(t) = html::tags(doc).find(|t| t.kind != html::TagKind::Close && t.is("html")) else {
        return doc.to_string();
    };
    let open = &doc[t.start..t.end];
    if html::attrs(open).iter().any(|a| a.name == "xmlns:epub") {
        return doc.to_string();
    }
    let at = t.start + 5; // `<html` 之后
    format!(
        "{} xmlns:epub=\"http://www.idpf.org/2007/ops\"{}",
        &doc[..at],
        &doc[at..]
    )
}

// ===== 优化器：跨文件普通尾注收集 + 全书 id 去重 =====
// （break_footnote_cycles 管同文件裸锚点环、preserve_relink_footnotes 管 noteref+跨文件普通尾注，
//  两者产物再过 dedup_ids_in_chapter 保证 id 全书唯一——reMarkable 锚点是全书命名空间。）

/// 元素开标签是否带"注释"语义：epub:type/type/class 含 footnote|endnote|rearnote|note。
/// 只认语义确证的块 → 目录页/普通交叉引用的跨文件链接绝不会被误当尾注搬走。
fn note_semantic(open_tag: &str) -> bool {
    html::attrs(open_tag).iter().any(|a| {
        let n = a.name.to_ascii_lowercase();
        (n == "type" || n == "class" || n.ends_with(":type"))
            && a.value.to_ascii_lowercase().contains("note")
    })
}
/// href 里的**跨文件** fragment：`href="非空路径#frag"` → Some(frag)；同文件 `href="#frag"` → None。
pub(super) fn href_crossfile_fragment(attrs: &str) -> Option<String> {
    let (path, frag) = html::split_href(html::attr_value(attrs, "href")?);
    frag.filter(|f| !path.is_empty() && !f.is_empty())
        .map(str::to_string)
}

/// 一章里会被 [`preserve_relink_footnotes`] 搬运的注释引用：noteref marker（任意路径）+ 跨文件普通 `<a>`，解析成
/// (目标文件, id)（`name` = 本章 zip 路径）。优化器据此只搬**被引用**的注释块（[`collect_footnote_notes`] 的过滤集），
/// 未被引用的原地不动。与 `preserve_relink_footnotes` 认 marker 的口径完全一致（都走 [`a_elems`]），收集了的注释一定有人接。
pub fn referenced_note_keys(html_text: &str, name: &str) -> Vec<NoteKey> {
    let tags: Vec<html::Tag> = html::tags(html_text).collect();
    let mut out = Vec::new();
    for a in a_elems(&tags) {
        let open = &html_text[a.start..a.open_end];
        let Some(href) = html::attr_value(open, "href") else {
            continue;
        };
        if html::is_external(html::split_href(href).0) {
            continue;
        }
        if is_noteref(open)
            || href_crossfile_fragment(open).is_some()
            || marker_like_link(html_text, &tags, &a)
        {
            out.extend(note_key(name, href));
        }
    }
    out
}

/// 同 [`referenced_note_keys`]，只要锚点（不解析文件；测试用）。
#[cfg(test)]
fn referenced_note_frags(html_text: &str) -> Vec<String> {
    referenced_note_keys(html_text, "")
        .into_iter()
        .map(|(_, id)| id)
        .collect()
}

/// 优化器版注释收集：从一章里抽出**被引用(referenced)**的注释块——`<aside>`/`<p>`/`<li>`/`<div>` 且带注释
/// 语义([`note_semantic`])——按 id 建索引、从原位移除，交给 [`preserve_relink_footnotes`] 搬进引用
/// 它的那一章。**只搬 referenced 里的**（`referenced` = 本章里被引用的 id）：未被任何 marker 引用的块原样留在原处
/// （杜绝"移走又没人接=内容丢失"）。按元素（`html::parse_spans`）切，不再用正则（2026-09-28 审计：`<p/>` 会吞下一段、
/// 属性值里的 `>` 会截断）；外层块收了，里面的块就不再单独收；含嵌套 `<div>` 的 `<div>` 仍不搬（多半是包住整个注释区的容器）。
/// `require_semantic`：优化器传 true——对任意导入书要求块自带 footnote/note 语义（`note_semantic`），
/// 防把普通跨文件交叉引用误当尾注搬走；传 **false** 时"**id 被 marker 引用**"本身即注释的充分证据
/// （微读注释块 class 是混淆名，如 `class_s1r`，13·67 的 `<p>` 注释即此）。
///
/// `referenced` 与返回的 id 都是解码后的值（`id` 属性还原字符引用，见 `note_key`）。有序列表 `<ol><li>` 里的注释连同编号记下
/// （[`Note::list`]）；搬空了的 `<ol>`，以及只剩分隔线的注释区容器（带注释语义、没有 `id`）连同紧挨在前面的 `<hr>` 一起去掉——
/// 注释会在引用它的章末重新放一个带分隔线的注释区，原处只剩空列表和多出来的分隔线。
pub fn collect_footnote_notes(
    html_text: &str,
    referenced: &std::collections::HashSet<String>,
    require_semantic: bool,
) -> (String, Vec<(String, Note)>) {
    collect_footnote_notes_with(html_text, referenced, require_semantic, &HashMap::new())
}

/// 同 [`collect_footnote_notes`]；`backrefs`（注释 id → 引用它的标号 id，见 [`marker_backrefs`]）：块没有注释语义、但开头的
/// 回链指回引用它的标号时，也算注释（2026-10-05 定：注释识别和 KFX 统一成"互相链接的一对"）。
pub fn collect_footnote_notes_with(
    html_text: &str,
    referenced: &std::collections::HashSet<String>,
    require_semantic: bool,
    backrefs: &HashMap<String, HashSet<String>>,
) -> (String, Vec<(String, Note)>) {
    let mut index: Vec<(String, Note)> = Vec::new();
    // 快速路径：块只有在开标签带 `id="X"` 且 X 被引用时才会被搬走。本章任何位置都找不到一个被引用的
    // `id="…"` 值 → 结果必然与原文相同，不必解析（2026-09-24 审计实测这一步占章节变换耗时的大头，绝大多数章节其实没有注释块）。
    if !mentions_referenced_id(html_text, referenced) {
        return (html_text.to_string(), index);
    }
    let spans = html::parse_spans(html_text, 0, html_text.len());
    let mut taken: Vec<usize> = Vec::new();
    let mut taken_until = 0usize;
    // 已收的 id：同一章里两条注释用了同一个 id（《绝叫》两条不同的注释都叫 `footnote-3-15`），后一条留在原处——
    // 注释按 (文件, id) 进索引，两条都搬走只放得回一条，另一条就丢了（2026-10-06 审计：真书回归《绝叫》少了「专业漫画用纸……」）。
    let mut ids_taken: HashSet<String> = HashSet::new();
    let mut ol_cache: HashMap<usize, Vec<(usize, ListItem)>> = HashMap::new();
    for (si, sp) in spans.iter().enumerate() {
        if sp.open_start < taken_until
            || !sp.closed()
            || !matches!(sp.name.as_str(), "aside" | "p" | "li" | "div")
        {
            continue;
        }
        let open = &html_text[sp.open_start..sp.open_end];
        let inner = &html_text[sp.open_end..sp.close_start];
        if sp.name == "div" && inner.contains("<div") {
            continue; // 嵌套 div：原样保留不搬
        }
        // 锚点在块自己身上，或在块开头的 `<a id>`/`<a name>` 上（《罗杰疑案》`<p class="fncontent"><a id="zhu1" href="#zw1">[1]</a>…`）
        let own = html::attr_value(open, "id")
            .filter(|v| !v.is_empty())
            .map(crate::util::xml_unescape);
        let lead = || leading_anchor_id(inner);
        let id = match own {
            Some(i) if referenced.contains(i.as_ref()) => i,
            _ => match lead() {
                Some(i) => std::borrow::Cow::Owned(i),
                None => continue,
            },
        };
        let paired = || {
            backrefs
                .get(id.as_ref())
                .is_some_and(|m| starts_with_backlink(inner, m))
        };
        if referenced.contains(id.as_ref())
            && !ids_taken.contains(id.as_ref())
            && (!require_semantic || note_semantic(open) || paired())
        {
            ids_taken.insert(id.to_string());
            let list = if sp.name == "li" {
                list_item(html_text, &spans, si, &mut ol_cache)
            } else {
                None
            };
            index.push((
                id.into_owned(),
                Note {
                    inner: inner.to_string(),
                    list,
                },
            ));
            taken.push(si);
            taken_until = sp.close_end;
        }
    }
    if taken.is_empty() {
        return (html_text.to_string(), index);
    }
    let mut removed: Vec<(usize, usize)> = taken
        .iter()
        .map(|&si| (spans[si].open_start, spans[si].close_end))
        .collect();
    // 搬空的列表、只剩分隔线的注释区：从被搬走的块往上找
    let mut checked: HashSet<usize> = HashSet::new();
    for &si in &taken {
        let mut p = spans[si].parent;
        while let Some(pi) = p {
            if !checked.insert(pi) {
                break;
            }
            let sp = &spans[pi];
            let open = &html_text[sp.open_start..sp.open_end];
            let container = matches!(sp.name.as_str(), "ol" | "ul")
                || (note_semantic(open) && !matches!(sp.name.as_str(), "body" | "html"));
            if !container
                || !sp.closed()
                || html::attr(open, "id").is_some()
                || !leftover_is_empty(html_text, sp.open_end, sp.close_start, &removed)
            {
                break;
            }
            // 紧挨在前面的分隔线（中间只有空白）一起去掉
            let before = html::tags(&html_text[..sp.open_start]).last().filter(|t| {
                t.is_start() && t.is("hr") && html_text[t.end..sp.open_start].trim().is_empty()
            });
            removed.push((before.map_or(sp.open_start, |t| t.start), sp.close_end));
            p = sp.parent;
        }
    }
    removed.sort_by_key(|r| (r.0, std::cmp::Reverse(r.1)));
    (
        html::apply_edits(
            html_text,
            removed
                .into_iter()
                .map(|(s, e)| (s, e, String::new()))
                .collect(),
        ),
        index,
    )
}

/// `html[lo..hi]` 去掉 `removed` 里落在其中的区间、再去掉分隔线 `<hr>` 后，还有没有读者看得见的内容（口径同 `html::has_visible`）。
fn leftover_is_empty(html_text: &str, lo: usize, hi: usize, removed: &[(usize, usize)]) -> bool {
    let mut cut: Vec<(usize, usize)> = removed
        .iter()
        .copied()
        .filter(|&(s, e)| s >= lo && e <= hi)
        .collect();
    cut.sort();
    let mut rest = String::new();
    let mut pos = lo;
    for (s, e) in cut {
        if s >= pos {
            rest.push_str(&html_text[pos..s]);
        }
        pos = pos.max(e);
    }
    rest.push_str(&html_text[pos..hi]);
    let hrs: Vec<(usize, usize, String)> = html::tags(&rest)
        .filter(|t| t.is_start() && t.is("hr"))
        .map(|t| (t.start, t.end, String::new()))
        .collect();
    !html::has_visible(&html::apply_edits(&rest, hrs))
}

/// `html` 里有没有某个开标签的 `id` 属性值（还原字符引用后）在 `referenced` 里。只看标签（`html::tags`）里、前一个字节是空白或
/// 引号的 `id`（不分大小写），在那里按属性解析（`html::attrs`，和 [`collect_footnote_notes`] 取 id 用的是同一套：`id = "n1"`、单引号、
/// 不带引号都认）——正文里的 "id" 不解析、不分配。是开标签上 `id` 属性可能取到的值的超集，所以返回 `false` 时可以断定没有块会被搬走。
fn mentions_referenced_id(html: &str, referenced: &std::collections::HashSet<String>) -> bool {
    if referenced.is_empty() {
        return false;
    }
    html::tags(html).filter(|t| t.is_start()).any(|t| {
        let tag = &html[t.start..t.end];
        let b = tag.as_bytes();
        // `i` 处是 ASCII 字节 `i`/`I`，必是字符边界
        (1..b.len().saturating_sub(1)).any(|i| {
            b[i..i + 2].eq_ignore_ascii_case(b"id")
                && matches!(
                    b[i - 1],
                    b' ' | b'\t' | b'\n' | b'\r' | b'\x0c' | b'"' | b'\''
                )
                && html::attrs(&tag[i..]).first().is_some_and(|a| {
                    a.is("id") && referenced.contains(crate::util::xml_unescape(a.value).as_ref())
                })
        })
    })
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
    fix_duokan_markers_with(html, true)
}

/// 注释链接的前处理（优化器第二遍的前两步）：`drop_backlinks` 时先拆"标号↔注释"互指环（注释里的回链去链），
/// 再换多看标记（连同它注释块里的回链一起去链）；不去回链时两处回链都留着（profile `note_backlinks`）。
pub fn prepare_note_links(html: &str, drop_backlinks: bool) -> String {
    if drop_backlinks {
        fix_duokan_markers_with(&crate::htmlproc::break_footnote_cycles(html), true)
    } else {
        fix_duokan_markers_with(html, false)
    }
}

fn fix_duokan_markers_with(html: &str, unlink_backlinks: bool) -> String {
    let mut local = 0usize;
    // 标记替换（转义 img 版 / Calibre 真 img 版共用）：保留 href 与 `<a>` 自带 id（Calibre 形态的
    // 回链落点，丢了则注释里的回链悬空 → reMarkable 判互指整对丢弃）。
    // `escaped`：标记里的 <img> 是被转义成字面文字的（微读 CDN 图，离线显示成一串代码）；否则是真的 <img> 图标。
    let mut rewrite = |a_attrs: &str, img_inner: &str, whole: &str, escaped: bool| -> String {
        // "duokan-footnote" 类名有两种真实变体：常见形态在 img 自己的 class 上（`img_inner`）；
        // 2026-09-23 真机《甲午：摇摆的战争》核实还有一种把这个类挂在外层 <a> 上、img 自己只是普通
        // `class="exs"` 图标——两种都得认，只查 img_inner 会漏判、图标原样穿透到 xochitl 按固有
        // 像素撑成巨大方块（真机坐实）。
        let is_duokan =
            img_inner.contains("duokan-footnote") || a_attrs.contains("duokan-footnote");
        match (is_duokan, href_fragment(a_attrs)) {
            (true, Some(frag)) => {
                let id_attr = html::attr_value(a_attrs, "id")
                    .map(|v| format!(" id=\"{v}\""))
                    .unwrap_or_default();
                if escaped {
                    // 转义成文字的那种：原书显示的就是一串 `<img …>` 代码、图在网上离线看不到，换成它自己 alt 里的注释序号（"注释12" → 12）
                    local += 1;
                    let num = duokan_note_num(img_inner).unwrap_or_else(|| local.to_string());
                    format!(
                        "<a href=\"#{frag}\"{id_attr}><sup>{}</sup></a>",
                        xml_escape(&num)
                    )
                } else {
                    // 真图标：原样保留（不再换成上标数字——那是多出来的字，2026-09-29 定），只加 `eink-noteicon` 类限成一个字高
                    // （没写宽高的 80×80 图标在 xochitl 上撑成一大块，真机《甲午：摇摆的战争》）
                    format!(
                        "<sup><a href=\"#{frag}\"{id_attr}>{}</a></sup>",
                        mark_note_icons(&format!(
                            "<img{}/>",
                            img_inner.trim_end().trim_end_matches('/')
                        ))
                    )
                }
            }
            _ => whole.to_string(),
        }
    };
    let markers = duokan_footnote_re()
        .replace_all(html, |c: &regex::Captures| {
            rewrite(
                c.get(1).unwrap().as_str(),
                c.get(2).unwrap().as_str(),
                c.get(0).unwrap().as_str(),
                true,
            )
        })
        .into_owned();
    let markers = duokan_footnote_img_re()
        .replace_all(&markers, |c: &regex::Captures| {
            rewrite(
                c.get(1).unwrap().as_str(),
                c.get(2).unwrap().as_str(),
                c.get(0).unwrap().as_str(),
                false,
            )
        })
        .into_owned();
    // 去链注释块里的回链（#c_X_Y），否则 reMarkable 判互指对整对丢弃、正向也点不动（只在去回链的模式）。
    let unlinked = if unlink_backlinks {
        unlink_duokan_backlinks(&markers)
    } else {
        markers
    };
    // 拍平注释块的无效嵌套 <p>（成簇相邻时会被 reMarkable 吞掉一条），锚点保留。
    flatten_duokan_note_blocks(&unlinked)
}

/// 优化器专用脚注处理：**保留标号原样**（上标、数字、图标都不动，前后一个字不加），只把 `<a>` 的 href 规整成同章 `#frag`，
/// 被引用的注释块（index 提供，跨文件也行；键是 (注释所在文件, id)，`name` = 本章 zip 路径）移到本章末尾 `<div class="footnotes">`。
/// - `Anchor`（跳转）：标号不带 `epub:type`（xochitl 不认），点标号跳到章末、用阅读器的"返回"回来；
/// - `Popup`（弹窗）：标号标 `epub:type="noteref"`、注释块是 `<aside epub:type="footnote">`，支持的阅读器点标号弹窗显示。
///
/// marker 按元素认（[`a_elems`]，与 [`referenced_note_keys`] 同一口径）：`<sup>` 整个包住的 noteref、其余 noteref、跨文件普通 `<a>`。
/// 2026-09-29 起不再在标号后追加 `[N]`（维护者定：严格说多了字符）。
///
/// 标号原样：`<a>` 只改 `href`（弹窗模式另标 `epub:type`），`id`（注释回链的落点）、`class` 等属性和外面的 `<sup>` 都不动——此前整个
/// 重写成 `<a href>`，丢了 `id`，注释里指回 `#fnref1` 的回链成了死链（pandoc 的写法）。有序列表来的注释连同编号放成 `<ol start=N>`。
pub fn preserve_relink_footnotes(
    html_text: &str,
    name: &str,
    index: &std::collections::HashMap<NoteKey, Note>,
    mode: crate::optimize::FootnoteMode,
) -> String {
    use crate::optimize::FootnoteMode;
    if index.is_empty() {
        return html_text.to_string();
    }
    let popup = mode == FootnoteMode::Popup;
    let mut appended: Vec<Placed> = Vec::new();
    let mut seen: std::collections::HashSet<NoteKey> = std::collections::HashSet::new();

    let mut make = |frag: &str, key: &NoteKey, note: &Note, open: &str, content: &str| -> String {
        if seen.insert(key.clone()) {
            // 注释放章末。不加可点回链——真机实测 reMarkable 会丢弃"marker↔注释"互指里较晚那条
            // (注释回链)，加了也点不了、反成死链迷惑人。返回靠阅读器原生。
            let inner = deprefix_footnote_hrefs(&note.inner);
            appended.push(match &note.list {
                Some(item) => Placed::Item {
                    file: key.0.clone(),
                    ol_open: item.ol_open.clone(),
                    number: item.number,
                    li: footnote_list_item(item, &inner, popup),
                },
                None => Placed::Block(footnote_block(&key.1, &inner, popup)),
            });
        }
        format!(
            "{}{}</a>",
            relinked_marker_open(open, frag, popup),
            mark_note_icons(content)
        )
    };

    let tags: Vec<html::Tag> = html::tags(html_text).collect();
    let elems = a_elems(&tags);
    let only_ws = |a: usize, b: usize| html_text[a..b].trim().is_empty();
    // 每个 <a>：(href, 锚点原文, 注释键, 注释)——只有键在 index 里的才算 marker。
    let lookup = |e: &AElem| -> Option<(&str, &str, NoteKey, &Note)> {
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
    // 1) <sup> 整体包裹的 noteref；2) 剩余裸 noteref（两遍只决定注释在章末的先后，`<sup>` 本身不动，只改里面的 `<a>`）
    for pass_sup in [true, false] {
        for (i, e) in elems.iter().enumerate() {
            if done[i] || !is_noteref(&html_text[e.start..e.open_end]) {
                continue;
            }
            if pass_sup {
                let sup_open = e.open_ix.checked_sub(1).map(|k| &tags[k]).filter(|t| {
                    t.kind == html::TagKind::Open && t.is("sup") && only_ws(t.end, e.start)
                });
                let sup_close = tags.get(e.close_ix + 1).filter(|t| {
                    t.kind == html::TagKind::Close && t.is("sup") && only_ws(e.end, t.start)
                });
                if sup_open.is_none() || sup_close.is_none() {
                    continue;
                }
            }
            let Some((_, frag, key, note)) = lookup(e) else {
                done[i] = true; // 查不到（不是收集来的注释）：三遍都不会改它
                continue;
            };
            done[i] = true;
            edits.push((
                e.start,
                e.end,
                make(
                    frag,
                    &key,
                    note,
                    &html_text[e.start..e.open_end],
                    &html_text[e.open_end..e.close_start],
                ),
            ));
        }
    }
    // 3) 普通 <a href="[其他文件]#frag">：非 noteref，但 frag 已被 collect_footnote_notes 收进 index（确证是注释：有注释语义，
    //    或和它互相链接）→ 改同章锚点 + 注释搬章末。同文件的也算（《罗杰疑案》注释和正文在同一个文件）。目标不在 index 的
    //    （目录/交叉引用）不动。
    for (i, e) in elems.iter().enumerate() {
        if done[i]
            || html::attr_value(&html_text[e.start..e.open_end], "href")
                .and_then(|h| html::split_href(h).1)
                .is_none_or(|f| f.is_empty())
        {
            continue;
        }
        let Some((_, frag, key, note)) = lookup(e) else {
            continue;
        };
        let new = make(
            frag,
            &key,
            note,
            &html_text[e.start..e.open_end],
            &html_text[e.open_end..e.close_start],
        );
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
    let block = format!(
        "\n<hr/>\n<div class=\"footnotes\">\n{}\n</div>\n",
        render_placed(&appended)
    );
    // 按 `html::body_range` 找 `</body>`（不分大小写；此前只认小写，`</BODY>` 的书注释区落到 `</html>` 后面）
    let out = match html::body_range(&out) {
        Some((_, pos)) => format!("{}{}{}", &out[..pos], block, &out[pos..]),
        None => format!("{out}{block}"),
    };
    if popup {
        ensure_epub_ns(&out)
    } else {
        out
    }
}

#[cfg(test)]
mod footnote_inline_tests;

#[cfg(test)]
mod optimizer_footnote_tests;
