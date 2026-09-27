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

    let out = lib.root().join("output");
    for dev in ["rmpp-move", "kindle-pw12-sig", "ireader-ocean5-pro"] {
        let p = profile::get(dev).unwrap();
        let Built::Written { path, warnings } = lib.build(&meta, p, false).unwrap() else { panic!("{dev} 第一次应生成") };
        assert!(warnings.is_empty(), "{warnings:?}");
        assert!(path.exists());
        assert_eq!(path.extension().unwrap(), if dev.starts_with("kindle") { "azw3" } else { "epub" });
        assert!(matches!(lib.build(&meta, p, false).unwrap(), Built::UpToDate(_)), "{dev} 没变化应跳过");
        assert!(matches!(lib.build(&meta, p, true).unwrap(), Built::Written { .. }), "--force 重建");
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
    // 书库只存索引，不复制原件
    let files: Vec<String> = std::fs::read_dir(dir.path().join(format!("lib/masters/{}", meta.id))).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
    assert_eq!(files, ["meta.json"]);

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
    assert!(!dir.path().join(format!("lib/masters/{}/master.epub", m.id)).exists(), "转换结果不落书库");
    // 生成时当场转换：三页都在
    let Built::Written { path, .. } = lib.build(&m, profile::get("rmpp-move").unwrap(), false).unwrap() else { panic!() };
    let names: Vec<String> = bookconv::epubzip::read_entries(&std::fs::read(path).unwrap()).unwrap().into_iter().map(|e| e.name).collect();
    assert_eq!(names.iter().filter(|n| n.contains("images/p")).count(), 3, "{names:?}");

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

#[test]
fn original_is_checked_before_build_and_can_move() {
    let dir = tempfile::tempdir().unwrap();
    let lib = Library::open(dir.path().join("lib")).unwrap();
    let src = dir.path().join("书.epub");
    std::fs::write(&src, sample_epub("书")).unwrap();
    let Added::New(m) = lib.add_file(&src).unwrap() else { panic!() };
    let kindle = lib.devices().get("kindle-pw12-sig").unwrap();

    // 只动了修改时间、内容没变：照常生成，记录更新
    let f = std::fs::File::options().write(true).open(&src).unwrap();
    f.set_modified(std::time::SystemTime::now() + std::time::Duration::from_secs(5)).unwrap();
    drop(f);
    assert_eq!(lib.original_state(&m), library::OriginalState::Touched);
    assert!(lib.build(&m, kindle, false).is_ok());
    let m = lib.list().remove(0);
    assert_eq!(lib.original_state(&m), library::OriginalState::Present);

    // 内容改了：不拿改过的内容冒充原书
    std::fs::write(&src, sample_epub("书（改）")).unwrap();
    let err = match lib.build(&m, kindle, true) { Err(e) => e, Ok(_) => panic!("原件改过应报错") };
    assert!(err.contains("改过"), "{err}");

    // 挪走：报原件不在；在新位置重新 add 就接上
    std::fs::write(&src, sample_epub("书")).unwrap();
    let moved = dir.path().join("新位置.epub");
    std::fs::rename(&src, &moved).unwrap();
    assert_eq!(lib.original_state(&m), library::OriginalState::Missing);
    assert!(lib.build(&m, kindle, true).unwrap_err().contains("不在"));
    assert!(matches!(lib.add_file(&moved).unwrap(), Added::Existing(_)));
    let m = lib.list().remove(0);
    assert!(m.source_path.ends_with("新位置.epub"));
    assert!(lib.build(&m, kindle, true).is_ok());
}

#[test]
fn dedupe_migrates_old_entries_to_index_only() {
    let dir = tempfile::tempdir().unwrap();
    let lib = Library::open(dir.path().join("lib")).unwrap();
    let books = dir.path().join("books");
    std::fs::create_dir_all(&books).unwrap();
    // 模拟早期版本的条目：书库里存着 master.epub 和 source.epub，meta 没有原件哈希，记着的原件路径已失效
    let src = books.join("旧书.epub");
    std::fs::write(&src, sample_epub("旧书")).unwrap();
    let Added::New(m) = lib.add_file(&src).unwrap() else { panic!() };
    let edir = dir.path().join(format!("lib/masters/{}", m.id));
    std::fs::write(edir.join("master.epub"), sample_epub("旧书")).unwrap();
    std::fs::write(edir.join("source.epub"), sample_epub("旧书")).unwrap();
    let mut meta: serde_json::Value = serde_json::from_slice(&std::fs::read(edir.join("meta.json")).unwrap()).unwrap();
    meta["master"] = "master.epub".into();
    meta["storage"] = "copy".into();
    meta["source_sha256"] = "".into();
    meta["source_path"] = "/不存在/旧书.epub".into();
    std::fs::write(edir.join("meta.json"), meta.to_string()).unwrap();
    // 另一本早期条目，原件找不到
    let gone = serde_json::json!({"id":"0123456789ab","title":"孤本","authors":[],"source":"孤本.epub","source_format":"epub","master":"master.epub","added":1});
    std::fs::create_dir_all(dir.path().join("lib/masters/0123456789ab")).unwrap();
    std::fs::write(dir.path().join("lib/masters/0123456789ab/meta.json"), gone.to_string()).unwrap();
    std::fs::write(dir.path().join("lib/masters/0123456789ab/master.epub"), sample_epub("孤本")).unwrap();

    let old = lib.list().into_iter().find(|x| x.id == m.id).unwrap();
    assert_eq!(old.source(), library::Source::Stored, "迁移前还读书库里的副本");
    let rep = lib.dedupe(std::slice::from_ref(&books)).unwrap();
    assert_eq!(rep.migrated, 1);
    assert_eq!(rep.kept.len(), 1, "找不到原件的保留副本");
    assert!(!edir.join("master.epub").exists() && !edir.join("source.epub").exists());
    let now = lib.list().into_iter().find(|x| x.id == m.id).unwrap();
    assert_eq!(now.source(), library::Source::Original);
    assert!(now.source_path.ends_with("旧书.epub"));
    assert!(lib.build(&now, lib.devices().get("ireader-ocean5-pro").unwrap(), false).is_ok());
    assert!(dir.path().join("lib/masters/0123456789ab/master.epub").exists());
    assert_eq!(lib.dedupe(&[books]).unwrap().migrated, 0, "幂等");
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

#[test]
fn track_and_sync_mirror_a_directory() {
    let dir = tempfile::tempdir().unwrap();
    let lib = Library::open(dir.path().join("lib")).unwrap();
    let books = dir.path().join("books");
    std::fs::create_dir_all(books.join("子目录")).unwrap();
    std::fs::write(books.join("甲.epub"), sample_epub("甲")).unwrap();
    std::fs::write(books.join("子目录/乙.epub"), sample_epub("乙")).unwrap();
    std::fs::write(books.join("说明.txt"), b"not a book").unwrap();
    std::fs::write(books.join(".隐藏.epub"), sample_epub("隐藏")).unwrap();
    assert_eq!(library::book_files(&books).len(), 2, "递归、只要能入库的、跳过隐藏文件");

    assert!(lib.track(&books).unwrap());
    assert!(!lib.track(&books).unwrap(), "重复跟踪");
    assert!(lib.track(&dir.path().join("lib")).is_err(), "不能跟踪书库自己");
    let r = lib.sync(false, |_| {}).unwrap();
    assert_eq!((r.added, r.updated, r.missing), (2, 0, 0));
    assert_eq!(lib.list().len(), 2);

    // 没变：不重读
    let r = lib.sync(false, |_| {}).unwrap();
    assert_eq!((r.added, r.unchanged), (0, 2));

    // 改名移动：认得出，不重复入库，也不算"原件不在"
    std::fs::rename(books.join("子目录/乙.epub"), books.join("乙（改名）.epub")).unwrap();
    let r = lib.sync(false, |_| {}).unwrap();
    assert_eq!((r.added, r.missing), (0, 0));
    assert_eq!(lib.list().len(), 2);

    // 内容变了：换成新版本，旧版本删掉
    std::fs::write(books.join("甲.epub"), sample_epub("甲（修订版）")).unwrap();
    let r = lib.sync(false, |_| {}).unwrap();
    assert_eq!(r.updated, 1);
    let titles: Vec<String> = lib.list().into_iter().map(|m| m.title).collect();
    assert!(titles.contains(&"甲（修订版）".to_string()) && !titles.contains(&"甲".to_string()), "{titles:?}");

    // 入库失败的记住，不反复重试
    std::fs::write(books.join("坏.epub"), b"not a zip").unwrap();
    assert_eq!(lib.sync(false, |_| {}).unwrap().failed, 1);
    assert_eq!(lib.sync(false, |_| {}).unwrap().failed, 0, "文件没变就不再重试");

    // 原件删了：缺省只报告；--prune 才删
    std::fs::remove_file(books.join("乙（改名）.epub")).unwrap();
    let r = lib.sync(false, |_| {}).unwrap();
    assert_eq!((r.missing, r.pruned), (1, 0));
    assert_eq!(lib.list().len(), 2);
    let r = lib.sync(true, |_| {}).unwrap();
    assert_eq!((r.missing, r.pruned), (1, 1));
    assert_eq!(lib.list().len(), 1);
    assert_eq!(lib.sync(false, |_| {}).unwrap().missing, 0, "删过就不再报告");

    assert!(lib.untrack(&books).unwrap());
    assert!(lib.tracked().is_empty());
    assert_eq!(lib.list().len(), 1, "不跟踪了，已入库的书保留");
}

#[test]
fn out_copies_built_file_and_tracks_it() {
    let dir = tempfile::tempdir().unwrap();
    let lib = Library::open(dir.path().join("lib")).unwrap();
    let src = dir.path().join("书.epub");
    std::fs::write(&src, sample_epub("书")).unwrap();
    let Added::New(m) = lib.add_file(&src).unwrap() else { panic!() };
    let kindle = lib.devices().get("kindle-pw12-sig").unwrap();
    let dest = dir.path().join("U盘 目录/documents");
    assert!(lib.deliver(&m, kindle, &dest, false).is_err(), "没生成过不能拷");
    lib.build(&m, kindle, false).unwrap();
    let library::Delivered::Copied(p) = lib.deliver(&m, kindle, &dest, false).unwrap() else { panic!("第一次要拷") };
    assert_eq!(p, dest.join("书.azw3"));
    assert_eq!(std::fs::read(&p).unwrap(), std::fs::read(lib.root().join("output/kindle-pw12-sig/书.azw3")).unwrap());
    assert!(matches!(lib.deliver(&m, kindle, &dest, false).unwrap(), library::Delivered::Unchanged(_)), "没变不重拷");
    assert_eq!(lib.deliveries(&m).into_iter().map(|d| d.fresh).collect::<Vec<_>>(), [Some(true)]);
    std::fs::remove_file(&p).unwrap();
    assert_eq!(lib.deliveries(&m)[0].fresh, None, "目标被删了");
    assert!(matches!(lib.deliver(&m, kindle, &dest, false).unwrap(), library::Delivered::Copied(_)), "目标不在就重拷");
    lib.remove(&m.id).unwrap();
    assert!(!p.exists(), "remove 连拷出去的一起删");
}
