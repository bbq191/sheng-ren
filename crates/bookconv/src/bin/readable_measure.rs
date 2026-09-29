//! 从测量书的两张截图（竖长黑块、横宽黑块）量出阅读器的真实可阅读范围，打印可直接贴进 profile 的 TOML。见 `bookconv::probe`。
//!
//! 用法: readable-measure 竖长图截图.png 横宽图截图.png
//! 退出码: 0 成功（有可疑之处时仍输出，但会列出警告）；1 用法错；2 读图/测量失败。

use bookconv::util::cli::{self, die};

fn main() {
    cli::restore_sigpipe();
    let args: Vec<String> = std::env::args().skip(1).collect();
    let files: Vec<&String> = args.iter().filter(|a| !a.starts_with("--")).collect();
    if files.len() != 2 || files.len() != args.len() {
        die(cli::USAGE, "用法: readable-measure 竖长图截图.png 横宽图截图.png");
    }
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
    println!("\n[readable.epub]\nwidth = {rw}\nheight = {rh}");
}
