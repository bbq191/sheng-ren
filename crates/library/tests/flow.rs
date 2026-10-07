mod common;

use common::{booklib, jpeg, sample_epub, sample_epub_with};
use library::{Added, Built, Library, Prune};
use std::io::Write;

#[test]
fn add_build_skip_remove() {
    let dir = tempfile::tempdir().unwrap();
    let mv = common::FakeMove::start();
    let lib_dir = dir.path().join("lib");
    let lib = common::open_with(&lib_dir, Some(&mv));
    let src = dir.path().join("风起.epub");
    std::fs::write(&src, sample_epub("风起")).unwrap();
    let Added::New(meta) = lib.add_file(&src).unwrap() else { panic!("应是新书") };
    assert_eq!(meta.title, "风起");
    assert_eq!(meta.authors, ["作者"]);
    assert!(matches!(lib.add_file(&src).unwrap(), Added::Existing(_)), "同一本书重复入库要认出来");
    assert!(!lib.is_comic(&meta).unwrap(), "文字书不算漫画");
    assert_eq!(lib.list().len(), 1);

    // 产物直接送设备：Kindle、掌阅放存储根目录的 documents/（add 进来的书在顶层），Move 经导入接口加入 xochitl 书库根
    for (dev, want) in [
        ("ireader", common::documents(&lib_dir, "ireader").join("风起.epub")),
        ("kindle", common::documents(&lib_dir, "kindle").join("风起.kfx")),
        ("xochitl", std::path::PathBuf::from("xochitl:风起.epub")),
    ] {
        let p = profile::get(dev).unwrap();
        let Built::Written { path, warnings } = lib.build(&meta, p, false).unwrap() else { panic!("{dev} 第一次应生成") };
        assert!(warnings.is_empty(), "{warnings:?}");
        assert_eq!(path, want);
        assert!(matches!(lib.build(&meta, p, false).unwrap(), Built::UpToDate(_)), "{dev} 没变化应跳过");
        assert!(matches!(lib.build(&meta, p, true).unwrap(), Built::Same { .. }), "{dev} --force 重建：和设备上的一样，不再传");
    }
    assert!(!std::path::absolute(lib.root()).unwrap().join("output").exists(), "电脑上不留产物");
    // Move：--force 重建出来一样的不再传；内容变了（这里改记录里的哈希来模拟）是原地替换同一本（uuid 不变），不另加一本
    assert_eq!(mv.docs.lock().unwrap().values().next().unwrap().replaced, 0);
    let xp = lib.root().join("output-state/xochitl.json");
    let mut st: serde_json::Value = serde_json::from_slice(&std::fs::read(&xp).unwrap()).unwrap();
    st["books"][&meta.id]["sha"] = "旧的".into();
    std::fs::write(&xp, st.to_string()).unwrap();
    let lib = common::open_with(&lib_dir, Some(&mv));
    assert!(matches!(lib.build(&meta, profile::get("xochitl").unwrap(), true).unwrap(), Built::Written { .. }));
    let docs = mv.docs.lock().unwrap().clone();
    assert_eq!(docs.len(), 1);
    let doc = docs.values().next().unwrap();
    assert_eq!((doc.name.as_str(), doc.folder.as_str(), doc.replaced), ("风起", "", 1));
    let names: Vec<String> = bookconv::epubzip::read_entries(&doc.bytes).unwrap().into_iter().map(|e| e.name).collect();
    assert!(names.iter().any(|n| n == "META-INF/eink-optimized"), "{names:?}");
    let status: Vec<(String, Option<bool>)> = lib.outputs(&meta).into_iter().map(|o| (o.device, o.fresh)).collect();
    assert_eq!(status, [("ireader".to_string(), Some(true)), ("kindle".to_string(), Some(true)), ("xochitl".to_string(), Some(true))], "list 能看到三个模式的产物且都最新");
    // Kindle 文字书的产物是 KFX（`CONT` 容器，读得回来）
    let kfx = std::fs::read(common::documents(&lib_dir, "kindle").join("风起.kfx")).unwrap();
    assert_eq!(&kfx[..4], b"CONT");
    assert!(kfx::Container::parse(&kfx).is_ok());
    // 产物指纹被改（模拟母版或规则变化）→ 过期
    let state_path = lib.root().join("output-state/xochitl.json");
    let st = std::fs::read_to_string(&state_path).unwrap().replace(&format!("|{}|", bookconv::optimize::OPTIMIZE_VERSION), "|0|");
    std::fs::write(&state_path, st).unwrap();
    assert_eq!(lib.outputs(&meta).into_iter().find(|o| o.device == "xochitl").unwrap().fresh, Some(false));
    // Move 上的那本被用户删了：重新加入一本
    mv.docs.lock().unwrap().values_mut().for_each(|d| d.deleted = true);
    assert!(matches!(lib.build(&meta, profile::get("xochitl").unwrap(), false).unwrap(), Built::Written { .. }));
    assert_eq!(mv.live(), [(String::new(), "风起".to_string())]);
    // 书库只存索引，不复制原件
    let files: Vec<String> = std::fs::read_dir(dir.path().join(format!("lib/masters/{}", meta.id))).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
    assert_eq!(files, ["meta.json"]);

    lib.remove(&meta.id).unwrap();
    assert!(lib.list().is_empty());
    assert!(!common::documents(&lib_dir, "ireader").join("风起.epub").exists() && !common::documents(&lib_dir, "kindle").join("风起.kfx").exists(), "删书连设备上的产物一起删");
    assert!(mv.live().is_empty(), "Move 上的进回收站");
}

/// 设备没接上：生成报没接上；删书时删不了的记着，接上后删（flush_removed）。手工拷上去、逐字节相同的同名文件认领，不另起名字。
#[test]
fn absent_device_defers_removal_and_identical_copy_is_adopted() {
    let dir = tempfile::tempdir().unwrap();
    let lib_dir = dir.path().join("lib");
    let lib = common::open(&lib_dir);
    let ireader = profile::get("ireader").unwrap();
    let src = dir.path().join("甲.epub");
    std::fs::write(&src, sample_epub("甲")).unwrap();
    let Added::New(m) = lib.add_file(&src).unwrap() else { panic!() };
    let Built::Written { path, .. } = lib.build(&m, ireader, false).unwrap() else { panic!() };
    let bytes = std::fs::read(&path).unwrap();
    // 记录丢了、设备上那份当成"手工拷上去的"：重建出来逐字节相同，认领原名
    std::fs::remove_dir_all(lib.root().join("output-state")).unwrap();
    let Built::Same { path: again, .. } = lib.build(&m, ireader, false).unwrap() else { panic!("重建出来一样，不再拷") };
    assert_eq!(again, path, "逐字节相同的同名文件认领");
    // 拔掉：没接上
    let storage = common::dev_dir(&lib_dir).join("ireader/Internal Storage");
    let unplugged = dir.path().join("拔下来的存储");
    std::fs::rename(&storage, &unplugged).unwrap();
    lib.refresh_devices();
    assert!(lib.device_status(ireader).is_err());
    assert!(lib.build(&m, ireader, false).unwrap_err().contains("没接上"));
    lib.remove(&m.id).unwrap();
    // 插回去：删掉
    std::fs::rename(&unplugged, &storage).unwrap();
    lib.refresh_devices();
    assert!(path.exists());
    assert_eq!(lib.flush_removed(ireader).unwrap(), 1);
    assert!(!path.exists(), "接上后删掉");
    assert!(!bytes.is_empty());
}

