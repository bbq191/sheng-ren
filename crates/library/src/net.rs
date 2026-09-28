//! 联网查书目用的 HTTP：节流、重试、网址编码。封面和元数据共用。

use serde_json::Value;
use std::cell::{Cell, RefCell};
use std::io::Read;
use std::time::{Duration, Instant};

pub(crate) const UA: &str = "booklib/0.1 (personal e-book library tool)";

/// 最多试几次（429、5xx、超时这类临时错误才重试）。
const TRIES: usize = 4;

/// 带节流和重试的 HTTP：Wikidata 限速严（连续请求会 429），每个请求间隔至少 1.2 秒，429 时按 Retry-After 等。
///
/// - 4xx（除了 429）是"没有"，不重试；
/// - 连不上网（DNS 解析失败、连接失败）记成**离线**，之后的请求立即失败，不再每次重试等待；
/// - 临时错误（离线、429/5xx 重试完、超时等）记下来（[`Net::transient_error`]）：调用方据此区分"没找到"和"没查成"，
///   没查成的不能当"没有封面"去生成封面并存下来。
pub(crate) struct Net {
    agent: ureq::Agent,
    last: Cell<Option<Instant>>,
    offline: Cell<bool>,
    transient: RefCell<Option<String>>,
}

impl Net {
    pub(crate) fn new() -> Net {
        Net { agent: ureq::AgentBuilder::new().timeout(Duration::from_secs(30)).user_agent(UA).build(), last: Cell::new(None), offline: Cell::new(false), transient: RefCell::new(None) }
    }

    /// 连不上网（之后的请求都直接失败）。
    pub(crate) fn offline(&self) -> bool {
        self.offline.get()
    }

    /// 从上次 [`Net::clear_transient`] 以来遇到的第一个临时网络错误。
    pub(crate) fn transient_error(&self) -> Option<String> {
        self.transient.borrow().clone()
    }

    pub(crate) fn clear_transient(&self) {
        self.transient.borrow_mut().take();
    }

    fn note_transient(&self, e: &str) {
        self.transient.borrow_mut().get_or_insert_with(|| e.to_string());
    }

    pub(crate) fn fetch(&self, url: &str) -> Result<Vec<u8>, String> {
        self.fetch_ref(url, None)
    }

    /// `referer`：豆瓣图片服务器不带来源页会拒绝（HTTP 418）。
    pub(crate) fn fetch_ref(&self, url: &str, referer: Option<&str>) -> Result<Vec<u8>, String> {
        if self.offline.get() {
            return Err(format!("{url}: 连不上网，跳过"));
        }
        let mut last_err = String::new();
        for attempt in 1..=TRIES {
            if let Some(t) = self.last.get() {
                let gap = Duration::from_millis(1200);
                if t.elapsed() < gap {
                    std::thread::sleep(gap - t.elapsed());
                }
            }
            self.last.set(Some(Instant::now()));
            let mut req = self.agent.get(url);
            if let Some(r) = referer {
                req = req.set("Referer", r);
            }
            let wait = match req.call() {
                Ok(r) => {
                    let mut buf = Vec::new();
                    match r.into_reader().take(20 << 20).read_to_end(&mut buf) {
                        Ok(_) => return Ok(buf),
                        Err(e) => {
                            last_err = e.to_string();
                            2
                        }
                    }
                }
                Err(ureq::Error::Status(code, r)) if code == 429 || code >= 500 => {
                    last_err = format!("HTTP {code}");
                    r.header("Retry-After").and_then(|v| v.parse().ok()).unwrap_or(5u64).min(60)
                }
                Err(ureq::Error::Status(code, _)) => return Err(format!("{url}: HTTP {code}")),
                Err(e @ ureq::Error::Transport(_)) => match e.kind() {
                    ureq::ErrorKind::Dns | ureq::ErrorKind::ConnectionFailed => {
                        self.offline.set(true);
                        let e = format!("连不上网（{e}）");
                        self.note_transient(&e);
                        return Err(e);
                    }
                    ureq::ErrorKind::Io | ureq::ErrorKind::BadStatus | ureq::ErrorKind::BadHeader | ureq::ErrorKind::ProxyConnect => {
                        last_err = e.to_string();
                        2
                    }
                    _ => return Err(format!("{url}: {e}")), // 网址不对、重定向太多：重试也没用
                },
            };
            if attempt < TRIES {
                std::thread::sleep(Duration::from_secs(wait));
            }
        }
        let e = format!("{url}: {last_err}");
        self.note_transient(&e);
        Err(e)
    }

    pub(crate) fn json(&self, url: &str) -> Result<Value, String> {
        serde_json::from_slice(&self.fetch(url)?).map_err(|e| format!("{url}: {e}"))
    }
}

pub(crate) fn enc(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}
