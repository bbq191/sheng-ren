//! 集成测试共用：造测试书、跑 booklib 命令。
#![allow(dead_code)] // 每个测试文件只用到其中一部分

use bookconv::epub::{assemble, Book, BookMeta, Chapter};
use library::{DeviceEnv, Library};
use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::Output;
use std::sync::{Arc, Mutex};

/// 测试用的"设备"目录（`<书库>-dev`）：MTP 挂载目录，下面 kindle、ireader 各有一个存储（算接上了）。
/// 测试一律不碰真设备：MTP 指向这里，Move 不走 SSH（要测 Move 的用 [`FakeMove`]）。
pub fn dev_dir(lib: &Path) -> PathBuf {
    let d = PathBuf::from(format!("{}-dev", lib.display()));
    for m in ["kindle", "ireader"] {
        std::fs::create_dir_all(d.join(m).join("Internal Storage")).unwrap();
    }
    d
}

/// 设备上 `mode`（kindle/ireader）的产物根目录：`<存储>/documents`。
pub fn documents(lib: &Path, mode: &str) -> PathBuf {
    dev_dir(lib).join(mode).join("Internal Storage/documents")
}

/// 打开书库，设备指向测试目录（见 [`dev_dir`]）；`mv` 给了就把 Move 指向这个假服务。
pub fn open_with(lib: &Path, mv: Option<&FakeMove>) -> Library {
    let mut l = Library::open(lib).unwrap();
    l.set_device_env(DeviceEnv { mtp_base: dev_dir(lib), xochitl_url: mv.map(|m| m.url.clone()), no_ssh: true });
    l
}

/// 在书库 `profiles/` 里放一个产物留在电脑上的自定义模式 `pc`（掌阅的参数，去掉 `[deliver]`）。返回书库路径。
pub fn with_pc_profile(lib: &Path) -> PathBuf {
    let src = std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("../profile/profiles/ireader.toml")).unwrap();
    let toml = &src[..src.find("[deliver]").expect("ireader.toml 有 [deliver]")];
    std::fs::create_dir_all(lib.join("profiles")).unwrap();
    std::fs::write(lib.join("profiles/pc.toml"), toml).unwrap();
    lib.to_path_buf()
}

pub fn open(lib: &Path) -> Library {
    open_with(lib, None)
}

/// 假的 Move 书架服务（只实现 booklib 用到的导入接口）：uuid → 书。
#[derive(Clone, Debug)]
pub struct MoveDoc {
    pub name: String,
    pub folder: String,
    pub bytes: Vec<u8>,
    pub deleted: bool,
    /// 被原地替换过几次。
    pub replaced: usize,
}

pub struct FakeMove {
    pub url: String,
    pub docs: Arc<Mutex<BTreeMap<String, MoveDoc>>>,
}

impl FakeMove {
    pub fn start() -> FakeMove {
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", l.local_addr().unwrap());
        let docs: Arc<Mutex<BTreeMap<String, MoveDoc>>> = Arc::default();
        let d = docs.clone();
        std::thread::spawn(move || {
            for s in l.incoming().flatten() {
                let _ = serve(s, &d);
            }
        });
        FakeMove { url, docs }
    }

    /// 没删的书：(文件夹, 显示名)。
    pub fn live(&self) -> Vec<(String, String)> {
        self.docs.lock().unwrap().values().filter(|d| !d.deleted).map(|d| (d.folder.clone(), d.name.clone())).collect()
    }
}

