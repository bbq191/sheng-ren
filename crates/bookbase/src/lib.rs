//! 书处理的基础件（2026-10-15 从 `bookconv` 拆出来）：不依赖本仓库别的 crate，`bookconv` 原样再导出
//! （`bookconv::util`、`bookconv::html`、`bookconv::color` 照旧能用）。
//!
//! - `util`：原子写（`produce_then_replace`）、限量读（`read_capped`）、并行（`par_map`）、图片格式表、XML 转义；
//! - `html`：容错的 HTML/CSS 文本操作（单双引号、注释、`data-id` 不误匹配）；
//! - `color`：CSS 颜色解析、对比度；
//! - `epubpath`：EPUB 里的路径换算、链接解析。

pub mod color;
pub mod epubpath;
pub mod html;
pub mod util;
