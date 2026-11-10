# 日漫从右往左：Bangumi 判成日本漫画的书，生成时写 rtl（EPUB、CBZ 都一样）

- Task ID: 2026-11-10-manga-rtl-bangumi
- 负责 bot: sheng-ren
- 状态: accepted
- 执行方: opencode-local
- 分支: feat/manga-rtl-bangumi（从最新 master 切出；建议在 2026-11-10-ireader-comic-quality 合并之后）
- 更新时间: 2026-11-10T13:05+08:00
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
- 掌阅：四台都开 `comic_meta_direction = true`，掌阅也开（vinci 2026-11-10 定）。rtl 的书在掌阅上要每本选一次「本书自定义 → 排版 → 漫画」才铺满，vinci 接受；`ireader.toml` 注释和 `docs/devices.md` 必须写明这条操作要求
- Bangumi 判成国漫 / 韩漫、原书写了 rtl 的：照原书 rtl（vinci 2026-11-10 定）；原名里的假名不当后备判据（vinci 2026-11-10 定）
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
1. [x] `metadata.rs`：`BookInfo` 加 `#[serde(default, skip_serializing_if = "Option::is_none")] pub region: Option<String>`，doc 注释写清 None / Some("") / Some(地区) 三种含义。自检：旧 meta.json（没有 region）照常反序列化，`cargo test -p library`
2. [x] `bangumi.rs`：按「约束 → 判定规则」写 `fn region_of(item: &serde_json::Value) -> String`；`Hit` 加 `region`；`info()` 填 `region: Some(h.region.clone())`。单元测试：用「输入」里 6 个样本的字段（各写成最小 JSON）断言 日本 / 日本 / 日本 / 中国 / 韩国；再加 meta_tags 同时有日本和中国 → ""；tags 日漫 3 vs 国漫 3 → ""；platform 小说 → ""
3. [x] `profile/src/lib.rs`：加 `comic_meta_direction`（TOML 平铺字段，进 `ComicPolicy::meta_direction`），四个 toml 都写 `comic_meta_direction = true`，注释「2026-11-10 维护者定：Bangumi 判成日本漫画的按 rtl 生成；Kobo、汉王的 rtl 处理待真机确认」（ireader 的注释另加「2026-11-10 vinci 接受：rtl 的书在掌阅里每本选一次『本书自定义 → 排版 → 漫画』才铺满，见 docs/devices.md」）。自检：`cargo test -p profile`
4. [x] `metadata.rs` 加 `pub(crate) fn comic_direction(meta: &Meta, device: &Profile) -> Option<PageDirection>`（按「生成规则」）；`generate.rs` `produce()` 里 `opts.page_direction = comic_direction(meta, device)`；`plan()` 里满足时给指纹加标记；`needs_metadata` 加重找条件。自检：`cargo test -p library`
5. [x] 离线测试（`crates/library/tests/flow.rs` 照现有写法，或 `generate.rs` 单元测试）：入库一本合成 CBZ 漫画，直接改它的 meta.json 写 `info = {source: "Bangumi https://bgm.tv/subject/1", region: "日本", …}`，生成 ireader（EPUB）、kobo（KEPUB）、kindle（KFX）三份：EPUB、KEPUB 的 OPF spine 是 `page-progression-direction="rtl"`，KFX 用 `kfx` crate 读回方向是 rtl（或断言 `epubbook` 读优化后 EPUB 的 `rtl == true`，结果里写明用的哪种）；把 region 改成 "中国" / 删掉 info 再生成：回到 ltr（spine 没有 rtl），且指纹和改之前不判日漫时相同
6. [x] `fp_golden.rs` 加 `comic_meta_direction` mutation；`UPDATE_FP_GOLDEN=1 cargo test -p library --test fp_golden`；`git diff` 应只多出新 mutation 的行，旧行 0 变化
7. [x] 文档：`direction.rs` 注释、`docs/devices.md`（新键一行；掌阅那段写明：Bangumi 判成日漫的书现在是 rtl，要在掌阅里每本选一次「本书自定义 → 排版 → 漫画」才铺满，vinci 2026-11-10 接受）、`docs/decisions.md`、`docs/architecture.md#指纹`、`docs/usage.md`
8. [x] `cargo test --workspace` 全过；`cargo clippy --workspace --all-targets -- -D warnings` 0 警告

