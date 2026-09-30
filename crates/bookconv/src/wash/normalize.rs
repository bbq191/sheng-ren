//! 规范整理（清洗层最后一步，2026-09-29 用户定：**所有产物一律升级成 EPUB 3**）。
//!
//! 1. **XHTML 修成合法 XML**（[`normalize_markup`]）：只改"按 XML 读就是错"的地方，可见文字一个不动——
//!    - `<!DOCTYPE …>` 换成 EPUB 3 的 `<!DOCTYPE html>`（《金庸全集》原书有几份写坏成 `<!DOCTYpE html pUBLIC "-//W4C//…`）；
//!    - HTML 命名实体（`&nbsp;`、`&hellip;`……，HTML 4 那一套）换成数字引用：换掉 DOCTYPE 后外部 DTD 不在了，
//!      命名实体按 XML 读就是未定义；认不出的名字原样留着（拿不准）；
//!    - 裸 `&` → `&amp;`，不是标签开头的 `<`（`a < b`）→ `&lt;`（标签之间的文字）；属性值里的 `<` 一律 → `&lt;`；
//!    - XML 1.0 不允许的控制字符（原书损坏留下的 U+0010 之类）去掉：它们不是看得见的字；
//!    - 空元素没闭合（`<br>`、`<img …>`）补成自闭合，无引号/无值属性补引号（`nowrap` → `nowrap="nowrap"`）；
//!    - 没有对应开标签的闭合标签（《绝叫》`<head>` 里多出来的 `</div>`）去掉——**只在去掉后整份标签配对完全平衡时**才去，
//!      否则一个都不去（交叉嵌套之类拿不准的留给质量门报）；
//!    - 根元素 `<html>` 补 `xmlns`（XHTML）和 `xmlns:epub`（以后写 `epub:type` 才是合法 XML）。
//!    - OPF、NCX 只做字符层面的那几条（实体、裸 `&`/`<`、控制字符），不碰结构。
//! 2. **OPF 升级到 3.0**（[`upgrade_opf`]）：`version="3.0"`；`unique-identifier` 指向一个真的 `<dc:identifier id>`（没有就补）；
//!    补 `<meta property="dcterms:modified">`（**固定值** [`EPUB3_MODIFIED`]，产物逐字节确定）；缺 `dc:language` 按书的语言补；
//!    EPUB 2 的 `opf:role`/`opf:file-as`/`opf:scheme` 属性改写成 EPUB 3 的 `<meta refines>`，`opf:event` 去掉，多余的 `dc:date`
//!    （EPUB 3 只许一个）改成 `<meta property="dcterms:date">`。已经是 3.x 的书元数据不动（只补缺的必需项）。
//!    `<meta name="cover">`、`<guide>` 是 EPUB 3 允许的旧写法，保留；`toc.ncx` 与 `<spine toc="ncx">` **保留**——
//!    reMarkable xochitl 读目录靠 NCX（见 `ncx_fix.rs`），NCX 的 `dtb:uid` 在这之后再对齐一次 OPF 标识符。
//! 3. **导航文档**（[`ensure_nav`]）：没有 `properties="nav"` 的书按 NCX 生成一份 `nav.xhtml`（NCX 也没有就指向第一章）；
//!    `<guide>` 里的条目同时写成 nav 里的 `landmarks`（只写进不在 spine 里的 nav，不会给正文添字）。
//! 4. manifest 里 XHTML 的 `properties`（`svg`/`mathml`/`scripted`/`remote-resources`）按**最终**内容标，在优化器写 OPF 时做
//!    （[`apply_content_properties`]；第二遍还会改章节：SVG 封面换 `<img>`、远程图抓进书里）。
use super::*;

/// 升级到 EPUB 3 时写的 `dcterms:modified`：固定值，不取当前时间——同样的输入每次产物逐字节相同（书库按指纹判断过期，
/// 回归按字节比较）。原书已有格式正确的 `dcterms:modified` 就沿用原书的。
pub const EPUB3_MODIFIED: &str = "2000-01-01T00:00:00Z";
const XHTML_NS: &str = "http://www.w3.org/1999/xhtml";
const OPS_NS: &str = "http://www.idpf.org/2007/ops";
const OPF_NS: &str = "http://www.idpf.org/2007/opf";
const DC_NS: &str = "http://purl.org/dc/elements/1.1/";

/// 规范整理修了什么（XML 层面的修复计数）。
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize)]
pub struct XmlFixes {
    /// 换成 `<!DOCTYPE html>` 的 DOCTYPE 数。
    pub doctypes: usize,
    /// HTML 命名实体换成数字引用的个数。
    pub named_entities: usize,
    /// 认不出的命名实体（原样留着，按 XML 读仍是错）。
    pub unknown_entities: usize,
    /// 裸 `&` 转成 `&amp;` 的个数。
    pub bare_amps: usize,
    /// 不是标签开头的 `<` 转成 `&lt;` 的个数。
    pub bare_lts: usize,
    /// 去掉的 XML 不允许的字符（含指向它们的数字引用）。
    pub control_chars: usize,
    /// 补成自闭合的空元素。
    pub void_tags_closed: usize,
    /// 补了引号或值的属性。
    pub attrs_quoted: usize,
    /// 去掉的多余闭合标签。
    pub stray_close_tags: usize,
    /// 根元素补上或改正的命名空间声明（`xmlns`、`xmlns:epub`；《金庸全集》原书有 `xmlns="http：//…"` 写成全角冒号的）。
    pub namespaces_fixed: usize,
}

impl XmlFixes {
    fn add(&mut self, o: &XmlFixes) {
        self.doctypes += o.doctypes;
        self.named_entities += o.named_entities;
        self.unknown_entities += o.unknown_entities;
        self.bare_amps += o.bare_amps;
        self.bare_lts += o.bare_lts;
        self.control_chars += o.control_chars;
        self.void_tags_closed += o.void_tags_closed;
        self.attrs_quoted += o.attrs_quoted;
        self.stray_close_tags += o.stray_close_tags;
        self.namespaces_fixed += o.namespaces_fixed;
    }
}

/// 清洗层入口：XML 修复 → OPF 升级 → 导航文档与 landmarks。`lang_tag`：缺 `dc:language` 时补的值；`heading`：新建 nav 的标题。
pub(super) fn normalize_book(entries: &mut Vec<Entry>, lang_tag: &str, heading: &str, rep: &mut WashReport) {
    for e in entries.iter_mut() {
        let l = e.name.to_ascii_lowercase();
        let xhtml = is_html_entry(&e.name, &e.data);
        if !(xhtml || l.ends_with(".opf") || l.ends_with(".ncx")) {
            continue;
        }
        let Ok(text) = std::str::from_utf8(&e.data) else { continue };
        let mut fx = XmlFixes::default();
        if let Cow::Owned(t) = normalize_markup(text, xhtml, &mut fx) {
            e.data = t.into_bytes();
        }
        rep.xml_fixes.add(&fx);
    }
    let Some(oi) = find_opf(entries) else { return };
    let opf_text = String::from_utf8_lossy(&entries[oi].data).into_owned();
    if let Some(t) = upgrade_opf(&opf_text, lang_tag) {
        entries[oi].data = t.into_bytes();
        rep.epub3_upgraded = 1;
    }
    ensure_nav(entries, heading, rep);
}

// ───────────────────────── XML 修复 ─────────────────────────

