//! 背景图铺满整页的真机探针（不是产品代码）：做几本很小的测试 KFX，每本换一种写法，看 Kindle 能不能把整页背景图画满一页。
//!
//! 背景：写出器照 Amazon 的写法把 `body` 的背景图写在包住整页内容的 `$270` 容器样式上，Kindle 只在容器（内容）范围里画，
//! 《绍宋》卷首页只露出图的一角（docs/kfx.md「背景图」）。这里每本书先用写出器正常转，再用 `Container` 改几处：
//! 只改已有实体的字段、不加新符号（加在符号表末尾的实体 id 会排到资源路径符号后面，违反硬规矩），节点个数不变（位置映射照旧）。
//!
//! 每本四页：1 带背景（`jsy.png`：贴底、cover、暗红底色）+ 几行字；2 普通文字（看背景串不串页）；
//! 3 《绍宋》卷首页式（`juan.png`：原尺寸、深灰底色、竖排短标题）；4 普通文字。
//!
//! ```sh
//! cargo run --release -p kfx --example bgprobe -- jsy.png juan.png 输出目录
//! ```
//! 两张图取自 `epub-optimize --device=kindle` 优化过的《绍宋》（`OEBPS/Images/`）。

use kfx::ion::Value;
use kfx::yj::*;
use kfx::{Body, Container, Opts};

/// 阅读范围（kindle profile 的 `[readable.kfx]`，1104×1546）：模板、整页容器用这个宽高，Kindle 按 `$156: $326` 缩放时正好铺满。
const PAGE_W: f64 = 1104.0;
const PAGE_H: f64 = 1546.0;

#[derive(Clone, Copy, PartialEq)]
enum Variant {
    /// 现在的写法（对照）。
    Control,
    /// 背景容器样式加 `$57` 高度 100%（`{100, $314}`）。
    Height100,
    /// 背景容器样式照抄 Amazon 固定版式整页容器的样式：宽高裸数（阅读范围像素）、`$546: $377`、`$476: true`、`$183: $488`。
    FixedBox,
    /// 版面模板改成整页图片版面那样的定尺寸容器（`$66`/`$67` 阅读范围、`$156: $326`、`$140: $320`、`$159: $270`），
    /// 背景容器宽高 100%。
    PageTemplate,
    /// 同上的定尺寸模板，背景样式挂到模板上（模板带 `$157`），内容容器不带样式。
    TemplateStyle,
    /// 整页图片叠文字：定尺寸模板 + 整页容器（relative、裁切）里先放一个绝对定位、铺满的图片节点，再放绝对定位的文字容器。
    Overlay,
}

const VARIANTS: &[(u64, Variant, &str, &str)] = &[
    (0, Variant::Control, "bg0", "对照现写法"),
    (1, Variant::Height100, "bg1", "容器高度百分百"),
    (2, Variant::FixedBox, "bg2", "容器照抄固定版式整页样式"),
    (3, Variant::PageTemplate, "bg3", "版面模板定尺寸加容器百分百"),
    (4, Variant::TemplateStyle, "bg4", "背景挂到版面模板"),
    (5, Variant::Overlay, "bg5", "整页图片节点叠文字"),
];

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [jsy, juan, out] = args.as_slice() else {
        eprintln!("用法：bgprobe jsy.png juan.png 输出目录");
        std::process::exit(1);
    };
    let jsy = std::fs::read(jsy).unwrap_or_else(|e| panic!("读 {jsy}：{e}"));
    let juan = std::fs::read(juan).unwrap_or_else(|e| panic!("读 {juan}：{e}"));
    std::fs::create_dir_all(out).unwrap_or_else(|e| panic!("建 {out}：{e}"));
    for &(n, v, code, what) in VARIANTS {
        let title = format!("{code} {what}");
        let epub = make_epub(&title, code, what, v, &jsy, &juan);
        // 每本不同的唯一 ID（容器 id、content_id、book_id 都由它派生），避开书库书 id 的范围。
        let (kfx, warnings) = kfx::epub_to_kfx(&epub, &Opts { fixed_id: Some(0xB6_0000 + n), ..Default::default() }).unwrap_or_else(|e| panic!("{title}：{e}"));
        for w in warnings {
            eprintln!("{title}：{w}");
        }
        let mut c = Container::parse(&kfx).unwrap_or_else(|e| panic!("{title}：{e:?}"));
        apply(&mut c, v);
        let bytes = c.to_bytes();
        // 自检：改过的书能再解开，解开再打包逐字节相同。
        let back = Container::parse(&bytes).unwrap_or_else(|e| panic!("{title} 回读：{e:?}"));
        assert_eq!(back.to_bytes(), bytes, "{title} 往返不一致");
        let path = format!("{out}/{code}-{what}.kfx");
        std::fs::write(&path, &bytes).unwrap_or_else(|e| panic!("写 {path}：{e}"));
        println!("{path}（{} 字节）", bytes.len());
    }
}

