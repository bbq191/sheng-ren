-- 文字书方案（settings.reader.lua 补丁）。全局设置本身就是文字书方案：任何没有单书设置的书都用它。
-- 排版（行距、字重、字体……）照个人设置（personal/）不动，这里只加功能项（用户 2026-09-28 选定）。
-- 键名全部按设备上 KOReader v2026.07.2 的源码核过（docs/koreader.md「键名怎么核」）。
return {
    -- ── 脚注回得去 ──
    ["footnote_link_in_popup"] = true,          -- 点脚注在底部弹窗显示，不跳页
    ["link_prefer_footnote"] = true,            -- 中文书的脚注常没有 epub:type，放宽"是不是脚注"的判定
    ["larger_tap_area_to_follow_links"] = true, -- 上标注释号太小，放大点击命中区
    ["swipe_to_go_back"] = true,                -- 跟着链接跳过去之后，左→右滑回原处（没有跳转记录时就是上一页，翻页不受影响）

    -- ── 断行 ──
    ["text_lang_fallback"] = "zh-CN",           -- 书没标语言时按中文断行（缺省 en-US）

    -- ── 刷新 ──
    ["full_refresh_count"] = 16,                -- 文字页每 16 页全刷一次清残影（缺省 6）。带图片的页 KOReader 缺省就每页全刷
                                                -- （refresh_on_pages_with_images 缺省开），漫画不用另设
}
