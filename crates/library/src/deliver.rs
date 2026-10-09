//! 产物送到设备（2026-10-07 用户定：`sync` 按设备接没接上直接传，电脑上不留产物）。怎么送由 profile 的 `[deliver]` 定：
//!
//! - **MTP**（Kindle、掌阅）：jmtpfs 自动挂载在 `<挂载目录>/<mount>`（缺省 `$XDG_RUNTIME_DIR/mtp`），下面是存储
//!   （「Internal Storage」之类），产物放进存储根目录的 `<dir>/`，按普通文件写。挂载点在、里面有存储就算接上了。
//! - **xochitl**（Move）：经 SSH 端口转发调设备上书架服务的导入接口：`POST /import?name=&folder=` 新加入（回 uuid），
//!   `POST /import?uuid=&name=` 原地替换（保留 uuid、阅读进度、所在文件夹；uuid 不在了回 404），`GET /import/<uuid>` 查还在不在，
//!   删除走它的回收站队列 `POST /trash/add {uuid, name}`（进 xochitl 回收站，能恢复）。
//! - 没写 `[deliver]` 的模式（书库 `profiles/` 里的自定义模式）：产物照旧放在电脑上。
//!
//! 一轮 `sync` 里每台设备只连一次。`--watch` 每轮开头调 [`crate::Library::refresh_devices`]：MTP 设备、没接上的下次用到时
//! 重新看；到 Move 的连接还通就接着用，断了才重连。

use crate::generate::{Done, Transfer, Work};
use crate::net::enc;
use profile::{Deliver, Profile};
use std::collections::HashMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

/// 设备在哪找（测试、特殊环境可改）。
#[derive(Clone, Debug)]
pub struct DeviceEnv {
    /// MTP 挂载点的上级目录（`<它>/<mount>`）。缺省 `$BOOKLIB_MTP_DIR`，没设就是 `$XDG_RUNTIME_DIR/mtp`。
    pub mtp_base: PathBuf,
    /// 直接用这个地址调 xochitl 的导入接口、不走 SSH（`$BOOKLIB_XOCHITL_URL`，测试用）。
    pub xochitl_url: Option<String>,
    /// 不连 Move（`$BOOKLIB_NO_SSH=1`；测试里别碰到插着的真机）。
    pub no_ssh: bool,
}

impl DeviceEnv {
    pub fn from_env() -> DeviceEnv {
        let var = |k: &str| std::env::var_os(k).filter(|v| !v.is_empty());
        let mtp_base = var("BOOKLIB_MTP_DIR").map(PathBuf::from).unwrap_or_else(|| {
            let run = var("XDG_RUNTIME_DIR").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(format!("/run/user/{}", uid())));
            run.join("mtp")
        });
        DeviceEnv {
            mtp_base,
            xochitl_url: var("BOOKLIB_XOCHITL_URL").map(|v| v.to_string_lossy().trim_end_matches('/').to_string()),
            no_ssh: var("BOOKLIB_NO_SSH").is_some_and(|v| v != "0"),
        }
    }
}

fn uid() -> u32 {
    // /proc/self 的属主就是本进程的 uid（不为这一个数引 libc）
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        std::fs::metadata("/proc/self").map(|m| m.uid()).unwrap_or(1000)
    }
    #[cfg(not(unix))]
    1000
}

/// 一个模式的产物送到哪。
pub(crate) enum Target {
    /// 放在电脑上（没写 `[deliver]`）。
    Local,
    /// MTP 设备上的产物根目录（`<存储>/<dir>`）。
    Dir { root: PathBuf },
    Xochitl(Xochitl),
}

/// 连设备：接上了返回送到哪，没接上返回原因。
pub(crate) fn connect(env: &DeviceEnv, p: &Profile) -> Result<Target, String> {
    match &p.deliver {
        None => Ok(Target::Local),
        Some(Deliver::Mtp { mount, dir }) => {
            let point = env.mtp_base.join(mount);
            let storage = mtp_storage(&point).ok_or_else(|| format!("没接上（{} 不在或没挂上）", point.display()))?;
            // `dir` 不在（掌阅原来没有 documents/）的话传书时再建：只看接没接上的命令（devices）不往设备上写
            Ok(Target::Dir { root: storage.join(dir) })
        }
        Some(Deliver::Xochitl { hosts, port }) => {
            if let Some(url) = &env.xochitl_url {
                return Xochitl::direct(url).map(Target::Xochitl);
            }
            if env.no_ssh {
                return Err("没接上（BOOKLIB_NO_SSH）".into());
            }
            Xochitl::connect(hosts, *port).map(Target::Xochitl)
        }
    }
}

