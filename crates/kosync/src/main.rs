//! kosync —— KOReader 阅读进度同步服务器（自托管）。协议与存储见 `lib.rs`，部署见 docs/koreader.md「进度同步服务器」。
//!
//! 用法:
//!   kosync serve [--listen=127.0.0.1:7200] [--data=/var/lib/kosync]   服务（只该监听本机，前面放 nginx 管 HTTPS）
//!   kosync useradd <用户名> [--data=…]    建账号或改密码（从标准输入读密码；终端里不回显）
//!   kosync userdel <用户名> [--data=…]    删账号和它的全部进度
//!   kosync users [--data=…]               列出账号
//! 注册接口是关的：设备上只点「登录」，用这里建的用户名和密码。
use std::io::{BufRead, IsTerminal};

const USAGE: &str = "用法: kosync serve [--listen=127.0.0.1:7200] [--data=/var/lib/kosync] | useradd <用户名> | userdel <用户名> | users   （后三个也认 --data=）";

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let opt = |name: &str, default: &str| args.iter().find_map(|a| a.strip_prefix(&format!("--{name}=")).map(str::to_string)).unwrap_or_else(|| default.to_string());
    let pos: Vec<&String> = args.iter().filter(|a| !a.starts_with("--")).collect();
    let data = std::path::PathBuf::from(opt("data", "/var/lib/kosync"));
    let result = (|| -> Result<(), String> {
        let mut store = kosync::Store::open(&data)?;
        match pos.as_slice() {
            [cmd] if *cmd == "serve" => kosync::http::serve(&opt("listen", "127.0.0.1:7200"), store),
            [cmd, name] if *cmd == "useradd" => {
                let pw = read_password()?;
                store.set_user(name, &pw)?;
                println!("✓ 账号 {name} 已设好：设备上「进度同步 → 登录」用它");
                Ok(())
            }
            [cmd, name] if *cmd == "userdel" => {
                if store.remove_user(name)? {
                    println!("✓ 删掉了 {name} 和它的进度");
                } else {
                    println!("= 没有账号 {name}");
                }
                Ok(())
            }
            [cmd] if *cmd == "users" => {
                store.users().for_each(|u| println!("{u}"));
                Ok(())
            }
            _ => Err(USAGE.to_string()),
        }
    })();
    if let Err(e) = result {
        eprintln!("✗ {e}");
        std::process::exit(1);
    }
}

/// 从标准输入读密码。终端里用 `stty -echo` 关回显、输两遍核对；管道里读一行。
fn read_password() -> Result<String, String> {
    let stdin = std::io::stdin();
    let tty = stdin.is_terminal();
    let stty = |arg: &str| {
        let _ = std::process::Command::new("stty").arg(arg).stdin(std::process::Stdio::inherit()).status();
    };
    let read = |prompt: &str| -> Result<String, String> {
        if tty {
            eprint!("{prompt}");
            stty("-echo");
        }
        let mut line = String::new();
        let r = stdin.lock().read_line(&mut line);
        if tty {
            stty("echo");
            eprintln!();
        }
        r.map_err(|e| e.to_string())?;
        Ok(line.trim_end_matches(['\r', '\n']).to_string())
    };
    let pw = read("密码: ")?;
    if pw.is_empty() {
        return Err("密码不能为空".into());
    }
    if tty && read("再输一遍: ")? != pw {
        return Err("两次输入不一样".into());
    }
    Ok(pw)
}
