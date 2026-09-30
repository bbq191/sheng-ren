//! 通用 EPUB 优化器：吃**任意结构**的 EPUB，按目标设备的阅读范围处理——清洗层（`wash`，可选）→ 每个 (x)html
//! 的字体解锁与注释重排 → 图片缩放（漫画单趟裁边/缩放/补白/灰度）。其余文件（css/opf/ncx）原样或只做小改，
//! 不重组目录/spine，最大限度兼容各家 EPUB。入口是流式的 [`optimize_epub_file_streaming`]（路径进路径出），
//! 调用方是书库 `booklib`（`crates/library`）和命令行 `epub-optimize`。

use std::collections::{HashMap, HashSet};
use std::io::Write;
use zip::{ZipArchive, ZipWriter};

/// 幂等标记：优化器把这个文件埋进产物 EPUB，内容=优化器版本号（见 [`marker_value`]）。放 META-INF/ 下
/// （EPUB 规范允许该目录放额外文件，阅读器忽略）。重优化时旧标记剔除、结尾重写一条。
pub const OPTIMIZE_MARKER: &str = "META-INF/eink-optimized";
/// 漫画要在阅读器里设成的页边距（内容就是数字），只有 profile 开了 `comic_reader_margins` 的漫画才有；`xochitl/comic-margins.sh` 凭它登记。
pub const READER_MARGINS_MARKER: &str = "META-INF/eink-reader-margins";
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
/// - v22（2026-09-28）：分页认好读式结构——目录里有、但只是一段字的章名（"第一章"）当章标题，目录改指到它；
///   全书没有节一级标题时，独占一段、每章从 1 连续编号的节号（`１`、`一`）当节标题，进目录。
/// - v23（2026-09-28）：和章标题同级、只写节号的标题（《13級階梯》`<h3>２</h3>` 单独成文件）至少两章都是这种编号时
///   降成节：不再单独占一页，跟正文同页；书自带的平目录里的节缩进到章下面，章标签末尾重复的第一节节号去掉。
/// - v24（2026-09-28）：漫画的 OPF 打上 `<dc:subject>漫画</dc:subject>`（KOReader 读成 keywords，配置档据此自动套漫画设置）。
/// - v25（2026-09-28）：部标题后面紧跟章标题时部、章各占一页（《雪人》）；没有 `<hN>` 的书，目录锚点是标题段落前面的空元素时
///   也认得出标题（《福尔摩斯探案全集》此前整本没分页）；书自带目录指错位置的，核实后改指（《占星术杀人魔法》NCX 整体错位）。
/// - v26（2026-09-28 审计）：SVG 封面换 img 只换整页只有一张图的封面页（此前的正则会吞掉两个 svg 之间的正文）；分页并回空份时
///   补闭合不再重复、只剩标题的份并回前页；交叉引用指向的普通段落不再当注释搬（要注释语义或在文件末尾注释区）；
///   单引号 OPF 的空页删得掉；`Chapter 1` 不拆成两级；已嵌套的目录不压平，重写目录时保留 navPoint id 与 pageList；
///   注释索引按 (文件, id)，目录页的链接不算注释引用，收集后没人接的注释放回原处；`@import` 后第一条规则照剥字体锁；
///   英文首段顶格保留作者的强调类（只去掉写了 text-indent 的类）；`margin:inherit` 不再写坏；目录与 OPF 里新写的 href 百分号编码。
/// - v27（2026-09-29，EPUB→EPUB 收窄）：
///   ① 字体字号解锁但不动别的样式——相对字号保留（正文整体那一层除外）、`font` 简写留粗斜体、`background` 简写留颜色；
///   不再把灰字改黑、细字重提到 400。
///   ② 注释按阅读模式：KOReader 弹窗（标号 `epub:type="noteref"`、注释块 `<aside epub:type="footnote">`）、xochitl 跳转；
///   标号原样、不再加 `[N]`；注释 0.85em、每条不跨页、图标标号限一个字高。
///   ③ 规范整理——产物一律升级 EPUB 3（OPF 3.0、固定值 dcterms:modified、unique-identifier 修正、opf:role/file-as/scheme 改 refines、
///   缺 nav 按 NCX 生成、只有 nav 的按 nav 生成 NCX、guide 写成 landmarks，NCX 与 spine toc 保留）；XHTML 修成合法 XML
///   （DOCTYPE、HTML 命名实体转数字引用、裸 &/<、XML 不允许的控制字符、空元素自闭合、属性补引号、根元素 xmlns/xmlns:epub、
///   多余闭合标签能配平才去）；manifest 的 svg/mathml/scripted/remote-resources 按最终内容标（OPF 最后写进 zip）；分页拆出的份不再重复 U+FEFF。
///   ④ 漫画页四边留 `comic_margin`（profile 字段，缺省 1px）白边，画布即阅读范围；比框小的 JPEG 页 Lanczos 放大（KOReader 不放大小图），
///   质量一律 95；PNG 按自身比例尺补白；已排好的页原样保留；900 万像素以上的页照常处理（只拒文件头超过 6400 万像素的）；
///   静态 GIF/WebP 页转 PNG/JPEG（条目名不变、manifest media-type 跟着改）。
/// - v28（2026-09-29）：JPEG 哈夫曼表按图重做（`jpegopt`，无损：解码逐像素相同，每页还会解码比对，不同就用原来的），漫画同画质小 7%–9%；
///   多看的图标注释号保留原图标（加 `eink-noteicon` 限一个字高），不再换成上标数字（多出来的字，用户定）。
/// - v29（2026-09-29）：漫画纯图页（没有可见文字）的 `<body>` 加 `eink-fullpage` 类（`line-height:0;font-size:0`），只在 profile 开了
///   `comic_fullpage` 时（koreader）：KOReader 里图在一行中，行高在下面留 10px、字号在行首多出 2px，去掉后整页图用满整屏，
///   `koreader` 阅读范围改成 1264×1680，1px 白边就是到屏幕边缘 1px（本机 KOReader 截图验证）。
/// - v30（2026-09-29）：xochitl 漫画按页边距 1 排（profile `comic_reader_margins` + `comic_readable` 952×1457，画布里不留白边）：
///   写 `META-INF/eink-reader-margins` 供 `xochitl/comic-margins.sh` 登记；文字页、混排页的字补回默认留白，图页去掉 `<body>` 的类（`comicpad`）。
/// - v31（2026-09-29）：只有图标的注释标号在 xochitl 模式换成上标数字（profile `note_icons = "number"`，`htmlproc::number_icon_note_links`）：
///   xochitl 里只有图的链接点不了、CSS 限不住图标大小（Move 真机）。编号取注释开头的 `[N]`，其次图标 alt 里的"注释N"，再次本章顺序。
/// - v32（2026-09-30）：撤掉 v29 的 `eink-fullpage`（只为 KOReader 加的，用户撤了 KOReader）；样式表少了这条规则。
pub const OPTIMIZE_VERSION: &str = "32";

