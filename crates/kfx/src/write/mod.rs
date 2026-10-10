//! EPUB → KFX 写出器（最小版）。
//!
//! 输入应是已经按设备优化过的 EPUB；这里只做格式转换，不改内容。
//! 写的片段是删除实验（`docs/kfx.md#删除实验`）定下的最小集合；分词、`$550`/`$621` 位置点、`$597`、`$585` 不写。
//! 支持的内容见 `docs/kfx.md#写出器`（段落、标题、行内样式、图片、链接与注释弹窗、目录、嵌入字体、列表、表格、边框）。

use crate::container::{Body, Entity};
use crate::css::{Border, BorderWidth, Computed, Len, Sheet};
use crate::ion::{self, Item, Value};
use crate::yj::*;
use bookconv::epubbook::{self, Loaded};
use bookconv::epubzip::resolve_link;
use bookconv::util::fnv64;
use ego_tree::NodeRef;
use scraper::{ElementRef, Html, Node};
use std::collections::{HashMap, HashSet};

mod analyze;
mod entities;
mod layout;
mod parse;
mod style;
#[cfg(test)]
mod tests;

use analyze::*;
use entities::*;
use layout::*;
use parse::*;
use style::*;

pub use entities::container_id;

/// 写出器版本：改了产物字节的修改要加一。只进书库指纹，**不写进书里**（见 [`FILE_CREATOR_VERSION`]）。
/// - 7（2026-10-06）：`rgb()`/`rgba()` 的百分比按百分比算；负字号作废；算不出有限值的长度（字号 0 时除以 0）写 0，不写 NaN。
///   没有文字的元素（`<span id>`、`<div id>`）、图片自己的 id 当锚点（挂到下一个块开头，文末的挂到最后一个块末尾），
///   以前丢掉、链接和目录退回文件开头。23 本测试书只有《福尔摩斯探案全集》变了（1101 个锚点、目录、115 个版面多配上注释弹窗），
///   其余逐字节不变；未真机验证。
/// - 8（2026-10-06）：书库生成时唯一 ID 取书 id（以前取不到，退回 OPF 唯一标识符的哈希，见 `library` 的 `kfx_id`）：**书库的 kindle 产物全部变了**
///   （容器 id、content_id、book_id；Kindle 上进度清零，用户接受）。`@media` 按阅读模式的阅读范围、屏幕求值（以前含 screen 就整块收），
///   `<link>`/`<style>` 的 `media` 属性同样处理；选择器优先级不再把伪类括号里的字计成标签；颜色认 `#rgba`/`#rrggbbaa`；声明用
///   `bookconv::html::css_decls` 切。同一份优化后 EPUB、同一个 `--id`，20 本测试书和一卷漫画新旧写出器逐字节相同（书里都没用到这些写法）。
/// - 9（2026-10-06）：图片节点写 CSS 的百分比宽度（`$56`，单位 `$314`，同表格宽度的写法；没有图片宽度的样本，按表格样本推的）。
///   以前图片一律不写宽度，Kindle 按图自身大小显示；优化器 v50 起给带图注的竖长图写 `width:P%`，图和图注排在同一页。
/// - 10（2026-10-08）：照 Send to Kindle 排同一本《绍宋》的结果对齐样式（逐个样式对照，置信度高的才改）：全透明的边框颜色写「透明」
///   `$349`（以前强制不透明，信件、诗词框画成 15.75pt 的黑框）；`<p>`、`<pre>`、`<dl>` 上下 1em，`<blockquote>`、`<figure>`
///   1em/1.5em，`<h1>`–`<h6>` 的 UA 字号和外边距（以前没有：段落挤在一起，没写样式的 `<h2>` 和正文一样大，《绝叫》）；
///   行高按全书正文行高归一（`base_line_height`，正文用阅读器的行距设置），竖直长度按本元素行高换算；左右百分比内边距照写百分比；
///   外边距、内边距的 px 按 1px＝0.45pt；`white-space:nowrap` 写 `$45: true`；`cover` 的页面背景写整页范围 `$645`（照 Amazon）；
///   SVG 包着的封面图（`<image xlink:href>`）不再丢。21 本测试书文字池逐字相同，漫画、全图书逐字节不变。
/// - 11（2026-10-08）：照 Send to Kindle 的规则（逐属性对照《绍宋》《绝叫》，见 docs/kfx.md#send-to-kindle-的样式规则）：normal 字重不写、
///   600 写半粗 `$360`；没有背景处的近黑文字颜色不写；透明度舍去小数；`word-break:break-all` 写 `$569: $570`；标题样式带 `$761: [$760]`；
///   缺字体文件的字体、没嵌入的正文字体写 `default`（以前不写）；左右外边距、内边距写包含块宽度的百分比，窄容器里的首行缩进写百分比；
///   折叠后的外边距在后一块没有上边距时留在前一块；边框的黑色不写。同日第二轮（21 本 Send to Kindle 样本逐属性对照）：字号按正文归一、
///   相对根；行高写成长度的按绝对值继承、下限 0.6；body 左右边距不写（负外边距那侧照加）；百分比按 CSS 实际宽度；`align`、`<font>`；
///   整段的行内样式并进段落；正文字体 `default`、备选整串；声明了的字重照写、`<b>` bolder；对比度 4.5；块宽度；`min-height`、阴影；
///   链接颜色 `$576`/`$577`；修掉 `<div><p style margin:0>` 的样式被外层盖掉。21 本测试书文字池逐字相同，漫画逐字节不变；未真机验证。
/// - 12（2026-10-08，审计修复）：OPF 日期只写到年、月的 `issue_date` 补成 `YYYY-01-01`/`YYYY-MM-01`（以前整个换成 2000-01-01）；
///   行内元素中途遇到块或图片时区间在切开处截断、到新块接着（以前 panic 或区间错位）；`<ul>`/`<ol>` 里直接放的文字、图片不再丢；
///   CSS 落单引号在换行处结束、没收尾的块在文末收尾；行内 `style` 的 `url()` 按文档路径解析；`background:none`/只写颜色的简写重置背景图；
///   `<font size="+很大">` 不溢出；固定版式取不到图的页登记成空页；正文字体字数打平时取名字小的（确定）。
///   24 本测试书和一卷漫画只有《风起陇西》（2011）、《绍宋》（2024-12）因日期变了，别的逐字节不变。
/// - 13（2026-10-09）：正文里没用到封面图（spine 里没有封面页）的书在最前面补一页整页封面，照 Send to Kindle（《绍宋》真机打开没有封面）。
/// - 14（2026-10-09）：spine 里标了 `linear="no"` 的目录页不排（用户定三台都拿掉；`epubbook::load`）。
/// - 15（2026-10-09，审计修复）：`<a href>` 里套着块级元素（`<a href><h2>…</h2></a>`）时块里的文字带上链接（以前丢了，《克莱因壶》目录页
///   5 项点不了）；行内元素前面待写出的空格不再算进它的区间、元素 id 的锚点同样落在第一个字上（`foo <a>bar</a>` 以前 offset 3 len 4），
///   注释配对认区间前紧挨着空格处的 id（《福尔摩斯》`<span id></span> <a>` 照样配上）；取不到的图片上的 id 挂到下一个节点（以前丢了）。
///   不改字节的：元素套得过深（超过 [`MAX_NESTING`]）报错、较深的在大栈线程里解析（以前栈溢出、整个进程中止），`@media` 最多套
///   32 层；同一张取不到的图只警告一次。25 本测试书 6 本变了（《克莱因壶》《绝叫》《消失的爱人》《福尔摩斯》《阿加莎》《啸风山庄》），
///   文字池逐字相同、注释配对不变；未真机验证。
/// - 16（2026-10-10 审计）：层叠认 `font` 简写（`bookconv::cascade` 展开成分项，没写的项重置成 `normal`；以前整条不认）。
///   测试书没有用简写的：24 本文字书 + 1 卷漫画 KFX 逐字节不变。
pub const WRITER_VERSION: &str = "16";