/// KFX 的容器 id 由书库的书 id 决定（不是 OPF 唯一标识符）：两本 OPF 标识符相同的书容器 id 不同；书里 4 处一致。
#[test]
fn kfx_container_id_comes_from_book_id() {
    let dir = tempfile::tempdir().unwrap();
    let lib = common::open(&dir.path().join("lib"));
    let kindle = profile::get("kindle").unwrap();
    let mut ids = Vec::new();
    for title in ["甲书", "乙书"] {
        let src = dir.path().join(format!("{title}.epub"));
        std::fs::write(&src, sample_epub(title)).unwrap(); // 两本的 OPF 唯一标识符都是 "t"
        let Added::New(meta) = lib.add_file(&src).unwrap() else { panic!() };
        let Built::Written { path, .. } = lib.build(&meta, kindle, false).unwrap() else { panic!() };
        let bytes = std::fs::read(&path).unwrap();
        let id = kfx::Container::parse(&bytes).unwrap().container_id().unwrap().to_string();
        assert_eq!(id, kfx::write::container_id(u64::from_str_radix(&meta.id, 16).unwrap()), "容器 id 由书 id 派生");
        // 容器信息、kfxgen 的 kfxgen_acr、$419 清单、$490 的 asset_id
        assert_eq!(bytes.windows(id.len()).filter(|w| *w == id.as_bytes()).count(), 4, "容器 id 在书里 4 处一致");
        // 重建不变（Kindle 进度靠它）
        let Built::Same { path: again, .. } = lib.build(&meta, kindle, true).unwrap() else { panic!("重建逐字节相同，不再传") };
        assert_eq!(std::fs::read(again).unwrap(), bytes, "同一本书重建逐字节相同");
        ids.push(id);
    }
    assert_ne!(ids[0], ids[1], "OPF 标识符相同的两本书容器 id 不同");
}

#[test]
fn cbz_becomes_comic_epub_master_and_drm_epub_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let lib = common::open(&dir.path().join("lib"));
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
    assert!(lib.is_comic(&m).unwrap(), "CBZ 算漫画（booklib meta --fetch 跳过）");
    assert!(!dir.path().join(format!("lib/masters/{}/master.epub", m.id)).exists(), "转换结果不落书库");
    // 生成时当场转换：三页都在
    let Built::Written { path, .. } = lib.build(&m, profile::get("ireader").unwrap(), false).unwrap() else { panic!() };
    let names: Vec<String> = bookconv::epubzip::read_entries(&std::fs::read(path).unwrap()).unwrap().into_iter().map(|e| e.name).collect();
    assert_eq!(names.iter().filter(|n| n.contains("images/p")).count(), 3, "{names:?}");
    // kindle：漫画也出 KFX（固定版式）
    let Built::Written { path, .. } = lib.build(&m, profile::get("kindle").unwrap(), false).unwrap() else { panic!() };
    assert_eq!(path.extension().and_then(|e| e.to_str()), Some("kfx"));
    let meta = format!("{:?}", kfx::Container::parse(&std::fs::read(path).unwrap()).unwrap().entities.iter().filter(|e| e.ty == 490).map(|e| e.value().unwrap().clone()).collect::<Vec<_>>());
    assert!(meta.contains("yj_fixed_layout"), "{meta}");

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
    let lib = common::open(&dir.path().join("lib"));
    let src = dir.path().join("书.epub");
    std::fs::write(&src, sample_epub("书")).unwrap();
    let _lock = lib.lock().unwrap(); // build 持锁时才把核对结果写回书库
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
    let lib = common::open(&dir.path().join("lib"));
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
    let lib = common::open(&dir.path().join("lib"));
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
    let lib = common::open(&dir.path().join("lib"));
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
    let r = lib.sync(Prune::Keep, |_| {}).unwrap();
    assert_eq!((r.added, r.updated, r.missing), (2, 0, 0));
    assert_eq!(lib.list().len(), 2);

    // 没变：不重读
    let r = lib.sync(Prune::Keep, |_| {}).unwrap();
    assert_eq!((r.added, r.unchanged), (0, 2));

    // 改名移动：认得出，不重复入库，也不算"原件不在"
    std::fs::rename(books.join("子目录/乙.epub"), books.join("乙（改名）.epub")).unwrap();
    let r = lib.sync(Prune::Keep, |_| {}).unwrap();
    assert_eq!((r.added, r.missing), (0, 0));
    assert_eq!(lib.list().len(), 2);

    // 内容变了：换成新版本，旧版本删掉
    std::fs::write(books.join("甲.epub"), sample_epub("甲（修订版）")).unwrap();
    let r = lib.sync(Prune::Keep, |_| {}).unwrap();
    assert_eq!(r.updated, 1);
    let titles: Vec<String> = lib.list().into_iter().map(|m| m.title).collect();
    assert!(titles.contains(&"甲（修订版）".to_string()) && !titles.contains(&"甲".to_string()), "{titles:?}");

    // 入库失败的记住，不反复重试
    std::fs::write(books.join("坏.epub"), b"not a zip").unwrap();
    assert_eq!(lib.sync(Prune::Keep, |_| {}).unwrap().failed, 1);
    assert_eq!(lib.sync(Prune::Keep, |_| {}).unwrap().failed, 0, "文件没变就不再重试");

    // 原件删了：--keep 只报告；缺省连同产物删掉
    std::fs::remove_file(books.join("乙（改名）.epub")).unwrap();
    let r = lib.sync(Prune::Keep, |_| {}).unwrap();
    assert_eq!((r.missing, r.pruned), (1, 0));
    assert_eq!(lib.list().len(), 2);
    let r = lib.sync(Prune::Auto, |_| {}).unwrap();
    assert_eq!((r.missing, r.pruned), (1, 1));
    assert_eq!(lib.list().len(), 1);
    assert_eq!(lib.sync(Prune::Keep, |_| {}).unwrap().missing, 0, "删过就不再报告");

    assert!(lib.untrack(&books).unwrap());
    assert!(lib.tracked().is_empty());
    assert_eq!(lib.list().len(), 1, "不跟踪了，已入库的书保留");
}

fn events(lib: &Library, prune: Prune) -> (library::SyncReport, Vec<String>) {
    let mut ev = Vec::new();
    let r = lib
        .sync(prune, |e| {
            ev.push(match e {
                library::SyncEvent::Added(p, _) => format!("added {}", p.display()),
                library::SyncEvent::Updated(p, _, _) => format!("updated {}", p.display()),
                library::SyncEvent::Missing(p, _, removed) => format!("missing {} {removed}", p.display()),
                library::SyncEvent::Failed(p, e) => format!("failed {}: {e}", p.display()),
                library::SyncEvent::Unreadable(p, e) => format!("unreadable {}: {e}", p.display()),
            })
        })
        .unwrap();
    (r, ev)
}

#[test]
fn non_utf8_file_names_are_reported_not_panicked() {
    use std::os::unix::ffi::OsStrExt;
    let dir = tempfile::tempdir().unwrap();
    let lib = common::open(&dir.path().join("lib"));
    let books = dir.path().join("books");
    std::fs::create_dir_all(&books).unwrap();
    std::fs::write(books.join("好.epub"), sample_epub("好")).unwrap();
    let bad = books.join(std::ffi::OsStr::from_bytes(b"\xff\xfe.epub"));
    std::fs::write(&bad, sample_epub("坏名")).unwrap();
    assert!(lib.add_file(&bad).err().unwrap().contains("UTF-8"), "add 拒收");
    lib.track(&books).unwrap();
    let (r, ev) = events(&lib, Prune::Keep);
    assert_eq!((r.added, r.failed), (1, 1), "{ev:?}");
    assert!(ev.iter().any(|e| e.starts_with("failed") && e.contains("UTF-8")), "{ev:?}");
    let sources = std::fs::read_to_string(dir.path().join("lib/sources.json")).unwrap();
    assert!(sources.contains("好.epub") && !sources.contains("\\u"), "坏名字的不记下来：{sources}");
    // 改名后下次同步入库
    std::fs::rename(&bad, books.join("改好了.epub")).unwrap();
    let (r, _) = events(&lib, Prune::Keep);
    assert_eq!((r.added, r.failed), (1, 0));
}

