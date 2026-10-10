//! 生成好的产物怎么放上设备：一件要传的（[`Transfer`]）、它要做的（[`Work`]：写文件，或交给 Move）、做完的结果（[`Done`]），
//! 以及 `sync` 的传输线程（[`Pipeline`]）。
//!
//! 依赖方向：`generate`（比较、生成、记录）→ 本模块（传）→ `deliver`（连设备、往 MTP 上写文件、调 Move 的接口）。
//! 记录只在主线程改：传输线程只做 [`Transfer::run`] 这类不碰书库的事，结果交回主线程由 [`crate::Library::complete`] 记下来。
//!
//! 投递只有"写文件"和"交给 Move"两种，用枚举分开（[`Work`]），Move 专属的数据（客户端、显示名、文件夹、要替换的 uuid）
//! 收在 [`MoveWork`] 里，不为两种实现上 trait。

use crate::deliver::{self, Doc, JobState, MoveClient, Submitted, POLL};
use crate::generate::StateEntry;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// 生成好、要放上设备的一本（[`crate::Library::prepare`] 交出，传输线程 [`Transfer::run`]，传完 [`crate::Library::complete`] 记下来）。
/// 只含普通数据，能送进别的线程。
pub struct Transfer {
    /// 模式 id、书名（报告用）。
    pub device: String,
    pub title: String,
    pub(crate) book: String,
    /// 生成记录文件。
    pub(crate) sp: PathBuf,
    /// 产物在书库临时目录里：传完删掉这个目录。
    pub(crate) tmp: Option<PathBuf>,
    pub(crate) src: PathBuf,
    pub(crate) sha: String,
    pub(crate) warnings: Vec<String>,
    /// 内容没变、只是挪位置：原来在哪。
    pub(crate) from: Option<PathBuf>,
    /// 传完写的记录（指纹已填；Move 的 uuid、显示名传完才知道）。
    pub(crate) entry: StateEntry,
    pub(crate) work: Work,
}

/// 传输线程要做的。
pub(crate) enum Work {
    /// 放到 `out`（MTP 设备、电脑上）；`compare` 时 `out` 已是逐字节相同的就不动（要把它读回来比）；`direct` 时不经临时文件
    /// （MTP 设备上，见 [`deliver::put_file`]）。
    File { out: PathBuf, compare: bool, direct: bool },
    /// 交给 Move 上的书架服务。
    Move(MoveWork),
}

/// 交给 Move 的一件：书架服务的客户端、xochitl 里的显示名和文件夹，`replace` 有就原地替换这个 uuid
/// （设备上已经没了就改成新加），否则新加进 `folder`。
pub(crate) struct MoveWork {
    pub(crate) client: MoveClient,
    pub(crate) name: String,
    pub(crate) folder: String,
    pub(crate) replace: Option<String>,
}

impl MoveWork {
    /// 把 `src` 交出去（替换的那本设备上已经没了就改成新加）。
    fn submit(&self, src: &Path) -> Result<Submitted, String> {
        if let Some(uuid) = &self.replace {
            if let Some(s) = self.client.submit_replace(uuid, src, &self.name)? {
                return Ok(s);
            }
        }
        self.client.submit_import(src, &self.name, &self.folder)
    }
}

impl Work {
    /// 写文件的话，写到哪（传的过程中这个文件名在内存里占着，见 `Library::prepare_file`）。交给 Move 的没有。
    pub(crate) fn file_out(&self) -> Option<&Path> {
        match self {
            Work::File { out, .. } => Some(out),
            Work::Move(_) => None,
        }
    }
}

/// 传输线程做完了。
pub enum Done {
    /// 放好了文件（`false`：设备上已是一样的，没写）。
    File(bool),
    /// Move 上加入（或替换）好了。
    Move(Doc),
    /// Move 上那份已经一样，没再传。
    Same(Doc),
}

impl Done {
    /// (真的往设备上写了没有, Move 上的那本)。
    pub(crate) fn outcome(self) -> (bool, Option<Doc>) {
        match self {
            Done::File(w) => (w, None),
            Done::Move(d) => (true, Some(d)),
            Done::Same(d) => (false, Some(d)),
        }
    }
}

