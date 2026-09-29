//! 生成"测量书"（EPUB），在设备上截屏后用 `readable-measure` 量出真实可阅读范围。见 `bookconv::probe`。
//!
//! 用法: readable-probe 输出.epub
//! 退出码: 0 成功；1 用法错；2 生成/写出失败（先写临时文件再改名，失败不留半成品）。

use bookconv::util::cli::{self, die};

fn main() {
    cli::restore_sigpipe();
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.len() != 1 || args[0].starts_with("--") {
        die(cli::USAGE, "用法: readable-probe 输出.epub");
    }
    let epub = bookconv::probe::probe_epub().unwrap_or_else(|e| die(cli::FAILED, format!("生成失败: {e}")));
    cli::write_or_die(&args[0], &epub);
    println!("readable-probe: 测量书已写到 {}（{} 字节）", args[0], epub.len());
}
