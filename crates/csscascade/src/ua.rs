//! 标签的缺省样式表（HTML 规范的 UA 样式表里影响版面的那几条，Send to Kindle 照它排）。说明见 `bookconv::uastyle`。

/// 标题：(标签, 字号倍数, 上下外边距 em)。都是粗体。
pub const HEADINGS: [(&str, f64, f64); 6] = [("h1", 2.0, 0.67), ("h2", 1.5, 0.83), ("h3", 1.17, 1.0), ("h4", 1.0, 1.33), ("h5", 0.83, 1.67), ("h6", 0.67, 2.33)];

/// 上下外边距 1em 的块。
pub const BLOCK_MARGIN_TAGS: [&str; 3] = ["p", "dl", "pre"];

/// 上下 1em、左右 40px 的块。
pub const INDENTED_BLOCK_TAGS: [&str; 2] = ["blockquote", "figure"];

/// UA 样式表左右缩进的 px 数（`blockquote`、`figure`）。
pub const INDENT_PX: u32 = 40;
