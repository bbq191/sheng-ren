//! Tab 补全（2026-10-19 要）：`booklib __complete <种类> [当前的词]`，给 shell 的补全脚本调，不在帮助里列。
//! - `fish`：打印 fish 的补全脚本（`install.sh` 装到 fish 的 completions 目录；命令、选项从 [`COMMANDS`] 表生成）
//! - `devices`：阅读模式 id（`--device=` 的值）
//! - `paths 词`：目录和 `.epub`、`.cbz`，当前目录下的和书根目录下的（`track` 的相对路径两处都认）；已在跟踪的不列。
//!   词是空的且有书根目录时只列书根目录第一层还没跟踪的目录（2026-11-03 要：track 先挑顶层目录；命令本身不限制）
//! - `tracked 命令行的词…`：跟踪着的条目（`untrack` 的），书根目录里的写成相对它的路径；命令行里有 `--device=` 时
//!   只列给这几台的（含给全部设备的）（2026-11-03 要）
//!
//! 只读：书库目录不在时不打开（打开会建出目录），出错一律不出声（补全时不该往终端上打字）。

use super::COMMANDS;
use library::Library;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

pub fn run(args: &[String]) {
    let tok = args.get(1).map(String::as_str).unwrap_or("");
    match args.first().map(String::as_str) {
        Some("fish") => print!("{}", fish_script()),
        Some("devices") => {
            if let Some(lib) = open() {
                for p in lib.devices().iter() {
                    println!("{}\t{}", p.id, p.name);
                }
            }
            println!("all\t全部设备");
        }
        Some("paths") => {
            let lib = open();
            let root = lib.as_ref().and_then(|l| l.books_root());
            // 只藏这条命令 --device 给的设备已跟踪的（2026-11-06：别台跟踪过的也藏，进不了子目录）
            let want = device_filter(args.get(2..).unwrap_or(&[]));
            let tracked: HashSet<PathBuf> = lib
                .map(|l| {
                    let es = l.tracked_entries();
                    es.iter()
                        .filter(|e| for_devices(&e.devices, &want))
                        .map(|e| canon(&e.path))
                        .collect()
                })
                .unwrap_or_default();
            for (c, d) in paths(tok, root.as_deref(), &tracked) {
                println!("{c}\t{d}");
            }
        }
        Some("tracked") => {
            let Some(lib) = open() else { return };
            let root = lib.books_root();
            let want = device_filter(&args[1..]);
            for e in lib.tracked_entries() {
                if !for_devices(&e.devices, &want) {
                    continue;
                }
                let shown = root
                    .as_ref()
                    .and_then(|r| e.path.strip_prefix(r).ok())
                    .filter(|r| !r.as_os_str().is_empty())
                    .unwrap_or(&e.path);
                println!(
                    "{}\t{}",
                    shown.display(),
                    if e.file {
                        "跟踪的书"
                    } else {
                        "跟踪的目录"
                    }
                );
            }
        }
        _ => {}
    }
}

/// 缺省书库（`$BOOKLIB_DIR` 或 `~/.local/share/booklib`）；还不存在就不打开。
fn open() -> Option<Library> {
    let root = Library::default_root();
    root.is_dir().then(|| Library::open(&root).ok()).flatten()
}

