//! EPUB → AZW3（KF8），给 Kindle 用 USB 侧载。输入应是已按设备优化过的 EPUB（`epub-optimize --device=kindle-pw12-sig`）。
//!
//! 用法: epub-to-azw3 [--ebok] 输入.epub 输出.azw3
//!   --ebok   归到 Kindle 的"书籍"（缺省"文档"PDOC：侧载书的封面显示最稳）
//! 退出码: 0 成功；1 用法错；2 转换/写出失败。

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let flags: Vec<&str> = args.iter().filter(|a| a.starts_with("--")).map(|s| s.as_str()).collect();
    let files: Vec<&String> = args.iter().filter(|a| !a.starts_with("--")).collect();
    if files.len() != 2 || flags.iter().any(|f| *f != "--ebok") {
        eprintln!("用法: epub-to-azw3 [--ebok] 输入.epub 输出.azw3");
        std::process::exit(1);
    }
    let run = || -> Result<usize, String> {
        let epub = std::fs::read(files[0]).map_err(|e| format!("读 {}: {e}", files[0]))?;
        let opts = azw3::Opts { cdetype: if flags.contains(&"--ebok") { azw3::CdeType::Ebok } else { azw3::CdeType::Pdoc }, ..Default::default() };
        let out = azw3::epub_to_azw3(&epub, &opts)?;
        std::fs::write(files[1], &out).map_err(|e| format!("写 {}: {e}", files[1]))?;
        Ok(out.len())
    };
    match run() {
        Ok(n) => println!("epub-to-azw3: {} → {}（{n} 字节）", files[0], files[1]),
        Err(e) => {
            eprintln!("转换失败: {e}");
            std::process::exit(2);
        }
    }
}
