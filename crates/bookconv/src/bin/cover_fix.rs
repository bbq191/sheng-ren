//! 给一本已有的 EPUB **无损补封面**：有的书没有有效的封面声明，阅读器（如 xochitl）就取不到封面缩略图。
//!
//! 用法: cover-fix 输入.epub 输出.epub [封面缩略图.png]
//!   - 只改 OPF（`wash::ensure_cover_declared`：保证 `<meta name="cover">` 指向真图片），**其余条目原样拷贝（zip raw copy，
//!     压缩数据一个字节不动，图片零重编码）**；输出与输入同名条目顺序一致。
//!   - 输出先写同目录临时文件、成功才改名：输入输出可以是同一个文件（就地修），中途失败原书不动。
//!   - 第三个参数给出时，另外把封面图按 xochitl 缩略图规格（552×981 RGB PNG、白底居中等比）写出，
//!     供直接放进设备书库文档的 `<uuid>.thumbnails/cover.png`。
//!
//! 退出码: 0 成功（含"本来就有有效封面，无需改"）；1 用法错；2 失败。
use bookconv::epubzip::Entry;
use bookconv::util::cli::{self, die};
use std::path::Path;

fn main() {
    cli::restore_sigpipe();
    let a: Vec<String> = std::env::args().skip(1).collect();
    if a.len() < 2 || a.len() > 3 {
        die(cli::USAGE, "用法: cover-fix 输入.epub 输出.epub [封面缩略图.png]");
    }
    if let Err(e) = run(Path::new(&a[0]), Path::new(&a[1]), a.get(2).map(Path::new)) {
        die(cli::FAILED, format!("失败: {e}"));
    }
}

fn run(input: &Path, output: &Path, png: Option<&Path>) -> Result<(), String> {
    let mut zin = zip::ZipArchive::new(std::io::BufReader::new(std::fs::File::open(input).map_err(|e| e.to_string())?)).map_err(|e| e.to_string())?;
    // 只读非图片条目（html/opf/…），图片留空占位——封面声明检查不需要图片字节。
    let mut entries: Vec<Entry> = bookconv::epubzip::read_skeleton(&mut zin)?.entries;
    if !entries.iter().any(|e| e.name.ends_with(".opf")) {
        return Err("找不到 OPF".into());
    }
    let changed = bookconv::wash::ensure_cover_declared(&mut entries);
    let opf = entries.iter().find(|e| e.name.ends_with(".opf")).ok_or("找不到 OPF")?;
    // 先写临时文件再改名：此前直接 `File::create(output)`，输入输出同路径时先把原书截成 0 字节再去读它。
    bookconv::util::produce_then_replace(&bookconv::util::tmp_beside(output, "cover-fix"), output, |tmp| {
        let mut zout = zip::ZipWriter::new(std::io::BufWriter::new(std::fs::File::create(tmp).map_err(|e| e.to_string())?));
        for i in 0..zin.len() {
            let f = zin.by_index(i).map_err(|e| e.to_string())?;
            if changed && f.name() == opf.name {
                let o = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
                zout.start_file(f.name(), o).map_err(|e| e.to_string())?;
                std::io::Write::write_all(&mut zout, &opf.data).map_err(|e| e.to_string())?;
            } else {
                zout.raw_copy_file(f).map_err(|e| e.to_string())?; // 压缩数据原样拷贝
            }
        }
        let mut w = zout.finish().map_err(|e| e.to_string())?;
        std::io::Write::flush(&mut w).map_err(|e| e.to_string())
    })?;
    println!("封面声明: {}", if changed { "已修复（OPF 已改）" } else { "本来就有效，无需改" });
    if let Some(png_path) = png {
        let (_, bytes) = bookconv::epubzip::cover_image_of(output).ok_or("找不到封面图")?;
        let img = image::load_from_memory(&bytes).map_err(|e| format!("解码封面失败: {e}"))?.to_rgb8();
        let (tw, th) = (552u32, 981u32);
        let s = (tw as f32 / img.width() as f32).min(th as f32 / img.height() as f32);
        let (nw, nh) = (((img.width() as f32 * s).round() as u32).max(1), ((img.height() as f32 * s).round() as u32).max(1));
        let resized = image::imageops::resize(&img, nw, nh, image::imageops::FilterType::Lanczos3);
        let mut canvas = image::RgbImage::from_pixel(tw, th, image::Rgb([255, 255, 255]));
        image::imageops::overlay(&mut canvas, &resized, ((tw - nw) / 2) as i64, ((th - nh) / 2) as i64);
        let mut out = Vec::new();
        canvas.write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png).map_err(|e| e.to_string())?;
        bookconv::util::write_atomic(png_path, &out)?;
        println!("封面缩略图: {}（{tw}×{th}）", png_path.display());
    }
    Ok(())
}