/// `词` 补全成目录（带 `/`，shell 不加空格、接着往下补）和书。先列当前目录下的，再列书根目录下的（同名的只列一次）；
/// 以 `/`、`~`、`./` 开头的只按它本身找。隐藏文件只在词以 `.` 开头时列。`tracked`（已规范化）里的不列。
/// 词是空的且有书根目录时只列书根目录第一层的目录（不列当前目录的、不列书）。
fn paths(
    tok: &str,
    root: Option<&Path>,
    tracked: &HashSet<PathBuf>,
) -> Vec<(String, &'static str)> {
    let top_only = tok.is_empty() && root.is_some();
    let (dir, name) = match tok.rfind('/') {
        Some(i) => (&tok[..=i], &tok[i + 1..]),
        None => ("", tok),
    };
    let home = std::env::var_os("HOME").map(PathBuf::from);
    let here = match (dir.strip_prefix("~/"), &home) {
        (Some(rest), Some(h)) => h.join(rest),
        _ if dir.is_empty() => PathBuf::from("."),
        _ => PathBuf::from(dir),
    };
    let mut bases = if top_only {
        Vec::new()
    } else {
        vec![(here, "目录", "书")]
    };
    if let Some(r) =
        root.filter(|_| !(tok.starts_with('/') || tok.starts_with('~') || tok.starts_with("./")))
    {
        bases.push((r.join(dir), "书根目录下的目录", "书根目录下的书"));
    }
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for (base, dir_desc, book_desc) in bases {
        let Ok(rd) = std::fs::read_dir(&base) else {
            continue;
        };
        let mut items: Vec<(String, &'static str)> = rd
            .flatten()
            .filter_map(|e| {
                let n = e.file_name().into_string().ok()?;
                if !n.starts_with(name) || (n.starts_with('.') && !name.starts_with('.')) {
                    return None;
                }
                let meta = std::fs::metadata(e.path()).ok()?;
                // 目录跟踪了也列：还要往下补子目录
                if !meta.is_dir() && tracked.contains(&canon(&e.path())) {
                    return None;
                }
                if meta.is_dir() {
                    Some((format!("{dir}{n}/"), dir_desc))
                } else if top_only {
                    None
                } else {
                    let ext = Path::new(&n).extension()?.to_str()?.to_ascii_lowercase();
                    matches!(ext.as_str(), "epub" | "cbz").then(|| (format!("{dir}{n}"), book_desc))
                }
            })
            .collect();
        items.sort();
        out.extend(items.into_iter().filter(|(c, _)| seen.insert(c.clone())));
    }
    out
}

/// 命令行词里 `--device=a,b`（或 `--device a,b`）给的模式 id；没写或写了 `all` 是空＝不过滤。
fn device_filter(words: &[String]) -> Vec<String> {
    let mut out = Vec::new();
    let mut it = words.iter();
    while let Some(w) = it.next() {
        let v = match w.strip_prefix("--device") {
            Some(r) if r.starts_with('=') => r[1..].to_string(),
            Some("") => it.next().cloned().unwrap_or_default(),
            _ => continue,
        };
        out.extend(v.split(',').filter(|d| !d.is_empty()).map(String::from));
    }
    if out.iter().any(|d| d == "all") {
        out.clear();
    }
    out
}

/// 跟踪条目（给 `entry` 这几台，空＝全部）该不该列给 `want`（空＝不过滤）。
fn for_devices(entry: &[String], want: &[String]) -> bool {
    want.is_empty() || entry.is_empty() || entry.iter().any(|d| want.contains(d))
}

/// 比较用的路径：能规范化就规范化（`./漫画` 和书根下的绝对路径算同一个），不能就照原样。
fn canon(p: &Path) -> PathBuf {
    std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf())
}

