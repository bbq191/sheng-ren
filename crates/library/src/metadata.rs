//! 联网补元数据：`booklib meta --fetch`，`sync` 生成前也会给还没找过的书找一次（2026-10-12 定：跟踪 → 补元数据 → 传设备）。
//!
//! 按书名、作者找到这本书，存下元数据（`Meta::info`），书里没有封面的顺带找封面（见 `cover`）：
//! 1. **豆瓣**条目（中文版）：内容简介、标签、原作名、带国籍的作者，以及条目对应版本的出版社、出版年、ISBN、译者；
//! 2. 豆瓣没有时 **QQ 阅读**（`qqread`，网络文学多在这里）：内容简介、分类当标签、封面；
//! 3. 都没有时 **Wikidata** 作品：原作名、首次出版年。
//!
//! **生成时补进书里的**（[`plan`]，只补书里没有的，正文不动）：内容简介、标签；作者拆成一个人一条、补上国籍（`[美] 玛格丽特·米切尔`，
//! 书里整串写成「某某 著，某某 译」的也拆开）；译者（`dc:contributor`，角色 trl）；出版社、出版日期、ISBN——这三项和译者一样是
//! 豆瓣那个**版本**的，书里写了译者而那个版本的译者对不上时不补（好读的书多是台湾译本，豆瓣条目多是大陆版）。
//! 2026-10-12 定（以前版本信息只记在 `meta.json`）。书名一律用书自己的。

use crate::cover::{epub_has_cover, incomplete, CoverInfo, CoverResult};
use crate::net::Net;
use crate::wikidata::Work;
use crate::{Library, Meta};
use bookconv::direction::PageDirection;
use profile::Profile;
use serde::{Deserialize, Serialize};
use std::path::Path;

/// 联网找来的元数据（`Meta::info`）。
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct BookInfo {
    /// 从哪来的："豆瓣 <网址>"、"QQ阅读 <网址>" 或 "Wikidata Q…"。
    pub source: String,
    /// 作品地区（Bangumi 的标签判出来的，2026-11-10）：`None`＝旧记录，还没查过；`Some("")`＝查过但判不出
    /// （不是漫画、地区词打平、没有地区词）；`Some("日本")` 等＝ `bangumi::REGIONS` 词表里的地区。
    /// 生成时 `comic_direction` 用它决定日漫要不要写成从右往左翻。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub region: Option<String>,
    /// 网站上这部作品的书名（豆瓣条目名不含副标题、Bangumi 中文名、QQ 阅读书名；Wikidata 不填）：生成时书名改用它
    /// 加上书里的卷次（[`corrected_title`]，2026-10-12 定）。
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub title: String,
    /// 网站上的作者署名（豆瓣带国籍：「[美] 玛格丽特·米切尔」）：书里的作者没写国籍时照它补。
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub authors: Vec<String>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub original_title: String,
    /// 首次出版年（原作）。
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub first_published: String,
    /// 内容简介，段落之间用换行。
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub subjects: Vec<String>,
    /// 匹配到的那个版本的信息（只供参考，不写进书）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub edition: Option<Edition>,
}

/// 豆瓣条目对应的版本。
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct Edition {
    pub title: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub authors: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub translators: Vec<String>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub publisher: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub pubdate: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub isbn: String,
}

/// 一次 `meta` 的结果。
pub enum InfoResult {
    Found(BookInfo),
    /// 同一套漫画别的卷找过的，直接用（不联网）：(元数据, 那一卷的书名)。
    Shared(BookInfo, String),
    /// 已经找过（`force` 才重找）。
    Existing(BookInfo),
    /// 没找到（原因）。
    NotFound(String),
    /// 网络出错没查成（原因），什么都没存、下次再查；封面这一步有结果（已存下）时才这样报，不然整本报错。
    Failed(String),
}

/// 一个书目网站（豆瓣、QQ 阅读）：按书名作者搜条目、从条目里挑封面、取元数据。搜索的骨架（书名规整、作者规整、
/// 依次试搜索词、出错就停、第一批对得上的就用）是 [`BookSource::search`] 的缺省实现，各网站只给搜索词、网址和怎么认条目。
/// 排进 [`CHAIN`] 的先后就是查找的先后。
pub(crate) trait BookSource {
    /// 搜索结果里书名、作者对得上的一条。
    type Hit;

    /// 搜索词，按先后试（`title` 已去掉首尾空白）。
    fn queries(&self, title: &str, authors: &[String]) -> Vec<String>;

    /// 一次搜索的网址（回 JSON）。
    fn search_url(&self, q: &str) -> String;

    /// 搜一次、取回 JSON：缺省 GET [`BookSource::search_url`]（Bangumi 是 POST）。出错已记成临时错误。
    fn fetch_hits(&self, net: &Net, q: &str) -> Result<serde_json::Value, String> {
        Ok(net.json_strict(&self.search_url(q))?)
    }

