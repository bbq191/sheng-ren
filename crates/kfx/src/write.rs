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
pub const WRITER_VERSION: &str = "12";

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
    let mut parsed = parse_docs(&book, b.media, true);
    analyze(&book, &mut b, &mut parsed);
    let mut out = format!("#base\t{}\t{}\n", b.base_fs, b.base_lh * b.base_fs);
    for (_, _, _, blocks) in &parsed {
        dump_blocks(blocks, &mut out);
    }
    Ok(out)
}

// ---------------------------------------------------------------- 中间结构

/// 竖直方向长度，单位：根字号的 em。
type Vert = f64;

/// 水平方向长度：根 em + 百分比。
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct Horiz {
    em: f64,
    pct: f64,
}

/// 写一个节点的样式时 KFX 父节点的情况（Send to Kindle 只写和父节点不同的字重、颜色；水平长度按包含块宽度写成百分比）。
#[derive(Clone, Copy, Debug)]
struct Parent {
    /// 实际显示的字重（`WEIGHT_*`）。
    weight: u32,
    /// 实际显示的文字颜色（写进样式的；`None`＝阅读器缺省）。
    color: Option<u32>,
    /// 包含块宽度（根 em，整页 [`PAGE_WIDTH_EM`]）。
    avail: f64,
    /// 全书正文字号（根 em，见 [`base_font_size`]）：字号写成它的倍数。
    base_fs: f64,
}

impl Parent {
    fn root(base_fs: f64) -> Parent {
        Parent { weight: WEIGHT_NORMAL, color: None, avail: PAGE_WIDTH_EM, base_fs }
    }
}

#[derive(Clone, Debug)]
struct Run {
    start: usize,
    len: usize,
    /// 和所在段落不同的样式；`None`＝只是链接。
    comp: Option<Computed>,
    /// 书内链接的目标（文件, 锚点）。
    link: Option<(String, String)>,
    /// 注释引用（点了弹窗）：见 [`mark_notes`]。
    note_ref: bool,
    /// 是 `<a>`（有没有 href 都算）：颜色写成链接的颜色（`$576`/`$577`），见 [`Builder::node`]。
    anchor: bool,
}

#[derive(Clone, Debug)]
enum Kind {
    Text { text: String, runs: Vec<Run> },
    Image { src: String },
    Container(Vec<Block>),
}

#[derive(Clone, Debug)]
struct Block {
    kind: Kind,
    comp: Computed,
    heading: Option<u8>,
    margin_top: Vert,
    margin_bottom: Vert,
    margin_left: Horiz,
    margin_right: Horiz,
    padding: [Vert; 4],
    /// 左右内边距（右、左）：百分比照写成百分比（Send to Kindle 同样；以前按页宽 32em 换成 em，字号调大以后
    /// 《绍宋》章名横幅 `padding: 0.3em 33%` 的两边内边距跟着变宽，挤掉文字）。上面 `padding` 的左右两项只用来判断有没有内边距。
    padding_h: [Horiz; 2],
    /// 本块里元素的 id → 字符偏移（目录、锚点用）。
    ids: Vec<(String, usize)>,
    /// 注释正文（弹窗里显示的内容）：见 [`mark_notes`]。
    note: bool,
    /// 节点类型（`None`＝文字 `$269`／容器 `$270`）：列表、列表项、表格各层、水平线。
    ty: Option<u32>,
    /// 节点上的字段（列表符号、表格边框合并等）。
    attrs: Vec<(u32, Value)>,
    /// 额外的样式属性（单元格跨行跨列、竖直对齐，表格宽度等）。
    extra: Vec<(u32, Value)>,
    /// 容器（带背景、边框的块，列表项、单元格）里只有本元素文字的匿名文字块：样式只写对齐，别的从容器继承
    /// （Send to Kindle 同样：《罗杰疑案》带背景的章标题，字体、字号、颜色都写在容器上，里面的文字只有 `text-align`）。
    bare: bool,
    /// 直接由行内内容合成的文字块（[`Doc::flush`]），不是哪个块级元素自己：包着它的元素可以把它当成自己的文字。
    /// 以前只看有没有边距，`<li><p class="footnote">` 这种没写边距的 `<p>` 也被当成匿名的，样式被 `<li>` 的盖掉（《春雪》注释的字号、缩进、行高）。
    inline: bool,
    /// 前面折掉的空段（[`Doc::gap`]）：外边距折叠之后加到上边距上（[`apply_gaps`]）。
    gap_before: Vert,
}

/// 只有空白的段落（`<p>&nbsp;</p>`、`<p>　</p>`、`<p><br/></p>`、段落之间单独的 `<br/>`）不出节点，折成下一块上边距多出的
/// 本元素行高的这么多倍（Send to Kindle 同样，2026-10-08 量的：《绝叫》空段 0.594 行高、《金庸》`<p><br/></p>` 0.595 × 1.7em、
/// 段落间单独的 `<br/>` 0.72em ＝ 0.6 × 缺省行高 1.2em）。以前整段丢掉、不折，场景空行在 Kindle 上没了。
const BLANK_LINE_FOLD: f64 = 0.6;

const BLOCK_TAGS: &[&str] = &[
    "address", "article", "aside", "blockquote", "body", "center", "dd", "details", "dialog", "dir", "div", "dl", "dt", "fieldset", "figcaption",
    "figure", "footer", "form", "h1", "h2", "h3", "h4", "h5", "h6", "header", "hgroup", "hr", "li", "main", "menu", "nav", "ol", "p", "pre",
    "section", "summary", "table", "tbody", "td", "tfoot", "th", "thead", "tr", "ul", "caption",
];

fn is_block_el(el: &ElementRef, comp: &Computed) -> bool {
    match comp.display.as_deref() {
        Some("block" | "list-item" | "table" | "table-row" | "table-cell" | "flex") => true,
        Some("inline" | "inline-block") => false,
        _ => BLOCK_TAGS.contains(&el.value().name()),
    }
}

fn to_vert(l: Option<Len>, fs: f64) -> Vert {
    match l {
        Some(Len::Em(n)) => n * fs,
        Some(Len::Pt(n)) => n / 12.0,
        Some(Len::Percent(p)) => p / 100.0 * PAGE_WIDTH_EM,
        None => 0.0,
    }
}

fn to_horiz(l: Option<Len>, fs: f64) -> Horiz {
    match l {
        Some(Len::Em(n)) => Horiz { em: n * fs, pct: 0.0 },
        Some(Len::Pt(n)) => Horiz { em: n / 12.0, pct: 0.0 },
        Some(Len::Percent(p)) => Horiz { em: 0.0, pct: p },
        None => Horiz::default(),
    }
}

/// 块的外边距、内边距换算成根 em（竖直）和根 em + 百分比（水平），字段同 [`Block`]。
struct BoxLens {
    margin_top: Vert,
    margin_bottom: Vert,
    margin_left: Horiz,
    margin_right: Horiz,
    padding: [Vert; 4],
    padding_h: [Horiz; 2],
}

impl BoxLens {
    fn of(c: &Computed) -> BoxLens {
        let fs = c.font_size;
        BoxLens {
            margin_top: to_vert(c.margin[0], fs),
            margin_bottom: to_vert(c.margin[2], fs),
            margin_left: to_horiz(c.margin[3], fs),
            margin_right: to_horiz(c.margin[1], fs),
            padding: [0, 1, 2, 3].map(|i| to_vert(c.padding[i], fs)),
            padding_h: [to_horiz(c.padding[1], fs), to_horiz(c.padding[3], fs)],
        }
    }
}

/// CSS 外边距折叠：两个相邻外边距合成一个。
fn collapse(a: Vert, b: Vert) -> Vert {
    match (a >= 0.0, b >= 0.0) {
        (true, true) => a.max(b),
        (false, false) => a.min(b),
        _ => a + b,
    }
}

struct Doc<'a> {
    path: &'a str,
    sheet: Sheet,
    lang: Option<String>,
    /// 还没落到内容上的锚点：没有文字的元素（`<span id="x"></span>`、`<div id="x"></div>`）的 id。挂到文档顺序里
    /// 下一个生成的块的开头（[`Doc::take_pending`]）。以前直接丢掉，指向它们的链接、目录项退回到文件开头
    /// （《福尔摩斯探案全集》目录页「第一册」指向页末插图前的空锚点，点了停在目录页开头；全书 1102 处）。
    pending: std::cell::RefCell<Vec<String>>,
    /// 还没落到块上的空段折出来的高度（根 em，见 [`BLANK_LINE_FOLD`]）：和 `pending` 一样给下一个生成的块。
    gap: std::cell::Cell<Vert>,
}

/// 收集行内内容：文字（空白按 CSS 折叠）、`<br>`、行内元素的样式区间、遇到图片就切开。
struct Inline {
    text: String,
    chars: usize,
    runs: Vec<Run>,
    ids: Vec<(String, usize)>,
    pending_space: bool,
    /// 还没走完的行内元素的区间（外层在前）：起点、插到 `runs` 的哪个下标（外层的区间排在里层的前面）、区间本身（长度待定）。
    /// 走到一半遇到块或图片要切开（[`Doc::flush`]）时，区间在切开处截断，到新缓冲从 0 接着开（见 [`Inline::cut_open`]）。
    open: Vec<(usize, usize, Run)>,
}

impl Inline {
    fn new() -> Self {
        Inline { text: String::new(), chars: 0, runs: Vec::new(), ids: Vec::new(), pending_space: false, open: Vec::new() }
    }

    /// 一个行内元素走完（或在切开处截断）：有字的话把区间插进 `runs`。
    fn close_run(&mut self, start: usize, run_at: usize, run: Run) {
        if self.chars > start {
            self.runs.insert(run_at, Run { start, len: self.chars - start, ..run });
        }
    }

    /// 切开之前：还开着的区间（从里层到外层，外层插在前面）截断在这里；返回给新缓冲用的、从 0 重新开始的区间。
    fn cut_open(&mut self) -> Vec<(usize, usize, Run)> {
        let open = std::mem::take(&mut self.open);
        for (start, run_at, run) in open.iter().rev() {
            self.close_run(*start, *run_at, run.clone());
        }
        open.into_iter().map(|(_, _, run)| (0, 0, run)).collect()
    }

    fn push_text(&mut self, s: &str, pre: bool) {
        for c in s.chars() {
            if !pre && c.is_ascii_whitespace() {
                self.pending_space = self.chars > 0 && !self.text.ends_with('\n');
                continue;
            }
            if self.pending_space {
                self.text.push(' ');
                self.chars += 1;
                self.pending_space = false;
            }
            self.text.push(c);
            self.chars += 1;
        }
    }

    fn push_break(&mut self) {
        self.pending_space = false;
        self.text.push('\n');
        self.chars += 1;
    }

    fn is_empty(&self) -> bool {
        self.text.trim().is_empty()
    }

    fn take(&mut self) -> Inline {
        std::mem::replace(self, Inline::new())
    }
}

/// HTML 的表现属性当成优先级最低的样式（样式表写了的不动）：块的 `align`、`<center>`、`<font size/color/face>`
/// （Send to Kindle 同样：《福尔摩斯》`<p align="justify">` 两端对齐、`<font size="1">` 字号 0.625、`size="7"` 3.0）。
fn presentational_hints(el: &ElementRef, decls: &mut HashMap<String, String>) {
    let e = el.value();
    let mut hint = |k: &str, v: String| {
        decls.entry(k.to_string()).or_insert(v);
    };
    match e.name() {
        "p" | "div" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6" | "td" | "th" | "tr" | "caption" | "blockquote" => {
            if let Some(a) = e.attr("align").map(|a| a.trim().to_ascii_lowercase()).filter(|a| ["left", "right", "center", "justify"].contains(&a.as_str())) {
                hint("text-align", a);
            }
        }
        "center" => hint("text-align", "center".into()),
        "font" => {
            // HTML 字号 1–7（同浏览器：10、13、16、18、24、32、48px，绝对字号），`+n`/`-n` 相对 3（饱和加减：`+2147483647` 以前溢出）
            if let Some(s) = e.attr("size").map(str::trim) {
                let n = match s.strip_prefix('+') {
                    Some(r) => r.parse::<i32>().ok().map(|r| 3i32.saturating_add(r)),
                    None => match s.strip_prefix('-') {
                        Some(r) => r.parse::<i32>().ok().map(|r| 3i32.saturating_sub(r)),
                        None => s.parse::<i32>().ok(),
                    },
                };
                if let Some(n) = n {
                    let em = [0.625, 0.8125, 1.0, 1.125, 1.5, 2.0, 3.0][(n.clamp(1, 7) - 1) as usize];
                    hint("font-size", format!("{em}rem"));
                }
            }
            if let Some(c) = e.attr("color") {
                hint("color", c.trim().to_string());
            }
            if let Some(f) = e.attr("face") {
                hint("font-family", f.trim().to_string());
            }
        }
        _ => {}
    }
}

/// 文档末尾没落到内容上的锚点（`…</p><span id="x"></span></body>`）挂到最后一个块的末尾。
fn attach_trailing(blocks: &mut [Block], ids: Vec<String>) {
    let Some(last) = blocks.last_mut() else { return };
    match &mut last.kind {
        Kind::Container(c) if !c.is_empty() => attach_trailing(c, ids),
        Kind::Text { text, .. } => {
            let end = text.chars().count();
            last.ids.extend(ids.into_iter().map(|i| (i, end)));
        }
        _ => last.ids.extend(ids.into_iter().map(|i| (i, 0))),
    }
}

/// 把 `lead` 里的锚点放到块开头（字符偏移 0）。
fn prepend_ids(ids: &mut Vec<(String, usize)>, lead: Vec<String>) {
    ids.splice(0..0, lead.into_iter().map(|i| (i, 0)));
}

