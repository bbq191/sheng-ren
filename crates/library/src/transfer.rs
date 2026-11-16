//! 生成好的产物怎么放上设备：一件要传的（[`Transfer`]）、它要做的（[`Work`]：写到哪个文件），以及 `sync` 的传输线程（[`Pipeline`]）。
//!
//! 依赖方向：`generate`（比较、生成、记录）→ 本模块（传）→ `deliver`（连设备、往 MTP 上写文件）。
//! 记录只在主线程改：传输线程只做 [`Transfer::run`] 这类不碰书库的事，结果交回主线程由 [`crate::Library::complete`] 记下来。

use crate::deliver;
use crate::error::{Error, Kind};
use crate::generate::StateEntry;
use crate::stall;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

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
    /// 传完写的记录（指纹已填）。
    pub(crate) entry: StateEntry,
    pub(crate) work: Work,
    /// USB 大容量存储（Kobo）：经临时文件再改名，不搬 `.sdr`。
    pub(crate) staged: bool,
    /// 传到哪一步了（主线程按它发现「太久没进展」）。
    pub(crate) progress: stall::Progress,
    /// 这一件算在哪台设备的挂载点上（被放弃时记进「还没返回的线程」，后面几轮不再碰这台）。
    pub(crate) stall_key: PathBuf,
    /// 测试用：一直卡在设备上（`#[cfg(test)]`，正式构建没有这个字段）。
    #[cfg(test)]
    pub(crate) test_hang: Option<Duration>,
}

/// 传输线程要做的：放到 `out`（MTP 设备、电脑上）；`compare` 时 `out` 已是逐字节相同的就不动（要把它读回来比）；`direct` 时不经临时文件
/// （MTP 设备上，见 [`deliver::put_file`]）。`mount`：MTP 设备的挂载点，每本写之前看它还挂着（传的途中设备断开了，别写进挂载点剩下的空目录）。
pub(crate) struct Work {
    pub(crate) out: PathBuf,
    pub(crate) compare: bool,
    pub(crate) direct: bool,
    pub(crate) mount: Option<PathBuf>,
}

impl Transfer {
    /// 放上设备，做完才返回：真的写了没有（`false`：设备上已是一样的，没写）。
    pub fn run(&self) -> Result<bool, Error> {
        #[cfg(test)]
        {
            if let Some(d) = self.test_hang {
                std::thread::sleep(d);
            }
            // 单元测试造的假传输（`generate::test_transfer`）：不碰真文件
            if self.work.out.starts_with("/测试") {
                return Ok(true);
            }
        }
        let Work {
            out,
            compare,
            direct,
            mount,
        } = &self.work;
        deliver::put_file_tracked(
            &self.src,
            out,
            deliver::PutOptions {
                keep_src: false,
                compare: *compare,
                direct: *direct,
                mount: mount.as_deref(),
            },
            Some(&self.progress),
        )
    }

    /// 书 id。
    pub fn book_id(&self) -> &str {
        &self.book
    }
}

/// `sync` 的传输线程：每台设备一条，主线程比较、生成的同时这里往设备上放（MTP 拷大漫画不挡生成）。
/// 每台在传（含排队）的最多 [`LANE_DEPTH`] 本（[`Pipeline::has_room`]；满了调用方先去做别的设备），书库临时目录里堆不起大漫画。
/// 传完的结果由主线程取回（[`Pipeline::next`]）、调 [`crate::Library::complete`] 记下来：书库的记录只在主线程里改。
///
/// 传输线程里出了 panic 的那件算传失败（不连累同一台后面的）；线程还是意外死了的话，主线程等结果时会发现
/// （[`Event::Lost`]），不会一直等下去。
pub struct Pipeline {
    lanes: HashMap<String, Lane>,
    done_tx: std::sync::mpsc::Sender<Event>,
    done_rx: std::sync::mpsc::Receiver<Event>,
    /// 各设备在传（含排队）的件数。
    busy: HashMap<String, usize>,
    /// 各设备正在传的那一件的进度（排队的不算）。
    current: Arc<Mutex<HashMap<String, stall::Progress>>>,
    /// 这一轮被放弃的设备：它们之后交回的结果一律丢掉（记录不能改）。
    abandoned: HashSet<String>,
}