/// fish 补全脚本。命令和选项照 [`COMMANDS`] 表；路径、设备、跟踪条目运行时调 `booklib __complete` 取。
fn fish_script() -> String {
    let q = |s: &str| format!("'{}'", s.replace('\\', "\\\\").replace('\'', "\\'"));
    let mut s = String::from(
        "# booklib 的 fish 补全：./install.sh 用 `booklib __complete fish` 生成，每次安装重写（别手改）\n\
         complete -c booklib -e\n\
         complete -c booklib -f\n\
         complete -c booklib -l library -r -F -d 书库目录\n\
         complete -c booklib -s h -l help -d 帮助\n\
         complete -c booklib -s V -l version -d 版本\n\
         complete -c booklib -n __fish_use_subcommand -a help -d 帮助\n",
    );
    for c in COMMANDS {
        let when = q(&format!("__fish_seen_subcommand_from {}", c.name));
        s += &format!(
            "complete -c booklib -n __fish_use_subcommand -a {} -d {}\n",
            c.name,
            q(c.brief)
        );
        for f in c.flags {
            // 既是开关又是选项的（sync --watch[=秒]）按选项写一次
            if !c.opts.contains(f) {
                s += &format!("complete -c booklib -n {when} -l {f}\n");
            }
        }
        for o in c.opts {
            let value = match *o {
                "device" => "-x -a '(booklib __complete devices)'",
                "root" => "-r -F",
                _ => "-x",
            };
            s += &format!("complete -c booklib -n {when} -l {o} {value}\n");
        }
        let args = match c.name {
            "track" => "-a '(booklib __complete paths (commandline -ct) (commandline -opc))'",
            "untrack" => "-a '(booklib __complete tracked (commandline -opc))'",
            "add" | "dedupe" | "meta" => "-F",
            _ => continue,
        };
        s += &format!("complete -c booklib -n {when} {args}\n");
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_lists_dirs_and_books_from_here_and_the_root() {
        let d = tempfile::tempdir().unwrap();
        let root = d.path().join("book");
        std::fs::create_dir_all(root.join("优秀漫画/死亡筆記")).unwrap();
        std::fs::write(root.join("优秀漫画/罗杰疑案.epub"), b"").unwrap();
        std::fs::write(root.join("优秀漫画/说明.txt"), b"").unwrap();
        std::fs::write(root.join("优秀漫画/.隐藏.epub"), b"").unwrap();
        let none = HashSet::new();
        let got = paths("优秀漫画/", Some(&root), &none);
        assert_eq!(
            got,
            vec![
                ("优秀漫画/死亡筆記/".to_string(), "书根目录下的目录"),
                ("优秀漫画/罗杰疑案.epub".to_string(), "书根目录下的书")
            ]
        );
        assert_eq!(paths("优秀漫画/死", Some(&root), &none).len(), 1);
        // 绝对路径只按它本身找
        let abs = format!("{}/优秀漫画/罗", root.display());
        assert_eq!(
            paths(&abs, Some(&root), &none),
            vec![(format!("{}/优秀漫画/罗杰疑案.epub", root.display()), "书")]
        );
    }

    #[test]
    fn paths_hides_tracked_and_empty_word_lists_only_untracked_top_dirs() {
        let d = tempfile::tempdir().unwrap();
        let root = d.path().join("book");
        std::fs::create_dir_all(root.join("漫画/死亡筆記")).unwrap();
        std::fs::create_dir_all(root.join("文字/科幻")).unwrap();
        std::fs::create_dir_all(root.join("杂志")).unwrap();
        std::fs::write(root.join("飘.epub"), b"").unwrap();
        std::fs::write(root.join("文字/三体.epub"), b"").unwrap();
        let tracked: HashSet<PathBuf> = [
            canon(&root.join("杂志")),
            canon(&root.join("文字/三体.epub")),
        ]
        .into();
        // 空词：只有书根第一层的目录，不列书；跟踪着的目录也列（要往下补）
        assert_eq!(
            paths("", Some(&root), &tracked),
            vec![
                ("文字/".to_string(), "书根目录下的目录"),
                ("杂志/".to_string(), "书根目录下的目录"),
                ("漫画/".to_string(), "书根目录下的目录")
            ]
        );
        // 往下补：书照列，跟踪着的不列
        assert_eq!(
            paths("文字/", Some(&root), &tracked),
            vec![("文字/科幻/".to_string(), "书根目录下的目录")]
        );
        assert_eq!(paths("杂", Some(&root), &tracked).len(), 1);
    }

    #[test]
    fn untrack_lists_only_entries_for_the_given_devices() {
        let w = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        let s = |v: &[&str]| w(v);
        assert_eq!(
            device_filter(&w(&["booklib", "untrack", "--device=ireader,kobo"])),
            s(&["ireader", "kobo"])
        );
        assert_eq!(
            device_filter(&w(&["booklib", "untrack", "--device", "kindle"])),
            s(&["kindle"])
        );
        assert!(device_filter(&w(&["booklib", "untrack", "--device=all"])).is_empty());
        assert!(device_filter(&w(&["booklib", "untrack", "漫画"])).is_empty());
        let ire = s(&["ireader"]);
        assert!(!for_devices(&s(&["kindle"]), &ire));
        assert!(for_devices(&s(&["ireader", "kindle"]), &ire));
        assert!(for_devices(&[], &ire));
        assert!(for_devices(&s(&["kindle"]), &[]));
    }

    #[test]
    fn fish_script_covers_every_command() {
        let s = fish_script();
        for c in COMMANDS {
            assert!(s.contains(&format!("-a {} -d", c.name)), "{}", c.name);
        }
        assert!(
            s.contains("__complete paths")
                && s.contains("__complete tracked")
                && s.contains("__complete devices")
        );
        assert_eq!(s.matches("-l watch").count(), 1);
    }
}
