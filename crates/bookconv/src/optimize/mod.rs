//! 通用 EPUB 优化器：吃**任意结构**的 EPUB，按目标设备的阅读范围处理——清洗层（`wash`，可选）→ 每个 (x)html
//! 的字体解锁与注释重排 → 图片缩放（漫画单趟裁边/缩放/补白/灰度）。其余文件（css/opf/ncx）原样或只做小改，
//! 不重组目录/spine，最大限度兼容各家 EPUB。入口是流式的 [`optimize_epub_file_streaming`]（路径进路径出），
//! 调用方是书库 `booklib`（`crates/library`）和命令行 `epub-optimize`。

use regex::Regex;
use std::collections::HashSet;
use std::io::Write;
use std::sync::OnceLock;
use zip::{ZipArchive, ZipWriter};

/// 幂等标记：优化器把这个文件埋进产物 EPUB，内容=优化器版本号（见 [`marker_value`]）。放 META-INF/ 下
/// （EPUB 规范允许该目录放额外文件，阅读器忽略）。重优化时旧标记剔除、结尾重写一条。
pub const OPTIMIZE_MARKER: &str = "META-INF/eink-optimized";
/// 优化逻辑版本。改了会影响产物的行为就 bump——书库按它（连同设备、阅读范围等）判断产物是否过期、需要重新生成。
///
/// 历史（只记还有参考价值的结论）：
/// - v2–v5：duokan 图片脚注标记修复；图片按屏幕降采样、e-ink 提对比（灰字→纯黑、细字重→400）；远程图内联；
///   同文件带文件名的 href 归一成裸锚；EPUB 内嵌图用竖向框（宽不超屏幕短边，防行内横幅溢出竖屏）。
/// - v6–v8：清洗层（伪 DRM 剥离、CSS 锁剥离、边距段距归零+首行缩进、空页清理、自动目录、单标签双 id 折叠）；
///   中英文分别排版；自动目录扩到 h1–h6 多级；内联脚注丢弃图标 marker；剥 CSS `background`（xochitl 会把背景图平铺满页）。
/// - v10：xochitl **只认外链 `.css` 文件里的规则**，无视内联 `<style>` 和 `style=`——排版规则改写成外链 `eink-wash.css`
///   + 每章 `<link>` + manifest 补项；外链 css 只用裸元素选择器（xochitl 的 css 解析器遇到复杂选择器整表失效）。
/// - v11：不再把"指向很多章节文件的页面"当冗余目录页从 spine 删掉（那是书的正文）。
/// - v12–v13：`toc.ncx` 的 `dtb:uid` 与 OPF 标识符不一致、或带外部 DTD 引用时，xochitl 不显示目录入口——
///   同步 `dtb:uid`（`wash::fix_ncx_uid`）、剥掉 DTD 声明（`wash::strip_ncx_doctype`）。
/// - v16：缩放与漫画补白一律按 profile 的**真实可阅读范围**（`OptimizeOpts::screen`），补白容差 0.3%。
/// - v17：章节分页（`wash::paginate`）。v18：黑白屏设备的漫画页转 8 位灰度（256 级，不抖动）。
/// - v19：排版细化——补 `<html>` 语言属性；两端对齐；居中/居右换成类；剥 `line-height` 与 `vh` 高度；章尾空白页。
/// - v20（2026-09-27）：`mimetype` 一律重写为首个 STORED 条目（源书缺它也补上）；已压缩的图片（JPEG/PNG/GIF）改 STORED；
///   抓到的远程图补进 OPF manifest（AZW3 写出器只认 manifest 里的图），抓不到的 `<img>` 原样保留、不再删除；
///   带透明通道的漫画页合成到白底（此前透明区域变黑）。
/// - v21（2026-09-27 审计）：清洗层改用容错的 `crate::html` 工具（单引号属性、`data-id` 误匹配、注释里的标签、
///   CSS 字符串里的分号都不再出错）；全书 id 去重挪到清洗层、跨文件链接一起改；章尾空元素按样式表判断保不保留；
///   分页不再把 `<html>`/`<head>` 之间的杂散文字复制进拆出的文件；同名不同目录的文件不再被当成"本文件"。
pub const OPTIMIZE_VERSION: &str = "21";

