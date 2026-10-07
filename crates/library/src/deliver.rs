//! 产物送到设备（2026-10-07 用户定：`sync` 按设备接没接上直接传，电脑上不留产物）。怎么送由 profile 的 `[deliver]` 定：
//!
//! - **MTP**（Kindle、掌阅）：jmtpfs 自动挂载在 `<挂载目录>/<mount>`（缺省 `$XDG_RUNTIME_DIR/mtp`），下面是存储
//!   （「Internal Storage」之类），产物放进存储根目录的 `<dir>/`，按普通文件写。挂载点在、里面有存储就算接上了。
//! - **xochitl**（Move）：经 SSH 端口转发调设备上书架服务的导入接口：`POST /import?name=&folder=` 新加入（回 uuid），
//!   `POST /import?uuid=&name=` 原地替换（保留 uuid、阅读进度、所在文件夹；uuid 不在了回 404），`GET /import/<uuid>` 查还在不在，
//!   删除走它的回收站队列 `POST /trash/add {uuid, name}`（进 xochitl 回收站，能恢复）。
//! - 没写 `[deliver]` 的模式（书库 `profiles/` 里的自定义模式）：产物照旧放在电脑上。
//!
//! 一轮 `sync` 里每台设备只连一次（[`crate::Library::refresh_devices`] 清掉重连，`--watch` 每轮清一次）。

use profile::{Deliver, Profile};
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

/// 把本地文件 `src` 放到 `dest`（同一文件系统直接改名、`keep_src` 时或跨文件系统时复制）。`dest` 已经是逐字节相同的文件
/// 就不动它（Kindle 覆盖成不同字节会清掉阅读进度，相同的不会；也省得在 MTP 上白拷）。复制先写旁边的临时文件再换上去：
/// MTP 上改名不能覆盖已有文件，先删旧的再改名。返回有没有真的写。
pub(crate) fn put_file(src: &Path, dest: &Path, keep_src: bool) -> Result<bool, String> {
    if same_bytes(src, dest) {
        if !keep_src {
            let _ = std::fs::remove_file(src);
        }
        return Ok(false);
    }
    if !keep_src && std::fs::rename(src, dest).is_ok() {
        let _ = crate::fsutil::sync_parent(dest);
        return Ok(true);
    }
    let tmp = crate::fsutil::tmp_sibling(dest);
    let copy = || -> std::io::Result<()> {
        let mut r = std::fs::File::open(src)?;
        let mut w = std::fs::File::create(&tmp)?;
        std::io::copy(&mut r, &mut w)?;
        w.sync_all()
    };
    if let Err(e) = copy() {
        let _ = std::fs::remove_file(&tmp);
        return Err(format!("写 {}: {e}", dest.display()));
    }
    let _ = std::fs::remove_file(dest);
    if let Err(e) = std::fs::rename(&tmp, dest) {
        let _ = std::fs::remove_file(&tmp);
        return Err(format!("改名成 {}: {e}", dest.display()));
    }
    let _ = crate::fsutil::sync_parent(dest);
    if !keep_src {
        let _ = std::fs::remove_file(src);
    }
    Ok(true)
}

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
pub(crate) struct Doc {
    pub uuid: String,
    /// xochitl 书库里显示的名字（`.metadata` 的 `visibleName`；回收站接口要核对它）。
    pub name: String,
}

/// 到 Move 上书架服务的连接：SSH 端口转发（[`Xochitl::connect`]）或直接给的地址（测试）。
pub(crate) struct Xochitl {
    base: String,
    agent: ureq::Agent,
    tunnel: Option<Child>,
    /// 连的是哪台（提示用）。
    pub host: String,
}

impl Drop for Xochitl {
    fn drop(&mut self) {
        if let Some(c) = self.tunnel.as_mut() {
            let _ = c.kill();
            let _ = c.wait();
        }
    }
}