impl Transfer {
    /// 放上设备，做完才返回（单本生成用；`sync` 里 Move 的那件由传输线程交了以后轮流查，见 [`Pipeline`]）。
    pub fn run(&self) -> Result<Done, String> {
        match &self.work {
            Work::File { out, compare, direct } => deliver::put_file(&self.src, out, false, *compare, *direct).map(Done::File),
            Work::Move(mv) => {
                let s = mv.submit(&self.src)?;
                mv.client.wait(s).map(Done::Move)
            }
        }
    }

    /// 交给 Move 以后不再需要产物文件（书架服务已经收下）：书库临时目录里的先删掉，省得排版排队时大漫画堆在电脑上。
    fn release_src(&self) {
        if self.tmp.is_some() {
            let _ = std::fs::remove_file(&self.src);
        }
    }

    /// 书 id。
    pub fn book_id(&self) -> &str {
        &self.book
    }
}

/// 给人看的时长：`45 秒`、`2 分 10 秒`。
fn elapsed(d: Duration) -> String {
    let s = d.as_secs();
    if s < 60 { format!("{s} 秒") } else { format!("{} 分 {} 秒", s / 60, s % 60) }
}

/// `sync` 的传输线程：每台设备一条，主线程比较、生成的同时这里往设备上放（Move 排版、MTP 拷大漫画都不挡生成）。
/// 每台在传（含排队）的最多 [`LANE_DEPTH`] 本（[`Pipeline::has_room`]；满了调用方先去做别的设备），书库临时目录里堆不起大漫画。
/// 传完的结果由主线程取回（[`Pipeline::next`]）、调 [`crate::Library::complete`] 记下来：书库的记录只在主线程里改。
/// Move 上一本做得久的，中途还交回进度（[`Event::Progress`]）。
///
/// 传输线程里出了 panic 的那件算传失败（不连累同一台后面的）；线程还是意外死了的话，主线程等结果时会发现
/// （[`Event::Lost`]），不会一直等下去。
pub struct Pipeline {
    lanes: HashMap<String, (std::sync::mpsc::Sender<Transfer>, std::thread::JoinHandle<()>)>,
    done_tx: std::sync::mpsc::Sender<Event>,
    done_rx: std::sync::mpsc::Receiver<Event>,
    /// 各设备在传（含排队）的件数。
    busy: HashMap<String, usize>,
}

/// 传输线程交回的。
pub enum Event {
    /// 传完一件（成功或失败）：交给 [`crate::Library::complete`]。
    Done(Box<Transfer>, Result<Done, String>),
    /// 还在做的一件的进度（给人看的一行）。
    Progress(String),
    /// 这台设备的传输线程意外退出了：手上还没交回的 `count` 件没传成（记录没改，下次再传）。
    Lost { device: String, count: usize },
}

/// 等传输线程交回结果时，隔多久看一眼线程还在不在。
const LANE_CHECK: Duration = Duration::from_millis(500);

/// Move 上一本做了多久以后开始报进度、阶段没变时隔多久再报一次。
const PROGRESS_AFTER: Duration = Duration::from_secs(30);
const PROGRESS_EVERY: Duration = Duration::from_secs(60);

/// 每台设备在传（含排队）的上限。
const LANE_DEPTH: usize = 2;

impl Default for Pipeline {
    fn default() -> Self {
        let (done_tx, done_rx) = std::sync::mpsc::channel();
        Pipeline { lanes: Default::default(), done_tx, done_rx, busy: Default::default() }
    }
}

impl Pipeline {
    /// 这台设备的传输线程还能再收一件（在传的不到 [`LANE_DEPTH`] 件）。
    pub fn has_room(&self, device: &str) -> bool {
        self.busy.get(device).copied().unwrap_or(0) < LANE_DEPTH
    }

