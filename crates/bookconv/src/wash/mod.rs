//! 清洗层（规则最初对照上游 Calibre 清洗脚本移植）。作用于**解包后的条目表**，由优化器在各遍处理之前调用
//! （`OptimizeOpts::wash`），书库生成与命令行 `epub-optimize` 走同一份。
//!
//! 规则（与 Calibre 参数一一对应）：
//! 1. 伪 DRM 剥离（= `strip_pseudo_drm.py`）：`META-INF/encryption.xml` 只列样式/字体/脚本 → 丢弃这些文件 +
//!    encryption.xml + OPF manifest 项；列了正文/图片/导航 = 真 DRM → **报错停下**。
//! 2. 字体字号解锁（独立 .css、`<style>`、`style=""` 三处，规则见 `crate::cssunlock`）：`font-family`、`line-height`、
//!    绝对字号去掉，相对字号保留（正文整体那一层的除外），`font` 简写只留粗斜体，`background` 简写只留颜色
//!    （背景图去掉：xochitl 把背景图平铺满页盖住正文，真机《飘》）。颜色、对齐等别的样式不动（用户 2026-09-29）。
//! 3. 边距归零（= `--margin-* 0`）：body/html/@page 的 margin/padding 删掉（不另注入规则）。
//! 4. 段距归零 + 首行缩进（= `--remove-paragraph-spacing --remove-paragraph-spacing-indent-size 2`）：p/div 的
//!    上下 margin/padding 归零（左右保留：blockquote/列表缩进不伤），`p{text-indent:2em}`；`keep_para_spacing` 时
//!    只注缩进。类规则须带元素名才压得过书自带类规则（见书架白皮书 §03y 的七条规则；xochitl 不认 `!important`）。
//! 5. 空页清理：正文无文字无图（Calibre MOBI 转出的 `mbppagebreak` 独占页）→ 从 spine/manifest/zip 删除，
//!    目录里指向它的条目改指下一篇。
//! 6. 自动目录（= `--use-auto-toc --level1-toc //h:h1 --level2-toc //h:h2`）：缺省**仅在书无目录时**从 h1/h2 生成
//!    `toc.ncx` + `nav.xhtml`（xochitl 两者都认）；`AutoToc::Always` 强制重建（原目录坏掉的书）。
//!    定章节（`chapters.rs`，文字书）：按目录层级定书/卷、章、节，漏掉的节补进目录、目录改指到文件中间的标题。
//!    **不拆文件**（2026-10-06 用户定：章节不强制分页，原书的文件结构原样保留）。
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
mod chapters;
mod cover;
mod css;
mod dead_refs;
mod drm;
mod empty_pages;
mod encoding;
pub mod fonts;
mod html5fix;
mod ids;
mod kindle_rules;
mod layout;
mod ncx_fix;
pub mod normalize;
pub mod opf;
mod safe_names;
mod toc;
mod typeset;

// 对外（优化器、质量门、书库、统计）用到的项；其余只在清洗层内部用。
pub use self::cover::{ensure_cover_declared, prepend_cover_page};

/// 「照 Send to Kindle 的规则统一」（[`WashOpts::kindle_rules`]，掌阅、Move 的文字书）这一路自己的版本，只进书库指纹（`u` 段），
/// 不写进书里的优化标记：这一路改了只让开了它的书过期，内容没变的书重建出逐字节相同的 EPUB、设备上不重传
/// （升 `OPTIMIZE_VERSION` 会改标记，掌阅、Move 上全部文字书都得重传）。
/// - 1（2026-10-08）：标签缺省样式、正文字体、body 左右边距、文字对比度（指纹里写 `u`）。
/// - 2（2026-10-09）：原书没有封面页时补一页（[`prepend_cover_page`]；《绍宋》《狼厅》）。
/// - 3（2026-10-09）：spine 里标了 `linear="no"` 的目录页拿掉（`drop_nonlinear_nav`；《绍宋》）。
pub const KINDLE_RULES_VERSION: &str = "3";
pub use self::dead_refs::font_face_re;
pub use self::css::filter_css;
#[cfg(test)]
use self::css::wash_html;
pub use self::drm::{encrypted_targets, real_drm_items, PSEUDO_DRM_SAFE_EXTS};
pub use self::opf::{manifest_items, opf_dc, parse_opf, tag_attr, ManifestItem, Opf, OpfDc};
pub use self::toc::{is_toc_file, toc_entry_count, TocItem};
pub use self::typeset::{count_dup_id_tags, wash_css, NOTEICON_RULE};
pub use crate::html::plain_text;

