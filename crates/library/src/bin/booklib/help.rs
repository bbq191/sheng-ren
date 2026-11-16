//! 命令表与帮助文字：`booklib --help`、`booklib help <命令>`、`--version`，以及用法错时的提示。
//! 新增命令只在 [`COMMANDS`] 里加一项（帮助、用法检查、加锁、分派都从这张表查）。

use super::{
    always, check_meta, check_options, check_sync, check_vertical, meta_writes, never, usage_error,
    Args, Ctx,
};
use crate::books::{cmd_add, cmd_dedupe, cmd_devices, cmd_list, cmd_remove, cmd_vertical};
use crate::meta::cmd_meta;
use crate::sync::cmd_sync;
use crate::track::{cmd_track, cmd_untrack};

/// 一条命令：帮助（参数写法、一句话、详细说明）、认哪些选项、要不要持锁、怎么做。
/// 新增命令只在 [`COMMANDS`] 里加一项（帮助、用法检查、加锁、分派都从这张表查）。
pub(crate) struct Command {
    pub(crate) name: &'static str,
    /// 参数写法：`booklib [--library=目录] ` 之后的部分；meta 有三种写法。
    synopsis: &'static [&'static str],
    pub(crate) brief: &'static str,
    detail: &'static str,
    /// 认的 `--名字=值` 选项和 `--名字` 开关（`--library=` 各命令都认）。
    pub(crate) opts: &'static [&'static str],
    pub(crate) flags: &'static [&'static str],
    /// 用法检查（打开书库之前）：一般是 [`check_options`]；有额外规矩的命令自己写。
    pub(crate) check: fn(&Command, &Args),
    /// 会改书库、要持锁到结束（`sync` 每一轮自己加锁：`--watch` 时不能一直占着）。
    pub(crate) lock: fn(&Args) -> bool,
    pub(crate) run: fn(&Ctx),
}

