//! `generate` 各段的小测试（拆分自 `generate.rs`）：记录删除、大小写别名、`.sdr`、设备目录名、KFX id。

#[cfg(test)]
mod remove_tests {
    use super::super::fsops::{carry_sidecar, case_twin, copy_dir, distinct_on_disk, remove_product};
    use super::super::placement::device_safe_rel;

    /// 开着挂载检查、挂载点只是普通目录（和上级同一个文件系统，拔掉后留下的空挂载点）而里面正好有同名目录：
    /// 不当成接上，产物不删、记录留进 orphans。
    #[test]
    fn unmounted_point_with_same_dir_keeps_orphan() {
        use super::super::types::{Placement, State};
        use crate::deliver::DeviceEnv;
        use crate::session::Devices;
        let d = tempfile::tempdir().unwrap();
        let usbms = d.path().join("media");
        let root = usbms.join("KOBOeReader/书");
        std::fs::create_dir_all(&root).unwrap();
        let f = root.join("书.kepub.epub");
        std::fs::write(&f, b"x").unwrap();
        let env = DeviceEnv {
            devices_base: d.path().join("devices"),
            mtp_base: d.path().join("mtp"),
            mount_check: true,
            usbms_base: usbms.clone(),
        };
        let mut state = State::default();
        let p = Placement {
            path: f.clone(),
            root: root.clone(),
        };
        state.delete_into_orphans(vec![p.clone()], &Devices::new(env.clone()));
        assert!(f.exists(), "没真挂上：不删");
        assert!(state.orphans == vec![p.clone()], "记录留进 orphans");
        // 关了挂载检查（测试用普通目录当设备）：根目录在就当接上，删掉
        let mut state = State::default();
        let off = DeviceEnv {
            mount_check: false,
            ..env
        };
        state.delete_into_orphans(vec![p], &Devices::new(off));
        assert!(!f.exists() && state.orphans.is_empty());
    }

    /// 旧记录 `…/documents`、设备上（不分大小写）叫 `Documents`：记录原样挪过去，不碰文件；再调一次不变。
    /// 新路径不在（大小写敏感）或根目录不是只差大小写：不换。
    #[test]
    fn rebase_case_root_only_when_new_path_exists() {
        use super::super::types::{Placement, State, StateEntry};
        let d = tempfile::tempdir().unwrap();
        let old_root = d.path().join("documents");
        std::fs::create_dir_all(old_root.join("子目录")).unwrap();
        let old_file = old_root.join("子目录/书.epub");
        std::fs::write(&old_file, b"x").unwrap();
        let new_root = d.path().join("Documents");
        #[cfg(unix)]
        std::os::unix::fs::symlink(&old_root, &new_root).unwrap();
        #[cfg(not(unix))]
        std::fs::create_dir_all(&new_root).unwrap();

        let mut state = State::default();
        state.books.insert(
            "b".into(),
            StateEntry {
                path: old_file.clone(),
                root: old_root.clone(),
                fingerprint: "f".into(),
                sha: String::new(),
                old: vec![Placement {
                    path: old_root.join("旧.epub"),
                    root: old_root.clone(),
                }],
                audit: Vec::new(),
            },
        );
        state.orphans.push(Placement {
            path: old_root.join("删了.epub"),
            root: old_root.clone(),
        });
        // 旧位置的文件都在（模拟不分大小写：新路径读得到）
        std::fs::write(old_root.join("旧.epub"), b"y").unwrap();
        std::fs::write(old_root.join("删了.epub"), b"z").unwrap();
        assert!(state.rebase_case_root(&new_root));
        let e = &state.books["b"];
        assert_eq!(e.root, new_root);
        assert_eq!(e.path, new_root.join("子目录/书.epub"));
        assert_eq!(e.old[0].path, new_root.join("旧.epub"));
        assert_eq!(state.orphans[0].path, new_root.join("删了.epub"));
        assert!(old_file.exists(), "只改记录，不碰文件");
        assert!(!state.rebase_case_root(&new_root), "幂等");

        // 根目录不是只差大小写：不换
        let mut state2 = State::default();
        state2.books.insert(
            "b".into(),
            StateEntry {
                path: old_file.clone(),
                root: old_root.clone(),
                fingerprint: String::new(),
                sha: String::new(),
                old: Vec::new(),
                audit: Vec::new(),
            },
        );
        assert!(!state2.rebase_case_root(&d.path().join("Books2")));

        // 新路径上的文件不在（两个真目录）：不换
        let other = d.path().join("DOCUMENTS");
        std::fs::create_dir_all(&other).unwrap();
        assert!(!state2.rebase_case_root(&other));
        assert_eq!(state2.books["b"].path, old_file);
    }

