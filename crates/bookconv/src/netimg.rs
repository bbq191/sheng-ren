//! 远程图抓取（优化器内联远程 `<img>` 与网页抽取 `article` 共用）。
use crate::convert::common;
use crate::imgopt;

/// 远程图的下载上限。
pub const MAX_IMAGE_BYTES: u64 = 20 * 1024 * 1024;

/// 抓图 UA（网页抽取与优化器共用同一标识）。
pub const UA: &str = "Mozilla/5.0 (compatible; readlater/1.0)";

/// 抓远程图；给了 `screen` 就按该设备降采样，`None` = 保留原图（入库母版用）。`src` 支持协议相对 `//host/path`；非 http(s) 返回 None。
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
    // 网上的图是外部输入：解码器 panic 时按没缩放算（原图），不让一张图摔掉整本书的优化（这里在主线程）
    if let Some(smaller) = screen.and_then(|s| imgopt::guard(|| imgopt::downscale_for_device(&bytes, s))) {
        return Some((smaller, ext, mime));
    }
    Some((bytes, ext, mime))
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
}