    /// 条目的作者署名（几个人用 `/`、`、` 分开）：书里有几位作者时挑作者对上得多的条目（[`BookSource::search`]）。
    fn hit_author<'h>(&self, hit: &'h Self::Hit) -> &'h str;

    /// 这一本要不要问这个网站（缺省都问；Bangumi 只问漫画）。
    fn applies(&self, _l: &Lookup<'_>) -> bool {
        true
    }

    /// 回执里书名（规整后 `nt`）、作者（规整后 `want`）对得上的条目，保持网站给的顺序；回执本身说出错了返回 `Err`
    /// （记成临时错误，别的搜索词也不再试）。
    fn matching_hits(
        &self,
        v: &serde_json::Value,
        nt: &str,
        want: &[String],
    ) -> Result<Vec<Self::Hit>, String>;

    /// 按书里元数据的书名、作者找（2026-10-08 定）：依次试 [`BookSource::queries`]，第一批对得上的就用。
    fn search(&self, net: &Net, title: &str, authors: &[String]) -> Vec<Self::Hit> {
        if crate::matching::norm_s(title).is_empty() {
            return Vec::new();
        }
        // 署名拆开后的作者（书里整串写成「[美] 某某 著，某某 译」的也认得出）
        let want = crate::matching::author_keys(authors);
        // 先按书名，搜不到再按去掉卷次、版本说明的书名（「飘（上册）」豆瓣搜不到，「飘」搜得到），比对也按这个书名
        let mut tries: Vec<(String, String)> = self
            .queries(title.trim(), authors)
            .into_iter()
            .map(|q| (q, crate::matching::norm_s(title)))
            .collect();
        if let Some(base) = crate::matching::base_title(title) {
            let nb = crate::matching::norm_s(&base);
            for q in self.queries(&base, authors) {
                if !tries.iter().any(|(t, _)| *t == q) {
                    tries.push((q, nb.clone()));
                }
            }
        }
        for (q, nt) in &tries {
            let (q, nt) = (q.clone(), nt.clone());
            let url = self.search_url(&q);
            // 出错（被拦、回来的不是 JSON、网络问题）已记成临时错误（`Net::transient_error`），调用方不会当成"没这本书"；
            // 别的搜索词也不用再试了
            let Ok(v) = self.fetch_hits(net, &q) else {
                break;
            };
            match self.matching_hits(&v, &nt, &want) {
                Ok(hits) if !hits.is_empty() => {
                    // 书里有两位以上作者、这里没有条目对得上两位：另一种搜法里可能有（豆瓣搜索建议只给了小说，书籍搜索页里有
                    // 图像小说），有对得上两位的就用那边的
                    if want.len() >= 2 && self.author_count(&hits, &want) < 2 {
                        let base = crate::matching::base_title(title)
                            .unwrap_or_else(|| title.trim().to_string());
                        let more = self.fallback_search(
                            net,
                            &base,
                            &crate::matching::norm_s(&base),
                            &want,
                        );
                        if self.author_count(&more, &want) >= 2 {
                            return self.most_authors(more, &want);
                        }
                    }
                    return self.most_authors(hits, &want);
                }
                Ok(_) => {}
                Err(e) => {
                    net.note_transient(&format!("{url}: {e}"));
                    return Vec::new();
                }
            }
        }
        // 都搜不到：网站另有的搜法（豆瓣的搜索建议不收漫画，书籍搜索页收）
        if net.transient_error().is_none() {
            let (title, nt) = match crate::matching::base_title(title) {
                Some(b) => (b.clone(), crate::matching::norm_s(&b)),
                None => (title.trim().to_string(), crate::matching::norm_s(title)),
            };
            return self.most_authors(self.fallback_search(net, &title, &nt, &want), &want);
        }
        Vec::new()
    }

    /// 书里有两位以上作者、有条目对上了其中两位以上时，只留对上得最多的（《杀死一只知更鸟》图像小说是哈珀·李、弗雷德·福德姆
    /// 两位，豆瓣上小说、图像小说条目同名，靠作者分；以前挑中了小说）。别的情况原样。
    fn most_authors(&self, hits: Vec<Self::Hit>, want: &[String]) -> Vec<Self::Hit> {
        if want.len() < 2 {
            return hits;
        }
        let best = self.author_count(&hits, want);
        if best < 2 {
            return hits;
        }
        hits.into_iter()
            .filter(|h| self.authors_matched(h, want) == best)
            .collect()
    }

    /// 条目对上了书里几位作者。
    fn authors_matched(&self, h: &Self::Hit, want: &[String]) -> usize {
        let parts: Vec<String> = self
            .hit_author(h)
            .split(['/', '、', ',', '，'])
            .map(crate::matching::norm_author)
            .filter(|p| !p.is_empty())
            .collect();
        want.iter()
            .filter(|w| {
                parts
                    .iter()
                    .any(|p| crate::matching::similarity(w, p) >= 0.6)
            })
            .count()
    }

    /// 这些条目里最多对上了书里几位作者。
    fn author_count(&self, hits: &[Self::Hit], want: &[String]) -> usize {
        hits.iter()
            .map(|h| self.authors_matched(h, want))
            .max()
            .unwrap_or(0)
    }

    /// 上面的搜索词都搜不到时的另一种搜法（`title` 已去掉卷次）；缺省没有。出错记成临时错误、返回空。
    fn fallback_search(
        &self,
        _net: &Net,
        _title: &str,
        _nt: &str,
        _want: &[String],
    ) -> Vec<Self::Hit> {
        Vec::new()
    }

    /// 从对得上的条目里挑一张像封面的图：(条目下标, 图片, 扩展名)。
    fn cover(&self, net: &Net, hits: &[Self::Hit]) -> Option<(usize, Vec<u8>, &'static str)>;

    /// 封面图从哪下载的、给人核对的条目说明（记进 `meta.json`）。
    fn cover_origin(&self, hit: &Self::Hit) -> (String, String);

    /// 元数据：先看 `chosen`（封面用了哪个条目就取哪个），网站自己决定还看不看别的条目。`authors` 用来从标签里去掉作者名。
    /// `translators` 是书里的译者（署名拆出来的、`dc:contributor` 里的），网站有几个版本时挑译者对得上的。
    fn info(
        &self,
        net: &Net,
        hits: &[Self::Hit],
        chosen: usize,
        authors: &[String],
        translators: &[String],
    ) -> Option<BookInfo>;
}

