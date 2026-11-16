//! 设备挂载卡死时 `sync`、`devices` 不会一直挂着（2026-11-16 单 `sync-mount-stall`）。
//!
//! 用测试钩子 `BOOKLIB_TEST_DEVICE_HANG=<目录>` 模拟卡死的挂载（进到这个目录下的设备 I/O 一直 sleep），
//! `BOOKLIB_DEVICE_TIMEOUT=1` 把时限压到 1 秒。所有子进程都由 `common::booklib_env` 带超时跑，测试本身不会挂住。

mod common;

use common::{booklib_env, dev_dir, documents, sample_epub, stderr, stdout};
use library::Built;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// 临时书库和一本要传的书。返回（临时目录、书库、原书）。
fn setup(lib_name: &str) -> (tempfile::TempDir, PathBuf, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let base = std::fs::canonicalize(dir.path()).unwrap();
    let lib = base.join(lib_name);
    common::open(&lib);
    dev_dir(&lib); // 建好各设备的目录（普通目录当设备）
    let src = base.join("甲.epub");
    std::fs::write(&src, sample_epub("甲")).unwrap();
    (dir, lib, src)
}

fn hang_env(hung: &Path) -> [(&'static str, String); 2] {
    [
        ("BOOKLIB_DEVICE_TIMEOUT", "1".to_string()),
        (
            "BOOKLIB_TEST_DEVICE_HANG",
            hung.to_string_lossy().into_owned(),
        ),
    ]
}

fn envs<'a>(e: &'a [(&'static str, String)]) -> Vec<(&'a str, &'a str)> {
    e.iter().map(|(k, v)| (*k, v.as_str())).collect()
}

/// 设备上产物目录里的文件名（目录不在就是空）。
fn names(dir: &Path) -> Vec<String> {
    let mut v: Vec<String> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    v.sort();
    v
}

/// 连接探测：卡死的设备报「挂载没响应」、这一轮不传；另一台正常设备照常传完，退出码按现有规则。
#[test]
fn sync_skips_a_hung_mount_and_transfers_the_other_device() {
    let (_dir, lib, src) = setup("lib");
    assert!(common::booklib(Some(&lib), &["track", src.to_str().unwrap()])
        .status
        .success());

    let hung = dev_dir(&lib).join("Ireader");
    let e = hang_env(&hung);
    let start = Instant::now();
    let o = booklib_env(
        Some(&lib),
        &["sync", "--device=ireader,kindle"],
        &envs(&e),
    );
    let took = start.elapsed();
    let all = format!("{}\n{}", stdout(&o), stderr(&o));
    assert!(
        took < Duration::from_secs(20),
        "卡死的设备上 sync 应在几秒内结束，实际 {took:?}：{all}"
    );
    assert!(
        all.contains("挂载没响应") && all.contains("Ireader"),
        "应报这台挂载没响应：{all}"
    );
    assert_eq!(
        o.status.code(),
        Some(0),
        "没接上的设备不算失败：{all}"
    );
    // 另一台照常传完
    assert_eq!(
        names(&documents(&lib, "kindle")),
        ["甲.kfx"],
        "Kindle 照常传完：{all}"
    );
    // 卡死的那台一本没传
    assert!(
        names(&documents(&lib, "ireader")).is_empty(),
        "卡死的那台一本没传：{all}"
    );
}

/// 传输途中卡住：sync 在时限内结束、报这台没传成；生成记录里那本不是传好了；去掉钩子再 sync，那本传上且内容逐字节对。
#[test]
fn transfer_stall_is_reported_and_retried_after_the_mount_recovers() {
    let (_dir, lib, src) = setup("lib");
    let o = common::booklib(Some(&lib), &["add", src.to_str().unwrap()]);
    assert!(o.status.success(), "{}", stderr(&o));
    let book = common::open(&lib).list().remove(0).id;

    let hung = dev_dir(&lib).join("Ireader");
    let mut e = hang_env(&hung).to_vec();
    // 只让传输写设备上的文件时卡住：连接探测、主线程那几处不卡（传输看门狗单独验）
    e.push(("BOOKLIB_TEST_DEVICE_HANG_ON", "write".to_string()));
    let start = Instant::now();
    let o = booklib_env(Some(&lib), &["sync", "--device=ireader"], &envs(&e));
    let took = start.elapsed();
    let all = format!("{}\n{}", stdout(&o), stderr(&o));
    assert!(
        took < Duration::from_secs(20),
        "传输卡住时 sync 应在时限内结束，实际 {took:?}：{all}"
    );
    assert!(
        all.contains("挂载没响应") && all.contains("没传成"),
        "应报这台没传成：{all}"
    );
    assert_eq!(o.status.code(), Some(2), "有书没传成：{all}");
    // 生成记录里那本不是「传好了」（新书直写时先登记指纹留空的记录）
    let state = std::fs::read(lib.join("output-state/ireader.json")).unwrap();
    let state: serde_json::Value = serde_json::from_slice(&state).unwrap();
    let entry = &state["books"][&book];
    assert!(
        entry.is_null() || entry["fingerprint"].as_str() == Some(""),
        "没传完的记录不能算传好：{entry}"
    );
    assert!(
        names(&documents(&lib, "ireader")).is_empty(),
        "设备上不该留下传好的那份：{all}"
    );

    // 去掉钩子再 sync：那本传上，内容和同一套规则生成的逐字节相同
    let o = booklib_env(Some(&lib), &["sync", "--device=ireader"], &[]);
    let all = format!("{}\n{}", stdout(&o), stderr(&o));
    assert_eq!(o.status.code(), Some(0), "恢复后照常传：{all}");
    let on_device = documents(&lib, "ireader").join("甲.epub");
    let bytes = std::fs::read(&on_device).unwrap_or_else(|e| panic!("{}：{e}", on_device.display()));
    assert!(!bytes.is_empty());
    // 用同一本书、同一套参数（不带 [deliver] 的 pc 模式）生成一份对照
    common::with_pc_profile(&lib);
    let lib2 = common::open(&lib);
    let m = lib2.list().remove(0);
    let pc = lib2.devices().get("pc").unwrap();
    let Built::Written { path, .. } = lib2.build(&m, pc, false).unwrap() else {
        panic!("对照产物应生成")
    };
    assert_eq!(std::fs::read(path).unwrap(), bytes, "设备上的那份逐字节对");
}

/// `booklib devices` 对卡住的设备在时限内返回并显示原因。
#[test]
fn devices_reports_a_hung_mount_without_hanging() {
    let (_dir, lib, _src) = setup("lib");
    let hung = dev_dir(&lib).join("Ireader");
    let e = hang_env(&hung);
    let start = Instant::now();
    let o = booklib_env(Some(&lib), &["devices"], &envs(&e));
    let took = start.elapsed();
    let out = stdout(&o);
    assert!(
        took < Duration::from_secs(10),
        "devices 应在时限内返回，实际 {took:?}：{out}"
    );
    assert_eq!(o.status.code(), Some(0), "{}", stderr(&o));
    let line = out
        .lines()
        .find(|l| l.contains("挂载没响应") && l.contains("Ireader"))
        .unwrap_or_else(|| panic!("devices 里应显示 ireader 挂载没响应：{out}"));
    assert!(line.contains("Ireader"), "{line}");
}

/// 主线程上碰设备的那一步卡住：看门狗在 `2T` 后打一行、退出码 2 结束整个进程，不无限挂起。
#[test]
fn main_thread_device_op_exits_with_code_2_after_the_fallback_timeout() {
    let (_dir, lib, src) = setup("lib");
    assert!(common::booklib(Some(&lib), &["track", src.to_str().unwrap()])
        .status
        .success());
    let hung = dev_dir(&lib).join("Ireader");
    let mut e = hang_env(&hung).to_vec();
    e.push(("BOOKLIB_TEST_DEVICE_HANG_ON", "main".to_string()));
    let start = Instant::now();
    let o = booklib_env(Some(&lib), &["sync", "--device=ireader"], &envs(&e));
    let took = start.elapsed();
    let all = format!("{}\n{}", stdout(&o), stderr(&o));
    assert!(
        took < Duration::from_secs(20),
        "主线程兜底应在 2T 后退出，实际 {took:?}：{all}"
    );
    assert_eq!(o.status.code(), Some(2), "兜底退出码是 2：{all}");
    assert!(
        all.contains("挂载没响应") && all.contains("booklib 退出"),
        "应打一行中文说明并退出：{all}"
    );
}