/// HTML 4 的命名实体（XML 预定义的 `amp`/`lt`/`gt`/`quot` 除外），按名字字节序排好，二分查找。XHTML 1.x 的 DTD 定义的就是这一套。
#[rustfmt::skip]
const HTML4_ENTITIES: &[(&str, u32)] = &[
    ("AElig", 198), ("Aacute", 193), ("Acirc", 194), ("Agrave", 192), ("Alpha", 913), ("Aring", 197), ("Atilde", 195), ("Auml", 196), ("Beta", 914),
    ("Ccedil", 199), ("Chi", 935), ("Dagger", 8225), ("Delta", 916), ("ETH", 208), ("Eacute", 201), ("Ecirc", 202), ("Egrave", 200), ("Epsilon", 917),
    ("Eta", 919), ("Euml", 203), ("Gamma", 915), ("Iacute", 205), ("Icirc", 206), ("Igrave", 204), ("Iota", 921), ("Iuml", 207), ("Kappa", 922),
    ("Lambda", 923), ("Mu", 924), ("Ntilde", 209), ("Nu", 925), ("OElig", 338), ("Oacute", 211), ("Ocirc", 212), ("Ograve", 210), ("Omega", 937),
    ("Omicron", 927), ("Oslash", 216), ("Otilde", 213), ("Ouml", 214), ("Phi", 934), ("Pi", 928), ("Prime", 8243), ("Psi", 936), ("Rho", 929),
    ("Scaron", 352), ("Sigma", 931), ("THORN", 222), ("Tau", 932), ("Theta", 920), ("Uacute", 218), ("Ucirc", 219), ("Ugrave", 217), ("Upsilon", 933),
    ("Uuml", 220), ("Xi", 926), ("Yacute", 221), ("Yuml", 376), ("Zeta", 918), ("aacute", 225), ("acirc", 226), ("acute", 180), ("aelig", 230),
    ("agrave", 224), ("alefsym", 8501), ("alpha", 945), ("and", 8743), ("ang", 8736), ("aring", 229), ("asymp", 8776), ("atilde", 227), ("auml", 228),
    ("bdquo", 8222), ("beta", 946), ("brvbar", 166), ("bull", 8226), ("cap", 8745), ("ccedil", 231), ("cedil", 184), ("cent", 162), ("chi", 967),
    ("circ", 710), ("clubs", 9827), ("cong", 8773), ("copy", 169), ("crarr", 8629), ("cup", 8746), ("curren", 164), ("dArr", 8659), ("dagger", 8224),
    ("darr", 8595), ("deg", 176), ("delta", 948), ("diams", 9830), ("divide", 247), ("eacute", 233), ("ecirc", 234), ("egrave", 232), ("empty", 8709),
    ("emsp", 8195), ("ensp", 8194), ("epsilon", 949), ("equiv", 8801), ("eta", 951), ("eth", 240), ("euml", 235), ("euro", 8364), ("exist", 8707),
    ("fnof", 402), ("forall", 8704), ("frac12", 189), ("frac14", 188), ("frac34", 190), ("frasl", 8260), ("gamma", 947), ("ge", 8805), ("hArr", 8660),
    ("harr", 8596), ("hearts", 9829), ("hellip", 8230), ("iacute", 237), ("icirc", 238), ("iexcl", 161), ("igrave", 236), ("image", 8465),
    ("infin", 8734), ("int", 8747), ("iota", 953), ("iquest", 191), ("isin", 8712), ("iuml", 239), ("kappa", 954), ("lArr", 8656), ("lambda", 955),
    ("lang", 9001), ("laquo", 171), ("larr", 8592), ("lceil", 8968), ("ldquo", 8220), ("le", 8804), ("lfloor", 8970), ("lowast", 8727), ("loz", 9674),
    ("lrm", 8206), ("lsaquo", 8249), ("lsquo", 8216), ("macr", 175), ("mdash", 8212), ("micro", 181), ("middot", 183), ("minus", 8722), ("mu", 956),
    ("nabla", 8711), ("nbsp", 160), ("ndash", 8211), ("ne", 8800), ("ni", 8715), ("not", 172), ("notin", 8713), ("nsub", 8836), ("ntilde", 241),
    ("nu", 957), ("oacute", 243), ("ocirc", 244), ("oelig", 339), ("ograve", 242), ("oline", 8254), ("omega", 969), ("omicron", 959), ("oplus", 8853),
    ("or", 8744), ("ordf", 170), ("ordm", 186), ("oslash", 248), ("otilde", 245), ("otimes", 8855), ("ouml", 246), ("para", 182), ("part", 8706),
    ("permil", 8240), ("perp", 8869), ("phi", 966), ("pi", 960), ("piv", 982), ("plusmn", 177), ("pound", 163), ("prime", 8242), ("prod", 8719),
    ("prop", 8733), ("psi", 968), ("rArr", 8658), ("radic", 8730), ("rang", 9002), ("raquo", 187), ("rarr", 8594), ("rceil", 8969), ("rdquo", 8221),
    ("real", 8476), ("reg", 174), ("rfloor", 8971), ("rho", 961), ("rlm", 8207), ("rsaquo", 8250), ("rsquo", 8217), ("sbquo", 8218), ("scaron", 353),
    ("sdot", 8901), ("sect", 167), ("shy", 173), ("sigma", 963), ("sigmaf", 962), ("sim", 8764), ("spades", 9824), ("sub", 8834), ("sube", 8838),
    ("sum", 8721), ("sup", 8835), ("sup1", 185), ("sup2", 178), ("sup3", 179), ("supe", 8839), ("szlig", 223), ("tau", 964), ("there4", 8756),
    ("theta", 952), ("thetasym", 977), ("thinsp", 8201), ("thorn", 254), ("tilde", 732), ("times", 215), ("trade", 8482), ("uArr", 8657),
    ("uacute", 250), ("uarr", 8593), ("ucirc", 251), ("ugrave", 249), ("uml", 168), ("upsih", 978), ("upsilon", 965), ("uuml", 252), ("weierp", 8472),
    ("xi", 958), ("yacute", 253), ("yen", 165), ("yuml", 255), ("zeta", 950), ("zwj", 8205), ("zwnj", 8204),
];

/// `s` 从 `&` 开始：是字符引用/实体引用就返回（整段长度, 种类）。
enum Ref<'a> {
    /// 合法的数字引用或 XML 预定义实体：原样。
    Keep(usize),
    /// 数字引用指向 XML 不允许的字符：去掉。
    Illegal(usize),
    /// 命名实体（不含 `&`、`;`）。
    Named(&'a str, usize),
    /// 不是引用：裸 `&`。
    Bare,
}

fn parse_ref(s: &str) -> Ref<'_> {
    let b = s.as_bytes();
    if b.get(1) == Some(&b'#') {
        let (hex, start) = if matches!(b.get(2), Some(b'x' | b'X')) { (true, 3) } else { (false, 2) };
        let mut i = start;
        while i < b.len() && i < start + 8 && (if hex { b[i].is_ascii_hexdigit() } else { b[i].is_ascii_digit() }) {
            i += 1;
        }
        if i == start || b.get(i) != Some(&b';') {
            return Ref::Bare;
        }
        let n = u32::from_str_radix(&s[start..i], if hex { 16 } else { 10 }).ok();
        return match n.and_then(char::from_u32) {
            Some(c) if crate::util::is_xml_char(c) => Ref::Keep(i + 1),
            _ => Ref::Illegal(i + 1),
        };
    }
    let mut i = 1;
    while i < b.len() && i < 40 && (b[i].is_ascii_alphanumeric() || (i > 1 && matches!(b[i], b'.' | b'-' | b'_'))) {
        i += 1;
    }
    if i == 1 || !b[1].is_ascii_alphabetic() || b.get(i) != Some(&b';') {
        return Ref::Bare;
    }
    match &s[1..i] {
        "amp" | "lt" | "gt" | "quot" | "apos" => Ref::Keep(i + 1),
        name => Ref::Named(name, i + 1),
    }
}

/// 一段字符数据（标签之间的文字或属性值原文）的修复：命名实体 → 数字引用、裸 `&`、裸 `<`、非法数字引用。
/// `quote_dq`：无引号的属性值（写回时要加双引号，值里的 `"` 要转义）。`in_attr`：属性值（里面的 `<` 一律转义，见下）。
/// 没有要改的原样借用。
fn fix_chars<'a>(s: &'a str, quote_dq: bool, in_attr: bool, fx: &mut XmlFixes) -> Cow<'a, str> {
    if !s.bytes().any(|b| b == b'&' || b == b'<' || (quote_dq && b == b'"')) {
        return Cow::Borrowed(s);
    }
    let mut out = String::with_capacity(s.len() + 16);
    let mut changed = false;
    let mut i = 0;
    while let Some(k) = s[i..].find(['&', '<', '"']).map(|k| k + i) {
        out.push_str(&s[i..k]);
        let rest = &s[k..];
        match rest.as_bytes()[0] {
            b'&' => match parse_ref(rest) {
                Ref::Keep(n) => {
                    out.push_str(&rest[..n]);
                    i = k + n;
                    continue;
                }
                Ref::Illegal(n) => {
                    fx.control_chars += 1;
                    changed = true;
                    i = k + n;
                    continue;
                }
                Ref::Named(name, n) => {
                    match HTML4_ENTITIES.binary_search_by(|(e, _)| e.cmp(&name)) {
                        Ok(p) => {
                            out.push_str(&format!("&#{};", HTML4_ENTITIES[p].1));
                            fx.named_entities += 1;
                            changed = true;
                        }
                        Err(_) => {
                            out.push_str(&rest[..n]);
                            fx.unknown_entities += 1;
                        }
                    }
                    i = k + n;
                    continue;
                }
                Ref::Bare => {
                    out.push_str("&amp;");
                    fx.bare_amps += 1;
                    changed = true;
                }
            },
            b'<' => {
                // 标签之间：后面是字母、`/`、`!`、`?` 的可能是扫描器没认出来的标签（引号没配对之类），拿不准，不动。
                // 属性值里（扫描器已经按引号认出了值的范围）`<` 在 XML 里一律不合法（`alt="<b>x</b>"` 会让 xochitl
                // 整章白屏，2026-09-30 审计），全部转义。
                if !in_attr && rest.as_bytes().get(1).is_some_and(|c| c.is_ascii_alphabetic() || matches!(c, b'/' | b'!' | b'?')) {
                    out.push('<');
                } else {
                    out.push_str("&lt;");
                    fx.bare_lts += 1;
                    changed = true;
                }
            }
            _ => {
                if quote_dq {
                    out.push_str("&quot;");
                    changed = true;
                } else {
                    out.push('"');
                }
            }
        }
        i = k + 1;
    }
    out.push_str(&s[i..]);
    if changed { Cow::Owned(out) } else { Cow::Borrowed(s) }
}

