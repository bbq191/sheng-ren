//! 书库命令行：入库、跟踪与同步（含按阅读模式生成）、列出、删除、去重。见 `library` crate 头注释。
//!
//! 书库目录缺省 $BOOKLIB_DIR 或 ~/.local/share/booklib。产物直接传到接着的设备上（见 `library::generate`、`library::deliver`）：
//! Kindle 放 documents/，掌阅、汉王放 Documents/，Bigme 放 Books/（镜像子目录）；没写 [deliver] 的自定义模式才放电脑上。
//! `meta --edit` 改单个 EPUB 文件、不碰书库，在 `meta_edit` 里（参数规则也不同：值可以是空字符串）。
//! 退出码: 0 全部成功；1 用法错；2 有书处理失败（或书库打不开、没有匹配的书）。

mod books;
mod complete;
mod help;
mod meta;
mod meta_edit;
mod report;
mod sync;
mod track;

#[cfg(test)]
mod tests;

use help::{command_word, print_help_if_asked, synopsis, Command, BUILD_MERGED, COMMANDS};
use library::Library;
use std::cell::Cell;
use std::ffi::OsString;
use std::path::PathBuf;

/// 用法错：说明哪里错了，再给这个命令的写法（不认识的命令给命令一览），退出码 1。
fn usage_error(msg: &str) -> ! {
    if !msg.is_empty() {
        eprintln!("{msg}\n");
    }
    match command_word().and_then(|c| COMMANDS.iter().find(|h| h.name == c)) {
        Some(h) => eprintln!(
            "用法：\n{}\n\n详细说明：booklib {} --help",
            synopsis(h),
            h.name
        ),
        None => {
            let names: Vec<&str> = COMMANDS.iter().map(|h| h.name).collect();
            eprintln!("用法：booklib [--library=目录] <命令> [参数]\n命令：{}\n\n全部说明：booklib --help", names.join("、"));
        }
    }
    std::process::exit(1);
}

fn fail(msg: &str) -> ! {
    eprintln!("{msg}");
    library::exit_process(2);
}

/// 解析后的命令行：`--名字=值` 选项、`--名字` 开关、其余是位置参数（`--` 之后全算位置参数）。
struct Args {
    opts: Vec<(String, String)>,
    flags: Vec<String>,
    pos: Vec<OsString>,
}

impl Args {
    fn parse() -> Args {
        let mut a = Args {
            opts: Vec::new(),
            flags: Vec::new(),
            pos: Vec::new(),
        };
        let mut rest = false;
        for arg in std::env::args_os().skip(1) {
            let s = arg.to_str();
            match s {
                Some("--") if !rest => rest = true,
                Some(s) if !rest && s.starts_with("--") => match s[2..].split_once('=') {
                    Some((k, v)) if v.trim().is_empty() => {
                        usage_error(&format!("--{k}= 后面要写值"))
                    }
                    Some((k, v)) => a.opts.push((k.to_string(), v.to_string())),
                    None => a.flags.push(s[2..].to_string()),
                },
                _ => a.pos.push(arg),
            }
        }
        a
    }

    fn has(&self, flag: &str) -> bool {
        self.flags.iter().any(|f| f == flag)
    }

    fn opt(&self, name: &str) -> Option<&str> {
        self.opts
            .iter()
            .rev()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.as_str())
    }

    /// 只允许这个命令认识的选项和开关；`--device kindle`（空格分隔）这类写法会在这里报错，不会悄悄当成书名。
    fn check(&self, cmd: &str, opts: &[&str], flags: &[&str]) {
        let opts: Vec<&str> = opts.iter().chain(&["library"]).copied().collect();
        if let Some((k, _)) = self.opts.iter().find(|(k, _)| !opts.contains(&k.as_str())) {
            usage_error(&format!("{cmd} 不认识选项 --{k}="));
        }
        if let Some(f) = self.flags.iter().find(|f| !flags.contains(&f.as_str())) {
            let hint = if opts.contains(&f.as_str()) {
                format!("（要写成 --{f}=值）")
            } else {
                String::new()
            };
            usage_error(&format!("{cmd} 不认识 --{f}{hint}"));
        }
    }

    /// 位置参数当文字用（书名片段、id、网址）。
    fn texts(&self) -> Vec<String> {
        self.pos[1..]
            .iter()
            .map(|s| s.to_string_lossy().into_owned())
            .collect()
    }
}

