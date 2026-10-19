//! Tab 补全（2026-10-19 用户要）：`booklib __complete <种类> [当前的词]`，给 shell 的补全脚本调，不在帮助里列。
//! - `fish`：打印 fish 的补全脚本（`install.sh` 装到 fish 的 completions 目录；命令、选项从 [`COMMANDS`] 表生成）
//! - `devices`：阅读模式 id（`--device=` 的值）
//! - `paths 词`：目录和 `.epub`、`.cbz`，当前目录下的和书根目录下的（`track` 的相对路径两处都认）
//! - `tracked 词`：跟踪着的条目（`untrack` 的），书根目录里的写成相对它的路径
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
            let root = open().and_then(|l| l.books_root());
            for (c, d) in paths(tok, root.as_deref()) {
                println!("{c}\t{d}");
            }
        }
        Some("tracked") => {
            let Some(lib) = open() else { return };
            let root = lib.books_root();
            for e in lib.tracked_entries() {
                let shown = root.as_ref().and_then(|r| e.path.strip_prefix(r).ok()).filter(|r| !r.as_os_str().is_empty()).unwrap_or(&e.path);
                println!("{}\t{}", shown.display(), if e.file { "跟踪的书" } else { "跟踪的目录" });
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
/// 以 `/`、`~`、`./` 开头的只按它本身找。隐藏文件只在词以 `.` 开头时列。
fn paths(tok: &str, root: Option<&Path>) -> Vec<(String, &'static str)> {
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
    let mut bases = vec![(here, "目录", "书")];
    if let Some(r) = root.filter(|_| !(tok.starts_with('/') || tok.starts_with('~') || tok.starts_with("./"))) {
        bases.push((r.join(dir), "书根目录下的目录", "书根目录下的书"));
    }
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for (base, dir_desc, book_desc) in bases {
        let Ok(rd) = std::fs::read_dir(&base) else { continue };
        let mut items: Vec<(String, &'static str)> = rd
            .flatten()
            .filter_map(|e| {
                let n = e.file_name().into_string().ok()?;
                if !n.starts_with(name) || (n.starts_with('.') && !name.starts_with('.')) {
                    return None;
                }
                let meta = std::fs::metadata(e.path()).ok()?;
                if meta.is_dir() {
                    Some((format!("{dir}{n}/"), dir_desc))
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
        s += &format!("complete -c booklib -n __fish_use_subcommand -a {} -d {}\n", c.name, q(c.brief));
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
            "track" => "-a '(booklib __complete paths (commandline -ct))'",
            "untrack" => "-a '(booklib __complete tracked (commandline -ct))'",
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
        let got = paths("优秀漫画/", Some(&root));
        assert_eq!(got, vec![("优秀漫画/死亡筆記/".to_string(), "书根目录下的目录"), ("优秀漫画/罗杰疑案.epub".to_string(), "书根目录下的书")]);
        assert_eq!(paths("优秀漫画/死", Some(&root)).len(), 1);
        // 绝对路径只按它本身找
        let abs = format!("{}/优秀漫画/罗", root.display());
        assert_eq!(paths(&abs, Some(&root)), vec![(format!("{}/优秀漫画/罗杰疑案.epub", root.display()), "书")]);
    }

    #[test]
    fn fish_script_covers_every_command() {
        let s = fish_script();
        for c in COMMANDS {
            assert!(s.contains(&format!("-a {} -d", c.name)), "{}", c.name);
        }
        assert!(s.contains("__complete paths") && s.contains("__complete tracked") && s.contains("__complete devices"));
        assert_eq!(s.matches("-l watch").count(), 1);
    }
}
