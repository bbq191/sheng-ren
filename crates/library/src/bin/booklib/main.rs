//! 书库命令行：入库、跟踪与同步（含按阅读模式生成）、列出、删除、去重。见 `library` crate 头注释。
//!
//! 书库目录缺省 $BOOKLIB_DIR 或 ~/.local/share/booklib。产物直接传到接着的设备上（见 `library::generate`、`library::deliver`）：
//! Kindle、掌阅放 documents/（镜像子目录），Move 加入 xochitl；没写 [deliver] 的自定义模式才放电脑上。
//! `meta --edit` 改单个 EPUB 文件、不碰书库，在 `meta_edit` 里（参数规则也不同：值可以是空字符串）。
//! 退出码: 0 全部成功；1 用法错；2 有书处理失败（或书库打不开、没有匹配的书）。

mod complete;
mod meta_edit;

use library::{Added, BuildCounts, BuildEvent, BuildRun, Built, CoverResult, FailMemo, InfoResult, Library, OriginalState, Profile, SyncEvent, SyncMemo, TrackChange, UntrackChange};
use std::cell::Cell;
use std::collections::HashSet;
use std::ffi::OsString;
use std::path::{Path, PathBuf};

/// 一条命令：帮助（参数写法、一句话、详细说明）、认哪些选项、要不要持锁、怎么做。
/// 新增命令只在 [`COMMANDS`] 里加一项（帮助、用法检查、加锁、分派都从这张表查）。
struct Command {
    name: &'static str,
    /// 参数写法：`booklib [--library=目录] ` 之后的部分；meta 有三种写法。
    synopsis: &'static [&'static str],
    brief: &'static str,
    detail: &'static str,
    /// 认的 `--名字=值` 选项和 `--名字` 开关（`--library=` 各命令都认）。
    opts: &'static [&'static str],
    flags: &'static [&'static str],
    /// 用法检查（打开书库之前）：一般是 [`check_options`]；有额外规矩的命令自己写。
    check: fn(&Command, &Args),
    /// 会改书库、要持锁到结束（`sync` 每一轮自己加锁：`--watch` 时不能一直占着）。
    lock: fn(&Args) -> bool,
    run: fn(&Ctx),
}

const COMMANDS: &[Command] = &[
    Command {
        name: "sync",
        synopsis: &["sync [--device=<模式>[,<模式>…]] [--force] [--no-build] [--no-meta] [--keep] [--watch[=秒]] [书…]"],
        brief: "同步跟踪目录，按阅读模式生成优化过的书，直接传到接着的设备（日常用这一条）",
        detail: "\
第一步 同步：书库严格镜像跟踪的目录——新增的入库，改过的换成新版本，挪动、改名的按内容认出来；
  原件删了的连同设备上的那本一起删（Move 上进回收站）。跟踪目录整个不在、读不了的目录里的书不删。
补元数据：还没找过的书（含漫画）联网找一次简介、标签、带国籍的作者、译者、出版社、ISBN（同 meta --fetch，找过的不再找），
  生成时补进书里；连不上网就跳过，下次再找。
第二步 生成并传：只给接着的设备生成，传上去，电脑上不留产物：
  kindle   Kindle（USB）  KFX   存储根目录 documents/<子目录>/
  ireader  掌阅（USB）    EPUB  存储根目录 documents/<子目录>/
  xochitl  Move（SSH）    EPUB  经 Move 上的书架服务加入 xochitl，放进文件夹 <子目录>；更新时原地替换
  <子目录> 是原件在跟踪目录里所在的子目录。没接上的设备这次跳过，下次接上再传。
  没变化的书不重新生成；重新生成出来和设备上一样的不再传（Kindle 进度不丢，Move 不重排）。

选项：
  --device=<模式>  只处理这几个模式（可写多次或用逗号分隔；all 或不写 = 全部）
  --force          选中的书（没选就是全部）重新生成；和设备上一样的照样不传
  --no-build       只同步，不生成、不传（不能和 --device、--force、书一起用）
  --no-meta        这一次不联网补元数据
  --keep           这一次原件不在了、不再跟踪的只报告，不从书库和设备上删
  --watch[=秒]     一直运行，每隔几秒（缺省 60）看一次，原件有变化或接上设备就处理（不能和书、--force 一起用）
  书…              只处理这几本：书名片段、id，或原件路径（文件，或目录 = 下面所有的书）

例子：
  booklib sync                         全部
  booklib sync --device=kindle 三体     只给 Kindle、只处理书名含「三体」的
  booklib sync --watch=300             挂着，每 5 分钟看一次",
        opts: &["device", "watch"],
        flags: &["keep", "watch", "no-build", "no-meta", "force"],
        check: check_sync,
        lock: never,
        run: cmd_sync,
    },
    Command {
        name: "track",
        synopsis: &["track [--device=模式,…] [--top] <目录或书>…", "track --root=目录", "track"],
        brief: "跟踪书目录（递归，收 .epub、.cbz）或单本书，可以只给某几台设备：之后 sync 把它镜像进书库",
        detail: "\
只需登记一次。目录和单本可以混着写；不写 --device 给全部设备，每台设备的跟踪范围可以不同。
设备上的放法：缺省照书库结构（相对书根目录：book/漫画/死亡筆記/卷01.epub → 漫画/死亡筆記/卷01）；
--top 从这一层起（目录 book/漫画/死亡筆記 → 死亡筆記/卷01，单本放根目录），对这条命令里的路径都生效。
同一本书被一台设备的几个条目覆盖时，用最具体的那个（单本 > 深的目录）的放法。
书根目录第一次 track 时定下（跟踪的目录；单本是它所在的目录），跟踪的都要在它里面；--root=目录 换书根
（设备上的书下次 sync 挪到新位置）。相对路径先按当前目录找，找不到按书根目录找（track 优秀漫画/死亡筆記）。已经在跟踪的再 track：设备范围加上新给的，放法改成这次的。
不写目录、书：按设备列出跟踪范围。
例子：
  booklib track ~/books                                               全部设备，照书库结构
  booklib track --device=xochitl --top ~/books/漫画/死亡筆記 ~/books/文字/飘.epub   Move：「死亡筆記」放最上层、《飘》放根目录
  booklib track --device=kindle ~/books/文字/科幻/三体.epub                Kindle 只要这一本",
        opts: &["device", "root"],
        flags: &["top"],
        check: check_options,
        lock: always,
        run: cmd_track,
    },
    Command {
        name: "untrack",
        synopsis: &["untrack [--device=模式,…] <目录或书>…"],
        brief: "不再跟踪：下次 sync 把其中的书连同设备上的产物删掉（原件不动）；给了 --device 只从这几台上删",
        detail: "\
路径同 track：相对路径先按当前目录、再按书根目录找。
不写 --device：整条去掉，里面没被别的跟踪条目覆盖的书下次 sync 从书库删掉（连同设备上的，Move 上进回收站）。
写了 --device：只从这几台的范围里拿掉，下次 sync 从这几台上删掉，别的设备照旧。
跟踪目录里的单本不能单独去掉（要么 untrack 整个目录）。删之前想先看看：sync --keep 只报告、不删。",
        opts: &["device"],
        flags: &[],
        check: check_options,
        lock: always,
        run: cmd_untrack,
    },
    Command {
        name: "add",
        synopsis: &["add <文件或网址>…"],
        brief: "入库单个 EPUB、CBZ 文件或网址（整个目录用 track）",
        detail: "\
add 进来的书 sync 照样生成、传（放设备 documents/ 顶层，Move 上放书库根）；原件没了也不自动删，不要了用 remove。
网址：抓网页正文做成 EPUB，存在书库里。同一路径的文件内容改了，再 add 一次换成新版本。",
        opts: &[],
        flags: &[],
        check: check_options,
        lock: always,
        run: cmd_add,
    },
    Command {
        name: "list",
        synopsis: &["list [书…]"],
        brief: "列出书，以及在各设备上的产物是否最新",
        detail: "\
书：书名片段、id 或原件路径；不写 = 全部。每本一行（id、格式、书名、作者），下面每个模式一行：
  ✓ 最新   ⚠ 过期（下次 sync 会重新生成）   ? 未知（设备没接上，或设备上那本不在了）
  ✗ 不在这台的跟踪范围（下次 sync 从设备上删）
Move 上的显示成 xochitl:文件夹/文件名（只看记录，不连 Move）。",
        opts: &[],
        flags: &[],
        check: check_options,
        lock: never,
        run: cmd_list,
    },
    Command {
        name: "meta",
        synopsis: &["meta --fetch [--force] [--clear] [书…]", "meta --show [书…]", "meta --edit 书.epub|书 [--title 书名 --author 作者 --tag 标签 --cover 图 …]"],
        brief: "书的元数据：联网补、查看、改写一个 EPUB",
        detail: "\
--fetch  联网补元数据（豆瓣 → QQ 阅读 → Wikidata）：简介、标签、原作名，书里没封面的顺带找封面（找不到就生成）；漫画跳过。
         生成产物时只补书里没有的简介、标签、封面，书名作者和正文不动，原件不动。
         --force 重找已找过的；--clear 去掉找来的元数据和封面（找错了时）
--show   查看跟踪目录里的书的元数据：书里写的，和 --fetch 找来的；只读
--edit   查看、改写一个 EPUB 文件的元数据和封面（改文件本身；值给空字符串 = 删掉）。给书库 id 或书名时改那本书的原件，
         详见 booklib meta --edit --help",
        opts: &[],
        flags: &["fetch", "force", "clear", "show"],
        check: check_meta,
        lock: meta_writes,
        run: cmd_meta,
    },
    Command {
        name: "remove",
        synopsis: &["remove <id>…"],
        brief: "从书库删书，连同设备上的产物（Move 上进回收站；设备没接上的下次接上时删）。原件不动",
        detail: "id 用 list 里显示的完整 id（不做模糊匹配）。原件在跟踪目录里的，下次 sync 会再入库：要彻底不要就删掉原件。",
        opts: &[],
        flags: &[],
        check: check_options,
        lock: always,
        run: cmd_remove,
    },
    Command {
        name: "dedupe",
        synopsis: &["dedupe [目录…]"],
        brief: "早期版本入库的书改成只存索引（在记着的位置和这些目录里找原件）",
        detail: "找不到原件的保留书库里的副本并列出来。可以反复跑。",
        opts: &[],
        flags: &[],
        check: check_options,
        lock: always,
        run: cmd_dedupe,
    },
    Command {
        name: "devices",
        synopsis: &["devices"],
        brief: "列出阅读模式，以及设备接没接上（只看，不往设备上写）",
        detail: "书库的 profiles/ 里放 <id>.toml 可以加模式或覆盖内置的参数（写法见 docs/devices.md）。",
        opts: &[],
        flags: &[],
        check: check_options,
        lock: never,
        run: cmd_devices,
    },
];

