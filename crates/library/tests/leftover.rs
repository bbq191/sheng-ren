//! 设备连接时的残留临时文件清理（2026-11-16 单 `device-leftover-subdir`）。
//!
//! 规则：`sync` 连上设备后，清理的目录 = 生成记录里的目录 ∪ 这一轮要往这台设备放书的目录；只删名字严格是
//! `.tmp-<进程号>-<计数>-<名>`、主人进程不在的普通文件。用户放在设备上的文件、目录、符号链接一个都不能碰。
//! 第一次往设备上的新子目录写、写到一半被杀时那里还没有记录，这一轮的计划目录正好覆盖它。
//!
//! 设备用临时目录当（`common` 的 `BOOKLIB_DEVICES_DIR`），子进程都带超时，不碰真机、不联网。

mod common;

use common::{booklib_env, dev_dir, documents, sample_epub, stderr, stdout};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Output;

/// 不存在的进程号：模拟写到一半被杀的进程。
const DEAD_PID: u32 = u32::MAX;

/// 设备上 `dir` 本层的名字。
fn names(dir: &Path) -> BTreeSet<String> {
    std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect()
}

fn sync_ok(lib_dir: &Path, mode: &str) {
    let device = format!("--device={mode}");
    let o: Output = booklib_env(Some(lib_dir), &["sync", &device, "--no-meta"], &[]);
    assert_eq!(
        o.status.code(),
        Some(0),
        "sync 失败：\nstdout:\n{}\nstderr:\n{}",
        stdout(&o),
        stderr(&o)
    );
}

/// Kobo（USB 存储）和掌阅（MTP）共用的用例。
fn leftover_case(mode: &str) {
    let dir = tempfile::tempdir().unwrap();
    let base = std::fs::canonicalize(dir.path()).unwrap();
    let lib_dir = base.join("lib");
    let books = base.join("books");
    let src = books.join("新目录/甲.epub");
    std::fs::create_dir_all(src.parent().unwrap()).unwrap();
    std::fs::write(&src, sample_epub("甲")).unwrap();

    let lib = common::open(&lib_dir);
    dev_dir(&lib_dir); // 建好各设备的目录（普通目录当设备）
    let docs = documents(&lib_dir, mode);
    let sub = docs.join("新目录");
    std::fs::create_dir_all(&sub).unwrap();

    // 死进程留下的残留：第一次 sync 就要清掉（这时这个目录还没有任何记录）
    let dead = sub.join(format!(".tmp-{DEAD_PID}-0-丙.kepub.epub"));
    std::fs::write(&dead, b"dead").unwrap();

    // 用户自己放在设备上的文件：一个都不能少、内容不变
    let user_files: [(&str, &[u8]); 6] = [
        (".tmp-我的笔记.txt", "我的笔记".as_bytes()),
        (".tmp-abc-1-x.epub", b"abc"),
        (".tmp--1-x.epub", b"nopid"),
        (".tmp-1-x.epub", b"nocount"),
        (".tmp-12-34-", b"noname"),
        ("笔记.txt", b"notes"),
    ];
    for (name, data) in user_files {
        std::fs::write(sub.join(name), data).unwrap();
    }
    // 主人进程还活着的临时文件（沿用原有的保护），名字合规也留着
    let live = sub.join(format!(".tmp-{}-0-live.epub", std::process::id()));
    std::fs::write(&live, b"live").unwrap();
    // 名字合规但是个目录：不删
    let user_dir = sub.join(format!(".tmp-{DEAD_PID}-0-x"));
    std::fs::create_dir(&user_dir).unwrap();
    std::fs::write(user_dir.join("里面.epub"), b"inner").unwrap();
    // 名字合规的符号链接：不删，目标文件也不能动
    let link_target = base.join("链接目标.txt");
    std::fs::write(&link_target, b"target").unwrap();
    let link = sub.join(format!(".tmp-{DEAD_PID}-0-y"));
    std::os::unix::fs::symlink(&link_target, &link).unwrap();
    // 不在计划目录、也不在记录里的用户目录：不扫那里，里面合规的 .tmp-* 也不碰
    let outside_dir = docs.join("用户自己的");
    std::fs::create_dir_all(&outside_dir).unwrap();
    let outside = outside_dir.join(format!(".tmp-{DEAD_PID}-0-x.epub"));
    std::fs::write(&outside, b"keep").unwrap();

    lib.track(&books).unwrap();
    assert!(
        !lib_dir.join(format!("output-state/{mode}.json")).exists(),
        "第一次 sync 之前这台设备还没有生成记录，新子目录只能靠这一轮的计划目录扫到"
    );
    let before = names(&sub);

    // 第一次 sync：新子目录里的残留清掉，书照常传上，用户的东西全在
    sync_ok(&lib_dir, mode);

    assert!(!dead.exists(), "新子目录里死进程的临时文件第一次 sync 就要清掉");
    for (name, data) in user_files {
        let p = sub.join(name);
        assert!(p.is_file(), "用户文件被删了：{}", p.display());
        assert_eq!(std::fs::read(&p).unwrap(), data, "用户文件内容变了：{name}");
    }
    assert!(live.is_file(), "主人进程还在的临时文件要留着");
    assert!(user_dir.is_dir(), "名字合规的目录不能删");
    assert!(
        user_dir.join("里面.epub").is_file(),
        "目录里的东西不能动"
    );
    assert!(
        link.symlink_metadata()
            .is_ok_and(|m| m.file_type().is_symlink()),
        "名字合规的符号链接不能删"
    );
    assert_eq!(
        std::fs::read(&link_target).unwrap(),
        b"target",
        "符号链接的目标文件不能动"
    );
    assert!(outside.is_file(), "没扫的用户目录里的临时文件不能删");
    assert!(sub.is_dir(), "清完临时文件，子目录本身要还在");

    let after = names(&sub);
    let mut old = before.clone();
    old.remove(&format!(".tmp-{DEAD_PID}-0-丙.kepub.epub"));
    let added: Vec<&String> = after.difference(&old).collect();
    assert_eq!(added.len(), 1, "第一次 sync 该传上这本书：{added:?}");
    assert!(added[0].contains("甲"), "传上的是这本书：{added:?}");
    let product = sub.join(added[0]);
    assert!(product.is_file());
    let product_bytes = std::fs::read(&product).unwrap();
    assert!(!product_bytes.is_empty(), "传上的书不能是空的");

    // 已有记录的目录照旧清（回归）：再 sync 一次，同样的残留也要清掉
    let again = sub.join(format!(".tmp-{DEAD_PID}-0-again.epub"));
    std::fs::write(&again, b"again").unwrap();
    sync_ok(&lib_dir, mode);
    assert!(!again.exists(), "记录里的目录照旧清残留");
    assert_eq!(
        std::fs::read(&product).unwrap(),
        product_bytes,
        "第二次 sync 不动已经传好的书"
    );
    for (name, data) in user_files {
        let p = sub.join(name);
        assert!(p.is_file(), "第二次 sync 删了用户文件：{}", p.display());
        assert_eq!(std::fs::read(&p).unwrap(), data, "第二次 sync 改了用户文件：{name}");
    }
    assert!(outside.is_file(), "第二次 sync 也不能碰没扫的用户目录");
    assert!(live.is_file(), "活着的主人的临时文件要留着");
}

