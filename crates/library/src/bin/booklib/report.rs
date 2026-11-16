//! 结果行与一轮汇总的格式化：生成/传输事件、计数汇总、按类归并的提醒，以及拼给用户的路径提示。

use library::{Added, BuildCounts, BuildEvent, Built};
use std::collections::HashSet;
use std::path::{Path, PathBuf};

pub(crate) fn added_line(a: &Added) -> String {
    match a {
        Added::New(m) => format!("✓ 入库 {}  {}", m.id, m.title),
        Added::Existing(m) => format!("= 已在库里 {}  {}", m.id, m.title),
        Added::Replaced(m, old) => format!(
            "↻ 原件改过，换成新版本 {}  {} ← {old}（旧版本的条目和产物已删）",
            m.id, m.title
        ),
    }
}

/// 一轮生成的汇总一行（2026-09-29 反馈"看不出是否更新过"）。
pub(crate) fn counts_summary(c: &BuildCounts) -> String {
    let same = if c.same > 0 {
        format!("，重新生成但内容没变（没再传）{} 本", c.same)
    } else {
        String::new()
    };
    let adopted = if c.adopted > 0 {
        format!("，认领设备上已有的 {} 本", c.adopted)
    } else {
        String::new()
    };
    let retracted = if c.retracted > 0 {
        format!("，不在跟踪范围、从设备上删了 {} 本", c.retracted)
    } else {
        String::new()
    };
    format!("生成：重新生成并传上 {} 本{same}{adopted}，挪位置 {} 本，已是最新 {} 本{retracted}，失败 {} 本", c.written, c.moved, c.up_to_date, c.failed)
}

/// 一轮生成里的一件事显示成一行（`quiet` 时没变化的不报）；`Err` 的算失败（退出码 2）。
pub(crate) fn event_line(ev: BuildEvent, quiet: bool) -> Option<Result<String, String>> {
    Some(match ev {
        BuildEvent::Unsupported { id, title, reason } => {
            Ok(format!("- 跳过 {id}  {title}：{reason}"))
        }
        BuildEvent::RemovedFlushed { device, count } => {
            Ok(format!("✓ [{device}] 删掉了 {count} 本已从书库删除的书"))
        }
        BuildEvent::RemoveFlushFailed { device, error } => {
            Err(format!("✗ [{device}] 删已从书库删除的书：{error}"))
        }
        BuildEvent::DeviceAbsent { device, reason } => {
            Ok(format!("- [{device}] {reason}，这次不传"))
        }
        BuildEvent::Outside { device, title } => Ok(format!(
            "? [{device}] {title}：不在这台的跟踪范围了（--keep，这次没从设备上删）"
        )),
        BuildEvent::Retracted { device, title } => Ok(format!(
            "✗ [{device}] {title}：不在这台的跟踪范围了，从设备上删了"
        )),
        BuildEvent::RetractFailed {
            device,
            title,
            error,
        } => Err(format!(
            "✗ [{device}] {title}：不在这台的跟踪范围了，从设备上删失败：{error}"
        )),
        BuildEvent::Built {
            device,
            title,
            built,
        } => return built_line(&device, &title, built, quiet).map(Ok),
        BuildEvent::Failed {
            device,
            title,
            error,
        } => Err(format!("✗ [{device}] {title}: {error}")),
        BuildEvent::TransportLost { device, count } => Err(format!(
            "✗ [{device}] 传输线程意外退出，{count} 本没传成（下次再传）"
        )),
        BuildEvent::Stalled {
            device,
            count,
            secs,
        } => Err(format!(
            "✗ [{device}] 挂载没响应（{secs} 秒没有进展），这一轮不再往它传；{count} 本没传成，记录没改，下次重传"
        )),
        // 空间不够不算失败（不影响退出码）：下一轮、下次同步腾出空间再传
        BuildEvent::NoSpace {
            device,
            count,
            need,
            free,
            short,
            orphans,
        } => Ok(format!(
            "! [{device}] 空间不够：{count} 本要传，连余量约要 {}，只剩 {}，还差 {}；这次不往这台写（腾出空间后再同步）{}",
            library::mb(need),
            library::mb(free),
            library::mb(short),
            if orphans > 0 {
                format!("；这一轮末尾会先删掉 {orphans} 个已删书的旧文件，删完可再同步一次")
            } else {
                String::new()
            }
        )),
        BuildEvent::SpaceUnknown { device, reason } => Ok(format!(
            "! [{device}] 读不到剩余空间（{reason}），不预先检查，照旧传"
        )),
        BuildEvent::DeviceFull {
            device,
            title,
            error,
        } => Ok(format!(
            "! [{device}] {title}：空间满，写了一半的已删掉（覆盖旧版本的话设备上的旧版本也已不在），这一轮不再往这台传（下次重传）：{error}"
        )),
        BuildEvent::Waiting { in_flight } => Ok(format!(
            "… 都生成好了，等设备那边传完（还有 {in_flight} 本）"
        )),
    })
}

