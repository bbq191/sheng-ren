//! 设备挂载卡死时的限时与兜底（2026-11-16 单 `sync-mount-stall`）。
//!
//! 设备（MTP 的 jmtpfs、USB 大容量存储）卡死时，`read_dir`、`stat`、`write`、`close` 都会一直不返回
//! （卡住时 `wchan=request_wait_answer`）。主线程上碰设备的调用没法取消，只能：
//!
//! - **限时探测**（[`run_limited`]）：把「进挂载点」的那一步放到辅助线程，主线程最多等 [`probe_timeout`]；
//!   超时就不等这轮了，辅助线程不 join、由它去。还在跑的都记在 [`outstanding`] 里，这台设备这一轮、甚至
//!   `--watch` 后面几轮都不再起新线程、不再碰它（[`Devices`](crate::session::Devices) 那边照旧只连一次）。
//! - **主线程兜底**（[`watch`]）：主线程上碰设备的一步（清残留临时文件、删产物、看空间、认领、记完删旧位置……）
//!   开始前登记，结束后取消。一个进程一个的看门狗每秒看一次，超过 [`operation_timeout`] 还在的，往标准错误打一行
//!   中文、以退出码 2 结束整个进程（等于 Ctrl-C，靠 `sync` 已有的恢复机制下次重传）。
//! - **传输看门狗**：传输线程按写进度打心跳（[`Progress`]），主线程 [`crate::Pipeline`] 等结果时发现某台太久没进展，
//!   放弃那条线程、这台这一轮不再传。
//!
//! 放弃的线程挡得住进程退出：请求已交给 FUSE 守护进程时，内核不可中断地等它回应（`request_wait_answer`，SIGKILL 也要等；
//! 2026-11-16 验收时用卡死的 FUSE 小程序实测），USB 存储（vfat）硬件卡死的 D 状态也一样。所以 `sync`、`devices` 在工作进程里跑，
//! 退出一律经 [`exit_process`]：放开书库锁、把退出码交给前台进程，前台不等卡住的工作进程（见 `booklib` 的 `supervise`）。
//!
//! 测试钩子：`BOOKLIB_TEST_DEVICE_HANG=<目录>` 时，设备 I/O 进到这个目录下的路径就一直 sleep，模拟卡死的挂载；
//! `BOOKLIB_TEST_DEVICE_HANG_ON=probe|write|main|all` 只让其中一类入口卡住（缺省 `all`）。

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};
use std::time::{Duration, Instant};

/// 时限基数（秒）：`BOOKLIB_DEVICE_TIMEOUT` 可改，缺省 30。
const DEFAULT_SECS: u64 = 30;

/// 长调用（`sync_all`、`close`、`rename`、读回整本）按这个速度给时限：MTP 慢，大漫画不能误判。
const DEVICE_BYTES_PER_SEC: u64 = 256 * 1024;

/// 看门狗多久看一次。
const WATCH_INTERVAL: Duration = Duration::from_secs(1);

/// 时限基数 `T`。
pub(crate) fn base_timeout() -> Duration {
    std::env::var("BOOKLIB_DEVICE_TIMEOUT")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .filter(|&n| n > 0)
        .map(Duration::from_secs)
        .unwrap_or(Duration::from_secs(DEFAULT_SECS))
}

/// 连接时进挂载点看一眼的时限：`T`。
pub(crate) fn probe_timeout() -> Duration {
    base_timeout()
}

/// 主线程单次设备操作的时限、传输途中两次进展之间允许的最长间隔：`2T`。
pub(crate) fn operation_timeout() -> Duration {
    base_timeout() * 2
}

/// 一次调用就要把整本书传完或读完的（`sync_all`、`close`、`rename`、读回比较）：`2T + 字节数 / 256KiB/秒`。
pub(crate) fn long_call_timeout(bytes: u64) -> Duration {
    operation_timeout() + Duration::from_secs_f64(bytes as f64 / DEVICE_BYTES_PER_SEC as f64)
}

/// 测试钩子 `BOOKLIB_TEST_DEVICE_HANG` 的入口类别。
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Where {
    /// 连接探测。
    Probe,
    /// 传输线程写设备上的文件。
    Write,
    /// 主线程兜底包住的设备操作。
    Main,
}

/// 测试钩子：`path` 在 `BOOKLIB_TEST_DEVICE_HANG` 下、而且这一类入口要卡时，一直 sleep。
pub(crate) fn test_hang(path: &Path, at: Where) {
    if hang_enabled(path, at) {
        loop {
            std::thread::sleep(Duration::from_secs(3600));
        }
    }
}

