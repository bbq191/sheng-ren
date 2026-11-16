//! 一个文档用到的样式表与层叠主体。

use crate::media::{media_ok, MediaEnv};
use crate::rules::{ElementKeys, Rules};
use crate::text::parse_decls;
use crate::{resolve_urls, Decl, Decls, Origins};
use scraper::{ElementRef, Selector};
use std::sync::Arc;

/// 一个文档用到的样式表：各份 [`Rules`]、它们第一条规则的全局序号（后出现的规则优先）、从哪来。
#[derive(Default)]
pub struct Sheet {
    pub(crate) parts: Vec<(Arc<Rules>, usize, Origin)>,
}

/// 一条声明从哪来（有的阅读器按来源区别对待：xochitl 行内 `style`、`<style>` 里的 em 只按 23px 排，见 git 历史里 `5fd0897` 的 docs/xochitl.md）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Origin {
    /// 外链样式表（`<link rel="stylesheet">`）。
    Sheet,
    /// 文档里的 `<style>`。
    Style,
    /// 元素的 `style` 属性。
    Inline,
}

impl Sheet {
    /// 追加一份样式表。`order` 是全局序号起点（后出现的规则优先），返回下一个序号。
    pub fn add(&mut self, css: &str, order: usize) -> usize {
        self.add_at(css, order, "", None)
    }

    /// 同 [`Sheet::add`]，`url(…)` 按样式表自己的路径 `base`（书内路径）换成书内路径。
    pub fn add_at(
        &mut self,
        css: &str,
        order: usize,
        base: &str,
        media: Option<&MediaEnv>,
    ) -> usize {
        self.add_rules(Rules::parse(css, base, media), order)
    }

    /// 追加一份解析好的样式表（见 [`Rules::parse`]），返回下一个序号。
    pub fn add_rules(&mut self, rules: Arc<Rules>, order: usize) -> usize {
        self.add_rules_from(rules, order, Origin::Sheet)
    }

    fn add_rules_from(&mut self, rules: Arc<Rules>, order: usize, origin: Origin) -> usize {
        let next = order + rules.len();
        self.parts.push((rules, order, origin));
        next
    }

    /// 一篇 HTML 文档（书内路径 `doc_path`）生效的样式表：按文档顺序收 `<style>` 和 `<link rel="stylesheet">`，`media` 属性
    /// 和 `@media` 同一口径（按 `media` 求，不成立的整份跳过）。外部样式表由 `rules_for(书内路径)` 给解析好的规则——
    /// 怎么解析、怎么缓存是调用方的事（KFX 写出器跨线程共享缓存，`kindle_rules` 事先全解析好），找不到的跳过。
    /// KFX 写出器和掌阅、Move 的 `kindle_rules` 共用，两边统计正文字号的口径因此一样（2026-10-10 审计以前各写一份）。
    pub fn for_doc(
        html: &scraper::Html,
        doc_path: &str,
        media: Option<&MediaEnv>,
        mut rules_for: impl FnMut(&str) -> Option<Arc<Rules>>,
    ) -> Sheet {
        static LINK_STYLE: std::sync::OnceLock<Selector> = std::sync::OnceLock::new();
        let sel = LINK_STYLE
            .get_or_init(|| Selector::parse("link, style").unwrap_or_else(|_| unreachable!()));
        let mut sheet = Sheet::default();
        let mut order = 0;
        for el in html.select(sel) {
            if el
                .value()
                .attr("media")
                .is_some_and(|m| !media_ok(m, media))
            {
                continue;
            }
            if el.value().name() == "style" {
                order = sheet.add_rules_from(
                    Rules::parse(&el.text().collect::<String>(), doc_path, media),
                    order,
                    Origin::Style,
                );
            } else if el
                .value()
                .attr("rel")
                .is_some_and(|r| r.to_ascii_lowercase().contains("stylesheet"))
            {
                if let Some(rules) = el
                    .value()
                    .attr("href")
                    .and_then(|h| rules_for(&bookbase::epubpath::resolve_link(doc_path, h).0))
                {
                    order = sheet.add_rules(rules, order);
                }
            }
        }
        sheet
    }

    /// 一个元素上生效的声明（按层叠排好，后面的覆盖前面的），含行内 `style`。
    /// 行内 `style` 里的 `url(…)` 按文档路径 `base` 解析（同样式表按自己的路径；以前不解析，子目录里文档的行内背景图找不到）。
    pub fn cascade(&self, el: &ElementRef, base: &str) -> Decls {
        let mut out = Decls::default();
        self.cascade_each(el, base, true, |d, _| {
            out.insert(d.prop.clone(), d.value.clone());
        });
        out
    }

