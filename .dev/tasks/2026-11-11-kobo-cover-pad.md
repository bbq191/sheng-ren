# Kobo 书架封面不等高：另做一张补白边到屏幕比例的书架封面

- Task ID: 2026-11-11-kobo-cover-pad
- 负责 bot: sheng-ren
- 状态: verified
- 执行方: opencode-local
- 分支: kobo-cover-pad（从 master `fe5bb44` 切出）
- 更新时间: 2026-11-11T14:10+08:00
- 引用: 无

## 需求原文
> （vinci 2026-11-11 在 Clara BW 书库截图里指出）书库封面不等高
> Kobo 封面补白边统一成屏幕比例

背景：Clara BW 书架按封面**自己的比例**缩到同一宽度、底边对齐（`crates/profile/profiles/kobo.toml:26-27` 的注释，2026-11-10 截屏），比例宽的封面（如《竹光侍》）就矮一截。书架封面取 manifest 里带 `properties="cover-image"` 的那张（同处注释：封面来源探针）。

## 输入
- 现有书架缩略图机制（汉王 / 掌阅 0.754、Kindle 0.65）：
  - profile 字段 `cover_thumb_aspect`、`cover_meta_thumb`：`crates/profile/src/lib.rs:415-423`，校验 `:739`
  - 做图：`crates/bookconv/src/imgopt.rs:1028` `cover_thumbnail`——去扫描窄白边后**居中裁**成比例
  - 接入：`crates/bookconv/src/optimize/streaming.rs:115` 起；OPF：`crates/bookconv/src/wash/cover.rs:219` `add_cover_thumbnail`（缩略图条目 id `eink-thumb`，`<meta name="eink-thumbnail">`；`cover_meta_thumb` 时把 `<meta name="cover">` 指过去）
  - 版本：`crates/bookconv/src/optimize/mod.rs` `OPTIMIZE_VERSION`（现在 66）及历史说明；漫画另有 `COMIC_OPTIMIZE_VERSION`（看缩略图对漫画是否也走这里，照实处理）
- Kobo 屏幕 1072×1448（`kobo.toml` `[screen]`），比例 1072/1448 ≈ 0.7403
- 涉及文件（预计）：`crates/profile/src/lib.rs`、`crates/profile/profiles/kobo.toml`、`crates/bookconv/src/imgopt.rs`、`crates/bookconv/src/optimize/{mod.rs,streaming.rs}`、`crates/bookconv/src/wash/cover.rs`、相关测试、`crates/library/tests/fp_golden.txt`、`docs/decisions.md`、`docs/devices.md`（或现有写书架缩略图的文档）

## 约束
- 不做：不改汉王、Kindle、Bigme 的缩略图行为和产物（逐字节不变）；不动书里的封面页和正文；不裁封面任何内容
- 只在上面的分支工作；不合并；改「涉及文件」以外的文件写进「结果 → 偏离」
- 提交信息中文，`feat:` / `test:` / `docs:` 等前缀 + 正文，不加 Co-Authored-By；不读密钥、`.env`、`auth*`
- 风险点：设备行为（Kobo 书架封面变了、Kobo 有封面的书都重新生成重传）、跨多文件 → 要 agy 二审

## 规则（要实现的行为）
1. profile 新增「缩略图怎么凑比例」：裁（现有行为，默认）或**补白**。补白＝不裁不拉伸，按比例在缺的那一方向两边各补一半白（`#FFFFFF`；黑白屏照旧转灰度），长边上限、JPEG 质量同现有缩略图。补白时**不做**去扫描窄白边那一步（白边本来就要补）。比例差在现有容差内 → 不另做（同现有）。字段名、写法自定（如 `cover_thumb_fit = "pad"`），在 `lib.rs` 写清注释。
2. profile 新增「书架认 `cover-image` 属性」：另做了缩略图时，把 `properties="cover-image"` 从真封面条目**挪到**缩略图条目上（真封面条目去掉这个值、保留它别的 properties）；`<meta name="cover">` 也指向缩略图；书里的封面页 `<img>` 仍引用原图。和 `cover_meta_thumb` 一样要求同时写了 `cover_thumb_aspect`，在 `:739` 一带加校验。字段名自定。
   - 幂等：对我们自己的产物再优化（换设备、重跑）结果一致；换到不做缩略图的设备时，属性和 meta 改回真封面（照现有 `add_cover_thumbnail` 对 meta 的处理方式）。`declared_cover` 之类找「真封面」的地方不能把缩略图当真封面——注意现在是靠 `cover-image` 属性认真封面的，挪走以后要另有办法认（如按 `THUMB_ID` 排除、或记住原条目），写清楚。