/// 这一类入口现在会不会被测试钩子卡住（`put_file` 据此决定要不要强制走复制，别让同一文件系统上的改名把钩子绕过去）。
pub(crate) fn hang_enabled(path: &Path, at: Where) -> bool {
    let Some(dir) = std::env::var_os("BOOKLIB_TEST_DEVICE_HANG")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
    else {
        return false;
    };
    if !path.starts_with(&dir) {
        return false;
    }
    let only = std::env::var("BOOKLIB_TEST_DEVICE_HANG_ON").unwrap_or_default();
    only.is_empty()
        || only == "all"
        || matches!(
            (at, only.as_str()),
            (Where::Probe, "probe") | (Where::Write, "write") | (Where::Main, "main")
        )
}

#[derive(Default)]
struct Registry {
    /// 挂载点（设备入口路径）→ 还没返回的线程数（探测、被放弃的传输线程）。
    outstanding: HashMap<PathBuf, usize>,
    /// 主线程上正在做的设备操作。
    ops: Vec<Op>,
    next_id: u64,
    /// 看门狗线程在跑。
    watching: bool,
}

struct Op {
    id: u64,
    /// 登记它的线程：[`beat`] 只刷新本线程的（别的设备的传输线程有进展，不能替卡住的主线程续命）。
    thread: std::thread::ThreadId,
    device: String,
    what: String,
    started: Instant,
    limit: Duration,
}

fn registry() -> &'static Mutex<Registry> {
    static REGISTRY: OnceLock<Mutex<Registry>> = OnceLock::new();
    REGISTRY.get_or_init(|| Mutex::new(Registry::default()))
}

fn lock() -> MutexGuard<'static, Registry> {
    registry().lock().unwrap_or_else(|e| e.into_inner())
}

/// 这个挂载点还有没返回的线程（上一轮的探测 / 被放弃的传输线程）。
pub(crate) fn outstanding(key: &Path) -> bool {
    lock().outstanding.get(key).is_some_and(|n| *n > 0)
}

/// 进来时给这个挂载点记一笔，线程结束时自己清掉。
pub(crate) struct StallGuard {
    key: PathBuf,
}

impl StallGuard {
    pub(crate) fn enter(key: &Path) -> StallGuard {
        *lock().outstanding.entry(key.to_path_buf()).or_insert(0) += 1;
        StallGuard {
            key: key.to_path_buf(),
        }
    }
}

impl Drop for StallGuard {
    fn drop(&mut self) {
        let mut r = lock();
        if let Some(n) = r.outstanding.get_mut(&self.key) {
            *n = n.saturating_sub(1);
            if *n == 0 {
                r.outstanding.remove(&self.key);
            }
        }
    }
}

/// 限时跑一个闭包（辅助线程 + `recv_timeout`）：按时返回的拿到结果；超时返回 `None`，线程不 join、放着。
///
/// 线程还在跑时这个 `key` 一直记着（[`outstanding`]），它返回后自己清掉。
pub(crate) fn run_limited<T, F>(key: &Path, limit: Duration, f: F) -> Option<T>
where
    T: Send + 'static,
    F: FnOnce() -> T + Send + 'static,
{
    let guard = StallGuard::enter(key);
    let (tx, rx) = mpsc::channel();
    let handle = std::thread::spawn(move || {
        let _guard = guard;
        let _ = tx.send(f());
    });
    match rx.recv_timeout(limit) {
        Ok(v) => {
            let _ = handle.join();
            Some(v)
        }
        Err(_) => None,
    }
}

/// 主线程上包一层设备操作：开始前登记，结束后取消；超过 `2T` 由看门狗打一行、退出码 2 结束进程。
pub(crate) fn watch<T>(device: &str, key: &Path, what: &str, f: impl FnOnce() -> T) -> T {
    let guard = OpGuard::begin(device, key, what);
    let v = f();
    drop(guard);
    v
}

struct OpGuard {
    id: u64,
}

impl OpGuard {
    fn begin(device: &str, key: &Path, what: &str) -> OpGuard {
        let id = {
            let mut r = lock();
            let id = r.next_id;
            r.next_id += 1;
            r.ops.push(Op {
                id,
                thread: std::thread::current().id(),
                device: device.to_string(),
                what: what.to_string(),
                started: Instant::now(),
                limit: operation_timeout(),
            });
            id
        };
        ensure_watcher();
        // 设备目录在测试钩子下面：这一步整个卡住（模拟主线程碰上卡死的挂载）
        test_hang(key, Where::Main);
        OpGuard { id }
    }
}

