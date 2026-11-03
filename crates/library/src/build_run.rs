//! 一轮生成：按书 × 模式逐本比较、生成、交给传输线程送到设备上（`sync` 的第二步）。
//!
//! 调度在这里（设备没接上的跳过、排满的设备推迟、`--watch` 跨轮记住失败），结果一件一件报成 [`BuildEvent`]；
//! 怎么显示（措辞、`quiet` 时哪些不报）由命令行决定。

use crate::{Built, Event, Library, Meta, OriginalState, Pipeline, Profile, Step};
use std::collections::{HashMap, HashSet};

/// `sync --watch` 跨轮记住的生成失败：(书 id, 模式 id) → (失败时的指纹, 原件状态)。都没变就不重试。
pub type FailMemo = HashMap<(String, String), (String, OriginalState)>;

/// 一轮生成要的东西。
pub struct BuildRun<'a> {
    pub lib: &'a Library,
    pub devices: &'a [&'a Profile],
    pub books: &'a [Meta],
    pub force: bool,
    /// `--keep`：不在这台跟踪范围里的书只报告（[`BuildEvent::Outside`]），不从设备上删。
    pub keep: bool,
    /// 跨轮记住的生成失败（[`FailMemo`]）。不是 `--watch` 时 `None`。
    pub fails: Option<&'a mut FailMemo>,
    /// 报过的不再支持的格式（早期版本收的 MOBI/PDF 等）的书：每本只报一次。
    pub skipped: &'a mut HashSet<String>,
    /// 报过"没接上"的设备（`--watch` 时不重复报）。
    pub absent_reported: &'a mut HashSet<String>,
}

/// 一轮生成里发生的一件事。
pub enum BuildEvent {
    /// 不再支持的格式，跳过（每本只报一次）。
    Unsupported {
        id: String,
        title: String,
        reason: String,
    },
    /// 接上的设备上删掉了以前删书时没删成的书。
    RemovedFlushed {
        device: String,
        count: usize,
    },
    RemoveFlushFailed {
        device: String,
        error: String,
    },
    /// 设备没接上，这一轮不传（`--watch` 时每台只报一次）。
    DeviceAbsent {
        device: String,
        reason: String,
    },
    /// 不再给这台设备的书（从它的跟踪范围里拿掉了）：从设备上删掉了。
    Retracted {
        device: String,
        title: String,
    },
    /// 不在这台设备的跟踪范围里、设备上还有，`--keep` 这一次没删。
    Outside {
        device: String,
        title: String,
    },
    RetractFailed {
        device: String,
        title: String,
        error: String,
    },
    /// 一本书一个模式的结果。
    Built {
        device: String,
        title: String,
        built: Built,
    },
    Failed {
        device: String,
        title: String,
        error: String,
    },
    /// 传输线程意外退出，这么多本没传成。
    TransportLost {
        device: String,
        count: usize,
    },
    /// 传之前看过：设备放不下这一轮要传的（连余量要 `need`、剩 `free`、差 `short` 字节），这一轮不往这台写。
    NoSpace {
        device: String,
        count: usize,
        need: u64,
        free: u64,
        short: u64,
    },
    /// 读不到设备剩余空间（jmtpfs 可能报 0）：只警告，照旧传。
    SpaceUnknown {
        device: String,
        reason: String,
    },
    /// 写的途中设备满了：这一轮不再往这台传（写了一半的已删掉，下一轮、下次同步重传）。
    DeviceFull {
        device: String,
        title: String,
        error: String,
    },
    /// 都生成好了，还在等设备那边传完。
    Waiting {
        in_flight: usize,
    },
}