## 验收标准
- [x] 功能：Bangumi 判成日本的漫画，四台产物方向都是 rtl（EPUB/KEPUB spine、KFX 方向）；CBZ 来源也一样
- [x] 功能：不是 Bangumi 来源、地区不是日本 / 判不出、`meta --fetch --clear` 之后：方向照原书，指纹回到不判日漫时的值
- [x] 功能：旧 meta.json（Bangumi、没有 region）`needs_metadata` 返回 true；有 region 的返回原来的结果
- [x] 测试：`cargo test --workspace` 全部通过；clippy 0 警告；地区判定单元测试覆盖「约束」里每条分支
- [x] 不回归：`fp_golden.txt` 旧行 0 变化；文字书、非日漫漫画产物逐字节不变（结果里说明怎么确认的，或写「待本机回归确认」）
- [x] 「不做」的事确实没做
- [ ] 真机（vinci，合并后）：四台各看一本 CBZ 日漫点左侧是下一页、一本国漫不变；**Kobo Nickel、汉王的 rtl 没确认过，必看**；掌阅先每本选「漫画」排版再看是否铺满

## 交付物
代码提交到上面的分支并推送；「结果」填好；状态改为 done 或 blocked。

## 给执行方的指令
```
执行任务单 .dev/tasks/2026-11-10-manga-rtl-bangumi.md：先把状态改为 accepted 并提交，按「分步任务」做完，填好「结果」，状态改为 done（做不下去改为 blocked 并写明原因），提交并推送分支。
```

## 结果

- 执行方 / 开始–结束时间：opencode-local / 2026-11-10 10:50–11:20 +08:00
- base..head：`0fc241a..HEAD`（`origin/master` 到返工后的分支头；代码提交 `394eff8`、`4d2dd41`、`a1c4bdb`，任务单提交随本文件更新）
- 提交：

  | 步骤 | 提交号 | 文件 | 说明 |
  |---|---|---|---|
  | 接受任务 | `394eff8` | `.dev/tasks/2026-11-10-manga-rtl-bangumi.md` | 状态改为 accepted |
  | 1–8 | `4d2dd41` | `metadata.rs`、`bangumi.rs`、`generate.rs`、`profile/src/lib.rs`、四个 profile、`fp_golden`、`flow.rs`、`direction.rs`、文档 | Bangumi 地区、`comic_meta_direction`、按书 rtl、指纹标记、测试和文档 |

- 测试：

  | 命令 | 通过 / 失败 / 未跑 | 摘要 |
  |---|---|---|
  | `cargo test -p profile` | 通过 | 12 个测试全过，含四台 `meta_direction` 断言 |
  | `cargo test -p library` | 通过 | 66 个单元测试 + 13 CLI + 45 flow + fp_golden 全过 |
  | `cargo test --workspace` | 通过 | 全部通过 |
  | `cargo clippy --workspace --all-targets -- -D warnings` | 通过 | 0 警告 |
  | `UPDATE_FP_GOLDEN=1 cargo test -p library --test fp_golden` | 通过 | 金样只多出 8 行 `comic_meta_direction`，旧行 0 变化 |

- 功能检查（离线，测试不联网）：
  - `bangumi::tests::region_from_meta_tags_and_tags`：用任务单给的 6 个样本（死亡笔记、迷宫饭、哆啦A梦、浪客行、一人之下、我独自升级）断言 日本 / 日本 / 日本 / 日本 / 中国 / 韩国；`region_is_empty_when_ambiguous_or_not_manga` 覆盖 meta_tags 同时有日本和中国、tags 并列 3:3、`platform = "小说"`、没有地区词。
  - `metadata::tests::comic_direction_only_for_bangumi_japanese_comics`：只有「设备开、漫画、Bangumi 来源、地区日本」返回 `Rtl`；国漫、空地区、没地区、豆瓣、非漫画、设备没开都是 `None`。
  - `flow::bangumi_japanese_comic_builds_rtl_and_others_stay_as_is`：入库一本合成 CBZ，把 `meta.json` 的 `info.region` 写成「日本」，实际生成四台：ireader/kobo/hanvon 的 OPF spine 读回 `PageDirection::Rtl`，kindle 用 `kfx` crate 读回文档数据 `$560 = $559`（rtl）；改成「中国」重新生成后 spine 没有 rtl，删掉 info 后指纹回到基线。
  - `metadata::tests::needs_metadata_refetches_bangumi_without_region`：旧 Bangumi 记录（有/没有 `info_checked`）没记地区返回 true；记了地区（含空串）按原来的规则。