    #[test]
    fn case_alias_paths_are_not_deleted() {
        use super::super::types::{Placement, State};
        use crate::deliver::DeviceEnv;
        use crate::session::Devices;
        let d = tempfile::tempdir().unwrap();
        let old_root = d.path().join("documents");
        let new_root = d.path().join("Documents");
        std::fs::create_dir_all(&old_root).unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(&old_root, &new_root).unwrap();
        #[cfg(not(unix))]
        std::fs::create_dir_all(&new_root).unwrap();
        let file = old_root.join("书.epub");
        std::fs::write(&file, b"x").unwrap();
        let dev = Devices::new(DeviceEnv {
            devices_base: d.path().join("devices"),
            mtp_base: d.path().join("mtp"),
            mount_check: false,
            usbms_base: d.path().join("media"),
        });
        let state = State::default();
        let old = Placement {
            path: file.clone(),
            root: old_root,
        };
        let cur = new_root.join("书.epub");
        let left = state.delete_placements(vec![old], Some(&cur), &dev);
        assert!(left.is_empty(), "记录丢掉");
        assert!(file.exists(), "只差大小写的位置不删文件");
        assert!(cur.exists());
    }

    /// 只有 MTP 直写；USB 大容量存储（Kobo）和电脑上都经临时文件。
    #[test]
    fn only_mtp_writes_direct() {
        use super::super::flow::direct_write;
        use crate::deliver::Target;
        use std::path::PathBuf;
        let dir = |r: &str, staged| Target::Dir {
            root: PathBuf::from(r),
            mount: None,
            staged,
        };
        assert!(direct_write(&dir(
            "/run/user/1/mtp/k/内部存储/documents",
            false
        )));
        assert!(!direct_write(&dir("/run/media/u/KOBOeReader/书", true)));
        assert!(!direct_write(&Target::Local));
    }

    /// 删书：Kindle 书旁的 `.sdr` 一起删（不然目录删不空）；同名的书换了目录、`.sdr` 没挪过去的不删。
    /// `carry_sidecar` 改不了名时拷过去。
    #[test]
    fn sidecar_removed_with_book_and_copied_when_rename_fails() {
        use super::super::types::{Placement, State, StateEntry};
        use crate::deliver::DeviceEnv;
        use crate::session::Devices;
        let d = tempfile::tempdir().unwrap();
        let root = d.path().join("out");
        let sub = root.join("漫画");
        std::fs::create_dir_all(sub.join("a.sdr")).unwrap();
        std::fs::write(sub.join("a.sdr/a.yjf"), b"p").unwrap();
        std::fs::write(sub.join("a.kfx"), b"k").unwrap();
        let dev = Devices::new(DeviceEnv {
            devices_base: d.path().join("devices"),
            mtp_base: d.path().join("mtp"),
            mount_check: false,
            usbms_base: d.path().join("media"),
        });
        let p = Placement {
            path: sub.join("a.kfx"),
            root: root.clone(),
        };
        // 换目录、同名：不删 .sdr
        let other = root.join("新/a.kfx");
        std::fs::create_dir_all(sub.join("x")).unwrap(); // 让旧目录删不空，看得见 .sdr 还在
        State::default().delete_placements(vec![p.clone()], Some(&other), &dev);
        assert!(sub.join("a.sdr/a.yjf").exists());
        // 删书：一起删，目录删空；别的目录里有同名的另一本书不碍事
        std::fs::write(sub.join("a.kfx"), b"k").unwrap();
        std::fs::remove_dir(sub.join("x")).unwrap();
        let mut st = State::default();
        st.books.insert(
            "别的".into(),
            StateEntry::file(
                root.join("名著/a.kfx"),
                root.clone(),
                "f".into(),
                Vec::new(),
            ),
        );
        st.delete_placements(vec![p], None, &dev);
        assert!(!sub.exists(), "书和 .sdr 都删了，目录删空");
        // 拷目录
        let (a, b) = (d.path().join("s.sdr"), d.path().join("t.sdr"));
        std::fs::create_dir_all(a.join("子")).unwrap();
        std::fs::write(a.join("子/f"), b"1").unwrap();
        copy_dir(&a, &b).unwrap();
        assert_eq!(std::fs::read(b.join("子/f")).unwrap(), b"1");
    }