fn make_epub(title: &str, code: &str, what: &str, v: Variant, jsy: &[u8], juan: &[u8]) -> Vec<u8> {
    let mut w = bookconv::epubzip::EpubWriter::new(std::io::Cursor::new(Vec::new())).expect("epub");
    let mut put = |name: &str, data: &[u8]| w.put(name, data).expect("epub");
    put("META-INF/container.xml", br#"<?xml version="1.0"?><container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container"><rootfiles><rootfile full-path="OEBPS/content.opf" media-type="application/oebps-package+xml"/></rootfiles></container>"#);
    let opf = format!(
        r#"<?xml version="1.0"?><package xmlns="http://www.idpf.org/2007/opf" version="3.0" unique-identifier="id"><metadata xmlns:dc="http://purl.org/dc/elements/1.1/"><dc:identifier id="id">bgprobe-{code}</dc:identifier><dc:title>{title}</dc:title><dc:creator>背景图探针</dc:creator><dc:language>zh</dc:language></metadata><manifest><item id="nav" href="nav.xhtml" media-type="application/xhtml+xml" properties="nav"/><item id="css" href="s.css" media-type="text/css"/><item id="jsy" href="jsy.png" media-type="image/png"/><item id="juan" href="juan.png" media-type="image/png"/><item id="p1" href="p1.xhtml" media-type="application/xhtml+xml"/><item id="p2" href="p2.xhtml" media-type="application/xhtml+xml"/><item id="p3" href="p3.xhtml" media-type="application/xhtml+xml"/><item id="p4" href="p4.xhtml" media-type="application/xhtml+xml"/></manifest><spine><itemref idref="p1"/><itemref idref="p2"/><itemref idref="p3"/><itemref idref="p4"/></spine></package>"#
    );
    put("OEBPS/content.opf", opf.as_bytes());
    put("OEBPS/jsy.png", jsy);
    put("OEBPS/juan.png", juan);
    put("OEBPS/nav.xhtml", r#"<html xmlns="http://www.w3.org/1999/xhtml" xmlns:epub="http://www.idpf.org/2007/ops"><body><nav epub:type="toc"><ol><li><a href="p1.xhtml">1 书信背景</a></li><li><a href="p2.xhtml">2 普通文字</a></li><li><a href="p3.xhtml">3 卷首页背景</a></li><li><a href="p4.xhtml">4 普通文字</a></li></ol></nav></body></html>"#.as_bytes());
    // 同《绍宋》优化后的写法（body.jsy、body.juan、h1.juan）。Overlay 那本不用 body 背景，改成图片 + 文字两层，
    // 各层的内边距只是为了让写出器把它们写成容器、各有一个样式（样式随后整个改掉）。
    let css = "p{text-indent:2em;margin:0.5em 0}\
        body.jsy{background-image:url(\"jsy.png\");background-repeat:no-repeat;background-attachment:fixed;background-color:rgba(117,0,0,1);background-position:bottom;background-size:cover}\
        body.juan{background-image:url(\"juan.png\");background-repeat:no-repeat;background-attachment:fixed;background-color:rgba(17,17,17,1)}\
        h1.juan{text-align:right;color:rgba(255,255,255,1);margin:50% 15% auto auto;text-indent:0;font-size:1.2em}\
        p.juan{color:rgba(255,255,255,1);text-indent:0}\
        div.layer1{padding:1px}div.layer3{padding:3px}div.txt1{padding:2px}div.txt3{padding:4px}\
        img.bg1{width:100%}img.bg3{width:99%}";
    put("OEBPS/s.css", css.as_bytes());
    let head = |t: &str| format!(r#"<?xml version="1.0" encoding="utf-8"?><html xmlns="http://www.w3.org/1999/xhtml" xml:lang="zh-CN"><head><title>{t}</title><link href="s.css" rel="stylesheet" type="text/css"/></head>"#);
    let p1_text = format!(
        "<p>{code}：{what}。</p><p>第 1 页：书信式背景（暗红底色，米色图贴底、cover）。</p><p>成功：图或暗红底色铺满整页（至少到页面底部），不只在这几行字后面。</p><p>失败：只有这几行字后面有底色或图，下面是白的。</p>"
    );
    let p3_text = "<h1 class=\"juan\">第<br/>四<br/>卷<br/><br/>破<br/>茧<br/>展<br/>翼</h1>";
    let (p1, p3) = if v == Variant::Overlay {
        (
            format!(r#"<body><div class="layer1"><img class="bg1" src="jsy.png" alt=""/><div class="txt1">{p1_text}</div></div></body>"#),
            format!(r#"<body><div class="layer3"><img class="bg3" src="juan.png" alt=""/><div class="txt3">{p3_text}<p class="juan">第 3 页：卷首页式背景（原尺寸图、深灰底色）。</p></div></div></body>"#),
        )
    } else {
        (
            format!(r#"<body class="jsy">{p1_text}</body>"#),
            format!(r#"<body class="juan">{p3_text}<p class="juan">第 3 页：卷首页式背景（原尺寸图、深灰底色）。</p></body>"#),
        )
    };
    let plain = |n: u32| {
        format!(
            "<body><h2>第 {n} 页：普通文字</h2><p>这一页不应该有任何背景。如果上一页的背景图或底色串到了这里，说明这种写法会串页。</p>{}</body>",
            "<p>天地玄黄，宇宙洪荒。日月盈昃，辰宿列张。寒来暑往，秋收冬藏。闰余成岁，律吕调阳。</p>".repeat(4)
        )
    };
    put("OEBPS/p1.xhtml", format!("{}{p1}</html>", head("1")).as_bytes());
    put("OEBPS/p2.xhtml", format!("{}{}</html>", head("2"), plain(2)).as_bytes());
    put("OEBPS/p3.xhtml", format!("{}{p3}</html>", head("3")).as_bytes());
    put("OEBPS/p4.xhtml", format!("{}{}</html>", head("4"), plain(4)).as_bytes());
    w.finish().expect("epub").into_inner()
}

// ---------------------------------------------------------------- 改写

fn ion_mut(c: &mut Container, i: usize) -> &mut Value {
    let Body::Ion(items) = &mut c.entities[i].body else { panic!("不是 Ion 实体") };
    items.iter_mut().find_map(|it| if let kfx::ion::Item::Value(v) = it { Some(v) } else { None }).expect("空实体")
}

fn fields(v: &mut Value) -> &mut Vec<(u32, Value)> {
    match v {
        Value::Struct(f) => f,
        Value::Annotated(_, b) => fields(b),
        _ => panic!("不是结构体"),
    }
}

fn set(f: &mut Vec<(u32, Value)>, k: u32, v: Value) {
    f.retain(|(x, _)| *x != k);
    f.push((k, v));
}

fn pct(n: f64) -> Value {
    Value::Struct(vec![(VALUE, Value::F64(n)), (UNIT, Value::Symbol(U_PERCENT))])
}

fn style_index(c: &Container, sym: u32) -> usize {
    c.entities.iter().position(|e| e.ty == T_STYLE && e.id == sym).unwrap_or_else(|| panic!("找不到样式 {sym}"))
}

/// 版面模板改成定尺寸的容器（同流式书里整页图片版面的模板写法）。
fn page_template(c: &mut Container, story: u32, style: Option<u32>) {
    let i = c
        .entities
        .iter()
        .position(|e| e.ty == T_SECTION && e.value().and_then(|v| v.field(TEMPLATES)).and_then(Value::as_list).is_some_and(|t| t[0].field(STORYLINE_REF) == Some(&Value::Symbol(story))))
        .expect("找不到版面");
    let sec = fields(ion_mut(c, i));
    let Some((_, Value::List(t))) = sec.iter_mut().find(|(k, _)| *k == TEMPLATES) else { panic!("版面没有模板") };
    let tf = fields(&mut t[0]);
    tf.retain(|(k, _)| *k == EID || *k == STORYLINE_REF);
    tf.push((TMPL_WIDTH, Value::Int(PAGE_W as i64)));
    tf.push((TMPL_HEIGHT, Value::Int(PAGE_H as i64)));
    tf.push((TMPL_FIT, Value::Symbol(TMPL_FIT_VALUE)));
    tf.push((TMPL_ALIGN, Value::Symbol(ALIGN_CENTER)));
    if let Some(s) = style {
        tf.push((STYLE_REF, Value::Symbol(s)));
    }
    tf.push((NODE_TYPE, Value::Symbol(NODE_CONTAINER)));
}

fn apply(c: &mut Container, v: Variant) {
    if v == Variant::Control {
        return;
    }
    // 带背景的版面：排版流的根节点是容器、样式里有背景图（Overlay 那本没有背景图，按图片节点找）。
    let stories: Vec<(usize, u32)> = c.entities.iter().enumerate().filter(|(_, e)| e.ty == T_STORYLINE).map(|(i, e)| (i, e.id)).collect();
    for (si, story) in stories {
        let root = c.entities[si].value().and_then(|v| v.field(CHILDREN)).and_then(Value::as_list).map(|l| l[0].clone()).expect("空排版流");
        let Some(sty) = root.field(STYLE_REF).and_then(Value::as_symbol) else { continue };
        let has_bg = c.entities[style_index(c, sty)].value().and_then(|s| s.field(P_BG_IMAGE)).is_some();
        match v {
            Variant::Control => {}
            Variant::Height100 | Variant::FixedBox | Variant::PageTemplate if has_bg => {
                let si = style_index(c, sty);
                let f = fields(ion_mut(c, si));
                if v == Variant::FixedBox {
                    set(f, P_WIDTH, Value::F64(PAGE_W));
                    set(f, P_HEIGHT, Value::F64(PAGE_H));
                    set(f, P_SIZING, Value::Symbol(SIZING_VALUE));
                    set(f, P_CLIP, Value::Bool(true));
                    set(f, P_POSITION, Value::Symbol(POSITION_RELATIVE));
                } else {
                    if v == Variant::PageTemplate {
                        set(f, P_WIDTH, pct(100.0));
                    }
                    set(f, P_HEIGHT, pct(100.0));
                }
                if v == Variant::PageTemplate {
                    page_template(c, story, None);
                }
            }
            Variant::TemplateStyle if has_bg => {
                // 内容容器去掉样式引用，背景样式改挂到模板上。
                let sf = fields(ion_mut(c, si));
                let Some((_, Value::List(kids))) = sf.iter_mut().find(|(k, _)| *k == CHILDREN) else { unreachable!() };
                fields(&mut kids[0]).retain(|(k, _)| *k != STYLE_REF);
                page_template(c, story, Some(sty));
            }
            Variant::Overlay => {
                // 根容器（layer）→ [图片节点, 文字容器]；样式整个换掉（这些样式只被这一处用）。
                let kids = root.field(CHILDREN).and_then(Value::as_list).map(<[Value]>::to_vec).unwrap_or_default();
                let Some(img) = kids.iter().find(|k| k.field(NODE_TYPE) == Some(&Value::Symbol(NODE_IMAGE))) else { continue };
                let txt = kids.iter().find(|k| k.field(NODE_TYPE) == Some(&Value::Symbol(NODE_CONTAINER))).expect("没有文字容器");
                let (img_sty, txt_sty) = (img.field(STYLE_REF).and_then(Value::as_symbol).expect("图片样式"), txt.field(STYLE_REF).and_then(Value::as_symbol).expect("文字样式"));
                let replace = |c: &mut Container, s: u32, props: Vec<(u32, Value)>| {
                    let i = style_index(c, s);
                    let f = fields(ion_mut(c, i));
                    let keep: Vec<(u32, Value)> = f.iter().filter(|(k, _)| *k == STYLE_NAME || *k == P_LANG).cloned().collect();
                    *f = props;
                    f.extend(keep);
                };
                let layer_props = vec![
                    (P_WIDTH, Value::F64(PAGE_W)),
                    (P_HEIGHT, Value::F64(PAGE_H)),
                    (P_SIZING, Value::Symbol(SIZING_VALUE)),
                    (P_CLIP, Value::Bool(true)),
                    (P_POSITION, Value::Symbol(POSITION_RELATIVE)),
                ];
                replace(c, sty, layer_props);
                replace(
                    c,
                    img_sty,
                    vec![
                        (P_WIDTH, Value::F64(PAGE_W)),
                        (P_HEIGHT, Value::F64(PAGE_H)),
                        (P_SIZING, Value::Symbol(SIZING_VALUE)),
                        (P_TOP, Value::F64(0.0)),
                        (P_LEFT, Value::F64(0.0)),
                        (P_POSITION, Value::Symbol(POSITION_ABSOLUTE)),
                    ],
                );
                replace(
                    c,
                    txt_sty,
                    vec![
                        (P_WIDTH, Value::F64(PAGE_W)),
                        (P_SIZING, Value::Symbol(SIZING_VALUE)),
                        (P_TOP, Value::F64(0.0)),
                        (P_LEFT, Value::F64(0.0)),
                        (P_POSITION, Value::Symbol(POSITION_ABSOLUTE)),
                    ],
                );
                page_template(c, story, None);
            }
            _ => {}
        }
    }
}
