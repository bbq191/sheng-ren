//! 幂等标记：优化产物内埋的 `META-INF/eink-optimized` 的内容。
use super::*;

/// 标记值分等级：**含清洗层的完整优化 = 版本号本身**；只跑核心遍（`wash=None`）= `<版本>-core`。版本号按这本书走的那一路：
/// 文字书 [`OPTIMIZE_VERSION`]、漫画 [`COMIC_VERSION`]（只改了一路的规则时，另一路的产物逐字节不变、设备上不用重传）。
pub fn marker_value(full: bool, comic: bool) -> String {
    let v = if comic { COMIC_VERSION } else { OPTIMIZE_VERSION };
    if full { v.to_string() } else { format!("{v}-core") }
}

/// 书里优化标记（[`OPTIMIZE_MARKER`]）的内容（去掉首尾空白），即优化它的优化器版本（见 [`marker_value`]）。
/// 文件打不开、不是 zip、没有标记、标记不是 UTF-8 → `None`（没优化过，或不是本优化器的产物）。标记超过 64 字节也算读不出来。
pub fn optimized_version_file(path: &std::path::Path) -> Option<String> {
    let f = std::fs::File::open(path).ok()?;
    let mut ar = ZipArchive::new(std::io::BufReader::new(f)).ok()?;
    let entry = ar.by_name(OPTIMIZE_MARKER).ok()?;
    let bytes = crate::util::read_capped(entry, 64, 0).ok()??;
    Some(String::from_utf8(bytes).ok()?.trim().to_string())
}