/// 挂载点下的存储目录：只有一个就是它；几个（插了存储卡）时取名字像内部存储的，否则按名字排第一个。
fn mtp_storage(point: &Path) -> Option<PathBuf> {
    let mut dirs: Vec<PathBuf> = std::fs::read_dir(point)
        .ok()?
        .flatten()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()) && !e.file_name().to_string_lossy().starts_with('.'))
        .map(|e| e.path())
        .collect();
    dirs.sort();
    let internal = dirs.iter().position(|d| d.file_name().is_some_and(|n| {
        let n = n.to_string_lossy().to_lowercase();
        n.contains("internal") || n.contains("内部")
    }));
    match internal {
        Some(i) => Some(dirs.swap_remove(i)),
        None => dirs.into_iter().next(),
    }
}

/// 把文件 `src` 放到 `dest`（目录没有就建；同一文件系统直接改名；`keep_src` 时或跨文件系统时复制）。`compare` 时 `dest` 已经是逐字节相同的
/// 文件就不动它（Kindle 覆盖成不同字节会清掉阅读进度，相同的不会；也省得在 MTP 上白拷）——要把 `dest` 读回来，调用方能用记录的
/// 哈希判断时传 `false`。复制缺省先写旁边的临时文件再换上去（先删旧的再改名：MTP 上改名不能覆盖已有文件）；`direct` 时先删旧的、
/// 直接写 `dest`——jmtpfs 的改名是把整个文件下载回来再上传一份，经临时文件等于传三遍（调用方要自己保证写到一半被打断时认得出来，
/// 见 `generate::Library::prepare_file`）。返回有没有真的写。
///
/// 目录刚建好、文件还没放进去时，主线程可能正好把它当成"变空的目录"删掉（删别的书的旧位置时顺带删空目录，
/// `generate::State::delete_placements`）：碰到目录不在了就重建再放，试几次。
pub(crate) fn put_file(src: &Path, dest: &Path, keep_src: bool, compare: bool, direct: bool) -> Result<bool, String> {
    let mut tries = 0;
    loop {
        match put_file_once(src, dest, keep_src, compare, direct) {
            Err((_, true)) if tries < 3 && dest.parent().is_some_and(|d| !d.is_dir()) && src.is_file() => tries += 1,
            r => return r.map_err(|(e, _)| e),
        }
    }
}

/// [`put_file`] 的一次：出错时第二项说是不是"目录（或文件）不在"一类的错误（可以重建目录再试）。
fn put_file_once(src: &Path, dest: &Path, keep_src: bool, compare: bool, direct: bool) -> Result<bool, (String, bool)> {
    let missing = |e: &std::io::Error| e.kind() == std::io::ErrorKind::NotFound;
    if let Some(dir) = dest.parent() {
        std::fs::create_dir_all(dir).map_err(|e| (format!("{}: {e}", dir.display()), missing(&e)))?;
    }
    if compare && same_bytes(src, dest) {
        if !keep_src {
            let _ = std::fs::remove_file(src);
        }
        return Ok(false);
    }
    if !keep_src && std::fs::rename(src, dest).is_ok() {
        let _ = crate::fsutil::sync_parent(dest);
        return Ok(true);
    }
    let tmp = if direct {
        let _ = std::fs::remove_file(dest);
        dest.to_path_buf()
    } else {
        crate::fsutil::tmp_sibling(dest)
    };
    let copy = || -> std::io::Result<()> {
        // 大块读写：经 FUSE（jmtpfs）每次写都是一次往返，缺省 8KB 一块太碎
        let mut r = std::io::BufReader::with_capacity(COPY_BUF, std::fs::File::open(src)?);
        let mut w = std::io::BufWriter::with_capacity(COPY_BUF, std::fs::File::create(&tmp)?);
        std::io::copy(&mut r, &mut w)?;
        w.into_inner().map_err(|e| e.into_error())?.sync_all()
    };
    if let Err(e) = copy() {
        let _ = std::fs::remove_file(&tmp);
        return Err((format!("写 {}: {e}", dest.display()), missing(&e)));
    }
    if !direct {
        let _ = std::fs::remove_file(dest);
        if let Err(e) = std::fs::rename(&tmp, dest) {
            let _ = std::fs::remove_file(&tmp);
            return Err((format!("改名成 {}: {e}", dest.display()), missing(&e)));
        }
    }
    let _ = crate::fsutil::sync_parent(dest);
    if !keep_src {
        let _ = std::fs::remove_file(src);
    }
    Ok(true)
}