impl Doc<'_> {
    /// 取走还没落到内容上的锚点（见 [`Doc::pending`]）。
    fn take_pending(&self) -> Vec<String> {
        self.pending.take()
    }

    fn comp(&self, el: &ElementRef, parent: &Computed) -> Computed {
        let mut decls = self.sheet.cascade(el, self.path);
        presentational_hints(el, &mut decls);
        let mut c = Computed::derive(parent, &decls, el.value().name());
        if let Some(l) = el.value().attr("xml:lang").or_else(|| el.value().attr("lang")) {
            c.lang = Some(l.to_string());
        }
        c
    }

    /// 把一个块级元素（计算值 `comp` 由调用方算好，层叠不重复做）展开成块序列。
    fn block(&self, el: ElementRef, comp: Computed, out: &mut Vec<Block>) {
        if comp.display.as_deref() == Some("none") {
            return;
        }
        let name = el.value().name();
        if matches!(name, "head" | "script" | "style" | "title") {
            return;
        }
        // 本元素之前攒下的锚点落在本元素的开头。
        let pre = self.take_pending();
        let pre_gap = self.gap.take();
        match name {
            "ul" | "ol" => {
                // 列表符号看列表项（`li` 上写的优先，《雪国》把 `cjk-ideographic` 写在 `li` 上）；列表项写成
                // `display:block` 的不显示符号（《啸风山庄》`li {display:block}`），和 `list-style:none` 一样当普通块。
                let first = el.children().filter_map(ElementRef::wrap).find(|c| c.value().name() == "li").map(|li| self.comp(&li, &comp));
                let mut comp = comp.clone();
                if let Some(li) = &first {
                    if li.list_style.is_some() {
                        comp.list_style = li.list_style.clone();
                    }
                }
                let marker = first.as_ref().is_none_or(|li| li.display.as_deref().is_none_or(|d| d == "list-item"));
                if marker && comp.list_style.as_deref() != Some("none") {
                    let mut b = self.list(el, comp, first);
                    prepend_ids(&mut b.ids, pre);
                    b.gap_before += pre_gap;
                    return out.push(b);
                }
            }
            "table" => {
                let mut b = self.table(el, comp);
                prepend_ids(&mut b.ids, pre);
                b.gap_before += pre_gap;
                return out.push(b);
            }
            "hr" => {
                let mut b = self.boxed(Kind::Container(Vec::new()), comp, el.value().attr("id"));
                b.ty = Some(NODE_HR);
                // UA 样式：上下 0.5em。
                if b.comp.margin[0].is_none() {
                    b.margin_top = 0.5 * b.comp.font_size;
                }
                if b.comp.margin[2].is_none() {
                    b.margin_bottom = 0.5 * b.comp.font_size;
                }
                prepend_ids(&mut b.ids, pre);
                b.gap_before += pre_gap;
                return out.push(b);
            }
            _ => {}
        }
        let heading = bookconv::html::heading_level_of(name);
        let mut children = Vec::new();
        self.children(el, &comp, &mut children);
        let id = el.value().attr("id").map(str::to_string);
        // 开头的锚点：之前攒下的 + 本元素自己的 id。
        let mut lead = pre;
        lead.extend(id);
        if children.is_empty() {
            // 没有内容的元素：锚点（连同里面攒下的）留给后面的内容。
            let inner = self.take_pending();
            lead.extend(inner);
            *self.pending.borrow_mut() = lead;
            self.gap.set(self.gap.get() + pre_gap);
            return;
        }
        let BoxLens { margin_top, margin_bottom, margin_left: mut ml, margin_right: mr, padding, padding_h } = BoxLens::of(&comp);
        // 不显示符号的列表（`list-style:none`）当普通块，照 Amazon 缩进 1.5em（测试书 L04：`$48` 4.688%）。
        if matches!(name, "ul" | "ol") && comp.margin[3].is_none() && comp.padding[3].is_none() {
            ml.em += 1.5;
        }
        // 自己只包着一个文字块（普通段落）：本元素就是这个块。
        // 有背景或内边距的块写成容器套文字（样本里带背景色的 h1 就是这样；背景、内边距、负外边距直接放在文字段落上，
        // Kindle 上长标题会溢出屏幕，2026-10-05 真机）。
        let boxed = comp.background.is_some() || comp.bg_image.is_some() || padding.iter().any(|p| *p != 0.0) || comp.has_border() || comp.box_shadow.is_some();
        let own = !boxed && children.len() == 1 && matches!(children[0].kind, Kind::Text { .. }) && children[0].is_anonymous() && children[0].inline;
        // 有宽度的包裹层也写成容器，宽度写在容器上；只有自己文字的块宽度写在文字上（Send to Kindle 同样：《金庸》60% 宽的 div、
        // 《平凡的世界》`width:100%` 的章标题）
        let boxed = boxed || (!own && comp.width.is_some());
        if own {
            let mut b = children.pop().unwrap_or_else(|| unreachable!());
            // 整段只有一个带样式的行内元素（`<p><span class="大字">版权信息</span></p>`、`<p><b>注：…</b></p>`）：它的文字样式并进段落
            // （Send to Kindle 同样：《人生海海》字号 1.833 写在段落上、行高按它算；《金庸》整段的 `<b>` 是段落的 bolder）
            let mut comp = comp;
            if let Kind::Text { text, runs } = &mut b.kind {
                // 几层都覆盖整段时（`<span class="大字"><span class="bold">目录</span></span>`）取最里层，它的计算值已经含外层；链接照留
                let n = text.chars().count();
                let whole = !runs.is_empty() && runs.iter().all(|r| r.start == 0 && r.len == n && !r.note_ref);
                let inner = runs.iter().rev().find_map(|r| r.comp.as_ref()).cloned();
                if let (true, Some(rc)) = (whole, inner) {
                    if !rc.superscript && !rc.subscript && !rc.has_border() && rc.background.is_none() {
                        // `<a>` 的颜色是链接颜色，留在链接区间上（Send to Kindle 写成 `$576`/`$577`），不并进段落
                        let anchor = runs.iter().any(|r| r.anchor);
                        let color = comp.color;
                        comp = merge_text_style(&comp, &rc);
                        if anchor {
                            comp.color = color;
                        }
                        runs.retain_mut(|r| {
                            if !(r.anchor && r.comp.as_ref().is_some_and(|c| c.color != color)) {
                                r.comp = None;
                            }
                            r.link.is_some() || r.comp.is_some()
                        });
                    }
                }
            }
            b.comp = comp;
            // 现在它是本元素自己的块了，外层元素不能再把它当成自己的文字
            b.inline = false;
            b.heading = heading;
            b.margin_top = margin_top;
            b.margin_bottom = margin_bottom;
            b.margin_left = ml;
            b.margin_right = mr;
            b.padding = padding;
            b.padding_h = padding_h;
            prepend_ids(&mut b.ids, lead);
            b.gap_before += pre_gap;
            out.push(b);
            return;
        }
        if boxed {
            // 页面一级的 `cover` 背景：照 Send to Kindle 写整页范围（`$645`），背景铺满一页而不是只在内容范围里画（《绍宋》卷首语，真机 ✓）。
            // 2026-10-08 试过也给 `fixed` 写了尺寸的写（制作说明），真机没有效果，用户说不改了，撤回。只认 body：别的块上没见过样本。
            let page_bg = comp.bg_cover;
            let attrs = if name == "body" && page_bg && comp.bg_image.is_some() {
                let b = BG_PAGE_BOUNDS_KEYS.iter().zip([0.0, 0.0, 100.0, 100.0]).map(|(&k, v)| (k, num(v, U_PERCENT))).collect();
                vec![(BG_PAGE_BOUNDS, Value::Struct(b))]
            } else {
                Vec::new()
            };
            let mut children = children;
            mark_bare(&mut children, &comp);
            out.push(Block {
                kind: Kind::Container(children),
                comp,
                heading,
                margin_top,
                margin_bottom,
                margin_left: ml,
                margin_right: mr,
                padding,
                padding_h,
                ids: lead.into_iter().map(|i| (i, 0)).collect(),
                note: false,
                ty: None,
                attrs,
                extra: Vec::new(),
            bare: false,
            inline: false,
            gap_before: pre_gap,
            });
            return;
        }
        // 没有背景的包裹层摊平：竖直外边距、内边距并进首尾子块，水平外边距加到每个子块上。
        let n = children.len();
        let mut lead = Some(lead);
        for (i, mut c) in children.into_iter().enumerate() {
            if i == 0 {
                c.margin_top = collapse(margin_top + padding[0], c.margin_top);
                c.gap_before += pre_gap;
                prepend_ids(&mut c.ids, lead.take().unwrap_or_default());
                if c.heading.is_none() {
                    c.heading = heading;
                }
            }
            if i + 1 == n {
                c.margin_bottom = collapse(margin_bottom + padding[2], c.margin_bottom);
            }
            c.margin_left.em += ml.em;
            c.margin_left.pct += ml.pct;
            c.margin_right.em += mr.em;
            c.margin_right.pct += mr.pct;
            out.push(c);
        }
    }

    /// 自己的外边距、内边距照计算值的块（列表、表格、单元格、水平线用；不摊平、不并进子块）。
    fn boxed(&self, kind: Kind, comp: Computed, id: Option<&str>) -> Block {
        let l = BoxLens::of(&comp);
        let mut b = Block::anonymous(kind, comp, id.map(|i| vec![(i.to_string(), 0)]).unwrap_or_default());
        (b.margin_top, b.margin_bottom, b.margin_left, b.margin_right, b.padding, b.padding_h) =
            (l.margin_top, l.margin_bottom, l.margin_left, l.margin_right, l.padding, l.padding_h);
        b
    }

    /// 元素的子块；只有一个匿名文字块时直接用它当 `ty` 类型的节点（列表项、表格标题里的文字），否则包成容器。
    fn wrap_children(&self, el: ElementRef, comp: Computed, ty: Option<u32>) -> Block {
        let mut kids = Vec::new();
        self.children(el, &comp, &mut kids);
        let id = el.value().attr("id");
        if ty.is_some() && kids.len() == 1 && matches!(kids[0].kind, Kind::Text { .. }) && kids[0].is_anonymous() {
            if let Some(k) = kids.pop() {
                // 文字的样式用里面那个块的（`<li><p class="footnote">` 的字号、缩进、行高、字体，以前丢了，用的是 `<li>` 的），
                // 盒子（边距、内边距、背景、边框）用本元素的
                let text_comp = with_box(&k.comp, &comp);
                let mut b = self.boxed(k.kind, comp, id);
                b.comp = text_comp;
                b.ids.extend(k.ids);
                b.ty = ty;
                return b;
            }
        }
        mark_bare(&mut kids, &comp);
        let mut b = self.boxed(Kind::Container(kids), comp, id);
        b.ty = ty;
        b
    }

    /// `<ul>`/`<ol>` → `$276` 列表，`<li>` → `$277` 列表项（只有文字的列表项本身就是文字节点）。
    /// 列表符号的缺省按标签和嵌套层数（同浏览器：圆点 → 空心圆 → 方块），Amazon 转出来也是这样（2026-10-05 测试书）。
    /// `first_li`：第一个 `<li>` 的计算值（调用方判断要不要当列表时已经层叠过，不再算一遍；父元素只差列表符号，
    /// 而 `<li>` 的列表符号本来就继承或自己写，结果一样）。
    /// 直接放在 `<ul>`/`<ol>` 里的文字、行内元素、图片（不在 `<li>` 里）照浏览器当匿名块排在列表里、不带符号（以前丢掉）。
    fn list(&self, el: ElementRef, comp: Computed, mut first_li: Option<Computed>) -> Block {
        let ordered = el.value().name() == "ol";
        let depth = el.ancestors().filter_map(ElementRef::wrap).filter(|a| a.value().name() == "ul").count();
        let style = match comp.list_style.as_deref() {
            Some("disc") => LIST_DISC,
            Some("circle") => LIST_CIRCLE,
            Some("square") => LIST_SQUARE,
            Some("decimal") => LIST_DECIMAL,
            Some("decimal-leading-zero") => LIST_DECIMAL_ZERO,
            Some("lower-roman") => LIST_LOWER_ROMAN,
            Some("upper-roman") => LIST_UPPER_ROMAN,
            Some("lower-alpha" | "lower-latin") => LIST_LOWER_ALPHA,
            Some("upper-alpha" | "upper-latin") => LIST_UPPER_ALPHA,
            Some("lower-greek") => LIST_LOWER_GREEK,
            Some("cjk-ideographic" | "simp-chinese-informal" | "trad-chinese-informal" | "cjk-decimal") => LIST_CJK,
            _ if ordered => LIST_DECIMAL,
            _ => [LIST_DISC, LIST_CIRCLE, LIST_SQUARE][depth.min(2)],
        };
        let mut items = Vec::new();
        let mut inl = Inline::new();
        for node in el.children() {
            // 文字和图片走行内收集（连续的合成一个匿名文字块，图片单独成块）；别的元素照旧当列表项
            let Some(child) = ElementRef::wrap(node).filter(|e| image_src(e).is_none()) else {
                self.inline_or_block(node, &comp, &comp, &mut inl, &mut items);
                continue;
            };
            self.flush(&mut inl, &comp, &mut items);
            let c = match first_li.take_if(|_| child.value().name() == "li") {
                Some(c) => c,
                None => self.comp(&child, &comp),
            };
            if c.display.as_deref() == Some("none") {
                continue;
            }
            let mut item = self.wrap_children(child, c, Some(NODE_LIST_ITEM));
            if let Some(v) = child.value().attr("value").and_then(|v| v.trim().parse::<i64>().ok()) {
                item.attrs.push((LIST_START, Value::Int(v)));
            }
            items.push(item);
        }
        self.flush(&mut inl, &comp, &mut items);
        let inside = comp.list_inside;
        let mut b = self.boxed(Kind::Container(items), comp, el.value().attr("id"));
        b.ty = Some(NODE_LIST);
        b.attrs.push((LIST_STYLE, Value::Symbol(style)));
        if let Some(n) = el.value().attr("start").and_then(|v| v.trim().parse::<i64>().ok()) {
            b.attrs.push((LIST_START, Value::Int(n)));
        }
        if inside {
            b.attrs.push((LIST_POSITION, Value::Symbol(LIST_INSIDE)));
        }
        b
    }

    /// `<table>` → `$278` 表 → `$151`/`$454`/`$455` 表头/表体/表脚 → `$279` 行 → `$270` 单元格（2026-10-05 《绍宋》样本和测试书对照）。
    /// 跨列、跨行、竖直对齐写在单元格的样式里；`<caption>` 是 `$269` 节点（`$615: $453`）套文字。
    fn table(&self, el: ElementRef, comp: Computed) -> Block {
        let mut parts = Vec::new();
        let mut col_widths: Option<Vec<Len>> = None;
        let spacing = |l: Option<Len>| match l {
            Some(Len::Pt(n)) => n * 0.6,
            Some(Len::Em(n)) => n * 12.0 * comp.font_size,
            _ => 0.9,
        };
        for part in el.children().filter_map(ElementRef::wrap) {
            let pc = self.comp(&part, &comp);
            if pc.display.as_deref() == Some("none") {
                continue;
            }
            match part.value().name() {
                "caption" => {
                    let mut cap = self.wrap_children(part, pc, None);
                    cap.ty = Some(NODE_TEXT);
                    cap.attrs.push((NOTE_CONTENT, Value::Symbol(CAPTION)));
                    cap.extra.push((P_LAYOUT_HINTS, Value::List(vec![Value::Symbol(CAPTION)])));
                    parts.push(cap);
                }
                tag @ ("thead" | "tbody" | "tfoot") => {
                    let ty = match tag {
                        "thead" => NODE_THEAD,
                        "tfoot" => NODE_TFOOT,
                        _ => NODE_TBODY,
                    };
                    let mut rows = Vec::new();
                    for tr in part.children().filter_map(ElementRef::wrap).filter(|e| e.value().name() == "tr") {
                        let rc = self.comp(&tr, &pc);
                        let mut cells = Vec::new();
                        let mut widths = Vec::new();
                        for td in tr.children().filter_map(ElementRef::wrap).filter(|e| matches!(e.value().name(), "td" | "th")) {
                            let mut cc = self.comp(&td, &rc);
                            if cc.width.is_none() {
                                cc.width = td.value().attr("width").and_then(crate::css::parse_len);
                            }
                            // UA 样式：单元格内边距 1px。
                            for p in cc.padding.iter_mut() {
                                p.get_or_insert(Len::Pt(0.75));
                            }
                            widths.push(cc.width);
                            let valign = match cc.valign.as_deref() {
                                Some("top" | "text-top") => VALIGN_TOP,
                                Some("bottom" | "text-bottom") => VALIGN_BOTTOM,
                                _ => ALIGN_CENTER,
                            };
                            let mut cell = self.wrap_children(td, cc, None);
                            cell.extra.push((P_CELL_VALIGN, Value::Symbol(valign)));
                            for (attr, key) in [("colspan", P_COLSPAN), ("rowspan", P_ROWSPAN)] {
                                if let Some(n) = td.value().attr(attr).and_then(|v| v.trim().parse::<i64>().ok()).filter(|n| *n > 1) {
                                    cell.extra.push((key, Value::Int(n)));
                                }
                            }
                            cells.push(cell);
                        }
                        if col_widths.is_none() {
                            col_widths = Some(widths.into_iter().map_while(|w| w).collect());
                        }
                        let mut row = Block::anonymous(Kind::Container(cells), rc.inherited(), Vec::new());
                        row.ty = Some(NODE_ROW);
                        rows.push(row);
                    }
                    let mut p = Block::anonymous(Kind::Container(rows), pc.inherited(), Vec::new());
                    p.ty = Some(ty);
                    parts.push(p);
                }
                _ => {}
            }
        }
        let mut attrs = vec![
            (TABLE_COLLAPSE, Value::Bool(comp.border_collapse)),
            (TABLE_SPACING_H, num(spacing(comp.border_spacing), U_PT)),
            (TABLE_SPACING_V, num(spacing(comp.border_spacing), U_PT)),
        ];
        let cols: Vec<Value> = col_widths.unwrap_or_default().into_iter().filter_map(|w| width_value(w, comp.font_size)).map(|v| Value::Struct(vec![(P_WIDTH, v)])).collect();
        if !cols.is_empty() {
            attrs.push((TABLE_COLUMNS, Value::List(cols)));
        }
        let mut b = self.boxed(Kind::Container(parts), comp, el.value().attr("id"));
        b.ty = Some(NODE_TABLE);
        b.attrs = attrs;
        b
    }

    /// 块级元素的子节点：连续的行内内容合成匿名文字块，块级子元素递归，图片单独成块。
    fn children(&self, el: ElementRef, comp: &Computed, out: &mut Vec<Block>) {
        let mut inl = Inline::new();
        for child in el.children() {
            self.inline_or_block(child, comp, comp, &mut inl, out);
        }
        self.flush(&mut inl, comp, out);
    }

    fn flush(&self, inl: &mut Inline, comp: &Computed, out: &mut Vec<Block>) {
        let reopened = inl.cut_open();
        let taken = inl.take();
        inl.open = reopened;
        if taken.is_empty() {
            // 没有文字：里面的锚点留给后面的内容。只有 `&nbsp;`、全角空格、`<br/>` 的（按 CSS 排出一个空行；只有
            // 普通空白的排不出行，不算）折成下一块的上边距。
            if !taken.text.is_empty() {
                self.gap.set(self.gap.get() + BLANK_LINE_FOLD * comp.line_height.unwrap_or(LH_EM) * comp.font_size);
            }
            self.pending.borrow_mut().extend(taken.ids.into_iter().map(|(i, _)| i));
            return;
        }
        let mut text = taken.text;
        // 末尾的换行、空格去掉（字符数不变的前提下只能删尾部，不影响区间起点）。
        while text.ends_with([' ', '\n']) {
            text.pop();
        }
        let chars = text.chars().count();
        let runs = taken
            .runs
            .into_iter()
            .filter_map(|mut r| {
                r.len = r.len.min(chars.saturating_sub(r.start));
                (r.len > 0).then_some(r)
            })
            .collect();
        let mut ids = taken.ids.into_iter().map(|(i, o)| (i, o.min(chars))).collect();
        prepend_ids(&mut ids, self.take_pending());
        let mut b = Block::anonymous(Kind::Text { text, runs }, comp.inherited(), ids);
        b.inline = true;
        b.gap_before = self.gap.take();
        out.push(b);
    }

    fn inline_or_block(&self, node: NodeRef<Node>, block_comp: &Computed, parent: &Computed, inl: &mut Inline, out: &mut Vec<Block>) {
        match node.value() {
            Node::Text(t) => inl.push_text(t, parent.pre),
            Node::Element(_) => {
                let Some(el) = ElementRef::wrap(node) else { return };
                let name = el.value().name();
                if name == "br" {
                    inl.push_break();
                    return;
                }
                if let Some(src) = image_src(&el) {
                    self.flush(inl, block_comp, out);
                    let comp = self.comp(&el, parent);
                    // 图片自己的 id（`<img id="filepos152">`）也是锚点
                    let mut lead = self.take_pending();
                    lead.extend(el.value().attr("id").map(str::to_string));
                    let ids = lead.into_iter().map(|i| (i, 0)).collect();
                    let mut b = Block::anonymous(Kind::Image { src: resolve_link(self.path, &src).0 }, comp, ids);
                    b.gap_before = self.gap.take();
                    out.push(b);
                    return;
                }
                let comp = self.comp(&el, parent);
                if comp.display.as_deref() == Some("none") || matches!(name, "script" | "style" | "head") {
                    return;
                }
                if is_block_el(&el, &comp) {
                    self.flush(inl, block_comp, out);
                    self.block(el, comp, out);
                    return;
                }
                if let Some(id) = el.value().attr("id") {
                    inl.ids.push((id.to_string(), inl.chars));
                }
                let link = (name == "a")
                    .then(|| el.value().attr("href"))
                    .flatten()
                    .filter(|h| !h.contains("://") && !h.starts_with("mailto:"))
                    .map(|h| {
                        let (path, frag) = resolve_link(self.path, h);
                        (if path.is_empty() { self.path.to_string() } else { path }, frag.unwrap_or_default())
                    });
                let styled = run_differs(&comp, block_comp);
                // 区间先登记成「开着的」：子节点里遇到块或图片切开时，`flush` 在切开处截断它、到新缓冲从 0 接着开
                // （以前事后按进来时的下标插，切开过的话缓冲已经换了：下标越界 panic，或区间错位到别的字上）
                let opened = styled || link.is_some();
                if opened {
                    let run = Run { start: 0, len: 0, comp: styled.then(|| comp.clone()), link, note_ref: false, anchor: name == "a" };
                    inl.open.push((inl.chars, inl.runs.len(), run));
                }
                for c in el.children() {
                    self.inline_or_block(c, block_comp, &comp, inl, out);
                }
                // 子节点进出成对、切开时开着的个数不变，弹出来的就是本元素的
                if let Some((start, run_at, run)) = opened.then(|| inl.open.pop()).flatten() {
                    inl.close_run(start, run_at, run);
                }
            }
            _ => {}
        }
    }
}

impl Block {
    fn anonymous(kind: Kind, comp: Computed, ids: Vec<(String, usize)>) -> Block {
        Block {
            kind,
            comp,
            heading: None,
            margin_top: 0.0,
            margin_bottom: 0.0,
            margin_left: Horiz::default(),
            margin_right: Horiz::default(),
            padding: [0.0; 4],
            padding_h: [Horiz::default(); 2],
            ids,
            note: false,
            ty: None,
            attrs: Vec::new(),
            extra: Vec::new(),
            bare: false,
            inline: false,
            gap_before: 0.0,
        }
    }

    fn is_anonymous(&self) -> bool {
        self.margin_top == 0.0 && self.margin_bottom == 0.0 && self.heading.is_none() && self.margin_left == Horiz::default() && self.ty.is_none()
    }
}

fn image_src(el: &ElementRef) -> Option<String> {
    let v = el.value();
    match v.name() {
        "img" => v.attr("src").map(str::to_string),
        // SVG 的 `<image xlink:href>`：属性在 xlink 命名空间里，`attr()` 只认无命名空间的，按本地名找（《绍宋》封面页
        // `<svg><image xlink:href="../Images/cover.png"/></svg>`；以前找不到、封面页整页丢掉——优化器以前把 SVG 封面换成 `<img>`，
        // 只修复模式不换了才露出来，2026-10-08 真机：封面没了）。
        "image" => v.attrs().find(|(k, _)| *k == "href").map(|(_, val)| val.to_string()),
        _ => None,
    }
}

fn run_differs(a: &Computed, b: &Computed) -> bool {
    a.bold != b.bold
        || a.semibold != b.semibold
        || a.bolder != b.bolder
        || a.italic != b.italic
        || a.color != b.color
        || (a.font_size - b.font_size).abs() > 1e-6
        || a.font_family != b.font_family
        || a.superscript != b.superscript
        || a.subscript != b.subscript
        || a.decoration != b.decoration
        || a.small_caps != b.small_caps
        || a.letter_spacing != b.letter_spacing
        || a.background.is_some()
        || a.has_border()
}

/// 解析好的一个文档：(spine 下标, 路径, 语言, 块)。
type ParsedDoc<'a> = (usize, &'a str, Option<String>, Vec<Block>);

/// 全书的文字块按深度优先编号（含容器里的），`f` 拿到 (文档下标, 编号, 块)。
fn each_text_block(docs: &mut [ParsedDoc], mut f: impl FnMut(usize, usize, &mut Block)) {
    fn walk(blocks: &mut [Block], di: usize, n: &mut usize, f: &mut dyn FnMut(usize, usize, &mut Block)) {
        for b in blocks {
            match &mut b.kind {
                Kind::Container(c) => walk(c, di, n, f),
                Kind::Text { .. } => {
                    f(di, *n, b);
                    *n += 1;
                }
                Kind::Image { .. } => {}
            }
        }
    }
    let mut n = 0;
    for (di, d) in docs.iter_mut().enumerate() {
        walk(&mut d.3, di, &mut n, &mut f);
    }
}

/// 注释配对（同 Amazon 的转换：原书多半只是普通链接，没有 `epub:type`）：正文链接指到后面的一个块，那个块**开头**的链接
/// 指回这个正文链接所在的位置（链接自己或包着它的元素的 id），正文链接就是注释引用（`$616: $617`，Kindle 点了弹窗），目标块是注释正文
/// （`$615: $618`）。优化器 `kindle` 模式保留回链（`note_backlinks`）、把注释搬到引用它的那一章（文件）末尾，正好配得上。
/// 返回配上了几条。
fn mark_notes(docs: &mut [ParsedDoc]) -> usize {
    // 第一遍：每个块的 id、每个链接区间（块编号, 区间下标, 目标, 区间起点处的 id）
    let mut id_block: HashMap<(String, String), usize> = HashMap::new();
    // (块编号, 区间下标, 目标, 区间起点处的 id)
    type Link = (usize, usize, (String, String), Vec<String>);
    let mut links: Vec<Link> = Vec::new();
    let paths: Vec<String> = docs.iter().map(|d| d.1.to_string()).collect();
    each_text_block(docs, |di, n, b| {
        for (id, _) in &b.ids {
            id_block.entry((paths[di].clone(), id.clone())).or_insert(n);
        }
        if let Kind::Text { runs, .. } = &b.kind {
            for (ri, r) in runs.iter().enumerate() {
                if let Some(t) = &r.link {
                    let src: Vec<String> = b.ids.iter().filter(|(_, o)| *o >= r.start && *o <= r.start + r.len).map(|(i, _)| i.clone()).collect();
                    links.push((n, ri, t.clone(), src));
                }
            }
        }
    });
    // 每个块开头（第一个字起）的链接链回到哪里：注释正文的回链在段首（「[1]Queen of Sheba…」）
    let mut back: HashMap<usize, Vec<(String, String)>> = HashMap::new();
    let mut run_start: HashMap<(usize, usize), usize> = HashMap::new();
    each_text_block(docs, |_, n, b| {
        if let Kind::Text { runs, .. } = &b.kind {
            for (ri, r) in runs.iter().enumerate() {
                run_start.insert((n, ri), r.start);
            }
        }
    });
    for (n, ri, t, _) in &links {
        if run_start.get(&(*n, *ri)).is_some_and(|&s| s == 0) {
            back.entry(*n).or_default().push(t.clone());
        }
    }
    let block_path: HashMap<usize, usize> = {
        let mut m = HashMap::new();
        each_text_block(docs, |di, n, _| {
            m.insert(n, di);
        });
        m
    };
    let mut refs: HashSet<(usize, usize)> = HashSet::new();
    let mut notes: HashSet<usize> = HashSet::new();
    for (n, ri, target, src) in &links {
        // 注释在引用后面（优化器把注释搬到引用它的那一章（文件）末尾）；反过来那条是回链。
        let Some(&tb) = id_block.get(target) else { continue };
        if tb <= *n {
            continue;
        }
        let src_path = &paths[block_path[n]];
        let points_back = back.get(&tb).is_some_and(|ts| ts.iter().any(|(p, f)| p == src_path && src.contains(f)));
        if points_back {
            refs.insert((*n, *ri));
            notes.insert(tb);
        }
    }
    each_text_block(docs, |_, n, b| {
        if notes.contains(&n) {
            b.note = true;
        }
        if let Kind::Text { runs, .. } = &mut b.kind {
            for (ri, r) in runs.iter_mut().enumerate() {
                if refs.contains(&(n, ri)) {
                    r.note_ref = true;
                }
            }
        }
    });
    refs.len()
}

/// 正文字体（字数最多的那个）没有嵌入时不写进样式：Kindle 遇到不认识的字体名会换成别的字体，和没写字体的表格、
/// 阅读器设置里选的字体都对不上（《啸风山庄》正文写着没嵌入的「AR MingU30 DemiBold」，2026-10-05 真机）。
/// 正文本来就该用阅读器的字体（用户定的字体规矩，见 typesetting.md）。
/// 写出器 11 起嵌入了的也一样（Send to Kindle：《绍宋》新版嵌入的 NotoSerifSCLight、《平凡的世界》的 FZLanTingSong 都写 `default`，2026-10-08）。
fn body_font_to_drop(docs: &mut [ParsedDoc]) -> Option<String> {
    let mut count: HashMap<Option<String>, usize> = HashMap::new();
    each_text_block(docs, |_, _, b| {
        if let Kind::Text { text, .. } = &b.kind {
            *count.entry(b.comp.font_family.as_ref().map(|f| f.to_lowercase())).or_default() += text.chars().count();
        }
    });
    // 字数打平时取字体名小的（没写字体的排最前），结果和 HashMap 的遍历顺序无关（以前 `max_by_key` 打平时随遍历顺序，同一本书
    // 两次生成的 KFX 可能不一样，覆盖到 Kindle 上进度清零）。打破平局的口径同优化器的 `wash::fonts::pick_body_font`
    // （那个按原始 HTML 数、只给清洗层用，这里按解析好的块数，复用不了）。
    count.into_iter().max_by(|a, b| a.1.cmp(&b.1).then_with(|| b.0.cmp(&a.0)))?.0
}

/// 书里有字体文件的 `@font-face` 字体（小写）。
fn embedded_font_faces(book: &Loaded) -> HashSet<String> {
    let files: HashSet<&str> = book.fonts.iter().map(|(p, _)| p.as_str()).collect();
    book.css.iter().flat_map(|(p, t)| crate::css::font_faces(t, p)).filter(|f| files.contains(f.path.as_str())).map(|f| f.family.to_lowercase()).collect()
}


/// 全书文字块按字数（不算空白）加权的众数：`key` 取块的计算值（按千分之一取整归类）。字数一样多的取小的（结果和遍历顺序无关）。
fn weighted_mode(docs: &[ParsedDoc], key: impl Fn(&Computed) -> f64) -> Option<f64> {
    fn walk(blocks: &[Block], key: &dyn Fn(&Computed) -> f64, count: &mut HashMap<i64, usize>) {
        for b in blocks {
            match &b.kind {
                Kind::Text { text, .. } => {
                    *count.entry((key(&b.comp) * 1000.0).round() as i64).or_default() += text.chars().filter(|c| !c.is_whitespace()).count();
                }
                Kind::Container(c) => walk(c, key, count),
                Kind::Image { .. } => {}
            }
        }
    }
    let mut count = HashMap::new();
    for (_, _, _, blocks) in docs {
        walk(blocks, &key, &mut count);
    }
    count.into_iter().max_by_key(|&(k, n)| (n, std::cmp::Reverse(k))).map(|(k, _)| k as f64 / 1000.0)
}

/// 全书正文的字号（根 em）：所有文字块按字数加权，最多的那个。Send to Kindle 把它当成阅读器字号设置的 1.0，别的字号按比例
/// （《啸风山庄》正文 `font-size:1.167em` 不写字号、1.083em 的写 0.928；《疯探》正文 1.25em，没写字号的容器写 0.8，2026-10-08）。
/// 算出来不在 0.5–3 之间的不信，用 1。
fn base_font_size(docs: &[ParsedDoc]) -> f64 {
    weighted_mode(docs, |c| c.font_size).filter(|v| (0.5..=3.0).contains(v)).unwrap_or(1.0)
}

/// 全书正文的行高（元素字号的倍数）：所有文字块按字数加权，最多的那个行高；没写行高的按 normal（1.2）。
/// Send to Kindle 把它当成阅读器行距设置的 1.0（《绍宋》正文 `line-height:1.5em`）。算出来不在 1–3 之间的不信，用 1.2。
fn base_line_height(docs: &[ParsedDoc]) -> f64 {
    weighted_mode(docs, |c| c.line_height.unwrap_or(LH_EM)).filter(|v| (1.0..=3.0).contains(v)).unwrap_or(LH_EM)
}

/// 有没有块（含容器里的）的左（`left`）或右外边距是负的。
fn has_negative(blocks: &[Block], left: bool) -> bool {
    blocks.iter().any(|b| {
        let h = if left { b.margin_left } else { b.margin_right };
        h.em + h.pct / 100.0 * PAGE_WIDTH_EM < -1e-9 || matches!(&b.kind, Kind::Container(c) if has_negative(c, left))
    })
}