/// 脚注呈现方式，按阅读器定（profile 的 `notes`，见 [`OptimizeOpts::for_profile`]）。注释都移到章末、标号改同章锚点。
/// 曾试过"注释移到引用它的段落末尾"，真机验证后撤回删除——用户真实期望是"翻到哪页注释固定在那页最下面"，
/// EPUB 流式重排做不到（"页"是阅读器翻页时才算出来的），"跟着段落走"的近似不符合预期。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum FootnoteMode {
    /// 跳转（xochitl）：注释移章末 `<div class="footnotes">`，标号改同章锚点，点了跳过去、用阅读器的"返回"回来。
    #[default]
    Anchor,
    /// 弹窗（KOReader）：同 `Anchor`，另给标号标 `epub:type="noteref"`、注释块用 `<aside epub:type="footnote">`。
    Popup,
    /// 注释文字就地内联显示在引用处 `<span class="eink-fnote">〔…〕</span>`，始终可见、不跳转（只在测试里用）。
    Inline,
}

impl From<profile::Notes> for FootnoteMode {
    fn from(n: profile::Notes) -> Self {
        match n {
            profile::Notes::Popup => FootnoteMode::Popup,
            profile::Notes::Jump => FootnoteMode::Anchor,
        }
    }
}

/// 优化选项：`wash=Some` 时先过清洗层。没有缺省设备，所以不实现 `Default`，用 [`OptimizeOpts::new`] 起步。
#[derive(Clone, Debug, PartialEq)]
pub struct OptimizeOpts {
    /// 目标设备的真实可阅读范围（`profile::Profile::readable`），图片缩放与漫画补白都按它算。
    pub screen: crate::imgopt::Screen,
    /// 黑白屏设备（profile `color = false`）：漫画页转成单分量 8 位灰度（256 级，不抖动）。
    /// `epub-optimize --keep-color` 关掉它（黑白屏也保留彩色，做灰度与彩色的对比）。
    pub grayscale: bool,
    /// 漫画页图到阅读范围四边的白边（像素，profile 的 `comic_margin`，缺省 1），见 `imgopt::prepare_comic_page_for_epub`。
    pub comic_margin: u32,
    /// 漫画页排版用的阅读范围（profile 的 `comic_readable`）；`None` 用 `screen`。
    pub comic_screen: Option<crate::imgopt::Screen>,
    /// 漫画在阅读器里要设成的页边距（profile 的 `comic_reader_margins`）：写标记、按 `comicpad` 处理各页。
    pub comic_reader_margins: Option<u32>,
    /// 只有图标的注释标号换成上标数字（profile `note_icons = "number"`）。
    pub number_note_icons: bool,
    pub wash: Option<crate::wash::WashOpts>,
    /// 脚注呈现方式（缺省 `Anchor`，书库与 `epub-optimize` 都用它）。
    pub footnote: FootnoteMode,
    /// 翻页方向（按书手动指定）：`Some` 时把 OPF `<spine page-progression-direction>` 写成这个值，
    /// `None`（缺省）＝保留原书。见 [`crate::direction`]。
    pub page_direction: Option<crate::direction::PageDirection>,
}

