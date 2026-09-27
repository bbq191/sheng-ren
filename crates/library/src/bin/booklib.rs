//! 书库命令行：入库、列出、按设备生成、删除、去重。见 `library` crate 头注释。
//!
//! 书库目录缺省 $BOOKLIB_DIR 或 ~/.local/share/booklib；产物缺省放在书库的 output/<设备>/ 下。
//! 退出码: 0 全部成功；1 用法错；2 有书处理失败（或书库打不开、没有匹配的书）。

use library::{Added, Built, Library, OriginalState, Profile, SyncEvent};
use std::ffi::OsString;
use std::path::PathBuf;

const USAGE: &str = "用法:
  booklib [--library=目录] add <文件、目录或网址>...        目录会递归找出能入库的书
  booklib [--library=目录] list [书名片段或 id...]      列出书，以及给哪些设备生成过、是否最新
  booklib [--library=目录] build --device=<设备>[,<设备>…] [--force] [--out=目录] [书名片段或 id...]
      --device 可写多次或用逗号分隔，--device=all 表示全部设备；不写书名 = 全部书
  booklib [--library=目录] track <目录>...               跟踪目录：之后 sync 把它镜像进书库
  booklib [--library=目录] untrack <目录>...             不再跟踪（已入库的书保留）
  booklib [--library=目录] sync [--prune] [--device=<设备>…] [--out=目录] [--watch[=秒]]
      新增的入库、改过的换成新版本、移动改名的认得出；原件删了的只报告，--prune 才从书库删掉
      --device 给了就接着生成（只重建有变化的）；--watch 一直运行，每隔几秒（缺省 60）检查一次
  booklib [--library=目录] remove <id>...               从书库删掉（连同产物；原件不动）。id 用 list 里显示的完整 id
  booklib [--library=目录] dedupe [目录...]             早期版本入库的书改成只存索引（在记着的位置和这些目录里找原件）
  booklib [--library=目录] devices                      列出设备（书库 profiles/ 目录里的自定义设备也算）";

fn usage_error(msg: &str) -> ! {
    if !msg.is_empty() {
        eprintln!("{msg}\n");
    }
    eprintln!("{USAGE}");
    std::process::exit(1);
}

fn fail(msg: &str) -> ! {
    eprintln!("{msg}");
    std::process::exit(2);
}

/// 解析后的命令行：`--名字=值` 选项、`--名字` 开关、其余是位置参数（`--` 之后全算位置参数）。
struct Args {
    opts: Vec<(String, String)>,
    flags: Vec<String>,
    pos: Vec<OsString>,
}

impl Args {
    fn parse() -> Args {
        let mut a = Args { opts: Vec::new(), flags: Vec::new(), pos: Vec::new() };
        let mut rest = false;
        for arg in std::env::args_os().skip(1) {
            let s = arg.to_str();
            match s {
                Some("--") if !rest => rest = true,
                Some(s) if !rest && s.starts_with("--") => match s[2..].split_once('=') {
                    Some((k, v)) if v.trim().is_empty() => usage_error(&format!("--{k}= 后面要写值")),
                    Some((k, v)) => a.opts.push((k.to_string(), v.to_string())),
                    None => a.flags.push(s[2..].to_string()),
                },
                _ => a.pos.push(arg),
            }
        }
        a
    }

    fn opt(&self, name: &str) -> Option<&str> {
        self.opts.iter().rev().find(|(k, _)| k == name).map(|(_, v)| v.as_str())
    }

    /// 只允许这个命令认识的选项和开关；`--device kindle`（空格分隔）这类写法会在这里报错，不会悄悄当成书名。
    fn check(&self, cmd: &str, opts: &[&str], flags: &[&str]) {
        let opts: Vec<&str> = opts.iter().chain(&["library"]).copied().collect();
        if let Some((k, _)) = self.opts.iter().find(|(k, _)| !opts.contains(&k.as_str())) {
            usage_error(&format!("{cmd} 不认识选项 --{k}="));
        }
        if let Some(f) = self.flags.iter().find(|f| !flags.contains(&f.as_str())) {
            let hint = if opts.contains(&f.as_str()) { format!("（要写成 --{f}=值）") } else { String::new() };
            usage_error(&format!("{cmd} 不认识 --{f}{hint}"));
        }
    }

    /// 位置参数当文字用（书名片段、id、网址）。
    fn texts(&self) -> Vec<String> {
        self.pos[1..].iter().map(|s| s.to_string_lossy().into_owned()).collect()
    }
}

fn added_line(a: &Added) -> String {
    match a {
        Added::New(m) => format!("✓ 入库 {}  {}", m.id, m.title),
        Added::Existing(m) => format!("= 已在库里 {}  {}", m.id, m.title),
    }
}