impl Drop for OpGuard {
    fn drop(&mut self) {
        lock().ops.retain(|o| o.id != self.id);
    }
}

/// 主线程上一步设备操作有进展（读完一块、删完一个文件）：本线程登记着的操作从现在起重新计时。
/// 读回整本（`same_bytes`、算哈希）、一次删一批文件时调用，大文件、长列表不会被误判成卡死；本线程没登记的话什么都不做。
pub(crate) fn beat() {
    let me = std::thread::current().id();
    let mut r = lock();
    if r.ops.is_empty() {
        return;
    }
    let now = Instant::now();
    for o in r.ops.iter_mut().filter(|o| o.thread == me) {
        o.started = now;
    }
}

/// 书库锁的文件描述符（持锁时登记，见 `fsutil::lock`）：兜底退出时先放开它。
static LOCK_FD: std::sync::atomic::AtomicI32 = std::sync::atomic::AtomicI32::new(-1);

/// 持锁 / 放锁时登记书库锁的文件描述符（`-1` 表示没持锁）。
pub(crate) fn note_lock_fd(fd: i32) {
    LOCK_FD.store(fd, std::sync::atomic::Ordering::SeqCst);
}

/// 结束进程（`booklib` 所有会碰设备的退出都走这里）。
///
/// 卡在 FUSE 上的线程等的是已经交给 FUSE 守护进程的请求，内核不可中断地等它回应（`request_wait_answer`，连 SIGKILL 也要等）：
/// 进程的主线程退出了，整个进程却要等那条线程，成了僵尸，命令行一直回不来，书库锁（flock）也跟着不放。所以这里：
/// 1. 先放开书库锁（卡住的线程从内核返回时会被结束、不会再跑用户态代码，放锁是安全的），下次 `booklib` 不会被挡；
/// 2. 刷掉标准输出、标准错误；
/// 3. 是 `booklib` 的工作进程（环境变量 `BOOKLIB_EXIT_FD`，见 `booklib` 的 `supervise`）的话，把退出码写给前台进程，
///    前台进程拿到就按这个码退出，不等工作进程真的结束；
/// 4. `std::process::exit`。
pub fn exit_process(code: i32) -> ! {
    let fd = LOCK_FD.swap(-1, std::sync::atomic::Ordering::SeqCst);
    if fd >= 0 {
        // SAFETY: 只对登记过的书库锁描述符放锁，不关它
        unsafe {
            libc::flock(fd, libc::LOCK_UN);
        }
    }
    {
        use std::io::Write;
        let _ = std::io::stdout().flush();
        let _ = std::io::stderr().flush();
    }
    if let Some(fd) = std::env::var("BOOKLIB_EXIT_FD")
        .ok()
        .and_then(|v| v.parse::<i32>().ok())
    {
        let b = [code.clamp(0, 255) as u8];
        // SAFETY: 写前台进程传下来的管道写端，一个字节
        unsafe {
            libc::write(fd, b.as_ptr().cast(), 1);
        }
    }
    std::process::exit(code)
}

/// 第一次登记时起看门狗（整个进程一个）。
fn ensure_watcher() {
    let start = {
        let mut r = lock();
        if r.watching {
            false
        } else {
            r.watching = true;
            true
        }
    };
    if start {
        std::thread::spawn(|| {
            loop {
                std::thread::sleep(WATCH_INTERVAL);
                let hit = {
                    let now = Instant::now();
                    let r = lock();
                    r.ops.iter().find_map(|o| {
                        let took = now.duration_since(o.started);
                        (took > o.limit).then(|| {
                            (
                                o.device.clone(),
                                o.what.clone(),
                                took.as_secs().max(1),
                            )
                        })
                    })
                };
                if let Some((device, what, secs)) = hit {
                    eprintln!(
                        "✗ [{device}] 挂载没响应（{what} {secs} 秒没返回），booklib 退出；没传完的书记录没改，下次接上会重传"
                    );
                    exit_process(2);
                }
                let mut r = lock();
                if r.ops.is_empty() {
                    r.watching = false;
                    return;
                }
            }
        });
    }
}

/// 一条传输线程的进度：最近一次进展的时刻、当前这一步的时限。
#[derive(Clone)]
pub(crate) struct Progress(Arc<Mutex<Prog>>);

struct Prog {
    /// 最近一次进展（一块写成功、一步开始）。
    last: Instant,
    /// 这一步最多允许多久没进展（`2T`，或长调用按字节数放宽）。
    limit: Duration,
    /// 一步开始时用的基准时限（`2T`）。
    step: Duration,
}