fn serve(s: std::net::TcpStream, docs: &Mutex<BTreeMap<String, MoveDoc>>) -> std::io::Result<()> {
    let mut r = BufReader::new(s.try_clone()?);
    let mut line = String::new();
    r.read_line(&mut line)?;
    let mut parts = line.split_whitespace();
    let (method, target) = (parts.next().unwrap_or("").to_string(), parts.next().unwrap_or("").to_string());
    let mut len = 0usize;
    loop {
        let mut h = String::new();
        r.read_line(&mut h)?;
        if h.trim().is_empty() {
            break;
        }
        if let Some((k, v)) = h.split_once(':') {
            if k.eq_ignore_ascii_case("content-length") {
                len = v.trim().parse().unwrap_or(0);
            }
        }
    }
    let mut body = vec![0; len];
    r.read_exact(&mut body)?;
    let (path, query) = target.split_once('?').unwrap_or((&target, ""));
    let q: BTreeMap<String, String> = query.split('&').filter_map(|kv| kv.split_once('=')).map(|(k, v)| (k.to_string(), unpct(v))).collect();
    let mut docs = docs.lock().unwrap();
    let reply = |code: u16, v: serde_json::Value| (code, v.to_string());
    let (code, out) = match (method.as_str(), path) {
        ("GET", "/status") => reply(200, serde_json::json!({})),
        ("POST", "/import") => match q.get("uuid") {
            Some(u) => match docs.get_mut(u).filter(|d| !d.deleted) {
                Some(d) => {
                    d.bytes = body;
                    d.replaced += 1;
                    reply(200, serde_json::json!({"uuid": u, "name": d.name, "folder": d.folder}))
                }
                None => reply(404, serde_json::json!({"message": "没有这本"})),
            },
            None => {
                let uuid = format!("00000000-0000-0000-0000-{:012}", docs.len() + 1);
                let name = q.get("name").cloned().unwrap_or_default().trim_end_matches(".epub").to_string();
                let folder = q.get("folder").cloned().unwrap_or_default();
                docs.insert(uuid.clone(), MoveDoc { name: name.clone(), folder: folder.clone(), bytes: body, deleted: false, replaced: 0 });
                reply(200, serde_json::json!({"uuid": uuid, "name": name, "folder": folder}))
            }
        },
        ("GET", p) if p.starts_with("/import/") => match docs.get(&p["/import/".len()..]) {
            Some(d) => reply(200, serde_json::json!({"deleted": d.deleted, "name": d.name})),
            None => reply(404, serde_json::json!({})),
        },
        ("POST", "/trash/add") => {
            let v: serde_json::Value = serde_json::from_slice(&body).unwrap_or_default();
            match docs.get_mut(v["uuid"].as_str().unwrap_or("")).filter(|d| Some(d.name.as_str()) == v["name"].as_str()) {
                Some(d) => {
                    d.deleted = true;
                    reply(200, serde_json::json!({"ok": true}))
                }
                None => reply(400, serde_json::json!({"message": "名字对不上"})),
            }
        }
        _ => reply(404, serde_json::json!({})),
    };
    let mut s = s;
    write!(s, "HTTP/1.1 {code} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{out}", out.len())?;
    s.flush()
}

fn unpct(v: &str) -> String {
    let b = v.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            out.push(u8::from_str_radix(&v[i + 1..i + 3], 16).unwrap_or(b'?'));
            i += 3;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

pub fn sample_epub(title: &str) -> Vec<u8> {
    sample_epub_with(title, "")
}

/// 书名相同、正文不同的书（`marker` 写进第一段开头）：同名同作者的另一个版本。
pub fn sample_epub_with(title: &str, marker: &str) -> Vec<u8> {
    let long = format!("{marker}{}", "正文段落，足够长的文字内容，确保标题页之后的内容超过门槛。".repeat(5));
    let mut book = Book {
        meta: BookMeta { book_id: "t".into(), title: title.into(), author: "作者".into(), language: "zh".into(), publisher: "".into(), cover: None, cover_ext: "jpg".into(), cover_media_type: "image/jpeg".into(), subjects: Vec::new() },
        chapters: vec![Chapter { title: "第一章".into(), html_body: format!("<h1>第一章</h1><p>{long}</p><h2>第一节</h2><p>{long}</p>"), level: 1 }],
        resources: vec![],
        nav: vec![],
    };
    assemble(&mut book).unwrap()
}

pub fn jpeg(w: u32, h: u32) -> Vec<u8> {
    let mut out = Vec::new();
    image::codecs::jpeg::JpegEncoder::new(&mut out).encode_image(&image::RgbImage::from_pixel(w, h, image::Rgb([90, 90, 90]))).unwrap();
    out
}

/// 在 `lib` 书库上跑 `booklib <args>`（`lib` 为 `None` 时不加 `--library=`）。
pub fn booklib(lib: Option<&Path>, args: &[&str]) -> Output {
    let mut cmd = std::process::Command::new(env!("CARGO_BIN_EXE_booklib"));
    cmd.env("BOOKLIB_NO_SSH", "1").env_remove("BOOKLIB_XOCHITL_URL");
    if let Some(lib) = lib {
        cmd.arg(format!("--library={}", lib.display()));
        // 书库还没建（命令本身不该建出东西）时不建设备目录
        let dev = if lib.exists() { dev_dir(lib) } else { PathBuf::from(format!("{}-dev", lib.display())) };
        cmd.env("BOOKLIB_MTP_DIR", dev);
    } else {
        cmd.env("BOOKLIB_MTP_DIR", "/nonexistent-booklib-test");
    }
    cmd.args(args).output().unwrap()
}

pub fn stdout(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}

pub fn stderr(o: &Output) -> String {
    String::from_utf8_lossy(&o.stderr).into_owned()
}
