//! 清洗层（规则最初对照上游 Calibre 清洗脚本移植）。作用于**解包后的条目表**，由优化器在各遍处理之前调用
//! （`OptimizeOpts::wash`），书库生成与命令行 `epub-optimize` 走同一份。
//!
//! 规则（与 Calibre 参数一一对应）：
//! 1. 伪 DRM 剥离（= `strip_pseudo_drm.py`）：`META-INF/encryption.xml` 只列样式/字体/脚本 → 丢弃这些文件 +
//!    encryption.xml + OPF manifest 项；列了正文/图片/导航 = 真 DRM → **报错停下**。
//! 2. 字体字号解锁（独立 .css、`<style>`、`style=""` 三处，规则见 `crate::cssunlock`）：`font-family`、`line-height`、
//!    绝对字号去掉，相对字号保留（正文整体那一层的除外），`font` 简写只留粗斜体，`background` 简写只留颜色
//!    （背景图去掉：xochitl 把背景图平铺满页盖住正文，真机《飘》）。颜色、对齐等别的样式不动（用户 2026-09-29）。
//! 3. 边距归零（= `--margin-* 0`）：body/html/@page 的 margin/padding 删掉，并注入 `html,body{margin:0;padding:0}`。
//! 4. 段距归零 + 首行缩进（= `--remove-paragraph-spacing --remove-paragraph-spacing-indent-size 2`）：p/div 的
//!    上下 margin/padding 归零（左右保留：blockquote/列表缩进不伤），`p{text-indent:2em}`；`keep_para_spacing` 时
//!    只注缩进（= `WASH_KEEP_PARA_SPACING=1`）。类规则须带元素名才压得过书自带类规则（见书架白皮书 §03y 的七条规则；xochitl 不认 `!important`）。
//! 5. 空页清理：正文无文字无图（Calibre MOBI 转出的 `mbppagebreak` 独占页）→ 从 spine/manifest/zip 删除，
//!    目录里指向它的条目改指下一篇。
//! 6. 自动目录（= `--use-auto-toc --level1-toc //h:h1 --level2-toc //h:h2`）：缺省**仅在书无目录时**从 h1/h2 生成
//!    `toc.ncx` + `nav.xhtml`（xochitl 两者都认）；`AutoToc::Always` 强制重建（原目录坏掉的书）。
//! 7. 单标签重复 `id=` 折叠（`collapse_dup_id_attrs`）：非法 XHTML 会让 xochitl 整章白屏，这里先修、质量门再拦。
//! 8. 全书 id 去重（`ids.rs`）：跨文件重复的 id 改名，全书指向它的链接一起改。
//! 9. 规范整理（`normalize.rs`，最后一步）：XHTML 修成合法 XML（DOCTYPE、命名实体、裸 `&`/`<`、控制字符、空元素、多余闭合标签），
//!    OPF 升级到 EPUB 3（`dcterms:modified` 固定值、唯一标识符、`opf:` 属性改 `refines`），缺导航文档的按 NCX 生成，guide 写成 landmarks；
//!    NCX 与 `<spine toc>` 保留（xochitl 靠它）。
//!
//! 标签、属性、纯文本、可见性一律走 `crate::html`（完整属性名、两种引号、注释不当标签）。
//! 全部规则幂等：注入块带 `class="eink-wash"` 标记，重复过不再叠加。
use crate::html::{self, Edit};
use crate::htmlproc::collapse_dup_id_attrs;
// zip 条目与 zip 内 posix 路径工具已迁到 `epubzip`；这里 re-export，保住 `crate::wash::Entry`/`wash::resolve` 等旧路径。
pub use crate::epubzip::{dir_of, is_html, is_html_entry, percent_decode, posix_norm, relative_to, resolve, Entry};
use crate::util::{is_image_ext, xml_escape};
use regex::Regex;
use std::borrow::Cow;
use std::collections::{HashMap, HashSet};
use std::sync::OnceLock;

// 按职责拆成子模块（原 `wash.rs` 一个文件 2000+ 行）；兄弟模块之间的互相调用走这层（各子模块 `use super::*`）。
mod cover;
mod css;
mod dead_refs;
mod drm;
mod empty_pages;
mod ids;
mod layout;
mod ncx_fix;
pub mod normalize;
pub mod opf;
mod paginate;
mod toc;
mod typeset;

