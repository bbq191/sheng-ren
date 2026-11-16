//! 计算值：一个元素的层叠结果（含本阅读器的行高、字号怪癖）和图片的 CSS 宽高。

use super::*;

impl Doc<'_> {
    pub(in crate::write) fn comp(&self, el: &ElementRef, parent: &Computed) -> Computed {
        let mut decls = self.sheet.cascade(el, self.path);
        crate::css::presentational_hints(el, &mut decls);
        let line = crate::css::device_line();
        if line != Default::default() {
            device_line_height(el, &mut decls, line);
        }
        let mut c = Computed::derive(parent, &decls, el.value().name());
        if let Some(l) = lang_attr(el) {
            c.lang = Some(l.to_string());
        }
        c
    }

    /// 图片的 CSS 宽、高（见 [`InlineImage::width`]）：没写的用 HTML 属性（纯数字的像素；百分比的宽度层叠时已经当 CSS）。
    pub(super) fn image_size(&self, el: &ElementRef) -> (Option<Len>, Option<Len>) {
        let mut decls = self.sheet.cascade(el, self.path);
        crate::css::presentational_hints(el, &mut decls);
        let get = |k: &str| {
            let css = decls
                .get(k)
                .filter(|v| v.trim() != "auto")
                .and_then(|v| crate::css::parse_box_len(v));
            css.or_else(|| {
                el.value()
                    .attr(k)
                    .and_then(|v| v.trim().parse::<f64>().ok())
                    .filter(|n| *n > 0.0)
                    .map(|n| Len::Pt(n * 0.45))
            })
        };
        (get("width"), get("height"))
    }
}