/// 开标签（或自闭合标签）原文的修复：属性值里的字符、无引号/无值的属性。
fn fix_start_tag<'a>(raw: &'a str, fx: &mut XmlFixes) -> Cow<'a, str> {
    let mut edits: Vec<(usize, usize, String)> = Vec::new();
    for a in html::attrs(raw) {
        match a.quote {
            Some(_) => {
                // 引号里的值：同种引号不会出现在值里，只修实体、`&`、`<`。
                if let Cow::Owned(v) = fix_chars(a.value, false, true, fx) {
                    edits.push((a.value_start, a.value_end, v));
                }
            }
            None if a.value_end > a.value_start => {
                // 无引号的值（`width=100`）：补双引号。
                let v = fix_chars(a.value, true, true, fx);
                edits.push((a.value_start, a.value_end, format!("\"{v}\"")));
                fx.attrs_quoted += 1;
            }
            None => {
                // 无值属性（`<td nowrap>`）：XHTML 的写法是 `nowrap="nowrap"`。名字里有奇怪字符的（杂散引号等）不动。
                if !a.name.is_empty() && a.name.bytes().all(|c| c.is_ascii_alphanumeric() || matches!(c, b':' | b'_' | b'-' | b'.')) {
                    edits.push((a.end, a.end, format!("=\"{}\"", a.name)));
                    fx.attrs_quoted += 1;
                }
            }
        }
    }
    if edits.is_empty() {
        return Cow::Borrowed(raw);
    }
    Cow::Owned(html::apply_edits(raw, edits))
}

/// 全角 ASCII（U+FF01–U+FF5E）换成半角。
fn fold_fullwidth(s: &str) -> String {
    s.chars().map(crate::util::to_halfwidth).collect()
}

/// 标签配对是否完全平衡（按原文大小写严格比较，与 XML 一致）：每个闭合标签都关掉栈顶，结束时栈空。空元素须已自闭合或紧跟闭合标签。
fn tags_balanced(text: &str) -> bool {
    let mut stack: Vec<&str> = Vec::new();
    for t in html::tags(text) {
        match t.kind {
            html::TagKind::Open => stack.push(t.name),
            html::TagKind::Close if stack.pop() != Some(t.name) => return false,
            _ => {}
        }
    }
    stack.is_empty()
}

/// 一份 XHTML（`xhtml = true`）或 OPF/NCX 的 XML 修复，见模块说明。没有要改的原样借用。
pub(crate) fn normalize_markup<'a>(text: &'a str, xhtml: bool, fx: &mut XmlFixes) -> Cow<'a, str> {
    // XML 不允许的字符：先整份去掉（注释里的也算）。
    let cleaned: Cow<str> = if text.chars().all(crate::util::is_xml_char) {
        Cow::Borrowed(text)
    } else {
        let before = text.chars().count();
        let s: String = text.chars().filter(|&c| crate::util::is_xml_char(c)).collect();
        fx.control_chars += before - s.chars().count();
        Cow::Owned(s)
    };
    let mut trial = fx.clone();
    let mut out = markup_pass(&cleaned, xhtml, true, &mut trial);
    if trial.stray_close_tags > 0 && !tags_balanced(&out) {
        // 去掉多余闭合标签也没能让标签配平：拿不准，一个都不去。
        trial = fx.clone();
        out = markup_pass(&cleaned, xhtml, false, &mut trial);
    }
    *fx = trial;
    if out == text {
        Cow::Borrowed(text)
    } else {
        Cow::Owned(out)
    }
}

fn markup_pass(text: &str, xhtml: bool, drop_stray: bool, fx: &mut XmlFixes) -> String {
    let tags: Vec<html::Tag> = html::tags(text).collect();
    let mut out = String::with_capacity(text.len() + 256);
    let mut stack: Vec<&str> = Vec::new();
    let (mut pos, mut seen_root) = (0usize, false);
    for (k, t) in tags.iter().enumerate() {
        out.push_str(&fix_chars(&text[pos..t.start], false, false, fx));
        pos = t.end;
        let raw = &text[t.start..t.end];
        match t.kind {
            html::TagKind::Other => {
                // 根元素之前的 DOCTYPE 换成 EPUB 3 的写法；带内部子集（`[…]`）的拿不准，不动。
                let is_doctype = raw.len() >= 9 && raw.as_bytes()[..9].eq_ignore_ascii_case(b"<!doctype");
                if xhtml && is_doctype && !seen_root && !raw.contains('[') {
                    if raw != "<!DOCTYPE html>" {
                        fx.doctypes += 1;
                    }
                    out.push_str("<!DOCTYPE html>");
                } else {
                    out.push_str(raw);
                }
            }
            html::TagKind::Close => {
                if !xhtml {
                    out.push_str(raw);
                    continue;
                }
                match stack.iter().rposition(|n| n.eq_ignore_ascii_case(t.name)) {
                    Some(p) => {
                        stack.truncate(p);
                        out.push_str(raw);
                    }
                    None if drop_stray => fx.stray_close_tags += 1,
                    None => out.push_str(raw),
                }
            }
            html::TagKind::Open | html::TagKind::SelfClosing => {
                let mut tag = fix_start_tag(raw, fx).into_owned();
                if xhtml && !seen_root && t.is("html") {
                    for (name, ns) in [("xmlns", XHTML_NS), ("xmlns:epub", OPS_NS)] {
                        // 没有就补；写坏的（全角标点混进去了，换回半角就是它）改正。别的值是作者有意写的，不动。
                        let fix = match html::attr_value(&tag, name) {
                            None => true,
                            Some(v) => v != ns && fold_fullwidth(v.trim()) == ns,
                        };
                        if fix {
                            tag = html::set_attr(&tag, name, ns);
                            fx.namespaces_fixed += 1;
                        }
                    }
                }
                seen_root |= xhtml;
                if t.kind == html::TagKind::Open && xhtml {
                    let closed_next = tags.get(k + 1).is_some_and(|n| n.kind == html::TagKind::Close && n.name.eq_ignore_ascii_case(t.name));
                    if html::is_void(t.name) && !closed_next && tag.ends_with('>') {
                        tag.insert(tag.len() - 1, '/');
                        fx.void_tags_closed += 1;
                    } else {
                        stack.push(t.name);
                    }
                }
                out.push_str(&tag);
            }
        }
    }
    out.push_str(&fix_chars(&text[pos..], false, false, fx));
    out
}

// ───────────────────────── OPF 升级 ─────────────────────────

/// 一个元数据元素（`<dc:…>…</dc:…>`）：开标签、闭合标签的位置。
struct MetaEl<'a> {
    name: &'a str,
    start: usize,
    open_end: usize,
    close_start: usize,
    end: usize,
}

fn metadata_elements(opf: &str) -> Vec<MetaEl<'_>> {
    let Some(m) = html::tags(opf).find(|t| t.kind == html::TagKind::Open && opf::is_local(t.name, "metadata")) else { return Vec::new() };
    let Some(mc) = html::tags_in(opf, m.end, opf.len()).find(|t| t.kind == html::TagKind::Close && opf::is_local(t.name, "metadata")) else { return Vec::new() };
    let mut out = Vec::new();
    let mut it = html::tags_in(opf, m.end, mc.start);
    while let Some(t) = it.next() {
        if !t.is_start() {
            continue;
        }
        if t.kind == html::TagKind::SelfClosing {
            out.push(MetaEl { name: t.name, start: t.start, open_end: t.end, close_start: t.end, end: t.end });
            continue;
        }
        let Some(c) = html::find_close(opf, t.end, t.name).filter(|c| c.end <= mc.start) else { continue };
        out.push(MetaEl { name: t.name, start: t.start, open_end: t.end, close_start: c.start, end: c.end });
        // 跳过元素内部
        it = html::tags_in(opf, c.end, mc.start);
    }
    out
}

/// FNV-1a 64 位：书没有任何标识符时，按 OPF 原文派生一个确定的标识符。
fn fnv64(b: &[u8]) -> u64 {
    b.iter().fold(0xcbf29ce484222325u64, |h, &c| (h ^ c as u64).wrapping_mul(0x100000001b3))
}

