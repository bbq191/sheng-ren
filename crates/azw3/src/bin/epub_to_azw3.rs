//! EPUB → AZW3（KF8），给 Kindle 用 USB 侧载。输入应是已按设备优化过的 EPUB（`epub-optimize --device=kindle`；书库 `booklib build` 自动转）。
//!
//! 用法: epub-to-azw3 [--ebok] 输入.epub 输出.azw3
//!   --ebok   归到 Kindle 的"书籍"（缺省"文档"PDOC：侧载书的封面显示最稳）
//! 退出码: 0 成功；1 用法错；2 转换/写出失败（先写临时文件再改名，失败不留半成品）。

use bookconv::util::cli::{self, die};

const USAGE: &str = "用法: epub-to-azw3 [--ebok] 输入.epub 输出.azw3";

fn main() {
    cli::restore_sigpipe();
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "-h" || a == "--help") {
        println!("{USAGE}");
        return;
    }
    let flags: Vec<&str> = args.iter().filter(|a| a.starts_with("--")).map(|s| s.as_str()).collect();
    let files: Vec<&String> = args.iter().filter(|a| !a.starts_with("--")).collect();
    if files.len() != 2 || flags.iter().any(|f| *f != "--ebok") {
        die(cli::USAGE, USAGE);
    }
    let epub = cli::read_or_die(files[0]);
    let opts = azw3::Opts { cdetype: if flags.contains(&"--ebok") { azw3::CdeType::Ebok } else { azw3::CdeType::Pdoc }, ..Default::default() };
    let out = azw3::epub_to_azw3(&epub, &opts).unwrap_or_else(|e| die(cli::FAILED, format!("转换失败: {e}")));
    cli::write_or_die(files[1], &out);
    println!("epub-to-azw3: {} → {}（{} 字节）", files[0], files[1], out.len());
}
