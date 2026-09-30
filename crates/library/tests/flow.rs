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
    assert!(!lib.is_comic(&meta).unwrap(), "文字书不算漫画");
    assert_eq!(lib.list().len(), 1);

    // add 进来的书（不在跟踪目录里）：产物在书库 output/<模式>/
    let out = std::path::absolute(lib.root()).unwrap().join("output");
    for (dev, ext) in [("ireader", "epub"), ("kindle", "azw3"), ("xochitl", "epub")] {
        let p = profile::get(dev).unwrap();
        let Built::Written { path, warnings } = lib.build(&meta, p, false).unwrap() else { panic!("{dev} 第一次应生成") };
        assert!(warnings.is_empty(), "{warnings:?}");
        assert_eq!(path, out.join(dev).join(format!("风起.{ext}")));
        assert!(matches!(lib.build(&meta, p, false).unwrap(), Built::UpToDate(_)), "{dev} 没变化应跳过");
        assert!(matches!(lib.build(&meta, p, true).unwrap(), Built::Written { .. }), "--force 重建");
    }
    let status: Vec<(String, Option<bool>)> = lib.outputs(&meta).into_iter().map(|o| (o.device, o.fresh)).collect();
    assert_eq!(status, [("ireader".to_string(), Some(true)), ("kindle".to_string(), Some(true)), ("xochitl".to_string(), Some(true))], "list 能看到三个模式的产物且都最新");
    // Kindle 的产物是 AZW3（PalmDB 头里的类型/创建者是 BOOKMOBI），能用读取器转回 EPUB
    let azw3 = std::fs::read(out.join("kindle/风起.azw3")).unwrap();
    assert_eq!(&azw3[60..68], b"BOOKMOBI");
    assert!(azw3::read::kf8::azw3_to_epub(&azw3).is_ok());
    // 产物指纹被改（模拟母版或规则变化）→ 过期
    let state_path = lib.root().join("output-state/xochitl.json");
    let st = std::fs::read_to_string(&state_path).unwrap().replace(&format!("|{}|", bookconv::optimize::OPTIMIZE_VERSION), "|0|");
    std::fs::write(&state_path, st).unwrap();
    assert_eq!(lib.outputs(&meta).into_iter().find(|o| o.device == "xochitl").unwrap().fresh, Some(false));
    let names: Vec<String> = bookconv::epubzip::read_entries(&std::fs::read(out.join("xochitl/风起.epub")).unwrap()).unwrap().into_iter().map(|e| e.name).collect();
    assert!(names.iter().any(|n| n == "META-INF/eink-optimized"), "{names:?}");
    // 书库只存索引，不复制原件
    let files: Vec<String> = std::fs::read_dir(dir.path().join(format!("lib/masters/{}", meta.id))).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
    assert_eq!(files, ["meta.json"]);

    lib.remove(&meta.id).unwrap();
    assert!(lib.list().is_empty());
    assert!(!out.join("xochitl/风起.epub").exists() && !out.join("ireader/风起.epub").exists() && !out.join("kindle/风起.azw3").exists(), "删书连产物一起删");
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
    assert!(lib.is_comic(&m).unwrap(), "CBZ 算漫画（booklib meta 跳过）");
    assert!(!dir.path().join(format!("lib/masters/{}/master.epub", m.id)).exists(), "转换结果不落书库");
    // 生成时当场转换：三页都在
    let Built::Written { path, .. } = lib.build(&m, profile::get("xochitl").unwrap(), false).unwrap() else { panic!() };
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
    let ireader = lib.devices().get("ireader").unwrap();

    // 只动了修改时间、内容没变：照常生成，记录更新
    let f = std::fs::File::options().write(true).open(&src).unwrap();
    f.set_modified(std::time::SystemTime::now() + std::time::Duration::from_secs(5)).unwrap();
    drop(f);
    assert_eq!(lib.original_state(&m), library::OriginalState::Touched);
    assert!(lib.build(&m, ireader, false).is_ok());
    let m = lib.list().remove(0);
    assert_eq!(lib.original_state(&m), library::OriginalState::Present);

    // 内容改了：不拿改过的内容冒充原书
    std::fs::write(&src, sample_epub("书（改）")).unwrap();
    let err = match lib.build(&m, ireader, true) { Err(e) => e, Ok(_) => panic!("原件改过应报错") };
    assert!(err.contains("改过"), "{err}");

    // 挪走：报原件不在；在新位置重新 add 就接上
    std::fs::write(&src, sample_epub("书")).unwrap();
    let moved = dir.path().join("新位置.epub");
    std::fs::rename(&src, &moved).unwrap();
    assert_eq!(lib.original_state(&m), library::OriginalState::Missing);
    assert!(lib.build(&m, ireader, true).unwrap_err().contains("不在"));
    assert!(matches!(lib.add_file(&moved).unwrap(), Added::Existing(_)));
    let m = lib.list().remove(0);
    assert!(m.source_path.ends_with("新位置.epub"));
    assert!(lib.build(&m, ireader, true).is_ok());
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
    assert!(lib.build(&now, lib.devices().get("ireader").unwrap(), false).is_ok());
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