/// 写进书里的创建器版本（`creator_version`、`kfxgen_package_version`），固定不变：Kindle 发现文件字节变了就把书当新书、
/// 阅读进度清零（2026-10-06 真机：只差版本号的《绍宋》覆盖后进度没了，逐字节相同的《嘯風山莊》覆盖后进度还在）。
/// 写出器升版本后内容没变的书要生成逐字节相同的文件，所以这里不跟 [`WRITER_VERSION`] 走。
const FILE_CREATOR_VERSION: &str = "1";

/// 第一个本地符号的编号：系统表 9 个 + `YJ_symbols` v10 的 859 个。
const FIRST_LOCAL_SID: u32 = 10 + 859;

const YJ_SYMBOLS_MAX_ID: i64 = 859;

/// KFX 的 `lh` 单位折合多少个 em（样本里文档缺省行高 1.2em，长度都按它换算）。
const LH_EM: f64 = 1.2;

/// 百分比换算成 em 时假定的页宽（样本里 `margin-top:30%` → 9.6em）。
const PAGE_WIDTH_EM: f64 = 32.0;

/// 资源路径符号和图片字节实体符号的编号差（正好是 Ion 系统符号的个数）。
const SID_GAP: u32 = 9;

/// 元数据 `cover_image` 写的名字，符号编号＝封面资源 + [`SID_GAP`]。
const COVER_REF: &str = "cover-ref";

