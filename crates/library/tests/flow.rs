use bookconv::epub::{assemble, Book, BookMeta, Chapter};
use library::{Added, Built, Library};
use std::io::Write;

fn sample_epub(title: &str) -> Vec<u8> {
    let long = "正文段落，足够长的文字内容，确保标题页之后的内容超过门槛。".repeat(5);
    let mut book = Book {
        meta: BookMeta { book_id: "t".into(), title: title.into(), author: "作者".into(), language: "zh".into(), publisher: "".into(), cover: None, cover_ext: "jpg".into(), cover_media_type: "image/jpeg".into() },
        chapters: vec![Chapter { title: "第一章".into(), html_body: format!("<h1>第一章</h1><p>{long}</p><h2>第一节</h2><p>{long}</p>"), level: 1 }],
        resources: vec![],
        nav: vec![],
    };
    assemble(&mut book).unwrap()
}

fn jpeg(w: u32, h: u32) -> Vec<u8> {
    let mut out = Vec::new();
    image::codecs::jpeg::JpegEncoder::new(&mut out).encode_image(&image::RgbImage::from_pixel(w, h, image::Rgb([90, 90, 90]))).unwrap();
    out
}

#[test]
fn add_build_skip_remove() {
    let dir = tempfile::tempdir().unwrap();
    let lib = Library::open(dir.path().join("lib")).unwrap();
    let src = dir.path().join("风起.epub");
    std::fs::write(&src, sample_epub("风起")).unwrap();
    let Added::New(meta) = lib.add_file(&src).unwrap() else { panic!("应是新书") };
    assert_eq!(meta.title, "风起");
    assert_eq!(meta.authors, ["作者"]);
    assert!(matches!(lib.add_file(&src).unwrap(), Added::Existing(_)), "同一本书重复入库要认出来");
    assert_eq!(lib.list().len(), 1);

    let out = dir.path().join("out");
    for dev in ["rmpp-move", "kindle-pw12-sig", "ireader-ocean5-pro"] {
        let p = profile::get(dev).unwrap();
        let Built::Written { path, warnings } = lib.build(&meta, p, &out, false).unwrap() else { panic!("{dev} 第一次应生成") };
        assert!(warnings.is_empty(), "{warnings:?}");
        assert!(path.exists());
        assert_eq!(path.extension().unwrap(), if dev.starts_with("kindle") { "azw3" } else { "epub" });
        assert!(matches!(lib.build(&meta, p, &out, false).unwrap(), Built::UpToDate(_)), "{dev} 没变化应跳过");
        assert!(matches!(lib.build(&meta, p, &out, true).unwrap(), Built::Written { .. }), "--force 重建");
    }
    let status: Vec<(String, Option<bool>)> = lib.outputs(&meta).into_iter().map(|o| (o.device, o.fresh)).collect();
    assert_eq!(status, [("ireader-ocean5-pro".to_string(), Some(true)), ("kindle-pw12-sig".to_string(), Some(true)), ("rmpp-move".to_string(), Some(true))], "list 能看到三台设备的产物且都最新");
    // 产物指纹被改（模拟母版或规则变化）→ 过期
    let state_path = out.join("kindle-pw12-sig/.state.json");
    let st = std::fs::read_to_string(&state_path).unwrap().replace(&format!("|{}|", bookconv::optimize::OPTIMIZE_VERSION), "|0|");
    std::fs::write(&state_path, st).unwrap();
    assert_eq!(lib.outputs(&meta).into_iter().find(|o| o.device == "kindle-pw12-sig").unwrap().fresh, Some(false));
    let azw3 = std::fs::read(out.join("kindle-pw12-sig/风起.azw3")).unwrap();
    assert_eq!(&azw3[60..68], b"BOOKMOBI");
    // 母版原样保存
    assert_eq!(std::fs::read(dir.path().join(format!("lib/masters/{}/master.epub", meta.id))).unwrap(), sample_epub("风起"));

    lib.remove(&meta.id).unwrap();
    assert!(lib.list().is_empty());
    assert!(!out.join("kindle-pw12-sig/风起.azw3").exists(), "删书连产物一起删");
}