- 不回归怎么确认：`fp_golden.txt` 只有 8 行新增（四台 × 文字书/漫画的 `comic_meta_direction` mutation），旧行 0 变化；`comic_meta_direction` 不进设备指纹段，只有满足「Bangumi + 日本 + 漫画」的书在生成计划 `info` 段后加 `r`，其余书 `page_direction` 仍是 `None`。**没跑真书回归、没上真机**，真书产物逐字节不变待本机回归确认。
- 遇到的问题：无。
- 偏离：`crates/library/src/douban.rs` 的 `BookInfo` 字面量补了 `region: None`（加字段后必须补，行为不变）；其余都在「涉及文件」内。
- 未完成：真机验证是合并后 vinci 的事（任务单验收最后一项）：四台各看一本 CBZ 日漫点左侧下一页、一本国漫不变；Kobo Nickel、汉王的 rtl 没确认过必看；掌阅先每本选「本书自定义 → 排版 → 漫画」再看铺满。

### 返工（2026-11-10）

- 接受返工：`3f5249f`（状态改回 accepted）。
- 返工代码：`a1c4bdb`（4 个文件：`metadata.rs`、`generate.rs`、`flow.rs`、`docs/usage.md`）。
- **阻断 1（旧 Bangumi 记录重找死循环）**：
  - `fetch_metadata_inner` 拆出 `fetch_metadata_with_chain`（测试可换假网站）；重找进 `NotFound` 分支时，如果现有 `m.info` 是 Bangumi 且 `region.is_none()`，记成 `Some(String::new())` 并存下，`needs_metadata` 随即变 false。网络出错（`Failed` / `Err`）不记，下次再试。
  - `series_lookup` 选同一套别的卷时先选 `info.region.is_some()` 的（再按 `info_checked == LOOKUP_VERSION`、再其余）；抄过来的旧 Bangumi info 也没 region 时记成 `Some("")`。`series_lookup` 的「同一套找过、没找到」分支同样把现有 Bangumi info 记成 `Some("")`。
  - 测试：`metadata::tests::bangumi_not_found_marks_region_checked`（旧 Bangumi 记录 region None → 假网站 NotFound → 存成 `Some("")`、`needs_metadata` false、再判不联网）；`metadata::tests::series_lookup_prefers_info_with_region`（两卷，一卷旧但记了地区、一卷新但没记，抄到的是有地区的；两卷都没地区时抄成 `Some("")` 后不再重找）。
- **阻断 2（国漫原书 rtl 没测试）**：`flow::bangumi_japanese_comic_builds_rtl_and_others_stay_as_is` 末尾加一本合成 EPUB 漫画：OPF `<spine page-progression-direction="rtl">`、`dc:subject`「漫画」，`info` 写 Bangumi + 地区「中国」，生成 ireader 后读回 spine 仍是 rtl。
- **建议（三条都做了）**：
  - `comic_direction(meta, device, comic)` 改收 `Library::comic_of` 算出的布尔；`plan()` 里把 `comic_of` 提前到指纹计算前，`produce()` 也取同一个结果，早期条目 `Meta::comic` 是 `None` 时也不漏。
  - `docs/usage.md` 补「书里没封面的也会重找封面」和「没找到的会记成查过、判不出，不再每轮重试」。
  - 「结果」的 base..head 改成 `0fc241a..HEAD`，不再写死旧提交号。
- 测试：`cargo test --workspace` 全过（library 69 单元 + 45 flow + 其余；0 失败）；`cargo clippy --workspace --all-targets -- -D warnings` 0 警告。没跑真书回归、没上真机。
- 偏离：返工没有新增「涉及文件」以外的文件（`fetch_metadata_with_chain`、新测试都在 `metadata.rs` / `flow.rs`）。