/// 查找过程中的进度：责任链上各环依次往里填。
pub(crate) struct Lookup<'a> {
    pub(crate) lib: &'a Library,
    pub(crate) net: &'a Net,
    pub(crate) meta: &'a Meta,
    /// 书里元数据的书名（去掉首尾空白）。
    pub(crate) title: &'a str,
    pub(crate) need_info: bool,
    pub(crate) need_cover: bool,
    /// 是漫画（Bangumi 只给漫画查）。
    pub(crate) comic: bool,
    pub(crate) info: Option<BookInfo>,
    /// 网站上找到、已经存下的封面。
    pub(crate) cover: Option<CoverResult>,
    /// Wikidata 找到的原作（找封面的后备：Open Library / Commons），和查它时出的错。
    pub(crate) work: Option<Work>,
    pub(crate) work_err: Option<String>,
}

impl Lookup<'_> {
    pub(crate) fn wants_info(&self) -> bool {
        self.need_info && self.info.is_none()
    }

    pub(crate) fn wants_cover(&self) -> bool {
        self.need_cover && self.cover.is_none()
    }
}

/// 责任链上的一环：还缺东西时才轮到它（[`Library::fetch_metadata_inner`]）。
pub(crate) trait Step {
    fn run(&self, l: &mut Lookup<'_>) -> Result<(), String>;
}

/// 书目网站这一环：搜、封面（先存下）、元数据。
impl<S: BookSource> Step for S {
    fn run(&self, l: &mut Lookup<'_>) -> Result<(), String> {
        // 前面的网站出错没查成时不查：这个网站的结果存下了，以后就不会再去前面的网站找
        if l.net.transient_error().is_some() || !self.applies(l) {
            return Ok(());
        }
        let hits = self.search(l.net, l.title, &l.meta.authors);
        let mut chosen = 0; // 元数据取哪个条目：封面用了哪个就取哪个
        if l.wants_cover() {
            if let Some((i, bytes, ext)) = self.cover(l.net, &hits) {
                chosen = i;
                let (url, label) = self.cover_origin(&hits[i]);
                l.cover = Some(CoverResult::Found(
                    l.lib.store_cover(l.meta, &bytes, ext, url, label)?,
                ));
            }
        }
        if l.wants_info() {
            l.info = self.info(
                l.net,
                &hits,
                chosen,
                &l.meta.authors,
                &crate::metadata::book_translators(l.meta),
            );
        }
        Ok(())
    }
}

/// 找书的匹配规则版本：改了搜索词、比对口径就加一，以前没找到的书 `sync` 会再找一次（[`crate::Meta::info_checked`]）。
/// 2：署名拆开（「某某 著，某某 译」）、书名去卷次再搜、书名用字重合 80% 也算、按译者挑版本（2026-10-12）。
/// 3：记下网站书名（书名改用它，2026-10-12）；以前找到过、没记书名的再找一次。
/// 4：Bangumi 的漫画作者读「原作」「作画」、书名开头的「XX系列NN：」去掉再搜、QQ 阅读「不在关键词库中」算没找到（2026-10-12）。
pub const LOOKUP_VERSION: &str = "4";

/// 一套书的认法：书名去掉卷次、版本字样（[`crate::matching::base_title`]）规整后加上作者（[`crate::matching::author_keys`]）。
pub(crate) fn series_key(meta: &Meta) -> String {
    let t = crate::matching::base_title(&meta.title).unwrap_or_else(|| meta.title.clone());
    let mut a = crate::matching::author_keys(&meta.authors);
    a.sort();
    format!("{}|{}", crate::matching::norm_s(&t), a.join("/"))
}

/// 书里的译者：署名里写的（「某某 著，梅静 译」）加上 `dc:contributor` 里角色是译者的（入库时存下的 [`crate::OwnDc::translators`]）。
pub(crate) fn book_translators(meta: &Meta) -> Vec<String> {
    let mut t = crate::matching::parse_credits(&meta.authors).translators;
    for x in meta
        .own_dc
        .as_ref()
        .map(|o| o.translators.as_slice())
        .unwrap_or_default()
    {
        if !t.contains(x) {
            t.push(x.clone());
        }
    }
    t
}

/// 查找的先后：⓪ Bangumi（只查漫画：正式公开的 API，漫画条目全、简介标签好）① 豆瓣（中文版）② QQ 阅读（豆瓣没有的，网络文学多是这样）
/// ③ Wikidata（原作）。
const CHAIN: &[&dyn Step] = &[
    &crate::bangumi::Bangumi,
    &crate::douban::Douban,
    &crate::qqread::QqRead,
    &crate::wikidata::Wikidata,
];

/// 从标签里去掉作者名（豆瓣标签常带作者）和年份，最多 8 个。
pub(crate) fn tags_without_authors<'a>(
    tags: impl Iterator<Item = &'a String>,
    authors: impl Iterator<Item = &'a String>,
) -> Vec<String> {
    let author_keys: Vec<String> = authors
        .flat_map(|a| crate::matching::author_keys(std::slice::from_ref(a)))
        .collect();
    // 年份（「1983」「2023~2026年」）不算标签
    let year = |t: &str| {
        t.chars().any(|c| c.is_ascii_digit())
            && t.chars()
                .all(|c| c.is_ascii_digit() || "~-～年 ".contains(c))
    };
    tags.filter(|t| !year(t) && !author_keys.contains(&crate::matching::norm_author(t)))
        .take(8)
        .cloned()
        .collect()
}

