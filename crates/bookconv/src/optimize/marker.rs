//! 幂等标记：优化产物内埋的 `META-INF/eink-optimized` 的内容。
use super::*;

/// 标记值：含清洗层的完整优化写 `full`，只跑核心遍（`wash=None`）写 `core`。**不写版本号**（2026-10-09 用户定）：以前写的是
/// [`OPTIMIZE_VERSION`]/[`COMIC_VERSION`]，规则版本一加，所有产物的字节都跟着变，掌阅、Move 上的书哪怕内容一个字没变也要全部重传
/// （Move 进度回到开头）。版本号照旧进书库的指纹（决定要不要重新生成），重新生成出逐字节相同的书就不再传。
pub fn marker_value(full: bool) -> &'static str {
    if full { "full" } else { "core" }
}

/// 书里优化标记（[`OPTIMIZE_MARKER`]）的内容（去掉首尾空白，见 [`marker_value`]；2026-10-09 以前的产物里是优化器版本号）。
/// 文件打不开、不是 zip、没有标记、标记不是 UTF-8 → `None`（没优化过，或不是本优化器的产物）。标记超过 64 字节也算读不出来。
pub fn optimized_version_file(path: &std::path::Path) -> Option<String> {
    let f = std::fs::File::open(path).ok()?;
    let mut ar = ZipArchive::new(std::io::BufReader::new(f)).ok()?;
    let entry = ar.by_name(OPTIMIZE_MARKER).ok()?;
    let bytes = crate::util::read_capped(entry, 64, 0).ok()??;
    Some(String::from_utf8(bytes).ok()?.trim().to_string())
}