/// 一本书一个模式的结果一行（`quiet` 时没变化的不报）。
pub(crate) fn built_line(device: &str, title: &str, b: Built, quiet: bool) -> Option<String> {
    let with_warnings = |head: String, warnings: &[String]| {
        std::iter::once(head)
            .chain(
                warnings
                    .iter()
                    // 按书刷屏的这几类由 [`WarningSummary`] 一轮汇总成一行
                    .filter(|w| !is_round_summary_warning(w))
                    .map(|w| format!("  ⚠ {w}")),
            )
            .collect::<Vec<_>>()
            .join("\n")
    };
    match b {
        Built::Written { path, warnings } => Some(with_warnings(
            format!("✓ 生成 [{device}] {title} → {}", path.display()),
            &warnings,
        )),
        Built::Same { path, warnings } => Some(with_warnings(
            format!(
                "≡ 重新生成 [{device}] {title}：和设备上的一样，没再传（{}）",
                path.display()
            ),
            &warnings,
        )),
        Built::UpToDate(path) => {
            (!quiet).then(|| format!("= [{device}] {title} 已是最新（{}）", path.display()))
        }
        Built::Adopted(path) => Some(format!(
            "= [{device}] {title} 认领设备上已有的那份，没重新生成（{}）",
            path.display()
        )),
        Built::Moved { from, to } => Some(format!(
            "↪ 挪位置 [{device}] {title} → {}（原来在 {}）",
            to.display(),
            from.display()
        )),
    }
}

/// 这些提醒按书刷屏（vinci 2026-11-16），一轮结束时由 [`WarningSummary`] 每类汇总成一行，逐本结果行里不再打。
/// 认法用 `library` / `bookconv` 里的公开常量，不做模糊匹配；真正影响结果的失败（`BuildEvent::Failed`、传输错误）
/// 不走这里，照旧逐本打。
fn is_round_summary_warning(w: &str) -> bool {
    w == library::AUDIT_WARNING
        || w == library::GATE_WARNING
        || w == library::KFX_WARNING
        || w == bookconv::kepub::RISKY_CSS_WARNING
}

/// 一轮里命中的提醒（按设备 + 产物路径去重，同名书也不混）。以前逐本打，书一多就刷屏：现在每类一轮只汇总一行，
/// 哪几本用 `booklib list` 看（明细记在生成记录的 `audit` 里）。只算这一轮真的重新生成的（`Written` + `Same`）。
#[derive(Default)]
pub(crate) struct WarningSummary {
    audit: WarnedBooks,
    gate: WarnedBooks,
    kfx: WarnedBooks,
    kepub_css: WarnedBooks,
}

/// 同一类提醒命中的书：`(设备, 产物路径)` 去重；按书名归并，同一本书在几台上都命中只算一本、列出是哪几台
/// （以前按产物计，一本书三台都没过质量门会算成 3 本，前 5 个名额被同一本占掉，别的书就看不到了）。
/// 只认得书名（`BuildEvent::Built` 不带书的 id）：两本同名的书会合成一项，汇总行只是指个方向，明细以 `list` 为准。
#[derive(Default)]
struct WarnedBooks {
    /// 先来的在前：(书名, 命中的设备)。汇总行列前 5 本按这个顺序（同一轮里顺序稳定）。
    books: Vec<(String, Vec<String>)>,
    seen: HashSet<(String, PathBuf)>,
}