impl Library {
    /// 给一本书补元数据，书里没有封面的顺带找封面。已经找过的跳过（`force` 重找）。
    /// 网络出错（连不上网、重试完还不行）时返回错误、不把"没查成"当"没有"存下来，下次再查。
    pub fn fetch_metadata(
        &self,
        meta: &Meta,
        force: bool,
    ) -> Result<(InfoResult, CoverResult), String> {
        let r = self.fetch_metadata_inner(meta, force);
        match self.net.get() {
            // 连不上网：报第一个连不上的请求，而不是后面那些"跳过"的
            Some(net) if net.offline() && r.is_err() => {
                Err(net.transient_error().unwrap_or_else(|| "连不上网".into()))
            }
            _ => r,
        }
    }

    fn fetch_metadata_inner(
        &self,
        meta: &Meta,
        force: bool,
    ) -> Result<(InfoResult, CoverResult), String> {
        self.fetch_metadata_with_chain(meta, force, CHAIN)
    }

    /// 同 [`Library::fetch_metadata_inner`]，但要查的网站由调用方给（测试用假网站）。
    fn fetch_metadata_with_chain(
        &self,
        meta: &Meta,
        force: bool,
        chain: &[&dyn Step],
    ) -> Result<(InfoResult, CoverResult), String> {
        let need_info = force || meta.info.is_none();
        let need_cover = match &meta.cover {
            Some(_) if !force => false,
            _ => !self.book_has_own_cover(meta)?,
        };
        let existing_cover = || match &meta.cover {
            Some(c) if !force => CoverResult::Existing(c.clone()),
            _ => CoverResult::HasCover,
        };
        let existing_info = || {
            meta.info
                .clone()
                .map_or_else(|| InfoResult::NotFound(String::new()), InfoResult::Existing)
        };
        if !need_info && !need_cover {
            return Ok((existing_info(), existing_cover()));
        }
        // 同一套漫画只查一次（2026-10-12 要：漫画本数多、元数据一样）：别的卷找到过就照用，这一版规则下没找到过就不再找
        // `--force` 重找时，这次运行里已经重找过的那一套也照用（全量重找时漫画不必每卷联网）
        if need_info && !need_cover && (!force || self.facts.series_fetched(&series_key(meta))) {
            if let Some(r) = self.series_lookup(meta)? {
                return Ok((r, existing_cover()));
            }
        }
        let net = self.net();
        if net.offline() {
            return Err("连不上网".into());
        }
        net.clear_transient();
        // 找书只用书里的元数据：书名 + 作者（2026-10-08 定，不再从文件名里取书名）
        let comic = self.is_comic(meta).unwrap_or(false);
        let mut l = Lookup {
            lib: self,
            net,
            meta,
            title: meta.title.trim(),
            need_info,
            need_cover,
            comic,
            info: None,
            cover: None,
            work: None,
            work_err: None,
        };
        for step in chain {
            if l.wants_info() || l.wants_cover() {
                step.run(&mut l)?;
            }
        }
        let l_comic = l.comic;
        let Lookup {
            info,
            mut cover,
            work,
            work_err,
            ..
        } = l;
        // 网站上的封面已经存下了：后面出错也不整本报错，如实报"封面已存、元数据没查成"
        let cover_stored = cover.is_some();

        let info_result = if !need_info {
            existing_info()
        } else if let Some(i) = info {
            if l_comic {
                self.facts.note_series_fetched(&series_key(meta));
            }
            let mut m = self.read_meta(&meta.id).ok_or("条目读不出来")?;
            m.info = Some(i.clone());
            m.info_checked = LOOKUP_VERSION.to_string();
            self.save_meta(&m)?;
            InfoResult::Found(i)
        } else if let Some(e) = net.transient_error().or_else(|| work_err.clone()) {
            let e = format!("网络出错，没查完（{e}），下次再试");
            if !cover_stored {
                return Err(e);
            }
            InfoResult::Failed(e)
        } else {
            // 记下这一版规则找过了：`sync` 不再每次都去找。2026-11-10：旧 Bangumi 记录重找没拿到新结果时，
            // 把现有 info 的地区记成 Some("")（查过、判不出），否则 `needs_metadata` 每次 sync 都会再触发重找。
            // 网络出错（上面的 Failed / Err）不走到这里，下次再试。
            if let Some(mut m) = self.read_meta(&meta.id) {
                let mut changed = false;
                if m.info_checked != LOOKUP_VERSION {
                    m.info_checked = LOOKUP_VERSION.to_string();
                    changed = true;
                }
                if let Some(i) = m
                    .info
                    .as_mut()
                    .filter(|i| i.source.starts_with("Bangumi") && i.region.is_none())
                {
                    i.region = Some(String::new());
                    changed = true;
                }
                if changed {
                    self.save_meta(&m)?;
                }
            }
            InfoResult::NotFound("豆瓣、QQ 阅读、Wikidata 里都找不到书名、作者对得上的书".into())
        };
        if need_cover && cover.is_none() {
            let found = (|| {
                if let Some(e) = &work_err {
                    return Err(incomplete(e));
                }
                Ok(match self.cover_from_work(net, meta, work.as_ref())? {
                    Ok(c) => CoverResult::Found(c),
                    // 没找到是因为网络出错：不生成（生成的会存下来，以后就不找了）
                    Err(_) if net.transient_error().is_some() => {
                        return Err(incomplete(&net.transient_error().unwrap_or_default()))
                    }
                    // ④ 都没有：生成
                    Err(why) => CoverResult::Generated(self.generate_cover(net, meta)?, why),
                })
            })();
            cover = Some(match found {
                Ok(c) => c,
                // 元数据这次已经存下了：如实报"元数据已存、封面没查成"，不整本报错
                Err(e) if matches!(info_result, InfoResult::Found(_)) => CoverResult::Failed(e),
                Err(e) => return Err(e),
            });
        }
        Ok((info_result, cover.unwrap_or_else(existing_cover)))
    }