3. `kobo.toml`：写 `cover_thumb_aspect = 0.7403`（注释：屏幕 1072×1448，vinci 2026-11-11 定补白统一书架封面高度）、补白、认 `cover-image`；把 26–27 行「不需要书架缩略图」的旧注释改掉（注明推翻）。
4. `OPTIMIZE_VERSION` 66 → 67（漫画那一路若也受影响，`COMIC_OPTIMIZE_VERSION` 也加一），历史说明写清：只有 Kobo 有封面的书变，别的设备逐字节不变。

## 分步任务
1. [ ] `imgopt.rs`：补白版缩略图函数（或给 `cover_thumbnail` 加模式参数），单元测试：宽封面补上下、窄封面补左右、比例已合不做、输出比例在 ±1px 内、原图内容像素不变（中心区域对比）。
2. [ ] profile 两个新字段 + 校验 + 测试（缺 `cover_thumb_aspect` 时报错）。
3. [ ] `streaming.rs` / `wash/cover.rs` 接入；测试：Kobo 产物 OPF 里 `cover-image` 在 `eink-thumb` 上、meta cover 指向它、封面页引用原图；再拿这个产物按汉王 / Bigme / 不做缩略图的设备再优化，`cover-image` 回到真封面；重跑 Kobo 幂等。
4. [ ] `kobo.toml`、版本号、`fp_golden.txt`（照实更新，结果里写变了哪些行：应只有 kobo 行）。
5. [ ] 文档：`docs/decisions.md` 加「2026-11-11 Kobo 书架封面补白到屏幕比例」；描述 Kobo 书架的文档同步。
6. [ ] 合成回归：一张宽封面（如 800×1000）、一张窄封面（如 600×1000）的最小 EPUB，`epub-optimize --device=kobo` 后解包，贴出缩略图尺寸和 OPF 相关几行。

## 验收标准
- [ ] Kobo 产物：书架缩略图比例 0.740（±1px），四周白边补齐、内容未裁；`cover-image` 和 meta cover 都指向它；书里封面页不变
- [ ] 汉王、Kindle、Bigme、掌阅产物逐字节不变（`fp_golden` 只有 kobo 行变）
- [ ] 换设备再优化、重跑都幂等正确
- [ ] `cargo test --workspace` 全过；`cargo clippy --workspace --all-targets -- -D warnings` 0 警告；`cargo fmt --all -- --check` 干净
- [ ] 结果里写明没上真机（真机由 vinci 验：同步后 Kobo 书架封面一样高；旧封面若被 Kobo 缓存没刷新，记下现象）

## 交付物
代码提交到上面的分支并推送；「结果」填好；状态改为 done 或 blocked。

## 给执行方的指令
```
执行任务单 .dev/tasks/2026-11-11-kobo-cover-pad.md：先把状态改为 accepted 并提交，按「分步任务」做完，填好「结果」，状态改为 done（做不下去改为 blocked 并写明原因），提交并推送分支。
```

## 结果
<!-- 执行方填写 -->
- 执行方 / 开始–结束时间：opencode-local / 2026-11-11 13:49–14:10 +08:00
- base..head：`4302b67..HEAD`（`4302b67` 是任务单提交）
- 提交：

  | 步骤 | 提交号 | 文件 | 说明 |
  | --- | --- | --- | --- |
  | 接受 | `deb9fc4` | `.dev/tasks/2026-11-11-kobo-cover-pad.md` | 状态改 accepted |
  | 1–5 | `e70214a` | 见 `git show --stat e70214a` | 补白缩略图、两个 profile 字段、OPF 属性挪动、kobo.toml、版本号、金样、测试、文档 |
  | 结果 | 本提交 | `.dev/tasks/2026-11-11-kobo-cover-pad.md` | 填结果、状态 done |