// 对外（优化器、质量门、书库、统计）用到的项；其余只在清洗层内部用。
pub use self::cover::ensure_cover_declared;
pub use self::css::{filter_css, wash_html};
pub use self::drm::{encrypted_targets, real_drm_items, PSEUDO_DRM_SAFE_EXTS};
pub(crate) use self::drm::cipher_reference_re;
pub use self::opf::{manifest_items, opf_dc, parse_opf, tag_attr, ManifestItem, Opf, OpfDc};
pub use self::toc::{href_re, is_toc_file, toc_entry_count};
pub use self::typeset::{count_dup_id_tags, wash_css};
pub use crate::html::plain_text;

use self::css::*;
use self::dead_refs::*;
use self::drm::strip_pseudo_drm;
use self::empty_pages::*;
use self::ids::dedup_ids_across_book;
use self::layout::*;
use self::ncx_fix::*;
use self::opf::{find_opf, opf_book_title, opf_unique_identifier};
use self::paginate::paginate_sections;
pub(crate) use self::paginate::is_toc_like_page;
use self::toc::*;
use self::typeset::*;

#[cfg(test)]
mod tests;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AutoToc {
    Off,
    /// 书无目录（nav/ncx 缺失或零条目）时生成。
    IfMissing,
    Always,
}

/// 正文排版语言（决定首行缩进/段落习惯）。`Auto` 由 `wash_entries` 按全书 CJK/拉丁字符占比判定。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LangMode {
    Auto,
    /// 中文习惯：首行缩进 2em（两个全角字）、段间无空。
    Cjk,
    /// 拉丁习惯：首行缩进 1.2em、标题后首段不缩进。
    Latin,
}

#[derive(Clone, Debug, PartialEq)]
pub struct WashOpts {
    /// 保留原书段间距（诗集/剧本靠空行分节）。
    pub keep_para_spacing: bool,
    pub auto_toc: AutoToc,
    /// 剥掉的 CSS 属性（小写）。缺省与 host `--filter-css` 一致。
    pub filter_props: Vec<String>,
    /// 正文排版语言（`Auto`=自动探测）。
    pub lang: LangMode,
    /// 章节分页：章标题独立一页、节与节/节与章之间分页（见 `paginate.rs`）。文字书缺省开，漫画自动跳过。
    pub paginate: bool,
}

impl Default for WashOpts {
    fn default() -> Self {
        WashOpts { keep_para_spacing: false, auto_toc: AutoToc::IfMissing, filter_props: DEFAULT_FILTER_PROPS.iter().map(|s| s.to_string()).collect(), lang: LangMode::Auto, paginate: true }
    }
}

/// 注入排版规则的外链 css 文件名（放 OPF 同目录）。真机坐实（2026-09-04《缩进诊断6》/《飘》）：
/// **xochitl 只认外链 `.css` 文件里的规则，完全无视内联 `<style>` 块和元素 `style=` 属性**——所以
/// 排版规则（首行缩进/边距）必须写成外链 css 才在 xochitl 生效（KOReader/crengine 两者都认）。
/// ⚠ xochitl 的 css 解析器很脆：**只用裸元素选择器**（`p`/`body`），一条类/复杂选择器就可能让整表失效
/// （《缩进诊断5》带 `.big` 类规则时整表不生效，diag6 纯 `p{}` 生效）。
const WASH_CSS_NAME: &str = "eink-wash.css";

/// zip 条目是不是清洗层写的样式表（任意目录下的 `eink-wash.css`）。
pub fn is_wash_css_name(name: &str) -> bool {
    name.rsplit('/').next() == Some(WASH_CSS_NAME)
}

// 要解锁的属性。怎么解（整条去掉，还是只去掉字体、背景图、绝对字号）见 `crate::cssunlock`。
// background / background-image：书常在 body/分卷页用 CSS 背景图（装饰纹样、分卷插画）。xochitl **无视
// no-repeat / background-size** → 把背景图**平铺**满页盖住正文（真机《飘》body.fen 的 `background:url() no-repeat`
// 被铺成多幅）；`background` 简写里的颜色保留成 `background-color`。
// line-height（2026-09-27 用户定）：与字号字体同理，书写死行高会让设备的"行距"设置不起作用。
pub const DEFAULT_FILTER_PROPS: &[&str] = &["font-family", "font-size", "font", "line-height", "background-image", "background"];
const WASH_MARK: &str = "eink-wash";