/// `booklib [--library=…] meta … --edit …`：返回 `meta` 之后的参数（去掉第一个 `--edit` 和 `--library=`）和 `--library=` 的值，交给 `meta_edit`。
/// 它在通用解析之前分出去：通用解析不许 `--名字=` 留空，而 `--edit` 的空值表示删掉。
fn meta_edit_args() -> Option<(Vec<OsString>, Option<PathBuf>)> {
    let is_library = |a: &OsString| a.to_str().is_some_and(|s| s.starts_with("--library="));
    let raw: Vec<OsString> = std::env::args_os().skip(1).collect();
    let i = raw.iter().position(|a| !is_library(a))?;
    if raw[i] != "meta" {
        return None;
    }
    let mut rest: Vec<OsString> = raw[i + 1..]
        .iter()
        .filter(|a| !is_library(a))
        .cloned()
        .collect();
    let e = rest.iter().position(|a| a == "--edit")?;
    rest.remove(e);
    let library = raw
        .iter()
        .rev()
        .find_map(|a| a.to_str()?.strip_prefix("--library=").map(PathBuf::from));
    Some((rest, library))
}

fn main() {
    bookconv::util::restore_sigpipe();
    // 补全脚本调的，不在帮助里列（见 complete.rs）
    if std::env::args_os()
        .nth(1)
        .is_some_and(|a| a == "__complete")
    {
        let rest: Vec<String> = std::env::args_os()
            .skip(2)
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        complete::run(&rest);
        return;
    }
    if let Some((rest, library)) = meta_edit_args() {
        meta_edit::run(rest, library.unwrap_or_else(Library::default_root));
        return;
    }
    print_help_if_asked();
    let args = Args::parse();
    let Some(name) = args
        .pos
        .first()
        .and_then(|c| c.to_str())
        .map(str::to_string)
    else {
        usage_error("")
    };
    if name == "build" {
        usage_error(BUILD_MERGED);
    }
    let Some(cmd) = COMMANDS.iter().find(|c| c.name == name) else {
        usage_error(&format!("不认识的命令 {name}"))
    };
    (cmd.check)(cmd, &args);
    // 会碰设备的命令在工作进程里跑：设备挂载卡死时，卡在 FUSE 上的线程会让进程退不掉（见 `supervise`）
    if matches!(cmd.name, "sync" | "devices") {
        supervise();
    }
    let root = args
        .opt("library")
        .map(PathBuf::from)
        .unwrap_or_else(Library::default_root);
    let lib = Library::open(&root).unwrap_or_else(|e| fail(&format!("打开书库失败: {e}")));
    // 会改动书库的命令持锁到结束
    let _lock = (cmd.lock)(&args).then(|| lib.lock().unwrap_or_else(|e| fail(&e)));
    let ctx = Ctx {
        lib: &lib,
        args: &args,
        cmd: cmd.name,
        failed: Cell::new(0),
    };
    (cmd.run)(&ctx);
    drop(_lock);
    // 一律经 `exit_process`：工作进程要把退出码交给前台进程（见 `supervise`）
    library::exit_process(if ctx.failed.get() > 0 { 2 } else { 0 });
}

