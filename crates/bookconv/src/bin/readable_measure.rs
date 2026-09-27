//! 从测量书的两张截图（竖长黑块、横宽黑块）量出阅读器的真实可阅读范围，打印可直接贴进 profile 的 TOML。见 `bookconv::probe`。
//!
//! 用法: readable-measure [--format=epub|azw3|pdf] 竖长图截图.png 横宽图截图.png
//! 退出码: 0 成功（有可疑之处时仍输出，但会列出警告）；1 用法错；2 读图/测量失败。

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let format = args.iter().find_map(|a| a.strip_prefix("--format=")).unwrap_or("epub");
    let files: Vec<&String> = args.iter().filter(|a| !a.starts_with("--")).collect();
    if files.len() != 2 || !["epub", "azw3", "pdf"].contains(&format) {
        eprintln!("用法: readable-measure [--format=epub|azw3|pdf] 竖长图截图.png 横宽图截图.png");
        std::process::exit(1);
    }
    let load = |p: &str| match image::open(p) {
        Ok(i) => i.to_luma8(),
        Err(e) => {
            eprintln!("读 {p}: {e}");
            std::process::exit(2);
        }
    };
    let m = match bookconv::probe::measure(&load(files[0]), &load(files[1])) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("测量失败: {e}");
            std::process::exit(2);
        }
    };
    let (sw, sh) = m.screen;
    let (rw, rh) = m.readable;
    let tb = m.tall_box;
    let wb = m.wide_box;
    println!("截图尺寸 {sw}×{sh}，可阅读范围 {rw}×{rh}");
    println!("竖长黑块 {}×{}（上 {} 下 {}），横宽黑块 {}×{}（左 {} 右 {}）", tb.2 - tb.0, tb.3 - tb.1, tb.1, sh - tb.3, wb.2 - wb.0, wb.3 - wb.1, wb.0, sw - wb.2);
    for w in &m.warnings {
        println!("⚠ {w}");
    }
    println!("\n[readable.{format}]\nwidth = {rw}\nheight = {rh}");
}