#[derive(Clone, Debug, Default, PartialEq, serde::Serialize)]
pub struct WashReport {
    pub pseudo_drm_stripped: Vec<String>,
    pub css_files: usize,
    pub html_files: usize,
    pub empty_pages_removed: Vec<String>,
    pub toc_generated: usize,
    pub dup_id_tags_collapsed: usize,
    /// `toc.ncx` 的 `dtb:uid` 跟 OPF 标识符不一致、被改到一致（0 或 1——一本书只有一个 ncx）。
    pub ncx_uid_fixed: usize,
    /// 书自带的扁平目录（"第X部　编号　章名"排版惯例）被重建成两级后的条目数；0＝没检测到这种
    /// 惯例、原样没动。
    pub toc_parts_restructured: usize,
    /// `toc.ncx` 里指向外部 DTD 的 `<!DOCTYPE>` 声明被剥掉（0 或 1）。
    pub ncx_doctype_stripped: usize,
    /// manifest 里 NCX 条目的 `id` 被改成 `"ncx"`（0 或 1）。见 `fix_ncx_manifest_id`。
    pub ncx_manifest_id_fixed: usize,
    /// 指向书内不存在文件的 `<img>` / 字体全缺的 `@font-face` 被去掉的个数。见 `drop_dead_refs`。
    pub dead_refs_removed: usize,
    /// 章节分页新拆出来的文件数（0＝没有需要拆的章节）。见 `paginate.rs`。
    pub sections_paginated: usize,
    /// 分页时搬到引用处那一份的注释块数。
    pub paginate_notes_moved: usize,
    /// 书自带目录漏掉、分页时补进目录的节数。
    pub toc_sections_added: usize,
    /// 书自带目录（NCX）指错位置、按书里的目录页或下一个文件核实后改指的条目数。见 `toc::repair_ncx_targets`。
    pub ncx_targets_repaired: usize,
    /// 跨文件重复、被改名的 id 数（全书指向它们的链接一起改）。见 `ids.rs`。
    pub dup_ids_renamed: usize,
    /// 文件末尾删掉的空元素/换行数（章尾空白页）。
    pub trailing_blanks_removed: usize,
    /// 去掉了下边距/之后分页的样式表数（包住章节结尾的容器）。
    pub tail_spacing_rules_fixed: usize,
    /// 规范整理的 XML 修复计数。见 `normalize.rs`。
    pub xml_fixes: normalize::XmlFixes,
    /// OPF 被改写（升级到 EPUB 3 或补必需项）：0 或 1。
    pub epub3_upgraded: usize,
    /// 按 NCX 新生成的导航文档条目数（0＝书本来就有 nav）。
    pub nav_generated: usize,
    /// 从 `<guide>` 写进 nav 的 landmarks 条数。
    pub landmarks_added: usize,
    /// 没有 NCX 的书按 nav 生成的 NCX 条目数（xochitl 读目录靠 NCX）。
    pub ncx_generated: usize,
}


// ───────────────────────── 入口 ─────────────────────────

/// 全书 CJK vs 拉丁字符占比 → 主语言（Han 字数 ≥ 拉丁字母数 = Cjk）。扫全部 html 正文，早停够量即定。
fn detect_dominant_script(entries: &[Entry]) -> LangMode {
    let (mut han, mut latin) = (0u64, 0u64);
    for e in entries.iter().filter(|e| is_html_entry(&e.name, &e.data) && !is_toc_file(&e.name)) {
        let Ok(t) = std::str::from_utf8(&e.data) else { continue };
        for ch in plain_text(t).chars() {
            if matches!(ch, '\u{4E00}'..='\u{9FFF}' | '\u{3400}'..='\u{4DBF}' | '\u{F900}'..='\u{FAFF}') {
                han += 1;
            } else if ch.is_ascii_alphabetic() {
                latin += 1;
            }
        }
        if han + latin > 20_000 {
            break; // 够量即判，不必扫全书
        }
    }
    if han >= latin {
        LangMode::Cjk
    } else {
        LangMode::Latin
    }
}

/// 对条目表就地清洗。真 DRM 返回 Err（调用方应整体失败、原样不动）。
pub fn wash_entries(entries: &mut Vec<Entry>, opts: &WashOpts) -> Result<WashReport, String> {
    wash_entries_detect(entries, opts).map(|(rep, _)| rep)
}