/// 一台设备的传输线程：交活的队列、线程，和取消标记（放弃了这条线程时打上：它从卡住的那一件返回后，排着的不再碰设备）。
type Lane = (
    std::sync::mpsc::Sender<Transfer>,
    std::thread::JoinHandle<()>,
    Arc<std::sync::atomic::AtomicBool>,
);

/// 传输线程交回的。
pub enum Event {
    /// 传完一件（成功或失败；成功时是真的写了没有）：交给 [`crate::Library::complete`]。
    Done(Box<Transfer>, Result<bool, Error>),
    /// 这台设备的传输线程意外退出了：手上还没交回的 `count` 件没传成（记录没改，下次再传）。
    Lost { device: String, count: usize },
    /// 这台设备太久没有进展（挂载卡死）：这条传输线程放弃了，`count` 件没传成，这一轮不再往它传。
    Stalled {
        device: String,
        count: usize,
        secs: u64,
    },
}

/// 等传输线程交回结果时，隔多久看一眼线程还在不在。
const LANE_CHECK: Duration = Duration::from_millis(500);

/// 每台设备在传（含排队）的上限。
const LANE_DEPTH: usize = 2;

impl Default for Pipeline {
    fn default() -> Self {
        let (done_tx, done_rx) = std::sync::mpsc::channel();
        Pipeline {
            lanes: Default::default(),
            done_tx,
            done_rx,
            busy: Default::default(),
            current: Default::default(),
            abandoned: Default::default(),
        }
    }
}

impl Pipeline {
    /// 这台设备的传输线程还能再收一件（在传的不到 [`LANE_DEPTH`] 件）。
    pub fn has_room(&self, device: &str) -> bool {
        self.busy.get(device).copied().unwrap_or(0) < LANE_DEPTH
    }

    /// 交给这台设备的传输线程（第一次用时起）。不等：先用 [`Pipeline::has_room`] 看有没有空。
    pub fn submit(&mut self, t: Transfer) {
        *self.busy.entry(t.device.clone()).or_default() += 1;
        let done = self.done_tx.clone();
        let current = self.current.clone();
        let (tx, _, _) = self.lanes.entry(t.device.clone()).or_insert_with_key(|_| {
            let (tx, rx) = std::sync::mpsc::channel();
            let cancelled = Arc::new(std::sync::atomic::AtomicBool::new(false));
            let flag = cancelled.clone();
            (
                tx,
                std::thread::spawn(move || file_lane(rx, done, current, flag)),
                cancelled,
            )
        });
        if let Err(std::sync::mpsc::SendError(_t)) = tx.send(t) {
            // 线程已经退出：这件交不出去，等结果时按线程死了报（`next`）
        }
    }

    /// 取一件传输线程交回的：`wait` 时没有就等到有；没有在传的了返回 `None`。
    pub fn next(&mut self, wait: bool) -> Option<Event> {
        loop {
            if self.in_flight() == 0 {
                return None;
            }
            let r = if wait {
                self.done_rx.recv_timeout(LANE_CHECK).ok()
            } else {
                self.done_rx.try_recv().ok()
            };
            if let Some(ev) = r {
                // 被放弃的线程之后交回的结果：丢掉（记录不能改、busy 不能减错）
                if let Event::Done(t, _) = &ev {
                    if self.abandoned.contains(&t.device) {
                        continue;
                    }
                }
                return Some(self.count_back(ev));
            }
            // 没有结果：某台在传、却太久没有进展（挂载卡死）→ 放弃这条线程
            if let Some(stalled) = self.stalled_lane() {
                return Some(stalled);
            }
            // 看看有没有手上还有活、线程却已经退出的（panic 在单件之外）——它交不回来了，别一直等
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
                *n = n.saturating_sub(1);
            }
        }
        ev
    }

    /// 某台在传、而且超过这一步的时限没有进展：放弃这条线程（拿掉、不 join、记进放弃登记），
    /// 手上和排队的件都算没传成。返回 [`Event::Stalled`]。
    fn stalled_lane(&mut self) -> Option<Event> {
        let (device, secs) = {
            let cur = self.current.lock().unwrap_or_else(|e| e.into_inner());
            cur.iter().find_map(|(d, p)| {
                let secs = p.stalled_secs()?;
                self.busy.get(d).is_some_and(|n| *n > 0).then_some((d.clone(), secs))
            })?
        };
        let count = self.busy.remove(&device).unwrap_or(0);
        self.abandoned.insert(device.clone());
        if let Some((tx, handle, cancelled)) = self.lanes.remove(&device) {
            // 排着的那几件不再传：设备哪天回应了，卡住的那件返回后线程直接退出，不再往设备上写（`--watch` 进程还活着时）
            cancelled.store(true, std::sync::atomic::Ordering::SeqCst);
            // 不 join：线程可能还卡在 FUSE 上。丢掉 handle 让它自己跑完退出
            drop(tx);
            drop(handle);
        }
        self.current
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&device);
        Some(Event::Stalled {
            device,
            count,
            secs,
        })
    }

    /// 手上还有活、线程却已经退出的设备：去掉这条（下次交给它时重新起），返回 [`Event::Lost`]。
    /// 线程退出前交回的结果都已经在队列里（先看队列、空了才来这里），所以剩下的件数就是丢了的。
    fn dead_lane(&mut self) -> Option<Event> {
        let device = self
            .lanes
            .iter()
            .find(|(d, (_, h, _))| h.is_finished() && self.busy.get(*d).is_some_and(|n| *n > 0))
            .map(|(d, _)| d.clone())?;
        // 线程退出和它最后交回的结果之间没有先后保证：队列里还有的话先交那个
        if let Ok(ev) = self.done_rx.try_recv() {
            return Some(self.count_back(ev));
        }
        if let Some((_, h, _)) = self.lanes.remove(&device) {
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
        for (_, (tx, h, _)) in self.lanes.drain() {
            drop(tx);
            let _ = h.join();
        }
    }
}

