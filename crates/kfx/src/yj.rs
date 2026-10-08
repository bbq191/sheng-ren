//! KFX 里用到的 `YJ_symbols` 编号。名字没有公开，含义是对照样本和原书推出来的（见 `docs/kfx.md`），
//! 这里起的名字只是我们的叫法。没把握的标了"推测"。

// ---- 片段类型
pub const T_TEXT_POOL: u32 = 145;
pub const T_STYLE: u32 = 157;
pub const T_RESOURCE: u32 = 164;
pub const T_READING_ORDERS: u32 = 258;
pub const T_STORYLINE: u32 = 259;
pub const T_SECTION: u32 = 260;
pub const T_SECTION_EIDS: u32 = 264;
pub const T_SECTION_RANGES: u32 = 265;
pub const T_ANCHOR: u32 = 266;
pub const T_NAV_ROOTS: u32 = 389;
pub const T_NAV_CONTAINER: u32 = 391;
pub const T_NAV_EMPTY: u32 = 395;
pub const T_RAW_MEDIA: u32 = 417;
/// 嵌入字体：字体片段（字体名、字形、字重、宽度、资源路径）和字体字节。
pub const T_FONT: u32 = 262;
pub const T_RAW_FONT: u32 = 418;
pub const T_MANIFEST: u32 = 419;
pub const T_METADATA: u32 = 490;
pub const T_DOCUMENT_DATA: u32 = 538;
pub const T_SECTION_PID_MAP: u32 = 609;

/// 没有名字的片段的 id。
pub const NO_NAME: u32 = 348;

// ---- 通用字段
pub const NAME: u32 = crate::ion::SID_NAME;
pub const EID: u32 = 155;
pub const OFFSET: u32 = 143;
pub const LENGTH: u32 = 144;
pub const CHILDREN: u32 = 146;
pub const TEXT_REF: u32 = 145;
pub const TEXT_INDEX: u32 = 403;
pub const STYLE_REF: u32 = 157;
pub const NODE_TYPE: u32 = 159;
pub const RESOURCE_REF: u32 = 175;
pub const STORYLINE_REF: u32 = 176;
pub const SECTION_REF: u32 = 174;
pub const TEMPLATES: u32 = 141;
pub const RUNS: u32 = 142;
pub const LINK_TO: u32 = 179;
/// 链接区间上：注释引用（`$617`，Kindle 点了弹窗显示注释）。
pub const NOTE_REF: u32 = 616;
pub const NOTE_REF_POPUP: u32 = 617;
/// 段落节点上：注释正文（`$618`）。
pub const NOTE_CONTENT: u32 = 615;
pub const NOTE_CONTENT_FOOTNOTE: u32 = 618;
pub const HEADING_LEVEL: u32 = 790;
pub const STYLE_NAME: u32 = 173;
pub const LIST: u32 = 181;
pub const START: u32 = 184;

// ---- 节点类型
pub const NODE_TEXT: u32 = 269;
pub const NODE_CONTAINER: u32 = 270;
pub const NODE_IMAGE: u32 = 271;
/// 列表、列表项（2026-10-05 测试书对照，见 `docs/kfx.md#列表`）。
pub const NODE_LIST: u32 = 276;
pub const NODE_LIST_ITEM: u32 = 277;
/// 表格：表 → 表头/表体/表脚 → 行 → 单元格（`$270` 容器）。
pub const NODE_TABLE: u32 = 278;
pub const NODE_THEAD: u32 = 151;
pub const NODE_TBODY: u32 = 454;
pub const NODE_TFOOT: u32 = 455;
pub const NODE_ROW: u32 = 279;
/// 水平线 `<hr>`。
pub const NODE_HR: u32 = 596;
// ---- 列表、表格节点上的字段
/// 列表符号（`list-style-type`）。
pub const LIST_STYLE: u32 = 100;
/// 列表的 `start`、列表项的 `value`。
pub const LIST_START: u32 = 104;
/// `list-style-position: inside`：`$551: $552`。
pub const LIST_POSITION: u32 = 551;
pub const LIST_INSIDE: u32 = 552;
pub const LIST_DISC: u32 = 340;
pub const LIST_SQUARE: u32 = 341;
pub const LIST_CIRCLE: u32 = 342;
pub const LIST_DECIMAL: u32 = 343;
pub const LIST_LOWER_ROMAN: u32 = 344;
pub const LIST_UPPER_ROMAN: u32 = 345;
pub const LIST_LOWER_ALPHA: u32 = 346;
pub const LIST_UPPER_ALPHA: u32 = 347;
pub const LIST_CJK: u32 = 736;
pub const LIST_LOWER_GREEK: u32 = 791;
pub const LIST_DECIMAL_ZERO: u32 = 796;
/// `border-collapse: collapse`（布尔）。
pub const TABLE_COLLAPSE: u32 = 150;
/// `border-spacing` 水平、竖直（缺省 2px＝0.9pt）。
pub const TABLE_SPACING_H: u32 = 456;
pub const TABLE_SPACING_V: u32 = 457;
/// 列宽：`[{$56: 宽度}, …]`。
pub const TABLE_COLUMNS: u32 = 152;
/// 表格标题 `<caption>`：`$269` 节点上 `$615: $453`，里面套文字。
pub const CAPTION: u32 = 453;