/// `--device` 可写多次、可逗号分隔；all = 全部设备。写了不存在的设备直接报用法错。
fn parse_devices<'a>(args: &Args, lib: &'a Library) -> Vec<&'a Profile> {
    let mut devices: Vec<&Profile> = Vec::new();
    for id in args.opts.iter().filter(|(k, _)| k == "device").flat_map(|(_, v)| v.split(',')).map(str::trim).filter(|v| !v.is_empty()) {
        if id == "all" {
            devices.extend(lib.devices().iter());
            continue;
        }
        match lib.devices().get(id) {
            Some(p) => devices.push(p),
            None => usage_error(&format!("没有设备 {id}，运行 booklib devices 查看")),
        }
    }
    devices.sort_by(|a, b| a.id.cmp(&b.id));
    devices.dedup_by(|a, b| a.id == b.id);
    devices
}

/// 按设备 × 书逐本生成（没变化的跳过），每本一行结果。
/// `quiet` 时不打印"已是最新"（sync 用：只报有变化的）。
fn build_all(lib: &Library, devices: &[&Profile], books: &[library::Meta], out_root: &std::path::Path, force: bool, quiet: bool, report: &mut impl FnMut(Result<String, String>)) {
    for device in devices {
        for m in books {
            report(match lib.build(m, device, out_root, force) {
                Ok(Built::Written { path, warnings }) => {
                    Ok(std::iter::once(format!("✓ [{}] {} → {}", device.id, m.title, path.display())).chain(warnings.iter().map(|w| format!("  ⚠ {w}"))).collect::<Vec<_>>().join("\n"))
                }
                Ok(Built::UpToDate(_)) if quiet => continue,
                Ok(Built::UpToDate(path)) => Ok(format!("= [{}] {} 已是最新（{}）", device.id, m.title, path.display())),
                Err(e) => Err(format!("✗ [{}] {}: {e}", device.id, m.title)),
            });
        }
    }
}

