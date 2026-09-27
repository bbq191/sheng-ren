//! 章节 html / 图片的逐条变换（第一遍规整、第二遍脚注重排+提对比+远程图内联、图片降采样）。
use super::*;

/// HTML 里的远程图（http(s)/协议相对 `//`）→ 抓取降采样内联进 EPUB：抓到→存进 zip（与本章同目录，src 改本地文件名，
/// 免相对路径计算），调用方再把它补进 OPF manifest（[`add_manifest_items`]）；抓不到→`<img>` **原样保留**（不改书的内容，
/// 联网的阅读器仍可能显示它）。`chap_dir`=本章 zip 内目录；`counter` 跨章递增保资源名唯一。
/// `fetch(src)->Some((字节,ext))|None`（依赖注入便于测试，生产传抓图闭包）。返回（改写后 html, 新增资源 [(zip路径, 字节)]）。
pub(super) fn inline_remote_images<F>(
    html: &str,
    chap_dir: &str,
    counter: &mut usize,
    fetch: F,
) -> (String, Vec<(String, Vec<u8>)>)
where
    F: Fn(&str) -> Option<(Vec<u8>, &'static str)>,
{
    static RE_IMG: OnceLock<Regex> = OnceLock::new();
    static RE_SRC: OnceLock<Regex> = OnceLock::new();
    let re_img = RE_IMG.get_or_init(|| Regex::new(r#"(?is)<img\b[^>]*>"#).unwrap());
    let re_src = RE_SRC.get_or_init(|| Regex::new(r#"(?is)\ssrc="([^"]*)""#).unwrap());
    let mut resources: Vec<(String, Vec<u8>)> = Vec::new();
    let out = re_img.replace_all(html, |c: &regex::Captures| {
        let tag = &c[0];
        let src = match re_src.captures(tag).and_then(|m| m.get(1)) {
            Some(s) => s.as_str().to_string(),
            None => return tag.to_string(),
        };
        let remote = src.starts_with("http://") || src.starts_with("https://") || src.starts_with("//");
        if !remote {
            return tag.to_string();
        }
        match fetch(&src) {
            Some((bytes, ext)) => {
                let fname = format!("remote_img_{}.{ext}", *counter);
                *counter += 1;
                let path = if chap_dir.is_empty() { fname.clone() } else { format!("{chap_dir}/{fname}") };
                resources.push((path, bytes));
                re_src.replace(tag, regex::NoExpand(&format!(r#" src="{fname}""#))).into_owned()
            }
            None => tag.to_string(), // 抓不到 → 原样保留
        }
    });
    (out.into_owned(), resources)
}

/// 章节里有没有远程图（与 [`inline_remote_images`] 同一判据的快速预扫，只决定要不要推迟写 OPF）。
pub(super) fn has_remote_img(html: &str) -> bool {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r#"(?is)<img\b[^>]*?\ssrc="(?:https?:)?//"#).unwrap()).is_match(html)
}

/// 把抓到的远程图补进 OPF manifest（插在第一个 `</manifest>` 前，允许命名空间前缀）。`opf_path`/`imgs` 都是 zip 内路径，
/// href 按 OPF 所在目录算相对路径；id 用 `eink-remote-img-N`。没有 `</manifest>` 时原样返回。
pub(super) fn add_manifest_items(opf: &str, opf_path: &str, imgs: &[(String, Vec<u8>)]) -> String {
    static RE: OnceLock<Regex> = OnceLock::new();
    let Some(m) = RE.get_or_init(|| Regex::new(r#"</(?:[A-Za-z_][\w.-]*:)?manifest\s*>"#).unwrap()).find(opf) else { return opf.to_string() };
    let dir = crate::epubzip::dir_of(opf_path);
    let items: String = imgs
        .iter()
        .enumerate()
        .map(|(i, (path, _))| {
            let ext = crate::util::image_ext_of(path);
            let href = crate::util::xml_escape(&crate::epubzip::relative_to(dir, path));
            format!(r#"<item id="eink-remote-img-{i}" href="{href}" media-type="{}"/>"#, crate::util::image_media_type_of_ext(&ext))
        })
        .collect();
    format!("{}{items}{}", &opf[..m.start()], &opf[m.start()..])
}

/// 生产抓图闭包：`//`→https、Referer=图自身 origin（满足多数 CDN 同源防盗链）、抓取+降采样。
pub(super) fn remote_img_fetcher(ag: &ureq::Agent, screen: crate::imgopt::Screen) -> impl Fn(&str) -> Option<(Vec<u8>, &'static str)> + '_ {
    move |src: &str| {
        let abs = if let Some(r) = src.strip_prefix("//") { format!("https://{r}") } else { src.to_string() };
        let referer = abs
            .find("://")
            .and_then(|i| abs[i + 3..].find('/').map(|j| &abs[..i + 3 + j + 1]))
            .unwrap_or("")
            .to_string();
        crate::netimg::fetch_image(ag, src, &referer, Some(screen)).map(|(b, ext, _mime)| (b, ext))
    }
}

/// 修封面拉伸变形：calibre 封面页 SVG 常用 preserveAspectRatio="none"（强制铺满、不保宽高比，
/// 封面被拉伸放大变形），改成 "xMidYMid meet"（保持比例缩放到适配）。覆盖小写/标准两种写法。
pub(super) fn fix_cover_aspect(html: &str) -> String {
    html.replace("preserveaspectratio=\"none\"", "preserveAspectRatio=\"xMidYMid meet\"")
        .replace("preserveAspectRatio=\"none\"", "preserveAspectRatio=\"xMidYMid meet\"")
}

/// 封面 SVG 换普通 img：calibre 封面页 `<svg ...><image (xlink:)href="X"/></svg>` 被 xochitl
/// 拉伸放大（改 preserveAspectRatio 都不吃），换成标准 `<img src="X" style=max-width:100%>` 更可控。
pub(super) fn svg_cover_to_img(html: &str) -> String {
    static R: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
    let re = R.get_or_init(|| {
        Regex::new(r#"(?is)<svg\b[^>]*>.*?<image\b[^>]*?(?:xlink:)?href="([^"]+)"[^>]*?/?>.*?</svg>"#).unwrap()
    });
    re.replace_all(html, |c: &regex::Captures| {
        format!(
            r#"<img src="{}" alt="cover" style="display:block;margin:0 auto;max-width:100%;height:auto;"/>"#,
            &c[1]
        )
    })
    .into_owned()
}

/// 第一遍 html 处理：归一同文件 href（part0004.html#x 写在 part0004.html 里→改裸锚 #x，否则下面
/// referenced/搬注释/拆环全把同章脚注误当跨文件）→ 剥字体锁 → 扫这章引用了哪些脚注 frag。
pub(super) fn first_pass_html(text: &str, name: &str) -> (String, Vec<String>) {
    let own = std::path::Path::new(name).file_name().and_then(|s| s.to_str()).unwrap_or("");
    let text = crate::htmlproc::normalize_self_hrefs(text, own);
    let stripped = crate::htmlproc::strip_font_locks(&text);
    let referenced = crate::htmlproc::referenced_note_frags(&stripped);
    (stripped, referenced)
}

/// 图片最终变换：按漫画/文字书分流（漫画只裁边/适配阅读范围，画质优先）。
/// 返回 `None` = 无需改动、沿用原字节（调用方自己决定借用还是移走，不为"没变"整张图克隆一份）。
///
/// 解码器遇到畸形图片偶发 panic（第三方书的坏 JPEG/PNG 是外部输入）：这里兜住、按"失败原样保留"处理——否则 panic 会从
/// 图片 worker 线程一路把整本书的优化搞砸（`thread::scope` 把子线程 panic 重新抛给调用方），只为一张坏图不值得。
pub(super) fn transform_image_bytes(bytes: &[u8], is_comic_book: bool, screen: crate::imgopt::Screen, grayscale: bool) -> Option<Vec<u8>> {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        if is_comic_book {
            // 单趟（解码/编码各一次、灰度保持、缩放走 SIMD），见 `prepare_comic_page_for_epub`。
            crate::imgopt::prepare_comic_page_for_epub(bytes, screen, grayscale)
        } else {
            crate::imgopt::downscale_for_epub(bytes, screen)
        }
    }))
    .unwrap_or(None)
}
