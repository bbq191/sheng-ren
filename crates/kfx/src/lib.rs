//! KFX（Kindle 增强排版格式）读写。
//!
//! clean-room：Ion 编解码只照 Amazon 公开的 Ion 规范；容器布局和片段含义来自对 KFX 样本文件的**黑盒数据分析**
//! （只看文件字节），不参考 KFX Input/Output、DeDRM_tools、Calibre 的代码，也不看照这些代码写的讲解。
//! 现状和推出来的结构见 `docs/kfx.md`。[`write`] 是 EPUB → KFX 写出器（最小版，见模块文档）。

pub mod container;
pub mod ion;
pub mod write;
pub mod yj;

/// CSS 层叠（2026-10-10 挪到 bookconv，掌阅、Move 的 `kindle_rules` 算正文字号时用同一套）。
pub use bookconv::cascade as css;
pub use container::{Body, Container, Entity};
pub use write::{epub_text_styles, epub_to_kfx, epub_to_kfx_from, Opts, WRITER_VERSION};
