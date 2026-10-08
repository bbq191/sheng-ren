//! 章节 html / 图片的逐条变换（第一遍规整、第二遍脚注重排+远程图内联、图片降采样）。
use super::*;
use crate::html;

/// `<img src>` 是不是远程图（http(s) 或协议相对 `//`）。属性值是原文，先还原字符引用（`&amp;`）。
fn remote_src(tag: &str) -> Option<(html::Attr<'_>, String)> {
    let a = html::attr(tag, "src")?;
    let src = crate::util::xml_unescape(a.value).into_owned();
    (src.starts_with("http://") || src.starts_with("https://") || src.starts_with("//")).then_some((a, src))
}

/// HTML 里的远程图（http(s)/协议相对 `//`）→ 抓取降采样内联进 EPUB：抓到→存进 zip（与本章同目录，src 改本地文件名，
/// 免相对路径计算），调用方再把它补进 OPF manifest（[`add_manifest_items`]）；抓不到→**删掉这个 `<img>`**（2026-09-30 用户定：
/// 设备上的阅读器不联网，留着只是一个显示不出来的空框或断图标）。`chap_dir`=本章 zip 内目录；`counter` 跨章递增，`taken`（zip 里已有的条目名，含本次已抓到的）
/// 保资源名唯一——已经优化过的书再跑时书里已有 `remote_img_0.png`，新抓到的图不能再用这个名字（2026-09-28 审计）。
/// `drop_failed` 为假（只修复的文字书）时抓不到的 `<img>` 原样留着。
/// `fetch(src)->Some((字节,ext))|None`（依赖注入便于测试，生产传抓图闭包）。返回（改写后 html, 新增资源 [(zip路径, 字节)]）。
pub(super) fn inline_remote_images<F>(
    html_text: &str,
    chap_dir: &str,
    counter: &mut usize,
    taken: &mut HashSet<String>,
    drop_failed: bool,
    fetch: F,
) -> (String, Vec<(String, Vec<u8>)>)
where
    F: Fn(&str) -> Option<(Vec<u8>, &'static str)>,
{
    let mut resources: Vec<(String, Vec<u8>)> = Vec::new();
    let mut edits: Vec<(usize, usize, String)> = Vec::new();
    let tags: Vec<html::Tag> = html::tags(html_text).collect();
    for (k, t) in tags.iter().enumerate().filter(|(_, t)| t.is_start() && t.is("img")) {
        let Some((a, src)) = remote_src(&html_text[t.start..t.end]) else { continue };
        let Some((bytes, ext)) = fetch(&src) else {
            if !drop_failed {
                continue;
            }
            // 抓不到 → 删掉（写成 `<img …></img>` 的连闭合标签一起删）
            let end = match tags.get(k + 1) {
                Some(c) if t.kind == html::TagKind::Open && c.kind == html::TagKind::Close && c.is("img") => c.end,
                _ => t.end,
            };
            edits.push((t.start, end, String::new()));
            continue;
        };
        let (fname, path) = loop {
            let fname = format!("remote_img_{}.{ext}", *counter);
            *counter += 1;
            let path = if chap_dir.is_empty() { fname.clone() } else { format!("{chap_dir}/{fname}") };
            if taken.insert(path.clone()) {
                break (fname, path);
            }
        };
        resources.push((path, bytes));
        edits.push((t.start + a.value_start, t.start + a.value_end, fname));
    }
    if edits.is_empty() {
        return (html_text.to_string(), resources);
    }
    (html::apply_edits(html_text, edits), resources)
}

/// 章节里有没有远程图（与 [`inline_remote_images`] 同一判据的快速预扫，只决定要不要推迟写 OPF）。
pub(super) fn has_remote_img(html_text: &str) -> bool {
    html::tags(html_text).any(|t| t.is_start() && t.is("img") && remote_src(&html_text[t.start..t.end]).is_some())
}

/// 把抓到的远程图补进 OPF manifest（`wash::opf::insert_manifest_items`：插在 `</manifest>` 前，跟着 manifest 的命名空间前缀）。
/// `opf_path`/`imgs` 都是 zip 内路径，href 按 OPF 所在目录算相对路径；id 用 `eink-remote-img-N`（跳过 OPF 里已有的 id）。
/// 没有 `</manifest>` 时原样返回。
pub(super) fn add_manifest_items(opf: &str, opf_path: &str, imgs: &[(String, Vec<u8>)]) -> String {
    let dir = crate::epubzip::dir_of(opf_path);
    let existing: HashSet<&str> = crate::wash::manifest_items(opf).iter().map(|i| i.id).collect();
    let mut n = 0usize;
    let specs: Vec<(String, String, &'static str)> = imgs
        .iter()
        .map(|(path, _)| {
            let id = loop {
                let id = format!("eink-remote-img-{n}");
                n += 1;
                if !existing.contains(id.as_str()) {
                    break id;
                }
            };
            (id, crate::epubzip::href_to(dir, path, ""), crate::util::image_media_type_of_ext(&crate::util::image_ext_of(path)))
        })
        .collect();
    let items: Vec<crate::wash::opf::NewItem> = specs.iter().map(|(id, href, mt)| crate::wash::opf::NewItem { id, href, media_type: mt, properties: "" }).collect();
    crate::wash::opf::insert_manifest_items(opf, &items).unwrap_or_else(|| opf.to_string())
}

/// 漫画里转换了格式的图（GIF/WebP → PNG/JPEG，条目名不变）：把 OPF manifest 里对应项的 `media-type` 改成新格式。
/// `retyped` 是 (zip 路径, 新 media-type)；href 按 OPF 所在目录解析（百分号解码）后比对。没有 `media-type` 属性的项不动。
///
/// 为什么不改名：改名要把全书 xhtml 的 `src`、SVG 的 `href`、CSS 的 `url()`、OPF、NCX 里指向它的链接都改掉，漏一处图就丢；
/// EPUB 按 manifest 的 media-type 认图片格式，不看扩展名。
pub(super) fn set_manifest_media_types(opf: &str, opf_path: &str, retyped: &[(String, &'static str)]) -> String {
    let dir = crate::epubzip::dir_of(opf_path);
    let mut edits: Vec<(usize, usize, String)> = Vec::new();
    for it in crate::wash::manifest_items(opf) {
        let path = it.path(dir);
        let Some((_, mt)) = retyped.iter().find(|(p, _)| *p == path) else { continue };
        let Some(a) = html::attr(it.tag, "media-type") else { continue };
        if a.value != *mt {
            edits.push((it.pos + a.value_start, it.pos + a.value_end, (*mt).to_string()));
        }
    }
    if edits.is_empty() {
        return opf.to_string();
    }
    html::apply_edits(opf, edits)
}

/// 生产抓图闭包：`//`→https、Referer=图自身 origin（满足多数 CDN 同源防盗链）、抓取+降采样（`screen` 为 `None` 时不缩，原图）。
pub(super) fn remote_img_fetcher(ag: &ureq::Agent, screen: Option<crate::imgopt::Screen>) -> impl Fn(&str) -> Option<(Vec<u8>, &'static str)> + '_ {
    move |src: &str| {
        crate::netimg::fetch_image(ag, src, &crate::netimg::origin_of(src), screen).map(|(b, ext, _mime)| (b, ext))
    }
}

/// 修封面拉伸变形：calibre 封面页 SVG 常用 preserveAspectRatio="none"（强制铺满、不保宽高比，
/// 封面被拉伸放大变形），改成 "xMidYMid meet"（保持比例缩放到适配）。只改标签上的这个属性（按属性解析，属性名不分大小写、
/// 两种引号；小写写法的属性名顺带改成标准的 `preserveAspectRatio`）——此前对全文做字符串替换，正文里写着这串字的也会被改。
pub(super) fn fix_cover_aspect(html_text: &str) -> String {
    if !html::contains_ci(html_text, "preserveaspectratio") {
        return html_text.to_string();
    }
    let mut edits: Vec<(usize, usize, String)> = Vec::new();
    for t in html::tags(html_text).filter(|t| t.is_start()) {
        for a in html::attrs(&html_text[t.start..t.end]).into_iter().filter(|a| a.is("preserveAspectRatio") && a.value == "none") {
            edits.push((t.start + a.start, t.start + a.end, format!("preserveAspectRatio={q}xMidYMid meet{q}", q = a.quote.unwrap_or('"'))));
        }
    }
    if edits.is_empty() {
        return html_text.to_string();
    }
    html::apply_edits(html_text, edits)
}

/// 封面 SVG 换普通 img：calibre 封面页 `<svg ...><image (xlink:)href="X"/></svg>` 被 xochitl
/// 拉伸放大（改 preserveAspectRatio 都不吃），换成标准 `<img src="X" style=max-width:100%>` 更可控。
///
/// 只换**封面页**（2026-09-28 审计：此前的正则 `<svg…>.*?<image…>.*?</svg>` 会从前一个不含图的 `<svg>` 一路跨到后面那个
/// `<image>` 所在的 `</svg>`，把中间的正文段落整段吞掉）：按元素（`html::parse_spans`）逐个看最外层的 `<svg>`，要求
/// 它里面**只有一个** `<image>`、没有可见文字（`<text>` 之类），且页面正文里除了它没有别的可见内容。不满足就原样不动。
pub(super) fn svg_cover_to_img(html_text: &str) -> String {
    use crate::wash::opf::is_local;
    if !html::contains_ci(html_text, "<svg") {
        return html_text.to_string();
    }
    let (lo, hi) = html::body_range(html_text).unwrap_or((0, html_text.len()));
    let spans = html::parse_spans(html_text, lo, hi);
    let in_svg = |mut p: Option<usize>| {
        while let Some(i) = p {
            if is_local(&spans[i].name, "svg") {
                return true;
            }
            p = spans[i].parent;
        }
        false
    };
    let svgs: Vec<usize> = (0..spans.len()).filter(|&i| is_local(&spans[i].name, "svg") && spans[i].closed() && !in_svg(spans[i].parent)).collect();
    let [si] = svgs[..] else { return html_text.to_string() };
    let svg = &spans[si];
    let images: Vec<&html::Span> = spans.iter().filter(|s| is_local(&s.name, "image") && s.open_start >= svg.open_end && s.open_start < svg.close_start).collect();
    let [img] = images[..] else { return html_text.to_string() };
    let visible_outside = html::has_visible(&html_text[lo..svg.open_start]) || html::has_visible(&html_text[svg.close_end..hi]);
    if visible_outside || !html::plain_text(&html_text[svg.open_end..svg.close_start]).is_empty() {
        return html_text.to_string();
    }
    let tag = &html_text[img.open_start..img.open_end];
    let Some(href) = html::attr_value(tag, "xlink:href").or_else(|| html::attr_value(tag, "href")).filter(|v| !v.is_empty()) else { return html_text.to_string() };
    let new = format!(r#"<img src="{}" alt="cover" style="display:block;margin:0 auto;max-width:100%;height:auto;"/>"#, href.replace('"', "&quot;"));
    format!("{}{new}{}", &html_text[..svg.open_start], &html_text[svg.close_end..])
}

/// 第一遍 html 处理：归一同文件 href（part0004.html#x 写在 part0004.html 里→改裸锚 #x，否则下面
/// referenced/搬注释/拆环全把同章脚注误当跨文件）→ 剥字体锁。
pub(super) fn first_pass_html(text: &str, name: &str, keep_fonts: &HashSet<String>) -> String {
    let text = crate::htmlproc::normalize_self_hrefs(text, name);
    crate::htmlproc::strip_font_locks_keeping(&text, keep_fonts)
}

/// 图片最终变换：按漫画/文字书分流（漫画只裁边/适配阅读范围，画质优先）。
/// 返回 `None` = 无需改动、沿用原字节（调用方自己决定借用还是移走，不为"没变"整张图克隆一份）。
/// 解码器遇到畸形图片偶发 panic：兜住、按"失败原样保留"处理（[`crate::imgopt::guard`]）。
/// `flatten`：正文图合成白底（[`crate::imgalpha`]）——先合成再缩（带透明的图缩放时透明处的颜色会渗进边缘）。
/// `max_px`：解码上限（`OptimizeOpts::limits.max_decode_pixels`），超过的图原样保留。
#[allow(clippy::too_many_arguments)]
pub(super) fn transform_image_bytes(bytes: &[u8], is_comic_book: bool, screen: crate::imgopt::Screen, comic_margin: u32, grayscale: bool, bg: Option<crate::bgfit::BgFit>, flatten: bool, max_px: u64) -> Option<Vec<u8>> {
    crate::imgopt::guard(|| {
        if let Some(flat) = flatten.then(|| crate::imgopt::flatten_transparent_png(bytes, max_px)).flatten() {
            return Some(crate::imgopt::downscale_for_epub_limited(&flat, screen, max_px).unwrap_or(flat));
        }
        if let Some(fit) = bg {
            // 整页背景图：按原书尺寸意图缩，不再按普通插图缩（见 `bgfit`）
            crate::imgopt::downscale_background(bytes, fit, screen, max_px)
        } else if is_comic_book {
            // 单趟（解码/编码各一次、灰度保持、缩放走 SIMD），见 `prepare_comic_page_for_epub`。
            crate::imgopt::prepare_comic_page_limited(bytes, screen, comic_margin, grayscale, max_px)
        } else {
            crate::imgopt::downscale_for_epub_limited(bytes, screen, max_px)
        }
    })
}
