-- 个人手势（settings/gestures.lua 补丁）。gestures.lua 是 KOReader 第一次运行时从 plugins/gestures.koplugin/defaults.lua 整份复制的，
-- 每个手势是一张"动作 → 参数"表，合并是递归的：改动作时要把原来的动作写成 "__DELETE__"，不然两个动作都在。
-- 阅读界面（用户 2026-09-29 定）：
--   长按四角：左上 退出 KOReader、右上 休眠、左下 截屏、右下 全刷
--   点四角：  左上 回文件管理器、右上 书籍目录、左下 开关触屏（锁屏防误触）、右下 交换翻页键
--   边缘上下滑：左边缘 前光亮度、右边缘 暖光（参数 0 = 按滑动距离连续调，同 KOReader 缺省）。暖光要硬件支持（KOReader 判断
--   hasNaturalLight），不支持的设备上这两个手势什么也不做。
--   去掉的：短斜滑全刷（缺省）。其余（点左右翻页、中间菜单、双击左右跳 10 页……）是 KOReader 缺省。
-- 文件管理器（用户 2026-09-28 在掌阅上改的）：长按左下截屏、右下全刷、右上交换翻页键（原来的"目录"去掉）；去掉短斜滑全刷、点左下角开关前光。
-- 没写到的位置保留设备自己的缺省。
return {
    ["gesture_fm"] = {
        ["hold_bottom_left_corner"] = {
            ["screenshot"] = true,
        },
        ["hold_bottom_right_corner"] = {
            ["full_refresh"] = true,
        },
        ["hold_top_right_corner"] = {
            ["swap_page_turn_buttons"] = true,
            ["toc"] = "__DELETE__", -- 原来的"目录"去掉（合并是递归的，不删会两个动作都在）
        },
        ["short_diagonal_swipe"] = "__DELETE__",
        ["tap_left_bottom_corner"] = "__DELETE__",
    },
    ["gesture_reader"] = {
        -- 长按四角
        ["hold_top_left_corner"] = { ["exit"] = true },
        ["hold_top_right_corner"] = { ["suspend"] = true, ["toggle_frontlight"] = "__DELETE__" },
        ["hold_bottom_left_corner"] = { ["screenshot"] = true },
        ["hold_bottom_right_corner"] = { ["full_refresh"] = true, ["toggle_touch_input"] = "__DELETE__" },
        -- 点四角（缺省：左上切翻页模式、右上书签、左下开关前光）
        ["tap_top_left_corner"] = { ["filemanager"] = true, ["toggle_page_flipping"] = "__DELETE__" },
        ["tap_top_right_corner"] = { ["toc"] = true, ["toggle_bookmark"] = "__DELETE__" },
        ["tap_left_bottom_corner"] = { ["toggle_touch_input"] = true, ["toggle_frontlight"] = "__DELETE__" },
        ["tap_right_bottom_corner"] = { ["swap_page_turn_buttons"] = true },
        -- 边缘上下滑：左 前光、右 暖光（之前右边缘是全刷）
        ["one_finger_swipe_left_edge_up"] = { ["increase_frontlight"] = 0 },
        ["one_finger_swipe_left_edge_down"] = { ["decrease_frontlight"] = 0 },
        ["one_finger_swipe_right_edge_up"] = { ["increase_frontlight_warmth"] = 0, ["full_refresh"] = "__DELETE__" },
        ["one_finger_swipe_right_edge_down"] = { ["decrease_frontlight_warmth"] = 0, ["full_refresh"] = "__DELETE__" },
        ["short_diagonal_swipe"] = "__DELETE__",
    },
}
