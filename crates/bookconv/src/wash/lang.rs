//! 语言标错了改正（2026-10-19 用户定：缺 `dc:language` 时补，标错了改正）。阅读器按语言选排版：Kindle 遇到写 `en` 的中文
//! 按英文排（每行差 2–3px、上标把整行推下约 20px，见 `kfx::write` 的 `meta_language`），KFX 写出器还把 `<html lang>` 照写到
//! 每个块上——《狼厅》72 个正文文件写着 `lang="en"`，Kindle 产物里 33 个样式是 `en`。
//!
//! - **怎么判**：数正文里的汉字、假名、谚文和拉丁字母。汉字＋假名＋谚文不少于拉丁字母的是中日韩文（谚文多于汉字假名 → `ko`，
//!   假名占一成以上 → `ja`，其余 `zh`），拉丁字母是它们十倍以上的是西文（`en`），两者之间、字太少（全书不到 200 个、单个文件不到
//!   50 个）的不判——拿不准就不改。
//! - **算标错**：写的是 `und`/`mul`/`zxx`/`mis` 或不像语言标签的；中日韩文的书写了别的语言；西文的书写了中日韩语言（改成 `en`：
//!   分不出是哪种西文）。中日韩之间、西文之间不换（`zh-TW`/`zh-CN` 也不动）。
//! - **改哪里**：OPF 第一个有内容的 `dc:language` 改成判出来的；`<html>` 的 `lang`/`xml:lang` 和全书判出来的对不上时，这个
//!   文件自己的字够判、又和写的对得上的留着（中文书里整章英文的附录），否则改成全书的语言（OPF 写对了就用 OPF 的，如 `zh-CN`）。
//!   元素上的 `lang` 不动（书里引的外文）。缺的照旧在后面补（`normalize::upgrade_opf`、`layout::ensure_html_lang`）。

use super::*;

/// 判出来的文字。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Script {
    /// 中日韩文，带判出来的语言（`zh`/`ja`/`ko`）。
    Cjk(&'static str),
    /// 西文（拉丁字母）。
    Latin,
}

impl Script {
    fn tag(self) -> &'static str {
        match self {
            Script::Cjk(t) => t,
            Script::Latin => "en",
        }
    }
}

/// 字数统计。
#[derive(Debug, Default, Clone, Copy)]
struct Counts {
    han: u64,
    kana: u64,
    hangul: u64,
    latin: u64,
}

impl Counts {
    fn of(text: &str) -> Counts {
        let mut c = Counts::default();
        for ch in text.chars() {
            match ch {
                '\u{4E00}'..='\u{9FFF}' | '\u{3400}'..='\u{4DBF}' | '\u{F900}'..='\u{FAFF}' | '\u{20000}'..='\u{2FA1F}' => c.han += 1,
                '\u{3041}'..='\u{309F}' | '\u{30A1}'..='\u{30FA}' | '\u{31F0}'..='\u{31FF}' => c.kana += 1,
                '\u{AC00}'..='\u{D7AF}' | '\u{1100}'..='\u{11FF}' | '\u{3130}'..='\u{318F}' => c.hangul += 1,
                c2 if c2.is_alphabetic() && (c2 as u32) < 0x250 => c.latin += 1,
                _ => {}
            }
        }
        c
    }

    fn add(&mut self, o: Counts) {
        self.han += o.han;
        self.kana += o.kana;
        self.hangul += o.hangul;
        self.latin += o.latin;
    }

    fn total(&self) -> u64 {
        self.han + self.kana + self.hangul + self.latin
    }

    /// 字不少于 `min` 个、分得清的才判。
    fn script(&self, min: u64) -> Option<Script> {
        if self.total() < min {
            return None;
        }
        let cjk = self.han + self.kana + self.hangul;
        if cjk >= self.latin {
            Some(Script::Cjk(if self.hangul > self.han + self.kana {
                "ko"
            } else if self.kana * 10 >= self.han + self.kana {
                "ja"
            } else {
                "zh"
            }))
        } else if self.latin >= cjk * 10 {
            Some(Script::Latin)
        } else {
            None
        }
    }
}

/// 全书最少多少字才判。
const BOOK_MIN: u64 = 200;
/// 单个文件最少多少字才按它自己的判。
const FILE_MIN: u64 = 50;
/// 全书够这么多字就不再往下数。
const BOOK_ENOUGH: u64 = 20_000;

/// 语言标签的主标签（小写）。
fn primary(tag: &str) -> String {
    tag.trim().split(['-', '_']).next().unwrap_or("").to_ascii_lowercase()
}

/// 中日韩的语言（ISO 639-1/2/3 里常见的写法）。
fn is_cjk_lang(p: &str) -> bool {
    matches!(p, "zh" | "chi" | "zho" | "cmn" | "yue" | "wuu" | "lzh" | "hak" | "nan" | "gan" | "hsn" | "cjy" | "ja" | "jpn" | "ko" | "kor")
}

