//! 最小的 HTTP/1.x 服务端，只够接 nginx 转过来的请求（不直接对公网）：
//! 一个连接一个请求（回完就关，`Connection: close`），请求头限 16KB、正文限 64KB（按 `Content-Length`，不认 chunked——
//! nginx 转给上游前会把正文收齐、带上 `Content-Length`），读写各 10 秒超时，同时最多处理 16 个连接，多了直接回 503。
//! 不用第三方 HTTP 库：接口只有四个，自己解析更可控。
use crate::{handle, Request, Response, Store};
use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

const MAX_HEAD: usize = 16 * 1024;
const MAX_BODY: usize = 64 * 1024;
const MAX_CONN: usize = 16;
const TIMEOUT: Duration = Duration::from_secs(10);

/// 在 `addr`（如 `127.0.0.1:7200`）上一直服务下去。
pub fn serve(addr: &str, store: Store) -> Result<(), String> {
    let listener = TcpListener::bind(addr).map_err(|e| format!("监听 {addr}: {e}"))?;
    eprintln!("kosync 在 {addr} 上服务");
    let store = Arc::new(Mutex::new(store));
    let busy = Arc::new(AtomicUsize::new(0));
    for conn in listener.incoming() {
        let Ok(mut stream) = conn else { continue };
        if busy.load(Ordering::SeqCst) >= MAX_CONN {
            let _ = write_response(&mut stream, &crate::message(503, "busy"));
            continue;
        }
        busy.fetch_add(1, Ordering::SeqCst);
        let (store, busy) = (store.clone(), busy.clone());
        std::thread::spawn(move || {
            let _ = stream.set_read_timeout(Some(TIMEOUT));
            let _ = stream.set_write_timeout(Some(TIMEOUT));
            let resp = match read_request(&mut stream) {
                Ok(req) => {
                    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
                    // 锁中毒（别的线程处理时 panic）也照样用：数据每次改完都已落盘，内存里的状态不会半截
                    let mut s = store.lock().unwrap_or_else(|p| p.into_inner());
                    handle(&mut s, &req, now)
                }
                Err(status) => crate::message(status, "bad request"),
            };
            let _ = write_response(&mut stream, &resp);
            busy.fetch_sub(1, Ordering::SeqCst);
        });
    }
    Ok(())
}

/// 读一个请求。出错返回要回的状态码（400、413、431）。
pub fn read_request(stream: &mut impl Read) -> Result<Request, u16> {
    let mut buf = Vec::with_capacity(1024);
    let mut chunk = [0u8; 4096];
    let head_end = loop {
        if let Some(i) = find(&buf, b"\r\n\r\n") {
            break i;
        }
        if buf.len() > MAX_HEAD {
            return Err(431);
        }
        let n = stream.read(&mut chunk).map_err(|_| 400u16)?;
        if n == 0 {
            return Err(400);
        }
        buf.extend_from_slice(&chunk[..n]);
    };
    let head = std::str::from_utf8(&buf[..head_end]).map_err(|_| 400u16)?;
    let mut lines = head.split("\r\n");
    let mut first = lines.next().ok_or(400u16)?.split(' ');
    let (method, target, version) = (first.next().ok_or(400u16)?, first.next().ok_or(400u16)?, first.next().ok_or(400u16)?);
    if !version.starts_with("HTTP/1.") || first.next().is_some() {
        return Err(400);
    }
    let mut headers = BTreeMap::new();
    for line in lines {
        let (k, v) = line.split_once(':').ok_or(400u16)?;
        headers.insert(k.trim().to_ascii_lowercase(), v.trim().to_string());
    }
    if headers.get("transfer-encoding").is_some_and(|v| !v.eq_ignore_ascii_case("identity")) {
        return Err(400);
    }
    let len: usize = match headers.get("content-length") {
        Some(v) => v.parse().map_err(|_| 400u16)?,
        None => 0,
    };
    if len > MAX_BODY {
        return Err(413);
    }
    let mut body = buf[head_end + 4..].to_vec();
    while body.len() < len {
        let n = stream.read(&mut chunk).map_err(|_| 400u16)?;
        if n == 0 {
            return Err(400);
        }
        body.extend_from_slice(&chunk[..n]);
    }
    body.truncate(len);
    let path = target.split('?').next().unwrap_or("");
    Ok(Request { method: method.to_string(), path: percent_decode(path).ok_or(400u16)?, headers, body })
}

fn write_response(stream: &mut TcpStream, resp: &Response) -> std::io::Result<()> {
    let body = resp.body.to_string();
    let reason = match resp.status {
        200 => "OK",
        201 => "Created",
        400 => "Bad Request",
        401 => "Unauthorized",
        402 => "Payment Required",
        404 => "Not Found",
        413 => "Payload Too Large",
        431 => "Request Header Fields Too Large",
        503 => "Service Unavailable",
        _ => "Error",
    };
    write!(
        stream,
        "HTTP/1.1 {} {reason}\r\nContent-Type: application/json; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        resp.status,
        body.len()
    )?;
    stream.flush()
}

fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len()).position(|w| w == needle)
}

/// `%XX` 解码成 UTF-8；坏的转义或解出来不是 UTF-8 返回 `None`。
fn percent_decode(s: &str) -> Option<String> {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' {
            let h = std::str::from_utf8(b.get(i + 1..i + 3)?).ok()?;
            out.push(u8::from_str_radix(h, 16).ok()?);
            i += 3;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_request_with_body_split_across_reads() {
        struct Chunks(Vec<&'static [u8]>);
        impl Read for Chunks {
            fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
                if self.0.is_empty() {
                    return Ok(0);
                }
                let c = self.0.remove(0);
                buf[..c.len()].copy_from_slice(c);
                Ok(c.len())
            }
        }
        let mut s = Chunks(vec![b"PUT /syncs/progress?x=1 HTTP/1.0\r\nX-Auth-User: afu\r\nContent-Length: 11\r\n", b"\r\n{\"a\":", b"\"bcd\"}"]);
        let r = read_request(&mut s).unwrap();
        assert_eq!((r.method.as_str(), r.path.as_str()), ("PUT", "/syncs/progress"));
        assert_eq!(r.headers["x-auth-user"], "afu");
        assert_eq!(r.body, b"{\"a\":\"bcd\"}");
    }

    #[test]
    fn rejects_bad_requests() {
        let mut big = std::io::Cursor::new(format!("PUT / HTTP/1.1\r\nContent-Length: {}\r\n\r\n", MAX_BODY + 1).into_bytes());
        assert_eq!(read_request(&mut big).unwrap_err(), 413);
        let mut chunked = std::io::Cursor::new(b"PUT / HTTP/1.1\r\nTransfer-Encoding: chunked\r\n\r\n".to_vec());
        assert_eq!(read_request(&mut chunked).unwrap_err(), 400);
        let mut junk = std::io::Cursor::new(b"hello\r\n\r\n".to_vec());
        assert_eq!(read_request(&mut junk).unwrap_err(), 400);
        let mut huge_head = std::io::Cursor::new(vec![b'a'; MAX_HEAD + 10]);
        assert_eq!(read_request(&mut huge_head).unwrap_err(), 431);
        assert_eq!(percent_decode("/syncs/progress/%E4%B8%AD"), Some("/syncs/progress/中".into()));
        assert_eq!(percent_decode("/x%G1"), None);
    }
}
