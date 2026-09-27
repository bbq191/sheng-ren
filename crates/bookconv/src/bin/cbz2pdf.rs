//! host CLI：漫画 CBZ → 按设备的固定版式 PDF（一图一页）。每页单趟处理：解码一次 → 裁白边 → 按设备 PDF 阅读范围里
//! 的绘制尺寸缩放一次 → 编码一次；不需要处理的页原图直嵌（见 `convert::cbz::cbz_to_pdf`）。黑白屏设备转 256 级灰度，不做抖动。
//!
//! 用法: cbz2pdf --device=<设备> 输入.cbz 输出.pdf
//! 退出码: 0 成功；1 用法错；2 转换失败（输入原样不动）。

use bookconv::convert::cbz::cbz_to_pdf;

/// `--device=<profile id>`（必填）→ 该设备 PDF 的真实可阅读范围（没有实测值时是标称屏幕）和是不是黑白屏；
/// 缺失或未知 id 时打印可用 id 并以用法错退出。
fn device_screen(args: &[String]) -> (bookconv::imgopt::Screen, bool) {
    let id = args.iter().find_map(|a| a.strip_prefix("--device="));
    match id.and_then(profile::get) {
        Some(p) => (p.readable(profile::Format::Pdf), !p.color),
        None => {
            let ids: Vec<_> = profile::Registry::builtin().iter().map(|p| p.id.as_str()).collect();
            eprintln!("需要 --device=<设备>，可选: {}", ids.join(" / "));
            std::process::exit(1);
        }
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let files: Vec<&String> = args.iter().filter(|a| !a.starts_with("--device=")).collect();
    if files.len() != 2 || files.iter().any(|a| a.starts_with("--")) {
        eprintln!("用法: cbz2pdf --device=<设备> 输入.cbz 输出.pdf");
        std::process::exit(1);
    }
    let (screen, grayscale) = device_screen(&args);
    let data = match std::fs::read(files[0]) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("读 {}: {e}", files[0]);
            std::process::exit(2);
        }
    };
    let pdf = match cbz_to_pdf(&data, screen, grayscale) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("转换失败: {e}");
            std::process::exit(2);
        }
    };
    if let Err(e) = std::fs::write(files[1], &pdf) {
        eprintln!("写 {}: {e}", files[1]);
        std::process::exit(2);
    }
    println!("cbz2pdf: {} → {} 字节", files[0], pdf.len());
}
