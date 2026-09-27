//! 幂等标记：优化产物内埋的 `META-INF/eink-optimized` 的内容。
use super::*;

/// 标记值分等级：**含清洗层的完整优化 = 版本号本身**；只跑核心遍（`wash=None`）= `<版本>-core`。
pub fn marker_value(full: bool) -> String {
    if full { OPTIMIZE_VERSION.to_string() } else { format!("{OPTIMIZE_VERSION}-core") }
}