#[test]
fn two_copies_deleting_the_indexed_one_repoints_to_the_other() {
    let dir = tempfile::tempdir().unwrap();
    let lib = common::open(&dir.path().join("lib"));
    let books = dir.path().join("books");
    std::fs::create_dir_all(&books).unwrap();
    std::fs::write(books.join("a.epub"), sample_epub("同一本")).unwrap();
    std::fs::write(books.join("b.epub"), sample_epub("同一本")).unwrap();
    lib.track(&books).unwrap();
    let (r, _) = events(&lib, Prune::Keep);
    assert_eq!((r.added, r.unchanged), (1, 1));
    let m = lib.list().remove(0);
    assert!(m.source_path.ends_with("a.epub"));
    std::fs::remove_file(books.join("a.epub")).unwrap();
    let (r, ev) = events(&lib, Prune::Auto);
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
    let lib = common::open(&dir.path().join("lib"));
    let books = dir.path().join("books");
    std::fs::create_dir_all(&books).unwrap();
    let f = books.join("书.epub");
    std::fs::write(&f, sample_epub("第一版")).unwrap();
    lib.track(&books).unwrap();
    events(&lib, Prune::Keep);
    let v1 = lib.list().remove(0).id;

    // 新版本入库失败：报错，旧版本继续跟踪（不算原件不在，文件没变不重试）
    std::fs::write(&f, b"not a zip").unwrap();
    let (r, ev) = events(&lib, Prune::Keep);
    assert_eq!((r.failed, r.updated), (1, 0), "{ev:?}");
    assert_eq!(lib.list().into_iter().map(|m| m.id).collect::<Vec<_>>(), std::slice::from_ref(&v1));
    let sources = std::fs::read_to_string(dir.path().join("lib/sources.json")).unwrap();
    assert!(sources.contains(&v1), "旧 id 还在跟踪：{sources}");
    let (r, _) = events(&lib, Prune::Auto);
    assert_eq!((r.failed, r.missing, r.pruned), (0, 0, 0));

    // 换成能用的新版本：旧版本删除失败时报错、下次再删
    std::fs::write(&f, sample_epub("第二版")).unwrap();
    let old_dir = dir.path().join(format!("lib/masters/{v1}"));
    // 删条目是先把条目目录改名再删：`masters/` 只读时改不了名，删不掉
    let masters = dir.path().join("lib/masters");
    let mut ev = Vec::new();
    let r = lib
        .sync(Prune::Keep, |e| match e {
            library::SyncEvent::Updated(..) => std::fs::set_permissions(&masters, std::fs::Permissions::from_mode(0o555)).unwrap(),
            library::SyncEvent::Failed(_, e) => ev.push(e.to_string()),
            _ => {}
        })
        .unwrap();
    std::fs::set_permissions(&masters, std::fs::Permissions::from_mode(0o755)).unwrap();
    if r.failed == 0 {
        return; // 以 root 运行：只读目录挡不住删除，测不了后半段
    }
    assert_eq!((r.updated, r.failed), (1, 1), "{ev:?}");
    assert!(ev[0].contains("删旧版本"), "{ev:?}");
    assert!(old_dir.exists());
    let sources = std::fs::read_to_string(dir.path().join("lib/sources.json")).unwrap();
    assert!(sources.contains("stale") && sources.contains(&v1), "{sources}");
    let (r, ev) = events(&lib, Prune::Keep);
    assert_eq!(r.failed, 0, "{ev:?}");
    assert!(!old_dir.exists(), "下次 sync 删掉旧版本");
    assert_eq!(lib.list().len(), 1);
    assert!(!std::fs::read_to_string(dir.path().join("lib/sources.json")).unwrap().contains("stale"));
}

#[test]
fn watch_memo_reports_missing_once_and_track_rejects_nesting() {
    let dir = tempfile::tempdir().unwrap();
    let lib = common::open(&dir.path().join("lib"));
    let books = dir.path().join("books");
    std::fs::create_dir_all(books.join("子")).unwrap();
    std::fs::write(books.join("甲.epub"), sample_epub("甲")).unwrap();
    lib.track(&books).unwrap();
    assert!(lib.track(&books.join("子")).unwrap_err().contains("包含"));
    let mut memo = library::SyncMemo::default();
    lib.sync_with(Prune::Keep, &mut memo, |_| {}).unwrap();
    let sources = dir.path().join("lib/sources.json");
    let t0 = std::fs::metadata(&sources).unwrap().modified().unwrap();
    std::thread::sleep(std::time::Duration::from_millis(20));
    lib.sync_with(Prune::Keep, &mut memo, |_| {}).unwrap();
    assert_eq!(std::fs::metadata(&sources).unwrap().modified().unwrap(), t0, "没变化不写 sources.json");
    std::fs::remove_file(books.join("甲.epub")).unwrap();
    assert_eq!(lib.sync_with(Prune::Keep, &mut memo, |_| {}).unwrap().missing, 1);
    assert_eq!(lib.sync_with(Prune::Keep, &mut memo, |_| {}).unwrap().missing, 0, "watch 下只报一次");
    assert_eq!(lib.sync(Prune::Keep, |_| {}).unwrap().missing, 1, "一次性 sync 每次都报");
}

#[test]
fn sync_failures_set_exit_code() {
    let dir = tempfile::tempdir().unwrap();
    let books = dir.path().join("books");
    std::fs::create_dir_all(&books).unwrap();
    std::fs::write(books.join("坏.epub"), b"not a zip").unwrap();
    let lib_dir = dir.path().join("lib");
    let run = |args: &[&str]| booklib(Some(&lib_dir), args);
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
    let mv = common::FakeMove::start();
    let lib = common::open_with(&base.join("lib"), Some(&mv));
    let (ir, kd) = (common::documents(&base.join("lib"), "ireader"), common::documents(&base.join("lib"), "kindle"));
    let books = base.join("ereader/books");
    std::fs::create_dir_all(books.join("haodoo")).unwrap();
    std::fs::write(books.join("haodoo/x.epub"), sample_epub("甲")).unwrap();
    std::fs::write(books.join("根.epub"), sample_epub("根")).unwrap();
    lib.track(&books).unwrap();
    lib.sync(Prune::Keep, |_| {}).unwrap();
    let find = |t: &str| lib.list().into_iter().find(|m| m.title == t).unwrap();
    let (ireader, kindle, xochitl) = (profile::get("ireader").unwrap(), profile::get("kindle").unwrap(), profile::get("xochitl").unwrap());
    // 用原件路径挑书：文件＝那一本，目录＝下面所有的；路径可以是相对的、不规范的
    let titles = |sel: &str| lib.select(&[sel.to_string()]).into_iter().map(|m| m.title).collect::<Vec<_>>();
    assert_eq!(titles(books.join("haodoo/x.epub").to_str().unwrap()), ["甲"]);
    assert_eq!(titles(books.join("haodoo/../haodoo").to_str().unwrap()), ["甲"]);
    assert_eq!(titles(books.to_str().unwrap()).len(), 2);

    // 跟踪目录 D 里的书：设备上 documents/<子目录>/<书名>.<扩展名>；Move 上放进同名文件夹
    let Built::Written { path, .. } = lib.build(&find("甲"), ireader, false).unwrap() else { panic!() };
    assert_eq!(path, ir.join("haodoo/甲.epub"));
    let Built::Written { path, .. } = lib.build(&find("甲"), kindle, false).unwrap() else { panic!() };
    assert_eq!(path, kd.join("haodoo/甲.kfx"));
    let Built::Written { path, .. } = lib.build(&find("甲"), xochitl, false).unwrap() else { panic!() };
    assert_eq!(path, std::path::PathBuf::from("xochitl:haodoo/甲.epub"));
    assert_eq!(mv.live(), [("haodoo".to_string(), "甲".to_string())]);
    let Built::Written { path, .. } = lib.build(&find("根"), ireader, false).unwrap() else { panic!() };
    assert_eq!(path, ir.join("根.epub"), "跟踪目录顶层的书放在 documents 顶层");
    // add 进来的书（不在跟踪目录里）：documents 顶层
    let single = base.join("单本.epub");
    std::fs::write(&single, sample_epub("单本")).unwrap();
    let Added::New(m) = lib.add_file(&single).unwrap() else { panic!() };
    let Built::Written { path, .. } = lib.build(&m, ireader, false).unwrap() else { panic!() };
    assert_eq!(path, ir.join("单本.epub"));

    // 原件挪到别的子目录：产物跟着挪（内容没变，不重新生成），旧文件删掉，变空的旧目录也删掉
    std::fs::create_dir_all(books.join("收藏")).unwrap();
    std::fs::rename(books.join("haodoo/x.epub"), books.join("收藏/x（改名）.epub")).unwrap();
    lib.sync(Prune::Keep, |_| {}).unwrap();
    let Built::Moved { from, to } = lib.build(&find("甲"), ireader, false).unwrap() else { panic!("应挪过去") };
    assert_eq!((from, to.clone()), (ir.join("haodoo/甲.epub"), ir.join("收藏/甲.epub")));
    assert!(to.is_file());
    assert!(!ir.join("haodoo").exists(), "变空的旧目录删掉");
    assert!(ir.is_dir(), "documents 不删");
    assert!(matches!(lib.build(&find("甲"), ireader, false).unwrap(), Built::UpToDate(_)));
    // Move：文件夹变了，加入新文件夹里的一本，旧的进回收站
    assert!(matches!(lib.build(&find("甲"), xochitl, false).unwrap(), Built::Written { .. }));
    assert_eq!(mv.live(), [("收藏".to_string(), "甲".to_string())]);

    // 目录里不认识的文件：不覆盖（撞名加 id 后缀），也从来不删（目录因此不空，也不删目录）
    let other = kd.join("收藏");
    std::fs::create_dir_all(&other).unwrap();
    std::fs::write(other.join("甲.kfx"), b"user's own file").unwrap();
    std::fs::write(kd.join("haodoo/笔记.txt"), b"mine").unwrap();
    let m = find("甲");
    let Built::Moved { to, .. } = lib.build(&m, kindle, false).unwrap() else { panic!() };
    assert_eq!(to, other.join(format!("甲 [{}].kfx", &m.id[..6])), "撞上不认识的同名文件：加 id 后缀");
    assert_eq!(std::fs::read(other.join("甲.kfx")).unwrap(), b"user's own file");
    assert!(!kd.join("haodoo/甲.kfx").exists(), "旧产物（记着的）删掉");
    assert_eq!(std::fs::read(kd.join("haodoo/笔记.txt")).unwrap(), b"mine", "不认识的文件不删");
    // 名字稳定：不认识的文件没了，也继续用带后缀的名字（产物改名，设备上就成了另一本书）
    std::fs::remove_file(other.join("甲.kfx")).unwrap();
    assert!(matches!(lib.build(&m, kindle, true).unwrap(), Built::Same { path, .. } if path == to));

    // 书名变了：改用新名字（内容没变，直接改名），旧文件不留
    let meta_path = base.join(format!("lib/masters/{}/meta.json", m.id));
    let mut v: serde_json::Value = serde_json::from_slice(&std::fs::read(&meta_path).unwrap()).unwrap();
    v["title"] = "甲（新书名）".into();
    std::fs::write(&meta_path, v.to_string()).unwrap();
    let Built::Moved { to: path, .. } = lib.build(&find("甲（新书名）"), ireader, false).unwrap() else { panic!("换成新名字") };
    assert_eq!(path, ir.join("收藏/甲（新书名）.epub"));
    assert!(!ir.join("收藏/甲.epub").exists());

    // remove：只删记着的产物
    std::fs::write(ir.join("收藏/别的.epub"), b"x").unwrap();
    lib.remove(&m.id).unwrap();
    assert!(!path.exists() && !to.exists());
    assert!(ir.join("收藏/别的.epub").exists(), "不认识的文件不删");
    assert!(mv.live().is_empty(), "Move 上的进回收站");
}

