//! 优化器图片降采样——按目标设备屏幕（[`Screen`]，来自 `profile` crate）做设备级优化。
//!
//! 书里常带 2000–4000px 的高清原图，超出屏幕的像素**纯属浪费**：拖慢加载、吃内存、还逼阅读器在渲染期
//! 临时缩放（慢且质量不可控）。优化器在组包/优化阶段把超大图 Lanczos3 预缩进屏幕框，缩放质量我们控。
//!
//! 纪律：**只缩不放、保宽高比、保原格式、达标即跳过（幂等 + 免二次编码损失）、任何失败原样保留**
//! （绝不因优化损坏原书）。文字书插图只碰 JPEG/PNG（书内图几乎都是；GIF 可能动图，跳过不冒险）。
//! 漫画页另有一套（[`prepare_comic_page_for_epub`]）：按阅读范围放大或缩小、四边留 `comic_margin` 像素白边；
//! 静态的 GIF/WebP 页也处理（转成 PNG/JPEG）。
//!
//! 本文件只留入口与再导出，各段拆在 `imgopt/` 下：`decode`（文件头、解码、缩放、透明合成）、`page8`
//! （归一后的整页与灰度转换）、`comic`（裁边、漫画页排版与跨页）、`thumbnail`（书架缩略图）、
//! `textbook`（文字书插图、背景图与条目元数据）、`tests`（各段的小测试）。

mod comic;
mod decode;
mod page8;
mod textbook;
mod thumbnail;

#[cfg(test)]
mod tests;

pub use profile::Screen;

/// 单测用的设备屏幕：rMPP Move（954×1696）。历史用例与真机数据都是在 Move 上量的，断言里的数字以它为准。
#[cfg(test)]
pub(crate) fn test_screen() -> Screen {
    profile::fixture("xochitl")
        .expect("内置 xochitl profile")
        .screen
}

pub use comic::{
    autocontrast_lut, comic_page_number, decode_page, gamma_lut, is_spread,
    prepare_comic_page_for_epub, prepare_comic_page_limited, prepare_comic_spread, trim_page,
    ComicPage, PageDecode, TrimMode, MIN_AUTOCONTRAST_RANGE, SPREAD_MIN_ASPECT,
};
pub use decode::{
    decode_capped, decode_oriented, flatten_transparent_png, header_dims, is_convertible_webp,
    pixel_count, webp_to_png, JPEG_QUALITY_COMIC, MAX_COMIC_DECODE_PIXELS, MAX_DECODE_PIXELS,
};
pub use page8::Page8;
pub use textbook::{
    converted_media_type, darken_gray_image, display_dims, downscale_background,
    downscale_for_epub, downscale_for_epub_limited, is_page_image, page_tile,
};
pub use thumbnail::{cover_thumbnail, thumb_aspect_fits};

pub(crate) use decode::guard;
#[cfg(test)]
pub(crate) use comic::{
    comic_layout, decode_comic, gif_frame_count, spread_widths, webp_is_lossless,
    TRIM_MAX_FRACTION,
};
#[cfg(test)]
pub(crate) use decode::{comic_header_dims, flatten_alpha_on_white, text_decode_limit, TEST_ANIM_WEBP};
#[cfg(test)]
pub(crate) use page8::rgb8_into_luma8;
#[cfg(test)]
pub(crate) use thumbnail::thumb_scan_margins;
