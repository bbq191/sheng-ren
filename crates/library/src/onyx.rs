//! 文石（Onyx）自带阅读器的按书显示设置（profile `[deliver]` 的 `reader_display = "onyx"`，见 [`profile::ReaderDisplay::Onyx`]）。
//!
//! 阅读器「显示」里的深色增强（缺省 30）、图片平滑（缺省开，抖动成 8 级灰）是按书存的、没有全局缺省，产物里也写不了。
//! 它的书库数据库能经 adb 的 `content` 命令读写（`content://com.onyx.content.database.ContentProvider/Metadata`，
//! 不用 root）：MTP 拷进去的书几秒内就有一行，`extraAttributes` 是空的（`NULL`），第一次打开时才填。往空的那栏只写
//! 显示设置一项（`backend.page_refresh_setting`），第一次打开就照它显示（2026-10-30 Poke7 灰阶测试书：写了以后 32 级灰
//! 逐值和原图相同、面板上深色增强 0、图片平滑关）。打开过的（那栏不空）不碰：维护者自己调过的照留。
//!
//! `content` 命令的值里冒号是分隔符、反斜杠会被吃掉（写进去的 JSON 嵌套的 `\"` 变成 `"`），两样都要先转义（[`escape_bind`]）。
//! 经 adb 推进去的书阅读器不收录，所以书得走 MTP（`sync` 本来就是）。

use std::collections::{BTreeMap, BTreeSet};
use std::io::Read;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const URI: &str = "content://com.onyx.content.database.ContentProvider/Metadata";
const READER_PACKAGE: &str = "com.onyx.kreader";
/// MTP 上的内部存储在设备上的路径（阅读器数据库里记的就是这个）。
pub(crate) const STORAGE: &str = "/storage/emulated/0";
/// 刚拷进去的书等阅读器收录的最长时间（实测 5 秒内）。
const INDEX_WAIT: Duration = Duration::from_secs(30);
/// 一次 adb 调用最长等多久（另一台经 Wi-Fi 连的设备半断线时 `adb -s … shell` 会一直挂着）。
const ADB_TIMEOUT: Duration = Duration::from_secs(20);

/// 要写的显示设置：深色增强 0（`imageGamma` 100；缺省 30 存的是 130）、图片平滑关（`ditherEnable`），别的照阅读器缺省。
fn seed_json() -> String {
    let refresh = r#"{"antialiasInFastMode":false,"colorBrightness":0,"ditherEnable":false,"fullRefreshType":"NORMAL_GC","imageGamma":100,"imageMono":0,"sharpeness":0.0,"updateInterval":10,"updateMode":"None","vividnessEnhancement":40}"#;
    serde_json::json!({ "backend": { "page_refresh_setting": refresh }, "dummyObject": false }).to_string()
}

/// 一次补写的结果（按书算，不按数据库行）。
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Seeded {
    /// 写了的书（数据库里那栏原来是空的）。
    pub written: usize,
    /// 等了 [`INDEX_WAIT`] 阅读器还没收录（或还没收录传完的那份）的：这次没写，打开前要手动调。
    pub pending: usize,
}

/// 给这一轮传进设备 `dir`（存储根目录下的产物目录，如 `Books`）的书写显示设置。`files` 是 (相对 `dir` 的路径（`/` 分隔）, 字节数)。
/// 只写这些书、只写那栏还空着的（没打开过的）；维护者自己放进去的别的书不碰。
pub(crate) fn seed(dir: &str, files: &[(String, u64)]) -> Result<Seeded, String> {
    let adb = Adb::find()?;
    let prefix = format!("{STORAGE}/{dir}/");
    let expected: BTreeMap<String, u64> = files.iter().map(|(f, n)| (format!("{prefix}{f}"), *n)).collect();
    // jmtpfs 写文件先建一个空文件再传（阅读器两次都收录，一本两行，一行 size 0）：要等到大小对得上的那行出来
    let start = Instant::now();
    let ready = loop {
        let rows = adb.rows(&prefix)?;
        let ready: BTreeSet<&String> = expected.iter().filter(|(p, n)| rows.iter().any(|(q, m)| q == *p && m == *n)).map(|(p, _)| p).collect();
        if ready.len() == expected.len() || start.elapsed() >= INDEX_WAIT {
            break ready;
        }
        std::thread::sleep(Duration::from_secs(2));
    };
    let blank: BTreeSet<String> = adb.blank(&prefix)?.into_iter().filter(|p| ready.contains(p)).collect();
    let json = seed_json();
    for path in &blank {
        adb.write(path, &json)?; // 同一路径的几行一起写
    }
    // 读回来核对：还是空的就是没写进去（阅读器换了数据库格式之类）
    let left = adb.blank(&prefix)?.into_iter().filter(|p| blank.contains(p)).collect::<BTreeSet<_>>().len();
    if left > 0 {
        return Err(format!("写了 {} 本，{left} 本读回来还是空的（阅读器的数据库可能换了格式）", blank.len()));
    }
    Ok(Seeded { written: blank.len(), pending: expected.len() - ready.len() })
}