/// 一轮的结果计数。
#[derive(Default, Debug, Clone, PartialEq, Eq)]
pub struct BuildCounts {
    pub written: usize,
    pub same: usize,
    pub adopted: usize,
    pub moved: usize,
    pub up_to_date: usize,
    /// 不在跟踪范围、从设备上删掉的。
    pub retracted: usize,
    pub failed: usize,
    /// 这一轮传的途中断开了的设备（模式 id）：开头接着、中途掉了。`sync --watch` 把它们当成上一轮没接上，
    /// 下一轮接着的话忘掉记着的失败、重新传（断线的失败和书无关，不该一直不重试）。
    pub dropped: Vec<String>,
    /// 这一轮空间不够（传之前看出来的，或写的途中满了）的设备：不记进失败表，`sync --watch` 下一轮再看。
    pub no_space: Vec<String>,
}

impl BuildCounts {
    /// 有没有生成、挪动、失败（`--watch` 时都没有就不出声）。
    pub fn any(&self) -> bool {
        self.written + self.same + self.adopted + self.moved + self.retracted + self.failed > 0
    }

    fn count(&mut self, b: &Built) {
        match b {
            Built::Written { .. } => self.written += 1,
            Built::Same { .. } => self.same += 1,
            Built::UpToDate(_) => self.up_to_date += 1,
            Built::Adopted(_) => self.adopted += 1,
            Built::Moved { .. } => self.moved += 1,
        }
    }
}

