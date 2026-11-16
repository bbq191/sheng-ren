# Kobo sync 的 KEPUB CSS 提醒：判得更准，并汇总成一行

- Task ID: 2026-11-16-kepub-css-warning
- 负责 bot: sheng-ren
- 状态: accepted
- 执行方: 待定
- 分支: task/2026-11-16-kepub-css-warning（从 master `7c1399f` 之后的最新 master 切出）
- 更新时间: 2026-11-16T10:58:26+08:00
- 引用: vinci 2026-11-16 在 sheng-ren 聊天里反馈 kobo sync 时「好多的 ⚠ KEPUB：书里的 CSS 有裸 div/span 或 body > 选择器……」警告；负责 bot 给了三种处理，vinci 选「按 1+2 开单」。

## 需求原文
> （vinci 2026-11-16，sheng-ren 聊天）在 kobo sync 的时候有好多的「⚠ KEPUB：书里的 CSS 有裸 div/span 或 body > 选择器，可能匹配到新加的包装或 koboSpan（只提醒，没改 CSS）」警告。→ 按 1+2 开单。
>
> 方案 1（汇总）：sync 结束时只报「N 本书 CSS 有这类选择器」，具体书目放到能查的地方。方案 2（判得更准）：只在命中规则里带会影响版面的属性时才报。

## 输入（开单时 master `7c1399f` 的现状，执行时以代码为准）

### 警告从哪来、怎么到 sync 输出
1. `crates/bookconv/src/kepub.rs`：
   - `risky_selectors(css) -> bool`（约 `:419`）：按 `crate::cascade::{rule_spans, rule_selector, split_top}` 切规则和选择器，选择器里有裸 `div`/`span`（可带伪类）或 `body >` 就算，**不看声明**。
   - `epub_to_kepub`（约 `:452`）：对书里**每个** `.css` 条目和每章 `<style>` 调它，任一命中就在 `Report::warnings` 推一条（约 `:515`）。注意它扫的是**优化器产出的 EPUB**，里面有我们自己写进去的 `eink-*.css`（`eink-wash.css`、`eink-box.css`、`eink-pb.css`、`eink-blank.css`、`eink-spacer.css`、`eink-body.css`、`eink-vertical.css`、`eink-ua.css` 等，名字见 `crates/bookconv/src/wash/*.rs`、`uastyle.rs`、`vertical.rs` 的 `CSS_NAME`/`*_CSS_NAME`）。
   - 现有单测：`risky_selector_detection`（约 `:871`）、`whole_book_mimetype_first_nav_untouched`（约 `:884`，断言 `warnings.len() == 1`）。
2. `crates/library/src/generate/produce.rs:48-52`：`Library::produce` 把 `kp.warnings` 并进本书的 `warnings`。
3. `crates/library/src/generate/flow.rs`：`warnings` → `Made` → `Transfer` → `Built::Written { warnings }` / `Built::Same { warnings }`。
4. `crates/library/src/bin/booklib/main.rs`：`built_line`（约 `:719`）把每条 warning 打成该书结果行下面一行 `  ⚠ …`；`cmd_sync` 的每轮（约 `:1892`）在 `library::build_all` 回调里逐件 `event_line`，结束打 `counts_summary`（约 `:623`）。`--watch` 每轮同样。
5. 另一个入口：`crates/bookconv/src/bin/epub_to_kepub.rs`（单本命令行工具）打印 `rep.warnings`。单本工具每次只有一本，**保留逐本提醒**（措辞跟新判定一致）。
6. **生成记录与指纹**：`warnings` 不进生成记录、不进指纹；生成记录（`generate/types.rs` 的 `Entry`）里有 `audit: Vec<String>`（`#[serde(default, skip_serializing_if = "Vec::is_empty")]`），由 `produce` 返回、记进记录、`booklib list` 显示（`main.rs` 约 `:1180`，现只显示 `✗`、`?` 开头的行），**不进指纹**（`crates/library/src/fingerprint.rs` 不读它）。

### 现有的「按书重复」的同类提醒（本单**不改**，列为线索）
- `bookconv::audit::summary`（`produce.rs:80`）：每本文字书一两行 `⚠ 体检：…`（只有开了 `kindle_rules` 的掌阅会有内容）。
- `质量门未过：…` / `质量门未过（KEPUB）：…`（`produce.rs`）：每本每条错误一行。
- KFX 写出器的 `warnings`（`kfx::write`，如「目录项找不到位置：…」）。
执行时再搜一遍，若有别的，写进「结果 · 线索」，不要顺手改。

## 设计（开单时定的，执行方照做；有更小的等价做法可以，但要在结果里写明理由）

### A. 判定（方案 2）
新判定替换 `risky_selectors`（可改名，如 `risky_css`），仍只读、不改 CSS：