    /// 书名只改了大小写（`Abc.epub` → `abc.epub`）：不分大小写的文件系统上目录里只剩新名字，旧位置不能删（删的就是现在这份）；
    /// 分大小写的文件系统上两个都在，旧的照删。
    #[test]
    fn case_only_rename_keeps_current() {
        use super::super::types::{Placement, State};
        use crate::deliver::DeviceEnv;
        use crate::session::Devices;
        let d = tempfile::tempdir().unwrap();
        let root = d.path().join("out");
        std::fs::create_dir_all(&root).unwrap();
        let (old, cur) = (root.join("Abc.epub"), root.join("abc.epub"));
        let dev = Devices::new(DeviceEnv {
            devices_base: d.path().join("devices"),
            mtp_base: d.path().join("mtp"),
            mount_check: false,
            usbms_base: d.path().join("media"),
        });
        let p = Placement {
            path: old.clone(),
            root: root.clone(),
        };
        // 模拟不分大小写：改名后目录里只有 abc.epub，删 Abc.epub 会删到它
        std::fs::write(&cur, b"new").unwrap();
        let state = State::default();
        let left = state.delete_placements(vec![p.clone()], Some(&cur), &dev);
        assert!(left.is_empty(), "旧名字的记录丢掉");
        assert!(
            case_twin(&old, &cur) && distinct_on_disk(&old, &[&cur]) == Some(false)
        );
        assert!(cur.exists());
        // 列不了目录（目录不在）：记着下次再看，不当成同一个文件丢掉
        let gone = Placement {
            path: root.join("没有的目录/Abc.epub"),
            root: p.root.clone(),
        };
        let twin = root.join("没有的目录/abc.epub");
        assert_eq!(
            state.delete_placements(vec![gone], Some(&twin), &dev).len(),
            1
        );
        // 分大小写：两个都在，旧的删掉
        std::fs::write(&old, b"old").unwrap();
        let left = state.delete_placements(vec![p], Some(&cur), &dev);
        assert!(left.is_empty() && !old.exists() && cur.exists());
    }

    /// 缺省的 usbms_base 两次运行之间在 `/run/media/$USER` 和 `/media/$USER` 之间换了：另一处下的旧产物也认得是设备上的，
    /// 没挂上就不当成已删（以前认不出，删不到就当删了，记录丢掉、书留在设备上）。
    #[test]
    fn unplugged_knows_both_media_dirs() {
        use crate::deliver::DeviceEnv;
        use crate::session::Devices;
        let u = "booklib-test-no-such-user";
        let dev = Devices::new(DeviceEnv {
            devices_base: std::path::PathBuf::from("/nonexistent/devices"),
            mtp_base: std::path::PathBuf::from("/nonexistent/mtp"),
            mount_check: true,
            usbms_base: std::path::Path::new("/run/media").join(u),
        });
        let root = std::path::Path::new("/media").join(u).join("KOBOeReader");
        assert!(dev.unplugged(&root.join("a.epub"), &root));
        let dev = Devices::new(DeviceEnv {
            devices_base: std::path::PathBuf::from("/nonexistent/devices"),
            mtp_base: std::path::PathBuf::from("/nonexistent/mtp"),
            mount_check: true,
            usbms_base: std::path::Path::new("/media").join(u),
        });
        let root = std::path::Path::new("/run/media")
            .join(u)
            .join("KOBOeReader");
        assert!(dev.unplugged(&root.join("a.epub"), &root));
    }