#[test]
fn tracked_dir_named_like_a_mode_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let base = std::fs::canonicalize(dir.path()).unwrap();
    let lib = common::open(&common::with_pc_profile(&base.join("lib")));
    let books = base.join("pc");
    std::fs::create_dir_all(&books).unwrap();
    std::fs::write(books.join("a.epub"), sample_epub("书")).unwrap();
    // track 时就拒绝：产物目录就是它自己
    assert!(lib.track(&books).unwrap_err().contains("产物目录"));
    // 早期版本已经跟踪了的：build 时拒绝这个模式，别的模式照常
    std::fs::write(base.join("lib/sources.json"), serde_json::json!({"dirs": [books]}).to_string()).unwrap();
    lib.sync(Prune::Keep, |_| {}).unwrap();
    let m = lib.list().remove(0);
    let e = lib.build(&m, lib.devices().get("pc").unwrap(), false).unwrap_err();
    assert!(e.contains("跟踪的目录"), "{e}");
    assert!(lib.build(&m, profile::get("ireader").unwrap(), false).is_ok(), "直接送设备的模式不受影响");
}

#[test]
fn tracked_dir_inside_another_dirs_output_root_is_refused() {
    // 产物放电脑上的模式 pc：跟踪 e/books 和 e/pc/sub，books 的 pc 产物（e/pc/…）包住了 e/pc/sub，产物会被当新书入库、层层嵌套
    let dir = tempfile::tempdir().unwrap();
    let base = std::fs::canonicalize(dir.path()).unwrap();
    let lib = common::open(&common::with_pc_profile(&base.join("lib")));
    let books = base.join("e/books");
    let sub = base.join("e/pc/sub");
    std::fs::create_dir_all(books.join("sub")).unwrap();
    std::fs::create_dir_all(&sub).unwrap();
    std::fs::write(books.join("sub/a.epub"), sample_epub("甲")).unwrap();
    lib.track(&books).unwrap();
    assert!(lib.track(&sub).unwrap_err().contains("产物目录"), "后跟踪的在前一个的产物目录里");
    // 顺序反过来也拒绝
    let lib2 = common::open(&common::with_pc_profile(&base.join("lib2")));
    lib2.track(&sub).unwrap();
    assert!(lib2.track(&books).unwrap_err().contains("产物目录"), "后跟踪的产物目录包住前一个");
    // 早期版本已经这样跟踪了：build 拒绝，不往跟踪目录里写产物
    std::fs::write(base.join("lib/sources.json"), serde_json::json!({"dirs": [books, sub]}).to_string()).unwrap();
    lib.sync(Prune::Keep, |_| {}).unwrap();
    let m = lib.list().remove(0);
    let e = lib.build(&m, lib.devices().get("pc").unwrap(), false).unwrap_err();
    assert!(e.contains("互相包含"), "{e}");
    assert!(!base.join("e/pc/sub/甲.epub").exists());
    assert_eq!(lib.sync(Prune::Keep, |_| {}).unwrap().added, 0);
}

#[test]
fn unreadable_subdir_keeps_its_books() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let base = std::fs::canonicalize(dir.path()).unwrap();
    let lib_dir = base.join("lib");
    let lib = common::open(&lib_dir);
    let books = base.join("books");
    let locked = books.join("锁");
    std::fs::create_dir_all(&locked).unwrap();
    std::fs::write(books.join("甲.epub"), sample_epub("甲")).unwrap();
    std::fs::write(locked.join("乙.epub"), sample_epub("乙")).unwrap();
    lib.track(&books).unwrap();
    lib.sync(Prune::Keep, |_| {}).unwrap();
    let ireader = profile::get("ireader").unwrap();
    for m in lib.list() {
        lib.build(&m, ireader, false).unwrap();
    }
    let out = common::documents(&lib_dir, "ireader").join("锁/乙.epub");
    assert!(out.is_file());
    // 000：读不了；444：列得出名字但取不到属性。两种都不能当成书没了
    for mode in [0o000, 0o444] {
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(mode)).unwrap();
        if std::fs::read_dir(&locked).ok().and_then(|mut d| d.next()).is_some_and(|e| e.is_ok_and(|e| e.metadata().is_ok())) {
            std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o755)).unwrap();
            return; // 以 root 运行：权限挡不住，测不了
        }
        let (r, ev) = events(&lib, Prune::Auto);
        assert_eq!((r.unreadable, r.missing, r.pruned), (1, 0, 0), "{mode:o} {ev:?}");
        assert_eq!(lib.list().len(), 2, "{mode:o}：读不了的目录里的书原样保留");
        assert!(std::fs::read_to_string(lib_dir.join("sources.json")).unwrap().contains("乙.epub"));
        // 命令行：明确报出来，退出码 2
        let o = booklib(Some(&lib_dir), &["sync", "--no-build"]);
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert_eq!(o.status.code(), Some(2), "{}", String::from_utf8_lossy(&o.stderr));
        assert!(String::from_utf8_lossy(&o.stderr).contains("读不了"), "{}", String::from_utf8_lossy(&o.stderr));
        assert!(out.is_file(), "产物也不删");
    }
    let (r, _) = events(&lib, Prune::Auto);
    assert_eq!((r.unreadable, r.missing, r.added, r.failed), (0, 0, 0, 0), "恢复后照常");
    assert_eq!(lib.list().len(), 2);
}

