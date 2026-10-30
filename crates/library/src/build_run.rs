//! 一轮生成：按书 × 模式逐本比较、生成、交给传输线程送到设备上（`sync` 的第二步）。
//!
//! 调度在这里（设备没接上的跳过、排满的设备推迟、`--watch` 跨轮记住失败），结果一件一件报成 [`BuildEvent`]；
//! 怎么显示（措辞、`quiet` 时哪些不报）由命令行决定。

use crate::{Built, Event, Library, Meta, OriginalState, Pipeline, Profile, Step};
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

/// `sync --watch` 跨轮记住的生成失败：(书 id, 模式 id) → (失败时的指纹, 原件状态)。都没变就不重试。
pub type FailMemo = HashMap<(String, String), (String, OriginalState)>;

/// 一轮生成要的东西。
pub struct BuildRun<'a> {
    pub lib: &'a Library,
    pub devices: &'a [&'a Profile],
    pub books: &'a [Meta],
    pub force: bool,
    /// `--keep`：不在这台跟踪范围里的书只报告（[`BuildEvent::Outside`]），不从设备上删。
    pub keep: bool,
    /// 跨轮记住的生成失败（[`FailMemo`]）。不是 `--watch` 时 `None`。
    pub fails: Option<&'a mut FailMemo>,
    /// 报过的不再支持的格式（早期版本收的 MOBI/PDF 等）的书：每本只报一次。
    pub skipped: &'a mut HashSet<String>,
    /// 报过"没接上"的设备（`--watch` 时不重复报）。
    pub absent_reported: &'a mut HashSet<String>,
}

/// 一轮生成里发生的一件事。
pub enum BuildEvent {
    /// 不再支持的格式，跳过（每本只报一次）。
    Unsupported { id: String, title: String, reason: String },
    /// 接上的设备上删掉了以前删书时没删成的书。
    RemovedFlushed { device: String, count: usize },
    RemoveFlushFailed { device: String, error: String },
    /// 设备没接上，这一轮不传（`--watch` 时每台只报一次）。
    DeviceAbsent { device: String, reason: String },
    /// 不再给这台设备的书（从它的跟踪范围里拿掉了）：从设备上删掉了。
    Retracted { device: String, title: String },
    /// 不在这台设备的跟踪范围里、设备上还有，`--keep` 这一次没删。
    Outside { device: String, title: String },
    RetractFailed { device: String, title: String, error: String },
    /// 一本书一个模式的结果。
    Built { device: String, title: String, built: Built },
    Failed { device: String, title: String, error: String },
    /// 传输线程报的进度（已经是一行字）。
    Progress(String),
    /// 传输线程意外退出，这么多本没传成。
    TransportLost { device: String, count: usize },
    /// 都生成好了，还在等设备那边传完。
    Waiting { in_flight: usize },
    /// 给这一轮传上去的书写了阅读器的显示设置（`written` 本）；`pending` 本等了一会儿阅读器还没收录，没写（要手动调）。
    ReaderDisplay { device: String, written: usize, pending: usize },
    ReaderDisplayFailed { device: String, error: String },
}

/// 一轮的结果计数。
#[derive(Default, Debug, Clone, Copy, PartialEq, Eq)]
pub struct BuildCounts {
    pub written: usize,
    pub same: usize,
    pub adopted: usize,
    pub moved: usize,
    pub up_to_date: usize,
    /// 不在跟踪范围、从设备上删掉的。
    pub retracted: usize,
    pub failed: usize,
}

impl BuildCounts {
    /// 有没有生成、挪动、失败（`--watch` 时都没有就不出声）。
    pub fn any(&self) -> bool {
        self.written + self.same + self.adopted + self.moved + self.retracted + self.failed > 0
    }

    fn count(&mut self, b: &Built) {
        match b {
            Built::Written { .. } => self.written += 1,
            Built::Same { .. } => self.same += 1,
            Built::UpToDate(_) => self.up_to_date += 1,
            Built::Adopted(_) => self.adopted += 1,
            Built::Moved { .. } => self.moved += 1,
        }
    }
}