fn events(lib: &Library, prune: bool) -> (library::SyncReport, Vec<String>) {
    let mut ev = Vec::new();
    let r = lib
        .sync(prune, |e| {
            ev.push(match e {
                library::SyncEvent::Added(p, _) => format!("added {}", p.display()),
                library::SyncEvent::Updated(p, _, _) => format!("updated {}", p.display()),
                library::SyncEvent::Missing(p, _, _) => format!("missing {}", p.display()),
                library::SyncEvent::Failed(p, e) => format!("failed {}: {e}", p.display()),
            })
        })
        .unwrap();
    (r, ev)
}

#[test]
fn non_utf8_file_names_are_reported_not_panicked() {
    use std::os::unix::ffi::OsStrExt;
    let dir = tempfile::tempdir().unwrap();
    let lib = Library::open(dir.path().join("lib")).unwrap();
    let books = dir.path().join("books");
    std::fs::create_dir_all(&books).unwrap();
    std::fs::write(books.join("好.epub"), sample_epub("好")).unwrap();
    let bad = books.join(std::ffi::OsStr::from_bytes(b"\xff\xfe.epub"));
    std::fs::write(&bad, sample_epub("坏名")).unwrap();
    assert!(lib.add_file(&bad).err().unwrap().contains("UTF-8"), "add 拒收");
    lib.track(&books).unwrap();
    let (r, ev) = events(&lib, false);
    assert_eq!((r.added, r.failed), (1, 1), "{ev:?}");
    assert!(ev.iter().any(|e| e.starts_with("failed") && e.contains("UTF-8")), "{ev:?}");
    let sources = std::fs::read_to_string(dir.path().join("lib/sources.json")).unwrap();
    assert!(sources.contains("好.epub") && !sources.contains("\\u"), "坏名字的不记下来：{sources}");
    // 改名后下次同步入库
    std::fs::rename(&bad, books.join("改好了.epub")).unwrap();
    let (r, _) = events(&lib, false);
    assert_eq!((r.added, r.failed), (1, 0));
}

#[test]
fn two_copies_deleting_the_indexed_one_repoints_to_the_other() {
    let dir = tempfile::tempdir().unwrap();
    let lib = Library::open(dir.path().join("lib")).unwrap();
    let books = dir.path().join("books");
    std::fs::create_dir_all(&books).unwrap();
    std::fs::write(books.join("a.epub"), sample_epub("同一本")).unwrap();
    std::fs::write(books.join("b.epub"), sample_epub("同一本")).unwrap();
    lib.track(&books).unwrap();
    let (r, _) = events(&lib, false);
    assert_eq!((r.added, r.unchanged), (1, 1));
    let m = lib.list().remove(0);
    assert!(m.source_path.ends_with("a.epub"));
    std::fs::remove_file(books.join("a.epub")).unwrap();
    let (r, ev) = events(&lib, true);
    assert_eq!((r.missing, r.pruned), (0, 0), "还有一份，不算不在：{ev:?}");
    let m = lib.list().remove(0);
    assert!(m.source_path.ends_with("b.epub"), "索引改记成还在的那份：{}", m.source_path);
    assert_eq!(lib.original_state(&m), library::OriginalState::Present);
    assert!(lib.build(&m, lib.devices().get("ireader").unwrap(), false).is_ok());
}