    /// 交给这台设备的传输线程（第一次用时起；Move 和写文件的线程做法不同，按第一件定）。不等：先用 [`Pipeline::has_room`] 看有没有空。
    pub fn submit(&mut self, t: Transfer) {
        *self.busy.entry(t.device.clone()).or_default() += 1;
        let (tx, _) = self.lanes.entry(t.device.clone()).or_insert_with_key(|_| {
            let (tx, rx) = std::sync::mpsc::channel();
            let done = self.done_tx.clone();
            let h = match t.work {
                Work::Move(_) => std::thread::spawn(move || move_lane(rx, done)),
                Work::File { .. } => std::thread::spawn(move || file_lane(rx, done)),
            };
            (tx, h)
        });
        if let Err(std::sync::mpsc::SendError(_t)) = tx.send(t) {
            // 线程已经退出：这件交不出去，等结果时按线程死了报（`next`）
        }
    }

    /// 取一件传输线程交回的（传完的、进度）：`wait` 时没有就等到有；没有在传的了返回 `None`。
    pub fn next(&mut self, wait: bool) -> Option<Event> {
        loop {
            if self.in_flight() == 0 {
                return None;
            }
            let r = if wait { self.done_rx.recv_timeout(LANE_CHECK).ok() } else { self.done_rx.try_recv().ok() };
            if let Some(ev) = r {
                return Some(self.count_back(ev));
            }
            // 没有结果：看看有没有手上还有活、线程却已经退出的（panic 在单件之外）——它交不回来了，别一直等
            if let Some(lost) = self.dead_lane() {
                return Some(lost);
            }
            if !wait {
                return None;
            }
        }
    }

    /// 交回了一件：这台在传的件数减一。
    fn count_back(&mut self, ev: Event) -> Event {
        if let Event::Done(t, _) = &ev {
            if let Some(n) = self.busy.get_mut(&t.device) {
                *n -= 1;
            }
        }
        ev
    }

    /// 手上还有活、线程却已经退出的设备：去掉这条（下次交给它时重新起），返回 [`Event::Lost`]。
    /// 线程退出前交回的结果都已经在队列里（先看队列、空了才来这里），所以剩下的件数就是丢了的。
    fn dead_lane(&mut self) -> Option<Event> {
        let device = self.lanes.iter().find(|(d, (_, h))| h.is_finished() && self.busy.get(*d).is_some_and(|n| *n > 0)).map(|(d, _)| d.clone())?;
        // 线程退出和它最后交回的结果之间没有先后保证：队列里还有的话先交那个
        if let Ok(ev) = self.done_rx.try_recv() {
            return Some(self.count_back(ev));
        }
        if let Some((_, h)) = self.lanes.remove(&device) {
            let _ = h.join();
        }
        let count = self.busy.remove(&device).unwrap_or(0);
        Some(Event::Lost { device, count })
    }

    /// 还在传（含排队）的件数。
    pub fn in_flight(&self) -> usize {
        self.busy.values().sum()
    }
}

impl Drop for Pipeline {
    fn drop(&mut self) {
        // 关掉各条队列，等线程把手上的做完退出
        for (_, (tx, h)) in self.lanes.drain() {
            drop(tx);
            let _ = h.join();
        }
    }
}

/// 跑 `f`，panic 了变成错误（传输线程里一件出了 panic 只算这件传失败，线程接着做下一件）。
fn guarded<T>(what: &str, f: impl FnOnce() -> Result<T, String>) -> Result<T, String> {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)).unwrap_or_else(|p| {
        let msg = p.downcast_ref::<&str>().map(|s| s.to_string()).or_else(|| p.downcast_ref::<String>().cloned()).unwrap_or_default();
        Err(format!("{what}时出了内部错误（panic：{msg}）"))
    })
}

/// Kindle、掌阅（文件）：一件一件放。
fn file_lane(rx: std::sync::mpsc::Receiver<Transfer>, done: std::sync::mpsc::Sender<Event>) {
    for t in rx {
        let r = guarded("传到设备", || t.run());
        if done.send(Event::Done(Box::new(t), r)).is_err() {
            return;
        }
    }
}

/// 交出去、在 Move 上做着的一件。
struct Pending {
    t: Transfer,
    client: MoveClient,
    job: String,
    /// 交出的时刻。
    at: Instant,
    /// 上次报的阶段、时刻。
    last_stage: String,
    last_emit: Option<Instant>,
}

