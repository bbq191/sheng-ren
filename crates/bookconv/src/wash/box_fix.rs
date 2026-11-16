//! 内边距、边框、标题的上下外边距照 Kindle（不认内边距、不画边框、不认标题外边距的阅读器：Move，profile `padding`、`borders`、
//! `heading_margins` 写 `false`；只修复、照 Send to Kindle 统一的文字书；2026-10-13 要修，边框维护者选「上/下边框插横线图」）。
//!
//! xochitl（2026-10-13 怪癖测试书 `tools/quirks/probe.py`）：内边距上下左右都不认（Q06、Q12、Q39），块的背景色照画（Q68）；边框一概
//! 不画（Q07、Q40），图照画（横线图放在段落后、标题里 `display:block` 的都画满宽，Q69、Q70）；h1–h6 的上下外边距都不认（Q05、Q65、
//! Q67），标题前后自带一段空，和前一块的下外边距、后一块的上外边距折叠（那两个认，Q66、Q74）；`margin-left` 加负的首行缩进排得出
//! 悬挂缩进（Q71）。所以（Kindle 上的值按 [`crate::kindle_model`] 从现在的 CSS 算，写成 `eink-box.css` 里的类规则（`html body 标签.类`，不用 `!important`：掌阅带了反而输；改到的元素行内的外边距删掉、并进类规则，行内的压过样式表）；
//! 竖直的写 em，之后照竖直间距换算）：
//!
//! - **px、% 写的左右外边距**（profile `px_margins`、`percent_margins` 写 `false`：掌阅、Move 的 px 是屏幕像素，Kindle 写出器按
//!   0.45pt 一 px；Move 的 % 当成 px）折成 em：px 按 0.45pt、12pt 一根 em，% 按这台的内容区宽（KINDLE_RULES 24）；认内边距的（掌阅）
//!   px 写的左右内边距（0.45pt）、首行缩进（同写出器 0.75pt）也折成 em（KINDLE_RULES 25；Move 首行缩进的 px 没量过，不动）；
//! - **左右内边距**并进同侧的外边距（悬挂缩进 `text-indent:-2em;padding-left:2em` 跟着对了）；带背景色、背景图的块（并了色块就
//!   缩到字上）、表格、列表、`margin:auto` 那一侧不动；
//! - **上下边框**（看得见的）：块里最前（上边框）、最后（下边框）插一张横线图 `eink-rule-N.png`（同色、同粗、实线 / 点线 / 虚线照
//!   画，连着这一侧的内边距画成透明的一截，宽 100%、高按比例）；文件里前面还没有内容的块不插上边框（页首的位置由垫图管，再插会多
//!   出来）；
//! - **左右有框的块**（全框、单边竖线）包进单格表格，框写成单元格行内的边框（xochitl 只画表格、单元格行内写的边框，见
//!   [`frame_open`]；KINDLE_RULES 27，维护者选的）；定了宽度、居中、有背景图的照插横线图；**书里的表格**单元格的边框写进行内；
//! - **上下内边距、标题的上下外边距**：同一层相邻的两块，Kindle 上两块之间的距离 = 外边距折叠 + 前一块的下内边距 + 后一块的上内
//!   边距（横线图里画了的不算），写到 Move 认的那一侧：后一块不是标题就写它的上外边距（不小于前一块的下外边距，折叠以后就是它），
//!   后一块是标题就写前一块的下外边距；两块都是标题的、有子块又有内边距的、空块（只有 id 的分页 div）、我们垫的页首空白图、
//!   页首的标题（文件里前面还没有内容，位置由垫图管）不动。
use super::*;
use crate::cascade::MediaEnv;
use crate::reader_model::ReaderModel;
use std::collections::BTreeMap;
// `box_fix/tests.rs` 用这两个名字（它 `use super::*`）。
#[cfg(test)]
use crate::kindle_model::{self as km, Chapter};

mod line;
mod plan;
mod rewrite;
#[cfg(test)]
mod tests;