#[test]
fn failed_new_version_keeps_old_tracked_and_failed_removal_is_retried() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let lib = Library::open(dir.path().join("lib")).unwrap();
    let books = dir.path().join("books");
    std::fs::create_dir_all(&books).unwrap();
    let f = books.join("书.epub");
    std::fs::write(&f, sample_epub("第一版")).unwrap();
    lib.track(&books).unwrap();
    events(&lib, false);
    let v1 = lib.list().remove(0).id;

    // 新版本入库失败：报错，旧版本继续跟踪（不算原件不在，文件没变不重试）
    std::fs::write(&f, b"not a zip").unwrap();
    let (r, ev) = events(&lib, false);
    assert_eq!((r.failed, r.updated), (1, 0), "{ev:?}");
    assert_eq!(lib.list().into_iter().map(|m| m.id).collect::<Vec<_>>(), std::slice::from_ref(&v1));
    let sources = std::fs::read_to_string(dir.path().join("lib/sources.json")).unwrap();
    assert!(sources.contains(&v1), "旧 id 还在跟踪：{sources}");
    let (r, _) = events(&lib, true);
    assert_eq!((r.failed, r.missing, r.pruned), (0, 0, 0));

    // 换成能用的新版本：旧版本删除失败时报错、下次再删
    std::fs::write(&f, sample_epub("第二版")).unwrap();
    let old_dir = dir.path().join(format!("lib/masters/{v1}"));
    let mut ev = Vec::new();
    let r = lib
        .sync(false, |e| match e {
            library::SyncEvent::Updated(..) => std::fs::set_permissions(&old_dir, std::fs::Permissions::from_mode(0o555)).unwrap(),
            library::SyncEvent::Failed(_, e) => ev.push(e.to_string()),
            _ => {}
        })
        .unwrap();
    std::fs::set_permissions(&old_dir, std::fs::Permissions::from_mode(0o755)).unwrap();
    if r.failed == 0 {
        return; // 以 root 运行：只读目录挡不住删除，测不了后半段
    }
    assert_eq!((r.updated, r.failed), (1, 1), "{ev:?}");
    assert!(ev[0].contains("删旧版本"), "{ev:?}");
    assert!(old_dir.exists());
    let sources = std::fs::read_to_string(dir.path().join("lib/sources.json")).unwrap();
    assert!(sources.contains("stale") && sources.contains(&v1), "{sources}");
    let (r, ev) = events(&lib, false);
    assert_eq!(r.failed, 0, "{ev:?}");
    assert!(!old_dir.exists(), "下次 sync 删掉旧版本");
    assert_eq!(lib.list().len(), 1);
    assert!(!std::fs::read_to_string(dir.path().join("lib/sources.json")).unwrap().contains("stale"));
}

#[test]
fn watch_memo_reports_missing_once_and_track_rejects_nesting() {
    let dir = tempfile::tempdir().unwrap();
    let lib = Library::open(dir.path().join("lib")).unwrap();
    let books = dir.path().join("books");
    std::fs::create_dir_all(books.join("子")).unwrap();
    std::fs::write(books.join("甲.epub"), sample_epub("甲")).unwrap();
    lib.track(&books).unwrap();
    assert!(lib.track(&books.join("子")).unwrap_err().contains("包含"));
    let mut memo = library::SyncMemo::default();
    lib.sync_with(false, &mut memo, |_| {}).unwrap();
    let sources = dir.path().join("lib/sources.json");
    let t0 = std::fs::metadata(&sources).unwrap().modified().unwrap();
    std::thread::sleep(std::time::Duration::from_millis(20));
    lib.sync_with(false, &mut memo, |_| {}).unwrap();
    assert_eq!(std::fs::metadata(&sources).unwrap().modified().unwrap(), t0, "没变化不写 sources.json");
    std::fs::remove_file(books.join("甲.epub")).unwrap();
    assert_eq!(lib.sync_with(false, &mut memo, |_| {}).unwrap().missing, 1);
    assert_eq!(lib.sync_with(false, &mut memo, |_| {}).unwrap().missing, 0, "watch 下只报一次");
    assert_eq!(lib.sync(false, |_| {}).unwrap().missing, 1, "一次性 sync 每次都报");
}