pub(crate) const COMMANDS: &[Command] = &[
    Command {
        name: "sync",
        synopsis: &["sync [--device=<模式>[,<模式>…]] [--force] [--no-build] [--no-meta] [--keep] [--watch[=秒]] [书…]"],
        brief: "同步跟踪目录，按阅读模式生成优化过的书，直接传到接着的设备（日常用这一条）",
        detail: "\
参数：
  --device=<模式>  只处理这几个模式（可写多次或用逗号分隔；all 或不写 = 全部）
  --force          选中的书（没选就是全部）重新生成；和设备上一样的照样不传
  --no-build       只同步，不生成、不传（不能和 --device、--force、书一起用）
  --no-meta        这一次不联网补元数据
  --keep           这一次原件不在了、不再跟踪的只报告，不从书库和设备上删
  --watch[=秒]     一直运行，每隔几秒（缺省 60）看一次，原件有变化或接上设备就处理（不能和书、--force 一起用）
  书…              只处理这几本：书名片段、id，或原件路径（文件，或目录 = 下面所有的书）

同步与生成：
  第一步 同步：书库严格镜像跟踪的目录——新增的入库，改过的换成新版本，挪动、改名的按内容认出来；
    原件删了的连同设备上的那本一起删。跟踪目录整个不在、读不了的目录里的书不删。
  补元数据：还没找过的书（含漫画）联网找一次简介、标签、带国籍的作者、译者、出版社、ISBN（同 meta --fetch，找过的不再找），
    生成时补进书里；连不上网就跳过，下次再找。
  第二步 生成并传：只给接着的设备生成，传上去，电脑上不留产物：
    kindle   Kindle（USB）  KFX   存储根目录 documents/<子目录>/
    ireader  掌阅（USB）    EPUB  存储根目录 Documents/<子目录>/
    kobo     Kobo（USB）    KEPUB 存储根目录 books/<子目录>/
    hanvon   汉王（USB）    EPUB  存储根目录 Documents/<子目录>/
    bigme    Bigme（USB）   EPUB  存储根目录 Books/<子目录>/
    <子目录> 是原件在跟踪目录里所在的子目录。没接上的设备这次跳过，下次接上再传。
    没变化的书不重新生成；重新生成出来和设备上一样的不再传（Kindle 进度不丢）。

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
三种写法：
  带目录或书      登记跟踪（目录递归收 .epub、.cbz），之后 sync 镜像进书库、传到设备
  --root=目录     换书根目录，设备上的子目录按它算（设备上的书下次 sync 挪到新位置）
  什么都不写      按设备列出跟踪范围；--device= 只列这几台（all 或不写 = 全部设备）

参数：
  --device=模式,…  只给这几台（可写多次、逗号分隔；all 或不写 = 全部设备，包括以后加的模式）
  --top            这批路径的放法：目录以它自己的名字为最上一层，单本放根目录（见下）
  --root=目录      换书根目录；只能单独用，不能和路径、--device、--top 一起
  目录或书…        要跟踪的目录（递归）或单本，可以混着写；相对路径先按当前目录找，
                   找不到再按书根目录找（booklib track 优秀漫画/死亡筆記）

设备上放在哪（书根目录 ~/book；设备的存储根目录下再按各模式放）：
  书根下的第一层目录，加不加 --top 一样：
    track ~/book/科幻神话                → 科幻神话/…
    track --top ~/book/科幻神话          → 科幻神话/…
  第二层起，--top 去掉上层分类目录：
    track ~/book/优秀漫画/竹光侍          → 优秀漫画/竹光侍/…
    track --top ~/book/优秀漫画/竹光侍    → 竹光侍/…
  单本：
    track ~/book/文字/科幻/三体.epub      → 文字/科幻/三体
    track --top ~/book/文字/科幻/三体.epub → 三体（根目录）
  同一本书被一台设备的几个条目覆盖时，用最具体的那个（单本 > 深的目录 > 浅的目录）的放法。
  同一个路径每台设备可以有自己的放法（给 Kindle --top、给掌阅不带，各是各的）。
  书根目录所有设备共用，第一次 track 时定下（跟踪的目录；单本是它所在的目录），跟踪的都要在它里面。

同一路径再 track：
  同一种放法：设备范围取并集；任一次是全部设备就是全部设备。
  换放法：这次给的设备从原来的放法挪过来；不写 --device（全部设备）则整条换成这次的放法，
  写了 --device 只挪这几台，其他设备留在原来的放法上。
  track 只加设备、不缩小范围；要从某几台拿掉用 booklib untrack --device=模式 <目录或书>。

例子：
  booklib track ~/book                                          全部设备，照书库结构
  booklib track --device=ireader ~/book/优秀漫画/竹光侍           掌阅：优秀漫画/竹光侍/…
  booklib track --device=ireader --top ~/book/优秀漫画/竹光侍     掌阅：竹光侍/…
  booklib track --device=kindle --top ~/book/文字/科幻/三体.epub  Kindle：根目录
  booklib track --device=ireader                                只列掌阅的跟踪范围",
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
参数：
  --device=模式,…  只从这几台上拿掉（可写多次、逗号分隔；all 或不写 = 全部设备）
  目录或书…        同 track：相对路径先按当前目录找，找不到再按书根目录找

不再跟踪之后：
  不写 --device：整条去掉，里面没被别的跟踪条目覆盖的书下次 sync 从书库删掉（连同设备上的）。
  写了 --device：只从这几台的范围里拿掉，下次 sync 从这几台上删掉，别的设备照旧。
  跟踪目录里的单本不能单独去掉（要么 untrack 整个目录）。删之前想先看看：sync --keep 只报告、不删。

例子：
  booklib untrack ~/book/漫画/哆啦                      整条不跟踪了
  booklib untrack --device=ireader ~/book/文字/飘.epub  只从掌阅上拿掉",
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
参数：
  文件或网址…  要入库的 EPUB、CBZ 文件或网址；整个目录用 track

入库之后：
  add 进来的书 sync 照样生成、传，放各设备产物目录顶层；原件没了也不自动删，不要了用 remove。
  网址：抓网页正文做成 EPUB，存在书库里。同一路径的文件内容改了，再 add 一次换成新版本。

例子：
  booklib add 三体.epub 乱马01.cbz https://example.com/post",
        opts: &[],
        flags: &[],
        check: check_options,
        lock: always,
        run: cmd_add,
    },
    Command {
        name: "list",
        synopsis: &["list [书…]"],
        brief: "列出书，以及在各设备上的产物和生成记录是否最新（设备上的不核对文件）",
        detail: "\
参数：
  书…  书名片段、id 或原件路径；不写 = 全部

输出：
  每本一行（id、格式、书名、作者），下面每个模式一行：
    ✓ 最新   ⚠ 过期（下次 sync 会重新生成）   ? 未知（判断不了，或这个模式已经不在了）
    ✗ 不在这台的跟踪范围（下次 sync 从设备上删）
  可拔下的设备上的产物只比生成记录和当前计划，不核对设备上文件还在不在、也不看设备接没接上：
    ✓/⚠ 后面注明「未核对设备」。电脑上的产物照旧核对文件。

例子：
  booklib list
  booklib list 三体",
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
参数：
  --fetch  联网补元数据（豆瓣 → QQ 阅读 → Wikidata）：简介、标签、原作名，书里没封面的顺带找封面（找不到就生成）；漫画跳过。
           生成产物时只补书里没有的简介、标签、封面，书名作者和正文不动，原件不动。
           --force 重找已找过的；--clear 去掉找来的元数据和封面（找错了时）
  --show   查看跟踪目录里的书的元数据：书里写的，和 --fetch 找来的；只读
  --edit   查看、改写一个 EPUB 文件的元数据和封面（改文件本身；值给空字符串 = 删掉）。给书库 id 或书名时改那本书的原件，
           详见 booklib meta --edit --help
  书…      书名片段、id 或原件路径；不写 = 全部

例子：
  booklib meta --fetch
  booklib meta --fetch --force 雪人
  booklib meta --show 绍宋
  booklib meta --edit 书.epub --title 书名",
        opts: &[],
        flags: &["fetch", "force", "clear", "show"],
        check: check_meta,
        lock: meta_writes,
        run: cmd_meta,
    },
    Command {
        name: "remove",
        synopsis: &["remove <id>…"],
        brief: "从书库删书，连同设备上的产物（设备没接上的下次接上时删）。原件不动",
        detail: "\
参数：
  id…  用 list 里显示的完整 id（不做模糊匹配）

说明：
  原件在跟踪目录里的，下次 sync 会再入库：要彻底不要就删掉原件。

例子：
  booklib remove 3fa9c1e07b2d",
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
        detail: "\
参数：
  目录…  额外在这些目录里找原件；不写就只按 meta.json 记着的位置找

说明：
  找到原件的改成只存索引、删掉书库里的副本；找不到原件的保留副本并列出来。可以反复跑。

例子：
  booklib dedupe ~/Documents/ereader",
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
        detail: "\
说明：
  书库的 profiles/ 里放 <id>.toml 可以加模式或覆盖内置的参数（写法见 docs/devices.md）。

例子：
  booklib devices",
        opts: &[],
        flags: &[],
        check: check_options,
        lock: never,
        run: cmd_devices,
    },
    Command {
        name: "vertical",
        synopsis: &["vertical --on|--off|--auto <书或目录>…"],
        brief: "这本 / 这个目录下的书竖不竖排（默认横排，原书本来就竖排的仍竖排；覆盖按书、按目录）",
        detail: "\
参数：
  --on       强制竖排
  --off      强制横排（原书已竖排的也关掉）
  --auto     删掉覆盖，回到默认（横排；原书已竖排的仍竖排）
  书或目录…  给书：写 / 删这本书的覆盖；给目录：写进书库根的 vertical.toml（相对书根目录、前缀匹配），
             并把这个目录下现有的书逐本写上，以后入库到这个目录的书继承。最长前缀胜。

说明：
  只对开了 vertical_text 的设备（Kobo、汉王）有意义；漫画、固定版式书不竖排。
  默认横排；原书本来就竖排（样式或 OPF 写着 writing-mode / primary-writing-mode）的仍竖排。
  相对路径先按当前目录找，找不到按书根目录找。

例子：
  booklib vertical --on 技术      这个目录强制竖排
  booklib vertical --off 三体     这本强制横排
  booklib vertical --auto 技术    删掉目录覆盖，回到默认",
        opts: &[],
        flags: &["on", "off", "auto"],
        check: check_vertical,
        lock: always,
        run: cmd_vertical,
    },
];