    /// 同 [`Sheet::cascade`]，但 `!important` 当没写（带它的声明照普通声明排）：去掉 `!important` 以后会怎么层叠。
    pub fn cascade_flat(&self, el: &ElementRef, base: &str) -> Decls {
        let mut out = Decls::default();
        self.cascade_each(el, base, false, |d, _| {
            out.insert(d.prop.clone(), d.value.clone());
        });
        out
    }

    /// 同 [`Sheet::cascade`]，另给出生效的那条声明从哪来、带没带 `!important`：只记不是外链样式表里普通声明的（[`Origins`]）。
    /// 两张表在层叠时一次填好（以前先建「属性 → (值, 来源, important)」再拆成两张，属性名复制两遍、第二张还用 SipHash）：
    /// 后来的声明覆盖先来的，来源表跟着改——这一条要记就记，不用记就把前面记的删掉，最后剩下的就是胜出那条的。
    pub fn cascade_split(&self, el: &ElementRef, base: &str) -> (Decls, Origins) {
        let mut decls = Decls::default();
        let mut origins = Origins::default();
        self.cascade_each(el, base, true, |d, origin| {
            if origin != Origin::Sheet || d.important {
                origins.insert(d.prop.clone(), (origin, d.important));
            } else {
                origins.remove(&d.prop);
            }
            decls.insert(d.prop.clone(), d.value.clone());
        });
        (decls, origins)
    }

    /// 照 xochitl 的层叠顺序（2026-10-16 探针「测试-规则顺序」1–5）：`!important` 不提优先级；优先级一样的先比规则在**各自那份
    /// 样式表里**的序号（靠后的胜），序号也一样才是后链接的那份胜；行内 `style` 照旧压过外链。
    pub fn cascade_xochitl(&self, el: &ElementRef, base: &str) -> Decls {
        // (优先级, 在本样式表里的序号, 样式表的起始序号, 声明)
        type Hit<'a> = ((u32, u32, u32), usize, usize, &'a Decl);
        let mut hits: Vec<Hit> = Vec::new();
        let keys = ElementKeys::of(el);
        let mut cand = Vec::new();
        for (rules, start, _) in &self.parts {
            rules.candidates(&keys, &mut cand);
            for &i in &cand {
                let (i, r) = (i as usize, &rules.list[i as usize]);
                if r.need.as_ref().is_none_or(|n| n.may_match(el)) && r.sel.matches(el) {
                    hits.extend(r.decls.iter().map(|d| (r.spec, i, *start, d)));
                }
            }
        }
        let mut inline: Vec<Decl> = el
            .value()
            .attr("style")
            .map(parse_decls)
            .unwrap_or_default();
        resolve_urls(&mut inline, base);
        hits.extend(
            inline
                .iter()
                .map(|d| ((1000, 0, 0), usize::MAX, usize::MAX, d)),
        );
        hits.sort_by_key(|h| (h.0, h.1, h.2));
        let mut out = Decls::default();
        for (_, _, _, d) in hits {
            out.insert(d.prop.clone(), d.value.clone());
        }
        out
    }

    /// 按层叠顺序（优先级低的先）把元素上匹配的声明依次交给 `f`：同一属性后给的覆盖先给的。`important` 为假时 `!important` 不提优先级。
    fn cascade_each(
        &self,
        el: &ElementRef,
        base: &str,
        important: bool,
        mut f: impl FnMut(&Decl, Origin),
    ) {
        // (!important, 优先级, 出现顺序, 声明, 来源)
        type Hit<'a> = (bool, (u32, u32, u32), usize, &'a Decl, Origin);
        let mut hits: Vec<Hit> = Vec::new();
        let keys = ElementKeys::of(el);
        let mut cand = Vec::new();
        for (rules, base, origin) in &self.parts {
            rules.candidates(&keys, &mut cand);
            for &i in &cand {
                let (i, r) = (i as usize, &rules.list[i as usize]);
                if r.need.as_ref().is_none_or(|n| n.may_match(el)) && r.sel.matches(el) {
                    for d in &r.decls {
                        hits.push((important && d.important, r.spec, base + i, d, *origin));
                    }
                }
            }
        }
        let mut inline: Vec<Decl> = el
            .value()
            .attr("style")
            .map(parse_decls)
            .unwrap_or_default();
        resolve_urls(&mut inline, base);
        for d in &inline {
            hits.push((
                important && d.important,
                (1000, 0, 0),
                usize::MAX,
                d,
                Origin::Inline,
            ));
        }
        hits.sort_by_key(|h| (h.0, h.1, h.2));
        for (_, _, _, d, origin) in hits {
            f(d, origin);
        }
    }
}