/// 文档级附加数据的名字；封面图实体叫它加 `-ad`。
const COVER_AUX: &str = "kfxdoc";

#[derive(Clone, Debug, Default)]
pub struct Opts {
    /// 固定唯一 ID（书库用书的 id）；`None` 按书的 OPF 唯一标识符派生。
    pub fixed_id: Option<u64>,
    /// 求值 `@media` 特性条件（`min-width` 等）用的阅读范围、屏幕（[`crate::css::MediaEnv::for_profile`]）；
    /// `None` 时带特性条件的一律不收，只看媒体类型。
    pub media: Option<crate::css::MediaEnv>,
}

/// 按 CSS 原样算出的每个文字块的样式（[`epub_text_styles`]），一行一块，制表符分隔：
/// 文字（去空白，前 60 字）、字号（根 em）、行高（根 em，绝对值）、字体、粗细（normal/bold/semibold/bolder）、斜体、颜色（ARGB 十六进制，`-` 没写）、
/// 对齐、首行缩进（`em:`/`pt:`/`%:` 加数值，`-` 没写）、上边距、下边距（根 em）、左边距、右边距（`em+pct`）。第一行是 `#base` 和全书正文的字号、行高（根 em）。
/// 给 `tools/kfx/s2kdev.py` 比较掌阅、Move 的 EPUB 交给阅读器的样式和 Send to Kindle 的是否一致：不做 Kindle 专有的变换（body 左右边距照算，
/// 不换 `default` 字体、不调对比度、不归一字号、没有行高下限）。
pub fn epub_text_styles<R: std::io::Read + std::io::Seek>(epub: R, opts: &Opts) -> Result<String, String> {
    let mut warnings = Vec::new();
    let book = epubbook::load_from(epub, &mut warnings)?;
    let mut b = Builder::new(HashMap::new(), warnings, opts);
    let mut parsed = parse_docs(&book, b.media, true)?;
    let deep = parsed.iter().map(|d| block_depth(&d.3)).max().unwrap_or(0) > DEEP_NESTING;
    let book = &book;
    // 块要在闭包里释放（`move`）：释放也是递归的
    maybe_big_stack(deep, move || {
        analyze(book, &mut b, &mut parsed);
        let mut out = format!("#base\t{}\t{}\n", b.base_fs, b.base_lh * b.base_fs);
        for (_, _, _, blocks) in &parsed {
            dump_blocks(blocks, &mut out);
        }
        out
    })
}

