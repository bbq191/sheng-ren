//! 书库命令行：入库、列出、按设备生成、删除。见 `library` crate 头注释。
//!
//! 用法:
//!   booklib [--library=目录] add <文件或网址>...
//!   booklib [--library=目录] list
//!   booklib [--library=目录] build --device=<设备> [--force] [--out=目录] [书名片段或 id...]
//!   booklib [--library=目录] remove <id>...
//!   booklib devices
//! 书库目录缺省 $BOOKLIB_DIR 或 ~/.local/share/booklib；产物缺省放在书库的 output/<设备>/ 下。
//! 退出码: 0 全部成功；1 用法错；2 有书处理失败。

use library::{Added, Built, Library};
use std::path::PathBuf;

const USAGE: &str = "用法:
  booklib [--library=目录] add <文件或网址>...
  booklib [--library=目录] list
  booklib [--library=目录] build --device=<设备> [--force] [--out=目录] [书名片段或 id...]
  booklib [--library=目录] remove <id>...
  booklib devices";

fn usage() -> ! {
    eprintln!("{USAGE}");
    std::process::exit(1);
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let opt = |name: &str| args.iter().find_map(|a| a.strip_prefix(&format!("--{name}="))).map(str::to_string);
    let flag = |name: &str| args.iter().any(|a| a == &format!("--{name}"));
    let pos: Vec<String> = args.iter().filter(|a| !a.starts_with("--")).cloned().collect();
    let Some(cmd) = pos.first().cloned() else { usage() };
    let rest = &pos[1..];
    if cmd == "devices" {
        for p in profile::Registry::builtin().iter() {
            let fmts: Vec<String> = p.formats.iter().map(|f| format!("{f:?}").to_lowercase()).collect();
            println!("{:<20} {}  屏幕 {}×{}  {}", p.id, p.name, p.screen.width, p.screen.height, fmts.join("/"));
        }
        return;
    }
    let root = opt("library").map(PathBuf::from).unwrap_or_else(Library::default_root);
    let lib = Library::open(&root).unwrap_or_else(|e| {
        eprintln!("打开书库失败: {e}");
        std::process::exit(2);
    });
    let mut failed = 0;
    match cmd.as_str() {
        "add" => {
            if rest.is_empty() {
                usage();
            }
            for item in rest {
                let r = if item.starts_with("http://") || item.starts_with("https://") { lib.add_url(item) } else { lib.add_file(std::path::Path::new(item)) };
                match r {
                    Ok(Added::New(m)) => println!("✓ 入库 {}  {}", m.id, m.title),
                    Ok(Added::Existing(m)) => println!("= 已在库里 {}  {}", m.id, m.title),
                    Err(e) => {
                        failed += 1;
                        eprintln!("✗ {item}: {e}");
                    }
                }
            }
        }
        "list" => {
            for m in lib.list() {
                println!("{}  {:<6} {}{}", m.id, m.source_format, m.title, if m.authors.is_empty() { String::new() } else { format!(" — {}", m.authors.join("、")) });
            }
        }
        "build" => {
            let Some(dev_id) = opt("device") else { usage() };
            let Some(device) = profile::get(&dev_id) else {
                eprintln!("没有设备 {dev_id}，运行 booklib devices 查看");
                std::process::exit(1);
            };
            let out_root = opt("out").map(PathBuf::from).unwrap_or_else(|| lib.root().join("output"));
            let books = lib.select(rest);
            if books.is_empty() {
                eprintln!("没有匹配的书");
            }
            for m in &books {
                match lib.build(m, device, &out_root, flag("force")) {
                    Ok(Built::Written { path, warnings }) => {
                        println!("✓ {} → {}", m.title, path.display());
                        for w in warnings {
                            println!("  ⚠ {w}");
                        }
                    }
                    Ok(Built::UpToDate(path)) => println!("= {} 已是最新（{}）", m.title, path.display()),
                    Err(e) => {
                        failed += 1;
                        eprintln!("✗ {}: {e}", m.title);
                    }
                }
            }
        }
        "remove" => {
            if rest.is_empty() {
                usage();
            }
            for id in rest {
                match lib.remove(id) {
                    Ok(m) => println!("✓ 删除 {}  {}", m.id, m.title),
                    Err(e) => {
                        failed += 1;
                        eprintln!("✗ {e}");
                    }
                }
            }
        }
        _ => usage(),
    }
    if failed > 0 {
        std::process::exit(2);
    }
}
