# 日漫从右往左：Bangumi 判成日本漫画的书，生成时写 rtl（EPUB、CBZ 都一样）

- Task ID: 2026-11-10-manga-rtl-bangumi
- 负责 bot: sheng-ren
- 状态: pending
- 执行方: opencode-local
- 分支: feat/manga-rtl-bangumi（从最新 master 切出；建议在 2026-11-10-ireader-comic-quality 合并之后）
- 更新时间: 2026-11-10T10:30+08:00
- 引用: 无（PRD：vault `Projects/sheng-ren/prd/2026-11-10-comic-layout-direction.md` 的 B 节）

## 需求原文
> 2. 只要是日漫无论格式都是点击左侧翻下一页
>
> （问：怎么判断一本书是日漫？）按联网元数据判断，Bangumi 查到是日本漫画的就是日漫

## 输入
- 背景：vault `Projects/sheng-ren/context.md`；仓库 `docs/decisions.md`（2026-10-15「掌阅漫画照书的方向翻」、「三台都照书自己写的方向翻页」）、`docs/devices.md`（`comic_page_direction` 那行、掌阅 rtl 留边那段）、`docs/architecture.md#指纹`
- 现状（sheng-ren 2026-11-10 读代码 + 实测 API）：
  - `crates/library/src/bangumi.rs`：搜索 `POST https://api.bgm.tv/v0/search/subjects` 的回执每条有 `platform`、`tags:[{name,count}]`、`meta_tags:[字符串]`、`infobox`。`matching_hits` 只用 `platform` 排序，没读 `meta_tags`；`Hit` 没有地区；`info()` 产出的 `BookInfo` 没有地区
  - `crates/library/src/metadata.rs:22` `BookInfo`：source、title、authors、original_title、first_published、description、subjects、edition，存在 `masters/<id>/meta.json` 的 `info`（`crates/library/src/lib.rs:99` `Meta::info`）
  - 方向：`crates/bookconv/src/direction.rs`（读写 OPF spine 的 `page-progression-direction`）；`OptimizeOpts::page_direction`（按书指定，优先）和 `comic_page_direction`（profile）在 `crates/bookconv/src/optimize/mod.rs:263-268`、`1058-1060`；**书库从来没设过 `page_direction`**（`crates/library/src/generate.rs:914` 只用 `OptimizeOpts::for_profile(device)`）
  - CBZ 母版（`crates/bookconv/src/convert/cbz.rs`）不写 rtl → CBZ 日漫现在都是 ltr
  - 产物：EPUB 照 OPF；KEPUB 是同一份优化后的 EPUB 再插 koboSpan（OPF spine 照旧）；KFX 由 `crates/bookconv/src/epubbook.rs:213` 读 spine rtl，`crates/kfx/src/write/mod.rs:287` 写 `$560`
  - `needs_metadata`（`metadata.rs:544`）：早先没记网站书名的会再找一次——本单照这个先例补「没记地区的 Bangumi 记录」
- 2026-11-10 API 实测样本（写单元测试用，回执字段名照抄）：
  - 死亡笔记 1674：platform 漫画，meta_tags ["日本","推理","漫画","已完结"]
  - 迷宫饭 105206：meta_tags ["日本","漫画",…]，tags 有「日本」「日漫」
  - 哆啦A梦 27684、浪客行 32435：meta_tags []，tags 有「日本」
  - 一人之下 175459、镖人 184725：meta_tags []，tags 有「国漫」「中国」
  - 我独自升级 302561：meta_tags ["漫画","韩国",…]，tags 有「韩漫」「韩国」「条漫」
- 涉及文件：
  - `crates/library/src/bangumi.rs`（修改：`Hit` 加地区；`matching_hits` 读 `meta_tags` 和带 count 的 `tags` 判地区；`info()` 填 `region`；单元测试）
  - `crates/library/src/metadata.rs`（修改：`BookInfo` 加 `region: Option<String>`；`needs_metadata` 加一条；新函数 `comic_direction(meta) -> Option<PageDirection>` 或同义）
  - `crates/library/src/generate.rs`（修改：`produce()` 里按书设 `opts.page_direction`；`plan()` 指纹里强制 rtl 的书加标记）
  - `crates/profile/src/lib.rs`（修改：新键 `comic_meta_direction: bool`，缺省 false，进 `ComicPolicy`；测试）
  - `crates/profile/profiles/{kindle,kobo,hanvon,ireader}.toml`（修改：各加 `comic_meta_direction = true` 和注释）
  - `crates/library/src/fingerprint.rs`（只在需要时改；优先把标记放在 `generate.rs` 的书那一段）
  - `crates/library/tests/fp_golden.rs`、`fp_golden.txt`（修改：加 `comic_meta_direction` 的 mutation；金样没有元数据，预期 0 行旧行变化，只多出新 mutation 的行）
  - `crates/library/tests/flow.rs` 或 `generate.rs` 里的测试（新增：离线测试，见分步 5）
  - `crates/bookconv/src/direction.rs`（修改：开头「不自动判」那段注释，改成「书库按 Bangumi 元数据按书指定，见 library::metadata」）
  - `docs/devices.md`（新键一行；`comic_page_direction` 那行补说明）、`docs/decisions.md`（一行，推翻 10-15/10-16「照书」的一部分）、`docs/architecture.md#指纹`（标记一句）、`docs/usage.md`（`meta --fetch` 说明里提一句会定日漫方向）