    /// usbms_base 在 mtp_base 下面（关了挂载检查时的缺省）：挂载点按 usbms_base 算（`<usbms>/KOBOeReader`），不是 `<mtp>/usbms`。
    /// MTP 下的也要真挂着才删。
    #[test]
    fn unplugged_uses_the_longer_base() {
        use crate::deliver::DeviceEnv;
        use crate::session::Devices;
        let d = tempfile::tempdir().unwrap();
        let mtp = d.path().join("mtp");
        let root = mtp.join("usbms/KOBOeReader/书");
        std::fs::create_dir_all(&root).unwrap();
        let dev = Devices::new(DeviceEnv {
            devices_base: mtp.join("devices"),
            mtp_base: mtp.clone(),
            mount_check: true,
            usbms_base: mtp.join("usbms"),
        });
        // 普通目录：两个 base 下都算没挂上（mtp 下的挂载点也只是普通目录）
        assert!(dev.unplugged(&root.join("a.epub"), &root));
        let mroot = mtp.join("iReader/内部存储/documents");
        std::fs::create_dir_all(&mroot).unwrap();
        assert!(dev.unplugged(&mroot.join("a.epub"), &mroot));
        // 不在设备上的不管
        assert!(!dev.unplugged(&d.path().join("out/a.epub"), d.path()));
        // 关了挂载检查：根目录在就算接上
        let off = Devices::new(DeviceEnv {
            devices_base: mtp.join("devices"),
            mtp_base: mtp.clone(),
            mount_check: false,
            usbms_base: mtp.join("usbms"),
        });
        assert!(!off.unplugged(&root.join("a.epub"), &root));
        assert!(off.unplugged(
            &mtp.join("usbms/没有/书/a.epub"),
            &mtp.join("usbms/没有/书")
        ));
    }

    #[test]
    fn remove_product_reports_gone_and_missing() {
        let d = tempfile::tempdir().unwrap();
        let f = d.path().join("书.epub");
        std::fs::write(&f, b"x").unwrap();
        assert!(remove_product(&f));
        assert!(!f.exists());
        assert!(remove_product(&f), "本来就不在：算删掉了");
        assert!(
            remove_product(&d.path().join("没有的目录/书.epub")),
            "目录都不在：算删掉了"
        );
    }

    #[test]
    fn sidecar_follows_a_moved_product() {
        let d = tempfile::tempdir().unwrap();
        let (a, b) = (d.path().join("漫画"), d.path().join("优秀漫画"));
        std::fs::create_dir_all(a.join("书 第01卷.sdr")).unwrap();
        std::fs::write(a.join("书 第01卷.sdr/进度.yjf"), b"p").unwrap();
        std::fs::create_dir_all(&b).unwrap();
        carry_sidecar(&a.join("书 第01卷.kfx"), &b.join("书 第01卷.kfx"));
        assert_eq!(
            std::fs::read(b.join("书 第01卷.sdr/进度.yjf")).unwrap(),
            b"p"
        );
        assert!(!a.join("书 第01卷.sdr").exists());
        // 改了名的不挪；新位置已有的不动
        std::fs::create_dir_all(a.join("甲.sdr")).unwrap();
        carry_sidecar(&a.join("甲.kfx"), &b.join("乙.kfx"));
        assert!(a.join("甲.sdr").is_dir() && !b.join("乙.sdr").exists());
        std::fs::create_dir_all(b.join("甲.sdr")).unwrap();
        std::fs::write(a.join("甲.sdr/x"), b"old").unwrap();
        carry_sidecar(&a.join("甲.kfx"), &b.join("甲.kfx"));
        assert!(a.join("甲.sdr/x").exists() && !b.join("甲.sdr/x").exists());
    }

    #[test]
    fn device_dir_names_made_safe_only_where_needed() {
        use std::path::{Path, PathBuf};
        assert_eq!(
            device_safe_rel(Path::new("侦探悬疑/亚洲侦探")),
            PathBuf::from("侦探悬疑/亚洲侦探"),
            "能用的原样"
        );
        assert_eq!(
            device_safe_rel(Path::new("科幻: 合集/上册.")),
            PathBuf::from("科幻_ 合集/上册")
        );
        assert_eq!(device_safe_rel(Path::new("")), PathBuf::from(""));
    }
}

#[cfg(test)]
mod kfx_id_tests {
    use super::super::produce::kfx_id;

    #[test]
    fn book_id_is_parsed_whole() {
        assert_eq!(kfx_id("0123456789ab"), Some(0x0123_4567_89ab));
        assert_eq!(kfx_id(""), None);
        assert_eq!(kfx_id("xyz"), None);
        assert_eq!(kfx_id("0123456789abcdef0"), None);
    }
}