/// 大书经 USB 也要传一阵、第一次导入时设备上还要排版：读写超时放宽。
const IO_TIMEOUT: Duration = Duration::from_secs(600);

impl Xochitl {
    fn agent() -> ureq::Agent {
        ureq::AgentBuilder::new().timeout_connect(Duration::from_secs(5)).timeout_read(IO_TIMEOUT).timeout_write(IO_TIMEOUT).build()
    }

    fn direct(url: &str) -> Result<Xochitl, String> {
        let x = Xochitl { base: url.to_string(), agent: Self::agent(), tunnel: None, host: url.to_string() };
        x.status()?;
        Ok(x)
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
        let mut x = Xochitl { base: format!("http://127.0.0.1:{local}"), agent: Self::agent(), tunnel: Some(child), host: host.to_string() };
        let deadline = Instant::now() + Duration::from_secs(8);
        loop {
            if let Some(st) = x.tunnel.as_mut().and_then(|c| c.try_wait().ok().flatten()) {
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

    fn status(&self) -> Result<(), String> {
        self.agent.get(&format!("{}/status", self.base)).timeout(Duration::from_secs(5)).call().map(|_| ()).map_err(|e| format!("书架服务没答话（{}）", http_err(e)))
    }

    /// 新加入 xochitl：放进文件夹 `folder`（空 = 书库根；没有就让设备建）。
    pub(crate) fn import(&self, file: &Path, name: &str, folder: &str) -> Result<Doc, String> {
        let url = format!("{}/import?name={}&folder={}", self.base, enc(name), enc(folder));
        self.send(&url, file)?.ok_or_else(|| "导入接口回 404（设备上的书架服务太旧，没有导入接口）".into())
    }

    /// 原地替换 `uuid` 的内容（保留 uuid、进度、文件夹）。这本在设备上已经不在了（删了、进了回收站）返回 `None`。
    pub(crate) fn replace(&self, uuid: &str, file: &Path, name: &str) -> Result<Option<Doc>, String> {
        let url = format!("{}/import?uuid={}&name={}", self.base, enc(uuid), enc(name));
        self.send(&url, file)
    }

    fn send(&self, url: &str, file: &Path) -> Result<Option<Doc>, String> {
        let f = std::fs::File::open(file).map_err(|e| format!("读 {}: {e}", file.display()))?;
        let len = f.metadata().map_err(|e| e.to_string())?.len();
        let r = self.agent.post(url).set("Content-Type", "application/epub+zip").set("Content-Length", &len.to_string()).send(std::io::BufReader::new(f));
        match r {
            Ok(resp) => doc_of(resp).map(Some),
            Err(ureq::Error::Status(404, _)) => Ok(None),
            Err(e) => Err(format!("传到 Move 失败：{}", http_err(e))),
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

fn json_of(resp: ureq::Response) -> Result<serde_json::Value, String> {
    let body = resp.into_string().map_err(|e| format!("回执读不出来：{e}"))?;
    serde_json::from_str(&body).map_err(|e| format!("回执不是 JSON：{e}"))
}

fn doc_of(resp: ureq::Response) -> Result<Doc, String> {
    let v = json_of(resp)?;
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

/// 查询参数编码（百分号编码 UTF-8 字节，只留非保留字符）。
fn enc(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
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
        assert!(put_file(&src, &dest, true).unwrap(), "不在：写");
        assert!(!put_file(&src, &dest, true).unwrap(), "相同：不写");
        std::fs::write(&src, b"abd").unwrap();
        assert!(put_file(&src, &dest, false).unwrap());
        assert_eq!(std::fs::read(&dest).unwrap(), b"abd");
        assert!(!src.exists(), "不留来源时挪走");
        assert!(!same_bytes(&dest, &d.path().join("x")));
    }

    #[test]
    fn query_encoding() {
        assert_eq!(enc("a b/书.epub"), "a%20b%2F%E4%B9%A6.epub");
    }
}
