# Kobo 漫画改用 D 方案（固定版式 + 对齐 viewport + 纯图页不加 koboSpan）

- Task ID: 2026-11-05-kobo-comic-fxl
- 负责 bot: sheng-ren
- 状态: accepted
- 执行方: opencode-local
- 分支: feat/kobo-comic-fxl（从最新 master 切出）
- 更新时间: 2026-11-05T16:00+08:00
- 引用: 无

## 需求原文
> 遵守我们漫画定下的规则即可
>
> （真机结论，Clara BW fw 4.45，合成 A/B 包 `/workspace/kobo-ab/`）：A（当前流水线）坏；B、C、E 正常；**只有 D 满屏**——KCC 式：固定版式、viewport 对齐 1072×1448、img width/height 100%、纯图页 img 外**不加** koboSpan（保留 book-columns 包装）。文字 KEPUB 正常，侧载 kepub 书签/笔记可用。

## 输入
- 背景：vault `Projects/sheng-ren/context.md`（漫画规则：伽马 1.1、autocontrast、page_numbers、trim_specks、dark_bg、margin=1、照书 rtl、comic_upscale 缺省开；Kobo USBMS + KEPUB）；仓库 `docs/kepub.md`、`docs/devices.md`、`docs/decisions.md`
- 真机 A/B：`/workspace/kobo-ab/README.txt`（A 坏；B/C/E 正常；D 满屏）
- 当前行为：
  - `kobo.toml` 已 `comic_fixed_layout=true` + `[comic_readable] 1072×1448`（标「推断，待确认」）
  - `comicfxl.rs` 把纯图页写成 `style="width:{w}px;height:{h}px"`，viewport=`width={w}, height={h}`（Kindle 用；Kobo 真机 D 要改成 100%/100%）
  - `kepub.rs` 对纯图页默认也加 `koboSpan` 包 img，并加 `#book-columns`/`#book-inner`；`--no-wrap-image-pages` 只关包装，**仍加** koboSpan
- 涉及文件：
  - `crates/bookconv/src/kepub.rs`（纯图章：pre-paginated 书至少不加 koboSpan 包 img；保留 book-columns，与 D 一致；`VERSION` 加一）
  - `crates/bookconv/src/comicfxl.rs`（及调用处 `optimize/mod.rs` 如需）：Kobo 路径 img 用 `width:100%;height:100%`，html/body（及必要时 `#book-columns`/`#book-inner`）高度链写全；viewport 对齐画布；去掉/覆盖原书错位的 rendition/viewport（现有 `comicfxl::opf`/`page` 已换声明，核对流经 kepub 后仍正确）。**Kindle 行为字节级不变**（仍用 px 画布写法，或等价输出）
  - `crates/profile/profiles/kobo.toml`（`comic_fixed_layout` / `[comic_readable]` 注释改为 2026-11-05 真机确认；漫画其它键沿用既有规则，不改数值）
  - `docs/kepub.md`（「真机 A/B」写结论：选 D；文字 KEPUB 正常；侧载书签/笔记可用；更新缺省说明）
  - `docs/devices.md`（Kobo 节：去掉「未上真机」里已证实部分；漫画固定版式已确认）
  - 单元测试：`kepub.rs` / `comicfxl.rs` 相关测试；`crates/library/tests/fp_golden.*` **只允许 kobo 行变化**（kindle/ireader/hanvon 0 行变）

## 约束
- 不做：不改 ireader/hanvon/kindle 的 profile 数值与漫画版式逻辑；不注入 Kobo 全局样式补丁（维护者定）；不改 install.sh；不改投递/USBMS；不加新 profile 键（除非实现 D 必需且写进「偏离」）；不读 GPL / kepubify 源码（clean-room）
- 漫画处理规则沿用既有（只改版式写出，不关掉这些）：`comic_gamma=1.1`、`comic_autocontrast`、`comic_page_numbers`、`comic_trim_specks`、`comic_dark_bg`、`comic_margin=1`、不写 `comic_page_direction`（照书 rtl）、`comic_upscale` 缺省开
- D 的结构约定（真机满屏那本）：
  1. OPF：`rendition:layout=pre-paginated` + `original-resolution=1072x1448`（及现有 fixed-layout / book-type / orientation-lock / zero-gutter / zero-margin）
  2. 每页 viewport 与画布一致：`width=1072, height=1448`
  3. 保留 `#book-columns` > `#book-inner` 包装
  4. **纯图页**的 `<img>` **外面不加** `<span class="koboSpan">`；文字页照旧按句加 koboSpan
  5. img：`width:100%;height:100%`（可用内联 style 和/或 CSS），配合 html/body 高度 100%
