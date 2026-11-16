//! 解析好的样式表、选择器必要条件与规则分桶。

use crate::media::{media_ok, MediaEnv};
use crate::text::{parse_decls, rule_head, rule_spans, specificity, split_top, strip_comments};
use crate::{resolve_urls, Decl};
use scraper::{ElementRef, Selector};
use std::collections::HashMap;
use std::sync::Arc;

pub(crate) struct Rule {
    pub(crate) sel: Selector,
    /// 快速排除：元素不满足它就一定匹配不上（见 [`Need::of`]）；`None` 时只能完整匹配。
    pub(crate) need: Option<Need>,
    pub(crate) spec: (u32, u32, u32),
    pub(crate) decls: Vec<Decl>,
}

/// 选择器最右边那一节（`p.a.b#x`）要求元素有的标签名、类、id。只是**必要条件**：比较一律不分 ASCII 大小写
/// （怪异模式下类名、id 不分大小写，宽一点不会把能匹配的排除掉），满足了还要完整匹配。
/// 大合集几十万个元素、每份样式表几十条规则，逐条完整匹配是写出器最慢的一步（2026-10-06：阿加莎全集层叠 1.1 秒）。
#[derive(Debug, PartialEq)]
pub(crate) struct Need {
    pub(crate) tag: Option<String>,
    pub(crate) classes: Vec<String>,
    pub(crate) ids: Vec<String>,
}

impl Need {
    /// 只认由标识符、`.`、`#`、空白和 `>`/`+`/`~` 组成的选择器；带属性、伪类、`*`、命名空间、转义的不做（返回 `None`）。
    pub(crate) fn of(sel: &str) -> Option<Need> {
        let ident = |c: char| c.is_alphanumeric() || c == '-' || c == '_';
        if !sel
            .chars()
            .all(|c| ident(c) || matches!(c, '.' | '#' | '>' | '+' | '~') || c.is_whitespace())
        {
            return None;
        }
        let last = sel
            .rsplit(|c: char| c.is_whitespace() || matches!(c, '>' | '+' | '~'))
            .next()?;
        if last.is_empty() {
            return None;
        }
        let mut need = Need {
            tag: None,
            classes: Vec::new(),
            ids: Vec::new(),
        };
        let mut rest = last;
        let tag_end = rest.find(['.', '#']).unwrap_or(rest.len());
        if tag_end > 0 {
            need.tag = Some(rest[..tag_end].to_string());
        }
        rest = &rest[tag_end..];
        while let Some(kind) = rest.chars().next() {
            let body = &rest[1..];
            let end = body.find(['.', '#']).unwrap_or(body.len());
            if end == 0 {
                return None;
            }
            let name = body[..end].to_string();
            if kind == '.' {
                need.classes.push(name)
            } else {
                need.ids.push(name)
            }
            rest = &body[end..];
        }
        Some(need)
    }

    pub(crate) fn may_match(&self, el: &ElementRef) -> bool {
        let v = el.value();
        self.tag
            .as_deref()
            .is_none_or(|t| v.name().eq_ignore_ascii_case(t))
            && self
                .ids
                .iter()
                .all(|i| v.id().is_some_and(|x| x.eq_ignore_ascii_case(i)))
            && self
                .classes
                .iter()
                .all(|c| v.classes().any(|x| x.eq_ignore_ascii_case(c)))
    }
}

/// 元素查规则桶用的键（[`RuleIndex`]）：id、类（去重）、标签名，都 ASCII 小写。一个元素在各份样式表里共用一份。
pub(crate) struct ElementKeys {
    id: Option<String>,
    classes: Vec<String>,
    tag: String,
}

impl ElementKeys {
    pub(crate) fn of(el: &ElementRef) -> ElementKeys {
        let v = el.value();
        let mut classes: Vec<String> = v.classes().map(|c| c.to_ascii_lowercase()).collect();
        classes.sort_unstable();
        classes.dedup();
        ElementKeys {
            id: v.id().map(str::to_ascii_lowercase),
            classes,
            tag: v.name().to_ascii_lowercase(),
        }
    }
}

/// 解析好的一份样式表（规则按出现顺序）。同一份样式表被很多文档引用时只解析一次，各文档的 [`Sheet`] 共用。
#[derive(Default)]
pub struct Rules {
    pub(crate) list: Vec<Rule>,
    pub(crate) index: RuleIndex,
}