/// `booklib --help` 的头尾：一句话说明、用法、命令一览、通用参数、环境变量、退出码、例子。
const HELP_HEAD: &str = "\
booklib —— 电子书书库：跟踪书目录，按 Kindle、掌阅、Kobo、汉王、Bigme 的自带阅读器优化，直接传到接着的设备

用法：booklib [--library=目录] <命令> [参数]

命令：";

const HELP_TAIL: &str = "\
通用参数：
  --library=目录   书库目录（缺省 $BOOKLIB_DIR，没设就是 ~/.local/share/booklib）
  -h, --help       帮助；booklib <命令> --help（或 booklib help <命令>）只看这个命令的详细说明
  -V, --version    版本：提交号、各项规则的版本
  --               之后的参数都当书名、路径（以 - 开头的书名用）

环境变量：
  BOOKLIB_DIR       书库目录
  BOOKLIB_DEVICES_DIR  设备的链接目录（缺省 $HOME/Devices）：链接名就是 Kindle、Bigme、Hanvon、Ireader、Kobo，
                    链接和挂载由 dotfiles 管，booklib 只读、不建也不删
  BOOKLIB_MTP_NO_MOUNT_CHECK=1  不查链接指向的挂载点挂没挂着（测试用普通目录当设备）
  BOOKLIB_DEVICE_TIMEOUT=秒  设备多久没回应算挂载卡死（缺省 30）：连接探测等 1 倍，传输、主线程碰设备没有进展等 2 倍；
                          连接、传输卡住只跳过那台；主线程那几步卡住时打一行、退出码 2 结束
  BOOKLIB_NO_NET=1  不联网（不找元数据、封面）

  BOOKLIB_MTP_DIR、BOOKLIB_USBMS_DIR 已废弃：只用来认旧生成记录里的前缀，不再用于找设备。