/// `dcterms:modified` 的格式：`CCYY-MM-DDThh:mm:ssZ`。
fn is_w3c_utc(v: &str) -> bool {
    let b = v.as_bytes();
    b.len() == 20 && b.iter().enumerate().all(|(i, &c)| match i {
        4 | 7 => c == b'-',
        10 => c == b'T',
        13 | 16 => c == b':',
        19 => c == b'Z',
        _ => c.is_ascii_digit(),
    })
}

/// OPF 升级到 EPUB 3（见模块说明）。什么都不用改 → `None`。
pub(crate) fn upgrade_opf(opf: &str, lang_tag: &str) -> Option<String> {
    let pkg = html::tags(opf).find(|t| t.is_start() && opf::is_local(t.name, "package"))?;
    let pkg_tag = &opf[pkg.start..pkg.end];
    let was3 = html::attr_value(pkg_tag, "version").is_some_and(|v| v.trim().starts_with('3'));
    let els = metadata_elements(opf);
    // OPF 里已有的全部 id（新起的 id 不能撞）
    let mut ids: HashSet<String> = html::tags(opf).filter(|t| t.is_start()).filter_map(|t| html::attr_value(&opf[t.start..t.end], "id").map(str::to_string)).collect();
    let mut new_id = |base: &str| -> String {
        let mut n = 1;
        let mut id = base.to_string();
        while ids.contains(&id) {
            n += 1;
            id = format!("{base}-{n}");
        }
        ids.insert(id.clone());
        id
    };
    let mut edits: Vec<(usize, usize, String)> = Vec::new();
    let mut add_meta = String::new(); // 插到 </metadata> 前
    let is_dc = |e: &MetaEl, local: &str| e.name.eq_ignore_ascii_case(&format!("dc:{local}"));

    // 1. EPUB 2 的 opf: 属性 → <meta refines>；多余的 dc:date → dcterms:date。已经是 3.x 的不动。
    // 每个元素最终的开标签（后面补 id 时在它上面改）
    let mut opens: Vec<String> = els.iter().map(|e| opf[e.start..e.open_end].to_string()).collect();
    let mut dropped: HashSet<usize> = HashSet::new();
    if !was3 {
        let dates: Vec<usize> = (0..els.len()).filter(|&i| is_dc(&els[i], "date")).collect();
        let keep_date = dates.iter().copied().find(|&i| html::attr_value(&opens[i], "opf:event").is_some_and(|v| v.eq_ignore_ascii_case("publication"))).or(dates.first().copied());
        for &i in dates.iter().filter(|&&i| Some(i) != keep_date) {
            let v = opf[els[i].open_end..els[i].close_start].trim();
            if !v.is_empty() {
                add_meta.push_str(&format!(r#"<meta property="dcterms:date">{v}</meta>"#));
            }
            dropped.insert(i);
        }
        for (i, e) in els.iter().enumerate() {
            if dropped.contains(&i) || !e.name.to_ascii_lowercase().starts_with("dc:") {
                continue;
            }
            let tag = opens[i].clone();
            let (role, file_as, scheme) = (html::attr_value(&tag, "opf:role"), html::attr_value(&tag, "opf:file-as"), html::attr_value(&tag, "opf:scheme"));
            let has_event = html::attr(&tag, "opf:event").is_some();
            if role.is_none() && file_as.is_none() && scheme.is_none() && !has_event {
                continue;
            }
            let mut t = tag.clone();
            for a in ["opf:role", "opf:file-as", "opf:scheme", "opf:event"] {
                t = html::remove_attr(&t, a);
            }
            let refines: Vec<String> = [(role.filter(|v| !v.trim().is_empty()), r#" scheme="marc:relators""#, "role"), (file_as.filter(|v| !v.trim().is_empty()), "", "file-as"), (scheme.filter(|v| !v.trim().is_empty()), "", "identifier-type")]
                .into_iter()
                .filter_map(|(v, extra, prop)| v.map(|v| (v, extra, prop)))
                .map(|(v, extra, prop)| format!(r#" property="{prop}"{extra}>{}</meta>"#, v.trim()))
                .collect();
            if !refines.is_empty() {
                let id = match html::attr_value(&t, "id").filter(|v| !v.is_empty()) {
                    Some(id) => id.to_string(),
                    None => {
                        let id = new_id("eink-meta");
                        t = html::set_attr(&t, "id", &id);
                        id
                    }
                };
                for r in refines {
                    add_meta.push_str(&format!(r##"<meta refines="#{id}"{r}"##));
                }
            }
            opens[i] = t;
        }
    }

    // 2. unique-identifier 指向一个非空的 <dc:identifier id>。
    let idents: Vec<usize> = (0..els.len()).filter(|&i| is_dc(&els[i], "identifier") && !dropped.contains(&i)).collect();
    let text_of = |i: usize| opf[els[i].open_end..els[i].close_start].trim();
    let uid_attr = html::attr_value(pkg_tag, "unique-identifier");
    let uid_ok = uid_attr.is_some_and(|u| idents.iter().any(|&i| html::attr_value(&opens[i], "id") == Some(u) && !text_of(i).is_empty()));
    let mut new_uid: Option<String> = None;
    if !uid_ok {
        match idents.iter().copied().find(|&i| !text_of(i).is_empty()) {
            Some(i) => {
                let id = match html::attr_value(&opens[i], "id").filter(|v| !v.is_empty()) {
                    Some(id) => id.to_string(),
                    None => {
                        let id = new_id("eink-uid");
                        opens[i] = html::set_attr(&opens[i], "id", &id);
                        id
                    }
                };
                new_uid = Some(id);
            }
            None => {
                let id = new_id("eink-uid");
                add_meta.push_str(&format!(r#"<dc:identifier id="{id}">urn:eink:{:016x}</dc:identifier>"#, fnv64(opf.as_bytes())));
                new_uid = Some(id);
            }
        }
    }
    // 3. 缺 dc:language 就补。
    if !els.iter().any(|e| is_dc(e, "language") && !opf[e.open_end..e.close_start].trim().is_empty()) {
        add_meta.push_str(&format!("<dc:language>{}</dc:language>", xml_escape(lang_tag)));
    }
    // 4. dcterms:modified：原书有格式正确的就沿用，格式不对的改成固定值，没有就补。
    let modified: Vec<&MetaEl> = els.iter().filter(|e| opf::is_local(e.name, "meta") && html::attr_value(&opf[e.start..e.open_end], "property") == Some("dcterms:modified")).collect();
    match modified.first() {
        Some(m) if is_w3c_utc(opf[m.open_end..m.close_start].trim()) => {}
        Some(m) if m.end > m.open_end => edits.push((m.open_end, m.close_start, EPUB3_MODIFIED.to_string())),
        Some(m) => {
            // 自闭合的 `<meta property="dcterms:modified"/>`：没有内容，整个换掉。
            let open = opf[m.start..m.open_end].trim_end_matches('>').trim_end_matches('/').trim_end();
            edits.push((m.start, m.end, format!("{open}>{EPUB3_MODIFIED}</{}>", m.name)));
        }
        _ => add_meta.push_str(&format!(r#"<meta property="dcterms:modified">{EPUB3_MODIFIED}</meta>"#)),
    }
    for (i, e) in els.iter().enumerate() {
        if dropped.contains(&i) {
            let ws = opf[e.end..].len() - opf[e.end..].trim_start().len();
            edits.push((e.start, e.end + ws, String::new()));
        } else if opens[i] != opf[e.start..e.open_end] {
            edits.push((e.start, e.open_end, opens[i].clone()));
        }
    }
    // 5. package：version、unique-identifier、默认命名空间。
    let mut new_pkg = pkg_tag.to_string();
    if !was3 {
        new_pkg = html::set_attr(&new_pkg, "version", "3.0");
    }
    if let Some(u) = &new_uid {
        new_pkg = html::set_attr(&new_pkg, "unique-identifier", u);
    }
    if pkg.name.eq_ignore_ascii_case("package") && html::attr(&new_pkg, "xmlns").is_none() {
        new_pkg = html::set_attr(&new_pkg, "xmlns", OPF_NS);
    }
    if new_pkg != pkg_tag {
        edits.push((pkg.start, pkg.end, new_pkg));
    }
    if edits.is_empty() && add_meta.is_empty() {
        return None;
    }
    edits.sort_by_key(|e| e.0);
    let mut out = html::apply_edits(opf, edits);
    if !add_meta.is_empty() {
        // 新写的 dc: 元素要有 dc 命名空间：原书一个 dc 元素都没有时补在 metadata 上。
        if add_meta.contains("<dc:") && !out.contains("xmlns:dc=") {
            if let Some(m) = html::tags(&out).find(|t| t.kind == html::TagKind::Open && opf::is_local(t.name, "metadata")) {
                let tag = html::set_attr(&out[m.start..m.end], "xmlns:dc", DC_NS);
                out = format!("{}{tag}{}", &out[..m.start], &out[m.end..]);
            }
        }
        out = opf::insert_metadata(&out, &add_meta).unwrap_or(out);
    }
    Some(out)
}

// ───────────────────────── 导航文档 ─────────────────────────

/// guide 的 `type` → landmarks 的 `epub:type`（EPUB 3 结构语义词表）。认不出的类型不写进 landmarks（guide 里照留）。
fn landmark_type(guide_type: &str) -> Option<&'static str> {
    Some(match guide_type.trim().to_ascii_lowercase().as_str() {
        "cover" => "cover",
        "title-page" => "titlepage",
        "toc" => "toc",
        "text" | "start" | "bodymatter" => "bodymatter",
        "index" => "index",
        "glossary" => "glossary",
        "acknowledgements" | "acknowledgments" => "acknowledgments",
        "bibliography" => "bibliography",
        "colophon" => "colophon",
        "copyright-page" => "copyright-page",
        "dedication" => "dedication",
        "epigraph" => "epigraph",
        "foreword" => "foreword",
        "preface" => "preface",
        "loi" => "loi",
        "lot" => "lot",
        _ => return None,
    })
}

/// 没有导航文档就按 NCX 生成一份并登记（`properties="nav"`）；再把 `<guide>` 写成 nav 里的 landmarks。
pub(super) fn ensure_nav(entries: &mut Vec<Entry>, heading: &str, rep: &mut WashReport) {
    let Some(opf) = parse_opf(entries) else { return };
    let nav_path = match opf.nav_doc.clone() {
        Some(p) => p,
        None => {
            let mut items: Vec<toc::TocItem> = Vec::new();
            if let Some(ncx_path) = opf.ncx.as_deref() {
                if let Some(e) = entries.iter().find(|e| e.name == ncx_path) {
                    for p in crate::ncx::parse_nav_points(&String::from_utf8_lossy(&e.data)) {
                        let label = p.label.trim();
                        if label.is_empty() {
                            continue; // nav 里的链接必须有文字
                        }
                        let (path, frag) = crate::epubzip::resolve_href(ncx_path, &p.src);
                        items.push(toc::TocItem::new(p.depth.clamp(1, 255) as u8, label, path, frag.unwrap_or("")));
                    }
                }
            }
            if items.is_empty() {
                // 没有可用的目录：一条指向第一章，标题用书名（EPUB 3 要求 nav 的 toc 至少一条）。
                let Some(first) = opf.spine.first() else { return };
                items.push(toc::TocItem::new(1, opf_book_title(entries, opf.index), first.clone(), ""));
            }
            let taken: HashSet<&str> = entries.iter().map(|e| e.name.as_str()).collect();
            let path = ["nav.xhtml", "eink-nav.xhtml"].iter().map(|n| resolve(&opf.dir, n)).find(|p| !taken.contains(p.as_str()));
            let Some(path) = path else { return };
            let text = String::from_utf8_lossy(&entries[opf.index].data).into_owned();
            let ids: HashSet<&str> = manifest_items(&text).iter().map(|i| i.id).collect();
            let id = ["eink-nav", "eink-nav-2", "eink-nav-3"].into_iter().find(|i| !ids.contains(i)).unwrap_or("eink-nav-x");
            let href = crate::epubzip::href_to(&opf.dir, &path, "");
            let Some(t) = opf::insert_manifest_items(&text, &[opf::NewItem { id, href: &href, media_type: "application/xhtml+xml", properties: "nav" }]) else { return };
            entries[opf.index].data = t.into_bytes();
            let nav = toc::build_nav(&items, dir_of(&path), heading);
            entries.push(Entry { name: path.clone(), data: nav.into_bytes() });
            rep.nav_generated = items.len();
            path
        }
    };
    if !opf.spine.contains(&nav_path) {
        rep.landmarks_added = add_landmarks(entries, &nav_path);
    }
    if opf.ncx.is_none() {
        rep.ncx_generated = ncx_from_nav(entries, &nav_path);
    }
}

/// 导航文档 `<nav epub:type="toc">` 里的目录条目（`<ol>` 嵌套深度当级别；没有 `href` 的 `<span>` 标题跳过）。
pub fn nav_toc_items(doc: &str, nav_path: &str) -> Vec<toc::TocItem> {
    let spans = html::parse_spans(doc, 0, doc.len());
    let Some(nav) = spans.iter().find(|s| s.name == "nav" && html::attr_value(&doc[s.open_start..s.open_end], "epub:type").is_some_and(|t| t.split_whitespace().any(|x| x == "toc"))) else { return Vec::new() };
    let mut out = Vec::new();
    for (i, a) in spans.iter().enumerate().filter(|(_, s)| s.name == "a" && s.open_start >= nav.open_end && s.close_end <= nav.close_start) {
        let Some(href) = html::attr_value(&doc[a.open_start..a.open_end], "href") else { continue };
        let label = plain_text(&doc[a.open_end..a.close_start]);
        if label.is_empty() || html::is_external(html::split_href(href).0) {
            continue;
        }
        let mut depth = 0u8;
        let mut p = spans[i].parent;
        while let Some(j) = p {
            if spans[j].name == "ol" {
                depth = depth.saturating_add(1);
            }
            p = spans[j].parent;
        }
        let href = crate::util::xml_unescape(href);
        let (path, frag) = crate::epubzip::resolve_href(nav_path, &href);
        out.push(toc::TocItem::new(depth.max(1), label, path, frag.unwrap_or("")));
    }
    out
}

/// 没有 NCX 的书（EPUB 3 原书只有 nav）按 nav 生成一份 `toc.ncx`，manifest id 叫 `ncx`，`<spine toc="ncx">`——
/// reMarkable xochitl 找目录只认 manifest 里 id 为 `ncx` 的条目（见 `ncx_fix::fix_ncx_manifest_id`）。返回条目数。
fn ncx_from_nav(entries: &mut Vec<Entry>, nav_path: &str) -> usize {
    let Some(opf) = parse_opf(entries) else { return 0 };
    let Some(nav) = entries.iter().find(|e| e.name == nav_path) else { return 0 };
    let items = nav_toc_items(&String::from_utf8_lossy(&nav.data), nav_path);
    if items.is_empty() {
        return 0;
    }
    let text = String::from_utf8_lossy(&entries[opf.index].data).into_owned();
    let ids: HashSet<&str> = manifest_items(&text).iter().map(|i| i.id).collect();
    let taken: HashSet<&str> = entries.iter().map(|e| e.name.as_str()).collect();
    let Some(path) = ["toc.ncx", "eink-toc.ncx"].iter().map(|n| resolve(&opf.dir, n)).find(|p| !taken.contains(p.as_str())) else { return 0 };
    let Some(id) = ["ncx", "eink-ncx"].into_iter().find(|i| !ids.contains(i)) else { return 0 };
    let href = crate::epubzip::href_to(&opf.dir, &path, "");
    let Some(mut t) = opf::insert_manifest_items(&text, &[opf::NewItem { id, href: &href, media_type: "application/x-dtbncx+xml", properties: "" }]) else { return 0 };
    if let Some(sp) = html::tags(&t).find(|g| g.is_start() && opf::is_local(g.name, "spine")) {
        let tag = html::set_attr(&t[sp.start..sp.end], "toc", id);
        t = format!("{}{tag}{}", &t[..sp.start], &t[sp.end..]);
    }
    entries[opf.index].data = t.into_bytes();
    // dtb:uid 用 OPF 标识符（升级时已保证有）；`fix_ncx_uid` 随后再核一遍。
    let uid = opf_unique_identifier(entries).unwrap_or_default();
    let ncx = toc::build_ncx(&items, dir_of(&path), &opf_book_title(entries, opf.index), &uid);
    entries.push(Entry { name: path, data: ncx.into_bytes() });
    items.len()
}

/// 把 OPF `<guide>` 写成导航文档里的 `<nav epub:type="landmarks">`（nav 已有 landmarks、guide 为空或都认不出时不动）。返回写了几条。
fn add_landmarks(entries: &mut [Entry], nav_path: &str) -> usize {
    let Some(oi) = find_opf(entries) else { return 0 };
    let opf_name = entries[oi].name.clone();
    let opf_text = String::from_utf8_lossy(&entries[oi].data).into_owned();
    let names: HashSet<&str> = entries.iter().map(|e| e.name.as_str()).collect();
    let mut seen: HashSet<(String, String)> = HashSet::new();
    let mut lis = String::new();
    let mut n = 0;
    // `<guide>` 的范围（第一个开标签到第一个闭合标签）只找一次
    let open = html::tags(&opf_text).find(|t| t.kind == html::TagKind::Open && opf::is_local(t.name, "guide"));
    let close = html::tags(&opf_text).find(|t| t.kind == html::TagKind::Close && opf::is_local(t.name, "guide"));
    let Some((lo, hi)) = open.zip(close).map(|(o, c)| (o.end, c.start)).filter(|(lo, hi)| lo <= hi) else { return 0 };
    for t in html::tags_in(&opf_text, lo, hi).filter(|t| t.is_start() && opf::is_local(t.name, "reference")) {
        let tag = &opf_text[t.start..t.end];
        let (Some(ty), Some(href)) = (html::attr_value(tag, "type").and_then(landmark_type), html::attr_value(tag, "href")) else { continue };
        let href = crate::util::xml_unescape(href);
        if html::is_external(html::split_href(&href).0) {
            continue;
        }
        let (path, frag) = crate::epubzip::resolve_href(&opf_name, &href);
        if !names.contains(path.as_str()) {
            continue;
        }
        let link = crate::epubzip::href_to(dir_of(nav_path), &path, frag.unwrap_or(""));
        if !seen.insert((ty.to_string(), link.clone())) {
            continue;
        }
        let title = html::attr_value(tag, "title").map(|v| crate::util::xml_unescape(v).trim().to_string()).filter(|v| !v.is_empty()).unwrap_or_else(|| ty.to_string());
        lis.push_str(&format!(r#"<li><a epub:type="{ty}" href="{}">{}</a></li>"#, xml_escape(&link), xml_escape(&title)));
        n += 1;
    }
    if n == 0 {
        return 0;
    }
    let Some(e) = entries.iter_mut().find(|e| e.name == nav_path) else { return 0 };
    let Ok(doc) = std::str::from_utf8(&e.data) else { return 0 };
    let has_landmarks = html::tags(doc).any(|t| t.is_start() && t.is("nav") && html::attr_value(&doc[t.start..t.end], "epub:type").is_some_and(|v| v.split_whitespace().any(|x| x == "landmarks")));
    let Some(close) = html::tags(doc).filter(|t| t.kind == html::TagKind::Close && t.is("body")).last() else { return 0 };
    if has_landmarks {
        return 0;
    }
    let block = format!(r#"<nav epub:type="landmarks" id="landmarks" hidden=""><ol>{lis}</ol></nav>"#);
    e.data = format!("{}{block}{}", &doc[..close.start], &doc[close.start..]).into_bytes();
    n
}

// ───────────────────────── manifest properties ─────────────────────────

/// 内容文档需要在 manifest 里声明的特性（EPUB 3：用了就必须声明，没用就不许声明）。
pub const PROP_SVG: u8 = 1;
pub const PROP_MATHML: u8 = 2;
pub const PROP_SCRIPTED: u8 = 4;
pub const PROP_REMOTE: u8 = 8;
const MANAGED: [(u8, &str); 4] = [(PROP_MATHML, "mathml"), (PROP_REMOTE, "remote-resources"), (PROP_SCRIPTED, "scripted"), (PROP_SVG, "svg")];

/// 一份 XHTML 用到的特性：内嵌 `<svg>`、`<math>`、`<script>`、远程资源（`src` 是 http(s) 或 `//` 的图片/音视频等、
/// 远程样式表、SVG `<image>` 的远程 `href`）。
pub fn content_properties(html_text: &str) -> u8 {
    let mut f = 0;
    let remote = |v: &str| {
        let v = v.trim();
        v.starts_with("http://") || v.starts_with("https://") || v.starts_with("//")
    };
    for t in html::tags(html_text).filter(|t| t.is_start()) {
        let tag = &html_text[t.start..t.end];
        if opf::is_local(t.name, "svg") {
            f |= PROP_SVG;
        } else if opf::is_local(t.name, "math") {
            f |= PROP_MATHML;
        } else if t.is("script") {
            f |= PROP_SCRIPTED;
        }
        let res = match t.name.to_ascii_lowercase().as_str() {
            "img" | "audio" | "video" | "source" | "track" | "embed" | "iframe" | "script" | "input" => html::attr_value(tag, "src").is_some_and(remote),
            "link" => html::attr_value(tag, "rel").is_some_and(|r| r.to_ascii_lowercase().contains("stylesheet")) && html::attr_value(tag, "href").is_some_and(remote),
            n if opf::is_local(n, "image") => ["xlink:href", "href"].iter().any(|a| html::attr_value(tag, a).is_some_and(remote)),
            _ => false,
        };
        if res {
            f |= PROP_REMOTE;
        }
    }
    f
}

/// 按各内容文档的最终特性（zip 路径 → [`content_properties`]）改 manifest 的 `properties`：只管 `svg`/`mathml`/`scripted`/
/// `remote-resources` 这四个，其它（`nav`、`cover-image`……）原样。只改 EPUB 3 的 OPF；不用改 → `None`。
pub fn apply_content_properties(opf_text: &str, opf_dir: &str, props: &HashMap<String, u8>) -> Option<String> {
    let pkg = html::tags(opf_text).find(|t| t.is_start() && opf::is_local(t.name, "package"))?;
    if !html::attr_value(&opf_text[pkg.start..pkg.end], "version").is_some_and(|v| v.trim().starts_with('3')) {
        return None;
    }
    let mut edits: Vec<(usize, usize, String)> = Vec::new();
    for it in manifest_items(opf_text) {
        let Some(&f) = props.get(&it.path(opf_dir)) else { continue };
        let mut kept: Vec<&str> = it.properties.split_whitespace().filter(|p| !MANAGED.iter().any(|(_, m)| m == p)).collect();
        kept.extend(MANAGED.iter().filter(|(b, _)| f & b != 0).map(|(_, m)| *m));
        let new = kept.join(" ");
        if new == it.properties.split_whitespace().collect::<Vec<_>>().join(" ") {
            continue;
        }
        let tag = if new.is_empty() { html::remove_attr(it.tag, "properties") } else { html::set_attr(it.tag, "properties", &new) };
        edits.push((it.pos, it.pos + it.tag.len(), tag));
    }
    if edits.is_empty() {
        return None;
    }
    Some(html::apply_edits(opf_text, edits))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn e(name: &str, data: &str) -> Entry {
        Entry { name: name.into(), data: data.as_bytes().to_vec() }
    }
    fn s(entries: &[Entry], name: &str) -> String {
        String::from_utf8(entries.iter().find(|e| e.name == name).unwrap_or_else(|| panic!("缺 {name}")).data.clone()).unwrap()
    }
    fn fix(t: &str) -> (String, XmlFixes) {
        let mut fx = XmlFixes::default();
        let out = normalize_markup(t, true, &mut fx).into_owned();
        (out, fx)
    }
    /// 合法 XML（测试用）：quick-xml 查结构 + 标签完全配平 + 只有 XML 预定义实体 + 没有非法字符。
    fn well_formed(t: &str) -> bool {
        let mut r = quick_xml::Reader::from_str(t);
        loop {
            match r.read_event() {
                Ok(quick_xml::events::Event::Eof) => break,
                Ok(_) => {}
                Err(_) => return false,
            }
        }
        let mut fx = XmlFixes::default();
        let chars_ok = matches!(fix_chars(t, false, false, &mut fx), Cow::Borrowed(_)) && fx.unknown_entities == 0;
        tags_balanced(t) && t.chars().all(crate::util::is_xml_char) && chars_ok
    }

    #[test]
    fn entities_amps_and_lts_become_xml() {
        let (out, fx) = fix("<html><body><p title='a&nbsp;b &amp; c&d'>甲&nbsp;乙&hellip;&amp;&#12288;&#x3000;&copy; A&B a < b &foo; &#16;</p></body></html>");
        assert_eq!(
            out,
            "<html xmlns=\"http://www.w3.org/1999/xhtml\" xmlns:epub=\"http://www.idpf.org/2007/ops\"><body><p title='a&#160;b &amp; c&amp;d'>甲&#160;乙&#8230;&amp;&#12288;&#x3000;&#169; A&amp;B a &lt; b &foo; </p></body></html>"
        );
        assert_eq!((fx.named_entities, fx.bare_amps, fx.bare_lts, fx.unknown_entities, fx.control_chars, fx.namespaces_fixed), (4, 2, 1, 1, 1, 2));
    }

    #[test]
    fn control_chars_dropped_and_doctype_replaced() {
        let garbled = "<?xml version=\"1.0\"?>\n<!DOCTYpE html pUBLIC \"-//W4C//DTD XHTML 1.0 Transitional//EN\" \"http://www.w3.org/TR/xhtml1/DTD/xhtml1-transitional.dtd\">\n<html xmlns=\"http：//www.w3.org/1999/xhtml\"><body><p>剧震，\u{10}感到</p></body></html>";
        let (out, fx) = fix(garbled);
        assert_eq!(out, "<?xml version=\"1.0\"?>\n<!DOCTYPE html>\n<html xmlns=\"http://www.w3.org/1999/xhtml\" xmlns:epub=\"http://www.idpf.org/2007/ops\"><body><p>剧震，感到</p></body></html>");
        assert_eq!((fx.doctypes, fx.control_chars, fx.namespaces_fixed), (1, 1, 2));
        assert!(well_formed(&out), "{out}");
        // 带内部子集的 DOCTYPE 拿不准，不动；别的命名空间值是作者写的，不动
        let internal = "<!DOCTYPE html [<!ENTITY x \"y\">]><html xmlns=\"urn:other\"><body/></html>";
        assert!(fix(internal).0.starts_with("<!DOCTYPE html [<!ENTITY x \"y\">]><html xmlns=\"urn:other\" xmlns:epub="), "{}", fix(internal).0);
    }

    #[test]
    fn void_tags_and_bare_attrs_fixed() {
        let (out, fx) = fix(r#"<html xmlns="http://www.w3.org/1999/xhtml" xmlns:epub="http://www.idpf.org/2007/ops"><body><p>一<br>二<br/><img src=a.png alt="x"><img src="b.png"></img></p><table><tr><td nowrap>三</td></tr></table></body></html>"#);
        assert_eq!(out, r#"<html xmlns="http://www.w3.org/1999/xhtml" xmlns:epub="http://www.idpf.org/2007/ops"><body><p>一<br/>二<br/><img src="a.png" alt="x"/><img src="b.png"></img></p><table><tr><td nowrap="nowrap">三</td></tr></table></body></html>"#);
        assert_eq!((fx.void_tags_closed, fx.attrs_quoted, fx.namespaces_fixed), (2, 2, 0));
        assert!(well_formed(&out), "{out}");
    }

    /// 《绝叫》：`<head>` 里多出来一个 `</div>`（原书把一段样式表连同封面 svg 漏进了 head），整章不是合法 XML。
    #[test]
    fn stray_close_tag_dropped_only_when_that_balances() {
        let jj = "<html><head><title>1</title>\n<svg xmlns=\"http://www.w3.org/2000/svg\"><image xlink:href=\"c.jpg\"></image></svg></div>*/\n.top30 { margin-top: 30%; }\n</head><body><p>正文</p></body></html>";
        let (out, fx) = fix(jj);
        assert_eq!(fx.stray_close_tags, 1);
        assert!(!out.contains("</div>") && out.contains("</svg>*/") && out.contains("<p>正文</p>"), "{out}");
        assert!(tags_balanced(&out));
        // 交叉嵌套：去掉多余的 `</b>` 也配不平 → 一个都不去
        let (out, fx) = fix("<html><body><p><i>甲</p></i></b></body></html>");
        assert_eq!(fx.stray_close_tags, 0);
        assert!(out.contains("</p></i></b>"), "{out}");
    }

    #[test]
    fn markup_fix_is_idempotent_and_borrows_when_clean() {
        let (once, _) = fix("<!DOCTYPE html PUBLIC \"-//W3C//DTD XHTML 1.1//EN\" \"x.dtd\"><html><body><p>a&nbsp;b<br></p></body></html>");
        let mut fx = XmlFixes::default();
        assert!(matches!(normalize_markup(&once, true, &mut fx), Cow::Borrowed(_)), "第二遍什么都不改");
        assert_eq!(fx, XmlFixes::default());
        // OPF/NCX 只修字符层面，不碰结构
        let ncx = "<ncx><navMap><navPoint><navLabel><text>甲&nbsp;乙 & 丙</text></navLabel></navPoint></navMap></ncx></extra>";
        let mut fx = XmlFixes::default();
        assert_eq!(normalize_markup(ncx, false, &mut fx), "<ncx><navMap><navPoint><navLabel><text>甲&#160;乙 &amp; 丙</text></navLabel></navPoint></navMap></ncx></extra>");
    }

    const OPF2: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<package xmlns="http://www.idpf.org/2007/opf" unique-identifier="uuid_id" version="2.0">
  <metadata xmlns:dc="http://purl.org/dc/elements/1.1/" xmlns:opf="http://www.idpf.org/2007/opf">
    <dc:title>克莱因壶</dc:title>
    <dc:creator opf:role="aut" opf:file-as="冈岛二人">冈岛二人</dc:creator>
    <dc:date opf:event="modification">2019-01-01</dc:date>
    <dc:date opf:event="publication">2019-08-31T16:00:00+00:00</dc:date>
    <dc:identifier id="uuid_id" opf:scheme="uuid">f9bca091</dc:identifier>
    <dc:identifier opf:scheme="ISBN">9787122346032</dc:identifier>
    <meta name="cover" content="cover"/>
  </metadata>
  <manifest><item id="c1" href="c1.xhtml" media-type="application/xhtml+xml"/><item id="ncx" href="toc.ncx" media-type="application/x-dtbncx+xml"/></manifest>
  <spine toc="ncx"><itemref idref="c1"/></spine>
  <guide><reference type="cover" title="封面" href="c1.xhtml"/></guide>
</package>"#;

    #[test]
    fn opf2_upgraded_to_epub3() {
        let out = upgrade_opf(OPF2, "zh").unwrap();
        assert!(out.contains(r#"<package xmlns="http://www.idpf.org/2007/opf" unique-identifier="uuid_id" version="3.0">"#), "{out}");
        assert!(!out.contains("opf:role") && !out.contains("opf:file-as") && !out.contains("opf:scheme") && !out.contains("opf:event"), "{out}");
        assert!(out.contains(r#"<dc:creator id="eink-meta">冈岛二人</dc:creator>"#), "{out}");
        assert!(out.contains(r##"<meta refines="#eink-meta" property="role" scheme="marc:relators">aut</meta><meta refines="#eink-meta" property="file-as">冈岛二人</meta>"##), "{out}");
        assert!(out.contains(r##"<meta refines="#uuid_id" property="identifier-type">uuid</meta>"##), "{out}");
        assert!(out.contains(r#"<dc:identifier id="eink-meta-2">9787122346032</dc:identifier>"#) && out.contains(r##"<meta refines="#eink-meta-2" property="identifier-type">ISBN</meta>"##), "{out}");
        // dc:date 只留一个（出版日期），另一个改成 dcterms:date
        assert_eq!(out.matches("<dc:date").count(), 1);
        assert!(out.contains("<dc:date>2019-08-31T16:00:00+00:00</dc:date>") && out.contains(r#"<meta property="dcterms:date">2019-01-01</meta>"#), "{out}");
        assert!(out.contains(&format!(r#"<meta property="dcterms:modified">{EPUB3_MODIFIED}</meta>"#)), "{out}");
        assert!(out.contains(r#"<meta name="cover" content="cover"/>"#) && out.contains("<guide>") && out.contains(r#"<spine toc="ncx">"#), "旧写法保留: {out}");
        assert!(out.contains("<dc:language>zh</dc:language>"), "缺语言补上: {out}");
        assert!(well_formed(&out), "{out}");
        assert_eq!(upgrade_opf(&out, "zh"), None, "幂等");
    }

    #[test]
    fn opf_identifier_and_modified_repaired() {
        // unique-identifier 指向不存在的 id、标识符没有 id → 补 id 并改指
        let opf = r#"<package version="2.0" unique-identifier="nope"><metadata><dc:title>t</dc:title><dc:language>en</dc:language><dc:identifier>urn:x</dc:identifier></metadata><manifest/></package>"#;
        let out = upgrade_opf(opf, "zh").unwrap();
        assert!(out.contains(r#"<dc:identifier id="eink-uid">urn:x</dc:identifier>"#) && out.contains(r#"unique-identifier="eink-uid""#), "{out}");
        assert!(out.contains(r#"xmlns="http://www.idpf.org/2007/opf""#), "{out}");
        // 一个标识符都没有：按 OPF 原文派生一个确定的
        let bare = r#"<package version="2.0"><metadata xmlns:dc="http://purl.org/dc/elements/1.1/"><dc:title>t</dc:title><dc:language>en</dc:language></metadata><manifest/></package>"#;
        let a = upgrade_opf(bare, "zh").unwrap();
        assert_eq!(a, upgrade_opf(bare, "zh").unwrap(), "确定");
        assert!(a.contains(r#"<dc:identifier id="eink-uid">urn:eink:"#) && a.contains(r#"unique-identifier="eink-uid""#), "{a}");
        // 已是 3.0、modified 格式正确 → 不动；格式不对的改成固定值；前缀 OPF 跟着前缀写
        let ok3 = r#"<package version="3.0" unique-identifier="u" xmlns="http://www.idpf.org/2007/opf"><metadata><dc:identifier id="u">x</dc:identifier><dc:title>t</dc:title><dc:language>zh</dc:language><meta property="dcterms:modified">2024-10-26T18:06:32Z</meta></metadata></package>"#;
        assert_eq!(upgrade_opf(ok3, "zh"), None);
        let bad = ok3.replace("2024-10-26T18:06:32Z", "2024-10-26");
        assert!(upgrade_opf(&bad, "zh").unwrap().contains(&format!(">{EPUB3_MODIFIED}</meta>")));
        let pre = r#"<opf:package xmlns:opf="http://www.idpf.org/2007/opf" version="2.0" unique-identifier="u"><opf:metadata><dc:identifier id="u">x</dc:identifier><dc:title>t</dc:title><dc:language>zh</dc:language></opf:metadata></opf:package>"#;
        let out = upgrade_opf(pre, "zh").unwrap();
        assert!(out.contains(&format!(r#"<opf:meta property="dcterms:modified">{EPUB3_MODIFIED}</opf:meta></opf:metadata>"#)) && !out.contains(" xmlns=\""), "{out}");
    }

    fn epub2_book() -> Vec<Entry> {
        vec![
            e("OEBPS/content.opf", OPF2),
            e("OEBPS/c1.xhtml", "<?xml version=\"1.0\"?><!DOCTYPE html PUBLIC \"-//W3C//DTD XHTML 1.1//EN\" \"http://www.w3.org/TR/xhtml11/DTD/xhtml11.dtd\"><html xmlns=\"http://www.w3.org/1999/xhtml\"><head><title>t</title></head><body><h1 id=\"h\">第一章</h1><p>甲&nbsp;乙&mdash;丙</p><h2 id=\"s\">一</h2><p>丁</p></body></html>"),
            e("OEBPS/toc.ncx", r#"<?xml version="1.0"?><ncx xmlns="http://www.daisy.org/z3986/2005/ncx/" version="2005-1"><head><meta name="dtb:uid" content="other"/></head><docTitle><text>克莱因壶</text></docTitle><navMap><navPoint id="a"><navLabel><text>第一章</text></navLabel><content src="c1.xhtml#h"/><navPoint id="b"><navLabel><text>一 &amp; 二</text></navLabel><content src="c1.xhtml#s"/></navPoint></navPoint></navMap></ncx>"#),
        ]
    }

    #[test]
    fn epub2_book_gets_nav_landmarks_and_keeps_ncx() {
        let mut v = epub2_book();
        let rep = wash_entries(&mut v, &WashOpts { paginate: false, ..Default::default() }).unwrap();
        assert_eq!((rep.epub3_upgraded, rep.nav_generated, rep.landmarks_added, rep.ncx_generated), (1, 2, 1, 0));
        let opf = s(&v, "OEBPS/content.opf");
        assert!(opf.contains(r#"version="3.0""#) && opf.contains(r#"<item id="eink-nav" href="nav.xhtml" media-type="application/xhtml+xml" properties="nav"/>"#), "{opf}");
        assert!(opf.contains(r#"<item id="ncx" href="toc.ncx" media-type="application/x-dtbncx+xml"/>"#) && opf.contains(r#"<spine toc="ncx">"#), "NCX 保留: {opf}");
        let nav = s(&v, "OEBPS/nav.xhtml");
        assert!(nav.contains(r#"<li><a href="c1.xhtml#h">第一章</a><ol><li><a href="c1.xhtml#s">一 &amp; 二</a></li></ol></li>"#), "按 NCX 的层级: {nav}");
        assert!(nav.contains(r#"<nav epub:type="landmarks" id="landmarks" hidden=""><ol><li><a epub:type="cover" href="c1.xhtml">封面</a></li></ol></nav></body>"#), "{nav}");
        let ncx = s(&v, "OEBPS/toc.ncx");
        assert!(ncx.contains(r#"<meta name="dtb:uid" content="f9bca091"/>"#), "dtb:uid 对齐 OPF 标识符: {ncx}");
        let c1 = s(&v, "OEBPS/c1.xhtml");
        assert!(c1.contains("<!DOCTYPE html><html") && c1.contains(r#"xmlns:epub="http://www.idpf.org/2007/ops""#) && c1.contains("甲&#160;乙&#8212;丙"), "{c1}");
        for f in ["OEBPS/content.opf", "OEBPS/nav.xhtml", "OEBPS/toc.ncx", "OEBPS/c1.xhtml"] {
            assert!(well_formed(&s(&v, f)), "{f}: {}", s(&v, f));
        }
        // 再洗一遍：不再生成 nav、OPF 不再变
        let before = v.clone();
        let rep2 = wash_entries(&mut v, &WashOpts { paginate: false, ..Default::default() }).unwrap();
        assert_eq!((rep2.epub3_upgraded, rep2.nav_generated, rep2.landmarks_added), (0, 0, 0));
        assert_eq!(s(&v, "OEBPS/content.opf"), s(&before, "OEBPS/content.opf"));
        assert_eq!(s(&v, "OEBPS/nav.xhtml"), s(&before, "OEBPS/nav.xhtml"));
    }

    /// EPUB 3 原书只有 nav、没有 NCX（《恶女的告白》）：按 nav 生成 NCX，id 叫 `ncx`，xochitl 才找得到目录。
    #[test]
    fn nav_only_book_gets_ncx() {
        let mut v = vec![
            e("content.opf", r#"<package xmlns="http://www.idpf.org/2007/opf" version="3.0" unique-identifier="u"><metadata xmlns:dc="http://purl.org/dc/elements/1.1/"><dc:identifier id="u">uuid:1</dc:identifier><dc:title>书</dc:title><dc:language>zh</dc:language><meta property="dcterms:modified">2024-10-26T18:06:32Z</meta></metadata><manifest><item id="nav" href="nav.xhtml" media-type="application/xhtml+xml" properties="nav"/><item id="c1" href="t/c1.xhtml" media-type="application/xhtml+xml"/></manifest><spine><itemref idref="c1"/></spine></package>"#),
            e("nav.xhtml", r#"<html xmlns="http://www.w3.org/1999/xhtml" xmlns:epub="http://www.idpf.org/2007/ops"><body><nav epub:type="toc"><ol><li><a href="t/c1.xhtml">第一章</a><ol><li><a href="t/c1.xhtml#s">一节</a></li></ol></li></ol></nav></body></html>"#),
            e("t/c1.xhtml", r#"<html xmlns="http://www.w3.org/1999/xhtml" xmlns:epub="http://www.idpf.org/2007/ops"><body><h1>第一章</h1><p>甲</p><h2 id="s">一节</h2><p>乙</p></body></html>"#),
        ];
        let rep = wash_entries(&mut v, &WashOpts { paginate: false, ..Default::default() }).unwrap();
        assert_eq!(rep.ncx_generated, 2);
        let opf = s(&v, "content.opf");
        assert!(opf.contains(r#"<item id="ncx" href="toc.ncx" media-type="application/x-dtbncx+xml"/>"#) && opf.contains(r#"<spine toc="ncx">"#), "{opf}");
        assert!(opf.contains("2024-10-26T18:06:32Z") && !opf.contains(EPUB3_MODIFIED), "原书的 modified 沿用: {opf}");
        let ncx = s(&v, "toc.ncx");
        assert!(ncx.contains(r#"<meta name="dtb:uid" content="uuid:1"/>"#) && ncx.contains(r#"<content src="t/c1.xhtml"/><navPoint id="np2" playOrder="2"><navLabel><text>一节</text></navLabel><content src="t/c1.xhtml#s"/>"#), "{ncx}");
        assert!(well_formed(&ncx), "{ncx}");
    }

    #[test]
    fn content_properties_follow_final_content() {
        assert_eq!(content_properties(r#"<body><svg:svg><image xlink:href="https://x/a.png"/></svg:svg><img src="//cdn/x.jpg"/><math/><script src="a.js"></script></body>"#), PROP_SVG | PROP_REMOTE | PROP_MATHML | PROP_SCRIPTED);
        assert_eq!(content_properties(r#"<body><img src="a.png"/><a href="http://x">外链不算</a><link rel="stylesheet" href="a.css"/></body>"#), 0);
        let opf = r#"<package version="3.0"><manifest><item id="a" href="a.xhtml" media-type="application/xhtml+xml" properties="svg scripted"/><item id="b" href="b%20c.xhtml" media-type="application/xhtml+xml"/><item id="n" href="nav.xhtml" media-type="application/xhtml+xml" properties="nav"/></manifest></package>"#;
        let props: HashMap<String, u8> = [("O/a.xhtml".to_string(), 0u8), ("O/b c.xhtml".to_string(), PROP_SVG | PROP_REMOTE), ("O/nav.xhtml".to_string(), 0)].into_iter().collect();
        let out = apply_content_properties(opf, "O", &props).unwrap();
        assert!(out.contains(r#"<item id="a" href="a.xhtml" media-type="application/xhtml+xml"/>"#), "没用到的去掉: {out}");
        assert!(out.contains(r#"<item id="b" href="b%20c.xhtml" media-type="application/xhtml+xml" properties="remote-resources svg"/>"#), "{out}");
        assert!(out.contains(r#"properties="nav""#), "别的属性不动");
        assert_eq!(apply_content_properties(&out, "O", &props), None);
        assert_eq!(apply_content_properties(&opf.replace("3.0", "2.0"), "O", &props), None, "EPUB 2 不管");
    }
}
