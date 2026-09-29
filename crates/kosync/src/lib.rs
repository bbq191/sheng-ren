//! KOReader 阅读进度同步服务器（kosync 协议）。KOReader 的「进度同步」插件（plugins/kosync.koplugin）按 `api.json` 调四个接口：
//!
//! | 接口 | 请求 | 成功 | 失败 |
//! |---|---|---|---|
//! | 注册 | `POST /users/create`，正文 `{username, password}`（password 是密码的 md5） | 201 | 402 + `{message}` |
//! | 登录校验 | `GET /users/auth`，头 `x-auth-user`、`x-auth-key`（密码的 md5） | 200 `{authorized:"OK"}` | 401 |
//! | 上传进度 | `PUT /syncs/progress`，正文 `{document, progress, percentage, device, device_id}` | 200 `{document, timestamp}` | 401 |
//! | 读进度 | `GET /syncs/progress/<document>` | 200 进度（没有就是 `{}`） | 401 |
//!
//! 客户端读回进度时用 `percentage`、`progress`（xpointer 或页码）、`device`、`device_id`、`timestamp`（秒，和它自己最后翻页的时间比新旧），
//! 出错时显示 `message`。`document` 是书的标识（按文件名或文件内容的 md5，客户端设置里选）。
//!
//! 本实现（2026-09-29，按 KOReader v2026.07 的 kosync 插件源码核对）：
//! - **不开放注册**：`/users/create` 一律 402「注册已关闭」，账号在服务器上用 `kosync useradd` 建（用户定）。
//! - 客户端发来的密钥（密码的 md5）不原样存：存随机盐 + `sha256(盐:密钥)`，比较用定长比较。
//! - 数据是 `<数据目录>/users.json`、`progress.json` 两个文件，每次改动先写临时文件、落盘、再改名（断电不会写坏）。
//!   两台设备、几百本书的量，整份读进内存足够。
//! - 这里只有纯逻辑（[`Store`]、[`handle`]），网络在 `http.rs`，命令行在 `main.rs`。
pub mod http;

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};

/// 一个账号：盐与 `sha256(盐:密钥)`（十六进制）。
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct User {
    pub salt: String,
    pub hash: String,
}

/// 一本书的进度（客户端上传什么就存什么，外加服务器收到的时间）。
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Progress {
    pub progress: String,
    pub percentage: f64,
    pub device: String,
    pub device_id: String,
    /// 收到时的 Unix 时间（秒）。
    pub timestamp: u64,
}

/// 账号与进度，落在数据目录的两个 JSON 文件里。
#[derive(Debug)]
pub struct Store {
    dir: PathBuf,
    users: BTreeMap<String, User>,
    /// 用户名 → 书的标识 → 进度
    progress: BTreeMap<String, BTreeMap<String, Progress>>,
}

const USERS: &str = "users.json";
const PROGRESS: &str = "progress.json";
/// 用户名、书的标识、进度串、设备名的长度上限（字节）：正常值都远小于它，防有人塞大字符串占内存和磁盘。
const MAX_FIELD: usize = 512;

impl Store {
    /// 打开数据目录（没有就建），读入两个文件（没有就是空的）。
    pub fn open(dir: &Path) -> Result<Store, String> {
        std::fs::create_dir_all(dir).map_err(|e| format!("建数据目录 {}: {e}", dir.display()))?;
        Ok(Store { dir: dir.to_path_buf(), users: read_json(&dir.join(USERS))?, progress: read_json(&dir.join(PROGRESS))? })
    }

    pub fn users(&self) -> impl Iterator<Item = &str> {
        self.users.keys().map(String::as_str)
    }

    /// 建账号或改密码（`password` 是明文，按 KOReader 的做法先取 md5 当密钥）。
    pub fn set_user(&mut self, name: &str, password: &str) -> Result<(), String> {
        if name.is_empty() || name.len() > MAX_FIELD || name.chars().any(|c| c.is_control()) {
            return Err("用户名不能为空、不能有控制字符".into());
        }
        let key = { use md5::Digest; hex(&md5::Md5::digest(password.as_bytes())) };
        let salt = hex(&random_bytes::<16>()?);
        let hash = key_hash(&salt, &key);
        self.users.insert(name.to_string(), User { salt, hash });
        write_json(&self.dir.join(USERS), &self.users)
    }