#[test]
fn sync_failures_set_exit_code() {
    let dir = tempfile::tempdir().unwrap();
    let books = dir.path().join("books");
    std::fs::create_dir_all(&books).unwrap();
    std::fs::write(books.join("坏.epub"), b"not a zip").unwrap();
    let lib_dir = dir.path().join("lib");
    let run = |args: &[&str]| std::process::Command::new(env!("CARGO_BIN_EXE_booklib")).arg(format!("--library={}", lib_dir.display())).args(args).output().unwrap();
    assert!(run(&["track", books.to_str().unwrap()]).status.success());
    let o = run(&["sync"]);
    assert_eq!(o.status.code(), Some(2), "有书入库失败，退出码 2：{}", String::from_utf8_lossy(&o.stderr));
    let o = run(&["sync"]);
    assert_eq!(o.status.code(), Some(0), "文件没变不重试，不算失败：{}", String::from_utf8_lossy(&o.stderr));
}

#[test]
fn outputs_mirror_tracked_dirs_beside_them() {
    let dir = tempfile::tempdir().unwrap();
    let base = std::fs::canonicalize(dir.path()).unwrap();
    let lib = Library::open(base.join("lib")).unwrap();
    let books = base.join("ereader/books");
    std::fs::create_dir_all(books.join("haodoo")).unwrap();
    std::fs::write(books.join("haodoo/x.epub"), sample_epub("甲")).unwrap();
    std::fs::write(books.join("根.epub"), sample_epub("根")).unwrap();
    lib.track(&books).unwrap();
    lib.sync(false, |_| {}).unwrap();
    let find = |t: &str| lib.list().into_iter().find(|m| m.title == t).unwrap();
    let (ireader, xochitl) = (profile::get("ireader").unwrap(), profile::get("xochitl").unwrap());
    // 用原件路径挑书：文件＝那一本，目录＝下面所有的；路径可以是相对的、不规范的
    let titles = |sel: &str| lib.select(&[sel.to_string()]).into_iter().map(|m| m.title).collect::<Vec<_>>();
    assert_eq!(titles(books.join("haodoo/x.epub").to_str().unwrap()), ["甲"]);
    assert_eq!(titles(books.join("haodoo/../haodoo").to_str().unwrap()), ["甲"]);
    assert_eq!(titles(books.to_str().unwrap()).len(), 2);

    // 跟踪目录 D 里的书：D/../<模式>/<子目录>/<书名>.epub
    let Built::Written { path, .. } = lib.build(&find("甲"), ireader, false).unwrap() else { panic!() };
    assert_eq!(path, base.join("ereader/ireader/haodoo/甲.epub"));
    let Built::Written { path, .. } = lib.build(&find("甲"), xochitl, false).unwrap() else { panic!() };
    assert_eq!(path, base.join("ereader/xochitl/haodoo/甲.epub"));
    let Built::Written { path, .. } = lib.build(&find("根"), ireader, false).unwrap() else { panic!() };
    assert_eq!(path, base.join("ereader/ireader/根.epub"), "跟踪目录顶层的书放在模式目录顶层");
    // add 进来的书（不在跟踪目录里）：书库 output/<模式>/
    let single = base.join("单本.epub");
    std::fs::write(&single, sample_epub("单本")).unwrap();
    let Added::New(m) = lib.add_file(&single).unwrap() else { panic!() };
    let Built::Written { path, .. } = lib.build(&m, ireader, false).unwrap() else { panic!() };
    assert_eq!(path, base.join("lib/output/ireader/单本.epub"));

    // 原件挪到别的子目录：产物跟着挪（内容没变，不重新生成），旧文件删掉，变空的旧目录也删掉
    std::fs::create_dir_all(books.join("收藏")).unwrap();
    std::fs::rename(books.join("haodoo/x.epub"), books.join("收藏/x（改名）.epub")).unwrap();
    lib.sync(false, |_| {}).unwrap();
    let Built::Moved { from, to } = lib.build(&find("甲"), ireader, false).unwrap() else { panic!("应挪过去") };
    assert_eq!((from, to.clone()), (base.join("ereader/ireader/haodoo/甲.epub"), base.join("ereader/ireader/收藏/甲.epub")));
    assert!(to.is_file());
    assert!(!base.join("ereader/ireader/haodoo").exists(), "变空的旧目录删掉");
    assert!(base.join("ereader/ireader").is_dir(), "模式根目录不删");
    assert!(matches!(lib.build(&find("甲"), ireader, false).unwrap(), Built::UpToDate(_)));

    // 目录里不认识的文件：不覆盖（撞名加 id 后缀），也从来不删（目录因此不空，也不删目录）
    let other = base.join("ereader/xochitl/收藏");
    std::fs::create_dir_all(&other).unwrap();
    std::fs::write(other.join("甲.epub"), b"user's own file").unwrap();
    std::fs::write(base.join("ereader/xochitl/haodoo/笔记.txt"), b"mine").unwrap();
    let m = find("甲");
    let Built::Moved { to, .. } = lib.build(&m, xochitl, false).unwrap() else { panic!() };
    assert_eq!(to, other.join(format!("甲 [{}].epub", &m.id[..6])), "撞上不认识的同名文件：加 id 后缀");
    assert_eq!(std::fs::read(other.join("甲.epub")).unwrap(), b"user's own file");
    assert!(!base.join("ereader/xochitl/haodoo/甲.epub").exists(), "旧产物（记着的）删掉");
    assert_eq!(std::fs::read(base.join("ereader/xochitl/haodoo/笔记.txt")).unwrap(), b"mine", "不认识的文件不删");
    // 名字稳定：不认识的文件没了，也继续用带后缀的名字（KOReader 按文件名对阅读进度）
    std::fs::remove_file(other.join("甲.epub")).unwrap();
    assert!(matches!(lib.build(&m, xochitl, true).unwrap(), Built::Written { path, .. } if path == to));

    // 书名变了：改用新名字（内容没变，直接改名），旧文件不留
    let meta_path = base.join(format!("lib/masters/{}/meta.json", m.id));
    let mut v: serde_json::Value = serde_json::from_slice(&std::fs::read(&meta_path).unwrap()).unwrap();
    v["title"] = "甲（新书名）".into();
    std::fs::write(&meta_path, v.to_string()).unwrap();
    let Built::Moved { to: path, .. } = lib.build(&find("甲（新书名）"), ireader, false).unwrap() else { panic!("换成新名字") };
    assert_eq!(path, base.join("ereader/ireader/收藏/甲（新书名）.epub"));
    assert!(!base.join("ereader/ireader/收藏/甲.epub").exists());

    // remove：只删记着的产物
    std::fs::write(base.join("ereader/ireader/收藏/别的.epub"), b"x").unwrap();
    lib.remove(&m.id).unwrap();
    assert!(!path.exists() && !to.exists());
    assert!(base.join("ereader/ireader/收藏/别的.epub").exists(), "不认识的文件不删");
}

