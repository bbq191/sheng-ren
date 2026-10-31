//! 集成测试共用：造测试书、跑 booklib 命令。
#![allow(dead_code)] // 每个测试文件只用到其中一部分

use bookconv::epub::{assemble, Book, BookMeta, Chapter};
use library::{DeviceEnv, Library};
use std::path::{Path, PathBuf};
use std::process::Output;

/// 测试用的"设备"目录（`<书库>-dev`）：MTP 挂载目录，下面 kindle、ireader 各有一个存储（算接上了）。
/// 测试一律不碰真设备：MTP 指向这里。
pub fn dev_dir(lib: &Path) -> PathBuf {
    let d = PathBuf::from(format!("{}-dev", lib.display()));
    for m in ["kindle", "ireader"] {
        std::fs::create_dir_all(d.join(m).join("Internal Storage")).unwrap();
    }
    d
}

/// 设备上 `mode`（kindle/ireader）的产物根目录：`<存储>/documents`。
pub fn documents(lib: &Path, mode: &str) -> PathBuf {
    dev_dir(lib).join(mode).join("Internal Storage/documents")
}

/// 打开书库，设备指向测试目录（见 [`dev_dir`]）。
pub fn open(lib: &Path) -> Library {
    let mut l = Library::open(lib).unwrap();
    l.set_device_env(DeviceEnv { mtp_base: dev_dir(lib), mount_check: false });
    l
}

/// 在书库 `profiles/` 里放一个产物留在电脑上的自定义模式 `pc`（掌阅的参数，去掉 `[deliver]`）。返回书库路径。
pub fn with_pc_profile(lib: &Path) -> PathBuf {
    let src = std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("../profile/profiles/ireader.toml")).unwrap();
    let toml = &src[..src.find("[deliver]").expect("ireader.toml 有 [deliver]")];
    std::fs::create_dir_all(lib.join("profiles")).unwrap();
    std::fs::write(lib.join("profiles/pc.toml"), toml).unwrap();
    lib.to_path_buf()
}

pub fn sample_epub(title: &str) -> Vec<u8> {
    sample_epub_with(title, "")
}

/// 书名相同、正文不同的书（`marker` 写进第一段开头）：同名同作者的另一个版本。
pub fn sample_epub_with(title: &str, marker: &str) -> Vec<u8> {
    let long = format!("{marker}{}", "正文段落，足够长的文字内容，确保标题页之后的内容超过门槛。".repeat(5));
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
    cmd.env("BOOKLIB_MTP_NO_MOUNT_CHECK", "1").env("BOOKLIB_NO_NET", "1");
    if let Some(lib) = lib {
        cmd.arg(format!("--library={}", lib.display()));
        // 书库还没建（命令本身不该建出东西）时不建设备目录
        let dev = if lib.exists() { dev_dir(lib) } else { PathBuf::from(format!("{}-dev", lib.display())) };
        cmd.env("BOOKLIB_MTP_DIR", dev);
    } else {
        cmd.env("BOOKLIB_MTP_DIR", "/nonexistent-booklib-test");
    }
    cmd.args(args).output().unwrap()
}

pub fn stdout(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}

pub fn stderr(o: &Output) -> String {
    String::from_utf8_lossy(&o.stderr).into_owned()
}
