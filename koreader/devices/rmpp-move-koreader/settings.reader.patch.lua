-- Move 专属（settings.reader.lua 补丁）。个人设置以掌阅为准（../../personal/），这里只放 Move 屏幕、机身带来的不同（用户 2026-09-28）。
-- 没写到的 Move 自己的调优原样保留：刷新波形（wf_level）、菜单与键盘不闪（flash_ui、flash_keyboard）、翻页点击区四周的
-- 握持死区（defaults.custom.lua，这里不碰）等。
return {
    -- 不分栏：Move 屏幕窄长（954×1696，宽高比 0.56），两栏每栏太窄。掌阅的个人设置是两栏，这里改一栏（用户定）。
    ["copt_visible_pages"] = 1,
    -- 彩屏（Gallery 3）：彩色封面、彩页要它。掌阅是黑白屏，个人设置里是 false。
    ["color_rendering"] = true,
    -- 全刷只在章节交界做（-1）：Move 彩屏全刷又慢又闪，这是 Move 上原有的设置，保留；文字书方案的"每 16 页全刷"不用在 Move 上。
    ["full_refresh_count"] = -1,
    -- 状态栏字体：京華老宋体 v3.0
    ["footer"] = { ["text_font_face"] = "./fonts/京華老宋体v3.0.ttf" },
    -- 漫画方案统一按"漫画"标签切换（schemes/comic），去掉 Move 上原来按文件夹（books/漫画/、books/小说/）切换的条件
    ["profiles_autoexec"] = {
        ["ReaderReadyAll"] = {
            ["漫画"] = { ["filepath"] = "__DELETE__" },
            ["文字"] = "__DELETE__",
        },
        ["CloseDocumentAll"] = {
            ["文字"] = { ["filepath"] = "__DELETE__" },
        },
    },
}
