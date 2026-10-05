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
pub const HEADING_LEVEL: u32 = 790;
pub const STYLE_NAME: u32 = 173;
pub const LIST: u32 = 181;
pub const START: u32 = 184;

// ---- 节点类型
pub const NODE_TEXT: u32 = 269;
pub const NODE_CONTAINER: u32 = 270;
pub const NODE_IMAGE: u32 = 271;

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
pub const P_PADDING_RIGHT: u32 = 53;
pub const P_PADDING_BOTTOM: u32 = 54;
pub const P_PADDING_LEFT: u32 = 55;
pub const P_BACKGROUND: u32 = 70;

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
pub const WEIGHT_BOLD: u32 = 350;
pub const WEIGHT_NORMAL: u32 = 361;
/// 推测：italic。
pub const STYLE_ITALIC: u32 = 382;
pub const VALIGN_SUPER: u32 = 370;

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
/// 推测：PNG（样本里没有 PNG 图）。
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