/// 日常用法、通用选项、环境变量、退出码（`booklib --help` 的头尾）。
const HELP_HEAD: &str = "\
booklib —— 电子书书库：跟踪书目录，按 Kindle、掌阅、Move 的自带阅读器优化，直接传到接着的设备

用法：booklib [--library=目录] <命令> [参数]

日常：
  booklib track ~/Documents/ereader/books    第一次：登记要跟踪的书目录
  booklib sync                               之后每次：同步书目录、优化、传到接着的设备
  booklib devices                            看设备接没接上";

const HELP_TAIL: &str = "\
通用选项：
  --library=目录   书库目录（缺省 $BOOKLIB_DIR，没设就是 ~/.local/share/booklib）
  -h, --help       帮助；booklib <命令> --help（或 booklib help <命令>）只看这个命令的详细说明
  -V, --version    版本：提交号、各项规则的版本
  --               之后的参数都当书名、路径（以 - 开头的书名用）

环境变量：
  BOOKLIB_DIR       书库目录
  BOOKLIB_MTP_DIR   Kindle、掌阅的 jmtpfs 挂载点所在的目录（缺省 $XDG_RUNTIME_DIR/mtp）
  BOOKLIB_NO_SSH=1  不连 Move
  BOOKLIB_MTP_NO_MOUNT_CHECK=1  不查 MTP 挂载点挂没挂着（测试用普通目录当设备）
  BOOKLIB_NO_NET=1  不联网（不找元数据、封面）

退出码：0 全部成功；1 用法错；2 有书处理失败（或书库打不开、没有匹配的书）";

/// 命令的参数写法，每种一行（`  booklib [--library=目录] …`）。
fn synopsis(h: &Command) -> String {
    h.synopsis.iter().map(|s| format!("  booklib [--library=目录] {s}")).collect::<Vec<_>>().join("\n")
}

/// `booklib --help` 的全文。
fn full_help() -> String {
    let cmds: Vec<String> = COMMANDS.iter().map(|h| format!("{}\n      {}", synopsis(h), h.brief)).collect();
    format!("{HELP_HEAD}\n\n命令：\n{}\n\n{HELP_TAIL}", cmds.join("\n"))
}

/// 一个命令的详细说明。
fn command_usage(cmd: &str) -> Option<String> {
    let h = COMMANDS.iter().find(|h| h.name == cmd)?;
    let detail = if h.detail.is_empty() { String::new() } else { format!("\n\n{}", h.detail) };
    Some(format!("用法：\n{}\n\n{}{detail}", synopsis(h), h.brief))
}

/// `booklib --version`、`-V`、`version`。
fn version() -> String {
    let date = env!("BOOKLIB_GIT_DATE");
    let rules: Vec<String> = library::rule_versions().into_iter().map(|(k, v)| format!("{k} {v}")).collect();
    format!(
        "booklib {}（提交 {}{}）\n规则版本：{}",
        env!("CARGO_PKG_VERSION"),
        env!("BOOKLIB_GIT_COMMIT"),
        if date.is_empty() { String::new() } else { format!("，{date}") },
        rules.join(" · ")
    )
}

