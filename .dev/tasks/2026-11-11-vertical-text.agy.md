## 结论

**无阻断**。两轮验收发现的两处阻断（原书竖排 CSS 误转、未读到原件时写死 meta.json）均已妥善修复且有针对性测试；核心功能、优先级、指纹与版本号均对齐任务单与决策记录；25 组合成书回归保证了未开竖排设备及横排书逐字节不回归。存在几处关于 CSS `!important` 解析、原生竖排判定边界及实体识别的**建议**，但不阻碍合并。

---

## 问题分档

### 阻断
无。

---

### 建议

1. **`rotate_box_values` 遇到 `!important` 会导致 token 计数错位破坏 CSS 语法**
   - **现象**：`css_decls` 解析的值保留了末尾的 `!important`。当简写属性带 `!important` 时，`rotate_box_values` 直接按空格拆分 token，将 `!important` 误当成方向值：单值声明（如 `margin: 1em !important;`）会被当成 2 值重排为 `margin: !important 1em;`（非法语法）；2 值声明带 `!important` 会把 `!important` 当成第三值展开为 4 值；4 值声明带 `!important`（长度 5）则无法识别而不旋转。
   - **建议**：在 `rotate_box_values` 处理前先剥离并记录 `!important`，对纯数值完成几何旋转后再追加回末尾。
   - **依据**：[`crates/bookconv/src/vertical.rs:668-679`](file:///home/afu/Projects/sheng-ren/crates/bookconv/src/vertical.rs#L668-L679)、[`crates/bookbase/src/html.rs:924`](file:///home/afu/Projects/sheng-ren/crates/bookbase/src/html.rs#L924)。

2. **`is_natively_vertical` 子串匹配 `rt`/`rp` 易误排除类名，且全局判定范围过宽**
   - **现象**：
     1. `rule_sets_vertical` 在排除干扰元素时使用 `["table", "img", "svg", "ruby", "rt", "rp"].iter().any(|n| sel.to_ascii_lowercase().contains(n))`。因采用子串 `.contains("rt")`，会导致任何选择器中包含 `rt` 的常见类名（如 `.article`、`.part`、`.short`、`.start`、`.report`）即使写了 `writing-mode: vertical-rl`，也会被直接 `continue` 跳过，存在漏判风险。
     2. 判定缺乏作用域限制：除上述 6 个关键字外，任意局部小类（如签名 `.signature`、印章 `.seal`）只要写了 `writing-mode: vertical-*`，就会将整本书判定为原生竖排，导致根元素不注入竖排 CSS、书 CSS 不横转竖，却给 OPF 加上了 spine rtl 和全角/引号替换，使横排书翻页手势反转。
   - **建议**：对选择器标签做分词或按选择器 AST 匹配；限定只有作用于 `html`、`body` 或 `:root` / 全局容器的选择器才视为全书原生竖排。
   - **依据**：[`crates/bookconv/src/vertical.rs:181-198`](file:///home/afu/Projects/sheng-ren/crates/bookconv/src/vertical.rs#L181-L198)。

3. **`fullwidth_segment` 寻找实体分号未排除空白，正文遇未转义 `&` 与英文分号会漏转全角**
   - **现象**：`fullwidth_segment` 查找实体分号时仅限制 `s[i..].find(';').filter(|&r| r <= 40)`，未限定中间不能包含空白或换行。若正文中存在未转义的 `&` 且其后 40 字符内有英文分号 `;`（如 `<p>Tom & Jerry; 2026年</p>`），会将 `& Jerry;` 当作未知实体整体跳过（原样 `out.push_str(r)`），导致 `Jerry` 漏转全角。
   - **建议**：查找 `;` 时限定中间字符必须满足 XML 实体字符集（`[a-zA-Z0-9#xX_.-]`），遇到空白字符立即终止实体匹配。
   - **依据**：[`crates/bookconv/src/vertical.rs:458-477`](file:///home/afu/Projects/sheng-ren/crates/bookconv/src/vertical.rs#L458-L477)。

4. **Kobo 注释弹窗（规则 7）的文档散落在主提交，无法纯净 `git revert b19a963`**
   - **现象**：需求明确要求规则 7 单独提交以便一刀回退。虽配置、测试与金样独立在 `b19a963`，但 4 份文档（`docs/decisions.md`、`docs/devices.md`、`docs/typesetting.md`、`docs/kepub.md`）关于弹窗的叙述混入了主提交 `cb12f7e`。单独 revert `b19a963` 能回退程序行为，但会导致文档脱节，需人工修剪。
   - **依据**：[.dev/tasks/2026-11-11-vertical-text.md:233-234](file:///home/afu/Projects/sheng-ren/.dev/tasks/2026-11-11-vertical-text.md#L233-L234)、[.dev/tasks/2026-11-11-vertical-text.md:250](file:///home/afu/Projects/sheng-ren/.dev/tasks/2026-11-11-vertical-text.md#L250)、[docs/decisions.md:354-355](file:///home/afu/Projects/sheng-ren/docs/decisions.md#L354-L355)。

5. **`border-radius` 四值简写未旋转**
   - **现象**：`rewrite_css` 支持了四个角的单边长写法，但 `border-radius: 1px 2px 3px 4px;` 未进入简写旋转白名单，原样输出。
   - **建议**：后续版本补入 `border-radius` 多值映射。
   - **依据**：[`crates/bookconv/src/vertical.rs:661-665`](file:///home/afu/Projects/sheng-ren/crates/bookconv/src/vertical.rs#L661-L665)。

---

### 线索

1. **段内出现成对的英文英寸双引号会被误当成直引号配对**
   - **现象**：直引号判定仅针对单引号 `'` 做了前后字符的撇号识别（`apostrophe`），对双引号 `"` 未做数字伴随检查。若一段文字中出现两个带双引号的尺寸标注（如 `12" x 14"`），会被 `chunks(2)` 交替配对替换为 `12『 x 14』`。
   - **依据**：[`crates/bookconv/src/vertical.rs:410-428`](file:///home/afu/Projects/sheng-ren/crates/bookconv/src/vertical.rs#L410-L428)。

2. **Kobo 弹窗注释模式下独立注释章被搬空留有空页面**
   - **现象**：原书若有一整章仅放注释，开启 `repair_note_links` 后注释被移入各引用章，该章剩余空 `<body>`。此为 `xochitl` 遗留行为，Kobo 首次触发，真机需关注是否产生翻页白页。
   - **依据**：[.dev/tasks/2026-11-11-vertical-text.md:276](file:///home/afu/Projects/sheng-ren/.dev/tasks/2026-11-11-vertical-text.md#L276)。

3. **`eink-vertical.css` 物理外边距段距可能盖过原书紧凑排版**
   - **现象**：`p{margin-left:1em;margin-right:1em;}` 挂在样式表末尾（盖过原书），对于原书无段距仅首行缩进的书籍，竖排后会统一产生一列间距，需真机确认排版视觉感受。
   - **依据**：[`crates/bookconv/src/vertical.rs:531-532`](file:///home/afu/Projects/sheng-ren/crates/bookconv/src/vertical.rs#L531-L532)。

---

## 重点审查项专项结论

| 审查项 | 结论 | 依据位置 |
|---|---|---|
| **1) Kobo 注释弹窗** | 未超需求（vinci 明确要求）。程序与配置可一刀回退，文档混在主提交中需人工清理。 | [.dev/tasks/2026-11-11-vertical-text.md:21](file:///home/afu/Projects/sheng-ren/.dev/tasks/2026-11-11-vertical-text.md#L21)、[`crates/profile/profiles/kobo.toml:8-10`](file:///home/afu/Projects/sheng-ren/crates/profile/profiles/kobo.toml#L8-L10) |
| **2) 中文判定与原件边界** | 入库时判、早期首次判均已闭环；读不到原件时不写 meta.json 且不缓存；持锁写回与无锁只读边界清晰。 | [`crates/library/src/vertical.rs:86-110`](file:///home/afu/Projects/sheng-ren/crates/library/src/vertical.rs#L86-L110)、[`crates/library/tests/flow.rs:3126-3150`](file:///home/afu/Projects/sheng-ren/crates/library/tests/flow.rs#L3126-L3150) |
| **3) 原书竖排判定** | `if !native` 成功跳过书 CSS 横转竖与根样式注入；但判定正则存在子串误排除与局部类误判全书的边界风险。 | [`crates/bookconv/src/vertical.rs:181-198`](file:///home/afu/Projects/sheng-ren/crates/bookconv/src/vertical.rs#L181-L198)、[`crates/bookconv/src/wash/mod.rs:1072-1075`](file:///home/afu/Projects/sheng-ren/crates/bookconv/src/wash/mod.rs#L1072-L1075) |
| **4) 引号与全角转换** | 弯撇号与直撇号跳过正确，标签/属性/code/ruby/注释未动；实体查找逻辑遇未转义 `&` 与英文分号存在漏转风险。 | [`crates/bookconv/src/vertical.rs:354-430`](file:///home/afu/Projects/sheng-ren/crates/bookconv/src/vertical.rs#L354-L430)、[`crates/bookconv/src/vertical.rs:434-486`](file:///home/afu/Projects/sheng-ren/crates/bookconv/src/vertical.rs#L434-L486) |
| **5) CSS 盒子横转竖** | 4 值简写、三段式 border、圆角长写法、absolute 保护及 @media 穿透改写均正确；带 `!important` 简写存在语法破坏缺陷。 | [`crates/bookconv/src/vertical.rs:569-679`](file:///home/afu/Projects/sheng-ren/crates/bookconv/src/vertical.rs#L569-L679) |
| **6) 书库 vertical 命令** | 目录清单与单本覆盖优先级严格对齐；遍历竖排设备已脱离写死 ID；`__complete vertical` 补全正常。 | [`crates/library/src/vertical.rs:136-196`](file:///home/afu/Projects/sheng-ren/crates/library/src/vertical.rs#L136-L196)、[`crates/library/src/bin/booklib/complete.rs:46-57`](file:///home/afu/Projects/sheng-ren/crates/library/src/bin/booklib/complete.rs#L46-L57) |
| **7) 版本、指纹与金样** | `OPTIMIZE_VERSION 68` 与 `V`/`f` 段生成正确；金样新增 20 行足以证明指纹算法迁移无漂移，但产物正确性依赖 25 组 cmp 与字符账。 | [`crates/bookconv/src/optimize/mod.rs:171`](file:///home/afu/Projects/sheng-ren/crates/bookconv/src/optimize/mod.rs#L171)、[`crates/library/src/generate.rs:149-159`](file:///home/afu/Projects/sheng-ren/crates/library/src/generate.rs#L149-L159) |

---

## 置信度

**高**。
代码与测试已完成完整通读，数据流与 AST 处理分支均有明确代码行号与测试用例佐证；针对 `!important` 解析异常与实体判定边界已做逐字符逻辑推演。

---

## 未核实

1. **真机阅读体验与渲染**：Kobo Clara BW（Nickel 固件）上竖排文字翻页手势、预览注释弹窗交互；汉王 Clear 6 Turbo 上自带阅读器竖排与翻页流畅度；KOReader 上横排退化表现。
2. **海量真书排版回归**：针对真实世界复杂 CSS（多样化的类名、`!important` 覆盖规则、多重嵌套媒体查询）的视觉效果。
