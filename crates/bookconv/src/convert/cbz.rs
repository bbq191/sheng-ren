//! CBZ（漫画 zip 归档）：解包 → 图片按文件名自然序排 → 母版 EPUB（入库用）。
//! macOS 打包带进来的 `__MACOSX/` 目录和 `._*` 资源分叉文件不是页面，跳过。

use std::io::Read;
use zip::ZipArchive;

/// 归档里的页面图片条目名（jpg/jpeg/png，按文件名自然序：page_2 < page_10）。跳过目录、macOS 垃圾条目和非图片条目。
fn page_names<R: Read + std::io::Seek>(zip: &mut ZipArchive<R>) -> Vec<String> {
    let mut names: Vec<String> = (0..zip.len())
        .filter_map(|i| zip.by_index(i).ok().filter(|f| !f.is_dir()).map(|f| f.name().to_string()))
        .filter(|n| !is_macos_junk(n) && crate::imgopt::is_downscalable(n))
        .collect();
    names.sort_by(|a, b| natural_cmp(a, b));
    names
}

/// macOS 压缩时附带的元数据：`__MACOSX/` 下的一切，以及文件名以 `._` 开头的 AppleDouble 资源分叉。
fn is_macos_junk(name: &str) -> bool {
    name.split('/').any(|seg| seg == "__MACOSX") || name.rsplit('/').next().is_some_and(|base| base.starts_with("._"))
}

/// 单页图片解压后的上限。真实漫画页（含 600dpi 扫描的 PNG）远小于它；几 KB 的压缩条目能解出几 GB（zip 炸弹），
/// 目录里声明的大小也可以造假，所以既查声明、读的时候也按上限截。
const MAX_PAGE_BYTES: u64 = 256 * 1024 * 1024;

fn read_entry<R: Read + std::io::Seek>(zip: &mut ZipArchive<R>, name: &str) -> Result<Vec<u8>, String> {
    let f = zip.by_name(name).map_err(|e| e.to_string())?;
    if f.size() > MAX_PAGE_BYTES {
        return Err(format!("{name}: 解压后 {} MB，超过单页上限 {} MB（损坏或恶意的压缩包？）", f.size() >> 20, MAX_PAGE_BYTES >> 20));
    }
    let mut bytes = Vec::with_capacity(f.size() as usize);
    f.take(MAX_PAGE_BYTES + 1).read_to_end(&mut bytes).map_err(|e| format!("{name}: {e}"))?;
    if bytes.len() as u64 > MAX_PAGE_BYTES {
        return Err(format!("{name}: 解压后超过单页上限 {} MB（损坏或恶意的压缩包？）", MAX_PAGE_BYTES >> 20));
    }
    Ok(bytes)
}