/// 命令行里的命令名（跳过 `--library=` 等选项；`--` 之前）。
fn command_word() -> Option<String> {
    std::env::args_os().skip(1).map(|a| a.to_string_lossy().into_owned()).take_while(|a| a != "--").find(|a| !a.starts_with('-'))
}

/// `booklib --help`、`-h`、`help` 打印全部用法；`booklib <命令> --help`（或 `-h`）、`booklib help <命令>` 只打印这个命令的；
/// `--version`、`-V`、`version` 打印版本。打到标准输出、退出码 0（不算用错）。`--` 之后的参数不算。
fn print_help_if_asked() {
    let raw: Vec<String> = std::env::args_os().skip(1).map(|a| a.to_string_lossy().into_owned()).collect();
    let args: Vec<&str> = raw.iter().map(String::as_str).take_while(|a| *a != "--").filter(|a| !a.starts_with("--library=")).collect();
    if matches!(args.first(), Some(&("--version" | "-V" | "version"))) {
        println!("{}", version());
        std::process::exit(0);
    }
    let is_help = |a: &&str| matches!(*a, "--help" | "-h");
    let cmd = match args.first() {
        None => return,
        Some(&"help") => args.get(1).copied(),
        Some(a) if is_help(a) => None,
        Some(c) if args.iter().any(is_help) => Some(*c),
        _ => return,
    };
    match cmd.map(|c| (c, command_usage(c))) {
        None => println!("{}", full_help()),
        Some((_, Some(u))) => println!("{u}"),
        Some(("build", None)) => usage_error(BUILD_MERGED),
        Some((c, None)) => usage_error(&format!("不认识的命令 {c}")),
    }
    std::process::exit(0);
}

/// 2026-10-06 起 `build` 并入 `sync`：敲旧命令（或看它的帮助）时的提示。
const BUILD_MERGED: &str = "build 已并入 sync：booklib sync [--device=…] [--force] [书...]";

/// 用法错：说明哪里错了，再给这个命令的写法（不认识的命令给命令一览），退出码 1。
fn usage_error(msg: &str) -> ! {
    if !msg.is_empty() {
        eprintln!("{msg}\n");
    }
    match command_word().and_then(|c| COMMANDS.iter().find(|h| h.name == c)) {
        Some(h) => eprintln!("用法：\n{}\n\n详细说明：booklib {} --help", synopsis(h), h.name),
        None => {
            let names: Vec<&str> = COMMANDS.iter().map(|h| h.name).collect();
            eprintln!("用法：booklib [--library=目录] <命令> [参数]\n命令：{}\n\n全部说明：booklib --help", names.join("、"));
        }
    }
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

    fn has(&self, flag: &str) -> bool {
        self.flags.iter().any(|f| f == flag)
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
        Added::Replaced(m, old) => format!("↻ 原件改过，换成新版本 {}  {} ← {old}（旧版本的条目和产物已删）", m.id, m.title),
    }
}

/// `--device` 可写多次、可逗号分隔；all = 全部模式；一个都没写也是全部模式。写了不存在的模式直接报用法错。
fn parse_devices<'a>(args: &Args, lib: &'a Library) -> Vec<&'a Profile> {
    let mut devices: Vec<&Profile> = Vec::new();
    if !args.opts.iter().any(|(k, _)| k == "device") {
        return lib.devices().iter().collect();
    }
    for id in args.opts.iter().filter(|(k, _)| k == "device").flat_map(|(_, v)| v.split(',')).map(str::trim).filter(|v| !v.is_empty()) {
        if id == "all" {
            devices.extend(lib.devices().iter());
            continue;
        }
        match lib.devices().get(id) {
            Some(p) => devices.push(p),
            None => usage_error(&format!("没有阅读模式 {id}，运行 booklib devices 查看")),
        }
    }
    devices.sort_by(|a, b| a.id.cmp(&b.id));
    devices.dedup_by(|a, b| a.id == b.id);
    devices
}

/// `sync --watch` 记住的生成失败：(书 id, 模式 id) → 失败时的指纹和原件状态。都没变就不再重试、不再重复报错。
/// 设备重新接上时这台的全部忘掉（[`forget_reconnected`]）：传输失败、Move 掉线时查书出的错多半是连接的事，接上了要重试。
/// 这一轮重新接上的设备（上一轮没接上、这轮接上了，或 Move 的连接断过、重连了）：忘掉它们记着的失败。
/// `last_live`、`live` 和 `devices` 一一对应；`dropped` 是这轮开头发现断了的 Move（[`Library::refresh_devices`]）。
fn forget_reconnected(fails: &mut FailMemo, devices: &[&Profile], last_live: Option<&[bool]>, live: &[bool], dropped: &[String]) {
    for (i, (d, ok)) in devices.iter().zip(live).enumerate() {
        let was_down = last_live.and_then(|l| l.get(i)).is_some_and(|w| !w);
        if dropped.contains(&d.id) || (*ok && was_down) {
            fails.retain(|(_, dev), _| *dev != d.id);
        }
    }
}

/// 一轮生成的汇总一行（用户 2026-09-29 反馈"看不出是否更新过"）。
fn counts_summary(c: &BuildCounts) -> String {
    let same = if c.same > 0 { format!("，重新生成但内容没变（没再传）{} 本", c.same) } else { String::new() };
    let adopted = if c.adopted > 0 { format!("，认领设备上已有的 {} 本", c.adopted) } else { String::new() };
    let retracted = if c.retracted > 0 { format!("，不在跟踪范围、从设备上删了 {} 本", c.retracted) } else { String::new() };
    format!("生成：重新生成并传上 {} 本{same}{adopted}，挪位置 {} 本，已是最新 {} 本{retracted}，失败 {} 本", c.written, c.moved, c.up_to_date, c.failed)
}

/// 一轮生成里的一件事显示成一行（`quiet` 时没变化的不报）；`Err` 的算失败（退出码 2）。
fn event_line(ev: BuildEvent, quiet: bool) -> Option<Result<String, String>> {
    Some(match ev {
        BuildEvent::Unsupported { id, title, reason } => Ok(format!("- 跳过 {id}  {title}：{reason}")),
        BuildEvent::RemovedFlushed { device, count } => Ok(format!("✓ [{device}] 删掉了 {count} 本已从书库删除的书")),
        BuildEvent::RemoveFlushFailed { device, error } => Err(format!("✗ [{device}] 删已从书库删除的书：{error}")),
        BuildEvent::DeviceAbsent { device, reason } => Ok(format!("- [{device}] {reason}，这次不传")),
        BuildEvent::Outside { device, title } => Ok(format!("? [{device}] {title}：不在这台的跟踪范围了（--keep，这次没从设备上删）")),
        BuildEvent::Retracted { device, title } => Ok(format!("✗ [{device}] {title}：不在这台的跟踪范围了，从设备上删了")),
        BuildEvent::RetractFailed { device, title, error } => Err(format!("✗ [{device}] {title}：不在这台的跟踪范围了，从设备上删失败：{error}")),
        BuildEvent::Built { device, title, built } => return built_line(&device, &title, built, quiet).map(Ok),
        BuildEvent::Failed { device, title, error } => Err(format!("✗ [{device}] {title}: {error}")),
        BuildEvent::Progress(line) => Ok(line),
        BuildEvent::TransportLost { device, count } => Err(format!("✗ [{device}] 传输线程意外退出，{count} 本没传成（下次再传）")),
        BuildEvent::Waiting { in_flight } => Ok(format!("… 都生成好了，等设备那边传完（还有 {in_flight} 本；Move 加入时要排版，大书要几分钟）")),
    })
}

