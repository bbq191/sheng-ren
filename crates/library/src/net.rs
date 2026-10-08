//! 联网查书目用的 HTTP：节流、重试、网址编码。封面和元数据共用。

use serde_json::Value;
use std::cell::{Cell, RefCell};
use std::collections::HashSet;
use std::time::{Duration, Instant};

/// 一次响应最多读多少字节。
const MAX_BODY: u64 = 20 << 20;

pub(crate) const UA: &str = "booklib/0.1 (personal e-book library tool)";

/// 最多试几次（429、5xx、超时这类临时错误才重试）。
const TRIES: usize = 4;

/// 带节流和重试的 HTTP：Wikidata 限速严（连续请求会 429），每个请求间隔至少 1.2 秒，429 时按 Retry-After 等。
///
/// - 4xx（除了 429、403）是"没有"，不重试；403 不重试、但算临时错误（多半是被反爬拦了，见下）；
/// - 连不上某个网站（DNS 解析失败、连接失败）就记下这个网站，之后发给它的请求立即失败，不再每次重试等待；
///   一个网站连不上不等于没网（Wikidata、Open Library 在有些网络里单独连不上）：接连有两个不同的网站连不上、
///   其间没有任何请求成功，才算**离线**（[`Net::offline`]）；
/// - 临时错误（离线、429/5xx 重试完、超时等）记下来（[`Net::transient_error`]）：调用方据此区分"没找到"和"没查成"，
///   没查成的不能当"没有封面"去生成封面并存下来。
pub(crate) struct Net {
    agent: ureq::Agent,
    last: Cell<Option<Instant>>,
    /// 连不上的网站（主机名）。
    down: RefCell<HashSet<String>>,
    /// 上次有请求成功以来，新发现连不上的网站有几个。
    down_since_ok: Cell<usize>,
    transient: RefCell<Option<String>>,
}

impl Net {
    pub(crate) fn new() -> Net {
        Net { agent: ureq::AgentBuilder::new().timeout(Duration::from_secs(30)).user_agent(UA).build(), last: Cell::new(None), down: RefCell::default(), down_since_ok: Cell::new(0), transient: RefCell::new(None) }
    }

    /// 看起来整个断网了：接连两个不同的网站连不上，其间没有请求成功。
    pub(crate) fn offline(&self) -> bool {
        self.down_since_ok.get() >= 2
    }

    /// 从上次 [`Net::clear_transient`] 以来遇到的第一个临时网络错误。
    pub(crate) fn transient_error(&self) -> Option<String> {
        self.transient.borrow().clone()
    }

    pub(crate) fn clear_transient(&self) {
        self.transient.borrow_mut().take();
    }

    pub(crate) fn note_transient(&self, e: &str) {
        self.transient.borrow_mut().get_or_insert_with(|| e.to_string());
    }

    pub(crate) fn fetch(&self, url: &str) -> Result<Vec<u8>, String> {
        self.fetch_ref(url, None)
    }

