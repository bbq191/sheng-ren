# Kobo 书架缩略图：真机量 Clara BW 书架封面框比例，写进 kobo.toml

- Task ID: 2026-11-10-kobo-cover-thumb
- 负责 bot: sheng-ren
- 状态: pending
- 执行方: opencode-local
- 分支: feat/kobo-cover-thumb（从最新 master 切出）
- 更新时间: 2026-11-10T10:40+08:00
- 引用: 无（vault PRD `Projects/sheng-ren/prd/2026-11-10-comic-layout-direction.md`「已定事项」7：从那份 PRD 里拆出来单独做）

## 需求原文
> 7. Kobo cover_thumb_aspect: open a separate new ticket（vinci 2026-11-10 答复：Kobo 没写 `cover_thumb_aspect`，另开单）

## 输入
- 背景：vault `Projects/sheng-ren/context.md`（2026-10-30 Kindle 书架缩略图 0.65；2026-11-03 掌阅 0.754 + meta cover；2026-11-06 汉王 0.754）；仓库 `docs/devices.md` 的 `cover_thumb_aspect`、`cover_meta_thumb` 两行（约 173–174 行）和 Kindle / 掌阅 / 汉王的书架截屏记录
- 现状：`crates/profile/profiles/kobo.toml` 只有注释 `# cover_thumb_aspect：Kobo 书架封面框比例未知（待真机截图量）`，没写 `cover_thumb_aspect`、`cover_meta_thumb`；缩略图由 `bookconv::wash::add_cover_thumbnail` 做（v64/c59），KEPUB 是优化后的 EPUB 再插 koboSpan（`crates/bookconv/src/kepub.rs`）
- profile 校验：`cover_meta_thumb` 要和 `cover_thumb_aspect` 一起写（`crates/profile/src/lib.rs:715`）
- **真机测量（开工前由 sheng-ren 和 vinci 做，结果填在下面；没填完执行方不开工，直接改 blocked）**：
  - 测试书：封面比例 0.5 / 0.75 / 1.0 三本（方块、圆、四边刻度，同 2026-11-03 掌阅、11-06 汉王用过的；box 上 `/workspace/hv/封面比例-*.epub`，KEPUB 版由 sheng-ren 用 `epub-to-kepub` 转），再加一本「封面来源探针」（meta cover、`cover-image`、正文第一页各指一张不同的图）
  - vinci 放进 Kobo `books/测试/`，在书架（网格、列表各一）截图（Kobo 设置里开截屏或手机拍正面）
  - 要量出：
    - [ ] 书架封面框像素尺寸和比例：（待填）
    - [ ] 比例不对的封面怎么放：拉伸 / 裁切 / 留白：（待填）
    - [ ] 书架取哪张封面：meta cover / cover-image / 第一页：（待填）
    - [ ] 网格和列表的框比例是否一样：（待填）
- 涉及文件：
  - `crates/profile/profiles/kobo.toml`（修改：按测量写 `cover_thumb_aspect`；书架取的是 meta cover 才写 `cover_meta_thumb = true`；注释写来源和日期）
  - `crates/profile/src/lib.rs`（修改：内置 profile 测试补 kobo 的断言）
  - `crates/library/tests/fp_golden.txt`（修改：只允许 `kobo` 开头的行变化）
  - `docs/devices.md`（两行的 kobo 列、Kobo 那一节加书架截屏结论）
  - `crates/bookconv/src/kepub.rs`（只在测出来 KEPUB 丢了缩略图 / meta cover 时才改，改了写进「偏离」）

## 约束
- 不做：不改 kindle / ireader / hanvon 的值；不改缩略图算法（`add_cover_thumbnail`、`cover_thumbnail`）；不加新 profile 键；不改 install.sh；不碰 Kobo 的 `.kobo/`、KoboReader.sqlite
- 测量结论填不全（「输入 → 真机测量」有「待填」）时不写数，状态改 blocked
- 比例取测量值四舍五入到三位小数（同掌阅 0.754 的写法）
- 只在上面的分支工作；不合并；不改「涉及文件」以外的文件，确需改时写进「结果 → 偏离」
- 提交信息用中文，`feat:` 等前缀 + 正文，不加 Co-Authored-By；正文写「没跑真书回归；Kobo 全部书（文字书、漫画）指纹变，下次 sync 全部重建、多出 eink-thumb 缩略图并重传」
- 不读密钥、`.env`、`auth*`；clean-room：不读 GPL 代码
- 只对改动的 Rust 文件跑 rustfmt；编译用 `CARGO_TARGET_DIR=~/.cache/sheng-ren-target`
- 风险点：设备行为（Kobo 全部书重建重传）→ 合并前 agy 二审

## 分步任务
1. [ ] 核对「真机测量」四项都已填；没填完 → blocked
2. [ ] `kobo.toml`：删掉「比例未知」那行注释，写 `cover_thumb_aspect = <测量值>`（书架取 meta cover 时再写 `cover_meta_thumb = true`），注释写 2026-11-xx Clara BW 书架截屏结论。自检：`cargo test -p profile`
3. [ ] `lib.rs` 补 kobo 断言（`cover_thumb_aspect == Some(<值>)`、`cover_meta_thumb` 的值）
4. [ ] 指纹金样：`UPDATE_FP_GOLDEN=1 cargo test -p library --test fp_golden`；`git diff` 应只有 `kobo` 开头的行变（约 118 行，文字书、漫画段都多出 `bt<比例>`，开了 meta 再多 `M`，同 hanvon 的写法），kindle / ireader / hanvon 0 行变化
5. [ ] 合成测试书跑一遍 KEPUB：`cargo run --release -p bookconv --bin epub-optimize -- --device=kobo 书.epub 优化.epub` 再 `epub-to-kepub`，解开看产物里有 `eink-thumb`，OPF 的封面声明符合第 2 步（结果里贴摘要）
6. [ ] `docs/devices.md` 两行的 kobo 列、Kobo 一节
7. [ ] `cargo test --workspace` 全过；`cargo clippy --workspace --all-targets -- -D warnings` 0 警告

## 验收标准
- [ ] 功能：kobo 产物多出书架缩略图，比例等于测量值；书架取封面的方式和测量一致
- [ ] 测试：`cargo test --workspace` 全部通过；clippy 0 警告
- [ ] 不回归：`fp_golden.txt` 只有 kobo 行变
- [ ] 「不做」的事确实没做
- [ ] 真机（vinci，合并后）：同步一本封面比例不对的书，Kobo 书架上封面不变形、不留白

## 交付物
代码提交到上面的分支并推送；「结果」填好；状态改为 done 或 blocked。

## 给执行方的指令
```
执行任务单 .dev/tasks/2026-11-10-kobo-cover-thumb.md：先把状态改为 accepted 并提交，按「分步任务」做完，填好「结果」，状态改为 done（做不下去改为 blocked 并写明原因），提交并推送分支。
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
