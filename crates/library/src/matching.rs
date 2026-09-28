//! 书名、人名比对：全角半角、繁简、括号说明、译名用字不同。豆瓣和 Wikidata 匹配共用。

use crate::Meta;

/// 比较用的规整：全角转半角、去掉结尾括号里的消歧义说明（"雪人 (小說)"）、只留字母数字和汉字、小写。
pub(crate) fn norm(s: &str) -> String {
    let s: String = s.chars().map(|c| if ('\u{FF01}'..='\u{FF5E}').contains(&c) { char::from_u32(c as u32 - 0xFEE0).unwrap_or(c) } else { c }).collect();
    let s = s.trim();
    let s = match s.rfind(['(', '（']) {
        Some(i) if s.ends_with([')', '）']) && i > 0 => &s[..i],
        _ => s,
    };
    s.chars().filter(|c| c.is_alphanumeric()).flat_map(char::to_lowercase).collect()
}

/// 字重合度：两边共有的字数 / 较长一边的字数（译名用字不同时判断"是不是同一个名字"）。
pub(crate) fn similarity(a: &str, b: &str) -> f32 {
    let (a, b): (Vec<char>, Vec<char>) = (a.chars().collect(), b.chars().collect());
    if a.is_empty() || b.is_empty() {
        return 0.0;
    }
    let mut rest = b.clone();
    let common = a.iter().filter(|c| rest.iter().position(|x| x == *c).map(|i| rest.remove(i)).is_some()).count();
    common as f32 / a.len().max(b.len()) as f32
}

/// 拿去找作品的书名：书里的书名，加上原件文件名里的（`作者《书名》` 取书名号里的；其它按常见命名规整）。
/// 用户改过文件名（比如改成更通行的译名）时，文件名里的书名往往更准。规整后相同的只留一个。
pub(crate) fn title_candidates(meta: &Meta) -> Vec<String> {
    let stem = meta.source.rsplit_once('.').map_or(meta.source.as_str(), |(s, _)| s);
    let from_file = match (stem.find('《'), stem.rfind('》')) {
        (Some(a), Some(b)) if b > a => stem[a + '《'.len_utf8()..b].to_string(),
        _ => bookconv::naming::canonical_book_name(stem),
    };
    let mut out: Vec<String> = Vec::new();
    for t in [meta.title.clone(), from_file] {
        if !norm(&t).is_empty() && !out.iter().any(|o| norm(o) == norm(&t)) {
            out.push(t);
        }
    }
    out
}

/// 简体化后再规整（豆瓣多是简体，好读的书是繁体）。
pub(crate) fn norm_s(s: &str) -> String {
    norm(&fast2s::convert(s))
}

/// 作者名去掉国籍前缀（"[美] 欧·亨利"、"(法) 阿尔贝·加缪"）再规整。
pub(crate) fn norm_author(s: &str) -> String {
    let s = s.trim();
    let s = match s.chars().next() {
        Some('[' | '［' | '(' | '（' | '【') => s.find([']', '］', ')', '）', '】']).map_or(s, |i| &s[i + s[i..].chars().next().unwrap().len_utf8()..]),
        _ => s,
    };
    norm_s(s)
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_compare_across_width_brackets_and_transliteration() {
        assert_eq!(norm("雪人 (小說)"), "雪人");
        assert_eq!(norm("１３級階梯"), "13級階梯");
        assert_eq!(norm("羅傑．艾克洛命案"), "羅傑艾克洛命案");
        assert!(similarity(&norm("喬治．歐威爾"), &norm("喬治·奧威爾")) >= 0.6);
        assert!(similarity(&norm("13級階梯"), &norm("13級台階")) >= 0.75);
        assert!(similarity(&norm("瘟疫"), &norm("鼠疫")) < 0.75, "不同的书名不能算像");
        assert_eq!(norm_author("[美] 欧·亨利"), norm_s("歐．亨利").replace('．', ""));
    }
}
