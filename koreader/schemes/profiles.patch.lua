-- 配置档（settings/profiles.lua 补丁）：由 comic.settings.patch.lua 的 profiles_autoexec 按书的元数据自动执行。
-- 配置档 = 一组 Dispatcher 动作（源码 frontend/dispatcher.lua）。单书排版类动作（页边距、状态行、缩放、翻页方向）执行时
-- 写进这本书自己的设置，关书时存下，所以「漫画·首次」只需在第一次打开时跑一次。
-- 状态栏预设「文字」「漫画」不在这里：apply.sh 按设备当前的状态栏生成（字体路径随设备），见 presets.lua。
return {
    ["漫画·首次"] = {
        ["set_inverse_reading_order"] = true, -- 从右往左翻：点左侧下一页、点右侧上一页，滑动方向一起反转（用户 2026-09-28 定：
                                              -- 漫画默认按日漫；国漫/美漫在书里菜单关掉「反转翻页方向」，这本书会记住）。
                                              -- KOReader 不读 OPF 的 page-progression-direction，只能这样设
        ["h_page_margins"] = { 0, 0 },        -- 左右页边距 0：画面铺满整屏宽
        ["t_page_margin"] = 0,
        ["b_page_margin"] = 0,
        ["status_line"] = 1,                  -- 关 crengine 顶部标题栏（1 = 关）
        ["smooth_scaling"] = true,            -- 图片缩放用「最佳」算法，网点、线稿不糊不锯齿
        ["view_mode"] = "page",               -- 翻页模式（不是滚动）
        ["visible_pages"] = 1,                -- 不分栏：一屏一页（个人设置里全局是两栏 copt_visible_pages = 2，
                                              -- 源码里两栏竖屏也生效；漫画一页就是一整幅画，用户 2026-09-28 定）
        ["settings"] = { ["name"] = "漫画·首次" },
    },
    ["漫画"] = {
        ["load_footer_preset"] = "漫画",      -- 隐藏状态栏和进度条，画面用满整屏高度
        ["settings"] = { ["name"] = "漫画" },
    },
    ["文字"] = {
        ["load_footer_preset"] = "文字",      -- 恢复文字书的状态栏
        ["settings"] = { ["name"] = "文字" },
    },
}