/// 往设备上拷文件、传给 Move 的读写块大小。
const COPY_BUF: usize = 1 << 20;

/// 两个文件逐字节相同（`b` 不在算不同）。先比大小，一样才读。
pub(crate) fn same_bytes(a: &Path, b: &Path) -> bool {
    let (Ok(ma), Ok(mb)) = (std::fs::metadata(a), std::fs::metadata(b)) else { return false };
    if !mb.is_file() || ma.len() != mb.len() {
        return false;
    }
    let (Ok(fa), Ok(fb)) = (std::fs::File::open(a), std::fs::File::open(b)) else { return false };
    let (mut ra, mut rb) = (std::io::BufReader::with_capacity(1 << 16, fa), std::io::BufReader::with_capacity(1 << 16, fb));
    let (mut ba, mut bb) = (vec![0u8; 1 << 16], vec![0u8; 1 << 16]);
    loop {
        let n = match ra.read(&mut ba) {
            Ok(n) => n,
            Err(_) => return false,
        };
        if n == 0 {
            return rb.read(&mut bb[..1]).is_ok_and(|m| m == 0);
        }
        if rb.read_exact(&mut bb[..n]).is_err() || ba[..n] != bb[..n] {
            return false;
        }
    }
}

/// xochitl 里的一本书（导入接口的回执）。
#[derive(Debug, Clone)]
pub struct Doc {
    pub uuid: String,
    /// xochitl 书库里显示的名字（`.metadata` 的 `visibleName`；回收站接口要核对它）。
    pub name: String,
}

/// 给人看的时长：`45 秒`、`2 分 10 秒`。
fn elapsed(d: Duration) -> String {
    let s = d.as_secs();
    if s < 60 { format!("{s} 秒") } else { format!("{} 分 {} 秒", s / 60, s % 60) }
}

/// 交了导入（或替换）以后：书架服务给的任务号（它在后台加入 xochitl、等排版，之后查 [`MoveClient::poll`]），
/// 或旧版书架服务当场给的结果。
pub(crate) enum Submitted {
    Job(String),
    Done(Doc),
}

/// 导入任务现在怎样。
pub(crate) enum JobState {
    /// 还在做：书架服务说的当前阶段（排队、建文件夹、等 xochitl 排版……）。
    Running(String),
    Done(Doc),
    Failed(String),
}

/// 连 Move 之前探 SSH 端口最多等多久（USB 网卡、局域网里几毫秒就通）。
const SSH_PROBE: Duration = Duration::from_millis(800);

/// 查导入任务的间隔。
const POLL: Duration = Duration::from_secs(2);
/// 要替换的那本书架服务那边正在替换时最多等多久（大漫画排版要几分钟）；旧版书架服务查不到替换做完没有，隔多久重交。
const BUSY_RETRY: Duration = Duration::from_secs(5);
const BUSY_WAIT: Duration = Duration::from_secs(30 * 60);

/// 交书没交成：这本正在替换（等一会儿再交），或别的错。
enum Sent {
    Busy(String),
    Failed(String),
}

impl Sent {
    fn into_message(self) -> String {
        match self {
            Sent::Busy(m) => format!("传到 Move 失败：{m}（等了 {} 分钟还在替换）", BUSY_WAIT.as_secs() / 60),
            Sent::Failed(m) => m,
        }
    }
}

/// 调 Move 上书架服务的接口：只有地址和 HTTP 客户端，能复制、能交给传输线程用（SSH 隧道留在 [`Xochitl`] 里）。
#[derive(Clone)]
pub(crate) struct MoveClient {
    base: String,
    agent: ureq::Agent,
}

impl MoveClient {
    fn new(base: String) -> MoveClient {
        // 读写超时只管一次请求（传书体、查任务）；等 xochitl 排版是查任务，不挂着连接
        MoveClient { base, agent: ureq::AgentBuilder::new().timeout_connect(Duration::from_secs(5)).timeout_read(IO_TIMEOUT).timeout_write(IO_TIMEOUT).build() }
    }