fn num(v: f64, unit: u32) -> Value {
    // 保留 6 位有效数字左右（和样本一样的精度，避免 0.8333333333 这种长尾）。
    // 字号 0 的元素换算长度时会除以 0：算不出有限值的写 0，不往书里写 NaN、无穷大。
    let r = if v.is_finite() { (v * 1e6).round() / 1e6 } else { 0.0 };
    Value::Struct(vec![(VALUE, Value::F64(if r == 0.0 { 0.0 } else { r })), (UNIT, Value::Symbol(unit))])
}

struct Res {
    name: String,
    location: String,
    format: u32,
    mime: &'static str,
    width: u32,
    height: u32,
    bytes: Vec<u8>,
}

struct Builder {
    locals: Vec<String>,
    local_index: HashMap<String, u32>,
    next_eid: i64,
    /// 样式去重：属性编码 → 样式名的符号。
    styles: HashMap<Vec<u8>, u32>,
    /// 算样式去重键用的缓冲（每次清空复用，只在新样式时复制一份当键）。
    style_key: Vec<u8>,
    /// 对比度调整的结果（[`crate::css::ensure_contrast`] 要逐级试，全书反复是那几对颜色）：(前景, 背景) → 调过的前景。
    contrast_fix: HashMap<(u32, u32), u32>,
    /// 没写颜色时缺省黑字在这个背景上要不要换颜色（[`text_color`]）：背景 → 换成的颜色。
    contrast_default: HashMap<u32, Option<u32>>,
    style_entities: Vec<(String, Vec<(u32, Value)>)>,
    resources: Vec<Res>,
    res_by_path: HashMap<String, usize>,
    /// 取不到的图片（找不到或格式不支持）：只警告一次，以后直接跳过（以前每引用一次警告一次）。
    missing: HashSet<String>,
    images: HashMap<String, (Vec<u8>, &'static str)>,
    warnings: Vec<String>,
    /// 链接、目录用到的锚点：(文件, 锚点) → 锚点名，按出现顺序。
    anchors: Vec<((String, String), u32)>,
    anchor_index: HashMap<(String, String), u32>,
    /// 标题（级别, 节点 id），按书里的顺序。
    headings: Vec<(u8, i64)>,
    /// 样式里用到的字体名（嵌入字体只嵌这些）。
    used_fonts: std::collections::BTreeSet<String>,
    /// 写成 `default` 的字体名（小写）：正文字体（见 [`TextCounts::body_font`]）。
    default_fonts: HashSet<String>,
    /// 书里嵌入了字体文件的字体名（小写）。
    embedded_fonts: HashSet<String>,
    /// 求值 `@media` 用（见 [`Opts::media`]）。
    media: Option<crate::css::MediaEnv>,
    /// 全书正文的行高（元素字号的倍数，见 [`base_line_height`]）：KFX 的行高按它归一。
    base_lh: f64,
    /// 全书正文的字号（根 em，见 [`base_font_size`]）。
    base_fs: f64,
}

/// EPUB → KFX。返回 KFX 字节和警告。
pub fn epub_to_kfx(epub: &[u8], opts: &Opts) -> Result<(Vec<u8>, Vec<String>), String> {
    epub_to_kfx_from(std::io::Cursor::new(epub), opts)
}

/// 同 [`epub_to_kfx`]，从可定位的读取器（如打开的文件）读 EPUB：不用先把整本读进内存，峰值少一份压缩包大小。
pub fn epub_to_kfx_from<R: std::io::Read + std::io::Seek>(epub: R, opts: &Opts) -> Result<(Vec<u8>, Vec<String>), String> {
    let mut warnings = Vec::new();
    let mut book = epubbook::load_from(epub, &mut warnings)?;
    let book = &mut book;
    // 图片字节从书里搬出来（写出器只在这里用到它们），不复制。
    let images = std::mem::take(&mut book.images).into_iter().map(|i| (i.path, (i.bytes, i.mime))).collect();
    let mut b = Builder::new(images, warnings, opts);
    // 封面资源最先登记，紧跟着排好 `cover_image` 要用的名字：元数据 `cover_image` 也是按「这个名字的符号编号 − 9」
    // 找封面资源的（6 本样本的 `e6` 减 9 都正好是封面 JPEG 的 `$164`；书架缩略图靠它，2026-10-05 真机）。
    if let Some(i) = book.cover.as_deref().and_then(|c| b.resource(c)) {
        let name = b.resources[i].name.clone();
        let res = b.sym(&name);
        for k in 1..SID_GAP {
            b.sym(&format!("pad-cover-{k}"));
        }
        let cover_ref = b.sym(COVER_REF);
        debug_assert_eq!(cover_ref, res + SID_GAP);
    }
    let id = opts.fixed_id.unwrap_or(book.meta.stable_id);
    let out = build(book, &mut b, id)?;
    Ok((out, b.warnings))
}

/// 整本书写成 KFX：解析文档 → 全书分析 → 版面 → 样式 → 位置映射 → 目录、锚点、导航 → 元数据 → 资源 → 清单与容器。
/// 各阶段按这个顺序分配本地符号，换顺序会改产物字节（资源的符号必须最后分配，见 [`resource_entities`]）。
/// 元素套得深的书（见 [`MAX_NESTING`]）解析以后的部分在大栈线程里跑：版面、编码、释放块都是递归的。
fn build(book: &mut Loaded, b: &mut Builder, id: u64) -> Result<Vec<u8>, String> {
    let mut parsed = parse_docs(book, b.media, false)?;
    let deep = parsed.iter().map(|d| block_depth(&d.3)).max().unwrap_or(0) > DEEP_NESTING;
    let fixed_canvas = fixed_canvas(book);
    // 翻页方向：`$557` 从左往右、`$559` 从右往左（2026-10-05 测试漫画 LTR/RTL 两本只差这一处）。
    let direction = if book.meta.rtl { DIR_RTL } else { DIR_LTR };
    let Layout { mut sections, mut entities, id_map, cover_tmpl } = {
        let book = &*book;
        maybe_big_stack(deep, || {
            prepend_cover_page(book, &mut parsed);
            analyze(book, b, &mut parsed);
            lay_out(book, b, parsed, fixed_canvas, direction)
        })??
    };
    // 封面图即使没出现在正文里也要带上（书架缩略图）。
    let cover_res = book.cover.as_deref().and_then(|c| b.resource(c));
    style_entities(b, &mut entities);
    let fonts = take_fonts(book, b);
    position_entities(&mut sections, &mut entities);
    navigation_entities(book, b, &sections, &id_map, cover_tmpl, &mut entities);
    metadata_entities(book, b, id, &sections, fixed_canvas, direction, cover_res, &mut entities);
    resource_entities(b, fonts, cover_res, &mut entities);
    maybe_big_stack(deep, || finish(b, &sections, entities, cover_res, id))
}

impl Builder {
    fn sym(&mut self, name: &str) -> u32 {
        if let Some(&s) = self.local_index.get(name) {
            return s;
        }
        let s = FIRST_LOCAL_SID + self.locals.len() as u32;
        self.locals.push(name.to_string());
        self.local_index.insert(name.to_string(), s);
        s
    }