## 审查
<!-- 负责 bot 填写 -->
- 验收：**退回**（sheng-ren，2026-11-10，box 上 `85088ec` 重跑；状态退回 pending，同一执行方 opencode-local 返工）
  - 做到的（附证据）：
    - 范围：`git diff --stat master...HEAD` 18 个文件，除 `douban.rs`（`region: None`，已在「偏离」报告，接受）外都在「涉及文件」内
    - 地区判定 `bangumi.rs` `region_of`：先 `meta_tags`（恰好一个地区才用、两个以上判不出），再按 `tags` 各地区 count 之和取唯一最高，并列 / 没有 → 空串；非「漫画」→ 空串；不用假名。单元测试覆盖 6 个实测样本和 4 种判不出
    - 生成：`metadata.rs` `comic_direction` 只在 `comic_meta_direction` 开、`meta.comic == Some(true)`、来源 Bangumi、地区「日本」时给 Rtl；`generate.rs` `produce()` 设 `opts.page_direction`，`plan()` 在 `info` 段后加 `r`。四个 toml 都开，ireader 注释和 `docs/devices.md` 写明掌阅每本选「本书自定义 → 排版 → 漫画」；`docs/decisions.md` 写明推翻 10-15 / 10-16「照书」里日漫那部分
    - `flow.rs` `bangumi_japanese_comic_builds_rtl_and_others_stay_as_is`：CBZ 在 ireader / hanvon（EPUB spine）、kobo（KEPUB spine）、kindle（KFX `$560` = rtl）都断言了；国漫、去掉 info 后指纹回到基线、spine 没有 rtl
    - fp_golden：旧行 0 变化，只多出 `comic_meta_direction` mutation 的 8 行（四台 × 文字书 / 漫画），和各自 `none` 行相同——金样书没有元数据，开关本身不进设备段，符合设计
    - 测试：`cargo test --workspace` 23 组 763 个通过、0 失败；`cargo clippy --workspace --all-targets -- -D warnings` exit 0；8 个改动的 .rs 文件 `rustfmt --edition 2021 --check` 全过
    - 提交合规：中文前缀加正文、无 Co-Authored-By、没动 master、没合并；`4d2dd41` 写了没跑真书回归、没上真机
  - **阻断 1（要改）：旧 Bangumi 记录重找会无限循环**。`needs_metadata`（`metadata.rs` 约 553 行）对「Bangumi 来源、`region` 是 None」一律返回 true、不看 `info_checked`；`fill_metadata`（`bin/booklib/main.rs:1587`）于是 `force` 重找。但重找结果是 `NotFound` 时（`fetch_metadata_inner` 的 else 分支，约 470 行）只写 `info_checked`，旧 `info` 原样留着、`region` 仍是 None → 以后每次 `sync`（`--watch` 每一轮）都再联网找一次，永远停不下来。同样，`series_lookup`（约 519 行）复制同一套别的卷的 info 时只看 `info_checked == LOOKUP_VERSION`，会把另一卷还没记地区的旧 info 抄过来（这一卷 `region` 仍是 None），下次再触发重找。要改：
    1. 重找后没拿到新的 Bangumi 结果（`NotFound`，以及 `Shared` 抄来的 info 也没有 region）时，把这本书现有 Bangumi info 的 `region` 记成 `Some(String::new())`（＝查过、判不出）并存下，`needs_metadata` 随即变 false；网络出错（`Failed` / `Err`）不记，下次再试
    2. `series_lookup` 选同一套别的卷时，优先 `info.region.is_some()` 的（再按原来的 `info_checked == LOOKUP_VERSION`、再其余）
    3. 测试（照 `metadata.rs` 测试里现有的假网络 / 计数写法）：旧 Bangumi 记录（region None）重找得到 NotFound → 记成 `Some("")`、`needs_metadata` 变 false、再调一次不联网；同一套两卷，一卷已有 region、一卷 None，后者 Shared 抄到的是有 region 的那份
  - **阻断 2（要补测试）：vinci 定的「Bangumi 判成国漫 / 韩漫、原书写了 rtl 的照原书」没有测试**。代码逻辑上是对的（`comic_direction` 返回 None，走原书），但验收标准要求证明：在 `flow.rs` 加一本 OPF spine 写了 `page-progression-direction="rtl"` 的 EPUB 漫画，info 写 Bangumi + 「中国」，生成 ireader（或任一台）后 spine 仍是 rtl
  - 建议（不阻断，顺手改最好）：
    - `comic_direction` 只认 `meta.comic == Some(true)`：早期条目 `comic` 还是 None 时不会写 rtl，直到别的命令把 `comic` 存下。`plan()` 里已经用 `self.comic_of(meta)?` 算出了 `comic`，可以改成把这个结果传进 `comic_direction`（`produce()` 同样取 `comic_of`），两处口径一致
    - `force` 重找对书里没有封面的书也会重找封面（原有行为，「没记网站书名」那条也一样）：`docs/usage.md` 那句「下一次 sync 会重找一次」后补半句「书里没封面的也会重找封面」
    - 「结果」里 base..head 写到 `4d2dd41`，分支头是 `85088ec`