/// 跑 `f`，panic 了变成错误（传输线程里一件出了 panic 只算这件传失败，线程接着做下一件）。
fn guarded<T, E: From<String>>(what: &str, f: impl FnOnce() -> Result<T, E>) -> Result<T, E> {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)).unwrap_or_else(|p| {
        let msg = p
            .downcast_ref::<&str>()
            .map(|s| s.to_string())
            .or_else(|| p.downcast_ref::<String>().cloned())
            .unwrap_or_default();
        Err(E::from(format!("{what}时出了内部错误（panic：{msg}）")))
    })
}

/// 一件一件放。一件写满了设备（错误种类 [`Kind::NoSpace`]）后，排着的这一轮都不再写（[`lane_step`]）。
fn file_lane(
    rx: std::sync::mpsc::Receiver<Transfer>,
    done: std::sync::mpsc::Sender<Event>,
    current: Arc<Mutex<HashMap<String, stall::Progress>>>,
    cancelled: Arc<std::sync::atomic::AtomicBool>,
) {
    let mut full = false;
    for t in rx {
        // 主线程放弃了这条线程（设备卡死）：排着的不再碰设备，直接退出（记录不改，下次重传）
        if cancelled.load(std::sync::atomic::Ordering::SeqCst) {
            return;
        }
        // 这一件开始传：进度从这里算；主线程按它发现卡死。线程还卡着时，挂载点一直记着「没返回」，
        // 后面几轮（`--watch`）连接时直接跳过，不再起新线程、不再碰这台
        t.progress.start();
        let _outstanding = stall::StallGuard::enter(&t.stall_key);
        current
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(t.device.clone(), t.progress.clone());
        let r = lane_step(&mut full, || guarded("传到设备", || t.run()));
        current
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&t.device);
        if done.send(Event::Done(Box::new(t), r)).is_err() {
            return;
        }
    }
}