/// CBZ 字节 → **与设备无关的母版 EPUB**：图片按文件名自然序每页一张，原图字节原样放进去（不缩放、不重编码），
/// 第一张当封面。按设备的缩放/补白在之后的优化步骤里做（整本会被判成漫画）。
/// 扩展名是图片但内容认不出的条目跳过并警告。
pub fn cbz_to_epub(data: &[u8], title: &str) -> Result<Vec<u8>, String> {
    use crate::epub::{Book, BookMeta, Chapter, Resource};
    let mut zip = ZipArchive::new(std::io::Cursor::new(data)).map_err(|e| format!("CBZ 打开: {e}"))?;
    let names = page_names(&mut zip);
    let mut resources: Vec<Resource> = Vec::with_capacity(names.len());
    let mut chapters = Vec::with_capacity(names.len());
    for name in &names {
        let bytes = read_entry(&mut zip, name)?;
        let Some((ext, mime)) = super::common::image_ext_mime(&bytes) else {
            eprintln!("警告：{name} 不是可识别的图片，跳过");
            continue;
        };
        let n = resources.len() + 1;
        let path = format!("images/p{n:04}.{ext}");
        chapters.push(Chapter { title: format!("第 {n} 页"), html_body: format!(r#"<div><img src="{path}" alt=""/></div>"#), level: 1 });
        resources.push(Resource { path, media_type: mime.to_string(), bytes });
    }
    let Some(first) = resources.first() else {
        return Err("CBZ 内无图片（jpg/jpeg/png）".into());
    };
    let cover_ext = first.path.rsplit('.').next().unwrap_or("jpg").to_string();
    let (cover, cover_media_type) = (Some(first.bytes.clone()), first.media_type.clone());
    let mut book = Book {
        meta: BookMeta { book_id: format!("cbz:{}", super::common::sanitize_id(title)), title: title.to_string(), author: String::new(), language: "zh".into(), publisher: String::new(), cover, cover_ext, cover_media_type },
        chapters,
        resources,
        nav: Vec::new(),
    };
    crate::epub::assemble_master(&mut book)
}

/// 自然排序：连续数字段按数值比较（去前导零后先比位数再逐位），其余按字节。
fn natural_cmp(a: &str, b: &str) -> std::cmp::Ordering {
    use std::cmp::Ordering;
    let (a, b) = (a.as_bytes(), b.as_bytes());
    let (mut i, mut j) = (0usize, 0usize);
    while i < a.len() && j < b.len() {
        if a[i].is_ascii_digit() && b[j].is_ascii_digit() {
            let (mut ni, mut nj) = (i, j);
            while ni < a.len() && a[ni].is_ascii_digit() {
                ni += 1;
            }
            while nj < b.len() && b[nj].is_ascii_digit() {
                nj += 1;
            }
            let (ta, tb) = (trim_leading_zeros(&a[i..ni]), trim_leading_zeros(&b[j..nj]));
            match ta.len().cmp(&tb.len()).then_with(|| ta.cmp(tb)) {
                Ordering::Equal => {}
                o => return o,
            }
            i = ni;
            j = nj;
        } else {
            match a[i].cmp(&b[j]) {
                Ordering::Equal => {
                    i += 1;
                    j += 1;
                }
                o => return o,
            }
        }
    }
    a.len().cmp(&b.len())
}

fn trim_leading_zeros(s: &[u8]) -> &[u8] {
    let mut k = 0;
    while k + 1 < s.len() && s[k] == b'0' {
        k += 1;
    }
    &s[k..]
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cmp::Ordering;

    #[test]
    fn natural_sort_orders_numeric_chunks() {
        let mut v = vec![
            "page_10.jpg".to_string(),
            "page_2.jpg".into(),
            "page_1.jpg".into(),
            "page_100.jpg".into(),
        ];
        v.sort_by(|a, b| natural_cmp(a, b));
        assert_eq!(v, ["page_1.jpg", "page_2.jpg", "page_10.jpg", "page_100.jpg"]);
    }

    #[test]
    fn natural_sort_handles_leading_zeros() {
        // 数值相等（去前导零后同）时，原串更长者排后（p007 长于 p7）
        assert_eq!(natural_cmp("p007", "p7"), Ordering::Greater);
        let mut v = vec!["p010".to_string(), "p09".into(), "p1".into()];
        v.sort_by(|a, b| natural_cmp(a, b));
        assert_eq!(v, ["p1", "p09", "p010"]);
    }

    fn zip_of(entries: &[(&str, &[u8])]) -> Vec<u8> {
        use std::io::Write;
        let mut buf = Vec::new();
        {
            let mut z = zip::ZipWriter::new(std::io::Cursor::new(&mut buf));
            for (name, data) in entries {
                z.start_file(*name, zip::write::SimpleFileOptions::default()).unwrap();
                z.write_all(data).unwrap();
            }
            z.finish().unwrap();
        }
        buf
    }

    fn jpeg(w: u32, h: u32) -> Vec<u8> {
        use image::{codecs::jpeg::JpegEncoder, DynamicImage, RgbImage};
        let img = DynamicImage::ImageRgb8(RgbImage::from_fn(w, h, |x, _| image::Rgb([(x % 256) as u8, 90, 160])));
        let mut out = Vec::new();
        JpegEncoder::new_with_quality(&mut out, 90).encode_image(&img).unwrap();
        out
    }

    #[test]
    fn macos_junk_and_non_images_are_skipped() {
        let page = jpeg(100, 150);
        let buf = zip_of(&[
            ("__MACOSX/vol/._p1.jpg", b"\0\x05\x16\x07AppleDouble"),
            ("vol/._p2.jpg", b"\0\x05\x16\x07AppleDouble"),
            ("vol/ComicInfo.xml", b"<ComicInfo/>"),
            ("vol/bad.jpg", b"not an image"),
            ("vol/p1.jpg", &page),
            ("vol/p2.jpg", &page),
        ]);
        let epub = cbz_to_epub(&buf, "测试").unwrap();
        let names: Vec<String> = crate::epubzip::read_entries(&epub).unwrap().into_iter().map(|e| e.name).collect();
        assert!(names.iter().any(|n| n.ends_with("images/p0002.jpg")) && !names.iter().any(|n| n.ends_with("images/p0003.jpg")), "{names:?}");
    }

    #[test]
    fn cbz_with_no_images_errors() {
        // 空 zip
        let mut buf = Vec::new();
        {
            let z = zip::ZipWriter::new(std::io::Cursor::new(&mut buf));
            z.finish().unwrap();
        }
        assert!(cbz_to_epub(&buf, "空").is_err());
    }
}