/// 顶层块的左右百分比长度（书里相对 body 内容宽度）折算成相对整页：乘 `s`。
fn scale_percent(blocks: &mut [Block], s: f64) {
    for b in blocks {
        let [p0, p1] = &mut b.padding_h;
        for h in [&mut b.margin_left, &mut b.margin_right, p0, p1] {
            h.pct *= s;
        }
        if let Some(Len::Percent(p)) = &mut b.comp.text_indent {
            *p *= s;
        }
    }
}

/// 相邻块的外边距折叠（同一层）。合成的边距放在后一块的上边距；后一块没有上边距时留在前一块的下边距
/// （Send to Kindle 同样：段距写在每段的上边距，「目录」标题的下边距、《绍宋》章号 `p.j11` 的 `margin-bottom:2px` 留在自己身上，2026-10-08）。
/// 折掉的空段加到下一块的上边距上（外边距折叠之后：多出的高度不参与折叠）。文件末尾的空段没有下一块，不折。
fn apply_gaps(blocks: &mut [Block]) {
    for b in blocks.iter_mut() {
        b.margin_top += b.gap_before;
        b.gap_before = 0.0;
        if let Kind::Container(c) = &mut b.kind {
            apply_gaps(c);
        }
    }
}

fn collapse_siblings(blocks: &mut [Block]) {
    for i in 1..blocks.len() {
        let prev_bottom = blocks[i - 1].margin_bottom;
        if blocks[i].margin_top == 0.0 {
            continue;
        }
        blocks[i].margin_top = collapse(prev_bottom, blocks[i].margin_top);
        blocks[i - 1].margin_bottom = 0.0;
    }
    for b in blocks.iter_mut() {
        if let Kind::Container(c) = &mut b.kind {
            collapse_siblings(c);
        }
    }
}