/// 传输线程放一件（纯逻辑）：前面已经写满了（`full`）就不碰设备，直接交回"空间满、没传"——MTP 直接写会先删掉设备上的旧版本，
/// 接着写还是满，等于白删一本完好的书（agy 复审）。这一件写满了就记下 `full`。
fn lane_step(full: &mut bool, run: impl FnOnce() -> Result<bool, Error>) -> Result<bool, Error> {
    if *full {
        return Err(Error::new(
            Kind::NoSpace,
            format!(
                "{}：同一台前面的书已经写满，这一本这一轮没传",
                crate::space::NO_SPACE
            ),
        ));
    }
    let r = run();
    if r.as_ref().is_err_and(|e| e.kind() == Kind::NoSpace) {
        *full = true;
    }
    r
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 写满以后排着的不再写；别的错误不影响后面的。
    #[test]
    fn lane_stops_after_no_space() {
        let mut full = false;
        assert!(lane_step(&mut full, || Err(Error::other("别的错"))).is_err());
        assert!(!full);
        assert_eq!(lane_step(&mut full, || Ok(true)), Ok(true));
        let e = Error::new(Kind::NoSpace, format!("{}：写 x", crate::space::NO_SPACE));
        assert!(lane_step(&mut full, || Err(e)).is_err());
        assert!(full);
        let mut ran = false;
        let r = lane_step(&mut full, || {
            ran = true;
            Ok(true)
        });
        assert!(!ran && r.unwrap_err().kind() == Kind::NoSpace);
    }

    /// 传输线程死了（手上还有活）：主线程等结果时报出来，不一直卡着。
    #[test]
    fn dead_lane_is_reported_not_waited_on_forever() {
        let mut pipe = Pipeline::default();
        let (tx, _rx) = std::sync::mpsc::channel::<Transfer>();
        let h = std::thread::spawn(|| panic!("传输线程意外退出（测试）"));
        pipe.lanes.insert("kindle".into(), (tx, h, Default::default()));
        pipe.busy.insert("kindle".into(), 2);
        match pipe.next(true) {
            Some(Event::Lost { device, count }) => {
                assert_eq!((device.as_str(), count), ("kindle", 2))
            }
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
        let r: Result<(), Error> = guarded("传到设备", || panic!("坏了"));
        assert!(r.unwrap_err().as_str().contains("坏了"));
    }

    /// 一条线程卡住、另一条正常：`next(true)` 在时限后返回 `Stalled`，正常那条的结果照常拿到，
    /// `in_flight` 归零，卡住的那条之后交回的结果被丢掉。
    #[test]
    fn stalled_lane_is_abandoned_and_normal_lane_still_lands() {
        let step = Duration::from_millis(120);
        let mut pipe = Pipeline::default();
        pipe.submit(crate::generate::test_transfer(
            "hanvon",
            step,
            Some(Duration::from_millis(700)),
        ));
        pipe.submit(crate::generate::test_transfer("kindle", step, None));

        let mut stalled = None;
        let mut done = Vec::new();
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while pipe.in_flight() > 0 && std::time::Instant::now() < deadline {
            match pipe.next(true) {
                Some(Event::Stalled {
                    device,
                    count,
                    secs,
                }) => stalled = Some((device, count, secs)),
                Some(Event::Done(t, r)) => done.push((t.device.clone(), r.is_ok())),
                Some(Event::Lost { device, .. }) => panic!("{device} 的线程不该丢"),
                None => break,
            }
        }
        let (device, count, secs) = stalled.expect("应报卡住");
        assert_eq!((device.as_str(), count), ("hanvon", 1));
        assert!(secs >= 1, "至少报 1 秒没进展：{secs}");
        assert!(done.contains(&("kindle".to_string(), true)), "{done:?}");
        assert_eq!(pipe.in_flight(), 0, "放弃和传完的都算清了");
        assert!(pipe.has_room("hanvon"), "这一轮不再给它，但排队上限不卡住");

        // 被放弃的线程（700ms 后）交回结果：丢掉，不再冒出来
        std::thread::sleep(Duration::from_millis(900));
        assert!(pipe.next(false).is_none(), "放弃的线程结果被丢掉");
        assert!(pipe.next(true).is_none(), "没有在传的了");
        assert_eq!(pipe.in_flight(), 0);
        assert_eq!(pipe.busy.get("hanvon"), None);
    }

    /// `Drop` 不 join 放弃的线程：卡住的那条还睡着，drop 马上返回。
    #[test]
    fn drop_does_not_join_abandoned_lanes() {
        let mut pipe = Pipeline::default();
        pipe.submit(crate::generate::test_transfer(
            "hanvon",
            Duration::from_millis(50),
            Some(Duration::from_secs(5)),
        ));
        match pipe.next(true) {
            Some(Event::Stalled { device, .. }) => assert_eq!(device, "hanvon"),
            _ => panic!("应报卡住"),
        }
        let start = std::time::Instant::now();
        drop(pipe);
        assert!(
            start.elapsed() < Duration::from_secs(1),
            "drop 不该等卡住的线程：{:?}",
            start.elapsed()
        );
    }
}
