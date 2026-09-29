//! XHTML 规则：同章内链规整、单标签重复 id 折叠与全书 id 去重、脚注（识别/收集/就地重排、互指环拆解）、
//! 字体锁解除。标签与属性的解析统一走 `crate::html`。
//!
//! 子模块：`basic`（内链规整/id 去重）· `footnote_cycles`（脚注互指环拆解）· `footnote`（脚注识别/收集/就地重排）·
//! `fontlock`（内联样式的字体、字号锁）。

use crate::html::{self, Edit};
use crate::util::xml_escape;
use regex::Regex;
use std::collections::{HashMap, HashSet};
use std::sync::OnceLock;

mod basic;
mod fontlock;
mod footnote;
mod footnote_cycles;

// 对外（优化器、EPUB 组装）用到的项。
pub use self::basic::{collapse_dup_id_attrs, dedup_ids_in_chapter, fix_internal_links};
pub(crate) use self::basic::plan_id_renames;
pub use self::fontlock::strip_font_locks;
pub use self::footnote::{collect_footnote_notes, fix_duokan_markers, normalize_self_hrefs, number_icon_note_links, preserve_relink_footnotes, referenced_note_frags, referenced_note_keys, NoteKey};
pub(crate) use self::footnote::note_semantic;
pub use self::footnote_cycles::break_footnote_cycles;