    fn anchor(&mut self, key: (String, String)) -> u32 {
        if let Some(&a) = self.anchor_index.get(&key) {
            return a;
        }
        let a = self.sym(&format!("anchor{}", self.anchors.len()));
        self.anchor_index.insert(key.clone(), a);
        self.anchors.push((key, a));
        a
    }

    fn eid(&mut self) -> i64 {
        let e = self.next_eid;
        self.next_eid += 1;
        e
    }

    /// 样式去重：属性一样的共用一个样式片段。
    fn style(&mut self, mut props: Vec<(u32, Value)>) -> u32 {
        props.sort_by_key(|(k, _)| *k);
        // 没嵌入的正文字体、`@font-face` 声明了却没有字体文件的字体写成 `default`（Send to Kindle 同样，《绍宋》的「宋体」）：
        // 阅读器用自己的字体；以前不写，嵌入了字体的父节点下面会继承父节点的字体
        // 备选照写小写；第一个是嵌入字体时照原样（字体片段按这个名字找）
        for (k, v) in props.iter_mut() {
            if let (P_FONT_FAMILY, Value::String(f)) = (*k, &*v) {
                let (first, rest) = f.split_once(',').map_or((f.as_str(), None), |(a, b)| (a, Some(b)));
                let first = if self.default_fonts.contains(&first.to_lowercase()) {
                    FONT_DEFAULT.to_string()
                } else if self.embedded_fonts.contains(&first.to_lowercase()) {
                    first.to_string()
                } else {
                    first.to_lowercase()
                };
                *v = Value::String(match rest {
                    Some(r) => format!("{first},{r}"),
                    None => first,
                });
            }
        }
        for (k, v) in &props {
            if let (P_FONT_FAMILY, Value::String(f)) = (*k, v) {
                let first = f.split(',').next().unwrap_or_default();
                if first != FONT_DEFAULT && self.embedded_fonts.contains(&first.to_lowercase()) {
                    self.used_fonts.insert(first.to_string());
                }
            }
        }
        // 键：每个属性的编号 + 值的 Ion 编码（Ion 值自带长度，拼起来不会混淆）。缓冲复用，查到了就不分配。
        let mut key = std::mem::take(&mut self.style_key);
        key.clear();
        for (k, v) in &props {
            key.extend(k.to_le_bytes());
            ion::encode_value(&mut key, v);
        }
        if let Some(&s) = self.styles.get(key.as_slice()) {
            self.style_key = key;
            return s;
        }
        let name = format!("style{}", self.style_entities.len());
        let s = self.sym(&name);
        self.styles.insert(key.clone(), s);
        self.style_key = key;
        self.style_entities.push((name, props));
        s
    }