/// 按模式 × 书逐本生成并送到设备上（没变化的跳过）。设备没接上的模式跳过。
pub fn build_all(run: BuildRun, report: &mut impl FnMut(BuildEvent)) -> BuildCounts {
    let BuildRun { lib, devices, books, force, keep, mut fails, skipped, absent_reported } = run;
    // 这一轮放上各设备的新文件（写阅读器显示设置用，见最后）
    let fresh: RefCell<HashMap<String, Vec<PathBuf>>> = RefCell::default();
    let mut tap = |ev: BuildEvent| {
        if let BuildEvent::Built { device, built: Built::Written { path, .. } | Built::Moved { to: path, .. }, .. } = &ev {
            fresh.borrow_mut().entry(device.clone()).or_default().push(path.clone());
        }
        report(ev)
    };
    let report = &mut tap;
    let mut counts = BuildCounts::default();
    for m in books.iter().filter(|m| !m.supported()) {
        if skipped.insert(m.id.clone()) {
            report(BuildEvent::Unsupported { id: m.id.clone(), title: m.title.clone(), reason: crate::unsupported(m.content_format()) });
        }
    }
    // 产物直接送设备（profile 的 [deliver]）：没接上的设备这一轮整个跳过（每轮报一次），接上的先删掉以前删书时没删成的
    let mut live: Vec<&Profile> = Vec::new();
    for device in devices {
        match lib.device_status(device) {
            Ok(_) => {
                match lib.flush_removed(device) {
                    Ok(0) => {}
                    Ok(count) => report(BuildEvent::RemovedFlushed { device: device.id.clone(), count }),
                    Err(error) => report(BuildEvent::RemoveFlushFailed { device: device.id.clone(), error }),
                }
                live.push(device);
            }
            Err(reason) => {
                // --watch（给了 fails）时同一台没接上只报一次
                if fails.is_none() || !absent_reported.contains(&device.id) {
                    report(BuildEvent::DeviceAbsent { device: device.id.clone(), reason });
                }
                absent_reported.insert(device.id.clone());
            }
        }
    }
    // 不在这台设备的跟踪范围里的书：以前传过的从设备上删掉（2026-10-19 定），这一轮也不生成
    let mut outside: HashSet<(&str, &str)> = HashSet::new();
    for m in books {
        for device in &live {
            if lib.wants(m, &device.id) {
                continue;
            }
            outside.insert((m.id.as_str(), device.id.as_str()));
            if !lib.has_output(&m.id, &device.id) {
                continue;
            }
            if keep {
                report(BuildEvent::Outside { device: device.id.clone(), title: m.title.clone() });
                continue;
            }
            match lib.retract(&m.id, device) {
                Ok(_) => {
                    counts.retracted += 1;
                    report(BuildEvent::Retracted { device: device.id.clone(), title: m.title.clone() });
                }
                Err(error) => {
                    counts.failed += 1;
                    report(BuildEvent::RetractFailed { device: device.id.clone(), title: m.title.clone(), error });
                }
            }
        }
    }
    // 先比较、再生成（主线程）；要传的交给各设备的传输线程，和生成同时进行；传完的随时取回来记下、报一件
    let mut pipe = Pipeline::default();
    let landed = |counts: &mut BuildCounts, fails: &mut Option<&mut FailMemo>, report: &mut dyn FnMut(BuildEvent), ev: Event| {
        let (t, r) = match ev {
            Event::Done(t, r) => (t, r),
            Event::Progress(line) => return report(BuildEvent::Progress(line)),
            Event::Lost { device, count } => {
                counts.failed += count;
                return report(BuildEvent::TransportLost { device, count });
            }
        };
        let (device, title) = (t.device.clone(), t.title.clone());
        let book = t.book_id().to_string();
        match lib.complete(*t, r) {
            Ok(built) => {
                counts.count(&built);
                report(BuildEvent::Built { device, title, built });
            }
            Err(error) => {
                counts.failed += 1;
                if let (Some(f), Some(m), Some(p)) = (fails.as_deref_mut(), books.iter().find(|m| m.id == book), lib.devices().get(&device)) {
                    f.insert((book, device.clone()), (lib.fingerprint(m, p).unwrap_or_else(|e| e), lib.original_state(m)));
                }
                report(BuildEvent::Failed { device, title, error });
            }
        }
    };
    // 一本书一个模式：比较、生成，要传的交给传输线程
    let one = |m: &Meta, device: &Profile, counts: &mut BuildCounts, fails: &mut Option<&mut FailMemo>, pipe: &mut Pipeline, report: &mut dyn FnMut(BuildEvent)| {
        let key = (m.id.clone(), device.id.clone());
        let now = || (lib.fingerprint(m, device).unwrap_or_else(|e| e), lib.original_state(m));
        if let Some(f) = fails.as_deref_mut() {
            match f.get(&key).map(|v| *v == now()) {
                Some(true) => return,
                Some(false) => {
                    f.remove(&key);
                }
                None => {}
            }
        }
        match lib.prepare(m, device, force) {
            Ok(Step::Done(built)) => {
                counts.count(&built);
                report(BuildEvent::Built { device: device.id.clone(), title: m.title.clone(), built });
            }
            Ok(Step::Transfer(t)) => pipe.submit(*t),
            Err(error) => {
                counts.failed += 1;
                report(BuildEvent::Failed { device: device.id.clone(), title: m.title.clone(), error });
                if let Some(f) = fails.as_deref_mut() {
                    f.insert(key, now());
                }
            }
        }
    };
    // 书在外层、模式在内层：同一本书与模式无关的中间文件（CBZ 转换、补元数据）只做一次。
    // 哪台设备那边排满了（Move 排版慢），先跳过它做别的设备，留到最后补，不让一台挡住另外几台
    // 推迟的书的中间文件留着（按书 id，`Library::release_prepared_of`），补的时候不用再转一遍；这本书所有模式做完就删
    let mut later: Vec<(&Meta, &Profile)> = Vec::new();
    let mut deferred: HashMap<&str, usize> = HashMap::new();
    for m in books.iter().filter(|m| m.supported()) {
        for device in live.iter().filter(|d| !outside.contains(&(m.id.as_str(), d.id.as_str()))) {
            while let Some(x) = pipe.next(false) {
                landed(&mut counts, &mut fails, report, x);
            }
            if pipe.has_room(&device.id) {
                one(m, device, &mut counts, &mut fails, &mut pipe, report);
            } else {
                later.push((m, device));
                *deferred.entry(m.id.as_str()).or_default() += 1;
            }
        }
        if !deferred.contains_key(m.id.as_str()) {
            lib.release_prepared_of(&m.id);
        }
    }
    for (m, device) in later {
        while !pipe.has_room(&device.id) {
            let Some(x) = pipe.next(true) else { break };
            landed(&mut counts, &mut fails, report, x);
        }
        one(m, device, &mut counts, &mut fails, &mut pipe, report);
        if let Some(n) = deferred.get_mut(m.id.as_str()) {
            *n -= 1;
            if *n == 0 {
                lib.release_prepared_of(&m.id);
            }
        }
    }
    lib.release_prepared();
    if pipe.in_flight() > 0 {
        report(BuildEvent::Waiting { in_flight: pipe.in_flight() });
    }
    while let Some(x) = pipe.next(true) {
        landed(&mut counts, &mut fails, report, x);
    }
    // 都传完了：给这一轮传上去的书写阅读器的显示设置（文石，见 `onyx`）
    let fresh = fresh.take();
    for device in &live {
        match lib.seed_reader_display(device, fresh.get(&device.id).map_or(&[][..], Vec::as_slice)) {
            None | Some(Ok(crate::onyx::Seeded { written: 0, pending: 0 })) => {}
            Some(Ok(s)) => report(BuildEvent::ReaderDisplay { device: device.id.clone(), written: s.written, pending: s.pending }),
            Some(Err(error)) => report(BuildEvent::ReaderDisplayFailed { device: device.id.clone(), error }),
        }
    }
    counts
}