/// 同 [`wash_entries`]，另返回漫画识别结果（`comic_detect::is_comic`，分页前判一次；优化器直接用，不再判第二遍——
/// 分页只拆文件，不改图片数和字数，判定结果不变）。
pub(crate) fn wash_entries_detect(entries: &mut Vec<Entry>, opts: &WashOpts) -> Result<(WashReport, bool), String> {
    let mut rep = WashReport::default();
    strip_pseudo_drm(entries, &mut rep)?;
    remove_empty_pages(entries, &mut rep);
    drop_dead_refs(entries, &mut rep);
    // Auto → 探测主语言，解析成具体 Cjk/Latin 再逐文件注排版（探测在剥空页之后、注样式之前）。
    let opts = if opts.lang == LangMode::Auto {
        let mut o = opts.clone();
        o.lang = detect_dominant_script(entries);
        o
    } else {
        opts.clone()
    };
    let opts = &opts;
    // 书的语言标签：OPF `dc:language` 优先，没有就按探测到的主语言。补给缺 lang 的 <html>。
    let opf_idx = find_opf(entries);
    let lang_tag = book_lang_tag(entries, opf_idx, opts.lang);
    // 新建目录时的标题按书的语言（中文"目录"、其它"Contents"）。
    let heading = toc_title(opts.lang);
    // 外链 wash css 的 zip 路径（放 OPF 同目录；无 OPF 兜底放根）。排版规则写这里、逐 html 加 <link>——
    // xochitl 只认外链 css（内联 <style> 无视），见 WASH_CSS_NAME 注。
    let css_path = match opf_idx {
        Some(i) => {
            let d = dir_of(&entries[i].name);
            if d.is_empty() { WASH_CSS_NAME.to_string() } else { format!("{d}/{WASH_CSS_NAME}") }
        }
        None => WASH_CSS_NAME.to_string(),
    };
    // 书的样式表里写了首行缩进的类（英文首段顶格时不留在 eink-flush 上，见 `typeset::flush_first_para_after_heading`）
    let mut indent_classes: HashSet<String> = HashSet::new();
    for e in entries.iter_mut().filter(|e| e.name.to_ascii_lowercase().ends_with(".css")) {
        if let Ok(t) = std::str::from_utf8(&e.data) {
            let css = filter_css(t, opts);
            if opts.lang == LangMode::Latin {
                indent_classes.extend(indent_classes_of(&css));
            }
            e.data = css.into_bytes();
            rep.css_files += 1;
        }
    }
    for e in entries.iter_mut() {
        if is_html_entry(&e.name, &e.data) && !is_toc_file(&e.name) {
            if let Ok(t) = std::str::from_utf8(&e.data) {
                let (out, dups) = wash_html_with(t, opts, &indent_classes);
                let href = relative_to(dir_of(&e.name), &css_path);
                let out = inject_css_link(&out, &href);
                let out = ensure_html_lang(&align_classes(&out), &lang_tag);
                rep.dup_id_tags_collapsed += dups;
                e.data = out.into_bytes();
                rep.html_files += 1;
            }
        }
    }
    add_wash_css_entry(entries, opf_idx, &css_path, &wash_css(opts));
    fix_ncx_manifest_id(entries, &mut rep);
    // 目录指错位置的先修好：后面的分部重建、分页都按目录找标题
    repair_ncx_targets(entries, &mut rep);
    restructure_existing_toc_parts(entries, opts.auto_toc, heading, &mut rep);
    auto_toc(entries, opts.auto_toc, heading, &mut rep);
    // 分页放在自动目录之后：自动目录给标题补的 id 已经在，分页改写目录链接时能对上。漫画不拆。
    let comic = crate::comic_detect::is_comic(entries);
    if opts.paginate && !comic {
        paginate_sections(entries, heading, &mut rep);
    }
    // 全书 id 去重放在分页之后（拆出来的份不会新增重复 id，但链接要按拆好后的文件改）。
    dedup_ids_across_book(entries, &mut rep);
    // 章尾空白页放在分页之后：拆出来的每一份文件末尾也要清。
    remove_chapter_end_blanks(entries, &mut rep);
    strip_ncx_doctype(entries, &mut rep);
    // 规范整理：XML 修复、升级 EPUB 3、导航文档（见 `normalize.rs`）。放最后：前面各步新写的文件也要过一遍；
    // 升级可能补了标识符，NCX 的 dtb:uid 在它之后对齐。
    normalize::normalize_book(entries, &lang_tag, heading, &mut rep);
    fix_ncx_uid(entries, &mut rep);
    Ok((rep, comic))
}

