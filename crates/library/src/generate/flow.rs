//! 生成与传输主流程（拆分自 `generate.rs`）：先生成、再交给传输线程、传完记录。

use super::types::{Built, Made, Plan, RecordState, StateEntry, Step, UNMOUNTED};
use crate::deliver::{self, Target};
use crate::error::{Error, Kind};
use crate::fsutil::TMP_PREFIX;
use crate::transfer::{Transfer, Work};
use crate::{Library, Meta};
use profile::{Format, Profile};
use std::path::PathBuf;

impl Library {
    /// 为一个阅读模式生成一本书的产物并送到设备上，做完才返回（没变化就跳过，`force` 强制重建）。放在哪见模块注释。
    /// 设备没接上返回错误（`sync` 先用 [`Library::device_status`] 看过，没接上的模式整个跳过）。
    /// `sync` 不用它：先比较、生成（[`Library::prepare`]），传设备交给传输线程（[`crate::Pipeline`]），传完再记（[`Library::complete`]）。
    pub fn build(&self, meta: &Meta, device: &Profile, force: bool) -> Result<Built, String> {
        self.build_typed(meta, device, force)
            .map_err(|e| e.to_string())
    }

    fn build_typed(&self, meta: &Meta, device: &Profile, force: bool) -> Result<Built, Error> {
        match self.prepare_typed(meta, device, force)? {
            Step::Done(b) => Ok(b),
            Step::Transfer(t) => {
                let r = t.run();
                self.complete_typed(*t, r)
            }
        }
    }

    /// 先比较、再生成：没变化的、设备上已有一样的直接给出结果（[`Step::Done`]）；要传的生成好、交回一件 [`Transfer`]
    /// （放上设备是别的线程的事，传完调 [`Library::complete`] 记下来）。
    pub fn prepare(&self, meta: &Meta, device: &Profile, force: bool) -> Result<Step, String> {
        self.prepare_typed(meta, device, force)
            .map_err(|e| e.to_string())
    }

