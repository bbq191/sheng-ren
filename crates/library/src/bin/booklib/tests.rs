//! `booklib` 的单元测试：一轮汇总、`list` 的生成记录显示、失败记忆与空间不够的重试判断。

use crate::books::audit_lines;
use crate::report::{built_line, WarningSummary};
use crate::sync::{forget_reconnected, mark_dropped, space_freed};
use library::{BuildEvent, Built, FailMemo, OriginalState, Profile};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

/// 一轮 3 本里 2 本命中「KEPUB CSS 提醒」：逐本结果行不再有这条，汇总只有一行、N = 2；
/// 同一本书（同一台设备上）重复出现只算一次。
#[test]
fn kepub_css_warning_is_summarized_once_per_round() {
    let built = |title: &str, warn: bool| BuildEvent::Built {
        device: "kobo".into(),
        title: title.into(),
        built: Built::Written {
            path: PathBuf::from(format!("/tmp/{title}.kepub.epub")),
            warnings: if warn {
                vec![bookconv::kepub::RISKY_CSS_WARNING.to_string()]
            } else {
                Vec::new()
            },
        },
    };
    let events = [
        built("甲", true),
        built("乙", true),
        built("丙", false),
        built("甲", true),
    ];
    let mut hit = WarningSummary::default();
    for ev in events {
        hit.observe(&ev);
        let BuildEvent::Built {
            device,
            title,
            built,
        } = ev
        else {
            unreachable!()
        };
        let line = built_line(&device, &title, built, false).unwrap();
        assert!(!line.contains("KEPUB"), "逐本不打这条：{line}");
    }
    let lines = hit.lines();
    assert_eq!(lines.len(), 1, "{lines:?}");
    assert_eq!(lines[0].matches("KEPUB").count(), 1, "{lines:?}");
    assert!(lines[0].contains("2 本书"), "{lines:?}");
}

/// 一轮里体检 3 本、质量门 2 本、KFX 1 本，外加一条不认识的提醒：逐本结果行下面只剩不认识的；
/// 每类各汇总一行、N 对；质量门、KFX 两行带书名；同一本（同设备同路径）出现两次只算一次。
#[test]
fn round_warnings_are_summarized_by_category() {
    let ev = |device: &str, title: &str, warnings: Vec<String>| BuildEvent::Built {
        device: device.into(),
        title: title.into(),
        built: Built::Written {
            path: PathBuf::from(format!("/tmp/{device}/{title}.epub")),
            warnings,
        },
    };
    let one = |w: &str| vec![w.to_string()];
    let events = [
        ev("ireader", "甲", one(library::AUDIT_WARNING)),
        ev("ireader", "乙", one(library::AUDIT_WARNING)),
        ev("ireader", "丙", one(library::AUDIT_WARNING)),
        ev("kobo", "丁", one(library::GATE_WARNING)),
        ev("kobo", "戊", one(library::GATE_WARNING)),
        ev("kindle", "己", one(library::KFX_WARNING)),
        ev("ireader", "甲", one(library::AUDIT_WARNING)),
        ev("ireader", "庚", one("别的提醒")),
    ];
    let mut summary = WarningSummary::default();
    let mut unknown_lines = 0;
    for ev in events {
        summary.observe(&ev);
        let BuildEvent::Built {
            device,
            title,
            built,
        } = ev
        else {
            unreachable!()
        };
        let line = built_line(&device, &title, built, false).unwrap();
        for marker in [
            library::AUDIT_WARNING,
            library::GATE_WARNING,
            library::KFX_WARNING,
            bookconv::kepub::RISKY_CSS_WARNING,
        ] {
            assert!(!line.contains(marker), "逐本不打 {marker}：{line}");
        }
        if line.contains("别的提醒") {
            unknown_lines += 1;
        }
    }
    assert_eq!(unknown_lines, 1, "不认识的提醒照旧逐本打");
    let lines = summary.lines();
    assert_eq!(lines.len(), 3, "体检、质量门、KFX 各一行：{lines:?}");
    assert!(
        lines[0].contains("体检：3 本书") && !lines[0].contains("《"),
        "{}",
        lines[0]
    );
    assert!(
        lines[1].contains("质量门未过：2 本书")
            && lines[1].contains("《丁》")
            && lines[1].contains("《戊》"),
        "{}",
        lines[1]
    );
    assert!(
        lines[2].contains("KFX：1 本书") && lines[2].contains("《己》"),
        "{}",
        lines[2]
    );
}

/// 同一本书在三台上都没过质量门：算 1 本，列出三台；不会把别的书挤出前 5 个名额。
#[test]
fn warning_summary_groups_devices_of_the_same_book() {
    let mut summary = WarningSummary::default();
    for title in ["丙", "乙"] {
        for dev in ["ireader", "kindle", "kobo"] {
            summary.observe(&BuildEvent::Built {
                device: dev.into(),
                title: title.into(),
                built: Built::Written {
                    path: PathBuf::from(format!("/tmp/{dev}/{title}.epub")),
                    warnings: vec![library::GATE_WARNING.to_string()],
                },
            });
        }
    }
    let lines = summary.lines();
    assert_eq!(lines.len(), 1, "{lines:?}");
    assert!(lines[0].contains("质量门未过：2 本书"), "{}", lines[0]);
    assert!(
        lines[0].contains("《丙》[ireader、kindle、kobo]")
            && lines[0].contains("《乙》[ireader、kindle、kobo]"),
        "{}",
        lines[0]
    );
}