/// 前台进程：把同样的命令交给一个工作进程（同一个可执行文件，环境变量 `BOOKLIB_EXIT_FD` 带上一根管道的写端）去跑，
/// 自己只等它的退出码。设备挂载卡死时，卡在 FUSE 上的线程等的是已经交给守护进程的请求，内核不可中断地等（连 SIGKILL 也要等），
/// 工作进程的主线程退出了、整个进程却成了僵尸，命令行一直回不来。工作进程退出前经 [`library::exit_process`] 把退出码写进管道，
/// 前台进程读到就按它退出，不等工作进程真的结束；卡住的工作进程留在后台，设备回应（或拔掉）后自己结束。
/// 管道读到头也没有退出码（工作进程崩了、被杀了）就等它结束，照它的退出状态退出。标准输入、输出、错误原样继承。
/// 已经是工作进程，或起不了工作进程时，就在本进程里跑（和以前一样）。
fn supervise() {
    if std::env::var_os("BOOKLIB_EXIT_FD").is_some() {
        return;
    }
    let Ok(exe) = std::env::current_exe() else {
        return;
    };
    let mut fds = [0i32; 2];
    // SAFETY: pipe2 只写这两个整数
    if unsafe { libc::pipe2(fds.as_mut_ptr(), libc::O_CLOEXEC) } != 0 {
        return;
    }
    let (r, w) = (fds[0], fds[1]);
    use std::os::unix::process::CommandExt;
    let mut cmd = std::process::Command::new(exe);
    cmd.args(std::env::args_os().skip(1))
        .env("BOOKLIB_EXIT_FD", w.to_string());
    // SAFETY: 子进程 exec 之前只调 fcntl（异步信号安全）：写端在 exec 后留着，读端照旧关掉
    unsafe {
        cmd.pre_exec(move || {
            if libc::fcntl(w, libc::F_SETFD, 0) != 0 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
    let spawned = cmd.spawn();
    // SAFETY: 前台进程自己不写，关掉写端（不然读不到头）
    unsafe {
        libc::close(w);
    }
    let Ok(mut child) = spawned else {
        // SAFETY: 读端没用了
        unsafe {
            libc::close(r);
        }
        return;
    };
    let mut b = [0u8; 1];
    let n = loop {
        // SAFETY: 读一个字节到本地缓冲
        let n = unsafe { libc::read(r, b.as_mut_ptr().cast(), 1) };
        if n < 0 && std::io::Error::last_os_error().kind() == std::io::ErrorKind::Interrupted {
            continue;
        }
        break n;
    };
    if n == 1 {
        std::process::exit(i32::from(b[0]));
    }
    use std::os::unix::process::ExitStatusExt;
    let code = match child.wait() {
        Ok(s) => s.code().or(s.signal().map(|g| 128 + g)).unwrap_or(2),
        Err(_) => 2,
    };
    std::process::exit(code);
}

/// 命令运行时手边的东西：书库、命令行、出错计数（有出错的退出码 2）。
struct Ctx<'a> {
    lib: &'a Library,
    args: &'a Args,
    cmd: &'static str,
    failed: Cell<usize>,
}

impl Ctx<'_> {
    /// 一行出错（打到标准错误，计数）。
    fn fail_line(&self, e: &str) {
        self.failed.set(self.failed.get() + 1);
        eprintln!("{e}");
    }

    /// 一条结果：成功的打到标准输出，失败的见 [`Ctx::fail_line`]。
    fn report(&self, r: Result<String, String>) {
        match r {
            Ok(line) => println!("{line}"),
            Err(e) => self.fail_line(&e),
        }
    }

    /// 命令名之后的位置参数。
    fn rest(&self) -> &[OsString] {
        &self.args.pos[1..]
    }
}

/// 一般的用法检查：只认这个命令表里写的选项和开关。
fn check_options(c: &Command, a: &Args) {
    a.check(c.name, c.opts, c.flags);
}

fn check_sync(c: &Command, a: &Args) {
    if a.has("prune") {
        usage_error("sync 现在总会清理（原件删了的连同产物从书库删掉），不用 --prune；这一次不想删就加 --keep");
    }
    check_options(c, a);
}

fn check_meta(c: &Command, a: &Args) {
    check_options(c, a);
    match (a.has("fetch"), a.has("show")) {
        (true, true) => usage_error("meta 的 --fetch 和 --show 不能一起用"),
        (false, true) if a.has("force") || a.has("clear") => usage_error("--force、--clear 只配 --fetch"),
        (false, false) => usage_error("meta 要选一种：--fetch（联网给书库里的书补元数据）、--show（查看跟踪目录里的书的元数据）或 --edit 书.epub（查看、改写一个 EPUB 文件）"),
        _ => {}
    }
}

fn always(_: &Args) -> bool {
    true
}

fn never(_: &Args) -> bool {
    false
}

/// `meta --show` 只读，不持锁。
fn meta_writes(a: &Args) -> bool {
    !a.has("show")
}

fn check_vertical(c: &Command, a: &Args) {
    check_options(c, a);
    let n = ["on", "off", "auto"].iter().filter(|f| a.has(f)).count();
    if n != 1 {
        usage_error(
            "vertical 要选一种：--on（强制竖排）、--off（强制横排）或 --auto（删掉覆盖，回到默认）",
        );
    }
    if a.pos.len() < 2 {
        usage_error("vertical 要写书或目录");
    }
}
