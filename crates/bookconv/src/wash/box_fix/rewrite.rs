//! 一章编辑的解算与改写（拆分自 `box_fix.rs`）：`resolve`、`rewrite`，以及页首横线、窄块、列表缩进等共用判断。

use super::line::{Line, FRAME_CLOSE};
use super::{dir_of, frame_open, html, skipped, typeset, Entry, Plan};
use crate::cascade::{BorderWidth, Len};
use crate::kindle_model as km;
use crate::reader_model::ReaderModel;

/// 一处编辑的新内容（[`resolve`]）：定好了的，或者要等编号的（下标指 [`Resolved`] 里登记的第几个）。
enum Out {
    Done(String),
    /// 原标签改好了、还差加类：`eink-b<类号>`。
    Class(String, usize),
    /// 插一张横线图。
    Line(usize),
    /// 包框表格的开头（单元格的行内 `style`、窄块的表格宽和靠哪边，见 [`frame_open`]）。
    Frame(usize, String, Option<(String, &'static str)>),
}

/// 一章算好的编辑和编好的类号、横线图号（[`apply`] 的第三段）。
pub(super) type Job = (Resolved, Vec<usize>, Vec<usize>);

/// 一章算好的编辑：要登记的类、横线图（按以前遇到的先后），编辑 (起, 止, 新内容)（按以前的先后：加声明的标签、横线图、框、单元格）。
pub(super) struct Resolved {
    pub(super) classes: Vec<(String, String)>,
    pub(super) lines: Vec<Line>,
    edits: Vec<(usize, usize, Out)>,
}

/// 照 `p` 算一章的编辑（只读）。不是 UTF-8、没有 `<body>` 的章不动（`None`）。登记和编辑的先后、哪些情况跳过都同以前逐章串行时：
/// 横线图在找闭合标签之前登记（找不到闭合标签的不插图，但占一个编号），框在找到闭合标签之后登记。
pub(super) fn resolve(e: &Entry, p: &Plan) -> Option<Resolved> {
    let text = std::str::from_utf8(&e.data).ok()?;
    let (bs, be) = html::body_range(text)?;
    let starts: Vec<html::Tag> = html::tags_in(text, bs, be)
        .filter(|t| t.is_start())
        .collect();
    let mut r = Resolved {
        classes: Vec::new(),
        lines: Vec::new(),
        edits: Vec::new(),
    };
    let empty = Vec::new();
    let tagged: std::collections::BTreeSet<usize> =
        p.decls.keys().chain(p.inline.keys()).copied().collect();
    for n in tagged {
        let ds = p.decls.get(&n).unwrap_or(&empty);
        let Some(t) = starts.get(n) else { continue };
        let tag = t.name.to_ascii_lowercase();
        let mut raw = text[t.start..t.end].to_string();
        if let (Some(pre), Some(style)) = (
            p.strip.get(&n),
            html::attr_value(&raw, "style").map(|s| crate::util::xml_unescape(s).into_owned()),
        ) {
            let kept: String = html::css_decls(&style)
                .iter()
                .filter(|d| {
                    !pre.iter()
                        .any(|p| d.prop.to_ascii_lowercase().starts_with(p))
                })
                .map(|d| d.raw)
                .collect();
            raw = if kept.trim().is_empty() {
                html::remove_attr(&raw, "style")
            } else {
                html::set_attr(&raw, "style", &crate::util::xml_escape(kept.trim()))
            };
        }
        if let Some(adds) = p.inline.get(&n) {
            let old = html::attr_value(&raw, "style")
                .map(|s| crate::util::xml_unescape(s).into_owned())
                .unwrap_or_default();
            let old = old.trim_end();
            let old = old.strip_suffix(';').unwrap_or(old);
            let add: String = adds.iter().map(|d| format!("{d};")).collect();
            let style = if old.trim().is_empty() {
                add
            } else {
                format!("{old};{add}")
            };
            raw = html::set_attr(&raw, "style", &crate::util::xml_escape(&style));
        }
        let out = if ds.is_empty() {
            Out::Done(raw)
        } else {
            r.classes
                .push((tag, ds.iter().map(|d| format!("{d};")).collect::<String>()));
            Out::Class(raw, r.classes.len() - 1)
        };
        r.edits.push((t.start, t.end, out));
    }
    for &(n, line) in &p.lines {
        let Some(t) = starts.get(n) else { continue };
        r.lines.push(line);
        let pos = if line.top {
            t.end
        } else {
            let name = t.name.to_ascii_lowercase();
            let Some(close) = html::find_matching_close(text, t.end, &name) else {
                continue;
            };
            close.start
        };
        r.edits.push((pos, pos, Out::Line(r.lines.len() - 1)));
    }
    for (n, body, ds, table) in &p.frames {
        let Some(t) = starts.get(*n) else { continue };
        let Some(close) = html::find_matching_close(text, t.end, &t.name.to_ascii_lowercase())
        else {
            continue;
        };
        r.classes.push(("div".to_string(), body.clone()));
        r.edits.push((
            t.start,
            t.start,
            Out::Frame(r.classes.len() - 1, ds.clone(), table.clone()),
        ));
        r.edits
            .push((close.end, close.end, Out::Done(FRAME_CLOSE.to_string())));
    }
    for (n, add) in &p.cells {
        let Some(t) = starts.get(*n) else { continue };
        let mut raw = text[t.start..t.end].to_string();
        match p.cell_widths.iter().find(|(j, _)| j == n) {
            Some((_, Some(v))) => raw = html::set_attr(&raw, "width", v),
            Some((_, None)) => raw = html::remove_attr(&raw, "width"),
            None => {}
        }
        if t.name.eq_ignore_ascii_case("table") {
            raw = html::set_attr(&raw, "cellspacing", "0");
        }
        if !add.is_empty() {
            let style = match html::attr_value(&raw, "style") {
                Some(s) if !s.trim().is_empty() => format!("{};{add}", html::style_base(s)),
                _ => add.clone(),
            };
            raw = html::set_attr(&raw, "style", &style);
        }
        r.edits.push((t.start, t.end, Out::Done(raw)));
    }
    Some(r)
}

/// 照算好的编辑改写一章：`ks`、`ls` 是这章登记的类、横线图编好的全书序号（从 0 起），`at` 把文件名换成 OPF 同目录的书内路径。
/// 返回改动数。
pub(super) fn rewrite(
    e: &mut Entry,
    r: &Resolved,
    ks: &[usize],
    ls: &[usize],
    at: &(impl Fn(&str) -> String + Sync),
    css_path: &str,
) -> usize {
    let Ok(text) = std::str::from_utf8(&e.data) else {
        return 0;
    };
    let dir = dir_of(&e.name);
    let mut edits: Vec<(usize, usize, String)> = r
        .edits
        .iter()
        .map(|(s, t, out)| {
            let new = match out {
                Out::Done(v) => v.clone(),
                Out::Class(raw, c) => html::add_class(raw, &format!("eink-b{}", ks[*c] + 1)),
                Out::Line(l) => {
                    let k = ls[*l];
                    let href =
                        crate::epubzip::href_to(dir, &at(&format!("eink-rule-{}.png", k + 1)), "");
                    format!(
                        r#"<img class="eink-sp eink-r{}" src="{href}" alt=""/>"#,
                        k + 1
                    )
                }
                Out::Frame(c, ds, table) => frame_open(ks[*c] + 1, ds, table),
            };
            (*s, *t, new)
        })
        .collect();
    edits.sort_by_key(|x| (x.0, x.1));
    let n = edits.len();
    let out = html::apply_edits(text, edits);
    let href = crate::epubzip::href_to(dir, css_path, "");
    e.data = typeset::inject_css_link(&out, &href).into_bytes();
    n
}

/// 包进单格表格的列表（《雪国》注释列表 `ol{border:1px dashed}`）：Kindle 上文字离框左边约 10.3% 个内容区宽（2026-10-15 Kindle 弹窗
/// 截图：1104px 宽的内容区里 114px，序号在 60px 处），xochitl 自己的列表缩进只有约 25px（同日 Move 排出的 PDF，842px 的内容区里
/// 文字在 25px、序号在 7px 处）：单元格左内边距补上差的那截。
pub(super) fn list_indent_extra_px(area_w: f64) -> f64 {
    (0.103 * area_w - 25.0).max(0.0).round()
}

/// 页首的块上内边距超过这么多 rem（Kindle 正文一行 1.2 rem）的，上横线图不插（见 [`page_top_rule_px`]）。
const MAX_TOP_PAD_REM: f64 = 3.0;

/// 定宽块占能用宽的这么多以内算窄（卷名竖排这类），包表格写百分比宽。
pub(super) const NARROW_SHARE: f64 = 0.3;

/// 上内边距大到是拿来推字的（[`MAX_TOP_PAD_REM`]）：页首的块上横线图照插、不从垫图里扣。
pub(super) fn big_top_pad(c: &crate::cascade::Computed, reader: &ReaderModel) -> bool {
    !reader.padding && km::vertical_rem(c.padding[0], c.font_size) > MAX_TOP_PAD_REM
}

/// 页首（文件里前面还没有内容）的块插的上横线图有多高（这台的屏幕像素：线粗，加上不认内边距时连着画的上内边距）；不插的 `None`。
/// 页首的块由页首垫图（`top_spacer.rs`）推到 Kindle 上字的位置，垫图要减去它，两边共用这一个判断（KINDLE_RULES 35：以前页首一律
/// 不插上横线，《人生海海》「第一部」、《平凡的世界》卷名上的线 Move 上没有，维护者要修）。包进单格表格画框的块（左右有框）不在这里。
pub(in crate::wash) fn page_top_rule_px(
    c: &crate::cascade::Computed,
    tag: &str,
    reader: &ReaderModel,
    v_rem_px: f64,
) -> Option<f64> {
    if reader.borders || skipped(tag) {
        return None;
    }
    let seen = |side: usize| {
        c.border[side].as_ref().filter(|b| {
            b.width != BorderWidth::Pt(0.0)
                && b.width != BorderWidth::Em(0.0)
                && b.color.is_none_or(|c| c >> 24 != 0)
        })
    };
    let b = seen(0)?;
    let framed = (seen(1).is_some() || seen(3).is_some())
        && c.bg_image.is_none()
        && (c.width.is_some() || !(c.margin_auto[1] || c.margin_auto[3]));
    // 窄的定宽块会包进表格（见 [`NARROW_SHARE`]）：这里没有页宽，按 Kindle 一行 32 em 估（《平凡的世界》卷名 `width:100%` 不算窄）
    let narrow = match c.width {
        Some(Len::Percent(p)) => p < NARROW_SHARE * 100.0,
        Some(Len::Em(n)) => n * c.font_size < NARROW_SHARE * km::PAGE_WIDTH_EM,
        Some(Len::Pt(n)) => n / km::PT_PER_EM * c.font_size < NARROW_SHARE * km::PAGE_WIDTH_EM,
        None => false,
    };
    if framed || narrow {
        return None;
    }
    let border_rem = match b.width {
        BorderWidth::Pt(n) => n / km::PT_PER_EM,
        BorderWidth::Em(n) => n * c.font_size,
    };
    let pad_rem = if reader.padding {
        0.0
    } else {
        km::vertical_rem(c.padding[0], c.font_size).max(0.0)
    };
    // 上内边距大到像是拿来把页首的字往下推的（《梁羽生》卷名 `padding-top:6em` 配 3 倍字号、负上外边距，18 rem）：这段距离已经算在垫图里
    // （垫图照 Kindle 字顶的位置），横线图只画线、不连着内边距，不然距离算两遍，垫图 + xochitl 标题前自带的空 + 横线图的透明段超过
    // 一页，卷名被推到下一页、前面多出一页空背景（2026-10-15 发现，33 版就有）
    let pad_rem = if pad_rem > MAX_TOP_PAD_REM {
        0.0
    } else {
        pad_rem
    };
    Some((border_rem * v_rem_px).max(1.0) + pad_rem * v_rem_px)
}
