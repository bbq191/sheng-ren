//! 集成测试共用：造测试书、跑 booklib 命令。
#![allow(dead_code)] // 每个测试文件只用到其中一部分

use bookconv::epub::{assemble, Book, BookMeta, Chapter};
use std::path::Path;
use std::process::Output;

pub fn sample_epub(title: &str) -> Vec<u8> {
    let long = "正文段落，足够长的文字内容，确保标题页之后的内容超过门槛。".repeat(5);
    let mut book = Book {
        meta: BookMeta { book_id: "t".into(), title: title.into(), author: "作者".into(), language: "zh".into(), publisher: "".into(), cover: None, cover_ext: "jpg".into(), cover_media_type: "image/jpeg".into(), subjects: Vec::new() },
        chapters: vec![Chapter { title: "第一章".into(), html_body: format!("<h1>第一章</h1><p>{long}</p><h2>第一节</h2><p>{long}</p>"), level: 1 }],
        resources: vec![],
        nav: vec![],
    };
    assemble(&mut book).unwrap()
}

pub fn jpeg(w: u32, h: u32) -> Vec<u8> {
    let mut out = Vec::new();
    image::codecs::jpeg::JpegEncoder::new(&mut out).encode_image(&image::RgbImage::from_pixel(w, h, image::Rgb([90, 90, 90]))).unwrap();
    out
}

/// 在 `lib` 书库上跑 `booklib <args>`（`lib` 为 `None` 时不加 `--library=`）。
pub fn booklib(lib: Option<&Path>, args: &[&str]) -> Output {
    let mut cmd = std::process::Command::new(env!("CARGO_BIN_EXE_booklib"));
    if let Some(lib) = lib {
        cmd.arg(format!("--library={}", lib.display()));
    }
    cmd.args(args).output().unwrap()
}

pub fn stdout(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}

pub fn stderr(o: &Output) -> String {
    String::from_utf8_lossy(&o.stderr).into_owned()
}