// ---------------------------------------------------------------- 写 KFX

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
    style_entities: Vec<(String, Vec<(u32, Value)>)>,
    resources: Vec<Res>,
    res_by_path: HashMap<String, usize>,
    images: HashMap<String, (Vec<u8>, &'static str)>,
    warnings: Vec<String>,
    /// 链接、目录用到的锚点：(文件, 锚点) → 锚点名，按出现顺序。
    anchors: Vec<((String, String), u32)>,
    anchor_index: HashMap<(String, String), u32>,
    /// 标题（级别, 节点 id），按书里的顺序。
    headings: Vec<(u8, i64)>,
    /// 样式里用到的字体名（嵌入字体只嵌这些）。
    used_fonts: std::collections::BTreeSet<String>,
    /// 写成 `default` 的字体名（小写）：正文字体（见 [`body_font_to_drop`]）。
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
        // 键：每个属性的编号 + 值的 Ion 编码（Ion 值自带长度，拼起来不会混淆）。
        let mut key = Vec::new();
        for (k, v) in &props {
            key.extend(k.to_le_bytes());
            ion::encode_value(&mut key, v);
        }
        if let Some(&s) = self.styles.get(&key) {
            return s;
        }
        let name = format!("style{}", self.style_entities.len());
        let s = self.sym(&name);
        self.styles.insert(key, s);
        self.style_entities.push((name, props));
        s
    }

    fn resource(&mut self, path: &str) -> Option<usize> {
        if let Some(&i) = self.res_by_path.get(path) {
            return Some(i);
        }
        let (format, mime) = match self.images.get(path).map(|(_, m)| *m) {
            Some("image/jpeg") => (FORMAT_JPG, "image/jpg"),
            Some("image/png") => (FORMAT_PNG, "image/png"),
            Some("image/gif") => (FORMAT_GIF, "image/gif"),
            Some(other) => {
                self.warnings.push(format!("图片格式 {other} 不支持：{path}"));
                return None;
            }
            None => {
                self.warnings.push(format!("图片找不到或格式不支持：{path}"));
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

    /// 块的样式属性。`parent`：KFX 里的父节点（字号写成相对它的倍数，字重、颜色只在需要时写，水平长度按它的宽度换算）。
    /// `col`：本块的文字颜色（[`text_color`]，调用方算好）。
    fn block_props(&mut self, b: &Block, parent: &Parent, doc_lang: &Option<String>, col: Option<u32>) -> Vec<(u32, Value)> {
        let c = &b.comp;
        let fs = c.font_size;
        if b.bare {
            return c.text_align.as_deref().map(|a| vec![(P_TEXT_ALIGN, Value::Symbol(align_symbol(a)))]).unwrap_or_default();
        }
        let mut p = text_props_with(c, parent, col);
        // 标题自己的样式带「标题」提示（Send to Kindle 同样）
        if b.heading.is_some() {
            p.push((P_LAYOUT_HINTS, Value::List(vec![Value::Symbol(HINT_HEADING)])));
        }
        if let Some(l) = c.lang.as_ref().or(doc_lang.as_ref()) {
            p.push((P_LANG, Value::String(l.to_ascii_lowercase())));
        }
        // 容器不写对齐（Send to Kindle 同样，对齐写在里面的文字上）
        if let (Some(a), false) = (c.text_align.as_deref(), matches!(b.kind, Kind::Container(_))) {
            p.push((P_TEXT_ALIGN, Value::Symbol(align_symbol(a))));
        }
        // 首行缩进：包含块是整页宽时照写 em，比整页窄（在有左右边距、内边距、边框的容器里）时写成包含块宽度的百分比
        // （Send to Kindle：正文 `text-indent:2em` 写 2em；《绍宋》信件框里的 2em 写 6.809%＝2/29.375，小注框里 6.78%，2026-10-08）
        let avail = parent.avail;
        // 自己或祖先（含 body，body 的左右边距本身不写）有左右边距、内边距、边框时写百分比（《人生海海》body `margin:0 5pt`、
        // 《揭露人性》`p.kindle-cn-ref{margin:0 1em 0 2em}` 里的 2em 都写 6.25%；没有的照写 em）
        let narrowed = avail < PAGE_WIDTH_EM - 1e-6 || c.in_hbox;
        let indent_rem = match c.text_indent {
            Some(Len::Em(n)) => Some(n * fs),
            Some(Len::Pt(n)) => Some(n / 12.0),
            Some(Len::Percent(n)) => {
                p.push((P_TEXT_INDENT, num(n, U_PERCENT)));
                None
            }
            None => None,
        };
        if let Some(r) = indent_rem {
            p.push((P_TEXT_INDENT, if narrowed { num(r / avail * 100.0, U_PERCENT) } else { num(r / fs, U_EM) }));
        }
        // 行高按全书正文的行高归一（Send to Kindle 的做法，2026-10-08 对照《绍宋》：正文 `line-height:1.5em` 写成 1.0，
        // 行高 1em 的标题写成 0.667，没写行高的容器 0.8）：正文用的就是阅读器的行距设置，别的按比例。以前一律除以 1.2，
        // 正文行距比 Send to Kindle 的大 25%。
        // 下限 0.6（Send to Kindle 同样：《揭露人性》body `line-height:1.618em` 继承到 2.5 倍字号的卷号上只剩 0.4，写 0.6；
        // 《疯探》《克莱因壶》《占星术》的小字、目录同样落在 0.6）
        let lh = (c.line_height.unwrap_or(LH_EM) / self.base_lh).max(0.6);
        p.push((P_LINE_HEIGHT, num(lh, U_LH)));
        if c.nowrap {
            p.push((P_NOWRAP, Value::Bool(true)));
        }
        if c.break_all {
            p.push((P_WORD_BREAK, Value::Symbol(WORD_BREAK_ALL)));
        }
        match c.min_height.filter(|_| !matches!(b.kind, Kind::Image { .. })) {
            Some(Len::Em(n)) => p.push((P_MIN_HEIGHT, num(n, U_EM))),
            Some(Len::Pt(n)) => p.push((P_MIN_HEIGHT, num(n / 12.0 / fs, U_EM))),
            _ => {}
        }
        // `lh` 是本元素行高的倍数（Send to Kindle：没写行高的容器行高 0.8，同样 7% 的上边距写成 2.333 而不是 1.867；
        // 0.5em 的内边距在行高 0.667 的标题上写成 0.625）。以前按固定 1.2em 换算，行高不是 1 的块边距都偏了。
        let vert = |v: Vert| num(v / fs / (LH_EM * lh), U_LH);
        if b.margin_top != 0.0 {
            p.push((P_MARGIN_TOP, vert(b.margin_top)));
        }
        if b.margin_bottom != 0.0 {
            p.push((P_MARGIN_BOTTOM, vert(b.margin_bottom)));
        }
        // 左右外边距、内边距一律写成包含块宽度的百分比（Send to Kindle 同样：body 的 5pt → 1.302%、`list-style:none` 的 1.5em → 4.688%、
        // 《绍宋》`p.ganyan1` 的 0.5em → 1.484%、表格单元格的 0.2em → 0.375%；包含块按整页 32em 减去外层容器的左右边距、内边距、边框）
        let horiz = |h: Horiz| num(if h.em == 0.0 { h.pct } else { (h.em + h.pct / 100.0 * avail) / avail * 100.0 }, U_PERCENT);
        if b.margin_left != Horiz::default() {
            p.push((P_MARGIN_LEFT, horiz(b.margin_left)));
        }
        if b.margin_right != Horiz::default() {
            p.push((P_MARGIN_RIGHT, horiz(b.margin_right)));
        }
        for (i, k) in [P_PADDING_TOP, P_PADDING_RIGHT, P_PADDING_BOTTOM, P_PADDING_LEFT].into_iter().enumerate() {
            if i % 2 == 0 {
                if b.padding[i] != 0.0 {
                    p.push((k, vert(b.padding[i])));
                }
            } else if b.padding_h[i / 2] != Horiz::default() {
                p.push((k, horiz(b.padding_h[i / 2])));
            }
        }
        if let Some(bg) = c.background {
            p.push((P_BACKGROUND, Value::Int(i64::from(bg))));
        }
        p.extend(border_props(c));
        // 图片只认百分比宽度（写出器 9）：优化器给带图注的竖长图写的 `width:P%`（`bookconv::capfit`）、书里样式表的 `width:40%`；
        // 别的单位（em、px）没对过样本，不写
        let image_pct = matches!(b.kind, Kind::Image { .. }) && matches!(c.width, Some(Len::Percent(_)));
        if matches!(b.ty, Some(NODE_TABLE | NODE_HR)) || image_pct {
            if let Some(v) = c.width.and_then(|w| width_value(w, fs)) {
                p.push((P_WIDTH, v));
            }
        } else if b.ty.is_none() && !matches!(b.kind, Kind::Image { .. }) {
            // 有宽度的块（Send to Kindle 同样：宽度照写、带 `$546: $377`，em 宽度另加最大宽度 100%，左右外边距 auto 的按它对齐：
            // 《阿加莎》`h2{width:3em;margin:2.5em auto 1.8em -2em}` 靠左的小色块、《金庸》60% 宽的图注框）
            if let Some(v) = c.width.and_then(|w| width_value(w, fs)) {
                p.push((P_WIDTH, v));
                p.push((P_SIZING, Value::Symbol(SIZING_VALUE)));
                if !matches!(c.width, Some(Len::Percent(_))) {
                    p.push((P_MAX_WIDTH, num(100.0, U_PERCENT)));
                }
                let align = match (c.margin_auto[3], c.margin_auto[1]) {
                    (true, true) => Some(ALIGN_CENTER),
                    (false, true) => Some(ALIGN_LEFT),
                    (true, false) => Some(ALIGN_RIGHT),
                    _ => None,
                };
                if let Some(a) = align {
                    p.push((P_BOX_ALIGN, Value::Symbol(a)));
                }
            }
        }
        for (s, k) in [(c.box_shadow, P_BOX_SHADOW), (c.text_shadow, P_TEXT_SHADOW)] {
            if let Some(s) = s {
                p.push((k, shadow_value(&s, c.color)));
            }
        }
        p.extend(b.extra.iter().cloned());
        p
    }

    /// 生成一个块的节点（递归），同时登记位置、id。
    fn node(&mut self, b: &Block, parent: &Parent, ctx: &mut SectionCtx) -> Option<Value> {
        let col = text_color(&b.comp);
        let props = self.block_props(b, parent, &ctx.lang, col);
        // 本节点给子节点（行内区间、容器里的块）的「父节点」
        let me = Parent { weight: weight_of(&b.comp), color: shown_color(col, &b.comp, parent), avail: inner_width(b, parent.avail), base_fs: parent.base_fs };
        match &b.kind {
            Kind::Text { text, runs } => {
                let eid = self.eid();
                let chars = text.chars().count();
                for (id, off) in &b.ids {
                    ctx.ids.push((id.clone(), eid, *off));
                }
                ctx.order.push((eid, chars));
                let idx = ctx.texts.len();
                ctx.texts.push(Value::String(text.clone()));
                let style = self.style(props);
                let mut f = vec![(EID, Value::Int(eid)), (STYLE_REF, Value::Symbol(style)), (NODE_TYPE, Value::Symbol(b.ty.unwrap_or(NODE_TEXT)))];
                f.extend(b.attrs.iter().cloned());
                if let Some(h) = b.heading {
                    f.push((HEADING_LEVEL, Value::Int(i64::from(h))));
                }
                if b.note {
                    f.push((NOTE_CONTENT, Value::Symbol(NOTE_CONTENT_FOOTNOTE)));
                }
                if let Some(h) = b.heading {
                    self.headings.push((h, eid));
                }
                let run_values: Vec<Value> = runs
                    .iter()
                    .map(|r| {
                        let mut f = vec![(OFFSET, Value::Int(r.start as i64)), (LENGTH, Value::Int(r.len as i64))];
                        if r.note_ref {
                            f.push((NOTE_REF, Value::Symbol(NOTE_REF_POPUP)));
                        }
                        if let Some(key) = &r.link {
                            f.push((LINK_TO, Value::Symbol(self.anchor(key.clone()))));
                        }
                        if let Some(rc) = &r.comp {
                            let mut rp = text_props(rc, &me);
                            if rc.superscript && !b.comp.superscript {
                                rp.push((P_VERTICAL_ALIGN, Value::Symbol(VALIGN_SUPER)));
                            } else if rc.subscript && !b.comp.subscript {
                                rp.push((P_VERTICAL_ALIGN, Value::Symbol(VALIGN_SUB)));
                            }
                            rp.extend(border_props(rc));
                            // `<a>` 的颜色写成链接（未访问、已访问）的颜色（Send to Kindle 同样：《人生海海》目录 `color:#00C`）
                            if r.anchor {
                                if let Some(i) = rp.iter().position(|(k, _)| *k == P_COLOR) {
                                    let (_, col) = rp.remove(i);
                                    for k in [P_LINK_UNVISITED, P_LINK_VISITED] {
                                        rp.push((k, Value::Struct(vec![(P_COLOR, col.clone())])));
                                    }
                                }
                            }
                            if let Some(bg) = rc.background {
                                rp.push((P_INLINE_BACKGROUND, Value::Int(i64::from(bg))));
                            }
                            f.push((STYLE_REF, Value::Symbol(self.style(rp))));
                        }
                        Value::Struct(f)
                    })
                    .collect();
                if !run_values.is_empty() {
                    f.push((RUNS, Value::List(run_values)));
                }
                f.push((TEXT_REF, Value::Struct(vec![(NAME, Value::Symbol(ctx.pool)), (TEXT_INDEX, Value::Int(idx as i64))])));
                Some(Value::Struct(f))
            }
            Kind::Image { src } => {
                let r = self.resource(src)?;
                let eid = self.eid();
                for (id, _) in &b.ids {
                    ctx.ids.push((id.clone(), eid, 0));
                }
                ctx.order.push((eid, 1));
                ctx.resources.push(r);
                let style = self.style(props);
                let res = self.sym(&self.resources[r].name.clone());
                Some(Value::Struct(vec![
                    (EID, Value::Int(eid)),
                    (STYLE_REF, Value::Symbol(style)),
                    (NODE_TYPE, Value::Symbol(NODE_IMAGE)),
                    (RESOURCE_REF, Value::Symbol(res)),
                ]))
            }
            Kind::Container(children) => {
                let eid = self.eid();
                for (id, _) in &b.ids {
                    ctx.ids.push((id.clone(), eid, 0));
                }
                ctx.order.push((eid, 1));
                if let Some(h) = b.heading {
                    self.headings.push((h, eid));
                }
                let ty = b.ty.unwrap_or(NODE_CONTAINER);
                let mut props = props;
                // 背景图：样式里指向图片资源（`$479`），同《绍宋》样本 `body.juan` 等
                if let Some(r) = b.comp.bg_image.as_deref().and_then(|src| self.resource(src)) {
                    ctx.resources.push(r);
                    props.push((P_BG_IMAGE, Value::Symbol(self.sym(&self.resources[r].name.clone()))));
                    if b.comp.bg_no_repeat {
                        props.push((P_BG_REPEAT, Value::Symbol(BG_NO_REPEAT)));
                    }
                    if b.comp.bg_fixed {
                        props.push((P_BG_ATTACHMENT, Value::Symbol(BG_FIXED)));
                    }
                    let len = |l: Len| match l {
                        Len::Percent(p) => num(p, U_PERCENT),
                        Len::Em(n) => num(n, U_EM),
                        Len::Pt(n) => num(n, U_PT),
                    };
                    for (v, k) in b.comp.bg_position.iter().zip([P_BG_POS_X, P_BG_POS_Y]).chain(b.comp.bg_size.iter().zip([P_BG_SIZE_W, P_BG_SIZE_H])) {
                        if let Some(l) = v {
                            props.push((k, len(*l)));
                        }
                    }
                }
                // 表体、行这些结构层没有样式（同样本）。
                let styled = !matches!(ty, NODE_THEAD | NODE_TBODY | NODE_TFOOT | NODE_ROW);
                let style = styled.then(|| self.style(props));
                let kids: Vec<Value> = children.iter().filter_map(|c| self.node(c, &me, ctx)).collect();
                let mut f = vec![(EID, Value::Int(eid))];
                if ty == NODE_CONTAINER {
                    f.push((TMPL_FIT, Value::Symbol(CONTAINER_LAYOUT)));
                }
                if let Some(style) = style {
                    f.push((STYLE_REF, Value::Symbol(style)));
                }
                f.extend(b.attrs.iter().cloned());
                if let Some(h) = b.heading {
                    f.push((HEADING_LEVEL, Value::Int(i64::from(h))));
                }
                f.push((NODE_TYPE, Value::Symbol(ty)));
                if !kids.is_empty() {
                    f.push((CHILDREN, Value::List(kids)));
                }
                Some(Value::Struct(f))
            }
        }
    }
}

/// 阴影：`{颜色, x, y, 模糊}`，长度 pt 或 em（Send to Kindle：`box-shadow:0 0 0.5em #aaa` → `$501: 0.5em`，`text-shadow` 的 3px → 1.35pt）。
fn shadow_value(s: &crate::css::Shadow, text: Option<u32>) -> Value {
    let len = |l: Len| match l {
        Len::Em(n) if n != 0.0 => num(n, U_EM),
        Len::Em(_) => num(0.0, U_PT),
        Len::Pt(n) => num(n, U_PT),
        Len::Percent(n) => num(n, U_PERCENT),
    };
    let col = s.color.or(text).unwrap_or(0xFF00_0000);
    Value::Struct(vec![(SHADOW_COLOR, Value::Int(i64::from(col))), (SHADOW_X, len(s.x)), (SHADOW_Y, len(s.y)), (SHADOW_BLUR, len(s.blur))])
}

/// 宽度：百分比照写，em/pt 换成 em。
fn width_value(l: Len, fs: f64) -> Option<Value> {
    match l {
        Len::Percent(p) => Some(num(p, U_PERCENT)),
        Len::Em(n) if n > 0.0 => Some(num(n, U_EM)),
        Len::Pt(n) if n > 0.0 => Some(num(n / 12.0 / fs, U_EM)),
        _ => None,
    }
}

/// 边框、圆角（块和行内区间共用）。四边样式、宽度、颜色都一样时写「全部」那一组，否则按边写（上、左、下、右）。
fn border_props(c: &Computed) -> Vec<(u32, Value)> {
    let mut p = Vec::new();
    let bw = |w: BorderWidth| match w {
        BorderWidth::Pt(n) => num(n, U_PT),
        BorderWidth::Em(n) => num(n, U_EM),
    };
    let style = |b: &Border| match b.style.as_str() {
        "dashed" => BORDER_DASHED,
        "dotted" => BORDER_DOTTED,
        "double" => BORDER_DOUBLE,
        "groove" => BORDER_GROOVE,
        "ridge" => BORDER_RIDGE,
        "inset" => BORDER_INSET,
        "outset" => BORDER_OUTSET,
        _ => BORDER_SOLID,
    };
    let zero = |b: &Border| matches!(b.width, BorderWidth::Pt(n) | BorderWidth::Em(n) if n == 0.0);
    let sides: Vec<Option<&Border>> = c.border.iter().map(|b| b.as_ref().filter(|b| !zero(b))).collect();
    let mut emit = |slot: usize, b: &Border| {
        p.push((P_BORDER_STYLE[slot], Value::Symbol(style(b))));
        p.push((P_BORDER_WIDTH[slot], bw(b.width)));
        // 黑色不写（Send to Kindle 同样；文字也是黑色或缺省时边框本来就画成黑色）
        let black_text = c.color.is_none_or(|t| t & 0xFF_FFFF == 0);
        if let Some(col) = b.color.filter(|&col| !(col == 0xFF00_0000 && black_text)) {
            // 全透明的写「透明」（Send to Kindle 同样写 `$349`）
            p.push((P_BORDER_COLOR[slot], if col >> 24 == 0 { Value::Symbol(COLOR_TRANSPARENT) } else { Value::Int(i64::from(col)) }));
        }
    };
    if let (Some(first), true) = (sides[0], sides.iter().all(|s| *s == sides[0])) {
        emit(0, first);
    } else {
        // CSS 顺序（上、右、下、左）→ KFX 顺序（上、左、下、右）的下标
        for (css_i, slot) in [(0, 1), (3, 2), (2, 3), (1, 4)] {
            if let Some(b) = sides[css_i] {
                emit(slot, b);
            }
        }
    }
    if c.radius.iter().any(Option::is_some) {
        // 左上、右上、右下、左下 → `$459`、`$460`、`$462`、`$461`
        for (i, slot) in [(0, 0), (1, 1), (2, 3), (3, 2)] {
            let v = match c.radius[i] {
                Some(Len::Em(n)) => num(n, U_EM),
                Some(Len::Pt(n)) => num(n * 0.6, U_PT),
                Some(Len::Percent(n)) => num(n, U_PERCENT),
                None => num(0.0, U_PT),
            };
            p.push((P_BORDER_RADIUS[slot], v));
        }
    }
    p
}

/// 段落的计算值换上行内元素的文字样式（字体、字号、字重、斜体、颜色、行高、装饰、字间距）；盒子（边距、背景、边框）照段落的。
fn merge_text_style(block: &Computed, run: &Computed) -> Computed {
    Computed {
        font_family: run.font_family.clone(),
        font_fallbacks: run.font_fallbacks.clone(),
        font_size: run.font_size,
        bold: run.bold,
        semibold: run.semibold,
        bolder: run.bolder,
        weight_declared: run.weight_declared,
        italic: run.italic,
        line_height: run.line_height,
        line_height_abs: run.line_height_abs,
        color: run.color,
        decoration: run.decoration,
        small_caps: run.small_caps,
        letter_spacing: run.letter_spacing,
        ..block.clone()
    }
}

fn align_symbol(a: &str) -> u32 {
    match a {
        "center" => ALIGN_CENTER,
        "right" | "end" => ALIGN_RIGHT,
        "justify" => ALIGN_JUSTIFY,
        _ => ALIGN_LEFT,
    }
}

/// 标出容器里只有本元素文字的匿名文字块（见 [`Block::bare`]）。
fn mark_bare(children: &mut [Block], comp: &Computed) {
    let inherited = comp.inherited();
    for c in children {
        // 行内区间的样式相对段落写，段落样式空着照样成立
        if matches!(c.kind, Kind::Text { .. }) && c.is_anonymous() && c.inline && c.comp == inherited {
            c.bare = true;
        }
    }
}

/// `text` 的计算值换上 `boxc` 的盒子属性（外边距、内边距、背景、边框、宽高、显示方式、单元格竖直对齐）。
fn with_box(text: &Computed, boxc: &Computed) -> Computed {
    Computed {
        display: boxc.display.clone(),
        margin: boxc.margin,
        padding: boxc.padding,
        background: boxc.background,
        border: boxc.border.clone(),
        radius: boxc.radius,
        width: boxc.width,
        min_height: boxc.min_height,
        valign: boxc.valign.clone(),
        bg_image: boxc.bg_image.clone(),
        bg_no_repeat: boxc.bg_no_repeat,
        bg_fixed: boxc.bg_fixed,
        bg_position: boxc.bg_position,
        bg_size: boxc.bg_size,
        bg_cover: boxc.bg_cover,
        ..text.clone()
    }
}

/// 显示的字重：`<b>`/`<strong>` 的 bolder、600 半粗、粗体、正常。
fn weight_of(c: &Computed) -> u32 {
    if c.bolder {
        WEIGHT_BOLDER
    } else if c.semibold {
        WEIGHT_SEMIBOLD
    } else if c.bold {
        WEIGHT_BOLD
    } else {
        WEIGHT_NORMAL
    }
}

/// 近黑：三个分量都不超过 0x11（样本里见过的是纯黑和 `#111111`）。
fn near_black(col: u32) -> bool {
    col >> 24 == 0xFF && (col >> 16 & 0xFF) <= 0x11 && (col >> 8 & 0xFF) <= 0x11 && (col & 0xFF) <= 0x11
}

/// 这个节点实际显示的文字颜色（写进样式的，`None`＝阅读器缺省）。Send to Kindle 在没有背景的地方省掉近黑的文字颜色
/// （《绍宋》正文的黑、章号的 `#111111` 都不写；同样的颜色在有背景色的小注框、卷首语里照写，2026-10-08），
/// 父节点写了别的颜色时照写，免得继承父节点的颜色。
/// `col` 是 [`text_color`] 算好的（别再算一遍）。
fn shown_color(col: Option<u32>, c: &Computed, parent: &Parent) -> Option<u32> {
    match col {
        Some(col) if near_black(col) && !c.on_background && c.background.is_none() && c.bg_image.is_none() && parent.color.is_none() => None,
        Some(col) => Some(col),
        None => parent.color,
    }
}

/// 按对比度调过的文字颜色（[`crate::css::ensure_contrast`]，背景是自己或祖先的背景色，没有按白页面）；没写颜色、缺省的黑字
/// 和背景对比度不够时也给一个颜色（深蓝底上的黑字写 `#e4e4e4`）。
fn text_color(c: &Computed) -> Option<u32> {
    // 底下只有背景图、没有背景色：看不出对比度，照写
    if c.backdrop.is_none() && (c.on_image || c.bg_image.is_some()) {
        return c.color;
    }
    let bg = c.backdrop.unwrap_or(0xFFFF_FFFF);
    match c.color {
        Some(col) => Some(crate::css::ensure_contrast(col, bg)),
        None => (crate::css::contrast(0xFF00_0000, bg) < 4.5).then(|| crate::css::ensure_contrast(0xFF00_0000, bg)),
    }
}

/// 容器给子节点的包含块宽度（根 em）：减去自己的左右外边距、内边距、边框（百分比按外面的包含块算）。
/// 表格按表格宽度算，不减边框（Send to Kindle：《绍宋》表格 `width:100%` 带 0.5px 边框，单元格的 0.2em 内边距写 0.375%＝0.12/32）。
fn inner_width(b: &Block, avail: f64) -> f64 {
    let rem = |h: Horiz| h.em + h.pct / 100.0 * avail;
    if b.ty == Some(NODE_TABLE) {
        let w = match b.comp.width {
            Some(Len::Percent(p)) => p / 100.0 * avail,
            Some(Len::Em(n)) => n * b.comp.font_size,
            Some(Len::Pt(n)) => n / 12.0,
            None => avail - rem(b.margin_left) - rem(b.margin_right),
        };
        return if w.is_finite() && w > 1.0 { w } else { avail };
    }
    let border = |s: usize| match b.comp.border[s].as_ref().map(|x| x.width) {
        Some(BorderWidth::Pt(n)) => n / 12.0,
        Some(BorderWidth::Em(n)) => n * b.comp.font_size,
        None => 0.0,
    };
    let w = avail - rem(b.margin_left) - rem(b.margin_right) - rem(b.padding_h[0]) - rem(b.padding_h[1]) - border(1) - border(3);
    if w.is_finite() && w > 1.0 { w } else { avail }
}

/// 字体、字号、粗体、斜体、颜色（块和行内区间共用）。字重、颜色只在和父节点显示的不一样时写（Send to Kindle 不写 normal 字重、
/// 不写没有背景处的近黑颜色，见 [`shown_color`]）。
fn text_props(c: &Computed, parent: &Parent) -> Vec<(u32, Value)> {
    text_props_with(c, parent, text_color(c))
}

/// 同 [`text_props`]，文字颜色 `col`（[`text_color`]）由调用方算好。
fn text_props_with(c: &Computed, parent: &Parent, col: Option<u32>) -> Vec<(u32, Value)> {
    let mut p = Vec::new();
    if let Some(f) = &c.font_family {
        // 整串备选照写（Send to Kindle 同样，小写、逗号连接）；第一个在 `Builder::style` 里换成 `default` 或嵌入字体名
        let v = match &c.font_fallbacks {
            Some(rest) => format!("{f},{rest}"),
            None => f.clone(),
        };
        p.push((P_FONT_FAMILY, Value::String(v)));
    }
    // 字号是全书正文字号的倍数，相对根而不是父节点（Send to Kindle：《疯探》0.8 的容器里 1.2em 的作者行写 0.96＝1.2/1.25）
    let rel = if parent.base_fs > 0.0 { c.font_size / parent.base_fs } else { c.font_size };
    p.push((P_FONT_SIZE, num(rel, U_FONT_EM)));
    let w = weight_of(c);
    if w != WEIGHT_NORMAL || parent.weight != WEIGHT_NORMAL || c.weight_declared {
        p.push((P_FONT_WEIGHT, Value::Symbol(w)));
    }
    if c.italic {
        p.push((P_FONT_STYLE, Value::Symbol(STYLE_ITALIC)));
    }
    if let (Some(col), Some(_)) = (col, shown_color(col, c, parent)) {
        p.push((P_COLOR, Value::Int(i64::from(col))));
    }
    for (on, k) in c.decoration.iter().zip([P_UNDERLINE, P_LINE_THROUGH, P_OVERLINE]) {
        if *on {
            p.push((k, Value::Symbol(BORDER_SOLID)));
        }
    }
    if c.small_caps {
        p.push((P_FONT_VARIANT, Value::Symbol(SMALL_CAPS)));
    }
    if let Some(ls) = c.letter_spacing {
        p.push((P_LETTER_SPACING, num(ls, U_EM)));
    }
    p
}

struct SectionCtx {
    pool: u32,
    texts: Vec<Value>,
    /// 节点顺序与各自占的位置数。
    order: Vec<(i64, usize)>,
    ids: Vec<(String, i64, usize)>,
    resources: Vec<usize>,
    lang: Option<String>,
}

struct SectionOut {
    name: u32,
    length: usize,
    eids: Vec<i64>,
    pid_map: Vec<Value>,
    resources: Vec<usize>,
    first_eid: i64,
}

impl Builder {
    fn new(images: HashMap<String, (Vec<u8>, &'static str)>, warnings: Vec<String>, opts: &Opts) -> Builder {
        Builder {
            locals: Vec::new(),
            local_index: HashMap::new(),
            next_eid: 1,
            styles: HashMap::new(),
            style_entities: Vec::new(),
            resources: Vec::new(),
            res_by_path: HashMap::new(),
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

/// 一个文字块一行（见 [`epub_text_styles`]）。
/// 容器里只有本元素文字的块（[`Block::bare`]）用容器的边距（和 KFX 那边一样：边距在容器上）。
fn dump_blocks(blocks: &[Block], out: &mut String) {
    dump_blocks_in(blocks, None, out)
}

fn dump_blocks_in(blocks: &[Block], parent: Option<&Block>, out: &mut String) {
    use std::fmt::Write;
    for b in blocks {
        match &b.kind {
            Kind::Container(c) => dump_blocks_in(c, Some(b), out),
            Kind::Text { text, .. } => {
                let c = &b.comp;
                let key: String = text.chars().filter(|ch| !ch.is_whitespace()).take(60).collect();
                if key.is_empty() {
                    continue;
                }
                let lh = c.line_height.unwrap_or(LH_EM) * c.font_size;
                let weight = if c.bolder { "bolder" } else if c.semibold { "semibold" } else if c.bold { "bold" } else { "normal" };
                let indent = match c.text_indent {
                    Some(Len::Em(n)) => format!("em:{n}"),
                    Some(Len::Pt(n)) => format!("pt:{n}"),
                    Some(Len::Percent(n)) => format!("%:{n}"),
                    None => "-".into(),
                };
                let m = match parent {
                    Some(p) if b.bare => p,
                    _ => b,
                };
                let _ = writeln!(
                    out,
                    "{key}\t{}\t{lh}\t{}\t{weight}\t{}\t{}\t{}\t{indent}\t{}\t{}\t{}+{}\t{}+{}",
                    c.font_size,
                    c.font_family.as_deref().unwrap_or("-"),
                    c.italic,
                    c.color.map_or("-".into(), |x| format!("{x:08x}")),
                    c.text_align.as_deref().unwrap_or("-"),
                    m.margin_top,
                    m.margin_bottom,
                    m.margin_left.em,
                    m.margin_left.pct,
                    m.margin_right.em,
                    m.margin_right.pct,
                );
            }
            Kind::Image { .. } => {}
        }
    }
}

/// 32 进制大写（容器 id、book_id 用）。
/// 唯一 ID → 容器 id（`CR!` + 28 个字符）。书里存 4 处（见 `Container::set_container_id`），都写这一个值。
pub fn container_id(id: u64) -> String {
    format!("CR!{}", base32(id, 28))
}

fn base32(mut n: u64, len: usize) -> String {
    const A: &[u8] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";
    let mut s = Vec::with_capacity(len);
    for _ in 0..len {
        s.push(A[(n % 32) as usize]);
        n = (n / 32) ^ n.rotate_left(13);
    }
    String::from_utf8(s).unwrap_or_default()
}

/// 实体头：样本里都是 `{$410: 0, $411: 0}`。
fn entity_header() -> Vec<Item> {
    vec![Item::Bvm, Item::Value(Value::Struct(vec![(410, Value::Int(0)), (411, Value::Int(0))]))]
}

fn ent(id: u32, ty: u32, v: Value) -> Entity {
    Entity { id, ty, version: 1, header: entity_header(), body: Body::Ion(vec![Item::Bvm, Item::Value(v)]) }
}

/// `$264` 的写法：升序 id，连续的写成 `[起点, 个数]`。
fn compress_ids(mut ids: Vec<i64>) -> Value {
    ids.sort_unstable();
    ids.dedup();
    let mut out = Vec::new();
    let mut i = 0;
    while i < ids.len() {
        let mut j = i;
        while j + 1 < ids.len() && ids[j + 1] == ids[j] + 1 {
            j += 1;
        }
        if j > i {
            out.push(Value::List(vec![Value::Int(ids[i]), Value::Int((j - i + 1) as i64)]));
        } else {
            out.push(Value::Int(ids[i]));
        }
        i = j + 1;
    }
    Value::List(out)
}

/// `$609` 的写法：`[前进, 新 id]`，id 正好加一时只写前进量，最后 `[前进, 0]`。
fn pid_map(order: &[(i64, usize)]) -> Vec<Value> {
    let mut out = Vec::new();
    let mut prev: Option<(i64, usize)> = None;
    for &(eid, len) in order {
        let adv = prev.map_or(0, |p| p.1) as i64;
        match prev {
            Some((pe, _)) if eid == pe + 1 => out.push(Value::Int(adv)),
            _ => out.push(Value::List(vec![Value::Int(adv), Value::Int(eid)])),
        }
        prev = Some((eid, len));
    }
    if let Some((_, len)) = prev {
        out.push(Value::List(vec![Value::Int(len as i64), Value::Int(0)]));
    }
    out
}

/// EPUB → KFX。返回 KFX 字节和警告。
pub fn epub_to_kfx(epub: &[u8], opts: &Opts) -> Result<(Vec<u8>, Vec<String>), String> {
    epub_to_kfx_from(std::io::Cursor::new(epub), opts)
}

/// 同 [`epub_to_kfx`]，从可定位的读取器（如打开的文件）读 EPUB：不用先把整本读进内存，峰值少一份压缩包大小。
pub fn epub_to_kfx_from<R: std::io::Read + std::io::Seek>(epub: R, opts: &Opts) -> Result<(Vec<u8>, Vec<String>), String> {
    let mut warnings = Vec::new();
    let mut book = epubbook::load_from(epub, &mut warnings)?;
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
    let out = build(&mut book, &mut b, id)?;
    Ok((out, b.warnings))
}

/// 整本书写成 KFX：解析文档 → 全书分析 → 版面 → 样式 → 位置映射 → 目录、锚点、导航 → 元数据 → 资源 → 清单与容器。
/// 各阶段按这个顺序分配本地符号，换顺序会改产物字节（资源的符号必须最后分配，见 [`resource_entities`]）。
fn build(book: &mut Loaded, b: &mut Builder, id: u64) -> Result<Vec<u8>, String> {
    let mut parsed = parse_docs(book, b.media, false);
    analyze(book, b, &mut parsed);
    let fixed_canvas = fixed_canvas(book);
    // 翻页方向：`$557` 从左往右、`$559` 从右往左（2026-10-05 测试漫画 LTR/RTL 两本只差这一处）。
    let direction = if book.meta.rtl { DIR_RTL } else { DIR_LTR };
    let Layout { mut sections, mut entities, id_map, cover_tmpl } = lay_out(book, b, parsed, fixed_canvas, direction)?;
    // 封面图即使没出现在正文里也要带上（书架缩略图）。
    let cover_res = book.cover.as_deref().and_then(|c| b.resource(c));
    style_entities(b, &mut entities);
    let fonts = take_fonts(book, b);
    position_entities(&mut sections, &mut entities);
    navigation_entities(book, b, &sections, &id_map, cover_tmpl, &mut entities);
    metadata_entities(book, b, id, &sections, fixed_canvas, direction, cover_res, &mut entities);
    resource_entities(b, fonts, cover_res, &mut entities);
    Ok(finish(b, &sections, entities, cover_res, id))
}

/// 固定版式（漫画，优化器 `comicfxl` 写的 `fixed-layout`/`original-resolution`）：画布宽高（取不到写 0）。见 docs/kfx.md#固定版式。
fn fixed_canvas(book: &Loaded) -> Option<(i64, i64)> {
    (!book.meta.fixed_layout.is_empty()).then(|| {
        book.meta
            .fixed_layout
            .iter()
            .find(|(n, _)| *n == 126)
            .and_then(|(_, v)| v.split_once('x'))
            .and_then(|(w, h)| Some((w.trim().parse().ok()?, h.trim().parse().ok()?)))
            .unwrap_or((0, 0))
    })
}

/// 书的语言（OPF 里的；空的算没写）。
fn book_language(book: &Loaded) -> Option<String> {
    (!book.meta.language.is_empty()).then(|| book.meta.language.clone())
}

/// 先把所有文档解析成块（注释配对要看全书），再逐个生成版面。各文档独立，多线程解析（`bookconv::util::par_map`，按原顺序收回）。
/// 外部样式表按路径只解析一次（大合集几百个文档共用一份样式表），各线程共用。`faithful`：按 CSS 原样算（[`epub_text_styles`]），
/// body 的左右边距照算。
fn parse_docs(book: &Loaded, media: Option<crate::css::MediaEnv>, faithful: bool) -> Vec<ParsedDoc<'_>> {
    let css: HashMap<&str, &str> = book.css.iter().map(|(p, c)| (p.as_str(), c.as_str())).collect();
    let book_lang = book_language(book);
    let sheets: std::sync::Mutex<HashMap<String, std::sync::Arc<crate::css::Rules>>> = Default::default();
    let docs: Vec<(usize, &bookconv::epubbook::Doc)> = book.docs.iter().enumerate().collect();
    let parse_doc = |&(si, doc): &(usize, &bookconv::epubbook::Doc)| -> (usize, Option<String>, Vec<Block>) {
        let html = Html::parse_document(&doc.html);
        let mut sheet = Sheet::default();
        let mut order = 0;
        for el in html.select(&scraper::Selector::parse("link, style").unwrap_or_else(|_| unreachable!())) {
            // `<link>`/`<style>` 的 `media` 属性和 `@media` 同一口径
            if el.value().attr("media").is_some_and(|m| !crate::css::media_ok(m, media.as_ref())) {
                continue;
            }
            if el.value().name() == "style" {
                order = sheet.add_at(&el.text().collect::<String>(), order, &doc.path, media.as_ref());
            } else if el.value().attr("rel").is_some_and(|r| r.to_ascii_lowercase().contains("stylesheet")) {
                if let Some(h) = el.value().attr("href") {
                    let path = resolve_link(&doc.path, h).0;
                    if let Some(c) = css.get(path.as_str()) {
                        let cached = sheets.lock().unwrap_or_else(|e| e.into_inner()).get(&path).cloned();
                        // 没解析过的在锁外解析（几个线程同时碰上同一份时各解析一次，结果一样，留先放进去的那份）
                        let rules = cached.unwrap_or_else(|| {
                            let r = crate::css::Rules::parse(c, &path, media.as_ref());
                            sheets.lock().unwrap_or_else(|e| e.into_inner()).entry(path).or_insert(r).clone()
                        });
                        order = sheet.add_rules(rules, order);
                    }
                }
            }
        }
        let root = html.root_element();
        let lang = root.value().attr("xml:lang").or_else(|| root.value().attr("lang")).map(str::to_string).or_else(|| book_lang.clone());
        let d = Doc { path: &doc.path, sheet, lang: lang.clone(), pending: Default::default(), gap: Default::default() };
        let mut blocks = Vec::new();
        let root_comp = d.comp(&root, &Computed::root());
        if let Some(body) = root.children().filter_map(ElementRef::wrap).find(|e| e.value().name() == "body") {
            // body 的左右外边距、内边距不写（Send to Kindle 同样：《人生海海》《ABC谋杀案》body `margin:0 5pt`、《绍宋》新版
            // `padding:1em` 都没有出现在段落上，2026-10-08；更早的《ABC谋杀案》样本加到了每段上，那是另一版 EPUB）。
            // 书里按 body 内容宽度算的百分比折算成整页的（《雪国》body 左右 1%，段落的 1% 写 0.98%）。
            // 例外：文件里有块的左（右）外边距是负的，这一侧 body 的边距照旧加到每个块上（《罗杰疑案》《ABC谋杀案》章标题
            // `margin:-2em` 两侧都留；《平凡的世界》只有左边 `-1em` 的那几个文件只留左边）。
            let mut bc = d.comp(&body, &root_comp);
            if faithful {
                d.block(body, bc, &mut blocks);
                attach_trailing(&mut blocks, d.take_pending());
                collapse_siblings(&mut blocks);
                return (si, d.lang.clone(), blocks);
            }
            let fs = bc.font_size;
            let side = |i: usize| {
                let (m, p) = (to_horiz(bc.margin[i], fs), to_horiz(bc.padding[i], fs));
                Horiz { em: m.em + p.em, pct: m.pct + p.pct }
            };
            let (left, right) = (side(3), side(1));
            for i in [1, 3] {
                bc.margin[i] = None;
                bc.padding[i] = None;
            }
            d.block(body, bc, &mut blocks);
            let (neg_l, neg_r) = (has_negative(&blocks, true), has_negative(&blocks, false));
            let rem = |h: Horiz| h.em + h.pct / 100.0 * PAGE_WIDTH_EM;
            let width = PAGE_WIDTH_EM - if neg_l { 0.0 } else { rem(left) } - if neg_r { 0.0 } else { rem(right) };
            if width > 1.0 && width < PAGE_WIDTH_EM {
                scale_percent(&mut blocks, width / PAGE_WIDTH_EM);
            }
            for b in &mut blocks {
                if neg_l {
                    b.margin_left.em += left.em;
                    b.margin_left.pct += left.pct;
                }
                if neg_r {
                    b.margin_right.em += right.em;
                    b.margin_right.pct += right.pct;
                }
            }
        }
        attach_trailing(&mut blocks, d.take_pending());
        collapse_siblings(&mut blocks);
        apply_gaps(&mut blocks);
        (si, d.lang.clone(), blocks)
    };
    // 一批批解析，每批的结果在本线程复制一份、工作线程分配的那份随即释放：块是在工作线程里边解析 DOM 边分配的，和已经释放的 DOM
    // 交错着留在那几个线程的分配区里，本线程后面生成版面时用不上那些空洞（glibc 按线程分区），不复制的话《阿加莎全集》峰值
    // 从 0.47GB 涨到 0.63GB；复制一遍只多花约 0.1 秒，工作线程那份整块释放掉。
    let mut parsed: Vec<ParsedDoc> = Vec::with_capacity(docs.len());
    for batch in docs.chunks(64) {
        let got = bookconv::util::par_map(batch, parse_doc);
        parsed.extend(got.iter().map(|(si, l, b)| (*si, book.docs[*si].path.as_str(), l.clone(), b.clone())));
    }
    parsed
}

/// 全书范围的分析（版面要用）：注释配对、嵌入字体、写成 `default` 的正文字体、正文字号和行高。
fn analyze(book: &Loaded, b: &mut Builder, parsed: &mut [ParsedDoc]) {
    mark_notes(parsed);
    // 缺字体文件的字体照写原名（Send to Kindle：《春雪》注释的 `ZY-KAITI` 照写；《绍宋》旧版的「宋体」写 `default` 是因为它是正文字体）
    b.embedded_fonts = embedded_font_faces(book);
    b.default_fonts = body_font_to_drop(parsed).into_iter().collect();
    b.base_lh = base_line_height(parsed);
    b.base_fs = base_font_size(parsed);
}

/// 版面阶段的结果。
struct Layout {
    sections: Vec<SectionOut>,
    /// 版面、故事线、文字池实体。
    entities: Vec<Entity>,
    /// (文件, 锚点) → (节点 id, 字符偏移)；锚点空的是文件开头。
    id_map: HashMap<(String, String), (i64, usize)>,
    /// 封面图那一页的版面模板节点 id（导航的封面地标指向它）。
    cover_tmpl: Option<i64>,
}

/// 逐个文档生成版面（`$260`）、故事线（`$259`）、文字池（`$145`），登记每个 id 的位置。
fn lay_out(book: &Loaded, b: &mut Builder, parsed: Vec<ParsedDoc>, fixed_canvas: Option<(i64, i64)>, direction: u32) -> Result<Layout, String> {
    let mut sections: Vec<SectionOut> = Vec::new();
    let mut entities: Vec<Entity> = Vec::new();
    let mut id_map: HashMap<(String, String), (i64, usize)> = HashMap::new();
    let mut cover_tmpl: Option<i64> = None;
    // 没有可见内容的文件（只有隐藏标题之类）：目录项、链接改指到下一个版面的开头。
    let mut empty_docs: Vec<String> = Vec::new();
    let is_cover = |b: &Builder, r: usize| book.cover.as_deref().is_some_and(|c| b.res_by_path.get(c) == Some(&r));
    for (si, path, lang, blocks) in parsed {
        if blocks.is_empty() {
            empty_docs.push(path.to_string());
            continue;
        }
        let sec_name = b.sym(&format!("sec{si}"));
        let story_name = b.sym(&format!("story{si}"));
        let pool = b.sym(&format!("text{si}"));
        let tmpl_eid = b.eid();
        let mut ctx = SectionCtx { pool, texts: Vec::new(), order: vec![(tmpl_eid, 1)], ids: Vec::new(), resources: Vec::new(), lang };
        // 只有一张图的文档写成整页图片版面（封面、插图页），和样本一样。
        let single_image = blocks.len() == 1 && matches!(blocks[0].kind, Kind::Image { .. });
        let nodes: Vec<Value> = if let (true, Some((cw, ch)), Kind::Image { src }) = (single_image, fixed_canvas, &blocks[0].kind) {
            // 固定版式的一页，照 Amazon 转的异形页样本：整页容器（画布宽高、`$476`、position relative）里放一个绝对定位的
            // 图片节点（宽高、上、左）。比画布小的图有的页整页空白是资源符号顺序造成的，不是这里（见下面分配资源符号处）。
            // 图片取不到的这一页同没有内容的文件：目录项、链接改指到下一个版面（以前没登记，指到它的退回书的开头）。
            let Some(r) = b.resource(src) else {
                empty_docs.push(path.to_string());
                continue;
            };
            let (cid, iid) = (b.eid(), b.eid());
            ctx.order.push((cid, 1));
            ctx.order.push((iid, 1));
            ctx.resources.push(r);
            let (w, h) = fixed_image_size((i64::from(b.resources[r].width), i64::from(b.resources[r].height)), (cw, ch));
            let res = b.sym(&b.resources[r].name.clone());
            if cw > 0 {
                let cstyle = b.style(vec![
                    (P_WIDTH, Value::F64(cw as f64)),
                    (P_HEIGHT, Value::F64(ch as f64)),
                    (P_SIZING, Value::Symbol(SIZING_VALUE)),
                    (P_CLIP, Value::Bool(true)),
                    (P_POSITION, Value::Symbol(POSITION_RELATIVE)),
                ]);
                let istyle = b.style(vec![
                    (P_WIDTH, Value::F64(w as f64)),
                    (P_HEIGHT, Value::F64(h as f64)),
                    (P_SIZING, Value::Symbol(SIZING_VALUE)),
                    (P_TOP, Value::F64(((ch - h) / 2) as f64)),
                    (P_LEFT, Value::F64(((cw - w) / 2) as f64)),
                    (P_POSITION, Value::Symbol(POSITION_ABSOLUTE)),
                ]);
                let img = Value::Struct(vec![(EID, Value::Int(iid)), (STYLE_REF, Value::Symbol(istyle)), (NODE_TYPE, Value::Symbol(NODE_IMAGE)), (RESOURCE_REF, Value::Symbol(res))]);
                vec![Value::Struct(vec![
                    (EID, Value::Int(cid)),
                    (TMPL_FIT, Value::Symbol(CONTAINER_LAYOUT)),
                    (STYLE_REF, Value::Symbol(cstyle)),
                    (NODE_TYPE, Value::Symbol(NODE_CONTAINER)),
                    (CHILDREN, Value::List(vec![img])),
                ])]
            } else {
                let style = b.style(vec![(P_WIDTH, Value::F64(w as f64)), (P_HEIGHT, Value::F64(h as f64)), (P_SIZING, Value::Symbol(SIZING_VALUE))]);
                vec![Value::Struct(vec![(EID, Value::Int(iid)), (STYLE_REF, Value::Symbol(style)), (NODE_TYPE, Value::Symbol(NODE_IMAGE)), (RESOURCE_REF, Value::Symbol(res))])]
            }
        } else {
            blocks.iter().filter_map(|bl| b.node(bl, &Parent::root(b.base_fs), &mut ctx)).collect()
        };
        if nodes.is_empty() {
            empty_docs.push(path.to_string());
            continue;
        }
        let mut tmpl = vec![(EID, Value::Int(tmpl_eid)), (STORYLINE_REF, Value::Symbol(story_name))];
        if let (true, Some((cw, ch))) = (single_image, fixed_canvas.filter(|c| c.0 > 0)) {
            tmpl.push((TMPL_WIDTH, Value::Int(cw)));
            tmpl.push((TMPL_HEIGHT, Value::Int(ch)));
            tmpl.push((FIXED_PAGE_FIT, Value::Symbol(FIXED_PAGE_FIT_VALUE)));
            tmpl.push((P_FONT_SIZE, Value::F64(16.0)));
            tmpl.push((DOC_DIRECTION, Value::Symbol(direction)));
            tmpl.push((DOC_WRITING_MODE, Value::Symbol(WRITING_HORIZONTAL)));
            tmpl.push((TMPL_FIT, Value::Symbol(TMPL_FIT_VALUE)));
            tmpl.push((TMPL_ALIGN, Value::Symbol(ALIGN_CENTER)));
            tmpl.push((NODE_TYPE, Value::Symbol(NODE_CONTAINER)));
            if is_cover(b, ctx.resources[0]) {
                cover_tmpl.get_or_insert(tmpl_eid);
            }
        } else if single_image {
            let r = &b.resources[ctx.resources[0]];
            tmpl.push((TMPL_WIDTH, Value::Int(i64::from(r.width))));
            tmpl.push((TMPL_HEIGHT, Value::Int(i64::from(r.height))));
            tmpl.push((TMPL_FIT, Value::Symbol(TMPL_FIT_VALUE)));
            tmpl.push((TMPL_ALIGN, Value::Symbol(ALIGN_CENTER)));
            tmpl.push((NODE_TYPE, Value::Symbol(NODE_CONTAINER)));
            if is_cover(b, ctx.resources[0]) {
                cover_tmpl.get_or_insert(tmpl_eid);
            }
        } else {
            tmpl.push((NODE_TYPE, Value::Symbol(NODE_TEXT)));
        }
        entities.push(ent(sec_name, T_SECTION, Value::Struct(vec![(SECTION_REF, Value::Symbol(sec_name)), (TEMPLATES, Value::List(vec![Value::Struct(tmpl)]))])));
        entities.push(ent(story_name, T_STORYLINE, Value::Struct(vec![(STORYLINE_REF, Value::Symbol(story_name)), (CHILDREN, Value::List(nodes))])));
        if !ctx.texts.is_empty() {
            entities.push(ent(pool, T_TEXT_POOL, Value::Struct(vec![(NAME, Value::Symbol(pool)), (CHILDREN, Value::List(std::mem::take(&mut ctx.texts)))])));
        }
        let first_eid = ctx.order.get(1).map_or(tmpl_eid, |o| o.0);
        id_map.insert((path.to_string(), String::new()), (first_eid, 0));
        for p in empty_docs.drain(..) {
            id_map.insert((p, String::new()), (first_eid, 0));
        }
        for (i, e, o) in ctx.ids {
            id_map.entry((path.to_string(), i)).or_insert((e, o));
        }
        let length = ctx.order.iter().map(|o| o.1).sum();
        sections.push(SectionOut {
            name: sec_name,
            length,
            eids: ctx.order.iter().map(|o| o.0).collect(),
            pid_map: pid_map(&ctx.order),
            resources: ctx.resources,
            first_eid,
        });
    }
    if sections.is_empty() {
        return Err("书里没有可显示的内容".into());
    }
    if let Some(last) = sections.last() {
        for p in empty_docs.drain(..) {
            id_map.insert((p, String::new()), (last.first_eid, 0));
        }
    }
    Ok(Layout { sections, entities, id_map, cover_tmpl })
}

/// 样式实体（`$157`），按第一次用到的顺序。
fn style_entities(b: &mut Builder, entities: &mut Vec<Entity>) {
    for (name, props) in std::mem::take(&mut b.style_entities) {
        let s = b.sym(&name);
        let mut f = props;
        f.push((STYLE_NAME, Value::Symbol(s)));
        entities.push(ent(s, T_STYLE, Value::Struct(f)));
    }
}

/// 要嵌入的字体：样式里用到、书里有 `@font-face` 和字体文件的（正文、批注的字体优化器已经去掉，不会出现在这里）。
/// 字体字节从书里搬出来（写出器只在这里用到），同一个文件给了几个字体名的，前面的复制、最后一个搬走。
fn take_fonts(book: &mut Loaded, b: &Builder) -> Vec<(String, crate::css::FontFace, Vec<u8>)> {
    let mut faces: HashMap<String, crate::css::FontFace> = HashMap::new();
    for (path, text) in &book.css {
        for f in crate::css::font_faces(text, path) {
            faces.entry(f.family.to_lowercase()).or_insert(f);
        }
    }
    let wanted: Vec<(String, crate::css::FontFace)> = b
        .used_fonts
        .iter()
        .filter_map(|f| {
            let face = faces.get(&f.to_lowercase())?;
            book.fonts.iter().any(|(p, _)| *p == face.path).then(|| (f.clone(), face.clone()))
        })
        .collect();
    let mut files: HashMap<String, Vec<u8>> = std::mem::take(&mut book.fonts).into_iter().collect();
    let mut out = Vec::with_capacity(wanted.len());
    for (i, (family, face)) in wanted.iter().enumerate() {
        let used_later = wanted[i + 1..].iter().any(|(_, f)| f.path == face.path);
        let bytes = if used_later { files.get(&face.path).cloned() } else { files.remove(&face.path) };
        out.push((family.clone(), face.clone(), bytes.unwrap_or_default()));
    }
    out
}

/// 阅读顺序（`$169`）：全部版面按书的顺序。阅读顺序实体和文档数据里各写一份。
fn reading_orders(sections: &[SectionOut]) -> Value {
    let order_list = Value::List(sections.iter().map(|s| Value::Symbol(s.name)).collect());
    Value::List(vec![Value::Struct(vec![(READING_ORDER_NAME, Value::Symbol(DEFAULT_READING_ORDER)), (SECTIONS, order_list)])])
}

/// 阅读顺序、各版面的节点、位置范围、位置映射（`$609`）。
fn position_entities(sections: &mut [SectionOut], entities: &mut Vec<Entity>) {
    entities.push(ent(NO_NAME, T_READING_ORDERS, Value::Struct(vec![(READING_ORDERS, reading_orders(sections))])));
    entities.push(ent(
        NO_NAME,
        T_SECTION_EIDS,
        Value::List(sections.iter_mut().map(|s| Value::Struct(vec![(SECTION_REF, Value::Symbol(s.name)), (LIST, compress_ids(std::mem::take(&mut s.eids)))])).collect()),
    ));
    let mut pos = 0usize;
    let mut ranges = Vec::new();
    for s in sections.iter() {
        ranges.push(Value::Struct(vec![(SECTION_REF, Value::Symbol(s.name)), (START, Value::Int(pos as i64)), (LENGTH, Value::Int(s.length as i64))]));
        pos += s.length;
    }
    entities.push(ent(NO_NAME, T_SECTION_RANGES, Value::Struct(vec![(LIST, Value::List(ranges))])));
    for s in sections.iter_mut() {
        entities.push(ent(s.name, T_SECTION_PID_MAP, Value::Struct(vec![(SECTION_REF, Value::Symbol(s.name)), (LIST, Value::List(std::mem::take(&mut s.pid_map)))])));
    }
}

/// 目录、锚点（链接和目录的目标）、标题导航、封面地标。
fn navigation_entities(
    book: &Loaded,
    b: &mut Builder,
    sections: &[SectionOut],
    id_map: &HashMap<(String, String), (i64, usize)>,
    cover_tmpl: Option<i64>,
    entities: &mut Vec<Entity>,
) {
    // 目录：锚点 + 导航。
    let mut toc_stack: Vec<(u32, Vec<Value>)> = vec![(0, Vec::new())];
    for t in &book.toc {
        let Some(&(eid, off)) = id_map.get(&(t.path.clone(), t.frag.clone())).or_else(|| id_map.get(&(t.path.clone(), String::new()))) else {
            b.warnings.push(format!("目录项找不到位置：{}", t.label));
            continue;
        };
        let level = t.level + 1;
        while toc_stack.len() > 1 && toc_stack.last().is_some_and(|(l, _)| *l >= level) {
            close_level(&mut toc_stack);
        }
        b.anchor((t.path.clone(), t.frag.clone()));
        let target = Value::Struct(vec![(EID, Value::Int(eid)), (OFFSET, Value::Int(off as i64))]);
        let entry = Value::Struct(vec![(NAV_LABEL, Value::Struct(vec![(NAV_LABEL_TEXT, Value::String(t.label.clone()))])), (NAV_TARGET, target)]);
        if let Some(last) = toc_stack.last_mut() {
            last.1.push(entry);
        }
        toc_stack.push((level, Vec::new()));
    }
    while toc_stack.len() > 1 {
        close_level(&mut toc_stack);
    }
    let mut toc_entries = toc_stack.pop().map(|t| t.1).unwrap_or_default();
    if toc_entries.is_empty() {
        // 没有目录：每个版面一项，标签用序号。
        toc_entries = sections
            .iter()
            .enumerate()
            .map(|(i, s)| {
                Value::Struct(vec![
                    (NAV_LABEL, Value::Struct(vec![(NAV_LABEL_TEXT, Value::String((i + 1).to_string()))])),
                    (NAV_TARGET, Value::Struct(vec![(EID, Value::Int(s.first_eid)), (OFFSET, Value::Int(0))])),
                ])
            })
            .collect();
    }
    // 锚点（链接、目录的目标）。找不到目标的指向目标文件开头，文件也找不到就指向书的开头。之后不再登记锚点。
    let book_start = (sections[0].first_eid, 0usize);
    for (key, an) in std::mem::take(&mut b.anchors) {
        let pos = id_map.get(&key).or_else(|| id_map.get(&(key.0.clone(), String::new()))).copied().unwrap_or_else(|| {
            b.warnings.push(format!("链接目标找不到：{}#{}", key.0, key.1));
            book_start
        });
        let target = Value::Struct(vec![(EID, Value::Int(pos.0)), (OFFSET, Value::Int(pos.1 as i64))]);
        entities.push(ent(an, T_ANCHOR, Value::Struct(vec![(ANCHOR_NAME, Value::Symbol(an)), (ANCHOR_POSITION, target)])));
    }
    let mut nav_names = Vec::new();
    // 标题导航：每个标题级别一组（样本里按级别第一次出现的顺序排）。
    if !b.headings.is_empty() {
        let mut levels: Vec<u8> = Vec::new();
        for (l, _) in &b.headings {
            if !levels.contains(l) {
                levels.push(*l);
            }
        }
        let unit = |eid: i64| {
            vec![
                (NAV_LABEL, Value::Struct(vec![(NAV_LABEL_TEXT, Value::String("heading-nav-unit".into()))])),
                (NAV_TARGET, Value::Struct(vec![(EID, Value::Int(eid)), (OFFSET, Value::Int(0))])),
            ]
        };
        let groups: Vec<Value> = levels
            .iter()
            .map(|&l| {
                let eids: Vec<i64> = b.headings.iter().filter(|(hl, _)| *hl == l).map(|h| h.1).collect();
                let mut f = vec![(NAV_LANDMARK_TYPE, Value::Symbol(HEADING_LEVEL_1 + u32::from(l) - 1))];
                f.extend(unit(eids[0]));
                f.push((NAV_ENTRIES, Value::List(eids.iter().map(|&e| Value::Struct(unit(e))).collect())));
                Value::Struct(f)
            })
            .collect();
        let n = b.sym("nav-headings");
        nav_names.push(Value::Symbol(n));
        entities.push(ent(n, T_NAV_CONTAINER, Value::Struct(vec![(NAV_TYPE, Value::Symbol(NAV_TYPE_HEADINGS)), (NAV_NAME, Value::Symbol(n)), (NAV_ENTRIES, Value::List(groups))])));
    }
    if let Some(ct) = cover_tmpl {
        let n = b.sym("nav-landmarks");
        nav_names.push(Value::Symbol(n));
        entities.push(ent(
            n,
            T_NAV_CONTAINER,
            Value::Struct(vec![
                (NAV_TYPE, Value::Symbol(NAV_TYPE_LANDMARKS)),
                (NAV_NAME, Value::Symbol(n)),
                (
                    NAV_ENTRIES,
                    Value::List(vec![Value::Struct(vec![
                        (NAV_LANDMARK_TYPE, Value::Symbol(LANDMARK_COVER)),
                        (NAV_LABEL, Value::Struct(vec![(NAV_LABEL_TEXT, Value::String("cover-nav-unit".into()))])),
                        (NAV_TARGET, Value::Struct(vec![(EID, Value::Int(ct)), (OFFSET, Value::Int(0))])),
                    ])]),
                ),
            ]),
        ));
    }
    let toc_name = b.sym("nav-toc");
    nav_names.push(Value::Symbol(toc_name));
    entities.push(ent(
        toc_name,
        T_NAV_CONTAINER,
        Value::Struct(vec![(NAV_TYPE, Value::Symbol(NAV_TYPE_TOC)), (NAV_NAME, Value::Symbol(toc_name)), (NAV_ENTRIES, Value::List(toc_entries))]),
    ));
    entities.push(ent(
        NO_NAME,
        T_NAV_ROOTS,
        Value::List(vec![Value::Struct(vec![(READING_ORDER_NAME, Value::Symbol(DEFAULT_READING_ORDER)), (NAV_CONTAINERS, Value::List(nav_names))])]),
    ));
    entities.push(ent(NO_NAME, T_NAV_EMPTY, Value::Struct(vec![(NAV_ENTRIES, Value::List(Vec::new()))])));
}

/// 唯一 ID 派生的 content_id（也当 ASIN 写）。
fn content_id(id: u64) -> String {
    format!("{:016X}{:016X}", id, fnv64(&id.to_le_bytes()))
}

/// 元数据（`$490`）和文档数据（`$538`）。
#[allow(clippy::too_many_arguments)]
fn metadata_entities(
    book: &Loaded,
    b: &mut Builder,
    id: u64,
    sections: &[SectionOut],
    fixed_canvas: Option<(i64, i64)>,
    direction: u32,
    cover_res: Option<usize>,
    entities: &mut Vec<Entity>,
) {
    let content_id = content_id(id);
    let container_id = container_id(id);
    let book_id = base32(id.rotate_left(17), 23);
    let book_lang = book_language(book);
    let kv = |k: &str, v: Value| Value::Struct(vec![(META_KEY, Value::String(k.into())), (META_VALUE, v)]);
    let s = |v: &str| Value::String(v.to_string());
    let mut title_meta = vec![
        kv("book_id", s(&book_id)),
        kv("title", s(&book.meta.title)),
        kv("publisher", s(&book.meta.publisher)),
        kv("language", s(&meta_language(book_lang.as_deref()))),
        kv("issue_date", s(&issue_date(&book.meta.date))),
        kv("content_id", s(&content_id)),
        kv("cde_content_type", s("PDOC")),
        kv("ASIN", s(&content_id)),
        kv("is_sample", Value::Bool(false)),
        kv("asset_id", s(&container_id)),
    ];
    for a in &book.meta.authors {
        title_meta.push(kv("author", s(a)));
    }
    if cover_res.is_some() {
        title_meta.push(kv("cover_image", s(COVER_REF)));
    }
    let group = |name: &str, entries: Vec<Value>| Value::Struct(vec![(META_GROUP_NAME, s(name)), (META_ENTRIES, Value::List(entries))]);
    entities.push(ent(
        NO_NAME,
        T_METADATA,
        Value::Struct(vec![(
            META_GROUPS,
            Value::List(
                vec![
                    if fixed_canvas.is_some() {
                        // 固定版式（同 Amazon 转的测试漫画）：锁竖屏、声明固定版式
                        let lock = book.meta.fixed_layout.iter().find(|(n, _)| *n == 124).map_or("portrait", |(_, v)| v.as_str());
                        group("kindle_ebook_metadata", vec![kv("book_orientation_lock", s(lock))])
                    } else {
                        group("kindle_ebook_metadata", vec![kv("nested_span", s("enabled")), kv("selection", s("enabled"))])
                    },
                    group("kindle_title_metadata", title_meta),
                    group("kindle_audit_metadata", vec![kv("creator_version", s(FILE_CREATOR_VERSION)), kv("file_creator", s("epub-to-kfx"))]),
                ]
                .into_iter()
                .chain(fixed_canvas.map(|_| group("kindle_capability_metadata", vec![kv("yj_fixed_layout", Value::Int(1)), kv("continuous_popup_progression", Value::Int(0))])))
                .collect(),
            ),
        )]),
    ));

    // 文档数据（照样本的值，含义还没全弄清）。
    let mut doc_data = if fixed_canvas.is_some() {
        vec![(DOC_DIRECTION, Value::Symbol(direction)), (DOC_WRITING_MODE, Value::Symbol(WRITING_HORIZONTAL)), (DOC_FIXED, Value::Symbol(DOC_FIXED_VALUE))]
    } else {
        vec![
            (112, Value::Symbol(383)),
            (P_FONT_SIZE, num(1.0, U_EM)),
            (DOC_WRITING_MODE, Value::Symbol(WRITING_HORIZONTAL)),
            (DOC_DIRECTION, Value::Symbol(direction)),
            (436, Value::Symbol(441)),
        ]
    };
    if cover_res.is_some() {
        let aux = b.sym(COVER_AUX);
        doc_data.push((DOC_AUX, Value::Struct(vec![(DOC_AUX_NAME, Value::Symbol(aux))])));
    }
    doc_data.push((ion::SID_MAX_ID, Value::Int(b.next_eid)));
    if fixed_canvas.is_none() {
        doc_data.push((P_LINE_HEIGHT, num(LH_EM, U_EM)));
    }
    doc_data.extend([(477, Value::Symbol(56)), (READING_ORDERS, reading_orders(sections))]);
    entities.push(ent(NO_NAME, T_DOCUMENT_DATA, Value::Struct(doc_data)));
}

/// 元数据的 `issue_date`（`YYYY-MM-DD`）：OPF 日期取前面能用的部分，只写到年、月的补成当年 1 月 1 日、当月 1 日
/// （`2019` → `2019-01-01`，`2019-05` → `2019-05-01`；以前不足 10 个字符的整个换成 2000-01-01）。认不出年份的写 2000-01-01。
fn issue_date(date: &str) -> String {
    let d = date.trim();
    fn digits(s: Option<&str>) -> Option<&str> {
        s.filter(|s| s.bytes().all(|c| c.is_ascii_digit()))
    }
    let (Some(y), m, day) = (digits(d.get(..4)), digits(d.get(5..7)), digits(d.get(8..10))) else {
        return "2000-01-01".to_string();
    };
    let sep = |i: usize| d.as_bytes().get(i) == Some(&b'-');
    match (m.filter(|_| sep(4)), day.filter(|_| sep(7))) {
        (Some(m), Some(day)) => format!("{y}-{m}-{day}"),
        (Some(m), None) => format!("{y}-{m}-01"),
        _ => format!("{y}-01-01"),
    }
}

/// 图片、字体的字节实体（`$417`/`$418`）、资源（`$164`）、字体（`$262`）。
fn resource_entities(b: &mut Builder, fonts: Vec<(String, crate::css::FontFace, Vec<u8>)>, cover_res: Option<usize>, entities: &mut Vec<Entity>) {
    let res_list: Vec<(String, String, u32, &'static str, u32, u32)> =
        b.resources.iter().map(|r| (r.name.clone(), r.location.clone(), r.format, r.mime, r.width, r.height)).collect();
    // 资源（字节实体名、资源路径）的符号最后分配：书里不能有实体的 id 排在资源路径的符号后面。以前资源先分配、目录锚点和
    // 导航后分配，固定版式里比画布小的图有的页整页空白（哪几页随实体集合变）；Amazon 转的书从来不这样排，给它加上排在后面的
    // 锚点也出空白页（2026-10-06 真机，42 本测试书对照，见 docs/kfx.md）。
    // 字节实体名、资源路径：图片在前、字体在后，一起按组分配符号。
    let mut raws: Vec<(String, String)> = res_list.iter().enumerate().map(|(i, (name, loc, ..))| (raw_name(i, name, cover_res), loc.clone())).collect();
    raws.extend((0..fonts.len()).map(|i| (format!("font{i}-ad"), format!("resource/font{i}"))));
    // 图片字节实体和资源路径的符号要隔 9 个：Kindle 按「`$165` 资源路径的符号编号 − 9」找图片字节
    // （6 本样本 443 个资源全是这样；只改资源路径或只给字节实体改名，书架缩略图就没了，2026-10-05 真机）。
    // 每 9 个资源一组：先这组的字节实体名，不满 9 个用占位符号补齐，再这组的资源路径。
    let mut raw_sids = vec![0u32; raws.len()];
    for (g, chunk) in raws.chunks(SID_GAP as usize).enumerate() {
        let base = g * SID_GAP as usize;
        for (k, (raw, _)) in chunk.iter().enumerate() {
            raw_sids[base + k] = b.sym(raw);
        }
        for k in chunk.len()..SID_GAP as usize {
            b.sym(&format!("pad{g}-{k}"));
        }
        for (k, (_, loc)) in chunk.iter().enumerate() {
            let l = b.sym(loc);
            debug_assert_eq!(l, raw_sids[base + k] + SID_GAP);
        }
    }
    for (i, (family, face, bytes)) in fonts.into_iter().enumerate() {
        let (raw, loc) = &raws[res_list.len() + i];
        entities.push(ent(
            NO_NAME,
            T_FONT,
            Value::Struct(vec![
                (P_FONT_FAMILY, Value::String(family)),
                (P_FONT_STYLE, Value::Symbol(if face.italic { STYLE_ITALIC } else { FONT_NORMAL })),
                (P_FONT_WEIGHT, Value::Symbol(if face.bold { WEIGHT_BOLD } else { WEIGHT_NORMAL })),
                (P_FONT_STRETCH, Value::Symbol(FONT_NORMAL)),
                (RES_LOCATION, Value::String(loc.clone())),
            ]),
        ));
        let id = b.local_index[raw];
        entities.push(Entity { id, ty: T_RAW_FONT, version: 1, header: entity_header(), body: Body::Raw(bytes) });
    }
    for (i, (name, loc, format, mime, w, h)) in res_list.iter().enumerate() {
        let n = b.sym(name);
        let l = raw_sids[i];
        entities.push(ent(
            n,
            T_RESOURCE,
            Value::Struct(vec![
                (RES_FORMAT, Value::Symbol(*format)),
                (RES_MIME, Value::String(mime.to_string())),
                (RES_LOCATION, Value::String(loc.clone())),
                (RES_WIDTH, Value::Int(i64::from(*w))),
                (RESOURCE_REF, Value::Symbol(n)),
                (RES_HEIGHT, Value::Int(i64::from(*h))),
            ]),
        ));
        let bytes = std::mem::take(&mut b.resources[i].bytes);
        entities.push(Entity { id: l, ty: T_RAW_MEDIA, version: 1, header: entity_header(), body: Body::Raw(bytes) });
    }
}

/// 图片字节的实体名。封面的要叫「文档数据 `$538.$597.$614` 的名字 + `-ad`」：Kindle 书架缩略图按这个找
/// （样本里元数据的 `cover_image` 一律写 `e6`，常常指到别的插图甚至锚点，缩略图照样对；
/// 把这个实体改名缩略图就没了，2026-10-05 真机）。
fn raw_name(i: usize, name: &str, cover_res: Option<usize>) -> String {
    if Some(i) == cover_res { format!("{COVER_AUX}-ad") } else { format!("{name}-ad") }
}

/// 实体按类型排序、加上清单（`$419`），连同符号表、能力表写成容器。
fn finish(b: &Builder, sections: &[SectionOut], mut entities: Vec<Entity>, cover_res: Option<usize>, id: u64) -> Vec<u8> {
    let container_id = container_id(id);
    let s = |v: &str| Value::String(v.to_string());
    // 按类型排序（和样本一样），清单放最后。
    entities.sort_by_key(|e| e.ty);
    let mut deps = Vec::new();
    for s in sections {
        if !s.resources.is_empty() {
            // 样本里每个资源列两次（含义不明，照抄）。
            let mut names: Vec<Value> = Vec::new();
            for &r in &s.resources {
                let v = Value::Symbol(b.local_index[&b.resources[r].name]);
                if !names.contains(&v) {
                    names.push(v.clone());
                    names.push(v);
                }
            }
            deps.push(Value::Struct(vec![(EID, Value::Symbol(s.name)), (MANIFEST_DEP_LIST, Value::List(names))]));
        }
    }
    for (i, r) in b.resources.iter().enumerate() {
        deps.push(Value::Struct(vec![
            (EID, Value::Symbol(b.local_index[&r.name])),
            (MANIFEST_DEP_LIST, Value::List(vec![Value::Symbol(b.local_index[&raw_name(i, &r.name, cover_res)])])),
        ]));
    }
    let all_ids = Value::List(entities.iter().map(|e| Value::Symbol(e.id)).collect());
    let manifest = Value::Annotated(
        vec![T_MANIFEST],
        Box::new(Value::Struct(vec![
            (MANIFEST_CONTAINERS, Value::List(vec![Value::Struct(vec![(EID, s(&container_id)), (LIST, all_ids)])])),
            (MANIFEST_DEPS, Value::List(deps)),
        ])),
    );
    entities.push(ent(NO_NAME, T_MANIFEST, manifest));

    // 容器。
    let symtab = vec![
        Item::Bvm,
        Item::Value(Value::Annotated(
            vec![ion::SID_ION_SYMBOL_TABLE],
            Box::new(Value::Struct(vec![
                (
                    ion::SID_IMPORTS,
                    Value::List(vec![Value::Struct(vec![
                        (ion::SID_NAME, s("YJ_symbols")),
                        (ion::SID_VERSION, Value::Int(10)),
                        (ion::SID_MAX_ID, Value::Int(YJ_SYMBOLS_MAX_ID)),
                    ])]),
                ),
                (ion::SID_SYMBOLS, Value::List(b.locals.iter().map(|l| s(l)).collect())),
            ])),
        )),
    ];
    let caps = vec![
        Item::Bvm,
        Item::Value(Value::Annotated(
            vec![CAPABILITIES],
            Box::new(Value::List(vec![
                Value::Struct(vec![(META_KEY, s("kfxgen.positionMaps")), (ion::SID_VERSION, Value::Int(2))]),
                Value::Struct(vec![(META_KEY, s("kfxgen.pidMapWithOffset")), (ion::SID_VERSION, Value::Int(1))]),
            ])),
        )),
    ];
    use crate::container::*;
    let info = Value::Struct(vec![
        (SID_CONTAINER_ID, s(&container_id)),
        (SID_COMPRESSION, Value::Int(0)),
        (SID_DRM_SCHEME, Value::Int(0)),
        (SID_INDEX_OFFSET, Value::Int(0)),
        (SID_INDEX_LENGTH, Value::Int(0)),
        (SID_SYMTAB_OFFSET, Value::Int(0)),
        (SID_SYMTAB_LENGTH, Value::Int(0)),
        (SID_CHUNK_SIZE, Value::Int(4096)),
        (SID_CAPS_OFFSET, Value::Int(0)),
        (SID_CAPS_LENGTH, Value::Int(0)),
    ]);
    let kfxgen = format!(
        "[{{key:\"kfxgen_package_version\",value:\"epub-to-kfx {FILE_CREATOR_VERSION}\"}},{{key:\"kfxgen_payload_sha1\",value:\"{}\"}},{{key:\"kfxgen_acr\",value:\"{container_id}\"}}]",
        "0".repeat(40)
    );
    let c = Container { version: 2, info, symtab, capabilities: caps, kfxgen: kfxgen.into_bytes(), entities };
    c.into_bytes()
}

/// 元数据里写的语言。中文书写 `en`：Kindle 只看元数据语言决定开不开「Aa → 间距」里的段间距、字间距、字符间距，
/// 中文书不开；正文样式里的 `$10` 仍是书本身的语言，中文排版（字体、避头尾、标点挤压）不受影响。代价是长按查词
/// 默认弹英文词典，要在右下角手动切到中文词典（用户 2026-10-05 选的，真机「封面结构63」）。
fn meta_language(book_lang: Option<&str>) -> String {
    match book_lang {
        Some(l) if l.to_ascii_lowercase().starts_with("zh") => "en".to_string(),
        Some(l) => l.to_string(),
        None => "en".to_string(),
    }
}

fn close_level(stack: &mut Vec<(u32, Vec<Value>)>) {
    let Some((_, kids)) = stack.pop() else { return };
    if kids.is_empty() {
        return;
    }
    if let Some(Value::Struct(f)) = stack.last_mut().and_then(|p| p.1.last_mut()) {
        f.push((NAV_ENTRIES, Value::List(kids)));
    }
}

/// 固定版式一页里图片的显示宽高：保持比例缩进画布，**不超过图自己的尺寸**——节点比图片资源大时 Kindle 不放大、整页空白
/// （2026-10-06 真机：954×1272 的图写成画布 1272×1696 → 空白页）。所以比例和画布一致（`comicfxl::fills_canvas`，误差 1px 内）
/// 且不比画布小的写画布大小，比画布小的、比例不一致的（装饰小图、窄页、跨页）按缩放后的尺寸，由调用方居中。
/// 以前一律写画布大小：比例不一致的被拉伸，小的空白。画布宽高未知（0）或图的尺寸取不到时退回另一方。
fn fixed_image_size((iw, ih): (i64, i64), (cw, ch): (i64, i64)) -> (i64, i64) {
    if cw <= 0 || ch <= 0 {
        return (iw, ih);
    }
    if iw <= 0 || ih <= 0 || (iw >= cw && bookconv::comicfxl::fills_canvas((iw as u32, ih as u32), cw as u32, ch as u32)) {
        return (cw, ch);
    }
    let s = (cw as f64 / iw as f64).min(ch as f64 / ih as f64).min(1.0);
    (((iw as f64 * s).round() as i64).max(1), ((ih as f64 * s).round() as i64).max(1))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 一本小 EPUB（封面、章节、粗体、换行、书内链接、两级目录）走一遍，核对真机定下来的几条规矩。
    #[test]
    fn end_to_end_rules() {
        use crate::container::Container;
        use crate::ion::SymbolTable;
        let mut jpeg = Vec::new();
        image::codecs::jpeg::JpegEncoder::new(&mut jpeg).encode_image(&image::GrayImage::from_pixel(4, 6, image::Luma([128]))).unwrap();
        let mut w = bookconv::epubzip::EpubWriter::new(std::io::Cursor::new(Vec::new())).unwrap();
        w.put("META-INF/container.xml", br#"<?xml version="1.0"?><container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container"><rootfiles><rootfile full-path="OEBPS/content.opf" media-type="application/oebps-package+xml"/></rootfiles></container>"#).unwrap();
        w.put("OEBPS/content.opf", r#"<?xml version="1.0"?><package xmlns="http://www.idpf.org/2007/opf" version="3.0" unique-identifier="id"><metadata xmlns:dc="http://purl.org/dc/elements/1.1/"><dc:identifier id="id">x</dc:identifier><dc:title>书</dc:title><dc:language>zh</dc:language></metadata><manifest><item id="nav" href="nav.xhtml" media-type="application/xhtml+xml" properties="nav"/><item id="img" href="c.jpg" media-type="image/jpeg" properties="cover-image"/><item id="cv" href="cover.xhtml" media-type="application/xhtml+xml"/><item id="c1" href="c1.xhtml" media-type="application/xhtml+xml"/><item id="c2" href="c2.xhtml" media-type="application/xhtml+xml"/></manifest><spine><itemref idref="cv"/><itemref idref="c1"/><itemref idref="c2"/></spine></package>"#.as_bytes()).unwrap();
        w.put("OEBPS/c.jpg", &jpeg).unwrap();
        w.put("OEBPS/nav.xhtml", r#"<html xmlns="http://www.w3.org/1999/xhtml" xmlns:epub="http://www.idpf.org/2007/ops"><body><nav epub:type="toc"><ol><li><a href="c1.xhtml">第一章</a><ol><li><a href="c2.xhtml#s">1</a></li></ol></li></ol></nav></body></html>"#.as_bytes()).unwrap();
        w.put("OEBPS/cover.xhtml", br#"<html xmlns="http://www.w3.org/1999/xhtml"><body><div><img src="c.jpg"/></div></body></html>"#).unwrap();
        w.put("OEBPS/c1.xhtml", r#"<html xmlns="http://www.w3.org/1999/xhtml"><head><style>p{text-indent:2em}</style></head><body><h1>第一章</h1><p>甲<b>乙</b>丙<br/>丁 <a href="c2.xhtml#s">跳</a></p></body></html>"#.as_bytes()).unwrap();
        w.put("OEBPS/c2.xhtml", r#"<html xmlns="http://www.w3.org/1999/xhtml"><body><h3 id="s">1</h3><p>戊</p></body></html>"#.as_bytes()).unwrap();
        let epub = w.finish().unwrap().into_inner();

        let (kfx, warnings) = epub_to_kfx(&epub, &Opts { fixed_id: Some(1), ..Default::default() }).unwrap();
        assert!(warnings.is_empty(), "{warnings:?}");
        let c = Container::parse(&kfx).unwrap();
        let syms: SymbolTable = c.symbols();
        let of = |ty: u32| c.entities.iter().filter(move |e| e.ty == ty);
        // 文字一个不多一个不少，`<br>` 是 `\n`。
        let texts: Vec<String> = of(T_TEXT_POOL).flat_map(|e| e.value().unwrap().field(CHILDREN).unwrap().as_list().unwrap().iter().map(|v| v.as_str().unwrap().to_string())).collect();
        assert_eq!(texts, ["第一章", "甲乙丙\n丁 跳", "1", "戊"]);
        // 图片字节：资源路径的符号编号 − 9 = 字节实体（且就是清单里的依赖）。
        let res = of(T_RESOURCE).next().unwrap().value().unwrap().clone();
        let loc = syms.sid(res.field(RES_LOCATION).unwrap().as_str().unwrap()).unwrap();
        let raw = of(T_RAW_MEDIA).next().unwrap();
        assert_eq!(raw.id, loc - SID_GAP);
        // 封面：`cover_image` 的符号编号 − 9 = 封面资源；字节实体叫 `$538.$597.$614` + `-ad`。
        let meta = format!("{:?}", of(T_METADATA).next().unwrap().value().unwrap());
        assert!(meta.contains(&format!("String({COVER_REF:?})")), "{meta}");
        // 中文书的元数据语言写 en（开间距设置），正文样式里仍是 zh。
        assert!(meta.contains(r#"String("language")), (307, String("en"))"#), "{meta}");
        let styles = format!("{:?}", of(T_STYLE).map(|e| e.value().unwrap().clone()).collect::<Vec<_>>());
        assert!(styles.contains(r#"(10, String("zh"))"#), "{styles}");
        assert_eq!(syms.sid(COVER_REF).unwrap() - SID_GAP, res.field(RESOURCE_REF).unwrap().as_symbol().unwrap());
        let aux = of(T_DOCUMENT_DATA).next().unwrap().value().unwrap().field(DOC_AUX).unwrap().field(DOC_AUX_NAME).unwrap().as_symbol().unwrap();
        assert_eq!(syms.display(raw.id), format!("{}-ad", syms.display(aux)));
        // 链接指向的锚点存在；目录两级。
        let anchors: Vec<u32> = of(T_ANCHOR).map(|e| e.id).collect();
        let story = format!("{:?}", of(T_STORYLINE).map(|e| e.value().unwrap().clone()).collect::<Vec<_>>());
        assert!(anchors.iter().any(|a| story.contains(&format!("({LINK_TO}, Symbol({a}))"))), "{story}");
        let toc = of(T_NAV_CONTAINER).find(|e| e.value().unwrap().field(NAV_TYPE) == Some(&Value::Symbol(NAV_TYPE_TOC))).unwrap();
        let top = toc.value().unwrap().field(NAV_ENTRIES).unwrap().as_list().unwrap();
        assert_eq!(top.len(), 1);
        assert_eq!(top[0].field(NAV_ENTRIES).unwrap().as_list().unwrap().len(), 1);
        // 位置：各版面长度之和 = `$609` 游程之和。
        let ranges = of(T_SECTION_RANGES).next().unwrap().value().unwrap().field(LIST).unwrap().as_list().unwrap().to_vec();
        for (r, m) in ranges.iter().zip(of(T_SECTION_PID_MAP)) {
            let sum: i64 = m.value().unwrap().field(LIST).unwrap().as_list().unwrap().iter().map(|x| x.as_list().map_or_else(|| x.as_int().unwrap(), |p| p[0].as_int().unwrap())).sum();
            assert_eq!(sum, r.field(LENGTH).unwrap().as_int().unwrap());
        }
    }

    /// 注释配对：正文链接 → 章末注释，注释开头的回链指回正文链接。只有正文那条算注释引用，回链不算。
    #[test]
    fn notes_paired_by_backlink() {
        let mut w = bookconv::epubzip::EpubWriter::new(std::io::Cursor::new(Vec::new())).unwrap();
        w.put("META-INF/container.xml", br#"<?xml version="1.0"?><container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container"><rootfiles><rootfile full-path="c.opf" media-type="application/oebps-package+xml"/></rootfiles></container>"#).unwrap();
        w.put("c.opf", br#"<?xml version="1.0"?><package xmlns="http://www.idpf.org/2007/opf" version="3.0" unique-identifier="id"><metadata xmlns:dc="http://purl.org/dc/elements/1.1/"><dc:identifier id="id">x</dc:identifier><dc:title>t</dc:title><dc:language>zh</dc:language></metadata><manifest><item id="c1" href="c1.xhtml" media-type="application/xhtml+xml"/></manifest><spine><itemref idref="c1"/></spine></package>"#).unwrap();
        w.put("c1.xhtml", r##"<html xmlns="http://www.w3.org/1999/xhtml"><body><p>正文<a id="r1" href="#n1"><sup>[1]</sup></a>接着</p><p>别的<a href="#x">链接</a></p><p id="x">目标</p><p class="fn"><a id="n1" href="#r1">[1]</a>注释正文</p></body></html>"##.as_bytes()).unwrap();
        let epub = w.finish().unwrap().into_inner();
        let (kfx, _) = epub_to_kfx(&epub, &Opts { fixed_id: Some(1), ..Default::default() }).unwrap();
        let c = crate::container::Container::parse(&kfx).unwrap();
        let story = format!("{:?}", c.entities.iter().filter(|e| e.ty == T_STORYLINE).map(|e| e.value().unwrap().clone()).collect::<Vec<_>>());
        assert_eq!(story.matches(&format!("({NOTE_REF}, Symbol({NOTE_REF_POPUP}))")).count(), 1, "{story}");
        assert_eq!(story.matches(&format!("({NOTE_CONTENT}, Symbol({NOTE_CONTENT_FOOTNOTE}))")).count(), 1, "{story}");
    }

    /// 列表、表格、边框按 2026-10-05 测试书（Amazon 转出的 KFX）对照出来的写法。
    #[test]
    fn lists_tables_borders_like_amazon() {
        let mut w = bookconv::epubzip::EpubWriter::new(std::io::Cursor::new(Vec::new())).unwrap();
        w.put("META-INF/container.xml", br#"<?xml version="1.0"?><container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container"><rootfiles><rootfile full-path="c.opf" media-type="application/oebps-package+xml"/></rootfiles></container>"#).unwrap();
        w.put("c.opf", br#"<?xml version="1.0"?><package xmlns="http://www.idpf.org/2007/opf" version="3.0" unique-identifier="id"><metadata xmlns:dc="http://purl.org/dc/elements/1.1/"><dc:identifier id="id">x</dc:identifier><dc:title>t</dc:title><dc:language>zh</dc:language></metadata><manifest><item id="c1" href="c1.xhtml" media-type="application/xhtml+xml"/></manifest><spine><itemref idref="c1"/></spine></package>"#).unwrap();
        w.put("c1.xhtml", r#"<html xmlns="http://www.w3.org/1999/xhtml"><head><style>td{border:1px solid #000;vertical-align:top} .b{border-top:2px dashed red;padding-left:0.5em} li.x{list-style-type:lower-roman}</style></head><body>
            <ul><li>甲<ul><li>乙</li></ul></li></ul>
            <ol start="3"><li class="x">丙</li><li value="9"><p>丁</p><p>戊</p></li></ol>
            <ul style="list-style:none"><li>己</li></ul>
            <table style="border-collapse:collapse;width:80%"><caption>表题</caption><tr><td colspan="2" style="width:30%">庚</td></tr><tr><td rowspan="2">辛</td><td></td></tr></table>
            <div class="b"><p>壬</p></div><hr/><p>前<u>癸</u></p></body></html>"#.as_bytes()).unwrap();
        let epub = w.finish().unwrap().into_inner();
        let (kfx, warnings) = epub_to_kfx(&epub, &Opts { fixed_id: Some(1), ..Default::default() }).unwrap();
        assert!(warnings.is_empty(), "{warnings:?}");
        let c = crate::container::Container::parse(&kfx).unwrap();
        let story = format!("{:?}", c.entities.iter().filter(|e| e.ty == T_STORYLINE).map(|e| e.value().unwrap().clone()).collect::<Vec<_>>());
        let styles = format!("{:?}", c.entities.iter().filter(|e| e.ty == T_STYLE).map(|e| e.value().unwrap().clone()).collect::<Vec<_>>());
        let has = |k: u32, v: u32| story.contains(&format!("({k}, Symbol({v}))"));
        // 列表：嵌套的 ul 换空心圆；li 上写的符号优先；start/value；不显示符号的不是列表
        assert!(has(LIST_STYLE, LIST_DISC) && has(LIST_STYLE, LIST_CIRCLE) && has(LIST_STYLE, LIST_LOWER_ROMAN), "{story}");
        assert_eq!(story.matches(&format!("({NODE_TYPE}, Symbol({NODE_LIST}))")).count(), 3, "{story}");
        assert!(story.contains(&format!("({LIST_START}, Int(3))")) && story.contains(&format!("({LIST_START}, Int(9))")), "{story}");
        // 表格：表 → 表体 → 行 → 单元格，标题；跨列跨行、竖直对齐、列宽、边框合并
        for t in [NODE_TABLE, NODE_TBODY, NODE_ROW] {
            assert!(has(NODE_TYPE, t), "{t}: {story}");
        }
        assert!(has(NOTE_CONTENT, CAPTION) && story.contains(&format!("({TABLE_COLLAPSE}, Bool(true))")) && story.contains(&format!("({TABLE_COLUMNS}, ")), "{story}");
        assert!(styles.contains(&format!("({P_COLSPAN}, Int(2))")) && styles.contains(&format!("({P_ROWSPAN}, Int(2))")), "{styles}");
        assert!(styles.contains(&format!("({P_CELL_VALIGN}, Symbol({VALIGN_TOP}))")), "{styles}");
        // 边框：四边一样写「全部」（1px＝0.45pt），只有上边写上边；左内边距是 `$53`
        assert!(styles.contains(&format!("({}, Symbol({BORDER_SOLID}))", P_BORDER_STYLE[0])) && styles.contains("(307, F64(0.45))"), "{styles}");
        assert!(styles.contains(&format!("({}, Symbol({BORDER_DASHED}))", P_BORDER_STYLE[1])) && styles.contains(&format!("({}, Int({}))", P_BORDER_COLOR[1], 0xFFFF0000u32)), "{styles}");
        assert!(styles.contains(&format!("({P_PADDING_LEFT}, ")), "{styles}");
        assert!(has(NODE_TYPE, NODE_HR) && styles.contains(&format!("({P_UNDERLINE}, Symbol({BORDER_SOLID}))")), "{story}");
        // 文字一个不多一个不少
        let texts: Vec<String> = c.entities.iter().filter(|e| e.ty == T_TEXT_POOL).flat_map(|e| e.value().unwrap().field(CHILDREN).unwrap().as_list().unwrap().iter().map(|v| v.as_str().unwrap().to_string()).collect::<Vec<_>>()).collect();
        assert_eq!(texts.concat(), "甲乙丙丁戊己表题庚辛壬前癸");
    }

    /// 照 Send to Kindle 排《绍宋》的口径（2026-10-08 对照）：行高按正文行高归一、边距按本元素行高换算、`<p>` 和标题的 UA 外边距、
    /// 标题 UA 字号、全透明边框写「透明」、左右百分比内边距照写、px 外边距按 0.45pt、`white-space:nowrap`。
    #[test]
    fn styles_like_send_to_kindle() {
        let mut w = bookconv::epubzip::EpubWriter::new(std::io::Cursor::new(Vec::new())).unwrap();
        w.put("META-INF/container.xml", br#"<?xml version="1.0"?><container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container"><rootfiles><rootfile full-path="c.opf" media-type="application/oebps-package+xml"/></rootfiles></container>"#).unwrap();
        w.put("c.opf", br#"<?xml version="1.0"?><package xmlns="http://www.idpf.org/2007/opf" version="3.0" unique-identifier="id"><metadata xmlns:dc="http://purl.org/dc/elements/1.1/"><dc:identifier id="id">x</dc:identifier><dc:title>t</dc:title><dc:language>zh</dc:language></metadata><manifest><item id="c1" href="c1.xhtml" media-type="application/xhtml+xml"/></manifest><spine><itemref idref="c1"/></spine></package>"#).unwrap();
        w.put("c1.xhtml", r#"<html xmlns="http://www.w3.org/1999/xhtml"><head><style>p{line-height:1.5em;text-indent:2em}
            h1.j{line-height:1em;padding:0.5em 33%;background-color:#700}
            blockquote.l{margin:7% 0;border:35px solid rgba(0,0,0,0)} p.c{margin:-10px;white-space:nowrap}</style></head><body>
            <h2>一</h2><p>甲甲甲甲甲甲甲甲</p><p>乙乙乙乙乙乙乙乙</p><h1 class="j">章</h1>
            <blockquote class="l"><p class="c">诗</p></blockquote></body></html>"#.as_bytes()).unwrap();
        let epub = w.finish().unwrap().into_inner();
        let (kfx, warnings) = epub_to_kfx(&epub, &Opts { fixed_id: Some(1), ..Default::default() }).unwrap();
        assert!(warnings.is_empty(), "{warnings:?}");
        let c = crate::container::Container::parse(&kfx).unwrap();
        let styles: Vec<Value> = c.entities.iter().filter(|e| e.ty == T_STYLE).map(|e| e.value().unwrap().clone()).collect();
        let all = format!("{styles:?}");
        let n = |v: f64, u: u32| format!("{:?}", num(v, u));
        let has = |k: u32, v: &str| all.contains(&format!("({k}, {v})"));
        // 正文 `line-height:1.5em` 是全书最多的行高 → 1.0；段间 UA 外边距 1em → 0.833333lh（相邻的折叠成一个）
        assert!(has(P_LINE_HEIGHT, &n(1.0, U_LH)) && has(P_MARGIN_TOP, &n(0.833333, U_LH)), "{all}");
        // 标题 UA：h2 1.5 倍字号
        assert!(has(P_FONT_SIZE, &n(1.5, U_FONT_EM)), "{all}");
        // 行高 1em 的 h1 → 0.666667；它的 0.5em 内边距 → 0.625lh；左右 33% 照写百分比
        assert!(has(P_LINE_HEIGHT, &n(0.666667, U_LH)) && has(P_PADDING_TOP, &n(0.625, U_LH)) && has(P_PADDING_LEFT, &n(33.0, U_PERCENT)), "{all}");
        // 没写行高的引文容器 1.2/1.5 = 0.8；7% 上边距（2.24em）→ 2.333333lh；35px 透明边框 → 15.75pt、颜色写「透明」
        assert!(has(P_LINE_HEIGHT, &n(0.8, U_LH)) && has(P_MARGIN_TOP, &n(2.333333, U_LH)), "{all}");
        assert!(has(P_BORDER_COLOR[0], &format!("Symbol({COLOR_TRANSPARENT})")) && has(P_BORDER_WIDTH[0], &n(15.75, U_PT)), "{all}");
        // px 外边距按 1px＝0.45pt：-10px → -0.3125lh；不换行
        assert!(has(P_MARGIN_TOP, &n(-0.3125, U_LH)) && has(P_NOWRAP, "Bool(true)"), "{all}");
    }

    /// 写出器 11 照 Send to Kindle（2026-10-08 对照《绍宋》《绝叫》）：不写 normal 字重、没有背景处不写近黑颜色、600 半粗、
    /// `word-break:break-all`、标题提示、左右边距写包含块的百分比、窄容器里的首行缩进写百分比、缺字体文件的字体写 `default`、
    /// 折叠后的外边距在后一块没有上边距时留在前一块。
    #[test]
    fn styles_rules_of_send_to_kindle() {
        let mut w = bookconv::epubzip::EpubWriter::new(std::io::Cursor::new(Vec::new())).unwrap();
        w.put("META-INF/container.xml", br#"<?xml version="1.0"?><container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container"><rootfiles><rootfile full-path="c.opf" media-type="application/oebps-package+xml"/></rootfiles></container>"#).unwrap();
        w.put("c.opf", br#"<?xml version="1.0"?><package xmlns="http://www.idpf.org/2007/opf" version="3.0" unique-identifier="id"><metadata xmlns:dc="http://purl.org/dc/elements/1.1/"><dc:identifier id="id">x</dc:identifier><dc:title>t</dc:title><dc:language>zh</dc:language></metadata><manifest><item id="c1" href="c1.xhtml" media-type="application/xhtml+xml"/></manifest><spine><itemref idref="c1"/></spine></package>"#).unwrap();
        w.put("c1.xhtml", r#"<html xmlns="http://www.w3.org/1999/xhtml"><head><style>@font-face{font-family:"宋体";src:url(none.ttf)}
            p{font-family:"宋体";color:#111;word-break:break-all;text-indent:2em;margin:0}
            p.g{font-weight:600;margin-left:0.5em} div.box{background-color:#eee;padding:1em} div.t{margin-bottom:1em}</style></head><body>
            <h2>标题</h2><p>正文一正文一</p><p class="g">半粗半粗</p><div class="box"><p>框里的字</p></div><div class="t">目录</div><p>下一段</p></body></html>"#.as_bytes()).unwrap();
        let epub = w.finish().unwrap().into_inner();
        let (kfx, _) = epub_to_kfx(&epub, &Opts { fixed_id: Some(1), ..Default::default() }).unwrap();
        let c = crate::container::Container::parse(&kfx).unwrap();
        let styles: Vec<Value> = c.entities.iter().filter(|e| e.ty == T_STYLE).map(|e| e.value().unwrap().clone()).collect();
        let all = format!("{styles:?}");
        let n = |v: f64, u: u32| format!("{:?}", num(v, u));
        let has = |k: u32, v: &str| all.contains(&format!("({k}, {v})"));
        assert!(!has(P_FONT_WEIGHT, &format!("Symbol({WEIGHT_NORMAL})")), "normal 字重不写: {all}");
        assert!(has(P_FONT_WEIGHT, &format!("Symbol({WEIGHT_SEMIBOLD})")) && has(P_FONT_WEIGHT, &format!("Symbol({WEIGHT_BOLD})")), "{all}");
        // #111 只在有背景色的框里写
        assert_eq!(all.matches(&format!("({P_COLOR}, Int({}))", 0xFF11_1111u32)).count(), 1, "{all}");
        assert!(has(P_WORD_BREAK, &format!("Symbol({WORD_BREAK_ALL})")) && has(P_LAYOUT_HINTS, &format!("List([Symbol({HINT_HEADING})])")), "{all}");
        assert!(has(P_FONT_FAMILY, "String(\"default\")") && !all.contains("宋体"), "{all}");
        // 0.5em 左边距 → 1.5625%；正文缩进 2em；框（左右各 1em 内边距）里的缩进 2/30 → 6.666667%
        assert!(has(P_MARGIN_LEFT, &n(1.5625, U_PERCENT)) && has(P_TEXT_INDENT, &n(2.0, U_EM)) && has(P_TEXT_INDENT, &n(6.666667, U_PERCENT)), "{all}");
        // 「目录」的下边距 1em（0.833333lh）留在自己身上：下一段没有上边距
        assert!(has(P_MARGIN_BOTTOM, &n(0.833333, U_LH)), "{all}");
    }

    /// 写出器 11 第二轮（2026-10-08，逐属性对照 21 本 Send to Kindle 的书）：字号按正文归一、相对根；行高写成长度的按绝对值继承、
    /// 下限 0.6；body 左右边距不写（这一侧有负外边距时照加）；表现属性（`align`、`<font size>`）；整段的行内样式并进段落；
    /// 正文字体写 `default`、备选整串照写；声明了的 normal 字重照写、`<b>` 写 bolder；对比度不到 4.5 的文字颜色调深；块宽度；
    /// `min-height`；链接颜色写 `$576`/`$577`；`<li><p>` 里 `<p>` 自己的样式不丢。
    #[test]
    fn send_to_kindle_rules_round_two() {
        let mut w = bookconv::epubzip::EpubWriter::new(std::io::Cursor::new(Vec::new())).unwrap();
        w.put("META-INF/container.xml", br#"<?xml version="1.0"?><container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container"><rootfiles><rootfile full-path="c.opf" media-type="application/oebps-package+xml"/></rootfiles></container>"#).unwrap();
        w.put("c.opf", br#"<?xml version="1.0"?><package xmlns="http://www.idpf.org/2007/opf" version="3.0" unique-identifier="id"><metadata xmlns:dc="http://purl.org/dc/elements/1.1/"><dc:identifier id="id">x</dc:identifier><dc:title>t</dc:title><dc:language>zh</dc:language></metadata><manifest><item id="c1" href="c1.xhtml" media-type="application/xhtml+xml"/></manifest><spine><itemref idref="c1"/></spine></package>"#).unwrap();
        w.put("c1.xhtml", r#"<html xmlns="http://www.w3.org/1999/xhtml"><head><style>
            body{margin:0 5pt;line-height:130%;font-family:"Body Font",serif} p{font-size:1.25em;margin:0}
            p.big{font-size:1.5em} h2{font-weight:normal;color:#fff;background-color:#f0a200;width:3em;margin:1em auto 1em -2em}
            p.min{min-height:2em} p.pale{color:#f5ac00} a{color:#00c} li p.note{font-size:0.95em;text-indent:-1em}</style></head><body>
            <p>正文正文正文正文正文正文正文正文正文正文正文正文正文正文正文正文正文正文</p><p>正文正文正文正文正文正文正文正文正文正文正文正文</p>
            <p class="big">大字</p><h2>小标题</h2><p align="center"><font size="1">小字</font></p><p><span class="big"><b>整段</b></span></p>
            <p class="min">最小高度</p><p class="pale">浅色字</p><p><a href="c1.xhtml">链接链接</a>后面</p>
            <ul style="list-style:none;margin:0;padding:0"><li><p class="note">注释</p></li></ul></body></html>"#.as_bytes()).unwrap();
        let epub = w.finish().unwrap().into_inner();
        let (kfx, _) = epub_to_kfx(&epub, &Opts { fixed_id: Some(1), ..Default::default() }).unwrap();
        let c = crate::container::Container::parse(&kfx).unwrap();
        let styles: Vec<Value> = c.entities.iter().filter(|e| e.ty == T_STYLE).map(|e| e.value().unwrap().clone()).collect();
        let all = format!("{styles:?}");
        let n = |v: f64, u: u32| format!("{:?}", num(v, u));
        let has = |k: u32, v: &str| all.contains(&format!("({k}, {v})"));
        // 正文 1.25em 归一成 1.0；1.5em → 1.2；`<font size="1">` 0.625em → 0.5；整段的 span 并进段落（字号 1.2、bolder）
        assert!(has(P_FONT_SIZE, &n(1.2, U_FONT_EM)) && has(P_FONT_SIZE, &n(0.5, U_FONT_EM)) && has(P_FONT_WEIGHT, &format!("Symbol({WEIGHT_BOLDER})")), "{all}");
        // body 的 5pt 不写：没有 1.302% 的边距；h2 的 -2em（负的）让左边照加 → 左边距 (-2em*1.5 + 5pt)/32
        assert!(!has(P_MARGIN_RIGHT, &n(1.302083, U_PERCENT)), "{all}");
        assert!(has(P_MARGIN_LEFT, &n((-3.0 + 5.0 / 12.0) / 32.0 * 100.0, U_PERCENT)), "{all}");
        // 正文字体写 default + 备选；h2：normal 照写、白字在橙底上调成 #454545、宽度 3em + 靠左
        assert!(has(P_FONT_FAMILY, "String(\"default,serif\")") && has(P_FONT_WEIGHT, &format!("Symbol({WEIGHT_NORMAL})")), "{all}");
        assert!(has(P_COLOR, &format!("Int({})", 0xFF45_4545u32)) && has(P_WIDTH, &n(3.0, U_EM)) && has(P_BOX_ALIGN, &format!("Symbol({ALIGN_LEFT})")), "{all}");
        assert!(has(P_TEXT_ALIGN, &format!("Symbol({ALIGN_CENTER})")) && has(P_MIN_HEIGHT, &n(2.0, U_EM)), "{all}");
        // 浅色字压暗到 4.5:1；链接颜色写成链接的颜色
        assert!(has(P_COLOR, &format!("Int({})", 0xFF9D_6E00u32)) && has(P_LINK_UNVISITED, &format!("Struct([({P_COLOR}, Int({}))])", 0xFF00_00CCu32)), "{all}");
        // `<li><p class="note">`：p 自己的字号、缩进没丢（0.95/1.25 = 0.76）
        assert!(has(P_FONT_SIZE, &n(0.76, U_FONT_EM)), "{all}");
    }

    /// 固定版式（漫画）：照 Amazon 转的测试漫画写元数据、文档数据和每页的画布版面；从右往左翻写 `$559`。
    #[test]
    fn fixed_layout_odd_shaped_images_keep_aspect() {
        assert_eq!(fixed_image_size((1272, 1696), (1272, 1696)), (1272, 1696));
        assert_eq!(fixed_image_size((954, 1272), (1272, 1696)), (954, 1272), "同比例的小图不放大（Kindle 不放大、会空白）");
        assert_eq!(fixed_image_size((1440, 1920), (1272, 1696)), (1272, 1696), "同比例的大图缩到画布");
        assert_eq!(fixed_image_size((2544, 1696), (1272, 1696)), (1272, 848), "跨页按宽缩进画布");
        assert_eq!(fixed_image_size((300, 200), (1272, 1696)), (300, 200), "小图不放大");
        assert_eq!(fixed_image_size((400, 3392), (1272, 1696)), (200, 1696), "窄长图按高缩");
        assert_eq!(fixed_image_size((0, 0), (1272, 1696)), (1272, 1696));
        assert_eq!(fixed_image_size((300, 200), (0, 0)), (300, 200));
    }

    #[test]
    fn fixed_layout_comic_like_amazon() {
        let mut jpeg = Vec::new();
        image::codecs::jpeg::JpegEncoder::new(&mut jpeg).encode_image(&image::GrayImage::from_pixel(1272, 1696, image::Luma([128]))).unwrap();
        let mut w = bookconv::epubzip::EpubWriter::new(std::io::Cursor::new(Vec::new())).unwrap();
        w.put("META-INF/container.xml", br#"<?xml version="1.0"?><container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container"><rootfiles><rootfile full-path="c.opf" media-type="application/oebps-package+xml"/></rootfiles></container>"#).unwrap();
        w.put("c.opf", br#"<?xml version="1.0"?><package xmlns="http://www.idpf.org/2007/opf" version="3.0" unique-identifier="id"><metadata xmlns:dc="http://purl.org/dc/elements/1.1/"><dc:identifier id="id">x</dc:identifier><dc:title>t</dc:title><dc:language>zh</dc:language><meta name="fixed-layout" content="true"/><meta name="original-resolution" content="1272x1696"/><meta name="orientation-lock" content="portrait"/></metadata><manifest><item id="i" href="i.jpg" media-type="image/jpeg"/><item id="p1" href="p1.xhtml" media-type="application/xhtml+xml"/><item id="p2" href="p2.xhtml" media-type="application/xhtml+xml"/></manifest><spine page-progression-direction="rtl"><itemref idref="p1"/><itemref idref="p2"/></spine></package>"#).unwrap();
        w.put("i.jpg", &jpeg).unwrap();
        for p in ["p1.xhtml", "p2.xhtml"] {
            w.put(p, br#"<html xmlns="http://www.w3.org/1999/xhtml"><body><div><img src="i.jpg" style="width:1272px;height:1696px"/></div></body></html>"#).unwrap();
        }
        let epub = w.finish().unwrap().into_inner();
        let (kfx, _) = epub_to_kfx(&epub, &Opts { fixed_id: Some(1), ..Default::default() }).unwrap();
        let c = crate::container::Container::parse(&kfx).unwrap();
        let dump = |ty: u32| format!("{:?}", c.entities.iter().filter(|e| e.ty == ty).map(|e| e.value().unwrap().clone()).collect::<Vec<_>>());
        let meta = dump(T_METADATA);
        assert!(meta.contains(r#"String("yj_fixed_layout")), (307, Int(1))"#) && meta.contains(r#"String("book_orientation_lock")), (307, String("portrait"))"#), "{meta}");
        let doc = dump(T_DOCUMENT_DATA);
        assert!(doc.contains(&format!("({DOC_FIXED}, Symbol({DOC_FIXED_VALUE}))")) && doc.contains(&format!("({DOC_DIRECTION}, Symbol({DIR_RTL}))")), "{doc}");
        let secs = dump(T_SECTION);
        assert_eq!(secs.matches(&format!("({TMPL_WIDTH}, Int(1272)), ({TMPL_HEIGHT}, Int(1696)), ({FIXED_PAGE_FIT}, Symbol({FIXED_PAGE_FIT_VALUE}))")).count(), 2, "{secs}");
        assert!(dump(T_STYLE).contains(&format!("({P_WIDTH}, F64(1272.0)), ({P_HEIGHT}, F64(1696.0))")), "{}", dump(T_STYLE));
        // 没有实体的 id 排在资源路径的符号后面（否则 Kindle 上比画布小的图有的页空白）
        let syms = c.symbols();
        let max_loc = c.entities.iter().filter(|e| e.ty == T_RESOURCE).filter_map(|e| e.value()?.field(RES_LOCATION)?.as_str().and_then(|l| syms.sid(l))).max().unwrap();
        let max_ent = c.entities.iter().map(|e| e.id).max().unwrap();
        assert!(max_ent < max_loc, "最大实体 id {max_ent} 不能排在资源路径符号 {max_loc} 后面");
    }

    /// 没嵌入的正文字体不写原名（Kindle 会换成别的字体，和表格、阅读器设置都对不上），写 `default`（写出器 11）；嵌入的装饰字体照写。
    #[test]
    fn unembedded_body_font_dropped() {
        let mut w = bookconv::epubzip::EpubWriter::new(std::io::Cursor::new(Vec::new())).unwrap();
        w.put("META-INF/container.xml", br#"<?xml version="1.0"?><container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container"><rootfiles><rootfile full-path="c.opf" media-type="application/oebps-package+xml"/></rootfiles></container>"#).unwrap();
        w.put("c.opf", br#"<?xml version="1.0"?><package xmlns="http://www.idpf.org/2007/opf" version="3.0" unique-identifier="id"><metadata xmlns:dc="http://purl.org/dc/elements/1.1/"><dc:identifier id="id">x</dc:identifier><dc:title>t</dc:title><dc:language>zh</dc:language></metadata><manifest><item id="c1" href="c1.xhtml" media-type="application/xhtml+xml"/></manifest><spine><itemref idref="c1"/></spine></package>"#).unwrap();
        w.put("c1.xhtml", r#"<html xmlns="http://www.w3.org/1999/xhtml"><head><style>p{font-family:"AR MingU30"} h1{font-family:"黑体"}</style></head><body><h1>标题</h1><p>很长很长的正文一</p><p>很长很长的正文二</p><table><tr><td>表格</td></tr></table></body></html>"#.as_bytes()).unwrap();
        let epub = w.finish().unwrap().into_inner();
        let (kfx, _) = epub_to_kfx(&epub, &Opts { fixed_id: Some(1), ..Default::default() }).unwrap();
        let c = crate::container::Container::parse(&kfx).unwrap();
        let styles = format!("{:?}", c.entities.iter().filter(|e| e.ty == T_STYLE).map(|e| e.value().unwrap().clone()).collect::<Vec<_>>());
        assert!(!styles.contains("AR MingU30") && styles.contains("黑体"), "{styles}");
    }

    /// 背景图：同《绍宋》样本，写在包住整页的容器上（`$479` 指向资源、不重复、固定、位置、尺寸）；`url()` 按样式表路径解析。
    #[test]
    fn background_image_like_amazon() {
        let mut png = Vec::new();
        image::GrayImage::from_pixel(4, 4, image::Luma([0])).write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png).unwrap();
        let mut w = bookconv::epubzip::EpubWriter::new(std::io::Cursor::new(Vec::new())).unwrap();
        w.put("META-INF/container.xml", br#"<?xml version="1.0"?><container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container"><rootfiles><rootfile full-path="O/c.opf" media-type="application/oebps-package+xml"/></rootfiles></container>"#).unwrap();
        w.put("O/c.opf", br#"<?xml version="1.0"?><package xmlns="http://www.idpf.org/2007/opf" version="3.0" unique-identifier="id"><metadata xmlns:dc="http://purl.org/dc/elements/1.1/"><dc:identifier id="id">x</dc:identifier><dc:title>t</dc:title><dc:language>zh</dc:language></metadata><manifest><item id="s" href="S/a.css" media-type="text/css"/><item id="b" href="I/bg.png" media-type="image/png"/><item id="c1" href="T/c1.xhtml" media-type="application/xhtml+xml"/></manifest><spine><itemref idref="c1"/></spine></package>"#).unwrap();
        w.put("O/S/a.css", br#"body.j{background:url("../I/bg.png") bottom / cover no-repeat fixed rgba(117, 0, 0, 1)}"#).unwrap();
        w.put("O/I/bg.png", &png).unwrap();
        w.put("O/T/c1.xhtml", br#"<html xmlns="http://www.w3.org/1999/xhtml"><head><link rel="stylesheet" href="../S/a.css"/></head><body class="j"><p>x</p></body></html>"#).unwrap();
        let epub = w.finish().unwrap().into_inner();
        let (kfx, warnings) = epub_to_kfx(&epub, &Opts { fixed_id: Some(1), ..Default::default() }).unwrap();
        assert!(warnings.is_empty(), "{warnings:?}");
        let c = crate::container::Container::parse(&kfx).unwrap();
        let styles = format!("{:?}", c.entities.iter().filter(|e| e.ty == T_STYLE).map(|e| e.value().unwrap().clone()).collect::<Vec<_>>());
        assert!(styles.contains(&format!("({P_BG_IMAGE}, Symbol(")) && styles.contains(&format!("({P_BG_REPEAT}, Symbol({BG_NO_REPEAT}))")) && styles.contains(&format!("({P_BG_ATTACHMENT}, Symbol({BG_FIXED}))")), "{styles}");
        assert!(styles.contains(&format!("({P_BG_SIZE_H}, ")) && styles.contains(&format!("({P_BACKGROUND}, Int({}))", 0xFF750000u32)), "{styles}");
        assert!(c.entities.iter().any(|e| e.ty == T_RESOURCE), "背景图登记成资源");
        // `cover` 的页面背景：容器节点写整页范围 `$645`（同 Send to Kindle《绍宋》卷首语）
        let story = format!("{:?}", c.entities.iter().filter(|e| e.ty == T_STORYLINE).map(|e| e.value().unwrap().clone()).collect::<Vec<_>>());
        assert!(story.contains(&format!("({BG_PAGE_BOUNDS}, Struct(")), "{story}");
    }

    /// SVG 包着的封面图（`<svg><image xlink:href>`，属性在 xlink 命名空间）写成整页图片版面（2026-10-08：以前找不到属性，封面页丢掉）。
    #[test]
    fn svg_wrapped_cover_kept() {
        let mut png = Vec::new();
        image::GrayImage::from_pixel(4, 4, image::Luma([0])).write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png).unwrap();
        let mut w = bookconv::epubzip::EpubWriter::new(std::io::Cursor::new(Vec::new())).unwrap();
        w.put("META-INF/container.xml", br#"<?xml version="1.0"?><container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container"><rootfiles><rootfile full-path="O/c.opf" media-type="application/oebps-package+xml"/></rootfiles></container>"#).unwrap();
        w.put("O/c.opf", br#"<?xml version="1.0"?><package xmlns="http://www.idpf.org/2007/opf" version="3.0" unique-identifier="id"><metadata xmlns:dc="http://purl.org/dc/elements/1.1/"><dc:identifier id="id">x</dc:identifier><dc:title>t</dc:title><dc:language>zh</dc:language></metadata><manifest><item id="b" href="I/c.png" media-type="image/png" properties="cover-image"/><item id="c0" href="T/cover.xhtml" media-type="application/xhtml+xml"/><item id="c1" href="T/c1.xhtml" media-type="application/xhtml+xml"/></manifest><spine><itemref idref="c0"/><itemref idref="c1"/></spine></package>"#).unwrap();
        w.put("O/I/c.png", &png).unwrap();
        w.put("O/T/cover.xhtml", br#"<?xml version="1.0" encoding="utf-8"?><html xmlns="http://www.w3.org/1999/xhtml"><head><title>Cover</title></head><body><div style="text-align:center"><svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink" viewBox="0 0 4 4" width="100%" height="100%"><image width="4" height="4" xlink:href="../I/c.png"/></svg></div></body></html>"#).unwrap();
        w.put("O/T/c1.xhtml", br#"<html xmlns="http://www.w3.org/1999/xhtml"><body><p>x</p></body></html>"#).unwrap();
        let epub = w.finish().unwrap().into_inner();
        let (kfx, warnings) = epub_to_kfx(&epub, &Opts { fixed_id: Some(1), ..Default::default() }).unwrap();
        assert!(warnings.is_empty(), "{warnings:?}");
        let c = crate::container::Container::parse(&kfx).unwrap();
        let story = format!("{:?}", c.entities.iter().filter(|e| e.ty == T_STORYLINE).map(|e| e.value().unwrap().clone()).collect::<Vec<_>>());
        assert!(story.contains(&format!("({NODE_TYPE}, Symbol({NODE_IMAGE}))")), "封面页的图片节点: {story}");
        assert_eq!(c.entities.iter().filter(|e| e.ty == T_SECTION).count(), 2, "封面页成了一个版面");
    }

    /// 图片的百分比宽度写进样式（`$56`，单位百分比，同表格宽度）；别的单位不写（写出器 9）。
    #[test]
    fn image_percent_width() {
        let mut png = Vec::new();
        image::GrayImage::from_pixel(4, 8, image::Luma([0])).write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png).unwrap();
let page = |body: &str| format!(r#"<html xmlns="http://www.w3.org/1999/xhtml"><head><style>img.l{{width:40%}} img.e{{width:10em}}</style></head><body><p>甲</p>{body}<p>乙</p></body></html>"#);
        let styles_of = |body: &str| {
            let mut w2 = bookconv::epubzip::EpubWriter::new(std::io::Cursor::new(Vec::new())).unwrap();
            w2.put("META-INF/container.xml", br#"<?xml version="1.0"?><container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container"><rootfiles><rootfile full-path="c.opf" media-type="application/oebps-package+xml"/></rootfiles></container>"#).unwrap();
            w2.put("c.opf", br#"<?xml version="1.0"?><package xmlns="http://www.idpf.org/2007/opf" version="3.0" unique-identifier="id"><metadata xmlns:dc="http://purl.org/dc/elements/1.1/"><dc:identifier id="id">x</dc:identifier><dc:title>t</dc:title><dc:language>zh</dc:language></metadata><manifest><item id="i" href="i.png" media-type="image/png"/><item id="c1" href="c1.xhtml" media-type="application/xhtml+xml"/></manifest><spine><itemref idref="c1"/></spine></package>"#).unwrap();
            w2.put("i.png", &png).unwrap();
            w2.put("c1.xhtml", page(body).as_bytes()).unwrap();
            let epub = w2.finish().unwrap().into_inner();
            let (kfx, warnings) = epub_to_kfx(&epub, &Opts { fixed_id: Some(1), ..Default::default() }).unwrap();
            assert!(warnings.is_empty(), "{warnings:?}");
            let c = crate::container::Container::parse(&kfx).unwrap();
            format!("{:?}", c.entities.iter().filter(|e| e.ty == T_STYLE).map(|e| e.value().unwrap().clone()).collect::<Vec<_>>())
        };
        let want = |p: f64| format!("({P_WIDTH}, {:?})", num(p, U_PERCENT));
        assert!(styles_of(r#"<div><img src="i.png" style="width:61%"/></div>"#).contains(&want(61.0)), "行内百分比");
        assert!(styles_of(r#"<div><img class="l" src="i.png"/></div>"#).contains(&want(40.0)), "样式表百分比");
        assert!(!styles_of(r#"<div><img class="e" src="i.png"/></div>"#).contains(&format!("({P_WIDTH}, ")), "em 宽度不写");
        assert!(!styles_of(r#"<div><img src="i.png"/></div>"#).contains(&format!("({P_WIDTH}, ")), "没写宽度");
    }

    #[test]
    fn pid_map_matches_sample_shape() {
        // 样本《ABC谋杀案》第 2 个版面：[[0,3090],[1,325],[1,40],[4,333],9,14,6,9,15,[22,0]]
        let order = [(3090, 1), (325, 1), (40, 4), (333, 9), (334, 14), (335, 6), (336, 9), (337, 15), (338, 22)];
        let m = pid_map(&order);
        let expect = Value::List(vec![
            Value::List(vec![Value::Int(0), Value::Int(3090)]),
            Value::List(vec![Value::Int(1), Value::Int(325)]),
            Value::List(vec![Value::Int(1), Value::Int(40)]),
            Value::List(vec![Value::Int(4), Value::Int(333)]),
            Value::Int(9),
            Value::Int(14),
            Value::Int(6),
            Value::Int(9),
            Value::Int(15),
            Value::List(vec![Value::Int(22), Value::Int(0)]),
        ]);
        assert_eq!(Value::List(m), expect);
    }

    #[test]
    fn compress_ids_matches_sample_shape() {
        let ids = vec![3088, 31, 267, 268, 269, 270, 271];
        assert_eq!(
            compress_ids(ids),
            Value::List(vec![Value::Int(31), Value::List(vec![Value::Int(267), Value::Int(5)]), Value::Int(3088)])
        );
    }

    /// 没有文字的元素的 id 也是锚点：挂到文档顺序里下一个块的开头，文末的挂到最后一个块的末尾；图片自己的 id 也算。
    /// 以前丢掉，链接退回文件开头（《福尔摩斯探案全集》目录页「第一册」点了停在目录页开头）。
    #[test]
    fn empty_element_ids_anchor_to_next_block() {
        let mut png = Vec::new();
        image::GrayImage::from_pixel(4, 4, image::Luma([0])).write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png).unwrap();
        let mut w = bookconv::epubzip::EpubWriter::new(std::io::Cursor::new(Vec::new())).unwrap();
        w.put("META-INF/container.xml", br#"<?xml version="1.0"?><container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container"><rootfiles><rootfile full-path="c.opf" media-type="application/oebps-package+xml"/></rootfiles></container>"#).unwrap();
        w.put("c.opf", br#"<?xml version="1.0"?><package xmlns="http://www.idpf.org/2007/opf" version="3.0" unique-identifier="id"><metadata xmlns:dc="http://purl.org/dc/elements/1.1/"><dc:identifier id="id">x</dc:identifier><dc:title>t</dc:title><dc:language>zh</dc:language></metadata><manifest><item id="i" href="i.png" media-type="image/png"/><item id="c1" href="c1.xhtml" media-type="application/xhtml+xml"/></manifest><spine><itemref idref="c1"/></spine></package>"#).unwrap();
        w.put("i.png", &png).unwrap();
        w.put("c1.xhtml", r##"<html xmlns="http://www.w3.org/1999/xhtml"><body><p><a href="#a">1</a><a href="#b">2</a><a href="#c">3</a><a href="#d">4</a></p><p>甲</p><p><span id="a"></span></p><p>乙</p><div id="b"></div><p><img id="c" src="i.png"/></p><p>丙丁</p><span id="d"></span></body></html>"##.as_bytes()).unwrap();
        let epub = w.finish().unwrap().into_inner();
        let (kfx, warnings) = epub_to_kfx(&epub, &Opts { fixed_id: Some(1), ..Default::default() }).unwrap();
        assert!(warnings.is_empty(), "{warnings:?}");
        let c = crate::container::Container::parse(&kfx).unwrap();
        // 节点 id：按文字找文字节点，图片节点按类型找
        let story = c.entities.iter().find(|e| e.ty == T_STORYLINE).unwrap().value().unwrap().field(CHILDREN).unwrap().as_list().unwrap().to_vec();
        let pool = c.entities.iter().find(|e| e.ty == T_TEXT_POOL).unwrap().value().unwrap().field(CHILDREN).unwrap().as_list().unwrap().to_vec();
        let eid_of_text = |t: &str| {
            let idx = pool.iter().position(|v| v.as_str() == Some(t)).unwrap() as i64;
            story.iter().find(|n| n.field(TEXT_REF).and_then(|r| r.field(TEXT_INDEX)).and_then(Value::as_int) == Some(idx)).unwrap().field(EID).unwrap().as_int().unwrap()
        };
        let img = story.iter().find(|n| n.field(NODE_TYPE) == Some(&Value::Symbol(NODE_IMAGE))).unwrap().field(EID).unwrap().as_int().unwrap();
        // 链接按出现顺序配锚点：anchor0..3 → #a #b #c #d
        let syms = c.symbols();
        let target = |name: &str| {
            let sid = syms.sid(name).unwrap();
            let a = c.entities.iter().find(|e| e.ty == T_ANCHOR && e.id == sid).unwrap().value().unwrap().field(ANCHOR_POSITION).unwrap().clone();
            (a.field(EID).unwrap().as_int().unwrap(), a.field(OFFSET).unwrap().as_int().unwrap())
        };
        assert_eq!(target("anchor0"), (eid_of_text("乙"), 0), "空段落里的 span → 下一段开头");
        assert_eq!(target("anchor1"), (img, 0), "空 div → 下一块（图片）");
        assert_eq!(target("anchor2"), (img, 0), "图片自己的 id");
        assert_eq!(target("anchor3"), (eid_of_text("丙丁"), 2), "文末的 → 最后一块末尾");
    }

    /// 一个文档的 body 子节点解析成块（不走整本书）。
    fn body_blocks(body: &str) -> Vec<Block> {
        let html = Html::parse_document(&format!("<html><body>{body}</body></html>"));
        let d = Doc { path: "c.xhtml", sheet: Sheet::default(), lang: None, pending: Default::default(), gap: Default::default() };
        let body = html.select(&scraper::Selector::parse("body").unwrap()).next().unwrap();
        let comp = d.comp(&body, &Computed::root());
        let mut out = Vec::new();
        d.children(body, &comp, &mut out);
        out
    }

    /// 只有空白的段落不出节点，折成下一块上边距多出 0.6 × 本元素行高（Send to Kindle 同样）；只有普通空格的排不出行，不折。
    #[test]
    fn blank_paragraphs_fold_into_next_margin() {
        let mut blocks = body_blocks("<p>甲</p><p>&#160;</p><p>乙</p><p>　</p><p><br/></p><p>丙</p><p> </p><p>丁</p><p style=\"line-height:2\">&#160;</p><p>戊</p><p>&#160;</p>");
        collapse_siblings(&mut blocks);
        apply_gaps(&mut blocks);
        let tops: Vec<(String, f64)> = blocks
            .iter()
            .map(|b| match &b.kind {
                Kind::Text { text, .. } => (text.clone(), (b.margin_top * 1000.0).round() / 1000.0),
                _ => unreachable!(),
            })
            .collect();
        // <p> 上下 1em 折叠成 1em，再加折掉的空段：一个 0.72em（缺省行高 1.2），两个 1.44em，行高 2 的 1.2em；文末的不折
        assert_eq!(tops, [("甲".into(), 1.0), ("乙".into(), 1.72), ("丙".into(), 2.44), ("丁".into(), 1.0), ("戊".into(), 2.2)]);
    }

    type TextRuns = Vec<(String, Vec<(usize, usize, bool)>)>;

    /// 文字块（深度优先）的 (文字, [(起点, 长度, 有没有链接)])；图片记成 `<img>`。
    fn texts_and_runs(blocks: &[Block]) -> TextRuns {
        let mut out = Vec::new();
        for b in blocks {
            match &b.kind {
                Kind::Text { text, runs } => out.push((text.clone(), runs.iter().map(|r| (r.start, r.len, r.link.is_some())).collect())),
                Kind::Image { .. } => out.push(("<img>".to_string(), Vec::new())),
                Kind::Container(c) => out.extend(texts_and_runs(c)),
            }
        }
        out
    }

    /// 行内元素走到一半遇到块或图片（切开文字块）：区间在切开处截断，到新块从 0 接着开（以前按进来时的下标插进新缓冲：越界 panic、或区间错位）。
    #[test]
    fn inline_run_split_by_block_or_image() {
        let got = texts_and_runs(&body_blocks(r##"<div><b>x</b><a href="#n">y<div>z</div>wwwww</a></div>"##));
        assert_eq!(got, [("xy".into(), vec![(0, 1, false), (1, 1, true)]), ("z".into(), vec![]), ("wwwww".into(), vec![(0, 5, true)])]);
        let got = texts_and_runs(&body_blocks(r##"<p><b>x</b>t<a href="#n"><img src="i.png"/>see note one</a></p>"##));
        assert_eq!(got, [("xt".into(), vec![(0, 1, false)]), ("<img>".into(), vec![]), ("see note one".into(), vec![(0, 12, true)])]);
        // 不 panic 时以前区间错位到「www」上
        let got = texts_and_runs(&body_blocks(r##"<div>xx<a href="#n">y<div>z</div>wwwww</a></div>"##));
        assert_eq!(got, [("xxy".into(), vec![(2, 1, true)]), ("z".into(), vec![]), ("wwwww".into(), vec![(0, 5, true)])]);
        // 套着的两层都截断、都接着开，外层排在里层前面
        let got = texts_and_runs(&body_blocks(r##"<div>a<a href="#n">b<b>c<div>d</div>e</b>f</a></div>"##));
        assert_eq!(got, [("abc".into(), vec![(1, 2, true), (2, 1, false)]), ("d".into(), vec![]), ("ef".into(), vec![(0, 2, true), (0, 1, false)])]);
    }

    /// `<ul>`/`<ol>` 里直接放的文字、图片不丢（排成列表里的匿名块，不带符号）。
    #[test]
    fn list_direct_text_and_images_kept() {
        let blocks = body_blocks(r#"<ul>前言<li>甲</li>中间<li>乙</li><img src="i.png"/></ul>"#);
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].ty, Some(NODE_LIST));
        let got: Vec<String> = texts_and_runs(&blocks).into_iter().map(|t| t.0).collect();
        assert_eq!(got, ["前言", "甲", "中间", "乙", "<img>"]);
        let Kind::Container(items) = &blocks[0].kind else { panic!() };
        let types: Vec<Option<u32>> = items.iter().map(|i| i.ty).collect();
        assert_eq!(types, [None, Some(NODE_LIST_ITEM), None, Some(NODE_LIST_ITEM), None]);
    }

    /// `<font size>` 的 `+n`/`-n` 饱和加减，不溢出（以前 `+2147483647` debug 下 panic、release 下回绕成最小字号）。
    #[test]
    fn font_size_attribute_saturates() {
        for (size, want) in [("+2147483647", "3rem"), ("-2147483647", "0.625rem"), ("+1", "1.125rem"), ("7", "3rem")] {
            let html = Html::parse_fragment(&format!(r#"<font size="{size}">x</font>"#));
            let el = html.select(&scraper::Selector::parse("font").unwrap()).next().unwrap();
            let mut decls = HashMap::new();
            presentational_hints(&el, &mut decls);
            assert_eq!(decls.get("font-size").map(String::as_str), Some(want), "{size}");
        }
    }

    /// 行内 `style` 的 `url()` 按文档路径解析：子目录里文档的行内背景图找得到。
    #[test]
    fn inline_style_url_resolved_against_document() {
        let mut png = Vec::new();
        image::GrayImage::from_pixel(4, 4, image::Luma([0])).write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png).unwrap();
        let mut w = bookconv::epubzip::EpubWriter::new(std::io::Cursor::new(Vec::new())).unwrap();
        w.put("META-INF/container.xml", br#"<?xml version="1.0"?><container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container"><rootfiles><rootfile full-path="O/c.opf" media-type="application/oebps-package+xml"/></rootfiles></container>"#).unwrap();
        w.put("O/c.opf", br#"<?xml version="1.0"?><package xmlns="http://www.idpf.org/2007/opf" version="3.0" unique-identifier="id"><metadata xmlns:dc="http://purl.org/dc/elements/1.1/"><dc:identifier id="id">x</dc:identifier><dc:title>t</dc:title><dc:language>zh</dc:language><dc:date>2019</dc:date></metadata><manifest><item id="b" href="I/bg.png" media-type="image/png"/><item id="c1" href="T/c1.xhtml" media-type="application/xhtml+xml"/></manifest><spine><itemref idref="c1"/></spine></package>"#).unwrap();
        w.put("O/I/bg.png", &png).unwrap();
        w.put("O/T/c1.xhtml", br#"<html xmlns="http://www.w3.org/1999/xhtml"><body><div style="background-image:url('../I/bg.png')"><p>x</p></div></body></html>"#).unwrap();
        let epub = w.finish().unwrap().into_inner();
        let (kfx, warnings) = epub_to_kfx(&epub, &Opts { fixed_id: Some(1), ..Default::default() }).unwrap();
        assert!(warnings.is_empty(), "{warnings:?}");
        let c = crate::container::Container::parse(&kfx).unwrap();
        let styles = format!("{:?}", c.entities.iter().filter(|e| e.ty == T_STYLE).map(|e| e.value().unwrap().clone()).collect::<Vec<_>>());
        assert!(styles.contains(&format!("({P_BG_IMAGE}, Symbol(")) && c.entities.iter().any(|e| e.ty == T_RESOURCE), "{styles}");
        // 只写到年份的日期补成 1 月 1 日
        let meta = format!("{:?}", c.entities.iter().find(|e| e.ty == T_METADATA).unwrap().value().unwrap());
        assert!(meta.contains(r#"String("issue_date")), (307, String("2019-01-01"))"#), "{meta}");
    }

    /// 元数据 `issue_date`：能用的部分留下、按 `YYYY-MM-DD` 补齐（以前不足 10 个字符的整个换成 2000-01-01）。
    #[test]
    fn issue_date_keeps_usable_part() {
        for (d, want) in [
            ("2019-05-03T08:00:00Z", "2019-05-03"),
            ("2019-05-03", "2019-05-03"),
            ("2019-05", "2019-05-01"),
            ("2019", "2019-01-01"),
            (" 2019 ", "2019-01-01"),
            ("2019-5-3", "2019-01-01"),
            ("", "2000-01-01"),
            ("May 2019", "2000-01-01"),
            ("二〇一九年", "2000-01-01"),
        ] {
            assert_eq!(issue_date(d), want, "{d:?}");
        }
    }

    /// 固定版式的页图片取不到：同没有内容的文件，目录项改指下一页（以前没登记，目录项找不到位置）。
    #[test]
    fn fixed_layout_missing_image_page_registered_as_empty() {
        let mut jpeg = Vec::new();
        image::codecs::jpeg::JpegEncoder::new(&mut jpeg).encode_image(&image::GrayImage::from_pixel(8, 8, image::Luma([128]))).unwrap();
        let mut w = bookconv::epubzip::EpubWriter::new(std::io::Cursor::new(Vec::new())).unwrap();
        w.put("META-INF/container.xml", br#"<?xml version="1.0"?><container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container"><rootfiles><rootfile full-path="c.opf" media-type="application/oebps-package+xml"/></rootfiles></container>"#).unwrap();
        w.put("c.opf", br#"<?xml version="1.0"?><package xmlns="http://www.idpf.org/2007/opf" version="3.0" unique-identifier="id"><metadata xmlns:dc="http://purl.org/dc/elements/1.1/"><dc:identifier id="id">x</dc:identifier><dc:title>t</dc:title><dc:language>zh</dc:language><meta name="fixed-layout" content="true"/><meta name="original-resolution" content="8x8"/></metadata><manifest><item id="nav" href="nav.xhtml" media-type="application/xhtml+xml" properties="nav"/><item id="i" href="i.jpg" media-type="image/jpeg"/><item id="p1" href="p1.xhtml" media-type="application/xhtml+xml"/><item id="p2" href="p2.xhtml" media-type="application/xhtml+xml"/></manifest><spine><itemref idref="p1"/><itemref idref="p2"/></spine></package>"#).unwrap();
        w.put("nav.xhtml", br#"<html xmlns="http://www.w3.org/1999/xhtml" xmlns:epub="http://www.idpf.org/2007/ops"><body><nav epub:type="toc"><ol><li><a href="p1.xhtml">1</a></li><li><a href="p2.xhtml">2</a></li></ol></nav></body></html>"#).unwrap();
        w.put("i.jpg", &jpeg).unwrap();
        w.put("p1.xhtml", br#"<html xmlns="http://www.w3.org/1999/xhtml"><body><div><img src="missing.jpg"/></div></body></html>"#).unwrap();
        w.put("p2.xhtml", br#"<html xmlns="http://www.w3.org/1999/xhtml"><body><div><img src="i.jpg"/></div></body></html>"#).unwrap();
        let epub = w.finish().unwrap().into_inner();
        let (kfx, warnings) = epub_to_kfx(&epub, &Opts { fixed_id: Some(1), ..Default::default() }).unwrap();
        assert!(!warnings.iter().any(|w| w.contains("目录项找不到位置")), "{warnings:?}");
        let c = crate::container::Container::parse(&kfx).unwrap();
        let toc = c.entities.iter().find(|e| e.ty == T_NAV_CONTAINER && e.value().unwrap().field(NAV_TYPE) == Some(&Value::Symbol(NAV_TYPE_TOC))).unwrap();
        let targets: Vec<Value> = toc.value().unwrap().field(NAV_ENTRIES).unwrap().as_list().unwrap().iter().map(|e| e.field(NAV_TARGET).unwrap().clone()).collect();
        assert_eq!(targets.len(), 2);
        assert_eq!(targets[0], targets[1], "取不到图的第 1 页指到第 2 页");
    }

    /// 正文字体字数打平时结果确定（取字体名小的，没写字体的排最前），和 HashMap 的遍历顺序无关。
    #[test]
    fn body_font_tie_is_deterministic() {
        let block = |font: Option<&str>, text: &str| {
            let mut c = Computed::root();
            c.font_family = font.map(str::to_string);
            Block::anonymous(Kind::Text { text: text.into(), runs: Vec::new() }, c, Vec::new())
        };
        for _ in 0..20 {
            let mut docs: Vec<ParsedDoc> = vec![(0, "a", None, vec![block(Some("Zed"), "甲乙"), block(Some("Alpha"), "丙丁"), block(Some("Mid"), "戊")])];
            assert_eq!(body_font_to_drop(&mut docs), Some("alpha".to_string()));
            let mut docs: Vec<ParsedDoc> = vec![(0, "a", None, vec![block(Some("Zed"), "甲乙"), block(None, "丙丁")])];
            assert_eq!(body_font_to_drop(&mut docs), None);
        }
    }

    #[test]
    fn non_finite_lengths_written_as_zero() {
        for v in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            assert_eq!(num(v, U_EM), num(0.0, U_EM));
        }
        assert_eq!(num(0.8333333333, U_LH), Value::Struct(vec![(VALUE, Value::F64(0.833333)), (UNIT, Value::Symbol(U_LH))]));
    }

    #[test]
    fn margin_collapse() {
        assert_eq!(collapse(1.0, 1.5), 1.5);
        assert_eq!(collapse(-1.0, 2.0), 1.0);
        assert_eq!(collapse(-1.0, -2.0), -2.0);
    }
}
