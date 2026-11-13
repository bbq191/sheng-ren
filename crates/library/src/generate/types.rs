//! 生成流程各段共用的类型（拆分自 `generate.rs`）：生成结果、生成计划、产物位置、生成记录。

use crate::transfer::Transfer;
use profile::Format;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;

/// 生成流程本身（本 crate 的步骤、参数，以及生成时当场做的格式转换）的版本：改了会影响产物的地方要加一，
/// 旧产物随之判为过期。优化器、格式转换各有自己的版本号，也都进指纹。产物放在哪不影响产物内容，不进指纹。
pub(crate) const PIPELINE_VERSION: &str = "5";

#[derive(Debug)]
pub enum Built {
    Written {
        path: PathBuf,
        warnings: Vec<String>,
    },
    UpToDate(PathBuf),
    /// 内容没变、只是位置变了（原件移动改名、书名改了）：已有产物挪了过去，没有重新生成。
    Moved {
        from: PathBuf,
        to: PathBuf,
    },
    /// 重新生成了（规则版本变了、`--force`），但和设备上那份逐字节相同：没有再传（Kindle 进度不受影响）。
    Same {
        path: PathBuf,
        warnings: Vec<String>,
    },
    /// 没变化、记录里的位置空了（以前放电脑上、用户自己挪上了设备），设备上对应位置的那份认领下来，没有重新生成。
    Adopted(PathBuf),
}

/// [`Library::prepare`] 的结果：已经有结论了，或者生成好、要交给传输线程放上设备的一件。
pub enum Step {
    Done(Built),
    Transfer(Box<Transfer>),
}

/// 要放上设备的一份产物：书库临时目录里新生成的（`tmp` 是那个目录，传完删），或者已有的（挪位置）。
pub(super) struct Made {
    pub(super) tmp: Option<PathBuf>,
    pub(super) src: PathBuf,
    pub(super) sha: String,
    pub(super) warnings: Vec<String>,
    /// 体检清单（[`bookconv::audit`]，一行一种写法），记进生成记录。
    pub(super) audit: Vec<String>,
}

/// [`Library::produce`] 的结果：(警告, 产物的 SHA-256（算好了的话）, 体检清单)。
pub(super) type Produced = (Vec<String>, Option<String>, Vec<String>);

/// 一本书在某模式下的产物状态（`list` 用）。
pub struct OutputStatus {
    pub device: String,
    pub path: PathBuf,
    /// `Some(true)` 最新；`Some(false)` 过期（原件或处理规则变了，或上次生成没完成）；`None` 判断不了（相对路径旧记录、电脑上的文件不在、指纹算不出）。
    pub fresh: Option<bool>,
    /// 在可拔下的设备上：只比了指纹，没核对设备上的文件还在不在（`list` 在状态后注明「未核对设备」）。
    pub device_unchecked: bool,
    /// 生成时的体检清单（[`bookconv::audit::Finding::line`]）：这台上和 Kindle 不一样又没处理的（`✗`）、没验证过的（`?`）写法。
    pub audit: Vec<String>,
}

/// 生成计划：指纹。
pub(super) struct Plan {
    pub(super) fingerprint: String,
    /// 同一套规则按 2026-10-08 以前的写法（文字书、漫画不分）算出的指纹：记录里存的是它的，说明规则没变、只是指纹写法改了，
    /// 记录改成新写法、不重新生成（[`Library::prepare`]）。
    pub(super) legacy: String,
    pub(super) format: Format,
}

/// 一本书在这个模式下产物的位置，以及记录里的旧状态（[`Library::resolve_placement`]）。
pub(super) struct Slot {
    /// 产物根目录（删旧文件后只在它以内删变空的目录）。
    pub(super) root: PathBuf,
    /// 产物所在目录。
    pub(super) dir: PathBuf,
    /// 按记录算出来的产物路径（还没考虑设备上那份和这次产物逐字节相同的同名文件）。
    pub(super) out: PathBuf,
    /// 记录里这本书上次的条目。
    pub(super) prev: Option<StateEntry>,
    /// 记录说产物以前放在电脑上、用户自己拷上设备的那份拷贝的文件名。
    pub(super) copied: Option<String>,
    /// 同一轮里别的书正在传、还没记进来的文件名（小写）。
    pub(super) taken: std::collections::BTreeSet<String>,
}

/// 记录里的产物现在还作不作数（[`Library::prepare_file`]）：`unchanged` 指纹没变、不是 `--force`；`done` 文件也还在。
#[derive(Clone, Copy)]
pub(super) struct RecordState<'a> {
    pub(super) unchanged: Option<&'a StateEntry>,
    pub(super) done: Option<&'a StateEntry>,
}

/// 生成时发现设备的挂载点不在了（中途拔线）：`build_all` 认这个开头，把设备记成掉线。
/// [`Library::space_check`] 的结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SpaceCheck {
    /// 不用查（产物放电脑上、设备没接上）。
    Skip,
    Enough,
    /// 不够：`count` 本要传，连余量要 `need` 字节，剩 `free`，差 `short`。
    Short {
        count: usize,
        need: u64,
        free: u64,
        short: u64,
    },
    /// 读不到剩余空间（只警告、照旧传）。
    Unknown(String),
}

pub(crate) const UNMOUNTED: &str = "设备中途断开（挂载点不在了）";

/// 某模式的生成记录（`output-state/<模式 id>.json`）：书 id → 产物在哪、指纹。
#[derive(Default, Clone, Serialize, Deserialize)]
pub(crate) struct State {
    pub(super) books: BTreeMap<String, StateEntry>,
    /// 从书库删了的书留在设备上的产物：删的时候设备没接上，接上了再删（[`Library::flush_removed`]）。
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(super) orphans: Vec<Placement>,
}

#[derive(Serialize, Deserialize, Clone, PartialEq)]
pub(crate) struct StateEntry {
    /// 产物的绝对路径。
    pub(super) path: PathBuf,
    /// 产物根目录（`D/../<模式>` 或 `<书库>/output/<模式>`）：删旧文件后，只在它以内删变空的目录。
    pub(super) root: PathBuf,
    /// 空 = 这个位置的产物还没生成完（换位置时先登记，见模块注释）。
    pub(super) fingerprint: String,
    /// 放上去的产物的 SHA-256：下次重新生成出一样的就不再传（不用把设备上的文件读回来比）。旧记录没有。
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub(super) sha: String,
    /// 以前的位置、还没删掉的产物。
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(super) old: Vec<Placement>,
    /// 生成时的体检清单（[`bookconv::audit`]）：`booklib list` 显示。
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(super) audit: Vec<String>,
}

/// 一个产物文件和它的根目录。
#[derive(Serialize, Deserialize, Clone, PartialEq)]
pub(super) struct Placement {
    pub(super) path: PathBuf,
    pub(super) root: PathBuf,
}