1. **只看书本来的 CSS**：文件名（去掉目录）以 `eink-` 开头的样式表跳过（都是我们写进去的，比如 `eink-vertical.css`）。章内 `<style>` 照旧查——优化器只往章节里加 `<link>`、不写 `<style>`；执行时核实，如有我们写进 `<style>` 的，同样排除并写明。`vertical.rs` 约 `:1040` 的 `div{position:absolute;...}` 只是单测输入，不是注入的样式；竖排改写过的书本来的 CSS 照旧算书的。
2. **选择器**沿用现有识别（裸 `div`/`span` 类型选择器，可带伪类；`body >` 子代），实现照旧复用 `crate::cascade::{rule_spans, rule_selector, split_top}`；`@media`/`@supports` 里的规则照样看（`rule_spans` 已展开），`@font-face`、`@page` 这类不看。注释、字符串里的东西不算（`rule_spans` 的词法已处理）。
3. **声明**用 `crate::cascade::parse_decls(r.body)` 切（会展开 `margin`、`border`、`background` 等简写）。
   - **裸 `div` / 裸 `span` 命中**：规则里有一条「影响版面」属性（清单见下）**且值不是无害值**才算。无害值：所有长度为 0（`0`、`0em`、`0px`、`0%`…）、`none`、`auto`、`normal`、`transparent`、`initial`、`inherit`、`unset`、`static`（position）、`baseline`（vertical-align）；`display` 等于元素本来的值（`div` 的 `block`、`span` 的 `inline`）也算无害。理由：包装是 `div`、koboSpan 是 `span`，这些值和元素缺省一样，套上去不改版面；书里常见的 `div{margin:0;padding:0}` 这类「清零」就不再报。
   - **`body >` 命中**：插了包装以后，`body > p` 这类规则**不再匹配原来的元素**（它们挪到了包装里面），只会匹配包装 `div`。风险是「规则失效」而不是「套错」，所以**不看值**：规则里有任一「影响版面」属性就算（`body > p{margin:0}` 要报）。
   - **影响版面的属性清单**（展开简写以后按属性名前缀判，大小写不敏感）：
     - 盒子：`margin*`、`padding*`、`border*`（含 `border-*-width/style/color`，不含 `border-*-radius`）、`width`、`height`、`min-width`、`min-height`、`max-width`、`max-height`、`box-sizing`；
     - 背景：`background*`（包装铺满整页，背景色、背景图会盖整页）；
     - 排布：`display`、`position`、`top`、`right`、`bottom`、`left`、`float`、`clear`、`overflow*`、`column*`（含 `columns`）、`transform`；
     - 行与缩进：`text-indent`、`line-height`、`vertical-align`（`span` 上会把每句抬高压低）。
     理由：这些会改盒子大小、位置、流式排布或铺色，套到每页一个的包装或每句一个的 koboSpan 上，版面会变。
   - **不算**：`color`、`font*`、`text-align`、`text-decoration`、`letter-spacing`、`word-spacing`、`white-space` 等其余属性（vinci 2026-11-16 定的范围：只改颜色、字号这类不报）。⚠ 已知局限写进代码注释：`span{font-size:.8em}` 会套到每句的 koboSpan 上让正文变小、嵌套时还会叠乘；这类不报是按定的范围取舍，不是「无害」。
4. 函数返回命中的规则（选择器原文，去掉多余空白，最多留 3 条够看）而不只是 bool，给下面的记录用。

### B. 报告（方案 1）
1. `kepub::Report` 不再往 `warnings` 推这条；改成单独的字段（比如 `risky_css: Vec<String>`，命中的选择器，最多 3 条）。`epub_to_kepub` 单本命令行工具照旧在非空时打一行提醒（措辞见下）。
2. `Library::produce`：`risky_css` 非空时
   - 往本书这次的 `audit` 清单加一行（以 `⚠ ` 开头，如 `⚠ KEPUB：CSS 里 div{…}、body > p{…} 可能作用到包装或 koboSpan（只提醒，没改 CSS）`），这样会记进生成记录，`booklib list` 能查到是哪些书；
   - 让 sync 能数出这本书（做法任选其一，结果里写明：给 `Produced`/`Made`/`Built::Written|Same` 带一个标记；或用 `bookconv::kepub` 里的一个公开常量字符串放进 `warnings`，`main.rs` 认出来、不逐本打而是计数）。**不要**改指纹、`Entry` 以外的记录格式（`audit` 已有，`serde(default)`，旧记录照读）。
3. `main.rs`：
   - `built_line` 不再逐本打这条；每轮（`cmd_sync` 一轮 `build_all` 结束、`counts_summary` 那里；`--watch` 每轮同样）计数，有的话**只打一行**：`⚠ KEPUB：N 本书的 CSS 可能作用到新加的包装或 koboSpan（只提醒，没改 CSS；哪几本用 booklib list 看）`。N 是这一轮重新生成（`Written` + `Same`）且命中的书本数，同一本书只算一次。
   - `list`：生成记录 `audit` 里 `⚠ ` 开头的行照原文打出（和「体检」同一缩进，前缀可以写 `提醒`），现在的 `✗`、`?` 显示不变。