    /// 删账号和它的全部进度。返回有没有这个账号。
    pub fn remove_user(&mut self, name: &str) -> Result<bool, String> {
        let had = self.users.remove(name).is_some();
        if had {
            write_json(&self.dir.join(USERS), &self.users)?;
            if self.progress.remove(name).is_some() {
                write_json(&self.dir.join(PROGRESS), &self.progress)?;
            }
        }
        Ok(had)
    }

    /// 用户名和密钥（密码的 md5）对不对。
    pub fn check(&self, name: &str, key: &str) -> bool {
        match self.users.get(name) {
            Some(u) => ct_eq(key_hash(&u.salt, &key.to_ascii_lowercase()).as_bytes(), u.hash.as_bytes()),
            None => {
                // 没这个人也算一遍，让"用户名不存在"和"密码错"耗时一样
                let _ = key_hash("0000000000000000", key);
                false
            }
        }
    }

    fn put_progress(&mut self, user: &str, doc: &str, p: Progress) -> Result<(), String> {
        self.progress.entry(user.to_string()).or_default().insert(doc.to_string(), p);
        write_json(&self.dir.join(PROGRESS), &self.progress)
    }

    fn get_progress(&self, user: &str, doc: &str) -> Option<&Progress> {
        self.progress.get(user)?.get(doc)
    }
}

/// 一个请求（`http.rs` 解析好的）。
#[derive(Debug, Default)]
pub struct Request {
    pub method: String,
    /// 路径（不含查询串），已百分号解码。
    pub path: String,
    /// 头名一律小写。
    pub headers: BTreeMap<String, String>,
    pub body: Vec<u8>,
}

/// 回给客户端的：状态码与 JSON 正文。
#[derive(Debug, PartialEq)]
pub struct Response {
    pub status: u16,
    pub body: serde_json::Value,
}

fn reply(status: u16, body: serde_json::Value) -> Response {
    Response { status, body }
}

fn message(status: u16, text: &str) -> Response {
    reply(status, serde_json::json!({ "message": text }))
}

/// 处理一个请求。`now` 是当前 Unix 秒（测试里固定）。
pub fn handle(store: &mut Store, req: &Request, now: u64) -> Response {
    use serde_json::json;
    let authed = || -> Option<String> {
        let (Some(user), Some(key)) = (req.headers.get("x-auth-user"), req.headers.get("x-auth-key")) else {
            eprintln!("认证失败：请求没带 x-auth-user / x-auth-key");
            return None;
        };
        if store.check(user, key) {
            return Some(user.clone());
        }
        // 只记用户名和账号在不在（不记密钥），排查"用户名对不上"还是"密码对不上"用；进 journalctl -u kosync
        let exists = store.users.contains_key(user.as_str());
        eprintln!("认证失败：用户 {user:?}（{}）", if exists { "账号存在，密码不对" } else { "没有这个账号" });
        None
    };
    match (req.method.as_str(), req.path.as_str()) {
        ("GET", "/healthcheck") => reply(200, json!({ "state": "OK" })),
        ("POST", "/users/create") => message(402, "注册已关闭：账号请在服务器上用 kosync useradd 建"),
        ("GET", "/users/auth") => match authed() {
            Some(_) => reply(200, json!({ "authorized": "OK" })),
            None => message(401, "Unauthorized"),
        },
        ("PUT", "/syncs/progress") => {
            let Some(user) = authed() else { return message(401, "Unauthorized") };
            #[derive(Deserialize)]
            struct Up {
                document: String,
                progress: serde_json::Value,
                percentage: f64,
                device: String,
                device_id: String,
            }
            let Ok(up) = serde_json::from_slice::<Up>(&req.body) else { return message(400, "请求正文不对：要 document、progress、percentage、device、device_id") };
            // progress 在翻页式文档里是页码（数字），在流式文档里是 xpointer（字符串）：原样转成字符串存，读回时照样给
            let progress = match up.progress {
                serde_json::Value::String(s) => s,
                v => v.to_string(),
            };
            if [&up.document, &progress, &up.device, &up.device_id].iter().any(|s| s.len() > MAX_FIELD) || up.document.is_empty() || !up.percentage.is_finite() {
                return message(400, "字段太长或不合法");
            }
            let p = Progress { progress, percentage: up.percentage, device: up.device, device_id: up.device_id, timestamp: now };
            match store.put_progress(&user, &up.document, p) {
                Ok(()) => reply(200, json!({ "document": up.document, "timestamp": now })),
                Err(e) => message(500, &format!("存进度失败：{e}")),
            }
        }
        ("GET", p) if p.starts_with("/syncs/progress/") => {
            let Some(user) = authed() else { return message(401, "Unauthorized") };
            let doc = &p["/syncs/progress/".len()..];
            match store.get_progress(&user, doc) {
                Some(pr) => reply(
                    200,
                    json!({
                        "document": doc,
                        "progress": pr.progress,
                        "percentage": pr.percentage,
                        "device": pr.device,
                        "device_id": pr.device_id,
                        "timestamp": pr.timestamp,
                    }),
                ),
                None => reply(200, json!({})),
            }
        }
        _ => message(404, "Not found"),
    }
}

