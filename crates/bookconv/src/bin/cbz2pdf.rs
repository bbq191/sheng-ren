//! 命令行：漫画 CBZ → 按设备的固定版式 PDF（一图一页）。每页单趟处理：解码一次 → 裁白边 → 按设备 PDF 阅读范围里
//! 的绘制尺寸缩放一次 → 编码一次；不需要处理的页原图直嵌（见 `convert::cbz::cbz_to_pdf`）。黑白屏设备转 256 级灰度，不做抖动。
//!
//! 用法: cbz2pdf --device=<设备> 输入.cbz 输出.pdf
//! 退出码: 0 成功；1 用法错；2 转换失败（输入原样不动；输出先写临时文件再改名，失败不留半成品）。

use bookconv::util::cli::{self, die};

fn main() {
    cli::restore_sigpipe();
    let args: Vec<String> = std::env::args().skip(1).collect();
    let files: Vec<&String> = args.iter().filter(|a| !a.starts_with("--device=")).collect();
    if files.len() != 2 || files.iter().any(|a| a.starts_with("--")) {
        die(cli::USAGE, "用法: cbz2pdf --device=<设备> 输入.cbz 输出.pdf");
    }
    // 设备 PDF 的真实可阅读范围（没有实测值时是标称屏幕）和是不是黑白屏
    let device = profile::device_from_args(&args).unwrap_or_else(|e| die(cli::USAGE, e));
    let (screen, grayscale) = (device.readable(profile::Format::Pdf), !device.color);
    let data = cli::read_or_die(files[0]);
    let pdf = bookconv::convert::cbz::cbz_to_pdf(&data, screen, grayscale).unwrap_or_else(|e| die(cli::FAILED, format!("转换失败: {e}")));
    cli::write_or_die(files[1], &pdf);
    println!("cbz2pdf: {} → {} 字节", files[0], pdf.len());
}
