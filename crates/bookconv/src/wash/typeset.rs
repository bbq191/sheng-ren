//! 正文排版归一：CJK 假段落、标题后首段顶格、外链 wash css 注入与生成、重复 id 计数。
use super::*;

/// 中文书两种"假段落"归一（2026-09-06 《人骨拼圖》2017 旧 EPUB 真机）：
/// ① 全书没有 `<p>`，每章一个 `<div>` 里 `<br/>` 分行、段首两个全角空格——`p{text-indent:2em}` 没有对象，xochitl 又把
///    U+3000 折叠掉 → 零缩进；KOReader 把 U+3000 按字体宽度画出来 → "换字体缩进跟着变"。→ 按 `<br>` 切成 `<p>`。
/// ② 段首烘死的全角空格 / nbsp（有 `<p>` 的书也常见）→ 剥掉，缩进统一走外链 css（字体无关的精确 2em）。
///
/// 只在文件里 `<br>` 数 ≥ 4 且没有 `<p>` 时做 ①；② 对所有 `<p>` 做。块级标签（h1–h6/div/section 的开闭、img、table）原样保留。
/// ① 的保守条件（2026-09-27 审计：`<li>1981<br/></li>`、`<h2>第一章<br/>风起</h2>` 被切出 `<p>` 跨元素的坏标签）：
/// 每个 `<br>` 外面只能包着块容器（div/section/article/blockquote），切出来要包进 `<p>` 的每一段只能是成对的行内内容；
/// 有一处不满足就整个文件不做 ①。
pub(super) fn cjk_paragraphize(html: &str) -> String {
    static BR: OnceLock<Regex> = OnceLock::new();
    static BLOCK: OnceLock<Regex> = OnceLock::new();
    let out = strip_para_lead_spaces(html);
    let (mut n_br, mut n_p) = (0, 0);
    for t in html::tags(&out).filter(|t| t.is_start()) {
        if t.is("br") {
            n_br += 1;
        } else if t.is("p") {
            n_p += 1;
        }
    }
    if n_br < 4 || n_p > 0 {
        return out;
    }
    let Some((lo, hi)) = html::body_range(&out) else { return out };
    if !brs_only_in_containers(&out, lo, hi) {
        return out;
    }
    let (head, body, tail) = (&out[..lo], &out[lo..hi], &out[hi..]);
    let br = BR.get_or_init(|| Regex::new(r#"(?is)(?:\s*<br\b[^>]*>\s*)+"#).unwrap());
    // 块级开闭标签 / 整块元素：不裹进 p
    let block = BLOCK.get_or_init(|| Regex::new(r#"(?is)^\s*(?:</?(?:div|section|article|body|blockquote|ul|ol|li|table|tr|td|th|figure|figcaption)\b[^>]*>|<h[1-6]\b[^>]*>.*?</h[1-6]>|<img\b[^>]*>|<hr\b[^>]*>|<a\b[^>]*\bid\s*=\s*(?:"[^"]*"|'[^']*')[^>]*>\s*</a>)\s*"#).unwrap());
    let mut res = String::with_capacity(body.len() + 64);
    for piece in br.split(body) {
        let mut rest = piece;
        // 剥前导块级标签
        while let Some(m) = block.find(rest).filter(|m| m.start() == 0) {
            res.push_str(&rest[..m.end()]);
            rest = &rest[m.end()..];
        }
        // 剥尾随块级闭合标签
        let mut trailing = String::new();
        loop {
            let t = rest.trim_end();
            match html::tags(t).last() {
                Some(tag) if tag.end == t.len() && tag.kind == html::TagKind::Close && html::is_block(tag.name) => {
                    trailing.insert_str(0, &t[tag.start..]);
                    rest = &t[..tag.start];
                }
                _ => break,
            }
        }
        let text = rest.trim().trim_start_matches(|c: char| c == '\u{3000}' || c == '\u{a0}' || c.is_whitespace());
        if !text.is_empty() {
            if !inline_balanced(text) {
                return out; // 要包进 <p> 的一段里有块级标签或没配对的行内标签：拿不准，整个文件不动
            }
            res.push_str("<p>");
            res.push_str(text);
            res.push_str("</p>");
        }
        res.push_str(&trailing);
    }
    format!("{head}{res}{tail}")
}

/// 每个 `<p>` 开标签后面紧跟的空白、全角空格、不换行空格（含字符引用）剥掉。
fn strip_para_lead_spaces(html: &str) -> String {
    static LEAD: OnceLock<Regex> = OnceLock::new();
    let lead = LEAD.get_or_init(|| Regex::new(r#"^(?:\s|\u{3000}|&#12288;|&#x3000;|&nbsp;|&#160;|&#xa0;)+"#).unwrap());
    let edits: Vec<(usize, usize, String)> = html::tags(html)
        .filter(|t| t.kind == html::TagKind::Open && t.is("p"))
        .filter_map(|t| lead.find(&html[t.end..]).map(|m| (t.end, t.end + m.end(), String::new())))
        .collect();
    if edits.is_empty() {
        return html.to_string();
    }
    html::apply_edits(html, edits)
}

/// body 里每个 `<br>` 的祖先都只是块容器（div/section/article/blockquote）。
fn brs_only_in_containers(html: &str, lo: usize, hi: usize) -> bool {
    let spans = html::parse_spans(html, lo, hi);
    spans.iter().filter(|s| s.name == "br").all(|s| {
        let mut p = s.parent;
        while let Some(i) = p {
            if !matches!(spans[i].name.as_str(), "div" | "section" | "article" | "blockquote") {
                return false;
            }
            p = spans[i].parent;
        }
        true
    })
}

/// 片段里没有块级标签，行内标签都成对（能原样包进一个 `<p>`）。
fn inline_balanced(text: &str) -> bool {
    let mut stack: Vec<&str> = Vec::new();
    for t in html::tags(text) {
        if html::is_block(t.name) || t.is("body") {
            return false;
        }
        match t.kind {
            html::TagKind::Open if !html::is_void(t.name) => stack.push(t.name),
            html::TagKind::Close if stack.pop().is_none_or(|o| !o.eq_ignore_ascii_case(t.name)) => return false,
            _ => {}
        }
    }
    stack.is_empty()
}

/// 拉丁习惯：**标题后 / 章首 / 场景切换后的第一段不缩进**（英文排版惯例：只有紧接上一段的段落才缩进）。
/// xochitl 的 CSS 引擎（2026-09-06 八轮渲染缓存量化，书架白皮书 §03y）：不认内联 `style=""`；`text-indent:0` 当"没设"；
/// 类规则压过元素规则，但同为类规则时**先出现者胜**（书的表链接在前）；规则最后一个无分号的声明被丢。
/// 因此顶格段＝`<div class="eink-flush">`（**剥掉书的类与 style**，只留 eink-flush，id 等保留）+ 外链 `.eink-flush{text-indent:0.01em;…;}`；
/// KOReader 走标准 CSS 同样顶格。判定"前面是标题/切换"的信号（2026-09-06 用《Tell Me Your Dreams》AZW3 定，它的章名不是 `<h>`
/// 而是加粗段落、场景切换是段末双 `<br/>`）：
///   ① 前一个块是 `</h1>`–`</h6>`；② 前一段是"标题样段落"：≤80 字且（全文加粗/strong/class 含 bold、或以
///   Chapter/Book/Part/Prologue/Epilogue 开头）且不以句末标点结尾；③ 前一段以 ≥2 个 `<br>` 结尾或本身是空段（含 `<p/>`）/`* * *`
///   之类的分隔（空段过多的书——用空段当段距——不按分隔算）；④ 文件里第一个有正文的段（章首）。幂等。
/// 一趟扫出块序列（标签扫描，`<p/>` 自闭合算空段，不会把下一段吞进来），再统计、改写。
pub(super) fn flush_first_para_after_heading(html: &str, indent_classes: &HashSet<String>) -> String {
    static BR2: OnceLock<Regex> = OnceLock::new();
    static HEAD_WORD: OnceLock<Regex> = OnceLock::new();
    static SEP: OnceLock<Regex> = OnceLock::new();
    let br2 = BR2.get_or_init(|| Regex::new(r#"(?is)(<br\b[^>]*>\s*){2,}(</span>|</a>|\s)*$"#).unwrap());
    let head_word = HEAD_WORD.get_or_init(|| Regex::new(r#"(?i)^\s*(chapter|book|part|prologue|epilogue|section|interlude)\b"#).unwrap());
    let sep = SEP.get_or_init(|| Regex::new(r#"^[\s\*#~—–\-·•]*$"#).unwrap());
    // 块序列：h 结束标签 / p 元素 / 空段 `<p/>` / 上次洗出的 eink-flush div（重洗幂等）
    enum Block {
        HeadEnd,
        Para { open: (usize, usize), inner: (usize, usize), end: usize, is_div: bool },
    }
    let mut blocks: Vec<Block> = Vec::new();
    let mut it = html::tags(html);
    while let Some(t) = it.next() {
        if t.kind == html::TagKind::Close && t.heading_level().is_some() {
            blocks.push(Block::HeadEnd);
        } else if t.kind == html::TagKind::SelfClosing && t.is("p") {
            blocks.push(Block::Para { open: (t.start, t.end), inner: (t.end, t.end), end: t.end, is_div: false });
        } else if t.kind == html::TagKind::Open && t.is("p") {
            // p 里不嵌 p：到下一个 </p>；中途又遇到 <p（没闭合的段）就不认这一段。
            let mut close = None;
            for u in it.by_ref() {
                if u.is("p") {
                    if u.kind == html::TagKind::Close {
                        close = Some(u);
                    }
                    break;
                }
            }
            if let Some(c) = close {
                blocks.push(Block::Para { open: (t.start, t.end), inner: (t.end, c.start), end: c.end, is_div: false });
            }
        } else if t.kind == html::TagKind::Open && t.is("div") && html::attr_value(&html[t.start..t.end], "class").is_some_and(|c| c.split_whitespace().any(|x| x == "eink-flush")) {
            let mut depth = 1;
            for u in it.by_ref() {
                if u.is("div") {
                    match u.kind {
                        html::TagKind::Open => depth += 1,
                        html::TagKind::Close => depth -= 1,
                        _ => {}
                    }
                    if depth == 0 {
                        blocks.push(Block::Para { open: (t.start, t.end), inner: (t.end, u.start), end: u.end, is_div: true });
                        break;
                    }
                }
            }
        }
    }
    let texts: Vec<Option<String>> = blocks
        .iter()
        .map(|b| match b {
            Block::Para { inner, .. } => Some(html::plain_text(&html[inner.0..inner.1])),
            Block::HeadEnd => None,
        })
        .collect();
    // 空段太多（>20%）的书是拿空段当段距，不当场景分隔
    let total_p = texts.iter().flatten().count();
    let empty_p = texts.iter().flatten().filter(|t| sep.is_match(t)).count();
    let empty_is_sep = total_p == 0 || empty_p * 5 <= total_p;
    let mut flush_next = true; // ④ 章首
    let mut edits: Vec<(usize, usize, String)> = Vec::new();
    for (b, text) in blocks.iter().zip(&texts) {
        let (Block::Para { open, inner, end, is_div }, Some(text)) = (b, text) else {
            flush_next = true; // ① </hN>
            continue;
        };
        if text.is_empty() || sep.is_match(text) {
            if empty_is_sep {
                flush_next = true; // ③ 空段 / * * * 分隔
            }
            continue;
        }
        let inner_html = &html[inner.0..inner.1];
        let n = text.chars().count();
        let bold_wrapped = (inner_html.contains("<b>") || inner_html.contains("<strong") || inner_html.contains("bold")) && n <= 80;
        let heading_like = !terminal_latin(text) && n <= 80 && (bold_wrapped || head_word.is_match(text));
        if flush_next && !heading_like && !is_div {
            // 书里**设了首行缩进的类**（`indent_classes`，如 `.calibre_ {text-indent:1.2em}`）不能留：xochitl 里同为类规则时
            // **先出现者胜**（诊断 13/14），带着它就压不住 eink-flush；元素/内联通道又都不通（诊断 7–10）。别的类照留
            // （2026-09-28 审计：此前连同所有类一起删，作者用类写的强调——斜体、小型大写、颜色——跟着丢了）。
            // style 照留，只去掉 `text-indent`（KOReader 认行内样式，留着会压过 eink-flush）。id 等其它属性保留。
            let orig = &html[open.0..open.1];
            let classes: Vec<&str> = html::attr_value(orig, "class").unwrap_or("").split_whitespace().filter(|c| !indent_classes.contains(*c)).collect();
            let mut tag = html::remove_attr(orig, "class");
            if let Some(style) = html::attr_value(&tag, "style") {
                let kept: String = html::css_decls(style).iter().filter(|d| !d.prop.eq_ignore_ascii_case("text-indent")).map(|d| d.raw).collect();
                tag = if kept.trim().trim_matches(';').trim().is_empty() { html::remove_attr(&tag, "style") } else { html::set_attr(&tag, "style", kept.trim()) };
            }
            let kept = tag[2..tag.len() - 1].trim(); // 去掉 "<p" 与 ">"
            let sp = if kept.is_empty() { "" } else { " " };
            let class = std::iter::once("eink-flush").chain(classes).collect::<Vec<_>>().join(" ").replace('"', "&quot;");
            edits.push((open.0, *end, format!("<div class=\"{class}\"{sp}{kept}>{inner_html}</div>")));
        }
        // 下一段是否顶格：② 本段是标题样段落；③ 本段以双 <br> 结尾
        flush_next = heading_like || br2.is_match(inner_html);
    }
    if edits.is_empty() {
        return html.to_string();
    }
    html::apply_edits(html, edits)
}

/// 样式表里写了 `text-indent` 的规则用到的类名（选择器里的 `.类名`，不管选择器多复杂都算上——宁可多收）。
/// 英文首段顶格时这些类不留在 `eink-flush` 的 div 上（见 [`flush_first_para_after_heading`]）。
pub(super) fn indent_classes_of(css: &str) -> HashSet<String> {
    static CLASS: OnceLock<Regex> = OnceLock::new();
    let class = CLASS.get_or_init(|| Regex::new(r#"\.(-?[A-Za-z_][\w-]*)"#).unwrap());
    let mut out = HashSet::new();
    for c in css_rule_re().captures_iter(css) {
        if html::css_decls(&c[2]).iter().any(|d| d.prop.eq_ignore_ascii_case("text-indent")) {
            out.extend(class.captures_iter(&c[1]).map(|m| m[1].to_string()));
        }
    }
    out
}

/// 拉丁段落是否以句末标点结束（标题样段落判定用）。
pub(super) fn terminal_latin(t: &str) -> bool {
    t.trim_end().chars().last().map(|c| ".!?\"'”’)".contains(c)).unwrap_or(false)
}

/// 给 `<head>` 注入指向外链 wash css 的 `<link>`（`href`=该 html 相对 css 的路径）。幂等（已有则跳过）。
/// 无 `</head>` 时补一对 head；无 `<body` 也不动（异常文件）。
pub(super) fn inject_css_link(html: &str, href: &str) -> String {
    let marker = format!("href=\"{href}\"");
    if html.contains(&marker) {
        return html.to_string();
    }
    let link = format!("<link rel=\"stylesheet\" type=\"text/css\" href=\"{href}\"/>");
    if let Some(i) = html.find("</head>") {
        format!("{}{}{}", &html[..i], link, &html[i..])
    } else if let Some(i) = html.find("<body") {
        format!("{}<head>{}</head>{}", &html[..i], link, &html[i..])
    } else {
        html.to_string()
    }
}

/// 外链 wash css 的内容。按 `opts.lang` 注中/英首行缩进习惯 + 段落上下边距归零（除非 keep_para_spacing）。
/// ⚠ 两条 xochitl css 解析器的脆弱性（真机坐实）：
/// ① **只用裸元素选择器 `p{}`**——一条类/相邻/at-rule 选择器就让整表失效（《缩进诊断5》带 `.big` 时连 `p{}` 都不生效）。
/// ② **不用 `!important`**——带 `!important` 的外链规则不生效（v10 真机「都没缩进」），去掉即生效（diag6/《飘》验证）。
/// 我们的 `<link>` 注在 `</head>` 前、晚于书自带 css，同特异性靠源序后者胜，无需 `!important` 也能盖过书里的 `p{text-indent:0}`。
/// `opts.lang` 应已被 `wash_entries` 从 `Auto` 解析为具体值（此处把 `Auto` 兜底当 `Cjk`）。
pub fn wash_css(opts: &WashOpts) -> String {
    // 拉丁 1.2em / 中文 2em（Auto 兜底中文）。
    let indent = indent_for(opts);
    // 两端对齐（2026-09-27 用户定）：中文按排版规范两端对齐；英文两端对齐时配断字与孤行寡行控制（单独一条规则，
    // 阅读器不认这几个属性时只丢这一条）。书里用类写明的居中/居右（含 `align=`/行内样式换成的 eink-center/right）
    // 是类选择器，优先级高于 `p{}`，不受影响。
    let mut decl = format!("text-indent:{indent};text-align:justify;");
    if !opts.keep_para_spacing {
        decl.push_str("margin-top:0;margin-bottom:0;padding-top:0;padding-bottom:0;");
    }
    // ⚠ 每条声明都以 `;` 收尾：xochitl 会丢掉规则里最后一个没分号的声明（2026-09-06 诊断 11/12：`p{text-indent:2em}`
    // 整条不生效、`p{text-indent:2em;}` 生效）——keep_para_spacing 档位此前因此在 xochitl 上没缩进。
    // `.eink-flush`：拉丁首段顶格段（wash_html 换成的 `<div class="eink-flush">`），KOReader 靠它归零缩进/段距；
    // xochitl 上 div 本就无 p 规则、且书的类规则够不到（类已剥），此条只是保险。
    // ⚠ 值用 0.01em 不用 0：xochitl 把 `text-indent:0` 当"没设"→ 落回从外层 `<div class="calibre1">` 之类**继承**来的缩进
    //   （诊断 14 V1/V7 vs Sheldon v5，2026-09-06）；0.01em ≈ 0.1pt 肉眼不可见，KOReader 同样视为顶格。
    let flush = if opts.keep_para_spacing { ".eink-flush{text-indent:0.01em;}" } else { ".eink-flush{text-indent:0.01em;margin-top:0;margin-bottom:0;}" };
    // figure/figcaption：以前这条规则只管了 `<p>` 的边距，`article.rs` 网文管线常把图片包成
    // `<figure><img/><figcaption>…</figcaption></figure>`（真机书里也不算罕见），这两个元素
    // 完全没被清零过——默认（未洗）上下边距在"图片夹在正文中间"的场景会造成明显留白，2026-09-10
    // 真机拿 aeon.co 一篇网文复现坐实（诊断EPUB→投原生→量 xochitl 渲染 PDF，不肉眼猜，见书架
    // 白皮书 §03aq）。**两条规则分开写，不写成 `figure,figcaption{}`**——xochitl 的 CSS 解析器
    // 脆，只认裸元素选择器，逗号/复合选择器直接整条规则失效（`lang_aware_indent` 测试断言过
    // 这条红线，别在这里破例）。keep_para_spacing 档位同样清零：那档的意图是"保留正文段落之间
    // 的呼吸感"，不是"保留图片周围的默认边距"，两件事语义不同，不该被同一个开关连带控制。
    // 注释容器统一比正文小一号（相对单位，随用户当前字号缩放）——书自带 CSS 里已有注释规则的由
    // `filter_css` 就地改；这两条是给"书压根没给注释块写过 CSS"（纯靠我们自己生成的 `.footnotes`
    // 章末块 / `.eink-fnote` Inline 内联注释）兜底，不然那些书的注释永远跟正文同号，不满足这条通用
    // 要求。跟 `.eink-flush` 一样是单个裸类选择器，不逗号连写。
    let latin = if opts.lang == LangMode::Latin { "p{hyphens:auto;-webkit-hyphens:auto;orphans:2;widows:2;}\n" } else { "" };
    format!(
        "p{{{decl}}}\n{latin}.eink-center{{text-align:center;text-indent:0.01em;}}\n.eink-right{{text-align:right;text-indent:0.01em;}}\n{flush}\nfigure{{margin:0;padding:0;}}\nfigcaption{{margin:0;padding:0;}}\n.footnotes{{font-size:{FOOTNOTE_FONT_SIZE};}}\n.eink-fnote{{font-size:{FOOTNOTE_FONT_SIZE};}}\n"
    )
}

/// 带不止一个 `id` 属性的开标签数（质量门与清洗报告用；`aid`/`data-id` 不算 id）。
pub fn count_dup_id_tags(html: &str) -> usize {
    html::tags(html).filter(|t| t.is_start() && html::attrs(&html[t.start..t.end]).iter().filter(|a| a.is("id")).nth(1).is_some()).count()
}