/// 书的语言标签：OPF `dc:language` 优先，没有就按主语言（`Latin` → en，其余 zh）。
fn book_lang_tag(entries: &[Entry], opf_idx: Option<usize>, lang: LangMode) -> String {
    opf_idx
        .and_then(|i| {
            static DC_LANG: OnceLock<Regex> = OnceLock::new();
            let t = String::from_utf8_lossy(&entries[i].data);
            DC_LANG.get_or_init(|| Regex::new(r#"(?s)<dc:language\b[^>]*>\s*([A-Za-z]{2,3}(?:-[A-Za-z0-9]+)*)\s*</dc:language>"#).unwrap()).captures(&t).map(|c| c[1].to_string())
        })
        .unwrap_or_else(|| if lang == LangMode::Latin { "en".into() } else { "zh".into() })
}

/// **只做规范整理**（EPUB 3），和清洗层最后一步是同一套：NCX 去 DOCTYPE → XML 修复 → OPF 升级 3.0 → 导航文档与 landmarks → NCX
/// `dtb:uid` 对齐 OPF。给 `ebook-meta` 用（2026-09-30 用户：改元数据写出的书也要和 booklib 一样符合 EPUB 3）。不做排版、分页等清洗。
pub fn normalize_epub3(entries: &mut Vec<Entry>) -> WashReport {
    let mut rep = WashReport::default();
    let lang = detect_dominant_script(entries);
    let lang_tag = book_lang_tag(entries, find_opf(entries), lang);
    strip_ncx_doctype(entries, &mut rep);
    normalize::normalize_book(entries, &lang_tag, toc_title(lang), &mut rep);
    fix_ncx_uid(entries, &mut rep);
    rep
}

/// 新增（或重优化时更新）外链 wash css 文件，并往 OPF manifest 补一条 `<item>`（幂等）。
fn add_wash_css_entry(entries: &mut Vec<Entry>, opf_idx: Option<usize>, css_path: &str, content: &str) {
    if let Some(e) = entries.iter_mut().find(|e| e.name == css_path) {
        e.data = content.as_bytes().to_vec();
    } else {
        entries.push(Entry { name: css_path.to_string(), data: content.as_bytes().to_vec() });
    }
    if let Some(oi) = opf_idx {
        let opf_dir = dir_of(&entries[oi].name).to_string();
        let text = String::from_utf8_lossy(&entries[oi].data);
        if manifest_items(&text).iter().any(|it| resolve(&opf_dir, &percent_decode(it.href)) == css_path) {
            return;
        }
        let href = crate::epubzip::href_to(&opf_dir, css_path, "");
        if let Some(t) = opf::insert_manifest_items(&text, &[opf::NewItem { id: "eink-wash-css", href: &href, media_type: "text/css", properties: "" }]) {
            entries[oi].data = t.into_bytes();
        }
    }
}

/// 比较标题文字用：去掉所有空白（含全角空格）。
pub(super) fn squash_ws(t: &str) -> String {
    t.chars().filter(|c| !c.is_whitespace()).collect()
}

/// 全书链接改写的一处链接：所在文件、链接值原文、解析出的目标文件与锚点（锚点原文，未解码）。
pub(super) struct Link<'a> {
    pub file: &'a str,
    pub value: &'a str,
    pub target: String,
    pub frag: Option<&'a str>,
}

/// 全书（html、NCX、OPF）的 `href`/`src`/`xlink:href` 改写：`f` 返回新值就替换（保留原引号）。书外链接不回调；
/// `skip(条目名)` 为真的条目不动；OPF 里只改 `<guide>` 这类引用，manifest 的 `<item href>` 是文件本身的声明、从不改
/// （2026-09-28 审计：空页删除没删掉单引号 OPF 的 item 时，把它的 href 改成了邻页，spine 就重复了一章）。
/// 空页清理、全书 id 去重、分页后的链接改写共用。返回改了的条目数。
pub(super) fn rewrite_book_links(entries: &mut [Entry], skip: impl Fn(&str) -> bool, mut f: impl FnMut(&Link) -> Option<String>) -> usize {
    let mut changed = 0;
    for e in entries.iter_mut() {
        let l = e.name.to_ascii_lowercase();
        let is_opf = l.ends_with(".opf");
        if skip(&e.name) || !(is_opf || l.ends_with(".ncx") || is_html_entry(&e.name, &e.data)) {
            continue;
        }
        let Ok(text) = std::str::from_utf8(&e.data) else { continue };
        let name = e.name.as_str();
        let new = html::edit_attrs(text, &["href", "src", "xlink:href"], |t, a| {
            if is_opf && opf::is_local(t.name, "item") {
                return Edit::Keep;
            }
            let (p, _) = html::split_href(a.value);
            if html::is_external(p) {
                return Edit::Keep;
            }
            let (target, frag) = crate::epubzip::resolve_href(name, a.value);
            match f(&Link { file: name, value: a.value, target, frag }) {
                Some(v) if v != a.value => Edit::Set(v),
                _ => Edit::Keep,
            }
        });
        if let Cow::Owned(new) = new {
            e.data = new.into_bytes();
            changed += 1;
        }
    }
    changed
}