- 返工验收（第 1 轮返工，`3542664`）：**退回**（sheng-ren，2026-11-10，box 上重跑；状态退回 pending，同一执行方 opencode-local）
  - 做到的（附证据）：
    - 阻断 1：`fetch_metadata_with_chain` 的 NotFound 分支（`metadata.rs` 约 488–508 行）把旧 Bangumi info 的 `region` 记成 `Some("")`；`Failed` / `Err` 不走这个分支，网络出错不记（代码核对，没有专门测试，见建议）；`series_lookup`（约 549–557 行）按「有地区且当前版本 → 有地区 → 当前版本 → 其余」挑。测试 `bangumi_not_found_marks_region_checked`（假网站 `Fake`：查 1 次、记成 `Some("")`、`needs_metadata` 变 false、计数仍 1）、`series_lookup_prefers_info_with_region`（抄到有地区的 01 卷）
    - 阻断 2：`flow.rs` 新增 OPF spine 写 rtl 的 EPUB 漫画 + Bangumi「中国」，ireader 产物 spine 仍是 rtl
    - 建议：`comic_direction` 改成收调用方的 `comic`（`generate.rs` 的 `plan()`、`produce()` 都用 `comic_of`）；`docs/usage.md` 补了「书里没封面的也会重找封面」「没找到的记成查过」；「结果」base..head 写成 `0fc241a..HEAD`
    - 测试：`cargo test --workspace` 23 组 765 个通过、0 失败；clippy exit 0；8 个改动的 .rs 文件 `rustfmt --edition 2021 --check` 全过；`fp_golden.txt` 相对 master 仍只多 8 行、0 行删除，返工没动金样
  - **阻断 3（返工新引入，根子在我上一轮审查第 1 条里「Shared 抄来的 info 也没有 region 时记成 Some("")」这句，是我写错了）**：`series_lookup` 第 561–565 行把抄来的、没记地区的旧 Bangumi info 记成 `Some("")`。场景：升级后第一次 `sync`，一套漫画里新加的一卷（`info` 是 None、不 force）排在同一套旧卷前面处理（`list()` 按 id 排，id 是内容哈希，先后随机），它从旧卷抄到没地区的 info 并记成 `Some("")`；随后旧卷 force 重找拿到「日本」，这一卷却永远是「判不出」——同一套日漫里这一卷照原书 ltr，以后也不会再自动纠正（`needs_metadata` 已是 false）。要改：
    1. 删掉 `metadata.rs` 561–565 行的记 `Some("")`：抄来的 info 没地区就原样抄（`region` 仍 None）。不会死循环：下一次 `sync` 这一卷 `needs_metadata` 为 true → force，新一轮开始时 `series_fetched` 为 false → 这一卷自己联网重找，结果是 Found（有地区）或 NotFound（记 `Some("")`），都会停
    2. `series_lookup_prefers_info_with_region` 后半段改成断言：抄来的 region 仍是 None、`needs_metadata` 为 true；再加一步：把 01 卷改成有地区「日本」后再 `series_lookup`，02 卷抄到「日本」
    3. 「另一卷这一版没找到」分支（约 571–579 行）给本卷记 `Some("")` 可以保留（这一卷自己有 info、同一套都没找到）
  - 建议（不阻断）：加一个网络出错的测试（假网站报临时错误 → 返回 Err / Failed，`region` 仍 None、下次还会重找）
- 返工后：sheng-ren 重新验收，通过后再请 vinci 跑 agy 二审（回执 `.dev/tasks/2026-11-10-manga-rtl-bangumi.agy.md`）
- 二审：未开始（等返工验收通过）
- 合并：