/// 一本书一个模式的结果一行（`quiet` 时没变化的不报）。
fn built_line(device: &str, title: &str, b: Built, quiet: bool) -> Option<String> {
    let with_warnings = |head: String, warnings: &[String]| std::iter::once(head).chain(warnings.iter().map(|w| format!("  ⚠ {w}"))).collect::<Vec<_>>().join("\n");
    match b {
        Built::Written { path, warnings } => Some(with_warnings(format!("✓ 生成 [{device}] {title} → {}", path.display()), &warnings)),
        Built::Same { path, warnings } => Some(with_warnings(format!("≡ 重新生成 [{device}] {title}：和设备上的一样，没再传（{}）", path.display()), &warnings)),
        Built::UpToDate(path) => (!quiet).then(|| format!("= [{device}] {title} 已是最新（{}）", path.display())),
        Built::Adopted(path) => Some(format!("= [{device}] {title} 认领设备上已有的那份，没重新生成（{}）", path.display())),
        Built::Moved { from, to } => Some(format!("↪ 挪位置 [{device}] {title} → {}（原来在 {}）", to.display(), from.display())),
    }
}

/// 元数据一行摘要：来源、简介字数、标签、参考版本。
fn info_summary(i: &library::BookInfo) -> String {
    let mut parts = vec![i.source.clone()];
    if !i.original_title.is_empty() {
        parts.push(format!("原作名 {}", i.original_title));
    }
    if !i.first_published.is_empty() {
        parts.push(format!("{} 年首次出版", i.first_published));
    }
    if !i.description.is_empty() {
        parts.push(format!("简介 {} 字", i.description.chars().count()));
    }
    if !i.subjects.is_empty() {
        parts.push(format!("标签 {}", i.subjects.join("、")));
    }
    if let Some(e) = &i.edition {
        let v: Vec<&str> = [e.publisher.as_str(), e.pubdate.as_str()].into_iter().filter(|x| !x.is_empty()).collect();
        let tr = if e.translators.is_empty() { String::new() } else { format!(" {} 译", e.translators.join("、")) };
        parts.push(format!("参考版本 {}{tr}", v.join(" ")));
    }
    parts.join("；")
}

/// `meta --show` 的一本：书里写的元数据（读原件的 OPF），和联网找来的。
fn show_meta(m: &library::Meta, fail_line: &mut dyn FnMut(String)) {
    println!("{}  {}{}", m.id, m.title, if m.authors.is_empty() { String::new() } else { format!(" — {}", m.authors.join("、")) });
    println!("  原件：{}", m.source_path);
    let path = Path::new(&m.source_path);
    if m.content_format() != "epub" {
        println!("  书里：（{} 来源，没有 OPF 元数据）", m.content_format().to_uppercase());
    } else if !path.is_file() {
        fail_line(format!("  ✗ 原件不在了：{}（改名或移动过就 booklib sync）", m.source_path));
    } else {
        match bookconv::opfmeta::read_epub(path) {
            Ok(fields) => {
                for (field, values) in fields.iter().filter(|(_, v)| !v.is_empty()) {
                    let v = values.join("; ");
                    let v = if v.chars().count() > 80 { format!("{}…（{} 字）", v.chars().take(80).collect::<String>(), v.chars().count()) } else { v };
                    println!("  书里 {}：{}", field.label(), v.replace('\n', " "));
                }
                match bookconv::epubzip::cover_image_of(path) {
                    Some((ext, bytes)) => {
                        let dims = image::load_from_memory(&bytes).map(|i| format!("，{}×{}", i.width(), i.height())).unwrap_or_default();
                        println!("  书里 封面：有（{ext}{dims}，{} KB）", bytes.len() / 1024);
                    }
                    None => println!("  书里 封面：无"),
                }
            }
            Err(e) => fail_line(format!("  ✗ 读不出元数据：{e}")),
        }
    }
    match &m.info {
        Some(i) => println!("  找来：{}", info_summary(i)),
        None => println!("  找来：没找过（booklib meta --fetch）"),
    }
    if let Some(c) = &m.cover {
        println!("  找来 封面：{}（{}）", c.work, c.source_url);
    }
}