#[test]
fn changed_added_original_is_caught_by_build_and_re_add_replaces_it() {
    let dir = tempfile::tempdir().unwrap();
    let base = std::fs::canonicalize(dir.path()).unwrap();
    let lib = common::open(&base.join("lib"));
    let src = base.join("书.epub");
    std::fs::write(&src, sample_epub("第一版")).unwrap();
    let Added::New(m) = lib.add_file(&src).unwrap() else { panic!() };
    let dev = profile::get("ireader").unwrap();
    let Built::Written { path, .. } = lib.build(&m, dev, false).unwrap() else { panic!() };
    assert!(matches!(lib.build(&m, dev, false).unwrap(), Built::UpToDate(_)));
    // 原件被改：build 不再说"已是最新"，停下来
    std::fs::write(&src, sample_epub("第二版")).unwrap();
    let e = lib.build(&m, dev, false).unwrap_err();
    assert!(e.contains("改过"), "{e}");
    // 再 add：换成新版本（和 sync 一样），旧条目连同产物删掉，不留重复条目
    let Added::Replaced(m2, old) = lib.add_file(&src).unwrap() else { panic!("应换成新版本") };
    assert_eq!((m2.title.as_str(), old.as_str()), ("第二版", "第一版"));
    assert_eq!(lib.list().into_iter().map(|m| m.id).collect::<Vec<_>>(), std::slice::from_ref(&m2.id));
    assert!(!path.exists(), "旧版本的产物删掉");
    let Built::Written { path: p2, .. } = lib.build(&m2, dev, false).unwrap() else { panic!() };
    assert!(p2.ends_with("第二版.epub"));
    assert!(matches!(lib.add_file(&src).unwrap(), Added::Existing(_)));

    // 跟踪目录里的文件被 add 换了新版本：sources.json 也记成新版本，下次 sync 不再报"更新"
    let books = base.join("books");
    std::fs::create_dir_all(&books).unwrap();
    let t = books.join("跟踪.epub");
    std::fs::write(&t, sample_epub("跟踪一")).unwrap();
    lib.track(&books).unwrap();
    lib.sync(Prune::Keep, |_| {}).unwrap();
    std::fs::write(&t, sample_epub("跟踪二")).unwrap();
    assert!(matches!(lib.add_file(&t).unwrap(), Added::Replaced(..)));
    let (r, ev) = events(&lib, Prune::Keep);
    assert_eq!((r.added, r.updated, r.unchanged), (0, 0, 1), "{ev:?}");
    assert_eq!(lib.list().len(), 2);
}

#[test]
fn undeletable_old_output_does_not_rewrite_the_record_every_time() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let base = std::fs::canonicalize(dir.path()).unwrap();
    let lib = common::open(&base.join("lib"));
    let src = base.join("书.epub");
    std::fs::write(&src, sample_epub("书")).unwrap();
    let Added::New(m) = lib.add_file(&src).unwrap() else { panic!() };
    let dev = profile::get("ireader").unwrap();
    lib.build(&m, dev, false).unwrap();
    // 旧位置在只读目录里：删不掉
    let ro = base.join("只读");
    std::fs::create_dir_all(&ro).unwrap();
    let stale = ro.join("旧.epub");
    std::fs::write(&stale, b"old").unwrap();
    std::fs::set_permissions(&ro, std::fs::Permissions::from_mode(0o555)).unwrap();
    let sp = base.join("lib/output-state/ireader.json");
    let mut st: serde_json::Value = serde_json::from_slice(&std::fs::read(&sp).unwrap()).unwrap();
    st["books"][&m.id]["old"] = serde_json::json!([{"path": stale, "root": ro}]);
    std::fs::write(&sp, st.to_string()).unwrap();
    let ino = |p: &std::path::Path| std::os::unix::fs::MetadataExt::ino(&std::fs::metadata(p).unwrap());
    let before = ino(&sp);
    assert!(matches!(lib.build(&m, dev, false).unwrap(), Built::UpToDate(_)));
    assert!(matches!(lib.build(&m, dev, false).unwrap(), Built::UpToDate(_)));
    let root_run = !stale.exists();
    std::fs::set_permissions(&ro, std::fs::Permissions::from_mode(0o755)).unwrap();
    if root_run {
        return; // 以 root 运行：只读目录挡不住删除
    }
    assert_eq!(ino(&sp), before, "待删列表没变：不重写生成记录");
    assert!(std::fs::read_to_string(&sp).unwrap().contains("旧.epub"), "删不掉的继续记着");
}

#[test]
fn fetched_description_already_in_the_book_does_not_change_the_fingerprint() {
    let dir = tempfile::tempdir().unwrap();
    let base = std::fs::canonicalize(dir.path()).unwrap();
    let lib = common::open(&base.join("lib"));
    // 书里自己有简介、没有标签
    let plain = base.join("plain.epub");
    std::fs::write(&plain, sample_epub("书")).unwrap();
    let src = base.join("书.epub");
    let edits = bookconv::opfmeta::Edits { set: vec![(bookconv::opfmeta::DcField::Description, vec!["原书简介".into()])], ..Default::default() };
    bookconv::opfmeta::edit_epub(&plain, &src, &edits).unwrap();
    let Added::New(m) = lib.add_file(&src).unwrap() else { panic!() };
    let dev = profile::get("ireader").unwrap();
    let fp0 = lib.fingerprint(&m, dev).unwrap();
    let set_info = |v: serde_json::Value| {
        let mp = base.join(format!("lib/masters/{}/meta.json", m.id));
        let mut j: serde_json::Value = serde_json::from_slice(&std::fs::read(&mp).unwrap()).unwrap();
        j["info"] = v;
        std::fs::write(&mp, j.to_string()).unwrap();
        lib.list().remove(0)
    };
    // 找来的简介：书里已有，不会补进去 → 指纹不变（不白重建）
    let m1 = set_info(serde_json::json!({"source": "t", "description": "找来的简介"}));
    assert_eq!(lib.fingerprint(&m1, dev).unwrap(), fp0);
    let m2 = set_info(serde_json::json!({"source": "t", "description": "换了一份简介"}));
    assert_eq!(lib.fingerprint(&m2, dev).unwrap(), fp0);
    // 标签书里没有，会补 → 指纹变；简介再变也不影响
    let m3 = set_info(serde_json::json!({"source": "t", "description": "找来的简介", "subjects": ["推理"]}));
    let fp3 = lib.fingerprint(&m3, dev).unwrap();
    assert_ne!(fp3, fp0);
    let m4 = set_info(serde_json::json!({"source": "t", "description": "又一份", "subjects": ["推理"]}));
    assert_eq!(lib.fingerprint(&m4, dev).unwrap(), fp3);
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
    let lib = common::open(&lib_dir);
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
    // sync：还在的旧格式文件不算"原件不在"，不删
    let r = lib.sync(Prune::Auto, |_| {}).unwrap();
    assert_eq!((r.added, r.missing, r.pruned), (1, 0, 0));
    assert!(lib.list().iter().any(|m| m.id == id));
    assert!(std::fs::read_to_string(lib_dir.join("sources.json")).unwrap().contains("旧书.mobi"));

    // 命令行：list 标出来；sync 生成时跳过（一行提示），其余照常生成，退出码 0
    let run = |args: &[&str]| booklib(Some(&lib_dir), args);
    let o = run(&["list"]);
    assert!(String::from_utf8_lossy(&o.stdout).contains("不再支持的格式"), "{}", String::from_utf8_lossy(&o.stdout));
    let o = run(&["sync"]);
    let stdout = String::from_utf8_lossy(&o.stdout);
    assert!(o.status.success(), "{stdout}{}", String::from_utf8_lossy(&o.stderr));
    assert_eq!(stdout.matches("跳过").count(), 1, "每本只提示一次：{stdout}");
    assert!(common::documents(&lib_dir, "ireader").join("新书.epub").is_file() && common::documents(&lib_dir, "kindle").join("新书.kfx").is_file(), "不写 --device = 全部模式：{stdout}");
    assert!(stdout.contains("[xochitl] 没接上"), "没接上的设备报一行、不算失败：{stdout}");
}

