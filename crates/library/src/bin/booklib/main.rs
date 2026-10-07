//! 书库命令行：入库、跟踪与同步（含按阅读模式生成）、列出、删除、去重。见 `library` crate 头注释。
//!
//! 书库目录缺省 $BOOKLIB_DIR 或 ~/.local/share/booklib。产物直接传到接着的设备上（见 `library::generate`、`library::deliver`）：
//! Kindle、掌阅放 documents/（镜像子目录），Move 加入 xochitl；没写 [deliver] 的自定义模式才放电脑上。
//! `meta --edit` 改单个 EPUB 文件、不碰书库，在 `meta_edit` 里（参数规则也不同：值可以是空字符串）。
//! 退出码: 0 全部成功；1 用法错；2 有书处理失败（或书库打不开、没有匹配的书）。

mod meta_edit;

use library::{Added, Built, CoverResult, InfoResult, Library, OriginalState, Profile, SyncEvent, SyncMemo};
use std::cell::Cell;
use std::collections::{HashMap, HashSet};
use std::ffi::OsString;
use std::path::{Path, PathBuf};

/// 一个命令的说明：参数写法（`booklib [--library=目录] ` 之后的部分；meta 有三种写法）、一句话、详细说明。
struct Help {
    name: &'static str,
    synopsis: &'static [&'static str],
    brief: &'static str,
    detail: &'static str,
}