use self::css::*;
use self::dead_refs::*;
use self::drm::strip_pseudo_drm;
use self::empty_pages::*;
use self::ids::dedup_ids_across_book;
use self::layout::*;
use self::ncx_fix::*;
use self::opf::{find_opf, opf_book_title, opf_unique_identifier};
use self::chapters::chapters_into_toc;
pub(crate) use self::chapters::is_toc_like_page;
pub(crate) use self::css::{css_rule_re, strip_css_comments};
pub(crate) use self::toc::name_index;
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
    /// 剥掉的 CSS 属性（小写）。缺省 [`DEFAULT_FILTER_PROPS`]。
    pub filter_props: Vec<String>,
    /// 正文排版语言（`Auto`=自动探测）。
    pub lang: LangMode,
    /// 保留的字体（规范化的名字）。`wash_entries` 开头按全书分析填上（见 `fonts`），调用方不用给。
    pub keep_fonts: HashSet<String>,
    /// 阅读器认 `rgba()` 颜色（profile 的 `css_rgba`，缺省 `true`）；`false` 时换成不透明写法（见 `cssunlock::rgba_to_opaque`）。
    pub css_rgba: bool,
    /// 只修复（文字书，profile `text_repair_only`）：只做 EPUB 3 修复和目录（[`repair_entries`]），不解锁、不排版、不删空白页，
    /// 别的选项不看。调用方已判定不是漫画。
    pub repair_only: bool,
    /// 只修复时照 Send to Kindle 的规则统一（profile `kindle_rules`）：标签缺省样式 `eink-ua.css`（[`crate::uastyle::ua_css`]），
    /// 正文字体、body 左右边距、文字对比度（`kindle_rules.rs`）。
    pub kindle_rules: bool,
}

impl Default for WashOpts {
    fn default() -> Self {
        WashOpts { keep_para_spacing: false, auto_toc: AutoToc::IfMissing, filter_props: DEFAULT_FILTER_PROPS.iter().map(|s| s.to_string()).collect(), lang: LangMode::Auto, keep_fonts: HashSet::new(), css_rgba: true, repair_only: false, kindle_rules: false }
    }
}

/// 注入排版规则的外链 css 文件名（放 OPF 同目录）。真机坐实（2026-09-04《缩进诊断6》/《飘》）：
/// **xochitl 只认外链 `.css` 文件里的规则，完全无视内联 `<style>` 块和元素 `style=` 属性**——所以
/// 排版规则（首行缩进/边距）必须写成外链 css 才在 xochitl 生效（按标准 CSS 渲染的阅读器外链、内联都认）。
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
    /// 文件名里有安卓存储不能用的字符（`*:?` 等）而改了名的条目：(原名, 新名)。流式优化器按原名回原书读图片字节。
    pub renamed: Vec<(String, String)>,
    pub css_files: usize,
    /// 保留下来的嵌入字体（规范化的名字，排好序）。
    pub kept_fonts: Vec<String>,
    /// 标成批注（`eink-annot`）的元素个数。
    pub annotations_marked: usize,
    pub html_files: usize,
    pub empty_pages_removed: Vec<String>,
    pub toc_generated: usize,
    pub dup_id_tags_collapsed: usize,
    /// 挂上标签缺省样式表 `eink-ua.css` 的章节数（[`WashOpts::kindle_rules`]）。
    pub ua_css_linked: usize,
    /// 照 Send to Kindle 的规则改了的声明数（正文字体、body 左右边距、文字对比度，见 `kindle_rules.rs`）。
    pub kindle_rule_edits: usize,
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
    /// 书自带目录漏掉、补进目录的节数。见 `chapters.rs`。
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
    /// 不是 UTF-8、转成了 UTF-8 的 XHTML/OPF/NCX 个数。见 `encoding.rs`。
    pub transcoded_to_utf8: usize,
}


// ───────────────────────── 入口 ─────────────────────────

