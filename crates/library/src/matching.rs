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

/// 搜索结果的一条（书名 `title`、作者 `author`，作者可能是 `甲 / 乙`、`甲、乙` 这样几个人）和书里的书名（`nt`，已 [`norm_s`]）、
/// 作者（`want`，已 [`norm_author`]）对不对得上：书名相同，或一个是另一个的前缀、长度只差两个字以内，或（书里有作者时）用字重合 80% 以上；
/// 作者有一个像（相似度 ≥ 0.6）。
/// 书里没写作者时书名要完全相同。豆瓣、QQ 阅读共用一套口径（免得两边调参数时不一致）。
pub(crate) fn hit_matches(title: &str, author: &str, nt: &str, want: &[String]) -> bool {
    // 条目名也去掉卷次再比一遍（「哆啦A梦 第0卷」和书里的「哆啦A夢全彩版 - 卷01」去掉卷次后都是「哆啦A梦」）
    if let Some(b) = base_title(title) {
        if norm_s(&b) == nt && hit_matches(&b, author, nt, want) {
            return true;
        }
    }
    let dt = norm_s(title);
    // 书名相同、只差卷次后缀（前缀、差两字以内），或者作者对得上时用字重合 80% 以上（「金庸作品全集」和豆瓣的「金庸作品集」）
    let prefix = (nt.starts_with(&dt) || dt.starts_with(nt)) && nt.chars().count().abs_diff(dt.chars().count()) <= 2;
    let title_ok = dt == nt || prefix || (!want.is_empty() && dt.chars().count() >= 4 && similarity(&dt, nt) >= 0.8);
    let author_ok = if want.is_empty() { dt == nt } else { author.split(['/', '、', ',', '，']).any(|a| want.iter().any(|w| similarity(w, &norm_author(a)) >= 0.6)) };
    title_ok && author_ok
}

/// 两组人名里有没有同一个人（规整后相似度 ≥ 0.6，同 [`hit_matches`] 的作者口径；国籍、角色词不算）。
pub(crate) fn same_person_any(a: &[String], b: &[String]) -> bool {
    let (ka, kb): (Vec<String>, Vec<String>) = (a.iter().map(|x| norm_author(x)).collect(), b.iter().map(|x| norm_author(x)).collect());
    ka.iter().any(|x| !x.is_empty() && kb.iter().any(|y| similarity(x, y) >= 0.6))
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

/// 书里署名的一个人。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Person {
    pub(crate) name: String,
    /// 国籍（"[美] 玛格丽特·米切尔" 的「美」）；没写是空。
    pub(crate) nationality: String,
}

impl Person {
    /// 写进书里的样子：有国籍的写成 "[美] 玛格丽特·米切尔"（同豆瓣）。
    pub(crate) fn display(&self) -> String {
        if self.nationality.is_empty() { self.name.clone() } else { format!("[{}] {}", self.nationality, self.name) }
    }
}

/// 拆开的署名：作者、译者。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Credits {
    pub(crate) authors: Vec<Person>,
    pub(crate) translators: Vec<String>,
}

/// 名字前面的国籍（`[美]`、`（法）`、`【日】`）：(国籍, 其余)。
fn split_nationality(s: &str) -> (&str, &str) {
    let s = s.trim();
    match s.chars().next() {
        Some('[' | '［' | '(' | '（' | '【' | '〔') => match s.find([']', '］', ')', '）', '】', '〕']) {
            Some(i) => (s[s.chars().next().map_or(1, char::len_utf8)..i].trim(), s[i + s[i..].chars().next().map_or(1, char::len_utf8)..].trim()),
            None => ("", s),
        },
        _ => ("", s),
    }
}

/// 署名后面的角色词：著、译……（长的在前）。
const AUTHOR_ROLES: &[&str] = &["原著", "编著", "編著", "主编", "主編", "著", "撰", "编", "編", "绘", "繪", "作", "文", "图", "圖"];
const TRANSLATOR_ROLES: &[&str] = &["翻译", "翻譯", "译", "譯"];

