//! 联网查书目用的 HTTP：节流、重试、网址编码。封面和元数据共用。

use serde_json::Value;
use std::cell::Cell;
use std::io::Read;
use std::time::{Duration, Instant};

pub(crate) const UA: &str = "booklib/0.1 (personal e-book library tool)";

/// 带节流和重试的 HTTP：Wikidata 限速严（连续请求会 429），每个请求间隔至少 1.2 秒，429 时按 Retry-After 等。
pub(crate) struct Net {
    agent: ureq::Agent,
    last: Cell<Option<Instant>>,
}

impl Net {
    pub(crate) fn new() -> Net {
        Net { agent: ureq::AgentBuilder::new().timeout(Duration::from_secs(30)).user_agent(UA).build(), last: Cell::new(None) }
    }

    pub(crate) fn fetch(&self, url: &str) -> Result<Vec<u8>, String> {
        self.fetch_ref(url, None)
    }

    /// `referer`：豆瓣图片服务器不带来源页会拒绝（HTTP 418）。
    pub(crate) fn fetch_ref(&self, url: &str, referer: Option<&str>) -> Result<Vec<u8>, String> {
        let mut last_err = String::new();
        for _ in 0..4 {
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
            match req.call() {
                Ok(r) => {
                    let mut buf = Vec::new();
                    r.into_reader().take(20 << 20).read_to_end(&mut buf).map_err(|e| e.to_string())?;
                    return Ok(buf);
                }
                Err(ureq::Error::Status(404, _)) => return Err("404".into()),
                Err(ureq::Error::Status(code, r)) if code == 429 || code >= 500 => {
                    let wait = r.header("Retry-After").and_then(|v| v.parse().ok()).unwrap_or(5u64).min(60);
                    last_err = format!("HTTP {code}");
                    std::thread::sleep(Duration::from_secs(wait));
                }
                Err(e) => {
                    last_err = e.to_string();
                    std::thread::sleep(Duration::from_secs(2));
                }
            }
        }
        Err(format!("{url}: {last_err}"))
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