    /// `referer`：豆瓣图片服务器不带来源页会拒绝（HTTP 418）。
    pub(crate) fn fetch_ref(&self, url: &str, referer: Option<&str>) -> Result<Vec<u8>, String> {
        let host = host_of(url);
        if self.down.borrow().contains(host) {
            let e = format!("{url}: 连不上 {host}，跳过");
            self.note_transient(&e);
            return Err(e);
        }
        let mut last_err = String::new();
        for attempt in 1..=TRIES {
            if let Some(t) = self.last.get() {
                // 先取一次已过时间再减：判断和相减之间时间还在走，`gap - t.elapsed()` 可能下溢 panic
                let wait = Duration::from_millis(1200).saturating_sub(t.elapsed());
                if !wait.is_zero() {
                    std::thread::sleep(wait);
                }
            }
            self.last.set(Some(Instant::now()));
            let mut req = self.agent.get(url);
            if let Some(r) = referer {
                req = req.set("Referer", r);
            }
            let res = req.call();
            // 网站回了 HTTP 状态（哪怕是 404、403、5xx），说明网是通的：断网的判定（接连两个网站连不上）从头算
            if matches!(res, Ok(_) | Err(ureq::Error::Status(..))) {
                self.down_since_ok.set(0);
            }
            let wait = match res {
                Ok(r) => {
                    match bookconv::util::read_capped(r.into_reader(), MAX_BODY, 0) {
                        // 超过上限的不是封面也不是条目页：报错，不能截断了当成功（截断的图读得出尺寸，会被当封面存下）
                        Ok(None) => return Err(format!("{url}: 响应超过 {} MB", MAX_BODY >> 20)),
                        Ok(Some(buf)) => return Ok(buf),
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
                // 403 多半是被反爬拦了（豆瓣），不是"没有"：记成临时错误，本次不落结论（不当成没这本书、不生成封面存下）
                Err(ureq::Error::Status(403, _)) => {
                    let e = format!("{url}: HTTP 403（可能被网站拦了）");
                    self.note_transient(&e);
                    return Err(e);
                }
                Err(ureq::Error::Status(code, _)) => return Err(format!("{url}: HTTP {code}")),
                Err(e @ ureq::Error::Transport(_)) => match e.kind() {
                    ureq::ErrorKind::Dns | ureq::ErrorKind::ConnectionFailed => {
                        if self.down.borrow_mut().insert(host.to_string()) {
                            self.down_since_ok.set(self.down_since_ok.get() + 1);
                        }
                        let e = format!("连不上 {host}（{e}）");
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

    /// 同 [`Net::json`]，但回来的不是 JSON 也算临时错误：豆瓣反爬时搜索接口照样回 200，内容是验证页，
    /// 不能当成"没这本书"（那样会生成封面存下来、以后不再找）。
    pub(crate) fn json_strict(&self, url: &str) -> Result<Value, String> {
        let body = self.fetch(url)?;
        serde_json::from_slice(&body).map_err(|e| {
            let e = format!("{url}: 回来的不是 JSON（{e}，可能被网站拦了）");
            self.note_transient(&e);
            e
        })
    }
}

/// 网址里的主机名（`https://book.douban.com/j/…` → `book.douban.com`）。
fn host_of(url: &str) -> &str {
    let rest = url.split_once("://").map_or(url, |(_, r)| r);
    rest.split(['/', '?', '#']).next().unwrap_or(rest)
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

#[cfg(test)]
mod tests {
    use super::Net;
    use std::io::{Read, Write};

    /// 本机上一个关着的端口（连上去立刻被拒）。
    fn closed_port() -> u16 {
        std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port()
    }

    #[test]
    fn an_http_error_reply_proves_the_network_is_up() {
        // 本机的小服务器：每个连接都回 404
        let srv = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = srv.local_addr().unwrap().port();
        std::thread::spawn(move || {
            for mut c in srv.incoming().flatten() {
                let mut buf = [0u8; 4096];
                let _ = c.read(&mut buf);
                let _ = c.write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
            }
        });
        let net = Net::new();
        assert!(net.fetch(&format!("http://127.0.0.1:{}/", closed_port())).is_err());
        assert!(!net.offline(), "一个网站连不上不算断网");
        assert!(net.fetch(&format!("http://127.0.0.1:{port}/x")).unwrap_err().contains("404"));
        assert!(net.fetch(&format!("http://127.0.0.1:{}/", closed_port())).is_err());
        assert!(!net.offline(), "两个连不上的网站之间有网站回了 404：网是通的，不算断网");
        assert!(net.fetch(&format!("http://127.0.0.1:{}/", closed_port())).is_err());
        assert!(net.offline(), "接连两个网站连不上才算断网");
    }

    #[test]
    fn host_of_takes_the_authority_part() {
        assert_eq!(super::host_of("https://book.douban.com/j/subject_suggest?q=x"), "book.douban.com");
        assert_eq!(super::host_of("https://www.wikidata.org?x"), "www.wikidata.org");
        assert_eq!(super::host_of("covers.openlibrary.org/b/id/1-L.jpg"), "covers.openlibrary.org");
    }
}