/// 脚注呈现方式。xochitl 没有弹窗脚注，统一用 `Anchor`（章末可见 + 同章锚点跳转 + 阅读器原生「返回」）。
/// 曾试过"注释移到引用它的段落末尾"，真机验证后撤回删除——用户真实期望是"翻到哪页注释固定在那页最下面"，
/// EPUB 流式重排做不到（"页"是阅读器翻页时才算出来的），"跟着段落走"的近似不符合预期。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum FootnoteMode {
    /// 注释移章末 `<div class="footnotes">` + marker 改同章锚点，点跳、原生浮标返回。
    #[default]
    Anchor,
    /// 注释文字就地内联显示在引用处 `<span class="eink-fnote">〔…〕</span>`，始终可见、不跳转。
    Inline,
}

/// 优化选项：`wash=Some` 时先过清洗层。没有缺省设备，所以不实现 `Default`，用 [`OptimizeOpts::new`] 起步。
#[derive(Clone, Debug, PartialEq)]
pub struct OptimizeOpts {
    /// 目标设备的真实可阅读范围（`profile::Profile::readable`），图片缩放与漫画补白都按它算。
    pub screen: crate::imgopt::Screen,
    /// 黑白屏设备（profile `color = false`）：漫画页转成单分量 8 位灰度（256 级，不抖动）。
    pub grayscale: bool,
    pub wash: Option<crate::wash::WashOpts>,
    /// 脚注呈现方式（缺省 `Anchor`，书库与 `epub-optimize` 都用它）。
    pub footnote: FootnoteMode,
    /// 翻页方向（按书手动指定）：`Some` 时把 OPF `<spine page-progression-direction>` 写成这个值，
    /// `None`（缺省）＝保留原书。见 [`crate::direction`]。
    pub page_direction: Option<crate::direction::PageDirection>,
}

impl OptimizeOpts {
    /// 只指定屏幕、其余取缺省（彩色、不清洗、`Anchor` 注释、保留原书翻页方向）。
    pub fn new(screen: crate::imgopt::Screen) -> Self {
        OptimizeOpts { screen, grayscale: false, wash: None, footnote: FootnoteMode::default(), page_direction: None }
    }
}

/// 优化统计，供回执。
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Report {
    pub wash: Option<crate::wash::WashReport>,
    pub total_files: usize,
    pub html_files: usize,
    pub bytes_before: usize,
    pub bytes_after: usize,
}

use crate::epubzip::is_html_entry;

// 按职责拆成子模块：`html_pass`（逐条变换）· `streaming`（流式写出）· `marker`（幂等标记）；
// `pub` 项在这里 glob re-export。
mod html_pass;
mod marker;
mod streaming;

use self::html_pass::*;
pub use self::marker::*;
pub use self::streaming::*;

#[cfg(test)]
mod tests;

/// 阶段一的产物。
struct Prepared {
    /// (条目名, 字节, 是否 html)，已过封面声明/清洗/第一遍 html 处理/注释块搬出；首个条目是重写过的 `mimetype`。
    entries: Vec<(String, Vec<u8>, bool)>,
    /// 全书"被引用的注释块"索引（id → 块 html），第二遍 `preserve_relink_footnotes` 搬进引用它的那一章。
    aside_index: std::collections::HashMap<String, String>,
    is_comic_book: bool,
    /// 要改 OPF 时（指定了翻页方向，或书里有远程图、抓到的图要补进 manifest）的 OPF 条目名。
    opf_name: Option<String>,
    /// 有章节引用远程图：OPF 推迟到最后写，好把抓到的图补进 manifest（见 `streaming`）。
    has_remote_imgs: bool,
    rep: Report,
}

/// EPUB 规范：`mimetype` 必须是 zip 的第一个条目、STORED、内容就是这串（不带换行）。
const MIMETYPE: &[u8] = b"application/epub+zip";

