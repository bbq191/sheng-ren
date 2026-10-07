//! 远程图抓取（优化器内联远程 `<img>` 与网页抽取 `article` 共用）。
use crate::convert::common;
use crate::imgopt;

/// 远程图的下载上限。
pub const MAX_IMAGE_BYTES: u64 = 20 * 1024 * 1024;

/// 抓图 UA（网页抽取与优化器共用同一标识）。
pub const UA: &str = "Mozilla/5.0 (compatible; readlater/1.0)";

/// 抓远程图；给了 `screen` 就按书里插图的规则降采样（[`fit_for_epub`]），`None` = 保留原图（入库母版用）。`src` 支持协议相对 `//host/path`；非 http(s) 返回 None。
/// 返回 (字节, 扩展名, mime)；非图（魔数不认）、超过 [`MAX_IMAGE_BYTES`] → None。
pub fn fetch_image(ag: &ureq::Agent, src: &str, referer: &str, screen: Option<imgopt::Screen>) -> Option<(Vec<u8>, &'static str, &'static str)> {
    // 协议相对 URL（`//host/path`，Wikipedia 等常用）补 https:；其余非 http(s)（data:/未解析相对）跳过。
    let abs = if let Some(rest) = src.strip_prefix("//") {
        format!("https://{rest}")
    } else if src.starts_with("http://") || src.starts_with("https://") {
        src.to_string()
    } else {
        return None;
    };
    let mut req = ag.get(&abs).set("User-Agent", UA);
    if !referer.is_empty() {
        req = req.set("Referer", referer);
    }
    let resp = req.call().ok()?;
    // 超过上限按抓不到算：以前截到 20MB 照样用，前半截图片魔数对得上，坏图就进了书
    let bytes = crate::util::read_capped(resp.into_reader(), MAX_IMAGE_BYTES, 0).ok()??;
    let (ext, mime) = common::image_ext_mime(&bytes)?; // 魔数识别 JPEG/PNG/GIF；非图→None
    match screen {
        Some(s) => Some((fit_for_epub(bytes, s), ext, mime)),
        None => Some((bytes, ext, mime)),
    }
}

/// 抓到的远程图按书里本地插图的规则缩（[`imgopt::downscale_for_epub`]：竖向框，宽不超过阅读范围宽）。v51 以前按
/// 横竖选框（`downscale_for_device`），横幅图能缩到阅读范围的长边宽、比插图框宽，在正文里溢出。
/// 网上的图是外部输入：解码器 panic 时按没缩放算（原图），不让一张图摔掉整本书的优化（这里在主线程）。
fn fit_for_epub(bytes: Vec<u8>, screen: imgopt::Screen) -> Vec<u8> {
    imgopt::guard(|| imgopt::downscale_for_epub(&bytes, screen)).unwrap_or(bytes)
}

/// URL 的 origin（`scheme://host/`），抓图时作 Referer（微信 mmbiz 等防盗链要；多数 CDN 只认同源）。协议相对的 `//host/…`
/// 按 https 算；不是 URL → 空串（不带 Referer）。网页抽取（页面的 origin）和优化器抓远程图（图自己的 origin）共用。
pub fn origin_of(url: &str) -> String {
    let (scheme, rest) = match url.strip_prefix("//") {
        Some(r) => ("https", r),
        None => match url.split_once("://") {
            Some(p) => p,
            None => return String::new(),
        },
    };
    format!("{scheme}://{}/", rest.split('/').next().unwrap_or(rest))
}

/// 统一超时的 HTTP agent（`timeout_secs`=0 表示不限）。
pub fn http_agent(timeout_secs: u64) -> ureq::Agent {
    let mut b = ureq::AgentBuilder::new();
    if timeout_secs > 0 {
        b = b.timeout(std::time::Duration::from_secs(timeout_secs));
    }
    b.build()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn origin_of_extracts_scheme_host() {
        assert_eq!(origin_of("https://mp.weixin.qq.com/s/ID?x=1"), "https://mp.weixin.qq.com/");
        assert_eq!(origin_of("//cdn.example.com/a/b.png"), "https://cdn.example.com/");
        assert_eq!(origin_of("http://host"), "http://host/");
        assert_eq!(origin_of("notaurl"), "");
    }

    /// 远程横幅图按插图框缩（宽不超过阅读范围宽），和本地插图一样；以前按横竖选框，横图能宽到阅读范围的长边。
    #[test]
    fn fetched_banner_fits_epub_box() {
        let screen = imgopt::Screen { width: 842, height: 1455 };
        let img = image::RgbImage::from_pixel(3000, 1000, image::Rgb([120, 30, 200]));
        let mut jpg = Vec::new();
        image::DynamicImage::ImageRgb8(img).write_to(&mut std::io::Cursor::new(&mut jpg), image::ImageFormat::Jpeg).unwrap();
        let out = fit_for_epub(jpg.clone(), screen);
        let (w, h) = image::ImageReader::new(std::io::Cursor::new(&out)).with_guessed_format().unwrap().into_dimensions().unwrap();
        assert_eq!((w, h), (842, 281), "宽不超过插图框");
        assert_eq!(out, imgopt::downscale_for_epub(&jpg, screen).unwrap(), "和本地插图同一个规则");
        assert_eq!(fit_for_epub(b"not image".to_vec(), screen), b"not image", "缩不了原样");
    }
}