impl OptimizeOpts {
    /// 只指定屏幕、其余取缺省（彩色、漫画白边 1px、不清洗、`Anchor` 注释、保留原书翻页方向）。
    pub fn new(screen: crate::imgopt::Screen) -> Self {
        OptimizeOpts { screen, grayscale: false, comic_margin: profile::DEFAULT_COMIC_MARGIN, comic_screen: None, comic_reader_margins: None, number_note_icons: false, wash: None, footnote: FootnoteMode::default(), page_direction: None }
    }

    /// 按阅读模式（profile）取选项：阅读范围、黑白屏转灰度、注释呈现方式、漫画白边；清洗层开（缺省选项）。书库和 `epub-optimize` 都从这里起步。
    pub fn for_profile(p: &profile::Profile) -> Self {
        OptimizeOpts {
            grayscale: !p.color,
            wash: Some(Default::default()),
            footnote: p.notes.into(),
            comic_margin: p.comic_margin,
            comic_screen: Some(p.comic_readable()),
            comic_reader_margins: p.comic_reader_margins,
            number_note_icons: p.note_icons == profile::NoteIcons::Number,
            ..OptimizeOpts::new(p.output_readable())
        }
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
    /// 全书"被引用的注释块"索引（(所在文件, id) → 块 html），第二遍 `preserve_relink_footnotes` 搬进引用它的那一章。
    aside_index: HashMap<crate::htmlproc::NoteKey, String>,
    /// 不做注释搬移的页：导航文档、目录文件、目录样的页（它们的链接不算注释引用，也不往它们里面搬注释）。
    skip_notes: HashSet<String>,
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
    let (wash_rep, washed_comic) = match &opts.wash {
        Some(w) => {
            let (r, c) = crate::wash::wash_entries_detect(&mut raw, w)?;
            (Some(r), Some(c))
        }
        None => (None, None),
    };
    let has_remote_imgs = raw.iter().any(|e| is_html_entry(&e.name, &e.data) && std::str::from_utf8(&e.data).is_ok_and(has_remote_img));
    // mimetype 一律重写成规范内容放在最前（源书缺它、内容不规范都修正），其余原序；旧标记剔除（结尾统一重写当前版本）。
    let mut ordered: Vec<crate::epubzip::Entry> = Vec::with_capacity(raw.len() + 1);
    ordered.push(crate::epubzip::Entry { name: "mimetype".into(), data: MIMETYPE.to_vec() });
    ordered.extend(raw.into_iter().filter(|e| e.name != "mimetype" && e.name != OPTIMIZE_MARKER && e.name != READER_MARGINS_MARKER));

    // 漫画识别（图 ≥20 张且平均每张图配的文字 <40 字）：决定图片走漫画单趟处理还是普通降采样。清洗过的书用清洗层判好的
    // （清洗层已把空页清理、目录归一，判定更准；不再判第二遍）。
    let is_comic_book = washed_comic.unwrap_or_else(|| crate::comic_detect::is_comic(&ordered));
    let opf = crate::wash::parse_opf(&ordered);
    // 只在真要改 OPF 时才记它：改翻页方向、补远程图的 manifest 项、给漫画打标签、（清洗过的书）按最终内容标 manifest 的 properties。
    let opf_name: Option<String> = opf.as_ref().filter(|_| opts.page_direction.is_some() || has_remote_imgs || is_comic_book || opts.wash.is_some()).map(|o| ordered[o.index].name.clone());
    // 导航文档与目录文件：不收它们里面的注释引用，也不往里面搬注释。
    let mut skip_notes: HashSet<String> = ordered.iter().filter(|e| crate::wash::is_toc_file(&e.name)).map(|e| e.name.clone()).collect();
    skip_notes.extend(opf.and_then(|o| o.nav_doc));

    let mut rep = Report { wash: wash_rep, total_files: 0, html_files: 0, bytes_before, bytes_after: 0 };

    // 第一遍：xhtml → strip_font_locks；同时扫全书 marker 得**被引用**的注释 (文件, id) 集（referenced），供下一步
    // "只搬被引用的注释块"用。目录样的页（链接文字占大半）不算。
    let mut entries: Vec<(String, Vec<u8>, bool)> = Vec::with_capacity(ordered.len());
    let mut referenced: HashMap<String, HashSet<String>> = HashMap::new(); // 注释所在文件 → 被 marker 引用的 id
    for crate::epubzip::Entry { name, data } in ordered {
        rep.total_files += 1;
        let ish = is_html_entry(&name, &data);
        // 非 UTF-8 的 html 原样保留（`from_utf8` 失败时把字节还回来，不克隆）。
        let data = if ish {
            match String::from_utf8(data) {
                Ok(text) => {
                    let stripped = first_pass_html(&text, &name);
                    if !skip_notes.contains(&name) {
                        let refs = crate::htmlproc::referenced_note_keys(&stripped, &name);
                        if !refs.is_empty() && crate::wash::is_toc_like_page(&stripped) {
                            skip_notes.insert(name.clone());
                        } else {
                            for (file, id) in refs {
                                referenced.entry(file).or_default().insert(id);
                            }
                        }
                    }
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
    let aside_index = collect_notes(&mut entries, &mut referenced, &skip_notes);
    Ok(Prepared { entries, aside_index, skip_notes, is_comic_book, opf_name, has_remote_imgs, rep })
}

/// 第一遍后半：把**被引用**的注释块（aside/p/li/div 且带注释语义）从各章移除、建全书索引 (文件, id) → 块，交给第二遍
/// `preserve_relink_footnotes` 搬进引用它的那一章。未被引用的块原样留在原处。
///
/// **搬走前核对每条都有人接**（2026-09-28 审计）：按第二遍真正会用的文字（先拆互指环、换 duokan 标记，这两步可能改掉 marker）
/// 重新扫一遍全书的注释引用；收集了却没有任何一章会接的注释，从原文重新收集时不再收它——放回原处。放回的注释里要是还引用着
/// 别的注释，下一轮会看到，所以反复到没有落空的为止。
fn collect_notes(entries: &mut [(String, Vec<u8>, bool)], referenced: &mut HashMap<String, HashSet<String>>, skip: &HashSet<String>) -> HashMap<crate::htmlproc::NoteKey, String> {
    let mut index: HashMap<crate::htmlproc::NoteKey, String> = HashMap::new();
    let mut originals: HashMap<usize, Vec<u8>> = HashMap::new();
    let collect_one = |text: &str, name: &str, referenced: &HashMap<String, HashSet<String>>| referenced.get(name).map(|ids| crate::htmlproc::collect_footnote_notes(text, ids, true));
    for (i, (name, data, ish)) in entries.iter_mut().enumerate() {
        if !*ish || !referenced.contains_key(name.as_str()) {
            continue;
        }
        let Ok(text) = std::str::from_utf8(data) else { continue };
        let Some((cleaned, notes)) = collect_one(text, name, referenced) else { continue };
        if !notes.is_empty() {
            index.extend(notes.into_iter().map(|(id, inner)| ((name.clone(), id), inner)));
            originals.insert(i, std::mem::replace(data, cleaned.into_bytes()));
        }
    }
    while !index.is_empty() {
        let mut claimed: HashSet<crate::htmlproc::NoteKey> = HashSet::new();
        for (name, data, ish) in entries.iter() {
            if !*ish || skip.contains(name) {
                continue;
            }
            let Ok(text) = std::str::from_utf8(data) else { continue };
            let t = crate::htmlproc::fix_duokan_markers(&crate::htmlproc::break_footnote_cycles(text));
            claimed.extend(crate::htmlproc::referenced_note_keys(&t, name).into_iter().filter(|k| index.contains_key(k)));
        }
        let unclaimed: Vec<crate::htmlproc::NoteKey> = index.keys().filter(|k| !claimed.contains(*k)).cloned().collect();
        if unclaimed.is_empty() {
            break;
        }
        let files: HashSet<String> = unclaimed.iter().map(|k| k.0.clone()).collect();
        for (file, id) in &unclaimed {
            if let Some(ids) = referenced.get_mut(file) {
                ids.remove(id);
            }
        }
        index.retain(|k, _| !files.contains(&k.0));
        for (&i, orig) in &originals {
            let (name, data, _) = &mut entries[i];
            if !files.contains(name.as_str()) {
                continue;
            }
            let Ok(text) = std::str::from_utf8(orig) else { continue };
            let (cleaned, notes) = collect_one(text, name, referenced).unwrap_or_else(|| (text.to_string(), Vec::new()));
            index.extend(notes.into_iter().map(|(id, inner)| ((name.clone(), id), inner)));
            *data = cleaned.into_bytes();
        }
    }
    index
}

/// 第二遍的"文本类条目"变换器：html 章节 / 独立 css / （改翻页方向时）OPF。跨条目状态（远程图计数、全书 id 去重表、
/// 抓到的远程图）都在这里。图片条目不归它管（走 `imgpool` 并行）。
struct EntryXform<'a> {
    aside_index: &'a HashMap<crate::htmlproc::NoteKey, String>,
    skip_notes: &'a HashSet<String>,
    footnote: FootnoteMode,
    page_direction: Option<crate::direction::PageDirection>,
    /// 漫画：OPF 里打上漫画标签（`comic_detect::tag_opf_as_comic`）。
    comic: bool,
    /// 漫画且 profile 开了 `comic_reader_margins`：各页按 `comicpad` 处理，`eink-wash.css` 追加它的规则。
    reader_margins: bool,
    /// 只有图标的注释标号换成上标数字。
    number_note_icons: bool,
    opf_name: Option<&'a str>,
    seen_ids: HashSet<String>, // 跨章累积，dedup_ids_in_chapter 用
    screen: crate::imgopt::Screen,
    img_agent: ureq::Agent, // 远程图抓取（仅当章内有远程 img 才发请求；抓不到 → 原样保留）
    remote_counter: usize,
    /// zip 里已有的条目名（含已抓到的远程图）：新抓的图不能跟它们重名。
    taken_names: HashSet<String>,
    /// 抓到的远程图 (zip 路径, 字节)，结尾写进 zip 并补进 manifest。
    fetched_imgs: Vec<(String, Vec<u8>)>,
    /// 清洗过的书：各 XHTML 最终内容用到的特性（zip 路径 → `wash::normalize::content_properties`），结尾写进 manifest 的 `properties`。
    content_props: Option<HashMap<String, u8>>,
}

impl<'a> EntryXform<'a> {
    fn new(prep: &'a Prepared, opts: &OptimizeOpts) -> EntryXform<'a> {
        EntryXform {
            aside_index: &prep.aside_index,
            skip_notes: &prep.skip_notes,
            taken_names: prep.entries.iter().map(|e| e.0.clone()).collect(),
            footnote: opts.footnote,
            page_direction: opts.page_direction,
            comic: prep.is_comic_book,
            reader_margins: prep.is_comic_book && opts.comic_reader_margins.is_some(),
            number_note_icons: opts.number_note_icons,
            opf_name: prep.opf_name.as_deref(),
            screen: opts.screen,
            seen_ids: HashSet::new(),
            img_agent: crate::netimg::http_agent(15),
            remote_counter: 0,
            fetched_imgs: Vec::new(),
            content_props: opts.wash.as_ref().map(|_| HashMap::new()),
        }
    }

    /// 章节 html 最终变换链：解双向脚注互指环 → duokan 图片脚注标记换上标 → 封面拉伸/SVG 修复 → 脚注就地关联重排 →
    /// 远程图内联 → 全书 id 去重。要用到第一遍扫全书才拿得到的 `aside_index`，所以与第一遍分开、顺序不能换。
    fn transform_html_chapter(&mut self, text: &str, name: &str) -> Vec<u8> {
        let t = crate::htmlproc::break_footnote_cycles(text);
        let t = crate::htmlproc::fix_duokan_markers(&t);
        let t = fix_cover_aspect(&t);
        let t = svg_cover_to_img(&t);
        let t = if self.reader_margins { crate::comicpad::pad_page(&t).unwrap_or(t) } else { t };
        let t = if self.skip_notes.contains(name) { t } else { crate::htmlproc::preserve_relink_footnotes(&t, name, self.aside_index, self.footnote) };
        let t = if self.number_note_icons { crate::htmlproc::number_icon_note_links(&t) } else { t };
        let chap_dir = std::path::Path::new(name).parent().and_then(|p| p.to_str()).unwrap_or("");
        let (t, imgs) = inline_remote_images(&t, chap_dir, &mut self.remote_counter, &mut self.taken_names, remote_img_fetcher(&self.img_agent, self.screen));
        self.fetched_imgs.extend(imgs);
        crate::htmlproc::dedup_ids_in_chapter(&t, &mut self.seen_ids).into_bytes()
    }

    /// 文本类条目 → `Some(最终字节)`（无法按 UTF-8 解读的原样借回）；不是文本类（图片/其它）→ `None`，调用方自己处理。
    fn transform_text<'d>(&mut self, name: &str, data: &'d [u8], is_html: bool) -> Option<std::borrow::Cow<'d, [u8]>> {
        use std::borrow::Cow;
        if is_html {
            return Some(match std::str::from_utf8(data) {
                Ok(text) => {
                    let out = self.transform_html_chapter(text, name);
                    if let (Some(props), Ok(t)) = (self.content_props.as_mut(), std::str::from_utf8(&out)) {
                        props.insert(name.to_string(), crate::wash::normalize::content_properties(t));
                    }
                    Cow::Owned(out)
                }
                Err(_) => Cow::Borrowed(data),
            });
        }
        if self.reader_margins && crate::wash::is_wash_css_name(name) {
            let mut out = data.to_vec();
            if !data.windows(crate::comicpad::CSS_RULES.len()).any(|w| w == crate::comicpad::CSS_RULES.as_bytes()) {
                out.extend_from_slice(crate::comicpad::CSS_RULES.as_bytes());
            }
            return Some(Cow::Owned(out));
        }
        if self.opf_name == Some(name) && (self.page_direction.is_some() || self.comic) {
            let Ok(text) = std::str::from_utf8(data) else { return Some(Cow::Borrowed(data)) };
            let mut text = Cow::Borrowed(text);
            if let Some(dir) = self.page_direction {
                text = Cow::Owned(crate::direction::set_spine_direction(&text, dir));
            }
            if self.comic {
                if let Some(t) = crate::comic_detect::tag_opf_as_comic(&text) {
                    text = Cow::Owned(t);
                }
            }
            return Some(match text {
                Cow::Borrowed(_) => Cow::Borrowed(data),
                Cow::Owned(t) => Cow::Owned(t.into_bytes()),
            });
        }
        None
    }
}
