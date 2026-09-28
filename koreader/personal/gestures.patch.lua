-- 个人手势（settings/gestures.lua 补丁）：掌阅上相对 KOReader 缺省改过的 16 处（用户 2026-09-28 定：以掌阅为准）。
-- 阅读界面：长按四角 = 截屏 / 切换触屏 / 退出 / 开关前光；右边缘上下滑 = 全刷；点右下角 = 交换翻页键；
-- 去掉了缺省的点左上角翻页模式、点右上角书签、点左下角开关前光、短斜滑全刷。
-- 文件管理器：长按左下截屏、右下全刷、右上目录；去掉短斜滑全刷、点左下角开关前光。
-- 没写到的位置保留设备自己的缺省（比如 Kindle 文件管理器里右边缘滑动调色温——色温是 Kindle 有、掌阅没有的硬件）。
return {
    ["gesture_fm"] = {
        ["hold_bottom_left_corner"] = {
            ["screenshot"] = true,
        },
        ["hold_bottom_right_corner"] = {
            ["full_refresh"] = true,
        },
        ["hold_top_right_corner"] = {
            ["toc"] = true,
        },
        ["short_diagonal_swipe"] = "__DELETE__",
        ["tap_left_bottom_corner"] = "__DELETE__",
    },
    ["gesture_reader"] = {
        ["hold_bottom_left_corner"] = {
            ["screenshot"] = true,
        },
        ["hold_bottom_right_corner"] = {
            ["toggle_touch_input"] = true,
        },
        ["hold_top_left_corner"] = {
            ["exit"] = true,
        },
        ["hold_top_right_corner"] = {
            ["toggle_frontlight"] = true,
        },
        ["one_finger_swipe_right_edge_down"] = {
            ["full_refresh"] = true,
        },
        ["one_finger_swipe_right_edge_up"] = {
            ["full_refresh"] = true,
        },
        ["short_diagonal_swipe"] = "__DELETE__",
        ["tap_left_bottom_corner"] = "__DELETE__",
        ["tap_right_bottom_corner"] = {
            ["swap_page_turn_buttons"] = true,
        },
        ["tap_top_left_corner"] = "__DELETE__",
        ["tap_top_right_corner"] = "__DELETE__",
    },
}