## 约束
- 不做：不按画面、文件名、原名里的假名猜日漫；不用豆瓣 / QQ 阅读 / Wikidata 判方向；Bangumi 判成国漫 / 韩漫时**不把原书的 rtl 改成 ltr**；不改 CBZ 转换（`convert/`）、不改三个写出器（KFX、KEPUB、EPUB 写 zip）；不改 `BookInfo.subjects` 的现有取法（「日本」照旧留在标签里）；不改 install.sh；不给 profile 设备段加指纹字母（免得全部漫画白白重建）
- 掌阅：vinci 还没定掌阅要不要开（PRD 开放问题 1：rtl 书在掌阅上要每本选「漫画」排版才铺满）。确认任务单时 sheng-ren 会把答案写在这里；没写就按四台都开 `true` 做
- 判定规则（写成纯函数，单元测试覆盖）：
  - 只看 `platform == "漫画"` 的条目，否则 `region = Some("")`
  - 地区词表：日本 ← {日本, 日漫, 日本漫画}；中国 ← {中国, 国漫, 国产漫画, 中国漫画}；韩国 ← {韩国, 韩漫}；台湾 ← {台湾, 台漫}；香港 ← {香港, 港漫}；美国 ← {美国, 美漫}（词表做成常量，注释写 2026-11-10 实测）
  - 先看 `meta_tags`：落在词表里的地区恰好一个 → 用它；两个以上 → `Some("")`；没有 → 看 `tags`
  - `tags`：每个地区取它所有词的 count 之和，最高的唯一 → 用它；并列最高或一个都没有 → `Some("")`
- 生成规则：设备 `comic.meta_direction == true` 且 `meta.comic == Some(true)` 且 `info.source` 以 `Bangumi` 开头且 `region == Some("日本")` → `OptimizeOpts::page_direction = Some(PageDirection::Rtl)`；其余一律 `None`（照原书 / profile）。方向只在生成时按当时的 meta.json 定，入库、转换时不写
- 指纹：只有满足上面条件的书在书那一段加固定标记（如 `info` 段后接 `r`）；不满足的书指纹**一个字不变**
- 重找：`needs_metadata` 加一条——`info.source` 以 `Bangumi` 开头且 `region.is_none()`（旧记录）→ 要再找；`info_checked` 的既有逻辑不变。同一套漫画照旧只联网一次（`series_key`）
- 只在上面的分支工作；不合并；不改「涉及文件」以外的文件，确需改时写进「结果 → 偏离」
- 提交信息用中文，`feat:` 等前缀 + 正文，不加 Co-Authored-By；正文写「没跑真书回归、没上真机；判成日本漫画的书四台都会重建成 rtl（Kindle 进度清零），别的书指纹不变；已有 Bangumi 记录下次 sync 会重找一次」
- 不读密钥、`.env`、`auth*`；clean-room：不读 GPL 代码（含 KCC 源码）；**测试不联网**（回执用 JSON 字面量）
- 只对改动的 Rust 文件跑 rustfmt；编译用 `CARGO_TARGET_DIR=~/.cache/sheng-ren-target`
- 风险点：设备行为（翻页方向）、改了指纹构成（只加按书标记）、元数据重找 → 合并前 agy 二审