/// Move：交出去一件，等它在 Move 上做完（书架服务在后台加入 xochitl、等排版）再交下一件——Move 上排队的书每本都占一份
/// 完整的空间，不一下子全推上去。
fn move_lane(rx: std::sync::mpsc::Receiver<Transfer>, done: std::sync::mpsc::Sender<Event>) {
    let mut pending: Vec<Pending> = Vec::new();
    let mut open = true;
    while open || !pending.is_empty() {
        let next = if open && pending.is_empty() { rx.recv().map_err(|_| open = false).ok() } else { None };
        let got = next.is_some();
        if let Some(t) = next {
            let at = Instant::now();
            // 每条线程只收一台设备的，这台是 Move 就都是 Move；万一混进来写文件的，当场照写
            let Work::Move(mv) = &t.work else {
                let r = guarded("传到设备", || t.run());
                let _ = done.send(Event::Done(Box::new(t), r));
                continue;
            };
            match guarded("交给 Move", || mv.submit(&t.src)) {
                Ok(Submitted::Done(d)) => {
                    let _ = done.send(Event::Done(Box::new(t), Ok(Done::Move(d))));
                }
                Ok(Submitted::Job(job)) => {
                    t.release_src();
                    let client = mv.client.clone();
                    pending.push(Pending { t, client, job, at, last_stage: String::new(), last_emit: None });
                }
                Err(e) => {
                    let _ = done.send(Event::Done(Box::new(t), Err(e)));
                }
            }
        }
        // 有一件在 Move 上做：隔一会查一次
        if !got && !pending.is_empty() {
            std::thread::sleep(POLL);
        }
        let mut still = Vec::new();
        for mut p in pending.drain(..) {
            match guarded("查 Move 上的导入任务", || p.client.poll(&p.job)) {
                Ok(JobState::Running(stage)) => {
                    // 做了 30 秒以上：阶段变了马上报，没变每分钟报一次
                    let due = p.at.elapsed() >= PROGRESS_AFTER && (stage != p.last_stage || p.last_emit.is_none_or(|e| e.elapsed() >= PROGRESS_EVERY));
                    if due {
                        let what = if stage.is_empty() { "在做" } else { stage.as_str() };
                        let _ = done.send(Event::Progress(format!("… [{}] {}：Move 上{what}（已 {}）", p.t.device, p.t.title, elapsed(p.at.elapsed()))));
                        p.last_emit = Some(Instant::now());
                        p.last_stage = stage;
                    }
                    still.push(p);
                }
                Ok(JobState::Done(d)) => {
                    let _ = done.send(Event::Done(Box::new(p.t), Ok(Done::Move(d))));
                }
                Ok(JobState::Failed(m)) => {
                    let _ = done.send(Event::Done(Box::new(p.t), Err(format!("Move 上加入失败：{m}"))));
                }
                Err(e) => {
                    let _ = done.send(Event::Done(Box::new(p.t), Err(e)));
                }
            }
        }
        pending = still;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 传输线程死了（手上还有活）：主线程等结果时报出来，不一直卡着。
    #[test]
    fn dead_lane_is_reported_not_waited_on_forever() {
        let mut pipe = Pipeline::default();
        let (tx, _rx) = std::sync::mpsc::channel::<Transfer>();
        let h = std::thread::spawn(|| panic!("传输线程意外退出（测试）"));
        pipe.lanes.insert("kindle".into(), (tx, h));
        pipe.busy.insert("kindle".into(), 2);
        match pipe.next(true) {
            Some(Event::Lost { device, count }) => assert_eq!((device.as_str(), count), ("kindle", 2)),
            _ => panic!("应报线程丢了"),
        }
        assert_eq!(pipe.in_flight(), 0);
        assert!(pipe.next(true).is_none());
        assert!(pipe.has_room("kindle"));
    }

    #[test]
    fn panics_become_errors() {
        let r: Result<(), String> = guarded("传到设备", || panic!("坏了"));
        assert!(r.unwrap_err().contains("坏了"));
    }

    #[test]
    fn elapsed_reads_naturally() {
        assert_eq!(elapsed(Duration::from_secs(45)), "45 秒");
        assert_eq!(elapsed(Duration::from_secs(130)), "2 分 10 秒");
    }
}