/// 拆书里的署名（`dc:creator`，有的书把整串写在一个里面）：`"[美] 玛格丽特·米切尔 著，梅静 译"` → 作者 玛格丽特·米切尔（美）、
/// 译者 梅静；`"金庸"` → 作者 金庸。逗号、分号、斜杠分段，每段末尾的角色词定是作者还是译者（没写算作者），一段里几个人用
/// 顿号、`&` 分开。国籍写在段首的跟着这一段的人。
pub(crate) fn parse_credits(values: &[String]) -> Credits {
    let mut c = Credits::default();
    for v in values {
        for seg in v.split(['，', ',', '；', ';', '/', '／']) {
            let seg = seg.trim();
            if seg.is_empty() {
                continue;
            }
            let (nat, rest) = split_nationality(seg);
            let rest = rest.trim();
            let (names, translator) = match TRANSLATOR_ROLES.iter().find_map(|r| rest.strip_suffix(r)) {
                Some(n) => (n, true),
                None => (AUTHOR_ROLES.iter().find_map(|r| rest.strip_suffix(r).filter(|n| !n.trim().is_empty())).unwrap_or(rest), false),
            };
            for name in names.split(['、', '&', '＆']).map(str::trim).filter(|n| !n.is_empty()) {
                // 名字里自己带国籍的（"梅静、[英] 某某"）
                let (n_nat, name) = split_nationality(name);
                let nationality = if n_nat.is_empty() { nat } else { n_nat };
                if translator {
                    c.translators.push(name.to_string());
                } else {
                    c.authors.push(Person { name: name.to_string(), nationality: nationality.to_string() });
                }
            }
        }
    }
    c
}

/// 拿去搜索的作者名：署名拆开后的第一个作者（不带国籍、角色词："[日] 东野圭吾 著" → 东野圭吾）。
pub(crate) fn author_for_query(authors: &[String]) -> Option<String> {
    parse_credits(authors).authors.into_iter().map(|p| p.name).find(|n| !norm_s(n).is_empty())
}

/// 比对用的作者名：署名拆开后的作者，各自规整（译者不算）。
pub(crate) fn author_keys(authors: &[String]) -> Vec<String> {
    parse_credits(authors).authors.iter().map(|p| norm_s(&p.name)).filter(|a| !a.is_empty()).collect()
}

/// 作者名去掉国籍前缀（"[美] 欧·亨利"、"(法) 阿尔贝·加缪"）和角色词再规整；一串署名取第一个作者。
pub(crate) fn norm_author(s: &str) -> String {
    let c = parse_credits(std::slice::from_ref(&s.to_string()));
    c.authors.first().map_or_else(|| norm_s(split_nationality(s).1), |p| norm_s(&p.name))
}

/// 书名末尾标的作品形式（「（漫）」「（漫画版）」「（图像小说）」「（绘本）」）：原样返回那段括号；没有返回 `None`。
pub(crate) fn form_marker(title: &str) -> Option<String> {
    let t = title.trim();
    if !t.ends_with([')', '）']) {
        return None;
    }
    let i = t.rfind(['(', '（'])?;
    let inner = &t[i..];
    ["漫", "图像小说", "圖像小說", "绘本", "繪本", "连环画", "連環畫"].iter().any(|k| inner.contains(k)).then(|| inner.to_string())
}

/// 去掉书名末尾的卷次、版本说明再拿去搜（`"飘（上册）"` → 飘，`"哆啦A夢 第01卷"` → 哆啦A夢）：豆瓣按带括号的全名搜不到。
/// 没有可去的返回 `None`。
pub(crate) fn base_title(title: &str) -> Option<String> {
    let mut t = title.trim().to_string();
    // 开头的系列说明：「波洛系列04：罗杰疑案」→ 罗杰疑案
    if let Some((head, rest)) = t.split_once(['：', ':']) {
        if head.contains("系列") && !rest.trim().is_empty() {
            t = rest.trim().to_string();
        }
    }
    loop {
        let before = t.clone();
        // 末尾的括号说明
        if t.ends_with([')', '）', ']', '】']) {
            if let Some(i) = t.rfind(['(', '（', '[', '【']).filter(|&i| i > 0) {
                t.truncate(i);
            }
        }
        let trimmed = t.trim_end();
        // 末尾的卷次：第01卷、第1册、上册、下册、Vol.3、01
        let vol = strip_volume(trimmed);
        t = vol.trim_end().trim_end_matches(['-', '_', '·', '：', ':']).trim_end().to_string();
        if t == before || t.is_empty() {
            break;
        }
    }
    (!t.is_empty() && t != title.trim()).then_some(t)
}