// ---- 版面模板（整页图片版面）
pub const TMPL_WIDTH: u32 = 66;
pub const TMPL_HEIGHT: u32 = 67;
pub const TMPL_FIT: u32 = 156;
pub const TMPL_FIT_VALUE: u32 = 326;
pub const TMPL_ALIGN: u32 = 140;
/// 推测：容器节点上的 `$156` 取值（样本里带背景的块容器都是它，含义未知）。
pub const CONTAINER_LAYOUT: u32 = 323;

// ---- 样式属性
pub const P_LANG: u32 = 10;
pub const P_FONT_FAMILY: u32 = 11;
/// 推测：font-style（样本里只见过 `$382`，没对上原书）。
pub const P_FONT_STYLE: u32 = 12;
pub const P_FONT_WEIGHT: u32 = 13;
/// 推测：font-stretch（只在字体片段里见过，取 `$350`）。
pub const P_FONT_STRETCH: u32 = 15;
pub const P_FONT_SIZE: u32 = 16;
pub const P_COLOR: u32 = 19;
pub const P_INLINE_BACKGROUND: u32 = 21;
pub const P_TEXT_ALIGN: u32 = 34;
pub const P_TEXT_INDENT: u32 = 36;
pub const P_LINE_HEIGHT: u32 = 42;
pub const P_VERTICAL_ALIGN: u32 = 44;
pub const P_MARGIN_TOP: u32 = 47;
pub const P_MARGIN_LEFT: u32 = 48;
pub const P_MARGIN_BOTTOM: u32 = 49;
pub const P_MARGIN_RIGHT: u32 = 50;
pub const P_PADDING_TOP: u32 = 52;
/// 左右和外边距一样是「上、左、下、右」的顺序（2026-10-05 测试书 B13 `padding-left` → `$53`；以前左右写反了）。
pub const P_PADDING_LEFT: u32 = 53;
pub const P_PADDING_BOTTOM: u32 = 54;
pub const P_PADDING_RIGHT: u32 = 55;
pub const P_BACKGROUND: u32 = 70;
/// 背景图（指向 `$164` 图片资源）、不重复（`$484: $487`）、固定（`$547: $378`）、位置 x/y、尺寸宽/高（《绍宋》样本 `body.bq/juan/jsy`）。
pub const P_BG_IMAGE: u32 = 479;
pub const P_BG_REPEAT: u32 = 484;
pub const BG_NO_REPEAT: u32 = 487;
pub const P_BG_ATTACHMENT: u32 = 547;
pub const BG_FIXED: u32 = 378;
pub const P_BG_POS_X: u32 = 480;
pub const P_BG_POS_Y: u32 = 481;
pub const P_BG_SIZE_W: u32 = 482;
pub const P_BG_SIZE_H: u32 = 483;
/// 下划线、删除线、上划线（取值 `$328`）、small-caps（`$583: $369`）、字间距（em）。
pub const P_UNDERLINE: u32 = 23;
pub const P_LINE_THROUGH: u32 = 27;
pub const P_OVERLINE: u32 = 554;
pub const P_FONT_VARIANT: u32 = 583;
pub const SMALL_CAPS: u32 = 369;
pub const P_LETTER_SPACING: u32 = 32;
pub const P_WIDTH: u32 = 56;
pub const P_HEIGHT: u32 = 57;
/// 推测：尺寸按内容盒算（有宽度的样式上常见 `$546: $377`）。
pub const P_SIZING: u32 = 546;
pub const SIZING_VALUE: u32 = 377;
/// 固定版式一页的定位（2026-10-06 Amazon 转的异形页样本）：整页容器 `{$476: true, $183: $488}`，里面的图片节点
/// `{$58: 上, $59: 左, $183: $324}`（裸浮点数）。按 CSS 推测 `$183` 是 position（`$488` relative、`$324` absolute），
/// `$476` 像 overflow:hidden。
pub const P_TOP: u32 = 58;
pub const P_LEFT: u32 = 59;
pub const P_POSITION: u32 = 183;
pub const POSITION_ABSOLUTE: u32 = 324;
pub const POSITION_RELATIVE: u32 = 488;
pub const P_CLIP: u32 = 476;
/// 推测：排版提示（标题样式上是 `[$760]`，表格标题上是 `[$453]`）。
pub const P_LAYOUT_HINTS: u32 = 761;
/// 排版提示「标题」：Send to Kindle 写在每个 `<h1>`–`<h6>` 自己的样式上（《绝叫》45 个 h2/h4 全有，2026-10-08）。
pub const HINT_HEADING: u32 = 760;
/// `word-break: break-all` → `$569: $570`（2026-10-08《绍宋》：全书 `p{word-break:break-all}`，没继承它的 `h1.juan` 就没有）。
pub const P_WORD_BREAK: u32 = 569;
pub const WORD_BREAK_ALL: u32 = 570;
/// 单元格：跨列、跨行、竖直对齐（`$58` top、`$320` middle、`$60` bottom）。
pub const P_COLSPAN: u32 = 148;
pub const P_ROWSPAN: u32 = 149;
pub const P_CELL_VALIGN: u32 = 633;
pub const VALIGN_TOP: u32 = 58;
pub const VALIGN_BOTTOM: u32 = 60;
/// 边框：四边一样时写「全部」，否则按「上、左、下、右」各写（同外边距的顺序）。
pub const P_BORDER_COLOR: [u32; 5] = [83, 84, 85, 86, 87];
pub const P_BORDER_STYLE: [u32; 5] = [88, 89, 90, 91, 92];
pub const P_BORDER_WIDTH: [u32; 5] = [93, 94, 95, 96, 97];
/// 圆角：左上、右上、左下、右下（右上和左下的先后是推测：测试书只有「1em 0」）。
pub const P_BORDER_RADIUS: [u32; 4] = [459, 460, 461, 462];
pub const BORDER_SOLID: u32 = 328;
pub const BORDER_DOUBLE: u32 = 329;
pub const BORDER_DASHED: u32 = 330;
pub const BORDER_DOTTED: u32 = 331;
pub const BORDER_GROOVE: u32 = 334;
pub const BORDER_RIDGE: u32 = 335;
pub const BORDER_INSET: u32 = 336;
pub const BORDER_OUTSET: u32 = 337;