#[test]
fn kobo_new_subdir_leftovers_cleaned_without_touching_user_files() {
    leftover_case("kobo");
}

#[test]
fn ireader_new_subdir_leftovers_cleaned_without_touching_user_files() {
    leftover_case("ireader");
}

/// 计划里的子目录和记录里的目录都清，但别的目录（连产物根目录都不是）不扫：这里再确认不在计划里的
/// 用户目录在整个 sync 之后原样（Kobo 用例已覆盖根目录下的用户目录，这里补一个 MTP 的）。
#[test]
fn mtp_leaves_unplanned_user_directory_alone() {
    let dir = tempfile::tempdir().unwrap();
    let base = std::fs::canonicalize(dir.path()).unwrap();
    let lib_dir = base.join("lib");
    let books = base.join("books");
    std::fs::create_dir_all(&books).unwrap();
    std::fs::write(books.join("甲.epub"), sample_epub("甲")).unwrap();

    let lib = common::open(&lib_dir);
    dev_dir(&lib_dir);
    let docs = documents(&lib_dir, "ireader");
    let user_dir = docs.join("用户自己的");
    std::fs::create_dir_all(&user_dir).unwrap();
    let dead_tmp = user_dir.join(format!(".tmp-{DEAD_PID}-0-x.epub"));
    std::fs::write(&dead_tmp, b"keep").unwrap();
    let user_file = user_dir.join("笔记.txt");
    std::fs::write(&user_file, b"notes").unwrap();

    lib.track(&books).unwrap();
    sync_ok(&lib_dir, "ireader");

    assert!(dead_tmp.is_file(), "不在计划和记录里的用户目录不扫");
    assert!(user_file.is_file());
    let product: Vec<PathBuf> = std::fs::read_dir(&docs)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_file() && p.extension().is_some_and(|x| x == "epub"))
        .collect();
    assert_eq!(product.len(), 1, "书照常传到产物根目录");
}