    /// 同一套漫画（作者相同、书名去掉卷次和版本字样后相同，[`series_key`]）别的卷的结果：找到过的复制过来存下（[`InfoResult::Shared`]），
    /// 这一版规则下没找到过的记成找过（[`InfoResult::NotFound`]）；都没有返回 `None`（照常联网找）。只管漫画：文字书的上下册
    /// 版本信息（ISBN）各不相同。
    ///
    /// 挑哪一卷：**判出地区的（`region` 非空）优先于判不出的（`Some("")`）**，判不出的又优先于还没记地区（`None`）的；
    /// 同一档里当前规则版本找到的优先，其余保持 `list()` 顺序。2026-11-10：不能把 `Some("")` 和「日本」一样当有地区，
    /// 否则同一套里个别卷搜不到、先记成判不出，别的卷就会抄到它，永远回不到「日本」。
    fn series_lookup(&self, meta: &Meta) -> Result<Option<InfoResult>, String> {
        if !self.is_comic(meta).unwrap_or(false) {
            return Ok(None);
        }
        let key = series_key(meta);
        let others: Vec<Meta> = self
            .list()
            .into_iter()
            .filter(|m| m.id != meta.id && series_key(m) == key)
            .collect();
        // 判出地区的优先于判不出的（`Some("")`），再按这一版规则找到的（`--force` 重找时用刚重找过的那一卷，
        // 不用别的卷留着的旧结果），最后其余。2026-11-10：`Some("")` 不能算「有地区」，否则同一套里先处理到、
        // 没搜到的那一卷会把后面各卷都带成判不出。
        let known = |m: &Meta| {
            m.info
                .as_ref()
                .is_some_and(|i| i.region.as_deref().is_some_and(|r| !r.is_empty()))
        };
        let unknown = |m: &Meta| {
            m.info
                .as_ref()
                .is_some_and(|i| i.region.as_deref() == Some(""))
        };
        let current = |m: &Meta| m.info_checked == LOOKUP_VERSION;
        let found = others
            .iter()
            .filter(|m| known(m) && current(m))
            .chain(others.iter().filter(|m| known(m)))
            .chain(others.iter().filter(|m| unknown(m) && current(m)))
            .chain(others.iter().filter(|m| unknown(m)))
            .chain(others.iter().filter(|m| current(m)))
            .chain(others.iter())
            .find_map(|m| m.info.as_ref().map(|i| (i.clone(), m.title.clone())));
        let mut m = self.read_meta(&meta.id).ok_or("条目读不出来")?;
        if let Some((info, from)) = found {
            // 抄来的旧 Bangumi info 没地区就原样抄（region 仍是 None）：下一次 sync 这一卷自己 force 重找，
            // 拿到 Found / NotFound 都会停；不能在这里记 Some("")，否则同一套的日漫这一卷会永远「判不出」。
            m.info = Some(info.clone());
            m.info_checked = LOOKUP_VERSION.to_string();
            self.save_meta(&m)?;
            return Ok(Some(InfoResult::Shared(info, from)));
        }
        if let Some(o) = others.iter().find(|o| o.info_checked == LOOKUP_VERSION) {
            m.info_checked = LOOKUP_VERSION.to_string();
            if let Some(i) = m
                .info
                .as_mut()
                .filter(|i| i.source.starts_with("Bangumi") && i.region.is_none())
            {
                i.region = Some(String::new());
            }
            self.save_meta(&m)?;
            return Ok(Some(InfoResult::NotFound(format!(
                "同一套的《{}》找过，没找到",
                o.title
            ))));
        }
        Ok(None)
    }

    /// `sync` 要不要给这本书联网找元数据：还没找到过（或者找到的是早先没记网站书名的），也没在这一版匹配规则下找过（[`crate::Meta::info_checked`]）。
    /// 2026-11-10 起 Bangumi 来源但没记地区（`BookInfo.region` 是 `None`）的旧记录也要再找一次（不管 `info_checked`）。
    pub fn needs_metadata(&self, meta: &Meta) -> bool {
        let bangumi_without_region = meta
            .info
            .as_ref()
            .is_some_and(|i| i.source.starts_with("Bangumi") && i.region.is_none());
        bangumi_without_region
            || (meta.info_checked != LOOKUP_VERSION
                && meta
                    .info
                    .as_ref()
                    .is_none_or(|i| i.title.is_empty() && !i.source.starts_with("Wikidata")))
    }