fn key_hash(salt: &str, key: &str) -> String {
    use sha2::Digest;
    let mut h = sha2::Sha256::new();
    h.update(salt.as_bytes());
    h.update(b":");
    h.update(key.as_bytes());
    hex(&h.finalize())
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// 定长比较（不因为前面几个字节不同就提前返回）。
fn ct_eq(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

fn random_bytes<const N: usize>() -> Result<[u8; N], String> {
    use std::io::Read;
    let mut buf = [0u8; N];
    std::fs::File::open("/dev/urandom").and_then(|mut f| f.read_exact(&mut buf)).map_err(|e| format!("读 /dev/urandom: {e}"))?;
    Ok(buf)
}

fn read_json<T: serde::de::DeserializeOwned + Default>(path: &Path) -> Result<T, String> {
    match std::fs::read(path) {
        Ok(b) => serde_json::from_slice(&b).map_err(|e| format!("{} 不是合法的 JSON：{e}", path.display())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(T::default()),
        Err(e) => Err(format!("读 {}: {e}", path.display())),
    }
}

/// 原子写：同目录临时文件 → 落盘 → 改名 → 目录落盘。
fn write_json<T: Serialize>(path: &Path, value: &T) -> Result<(), String> {
    let tmp = path.with_extension("json.tmp");
    let data = serde_json::to_vec_pretty(value).map_err(|e| e.to_string())?;
    let write = || -> std::io::Result<()> {
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(&data)?;
        f.sync_all()?;
        std::fs::rename(&tmp, path)?;
        if let Some(dir) = path.parent() {
            std::fs::File::open(dir)?.sync_all()?;
        }
        Ok(())
    };
    write().map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        format!("写 {}: {e}", path.display())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn tmpdir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("kosync-test-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    fn req(method: &str, path: &str, auth: Option<(&str, &str)>, body: serde_json::Value) -> Request {
        let mut headers = BTreeMap::new();
        if let Some((u, k)) = auth {
            headers.insert("x-auth-user".into(), u.into());
            headers.insert("x-auth-key".into(), k.into());
        }
        Request { method: method.into(), path: path.into(), headers, body: if body.is_null() { vec![] } else { body.to_string().into_bytes() } }
    }

    // "secret" 的 md5（KOReader 发的就是这个）
    const KEY: &str = "5ebe2294ecd0e0f08eab7690d2a6ee69";

    #[test]
    fn register_closed_login_and_progress_roundtrip() {
        let dir = tmpdir("flow");
        let mut s = Store::open(&dir).unwrap();
        s.set_user("afu", "secret").unwrap();
        assert_eq!(handle(&mut s, &req("POST", "/users/create", None, json!({"username":"x","password":"y"})), 1).status, 402);
        assert_eq!(handle(&mut s, &req("GET", "/users/auth", Some(("afu", KEY)), json!(null)), 1).status, 200);
        assert_eq!(handle(&mut s, &req("GET", "/users/auth", Some(("afu", &KEY.to_uppercase())), json!(null)), 1).status, 200, "密钥大小写不敏感");
        assert_eq!(handle(&mut s, &req("GET", "/users/auth", Some(("afu", "wrong")), json!(null)), 1).status, 401);
        assert_eq!(handle(&mut s, &req("GET", "/users/auth", Some(("nobody", KEY)), json!(null)), 1).status, 401);
        assert_eq!(handle(&mut s, &req("GET", "/users/auth", None, json!(null)), 1).status, 401);

        let up = json!({"document":"abc","progress":"/body/DocFragment[3]/body/p[2]/text().0","percentage":0.25,"device":"Kindle","device_id":"K1"});
        let r = handle(&mut s, &req("PUT", "/syncs/progress", Some(("afu", KEY)), up), 1000);
        assert_eq!(r, Response { status: 200, body: json!({"document":"abc","timestamp":1000}) });
        assert_eq!(handle(&mut s, &req("PUT", "/syncs/progress", Some(("afu", "wrong")), json!({})), 1).status, 401);

        // 重开（从文件读回）后还在
        let mut s = Store::open(&dir).unwrap();
        let r = handle(&mut s, &req("GET", "/syncs/progress/abc", Some(("afu", KEY)), json!(null)), 2000);
        assert_eq!(r.status, 200);
        assert_eq!(r.body["progress"], "/body/DocFragment[3]/body/p[2]/text().0");
        assert_eq!(r.body["percentage"], 0.25);
        assert_eq!(r.body["device_id"], "K1");
        assert_eq!(r.body["timestamp"], 1000);
        assert_eq!(handle(&mut s, &req("GET", "/syncs/progress/none", Some(("afu", KEY)), json!(null)), 1).body, json!({}));
        assert_eq!(handle(&mut s, &req("GET", "/syncs/progress/abc", None, json!(null)), 1).status, 401);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn page_number_progress_and_bad_bodies() {
        let dir = tmpdir("bad");
        let mut s = Store::open(&dir).unwrap();
        s.set_user("afu", "secret").unwrap();
        let a = Some(("afu", KEY));
        let up = json!({"document":"d","progress":42,"percentage":0.5,"device":"D","device_id":"i"});
        assert_eq!(handle(&mut s, &req("PUT", "/syncs/progress", a, up), 5).status, 200);
        assert_eq!(handle(&mut s, &req("GET", "/syncs/progress/d", a, json!(null)), 5).body["progress"], "42", "页码按字符串存取");
        assert_eq!(handle(&mut s, &req("PUT", "/syncs/progress", a, json!({"document":"d"})), 5).status, 400);
        let long = "x".repeat(MAX_FIELD + 1);
        assert_eq!(handle(&mut s, &req("PUT", "/syncs/progress", a, json!({"document":long,"progress":"p","percentage":0.1,"device":"D","device_id":"i"})), 5).status, 400);
        assert_eq!(handle(&mut s, &req("GET", "/nope", a, json!(null)), 5).status, 404);
        assert!(s.remove_user("afu").unwrap());
        assert_eq!(handle(&mut s, &req("GET", "/users/auth", a, json!(null)), 5).status, 401);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn stored_hash_is_not_the_client_key() {
        let dir = tmpdir("hash");
        let mut s = Store::open(&dir).unwrap();
        s.set_user("afu", "secret").unwrap();
        let text = std::fs::read_to_string(dir.join(USERS)).unwrap();
        assert!(!text.contains(KEY) && !text.contains("secret"), "不能存明文或客户端密钥: {text}");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
