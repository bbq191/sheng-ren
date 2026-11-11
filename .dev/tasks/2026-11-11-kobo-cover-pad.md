# Kobo 书架封面不等高：另做一张补白边到屏幕比例的书架封面

- Task ID: 2026-11-11-kobo-cover-pad
- 负责 bot: sheng-ren
- 状态: pending
- 执行方: opencode-local
- 分支: kobo-cover-pad（从 master `fe5bb44` 切出）
- 更新时间: 2026-11-11T13:55+08:00
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

## 审查
<!-- 负责 bot 填写 -->
- 验收：通过 / 退回（附证据：命令输出、路径:行号）
- 二审：回执 `.dev/tasks/2026-11-11-kobo-cover-pad.agy.md`；阻断 / 建议 / 线索，及采纳与否和理由
- 合并：合并提交号；状态改为 verified