#[test]
fn interrupted_move_is_finished_next_time() {
    // 换位置时先登记新位置（指纹留空）、旧位置记进 old：模拟"新产物写好了、记录还没补完"时被打断
    let dir = tempfile::tempdir().unwrap();
    let base = std::fs::canonicalize(dir.path()).unwrap();
    let lib = common::open(&base.join("lib"));
    let src = base.join("书.epub");
    std::fs::write(&src, sample_epub("书")).unwrap();
    let Added::New(m) = lib.add_file(&src).unwrap() else { panic!() };
    let dev = profile::get("ireader").unwrap();
    let Built::Written { path, .. } = lib.build(&m, dev, false).unwrap() else { panic!() };
    let docs = common::documents(&base.join("lib"), "ireader");
    let stale = docs.join("旧名字.epub");
    std::fs::write(&stale, b"old output").unwrap();
    let sp = base.join("lib/output-state/ireader.json");
    let mut st: serde_json::Value = serde_json::from_slice(&std::fs::read(&sp).unwrap()).unwrap();
    let e = &mut st["books"][&m.id];
    e["fingerprint"] = "".into();
    e["old"] = serde_json::json!([{"path": stale, "root": docs}]);
    std::fs::write(&sp, st.to_string()).unwrap();
    assert_eq!(lib.outputs(&m)[0].fresh, Some(false), "没完成的算过期");
    let Built::Same { path: again, .. } = lib.build(&m, dev, false).unwrap() else { panic!("没完成的要重建（和设备上的一样，不再拷）") };
    assert_eq!(again, path, "新位置上的文件认得是本书的，不改名");
    assert!(!stale.exists(), "旧位置补删");
    assert!(!std::fs::read_to_string(&sp).unwrap().contains("\"old\""));
}

#[test]
fn dedupe_never_takes_the_librarys_own_copy_for_the_original() {
    // 早期条目：书库里存着副本 master.epub（和原件内容相同，id 就是它的哈希），记着的原件已经不在了。
    // 给 dedupe 的目录正好包含书库：以前把副本当成"找到的原件"，改记成它再删掉副本——书就没了
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("lib");
    let lib = common::open(&root);
    let bytes = sample_epub("孤本");
    let sha: String = {
        use sha2::Digest;
        sha2::Sha256::digest(&bytes).iter().map(|b| format!("{b:02x}")).collect()
    };
    let id = &sha[..12];
    let edir = root.join("masters").join(id);
    std::fs::create_dir_all(&edir).unwrap();
    std::fs::write(edir.join("master.epub"), &bytes).unwrap();
    let meta = serde_json::json!({"id":id,"title":"孤本","authors":[],"source":"孤本.epub","source_format":"epub","master":"master.epub","added":1,"source_path":"/不存在/孤本.epub"});
    std::fs::write(edir.join("meta.json"), meta.to_string()).unwrap();
    let rep = lib.dedupe(&[dir.path().to_path_buf()]).unwrap();
    assert_eq!((rep.migrated, rep.kept.len()), (0, 1), "书库里的副本不算原件");
    assert!(edir.join("master.epub").exists(), "副本还在");
    assert_eq!(lib.list()[0].source(), library::Source::Stored);
}

#[test]
fn removed_entry_leaves_nothing_behind() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("lib");
    let lib = common::open(&root);
    let src = dir.path().join("书.epub");
    std::fs::write(&src, sample_epub("书")).unwrap();
    let _l = lib.lock().unwrap();
    let id = lib.add_file(&src).unwrap().meta().id.clone();
    lib.remove(&id).unwrap();
    let left: Vec<_> = std::fs::read_dir(root.join("masters")).unwrap().flatten().map(|e| e.file_name()).collect();
    assert!(left.is_empty(), "改名成临时目录再删，删完不留东西：{left:?}");
    assert!(lib.remove(&id).is_err());
}

#[test]
fn comic_fingerprint_follows_the_readable_area_the_optimizer_uses() {
    // 自定义模式：漫画另配格式（azw3），两种格式的阅读范围不同，漫画阅读范围单写。优化器的阅读范围是 formats 第一个（epub）的，
    // 它变了漫画产物会变——指纹也要变（以前指纹取的是漫画产物格式 azw3 的阅读范围，epub 的改了不过期）
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("lib");
    std::fs::create_dir_all(root.join("profiles")).unwrap();
    let profile = |w: u32| {
        format!("name = \"t\"\nppi = 300\ncolor = false\nformats = [\"epub\", \"azw3\"]\ncomic_format = \"azw3\"\nnotes = \"jump\"\n[screen]\nwidth = 1000\nheight = 1500\n[readable.epub]\nwidth = {w}\nheight = 1400\n[readable.azw3]\nwidth = 900\nheight = 1400\n[comic_readable]\nwidth = 1000\nheight = 1500\n")
    };
    let cbz = dir.path().join("漫画.cbz");
    {
        let mut z = zip::ZipWriter::new(std::fs::File::create(&cbz).unwrap());
        for i in 0..3 {
            z.start_file(format!("{i:02}.jpg"), zip::write::SimpleFileOptions::default()).unwrap();
            z.write_all(&jpeg(400, 600)).unwrap();
        }
        z.finish().unwrap();
    }
    let fp = |w: u32| {
        std::fs::write(root.join("profiles/t.toml"), profile(w)).unwrap();
        let lib = common::open(&root);
        let m = lib.add_file(&cbz).unwrap().meta().clone();
        lib.fingerprint(&m, lib.devices().get("t").unwrap()).unwrap()
    };
    assert_ne!(fp(800), fp(700));
}

/// 跑 booklib，要求退出码 0，返回 stdout。
#[track_caller]
fn ok(lib: &std::path::Path, args: &[&str]) -> String {
    let o = booklib(Some(lib), args);
    assert_eq!(o.status.code(), Some(0), "{args:?}\nstdout: {}\nstderr: {}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr));
    String::from_utf8_lossy(&o.stdout).into_owned()
}

/// 目录里的文件名（排好序）；目录不在算空。
fn names(dir: &std::path::Path) -> Vec<String> {
    let mut v: Vec<String> = std::fs::read_dir(dir).into_iter().flatten().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
    v.sort();
    v
}

/// 产物 EPUB 的正文里有没有 `marker`。
#[track_caller]
fn output_has(path: &std::path::Path, marker: &str) -> bool {
    let entries = bookconv::epubzip::read_entries(&std::fs::read(path).unwrap()).unwrap();
    entries.iter().any(|e| bookconv::epubzip::is_html(&e.name) && String::from_utf8_lossy(&e.data).contains(marker))
}

/// sync 选书：只生成选中的书；`--force` 重建；不跟踪目录时只生成 add 进来的书。
#[test]
fn sync_with_selection_builds_only_selected_books() {
    let dir = tempfile::tempdir().unwrap();
    let base = std::fs::canonicalize(dir.path()).unwrap();
    let lib = base.join("lib");
    let books = base.join("ereader/books");
    std::fs::create_dir_all(&books).unwrap();
    std::fs::write(books.join("甲.epub"), sample_epub("甲")).unwrap();
    std::fs::write(books.join("乙.epub"), sample_epub("乙")).unwrap();
    let out = common::documents(&lib, "ireader");
    ok(&lib, &["track", books.to_str().unwrap()]);
    // 选书前先扫目录：两本都入库，只生成甲
    let s = ok(&lib, &["sync", "--device=ireader", "甲"]);
    assert!(s.contains("新增 2"), "{s}");
    assert_eq!(names(&out), ["甲.epub"], "{s}");
    let s = ok(&lib, &["sync", "--device=ireader", "甲"]);
    assert!(s.contains("已是最新") && s.contains("重新生成并传上 0 本"), "选了书时没变化的也报：{s}");
    let s = ok(&lib, &["sync", "--device=ireader", "--force", books.join("甲.epub").to_str().unwrap()]);
    assert!(s.contains("≡ 重新生成 [ireader] 甲") && s.contains("内容没变（没再传）1 本"), "--force 重建（一样的不再传），原件路径也能选：{s}");
    // 不选书：全部，只报有变化的
    let s = ok(&lib, &["sync", "--device=ireader"]);
    assert!(s.contains("✓ 生成 [ireader] 乙") && !s.contains("已是最新（"), "{s}");
    assert_eq!(names(&out), ["乙.epub", "甲.epub"]);
    let o = booklib(Some(&lib), &["sync", "--device=ireader", "丙"]);
    assert_eq!(o.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&o.stderr).contains("没有匹配的书"));

    // 没跟踪任何目录：sync 照样生成 add 进来的书
    let lib2 = base.join("lib2");
    let single = base.join("单本.epub");
    std::fs::write(&single, sample_epub("单本")).unwrap();
    ok(&lib2, &["add", single.to_str().unwrap()]);
    ok(&lib2, &["sync", "--device=ireader"]);
    assert!(common::documents(&lib2, "ireader").join("单本.epub").is_file());
}