- 实现：
  - profile 新字段 `cover_thumb_fit = "crop" | "pad"`（缺省 `crop`）、`cover_image_thumb = bool`（缺省 `false`）。`pad` / `true` 没和 `cover_thumb_aspect` 一起写时报错（`crates/profile/src/lib.rs:771-786`）。
  - `imgopt::cover_thumbnail` 加 `fit` 参数：`pad` 不裁不拉伸，按 `cover_thumb_aspect` 补白（缺的方向两边各一半，`#FFFFFF`；黑白屏照旧转灰度），**不做** `thumb_scan_margins`；长边 1200 按补完的画布算，JPEG 质量 90 不变（`crates/bookconv/src/imgopt.rs:1026-1100`）。
  - `wash::add_cover_thumbnail` 新增 `cover_image_thumb`：`properties="cover-image"` 从真封面挪到 `eink-thumb`（真封面保留别的 properties），`<meta name="cover">` 指缩略图，真封面 id 记进 `<meta name="eink-thumbnail-source">`；`pad` 另写 `<meta name="eink-thumbnail-fit" content="pad"/>`。读封面的 `wash::opf::declared_cover` 跳过 `eink-thumb`、按 source meta 认回真封面；`epubbook` 扫 `cover-image` 时也跳过缩略图。换设备/重跑幂等由 `optimize` 测试覆盖。
  - `kobo.toml`：`cover_thumb_aspect = 0.7403`、`cover_thumb_fit = "pad"`、`cover_image_thumb = true`，注释写明 1072×1448、vinci 2026-11-11 定、推翻 11-10「不需要」。
  - `OPTIMIZE_VERSION` 66→67、`COMIC_VERSION` 67→68（漫画和文字书共用同一段缩略图代码，Kobo 漫画产物也会变）。

- 测试：

  | 命令 | 结果 | 摘要 |
  | --- | --- | --- |
  | `cargo test -p bookconv` | 通过 | 538 passed；含 `cover_thumbnail_pads_white_without_cropping`（宽封面 800×1081、窄封面 740×1000、比例已合不做、长边 889×1200、灰度）和 Kobo/汉王/Kindle/Bigme 换设备、重跑幂等 |
  | `cargo test -p profile` | 通过 | 18 passed；`kobo_pads_cover_thumbnail_and_moves_cover_image`，两个新字段缺 aspect 都报错，别的设备缺省值不变 |
  | `cargo test -p library` | 通过 | 71+7+14+46 passed，含重写后的 `fp_golden` |
  | `cargo test --workspace` | 通过 | 全部通过，0 failed |
  | `cargo clippy --workspace --all-targets -- -D warnings` | 通过 | 0 警告 |
  | `cargo fmt --all -- --check` | 通过 | 干净（只格式化了本次改过的文件） |

- `fp_golden.txt`（`UPDATE_FP_GOLDEN=1` 重写后复跑通过）：
  - 124 行 Kobo 的旧行（62 组变异 × 文字书/漫画）设备段按变异后的值变化：基线和多数行是 `t0.7403pC`（`cover_thumb_fit`、`cover_image_thumb` 的 `p`、`C`；`cover_thumb_aspect` 那组去掉比例、`cover_thumb_fit` 那组回到裁、`cover_image_thumb` 那组不挪属性，分别少对应字母）。
  - 其余 496 行老设备的旧行**除版本号 `66→67` / `c67→c68` 外一个字不变**（脚本逐行对拍确认）。任务单写「应只有 kobo 行变」是按设备字段说的；版本号在每一行的指纹里，所以每一行都会看到版本那一段变。
  - 末尾新增 `cover_thumb_fit`、`cover_image_thumb` 两组变异行，每台 4 行（文字书/漫画 × 2），共 20 行；旧行不挪。

- 合成回归（`/tmp/opencode/kobo-pad/`，不提交）：最小 EPUB 各一本，封面 800×1000（宽）、600×1000（窄），`epub-optimize --device=kobo` 后解包：
  - 宽封面缩略图 **800×1081**（内容 1000 高居中，上下共 81px 白），比例 0.7401；窄封面 **740×1000**（内容 600 宽居中，左右共 140px 白），比例 0.7400。
  - 两本 OPF 都有：`<meta name="cover" content="eink-thumb"/>`、`<meta name="eink-thumbnail" content="eink-thumb"/>`、`<meta name="eink-thumbnail-source" content="cv"/>`、`<meta name="eink-thumbnail-fit" content="pad"/>`；`<item id="eink-thumb" properties="cover-image" href="eink-thumb.jpg" .../>`；真封面 `<item id="cv" href="i/c.jpg" .../>` 去掉了 `cover-image`；`cover.xhtml` 仍是 `<img src="i/c.jpg" .../>`。
  - 再跑一次 Kobo：两本产物逐字节相同。
  - 对照 master 二进制：同样两本走 `kindle` / `ireader` / `hanvon` / `bigme`，产物逐字节相同（合成书上验证，不是真书回归）。