    fn status(&self) -> Result<(), String> {
        self.agent.get(&format!("{}/status", self.base)).timeout(Duration::from_secs(5)).call().map(|_| ()).map_err(|e| format!("书架服务没答话（{}）", http_err(e)))
    }

    /// 交一本新书：放进文件夹 `folder`（`a/b` 多级，书架服务逐级找、没有就建；空 = 书库根）。
    pub(crate) fn submit_import(&self, file: &Path, name: &str, folder: &str) -> Result<Submitted, String> {
        let url = format!("{}/import?name={}&folder={}", self.base, enc(name), enc(folder));
        self.send(&url, file).map_err(Sent::into_message)?.ok_or_else(|| "导入接口回 404（设备上的书架服务太旧，没有导入接口）".into())
    }

    /// 交一份新内容原地替换 `uuid`（保留 uuid、进度、文件夹）。这本在设备上已经不在了（删了、进了回收站）返回 `None`。
    /// 书架服务那边这本正在替换（上一次 `sync` 交的还没做完，比如被 Ctrl+C 打断以后它还在后台排版大漫画；回 409，旧版回 400）：
    /// 等它做完再交（[`MoveClient::wait_replaced`]），最多等 [`BUSY_WAIT`]（2026-10-09 真机：第二次 `sync` 交《哆啦A夢》卷03 时回"正在替换中"，算成了失败）。
    pub(crate) fn submit_replace(&self, uuid: &str, file: &Path, name: &str) -> Result<Option<Submitted>, String> {
        let url = format!("{}/import?uuid={}&name={}", self.base, enc(uuid), enc(name));
        let start = Instant::now();
        loop {
            match self.send(&url, file) {
                Err(Sent::Busy(_)) if start.elapsed() < BUSY_WAIT => self.wait_replaced(uuid, start),
                r => return r.map_err(Sent::into_message),
            }
        }
    }

    /// 等书架服务把这本上一次的替换做完：轮询 `GET /import/<uuid>` 的 `replacing`（轻，不用一遍遍重传整本书）。书架服务太旧、
    /// 回执里没有这个字段的，等 [`BUSY_RETRY`] 就回去重交；查不了的也回去重交（重交时再见分晓）。
    fn wait_replaced(&self, uuid: &str, start: Instant) {
        loop {
            std::thread::sleep(POLL);
            let replacing = self.agent.get(&format!("{}/import/{}", self.base, enc(uuid))).call().ok().and_then(|r| json_of(r).ok()).and_then(|v| v["replacing"].as_bool());
            match replacing {
                Some(true) if start.elapsed() < BUSY_WAIT => {}
                Some(_) => return,
                None => {
                    std::thread::sleep(BUSY_RETRY);
                    return;
                }
            }
        }
    }

    fn send(&self, url: &str, file: &Path) -> Result<Option<Submitted>, Sent> {
        let f = std::fs::File::open(file).map_err(|e| Sent::Failed(format!("读 {}: {e}", file.display())))?;
        let len = f.metadata().map_err(|e| Sent::Failed(e.to_string()))?.len();
        let r = self.agent.post(url).set("Content-Type", "application/epub+zip").set("Content-Length", &len.to_string()).send(std::io::BufReader::with_capacity(COPY_BUF, f));
        match r {
            Ok(resp) => {
                let v = json_of(resp).map_err(Sent::Failed)?;
                Ok(Some(match v["job"].as_str() {
                    Some(job) => Submitted::Job(job.to_string()),
                    None => Submitted::Done(doc_of(&v).map_err(Sent::Failed)?),
                }))
            }
            Err(ureq::Error::Status(404, _)) => Ok(None),
            Err(e) => {
                let busy = matches!(&e, ureq::Error::Status(409, _));
                let msg = http_err(e);
                // 旧版书架服务回 400 + "正在替换中"，新版回 409
                if busy || msg.contains("正在替换") { Err(Sent::Busy(msg)) } else { Err(Sent::Failed(format!("传到 Move 失败：{msg}"))) }
            }
        }
    }

