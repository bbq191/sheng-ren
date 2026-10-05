//! booklib 命令行：参数解析、报错、退出码（0 成功，1 用法错，2 处理失败）。都不联网。
mod common;

use common::{booklib, jpeg, sample_epub, stderr, stdout};
use std::io::Write;
use std::path::Path;

fn code(o: &std::process::Output) -> Option<i32> {
    o.status.code()
}

/// 退出码是 `want`，stderr 含 `msg`。
#[track_caller]
fn expect(o: std::process::Output, want: i32, msg: &str) {
    assert_eq!(code(&o), Some(want), "stdout: {}\nstderr: {}", stdout(&o), stderr(&o));
    assert!(stderr(&o).contains(msg), "stderr 里应有「{msg}」：{}", stderr(&o));
}

#[test]
fn usage_errors_exit_1() {
    let dir = tempfile::tempdir().unwrap();
    let lib = dir.path().join("lib");
    let lib = Some(lib.as_path());
    expect(booklib(lib, &[]), 1, "用法");
    expect(booklib(lib, &["frobnicate"]), 1, "不认识的命令 frobnicate");
    expect(booklib(lib, &["build", "--device", "kindle"]), 1, "要写成 --device=值");
    expect(booklib(lib, &["build", "--device=nosuch"]), 1, "没有阅读模式 nosuch");
    expect(booklib(lib, &["build", "--out=x"]), 1, "不认识选项 --out=");
    expect(booklib(lib, &["list", "--device="]), 1, "--device= 后面要写值");
    expect(booklib(lib, &["add"]), 1, "add 要给文件或网址");
    expect(booklib(lib, &["remove"]), 1, "remove 要给 id");
    expect(booklib(lib, &["track"]), 1, "track 要给目录");
    // meta 必须选 --fetch 或 --edit；两种模式的选项不能混用
    expect(booklib(lib, &["meta"]), 1, "--fetch");
    expect(booklib(lib, &["meta", "白夜行"]), 1, "--edit");
    expect(booklib(lib, &["meta", "--fetch", "--title=x"]), 1, "不认识选项 --title=");
    expect(booklib(lib, &["meta", "--fetch", "--no-backup"]), 1, "不认识 --no-backup");
    // sync 的选项组合（要先有跟踪目录，不然先报"还没有跟踪"）
    let books = dir.path().join("books");
    std::fs::create_dir_all(&books).unwrap();
    assert_eq!(code(&booklib(lib, &["track", books.to_str().unwrap()])), Some(0));
    expect(booklib(lib, &["sync", "--no-build", "--device=kindle"]), 1, "--no-build 和 --device 不能一起用");
    expect(booklib(lib, &["sync", "--watch=0"]), 1, "--watch= 要写正整数秒数");
    expect(booklib(lib, &["sync", "白夜行"]), 1, "sync 不接受书名参数");
}

#[test]
fn processing_failures_exit_2() {
    let dir = tempfile::tempdir().unwrap();
    let lib = dir.path().join("lib");
    let lib = Some(lib.as_path());
    expect(booklib(lib, &["sync"]), 2, "还没有跟踪任何目录");
    expect(booklib(lib, &["build", "不存在的书"]), 2, "没有匹配的书");
    expect(booklib(lib, &["meta", "--fetch", "不存在的书"]), 2, "没有匹配的书");
    expect(booklib(lib, &["build", "a/b"]), 2, "路径里有空格时要整个加引号");
    expect(booklib(lib, &["remove", "ffffffffffff"]), 2, "");
    let missing = dir.path().join("没有这本.epub");
    expect(booklib(lib, &["add", missing.to_str().unwrap()]), 2, "✗");
    expect(booklib(lib, &["add", dir.path().to_str().unwrap()]), 2, "是目录：目录用 booklib track");
}

#[test]
fn devices_lists_the_three_modes() {
    let dir = tempfile::tempdir().unwrap();
    let o = booklib(Some(&dir.path().join("lib")), &["devices"]);
    assert_eq!(code(&o), Some(0), "{}", stderr(&o));
    let out = stdout(&o);
    for (id, fmt) in [("ireader", "EPUB"), ("kindle", "KFX"), ("xochitl", "EPUB")] {
        assert!(out.lines().any(|l| l.starts_with(id) && l.contains(fmt)), "{id} {fmt}：{out}");
    }
}

