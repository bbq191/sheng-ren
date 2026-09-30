//! 从测量书的两张截图（竖长黑块、横宽黑块）量出阅读器的真实可阅读范围，打印可直接贴进 profile 的 TOML。见 `bookconv::probe`。
//!
//! 用法: readable-measure [--device=<id>] 竖长图截图.png 横宽图截图.png
//!   TOML 段名按产物格式：`[readable.epub]`、Kindle 的 `[readable.azw3]`。给了 `--device` 按这个阅读模式的产物格式写，
//!   没给写 `[readable.epub]` 并提示。
//! 退出码: 0 成功（有可疑之处时仍输出，但会列出警告）；1 用法错；2 读图/测量失败。

use bookconv::util::cli::{self, die};

fn main() {
    cli::restore_sigpipe();
    let args: Vec<String> = std::env::args().skip(1).collect();
    let files: Vec<&String> = args.iter().filter(|a| !a.starts_with("--")).collect();
    let device_given = args.iter().any(|a| a.starts_with("--device="));
    if files.len() != 2 || files.len() + usize::from(device_given) != args.len() {
        die(cli::USAGE, "用法: readable-measure [--device=<id>] 竖长图截图.png 横宽图截图.png");
    }
    let format = if device_given { Some(profile::device_from_args(&args).unwrap_or_else(|e| die(cli::USAGE, e)).format()) } else { None };
    let load = |p: &str| image::open(p).unwrap_or_else(|e| die(cli::FAILED, format!("读 {p}: {e}"))).to_luma8();
    let m = bookconv::probe::measure(&load(files[0]), &load(files[1])).unwrap_or_else(|e| die(cli::FAILED, format!("测量失败: {e}")));
    let (sw, sh) = m.screen;
    let (rw, rh) = m.readable;
    let tb = m.tall_box;
    let wb = m.wide_box;
    println!("截图尺寸 {sw}×{sh}，可阅读范围 {rw}×{rh}");
    println!("竖长黑块 {}×{}（上 {} 下 {}），横宽黑块 {}×{}（左 {} 右 {}）", tb.2 - tb.0, tb.3 - tb.1, tb.1, sh - tb.3, wb.2 - wb.0, wb.3 - wb.1, wb.0, sw - wb.2);
    for w in &m.warnings {
        println!("⚠ {w}");
    }
    let ext = format.map_or("epub", |f| f.ext());
    if format.is_none() {
        println!("\n（产物是 AZW3 的阅读模式（kindle）段名要写 [readable.azw3]；给 --device=<id> 自动按产物格式写）");
    }
    println!("\n[readable.{ext}]\nwidth = {rw}\nheight = {rh}");
}