// ---- 取值
pub const UNIT: u32 = 306;
pub const VALUE: u32 = 307;
pub const U_EM: u32 = 308;
pub const U_LH: u32 = 310;
pub const U_PERCENT: u32 = 314;
pub const U_PT: u32 = 318;
/// 字号用的单位（样本里字号一律用它，含义像 em）。
pub const U_FONT_EM: u32 = 505;
pub const ALIGN_LEFT: u32 = 59;
pub const ALIGN_RIGHT: u32 = 61;
pub const ALIGN_CENTER: u32 = 320;
pub const ALIGN_JUSTIFY: u32 = 321;
/// 2026-10-05 对照改正：以前写反了（ABC 的 `.contents-chapter{font-weight:bold}` 是 `$361`，`.bodycontent-title{font-weight:normal}` 是 `$350`；
/// 《绍宋》字体片段 `$262` 的字形、字重、宽度都写 `$350`＝normal）。
pub const WEIGHT_BOLD: u32 = 361;
/// 半粗：Send to Kindle 把 `font-weight:600` 写成它（《绍宋》`p.ganyan1`）。
pub const WEIGHT_SEMIBOLD: u32 = 360;
/// `bolder`：Send to Kindle 写在 `<b>`/`<strong>` 上（HTML 缺省样式就是 `font-weight: bolder`；《金庸》《克莱因壶》《福尔摩斯》）。
pub const WEIGHT_BOLDER: u32 = 362;
/// `min-height`，`height` 也写它（《恶女的告白》`min-height:2em`、《消失的爱人》`height:6em` → `$62`）。
pub const P_MIN_HEIGHT: u32 = 62;
/// 链接的颜色：`{$19: 颜色}`，两个一起写（推测是未访问、已访问；《人生海海》目录 `<a style="color:#00C">`）。
/// 有宽度的块：最大宽度（em 宽度时写 100%）、块的左右对齐（左右外边距 auto：`$320` 居中、`$59` 靠左、`$61` 靠右）。
pub const P_MAX_WIDTH: u32 = 65;
pub const P_BOX_ALIGN: u32 = 580;
/// `box-shadow`、`text-shadow`：`{$498 颜色, $499 x, $500 y, $501 模糊}`（《雪国》注释框、《阿加莎》卷号）。
pub const P_BOX_SHADOW: u32 = 496;
pub const P_TEXT_SHADOW: u32 = 497;
pub const SHADOW_COLOR: u32 = 498;
pub const SHADOW_X: u32 = 499;
pub const SHADOW_Y: u32 = 500;
pub const SHADOW_BLUR: u32 = 501;
pub const P_LINK_UNVISITED: u32 = 576;
pub const P_LINK_VISITED: u32 = 577;
/// 字体名 `default`：阅读器自己的字体（Send to Kindle 写在 `@font-face` 声明了却没有字体文件的字体上，《绍宋》的「宋体」）。
pub const FONT_DEFAULT: &str = "default";
/// 颜色「透明」（Send to Kindle 写在全透明的边框颜色上）。
pub const COLOR_TRANSPARENT: u32 = 349;
/// 整页背景的范围（节点上的字段，`{$58: 0%, $59: 0%, $60: 100%, $61: 100%}`）：Send to Kindle 写在 `background-size: cover` 的
/// 页面背景容器上，背景铺满一页（2026-10-08《绍宋》卷首语，6 处）。四个分量照样本写。
pub const BG_PAGE_BOUNDS: u32 = 645;
pub const BG_PAGE_BOUNDS_KEYS: [u32; 4] = [58, 59, 60, 61];
/// `white-space: nowrap`（值是 true）。
pub const P_NOWRAP: u32 = 45;
pub const WEIGHT_NORMAL: u32 = 350;
/// 字形、宽度的 normal（和字重的 normal 同一个符号）。
pub const FONT_NORMAL: u32 = 350;
/// 推测：italic。
pub const STYLE_ITALIC: u32 = 382;
pub const VALIGN_SUPER: u32 = 370;
pub const VALIGN_SUB: u32 = 371;