/// 选书的参数里有像路径的（通常是路径里有空格没加引号，被拆开了），提示一下。
fn path_hint(selectors: &[String]) -> String {
    match selectors.iter().find(|s| s.contains('/')) {
        Some(s) => format!("\n（\"{s}\" 看起来像路径的一部分：路径里有空格时要整个加引号）"),
        None => String::new(),
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
    let mut rest: Vec<OsString> = raw[i + 1..].iter().filter(|a| !is_library(a)).cloned().collect();
    let e = rest.iter().position(|a| a == "--edit")?;
    rest.remove(e);
    let library = raw.iter().rev().find_map(|a| a.to_str()?.strip_prefix("--library=").map(PathBuf::from));
    Some((rest, library))
}

fn main() {
    bookconv::util::restore_sigpipe();
    // 补全脚本调的，不在帮助里列（见 complete.rs）
    if std::env::args_os().nth(1).is_some_and(|a| a == "__complete") {
        let rest: Vec<String> = std::env::args_os().skip(2).map(|a| a.to_string_lossy().into_owned()).collect();
        complete::run(&rest);
        return;
    }
    if let Some((rest, library)) = meta_edit_args() {
        meta_edit::run(rest, library.unwrap_or_else(Library::default_root));
        return;
    }
    print_help_if_asked();
    let args = Args::parse();
    let Some(name) = args.pos.first().and_then(|c| c.to_str()).map(str::to_string) else { usage_error("") };
    if name == "build" {
        usage_error(BUILD_MERGED);
    }
    let Some(cmd) = COMMANDS.iter().find(|c| c.name == name) else { usage_error(&format!("不认识的命令 {name}")) };
    (cmd.check)(cmd, &args);
    let root = args.opt("library").map(PathBuf::from).unwrap_or_else(Library::default_root);
    let lib = Library::open(&root).unwrap_or_else(|e| fail(&format!("打开书库失败: {e}")));
    // 会改动书库的命令持锁到结束
    let _lock = (cmd.lock)(&args).then(|| lib.lock().unwrap_or_else(|e| fail(&e)));
    let ctx = Ctx { lib: &lib, args: &args, cmd: cmd.name, failed: Cell::new(0) };
    (cmd.run)(&ctx);
    if ctx.failed.get() > 0 {
        std::process::exit(2);
    }
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

fn cmd_devices(ctx: &Ctx) {
    let lib = ctx.lib;
    for p in lib.devices().iter() {
        let r = p.output_readable();
        println!("{:<12} {}  {}  屏幕 {}×{}  阅读范围 {}×{}  {}", p.id, p.name, match p.comic.format.filter(|f| *f != p.format()) {
            Some(c) => format!("{}（漫画 {}）", p.format().ext().to_uppercase(), c.ext().to_uppercase()),
            None => p.format().ext().to_uppercase(),
        }, p.screen.width, p.screen.height, r.width, r.height, if p.color { "彩色" } else { "黑白" });
        match lib.device_status(p) {
            Ok(Some(at)) => println!("{:<12} ✓ 接上了：{at}", ""),
            Ok(None) => println!("{:<12} 产物放电脑上", ""),
            Err(e) => println!("{:<12} - {e}", ""),
        }
    }
}

fn cmd_add(ctx: &Ctx) {
    let lib = ctx.lib;
    if ctx.rest().is_empty() {
        usage_error("add 要给文件或网址（目录用 track + sync）");
    }
    for item in ctx.rest() {
        let text = item.to_string_lossy().into_owned();
        let path = Path::new(item);
        ctx.report(if text.starts_with("http://") || text.starts_with("https://") {
            lib.add_url(&text).map(|a| added_line(&a)).map_err(|e| format!("✗ {text}: {e}"))
        } else if path.is_dir() {
            Err(format!("✗ {text} 是目录：目录用 booklib track {text} 登记跟踪，再 booklib sync 入库（之后增删改都会同步）"))
        } else {
            lib.add_file(path).map(|a| added_line(&a)).map_err(|e| format!("✗ {text}: {e}"))
        });
    }
}

fn cmd_list(ctx: &Ctx) {
    let lib = ctx.lib;
    for m in lib.select(&ctx.args.texts()) {
        println!("{}  {:<6} {}{}", m.id, m.source_format, m.title, if m.authors.is_empty() { String::new() } else { format!(" — {}", m.authors.join("、")) });
        if !m.supported() {
            println!("      - 不再支持的格式（.{}）：不再生成，已有的产物不动；不要了就 remove", m.content_format());
        }
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
            let state = if lib.wants(&m, &o.device) { state } else { "✗ 不在这台的跟踪范围（下次 sync 从设备上删）" };
            println!("      {:<20} {state}  {}", o.device, shown.display());
            // 体检（生成时记下的）：和 Kindle 不一样又没处理的逐条列出，没验证过的合成一行
            for line in o.audit.iter().filter(|l| l.starts_with('✗')) {
                println!("        体检 {line}");
            }
            let unknown: Vec<&str> = o.audit.iter().filter_map(|l| l.strip_prefix("? ")).collect();
            if !unknown.is_empty() {
                println!("        体检 ? 没验证过 {} 种：{}", unknown.len(), unknown.join("；"));
            }
        }
    }
    for id in lib.broken() {
        eprintln!("⚠ 条目 {id} 的 meta.json 读不出来（写坏了）：booklib remove {id} 删掉，或重新 add 原文件覆盖");
    }
}

fn cmd_dedupe(ctx: &Ctx) {
    let dirs: Vec<PathBuf> = ctx.rest().iter().map(PathBuf::from).collect();
    let mb = |b: u64| b as f64 / 1048576.0;
    ctx.report(ctx.lib.dedupe(&dirs).map(|r| {
        let mut lines = vec![format!("✓ {} 本书改成只存索引，删掉书库里的副本 {:.1} MB", r.migrated, mb(r.freed_bytes))];
        for m in &r.kept {
            lines.push(format!("  ? 没找到原件，副本保留：{}  {}（原来在 {}）", m.id, m.title, if m.source_path.is_empty() { &m.source } else { &m.source_path }));
        }
        lines.join("\n")
    }));
}

fn cmd_track(ctx: &Ctx) {
    track_or_untrack(ctx, true);
}

fn cmd_untrack(ctx: &Ctx) {
    track_or_untrack(ctx, false);
}

/// `track`/`untrack` 的 `--device=`：模式 id（不写、写了 all 是空＝全部设备）。写了不存在的模式报用法错。
fn device_ids(args: &Args, lib: &Library) -> Vec<String> {
    let all = args.opts.iter().filter(|(k, _)| k == "device").flat_map(|(_, v)| v.split(',')).any(|v| v.trim() == "all");
    if all || !args.opts.iter().any(|(k, _)| k == "device") {
        return Vec::new();
    }
    parse_devices(args, lib).into_iter().map(|p| p.id.clone()).collect()
}

fn track_or_untrack(ctx: &Ctx, track: bool) {
    let lib = ctx.lib;
    if let Some(r) = ctx.args.opt("root").filter(|_| track) {
        if !ctx.rest().is_empty() || ctx.args.opts.iter().any(|(k, _)| k == "device") || ctx.args.has("top") {
            usage_error("--root= 单独用：booklib track --root=目录");
        }
        ctx.report(lib.set_books_root(Path::new(r)).map(|changed| if changed { format!("✓ 书根目录设为 {r}（已传的书下次 sync 挪到按它算的位置）") } else { format!("= 书根目录本来就是 {r}") }).map_err(|e| format!("✗ {e}")));
        print_tracked(lib);
        return;
    }
    if ctx.rest().is_empty() {
        if track {
            print_tracked(lib);
            return;
        }
        usage_error(&format!("{} 要给目录或书", ctx.cmd));
    }
    let devices = device_ids(ctx.args, lib);
    let top = ctx.args.has("top");
    let to = if devices.is_empty() { "全部设备".to_string() } else { devices.join("、") };
    for d in ctx.rest() {
        let d = match resolve_track_path(lib, Path::new(d), track) {
            Ok(p) => p,
            Err(e) => {
                ctx.report(Err(e));
                continue;
            }
        };
        let d = d.as_path();
        let kind = if d.is_file() { "单本" } else { "目录" };
        let how = match (top, kind) {
            (false, _) => "，照书库结构",
            (true, "单本") => "，放根目录",
            (true, _) => "，从这一层起",
        };
        ctx.report(if track {
            lib.track_with(d, &devices, top).map(|c| match c {
                TrackChange::New => format!("✓ 开始跟踪{kind} {}，给 {to}{how}（运行 booklib sync 入库、传）", d.display()),
                TrackChange::Changed => format!("✓ 已在跟踪{kind} {}，加上 {to}{how}（运行 booklib sync 生效）", d.display()),
                TrackChange::Same => format!("= 已在跟踪{kind} {}", d.display()),
            })
        } else {
            lib.untrack_with(d, &devices).map(|c| match c {
                UntrackChange::Removed(n) => format!("✓ 不再跟踪 {}：下次 sync 从书库删掉 {n} 本（按上次 sync 看到的算；连同设备上的，还被别的跟踪条目覆盖的照留）", d.display()),
                UntrackChange::Narrowed(n) => format!("✓ {} 不再给 {to}：下次 sync 从这几台上删掉 {n} 本（按上次 sync 看到的算）", d.display()),
                UntrackChange::NotTracked => format!("= 本来就没在跟踪 {}{}", d.display(), if devices.is_empty() { String::new() } else { format!("（给 {to} 的）") }),
            })
        }
        .map_err(|e| format!("✗ {e}")));
    }
    print_tracked(lib);
}

/// `track`/`untrack` 的路径：相对路径先按当前目录找，找不到再按书根目录找（2026-10-19 用户要：`track 优秀漫画/死亡筆記`
/// 不用每次写全路径）；绝对路径照旧。两处都先按原样找，没有再补 `.epub`、`.cbz`（同日用户要：`track 欧美侦探/罗杰疑案`；
/// 两个都有时报错，要写上扩展名）。`track` 都找不到时报错；`untrack` 的（原件可能已经删了）在书根目录下跟踪着也算。
fn resolve_track_path(lib: &Library, p: &Path, track: bool) -> Result<PathBuf, String> {
    let tracked = if track { Vec::new() } else { lib.tracked() };
    let root = lib.books_root();
    let bases: Vec<PathBuf> = if p.is_absolute() { vec![p.to_path_buf()] } else { std::iter::once(p.to_path_buf()).chain(root.as_ref().map(|r| r.join(p))).collect() };
    for q in &bases {
        if q.exists() || tracked.contains(q) {
            return Ok(q.clone());
        }
        let found: Vec<PathBuf> = with_book_ext(q).into_iter().filter(|c| c.is_file() || tracked.contains(c)).collect();
        match found.as_slice() {
            [] => {}
            [one] => return Ok(one.clone()),
            _ => return Err(format!("✗ {} 有 .epub 和 .cbz 两本，写上扩展名", q.display())),
        }
    }
    if !track {
        return Ok(p.to_path_buf());
    }
    Err(match root {
        Some(r) if !p.is_absolute() => format!("✗ 找不到 {}：当前目录和书根目录 {} 里都没有（也试过补 .epub、.cbz）", p.display(), r.display()),
        _ => format!("✗ 找不到 {}（也试过补 .epub、.cbz）", p.display()),
    })
}

/// 补上书的扩展名的两个候选（`罗杰疑案` → `罗杰疑案.epub`、`罗杰疑案.cbz`）；以 `/` 结尾的（写的是目录）不补。
fn with_book_ext(p: &Path) -> Vec<PathBuf> {
    let s = p.as_os_str();
    if s.as_encoded_bytes().ends_with(b"/") {
        return Vec::new();
    }
    ["epub", "cbz"]
        .iter()
        .map(|e| {
            let mut o = s.to_owned();
            o.push(".");
            o.push(e);
            PathBuf::from(o)
        })
        .collect()
}

/// 按设备列出跟踪范围：每台的目录、单本（给全部设备的标出来）。
fn print_tracked(lib: &Library) {
    let entries = lib.tracked_entries();
    if entries.is_empty() {
        println!("  还没有跟踪任何目录或书");
        return;
    }
    if let Some(r) = lib.books_root() {
        println!("  书根目录：{}（设备上按相对它的路径放）", r.display());
    }
    for p in lib.devices().iter() {
        let mine: Vec<&library::Tracked> = entries.iter().filter(|e| e.for_device(&p.id)).collect();
        println!("  {}：{}", p.id, if mine.is_empty() { "（没有）" } else { "" });
        for e in &mine {
            let kind = if e.file { "单本" } else { "目录" };
            let how = match (e.top, e.file) {
                (true, true) => "，放根目录",
                (true, false) => "，从这一层起",
                (false, _) => "",
            };
            let scope = if e.devices.is_empty() { "（全部设备）" } else { "" };
            println!("    {kind} {}{how}{scope}", e.path.display());
        }
        // 从这一层起的目录在设备上同名：会并进同一个文件夹（书名也一样的，后来的那本带 id 后缀），只提示
        let mut seen: std::collections::BTreeMap<String, Vec<&Path>> = std::collections::BTreeMap::new();
        for e in mine.iter().filter(|e| e.top && !e.file) {
            if let Some(n) = e.path.file_name() {
                seen.entry(n.to_string_lossy().to_lowercase()).or_default().push(&e.path);
            }
        }
        for (name, paths) in seen.iter().filter(|(_, v)| v.len() > 1) {
            println!("    ⚠ 设备上会并进同一个文件夹「{name}」：{}", paths.iter().map(|p| p.display().to_string()).collect::<Vec<_>>().join("、"));
        }
    }
}

fn cmd_remove(ctx: &Ctx) {
    let lib = ctx.lib;
    if ctx.rest().is_empty() {
        usage_error("remove 要给 id");
    }
    for id in ctx.args.texts() {
        let tracked = lib.tracked_original(&id);
        ctx.report(lib.remove(&id).map(|title| format!("✓ 删除 {id}  {title}")).map_err(|e| format!("✗ {e}")));
        if let Some(p) = tracked.filter(|_| !lib.entry_exists(&id)) {
            println!("  ⚠ 这本的原件在跟踪的目录里：{}\n    下次 sync 会再入库；要彻底不要，就从书目录里删掉原件", p.display());
        }
    }
}

fn cmd_meta(ctx: &Ctx) {
    if ctx.args.has("show") { meta_show(ctx) } else { meta_fetch(ctx) }
}

/// `meta --show`：跟踪目录里的书的元数据。
fn meta_show(ctx: &Ctx) {
    let lib = ctx.lib;
    let tracked = lib.tracked();
    let books: Vec<_> = lib.select(&ctx.args.texts()).into_iter().filter(|m| !m.source_path.is_empty() && tracked.iter().any(|d| Path::new(&m.source_path).starts_with(d))).collect();
    if books.is_empty() {
        fail(if tracked.is_empty() { "没有跟踪的目录（booklib track 目录）" } else { "跟踪目录里没有匹配的书（booklib list 查看书库）" });
    }
    for (i, m) in books.iter().enumerate() {
        if i > 0 {
            println!();
        }
        show_meta(m, &mut |e| ctx.fail_line(&e));
    }
    println!("\n共 {} 本（跟踪目录：{}）", books.len(), tracked.iter().map(|d| d.display().to_string()).collect::<Vec<_>>().join("、"));
}

/// `meta --fetch [--force] [--clear]`：联网补元数据（或去掉找来的）。
fn meta_fetch(ctx: &Ctx) {
    let lib = ctx.lib;
    let books = lib.select(&ctx.args.texts());
    if books.is_empty() {
        fail("没有匹配的书（booklib list 查看书库）");
    }
    let (force, clear) = (ctx.args.has("force"), ctx.args.has("clear"));
    for m in &books {
        if !m.supported() && !clear {
            println!("- 跳过 {}：{}", m.title, library::unsupported(m.content_format()));
            continue;
        }
        if lib.offline() {
            ctx.fail_line("✗ 连不上网，这一轮中止（没查的书下次再查）");
            break;
        }
        if clear {
            ctx.report(lib.clear_metadata(m).map(|had| if had { format!("✓ 去掉找来的元数据和封面  {}", m.title) } else { format!("= 本来就没有找来的元数据  {}", m.title) }));
            continue;
        }
        // 漫画也找（2026-10-12 用户定，推翻 2026-09-30 的「漫画不找」）：书名的卷次去掉再搜（`matching::base_title`）
        match lib.fetch_metadata(m, force) {
            Ok((info, cover)) => {
                // 一半有结果、一半网络出错没查成：有结果的那半已存下，整本算失败（退出码 2），下次再查没查成的
                let partial = matches!(info, InfoResult::Failed(_)) || matches!(cover, CoverResult::Failed(_));
                let info = match info {
                    InfoResult::Found(i) => format!("元数据 ← {}", info_summary(&i)),
                    InfoResult::Shared(i, from) => format!("元数据 ← 同一套的《{from}》找过的（{}）", info_summary(&i)),
                    InfoResult::Existing(i) => format!("元数据 = 已有 ← {}", info_summary(&i)),
                    InfoResult::NotFound(why) => format!("元数据 ? {why}"),
                    InfoResult::Failed(why) => format!("元数据 ✗ {why}"),
                };
                let cover = match cover {
                    CoverResult::Found(c) => format!("封面 ← {}（{}）", c.work, c.source_url),
                    CoverResult::Existing(c) => format!("封面 = 已有 ← {}", c.work),
                    CoverResult::HasCover => "封面 = 书里有".to_string(),
                    CoverResult::Generated(c, why) => format!("封面 ◇ {why}，{}", c.work),
                    CoverResult::Failed(why) => format!("封面 ✗ {why}"),
                };
                let mark = if partial { "⚠" } else { "✓" };
                ctx.report(if partial { Err } else { Ok }(format!("{mark} {}\n      {info}\n      {cover}", m.title)));
            }
            Err(e) => ctx.report(Err(format!("✗ {}: {e}", m.title))),
        }
    }
    if !clear {
        println!("  简介、标签、作者国籍、译者、出版社、ISBN、封面在生成产物时补进书里（书里已有的不动）：booklib sync 会把这些书判为过期并重建");
    }
}

/// `sync` 的选项（解析时就把用错的写法挡掉）。
struct SyncPlan<'a> {
    selectors: Vec<String>,
    force: bool,
    /// `--watch` 的间隔秒数。
    watch: Option<u64>,
    /// 要生成的模式；`--no-build` 时为空。
    devices: Vec<&'a Profile>,
    prune: library::Prune,
    /// 有跟踪目录（没有时只生成 add 进来的书）。
    tracking: bool,
    /// 生成前给还没找过元数据的书联网找一次（`--no-meta` 关掉）。
    meta: bool,
}