/// [`base_title`] 的一步：去掉末尾一个卷次写法。
fn strip_volume(t: &str) -> &str {
    // 版本字样（漫画常见：「哆啦A夢全彩版」豆瓣没有，「哆啦A梦」有）
    for suf in ["全彩版", "彩色版", "修订版", "修訂版", "珍藏版", "典藏版", "完全版", "新装版", "新裝版", "精装版", "精裝版", "纪念版", "紀念版", "愛藏版", "爱藏版"] {
        if let Some(r) = t.strip_suffix(suf).filter(|r| !r.trim().is_empty()) {
            return r;
        }
    }
    for suf in ["上册", "中册", "下册", "上冊", "中冊", "下冊", "全集", "合集"] {
        if let Some(r) = t.strip_suffix(suf).filter(|r| !r.trim().is_empty()) {
            // 「全集」「合集」本身常是书名的一部分（《福尔摩斯探案全集》），只在前面已经有书名时才去
            if matches!(suf, "全集" | "合集") {
                return t;
            }
            return r;
        }
    }
    // 第N卷/册/集/部/巻、Vol.N、末尾的数字
    let chars: Vec<char> = t.chars().collect();
    let mut end = chars.len();
    let tail_unit = end > 0 && matches!(chars[end - 1], '卷' | '册' | '冊' | '集' | '部' | '巻' | '話' | '话');
    if tail_unit {
        end -= 1;
    }
    let digits_end = end;
    while end > 0 && (chars[end - 1].is_ascii_digit() || matches!(chars[end - 1], '０'..='９' | '一' | '二' | '三' | '四' | '五' | '六' | '七' | '八' | '九' | '十' | '百')) {
        end -= 1;
    }
    if end == digits_end || end == 0 {
        return t;
    }
    if tail_unit || (end > 0 && chars[end - 1] == '第') {
        if end > 0 && chars[end - 1] == '第' {
            end -= 1;
        }
    } else if !chars[end..digits_end].iter().all(|c| c.is_ascii_digit() || matches!(c, '０'..='９')) {
        // 只有中文数字、前后都没有「第…卷」的不是卷次（《三十六计》）
        return t;
    }
    let rest: String = chars[..end].iter().collect();
    let rest_trim = rest.trim_end().trim_end_matches(['.', ' ', '#']);
    // 数字前面的卷次字样：Vol.3、卷01（「哆啦A夢全彩版 - 卷01」）、第3
    let rest_trim = ["Vol", "vol", "VOL", "卷", "巻", "册", "冊", "第"].iter().find_map(|p| rest_trim.strip_suffix(p)).unwrap_or(rest_trim);
    let n = rest_trim.len();
    if rest_trim.trim().is_empty() { t } else { &t[..n] }
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

    /// 书里整串写在一个 `dc:creator` 里的署名拆成作者（带国籍）和译者（2026-10-12《飘》《消失的爱人》《福尔摩斯》以前因此搜不到）。
    #[test]
    fn credits_split_into_authors_nationality_and_translators() {
        let c = parse_credits(&["[美] 玛格丽特·米切尔 著，梅静 译".into()]);
        assert_eq!(c.authors, [Person { name: "玛格丽特·米切尔".into(), nationality: "美".into() }]);
        assert_eq!(c.translators, ["梅静"]);
        assert_eq!(c.authors[0].display(), "[美] 玛格丽特·米切尔");
        let c = parse_credits(&["[英] 亚瑟·柯南·道尔 著，李家真 译".into()]);
        assert_eq!((c.authors[0].name.as_str(), c.translators[0].as_str()), ("亚瑟·柯南·道尔", "李家真"));
        let c = parse_credits(&["金庸".into()]);
        assert_eq!(c.authors, [Person { name: "金庸".into(), nationality: String::new() }]);
        let c = parse_credits(&["（日）东野圭吾".into(), "刘姿君".into()]);
        assert_eq!(c.authors.len(), 2, "没写角色的都算作者");
        let c = parse_credits(&["甲、乙 著；丙 譯".into()]);
        assert_eq!((c.authors.len(), c.translators.clone()), (2, vec!["丙".to_string()]));
        assert_eq!(author_for_query(&["[美] 吉莉安•弗琳 著，胡绯 译".into()]).as_deref(), Some("吉莉安•弗琳"));
        assert_eq!(author_keys(&["[美] 吉莉安•弗琳 著，胡绯 译".into()]), [norm_s("吉莉安·弗琳")]);
    }

    #[test]
    fn base_title_drops_volume_and_edition_suffixes() {
        assert_eq!(base_title("飘（上册）").as_deref(), Some("飘"));
        assert_eq!(base_title("金庸作品全集（修订版）").as_deref(), Some("金庸作品全集"));
        assert_eq!(base_title("哆啦A夢 第01卷").as_deref(), Some("哆啦A夢"));
        assert_eq!(base_title("死亡筆記 Vol.3").as_deref(), Some("死亡筆記"));
        assert_eq!(base_title("飘 - 下册").as_deref(), Some("飘"));
        assert_eq!(base_title("哆啦A夢全彩版 - 卷01").as_deref(), Some("哆啦A夢"));
        assert_eq!(base_title("三体"), None);
        assert_eq!(base_title("三十六计"), None, "中文数字不是卷次");
        assert_eq!(base_title("福尔摩斯探案全集"), None, "全集是书名的一部分");
        assert_eq!(base_title("1984"), None, "全是数字的书名不动");
        assert_eq!(base_title("波洛系列04：罗杰疑案").as_deref(), Some("罗杰疑案"));
        assert_eq!(form_marker("杀死一只知更鸟（漫）").as_deref(), Some("（漫）"));
        assert_eq!(form_marker("飘（上册）"), None);
    }
}