    /// 去掉联网找来的元数据和封面（找错了的时候）。返回有没有东西被去掉。
    pub fn clear_metadata(&self, meta: &Meta) -> Result<bool, String> {
        let mut m = self.read_meta(&meta.id).ok_or("条目读不出来")?;
        let had = m.info.take().is_some();
        let cover: Option<CoverInfo> = m.cover.take();
        // 找错了才去掉：记成这一版规则找过了，`sync` 不会自动再找回来（要重找用 `meta --fetch --force`）
        let mark = m.info_checked != LOOKUP_VERSION;
        m.info_checked = LOOKUP_VERSION.to_string();
        if had || cover.is_some() || mark {
            self.save_meta(&m)?;
        }
        // meta 存好了才删图：存失败时 meta 还指着它
        if let Some(c) = &cover {
            let _ = std::fs::remove_file(self.entry_dir(&meta.id).join(&c.file));
        }
        Ok(had || cover.is_some())
    }
}

/// 这本书生成时按元数据强制指定的翻页方向（2026-11-10 定）：设备开了 `comic_meta_direction`、书是漫画
/// （`comic` 由调用方用 `Library::comic_of` 算好，早期条目 `Meta::comic` 还是 `None` 时不漏）、
/// 元数据来自 Bangumi、地区是日本时写成 `rtl`；其余 `None`（照原书 / profile）。只在生成时按当时的 `meta.json` 定，
/// 入库、转换都不写；判成国漫 / 韩漫时不会把原书的 `rtl` 改成 `ltr`。
pub(crate) fn comic_direction(meta: &Meta, device: &Profile, comic: bool) -> Option<PageDirection> {
    if !device.comic.meta_direction || !comic {
        return None;
    }
    let info = meta.info.as_ref()?;
    (info.source.starts_with("Bangumi") && info.region.as_deref() == Some("日本"))
        .then_some(PageDirection::Rtl)
}

/// 读书里自己的元数据（入库时存进 [`crate::Meta::own_dc`]，生成时据此定补什么）。
pub(crate) fn read_own(epub: &Path) -> Result<crate::OwnDc, String> {
    use bookconv::opfmeta::{self, DcField};
    let current = opfmeta::read_epub(epub)?;
    let vals = |f: DcField| {
        current
            .iter()
            .find(|(x, _)| *x == f)
            .map(|(_, v)| v.clone())
            .unwrap_or_default()
    };
    let has = |f: DcField| !vals(f).iter().all(|v| v.trim().is_empty());
    Ok(crate::OwnDc {
        description: has(DcField::Description),
        subjects: has(DcField::Subject),
        v: crate::OwnDc::VERSION,
        creators: vals(DcField::Creator)
            .into_iter()
            .filter(|v| !v.trim().is_empty())
            .collect(),
        translators: vals(DcField::Translator),
        publisher: has(DcField::Publisher),
        date: has(DcField::Date),
        isbn: vals(DcField::Identifier).iter().any(|v| is_isbn(v)),
    })
}

/// 标识符是不是 ISBN：写了 isbn（`urn:isbn:…`、`ISBN 978…`），或者去掉连字符就是 10、13 位书号。
fn is_isbn(v: &str) -> bool {
    let l = v.to_ascii_lowercase();
    let digits: String = v.chars().filter(|c| !matches!(c, '-' | ' ')).collect();
    l.contains("isbn")
        || ((digits.len() == 13 && digits.starts_with("97")) || digits.len() == 10)
            && digits
                .chars()
                .all(|c| c.is_ascii_digit() || c == 'X' || c == 'x')
}

/// 出版日期写成 `YYYY-MM-DD`/`YYYY-MM`/`YYYY`（豆瓣写 `2013-6-3`、`2000-9`、`2013年6月`）；认不出的 `None`。
fn norm_date(d: &str) -> Option<String> {
    let parts: Vec<u32> = d
        .split(|c: char| !c.is_ascii_digit())
        .filter(|p| !p.is_empty())
        .filter_map(|p| p.parse().ok())
        .collect();
    match parts.as_slice() {
        [y, m, day, ..]
            if (1000..3000).contains(y) && (1..=12).contains(m) && (1..=31).contains(day) =>
        {
            Some(format!("{y:04}-{m:02}-{day:02}"))
        }
        [y, m, ..] if (1000..3000).contains(y) && (1..=12).contains(m) => {
            Some(format!("{y:04}-{m:02}"))
        }
        [y, ..] if (1000..3000).contains(y) => Some(format!("{y:04}")),
        _ => None,
    }
}

