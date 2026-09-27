//! 生成"测量书"（EPUB），在设备上截屏后用 `readable-measure` 量出真实可阅读范围。见 `bookconv::probe`。
//!
//! 用法: readable-probe 输出.epub
//! 退出码: 0 成功；1 用法错；2 生成/写出失败。

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.len() != 1 || args[0].starts_with("--") {
        eprintln!("用法: readable-probe 输出.epub");
        std::process::exit(1);
    }
    let epub = match bookconv::probe::probe_epub() {
        Ok(b) => b,
        Err(e) => {
            eprintln!("生成失败: {e}");
            std::process::exit(2);
        }
    };
    if let Err(e) = std::fs::write(&args[0], &epub) {
        eprintln!("写 {}: {e}", args[0]);
        std::process::exit(2);
    }
    println!("readable-probe: 测量书已写到 {}（{} 字节）", args[0], epub.len());
}