/// `meta --fetch` 不联网的两条路：`--clear`（去掉找来的）、漫画（跳过）。
#[test]
fn meta_fetch_clear_and_comic_skip_work_offline() {
    let dir = tempfile::tempdir().unwrap();
    let lib = dir.path().join("lib");
    let lib = Some(lib.as_path());
    let book = dir.path().join("风起.epub");
    std::fs::write(&book, sample_epub("风起")).unwrap();
    let cbz = dir.path().join("漫画 - 01卷.cbz");
    {
        let mut z = zip::ZipWriter::new(std::fs::File::create(&cbz).unwrap());
        for i in 1..=3 {
            z.start_file(format!("p{i}.jpg"), zip::write::SimpleFileOptions::default()).unwrap();
            z.write_all(&jpeg(600, 900)).unwrap();
        }
        z.finish().unwrap();
    }
    assert_eq!(code(&booklib(lib, &["add", book.to_str().unwrap(), cbz.to_str().unwrap()])), Some(0));
    let o = booklib(lib, &["meta", "--fetch", "--clear", "风起"]);
    assert_eq!(code(&o), Some(0), "{}", stderr(&o));
    assert!(stdout(&o).contains("本来就没有找来的元数据  风起"), "{}", stdout(&o));
    let o = booklib(lib, &["meta", "--fetch", "漫画"]);
    assert_eq!(code(&o), Some(0), "{}", stderr(&o));
    assert!(stdout(&o).contains("漫画不找元数据"), "{}", stdout(&o));
}

/// 目录里除了 `keep` 以外没有别的文件（不留备份、不留临时文件）。
#[track_caller]
fn only(dir: &Path, keep: &[&str]) {
    let mut names: Vec<String> = std::fs::read_dir(dir).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
    names.sort();
    assert_eq!(names, keep, "目录里不该有别的文件");
}

#[test]
fn meta_edit_views_edits_and_deletes_with_empty_values() {
    let dir = tempfile::tempdir().unwrap();
    let book = dir.path().join("书.epub");
    std::fs::write(&book, sample_epub("风起")).unwrap();
    let b = book.to_str().unwrap();
    let view = || {
        let o = booklib(None, &["meta", "--edit", b]);
        assert_eq!(code(&o), Some(0), "{}", stderr(&o));
        stdout(&o)
    };
    let v = view();
    assert!(v.contains("标题: 风起") && v.contains("作者: 作者") && v.contains("封面: 无"), "{v}");

    // `--名字 值` 和 `--名字=值` 都认；可重复的选项整体替换；--edit 不一定紧跟 meta；--library= 不打开书库
    let unused_lib = dir.path().join("不该建的书库");
    let o = booklib(Some(&unused_lib), &["meta", b, "--edit", "--title", "新书名", "--publisher=某出版社", "--tag", "小说", "--tag=科幻"]);
    assert_eq!(code(&o), Some(0), "{}", stderr(&o));
    assert!(stdout(&o).contains("已写入"), "{}", stdout(&o));
    assert!(!unused_lib.exists(), "改文件不碰书库");
    let v = view();
    assert!(v.contains("标题: 新书名") && v.contains("出版社: 某出版社") && v.contains("标签: 小说; 科幻"), "{v}");
    only(dir.path(), &["书.epub"]);

    // 值给空字符串 = 删掉（两种写法）
    assert_eq!(code(&booklib(None, &["meta", "--edit", b, "--publisher", "", "--tag="])), Some(0));
    let v = view();
    assert!(!v.contains("出版社") && !v.contains("标签"), "{v}");
    assert!(v.contains("标题: 新书名"), "没给的选项不动：{v}");

    // 封面：加上、取出、去掉
    let img = dir.path().join("封面.jpg");
    std::fs::write(&img, jpeg(300, 400)).unwrap();
    assert_eq!(code(&booklib(None, &["meta", "--edit", b, "--cover", img.to_str().unwrap()])), Some(0));
    assert!(view().contains("封面: 有（jpg，300×400"), "{}", view());
    let got = dir.path().join("取出.jpg");
    assert_eq!(code(&booklib(None, &["meta", "--edit", b, "--get-cover", got.to_str().unwrap()])), Some(0));
    assert_eq!(image::image_dimensions(&got).unwrap(), (300, 400));
    assert_eq!(code(&booklib(None, &["meta", "--edit", b, "--cover="])), Some(0));
    assert!(view().contains("封面: 无"), "{}", view());
    only(dir.path(), &["书.epub", "取出.jpg", "封面.jpg"]);
}

