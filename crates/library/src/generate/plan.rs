//! 生成计划与指纹（拆分自 `generate.rs`）：按 profile、内容、元数据补充、竖排等算出指纹。

use super::types::{Plan, PIPELINE_VERSION};
use crate::{Library, Meta};
use profile::{Format, Profile};

impl Library {
    pub(super) fn plan(&self, meta: &Meta, device: &Profile) -> Result<Plan, String> {
        if !meta.supported() {
            return Err(crate::unsupported(meta.content_format()));
        }
        if meta.content_sha().is_empty() {
            return Err("条目缺内容哈希（早期版本入库），先运行 booklib dedupe 迁移".into());
        }
        let format = self.output_format(meta, device)?;
        // 优化器用的阅读范围（`OptimizeOpts::for_profile` 取 `output_readable`，漫画另配格式时也是它），不是这本书产物格式的；
        // 内置模式两者相同
        let area = device.output_readable();
        // KFX 再带上写出器的版本（写出器改了也要重建）和写出器求 `@media` 用的屏幕、KFX 阅读范围（`kfx::css::MediaEnv::for_profile`；
        // 书库 profiles/ 里的自定义模式改了屏幕也要重建，2026-10-09 审计补上）
        let format_seg = match format {
            Format::Epub => format.name().to_string(),
            // KEPUB：写出器版本（`bookconv::kepub::VERSION`）；不按屏幕求值，屏幕变了由优化器那段（阅读范围）管
            Format::Kepub => format!("{}{}", format.name(), bookconv::kepub::VERSION),
            Format::Kfx => {
                let r = device.readable(Format::Kfx);
                format!(
                    "{}{}@{}x{}r{}x{}",
                    format.name(),
                    kfx::write::WRITER_VERSION,
                    device.screen.width,
                    device.screen.height,
                    r.width,
                    r.height
                )
            }
        };
        let cover = meta
            .cover
            .as_ref()
            .map_or("-", |c| c.sha256.get(..12).unwrap_or(&c.sha256));
        // 先算是不是漫画：早期条目 `Meta::comic` 可能还没存，按书定方向要和指纹用同一个结果
        let comic = self.comic_of(meta)?;
        let info = self.injected_info_sig(meta).unwrap_or_else(|| "-".into());
        // 补元数据那一步（`metadata::inject`）改了会影响产物时 `bookconv::opfmeta::VERSION` 加一：只让补过东西的书过期（`i5`），
        // 没补过东西的书指纹不变
        let info = if cover != "-" || info != "-" {
            format!("{info}i{}", bookconv::opfmeta::VERSION)
        } else {
            info
        };
        // 按元数据强制成 rtl 的书（Bangumi 日本漫画，`metadata::comic_direction`）在 info 段后接 `r`：
        // 只有这些书指纹变，别的书一个字不变；方向不算 profile 字段，不放进设备段（不然全部漫画都重建）
        let info = if crate::metadata::comic_direction(meta, device, comic).is_some() {
            format!("{info}r")
        } else {
            info
        };
        // 最终竖排的书在 info 段后接 `V`（2026-11-11）；全角 ASCII（`vertical_ascii = fullwidth`）再接 `f`。
        // 不竖排的书不出新字母，指纹除版本号外不变。
        let info = if self.vertical_for(meta, device)? {
            let mut s = format!("{info}V");
            if device.vertical_ascii == profile::VerticalAscii::Fullwidth {
                s.push('f');
            }
            s
        } else {
            info
        };
        // 要当场转换的来源（CBZ）再带上格式转换的版本；写在流程版本后面，EPUB 来源的指纹保持原样（不白重建）
        let pipeline = if meta.content_format() == "epub" {
            PIPELINE_VERSION.to_string()
        } else {
            format!("{PIPELINE_VERSION}c{}", bookconv::convert::CONVERT_VERSION)
        };
        let crate::fingerprint::DeviceSig {
            version,
            area_seg,
            notes,
        } = crate::fingerprint::device_sig(device, area, comic);
        let fingerprint = format!(
            "{}|{cover}|{info}|{pipeline}|{version}|{notes}|{}|{area_seg}|{}|{}",
            meta.content_sha(),
            device.id,
            if device.color { "color" } else { "gray" },
            format_seg,
        );
        // 以前的写法：一个版本号、profile 的字段全带上。版本号用这一路现在的：这一路的版本变过，就对不上，照常重建
        let legacy = {
            let seg = crate::fingerprint::legacy_device_seg(device, area);
            let v = if comic {
                bookconv::optimize::COMIC_VERSION
            } else {
                bookconv::optimize::OPTIMIZE_VERSION
            };
            let gray = if device.color { "color" } else { "gray" };
            format!(
                "{}|{cover}|{info}|{pipeline}|{v}|{notes}|{}|{}x{}+{}{seg}|{gray}|{format_seg}",
                meta.content_sha(),
                device.id,
                area.width,
                area.height,
                device.comic.margin
            )
        };
        Ok(Plan {
            fingerprint,
            legacy,
            format,
        })
    }