#[test]
fn cbz_becomes_comic_epub_master_and_drm_epub_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let lib = Library::open(dir.path().join("lib")).unwrap();
    let cbz = dir.path().join("漫画 - 01卷.cbz");
    {
        let mut z = zip::ZipWriter::new(std::fs::File::create(&cbz).unwrap());
        for i in [10, 2, 1] {
            z.start_file(format!("p{i}.jpg"), zip::write::SimpleFileOptions::default()).unwrap();
            z.write_all(&jpeg(600, 900)).unwrap();
        }
        z.finish().unwrap();
    }
    let Added::New(m) = lib.add_file(&cbz).unwrap() else { panic!() };
    assert_eq!(m.title, "漫画 - 01卷");
    let master = std::fs::read(dir.path().join(format!("lib/masters/{}/master.epub", m.id))).unwrap();
    let names: Vec<String> = bookconv::epubzip::read_entries(&master).unwrap().into_iter().map(|e| e.name).collect();
    assert!(names.iter().any(|n| n.ends_with("images/p0003.jpg")), "三页按自然序进母版: {names:?}");
    assert!(dir.path().join(format!("lib/masters/{}/source.cbz", m.id)).exists(), "原始文件留底");

    let drm = dir.path().join("加密.epub");
    {
        let mut z = zip::ZipWriter::new(std::fs::File::create(&drm).unwrap());
        z.start_file("mimetype", zip::write::SimpleFileOptions::default()).unwrap();
        z.write_all(b"application/epub+zip").unwrap();
        z.start_file("META-INF/rights.xml", zip::write::SimpleFileOptions::default()).unwrap();
        z.write_all(b"<rights/>").unwrap();
        z.finish().unwrap();
    }
    let err = match lib.add_file(&drm) { Err(e) => e, Ok(_) => panic!("DRM 书应拒收") };
    assert!(err.contains("DRM"), "{err}");
}

/// 目录所在文件系统支不支持写时复制克隆（tmpfs、ext4 不支持；btrfs、xfs 支持）。
fn reflink_supported(dir: &std::path::Path) -> bool {
    let (a, b) = (dir.join(".probe-a"), dir.join(".probe-b"));
    std::fs::write(&a, b"x").unwrap();
    let ok = reflink_copy::reflink(&a, &b).is_ok();
    let _ = (std::fs::remove_file(&a), std::fs::remove_file(&b));
    ok
}

#[test]
fn epub_master_shares_storage_and_dedupe_fixes_old_copies() {
    // 放在 target 目录下（通常和源码同一个文件系统），/tmp 常是不支持克隆的 tmpfs
    let dir = tempfile::tempdir_in(env!("CARGO_TARGET_TMPDIR")).unwrap();
    let can_share = reflink_supported(dir.path());
    let expected = if can_share { library::Storage::Reflink } else { library::Storage::Copy };
    let lib = Library::open(dir.path().join("lib")).unwrap();
    let books = dir.path().join("books");
    std::fs::create_dir_all(&books).unwrap();
    let src = books.join("风起.epub");
    std::fs::write(&src, sample_epub("风起")).unwrap();
    let Added::New(m) = lib.add_file(&src).unwrap() else { panic!() };
    let mdir = dir.path().join(format!("lib/masters/{}", m.id));
    assert!(!mdir.join("source.epub").exists(), "EPUB 母版就是原文件，不再多存 source 副本");
    assert_eq!(m.storage, expected, "能克隆就克隆；不能就复制，绝不用硬链接");
    assert_eq!(std::fs::read(mdir.join("master.epub")).unwrap(), sample_epub("风起"));
    // 改原文件不影响母版（母版不可变）
    std::fs::write(&src, b"changed").unwrap();
    assert_eq!(std::fs::read(mdir.join("master.epub")).unwrap(), sample_epub("风起"));

    // 模拟老版本入库：master 是独立副本、还多存了一份 source.epub
    let old_src = books.join("雨落.epub");
    std::fs::write(&old_src, sample_epub("雨落")).unwrap();
    let Added::New(old) = lib.add_file(&old_src).unwrap() else { panic!() };
    let odir = dir.path().join(format!("lib/masters/{}", old.id));
    std::fs::remove_file(odir.join("master.epub")).unwrap();
    std::fs::write(odir.join("master.epub"), sample_epub("雨落")).unwrap();
    std::fs::write(odir.join("source.epub"), sample_epub("雨落")).unwrap();
    let mut meta: serde_json::Value = serde_json::from_slice(&std::fs::read(odir.join("meta.json")).unwrap()).unwrap();
    meta["storage"] = "copy".into();
    std::fs::write(odir.join("meta.json"), meta.to_string()).unwrap();

    // 书库自己不算作外部目录：把书库也传进去不会让母版和自己"去重"
    let rep = lib.dedupe(&[books.clone(), dir.path().join("lib")]).unwrap();
    assert_eq!(rep.removed_sources, 1, "多余的 source 副本删掉");
    assert_eq!(rep.shared_files, usize::from(can_share), "只有老条目需要改；新条目已经共享；不支持克隆时保持原样");
    assert!(!odir.join("source.epub").exists());
    assert_eq!(lib.list().iter().find(|x| x.id == old.id).unwrap().storage, expected);
    // 原文件删掉后母版仍在
    std::fs::remove_file(&old_src).unwrap();
    assert_eq!(std::fs::read(odir.join("master.epub")).unwrap(), sample_epub("雨落"), "原文件删了，母版还在");
    let rep2 = lib.dedupe(&[books]).unwrap();
    assert_eq!((rep2.shared_files, rep2.removed_sources), (0, 0), "幂等");
}