#[test]
fn tracked_dir_named_like_a_mode_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let base = std::fs::canonicalize(dir.path()).unwrap();
    let lib = Library::open(base.join("lib")).unwrap();
    let books = base.join("ireader");
    std::fs::create_dir_all(&books).unwrap();
    std::fs::write(books.join("a.epub"), sample_epub("书")).unwrap();
    lib.track(&books).unwrap();
    lib.sync(false, |_| {}).unwrap();
    let m = lib.list().remove(0);
    let e = lib.build(&m, profile::get("ireader").unwrap(), false).unwrap_err();
    assert!(e.contains("跟踪的目录"), "{e}");
    assert!(lib.build(&m, profile::get("xochitl").unwrap(), false).is_ok());
}

#[test]
fn legacy_entries_of_dropped_formats_are_kept_and_skipped() {
    let dir = tempfile::tempdir().unwrap();
    let base = std::fs::canonicalize(dir.path()).unwrap();
    let lib_dir = base.join("lib");
    let books = base.join("books");
    std::fs::create_dir_all(&books).unwrap();
    let mobi = books.join("旧书.mobi");
    std::fs::write(&mobi, b"BOOKMOBI").unwrap();
    std::fs::write(books.join("新书.epub"), sample_epub("新书")).unwrap();
    // 早期版本入库的 PDF/MOBI 条目：meta.json 里还有已经删掉的字段（pdf_text_layer）
    let lib = Library::open(&lib_dir).unwrap();
    lib.track(&books).unwrap();
    let (size, mtime) = {
        let md = std::fs::metadata(&mobi).unwrap();
        (md.len(), md.modified().unwrap().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos() as u64)
    };
    let id = "aaaaaaaaaaaa";
    std::fs::create_dir_all(lib_dir.join(format!("masters/{id}"))).unwrap();
    let meta = serde_json::json!({"id": id, "title": "旧书", "authors": [], "source": "旧书.mobi", "source_format": "mobi", "added": 1,
        "source_path": mobi.to_str().unwrap(), "source_sha256": format!("{id}0000"), "source_size": size, "source_mtime_ns": mtime, "pdf_text_layer": true});
    std::fs::write(lib_dir.join(format!("masters/{id}/meta.json")), meta.to_string()).unwrap();
    let sources = serde_json::json!({"dirs": [books.to_str().unwrap()], "files": {mobi.to_str().unwrap(): {"size": size, "mtime_ns": mtime, "id": id}}});
    std::fs::write(lib_dir.join("sources.json"), sources.to_string()).unwrap();

    let old = lib.list().into_iter().find(|m| m.id == id).expect("读得出来，不崩");
    assert!(!old.supported());
    assert!(lib.build(&old, profile::get("ireader").unwrap(), false).unwrap_err().contains("不再支持"));
    assert!(lib.add_file(&base.join("另一本.pdf")).is_err());
    std::fs::write(base.join("另一本.pdf"), b"%PDF-1.4").unwrap();
    assert_eq!(lib.add_file(&base.join("另一本.pdf")).err().unwrap(), "只支持 EPUB 和 CBZ");
    // sync：还在的旧格式文件不算"原件不在"，--prune 也不删
    let r = lib.sync(true, |_| {}).unwrap();
    assert_eq!((r.added, r.missing, r.pruned), (1, 0, 0));
    assert!(lib.list().iter().any(|m| m.id == id));
    assert!(std::fs::read_to_string(lib_dir.join("sources.json")).unwrap().contains("旧书.mobi"));

    // 命令行：list 标出来；build 跳过（一行提示），其余照常生成，退出码 0
    let run = |args: &[&str]| std::process::Command::new(env!("CARGO_BIN_EXE_booklib")).arg(format!("--library={}", lib_dir.display())).args(args).output().unwrap();
    let o = run(&["list"]);
    assert!(String::from_utf8_lossy(&o.stdout).contains("不再支持的格式"), "{}", String::from_utf8_lossy(&o.stdout));
    let o = run(&["build"]);
    let stdout = String::from_utf8_lossy(&o.stdout);
    assert!(o.status.success(), "{stdout}{}", String::from_utf8_lossy(&o.stderr));
    assert_eq!(stdout.matches("跳过").count(), 1, "每本只提示一次：{stdout}");
    assert!(base.join("ireader/新书.epub").is_file() && base.join("xochitl/新书.epub").is_file(), "不写 --device = 全部模式：{stdout}");
    assert!(!run(&["build", "--out=x"]).status.success(), "--out 已删");
}

