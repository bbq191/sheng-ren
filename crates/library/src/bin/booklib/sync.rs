//! `sync`：同步跟踪目录、补元数据、生成并传；`--watch` 跨轮次的状态（失败记忆、空间不够、设备接没接上）。

use super::{fail, usage_error, Args, Ctx};
use crate::meta::info_summary;
use crate::report::{counts_summary, event_line, path_hint, WarningSummary};
use library::{BuildRun, FailMemo, InfoResult, Library, Profile, SyncEvent, SyncMemo};
use std::collections::{HashMap, HashSet};

/// `--device` 可写多次、可逗号分隔；all = 全部模式；一个都没写也是全部模式。写了不存在的模式直接报用法错。
pub(crate) fn parse_devices<'a>(args: &Args, lib: &'a Library) -> Vec<&'a Profile> {
    let mut devices: Vec<&Profile> = Vec::new();
    if !args.opts.iter().any(|(k, _)| k == "device") {
        return lib.devices().iter().collect();
    }
    for id in args
        .opts
        .iter()
        .filter(|(k, _)| k == "device")
        .flat_map(|(_, v)| v.split(','))
        .map(str::trim)
        .filter(|v| !v.is_empty())
    {
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
/// 设备重新接上时这台的全部忘掉（[`forget_reconnected`]）：传输失败多半是连接的事，接上了要重试。
/// 这一轮重新接上的设备（上一轮没接上、这轮接上了）：忘掉它们记着的失败。`last_live`、`live` 和 `devices` 一一对应。
pub(crate) fn forget_reconnected(
    fails: &mut FailMemo,
    devices: &[&Profile],
    last_live: Option<&[bool]>,
    live: &[bool],
) {
    for (i, (d, ok)) in devices.iter().zip(live).enumerate() {
        let was_down = last_live.and_then(|l| l.get(i)).is_some_and(|w| !w);
        if *ok && was_down {
            fails.retain(|(_, dev), _| *dev != d.id);
        }
    }
}

/// `sync --watch` 空间不够停下的设备（[`library::BuildRun::space_held`]）这一轮要不要跑：
/// 书库有变化（`lib_changed`：要传的书可能少了）全部忘掉、重查；重新接上的（或没接上的）忘掉，
/// 连同报过的"空间不够/读不到空间"（`reported` 里 `空间:`/`空间?:` 开头的键），接上重查时再报一次；
/// 还停着的里有剩余空间明显变多了的（[`library::space_grew`]，`free` 读现在的）返回 `true`，让书库没变化也跑一轮生成。
/// 不碰失败表：空间不够和书本身无关，失败表里的书（比如原件坏了）不该因此重试、重报（agy 复审）。
pub(crate) fn space_freed(
    held: &mut HashMap<String, Option<u64>>,
    reported: &mut HashSet<String>,
    lib_changed: bool,
    devices: &[&Profile],
    last_live: Option<&[bool]>,
    live: &[bool],
    free: impl Fn(&Profile) -> Option<u64>,
) -> bool {
    if lib_changed {
        held.clear();
    }
    let mut any = false;
    for (i, (d, ok)) in devices.iter().zip(live).enumerate() {
        let was_down = last_live.and_then(|l| l.get(i)).is_some_and(|w| !w);
        if !*ok || was_down {
            held.remove(&d.id);
            if !*ok {
                reported.remove(&format!("空间:{}", d.id));
                reported.remove(&format!("空间?:{}", d.id));
            }
        } else if let Some(&then) = held.get(&d.id) {
            any |= library::space_grew(then, free(d));
        }
    }
    any
}

/// 这一轮传的途中断开了的设备（`dropped`）记成上一轮没接上：下一轮接着的话 [`forget_reconnected`] 忘掉它的失败、整轮重跑。
pub(crate) fn mark_dropped(last_live: Option<&mut [bool]>, devices: &[&Profile], dropped: &[String]) {
    if let Some(l) = last_live {
        for (i, d) in devices.iter().enumerate() {
            if dropped.contains(&d.id) {
                if let Some(x) = l.get_mut(i) {
                    *x = false;
                }
            }
        }
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
            (Some(v), _) => Some(
                v.parse()
                    .ok()
                    .filter(|&n| n > 0)
                    .unwrap_or_else(|| usage_error("--watch= 要写正整数秒数")),
            ),
            (None, true) => Some(60),
            (None, false) => None,
        };
        let devices = if args.has("no-build") {
            if args.opts.iter().any(|(k, _)| k == "device") {
                usage_error("--no-build 和 --device 不能一起用");
            }
            if force || !selectors.is_empty() {
                usage_error(&format!(
                    "--no-build 不生成，不能和 --force、书名一起用{}",
                    path_hint(&selectors)
                ));
            }
            Vec::new()
        } else {
            parse_devices(args, lib)
        };
        if watch.is_some() && (force || !selectors.is_empty()) {
            usage_error(&format!(
                "--watch 每轮同步、生成全部书，不能和 --force、书名一起用{}",
                path_hint(&selectors)
            ));
        }
        let prune = if args.has("keep") {
            library::Prune::Keep
        } else {
            library::Prune::Auto
        };
        // 没跟踪目录时只生成 add 进来的书；什么都没有（或只要同步）就提示先 track
        let tracking = !lib.tracked().is_empty();
        if !tracking
            && (devices.is_empty()
                || watch.is_some()
                || (selectors.is_empty() && lib.list().is_empty()))
        {
            fail("还没有跟踪任何目录。先登记要跟踪的书目录（只需一次），例如：\n  booklib track ~/Documents/ereader/books\n之后 booklib sync 就会把它镜像进书库");
        }
        SyncPlan {
            selectors,
            force,
            watch,
            devices,
            prune,
            tracking,
            meta: !args.has("no-meta"),
        }
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
    /// 空间不够停下的设备 → 停下时的剩余空间（[`space_freed`]）。
    space_held: HashMap<String, Option<u64>>,
    /// 报过「连不上网，元数据下次再找」（`--watch` 时连上网以前只报一次）。
    meta_offline_reported: bool,
}

pub(crate) fn cmd_sync(ctx: &Ctx) {
    let plan = SyncPlan::parse(ctx);
    let mut state = SyncState::default();
    loop {
        ctx.lib.refresh_devices();
        match ctx.lib.lock() {
            Ok(_guard) => state.round(ctx, &plan),
            Err(e) => ctx.fail_line(&e), // --watch 时只计数，下一轮再试
        }
        let Some(secs) = plan.watch else { break };
        std::thread::sleep(std::time::Duration::from_secs(secs));
    }
}

impl SyncState {
    /// 一轮（持着锁）：先同步跟踪目录，再生成并传。
    fn round(&mut self, ctx: &Ctx, plan: &SyncPlan) {
        let changed = if plan.tracking {
            self.sync_sources(ctx, plan)
        } else {
            true
        };
        if plan.meta {
            self.fill_metadata(ctx, plan);
        }
        if !plan.devices.is_empty() {
            self.build(ctx, plan, changed);
        }
    }

    /// 同步跟踪目录，逐条报。返回这一轮有没有新增、更新、删除（出错也算有，接着去生成）。
    fn sync_sources(&mut self, ctx: &Ctx, plan: &SyncPlan) -> bool {
        let r = ctx
            .lib
            .sync_with(plan.prune, &mut self.memo, |ev| match ev {
                SyncEvent::Added(p, m) => {
                    println!("✓ 入库 {}  {}  ({})", m.id, m.title, p.display())
                }
                SyncEvent::Updated(p, m, old) => {
                    println!("↻ 更新 {}  {} ← {old}  ({})", m.id, m.title, p.display())
                }
                SyncEvent::Missing(p, t, true) => println!(
                    "✗ 原件已删，书库里也删了（连同产物）  {t}  ({})",
                    p.display()
                ),
                SyncEvent::Missing(p, t, false) => println!(
                    "? 原件不在了（--keep，这次没从书库删）  {t}  ({})",
                    p.display()
                ),
                SyncEvent::Released(p, t, true) => {
                    println!("✗ 不再跟踪，书库里删了（连同产物）  {t}  ({})", p.display())
                }
                SyncEvent::Released(p, t, false) => println!(
                    "? 不再跟踪（--keep，这次没从书库删）  {t}  ({})",
                    p.display()
                ),
                SyncEvent::Failed(p, e) => ctx.fail_line(&format!("✗ {}: {e}", p.display())),
                // 算失败（退出码 2）：这次同步不完整
                SyncEvent::Unreadable(p, e) => ctx.fail_line(&format!(
                    "✗ 目录读不了，里面的书这次没同步（已登记的原样保留，不删）：{}（{e}）",
                    p.display()
                )),
                SyncEvent::ForgotDevices(p, gone, whole) => println!(
                    "- 跟踪条目 {}：阅读模式 {} 已经没有了，{}",
                    p.display(),
                    gone.join("、"),
                    if whole {
                        "整条去掉（里面的书按不再跟踪处理）"
                    } else {
                        "从条目里去掉"
                    }
                ),
            });
        let changed = r
            .as_ref()
            .map_or(true, |r| r.added + r.updated + r.pruned > 0);
        match r {
            // --watch 时没变化就不出声
            Ok(r)
                if plan.watch.is_some()
                    && r.added + r.updated + r.missing + r.released + r.failed + r.unreadable
                        == 0 => {}
            Ok(r) => {
                let unreadable = if r.unreadable > 0 {
                    format!("，读不了的目录 {}", r.unreadable)
                } else {
                    String::new()
                };
                let released = if r.released > 0 {
                    format!("，不再跟踪 {}", r.released)
                } else {
                    String::new()
                };
                println!("原件：新增 {}，改过 {}，没变 {}，不在了 {}{released}（从书库删了 {}），出错 {}{unreadable}", r.added, r.updated, r.unchanged, r.missing, r.pruned, r.failed)
            }
            Err(e) => ctx.report(Err(e)),
        }
        changed
    }

    /// 补元数据（2026-10-12 定：跟踪 → 补元数据 → 传设备）：还没找过的书联网找一次，找到的生成时补进书里。
    /// 找过的（找到了、或者这一版匹配规则下没找到）不再找，`--watch` 每轮不重复联网；连不上网整轮跳过，下次再找。
    fn fill_metadata(&mut self, ctx: &Ctx, plan: &SyncPlan) {
        let lib = ctx.lib;
        let books = if plan.selectors.is_empty() {
            lib.list()
        } else {
            lib.select(&plan.selectors)
        };
        let todo: Vec<&library::Meta> = books
            .iter()
            .filter(|m| m.supported() && lib.needs_metadata(m))
            .collect();
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
                Ok((InfoResult::Found(i), _)) => {
                    println!("✓ 元数据 {} ← {}", m.title, info_summary(&i))
                }
                Ok((InfoResult::Shared(_, from), _)) => {
                    println!("✓ 元数据 {} ← 同一套的《{from}》找过的", m.title)
                }
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
    fn build(&mut self, ctx: &Ctx, plan: &SyncPlan, changed: bool) {
        let lib = ctx.lib;
        let stamp = || lib.change_stamp();
        let live: Vec<bool> = plan
            .devices
            .iter()
            .map(|d| lib.device_status(d).is_ok())
            .collect();
        for (d, ok) in plan.devices.iter().zip(&live) {
            if *ok {
                self.absent_reported.remove(&d.id);
            }
        }
        forget_reconnected(
            &mut self.fails,
            &plan.devices,
            self.last_live.as_deref(),
            &live,
        );
        let lib_changed = changed || self.last_stamp != Some(stamp());
        let freed = space_freed(
            &mut self.space_held,
            &mut self.absent_reported,
            lib_changed,
            &plan.devices,
            self.last_live.as_deref(),
            &live,
            |d| lib.device_free(d),
        );
        if !(lib_changed || freed || self.last_live.as_ref() != Some(&live)) {
            return;
        }
        self.last_live = Some(live);
        // 没选书：全部书，只报有变化的；选了书：只生成这几本，每本都报
        let books = if plan.selectors.is_empty() {
            lib.list()
        } else {
            lib.select(&plan.selectors)
        };
        if books.is_empty() && !plan.selectors.is_empty() {
            fail(&format!(
                "没有匹配的书（booklib list 查看书库）{}",
                path_hint(&plan.selectors)
            ));
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
            space_held: plan.watch.is_some().then_some(&mut self.space_held),
        };
        // 没选书：只报有变化的
        let quiet = plan.selectors.is_empty();
        let mut warnings = WarningSummary::default();
        let counts = library::build_all(run, &mut |ev| {
            warnings.observe(&ev);
            if let Some(r) = event_line(ev, quiet) {
                ctx.report(r);
            }
        });
        // 传的途中断开了的设备：当成这一轮没接上，下一轮接着的话重新传（[`forget_reconnected`]）
        mark_dropped(
            self.last_live.as_deref_mut(),
            &plan.devices,
            &counts.dropped,
        );
        // 空间不够的设备不当成断开（会清掉失败表）：记在 space_held，剩余空间变多了再跑（space_freed）
        // --watch 时没有生成、挪动、失败就不出声
        if plan.watch.is_none() || counts.any() {
            ctx.report(Ok(counts_summary(&counts)));
        }
        // 按书刷屏的提醒一轮每类汇总成一行（[`WarningSummary::lines`]），逐本结果行里已经没有它们
        for line in warnings.lines() {
            ctx.report(Ok(line));
        }
        self.last_stamp = Some(stamp());
    }
}