#[test]
fn legacy_hardlinked_master_is_checked_and_unlinked() {
    let dir = tempfile::tempdir_in(env!("CARGO_TARGET_TMPDIR")).unwrap();
    let lib = Library::open(dir.path().join("lib")).unwrap();
    let src = dir.path().join("书.epub");
    std::fs::write(&src, sample_epub("书")).unwrap();
    let Added::New(m) = lib.add_file(&src).unwrap() else { panic!() };
    // 模拟早期版本：母版是原文件的硬链接
    let mdir = dir.path().join(format!("lib/masters/{}", m.id));
    std::fs::remove_file(mdir.join("master.epub")).unwrap();
    std::fs::hard_link(&src, mdir.join("master.epub")).unwrap();
    let mut meta: serde_json::Value = serde_json::from_slice(&std::fs::read(mdir.join("meta.json")).unwrap()).unwrap();
    meta["storage"] = "hardlink".into();
    std::fs::write(mdir.join("meta.json"), meta.to_string()).unwrap();
    let m = lib.list().remove(0);
    let kindle = lib.devices().get("kindle-pw12-sig").unwrap();
    let out = dir.path().join("out");
    // 原文件被就地改写 → 母版跟着变了，生成时要查出来，不能当作没变
    let mut changed = sample_epub("书");
    changed.extend_from_slice(b"\0");
    std::fs::write(&src, &changed).unwrap();
    let err = match lib.build(&m, kindle, &out, false) { Err(e) => e, Ok(_) => panic!("母版被改写应报错") };
    assert!(err.contains("不一样"), "{err}");
    // dedupe 把硬链接换成独立文件
    std::fs::write(&src, sample_epub("书")).unwrap();
    let rep = lib.dedupe(&[]).unwrap();
    assert_eq!(rep.unlinked, 1);
    std::fs::write(&src, b"changed again").unwrap();
    assert_eq!(std::fs::read(mdir.join("master.epub")).unwrap(), sample_epub("书"), "断开后原文件再改不影响母版");
}

#[test]
fn broken_meta_is_reported_removable_and_readdable() {
    let dir = tempfile::tempdir().unwrap();
    let lib = Library::open(dir.path().join("lib")).unwrap();
    let src = dir.path().join("书.epub");
    std::fs::write(&src, sample_epub("书")).unwrap();
    let Added::New(m) = lib.add_file(&src).unwrap() else { panic!() };
    let meta = dir.path().join(format!("lib/masters/{}/meta.json", m.id));
    std::fs::write(&meta, b"{\"id\": ").unwrap(); // 写到一半断电
    assert!(lib.list().is_empty());
    assert_eq!(lib.broken(), vec![m.id.clone()]);
    // 重新入库同一个文件：替换坏条目
    assert!(matches!(lib.add_file(&src).unwrap(), Added::New(_)));
    assert!(lib.broken().is_empty());
    std::fs::write(&meta, b"").unwrap();
    assert!(lib.remove(&m.id).is_ok(), "坏条目也能删");
    assert!(lib.remove("../lib").is_err());
}
