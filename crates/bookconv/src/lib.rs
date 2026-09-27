//! bookconv —— 电子书内容层：与设备无关的内容处理，按调用方传入的阅读范围与选项工作，自己不落盘、不管书库。
//!
//! - `convert`：MOBI/AZW/AZW3/PRC/FB2/CBZ → EPUB 母版；`article`：网页 → EPUB；`pdf_ingest`：PDF 分类与转 EPUB/裁白边。
//! - `optimize`：按设备优化 EPUB（图片缩放、漫画单趟处理、灰度），内含清洗层 `wash`（字体字号解锁、排版、章节分页、目录）。
//! - `htmlproc`：XHTML 处理规则（注释、对比度、重复 id）；`imgopt`/`imgpool`：图片处理与并发。
//! - `check`：EPUB 质量门；`epub`/`epubzip`：EPUB 组装与读取；`probe`：量可阅读范围用的测量书。
//!
//! 使用方：`library`（书库 `booklib`）、`azw3`（AZW3 写出）和本 crate 的命令行工具。
pub mod article;
pub mod check;
pub mod comic_detect;
pub mod comic_pdf;
pub mod ncx;
pub mod convert;
pub mod direction;
pub mod epub;
pub mod epubzip;
pub mod html;
pub mod htmlproc;
pub mod imgopt;
pub mod netimg;
pub mod imgpool;
pub mod naming;
pub mod placeholder;
pub mod probe;
pub mod optimize;
pub mod pdf_ingest;
pub mod stats;
pub mod util;
pub mod wash;

/// 调用方（书库、AZW3 写出器）读 zip 时用同一个版本。
pub use zip;
