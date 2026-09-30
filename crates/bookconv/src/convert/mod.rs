//! 源格式 → **与设备无关的母版 EPUB**（入库用）。现在只支持 CBZ → 每页一张原图的 EPUB（`cbz::cbz_to_epub`）；
//! EPUB 本身不转换。按阅读模式的优化是入库之后的独立步骤，这里不做。按扩展名分派在书库那边（`library::convert_to_epub`）。
//! 不支持：CBR（RAR 解包要 C++ 依赖和受限许可，先用外部工具解成 CBZ）；其它格式（MOBI/AZW3/FB2/PDF 等）2026-09-29 起不再支持。
//! KF8（AZW3）读取器不在这里：它只给 AZW3 写出器做回读自检和测试用，在 `azw3::read`。

/// 格式转换（CBZ → EPUB）的版本：改了会影响转换结果的代码要加一。书库只把它放进**需要转换的来源**（CBZ）的指纹，
/// 原本就是 EPUB 的书不受影响。
/// - 1（2026-09-28）：当时还覆盖 MOBI/AZW3/FB2/PDF 的转换；那些格式 2026-09-29 删掉，CBZ 的转换结果没有变，版本号不动。
pub const CONVERT_VERSION: &str = "1";

pub mod cbz;
pub mod common;