    /// 查导入任务。书架服务不认识这个任务（重启过）算失败：书可能已经加进去了，下次 sync 交同样的内容时它会认回来，不重复加。
    pub(crate) fn poll(&self, job: &str) -> Result<JobState, String> {
        match self.agent.get(&format!("{}/import/jobs/{}", self.base, enc(job))).call() {
            Ok(resp) => {
                let v = json_of(resp)?;
                Ok(match v["state"].as_str() {
                    Some("done") => JobState::Done(doc_of(&v)?),
                    Some("failed") => JobState::Failed(v["message"].as_str().unwrap_or("书架服务没说原因").to_string()),
                    _ => JobState::Running(v["stage"].as_str().unwrap_or_default().to_string()),
                })
            }
            Err(ureq::Error::Status(404, _)) => Ok(JobState::Failed("书架服务那边没有这个导入任务了（重启过？）".into())),
            Err(e) => Err(format!("查 Move 上的导入任务失败：{}", http_err(e))),
        }
    }

    /// 等交出去的这件做完（单本生成、测试用；`sync` 的传输线程自己轮流查，见 [`crate::Pipeline`]）。
    pub(crate) fn wait(&self, s: Submitted) -> Result<Doc, String> {
        let job = match s {
            Submitted::Done(d) => return Ok(d),
            Submitted::Job(j) => j,
        };
        loop {
            match self.poll(&job)? {
                JobState::Done(d) => return Ok(d),
                JobState::Failed(m) => return Err(format!("Move 上加入失败：{m}")),
                JobState::Running(_) => std::thread::sleep(POLL),
            }
        }
    }

    /// 一批文档还在不在 xochitl 书库里（`POST /import/states`，一次请求）：uuid → 在（没删、没进回收站）。设备上的书架服务太旧、
    /// 没有这个接口时 `None`（调用方改成一本一本查）。
    pub(crate) fn states(&self, uuids: &[String]) -> Result<Option<HashMap<String, bool>>, String> {
        let r = self.agent.post(&format!("{}/import/states", self.base)).set("Content-Type", "application/json").send_string(&serde_json::json!({ "uuids": uuids }).to_string());
        match r {
            Ok(resp) => {
                let v = json_of(resp)?;
                let docs = v["docs"].as_object().ok_or("书架服务的回执里没有 docs")?;
                Ok(Some(uuids.iter().map(|u| (u.clone(), docs.get(u).is_some_and(|d| !d["deleted"].as_bool().unwrap_or(false)))).collect()))
            }
            Err(ureq::Error::Status(404 | 405, _)) => Ok(None),
            Err(e) => Err(format!("查 Move 上的书失败：{}", http_err(e))),
        }
    }

    /// 这本还在 xochitl 书库里（没删、没进回收站）。
    pub(crate) fn present(&self, uuid: &str) -> Result<bool, String> {
        match self.agent.get(&format!("{}/import/{}", self.base, enc(uuid))).call() {
            Ok(resp) => {
                let v = json_of(resp)?;
                Ok(!v["deleted"].as_bool().unwrap_or(false))
            }
            Err(ureq::Error::Status(404, _)) => Ok(false),
            Err(e) => Err(format!("查 Move 上的书失败：{}", http_err(e))),
        }
    }

    /// 移进 xochitl 回收站（已经不在了算成功）。
    pub(crate) fn trash(&self, uuid: &str, name: &str) -> Result<(), String> {
        if !self.present(uuid)? {
            return Ok(());
        }
        self.agent
            .post(&format!("{}/trash/add", self.base))
            .set("Content-Type", "application/json")
            .send_string(&serde_json::json!({"uuid": uuid, "name": name}).to_string())
            .map(|_| ())
            .map_err(|e| format!("删 Move 上的《{name}》失败：{}", http_err(e)))
    }
}

/// 到 Move 上书架服务的连接：SSH 端口转发（[`Xochitl::connect`]）或直接给的地址（测试）。接口调用见 [`MoveClient`]（`Deref`）。
pub(crate) struct Xochitl {
    client: MoveClient,
    tunnel: std::cell::RefCell<Option<Child>>,
    /// 连的是哪台（提示用）。
    pub host: String,
    /// 这一轮查回来的书还在不在（[`Xochitl::present_cached`]）。
    presence: std::cell::RefCell<Presence>,
}

/// Move 上的书在不在，这一轮查过没有。
enum Presence {
    /// 还没查（每轮 `sync` 开头、`--watch` 每轮重新查）。
    Unknown,
    /// 一次查回来的一批：uuid → 在不在。
    Known(HashMap<String, bool>),
    /// 书架服务太旧、没有批量接口：一本一本查。
    Unsupported,
}

impl std::ops::Deref for Xochitl {
    type Target = MoveClient;
    fn deref(&self) -> &MoveClient {
        &self.client
    }
}