impl Progress {
    pub(crate) fn new(step: Duration) -> Progress {
        Progress(Arc::new(Mutex::new(Prog {
            last: Instant::now(),
            limit: step,
            step,
        })))
    }

    fn prog(&self) -> MutexGuard<'_, Prog> {
        self.0.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// 开始传下一件：从头计时。
    pub(crate) fn start(&self) {
        let mut p = self.prog();
        p.last = Instant::now();
        p.limit = p.step;
    }

    /// 开始一步（写完一块、做完一次短调用）：从现在起按基准时限算。
    pub(crate) fn begin_step(&self) {
        let mut p = self.prog();
        p.last = Instant::now();
        p.limit = p.step;
    }

    /// 做完一步：从现在起按基准时限算。
    pub(crate) fn beat(&self) {
        self.begin_step();
    }

    /// 进入一次「一次调用就要把整本传完/读完」的长调用：时限放宽到 `2T + 字节数 / 256KiB/秒`。
    pub(crate) fn beat_long(&self, bytes: u64) {
        let mut p = self.prog();
        p.last = Instant::now();
        p.limit = long_call_timeout(bytes);
    }

    /// 超过当前这一步的时限还没进展的话，返回已经多少秒（至少 1）。
    pub(crate) fn stalled_secs(&self) -> Option<u64> {
        let p = self.prog();
        let took = p.last.elapsed();
        (took > p.limit).then(|| took.as_secs().max(1))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 限时跑闭包：按时返回的拿到结果；超时的返回 `None`、不 join 线程；放弃登记在线程结束后清掉。
    #[test]
    fn run_limited_returns_or_times_out_and_clears_when_thread_ends() {
        let key = PathBuf::from("/测试/挂载点");
        assert_eq!(
            run_limited(&key, Duration::from_secs(5), || 42),
            Some(42),
            "按时返回的拿到结果"
        );
        assert!(!outstanding(&key), "正常返回的不留登记");

        let start = Instant::now();
        let r = run_limited(&key, Duration::from_millis(50), || {
            std::thread::sleep(Duration::from_millis(300));
            7
        });
        assert_eq!(r, None, "超时返回 None");
        assert!(
            start.elapsed() < Duration::from_millis(250),
            "不 join 还在跑的线程：{:?}",
            start.elapsed()
        );
        assert!(outstanding(&key), "放弃的线程记着");
        // 线程结束后自己清掉
        let deadline = Instant::now() + Duration::from_secs(3);
        while outstanding(&key) && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(!outstanding(&key), "线程结束后登记清掉");
    }

    /// 进度：按时打心跳的不算卡；超过时限没进展才算，长调用的时限按字节数放宽。
    #[test]
    fn progress_reports_only_when_over_the_step_limit() {
        let p = Progress::new(Duration::from_millis(80));
        assert_eq!(p.stalled_secs(), None);
        std::thread::sleep(Duration::from_millis(120));
        assert!(p.stalled_secs().is_some(), "没心跳：算卡住");
        p.beat();
        assert_eq!(p.stalled_secs(), None, "打心跳以后重新算");
        std::thread::sleep(Duration::from_millis(120));
        p.beat_long(64 * 1024 * 1024);
        assert_eq!(p.stalled_secs(), None, "长调用按字节数放宽");
        assert!(long_call_timeout(64 * 1024 * 1024) > operation_timeout());
    }

    /// 主线程兜底：登记在线程/闭包结束后取消；看门狗不会因为短操作退出。
    #[test]
    fn watch_registers_and_clears_main_thread_ops() {
        let key = PathBuf::from("/测试/主线程");
        // 登记表是整个进程一份，别的单测可能同时登记：只看本测试自己那一项
        let mine = |r: &Registry| r.ops.iter().filter(|o| o.what == "单测：看设备").count();
        let got = watch("kindle", &key, "单测：看设备", || {
            assert_eq!(mine(&lock()), 1, "做的时候登记着");
            let before = lock()
                .ops
                .iter()
                .find(|o| o.what == "单测：看设备")
                .map(|o| o.started)
                .unwrap();
            std::thread::sleep(Duration::from_millis(20));
            beat();
            let after = lock()
                .ops
                .iter()
                .find(|o| o.what == "单测：看设备")
                .map(|o| o.started)
                .unwrap();
            assert!(after > before, "beat 重新计时");
            5
        });
        assert_eq!(got, 5);
        assert_eq!(mine(&lock()), 0, "做完取消");
    }
}