/// 生成时往书里补的（[`inject`]）：(字段, 值)，`Identifier` 的值是要**加上**的 ISBN，别的字段整个换成这些值。
/// 书里有什么看 `own`（入库时记下的；没有记录时按书里只有 `meta.authors` 这些作者算）：
/// - 书名：换成 [`corrected_title`]（网站书名 + 书里的卷次）；
/// - 简介、标签：书里没有才补；
/// - 作者：书里的署名拆开（「[美] 某某 著，某某 译」拆成作者和译者），没写国籍的照找来的作者署名补上（同一个人：名字像）；
///   和书里原样相同就不动；
/// - 译者：署名里拆出来的；书里一个都没写时用找来的版本的；
/// - 出版社、出版日期、ISBN：书里没有、找来的版本就是这本（条目名和书名一致，书里写了译者时译者也对得上）才补。
pub(crate) fn plan(
    meta: &Meta,
    own: Option<&crate::OwnDc>,
) -> Vec<(bookconv::opfmeta::DcField, Vec<String>)> {
    use crate::matching::{norm_s, parse_credits, same_person_any, similarity};
    use bookconv::opfmeta::DcField;
    let info = meta.info.as_ref();
    let creators: &[String] = match own {
        Some(o) if o.v >= crate::OwnDc::VERSION => &o.creators,
        _ => &meta.authors,
    };
    let (has_desc, has_subj, own_trl, has_pub, has_date, has_isbn) = match own {
        Some(o) => (
            o.description,
            o.subjects,
            o.translators.as_slice(),
            o.publisher,
            o.date,
            o.isbn,
        ),
        None => (false, false, &[][..], false, false, false),
    };
    let mut out = Vec::new();
    let title = corrected_title(meta);
    if title != meta.title {
        out.push((DcField::Title, vec![title]));
    }
    if let Some(i) = info {
        if !has_desc && !i.description.is_empty() {
            out.push((DcField::Description, vec![i.description.clone()]));
        }
        if !has_subj && !i.subjects.is_empty() {
            out.push((DcField::Subject, i.subjects.clone()));
        }
    }
    let credits = parse_credits(creators);
    let web = info
        .map(|i| parse_credits(&i.authors).authors)
        .unwrap_or_default();
    let authors: Vec<String> = credits
        .authors
        .iter()
        .map(|p| {
            let mut p = p.clone();
            if p.nationality.is_empty() {
                if let Some(w) = web.iter().find(|w| {
                    !w.nationality.is_empty()
                        && similarity(&norm_s(&w.name), &norm_s(&p.name)) >= 0.6
                }) {
                    p.nationality = w.nationality.clone();
                }
            }
            p.display()
        })
        .collect();
    if !authors.is_empty() && authors.as_slice() != creators {
        out.push((DcField::Creator, authors));
    }
    let mut translators: Vec<String> = credits.translators.clone();
    for t in own_trl {
        if !translators.contains(t) {
            translators.push(t.clone());
        }
    }
    let edition = info.and_then(|i| i.edition.as_ref());
    // 版本可用：条目名和书名一致（规整后；靠去掉卷次、版本字样才对上的不算——「哆啦A梦 第0卷」的 ISBN 不是「全彩版 卷01」的），
    // 书里写了译者时那个版本的译者要对得上
    let same_title = |e: &&Edition| {
        norm_s(e.title.split(['：', ':']).next().unwrap_or("")) == norm_s(&meta.title)
    };
    let edition_ok = edition
        .filter(same_title)
        .filter(|e| translators.is_empty() || same_person_any(&e.translators, &translators));
    if translators.is_empty() {
        if let Some(e) = edition_ok.filter(|e| !e.translators.is_empty()) {
            translators = e.translators.clone();
        }
    }
    if !translators.is_empty() && translators.as_slice() != own_trl {
        out.push((DcField::Translator, translators));
    }
    if let Some(e) = edition_ok {
        if !has_pub && !e.publisher.trim().is_empty() {
            out.push((DcField::Publisher, vec![e.publisher.trim().to_string()]));
        }
        if let Some(d) = norm_date(&e.pubdate).filter(|_| !has_date) {
            out.push((DcField::Date, vec![d]));
        }
        let isbn: String = e
            .isbn
            .chars()
            .filter(|c| c.is_ascii_alphanumeric())
            .collect();
        if !has_isbn && !isbn.is_empty() {
            out.push((DcField::Identifier, vec![format!("urn:isbn:{isbn}")]));
        }
    }
    out
}

/// 书里书名末尾的卷次，统一写法：上中下册写成「（上册）」，第几卷写成「 第01卷」（数字照书里的）；没有、认不出（「（共3册）」这类说明）是空。
fn volume_suffix(title: &str) -> String {
    let base = crate::matching::base_title(title).unwrap_or_else(|| title.to_string());
    let tail: String = title
        .trim()
        .strip_prefix(base.as_str())
        .unwrap_or("")
        .to_string();
    for part in ["上", "中", "下"] {
        if tail.contains(&format!("{part}册")) || tail.contains(&format!("{part}冊")) {
            return format!("（{part}册）");
        }
    }
    if tail.contains('共') || tail.contains('全') && !tail.contains("全彩") {
        return String::new();
    }
    // 最后一串数字：卷01、第3卷、Vol.2、- 05
    let rev: Vec<char> = tail.chars().rev().collect();
    let after = rev.iter().take_while(|c| !c.is_ascii_digit()).count();
    let digits: String = rev[after..]
        .iter()
        .take_while(|c| c.is_ascii_digit())
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    // 「（精装72册）」「72本」是套书的册数，不是卷次（前面没有「第」）
    let unit = after.checked_sub(1).map(|i| rev[i]);
    let before = rev.get(after + digits.chars().count()).copied();
    if matches!(unit, Some('册' | '冊' | '本')) && before != Some('第') {
        return String::new();
    }
    if !digits.is_empty() {
        return format!(" 第{digits}卷");
    }
    // 卷次后面还跟着卷名的（「伊藤潤二愛藏版 - 卷08 呻吟的排水管」）：卷名一起留着，不然一套书都叫一个名字
    mid_volume(title).map_or_else(String::new, |(n, name)| {
        if name.is_empty() {
            format!(" 第{n}卷")
        } else {
            format!(" 第{n}卷 {name}")
        }
    })
}