/// 缺省的清理：原件删了的连同产物从书库删掉；挪动、--keep、跟踪目录整个不在、add 进来的书不删。
#[test]
fn sync_prunes_missing_originals_automatically() {
    let dir = tempfile::tempdir().unwrap();
    let base = std::fs::canonicalize(dir.path()).unwrap();
    let lib = base.join("lib");
    let books = base.join("ereader/books");
    std::fs::create_dir_all(books.join("子")).unwrap();
    for t in ["甲", "乙"] {
        std::fs::write(books.join(format!("{t}.epub")), sample_epub(t)).unwrap();
    }
    std::fs::write(books.join("子/丙.epub"), sample_epub("丙")).unwrap();
    let outside = base.join("外/外.epub");
    std::fs::create_dir_all(outside.parent().unwrap()).unwrap();
    std::fs::write(&outside, sample_epub("外")).unwrap();
    ok(&lib, &["track", books.to_str().unwrap()]);
    ok(&lib, &["add", outside.to_str().unwrap()]);
    ok(&lib, &["sync", "--device=ireader"]);
    let out = common::documents(&lib, "ireader");
    assert!(out.join("乙.epub").is_file() && out.join("子/丙.epub").is_file());
    let count = || common::open(&lib).list().len();
    assert_eq!(count(), 4);

    // 删一个原件：书库和产物都没了
    std::fs::remove_file(books.join("乙.epub")).unwrap();
    let s = ok(&lib, &["sync", "--device=ireader"]);
    assert!(s.contains("书库里也删了") && s.contains("不在了 1（从书库删了 1）"), "{s}");
    assert!(!out.join("乙.epub").exists());
    assert_eq!(count(), 3);

    // 挪动：按内容认出来，不删，产物跟着挪
    std::fs::rename(books.join("子/丙.epub"), books.join("丙（挪过）.epub")).unwrap();
    let s = ok(&lib, &["sync", "--device=ireader"]);
    assert!(s.contains("不在了 0"), "{s}");
    assert_eq!(count(), 3);
    assert!(out.join("丙.epub").is_file() && !out.join("子").exists(), "{s}");

    // --keep：这一次只报告；下次缺省的 sync 再删
    std::fs::remove_file(books.join("甲.epub")).unwrap();
    let s = ok(&lib, &["sync", "--keep", "--device=ireader"]);
    assert!(s.contains("--keep，这次没从书库删") && s.contains("（从书库删了 0）"), "{s}");
    assert_eq!(count(), 3);
    assert!(out.join("甲.epub").is_file());

    // 跟踪目录整个不在（U 盘没插）：什么都不动
    let away = base.join("ereader/books.挪开");
    std::fs::rename(&books, &away).unwrap();
    let s = ok(&lib, &["sync", "--no-build"]);
    assert!(s.contains("不在了 0"), "{s}");
    assert_eq!(count(), 3);
    std::fs::rename(&away, &books).unwrap();

    // 恢复后缺省 sync：--keep 留下的那本这次删掉
    let s = ok(&lib, &["sync", "--no-build"]);
    assert!(s.contains("不在了 1（从书库删了 1）"), "{s}");
    assert!(!out.join("甲.epub").exists());
    assert_eq!(count(), 2);

    // add 进来、原件在跟踪目录以外的书：原件没了也不自动删
    std::fs::remove_file(&outside).unwrap();
    ok(&lib, &["sync", "--no-build"]);
    let titles: Vec<String> = common::open(&lib).list().into_iter().map(|m| m.title).collect();
    assert!(titles.contains(&"外".to_string()), "{titles:?}");
}

/// 书的更新（同名同作者、内容不同 → 新 id）：产物路径和旧条目的一样，清理旧条目时不能删掉新书的产物；
/// 同一次 sync 里旧的走、新的来，产物目录里只剩新书、新内容。同名的两本同时在时各用各的文件名，互不覆盖。
#[test]
fn book_updates_replace_outputs_without_deleting_the_new_one() {
    let dir = tempfile::tempdir().unwrap();
    let base = std::fs::canonicalize(dir.path()).unwrap();
    let lib = base.join("lib");
    let books = base.join("ereader/books");
    std::fs::create_dir_all(&books).unwrap();
    let out = common::documents(&lib, "ireader");
    let plain = out.join("同名.epub");
    let sync = || ok(&lib, &["sync", "--device=ireader"]);
    ok(&lib, &["track", books.to_str().unwrap()]);
    let count = || common::open(&lib).list().len();

    std::fs::write(books.join("同名.epub"), sample_epub_with("同名", "第一版")).unwrap();
    sync();
    assert!(output_has(&plain, "第一版"));

    // 1. 同一路径换内容（换成新版本）
    std::fs::write(books.join("同名.epub"), sample_epub_with("同名", "第二版")).unwrap();
    let s = sync();
    assert!(s.contains("改过 1"), "{s}");
    assert_eq!(names(&out), ["同名.epub"], "{s}");
    assert!(output_has(&plain, "第二版") && count() == 1);

    // 2. 删旧加新，文件名不同
    std::fs::remove_file(books.join("同名.epub")).unwrap();
    std::fs::write(books.join("同名（修订）.epub"), sample_epub_with("同名", "第三版")).unwrap();
    let s = sync();
    assert!(s.contains("新增 1") && s.contains("从书库删了 1"), "{s}");
    assert_eq!(names(&out), ["同名.epub"], "{s}");
    assert!(output_has(&plain, "第三版") && count() == 1);

    // 3. 删旧加新，文件名相同（先删再放，比如从别处拷过来覆盖）
    std::fs::remove_file(books.join("同名（修订）.epub")).unwrap();
    std::fs::write(books.join("同名（修订）.epub"), sample_epub_with("同名", "第四版")).unwrap();
    let s = sync();
    assert_eq!(names(&out), ["同名.epub"], "{s}");
    assert!(output_has(&plain, "第四版") && count() == 1);

    // 4. 同名的两本同时在：后来的那本加 [id 前 6 位]，各是各的内容
    std::fs::write(books.join("另一份.epub"), sample_epub_with("同名", "另一份")).unwrap();
    let s = sync();
    let ls = names(&out);
    assert_eq!(ls.len(), 2, "{ls:?} {s}");
    let other = out.join(ls.iter().find(|n| n.starts_with("同名 [")).unwrap());
    assert!(output_has(&plain, "第四版") && output_has(&other, "另一份"));
    let s = sync();
    assert!(s.contains("重新生成并传上 0 本") && names(&out) == ls, "再跑一次不动：{s}");
    // 删掉其中一本：只删它自己的产物
    std::fs::remove_file(books.join("另一份.epub")).unwrap();
    sync();
    assert_eq!(names(&out), ["同名.epub"]);
    assert!(output_has(&plain, "第四版") && count() == 1);
    // 同一次 sync 里一本走、一本来：新来的用上腾出来的名字
    std::fs::remove_file(books.join("同名（修订）.epub")).unwrap();
    std::fs::write(books.join("另一份.epub"), sample_epub_with("同名", "另一份")).unwrap();
    let s = sync();
    assert_eq!(names(&out), ["同名.epub"], "{s}");
    assert!(output_has(&plain, "另一份") && count() == 1);
}