    fn resource(&mut self, path: &str) -> Option<usize> {
        if let Some(&i) = self.res_by_path.get(path) {
            return Some(i);
        }
        if self.missing.contains(path) {
            return None;
        }
        let (format, mime) = match self.images.get(path).map(|(_, m)| *m) {
            Some("image/jpeg") => (FORMAT_JPG, "image/jpg"),
            Some("image/png") => (FORMAT_PNG, "image/png"),
            Some("image/gif") => (FORMAT_GIF, "image/gif"),
            other => {
                self.warnings.push(match other {
                    Some(other) => format!("图片格式 {other} 不支持：{path}"),
                    None => format!("图片找不到或格式不支持：{path}"),
                });
                self.missing.insert(path.to_string());
                return None;
            }
        };
        // 字节搬进资源，不复制（大漫画几百 MB）；同一路径以后走上面的 `res_by_path`，不会再来取。
        let (bytes, _) = self.images.remove(path)?;
        let (width, height) = image::ImageReader::new(std::io::Cursor::new(&bytes))
            .with_guessed_format()
            .ok()
            .and_then(|r| r.into_dimensions().ok())
            .unwrap_or((0, 0));
        let i = self.resources.len();
        self.resources.push(Res { name: format!("img{i}"), location: format!("resource/img{i}"), format, mime, width, height, bytes });
        // 图片字节的实体名另起（样本里是 `…-ad`，和 `$165` 的资源路径不同名；用 `resource/…` 当实体名时
        // Kindle 不显示图片，2026-10-05 真机）。
        self.res_by_path.insert(path.to_string(), i);
        Some(i)
    }

    fn new(images: HashMap<String, (Vec<u8>, &'static str)>, warnings: Vec<String>, opts: &Opts) -> Builder {
        Builder {
            locals: Vec::new(),
            local_index: HashMap::new(),
            next_eid: 1,
            styles: HashMap::new(),
            style_key: Vec::new(),
            contrast_fix: HashMap::new(),
            contrast_default: HashMap::new(),
            style_entities: Vec::new(),
            resources: Vec::new(),
            res_by_path: HashMap::new(),
            missing: HashSet::new(),
            images,
            warnings,
            anchors: Vec::new(),
            anchor_index: HashMap::new(),
            headings: Vec::new(),
            used_fonts: Default::default(),
            default_fonts: HashSet::new(),
            embedded_fonts: HashSet::new(),
            base_lh: LH_EM,
            base_fs: 1.0,
            media: opts.media,
        }
    }
}