impl Drop for Xochitl {
    fn drop(&mut self) {
        if let Some(c) = self.tunnel.get_mut().as_mut() {
            let _ = c.kill();
            let _ = c.wait();
        }
    }
}

/// 传书体（大漫画经 USB 也要一阵）、查一次的读写超时。等排版不靠它（查任务）。
const IO_TIMEOUT: Duration = Duration::from_secs(600);

impl Xochitl {
    /// 这本还在不在：这一轮第一次问的时候把 `all()`（生成记录里这台 Move 上的全部 uuid）一次查回来，之后查表——以前每本书各开
    /// 一次请求，几百本经 Wi-Fi 和 SSH 隧道要好几秒（2026-10-09）。表里没有的（这一轮新加的）、书架服务太旧没有批量接口的，一本一本查。
    pub(crate) fn present_cached(&self, uuid: &str, all: impl FnOnce() -> Vec<String>) -> Result<bool, String> {
        let mut p = self.presence.borrow_mut();
        if matches!(*p, Presence::Unknown) {
            *p = match self.client.states(&all())? {
                Some(m) => Presence::Known(m),
                None => Presence::Unsupported,
            };
        }
        if let Presence::Known(m) = &*p {
            if let Some(&alive) = m.get(uuid) {
                return Ok(alive);
            }
        }
        drop(p);
        self.client.present(uuid)
    }

    /// 下一轮重新查（`sync --watch` 每轮开头：这期间用户可能在 Move 上删了书）。书架服务太旧的记着，不再试批量接口。
    pub(crate) fn forget_presence(&self) {
        let mut p = self.presence.borrow_mut();
        if matches!(*p, Presence::Known(_)) {
            *p = Presence::Unknown;
        }
    }

    fn direct(url: &str) -> Result<Xochitl, String> {
        let x = Xochitl { client: MoveClient::new(url.to_string()), tunnel: Default::default(), host: url.to_string(), presence: std::cell::RefCell::new(Presence::Unknown) };
        x.status()?;
        Ok(x)
    }

    /// 给传输线程用的一份客户端。
    pub(crate) fn client(&self) -> MoveClient {
        self.client.clone()
    }

    /// 依次试 `hosts`：SSH 连得上、端口转发建得起来、书架服务答话的第一个。
    fn connect(hosts: &[String], port: u16) -> Result<Xochitl, String> {
        let mut why = Vec::new();
        for host in hosts {
            match Self::tunnel(host, port) {
                Ok(x) => return Ok(x),
                Err(e) => why.push(format!("{host}：{e}")),
            }
        }
        Err(format!("没接上（{}）", why.join("；")))
    }

    fn tunnel(host: &str, port: u16) -> Result<Xochitl, String> {
        // 地址是 IP 的先探一下 22 端口：Move 没接、睡了时不用等 ssh 超时（每个地址最长 8 秒，`sync --watch` 每轮都要等两个地址；
        // 2026-10-09 审计）。写的是 ssh 配置里的别名（端口、跳板机可能另配）就不探，交给 ssh
        if let Some(ip) = host.rsplit('@').next().and_then(|h| h.parse::<std::net::IpAddr>().ok()) {
            if std::net::TcpStream::connect_timeout(&(ip, 22).into(), SSH_PROBE).is_err() {
                return Err("SSH 端口连不上（设备没接上或睡着了）".into());
            }
        }
        // 本地随便挑一个空闲端口（绑 0 再放开；和 ssh 绑上之间被别人占走的话，ssh 因 ExitOnForwardFailure 退出，报连不上）
        let local = std::net::TcpListener::bind("127.0.0.1:0").and_then(|l| l.local_addr()).map_err(|e| e.to_string())?.port();
        let child = Command::new("ssh")
            // 不复用 ssh 配置里的 ControlMaster：复用时转发交给已有的主连接、ssh 本身立刻退出，转发也不随本进程结束
            .args(["-o", "BatchMode=yes", "-o", "ConnectTimeout=4", "-o", "ExitOnForwardFailure=yes", "-o", "ServerAliveInterval=15", "-o", "ControlMaster=no", "-o", "ControlPath=none", "-N", "-L"])
            .arg(format!("127.0.0.1:{local}:127.0.0.1:{port}"))
            .arg(host)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| format!("起不了 ssh：{e}"))?;
        let x = Xochitl { client: MoveClient::new(format!("http://127.0.0.1:{local}")), tunnel: std::cell::RefCell::new(Some(child)), host: host.to_string(), presence: std::cell::RefCell::new(Presence::Unknown) };
        let deadline = Instant::now() + Duration::from_secs(8);
        loop {
            if let Some(st) = x.tunnel.borrow_mut().as_mut().and_then(|c| c.try_wait().ok().flatten()) {
                return Err(format!("SSH 连不上（{st}）"));
            }
            if std::net::TcpStream::connect_timeout(&([127, 0, 0, 1], local).into(), Duration::from_millis(300)).is_ok() {
                // 转发通了还要书架服务答话（服务没起时连接会被设备那头断掉）
                match x.status() {
                    Ok(()) => return Ok(x),
                    Err(e) if Instant::now() > deadline => return Err(e),
                    Err(_) => {}
                }
            } else if Instant::now() > deadline {
                return Err("端口转发没建起来".into());
            }
            std::thread::sleep(Duration::from_millis(200));
        }
    }

    /// 隧道还在、书架服务还答话（Move 拔了、睡了、服务停了都算断了）。
    pub(crate) fn alive(&self) -> bool {
        let tunnel_up = self.tunnel.borrow_mut().as_mut().is_none_or(|c| matches!(c.try_wait(), Ok(None)));
        tunnel_up && self.status().is_ok()
    }

}