/// 书名中间的卷次和后面的卷名：「卷08 呻吟的排水管」「第3卷：名字」「Vol.2 名字」→ (卷号, 卷名)。卷次前面得有书名。
fn mid_volume(title: &str) -> Option<(String, String)> {
    let c: Vec<char> = title.trim().chars().collect();
    let digit_run = |i: usize| c[i..].iter().take_while(|d| d.is_ascii_digit()).count();
    for i in 1..c.len() {
        let (start, n) = match c[i] {
            '卷' | '巻' => (i + 1, digit_run(i + 1)),
            '第' => {
                let n = digit_run(i + 1);
                if n == 0
                    || !matches!(
                        c.get(i + 1 + n),
                        Some('卷' | '巻' | '册' | '冊' | '集' | '部')
                    )
                {
                    continue;
                }
                (i + 1, n)
            }
            'V' | 'v'
                if c[i..]
                    .iter()
                    .take(3)
                    .collect::<String>()
                    .eq_ignore_ascii_case("vol") =>
            {
                let s = i
                    + 3
                    + c[i + 3..]
                        .iter()
                        .take_while(|d| matches!(d, '.' | ' '))
                        .count();
                (s, digit_run(s))
            }
            _ => continue,
        };
        if n == 0 {
            continue;
        }
        let mut end = start + n;
        if c[i] == '第' {
            end += 1;
        }
        let digits: String = c[start..start + n].iter().collect();
        let name: String = c[end..]
            .iter()
            .collect::<String>()
            .trim_start_matches([' ', '　', '-', '—', '·', '：', ':', '、'])
            .trim()
            .to_string();
        return Some((digits, name));
    }
    None
}

/// 修正后的书名（2026-10-12 定：书名用匹配到的网站书名，卷次照书里的补上；没找到的照书里的）：网站书名去掉「（共45册）」
/// 这类说明和卷次，加上书里的卷次（[`volume_suffix`]）。生成时写进书（`dc:title`），设备上的文件名也用它。
pub(crate) fn corrected_title(meta: &Meta) -> String {
    let Some(web) = meta
        .info
        .as_ref()
        .map(|i| i.title.trim())
        .filter(|t| !t.is_empty())
    else {
        return meta.title.clone();
    };
    let base = crate::matching::base_title(web).unwrap_or_else(|| web.to_string());
    // 书里标了作品形式（「（漫）」「（图像小说）」）而网站书名没有的：留着（和同名的小说分开，文件名不撞）
    let form = crate::matching::form_marker(&meta.title)
        .filter(|_| crate::matching::form_marker(web).is_none())
        .unwrap_or_default();
    let t = format!("{}{form}{}", base.trim(), volume_suffix(&meta.title));
    if t.trim().is_empty() {
        meta.title.clone()
    } else {
        t
    }
}

/// [`plan`] 的指纹（进生成指纹：补的东西变了产物就过期）；什么都不补时 `None`。
pub(crate) fn plan_sig(plan: &[(bookconv::opfmeta::DcField, Vec<String>)]) -> Option<String> {
    if plan.is_empty() {
        return None;
    }
    let s: String = plan
        .iter()
        .map(|(f, v)| format!("{}\u{1}{}\u{2}", f.name(), v.join("\u{0}")))
        .collect();
    Some(crate::fsutil::sha256_hex(s.as_bytes())[..12].to_string())
}

/// 复制 `src` 到 `dst`，补上 [`plan`] 定的元数据和（书里没有时）封面。
/// 改写用 `bookconv::opfmeta`（与 `booklib meta --edit` 同一份实现；不做 EPUB 3 规范整理——优化器的清洗层会做；不改 `dcterms:modified`，产物逐字节可重现）。
/// 什么都不用补时不写 `dst`，返回 `false`。
pub(crate) fn inject(
    src: &Path,
    dst: &Path,
    cover: Option<Vec<u8>>,
    plan: &[(bookconv::opfmeta::DcField, Vec<String>)],
) -> Result<bool, String> {
    use bookconv::opfmeta::{self, DcField, Edits};
    let mut edits = Edits {
        cover: cover.map(opfmeta::CoverEdit::Set),
        ..Default::default()
    };
    for (f, v) in plan {
        if *f == DcField::Identifier {
            edits.append.push((*f, v.clone()));
        } else {
            edits.set.push((*f, v.clone()));
        }
    }
    if edits.is_empty() {
        return Ok(false);
    }
    opfmeta::edit_epub(src, dst, &edits)?;
    Ok(true)
}

/// 生成产物前：书里缺的封面、[`plan`] 定的元数据补进去。补了返回新文件，没补返回原文件。`own` 是书里自己的元数据
/// （同生成指纹用的那份，见 `Library::own_dc_of`）。
pub(crate) fn with_additions(
    lib: &Library,
    meta: &Meta,
    own: Option<&crate::OwnDc>,
    epub: &Path,
    tmp: &Path,
) -> Result<std::path::PathBuf, String> {
    let cover = match &meta.cover {
        Some(c) if !epub_has_cover(epub) => Some(
            std::fs::read(lib.entry_dir(&meta.id).join(&c.file))
                .map_err(|e| format!("读封面: {e}"))?,
        ),
        _ => None,
    };
    let plan = plan(meta, own);
    if cover.is_none() && plan.is_empty() {
        return Ok(epub.to_path_buf());
    }
    let with = tmp.join("with-metadata.epub");
    Ok(if inject(epub, &with, cover, &plan)? {
        with
    } else {
        epub.to_path_buf()
    })
}

#[cfg(test)]
mod tests;