退出码：0 全部成功；1 用法错；2 有书处理失败（或书库打不开、没有匹配的书）

例子：
  booklib track ~/Documents/ereader/books    第一次：登记要跟踪的书目录
  booklib sync                               之后每次：同步书目录、优化、传到接着的设备
  booklib devices                            看设备接没接上";

/// 命令的参数写法，每种一行（`  booklib [--library=目录] …`）。
pub(crate) fn synopsis(h: &Command) -> String {
    h.synopsis
        .iter()
        .map(|s| format!("  booklib [--library=目录] {s}"))
        .collect::<Vec<_>>()
        .join("\n")
}

/// `booklib --help` 的全文。
fn full_help() -> String {
    let cmds: Vec<String> = COMMANDS
        .iter()
        .map(|h| format!("{}\n      {}", synopsis(h), h.brief))
        .collect();
    format!("{HELP_HEAD}\n{}\n\n{HELP_TAIL}", cmds.join("\n"))
}

/// 一个命令的详细说明。
fn command_usage(cmd: &str) -> Option<String> {
    let h = COMMANDS.iter().find(|h| h.name == cmd)?;
    let detail = if h.detail.is_empty() {
        String::new()
    } else {
        format!("\n\n{}", h.detail)
    };
    Some(format!("用法：\n{}\n\n{}{detail}", synopsis(h), h.brief))
}

/// `booklib --version`、`-V`、`version`。
fn version() -> String {
    let date = env!("BOOKLIB_GIT_DATE");
    let rules: Vec<String> = library::rule_versions()
        .into_iter()
        .map(|(k, v)| format!("{k} {v}"))
        .collect();
    format!(
        "booklib {}（提交 {}{}）\n规则版本：{}",
        env!("CARGO_PKG_VERSION"),
        env!("BOOKLIB_GIT_COMMIT"),
        if date.is_empty() {
            String::new()
        } else {
            format!("，{date}")
        },
        rules.join(" · ")
    )
}

/// 命令行里的命令名（跳过 `--library=` 等选项；`--` 之前）。
pub(crate) fn command_word() -> Option<String> {
    std::env::args_os()
        .skip(1)
        .map(|a| a.to_string_lossy().into_owned())
        .take_while(|a| a != "--")
        .find(|a| !a.starts_with('-'))
}

/// `booklib --help`、`-h`、`help` 打印全部用法；`booklib <命令> --help`（或 `-h`）、`booklib help <命令>` 只打印这个命令的；
/// `--version`、`-V`、`version` 打印版本。打到标准输出、退出码 0（不算用错）。`--` 之后的参数不算。
pub(crate) fn print_help_if_asked() {
    let raw: Vec<String> = std::env::args_os()
        .skip(1)
        .map(|a| a.to_string_lossy().into_owned())
        .collect();
    let args: Vec<&str> = raw
        .iter()
        .map(String::as_str)
        .take_while(|a| *a != "--")
        .filter(|a| !a.starts_with("--library="))
        .collect();
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
pub(crate) const BUILD_MERGED: &str = "build 已并入 sync：booklib sync [--device=…] [--force] [书...]";