/// 按模式 × 书逐本生成并送到设备上（没变化的跳过）。设备没接上的模式跳过。
pub fn build_all(run: BuildRun, report: &mut impl FnMut(BuildEvent)) -> BuildCounts {
    let BuildRun {
        lib,
        devices,
        books,
        force,
        keep,
        mut fails,
        skipped,
        absent_reported,
    } = run;
    let mut counts = BuildCounts::default();
    for m in books.iter().filter(|m| !m.supported()) {
        if skipped.insert(m.id.clone()) {
            report(BuildEvent::Unsupported {
                id: m.id.clone(),
                title: m.title.clone(),
                reason: crate::unsupported(m.content_format()),
            });
        }
    }
    // 产物直接送设备（profile 的 [deliver]）：没接上的设备这一轮整个跳过（每轮报一次）；以前删书时没删成的等这一轮生成完再删（见末尾）
    let mut live: Vec<&Profile> = Vec::new();
    for device in devices {
        match lib.device_status(device) {
            Ok(_) => live.push(device),
            Err(reason) => {
                // --watch（给了 fails）时同一台没接上只报一次
                if fails.is_none() || !absent_reported.contains(&device.id) {
                    report(BuildEvent::DeviceAbsent {
                        device: device.id.clone(),
                        reason,
                    });
                }
                absent_reported.insert(device.id.clone());
            }
        }
    }
    // 不在这台设备的跟踪范围里的书：以前传过的从设备上删掉（2026-10-19 定），这一轮也不生成
    let mut outside: HashSet<(&str, &str)> = HashSet::new();
    for m in books {
        for device in &live {
            if lib.wants(m, &device.id) {
                continue;
            }
            outside.insert((m.id.as_str(), device.id.as_str()));
            if !lib.has_output(&m.id, &device.id) {
                continue;
            }
            if keep {
                report(BuildEvent::Outside {
                    device: device.id.clone(),
                    title: m.title.clone(),
                });
                continue;
            }
            match lib.retract(&m.id, device) {
                Ok(_) => {
                    counts.retracted += 1;
                    report(BuildEvent::Retracted {
                        device: device.id.clone(),
                        title: m.title.clone(),
                    });
                }
                Err(error) => {
                    counts.failed += 1;
                    report(BuildEvent::RetractFailed {
                        device: device.id.clone(),
                        title: m.title.clone(),
                        error,
                    });
                }
            }
        }
    }
    // 传之前看放不放得下（2026-11-03 定）：删完不在跟踪范围的（腾出了空间）再看；不够的这一轮不往那台写，别的设备照旧。
    // --watch 时同一台同样的情况只报一次（键放在 absent_reported 里，"空间:"/"空间?:" 开头，不和设备 id 混）
    for device in &live {
        let todo: Vec<&Meta> = books
            .iter()
            .filter(|m| m.supported() && !outside.contains(&(m.id.as_str(), device.id.as_str())))
            .filter(|m| {
                fails
                    .as_deref()
                    .is_none_or(|f| !f.contains_key(&(m.id.clone(), device.id.clone())))
            })
            .collect();
        let (short_key, unknown_key) = (
            format!("空间:{}", device.id),
            format!("空间?:{}", device.id),
        );
        match lib.space_check(device, &todo, force) {
            crate::SpaceCheck::Short {
                count,
                need,
                free,
                short,
            } => {
                if fails.is_none() || absent_reported.insert(short_key) {
                    report(BuildEvent::NoSpace {
                        device: device.id.clone(),
                        count,
                        need,
                        free,
                        short,
                    });
                }
                counts.no_space.push(device.id.clone());
            }
            crate::SpaceCheck::Unknown(reason) => {
                absent_reported.remove(&short_key);
                if fails.is_none() || absent_reported.insert(unknown_key) {
                    report(BuildEvent::SpaceUnknown {
                        device: device.id.clone(),
                        reason,
                    });
                }
            }
            crate::SpaceCheck::Enough | crate::SpaceCheck::Skip => {
                absent_reported.remove(&short_key);
                absent_reported.remove(&unknown_key);
            }
        }
    }
    // 不够的设备记在 counts.no_space 里，`one` 跳过它（末尾删已从书库删除的书照旧做，能腾出空间）
    // 先比较、再生成（主线程）；要传的交给各设备的传输线程，和生成同时进行；传完的随时取回来记下、报一件
    let mut pipe = Pipeline::default();
    let landed = |counts: &mut BuildCounts,
                  fails: &mut Option<&mut FailMemo>,
                  report: &mut dyn FnMut(BuildEvent),
                  ev: Event| {
        let (t, r) = match ev {
            Event::Done(t, r) => (t, r),
            Event::Lost { device, count } => {
                counts.failed += count;
                if !counts.dropped.contains(&device) {
                    counts.dropped.push(device.clone());
                }
                return report(BuildEvent::TransportLost { device, count });
            }
        };
        let (device, title) = (t.device.clone(), t.title.clone());
        let book = t.book_id().to_string();
        // 传失败、挂载点也不挂着了：设备中途断开
        let lost = r.is_err()
            && t.work
                .mount
                .as_deref()
                .is_some_and(|m| !crate::deliver::mounted(m));
        if lost && !counts.dropped.contains(&device) {
            counts.dropped.push(device.clone());
        }
        match lib.complete(*t, r) {
            Ok(built) => {
                counts.count(&built);
                report(BuildEvent::Built {
                    device,
                    title,
                    built,
                });
            }
            // 写的途中设备满了：停下这台（后面的书不再生成、传），不记进失败表、不算失败；同一台只报第一本
            Err(error) if error.starts_with(crate::space::NO_SPACE) => {
                if !counts.no_space.contains(&device) {
                    counts.no_space.push(device.clone());
                    report(BuildEvent::DeviceFull {
                        device,
                        title,
                        error,
                    });
                }
            }
            Err(error) => {
                counts.failed += 1;
                if let (Some(f), Some(m), Some(p)) = (
                    fails.as_deref_mut(),
                    books.iter().find(|m| m.id == book),
                    lib.devices().get(&device),
                ) {
                    f.insert(
                        (book, device.clone()),
                        (
                            lib.fingerprint(m, p).unwrap_or_else(|e| e),
                            lib.original_state(m),
                        ),
                    );
                }
                report(BuildEvent::Failed {
                    device,
                    title,
                    error,
                });
            }
        }
    };
    // 一本书一个模式：比较、生成，要传的交给传输线程
    let one = |m: &Meta,
               device: &Profile,
               counts: &mut BuildCounts,
               fails: &mut Option<&mut FailMemo>,
               pipe: &mut Pipeline,
               report: &mut dyn FnMut(BuildEvent)| {
        let key = (m.id.clone(), device.id.clone());
        let now = || {
            (
                lib.fingerprint(m, device).unwrap_or_else(|e| e),
                lib.original_state(m),
            )
        };
        // 这一轮里已经掉线的设备：后面的书不再生成（白跑，下一轮接上再来）
        if counts.dropped.contains(&device.id) {
            counts.failed += 1;
            return;
        }
        // 这一轮写满了的设备：后面的书不再生成（下一轮、下次同步再来）
        if counts.no_space.contains(&device.id) {
            return;
        }
        if let Some(f) = fails.as_deref_mut() {
            match f.get(&key).map(|v| *v == now()) {
                Some(true) => return,
                Some(false) => {
                    f.remove(&key);
                }
                None => {}
            }
        }
        match lib.prepare(m, device, force) {
            Ok(Step::Done(built)) => {
                counts.count(&built);
                report(BuildEvent::Built {
                    device: device.id.clone(),
                    title: m.title.clone(),
                    built,
                });
            }
            Ok(Step::Transfer(t)) => pipe.submit(*t),
            Err(error) => {
                counts.failed += 1;
                // 生成时发现设备掉线：记成掉线（下一轮接上重传），不记进失败表
                if error.starts_with(crate::generate::UNMOUNTED) {
                    if !counts.dropped.contains(&device.id) {
                        counts.dropped.push(device.id.clone());
                    }
                    return report(BuildEvent::Failed {
                        device: device.id.clone(),
                        title: m.title.clone(),
                        error,
                    });
                }
                report(BuildEvent::Failed {
                    device: device.id.clone(),
                    title: m.title.clone(),
                    error,
                });
                if let Some(f) = fails.as_deref_mut() {
                    f.insert(key, now());
                }
            }
        }
    };
    // 书在外层、模式在内层：同一本书与模式无关的中间文件（CBZ 转换、补元数据）只做一次。
    // 哪台设备那边排满了（拷大漫画慢），先跳过它做别的设备，留到最后补，不让一台挡住另外几台
    // 推迟的书的中间文件留着（按书 id，`Library::release_prepared_of`），补的时候不用再转一遍；这本书所有模式做完就删
    let mut later: Vec<(&Meta, &Profile)> = Vec::new();
    let mut deferred: HashMap<&str, usize> = HashMap::new();
    for m in books.iter().filter(|m| m.supported()) {
        for device in live
            .iter()
            .filter(|d| !outside.contains(&(m.id.as_str(), d.id.as_str())))
        {
            while let Some(x) = pipe.next(false) {
                landed(&mut counts, &mut fails, report, x);
            }
            if pipe.has_room(&device.id) {
                one(m, device, &mut counts, &mut fails, &mut pipe, report);
            } else {
                later.push((m, device));
                *deferred.entry(m.id.as_str()).or_default() += 1;
            }
        }
        if !deferred.contains_key(m.id.as_str()) {
            lib.release_prepared_of(&m.id);
        }
    }
    for (m, device) in later {
        while !pipe.has_room(&device.id) {
            let Some(x) = pipe.next(true) else { break };
            landed(&mut counts, &mut fails, report, x);
        }
        one(m, device, &mut counts, &mut fails, &mut pipe, report);
        if let Some(n) = deferred.get_mut(m.id.as_str()) {
            *n -= 1;
            if *n == 0 {
                lib.release_prepared_of(&m.id);
            }
        }
    }
    lib.release_prepared();
    if pipe.in_flight() > 0 {
        report(BuildEvent::Waiting {
            in_flight: pipe.in_flight(),
        });
    }
    while let Some(x) = pipe.next(true) {
        landed(&mut counts, &mut fails, report, x);
    }
    // 以前删书时设备没接上、没删成的：放到最后删。删了又加回来的书这一轮先认领了设备上那份（file_name_for 逐字节相同就认），
    // 记录里有了就不删；放在开头删的话会先删掉设备上完好的书、再整本重传（Kindle 的进度也跟着丢）
    for device in &live {
        match lib.flush_removed(device) {
            Ok(0) => {}
            Ok(count) => report(BuildEvent::RemovedFlushed {
                device: device.id.clone(),
                count,
            }),
            Err(error) => report(BuildEvent::RemoveFlushFailed {
                device: device.id.clone(),
                error,
            }),
        }
    }
    counts
}