/// 经 adb 连着的文石阅读器。
struct Adb {
    serial: String,
}

impl Adb {
    /// 找装着文石阅读器的那台（`$ANDROID_SERIAL` 设了就只认它，adb 自己也认这个变量）。
    fn find() -> Result<Adb, String> {
        let serials: Vec<String> = match std::env::var("ANDROID_SERIAL").ok().filter(|s| !s.is_empty()) {
            Some(s) => vec![s],
            None => parse_devices(&run(Command::new("adb").arg("devices")).map_err(|e| format!("adb 用不了（{e}）"))?),
        };
        let mut found = serials.into_iter().filter(|s| run(Command::new("adb").args(["-s", s, "shell", "pm", "path", READER_PACKAGE])).is_ok_and(|o| o.contains("package:")));
        let first = found.next().ok_or("adb 上没找到文石阅读器（设备上 USB 调试开了、这台电脑授权了吗？）")?;
        if found.next().is_some() {
            return Err("adb 上连着几台装了文石阅读器的设备，设 ANDROID_SERIAL 指定一台".into());
        }
        Ok(Adb { serial: first })
    }

    /// 跑一条 `content` 命令；它出错时退出码也是 0，错误写在输出里。
    fn content(&self, args: &str) -> Result<String, String> {
        let out = run(Command::new("adb").args(["-s", &self.serial, "shell", &format!("content {args}")]))?;
        match out.lines().find(|l| content_error(l)) {
            Some(l) => Err(format!("阅读器数据库：{}", l.trim())),
            None => Ok(out),
        }
    }

    /// 数据库里 `prefix` 下的行：(路径, 字节数)。
    fn rows(&self, prefix: &str) -> Result<Vec<(String, u64)>, String> {
        let out = self.content(&format!("query --uri {URI} --projection nativeAbsolutePath:size --where {}", sh_quote(&under(prefix))))?;
        parse_rows(&out)?.into_iter().map(|r| {
            let (p, n) = r.rsplit_once(", size=").ok_or_else(|| format!("看不懂阅读器数据库的输出：{r}"))?;
            Ok((p.to_string(), n.trim().parse().unwrap_or(0)))
        }).collect()
    }

    /// `prefix` 下 `extraAttributes` 还空着的（没打开过的）书的路径。
    fn blank(&self, prefix: &str) -> Result<Vec<String>, String> {
        let cond = format!("{} AND extraAttributes IS NULL", under(prefix));
        parse_rows(&self.content(&format!("query --uri {URI} --projection nativeAbsolutePath --where {}", sh_quote(&cond)))?)
    }

    /// 只在那栏还空着时写（这期间维护者刚好打开了这本的话不盖掉）。
    fn write(&self, path: &str, json: &str) -> Result<(), String> {
        let cond = format!("nativeAbsolutePath={} AND extraAttributes IS NULL", sql_str(path));
        let bind = format!("extraAttributes:s:{}", escape_bind(json));
        self.content(&format!("update --uri {URI} --bind {} --where {}", sh_quote(&bind), sh_quote(&cond))).map(|_| ())
    }
}

/// 路径在 `prefix` 下（不用 LIKE：路径里的 `%`、`_` 会被当通配符）。
fn under(prefix: &str) -> String {
    format!("substr(nativeAbsolutePath,1,{})={}", prefix.chars().count(), sql_str(prefix))
}

/// `content` 输出里报错的行（查询、更新出错都是退出码 0：`Error while accessing provider`、异常栈、参数不对时打的用法）。
fn content_error(line: &str) -> bool {
    line.contains("Error") || line.contains("Exception") || line.starts_with("usage:")
}