/// 全书 CJK vs 拉丁字符占比 → 主语言（Han 字数 ≥ 拉丁字母数 = Cjk）。扫全部 html 正文，早停够量即定。
fn detect_dominant_script(entries: &[Entry]) -> LangMode {
    let (mut han, mut latin) = (0u64, 0u64);
    for e in entries.iter().filter(|e| is_chapter_entry(e)) {
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

/// 对条目表就地清洗。真 DRM 返回 Err（调用方应整体失败、原样不动）。漫画识别（`comic_detect::is_comic`）在定章节前按清洗到那一步的条目判。
pub fn wash_entries(entries: &mut Vec<Entry>, opts: &WashOpts) -> Result<WashReport, String> {
    wash_with(entries, opts, None)
}

/// 同 [`wash_entries`]，漫画识别用调用方判好的结果（优化器按原书判一次，清洗层和图片处理用同一个结果，不再判第二遍）。
pub(crate) fn wash_entries_as(entries: &mut Vec<Entry>, opts: &WashOpts, comic: bool) -> Result<WashReport, String> {
    wash_with(entries, opts, Some(comic))
}

fn wash_with(entries: &mut Vec<Entry>, opts: &WashOpts, comic: Option<bool>) -> Result<WashReport, String> {
    let mut rep = WashReport::default();
    // 先把 GBK、Big5、UTF-16 的文件转成 UTF-8：后面各步都按 UTF-8 读写
    encoding::transcode_to_utf8(entries, &mut rep);
    strip_pseudo_drm(entries, &mut rep)?;
    // 文件名有安卓存储不能用的字符的先改名：后面各步按条目名找文件
    safe_names::rename_unsafe_entries(entries, &mut rep);
    if opts.repair_only {
        repair_entries(entries, opts, &mut rep);
        return Ok(rep);
    }
    remove_empty_pages(entries, &mut rep);
    drop_dead_refs(entries, &mut rep);
    // Auto → 探测主语言，解析成具体 Cjk/Latin 再逐文件注排版（探测在剥空页之后、注样式之前）。
    let font_plan = fonts::analyze(entries);
    let opts = {
        let mut o = opts.clone();
        if o.lang == LangMode::Auto {
            o.lang = detect_dominant_script(entries);
        }
        o.keep_fonts = font_plan.keep.clone();
        o
    };
    rep.kept_fonts = font_plan.keep.iter().cloned().collect();
    rep.kept_fonts.sort();
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
    for e in entries.iter_mut().filter(|e| is_css_name(&e.name)) {
        if let Ok(t) = std::str::from_utf8(&e.data) {
            let css = filter_css(t, opts);
            if opts.lang == LangMode::Latin {
                indent_classes.extend(indent_classes_of(&css));
            }
            e.data = css.into_bytes();
            rep.css_files += 1;
        }
    }
    // 逐文件独立，多线程做（`util::par_map_mut`，结果与逐个做相同）
    let counts = crate::util::par_map_mut(entries, |e| {
        if !is_chapter_entry(e) {
            return None;
        }
        let t = std::str::from_utf8(&e.data).ok()?;
        let (marked, n) = fonts::mark_annotations(t, &font_plan);
        let (out, dups) = wash_html_with(&marked, opts, &indent_classes);
        let href = crate::epubzip::href_to(dir_of(&e.name), &css_path, "");
        let out = inject_css_link(&out, &href);
        let out = ensure_html_lang(&align_classes(&out), &lang_tag);
        e.data = out.into_bytes();
        Some((n, dups))
    });
    for (n, dups) in counts.into_iter().flatten() {
        rep.annotations_marked += n;
        rep.dup_id_tags_collapsed += dups;
        rep.html_files += 1;
    }
    add_wash_css_entry(entries, opf_idx, &css_path, &wash_css(opts));
    fix_ncx_manifest_id(entries, &mut rep);
    // 目录指错位置的先修好：后面的分部重建、定章节都按目录找标题
    repair_ncx_targets(entries, &mut rep);
    restructure_existing_toc_parts(entries, opts.auto_toc, heading, &mut rep);
    nest_parts_among_siblings(entries, opts.auto_toc, heading, &mut rep);
    auto_toc(entries, opts.auto_toc, heading, &mut rep);
    // 定章节、补节进目录放在自动目录之后：自动目录给标题补的 id 已经在，改目录链接时能对上。漫画不做。
    // 不拆文件（2026-10-06 用户定：章节不强制分页，原书的文件结构原样保留）。
    let comic = comic.unwrap_or_else(|| crate::comic_detect::is_comic(entries));
    if !comic {
        chapters_into_toc(entries, heading, &mut rep);
    }
    // 全书 id 去重放在补 id 之后：补的 `eink-sec-N` 只避开本文件已有的锚点，跨文件撞名在这里改。
    dedup_ids_across_book(entries, &mut rep);
    remove_chapter_end_blanks(entries, &mut rep);
    strip_ncx_doctype(entries, &mut rep);
    // 规范整理：XML 修复、升级 EPUB 3、导航文档（见 `normalize.rs`）。放最后：前面各步新写的文件也要过一遍；
    // 升级可能补了标识符，NCX 的 dtb:uid 在它之后对齐。
    normalize::normalize_book(entries, &lang_tag, heading, &mut rep);
    fix_ncx_uid(entries, &mut rep);
    Ok(rep)
}

/// 只修复（[`WashOpts::repair_only`]，文字书；2026-10-08 用户定三台（Kindle、掌阅、Move）都这样做）：书里的文字、图片、样式一概不动，只做
/// - 合规：指向不存在文件的引用去掉（`drop_dead_refs`）、一个标签上重复的 `id` 合并、跨文件重复的 id 改名（链接跟着改）、
///   NCX 的 DOCTYPE 和 manifest id、`dtb:uid` 对齐，最后规范整理成 EPUB 3（`normalize.rs`：合法 XML、OPF 3.0、nav 与 NCX 互补）；
/// - 目录：指错位置的改指、分部重建、没有目录的按标题生成、按目录层级定章节并把漏掉的节补进目录（补的 id 不改显示）；
/// - 照 Send to Kindle 的规则统一（[`WashOpts::kindle_rules`]，掌阅、Move）：标签缺省样式表挂在书自带样式之前，正文字体、
///   body 左右边距、文字对比度改进书的样式表（`kindle_rules.rs`）。
///
/// 伪 DRM 剥离、文件名改安全字符在调用方（[`wash_with`]）已经做了。
fn repair_entries(entries: &mut Vec<Entry>, opts: &WashOpts, rep: &mut WashReport) {
    let auto_toc_mode = opts.auto_toc;
    drop_dead_refs(entries, rep);
    let lang = detect_dominant_script(entries);
    let lang_tag = book_lang_tag(entries, find_opf(entries), lang);
    let heading = toc_title(lang);
    let dups = crate::util::par_map_mut(entries, |e| {
        if !is_chapter_entry(e) {
            return 0;
        }
        let Ok(t) = std::str::from_utf8(&e.data) else { return 0 };
        let n = count_dup_id_tags(t);
        if n > 0 {
            e.data = collapse_dup_id_attrs(t).into_bytes();
        }
        n
    });
    rep.dup_id_tags_collapsed += dups.into_iter().sum::<usize>();
    if opts.kindle_rules {
        kindle_rules::apply(entries, rep);
        add_ua_css(entries, rep);
        drop_nonlinear_nav(entries);
    }
    fix_ncx_manifest_id(entries, rep);
    repair_ncx_targets(entries, rep);
    restructure_existing_toc_parts(entries, auto_toc_mode, heading, rep);
    nest_parts_among_siblings(entries, auto_toc_mode, heading, rep);
    auto_toc(entries, auto_toc_mode, heading, rep);
    chapters_into_toc(entries, heading, rep);
    dedup_ids_across_book(entries, rep);
    strip_ncx_doctype(entries, rep);
    normalize::normalize_book(entries, &lang_tag, heading, rep);
    fix_ncx_uid(entries, rep);
}

/// spine 里标了 `linear="no"` 的目录页从 spine 拿掉（manifest 里留着，仍是导航文档），见 [`opf::nonlinear_nav_itemrefs`]。
fn drop_nonlinear_nav(entries: &mut [Entry]) {
    let Some(i) = find_opf(entries) else { return };
    let Ok(text) = std::str::from_utf8(&entries[i].data) else { return };
    let spans = opf::nonlinear_nav_itemrefs(text);
    if spans.is_empty() {
        return;
    }
    let out = html::apply_edits(text, spans.into_iter().map(|(s, e)| (s, e, String::new())).collect());
    entries[i].data = out.into_bytes();
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
/// `dtb:uid` 对齐 OPF。给 `booklib meta --edit` 用（2026-09-30 用户：改元数据写出的书也要和 booklib 一样符合 EPUB 3）。不做排版、目录等清洗。
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
    add_css_entry(entries, opf_idx, css_path, content, "eink-wash-css");
}

/// 只修复时挂上标签的缺省样式（[`WashOpts::kindle_rules`]）：OPF 同目录写 `eink-ua.css`，每章 `<head>` 里**第一个**放指向它的 `<link>`
/// （在书自带的样式表、`<style>` 之前，书里写了的照样盖过它）。目录页（nav）不挂。
fn add_ua_css(entries: &mut Vec<Entry>, rep: &mut WashReport) {
    let opf_idx = find_opf(entries);
    let name = crate::uastyle::UA_CSS_NAME;
    let css_path = match opf_idx.map(|i| dir_of(&entries[i].name)) {
        Some(d) if !d.is_empty() => format!("{d}/{name}"),
        _ => name.to_string(),
    };
    let linked = crate::util::par_map_mut(entries, |e| {
        if !is_chapter_entry(e) {
            return false;
        }
        let Ok(t) = std::str::from_utf8(&e.data) else { return false };
        let out = typeset::inject_css_link_first(t, &crate::epubzip::href_to(dir_of(&e.name), &css_path, ""));
        let changed = out != t;
        e.data = out.into_bytes();
        changed
    });
    rep.ua_css_linked += linked.into_iter().filter(|c| *c).count();
    add_css_entry(entries, opf_idx, &css_path, &crate::uastyle::ua_css(), "eink-ua-css");
}

/// 新增（或更新）一份我们写的样式表，并往 OPF manifest 补一条 `<item>`（幂等）。
fn add_css_entry(entries: &mut Vec<Entry>, opf_idx: Option<usize>, css_path: &str, content: &str, id: &str) {
    if let Some(e) = entries.iter_mut().find(|e| e.name == css_path) {
        e.data = content.as_bytes().to_vec();
    } else {
        entries.push(Entry { name: css_path.to_string(), data: content.as_bytes().to_vec() });
    }
    if let Some(oi) = opf_idx {
        let opf_dir = dir_of(&entries[oi].name).to_string();
        let text = String::from_utf8_lossy(&entries[oi].data);
        if manifest_items(&text).iter().any(|it| it.path(&opf_dir) == css_path) {
            return;
        }
        let href = crate::epubzip::href_to(&opf_dir, css_path, "");
        if let Some(t) = opf::insert_manifest_items(&text, &[opf::NewItem { id, href: &href, media_type: "text/css", properties: "" }]) {
            entries[oi].data = t.into_bytes();
        }
    }
}

/// 条目名是不是样式表（`.css`，不分大小写）。
pub(crate) fn is_css_name(name: &str) -> bool {
    name.len() >= 4 && name.as_bytes()[name.len() - 4..].eq_ignore_ascii_case(b".css")
}

/// 正文章节：是 html、又不是目录文件（[`is_toc_file`]）。
pub(crate) fn is_chapter_entry(e: &Entry) -> bool {
    is_html_entry(&e.name, &e.data) && !is_toc_file(&e.name)
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
/// 空页清理、全书 id 去重共用。返回改了的条目数。
pub(super) fn rewrite_book_links(entries: &mut [Entry], skip: impl Fn(&str) -> bool + Sync, f: impl Fn(&Link) -> Option<String> + Sync) -> usize {
    // 各文件独立，多线程做（`util::par_map_mut`）
    let changed = crate::util::par_map_mut(entries, |e| {
        let l = e.name.to_ascii_lowercase();
        let is_opf = l.ends_with(".opf");
        if skip(&e.name) || !(is_opf || l.ends_with(".ncx") || is_html_entry(&e.name, &e.data)) {
            return false;
        }
        let Ok(text) = std::str::from_utf8(&e.data) else { return false };
        let name = e.name.as_str();
        let new = html::edit_attrs(text, &["href", "src", "xlink:href"], |t, a| {
            if is_opf && opf::is_local(t.name, "item") {
                return Edit::Keep;
            }
            let (p, _) = html::split_href(a.value);
            if html::is_external(p) {
                return Edit::Keep;
            }
            // 目标文件按 `epubzip::resolve_link` 解析；锚点给原文（改链时原样写回，对 id 时再 `html::frag_id`）
            let target = crate::epubzip::resolve_link(name, a.value).0;
            match f(&Link { file: name, value: a.value, target, frag: html::split_href(a.value).1 }) {
                Some(v) if v != a.value => Edit::Set(v),
                _ => Edit::Keep,
            }
        });
        match new {
            Cow::Owned(new) => {
                e.data = new.into_bytes();
                true
            }
            Cow::Borrowed(_) => false,
        }
    });
    changed.into_iter().filter(|&c| c).count()
}