/// 质量门、KFX 汇总最多列 5 本书名，多的写「等 N 本」。
#[test]
fn warning_summary_lists_at_most_five_titles() {
    let mut summary = WarningSummary::default();
    for i in 0..7 {
        summary.observe(&BuildEvent::Built {
            device: "kobo".into(),
            title: format!("书{i}"),
            built: Built::Same {
                path: PathBuf::from(format!("/tmp/书{i}.kepub.epub")),
                warnings: vec![library::GATE_WARNING.to_string()],
            },
        });
    }
    let lines = summary.lines();
    assert_eq!(lines.len(), 1, "{lines:?}");
    assert!(lines[0].contains("7 本书"), "{}", lines[0]);
    assert!(lines[0].contains("等 7 本"), "{}", lines[0]);
    assert_eq!(lines[0].matches('《').count(), 5, "{}", lines[0]);
}

/// `list` 把生成记录里的 KEPUB CSS 提醒（`⚠ `）照原文打出来，`✗`、`?` 照旧。
#[test]
fn list_shows_kepub_css_reminder_from_record() {
    let audit = [
        "✗ 行内 style margin-left（px，list）×2：掌阅没处理".to_string(),
        "? text-decoration（keyword，inline）×55".to_string(),
        "⚠ KEPUB：CSS 里 div 可能作用到新加的包装或 koboSpan（只提醒，没改 CSS）".to_string(),
    ];
    let lines = audit_lines(&audit);
    assert!(
        lines.iter().any(|l| l
            == "提醒 ⚠ KEPUB：CSS 里 div 可能作用到新加的包装或 koboSpan（只提醒，没改 CSS）"),
        "{lines:?}"
    );
    assert!(lines[0].starts_with("体检 ✗"), "{lines:?}");
    assert!(lines.iter().any(|l| l.starts_with("体检 ? 没验证过 1 种")), "{lines:?}");
}

/// 设备从没接上到接上：这台记着的失败忘掉（下一轮重试）；别的设备的、一直接着的不动。
#[test]
fn failures_are_forgotten_when_a_device_comes_back() {
    let devices = [
        profile::get("ireader").unwrap(),
        profile::get("kindle").unwrap(),
    ];
    let memo = || -> FailMemo {
        devices
            .iter()
            .map(|d| {
                (
                    ("书".to_string(), d.id.clone()),
                    ("指纹".to_string(), OriginalState::Present),
                )
            })
            .collect()
    };
    let left = |f: &FailMemo| -> Vec<String> {
        let mut v: Vec<String> = f.keys().map(|(_, d)| d.clone()).collect();
        v.sort();
        v
    };
    let mut f = memo();
    forget_reconnected(&mut f, &devices, Some(&[true, true]), &[true, true]);
    assert_eq!(left(&f), ["ireader", "kindle"], "一直接着：都记着");
    forget_reconnected(&mut f, &devices, None, &[true, true]);
    assert_eq!(left(&f).len(), 2, "第一轮：没有上一轮可比");
    forget_reconnected(&mut f, &devices, Some(&[false, true]), &[true, true]);
    assert_eq!(left(&f), ["kindle"], "掌阅重新接上");
    let mut f = memo();
    forget_reconnected(&mut f, &devices, Some(&[true, false]), &[true, false]);
    assert_eq!(left(&f), ["ireader", "kindle"], "Kindle 还没接上，留着");
}

/// 空间不够停下的设备：空间明显变多了才再跑；书库变了、重新接上（或没接上）就忘掉，拔掉后报过的提示也忘掉；不清失败表。
#[test]
fn space_held_waits_for_freed_space() {
    let devices = [profile::get("ireader").unwrap()];
    let mb = 1024 * 1024;
    let mut held: HashMap<String, Option<u64>> = [("ireader".to_string(), Some(100 * mb))]
        .into_iter()
        .collect();
    let mut rep: HashSet<String> = ["空间:ireader".to_string()].into_iter().collect();
    let at = |n: u64| move |_: &Profile| Some(n);
    let go =
        |held: &mut HashMap<_, _>, rep: &mut HashSet<_>, ch, last: &[bool], now: &[bool], n| {
            space_freed(held, rep, ch, &devices, Some(last), now, at(n))
        };
    assert!(!go(&mut held, &mut rep, false, &[true], &[true], 100 * mb));
    assert!(
        !go(&mut held, &mut rep, false, &[true], &[true], 101 * mb),
        "多 1MB 不算"
    );
    assert!(go(&mut held, &mut rep, false, &[true], &[true], 150 * mb));
    assert!(
        held.contains_key("ireader"),
        "是否放行由 build_all 再查一次决定"
    );
    assert!(
        !go(&mut held, &mut rep, true, &[true], &[true], 0),
        "书库变了：忘掉"
    );
    assert!(
        held.is_empty() && rep.contains("空间:ireader"),
        "接着时提示照旧只报一次"
    );
    held.insert("ireader".into(), Some(0));
    assert!(!go(&mut held, &mut rep, false, &[true], &[false], 999 * mb));
    assert!(
        held.is_empty() && rep.is_empty(),
        "拔掉就忘掉，接上重查、再报"
    );
}

/// 一轮里传的途中掉线、睡眠间隔里又插回：开头看还是接着，也要忘掉失败重传（以前 last_live 只看开头，永远不重试）。
#[test]
fn dropped_mid_round_is_retried() {
    let devices = [profile::get("ireader").unwrap()];
    let mut f: FailMemo = [(
        ("书".to_string(), "ireader".to_string()),
        ("指纹".to_string(), OriginalState::Present),
    )]
    .into_iter()
    .collect();
    let mut last = vec![true];
    mark_dropped(Some(&mut last), &devices, &["ireader".to_string()]);
    assert_eq!(last, [false]);
    forget_reconnected(&mut f, &devices, Some(&last), &[true]);
    assert!(f.is_empty(), "重新接上：失败忘掉");
    assert_ne!(Some(&last), Some(&vec![true]), "live 变了：这一轮会跑");
}