/// 按选择器最右边那一节（[`Need`]）给规则分桶：一条规则只进一个桶（有 id 进 id 桶，否则有类进第一个类的桶，否则有标签名进
/// 标签桶，都没有的进 `other`），键一律 ASCII 小写。元素只看自己的 id、各个类、标签名对应的桶加 `other`（[`Rules::candidates`]）。
///
/// 为什么：以前每个元素把每条规则都拿 `Need` 预筛一遍。2026-10-15《古龙作品全集》Move 模式：433 万次元素层叠测了 2.68 亿条规则
/// （每个元素 62 条），只有 2.5% 过了预筛——层叠占 `Chapter::parse` 的七成，预筛又是层叠的大头。分桶只是缩小要试的范围：
/// 进了候选的照旧 `may_match` + 完整匹配，结果和顺序（层叠时按 (important, 优先级, 出现序号) 排，出现序号唯一）都不变。
/// 不会漏：`may_match` 要求 id / 每个类 / 标签名不分 ASCII 大小写相等，满足时元素对应的那个小写键一定就是规则所在的桶。
#[derive(Default)]
pub(crate) struct RuleIndex {
    pub(crate) by_id: HashMap<String, Vec<u32>>,
    pub(crate) by_class: HashMap<String, Vec<u32>>,
    pub(crate) by_tag: HashMap<String, Vec<u32>>,
    pub(crate) other: Vec<u32>,
}

impl RuleIndex {
    pub(crate) fn build(list: &[Rule]) -> RuleIndex {
        let mut ix = RuleIndex::default();
        for (i, r) in list.iter().enumerate() {
            let i = i as u32;
            match &r.need {
                Some(n) if !n.ids.is_empty() => ix
                    .by_id
                    .entry(n.ids[0].to_ascii_lowercase())
                    .or_default()
                    .push(i),
                Some(n) if !n.classes.is_empty() => ix
                    .by_class
                    .entry(n.classes[0].to_ascii_lowercase())
                    .or_default()
                    .push(i),
                Some(Need { tag: Some(t), .. }) => {
                    ix.by_tag.entry(t.to_ascii_lowercase()).or_default().push(i)
                }
                _ => ix.other.push(i),
            }
        }
        ix
    }
}

/// `@media` 最多套几层（真书里最多两层；更深的整块不收）。
pub(crate) const MAX_MEDIA_NESTING: usize = 32;

impl Rules {
    /// 解析一份样式表；`url(…)` 按样式表自己的路径 `base`（书内路径）换成书内路径（`base` 空时不换）。
    /// `@media` 块按 `media` 求值（见 [`media_ok`]）。
    pub fn parse(css: &str, base: &str, media: Option<&MediaEnv>) -> Arc<Rules> {
        let mut r = Rules::default();
        r.add(&strip_comments(css), base, media);
        r.index = RuleIndex::build(&r.list);
        Arc::new(r)
    }

    /// 有没有一条声明满足 `f`（只看按 `media` 收下的规则）。
    pub fn any_decl(&self, f: impl Fn(&Decl) -> bool) -> bool {
        self.list.iter().any(|r| r.decls.iter().any(&f))
    }

    /// 规则条数。
    pub fn len(&self) -> usize {
        self.list.len()
    }

    pub fn is_empty(&self) -> bool {
        self.list.is_empty()
    }

    /// 元素可能匹配的规则（下标升序，见 [`RuleIndex`]）：`keys` 是元素的 id、类、标签名（已 ASCII 小写）。
    pub(crate) fn candidates(&self, keys: &ElementKeys, out: &mut Vec<u32>) {
        out.clear();
        let ix = &self.index;
        if let Some(v) = keys.id.as_ref().and_then(|id| ix.by_id.get(id)) {
            out.extend_from_slice(v);
        }
        for c in &keys.classes {
            if let Some(v) = ix.by_class.get(c) {
                out.extend_from_slice(v);
            }
        }
        if let Some(v) = ix.by_tag.get(&keys.tag) {
            out.extend_from_slice(v);
        }
        out.extend_from_slice(&ix.other);
        // 一条规则只在一个桶里、元素的类已去重：没有重复，排个序按出现顺序试（结果本来就和顺序无关，排了好比对）
        out.sort_unstable();
    }

    /// `css` 已去掉注释。规则按 [`rule_spans`] 找：外面套的 at 规则都得是条件成立的 `@media`（别的 at 规则里面的不收），
    /// 套超过 [`MAX_MEDIA_NESTING`] 层的不收。
    fn add(&mut self, css: &str, base: &str, media: Option<&MediaEnv>) {
        let media_block_ok = |prelude: &str| {
            let head = rule_head(prelude).to_ascii_lowercase();
            head.strip_prefix("@media")
                .is_some_and(|q| media_ok(q, media))
        };
        for r in rule_spans(css) {
            if r.depth > MAX_MEDIA_NESTING || !r.parents.iter().all(|p| media_block_ok(p)) {
                continue;
            }
            let head = rule_head(r.prelude);
            if head.is_empty() || head.starts_with('@') {
                continue;
            }
            let mut decls = parse_decls(r.body);
            resolve_urls(&mut decls, base);
            if decls.is_empty() {
                continue;
            }
            for one in split_top(&head, ',') {
                let one = one.trim();
                if let Ok(sel) = Selector::parse(one) {
                    self.list.push(Rule {
                        sel,
                        need: Need::of(one),
                        spec: specificity(one),
                        decls: decls.clone(),
                    });
                }
            }
        }
    }
}