fn json_of(resp: ureq::Response) -> Result<serde_json::Value, String> {
    let body = resp.into_string().map_err(|e| format!("回执读不出来：{e}"))?;
    serde_json::from_str(&body).map_err(|e| format!("回执不是 JSON：{e}"))
}

fn doc_of(v: &serde_json::Value) -> Result<Doc, String> {
    let uuid = v["uuid"].as_str().filter(|u| !u.is_empty()).ok_or("导入接口回执里没有 uuid")?.to_string();
    Ok(Doc { uuid, name: v["name"].as_str().unwrap_or_default().to_string() })
}

/// 出错时尽量带上服务端给的说明（回执 `{"message": …}` 或正文）。
fn http_err(e: ureq::Error) -> String {
    match e {
        ureq::Error::Status(code, resp) => {
            let body = resp.into_string().unwrap_or_default();
            let msg = serde_json::from_str::<serde_json::Value>(&body).ok().and_then(|v| v["message"].as_str().or(v["error"].as_str()).map(str::to_string)).unwrap_or(body);
            format!("HTTP {code} {}", msg.trim())
        }
        ureq::Error::Transport(t) => t.to_string(),
    }
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
    busy: std::collections::HashMap<String, usize>,
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

    /// 交给这台设备的传输线程（第一次用时起）。不等：先用 [`Pipeline::has_room`] 看有没有空。
    pub fn submit(&mut self, t: Transfer) {
        *self.busy.entry(t.device.clone()).or_default() += 1;
        let (tx, _) = self.lanes.entry(t.device.clone()).or_insert_with_key(|_| {
            let (tx, rx) = std::sync::mpsc::channel();
            let done = self.done_tx.clone();
            let h = if t.is_move() { std::thread::spawn(move || move_lane(rx, done)) } else { std::thread::spawn(move || file_lane(rx, done)) };
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
                if let Event::Done(t, _) = &ev {
                    if let Some(n) = self.busy.get_mut(&t.device) {
                        *n -= 1;
                    }
                }
                return Some(ev);
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

    /// 手上还有活、线程却已经退出的设备：去掉这条（下次交给它时重新起），返回 [`Event::Lost`]。
    /// 线程退出前交回的结果都已经在队列里（先看队列、空了才来这里），所以剩下的件数就是丢了的。
    fn dead_lane(&mut self) -> Option<Event> {
        let device = self.lanes.iter().find(|(d, (_, h))| h.is_finished() && self.busy.get(*d).is_some_and(|n| *n > 0)).map(|(d, _)| d.clone())?;
        // 线程退出和它最后交回的结果之间没有先后保证：队列里还有的话先交那个
        if let Ok(ev) = self.done_rx.try_recv() {
            if let Event::Done(t, _) = &ev {
                if let Some(n) = self.busy.get_mut(&t.device) {
                    *n -= 1;
                }
            }
            return Some(ev);
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

/// Move：交出去一件，等它在 Move 上做完（书架服务在后台加入 xochitl、等排版）再交下一件——Move 上排队的书每本都占一份
/// 完整的空间，不一下子全推上去。
fn move_lane(rx: std::sync::mpsc::Receiver<Transfer>, done: std::sync::mpsc::Sender<Event>) {
    // 交出去在 Move 上做的：(那件, 任务号, 交出的时刻, 上次报的阶段, 上次报的时刻)
    let mut pending: Vec<(Transfer, String, Instant, String, Option<Instant>)> = Vec::new();
    let mut open = true;
    while open || !pending.is_empty() {
        let next = if open && pending.is_empty() { rx.recv().map_err(|_| open = false).ok() } else { None };
        let got = next.is_some();
        if let Some(t) = next {
            let at = Instant::now();
            match guarded("交给 Move", || t.submit()) {
                Ok(Submitted::Done(d)) => {
                    let _ = done.send(Event::Done(Box::new(t), Ok(Done::Move(d))));
                }
                Ok(Submitted::Job(job)) => {
                    t.release_src();
                    pending.push((t, job, at, String::new(), None));
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
        for (t, job, at, last_stage, last_emit) in pending.drain(..) {
            let state = match &t.work {
                Work::Move { client, .. } => guarded("查 Move 上的导入任务", || client.poll(&job)),
                Work::File { .. } => Err("内部错误：文件交给了 Move 的传输线程".into()),
            };
            match state {
                Ok(JobState::Running(stage)) => {
                    // 做了 30 秒以上：阶段变了马上报，没变每分钟报一次
                    let due = at.elapsed() >= PROGRESS_AFTER && (stage != last_stage || last_emit.is_none_or(|e| e.elapsed() >= PROGRESS_EVERY));
                    let emitted = if due {
                        let what = if stage.is_empty() { "在做" } else { stage.as_str() };
                        let _ = done.send(Event::Progress(format!("… [{}] {}：Move 上{what}（已 {}）", t.device, t.title, elapsed(at.elapsed()))));
                        Some(Instant::now())
                    } else {
                        last_emit
                    };
                    still.push((t, job, at, if due { stage } else { last_stage }, emitted));
                }
                Ok(JobState::Done(d)) => {
                    let _ = done.send(Event::Done(Box::new(t), Ok(Done::Move(d))));
                }
                Ok(JobState::Failed(m)) => {
                    let _ = done.send(Event::Done(Box::new(t), Err(format!("Move 上加入失败：{m}"))));
                }
                Err(e) => {
                    let _ = done.send(Event::Done(Box::new(t), Err(e)));
                }
            }
        }
        pending = still;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn storage_prefers_internal() {
        let d = tempfile::tempdir().unwrap();
        assert!(mtp_storage(&d.path().join("没有")).is_none());
        assert!(mtp_storage(d.path()).is_none(), "空的挂载点（没挂上）不算接上");
        std::fs::create_dir(d.path().join("SD 卡")).unwrap();
        assert_eq!(mtp_storage(d.path()).unwrap(), d.path().join("SD 卡"));
        std::fs::create_dir(d.path().join("Internal Storage")).unwrap();
        assert_eq!(mtp_storage(d.path()).unwrap(), d.path().join("Internal Storage"));
    }

    #[test]
    fn put_file_skips_identical_and_replaces_different() {
        let d = tempfile::tempdir().unwrap();
        let (src, dest) = (d.path().join("src"), d.path().join("dest"));
        std::fs::write(&src, b"abc").unwrap();
        assert!(put_file(&src, &dest, true, true, false).unwrap(), "不在：写");
        assert!(!put_file(&src, &dest, true, true, false).unwrap(), "相同：不写");
        assert!(put_file(&src, &dest, true, false, false).unwrap(), "不比较：照写");
        std::fs::write(&src, b"abd").unwrap();
        assert!(put_file(&src, &dest, false, true, false).unwrap());
        assert_eq!(std::fs::read(&dest).unwrap(), b"abd");
        assert!(!src.exists(), "不留来源时挪走");
        assert!(!same_bytes(&dest, &d.path().join("x")));
        // 直接写：换掉旧的、不留临时文件
        std::fs::write(&src, b"xyz1").unwrap();
        assert!(put_file(&src, &dest, true, false, true).unwrap());
        assert_eq!(std::fs::read(&dest).unwrap(), b"xyz1");
        assert_eq!(std::fs::read_dir(d.path()).unwrap().count(), 2, "只有 src、dest");
    }

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

    #[test]
    fn query_encoding() {
        assert_eq!(enc("a b/书.epub"), "a%20b%2F%E4%B9%A6.epub");
    }
}