impl<'a> SyncPlan<'a> {
    fn parse(ctx: &Ctx<'a>) -> SyncPlan<'a> {
        let (args, lib) = (ctx.args, ctx.lib);
        let selectors = args.texts();
        let force = args.has("force");
        let watch: Option<u64> = match (args.opt("watch"), args.has("watch")) {
            (Some(v), _) => Some(v.parse().ok().filter(|&n| n > 0).unwrap_or_else(|| usage_error("--watch= 要写正整数秒数"))),
            (None, true) => Some(60),
            (None, false) => None,
        };
        let devices = if args.has("no-build") {
            if args.opts.iter().any(|(k, _)| k == "device") {
                usage_error("--no-build 和 --device 不能一起用");
            }
            if force || !selectors.is_empty() {
                usage_error(&format!("--no-build 不生成，不能和 --force、书名一起用{}", path_hint(&selectors)));
            }
            Vec::new()
        } else {
            parse_devices(args, lib)
        };
        if watch.is_some() && (force || !selectors.is_empty()) {
            usage_error(&format!("--watch 每轮同步、生成全部书，不能和 --force、书名一起用{}", path_hint(&selectors)));
        }
        let prune = if args.has("keep") { library::Prune::Keep } else { library::Prune::Auto };
        // 没跟踪目录时只生成 add 进来的书；什么都没有（或只要同步）就提示先 track
        let tracking = !lib.tracked().is_empty();
        if !tracking && (devices.is_empty() || watch.is_some() || (selectors.is_empty() && lib.list().is_empty())) {
            fail("还没有跟踪任何目录。先登记要跟踪的书目录（只需一次），例如：\n  booklib track ~/Documents/ereader/books\n之后 booklib sync 就会把它镜像进书库");
        }
        SyncPlan { selectors, force, watch, devices, prune, tracking, meta: !args.has("no-meta") }
    }
}

/// `sync` 跨轮次的记忆（`--watch`；不是 `--watch` 时只有一轮）：报过的问题不重复报；生成失败的书没变化不重试；
/// 书库没变化、这一轮也没有新增更新删除时不跑生成（省得每轮把所有书的状态查一遍）。
#[derive(Default)]
struct SyncState {
    memo: SyncMemo,
    fails: FailMemo,
    last_stamp: Option<u64>,
    /// 没接上的设备报过了（`--watch` 时只在接上又拔掉以后再报）。
    absent_reported: HashSet<String>,
    /// 上一轮接着哪些设备（和 `SyncPlan::devices` 一一对应；接上新设备也要跑一轮生成）。
    last_live: Option<Vec<bool>>,
    /// 提示过的不再支持的格式的书。
    skipped: HashSet<String>,
    /// 报过「连不上网，元数据下次再找」（`--watch` 时连上网以前只报一次）。
    meta_offline_reported: bool,
}

fn cmd_sync(ctx: &Ctx) {
    let plan = SyncPlan::parse(ctx);
    let mut state = SyncState::default();
    loop {
        let dropped = ctx.lib.refresh_devices();
        match ctx.lib.lock() {
            Ok(_guard) => state.round(ctx, &plan, &dropped),
            Err(e) => ctx.fail_line(&e), // --watch 时只计数，下一轮再试
        }
        let Some(secs) = plan.watch else { break };
        std::thread::sleep(std::time::Duration::from_secs(secs));
    }
}

impl SyncState {
    /// 一轮（持着锁）：先同步跟踪目录，再生成并传。`dropped` 是这轮开头发现断了的 Move。
    fn round(&mut self, ctx: &Ctx, plan: &SyncPlan, dropped: &[String]) {
        let changed = if plan.tracking { self.sync_sources(ctx, plan) } else { true };
        if plan.meta {
            self.fill_metadata(ctx, plan);
        }
        if !plan.devices.is_empty() {
            self.build(ctx, plan, changed, dropped);
        }
    }