- Kindle：`comicfxl` 对 Kindle 的输出与改前逐字节或功能等价（fp_golden 的 kindle 行不变）；Kobo 与 Kindle 若写法必须分叉，在 `comicfxl` / 调用处按设备分支，注释写明原因
- 只在上面的分支工作；不合并；不改「涉及文件」以外的文件，确需改时写进「结果 → 偏离」
- 提交信息用中文，`feat:` / `fix:` / `docs:` 等前缀 + 正文，不加 Co-Authored-By
- 不读密钥、`.env`、`auth*`
- 只对改动的文件跑 rustfmt；`CARGO_TARGET_DIR=~/.cache/sheng-ren-target`；`source ~/.cargo/env`（若需要）
- 风险点：设备行为（Kobo 漫画版式 / kepub 写出）→ 合并前要 agy 二审

## 分步任务
1. [ ] `kepub.rs`：检测「本章是纯图页」（body 内无可见文字、仅一张 img，与现有 image-only 判定对齐）；对 **pre-paginated**（或「漫画固定版式产物」——实现时选一种并在注释写清）的纯图页：**仍加** book-columns 包装，**不加** koboSpan 包 img；文字页、非 FXL 书行为不变。自检：新增/改单元测试——纯图页输出含 `book-columns`、不含 `koboSpan` 包着的 img；文字页仍有 `kobo.1.1` 类 span
2. [ ] `comicfxl.rs`（+ 调用处）：Kobo 漫画页改 D 写法（100%/100%、对齐 viewport、html/body 尺寸）；去掉原书错位 viewport / 旧 rendition；Kindle 路径保持现状。自检：单元测试覆盖 Kobo 页含 `width:100%`/`height:100%` 与 `viewport`=`1072×1448`；Kindle 用例断言不变
3. [ ] `kepub::VERSION` 加一（输出变了）；指纹金样只更新 kobo 行
4. [ ] `kobo.toml` 注释：`comic_fixed_layout` / `[comic_readable]` 改为「2026-11-05 Clara BW 真机确认（方案 D）」；数值不动
5. [ ] `docs/kepub.md`：改「真机 A/B」为结论（D 入选；文字 OK；书签/笔记侧载可用；缺省即 D，不再靠 `.fxl` 文件名区分版式）；`docs/devices.md` 同步
6. [ ] `cargo test --workspace`；`cargo clippy --workspace --all-targets -- -D warnings` 0 警告

## 验收标准
- [ ] 功能：`--device=kobo` 漫画产物为固定版式 KEPUB；纯图页有 book-columns、**无** koboSpan 包 img；img 为 100%×100%；viewport / original-resolution 为 1072×1448；文字 KEPUB 仍按句加 koboSpan
- [ ] 测试：`cargo test -p bookconv` 含上述断言；`cargo test --workspace` 全过；clippy 0 警告
- [ ] 不回归：`fp_golden.txt` 仅 kobo 行变化；kindle/ireader/hanvon 0 删 0 改；Kindle 漫画固定版式行为不变
- [ ] 「不做」的事确实没做；漫画伽马/对比度/页码/黑底/margin/rtl/upscale 规则未关
- [ ] 文档：`kepub.md` / `devices.md` / `kobo.toml` 注释与真机结论一致

## 交付物
代码提交到上面的分支并推送；「结果」填好；状态改为 done 或 blocked。

## 给执行方的指令
```
执行任务单 .dev/tasks/2026-11-05-kobo-comic-fxl.md：先把状态改为 accepted 并提交，按「分步任务」做完，填好「结果」，状态改为 done（做不下去改为 blocked 并写明原因），提交并推送分支。编译用 CARGO_TARGET_DIR=~/.cache/sheng-ren-target；只 rustfmt 改过的文件；clean-room 不读 GPL/kepubify 源码；Kindle 行为与 fp_golden 非 kobo 行不得变。
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
- 验收：通过 / 退回（附证据：命令输出、路径:行号）
- 二审：回执 `.dev/tasks/<ID>.agy.md`；阻断 / 建议 / 线索，及采纳与否和理由
- 合并：PR 链接、合并提交号；状态改为 verified
