//! EPUB → AZW3（KF8）写出器：Kindle USB 侧载的唯一可用格式（EPUB 不认，2026-09-27 真机实测）。
//!
//! clean-room，依 KF8/MOBI 格式规范实现，不参考 GPL 的 KindleUnpack / Calibre 代码；
//! 读取侧已有 `bookconv::convert::{palm, kf8}` 可做往返校验。尚未实现。