fn main() {
    let args = Args::parse();
    let Some(cmd) = args.pos.first().and_then(|c| c.to_str()).map(str::to_string) else { usage_error("") };
    match cmd.as_str() {
        "add" | "remove" | "dedupe" | "list" | "devices" | "track" | "untrack" => args.check(&cmd, &[], &[]),
        "build" => args.check(&cmd, &["device", "out"], &["force"]),
        "sync" => args.check(&cmd, &["device", "out", "watch"], &["prune", "watch"]),
        _ => usage_error(&format!("不认识的命令 {cmd}")),
    }
    let root = args.opt("library").map(PathBuf::from).unwrap_or_else(Library::default_root);
    let lib = Library::open(&root).unwrap_or_else(|e| fail(&format!("打开书库失败: {e}")));
    // 会改动书库的命令持锁到结束
    let _lock = match cmd.as_str() {
        "list" | "devices" | "sync" => None, // sync 每一轮自己加锁（--watch 时不能一直占着）
        _ => Some(lib.lock().unwrap_or_else(|e| fail(&e))),
    };
    let mut failed = 0;
    let mut report = |r: Result<String, String>| match r {
        Ok(line) => println!("{line}"),
        Err(e) => {
            failed += 1;
            eprintln!("{e}");
        }
    };
    let rest = &args.pos[1..];
    match cmd.as_str() {
        "devices" => {
            for p in lib.devices().iter() {
                let fmts: Vec<&str> = p.formats.iter().map(|f| f.ext()).collect();
                println!("{:<20} {}  屏幕 {}×{}  {}", p.id, p.name, p.screen.width, p.screen.height, fmts.join("/"));
            }
        }
        "add" => {
            if rest.is_empty() {
                usage_error("add 要给文件或网址");
            }
            for item in rest {
                let text = item.to_string_lossy().into_owned();
                let path = std::path::Path::new(item);
                let files: Vec<PathBuf> = if text.starts_with("http://") || text.starts_with("https://") {
                    report(match lib.add_url(&text) {
                        Ok(a) => Ok(added_line(&a)),
                        Err(e) => Err(format!("✗ {text}: {e}")),
                    });
                    continue;
                } else if path.is_dir() {
                    let v = library::book_files(path);
                    if v.is_empty() {
                        report(Err(format!("✗ {text}: 目录里没有能入库的书（{}）", library::SUPPORTED_EXTS.join(" / "))));
                    }
                    v
                } else {
                    vec![path.to_path_buf()]
                };
                for f in files {
                    report(lib.add_file(&f).map(|a| added_line(&a)).map_err(|e| format!("✗ {}: {e}", f.display())));
                }
            }
        }
        "list" => {
            for m in lib.select(&args.texts()) {
                println!("{}  {:<6} {}{}", m.id, m.source_format, m.title, if m.authors.is_empty() { String::new() } else { format!(" — {}", m.authors.join("、")) });
                match lib.original_state(&m) {
                    OriginalState::Missing => println!("      ✗ 原件不在了：{}（移动过就 sync 或重新 add；不要了就 remove）", m.source_path),
                    OriginalState::Touched => println!("      ⚠ 原件可能改过：{}（生成前会核对）", m.source_path),
                    OriginalState::Present | OriginalState::NotNeeded => {}
                }
                for o in lib.outputs(&m) {
                    let state = match o.fresh {
                        Some(true) => "✓ 最新",
                        Some(false) => "⚠ 过期",
                        None => "? 未知",
                    };
                    let shown = o.path.strip_prefix(lib.root()).map(|p| p.to_path_buf()).unwrap_or(o.path.clone());
                    println!("      {:<20} {state}  {}", o.device, shown.display());
                }
            }
            for id in lib.broken() {
                eprintln!("⚠ 条目 {id} 的 meta.json 读不出来（写坏了）：booklib remove {id} 删掉，或重新 add 原文件覆盖");
            }
        }
        "build" => {
            let devices = parse_devices(&args, &lib);
            if devices.is_empty() {
                usage_error("build 要用 --device= 指定设备");
            }
            let out_root = args.opt("out").map(PathBuf::from).unwrap_or_else(|| lib.root().join("output"));
            let books = lib.select(&args.texts());
            if books.is_empty() {
                fail("没有匹配的书（booklib list 查看书库）");
            }
            let force = args.flags.iter().any(|f| f == "force");
            build_all(&lib, &devices, &books, &out_root, force, false, &mut report);
        }
        "dedupe" => {
            let dirs: Vec<PathBuf> = rest.iter().map(PathBuf::from).collect();
            let mb = |b: u64| b as f64 / 1048576.0;
            report(lib.dedupe(&dirs).map(|r| {
                let mut lines = vec![format!("✓ {} 本书改成只存索引，删掉书库里的副本 {:.1} MB", r.migrated, mb(r.freed_bytes))];
                for m in &r.kept {
                    lines.push(format!("  ? 没找到原件，副本保留：{}  {}（原来在 {}）", m.id, m.title, if m.source_path.is_empty() { &m.source } else { &m.source_path }));
                }
                lines.join("\n")
            }));
        }
        "track" | "untrack" => {
            if rest.is_empty() {
                usage_error(&format!("{cmd} 要给目录"));
            }
            for d in rest {
                let d = std::path::Path::new(d);
                report(if cmd == "track" {
                    lib.track(d).map(|new| if new { format!("✓ 开始跟踪 {}（运行 booklib sync 入库）", d.display()) } else { format!("= 已在跟踪 {}", d.display()) })
                } else {
                    lib.untrack(d).map(|had| if had { format!("✓ 不再跟踪 {}（已入库的书保留）", d.display()) } else { format!("= 本来就没在跟踪 {}", d.display()) })
                }
                .map_err(|e| format!("✗ {e}")));
            }
            if cmd == "track" {
                for d in lib.tracked() {
                    println!("  跟踪中: {}", d.display());
                }
            }
        }
        "sync" => {
            if lib.tracked().is_empty() {
                fail("还没有跟踪任何目录：先 booklib track <目录>");
            }
            let devices = parse_devices(&args, &lib);
            let out_root = args.opt("out").map(PathBuf::from).unwrap_or_else(|| lib.root().join("output"));
            let prune = args.flags.iter().any(|f| f == "prune");
            let watch: Option<u64> = match (args.opt("watch"), args.flags.iter().any(|f| f == "watch")) {
                (Some(v), _) => Some(v.parse().ok().filter(|&n| n > 0).unwrap_or_else(|| usage_error("--watch= 要写正整数秒数"))),
                (None, true) => Some(60),
                (None, false) => None,
            };
            loop {
                match lib.lock() {
                    Ok(_guard) => {
                        let r = lib.sync(prune, |ev| match ev {
                            SyncEvent::Added(p, m) => println!("✓ 入库 {}  {}  ({})", m.id, m.title, p.display()),
                            SyncEvent::Updated(p, m, old) => println!("↻ 更新 {}  {} ← {old}  ({})", m.id, m.title, p.display()),
                            SyncEvent::Missing(p, t, true) => println!("✗ 原件已删，书库里也删了  {t}  ({})", p.display()),
                            SyncEvent::Missing(p, t, false) => println!("? 原件不在了（--prune 才从书库删）  {t}  ({})", p.display()),
                            SyncEvent::Failed(p, e) => eprintln!("✗ {}: {e}", p.display()),
                        });
                        match r {
                            // --watch 时没变化就不出声
                            Ok(r) if watch.is_some() && r.added + r.updated + r.missing + r.failed == 0 => {}
                            r => report(r.map(|r| format!("同步完成：新增 {}，更新 {}，没变 {}，原件不在 {}（删了 {}），失败 {}", r.added, r.updated, r.unchanged, r.missing, r.pruned, r.failed))),
                        }
                        if !devices.is_empty() {
                            build_all(&lib, &devices, &lib.list(), &out_root, false, true, &mut report);
                        }
                    }
                    Err(e) => eprintln!("{e}"),
                }
                let Some(secs) = watch else { break };
                std::thread::sleep(std::time::Duration::from_secs(secs));
            }
        }
        "remove" => {
            if rest.is_empty() {
                usage_error("remove 要给 id");
            }
            for id in args.texts() {
                report(lib.remove(&id).map(|title| format!("✓ 删除 {id}  {title}")).map_err(|e| format!("✗ {e}")));
            }
        }
        _ => unreachable!(),
    }
    if failed > 0 {
        std::process::exit(2);
    }
}