use self::line::{Frame, Line, LineKey};
use self::line::frame_open;
use self::plan::plan;
use self::rewrite::{resolve, rewrite, Job, Resolved};
pub(super) use self::rewrite::page_top_rule_px;

const CSS_NAME: &str = "eink-box.css";

/// 这台阅读器的尺寸。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BoxGeometry {
    /// 内容区宽（屏幕像素）。
    pub area_w: f64,
    /// 缺省字号下 1em 的屏幕像素。
    pub em_px: f64,
    /// Kindle 上竖直一 rem 在这台上的屏幕像素（同竖直间距换算：Kindle 一 rem 的像素 × 两台内容区高之比）。
    pub v_rem_px: f64,
    /// 左右边距的 em 最后要乘的系数（profile `horizontal_scale`：这台一行的 em 数 ÷ 32，Move 约 0.7；不换算的 1）。这里折出来的左右
    /// 长度先按 Kindle 的口径写（一 em 在页宽里占 1/32），乘完正好是这台上占页宽的比例。
    pub h_scale: f64,
}

/// 一章要改的：元素（`<body>` 里第几个开始标签）→ 要加的声明；横线图 (第几个开始标签, 图)。
#[derive(Default)]
struct Plan {
    decls: BTreeMap<usize, Vec<String>>,
    lines: Vec<(usize, Line)>,
    /// 行内 `style` 里要删掉的属性（前缀：`margin`、`padding`）：行内的压过样式表（Move 连 `!important` 都压过，2026-10-13 探针
    /// Q48 修后），改了外边距的元素把行内的外边距（不认内边距的阅读器连内边距）删掉、值并进类规则。
    strip: BTreeMap<usize, Vec<&'static str>>,
    /// 左右有看得见的边框的块：包进单格表格画框（元素, 外面那层 div 的类规则（左右外边距）, 单元格的行内 `style`），见 [`frame_open`]。
    frames: Vec<Frame>,
    /// 表格、单元格：(元素, 行内 `style` 末尾接的边框)；表格另加 `cellspacing="0"`。
    cells: Vec<(usize, String)>,
    /// 单元格的 `width` 属性：(元素, 新值；`None`＝删掉)，见 [`ReaderModel::cell_width`]。
    cell_widths: Vec<(usize, Option<String>)>,
    /// 接在行内 `style` 末尾的声明（不能写进外链类规则的：xochitl 的外链样式表里一条不带单位的 `line-height` 会让排在它后面的规则
    /// 全部失效，2026-10-15《人生海海》实测）。
    inline: BTreeMap<usize, Vec<String>>,
}