- 偏离（改在「涉及文件」以外）：
  - `crates/bookconv/src/wash/opf.rs`：`declared_cover` 跳过带 `cover-image` 的缩略图、并支持 `eink-thumbnail-source` 回退（任务规则 2 要求「另有办法认真封面」）。
  - `crates/bookconv/src/epubbook.rs`：读 `cover-image` 时跳过 `eink-thumb`，否则 Kobo 产物被 KFX/书库路径读成本身是缩略图的封面；并加 source meta 回退。
  - `docs/architecture.md`（指纹字母 `p`/`C`）、`docs/development.md`（版本号现值 67/68）、`docs/typesetting.md`（书架缩略图一节）。
  - 新增 `<meta name="eink-thumbnail-source">`、`<meta name="eink-thumbnail-fit">` 两个约定：前者用来认回真封面，后者区分补白/裁（同一比例换设备时不会被误留）。只写在 Kobo 产物里；别的设备产物逐字节不变。

- 未完成 / 待真机：
  - **没跑真书回归、没上真机**。Kobo 书架封面是否一样高由 vinci 同步后验；旧封面可能被 Nickel 缓存，先看新书或重开书库，若仍显示旧封面记下现象。
  - Kobo 全部书的指纹都会变（版本号进指纹）、下次 `sync` 重建；只有有封面的书产物会变，没封面的重建后逐字节相同、不重传。

## 审查
<!-- 负责 bot 填写 -->
- 验收：通过（sheng-ren，2026-11-11，box 复跑 `2254256`）
  - `cargo fmt --all -- --check` 干净；`cargo clippy --workspace --all-targets -- -D warnings` 0 警告；`cargo test --workspace` 全过（bookconv 538、profile 18、library 71/7/14/46 等，0 failed）。
  - 金样：非 kobo 的旧行把版本段 `|66|→|67|`、`|c67|→|c68|` 抹平后与 `4302b67` 逐行相同（脚本 diff 无输出）；新增的 `cover_thumb_fit` / `cover_image_thumb` 两组变异行每台 4 行。kobo 基线 `kobo none` 设备段由 `…bktu42@…` 变 `…bt0.7403pCktu42@…`，只多了这三项。
  - 行为对照任务单规则 1–4：补白不裁、不做扫描白边；`cover-image` 挪到 `eink-thumb`，meta cover 指过去，真封面靠新 meta `eink-thumbnail-source` 认回；`kobo.toml` 三项和推翻旧注释都在；两路版本号都加一（漫画也走这段代码，合理）。
  - 偏离（`wash/opf.rs`、`epubbook.rs`、三份 docs、两个新 meta 约定）都是为满足规则 2「另有办法认真封面」，接受。
  - 留给二审的线索：
    1. 版本号进每一行指纹，四台全部书下次 sync 都会重建；执行方说字节不变的不重传——请核实传输层是按产物字节判断（Kindle 进度不能因此清零）。
    2. `epubbook.rs` 跳过 `eink-thumb` 的改动是否影响 Kindle 读 `eink-thumbnail` 作书架封面的现有路径。
- 二审：回执 `.dev/tasks/2026-11-11-kobo-cover-pad.agy.md`（`b8f9e83`）：阻断 0、建议 1、线索 1。
  - 建议「`arrange_cover_thumb` 认不回封面时加 debug 日志」：本次不改。只有手工弄坏的 OPF 才会走到，属于以后可做的小改进。
  - 线索「Nickel 书架封面缓存在 `.kobo-images/`」：留给 vinci 真机验收时观察，旧书封面不刷新就记下现象。
  - 验收时我请二审核实的两条（传输层只重传字节变了的书，Kindle 读 `eink-thumbnail` 的路径不受影响）二审都核实通过。
- 合并：`--no-ff` 合并到 master，见合并提交；状态改为 verified