/// 写的语言和判出来的文字对得上。占位的（`und` 等）、不像语言标签的算对不上。
fn fits(tag: &str, s: Script) -> bool {
    let p = primary(tag);
    if !(2..=3).contains(&p.len()) || !p.bytes().all(|b| b.is_ascii_lowercase()) || matches!(p.as_str(), "und" | "mul" | "zxx" | "mis") {
        return false;
    }
    match s {
        Script::Cjk(_) => is_cjk_lang(&p),
        Script::Latin => !is_cjk_lang(&p),
    }
}

/// OPF 里第一个有内容的 `dc:language`：(内容起, 止)。
fn dc_language_span(opf: &str) -> Option<(usize, usize)> {
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| Regex::new(r"(?is)<dc:language\b[^>]*>([^<]*)</dc:language\s*>").unwrap());
    re.captures_iter(opf).filter_map(|c| c.get(1)).find(|m| !m.as_str().trim().is_empty()).map(|m| (m.start(), m.end()))
}

/// 改正 OPF 的 `dc:language` 和各文件 `<html>` 的语言属性（见模块说明），改了几处记进 `rep.languages_fixed`。
pub(super) fn fix_languages(entries: &mut [Entry], rep: &mut WashReport) {
    let mut book = Counts::default();
    for e in entries.iter().filter(|e| is_chapter_entry(e)) {
        let Ok(t) = std::str::from_utf8(&e.data) else { continue };
        book.add(Counts::of(&plain_text(t)));
        if book.total() > BOOK_ENOUGH {
            break;
        }
    }
    let Some(script) = book.script(BOOK_MIN) else { return };
    let mut tag = script.tag().to_string();
    if let Some(i) = find_opf(entries) {
        if let Ok(opf) = std::str::from_utf8(&entries[i].data) {
            if let Some((s, e)) = dc_language_span(opf) {
                let old = opf[s..e].trim();
                if fits(old, script) {
                    tag = old.to_string();
                } else {
                    let out = format!("{}{}{}", &opf[..s], tag, &opf[e..]);
                    entries[i].data = out.into_bytes();
                    rep.languages_fixed += 1;
                }
            }
        }
    }
    let fixed = crate::util::par_map_mut(entries, |e| {
        if !is_html_entry(&e.name, &e.data) {
            return 0;
        }
        let Ok(t) = std::str::from_utf8(&e.data) else { return 0 };
        let Some(h) = html::tags(t).find(|x| x.is_start() && x.is("html")) else { return 0 };
        let open = &t[h.start..h.end];
        let wrong = |n: &str| html::attr_value(open, n).is_some_and(|v| !fits(v, script));
        if !wrong("lang") && !wrong("xml:lang") {
            return 0;
        }
        // 文件自己的字够判、又和写的对得上的留着
        let own = Counts::of(&plain_text(t)).script(FILE_MIN);
        let new_open = html::edit_attrs(open, &["lang", "xml:lang"], |_, a| match own {
            Some(s) if fits(a.value, s) => Edit::Keep,
            _ if fits(a.value, script) => Edit::Keep,
            Some(s) if !matches!((s, script), (Script::Cjk(_), Script::Cjk(_)) | (Script::Latin, Script::Latin)) => Edit::Set(s.tag().to_string()),
            _ => Edit::Set(tag.clone()),
        });
        if let Cow::Owned(o) = new_open {
            e.data = format!("{}{}{}", &t[..h.start], o, &t[h.end..]).into_bytes();
            1
        } else {
            0
        }
    });
    rep.languages_fixed += fixed.into_iter().sum::<usize>();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn e(name: &str, data: &str) -> Entry {
        Entry { name: name.into(), data: data.as_bytes().to_vec() }
    }

    fn opf(lang: &str) -> Entry {
        e("OEBPS/content.opf", &format!(r#"<package xmlns="http://www.idpf.org/2007/opf" version="3.0"><metadata xmlns:dc="http://purl.org/dc/elements/1.1/"><dc:title>书</dc:title><dc:language>{lang}</dc:language></metadata><manifest/><spine/></package>"#))
    }

    fn page(name: &str, lang: &str, body: &str) -> Entry {
        e(name, &format!(r#"<html xmlns="http://www.w3.org/1999/xhtml"{lang}><head><title>t</title></head><body><p>{body}</p></body></html>"#))
    }

    const ZH: &str = "他们走进了那间屋子，屋里很暗，只有一盏灯在角落里亮着。窗外下着雨，远处传来钟声。";
    const EN: &str = "It was the best of times, it was the worst of times, it was the age of wisdom, it was the age of foolishness.";

    fn text(e: &Entry) -> &str {
        std::str::from_utf8(&e.data).unwrap()
    }

    #[test]
    fn script_detection() {
        assert_eq!(Counts::of(&ZH.repeat(10)).script(BOOK_MIN), Some(Script::Cjk("zh")));
        assert_eq!(Counts::of(&EN.repeat(10)).script(BOOK_MIN), Some(Script::Latin));
        assert_eq!(Counts::of(&"吾輩は猫である。名前はまだ無い。どこで生れたかとんと見当がつかぬ。".repeat(10)).script(BOOK_MIN), Some(Script::Cjk("ja")));
        assert_eq!(Counts::of(&"나는 고양이로소이다. 이름은 아직 없다.".repeat(20)).script(BOOK_MIN), Some(Script::Cjk("ko")));
        assert_eq!(Counts::of(ZH).script(BOOK_MIN), None, "字太少不判");
        // 一半中文一半英文（拉丁字母是汉字的 2–10 倍）：不判
        assert_eq!(Counts::of(&format!("{}{}", ZH, EN.repeat(2))).script(10), None);
    }

    #[test]
    fn fits_rules() {
        let zh = Script::Cjk("zh");
        for ok in ["zh", "zh-CN", "zh-Hant-TW", "ZH-cn", "ja", "yue", "chi"] {
            assert!(fits(ok, zh), "{ok}");
        }
        for bad in ["en", "en-US", "und", "mul", "", "x", "chinese", "1234"] {
            assert!(!fits(bad, zh), "{bad}");
        }
        assert!(fits("fr", Script::Latin) && fits("en-GB", Script::Latin));
        assert!(!fits("zh", Script::Latin) && !fits("und", Script::Latin));
    }

    #[test]
    fn wrong_opf_and_html_lang_fixed() {
        let mut es = vec![opf("en"), page("OEBPS/c1.xhtml", r#" lang="en""#, &ZH.repeat(10)), page("OEBPS/c2.xhtml", r#" xml:lang='und' lang="zh-CN""#, &ZH.repeat(10))];
        let mut rep = WashReport::default();
        fix_languages(&mut es, &mut rep);
        assert!(text(&es[0]).contains("<dc:language>zh</dc:language>"), "{}", text(&es[0]));
        assert!(text(&es[1]).contains(r#"<html xmlns="http://www.w3.org/1999/xhtml" lang="zh">"#), "{}", text(&es[1]));
        assert!(text(&es[2]).contains(r#"xml:lang='zh' lang="zh-CN""#), "对得上的不动: {}", text(&es[2]));
        assert_eq!(rep.languages_fixed, 3);
    }

    #[test]
    fn html_lang_follows_correct_opf_and_keeps_own_language_files() {
        let cover = r#"<html xmlns="http://www.w3.org/1999/xhtml" xml:lang="en"><body><img src="c.jpg"/></body></html>"#;
        let mut es = vec![opf("zh-CN"), page("OEBPS/c1.xhtml", "", &ZH.repeat(10)), e("OEBPS/cover.xhtml", cover), page("OEBPS/app.xhtml", r#" lang="en""#, EN), page("OEBPS/app2.xhtml", r#" lang="zh""#, &EN.repeat(3))];
        let mut rep = WashReport::default();
        fix_languages(&mut es, &mut rep);
        assert!(text(&es[0]).contains("<dc:language>zh-CN</dc:language>"));
        assert!(text(&es[2]).contains(r#"xml:lang="zh-CN""#), "没字的文件跟全书（OPF 写对了用 OPF 的）: {}", text(&es[2]));
        assert!(text(&es[3]).contains(r#"lang="en""#), "整章英文的附录留着: {}", text(&es[3]));
        assert!(text(&es[4]).contains(r#"lang="zh""#), "写的是中日韩语言、合全书的不动: {}", text(&es[4]));
        assert_eq!(rep.languages_fixed, 1);
    }

    #[test]
    fn english_book_marked_chinese_and_unsure_books_untouched() {
        let mut es = vec![opf("zh"), page("OEBPS/c1.xhtml", r#" lang="zh-CN""#, &EN.repeat(10))];
        let mut rep = WashReport::default();
        fix_languages(&mut es, &mut rep);
        assert!(text(&es[0]).contains("<dc:language>en</dc:language>"));
        assert!(text(&es[1]).contains(r#"lang="en""#));
        // 字太少：什么都不改
        let mut es = vec![opf("en"), page("OEBPS/c1.xhtml", r#" lang="en""#, ZH)];
        let before: Vec<Vec<u8>> = es.iter().map(|e| e.data.clone()).collect();
        let mut rep = WashReport::default();
        fix_languages(&mut es, &mut rep);
        assert_eq!(es.iter().map(|e| e.data.clone()).collect::<Vec<_>>(), before);
        assert_eq!(rep.languages_fixed, 0);
    }

    #[test]
    fn idempotent() {
        let mut es = vec![opf("und"), page("OEBPS/c1.xhtml", r#" lang="en""#, &ZH.repeat(10))];
        let mut rep = WashReport::default();
        fix_languages(&mut es, &mut rep);
        let once: Vec<Vec<u8>> = es.iter().map(|e| e.data.clone()).collect();
        let mut rep2 = WashReport::default();
        fix_languages(&mut es, &mut rep2);
        assert_eq!(es.iter().map(|e| e.data.clone()).collect::<Vec<_>>(), once);
        assert_eq!(rep2.languages_fixed, 0);
    }
}
