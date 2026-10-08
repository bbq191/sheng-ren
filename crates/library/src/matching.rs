//! 书名、人名比对：全角半角、繁简、括号说明、译名用字不同。豆瓣和 Wikidata 匹配共用。

pub(crate) use bookconv::util::to_halfwidth;

/// 比较用的规整：全角转半角、去掉结尾括号里的消歧义说明（"雪人 (小說)"）、只留字母数字和汉字、小写。
pub(crate) fn norm(s: &str) -> String {
    let s: String = s.chars().map(to_halfwidth).collect();
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

/// 简体化后再规整（豆瓣多是简体，好读的书是繁体）。
pub(crate) fn norm_s(s: &str) -> String {
    norm(&fast2s::convert(s))
}

/// 拿去搜索的作者名：第一个不空的，去掉国籍前缀（"[日] 东野圭吾" → 东野圭吾）。
pub(crate) fn author_for_query(authors: &[String]) -> Option<&str> {
    authors.iter().map(|a| a.trim()).find(|a| !norm_author(a).is_empty()).map(|a| match a.chars().next() {
        Some('[' | '［' | '(' | '（' | '【') => a.find([']', '］', ')', '）', '】']).map_or(a, |i| a[i + a[i..].chars().next().unwrap().len_utf8()..].trim()),
        _ => a,
    })
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