## 分步任务
1. [ ] `metadata.rs`：`BookInfo` 加 `#[serde(default, skip_serializing_if = "Option::is_none")] pub region: Option<String>`，doc 注释写清 None / Some("") / Some(地区) 三种含义。自检：旧 meta.json（没有 region）照常反序列化，`cargo test -p library`
2. [ ] `bangumi.rs`：按「约束 → 判定规则」写 `fn region_of(item: &serde_json::Value) -> String`；`Hit` 加 `region`；`info()` 填 `region: Some(h.region.clone())`。单元测试：用「输入」里 6 个样本的字段（各写成最小 JSON）断言 日本 / 日本 / 日本 / 中国 / 韩国；再加 meta_tags 同时有日本和中国 → ""；tags 日漫 3 vs 国漫 3 → ""；platform 小说 → ""
3. [ ] `profile/src/lib.rs`：加 `comic_meta_direction`（TOML 平铺字段，进 `ComicPolicy::meta_direction`），四个 toml 都写 `comic_meta_direction = true`，注释「2026-11-10 维护者定：Bangumi 判成日本漫画的按 rtl 生成；Kobo、汉王的 rtl 处理待真机确认」（ireader 的注释另加「掌阅 rtl 书要在阅读器里选『漫画』排版才铺满，见 docs/devices.md」）。自检：`cargo test -p profile`
4. [ ] `metadata.rs` 加 `pub(crate) fn comic_direction(meta: &Meta, device: &Profile) -> Option<PageDirection>`（按「生成规则」）；`generate.rs` `produce()` 里 `opts.page_direction = comic_direction(meta, device)`；`plan()` 里满足时给指纹加标记；`needs_metadata` 加重找条件。自检：`cargo test -p library`
5. [ ] 离线测试（`crates/library/tests/flow.rs` 照现有写法，或 `generate.rs` 单元测试）：入库一本合成 CBZ 漫画，直接改它的 meta.json 写 `info = {source: "Bangumi https://bgm.tv/subject/1", region: "日本", …}`，生成 ireader（EPUB）、kobo（KEPUB）、kindle（KFX）三份：EPUB、KEPUB 的 OPF spine 是 `page-progression-direction="rtl"`，KFX 用 `kfx` crate 读回方向是 rtl（或断言 `epubbook` 读优化后 EPUB 的 `rtl == true`，结果里写明用的哪种）；把 region 改成 "中国" / 删掉 info 再生成：回到 ltr（spine 没有 rtl），且指纹和改之前不判日漫时相同
6. [ ] `fp_golden.rs` 加 `comic_meta_direction` mutation；`UPDATE_FP_GOLDEN=1 cargo test -p library --test fp_golden`；`git diff` 应只多出新 mutation 的行，旧行 0 变化
7. [ ] 文档：`direction.rs` 注释、`docs/devices.md`、`docs/decisions.md`、`docs/architecture.md#指纹`、`docs/usage.md`
8. [ ] `cargo test --workspace` 全过；`cargo clippy --workspace --all-targets -- -D warnings` 0 警告

## 验收标准
- [ ] 功能：Bangumi 判成日本的漫画，四台产物方向都是 rtl（EPUB/KEPUB spine、KFX 方向）；CBZ 来源也一样
- [ ] 功能：不是 Bangumi 来源、地区不是日本 / 判不出、`meta --fetch --clear` 之后：方向照原书，指纹回到不判日漫时的值
- [ ] 功能：旧 meta.json（Bangumi、没有 region）`needs_metadata` 返回 true；有 region 的返回原来的结果
- [ ] 测试：`cargo test --workspace` 全部通过；clippy 0 警告；地区判定单元测试覆盖「约束」里每条分支
- [ ] 不回归：`fp_golden.txt` 旧行 0 变化；文字书、非日漫漫画产物逐字节不变（结果里说明怎么确认的，或写「待本机回归确认」）
- [ ] 「不做」的事确实没做
- [ ] 真机（vinci，合并后）：四台各看一本 CBZ 日漫点左侧是下一页、一本国漫不变；**Kobo Nickel、汉王的 rtl 没确认过，必看**；掌阅看是否留边

## 交付物
代码提交到上面的分支并推送；「结果」填好；状态改为 done 或 blocked。

## 给执行方的指令
```
执行任务单 .dev/tasks/2026-11-10-manga-rtl-bangumi.md：先把状态改为 accepted 并提交，按「分步任务」做完，填好「结果」，状态改为 done（做不下去改为 blocked 并写明原因），提交并推送分支。
```

## 结果
<!-- 执行方填写 -->
- 执行方 / 开始–结束时间：
- base..head：
- 提交：| 步骤 | 提交号 | 文件 | 说明 |
- 测试：| 命令 | 通过 / 失败 / 未跑 | 摘要 |
- 遇到的问题：
- 偏离：无
- 未完成：

## 审查
<!-- 负责 bot 填写 -->
- 验收：
- 二审：
- 合并：