#[test]
fn meta_edit_errors_leave_the_file_untouched() {
    let dir = tempfile::tempdir().unwrap();
    let book = dir.path().join("书.epub");
    std::fs::write(&book, sample_epub("风起")).unwrap();
    let before = std::fs::read(&book).unwrap();
    let b = book.to_str().unwrap();
    expect(booklib(None, &["meta", "--edit"]), 1, "要给一个 EPUB 文件");
    expect(booklib(None, &["meta", "--edit", b, b]), 1, "只能给一个文件");
    expect(booklib(None, &["meta", "--edit", b, "--force"]), 1, "是 meta --fetch 的选项");
    expect(booklib(None, &["meta", "--edit", b, "--no-backup"]), 1, "不认识选项 --no-backup");
    expect(booklib(None, &["meta", "--edit", b, "--title"]), 1, "--title 要给值");
    expect(booklib(None, &["meta", "--edit", dir.path().join("没有.epub").to_str().unwrap()]), 2, "文件不存在");
    expect(booklib(None, &["meta", "--edit", b, "--get-cover", dir.path().join("x.jpg").to_str().unwrap()]), 2, "书里没有封面");
    expect(booklib(None, &["meta", "--edit", b, "--title", "新", "--cover", dir.path().join("没有.jpg").to_str().unwrap()]), 2, "没有.jpg");
    std::fs::write(dir.path().join("坏.epub"), b"not a zip").unwrap();
    expect(booklib(None, &["meta", "--edit", dir.path().join("坏.epub").to_str().unwrap(), "--title", "新"]), 2, "没改");
    assert_eq!(std::fs::read(&book).unwrap(), before, "出错时原文件一个字节不变");
    only(dir.path(), &["书.epub", "坏.epub"]);
    let o = booklib(None, &["meta", "--edit", "--help"]);
    assert_eq!(code(&o), Some(0));
    assert!(stdout(&o).contains("booklib meta --edit 书.epub"), "{}", stdout(&o));
}

#[test]
fn meta_edit_keeps_symlinks_and_permissions() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let real = dir.path().join("真.epub");
    std::fs::write(&real, sample_epub("风起")).unwrap();
    std::fs::set_permissions(&real, std::fs::Permissions::from_mode(0o640)).unwrap();
    let link = dir.path().join("链接.epub");
    std::os::unix::fs::symlink(&real, &link).unwrap();
    let o = booklib(None, &["meta", "--edit", link.to_str().unwrap(), "--title", "新书名"]);
    assert_eq!(code(&o), Some(0), "{}", stderr(&o));
    assert!(std::fs::symlink_metadata(&link).unwrap().file_type().is_symlink(), "链接还是链接");
    assert_eq!(std::fs::read_link(&link).unwrap(), real);
    assert_eq!(std::fs::metadata(&real).unwrap().permissions().mode() & 0o777, 0o640, "权限不变");
    assert!(stdout(&booklib(None, &["meta", "--edit", real.to_str().unwrap()])).contains("标题: 新书名"), "改的是链接指向的文件");
    only(dir.path(), &["真.epub", "链接.epub"]);
}

/// `meta --show`：只列跟踪目录里的书，书里写的和找来的元数据都显示；不加锁、不联网。
#[test]
fn meta_show_lists_tracked_books() {
    let dir = tempfile::tempdir().unwrap();
    let lib = dir.path().join("lib");
    let books = dir.path().join("books");
    std::fs::create_dir_all(&books).unwrap();
    std::fs::write(books.join("风起.epub"), sample_epub("风起")).unwrap();
    expect(booklib(Some(&lib), &["meta", "--show"]), 2, "没有跟踪的目录");
    assert_eq!(code(&booklib(Some(&lib), &["track", books.to_str().unwrap()])), Some(0));
    assert_eq!(code(&booklib(Some(&lib), &["sync", "--no-build"])), Some(0));
    let o = booklib(Some(&lib), &["meta", "--show"]);
    assert_eq!(code(&o), Some(0), "{}", stderr(&o));
    let out = stdout(&o);
    assert!(out.contains("风起") && out.contains("书里 标题：风起") && out.contains("找来：没找过") && out.contains("共 1 本"), "{out}");
    expect(booklib(Some(&lib), &["meta", "--show", "--fetch"]), 1, "不能一起用");
}