    /// [`Library::prepare`] 的内部版：错误带着种类，`build_run` 按种类决定掉线还是失败。
    pub(crate) fn prepare_typed(
        &self,
        meta: &Meta,
        device: &Profile,
        force: bool,
    ) -> Result<Step, Error> {
        // 原件的大小或修改时间变了：先核对内容（改过了就停下来），不能因为指纹没变就说"已是最新"。
        // 原件不在了的不在这里报：产物还是那本书的，照旧算最新（`sync`/`list` 会报原件不在）
        if self.original_state(meta) == crate::OriginalState::Touched {
            self.verified_original(meta)?;
        }
        let plan = self.plan(meta, device)?;
        self.rebase_state_case(device)?;
        // 指纹写法改了（2026-10-08 起文字书、漫画分开算）、规则没变的：记录改成新写法，不重新生成
        let sp = self.state_path(&device.id);
        let prev = self.records.states.get(&sp).books.get(&meta.id).cloned();
        if let Some(mut p) = prev.filter(|p| {
            !p.fingerprint.is_empty()
                && p.fingerprint != plan.fingerprint
                && p.fingerprint == plan.legacy
        }) {
            p.fingerprint = plan.fingerprint.clone();
            self.put_entry(&sp, &meta.id, p)?;
        }
        let t = self.target(device);
        let target = t
            .as_ref()
            .as_ref()
            .map_err(|e| format!("{} {e}", device.id))?;
        self.prepare_file(meta, device, target, plan, force)
    }
    /// 生成产物，放在书库临时目录 `.tmp-<id>-<模式>/` 里。失败时临时目录删掉。
    fn product(&self, meta: &Meta, device: &Profile, format: Format) -> Result<Made, String> {
        let tmp = self
            .root
            .join(format!("{TMP_PREFIX}{}-{}", meta.id, device.id));
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).map_err(|e| format!("{}: {e}", tmp.display()))?;
        let src = tmp.join(format!("out.{}", format.ext()));
        // KFX 在内存里就算好了哈希；EPUB 是优化器流式写的，写完再读一遍（刚写的，多半还在页缓存里）
        let made = self
            .produce(meta, device, format, &src, &tmp)
            .and_then(|(w, sha, a)| {
                Ok((
                    sha.map_or_else(|| crate::fsutil::sha256_file(&src), Ok)?,
                    w,
                    a,
                ))
            });
        match made {
            Ok((sha, warnings, audit)) => Ok(Made {
                tmp: Some(tmp),
                src,
                sha,
                warnings,
                audit,
            }),
            Err(e) => {
                let _ = std::fs::remove_dir_all(&tmp);
                Err(e)
            }
        }
    }

    /// 要放上设备的那份：`reuse` 是内容没变、只是位置变了（或以前放在电脑上）的已有产物，直接用它、不重新生成
    /// （记录里有哈希就用，没有就算）；没有就新生成（[`Library::product`]）。
    fn made(
        &self,
        meta: &Meta,
        device: &Profile,
        format: Format,
        reuse: Option<&StateEntry>,
    ) -> Result<Made, String> {
        match reuse {
            Some(p) => Ok(Made {
                tmp: None,
                src: p.path.clone(),
                sha: if p.sha.is_empty() {
                    crate::fsutil::sha256_file(&p.path)?
                } else {
                    p.sha.clone()
                },
                warnings: Vec::new(),
                audit: p.audit.clone(),
            }),
            None => self.product(meta, device, format),
        }
    }
    /// 产物是文件（电脑上、MTP 设备上）。
    fn prepare_file(
        &self,
        meta: &Meta,
        device: &Profile,
        target: &Target,
        plan: Plan,
        force: bool,
    ) -> Result<Step, Error> {
        let Plan {
            fingerprint,
            format,
            ..
        } = plan;
        let sp = self.state_path(&device.id);
        let slot = self.resolve_placement(meta, device, target, &sp, format.ext())?;
        let staged = matches!(target, Target::Dir { staged: true, .. });
        let unchanged = slot
            .prev
            .as_ref()
            .filter(|p| !force && p.fingerprint == fingerprint);
        let record = RecordState {
            done: unchanged.filter(|p| p.path.is_file()),
            unchanged,
        };
        if let Some(built) = self.settle_fresh(meta, &sp, &slot, &fingerprint, record, staged)? {
            return Ok(Step::Done(built));
        }
        // 设备中途断开了：不生成、不改记录（不然下面会把完好的指纹、哈希清掉，接上后整本重传），报给 `build_all` 记成掉线
        if let Target::Dir { mount: Some(m), .. } = target {
            if !deliver::mounted(m) {
                return Err(Error::new(
                    Kind::Unmounted,
                    format!("{UNMOUNTED}：{}", m.display()),
                ));
            }
        }
        // 来源：内容没变、只是位置变了的已有产物（挪过去，不重新生成），或者新生成的
        let made = self.made(meta, device, format, record.done)?;
        let tmp = made.tmp.clone();
        // MTP 设备上直接写正式文件名（不经临时文件，[`direct_write`]）：写到一半被打断，正式文件名上会留下半截，所以要先登记一条指纹留空的记录，
        // 下次认得出这个位置没传完、重新传（见 `Work`）
        let direct = direct_write(target);
        let mount = match target {
            Target::Dir { mount, .. } => mount.clone(),
            _ => None,
        };
        let r = (|| {
            // 目录里已有一个不认识的同名文件、和这份逐字节相同（以前手工拷上去的）：认领它，不另起名字
            let out = slot.dir.join(self.records.states.get(&sp).file_name_for(
                meta,
                &slot.dir,
                format.ext(),
                Some(&made.src),
                slot.copied.as_deref(),
                &slot.taken,
            )?);
            let (mut entry, same) =
                self.stage_registration(meta, &sp, &slot, &out, &made, direct)?;
            // 记录里没有哈希（不是我们传的、或旧记录）时才把设备上的读回来比
            let compare = entry.sha.is_empty();
            entry.fingerprint = fingerprint.clone();
            let t = self.transfer(
                meta,
                device,
                sp.clone(),
                made,
                record.done.map(|p| p.path.clone()),
                entry,
                Work {
                    out,
                    compare,
                    direct,
                    mount,
                },
                staged,
            );
            if same {
                // 和设备上的一样：不用传
                return self.complete_typed(t, Ok(false)).map(Step::Done);
            }
            Ok(Step::Transfer(Box::new(t)))
        })();
        if r.is_err() {
            if let Some(t) = &tmp {
                let _ = std::fs::remove_dir_all(t);
            }
        }
        r
    }

    /// 交给传输线程的一件。
    #[allow(clippy::too_many_arguments)]
    fn transfer(
        &self,
        meta: &Meta,
        device: &Profile,
        sp: PathBuf,
        made: Made,
        from: Option<PathBuf>,
        entry: StateEntry,
        work: Work,
        staged: bool,
    ) -> Transfer {
        let Made {
            tmp,
            src,
            sha,
            warnings,
            audit,
        } = made;
        let mut entry = entry;
        entry.audit = audit;
        Transfer {
            device: device.id.clone(),
            title: meta.title.clone(),
            book: meta.id.clone(),
            sp,
            tmp,
            src,
            sha,
            warnings,
            from,
            entry,
            work,
            staged,
        }
    }

    /// 传完了（`r` 是传输线程的结果：真的写了没有）：删掉临时目录，写上完成的记录、删掉旧位置。传失败的：记录不改
    /// （先登记了新位置、指纹留空，下次重做），返回错误。
    pub fn complete(&self, t: Transfer, r: Result<bool, Error>) -> Result<Built, String> {
        self.complete_typed(t, r).map_err(|e| e.to_string())
    }

    /// [`Library::complete`] 的内部版：传设备的错误带着种类，`build_run` 按种类决定停不停这台设备。
    pub(crate) fn complete_typed(
        &self,
        t: Transfer,
        r: Result<bool, Error>,
    ) -> Result<Built, Error> {
        if let Some(tmp) = &t.tmp {
            let _ = std::fs::remove_dir_all(tmp);
        }
        // 传完（或失败）：占着的文件名放开（成功的接着就记进生成记录）
        self.build.unreserve(&t.work.out);
        let wrote = r?;
        let meta_id = t.book.clone();
        let staged = t.staged;
        let mut entry = t.entry;
        entry.sha = t.sha;
        let path = entry.path.clone();
        self.finish(&t.sp, &meta_id, entry, staged)?;
        Ok(match t.from {
            Some(from) => Built::Moved { from, to: path },
            None if wrote => Built::Written {
                path,
                warnings: t.warnings,
            },
            None => Built::Same {
                path,
                warnings: t.warnings,
            },
        })
    }
    /// 删掉这本书留着给别的模式用的中间文件（这本书所有模式都做完了时调）。
    pub fn release_prepared_of(&self, book: &str) {
        self.build.release_prepared_of(book);
    }

    /// 删掉所有留着的中间文件（一轮生成结束时调）。
    pub fn release_prepared(&self) {
        self.build.release_prepared();
    }
}
/// 设备上直接写正式文件名（不经临时文件）：只在 MTP 上（jmtpfs 改名等于下载再上传一遍）。USB 大容量存储（Kobo）
/// 同目录改名是原子的，经临时文件：直写的话更新已有的书要先删旧的，写到一半拔线就丢了旧书、留下 Nickel 会去导入的半截。
pub(super) fn direct_write(target: &Target) -> bool {
    matches!(target, Target::Dir { staged: false, .. })
}