    /// 这本书在这个模式下的产物格式：profile 给漫画另配了格式（`comic_format`）时要先判断是不是漫画。
    pub(crate) fn output_format(&self, meta: &Meta, device: &Profile) -> Result<Format, String> {
        if device.format_for(true) == device.format_for(false) {
            return Ok(device.format());
        }
        Ok(device.format_for(self.comic_of(meta)?))
    }

    /// 这本书是不是漫画（同优化器的判定）：入库时存下的（`Meta::comic`）；早期条目当场判、按内容哈希缓存在这次运行里，
    /// 持锁时顺带存进 meta.json（只读的命令不写书库）。
    pub(crate) fn comic_of(&self, meta: &Meta) -> Result<bool, String> {
        if let Some(c) = meta.comic {
            return Ok(c);
        }
        let sha = meta.content_sha().to_string();
        if let Some(c) = self.facts.comic(&sha) {
            return Ok(c);
        }
        let c = self.is_comic(meta)?;
        self.facts.note_comic(&sha, c);
        if self.records.locked.get() {
            if let Some(mut m) = self
                .read_meta(&meta.id)
                .filter(|m| m.content_sha() == meta.content_sha())
            {
                m.comic = Some(c);
                self.save_meta(&m)?;
            }
        }
        Ok(c)
    }

    /// 生成时真正会补进书里的东西（[`crate::metadata::plan`]：简介、标签、带国籍的作者、译者、出版社、日期、ISBN）的指纹；
    /// 什么都不补时 `None`。书里已有什么见 [`Library::own_dc_of`]；不知道（原件暂时不在）、CBZ（当场转换）时按书里只有作者算。
    pub(super) fn injected_info_sig(&self, meta: &Meta) -> Option<String> {
        crate::metadata::plan_sig(&crate::metadata::plan(meta, self.own_dc_of(meta).as_ref()))
    }

    /// 书里自己的元数据：入库时存进 meta.json 的（`Meta::own_dc`，原件暂时不在也知道）。早期条目没存或是旧版记录：EPUB 当场读 OPF
    /// （按内容哈希缓存），持锁时顺带存进 meta.json（只读的命令不写书库）；读不出来（原件不在了）、不是 EPUB 返回 `None`。
    pub(crate) fn own_dc_of(&self, meta: &Meta) -> Option<crate::OwnDc> {
        if let Some(o) = meta
            .own_dc
            .as_ref()
            .filter(|o| o.v >= crate::OwnDc::VERSION)
        {
            return Some(o.clone());
        }
        if meta.content_format() != "epub" {
            return None;
        }
        let sha = meta.content_sha();
        if let Some(o) = self.facts.own_dc(sha) {
            return Some(o);
        }
        // 读不出来（原件暂时不在）：旧版记录里的简介、标签照用，别的按不知道
        let Ok(own) = crate::metadata::read_own(&self.content_location(meta)) else {
            return meta.own_dc.clone();
        };
        // 原件动过（可能不是这个内容了）的不记：生成前会核对，核对过再算
        if self.original_state(meta) != crate::OriginalState::Touched {
            self.facts.note_own_dc(sha, own.clone());
            if self.records.locked.get() {
                if let Some(mut m) = self.read_meta(&meta.id).filter(|m| {
                    m.content_sha() == sha
                        && m.own_dc
                            .as_ref()
                            .is_none_or(|o| o.v < crate::OwnDc::VERSION)
                }) {
                    m.own_dc = Some(own.clone());
                    let _ = self.save_meta(&m); // 写不回也不影响这次，下次再存
                }
            }
        }
        Some(own)
    }

    /// 这本书给该模式生成的话，产物的指纹（`sync --watch` 用它判断上次失败以后有没有变化）。
    pub fn fingerprint(&self, meta: &Meta, device: &Profile) -> Result<String, String> {
        self.plan(meta, device).map(|p| p.fingerprint)
    }
}
