//! 生成好的产物怎么放上设备：一件要传的（[`Transfer`]）、它要做的（[`Work`]：写到哪个文件），以及 `sync` 的传输线程（[`Pipeline`]）。
//!
//! 依赖方向：`generate`（比较、生成、记录）→ 本模块（传）→ `deliver`（连设备、往 MTP 上写文件）。
//! 记录只在主线程改：传输线程只做 [`Transfer::run`] 这类不碰书库的事，结果交回主线程由 [`crate::Library::complete`] 记下来。

use crate::deliver;
use crate::generate::StateEntry;
use std::collections::HashMap;
use std::path::PathBuf;
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
    pub fn run(&self) -> Result<bool, String> {
        let Work { out, compare, direct, mount } = &self.work;
        if let Some(m) = mount.as_deref().filter(|m| !deliver::mounted(m)) {
            return Err(format!("设备断开了（{} 没挂着），没传", m.display()));
        }
        deliver::put_file(&self.src, out, false, *compare, *direct)
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
    lanes: HashMap<String, (std::sync::mpsc::Sender<Transfer>, std::thread::JoinHandle<()>)>,
    done_tx: std::sync::mpsc::Sender<Event>,
    done_rx: std::sync::mpsc::Receiver<Event>,
    /// 各设备在传（含排队）的件数。
    busy: HashMap<String, usize>,
}

/// 传输线程交回的。
pub enum Event {
    /// 传完一件（成功或失败；成功时是真的写了没有）：交给 [`crate::Library::complete`]。
    Done(Box<Transfer>, Result<bool, String>),
    /// 这台设备的传输线程意外退出了：手上还没交回的 `count` 件没传成（记录没改，下次再传）。
    Lost { device: String, count: usize },
}

/// 等传输线程交回结果时，隔多久看一眼线程还在不在。
const LANE_CHECK: Duration = Duration::from_millis(500);

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

    /// 交给这台设备的传输线程（第一次用时起）。不等：先用 [`Pipeline::has_room`] 看有没有空。
    pub fn submit(&mut self, t: Transfer) {
        *self.busy.entry(t.device.clone()).or_default() += 1;
        let (tx, _) = self.lanes.entry(t.device.clone()).or_insert_with_key(|_| {
            let (tx, rx) = std::sync::mpsc::channel();
            let done = self.done_tx.clone();
            (tx, std::thread::spawn(move || file_lane(rx, done)))
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

/// 一件一件放。
fn file_lane(rx: std::sync::mpsc::Receiver<Transfer>, done: std::sync::mpsc::Sender<Event>) {
    for t in rx {
        let r = guarded("传到设备", || t.run());
        if done.send(Event::Done(Box::new(t), r)).is_err() {
            return;
        }
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
}
