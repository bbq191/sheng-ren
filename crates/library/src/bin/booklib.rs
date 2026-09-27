//! 书库命令行：入库、列出、按设备生成、删除、去重。见 `library` crate 头注释。
//!
//! 书库目录缺省 $BOOKLIB_DIR 或 ~/.local/share/booklib；产物缺省放在书库的 output/<设备>/ 下。
//! 退出码: 0 全部成功；1 用法错；2 有书处理失败（或书库打不开、没有匹配的书）。

use library::{Added, Built, Library};
use std::ffi::OsString;
use std::path::PathBuf;

const USAGE: &str = "用法:
  booklib [--library=目录] add <文件或网址>...
  booklib [--library=目录] list [书名片段或 id...]      列出书，以及给哪些设备生成过、是否最新
  booklib [--library=目录] build --device=<设备>[,<设备>…] [--force] [--out=目录] [书名片段或 id...]
      --device 可写多次或用逗号分隔，--device=all 表示全部设备；不写书名 = 全部书
  booklib [--library=目录] remove <id>...               id 用 list 里显示的完整 id
  booklib [--library=目录] dedupe <目录>...             书库里与这些目录（递归）内容相同的原文件改为共享存储
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

fn main() {
    let args = Args::parse();
    let Some(cmd) = args.pos.first().and_then(|c| c.to_str()).map(str::to_string) else { usage_error("") };
    match cmd.as_str() {
        "add" | "remove" | "dedupe" | "list" | "devices" => args.check(&cmd, &[], &[]),
        "build" => args.check(&cmd, &["device", "out"], &["force"]),
        _ => usage_error(&format!("不认识的命令 {cmd}")),
    }
    let root = args.opt("library").map(PathBuf::from).unwrap_or_else(Library::default_root);
    let lib = Library::open(&root).unwrap_or_else(|e| fail(&format!("打开书库失败: {e}")));
    // 会改动书库的命令持锁到结束
    let _lock = match cmd.as_str() {
        "list" | "devices" => None,
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
                let text = item.to_string_lossy();
                let r = if text.starts_with("http://") || text.starts_with("https://") { lib.add_url(&text) } else { lib.add_file(std::path::Path::new(item)) };
                report(match r {
                    Ok(Added::New(m)) => Ok(format!("✓ 入库 {}  {}", m.id, m.title)),
                    Ok(Added::Existing(m)) => Ok(format!("= 已在库里 {}  {}", m.id, m.title)),
                    Err(e) => Err(format!("✗ {text}: {e}")),
                });
            }
        }
        "list" => {
            for m in lib.select(&args.texts()) {
                println!("{}  {:<6} {}{}", m.id, m.source_format, m.title, if m.authors.is_empty() { String::new() } else { format!(" — {}", m.authors.join("、")) });
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
            // --device 可写多次、可逗号分隔；all = 全部设备
            let ids: Vec<&str> =
                args.opts.iter().filter(|(k, _)| k == "device").flat_map(|(_, v)| v.split(',')).map(str::trim).filter(|v| !v.is_empty()).collect();
            if ids.is_empty() {
                usage_error("build 要用 --device= 指定设备");
            }
            let mut devices: Vec<&library::Profile> = Vec::new();
            for id in ids {
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
            let out_root = args.opt("out").map(PathBuf::from).unwrap_or_else(|| lib.root().join("output"));
            let books = lib.select(&args.texts());
            if books.is_empty() {
                fail("没有匹配的书（booklib list 查看书库）");
            }
            let force = args.flags.iter().any(|f| f == "force");
            for device in &devices {
                for m in &books {
                    report(match lib.build(m, device, &out_root, force) {
                        Ok(Built::Written { path, warnings }) => {
                            Ok(std::iter::once(format!("✓ [{}] {} → {}", device.id, m.title, path.display())).chain(warnings.iter().map(|w| format!("  ⚠ {w}"))).collect::<Vec<_>>().join("\n"))
                        }
                        Ok(Built::UpToDate(path)) => Ok(format!("= [{}] {} 已是最新（{}）", device.id, m.title, path.display())),
                        Err(e) => Err(format!("✗ [{}] {}: {e}", device.id, m.title)),
                    });
                }
            }
        }
        "dedupe" => {
            if rest.is_empty() {
                usage_error("dedupe 要给目录");
            }
            let dirs: Vec<PathBuf> = rest.iter().map(PathBuf::from).collect();
            let mb = |b: u64| b as f64 / 1048576.0;
            report(lib.dedupe(&dirs).map(|r| {
                let mut lines = vec![
                    format!("✓ {} 个文件改为与原文件共享存储（{:.1} MB）", r.shared_files, mb(r.shared_bytes)),
                    format!("✓ 删掉 {} 个多余的 source 副本（{:.1} MB）", r.removed_sources, mb(r.removed_bytes)),
                ];
                if r.unlinked > 0 {
                    lines.push(format!("✓ {} 个早期的硬链接母版换成独立克隆（原文件再改也不影响母版）", r.unlinked));
                }
                lines.push("  注：克隆（reflink）的文件在 du 里仍按全尺寸显示，实际不占额外空间；btrfs 上可用 `btrfs filesystem du` 查看共享情况".into());
                lines.join("\n")
            }));
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