pub(super) fn apply(
    entries: &mut Vec<Entry>,
    rep: &mut WashReport,
    media: Option<&MediaEnv>,
    reader: &ReaderModel,
    g: BoxGeometry,
) {
    let (chapters, plans) =
        super::scan_chapters(entries, media, |e, css| plan(e, media, css, reader, g));
    if plans.iter().all(|p| {
        p.decls.is_empty()
            && p.lines.is_empty()
            && p.frames.is_empty()
            && p.cells.is_empty()
            && p.cell_widths.is_empty()
            && p.inline.is_empty()
    }) {
        return;
    }
    let opf_idx = find_opf(entries);
    let opf_dir = opf_idx
        .map(|i| dir_of(&entries[i].name).to_string())
        .unwrap_or_default();
    let at = |n: &str| {
        if opf_dir.is_empty() {
            n.to_string()
        } else {
            format!("{opf_dir}/{n}")
        }
    };
    let css_path = at(CSS_NAME);
    // 三段（2026-10-15 改，以前写回整段串行）：① 各章并行算好编辑（[`resolve`]，只读），类、横线图先不编号，按以前遇到的先后
    // 记下要登记的；② 串行按「章的顺序、章里登记的先后」编号——和以前逐章做时的先后一样，产物逐字节不变；③ 各章并行改写（[`rewrite`]）。
    let busy = |p: &Plan| {
        !(p.decls.is_empty()
            && p.lines.is_empty()
            && p.frames.is_empty()
            && p.cells.is_empty()
            && p.cell_widths.is_empty()
            && p.inline.is_empty())
    };
    let todo: Vec<(usize, &Plan)> = chapters
        .iter()
        .zip(&plans)
        .filter(|(_, p)| busy(p))
        .map(|(&i, p)| (i, p))
        .collect();
    let resolved: Vec<Option<Resolved>> =
        crate::util::par_map(&todo, |&(i, p)| resolve(&entries[i], p));
    // (标签, 声明) → 类序号；横线图 → 序号
    let mut classes: Vec<(String, String)> = Vec::new();
    let mut images: Vec<(LineKey, Line)> = Vec::new();
    let mut jobs: HashMap<usize, Job> = HashMap::new();
    for (&(i, _), r) in todo.iter().zip(resolved) {
        let Some(r) = r else { continue };
        let ks: Vec<usize> = r
            .classes
            .iter()
            .map(|key| {
                classes.iter().position(|c| c == key).unwrap_or_else(|| {
                    classes.push(key.clone());
                    classes.len() - 1
                })
            })
            .collect();
        let ls: Vec<usize> = r
            .lines
            .iter()
            .map(|line| {
                images
                    .iter()
                    .position(|(key, _)| *key == line.key())
                    .unwrap_or_else(|| {
                        images.push((line.key(), *line));
                        images.len() - 1
                    })
            })
            .collect();
        jobs.insert(i, (r, ks, ls));
    }
    let mut work: Vec<(&mut Entry, Job)> = entries
        .iter_mut()
        .enumerate()
        .filter_map(|(i, e)| jobs.remove(&i).map(|j| (e, j)))
        .collect();
    rep.kindle_rule_edits += crate::util::par_map_mut(&mut work, |(e, (r, ks, ls))| {
        rewrite(e, r, ks, ls, &at, &css_path)
    })
    .into_iter()
    .sum::<usize>();
    // 不写 `!important`（掌阅带了反而输给前面的普通声明）：选择器 `html body 标签.类` 压过书里的规则，这份样式表链在最后
    let mut sheet = String::from("/* 内边距、边框、标题外边距、左右外边距照 Kindle（sheng-ren kindle_rules，见 wash/box_fix.rs） */\n");
    for (k, (tag, body)) in classes.iter().enumerate() {
        sheet.push_str(&format!("html body {tag}.eink-b{}{{{body}}}\n", k + 1));
    }
    for k in 0..images.len() {
        sheet.push_str(&format!(
            "html body img.eink-r{}{{display:block;width:100%;height:auto;margin:0;padding:0;}}\n",
            k + 1
        ));
    }
    add_css_entry(
        entries,
        find_opf(entries),
        &css_path,
        &sheet,
        "eink-box-css",
    );
    for (k, (_, line)) in images.iter().enumerate() {
        add_entry(
            entries,
            opf_idx,
            &at(&format!("eink-rule-{}.png", k + 1)),
            line.png(),
            &format!("eink-rule-{}", k + 1),
            "image/png",
        );
    }
}

/// 表格、列表、分隔线、图这些不动（外边距、内边距的意思不一样，横线也画不对）。
fn skipped(tag: &str) -> bool {
    matches!(
        tag,
        "td" | "th"
            | "tr"
            | "table"
            | "thead"
            | "tbody"
            | "tfoot"
            | "caption"
            | "li"
            | "ul"
            | "ol"
            | "dl"
            | "dt"
            | "dd"
            | "hr"
            | "img"
            | "svg"
            | "body"
            | "html"
    )
}

fn heading(tag: &str) -> bool {
    matches!(tag, "h1" | "h2" | "h3" | "h4" | "h5" | "h6")
}

fn fmt(v: f64) -> String {
    let s = format!("{:.4}", v);
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s == "-0" {
        "0".to_string()
    } else {
        s.to_string()
    }
}