/// 阶段一：`raw` → 封面声明 → 清洗 → 排序（mimetype 置首、旧标记剔除）→ 漫画识别 → 第一遍 html → 注释块搬出。
/// 图片条目是空占位——这里所有判断只看 html 文字与 `<img>` 引用，不需要图片真实字节。
fn prepare_entries(mut raw: Vec<crate::epubzip::Entry>, opts: &OptimizeOpts, bytes_before: usize) -> Result<Prepared, String> {
    // 保证 OPF 声明了有效封面（见 `wash::ensure_cover_declared`）。
    // 必须在清洗之前：清洗会把只含 SVG 封面的 titlepage 当空页删掉。
    crate::wash::ensure_cover_declared(&mut raw);
    let wash_rep = match &opts.wash {
        Some(w) => Some(crate::wash::wash_entries(&mut raw, w)?),
        None => None,
    };
    let has_remote_imgs = raw.iter().any(|e| is_html_entry(&e.name, &e.data) && std::str::from_utf8(&e.data).is_ok_and(has_remote_img));
    // 只在真要改 OPF 时才找它（`parse_opf` 返回下标，所以在改排序之前找）。
    let opf_name: Option<String> = if opts.page_direction.is_some() || has_remote_imgs { crate::wash::parse_opf(&raw).map(|o| raw[o.index].name.clone()) } else { None };
    // mimetype 一律重写成规范内容放在最前（源书缺它、内容不规范都修正），其余原序；旧标记剔除（结尾统一重写当前版本）。
    let mut ordered: Vec<crate::epubzip::Entry> = Vec::with_capacity(raw.len() + 1);
    ordered.push(crate::epubzip::Entry { name: "mimetype".into(), data: MIMETYPE.to_vec() });
    ordered.extend(raw.into_iter().filter(|e| e.name != "mimetype" && e.name != OPTIMIZE_MARKER));

    // 漫画识别（图 ≥20 张且平均每张图配的文字 <40 字）：决定图片走漫画单趟处理还是普通降采样。用清洗之后的条目判——
    // 清洗层已把空页清理、目录归一，判定更准。
    let is_comic_book = crate::comic_detect::is_comic(&ordered);

    let mut rep = Report { wash: wash_rep, total_files: 0, html_files: 0, bytes_before, bytes_after: 0 };

    // 第一遍：xhtml → strip_font_locks；同时扫全书 marker 得**被引用**的尾注 frag 集（referenced），供下一步
    // "只搬被引用的注释块"用。
    let mut entries: Vec<(String, Vec<u8>, bool)> = Vec::with_capacity(ordered.len());
    let mut referenced: HashSet<String> = HashSet::new(); // 被 marker 引用的注释 id（noteref + 跨文件普通<a>）
    for crate::epubzip::Entry { name, data } in ordered {
        rep.total_files += 1;
        let ish = is_html_entry(&name, &data);
        // 非 UTF-8 的 html 原样保留（`from_utf8` 失败时把字节还回来，不克隆）。
        let data = if ish {
            match String::from_utf8(data) {
                Ok(text) => {
                    let (stripped, refs) = first_pass_html(&text, &name);
                    referenced.extend(refs);
                    rep.html_files += 1;
                    stripped.into_bytes()
                }
                Err(e) => e.into_bytes(),
            }
        } else {
            data
        };
        entries.push((name, data, ish));
    }

    // 第一遍后半：把**被引用**的注释块（aside/p/li 且带注释语义）从各章移除、建全书索引 aside_index，
    // 交给第二遍 preserve_relink_footnotes 搬进引用它的那一章。未被引用的块原样留在原处（零丢失）。
    let mut aside_index: std::collections::HashMap<String, String> = std::collections::HashMap::new();
    for (_, data, ish) in entries.iter_mut() {
        if !*ish {
            continue;
        }
        let Ok(text) = std::str::from_utf8(data) else { continue };
        let (cleaned, notes) = crate::htmlproc::collect_footnote_notes(text, &referenced, true);
        if !notes.is_empty() {
            aside_index.extend(notes);
            *data = cleaned.into_bytes();
        }
    }
    Ok(Prepared { entries, aside_index, is_comic_book, opf_name, has_remote_imgs, rep })
}