    /// 同步跟踪目录，逐条报。返回这一轮有没有新增、更新、删除（出错也算有，接着去生成）。
    fn sync_sources(&mut self, ctx: &Ctx, plan: &SyncPlan) -> bool {
        let r = ctx.lib.sync_with(plan.prune, &mut self.memo, |ev| match ev {
            SyncEvent::Added(p, m) => println!("✓ 入库 {}  {}  ({})", m.id, m.title, p.display()),
            SyncEvent::Updated(p, m, old) => println!("↻ 更新 {}  {} ← {old}  ({})", m.id, m.title, p.display()),
            SyncEvent::Missing(p, t, true) => println!("✗ 原件已删，书库里也删了（连同产物）  {t}  ({})", p.display()),
            SyncEvent::Missing(p, t, false) => println!("? 原件不在了（--keep，这次没从书库删）  {t}  ({})", p.display()),
            SyncEvent::Released(p, t, true) => println!("✗ 不再跟踪，书库里删了（连同产物）  {t}  ({})", p.display()),
            SyncEvent::Released(p, t, false) => println!("? 不再跟踪（--keep，这次没从书库删）  {t}  ({})", p.display()),
            SyncEvent::Failed(p, e) => ctx.fail_line(&format!("✗ {}: {e}", p.display())),
            // 算失败（退出码 2）：这次同步不完整
            SyncEvent::Unreadable(p, e) => ctx.fail_line(&format!("✗ 目录读不了，里面的书这次没同步（已登记的原样保留，不删）：{}（{e}）", p.display())),
        });
        let changed = r.as_ref().map_or(true, |r| r.added + r.updated + r.pruned > 0);
        match r {
            // --watch 时没变化就不出声
            Ok(r) if plan.watch.is_some() && r.added + r.updated + r.missing + r.released + r.failed + r.unreadable == 0 => {}
            Ok(r) => {
                let unreadable = if r.unreadable > 0 { format!("，读不了的目录 {}", r.unreadable) } else { String::new() };
                let released = if r.released > 0 { format!("，不再跟踪 {}", r.released) } else { String::new() };
                println!("原件：新增 {}，改过 {}，没变 {}，不在了 {}{released}（从书库删了 {}），出错 {}{unreadable}", r.added, r.updated, r.unchanged, r.missing, r.pruned, r.failed)
            }
            Err(e) => ctx.report(Err(e)),
        }
        changed
    }