#[test]
fn interrupted_move_is_finished_next_time() {
    // 换位置时先登记新位置（指纹留空）、旧位置记进 old：模拟"新产物写好了、记录还没补完"时被打断
    let dir = tempfile::tempdir().unwrap();
    let base = std::fs::canonicalize(dir.path()).unwrap();
    let lib = Library::open(base.join("lib")).unwrap();
    let src = base.join("书.epub");
    std::fs::write(&src, sample_epub("书")).unwrap();
    let Added::New(m) = lib.add_file(&src).unwrap() else { panic!() };
    let dev = profile::get("ireader").unwrap();
    let Built::Written { path, .. } = lib.build(&m, dev, false).unwrap() else { panic!() };
    let stale = base.join("lib/output/ireader/旧名字.epub");
    std::fs::write(&stale, b"old output").unwrap();
    let sp = base.join("lib/output-state/ireader.json");
    let mut st: serde_json::Value = serde_json::from_slice(&std::fs::read(&sp).unwrap()).unwrap();
    let e = &mut st["books"][&m.id];
    e["fingerprint"] = "".into();
    e["old"] = serde_json::json!([{"path": stale, "root": base.join("lib/output/ireader")}]);
    std::fs::write(&sp, st.to_string()).unwrap();
    assert_eq!(lib.outputs(&m)[0].fresh, Some(false), "没完成的算过期");
    let Built::Written { path: again, .. } = lib.build(&m, dev, false).unwrap() else { panic!("没完成的要重建") };
    assert_eq!(again, path, "新位置上的文件认得是本书的，不改名");
    assert!(!stale.exists(), "旧位置补删");
    assert!(!std::fs::read_to_string(&sp).unwrap().contains("\"old\""));
}
