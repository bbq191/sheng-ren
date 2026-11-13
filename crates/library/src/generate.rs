//! 按阅读模式（设备 profile）生成产物并送到设备上：生成计划与指纹、产物放在哪、跳过没变的、生成记录。
//!
//! 产物位置（每个模式一份，`<名>` 见 [`State::file_name_for`]；2026-10-07 起直接送设备，电脑上不留，见 [`crate::deliver`]）：
//! - Kindle、掌阅（profile `[deliver] kind = "mtp"`）：设备存储根目录的 `documents/<原件所在目录相对跟踪目录的路径>/<名>.<扩展名>`；
//!   `add` 进来的单个文件（不在跟踪目录里）、网址书放 `documents/` 顶层。先在书库的临时目录里生成，再拷上去
//!   （设备上已有逐字节相同的就不拷：Kindle 覆盖成不同字节会清掉阅读进度）。
//! - Kobo（`kind = "usbms"`）：同上，放在 USB 存储根目录的 `books/` 下（2026-11-01 加，未上真机）。
//! - 没写 `[deliver]` 的自定义模式：电脑上，跟踪目录 `D` 旁的 `D/../<模式 id>/<子目录>/`；add 进来的、网址书在 `<书库>/output/<模式 id>/`。
//!
//! 设备没接上的模式不生成（`sync` 跳过，下次接上再传）。
//!
//! 产物格式按模式：EPUB 直接是优化结果；KFX（Kindle）是同一份优化结果再转一次（`kfx` crate）。
//! profile 可以给漫画另配格式（`comic_format`，见 [`Library::output_format`]）；内置模式都不用（kindle 文字书、漫画都出 KFX）。
//!
//! 生成记录 `<书库>/output-state/<模式 id>.json`：书 id → 产物绝对路径、产物根目录、指纹
//! （2026-10-31 以前 Move 上的记的是 uuid 和相对的 `文件夹/文件名`：旧记录里这种相对路径删书时一概不动）。
//! 产物位置变了（原件移动、改名换了目录，书名改了）时删掉旧位置的——**只删记录里记着的**，不认识的文件一概不动；删完顺带删掉
//! 产物根目录以内变空的目录。内容没变、只是位置变了的，把已有产物挪过去，不重新生成（改成送设备以前留在电脑上的产物也这样挪上设备）。
//! 删书时设备没接上、删不掉的记在 `orphans`，接上后删。
//!
//! 换位置时先把记录改成新位置（指纹留空＝没完成，旧位置记进 `old` 待删），产物放好再补上指纹、删旧的：
//! 中途被打断的话，下次生成还认得新位置上的文件是这本书的（不会因为"有个不认识的同名文件"而改名），旧文件也还会删。
//!
//! 本文件只留入口与再导出，各段拆在 `generate/` 下：`plan`（指纹与计划）、`placement`（产物位置、认领）、
//! `flow`（生成与传输主流程）、`produce`（产出 EPUB/KFX）、`records`（生成记录的读写与删除）、
//! `state`（记录结构与位置改写）、`space`（设备空间）、`fsops`（文件操作小工具）、`types`（各段共用的类型）。

mod flow;
mod fsops;
mod placement;
mod plan;
mod produce;
mod records;
mod space;
mod state;
mod types;

#[cfg(test)]
mod tests;

pub(crate) use fsops::{case_alias, case_twin};
pub(crate) use placement::output_overlap;
pub(crate) use types::{State, StateEntry, PIPELINE_VERSION};
pub use types::{Built, OutputStatus, SpaceCheck, Step};