    /// 补元数据（2026-10-12 用户定：跟踪 → 补元数据 → 传设备）：还没找过的书联网找一次，找到的生成时补进书里。
    /// 找过的（找到了、或者这一版匹配规则下没找到）不再找，`--watch` 每轮不重复联网；连不上网整轮跳过，下次再找。
    fn fill_metadata(&mut self, ctx: &Ctx, plan: &SyncPlan) {
        let lib = ctx.lib;
        let books = if plan.selectors.is_empty() { lib.list() } else { lib.select(&plan.selectors) };
        let todo: Vec<&library::Meta> = books.iter().filter(|m| m.supported() && lib.needs_metadata(m)).collect();
        if todo.is_empty() {
            return;
        }
        if lib.offline() {
            if !self.meta_offline_reported {
                println!("- 连不上网，{} 本书的元数据下次再找", todo.len());
                self.meta_offline_reported = true;
            }
            return;
        }
        self.meta_offline_reported = false;
        for m in todo {
            // 找到过、但早先没记网站书名的：重找（force）
            match lib.fetch_metadata(m, m.info.is_some()) {
                Ok((InfoResult::Found(i), _)) => println!("✓ 元数据 {} ← {}", m.title, info_summary(&i)),
                Ok((InfoResult::Shared(_, from), _)) => println!("✓ 元数据 {} ← 同一套的《{from}》找过的", m.title),
                Ok((InfoResult::NotFound(why), _)) => println!("? 元数据 {}：{why}", m.title),
                Ok((InfoResult::Failed(why), _)) => println!("⚠ 元数据 {}：{why}", m.title),
                Ok(_) => {}
                // 网络出错：这一轮不再找，下次再试（不算失败：生成照常）
                Err(e) => {
                    println!("- 元数据没找完（{e}），下次再找");
                    break;
                }
            }
        }
    }

    /// 生成并传：这一轮同步有变化、书库或生成记录变了、接着的设备变了才做。
    fn build(&mut self, ctx: &Ctx, plan: &SyncPlan, changed: bool, dropped: &[String]) {
        let lib = ctx.lib;
        let stamp = || lib.change_stamp();
        let live: Vec<bool> = plan.devices.iter().map(|d| lib.device_status(d).is_ok()).collect();
        for (d, ok) in plan.devices.iter().zip(&live) {
            if *ok {
                self.absent_reported.remove(&d.id);
            }
        }
        forget_reconnected(&mut self.fails, &plan.devices, self.last_live.as_deref(), &live, dropped);
        if !(changed || self.last_stamp != Some(stamp()) || self.last_live.as_ref() != Some(&live)) {
            return;
        }
        self.last_live = Some(live);
        // 没选书：全部书，只报有变化的；选了书：只生成这几本，每本都报
        let books = if plan.selectors.is_empty() { lib.list() } else { lib.select(&plan.selectors) };
        if books.is_empty() && !plan.selectors.is_empty() {
            fail(&format!("没有匹配的书（booklib list 查看书库）{}", path_hint(&plan.selectors)));
        }
        let run = BuildRun {
            lib,
            devices: &plan.devices,
            books: &books,
            force: plan.force,
            keep: plan.prune == library::Prune::Keep,
            fails: plan.watch.is_some().then_some(&mut self.fails),
            skipped: &mut self.skipped,
            absent_reported: &mut self.absent_reported,
        };
        // 没选书：只报有变化的
        let quiet = plan.selectors.is_empty();
        let counts = library::build_all(run, &mut |ev| {
            if let Some(r) = event_line(ev, quiet) {
                ctx.report(r);
            }
        });
        // --watch 时没有生成、挪动、失败就不出声
        if plan.watch.is_none() || counts.any() {
            ctx.report(Ok(counts_summary(&counts)));
        }
        self.last_stamp = Some(stamp());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 设备从没接上到接上、Move 连接断过重连：这台记着的失败忘掉（下一轮重试）；别的设备的、一直接着的不动。
    #[test]
    fn failures_are_forgotten_when_a_device_comes_back() {
        let devices = [profile::get("ireader").unwrap(), profile::get("kindle").unwrap(), profile::get("xochitl").unwrap()];
        let memo = || -> FailMemo { devices.iter().map(|d| (("书".to_string(), d.id.clone()), ("指纹".to_string(), OriginalState::Present))).collect() };
        let left = |f: &FailMemo| -> Vec<String> {
            let mut v: Vec<String> = f.keys().map(|(_, d)| d.clone()).collect();
            v.sort();
            v
        };
        let mut f = memo();
        forget_reconnected(&mut f, &devices, Some(&[true, true, true]), &[true, true, true], &[]);
        assert_eq!(left(&f), ["ireader", "kindle", "xochitl"], "一直接着：都记着");
        forget_reconnected(&mut f, &devices, None, &[true, true, true], &[]);
        assert_eq!(left(&f).len(), 3, "第一轮：没有上一轮可比");
        forget_reconnected(&mut f, &devices, Some(&[false, true, true]), &[true, true, true], &[]);
        assert_eq!(left(&f), ["kindle", "xochitl"], "掌阅重新接上");
        let mut f = memo();
        forget_reconnected(&mut f, &devices, Some(&[true, false, true]), &[true, false, true], &["xochitl".to_string()]);
        assert_eq!(left(&f), ["ireader", "kindle"], "Move 掉过线（重连上了）；Kindle 还没接上，留着");
    }
}