/// 第二遍的"文本类条目"变换器：html 章节 / 独立 css / （改翻页方向时）OPF。跨条目状态（远程图计数、全书 id 去重表、
/// 抓到的远程图）都在这里。图片条目不归它管（走 `imgpool` 并行）。
struct EntryXform<'a> {
    aside_index: &'a std::collections::HashMap<String, String>,
    footnote: FootnoteMode,
    page_direction: Option<crate::direction::PageDirection>,
    opf_name: Option<&'a str>,
    seen_ids: HashSet<String>, // 跨章累积，dedup_ids_in_chapter 用
    screen: crate::imgopt::Screen,
    img_agent: ureq::Agent, // 远程图抓取（仅当章内有远程 img 才发请求；抓不到 → 原样保留）
    remote_counter: usize,
    /// 抓到的远程图 (zip 路径, 字节)，结尾写进 zip 并补进 manifest。
    fetched_imgs: Vec<(String, Vec<u8>)>,
}

impl<'a> EntryXform<'a> {
    fn new(aside_index: &'a std::collections::HashMap<String, String>, opf_name: Option<&'a str>, opts: &OptimizeOpts) -> EntryXform<'a> {
        EntryXform {
            aside_index,
            footnote: opts.footnote,
            page_direction: opts.page_direction,
            opf_name,
            screen: opts.screen,
            seen_ids: HashSet::new(),
            img_agent: crate::netimg::http_agent(15),
            remote_counter: 0,
            fetched_imgs: Vec::new(),
        }
    }

    /// 章节 html 最终变换链：解双向脚注互指环 → duokan 图片脚注标记换上标 → 封面拉伸/SVG 修复 → 脚注就地关联重排 →
    /// e-ink 提对比 → 远程图内联 → 全书 id 去重。要用到第一遍扫全书才拿得到的 `aside_index`，所以与第一遍分开、顺序不能换。
    fn transform_html_chapter(&mut self, text: &str, name: &str) -> Vec<u8> {
        let t = crate::htmlproc::break_footnote_cycles(text);
        let t = crate::htmlproc::fix_duokan_markers(&t);
        let t = fix_cover_aspect(&t);
        let t = svg_cover_to_img(&t);
        let t = crate::htmlproc::preserve_relink_footnotes(&t, self.aside_index, self.footnote);
        let t = crate::htmlproc::boost_text_contrast(&t);
        let chap_dir = std::path::Path::new(name).parent().and_then(|p| p.to_str()).unwrap_or("");
        let (t, imgs) = inline_remote_images(&t, chap_dir, &mut self.remote_counter, remote_img_fetcher(&self.img_agent, self.screen));
        self.fetched_imgs.extend(imgs);
        crate::htmlproc::dedup_ids_in_chapter(&t, &mut self.seen_ids).into_bytes()
    }

    /// 文本类条目 → `Some(最终字节)`（无法按 UTF-8 解读的原样借回）；不是文本类（图片/其它）→ `None`，调用方自己处理。
    fn transform_text<'d>(&mut self, name: &str, data: &'d [u8], is_html: bool) -> Option<std::borrow::Cow<'d, [u8]>> {
        use std::borrow::Cow;
        if is_html {
            return Some(match std::str::from_utf8(data) {
                Ok(text) => Cow::Owned(self.transform_html_chapter(text, name)),
                Err(_) => Cow::Borrowed(data),
            });
        }
        if name.to_lowercase().ends_with(".css") {
            // e-ink 提对比：独立 .css 文件里的灰字→纯黑、细字重→400。
            return Some(match std::str::from_utf8(data) {
                Ok(text) => Cow::Owned(crate::htmlproc::boost_contrast_css(text).into_bytes()),
                Err(_) => Cow::Borrowed(data),
            });
        }
        if let (true, Some(dir)) = (self.opf_name == Some(name), self.page_direction) {
            return Some(match std::str::from_utf8(data) {
                Ok(text) => Cow::Owned(crate::direction::set_spine_direction(text, dir).into_bytes()),
                Err(_) => Cow::Borrowed(data),
            });
        }
        None
    }
}