/// 改成直接送设备以前留在电脑上的产物（`D/../ireader/…`、`D/../xochitl/…`）：内容没变就直接挪到设备上（不重新生成），电脑上的删掉。
#[test]
fn local_outputs_from_before_move_onto_devices() {
    let dir = tempfile::tempdir().unwrap();
    let base = std::fs::canonicalize(dir.path()).unwrap();
    let lib_dir = base.join("lib");
    let mv = common::FakeMove::start();
    let lib = common::open_with(&lib_dir, Some(&mv));
    let src = base.join("书.epub");
    std::fs::write(&src, sample_epub("书")).unwrap();
    let Added::New(m) = lib.add_file(&src).unwrap() else { panic!() };
    for dev in ["ireader", "xochitl"] {
        lib.build(&m, profile::get(dev).unwrap(), false).unwrap();
        // 把记录改回"产物在电脑上"的样子
        let local_root = base.join(dev);
        std::fs::create_dir_all(&local_root).unwrap();
        let local = local_root.join("书.epub");
        let sp = lib_dir.join(format!("output-state/{dev}.json"));
        let mut st: serde_json::Value = serde_json::from_slice(&std::fs::read(&sp).unwrap()).unwrap();
        let e = &mut st["books"][&m.id];
        if dev == "ireader" {
            std::fs::rename(e["path"].as_str().unwrap(), &local).unwrap();
        } else {
            std::fs::write(&local, &mv.docs.lock().unwrap().values().next().unwrap().bytes).unwrap();
            mv.docs.lock().unwrap().clear();
            e.as_object_mut().unwrap().remove("uuid");
            e.as_object_mut().unwrap().remove("name");
        }
        e["path"] = serde_json::json!(local);
        e["root"] = serde_json::json!(local_root);
        std::fs::write(&sp, st.to_string()).unwrap();
        let lib = common::open_with(&lib_dir, Some(&mv));
        let Built::Moved { from, .. } = lib.build(&m, profile::get(dev).unwrap(), false).unwrap() else { panic!("{dev} 应直接挪过去") };
        assert_eq!(from, local);
        assert!(!local.exists(), "{dev}：电脑上的删掉");
    }
    assert!(common::documents(&lib_dir, "ireader").join("书.epub").is_file());
    assert_eq!(mv.live(), [(String::new(), "书".to_string())]);
}

/// 以前在电脑上生成、用户自己拷上设备（电脑上的已经挪走）的产物：先比较——指纹没变，设备上同子目录同名的那份直接认领、
/// 不重新生成；指纹变了才重新生成，覆盖那份（不另起 `[id]` 名字）。
#[test]
fn manual_copies_on_device_are_taken_over() {
    let dir = tempfile::tempdir().unwrap();
    let base = std::fs::canonicalize(dir.path()).unwrap();
    let lib_dir = base.join("lib");
    let lib = common::open(&lib_dir);
    let books = base.join("ereader/books");
    std::fs::create_dir_all(books.join("子")).unwrap();
    std::fs::write(books.join("子/x.epub"), sample_epub("书")).unwrap();
    lib.track(&books).unwrap();
    lib.sync(Prune::Keep, |_| {}).unwrap();
    let m = lib.list().remove(0);
    let docs = common::documents(&lib_dir, "ireader");
    let Built::Written { path, .. } = lib.build(&m, profile::get("ireader").unwrap(), false).unwrap() else { panic!() };
    // 改成：记录说产物在电脑上（ereader/ireader/子/书.epub，已被用户挪走），设备上是用户拷上去的那份
    std::fs::write(&path, b"copied by hand").unwrap();
    let sp = lib_dir.join("output-state/ireader.json");
    let mut st: serde_json::Value = serde_json::from_slice(&std::fs::read(&sp).unwrap()).unwrap();
    st["books"][&m.id]["path"] = serde_json::json!(base.join("ereader/ireader/子/书.epub"));
    st["books"][&m.id]["root"] = serde_json::json!(base.join("ereader/ireader"));
    st["books"][&m.id].as_object_mut().unwrap().remove("sha");
    std::fs::write(&sp, st.to_string()).unwrap();
    let lib = common::open(&lib_dir);
    let Built::Adopted(again) = lib.build(&m, profile::get("ireader").unwrap(), false).unwrap() else { panic!("指纹没变：认领，不重新生成") };
    assert_eq!(again, docs.join("子/书.epub"));
    assert_eq!(std::fs::read(&again).unwrap(), b"copied by hand", "认领不动设备上的文件");
    assert!(matches!(lib.build(&m, profile::get("ireader").unwrap(), false).unwrap(), Built::UpToDate(_)));
    // 指纹变了（规则升级）：重新生成，和设备上那份不同就覆盖，名字不变
    let mut st: serde_json::Value = serde_json::from_slice(&std::fs::read(&sp).unwrap()).unwrap();
    st["books"][&m.id]["fingerprint"] = "旧规则".into();
    std::fs::write(&sp, st.to_string()).unwrap();
    let lib = common::open(&lib_dir);
    let Built::Written { path: again, .. } = lib.build(&m, profile::get("ireader").unwrap(), false).unwrap() else { panic!() };
    assert_eq!(again, docs.join("子/书.epub"), "覆盖那份，不另起名字");
    assert_ne!(std::fs::read(&again).unwrap(), b"copied by hand");
    assert_eq!(names(&docs.join("子")), ["书.epub"], "不留重复");
}

/// sync 的做法：先比较、生成（prepare），传设备交给各设备的传输线程（Pipeline），传完记下来（complete）。
/// 几本书、Kindle 和 Move 同时在传；第二轮全都已是最新。
#[test]
fn pipeline_transfers_in_background_and_records_when_done() {
    let dir = tempfile::tempdir().unwrap();
    let lib_dir = dir.path().join("lib");
    let mv = common::FakeMove::start();
    let lib = common::open_with(&lib_dir, Some(&mv));
    for t in ["甲", "乙", "丙"] {
        let p = dir.path().join(format!("{t}.epub"));
        std::fs::write(&p, sample_epub(t)).unwrap();
        lib.add_file(&p).unwrap();
    }
    let devices = [profile::get("ireader").unwrap(), profile::get("xochitl").unwrap()];
    let run = || {
        let mut pipe = library::Pipeline::default();
        let mut results = Vec::new();
        for m in lib.list() {
            for d in devices {
                while !pipe.has_room(&d.id) {
                    if let library::Event::Done(t, r) = pipe.next(true).unwrap() {
                        results.push(lib.complete(*t, r).unwrap());
                    }
                }
                match lib.prepare(&m, d, false).unwrap() {
                    library::Step::Done(b) => results.push(b),
                    library::Step::Transfer(t) => pipe.submit(*t),
                }
            }
        }
        while let Some(ev) = pipe.next(true) {
            if let library::Event::Done(t, r) = ev {
                results.push(lib.complete(*t, r).unwrap());
            }
        }
        results
    };
    let first = run();
    assert_eq!(first.iter().filter(|b| matches!(b, Built::Written { .. })).count(), 6, "{first:?}");
    assert_eq!(mv.live().len(), 3);
    assert_eq!(common::documents(&lib_dir, "ireader").read_dir().unwrap().count(), 3);
    assert!(std::fs::read_dir(&lib_dir).unwrap().flatten().all(|e| !e.file_name().to_string_lossy().starts_with(".tmp-")), "临时目录传完就删");
    let second = run();
    assert!(second.iter().all(|b| matches!(b, Built::UpToDate(_))), "{second:?}");
}