// ---- 阅读顺序、导航
pub const READING_ORDERS: u32 = 169;
pub const READING_ORDER_NAME: u32 = 178;
pub const DEFAULT_READING_ORDER: u32 = 351;
pub const SECTIONS: u32 = 170;
pub const NAV_CONTAINERS: u32 = 392;
pub const NAV_TYPE: u32 = 235;
pub const NAV_TYPE_TOC: u32 = 212;
pub const NAV_TYPE_LANDMARKS: u32 = 236;
/// 标题导航（每个标题级别一组）。
pub const NAV_TYPE_HEADINGS: u32 = 798;
/// 标题导航里一级标题组的类型；二级 `$800`、三级 `$801`，往下推测依次加一。
pub const HEADING_LEVEL_1: u32 = 799;
pub const NAV_NAME: u32 = 239;
pub const NAV_ENTRIES: u32 = 247;
pub const NAV_LANDMARK_TYPE: u32 = 238;
pub const LANDMARK_COVER: u32 = 233;
pub const NAV_LABEL: u32 = 241;
pub const NAV_LABEL_TEXT: u32 = 244;
pub const NAV_TARGET: u32 = 246;
pub const ANCHOR_NAME: u32 = 180;
pub const ANCHOR_POSITION: u32 = 183;

// ---- 资源
pub const RES_FORMAT: u32 = 161;
pub const RES_MIME: u32 = 162;
pub const RES_LOCATION: u32 = 165;
pub const RES_WIDTH: u32 = 422;
pub const RES_HEIGHT: u32 = 423;
pub const FORMAT_JPG: u32 = 285;
/// PNG（样本里没有，Amazon 都转成 JPEG XR；2026-10-05 真机《绍宋》PNG 封面、插图、背景图都显示）。
pub const FORMAT_PNG: u32 = 284;
pub const FORMAT_GIF: u32 = 286;

// ---- 元数据、清单
pub const META_GROUPS: u32 = 491;
pub const META_GROUP_NAME: u32 = 495;
pub const META_ENTRIES: u32 = 258;
pub const META_KEY: u32 = 492;
pub const META_VALUE: u32 = 307;
pub const MANIFEST_CONTAINERS: u32 = 252;
pub const MANIFEST_DEPS: u32 = 253;
pub const MANIFEST_DEP_LIST: u32 = 254;
pub const CAPABILITIES: u32 = 593;
/// 文档数据里指向文档级附加数据的字段：`$597: {$614: 名字}`；封面图实体名＝这个名字 + `-ad`。
pub const DOC_AUX: u32 = 597;
pub const DOC_AUX_NAME: u32 = 614;
// ---- 固定版式（2026-10-05 Send to Kindle 测试漫画对照，见 docs/kfx.md#固定版式）
/// 文档数据、固定版式版面上的翻页方向：`$557` 从左往右、`$559` 从右往左。
pub const DOC_DIRECTION: u32 = 560;
pub const DIR_LTR: u32 = 557;
pub const DIR_RTL: u32 = 559;
/// 推测：书写方向（横排 `$376`）。
pub const DOC_WRITING_MODE: u32 = 192;
pub const WRITING_HORIZONTAL: u32 = 376;
/// 文档数据上的固定版式标记 `$433: $385`（流式的书没有）。
pub const DOC_FIXED: u32 = 433;
pub const DOC_FIXED_VALUE: u32 = 385;
/// 固定版式版面上的 `$434: $441`（含义不明，照样本）。
pub const FIXED_PAGE_FIT: u32 = 434;
pub const FIXED_PAGE_FIT_VALUE: u32 = 441;