/// 跑一条命令，限时 [`ADB_TIMEOUT`]；stdout、stderr 合在一起返回（adb 把设备上的 stderr 分开传回来）。
fn run(cmd: &mut Command) -> Result<String, String> {
    let mut child = cmd.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().map_err(|e| e.to_string())?;
    let pipe = |r: Option<Box<dyn Read + Send>>| std::thread::spawn(move || {
        let mut s = Vec::new();
        if let Some(mut r) = r {
            let _ = r.read_to_end(&mut s);
        }
        s
    });
    let out = pipe(child.stdout.take().map(|r| Box::new(r) as Box<dyn Read + Send>));
    let err = pipe(child.stderr.take().map(|r| Box::new(r) as Box<dyn Read + Send>));
    let start = Instant::now();
    let status = loop {
        match child.try_wait().map_err(|e| e.to_string())? {
            Some(s) => break s,
            None if start.elapsed() >= ADB_TIMEOUT => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!("adb 超过 {} 秒没回应", ADB_TIMEOUT.as_secs()));
            }
            None => std::thread::sleep(Duration::from_millis(50)),
        }
    };
    let text = [out, err].map(|t| String::from_utf8_lossy(&t.join().unwrap_or_default()).into_owned()).join("");
    if !status.success() {
        return Err(text.trim().to_string());
    }
    Ok(text)
}

/// `adb devices` 里状态是 `device` 的序列号（`unauthorized`、`offline` 的不算）。
fn parse_devices(out: &str) -> Vec<String> {
    out.lines().skip(1).filter_map(|l| {
        let mut f = l.split_whitespace();
        let (s, state) = (f.next()?, f.next()?);
        (state == "device").then(|| s.to_string())
    }).collect()
}

/// `content query --projection nativeAbsolutePath[:…]` 的输出：每行 `Row: N nativeAbsolutePath=路径[, 列=值…]`，返回 `Row: N ` 后面
/// `nativeAbsolutePath=` 之后的部分；没有时 `No result found.`。
fn parse_rows(out: &str) -> Result<Vec<String>, String> {
    let mut v = Vec::new();
    for l in out.lines() {
        if let Some(rest) = l.strip_prefix("Row: ") {
            match rest.split_once(" nativeAbsolutePath=") {
                Some((_, p)) => v.push(p.to_string()),
                None => return Err(format!("看不懂阅读器数据库的输出：{l}")),
            }
        } else if content_error(l) {
            return Err(format!("阅读器数据库：{}", l.trim()));
        }
    }
    Ok(v)
}

/// `content --bind` 的值：反斜杠、冒号前面加反斜杠（不转义的话冒号当分隔符报「Binding not well formed」、反斜杠被吃掉）。
fn escape_bind(v: &str) -> String {
    v.replace('\\', "\\\\").replace(':', "\\:")
}

/// SQL 字符串字面量。
fn sql_str(v: &str) -> String {
    format!("'{}'", v.replace('\'', "''"))
}

/// 设备上 sh 的单引号字符串（adb shell 把参数拼成一行交给 sh）。
fn sh_quote(v: &str) -> String {
    format!("'{}'", v.replace('\'', r"'\''"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seed_json_shape() {
        let v: serde_json::Value = serde_json::from_str(&seed_json()).unwrap();
        let r: serde_json::Value = serde_json::from_str(v["backend"]["page_refresh_setting"].as_str().unwrap()).unwrap();
        assert_eq!((r["imageGamma"].as_i64(), r["ditherEnable"].as_bool()), (Some(100), Some(false)));
    }

    #[test]
    fn bind_escaping() {
        assert_eq!(escape_bind(r#"{"a":"{\"b\":1}"}"#), r#"{"a"\:"{\\"b\\"\:1}"}"#);
        assert_eq!(sql_str("/x/它's.epub"), "'/x/它''s.epub'");
        assert_eq!(sh_quote("a'b"), r"'a'\''b'");
    }

    #[test]
    fn parses_adb_output() {
        assert_eq!(parse_devices("List of devices attached\nDAF8A2BD\tdevice usb:3-1\nX1\tunauthorized\n\n"), ["DAF8A2BD"]);
        let rows = "Row: 0 nativeAbsolutePath=/storage/emulated/0/Books/a b.epub\nRow: 1 nativeAbsolutePath=/storage/emulated/0/Books/漫画/c.epub\n";
        assert_eq!(parse_rows(rows).unwrap(), ["/storage/emulated/0/Books/a b.epub", "/storage/emulated/0/Books/漫画/c.epub"]);
        assert!(parse_rows("No result found.\n").unwrap().is_empty());
        assert!(parse_rows("Error while accessing provider:x\n").is_err());
        assert!(content_error("usage: adb shell content [subcommand] [options]") && !content_error("Row: 0 nativeAbsolutePath=/a"));
        assert_eq!(under("/s/Books/"), "substr(nativeAbsolutePath,1,9)='/s/Books/'");
    }
}
