//! 远程图抓取（优化器内联远程 `<img>` 与网页抽取 `article` 共用）。
use crate::convert::common;
use crate::imgopt;

/// 抓图 UA（网页抽取与优化器共用同一标识）。
pub const UA: &str = "Mozilla/5.0 (compatible; readlater/1.0)";

/// 抓远程图；给了 `screen` 就按该设备降采样，`None` = 保留原图（入库母版用）。`src` 支持协议相对 `//host/path`；非 http(s) 返回 None。
/// 返回 (字节, 扩展名, mime)；非图（魔数不认）→ None。上限 20MB。
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
    let mut bytes: Vec<u8> = Vec::new();
    use std::io::Read;
    resp.into_reader().take(20 * 1024 * 1024).read_to_end(&mut bytes).ok()?;
    let (ext, mime) = common::image_ext_mime(&bytes)?; // 魔数识别 JPEG/PNG/GIF；非图→None
    if let Some(smaller) = screen.and_then(|s| imgopt::downscale_for_device(&bytes, s)) {
        return Some((smaller, ext, mime));
    }
    Some((bytes, ext, mime))
}

/// 统一超时的 HTTP agent（`timeout_secs`=0 表示不限）。
pub fn http_agent(timeout_secs: u64) -> ureq::Agent {
    let mut b = ureq::AgentBuilder::new();
    if timeout_secs > 0 {
        b = b.timeout(std::time::Duration::from_secs(timeout_secs));
    }
    b.build()
}