impl WarnedBooks {
    fn insert(&mut self, device: &str, path: &Path, title: &str) {
        if !self.seen.insert((device.to_string(), path.to_path_buf())) {
            return;
        }
        match self.books.iter_mut().find(|(t, _)| t == title) {
            Some((_, devs)) => {
                if !devs.iter().any(|d| d == device) {
                    devs.push(device.to_string());
                }
            }
            None => self
                .books
                .push((title.to_string(), vec![device.to_string()])),
        }
    }

    fn len(&self) -> usize {
        self.books.len()
    }

    fn is_empty(&self) -> bool {
        self.books.is_empty()
    }

    /// 汇总行里的书名：最多列 5 本，每本带上命中的设备，多的写「等 N 本」（N 是这一类一共几本）。
    fn titles(&self) -> String {
        const SHOW: usize = 5;
        let shown: Vec<String> = self
            .books
            .iter()
            .take(SHOW)
            .map(|(t, devs)| format!("《{t}》[{}]", devs.join("、")))
            .collect();
        let mut out = shown.join("、");
        if self.len() > SHOW {
            out.push_str(&format!(" 等 {} 本", self.len()));
        }
        out
    }
}

impl WarningSummary {
    pub(crate) fn observe(&mut self, ev: &BuildEvent) {
        let BuildEvent::Built {
            device,
            title,
            built,
        } = ev
        else {
            return;
        };
        let (path, warnings) = match built {
            Built::Written { path, warnings } | Built::Same { path, warnings } => (path, warnings),
            _ => return,
        };
        for w in warnings {
            let set = if w == bookconv::kepub::RISKY_CSS_WARNING {
                &mut self.kepub_css
            } else if w == library::AUDIT_WARNING {
                &mut self.audit
            } else if w == library::GATE_WARNING {
                &mut self.gate
            } else if w == library::KFX_WARNING {
                &mut self.kfx
            } else {
                continue;
            };
            set.insert(device, path, title);
        }
    }

    /// 一轮结束的汇总行（每类一行）；没有就不打。体检只是提示，不列书名；质量门、KFX 要让人看出是哪几本。
    pub(crate) fn lines(&self) -> Vec<String> {
        let mut out = Vec::new();
        if !self.audit.is_empty() {
            out.push(format!(
                "⚠ 体检：{} 本书有和 Kindle 不一样没处理、或没验证过的写法（只是提示，书照常生成；明细用 booklib list 看）",
                self.audit.len()
            ));
        }
        if !self.gate.is_empty() {
            out.push(format!(
                "⚠ 质量门未过：{} 本书（{}）——产物照常生成、照常传；每本的问题用 booklib list 看",
                self.gate.len(),
                self.gate.titles()
            ));
        }
        if !self.kfx.is_empty() {
            out.push(format!(
                "⚠ KFX：{} 本书转换时有问题（{}；如图片取不到、目录项或链接找不到位置）——每本的明细用 booklib list 看",
                self.kfx.len(),
                self.kfx.titles()
            ));
        }
        if !self.kepub_css.is_empty() {
            out.push(format!(
                "⚠ KEPUB：{} 本书的 CSS 可能作用到新加的包装或 koboSpan（只提醒，没改 CSS；哪几本用 booklib list 看）",
                self.kepub_css.len()
            ));
        }
        out
    }
}

/// 选书的参数里有像路径的（通常是路径里有空格没加引号，被拆开了），提示一下。
pub(crate) fn path_hint(selectors: &[String]) -> String {
    match selectors.iter().find(|s| s.contains('/')) {
        Some(s) => format!("\n（\"{s}\" 看起来像路径的一部分：路径里有空格时要整个加引号）"),
        None => String::new(),
    }
}