4. 别的入口：执行时 `rg epub_to_kepub` 再核一遍，所有会生成 KEPUB 的路径都按上面处理。

### C. 不影响产物
只改判定和报告：KEPUB 产物逐字节不变；生成记录多一行 `audit`（不进指纹）。**不升任何版本号**（`bookconv::kepub::VERSION` 等都不动，理由：版本号只为产物和指纹，`docs/development.md#版本号`；警告和 `audit` 都不进指纹），`fp_golden.txt` 一行不动。

## 约束
- 只用 Rust；clean-room；仓库不出现上游项目名及缩写。注入 CSS 用 `eink-` 前缀（本单不新增）。
- 书的内容一个字不多一个字不少；**KEPUB 产物逐字节不变**。
- 不升版本号；不改指纹构成；`fp_golden.txt` 不动。
- 不碰 `~/.local/share/booklib`、`~/Documents/ereader/books`、真机；实验用 `target/` 下临时 `--library`；测试不碰真设备、不联网。
- `install.sh` 不用 `--force`。别对整个仓库跑 `cargo fmt`。
- 提交信息中文 + 前缀 + 正文写原因，不加 Co-Authored-By。
- 只处理这一条提醒；其他按书重复的提醒只列线索，不改。

## 分步任务
1. [ ] 核对「输入」里的链路和行号；`rg epub_to_kepub`、`rg "warnings"` 确认入口和同类提醒，写进结果。
2. [ ] 改 `kepub.rs` 的判定（A），更新 `risky_selector_detection` 和 `whole_book_mimetype_first_nav_untouched` 到新语义。
3. [ ] 改报告（B）：`kepub::Report`、`produce.rs`、`flow.rs`/`types.rs`（如需要）、`main.rs` 的 `built_line`、每轮汇总、`list`；`epub_to_kepub` 单本工具。
4. [ ] 测试（见验收标准），`docs/kepub.md` 约 `:27` 和 `docs/usage.md`（sync 输出说明约 `:156`、`list` 说明约 `:193`）同步现行说法。
5. [ ] `cargo test --workspace` 全过；`cargo clippy --workspace --all-targets -- -D warnings` 0 警告；确认版本号和 `fp_golden.txt` 都没动。

## 验收标准
- [ ] 单测（`kepub.rs`）：
  - `span{color:red}`、`div{font-size:1.2em;font-family:serif}` 不报；
  - `div{margin:1em}`、`span{display:block}`、`div{border:1px solid}` 报；`div{margin:0;padding:0}`、`div{display:block}`、`span{display:inline}` 不报；
  - `body > p{margin:0}` 报；`body > p{color:red}` 不报；
  - `@media screen { div { padding: 1em } }` 报；
  - 注释里（`/* div{margin:1em} */`）、字符串里（`p::after{content:"div{margin:1em}"}`）的不算；`div.x`、`p span.y`、`tbody > tr`、`.body > p` 照旧不算；
  - 整本：`eink-` 开头的样式表里的 `div{position:absolute;...}` 不算；书本来的样式表、章内 `<style>` 里的算；`Report` 的命中字段内容对、`warnings` 里不再有这条。
- [ ] 汇总：有单测（`main.rs` 的纯函数或 `library` 集成测试，用临时书库、kobo 模式，不碰真设备）证明一轮 3 本里 2 本命中时只出一行、`N` = 2，逐本结果行下面不再有这条；`list` 显示记录里的 `⚠` 行。
- [ ] KEPUB 产物逐字节不变（负责 bot 验收时用改动前的 master 二进制对照）。
- [ ] 版本号不变；`fp_golden.txt` 一行没动；指纹构成没动。
- [ ] `docs/kepub.md`、`docs/usage.md` 现行段落已同步。
- [ ] `cargo test --workspace` 全过；clippy `-D warnings` 0 警告。
- [ ] 结果里写明没跑真书回归、没上真机，列出发现的同类刷屏线索。

## 涉及文件
- `crates/bookconv/src/kepub.rs`、`crates/bookconv/src/bin/epub_to_kepub.rs`
- `crates/library/src/generate/produce.rs`（可能还有 `flow.rs`、`types.rs`）
- `crates/library/src/bin/booklib/main.rs`
- `crates/library/tests/*`（汇总的测试，如需要）
- `docs/kepub.md`、`docs/usage.md`

## 交付物
代码提交到任务分支并推送；「结果」填好；状态改为 done 或 blocked。任务单状态改动推 master，带 `[skip-notify]`。不要合并，不要跑二审。

## 给执行方的指令
```
执行任务单 .dev/tasks/2026-11-16-kepub-css-warning.md。先把状态改为 accepted 并提交，按「设计」「分步任务」做完，填好「结果」，状态改为 done（做不下去改为 blocked 并写明原因），提交并推送分支。任务单 done 推 master 带 [skip-notify]。不要合并，不要跑二审。不升版本号，不改指纹，KEPUB 产物要逐字节不变。
```

## 结果
<!-- 执行方填写 -->

## 审查
<!-- 负责 bot 填写 -->