const HELP: &[Help] = &[
    Help {
        name: "sync",
        synopsis: &["sync [--device=<模式>[,<模式>…]] [--force] [--no-build] [--keep] [--watch[=秒]] [书…]"],
        brief: "同步跟踪目录，按阅读模式生成优化过的书，直接传到接着的设备（日常用这一条）",
        detail: "\
第一步 同步：书库严格镜像跟踪的目录——新增的入库，改过的换成新版本，挪动、改名的按内容认出来；
  原件删了的连同设备上的那本一起删（Move 上进回收站）。跟踪目录整个不在、读不了的目录里的书不删。
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
  --keep           这一次原件不在了的只报告，不从书库删
  --watch[=秒]     一直运行，每隔几秒（缺省 60）看一次，原件有变化或接上设备就处理（不能和书、--force 一起用）
  书…              只处理这几本：书名片段、id，或原件路径（文件，或目录 = 下面所有的书）

例子：
  booklib sync                         全部
  booklib sync --device=kindle 三体     只给 Kindle、只处理书名含「三体」的
  booklib sync --watch=300             挂着，每 5 分钟看一次",
    },
    Help {
        name: "track",
        synopsis: &["track <目录>…"],
        brief: "跟踪书目录（递归，收 .epub、.cbz）：之后 sync 把它镜像进书库",
        detail: "只需登记一次。和已跟踪的目录互相包含（父目录或子目录）的会被拒绝。",
    },
    Help { name: "untrack", synopsis: &["untrack <目录>…"], brief: "不再跟踪这个目录（已入库的书和设备上的产物都保留）", detail: "" },
    Help {
        name: "add",
        synopsis: &["add <文件或网址>…"],
        brief: "入库单个 EPUB、CBZ 文件或网址（整个目录用 track）",
        detail: "\
add 进来的书 sync 照样生成、传（放设备 documents/ 顶层，Move 上放书库根）；原件没了也不自动删，不要了用 remove。
网址：抓网页正文做成 EPUB，存在书库里。同一路径的文件内容改了，再 add 一次换成新版本。",
    },
    Help {
        name: "list",
        synopsis: &["list [书…]"],
        brief: "列出书，以及在各设备上的产物是否最新",
        detail: "\
书：书名片段、id 或原件路径；不写 = 全部。每本一行（id、格式、书名、作者），下面每个模式一行：
  ✓ 最新   ⚠ 过期（下次 sync 会重新生成）   ? 未知（设备没接上，或设备上那本不在了）
Move 上的显示成 xochitl:文件夹/文件名（只看记录，不连 Move）。",
    },
    Help {
        name: "meta",
        synopsis: &["meta --fetch [--force] [--clear] [书…]", "meta --show [书…]", "meta --edit 书.epub [--title 书名 --author 作者 --tag 标签 --cover 图 …]"],
        brief: "书的元数据：联网补、查看、改写一个 EPUB",
        detail: "\
--fetch  联网补元数据（豆瓣 → Wikidata）：简介、标签、原作名，书里没封面的顺带找封面（找不到就生成）；漫画跳过。
         生成产物时只补书里没有的简介、标签、封面，书名作者和正文不动，原件不动。
         --force 重找已找过的；--clear 去掉找来的元数据和封面（找错了时）
--show   查看跟踪目录里的书的元数据：书里写的，和 --fetch 找来的；只读
--edit   查看、改写一个 EPUB 文件的元数据和封面（改文件本身，和书库无关；值给空字符串 = 删掉），
         详见 booklib meta --edit --help",
    },
    Help {
        name: "remove",
        synopsis: &["remove <id>…"],
        brief: "从书库删书，连同设备上的产物（Move 上进回收站；设备没接上的下次接上时删）。原件不动",
        detail: "id 用 list 里显示的完整 id（不做模糊匹配）。原件在跟踪目录里的，下次 sync 会再入库：要彻底不要就删掉原件。",
    },
    Help {
        name: "dedupe",
        synopsis: &["dedupe [目录…]"],
        brief: "早期版本入库的书改成只存索引（在记着的位置和这些目录里找原件）",
        detail: "找不到原件的保留书库里的副本并列出来。可以反复跑。",
    },
    Help {
        name: "devices",
        synopsis: &["devices"],
        brief: "列出阅读模式，以及设备接没接上（只看，不往设备上写）",
        detail: "书库的 profiles/ 里放 <id>.toml 可以加模式或覆盖内置的参数（写法见 docs/devices.md）。",
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

退出码：0 全部成功；1 用法错；2 有书处理失败（或书库打不开、没有匹配的书）";

/// 命令的参数写法，每种一行（`  booklib [--library=目录] …`）。
fn synopsis(h: &Help) -> String {
    h.synopsis.iter().map(|s| format!("  booklib [--library=目录] {s}")).collect::<Vec<_>>().join("\n")
}

/// `booklib --help` 的全文。
fn full_help() -> String {
    let cmds: Vec<String> = HELP.iter().map(|h| format!("{}\n      {}", synopsis(h), h.brief)).collect();
    format!("{HELP_HEAD}\n\n命令：\n{}\n\n{HELP_TAIL}", cmds.join("\n"))
}

/// 一个命令的详细说明。
fn command_usage(cmd: &str) -> Option<String> {
    let h = HELP.iter().find(|h| h.name == cmd)?;
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
    match command_word().and_then(|c| HELP.iter().find(|h| h.name == c)) {
        Some(h) => eprintln!("用法：\n{}\n\n详细说明：booklib {} --help", synopsis(h), h.name),
        None => {
            let names: Vec<&str> = HELP.iter().map(|h| h.name).collect();
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
type FailMemo = HashMap<(String, String), (String, OriginalState)>;

/// 按模式 × 书逐本生成并送到设备上（没变化的跳过），每本一行结果。`quiet` 时不打印没变化的（没选书时：只报有变化的）。
/// 设备没接上的模式跳过，报一行（`absent_reported` 记着报过的，`--watch` 时不重复报）。
/// 给了 `fails`（`sync --watch`）：上次失败以后指纹和原件状态都没变的书跳过。
/// 不再支持的格式（早期版本收的 MOBI/PDF 等）跳过，每本只提示一次（`skipped` 记着提示过的）。
/// `build_all` 的结果计数（结尾打一行汇总：用户 2026-09-29 反馈"看不出是否更新过"）。
#[derive(Default)]
struct BuildCounts {
    written: usize,
    same: usize,
    moved: usize,
    up_to_date: usize,
    failed: usize,
}

impl BuildCounts {
    fn summary(&self) -> String {
        let same = if self.same > 0 { format!("，重新生成但内容没变（没再传）{} 本", self.same) } else { String::new() };
        format!("生成：重新生成并传上 {} 本{same}，挪位置 {} 本，已是最新 {} 本，失败 {} 本", self.written, self.moved, self.up_to_date, self.failed)
    }
    fn any(&self) -> bool {
        self.written + self.same + self.moved + self.failed > 0
    }
}

#[allow(clippy::too_many_arguments)]
fn build_all(lib: &Library, devices: &[&Profile], books: &[library::Meta], force: bool, quiet: bool, mut fails: Option<&mut FailMemo>, skipped: &mut HashSet<String>, absent_reported: &mut HashSet<String>, report: &mut impl FnMut(Result<String, String>)) -> BuildCounts {
    let mut counts = BuildCounts::default();
    for m in books.iter().filter(|m| !m.supported()) {
        if skipped.insert(m.id.clone()) {
            report(Ok(format!("- 跳过 {}  {}：{}", m.id, m.title, library::unsupported(m.content_format()))));
        }
    }
    // 产物直接送设备（profile 的 [deliver]）：没接上的设备这一轮整个跳过（每轮报一次），接上的先删掉以前删书时没删成的
    let mut live: Vec<&Profile> = Vec::new();
    for device in devices {
        match lib.device_status(device) {
            Ok(_) => {
                match lib.flush_removed(device) {
                    Ok(0) => {}
                    Ok(n) => report(Ok(format!("✓ [{}] 删掉了 {n} 本已从书库删除的书", device.id))),
                    Err(e) => report(Err(format!("✗ [{}] 删已从书库删除的书：{e}", device.id))),
                }
                live.push(device);
            }
            Err(e) => {
                // --watch（给了 fails）时同一台没接上只报一次
                if fails.is_none() || !absent_reported.contains(&device.id) {
                    report(Ok(format!("- [{}] {e}，这次不传", device.id)));
                }
                absent_reported.insert(device.id.clone());
            }
        }
    }
    // 书在外层、模式在内层：同一本书与模式无关的中间文件（CBZ 转换、补元数据）只做一次
    for m in books.iter().filter(|m| m.supported()) {
        for device in &live {
            let key = (m.id.clone(), device.id.clone());
            let now = || (lib.fingerprint(m, device).unwrap_or_else(|e| e), lib.original_state(m));
            if let Some(f) = fails.as_deref_mut() {
                match f.get(&key).map(|v| *v == now()) {
                    Some(true) => continue,
                    Some(false) => {
                        f.remove(&key);
                    }
                    None => {}
                }
            }
            let built = match lib.build(m, device, force) {
                Ok(Built::Written { path, warnings }) => {
                    counts.written += 1;
                    Some(std::iter::once(format!("✓ 生成 [{}] {} → {}", device.id, m.title, path.display())).chain(warnings.iter().map(|w| format!("  ⚠ {w}"))).collect::<Vec<_>>().join("\n"))
                }
                Ok(Built::Same { path, warnings }) => {
                    counts.same += 1;
                    Some(std::iter::once(format!("≡ 重新生成 [{}] {}：和设备上的一样，没再传（{}）", device.id, m.title, path.display())).chain(warnings.iter().map(|w| format!("  ⚠ {w}"))).collect::<Vec<_>>().join("\n"))
                }
                Ok(Built::UpToDate(path)) => {
                    counts.up_to_date += 1;
                    (!quiet).then(|| format!("= [{}] {} 已是最新（{}）", device.id, m.title, path.display()))
                }
                Ok(Built::Moved { from, to }) => {
                    counts.moved += 1;
                    Some(format!("↪ 挪位置 [{}] {} → {}（原来在 {}）", device.id, m.title, to.display(), from.display()))
                }
                Err(e) => {
                    counts.failed += 1;
                    report(Err(format!("✗ [{}] {}: {e}", device.id, m.title)));
                    if let Some(f) = fails.as_deref_mut() {
                        f.insert(key, now());
                    }
                    continue;
                }
            };
            if let Some(line) = built {
                report(Ok(line));
            }
        }
    }
    lib.release_prepared();
    counts
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

/// `booklib [--library=…] meta … --edit …`：返回 `meta` 之后的参数（去掉第一个 `--edit` 和 `--library=`），交给 `meta_edit`。
/// 它在通用解析之前分出去：通用解析不许 `--名字=` 留空，而 `--edit` 的空值表示删掉。
fn meta_edit_args() -> Option<Vec<OsString>> {
    let is_library = |a: &OsString| a.to_str().is_some_and(|s| s.starts_with("--library="));
    let raw: Vec<OsString> = std::env::args_os().skip(1).collect();
    let i = raw.iter().position(|a| !is_library(a))?;
    if raw[i] != "meta" {
        return None;
    }
    let mut rest: Vec<OsString> = raw[i + 1..].iter().filter(|a| !is_library(a)).cloned().collect();
    let e = rest.iter().position(|a| a == "--edit")?;
    rest.remove(e);
    Some(rest)
}

fn main() {
    bookconv::util::restore_sigpipe();
    if let Some(rest) = meta_edit_args() {
        meta_edit::run(rest);
        return;
    }
    print_help_if_asked();
    let args = Args::parse();
    let Some(cmd) = args.pos.first().and_then(|c| c.to_str()).map(str::to_string) else { usage_error("") };
    match cmd.as_str() {
        "add" | "remove" | "dedupe" | "list" | "devices" | "track" | "untrack" => args.check(&cmd, &[], &[]),
        "build" => usage_error(BUILD_MERGED),
        "sync" if args.flags.iter().any(|f| f == "prune") => usage_error("sync 现在总会清理（原件删了的连同产物从书库删掉），不用 --prune；这一次不想删就加 --keep"),
        "sync" => args.check(&cmd, &["device", "watch"], &["keep", "watch", "no-build", "force"]),
        "meta" => {
            args.check(&cmd, &[], &["fetch", "force", "clear", "show"]);
            match (args.flags.iter().any(|f| f == "fetch"), args.flags.iter().any(|f| f == "show")) {
                (true, true) => usage_error("meta 的 --fetch 和 --show 不能一起用"),
                (false, true) if args.flags.iter().any(|f| f == "force" || f == "clear") => usage_error("--force、--clear 只配 --fetch"),
                (false, false) => usage_error("meta 要选一种：--fetch（联网给书库里的书补元数据）、--show（查看跟踪目录里的书的元数据）或 --edit 书.epub（查看、改写一个 EPUB 文件）"),
                _ => {}
            }
        }
        _ => usage_error(&format!("不认识的命令 {cmd}")),
    }
    let root = args.opt("library").map(PathBuf::from).unwrap_or_else(Library::default_root);
    let lib = Library::open(&root).unwrap_or_else(|e| fail(&format!("打开书库失败: {e}")));
    // 会改动书库的命令持锁到结束
    let _lock = match cmd.as_str() {
        "list" | "devices" | "sync" => None, // sync 每一轮自己加锁（--watch 时不能一直占着）
        "meta" if args.flags.iter().any(|f| f == "show") => None, // 只读
        _ => Some(lib.lock().unwrap_or_else(|e| fail(&e))),
    };
    let failed = Cell::new(0usize);
    let fail_line = |e: &str| {
        failed.set(failed.get() + 1);
        eprintln!("{e}");
    };
    let mut report = |r: Result<String, String>| match r {
        Ok(line) => println!("{line}"),
        Err(e) => fail_line(&e),
    };
    let mut skipped = HashSet::new();
    let rest = &args.pos[1..];
    match cmd.as_str() {
        "devices" => {
            for p in lib.devices().iter() {
                let r = p.output_readable();
                println!("{:<12} {}  {}  屏幕 {}×{}  阅读范围 {}×{}  {}", p.id, p.name, match p.comic_format.filter(|f| *f != p.format()) {
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
        "add" => {
            if rest.is_empty() {
                usage_error("add 要给文件或网址（目录用 track + sync）");
            }
            for item in rest {
                let text = item.to_string_lossy().into_owned();
                let path = Path::new(item);
                report(if text.starts_with("http://") || text.starts_with("https://") {
                    lib.add_url(&text).map(|a| added_line(&a)).map_err(|e| format!("✗ {text}: {e}"))
                } else if path.is_dir() {
                    Err(format!("✗ {text} 是目录：目录用 booklib track {text} 登记跟踪，再 booklib sync 入库（之后增删改都会同步）"))
                } else {
                    lib.add_file(path).map(|a| added_line(&a)).map_err(|e| format!("✗ {text}: {e}"))
                });
            }
        }
        "list" => {
            for m in lib.select(&args.texts()) {
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
                    println!("      {:<20} {state}  {}", o.device, shown.display());
                }
            }
            for id in lib.broken() {
                eprintln!("⚠ 条目 {id} 的 meta.json 读不出来（写坏了）：booklib remove {id} 删掉，或重新 add 原文件覆盖");
            }
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
                let d = Path::new(d);
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
            let has = |f: &str| args.flags.iter().any(|x| x == f);
            let selectors = args.texts();
            let force = has("force");
            let watch: Option<u64> = match (args.opt("watch"), has("watch")) {
                (Some(v), _) => Some(v.parse().ok().filter(|&n| n > 0).unwrap_or_else(|| usage_error("--watch= 要写正整数秒数"))),
                (None, true) => Some(60),
                (None, false) => None,
            };
            let devices = if has("no-build") {
                if args.opts.iter().any(|(k, _)| k == "device") {
                    usage_error("--no-build 和 --device 不能一起用");
                }
                if force || !selectors.is_empty() {
                    usage_error(&format!("--no-build 不生成，不能和 --force、书名一起用{}", path_hint(&selectors)));
                }
                Vec::new()
            } else {
                parse_devices(&args, &lib)
            };
            if watch.is_some() && (force || !selectors.is_empty()) {
                usage_error(&format!("--watch 每轮同步、生成全部书，不能和 --force、书名一起用{}", path_hint(&selectors)));
            }
            let prune = if has("keep") { library::Prune::Keep } else { library::Prune::Auto };
            // 没跟踪目录时只生成 add 进来的书；什么都没有（或只要同步）就提示先 track
            let tracking = !lib.tracked().is_empty();
            if !tracking && (devices.is_empty() || watch.is_some() || (selectors.is_empty() && lib.list().is_empty())) {
                fail("还没有跟踪任何目录。先登记要跟踪的书目录（只需一次），例如：\n  booklib track ~/Documents/ereader/books\n之后 booklib sync 就会把它镜像进书库");
            }
            // 跨轮次的记忆（--watch）：报过的问题不重复报；生成失败的书没变化不重试；
            // 书库没变化、这一轮也没有新增更新删除时不跑生成（省得每轮把所有书的状态查一遍）
            let mut memo = SyncMemo::default();
            let mut fails = FailMemo::new();
            let mut last_stamp: Option<u64> = None;
            // 没接上的设备报过了（--watch 时只在接上又拔掉以后再报）；上一轮接着哪些设备（接上新设备也要跑一轮生成）
            let mut absent_reported = HashSet::new();
            let mut last_live: Option<Vec<bool>> = None;
            loop {
                lib.refresh_devices();
                match lib.lock() {
                    Ok(_guard) => {
                        let mut changed = true;
                        if tracking {
                            let r = lib.sync_with(prune, &mut memo, |ev| match ev {
                                SyncEvent::Added(p, m) => println!("✓ 入库 {}  {}  ({})", m.id, m.title, p.display()),
                                SyncEvent::Updated(p, m, old) => println!("↻ 更新 {}  {} ← {old}  ({})", m.id, m.title, p.display()),
                                SyncEvent::Missing(p, t, true) => println!("✗ 原件已删，书库里也删了（连同产物）  {t}  ({})", p.display()),
                                SyncEvent::Missing(p, t, false) => println!("? 原件不在了（--keep，这次没从书库删）  {t}  ({})", p.display()),
                                SyncEvent::Failed(p, e) => fail_line(&format!("✗ {}: {e}", p.display())),
                                // 算失败（退出码 2）：这次同步不完整
                                SyncEvent::Unreadable(p, e) => fail_line(&format!("✗ 目录读不了，里面的书这次没同步（已登记的原样保留，不删）：{}（{e}）", p.display())),
                            });
                            changed = r.as_ref().map_or(true, |r| r.added + r.updated + r.pruned > 0);
                            match r {
                                // --watch 时没变化就不出声
                                Ok(r) if watch.is_some() && r.added + r.updated + r.missing + r.failed + r.unreadable == 0 => {}
                                Ok(r) => {
                                    let unreadable = if r.unreadable > 0 { format!("，读不了的目录 {}", r.unreadable) } else { String::new() };
                                    println!("原件：新增 {}，改过 {}，没变 {}，不在了 {}（从书库删了 {}），出错 {}{unreadable}", r.added, r.updated, r.unchanged, r.missing, r.pruned, r.failed)
                                }
                                Err(e) => report(Err(e)),
                            }
                        }
                        if !devices.is_empty() {
                            let stamp = || lib.change_stamp();
                            let live: Vec<bool> = devices.iter().map(|d| lib.device_status(d).is_ok()).collect();
                            for (d, ok) in devices.iter().zip(&live) {
                                if *ok {
                                    absent_reported.remove(&d.id);
                                }
                            }
                            if changed || last_stamp != Some(stamp()) || last_live.as_ref() != Some(&live) {
                                last_live = Some(live);
                                // 没选书：全部书，只报有变化的；选了书：只生成这几本，每本都报
                                let books = if selectors.is_empty() { lib.list() } else { lib.select(&selectors) };
                                if books.is_empty() && !selectors.is_empty() {
                                    fail(&format!("没有匹配的书（booklib list 查看书库）{}", path_hint(&selectors)));
                                }
                                let fails = watch.is_some().then_some(&mut fails);
                                let counts = build_all(&lib, &devices, &books, force, selectors.is_empty(), fails, &mut skipped, &mut absent_reported, &mut report);
                                // --watch 时没有生成、挪动、失败就不出声
                                if watch.is_none() || counts.any() {
                                    report(Ok(counts.summary()));
                                }
                                last_stamp = Some(stamp());
                            }
                        }
                    }
                    Err(e) => fail_line(&e), // --watch 时只计数，下一轮再试
                }
                let Some(secs) = watch else { break };
                std::thread::sleep(std::time::Duration::from_secs(secs));
            }
        }
        "meta" if args.flags.iter().any(|f| f == "show") => {
            let tracked = lib.tracked();
            let books: Vec<_> = lib.select(&args.texts()).into_iter().filter(|m| !m.source_path.is_empty() && tracked.iter().any(|d| Path::new(&m.source_path).starts_with(d))).collect();
            if books.is_empty() {
                fail(if tracked.is_empty() { "没有跟踪的目录（booklib track 目录）" } else { "跟踪目录里没有匹配的书（booklib list 查看书库）" });
            }
            for (i, m) in books.iter().enumerate() {
                if i > 0 {
                    println!();
                }
                show_meta(m, &mut |e| fail_line(&e));
            }
            println!("\n共 {} 本（跟踪目录：{}）", books.len(), tracked.iter().map(|d| d.display().to_string()).collect::<Vec<_>>().join("、"));
        }
        "meta" => {
            let books = lib.select(&args.texts());
            if books.is_empty() {
                fail("没有匹配的书（booklib list 查看书库）");
            }
            let (force, clear) = (args.flags.iter().any(|f| f == "force"), args.flags.iter().any(|f| f == "clear"));
            for m in &books {
                if !m.supported() && !clear {
                    println!("- 跳过 {}：{}", m.title, library::unsupported(m.content_format()));
                    continue;
                }
                if lib.offline() {
                    fail_line("✗ 连不上网，这一轮中止（没查的书下次再查）");
                    break;
                }
                if clear {
                    report(lib.clear_metadata(m).map(|had| if had { format!("✓ 去掉找来的元数据和封面  {}", m.title) } else { format!("= 本来就没有找来的元数据  {}", m.title) }));
                    continue;
                }
                // 漫画不联网找元数据（2026-09-30 用户定：豆瓣等按书名搜漫画多半对不上，书里的封面就是第一页）
                match lib.is_comic(m) {
                    Ok(true) => {
                        println!("- 跳过 {}：漫画不找元数据", m.title);
                        continue;
                    }
                    Ok(false) => {}
                    Err(e) => {
                        fail_line(&format!("✗ {}: {e}", m.title));
                        continue;
                    }
                }
                match lib.fetch_metadata(m, force) {
                    Ok((info, cover)) => {
                        // 一半有结果、一半网络出错没查成：有结果的那半已存下，整本算失败（退出码 2），下次再查没查成的
                        let partial = matches!(info, InfoResult::Failed(_)) || matches!(cover, CoverResult::Failed(_));
                        let info = match info {
                            InfoResult::Found(i) => format!("元数据 ← {}", info_summary(&i)),
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
                        report(if partial { Err } else { Ok }(format!("{mark} {}\n      {info}\n      {cover}", m.title)));
                    }
                    Err(e) => report(Err(format!("✗ {}: {e}", m.title))),
                }
            }
            if !clear {
                println!("  简介、标签、封面在生成产物时补进书里（书里已有的不动）：booklib sync 会把这些书判为过期并重建");
            }
        }
        "remove" => {
            if rest.is_empty() {
                usage_error("remove 要给 id");
            }
            for id in args.texts() {
                let tracked = lib.tracked_original(&id);
                report(lib.remove(&id).map(|title| format!("✓ 删除 {id}  {title}")).map_err(|e| format!("✗ {e}")));
                if let Some(p) = tracked.filter(|_| !lib.entry_exists(&id)) {
                    println!("  ⚠ 这本的原件在跟踪的目录里：{}\n    下次 sync 会再入库；要彻底不要，就从书目录里删掉原件", p.display());
                }
            }
        }
        _ => unreachable!(),
    }
    if failed.get() > 0 {
        std::process::exit(2);
    }
}
